// GATT handling patterns (close() on every disconnect, one retry on transient
// status 133/62/129, Android 13 write API split, permission re-check after the
// dialog's pause/resume) follow tauri-plugin-blec's Peripheral.kt / BlecPlugin.kt,
// https://github.com/MnlPhlp/tauri-plugin-blec (MIT OR Apache-2.0).
package app.tauri.serialplugin.manager

import android.annotation.SuppressLint
import android.app.Activity
import android.app.ActivityManager
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.location.LocationManager
import android.os.Build
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log
import androidx.annotation.VisibleForTesting
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import org.json.JSONObject
import java.io.IOException
import java.lang.ref.WeakReference
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** Pure helpers (unit-tested without a Bluetooth stack). */
object BleLogic {
    val NUS_SERVICE: UUID = UUID.fromString("6E400001-B5A3-F393-E0A9-E50E24DCCA9E")
    /** Central writes here. */
    val NUS_RX: UUID = UUID.fromString("6E400002-B5A3-F393-E0A9-E50E24DCCA9E")
    /** Peripheral notifies here. */
    val NUS_TX: UUID = UUID.fromString("6E400003-B5A3-F393-E0A9-E50E24DCCA9E")
    val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    const val REQUEST_MTU = 247
    const val UNKNOWN = "Unknown"

    /** GATT statuses that are usually transient on Android; worth one more try. */
    val TRANSIENT_STATUS = setOf(133, 62, 129)

    /**
     * CCCD write refused until the link is encrypted: INSUFFICIENT_AUTHENTICATION (5),
     * INSUFFICIENT_ENCRYPTION (15), GATT_AUTH_FAIL (137). Android starts pairing itself.
     */
    val AUTH_STATUS = setOf(5, 15, 137)

    /** BluetoothStatusCodes.ERROR_GATT_WRITE_REQUEST_BUSY (API 33 int-returning writes). */
    const val ERROR_GATT_WRITE_REQUEST_BUSY = 201
    /** Pre-API-33 boolean write returned false: busy, or the link is already gone. */
    const val PRE33_REFUSED = -2

    /** Start codes worth retrying while the connection is still alive. */
    private val BUSY_CODES = setOf(ERROR_GATT_WRITE_REQUEST_BUSY, PRE33_REFUSED)

    /**
     * Runtime permissions BLE scan + connect need on [sdk]. [scanDerivesLocation]: the app's
     * BLUETOOTH_SCAN lacks `neverForLocation` (it declared it itself), so Android 12+ also
     * wants location for scan results.
     */
    fun requiredPermissions(sdk: Int, scanDerivesLocation: Boolean = false): List<String> =
        if (sdk >= Build.VERSION_CODES.S) {
            listOf("android.permission.BLUETOOTH_SCAN", "android.permission.BLUETOOTH_CONNECT") +
                if (scanDerivesLocation) listOf("android.permission.ACCESS_FINE_LOCATION") else emptyList()
        } else {
            // API <= 30: scan results need location (BLUETOOTH/ADMIN are install-time).
            listOf("android.permission.ACCESS_FINE_LOCATION")
        }

    fun missingPermissions(sdk: Int, scanDerivesLocation: Boolean = false, granted: (String) -> Boolean): List<String> =
        requiredPermissions(sdk, scanDerivesLocation).filterNot(granted)

    /** Scans return nothing (no error) with Location Services off. */
    fun needsLocationServices(sdk: Int, scanDerivesLocation: Boolean): Boolean =
        sdk <= Build.VERSION_CODES.R || scanDerivesLocation

    /** Largest attribute value (Core spec Vol 3 Part F 3.2.9), whatever the MTU. */
    const val MAX_ATTRIBUTE_LEN = 512

    /**
     * Bytes per ATT write: MTU minus the 3-byte ATT header (default MTU 23), at most 512
     * (Android 14+ always negotiates 517).
     */
    fun chunkSize(mtu: Int): Int = minOf(maxOf(mtu, 23) - 3, MAX_ATTRIBUTE_LEN)

    /**
     * Every Nth chunk is written with response when RX allows both types, so the
     * peripheral can push back before its buffer overflows (as Adafruit's UART apps do).
     */
    const val WRITE_PACE_EVERY = 8

    /** Write type of the [n]th chunk (1-based) for an RX characteristic with [props]. */
    fun writeTypeFor(n: Int, props: Int): Int {
        val withResp = props and BluetoothGattCharacteristic.PROPERTY_WRITE != 0
        val noResp = props and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0
        return when {
            noResp && (!withResp || n % WRITE_PACE_EVERY != 0) -> BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
            else -> BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        }
    }

    /** CCCD value for a TX characteristic with [props]: notify, or indicate when it only indicates. */
    fun cccdValue(props: Int): ByteArray =
        if (props and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0) {
            BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
        } else {
            BluetoothGattDescriptor.ENABLE_INDICATION_VALUE
        }

    /**
     * Wait before service discovery: a bonded peer sends Service Changed right after
     * connecting, which would invalidate the handles of a discovery started at once
     * (Nordic's Android-BLE-Library waits the same 1.6 s).
     */
    fun discoveryDelayMs(bondState: Int): Long = if (bondState == BluetoothDevice.BOND_BONDED) 1_600L else 0L

    fun chunks(data: ByteArray, mtu: Int): List<ByteArray> {
        val size = chunkSize(mtu)
        return (data.indices step size).map { data.copyOfRange(it, minOf(it + size, data.size)) }
    }

    /**
     * Runs [start] again while it returns a busy code, for up to [budgetMs]; between tries
     * checks [alive] so a dropped link fails as "disconnected", not as "busy".
     */
    fun startWhileBusy(
        budgetMs: Long,
        alive: () -> Boolean,
        start: () -> Int,
        now: () -> Long = { SystemClock.uptimeMillis() },
        sleep: (Long) -> Unit = { Thread.sleep(it) },
    ): Int {
        val end = now() + budgetMs
        while (true) {
            val code = start()
            if (code !in BUSY_CODES || now() >= end) return code
            sleep(10)
            if (!alive()) throw IOException("BLE device disconnected")
        }
    }

    data class ScanEntry(val address: String, val name: String?, val rssi: Int)

    /** Same shape as the desktop scan: `ble://<MAC>` → port info, all values strings. */
    fun scanJson(entries: Collection<ScanEntry>): String {
        val out = JSONObject()
        for (e in entries) {
            out.put(
                "ble://${e.address}",
                JSONObject()
                    .put("type", "BLE")
                    .put("product", e.name?.takeIf { it.isNotEmpty() } ?: UNKNOWN)
                    .put("manufacturer", UNKNOWN)
                    .put("serial_number", e.address)
                    .put("vid", UNKNOWN)
                    .put("pid", UNKNOWN)
                    .put("rssi", e.rssi.toString()),
            )
        }
        return out.toString()
    }
}

/** The GATT callback a step waits for. */
enum class GattEvent { CONNECTED, SERVICES_DISCOVERED, MTU_CHANGED, DESCRIPTOR_WRITE, CHARACTERISTIC_WRITE }

/**
 * The one GATT operation in flight on a connection (Android allows one at a time).
 * A callback completes it only when it is the event (and descriptor / characteristic
 * UUID) the step waits for. Everything else is ignored: a peripheral-initiated MTU
 * exchange during discovery, a Service Changed re-discovery during a write, the late
 * callback of a step that already timed out.
 */
class GattOpSlot {
    class Pending(val event: GattEvent, val uuid: UUID?) {
        val result = CompletableFuture<Int>()
    }

    private val current = AtomicReference<Pending?>()

    fun begin(event: GattEvent, uuid: UUID? = null): Pending = Pending(event, uuid).also { current.set(it) }

    fun end(p: Pending) {
        current.compareAndSet(p, null)
    }

    /** True when this callback completed the pending step. */
    fun deliver(event: GattEvent, uuid: UUID?, status: Int): Boolean {
        val p = current.get() ?: return false
        if (p.event != event || (p.uuid != null && p.uuid != uuid)) return false
        return p.result.complete(status)
    }

    /** Link loss / close / Bluetooth off: whatever is pending fails. */
    fun fail(e: Throwable) {
        current.get()?.result?.completeExceptionally(e)
    }
}

/** The time left of one `open()`: every step gets `min(its own max, what is left)`. */
class Deadline(private val endMs: Long, private val now: () -> Long) {
    fun remaining(): Long = endMs - now()

    fun cap(stepMaxMs: Long, what: String): Long {
        val left = remaining()
        if (left <= 0) throw IOException("$what: BLE open timed out (connect budget used up)")
        return minOf(stepMaxMs, left)
    }

    companion object {
        fun after(ms: Long, now: () -> Long = { SystemClock.uptimeMillis() }): Deadline = Deadline(now() + ms, now)
    }
}

/**
 * Android (7+) lets an app start at most 5 scans per 30 s; more are silently given no
 * results. Counting our own starts turns that into a clear error. Scans other code in
 * the same app starts are not seen here.
 */
class ScanThrottle(private val max: Int = 5, private val windowMs: Long = 30_000) {
    private val starts = ArrayDeque<Long>()

    /** 0 (and records the start) when allowed, else the ms until a slot frees up. */
    @Synchronized
    fun acquire(now: Long): Long {
        while (starts.isNotEmpty() && now - starts.first() >= windowMs) starts.removeFirst()
        if (starts.size >= max) return windowMs - (now - starts.first())
        starts.addLast(now)
        return 0
    }
}

/**
 * Serializes writes per connection: one caller's buffer goes out as consecutive
 * MTU-sized chunks; concurrent callers never interleave. When a chunk fails after
 * earlier ones went out, returns the bytes sent so far (so `write_all` resends only
 * the rest); fails only when nothing was sent.
 */
class BleWriteQueue {
    private val lock = Any()

    fun write(data: ByteArray, mtu: Int, send: (ByteArray) -> Unit): Int = synchronized(lock) {
        var sent = 0
        for (chunk in BleLogic.chunks(data, mtu)) {
            try {
                send(chunk)
            } catch (e: IOException) {
                if (sent == 0) throw e
                Log.w("BleWriteQueue", "partial write: $sent of ${data.size} bytes, then ${e.message}")
                return sent
            }
            sent += chunk.size
        }
        sent
    }
}

/**
 * Nordic UART over BluetoothGatt for Rust `ble://` ports. All entry points block the
 * calling (Rust) thread; GATT callbacks only complete futures and forward bytes.
 */
@SuppressLint("MissingPermission") // checked at runtime by ensurePermissions()
class BleGattBridge(
    private val context: Context,
    activity: Activity?,
    private val sink: NativeSink = JniBleSink,
) {
    /** Only for the permission dialog; weak so the static BleNative does not pin it. */
    private val activityRef = WeakReference(activity)

    interface NativeSink {
        fun onBleData(handle: Long, data: ByteArray)
        fun onBleDisconnected(handle: Long, reason: String)
    }

    private class GattStatus(what: String, val status: Int) :
        IOException("$what failed: GATT status $status")

    private class NotStarted(what: String, code: Int) :
        IOException("$what could not start (code $code${if (code == BleLogic.ERROR_GATT_WRITE_REQUEST_BUSY || code == BleLogic.PRE33_REFUSED) ": GATT stayed busy" else ""})")

    private inner class Conn(val handle: Long) {
        @Volatile var gatt: BluetoothGatt? = null
        @Volatile var rx: BluetoothGattCharacteristic? = null
        /** Chunks written so far (write pacing, see [BleLogic.writeTypeFor]); guarded by [writes]. */
        var chunksSent = 0
        @Volatile var mtu = 23
        /** Set once notifications are on; a later link loss is reported to Rust. */
        @Volatile var ready = false
        @Volatile var closed = false
        val ops = GattOpSlot()
        val writes = BleWriteQueue()

        val alive: Boolean get() = !closed && gatt != null

        val callback = object : BluetoothGattCallback() {
            override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
                if (status == BluetoothGatt.GATT_SUCCESS && newState == BluetoothProfile.STATE_CONNECTED) {
                    ops.deliver(GattEvent.CONNECTED, null, status)
                    return
                }
                // Always close: leaked clients hit the 30-client limit → status 133.
                try {
                    g.close()
                } catch (e: Exception) {
                    Log.w(TAG, "gatt.close: ${e.message}")
                }
                gatt = null
                ops.fail(GattStatus("connection", status))
                linkLost(this@Conn, "link lost (status $status, state $newState)")
            }

            override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
                ops.deliver(GattEvent.SERVICES_DISCOVERED, null, status)
            }

            // API 31+: the peer's GATT table changed, the cached RX/TX handles are stale.
            override fun onServiceChanged(g: BluetoothGatt) {
                Log.w(TAG, "handle $handle: service changed")
                if (!ready) return // discovery runs after this during open()
                ops.fail(IOException("GATT services changed"))
                try {
                    g.disconnect()
                    g.close()
                } catch (e: Exception) {
                    Log.w(TAG, "gatt.close: ${e.message}")
                }
                gatt = null
                linkLost(this@Conn, "GATT services changed; open the port again")
            }

            override fun onMtuChanged(g: BluetoothGatt, value: Int, status: Int) {
                // Also fires for a peripheral-initiated exchange: keep the size either way.
                if (status == BluetoothGatt.GATT_SUCCESS) mtu = value
                ops.deliver(GattEvent.MTU_CHANGED, null, status)
            }

            override fun onDescriptorWrite(g: BluetoothGatt, d: BluetoothGattDescriptor, status: Int) {
                ops.deliver(GattEvent.DESCRIPTOR_WRITE, d.uuid, status)
            }

            override fun onCharacteristicWrite(
                g: BluetoothGatt,
                c: BluetoothGattCharacteristic,
                status: Int,
            ) {
                ops.deliver(GattEvent.CHARACTERISTIC_WRITE, c.uuid, status)
            }

            // API 33+
            override fun onCharacteristicChanged(
                g: BluetoothGatt,
                c: BluetoothGattCharacteristic,
                value: ByteArray,
            ) {
                if (c.uuid == BleLogic.NUS_TX) sink.onBleData(handle, value)
            }

            // API <= 32. Known platform limit: the characteristic object (and its `value`)
            // is shared, so a fast notifier can overwrite it before this runs; bytes can be
            // lost or repeated. Not fixable here; API 33+ passes the value by copy.
            @Deprecated("Deprecated in API 33")
            @Suppress("DEPRECATION")
            override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic) {
                if (Build.VERSION.SDK_INT >= 33) return // the 3-arg variant fires too
                val v = c.value ?: return
                if (c.uuid == BleLogic.NUS_TX) sink.onBleData(handle, v.copyOf())
            }
        }

        /**
         * Start one GATT op ([start] returns 0 when started; busy codes are retried for
         * [BUSY_RETRY_MS] while the link is alive) and wait for its [event] callback status.
         */
        fun step(what: String, event: GattEvent, uuid: UUID?, timeoutMs: Long, start: () -> Int): Int {
            val p = ops.begin(event, uuid)
            try {
                val code = BleLogic.startWhileBusy(BUSY_RETRY_MS, { alive }, start)
                if (code != 0) throw NotStarted(what, code)
                return p.result.get(timeoutMs, TimeUnit.MILLISECONDS)
            } catch (_: TimeoutException) {
                throw IOException("$what timed out after $timeoutMs ms")
            } catch (e: ExecutionException) {
                throw (e.cause as? IOException) ?: IOException("$what failed: ${e.cause}")
            } finally {
                ops.end(p)
            }
        }
    }

    private val connections = ConcurrentHashMap<Long, Conn>()
    private val adapter: BluetoothAdapter? =
        (context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

    /** address → device from a scan result (it carries the address type; random static on most NUS boards). */
    private val seenDevices = ConcurrentHashMap<String, BluetoothDevice>()
    /** advertised name → address, so `ble://<name>` does not need a scan per open. */
    private val nameToAddress = ConcurrentHashMap<String, String>()
    private val throttle = ScanThrottle()

    /** Bluetooth switched off: several stacks drop GATT clients without onConnectionStateChange. */
    private val adapterReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, intent: Intent) {
            when (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, -1)) {
                BluetoothAdapter.STATE_TURNING_OFF, BluetoothAdapter.STATE_OFF -> dropAll("Bluetooth turned off")
            }
        }
    }
    @Volatile private var receiverRegistered = false

    init {
        try {
            // Exported: the broadcast comes from the Bluetooth process; it is protected,
            // so other apps cannot send it.
            ContextCompat.registerReceiver(
                context,
                adapterReceiver,
                IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED),
                ContextCompat.RECEIVER_EXPORTED,
            )
            receiverRegistered = true
        } catch (e: Exception) {
            Log.w(TAG, "adapter state receiver: ${e.message}")
        }
    }

    // --- runtime permissions -------------------------------------------------

    private val permLock = Object()
    private var paused = false
    private var resumedAfterPause = false
    @Volatile var permissionWaitMs = 60_000L

    /** Forwarded from the activity lifecycle: the permission dialog pauses us. */
    fun onActivityPaused() = synchronized(permLock) { paused = true }

    fun onActivityResumed() = synchronized(permLock) {
        if (paused) {
            resumedAfterPause = true
            permLock.notifyAll()
        }
    }

    @Suppress("DEPRECATION") // getPackageInfo(String, Int): the flags overload is API 33+
    private val packageInfo: PackageInfo? by lazy {
        try {
            context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_PERMISSIONS)
        } catch (e: Exception) {
            Log.w(TAG, "getPackageInfo: ${e.message}")
            null
        }
    }

    /** The app declares BLUETOOTH_SCAN itself without `neverForLocation` (API 31+). */
    private val scanDerivesLocation: Boolean by lazy {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return@lazy false
        val pi = packageInfo ?: return@lazy false
        val i = pi.requestedPermissions?.indexOf("android.permission.BLUETOOTH_SCAN") ?: -1
        val flags = pi.requestedPermissionsFlags
        i >= 0 && flags != null && (flags[i] and PackageInfo.REQUESTED_PERMISSION_NEVER_FOR_LOCATION) == 0
    }

    private fun missing(): List<String> =
        BleLogic.missingPermissions(Build.VERSION.SDK_INT, scanDerivesLocation) {
            ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
        }

    private fun inForeground(): Boolean {
        val info = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(info)
        return info.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
    }

    @Synchronized
    private fun ensurePermissions() {
        val need = missing()
        if (need.isEmpty()) return
        val declared = packageInfo?.requestedPermissions?.toSet()
        val undeclared = if (declared == null) emptyList() else need.filterNot { it in declared }
        if (undeclared.isNotEmpty()) {
            throw IOException(
                "Bluetooth permission ${undeclared.joinToString()} is not declared in the app's AndroidManifest.xml; " +
                    "build with the plugin's `ble` feature through the Tauri CLI (it adds them) or declare them yourself",
            )
        }
        val act = activityRef.get() ?: throw IOException("Bluetooth permission missing (${need.joinToString()}) and no activity to ask")
        if (!inForeground()) {
            // Android shows no permission dialog for a background app; do not wait for one.
            throw IOException(
                "Bluetooth permission missing (${need.joinToString()}) and the app is in the background; " +
                    "open the app and retry",
            )
        }
        synchronized(permLock) {
            paused = false
            resumedAfterPause = false
        }
        Log.i(TAG, "requesting ${need.joinToString()}")
        act.runOnUiThread { ActivityCompat.requestPermissions(act, need.toTypedArray(), PERMISSION_REQUEST) }
        val start = System.currentTimeMillis()
        synchronized(permLock) {
            while (!resumedAfterPause && missing().isNotEmpty()) {
                val elapsed = System.currentTimeMillis() - start
                // No dialog within 3 s: permanently denied ("don't ask again").
                if (elapsed >= permissionWaitMs || (!paused && elapsed >= 3_000)) break
                permLock.wait(250)
            }
        }
        val still = missing()
        if (still.isNotEmpty()) {
            throw IOException(
                "Bluetooth permission denied (${still.joinToString()}); " +
                    "allow \"Nearby devices\" (Android 12+) or location in the app settings",
            )
        }
    }

    private fun enabledAdapter(): BluetoothAdapter {
        val a = adapter ?: throw IOException("this device has no Bluetooth adapter")
        if (!a.isEnabled) throw IOException("Bluetooth is off")
        return a
    }

    private fun checkLocationServices() {
        if (!BleLogic.needsLocationServices(Build.VERSION.SDK_INT, scanDerivesLocation)) return
        val lm = context.getSystemService(Context.LOCATION_SERVICE) as? LocationManager ?: return
        if (!LocationManagerCompat.isLocationEnabled(lm)) {
            throw IOException("Location services are off (needed for BLE scan on Android 11 and older); turn on Location")
        }
    }

    // --- scan --------------------------------------------------------------

    fun scanJson(timeoutMs: Long): String =
        BleLogic.scanJson(scan(listOf(NUS_FILTER), timeoutMs) { false }.values)

    /** Scan with [filters]; stops early when [stop] returns true for an entry. */
    private fun scan(
        filters: List<ScanFilter>,
        timeoutMs: Long,
        stop: (BleLogic.ScanEntry) -> Boolean,
    ): Map<String, BleLogic.ScanEntry> {
        ensurePermissions()
        val scanner = enabledAdapter().bluetoothLeScanner
            ?: throw IOException("Bluetooth LE scanner unavailable (Bluetooth off?)")
        checkLocationServices()
        val wait = throttle.acquire(SystemClock.elapsedRealtime())
        if (wait > 0) {
            throw IOException(
                "BLE scan throttled: Android allows 5 scan starts per 30 s per app; retry in ${(wait + 999) / 1000} s",
            )
        }
        val found = ConcurrentHashMap<String, BleLogic.ScanEntry>()
        val done = CompletableFuture<Unit>()
        val failure = AtomicInteger(0)
        val cb = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, r: ScanResult) {
                val name = r.scanRecord?.deviceName ?: try {
                    r.device.name
                } catch (_: SecurityException) {
                    null
                }
                val e = BleLogic.ScanEntry(r.device.address, name, r.rssi)
                found[e.address] = e
                seenDevices[e.address] = r.device
                if (!name.isNullOrEmpty()) nameToAddress[name] = e.address
                if (stop(e)) done.complete(Unit)
            }

            override fun onBatchScanResults(results: MutableList<ScanResult>) {
                results.forEach { onScanResult(0, it) }
            }

            override fun onScanFailed(errorCode: Int) {
                failure.set(errorCode)
                done.complete(Unit)
            }
        }
        val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
        scanner.startScan(filters, settings, cb)
        try {
            done.get(timeoutMs, TimeUnit.MILLISECONDS)
        } catch (_: TimeoutException) {
        } finally {
            try {
                scanner.stopScan(cb)
            } catch (e: Exception) {
                Log.w(TAG, "stopScan: ${e.message}")
            }
        }
        if (failure.get() != 0) throw IOException("BLE scan failed (ScanCallback error ${failure.get()})")
        return found
    }

    // --- connect / write / close ---------------------------------------------

    /**
     * [target] is a MAC (`AA:BB:..`) or an advertised name. [timeoutMs] is the budget of
     * the whole open: name/address scan, connect (and its one retry), discovery, MTU,
     * notifications and pairing.
     */
    fun connect(target: String, handle: Long, timeoutMs: Long) {
        val deadline = Deadline.after(timeoutMs)
        ensurePermissions()
        val adapter = enabledAdapter()
        val device = resolve(adapter, target, deadline)
        for (attempt in 1..2) {
            try {
                connectOnce(device, handle, deadline)
                return
            } catch (e: GattStatus) {
                val retry = attempt == 1 && e.status in BleLogic.TRANSIENT_STATUS && deadline.remaining() > RETRY_PAUSE_MS
                if (!retry) throw IOException(e.message)
                Log.w(TAG, "${device.address}: ${e.message}, retrying once")
                Thread.sleep(RETRY_PAUSE_MS)
            }
        }
    }

    private fun resolve(adapter: BluetoothAdapter, target: String, deadline: Deadline): BluetoothDevice {
        val mac = target.uppercase()
        if (BluetoothAdapter.checkBluetoothAddress(mac)) {
            seenDevices[mac]?.let { return it }
            adapter.bondedDevices?.firstOrNull { it.address == mac }?.let { return it }
            // Not seen in this process: getRemoteDevice() would assume a public address,
            // but most nRF / ESP32 NUS boards use a random static one (connect → timeout
            // or 133). A short scan for that address yields the device with its real type.
            try {
                scan(listOf(ScanFilter.Builder().setDeviceAddress(mac).build()), deadline.cap(ADDRESS_SCAN_MS, "address scan")) { true }
            } catch (e: IOException) {
                Log.w(TAG, "$mac: address scan skipped (${e.message}); connecting as a public address")
            }
            return seenDevices[mac] ?: adapter.getRemoteDevice(mac)
        }
        val cached = nameToAddress[target]?.let { seenDevices[it] }
        if (cached != null) return cached
        // Unfiltered: many NUS firmwares do not advertise the 128-bit service UUID (the
        // service is checked after connecting anyway).
        scan(emptyList(), deadline.cap(NAME_SCAN_MS, "name scan")) { it.name == target }
        val address = nameToAddress[target]
            ?: throw IOException("no BLE device named '$target' found")
        return seenDevices[address] ?: adapter.getRemoteDevice(address)
    }

    private fun connectOnce(device: BluetoothDevice, handle: Long, deadline: Deadline) {
        val conn = Conn(handle)
        connections[handle] = conn
        try {
            conn.step("connect to ${device.address}", GattEvent.CONNECTED, null, deadline.cap(Long.MAX_VALUE, "connect")) {
                conn.gatt = device.connectGatt(context, false, conn.callback, BluetoothDevice.TRANSPORT_LE)
                if (conn.gatt != null) 0 else -1
            }
            val gatt = conn.gatt ?: throw IOException("disconnected while connecting")
            // Pairing in progress: discovery now would be answered with stale handles.
            while (device.bondState == BluetoothDevice.BOND_BONDING) {
                deadline.cap(Long.MAX_VALUE, "pairing")
                Thread.sleep(100)
            }
            val delay = BleLogic.discoveryDelayMs(device.bondState)
            if (delay > 0) Thread.sleep(deadline.cap(delay, "service discovery"))
            val discover = {
                val st = conn.step("service discovery", GattEvent.SERVICES_DISCOVERED, null, deadline.cap(10_000, "service discovery")) {
                    ok(gatt.discoverServices())
                }
                if (st != BluetoothGatt.GATT_SUCCESS) throw GattStatus("service discovery", st)
            }
            discover()
            try {
                conn.step("MTU request", GattEvent.MTU_CHANGED, null, deadline.cap(3_000, "MTU request")) {
                    ok(gatt.requestMtu(BleLogic.REQUEST_MTU))
                }
            } catch (e: IOException) {
                Log.w(TAG, "${device.address}: ${e.message}; staying at MTU ${conn.mtu}")
            }
            // Shorter connection interval: one chunk per interval otherwise (fire and forget).
            gatt.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
            var service = gatt.getService(BleLogic.NUS_SERVICE)
            if (service?.getCharacteristic(BleLogic.NUS_RX) == null || service.getCharacteristic(BleLogic.NUS_TX) == null) {
                // Maybe a stale cached GATT table (a reflashed board): drop the cache once.
                if (refreshCache(gatt)) {
                    Log.w(TAG, "${device.address}: no NUS in the cached GATT table, refreshed it")
                    discover()
                    service = gatt.getService(BleLogic.NUS_SERVICE)
                }
            }
            service ?: throw IOException("${device.address} has no Nordic UART Service")
            val rx = service.getCharacteristic(BleLogic.NUS_RX)
                ?: throw IOException("${device.address} has no NUS RX characteristic")
            val tx = service.getCharacteristic(BleLogic.NUS_TX)
                ?: throw IOException("${device.address} has no NUS TX characteristic")
            conn.rx = rx
            if (!gatt.setCharacteristicNotification(tx, true)) throw IOException("enable notifications failed")
            val cccd = tx.getDescriptor(BleLogic.CCCD) ?: throw IOException("NUS TX has no CCCD")
            val enableNotifications = {
                conn.step("enable notifications", GattEvent.DESCRIPTOR_WRITE, BleLogic.CCCD, deadline.cap(5_000, "enable notifications")) {
                    val value = BleLogic.cccdValue(tx.properties)
                    if (Build.VERSION.SDK_INT >= 33) {
                        gatt.writeDescriptor(cccd, value)
                    } else {
                        @Suppress("DEPRECATION")
                        cccd.value = value
                        @Suppress("DEPRECATION")
                        if (gatt.writeDescriptor(cccd)) 0 else BleLogic.PRE33_REFUSED
                    }
                }
            }
            var ds = enableNotifications()
            if (ds in BleLogic.AUTH_STATUS) {
                awaitBond(device, ds, deadline, conn)
                ds = enableNotifications()
            }
            if (ds != BluetoothGatt.GATT_SUCCESS) throw GattStatus("enable notifications", ds)
            conn.ready = true
            Log.i(TAG, "handle $handle: ${device.address} connected, mtu ${conn.mtu}")
        } catch (e: Exception) {
            connections.remove(handle, conn)
            conn.closed = true
            conn.gatt?.let {
                it.disconnect()
                it.close()
            }
            throw e
        }
    }

    /** The CCCD needs an encrypted link: wait (within the open budget) for pairing, once. */
    private fun awaitBond(device: BluetoothDevice, status: Int, deadline: Deadline, conn: Conn) {
        Log.i(TAG, "${device.address}: CCCD write needs pairing (GATT status $status), waiting for bond")
        if (device.bondState == BluetoothDevice.BOND_NONE) device.createBond()
        var started = device.bondState == BluetoothDevice.BOND_BONDING
        while (device.bondState != BluetoothDevice.BOND_BONDED) {
            val state = device.bondState
            if (state == BluetoothDevice.BOND_BONDING) started = true
            val failed = started && state == BluetoothDevice.BOND_NONE
            if (failed || deadline.remaining() <= 0 || !conn.alive) {
                throw IOException(
                    "${device.address} requires pairing (GATT status $status) and pairing did not complete; " +
                        "pair it (Android Bluetooth settings or the system prompt) and open again",
                )
            }
            Thread.sleep(200)
        }
    }

    /** Hidden `BluetoothGatt.refresh()`: drops the cached GATT table. False if unavailable. */
    private fun refreshCache(gatt: BluetoothGatt): Boolean =
        try {
            gatt.javaClass.getMethod("refresh").invoke(gatt) as? Boolean ?: false
        } catch (e: Exception) {
            Log.w(TAG, "gatt.refresh: ${e.message}")
            false
        }

    fun write(handle: Long, data: ByteArray): Int {
        val conn = connections[handle] ?: throw IOException("BLE device disconnected")
        return conn.writes.write(data, conn.mtu) { chunk -> writeChunk(conn, chunk) }
    }

    private fun writeChunk(conn: Conn, chunk: ByteArray) {
        val rx = conn.rx ?: throw IOException("BLE device disconnected")
        val gatt = conn.gatt ?: throw IOException("BLE device disconnected")
        conn.chunksSent += 1
        val writeType = BleLogic.writeTypeFor(conn.chunksSent, rx.properties)
        val status = conn.step("write", GattEvent.CHARACTERISTIC_WRITE, BleLogic.NUS_RX, WRITE_TIMEOUT_MS) {
            if (Build.VERSION.SDK_INT >= 33) {
                gatt.writeCharacteristic(rx, chunk, writeType)
            } else {
                @Suppress("DEPRECATION")
                rx.writeType = writeType
                @Suppress("DEPRECATION")
                rx.value = chunk
                @Suppress("DEPRECATION")
                if (gatt.writeCharacteristic(rx)) 0 else BleLogic.PRE33_REFUSED
            }
        }
        if (status != BluetoothGatt.GATT_SUCCESS) throw IOException("write failed: GATT status $status")
    }

    /** Report a link loss to Rust once, for a port that was open (not a close Rust asked for). */
    private fun linkLost(conn: Conn, reason: String) {
        if (conn.ready && !conn.closed && connections.remove(conn.handle, conn)) {
            Log.i(TAG, "handle ${conn.handle}: $reason")
            sink.onBleDisconnected(conn.handle, reason)
        }
    }

    private fun dropAll(reason: String) {
        for (conn in connections.values.toList()) {
            conn.ops.fail(IOException(reason)) // a connect in progress fails and cleans up itself
            if (!conn.ready) continue
            conn.gatt?.let {
                try {
                    it.close()
                } catch (e: Exception) {
                    Log.w(TAG, "close ${conn.handle}: ${e.message}")
                }
            }
            conn.gatt = null
            linkLost(conn, reason)
            conn.closed = true
        }
    }

    fun close(handle: Long) {
        val conn = connections.remove(handle) ?: return
        conn.closed = true
        conn.ops.fail(IOException("closed"))
        conn.gatt?.let {
            try {
                it.disconnect()
                it.close()
            } catch (e: Exception) {
                Log.w(TAG, "close $handle: ${e.message}")
            }
        }
        conn.gatt = null
    }

    fun shutdown() {
        connections.keys.toList().forEach { close(it) }
        if (receiverRegistered) {
            receiverRegistered = false
            try {
                context.unregisterReceiver(adapterReceiver)
            } catch (e: Exception) {
                Log.w(TAG, "unregister adapter receiver: ${e.message}")
            }
        }
    }

    /** Test hook: an open (notifications on) port without a GATT link. */
    @VisibleForTesting
    internal fun adoptReadyForTest(handle: Long) {
        connections[handle] = Conn(handle).apply { ready = true }
    }

    companion object {
        private const val TAG = "BleGattBridge"
        private const val PERMISSION_REQUEST = 0x5E1B
        private const val BUSY_RETRY_MS = 2_000L
        private const val WRITE_TIMEOUT_MS = 5_000L
        private const val RETRY_PAUSE_MS = 500L
        private const val ADDRESS_SCAN_MS = 5_000L
        private const val NAME_SCAN_MS = 10_000L
        private val NUS_FILTER: ScanFilter =
            ScanFilter.Builder().setServiceUuid(ParcelUuid(BleLogic.NUS_SERVICE)).build()

        private fun ok(started: Boolean) = if (started) 0 else -1
    }
}

private object JniBleSink : BleGattBridge.NativeSink {
    override fun onBleData(handle: Long, data: ByteArray) =
        app.tauri.serialplugin.MobileBridge.onBleData(handle, data)

    override fun onBleDisconnected(handle: Long, reason: String) =
        app.tauri.serialplugin.MobileBridge.onBleDisconnected(handle, reason)
}
