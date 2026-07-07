package app.tauri.serialplugin.manager

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log
import androidx.core.content.ContextCompat
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.SerialPortConfig
import com.hoho.android.usbserial.driver.UsbSerialDriver
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.driver.UsbSerialProber
import java.io.IOException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit

/** path → open session; USB permission + attach/detach only. */
class UsbBridge private constructor(
    private val context: Context?,
    private val rxSink: SerialRxSink,
    private val registerReceiver: Boolean,
    private val testMode: Boolean,
) {
    constructor(context: Context) : this(
        context,
        CoalescingRxSink(JniSerialRxSink, immediate = false),
        true,
        false,
    )

    companion object {
        private const val TAG = "UsbBridge"
        private const val ACTION_USB_PERMISSION = "app.tauri.serialplugin.USB_PERMISSION"

        internal fun forTesting(rxSink: SerialRxSink): UsbBridge =
            UsbBridge(null, rxSink, false, true)
    }

    private val ioExecutor: Executor = if (testMode) {
        Executor { it.run() }
    } else {
        Executors.newSingleThreadExecutor { r -> Thread(r, "usb-io").apply { isDaemon = true } }
    }
    private val usbManager = context?.getSystemService(Context.USB_SERVICE) as? UsbManager
    private val sessions = ConcurrentHashMap<String, UsbPortSession>()
    private val permissionFutures = ConcurrentHashMap<String, CompletableFuture<Boolean>>()
    private val closing = ConcurrentHashMap.newKeySet<String>()
    private val defaultProber by lazy { UsbSerialProber.getDefaultProber() }
    private val customProber by lazy { CustomProber.getCustomProber() }

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
            when (intent.action) {
                ACTION_USB_PERMISSION -> {
                    val device = deviceFromIntent(intent)
                    val granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
                    device?.deviceName?.let { permissionFutures[it]?.complete(granted) }
                }
                UsbManager.ACTION_USB_DEVICE_DETACHED -> {
                    val device = deviceFromIntent(intent)
                    val deviceName = device?.deviceName
                    Log.w(TAG, "DETACH device=$deviceName vid=${device?.vendorId} pid=${device?.productId}")
                    if (deviceName != null) {
                        sessions.keys
                            .filter { UsbPath.deviceName(it) == deviceName }
                            .forEach { path -> runOnIo { fail(path, "USB device detached") } }
                    }
                    rxSink.onPortListChange()
                }
                UsbManager.ACTION_USB_DEVICE_ATTACHED -> {
                    val d = deviceFromIntent(intent)
                    Log.d(TAG, "ATTACH path=${d?.deviceName} vid=${d?.vendorId} pid=${d?.productId}")
                    rxSink.onPortListChange()
                }
            }
        }
    }

    init {
        if (registerReceiver && context != null) {
            val filter = IntentFilter(ACTION_USB_PERMISSION).apply {
                addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
                addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            }
            if (Build.VERSION.SDK_INT >= 33) {
                context.registerReceiver(usbReceiver, filter, Context.RECEIVER_EXPORTED)
            } else {
                ContextCompat.registerReceiver(
                    context, usbReceiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED,
                )
            }
        }
    }

    /** Enqueue port teardown on [usb-io], then stop the executor (PR #34 lifecycle pattern). */
    fun shutdown() {
        if (testMode) {
            sessions.keys.toList().forEach { close(it) }
            return
        }
        val pool = ioExecutor as ExecutorService
        val ctx = context
        val shouldUnregister = registerReceiver
        try {
            pool.execute {
                try {
                    sessions.keys.toList().forEach { close(it) }
                } finally {
                    if (shouldUnregister && ctx != null) {
                        try {
                            ctx.unregisterReceiver(usbReceiver)
                        } catch (_: IllegalArgumentException) {
                        }
                    }
                }
            }
        } catch (_: RejectedExecutionException) {
            sessions.keys.toList().forEach { close(it) }
            if (shouldUnregister && ctx != null) {
                try {
                    ctx.unregisterReceiver(usbReceiver)
                } catch (_: IllegalArgumentException) {
                }
            }
        }
        pool.shutdown()
    }

    /** All blocking USB work runs here — never on the UI thread. */
    fun runOnIo(block: () -> Unit) {
        ioExecutor.execute(block)
    }

    /** Blocking USB work on [usb-io]; safe from any JNI/Rust thread. */
    fun <T> runOnIoSync(block: () -> T): T =
        if (testMode) {
            block()
        } else {
            CompletableFuture.supplyAsync(block, ioExecutor).get()
        }

    internal fun adoptFakePort(port: UsbSerialPort, config: SerialPortConfig) {
        check(testMode)
        if (config.assertDtrRts) setModemLines(port, dtr = true, rts = true)
        putSession(config.path, newSession(config.path, port, config))
        sessions[config.path]!!.startReadLoop()
    }

    fun enumerate(): Map<String, Map<String, String>> {
        val mgr = usbManager ?: return emptyMap()
        return buildMap {
            allDrivers(mgr).forEach { driver ->
                val d = driver.device
                val ports = driver.ports
                ports.indices.forEach { idx ->
                    val key = UsbPath.sessionKey(d.deviceName, idx, ports.size)
                    put(key, deviceInfo(d, idx, ports.size))
                }
            }
        }
    }

    fun open(config: SerialPortConfig) {
        val mgr = usbManager ?: throw IOException("no UsbManager")
        val (deviceName, pathPort) = UsbPath.parse(config.path)
        val portIndex = if (pathPort != 0) pathPort else config.portIndex
        close(config.path)
        val device = mgr.deviceList.values.find { it.deviceName == deviceName }
            ?: throw IOException("device not found: $deviceName")
        val driver = probeDevice(device) ?: throw IOException("no driver: $deviceName")
        if (portIndex < 0 || portIndex >= driver.ports.size) {
            throw IOException("invalid port index $portIndex for $deviceName")
        }
        val sessionPath = UsbPath.sessionKey(deviceName, portIndex, driver.ports.size)
        if (!mgr.hasPermission(device)) requestPermission(device)
        val conn = mgr.openDevice(device) ?: throw IOException("open failed: $deviceName")
        val port = driver.ports[portIndex]
        port.open(conn)
        try {
            port.setParameters(
                config.baudRate, config.dataBits.value,
                config.stopBits.value, config.parity.value,
            )
        } catch (_: Exception) {
        }
        applyFlowControl(port, config.flowControl)
        if (config.assertDtrRts) setModemLines(port, dtr = true, rts = true)
        val sessionConfig = config.copy(path = sessionPath, portIndex = portIndex)
        putSession(sessionPath, newSession(sessionPath, port, sessionConfig))
        sessions[sessionPath]!!.startReadLoop()
        Log.i(TAG, "open $sessionPath port=$portIndex")
    }

    fun close(path: String?) {
        if (path == null) {
            sessions.keys.toList().forEach { close(it) }
            return
        }
        if (!closing.add(path)) return
        try {
            sessions.remove(path)?.let { session ->
                session.shutdown()
                session.close()
            }
        } finally {
            closing.remove(path)
        }
    }

    fun write(path: String, data: ByteArray): Int =
        session(path).write(data)

    fun read(path: String, timeout: Int, size: Int?): ByteArray =
        session(path).pollRead(timeout, size ?: 1024)

    fun ctl(path: String, op: String, params: Map<String, Any?>): Any? =
        session(path).ctl(op, params)

    fun signal(path: String, op: String, level: Boolean?): Boolean =
        session(path).signal(op, level)

    internal fun isSiomRunning(path: String) = sessions[path]?.isReading == true

    internal fun stopSiom(path: String) {
        sessions[path]?.stopReadLoop()
    }

    private fun putSession(path: String, s: UsbPortSession) {
        sessions[path]?.close()
        sessions[path] = s
    }

    private fun session(path: String): UsbPortSession =
        sessions[path] ?: throw IOException("port not open: $path")

    private fun newSession(
        path: String,
        port: UsbSerialPort,
        config: SerialPortConfig,
    ) = UsbPortSession(
        path,
        port,
        config,
        onRx = { rxSink.feedRx(path, it) },
        onError = { msg -> fail(path, msg) },
        onRecover = { runOnIo { sessions[path]?.restartReadLoop() } },
    )

    private fun fail(path: String, reason: String) {
        Log.e(TAG, "fail $path: $reason")
        val session = sessions.remove(path) ?: return
        session.shutdown()
        rxSink.onUsbError(path, reason)
        try {
            session.close()
        } catch (_: Exception) {
        }
    }

    private fun probeDevice(device: UsbDevice): UsbSerialDriver? =
        defaultProber.probeDevice(device) ?: customProber.probeDevice(device)

    private fun allDrivers(mgr: UsbManager): List<UsbSerialDriver> {
        val seen = LinkedHashSet<UsbDevice>()
        val out = ArrayList<UsbSerialDriver>()
        defaultProber.findAllDrivers(mgr).forEach { d ->
            if (seen.add(d.device)) out.add(d)
        }
        mgr.deviceList.values.forEach { device ->
            if (seen.add(device)) {
                customProber.probeDevice(device)?.let { out.add(it) }
            }
        }
        return out
    }

    private fun requestPermission(device: UsbDevice) {
        val ctx = context ?: throw IOException("no context")
        val future = CompletableFuture<Boolean>()
        permissionFutures[device.deviceName] = future
        try {
            val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                PendingIntent.FLAG_MUTABLE
            } else {
                PendingIntent.FLAG_IMMUTABLE
            }
            usbManager!!.requestPermission(
                device,
                PendingIntent.getBroadcast(ctx, 0, Intent(ACTION_USB_PERMISSION), flags),
            )
            if (!future.get(10, TimeUnit.SECONDS)) {
                throw IOException("permission denied")
            }
        } finally {
            permissionFutures.remove(device.deviceName)
        }
    }

    private fun applyFlowControl(port: UsbSerialPort, flow: FlowControl) {
        try {
            when (flow) {
                FlowControl.NONE -> port.setFlowControl(UsbSerialPort.FlowControl.NONE)
                FlowControl.HARDWARE -> port.setFlowControl(UsbSerialPort.FlowControl.RTS_CTS)
                FlowControl.SOFTWARE -> port.setFlowControl(UsbSerialPort.FlowControl.XON_XOFF)
            }
        } catch (_: Exception) {
        }
    }

    private fun deviceInfo(device: UsbDevice, portIndex: Int, portCount: Int): Map<String, String> {
        val ok = usbManager!!.hasPermission(device)
        val base = mapOf(
            "type" to "USB",
            "vid" to device.vendorId.toString(),
            "pid" to device.productId.toString(),
            "manufacturer" to if (ok) device.manufacturerName ?: "?" else "?",
            "product" to if (ok) device.productName ?: "?" else "?",
            "serial_number" to if (ok) try {
                device.serialNumber ?: "?"
            } catch (_: SecurityException) {
                "permission_required"
            } else "permission_required",
        )
        return if (portCount > 1) {
            base + mapOf("port_index" to portIndex.toString(), "port_count" to portCount.toString())
        } else {
            base
        }
    }

    private fun deviceFromIntent(intent: Intent): UsbDevice? =
        if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(UsbManager.EXTRA_DEVICE) as UsbDevice?
        }
}
