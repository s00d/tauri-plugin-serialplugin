package app.tauri.serialplugin.manager

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log
import androidx.core.content.ContextCompat
import org.json.JSONArray
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/**
 * USB fd provider for Rust [android-usb-serial] (no vendored usbserial).
 */
class UsbFdBridge private constructor(
    private val context: Context?,
    private val nativeSink: NativeSink,
    private val registerReceiver: Boolean,
    private val testMode: Boolean,
) {
    constructor(context: Context) : this(
        context,
        JniNativeSink,
        true,
        false,
    )

    companion object {
        private const val TAG = "UsbFdBridge"
        private const val ACTION_USB_PERMISSION = "app.tauri.serialplugin.USB_PERMISSION"

        fun forTesting(sink: NativeSink = object : NativeSink {
            override fun onPortListChange() {}
            override fun onDeviceDetached(deviceName: String) {}
        }): UsbFdBridge = UsbFdBridge(null, sink, false, true)

        /** Robolectric: real [Context] + receiver registration, synchronous IO. */
        fun forTesting(
            context: Context,
            sink: NativeSink = object : NativeSink {
                override fun onPortListChange() {}
                override fun onDeviceDetached(deviceName: String) {}
            },
        ): UsbFdBridge = UsbFdBridge(context, sink, registerReceiver = true, testMode = true)

        @JvmStatic
        fun forIntegrationTest(): UsbFdBridge = forTesting(JniNativeSink)
    }

    interface NativeSink {
        fun onPortListChange()
        fun onDeviceDetached(deviceName: String)
    }

    @Volatile private var shutDown = false

    private val ioExecutor: Executor = if (testMode) {
        Executor { it.run() }
    } else {
        Executors.newSingleThreadExecutor { r -> Thread(r, "usb-fd-io").apply { isDaemon = true } }
    }
    private val usbManager = context?.getSystemService(Context.USB_SERVICE) as? UsbManager
    private val connections = ConcurrentHashMap<String, UsbDeviceConnection>()
    private val permissionFutures = ConcurrentHashMap<String, CompletableFuture<Boolean>>()

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
                    val name = device?.deviceName
                    if (name != null) {
                        closeDeviceFd(name)
                        nativeSink.onDeviceDetached(name)
                    }
                    nativeSink.onPortListChange()
                }
                UsbManager.ACTION_USB_DEVICE_ATTACHED -> nativeSink.onPortListChange()
            }
        }
    }

    init {
        if (registerReceiver && context != null) {
            val filter = IntentFilter(ACTION_USB_PERMISSION).apply {
                addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
                addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            }
            if (Build.VERSION.SDK_INT >= 33 || testMode) {
                context.registerReceiver(usbReceiver, filter, Context.RECEIVER_EXPORTED)
            } else {
                ContextCompat.registerReceiver(
                    context, usbReceiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED,
                )
            }
        }
    }

    /** Test-only: invoke the USB broadcast receiver directly (Robolectric). */
    fun deliverBroadcastForTest(intent: Intent) {
        check(testMode)
        val ctx = context ?: throw IllegalStateException("no context")
        usbReceiver.onReceive(ctx, intent)
    }

    /** Test-only: complete a pending USB permission request without a broadcast. */
    fun completePermissionForTest(deviceName: String, granted: Boolean) {
        check(testMode)
        permissionFutures[deviceName]?.complete(granted)
    }

    fun shutdown() {
        if (testMode) {
            connections.keys.toList().forEach { closeDeviceFd(it) }
            if (registerReceiver && context != null) {
                try {
                    context.unregisterReceiver(usbReceiver)
                } catch (_: IllegalArgumentException) {
                }
            }
            return
        }
        shutDown = true
        val pool = ioExecutor as ExecutorService
        val ctx = context
        val shouldUnregister = registerReceiver
        try {
            pool.execute {
                try {
                    connections.keys.toList().forEach { closeDeviceFd(it) }
                } finally {
                    if (shouldUnregister && ctx != null) {
                        try {
                            ctx.unregisterReceiver(usbReceiver)
                        } catch (_: IllegalArgumentException) {
                        }
                    }
                    pool.shutdown()
                }
            }
        } catch (_: RejectedExecutionException) {
            connections.keys.toList().forEach { closeDeviceFd(it) }
            if (shouldUnregister && ctx != null) {
                try {
                    ctx.unregisterReceiver(usbReceiver)
                } catch (_: IllegalArgumentException) {
                }
            }
            pool.shutdown()
        }
    }

    fun runOnIoSync(block: () -> Unit) {
        runOnIoSync<Unit> { block() }
    }

    fun <T> runOnIoSync(block: () -> T): T {
        if (shutDown) throw IOException("USB fd bridge shut down")
        if (testMode) return block()
        return CompletableFuture.supplyAsync(block, ioExecutor).get()
    }

    fun enumerateJson(): String {
        val mgr = usbManager ?: run {
            Log.w(TAG, "enumerateJson: no UsbManager")
            return JSONObject().put("ports", JSONObject()).toString()
        }
        val ports = JSONObject()
        mgr.deviceList.values.forEach { device ->
            val key = device.deviceName
            try {
                ports.put(key, deviceInfo(device))
            } catch (e: Exception) {
                Log.e(TAG, "enumerateJson: skip $key: ${e.message}", e)
            }
        }
        val json = JSONObject().put("ports", ports).toString()
        Log.i(TAG, "enumerateJson: ${ports.length()} device(s) json=$json")
        return json
    }

    fun openDeviceFd(deviceName: String): Int {
        connections[deviceName]?.let {
            Log.i(TAG, "openDeviceFd reuse $deviceName fd=${it.fileDescriptor}")
            return it.fileDescriptor
        }
        val mgr = usbManager ?: throw IOException("no UsbManager")
        val device = mgr.deviceList.values.find { it.deviceName == deviceName }
            ?: throw IOException("device not found: $deviceName")
        if (!mgr.hasPermission(device)) requestPermission(device)
        // Do NOT claimInterface here — Rust nusb detach_and_claim owns claiming.
        // Pre-claim makes the same fd report "io interface is busy" on open.
        val conn = mgr.openDevice(device) ?: throw IOException("open failed: $deviceName")
        connections[deviceName] = conn
        Log.i(
            TAG,
            "openDeviceFd $deviceName fd=${conn.fileDescriptor} ifaces=${device.interfaceCount} (unclaimed for nusb)",
        )
        return conn.fileDescriptor
    }

    fun closeDeviceFd(deviceName: String) {
        connections.remove(deviceName)?.close()
    }

    /** Test-only: inject a pre-opened connection (Robolectric / harness). */
    fun adoptConnectionForTest(deviceName: String, connection: UsbDeviceConnection) {
        check(testMode)
        connections[deviceName] = connection
    }

    /** Nested [JSONObject] (not Map) — avoids ambiguous `JSONObject.put` wrap. */
    private fun deviceInfo(device: UsbDevice): JSONObject {
        fun safeName(block: () -> String?): String = try {
            block() ?: ""
        } catch (e: SecurityException) {
            Log.w(TAG, "deviceInfo string denied for ${device.deviceName}: ${e.message}")
            ""
        }
        val ifaces = JSONArray()
        for (i in 0 until device.interfaceCount) {
            val iface = device.getInterface(i)
            ifaces.put(
                JSONObject()
                    .put("id", iface.id)
                    .put("class", iface.interfaceClass)
                    .put("subclass", iface.interfaceSubclass)
                    .put("protocol", iface.interfaceProtocol),
            )
        }
        return JSONObject()
            .put("type", "Usb")
            .put("vid", "0x%04X".format(device.vendorId))
            .put("pid", "0x%04X".format(device.productId))
            .put("manufacturer", safeName { device.manufacturerName })
            .put("product", safeName { device.productName })
            .put("serial_number", safeName { device.serialNumber })
            .put("interfaces", ifaces)
    }

    private fun requestPermission(device: UsbDevice) {
        val mgr = usbManager ?: throw IOException("no UsbManager")
        val ctx = context ?: throw IOException("no context")
        val name = device.deviceName
        val fut = CompletableFuture<Boolean>()
        permissionFutures[name] = fut
        val intent = Intent(ACTION_USB_PERMISSION).apply {
            setPackage(ctx.packageName)
        }
        val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            PendingIntent.FLAG_MUTABLE
        } else {
            0
        }
        val pi = PendingIntent.getBroadcast(ctx, 0, intent, flags)
        mgr.requestPermission(device, pi)
        val granted = try {
            if (testMode) {
                fut.get(200, TimeUnit.MILLISECONDS)
            } else {
                fut.get(30, TimeUnit.SECONDS)
            }
        } catch (_: TimeoutException) {
            false
        }
        if (!granted) {
            throw IOException("USB permission denied for $name")
        }
    }

    private fun deviceFromIntent(intent: Intent): UsbDevice? =
        if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
        }
}

private object JniNativeSink : UsbFdBridge.NativeSink {
    override fun onPortListChange() = app.tauri.serialplugin.MobileBridge.onPortListChange()
    override fun onDeviceDetached(deviceName: String) =
        app.tauri.serialplugin.MobileBridge.onDeviceDetached(deviceName)
}
