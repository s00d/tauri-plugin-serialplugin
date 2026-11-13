package app.tauri.serialplugin.manager

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanRecord
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.content.pm.PackageInfo
import android.location.LocationManager
import android.os.Build
import android.os.Looper
import android.os.ParcelUuid
import android.os.Process
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.kotlin.any
import org.mockito.kotlin.atLeastOnce
import org.mockito.kotlin.doAnswer
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowSystemClock
import java.io.IOException
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

/**
 * The GATT open / write / close flow of [BleGattBridge] against mocked Bluetooth objects:
 * every GATT call answers through the captured [BluetoothGattCallback], as the stack does.
 * Robolectric's SystemClock stands still (paused looper), so an open's [Deadline] never runs
 * out by itself here; step timeouts use real time.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class BleGattBridgeFlowTest {

    private companion object {
        const val MAC = "AA:BB:CC:DD:EE:FF"
        const val MAC2 = "11:22:33:44:55:66"
        const val SCAN = "android.permission.BLUETOOTH_SCAN"
        const val CONNECT = "android.permission.BLUETOOTH_CONNECT"
        const val FINE = "android.permission.ACCESS_FINE_LOCATION"
        const val NO_RESP = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        const val WITH_RESP = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
    }

    private val app: Application = ApplicationProvider.getApplicationContext()

    private class Sink : BleGattBridge.NativeSink {
        val data: MutableList<Pair<Long, ByteArray>> = Collections.synchronizedList(mutableListOf())
        val disconnects: MutableList<Pair<Long, String>> = Collections.synchronizedList(mutableListOf())
        override fun onBleData(handle: Long, data: ByteArray) {
            this.data.add(handle to data)
        }

        override fun onBleDisconnected(handle: Long, reason: String) {
            disconnects.add(handle to reason)
        }
    }

    /** The app's declared permissions (PIT does not load the merged test manifest). */
    private fun declare(perms: Array<String>?, flags: IntArray? = perms?.let { IntArray(it.size) }) {
        val pi = shadowOf(app.packageManager).getInternalMutablePackageInfo(app.packageName)
        pi.requestedPermissions = perms
        pi.requestedPermissionsFlags = flags
    }

    private fun locationEnabled(on: Boolean) {
        shadowOf(app.getSystemService(LocationManager::class.java)).setLocationEnabled(on)
    }

    /** Declares and grants what BLE needs on this API level (BLUETOOTH_SCAN is neverForLocation). */
    private fun grantBle() {
        if (Build.VERSION.SDK_INT >= 31) {
            declare(arrayOf(SCAN, CONNECT), intArrayOf(PackageInfo.REQUESTED_PERMISSION_NEVER_FOR_LOCATION, 0))
            shadowOf(app).grantPermissions(SCAN, CONNECT)
        } else {
            declare(arrayOf(FINE))
            shadowOf(app).grantPermissions(FINE)
        }
        locationEnabled(true)
    }

    private fun foreground(importance: Int) {
        val info = ActivityManager.RunningAppProcessInfo().apply {
            pid = Process.myPid()
            processName = app.packageName
            this.importance = importance
        }
        shadowOf(app.getSystemService(ActivityManager::class.java)).setProcesses(listOf(info))
    }

    private fun <T> ArrayDeque<T>.next(): T = synchronized(this) { if (size > 1) removeFirst() else first() }

    private fun msg(block: () -> Unit): String = assertThrows(IOException::class.java) { block() }.message!!

    private fun elapsedMs(block: () -> Unit): Long {
        val t0 = System.nanoTime()
        block()
        return (System.nanoTime() - t0) / 1_000_000
    }

    private fun bluetoothState(ctx: Context, state: Int) {
        ctx.sendBroadcast(Intent(BluetoothAdapter.ACTION_STATE_CHANGED).putExtra(BluetoothAdapter.EXTRA_STATE, state))
        shadowOf(Looper.getMainLooper()).idle()
    }

    private inner class Harness(activity: Activity? = null, hasAdapter: Boolean = true) {
        val adapter: BluetoothAdapter = mock()
        val scanner: BluetoothLeScanner = mock()
        val manager: BluetoothManager = mock()
        val sink = Sink()
        val scanFilters: MutableList<List<ScanFilter>> = Collections.synchronizedList(mutableListOf())
        val scanCallbacks: MutableList<ScanCallback> = Collections.synchronizedList(mutableListOf())
        @Volatile var onScan: (ScanCallback) -> Unit = {}
        val ctx: Context = object : ContextWrapper(app) {
            override fun getSystemService(name: String): Any? =
                if (name == Context.BLUETOOTH_SERVICE) manager.takeIf { hasAdapter } else super.getSystemService(name)
        }
        val bridge: BleGattBridge

        init {
            whenever(manager.adapter).thenReturn(adapter)
            whenever(adapter.isEnabled).thenReturn(true)
            whenever(adapter.bluetoothLeScanner).thenReturn(scanner)
            whenever(adapter.bondedDevices).thenReturn(emptySet())
            doAnswer { inv ->
                scanFilters.add(inv.getArgument(0))
                val cb = inv.getArgument<ScanCallback>(2)
                scanCallbacks.add(cb)
                onScan(cb)
                null
            }.whenever(scanner).startScan(any<List<ScanFilter>>(), any<ScanSettings>(), any<ScanCallback>())
            bridge = BleGattBridge(ctx, activity, sink)
        }
    }

    private fun scanResult(device: BluetoothDevice, rssi: Int, recordName: String? = null): ScanResult {
        val r = mock<ScanResult>()
        whenever(r.device).thenReturn(device)
        whenever(r.rssi).thenReturn(rssi)
        if (recordName != null) {
            val rec = mock<ScanRecord>()
            whenever(rec.deviceName).thenReturn(recordName)
            whenever(r.scanRecord).thenReturn(rec)
        }
        return r
    }

    /** A NUS peripheral: GATT calls answer synchronously through the callback. */
    private inner class Peer(
        val h: Harness,
        val mac: String = MAC,
        txProps: Int = BluetoothGattCharacteristic.PROPERTY_NOTIFY,
        hasRx: Boolean = true,
        hasTx: Boolean = true,
        hasCccd: Boolean = true,
        bonded: Boolean = true,
        name: String? = null,
    ) {
        val device: BluetoothDevice = mock()
        val gatt: BluetoothGatt = mock()
        @Volatile var cb: BluetoothGattCallback? = null
        @Volatile var bond = BluetoothDevice.BOND_NONE
        val service = BluetoothGattService(BleLogic.NUS_SERVICE, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        val rx = BluetoothGattCharacteristic(
            BleLogic.NUS_RX,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE,
        )
        val tx = BluetoothGattCharacteristic(BleLogic.NUS_TX, txProps, 0)
        val cccd = BluetoothGattDescriptor(BleLogic.CCCD, BluetoothGattDescriptor.PERMISSION_WRITE)

        /** Status of each connection attempt (the last one repeats). */
        val connectStatuses = ArrayDeque(listOf(0))
        val connects = AtomicInteger()
        @Volatile var connectReturnsNull = false
        val discoveryStatuses = ArrayDeque(listOf(0))
        val discoveries = AtomicInteger()
        @Volatile var discoverStarts = true
        @Volatile var answerDiscovery = true
        @Volatile var onDiscover: () -> Unit = {}
        @Volatile var mtuStarts = true
        /** (mtu, status) of the MTU callback; null: none. */
        @Volatile var mtuReply: Pair<Int, Int>? = 247 to 0
        /** What getService(NUS) returns, call by call (the last one repeats). */
        val services = ArrayDeque<BluetoothGattService?>(listOf(service))
        @Volatile var notifyOk = true
        val cccdStatuses = ArrayDeque(listOf(0))
        val cccdValues: MutableList<ByteArray> = Collections.synchronizedList(mutableListOf())
        /** (chunk, write type) of every write started. */
        val writes: MutableList<Pair<ByteArray, Int>> = Collections.synchronizedList(mutableListOf())
        /** Status of the nth write's callback; null: no callback. */
        @Volatile var writeStatus: (Int) -> Int? = { 0 }
        /** API <= 32: false makes the nth CCCD write not start (busy). */
        @Volatile var cccdStarts: (Int) -> Boolean = { true }
        /** False: the nth write does not start (busy) and gets no callback. */
        @Volatile var writeStarts: (Int) -> Boolean = { true }

        init {
            if (hasCccd) tx.addDescriptor(cccd)
            if (hasRx) service.addCharacteristic(rx)
            if (hasTx) service.addCharacteristic(tx)
            whenever(device.address).thenReturn(mac)
            whenever(device.name).thenReturn(name)
            whenever(device.bondState).thenAnswer { bond }
            if (bonded) whenever(h.adapter.bondedDevices).thenReturn(setOf(device))
            whenever(
                device.connectGatt(any<Context>(), eq(false), any<BluetoothGattCallback>(), eq(BluetoothDevice.TRANSPORT_LE)),
            ).thenAnswer { inv ->
                val c = inv.getArgument<BluetoothGattCallback>(2)
                cb = c
                connects.incrementAndGet()
                if (connectReturnsNull) return@thenAnswer null
                val st = connectStatuses.next()
                val state = if (st == 0) BluetoothProfile.STATE_CONNECTED else BluetoothProfile.STATE_DISCONNECTED
                c.onConnectionStateChange(gatt, st, state)
                gatt
            }
            whenever(gatt.discoverServices()).thenAnswer {
                discoveries.incrementAndGet()
                onDiscover()
                if (discoverStarts && answerDiscovery) cb!!.onServicesDiscovered(gatt, discoveryStatuses.next())
                discoverStarts
            }
            whenever(gatt.requestMtu(any<Int>())).thenAnswer {
                val reply = mtuReply
                if (mtuStarts && reply != null) cb!!.onMtuChanged(gatt, reply.first, reply.second)
                mtuStarts
            }
            whenever(gatt.getService(BleLogic.NUS_SERVICE)).thenAnswer { services.next() }
            whenever(gatt.setCharacteristicNotification(tx, true)).thenAnswer { notifyOk }
            if (Build.VERSION.SDK_INT >= 33) {
                whenever(gatt.writeDescriptor(eq(cccd), any<ByteArray>())).thenAnswer { inv ->
                    cccdValues.add(inv.getArgument<ByteArray>(1).copyOf())
                    cb!!.onDescriptorWrite(gatt, cccd, cccdStatuses.next())
                    0
                }
                whenever(gatt.writeCharacteristic(eq(rx), any<ByteArray>(), any<Int>())).thenAnswer { inv ->
                    writes.add(inv.getArgument<ByteArray>(1).copyOf() to inv.getArgument<Int>(2))
                    val n = writes.size
                    if (writeStarts(n)) {
                        writeStatus(n)?.let { cb!!.onCharacteristicWrite(gatt, rx, it) }
                        0
                    } else {
                        BleLogic.ERROR_GATT_WRITE_REQUEST_BUSY
                    }
                }
            } else {
                @Suppress("DEPRECATION")
                whenever(gatt.writeDescriptor(cccd)).thenAnswer {
                    cccdValues.add(cccd.value.copyOf())
                    if (cccdStarts(cccdValues.size)) {
                        cb!!.onDescriptorWrite(gatt, cccd, cccdStatuses.next())
                        true
                    } else {
                        false
                    }
                }
                @Suppress("DEPRECATION")
                whenever(gatt.writeCharacteristic(rx)).thenAnswer {
                    writes.add(rx.value.copyOf() to rx.writeType)
                    val n = writes.size
                    if (writeStarts(n)) {
                        writeStatus(n)?.let { cb!!.onCharacteristicWrite(gatt, rx, it) }
                        true
                    } else {
                        false
                    }
                }
            }
        }

        fun open(handle: Long = 1, budgetMs: Long = 2_000, target: String = mac) = h.bridge.connect(target, handle, budgetMs)
    }

    private fun joined(chunks: List<Pair<ByteArray, Int>>): ByteArray = chunks.fold(ByteArray(0)) { acc, c -> acc + c.first }

    // --- open / write / notifications / close ---------------------------------------------

    @Test
    fun openWritesForwardsNotificationsAndCloses() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()

        verify(p.gatt).requestMtu(BleLogic.REQUEST_MTU)
        verify(p.gatt).requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
        verify(p.gatt).setCharacteristicNotification(p.tx, true)
        assertArrayEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE, p.cccdValues.single())
        verify(p.gatt, never()).close()

        // MTU 247: 244-byte chunks, all without response (fewer than WRITE_PACE_EVERY).
        val data = ByteArray(300) { it.toByte() }
        assertEquals(300, h.bridge.write(1, data))
        assertEquals(listOf(244, 56), p.writes.map { it.first.size })
        assertEquals(listOf(NO_RESP, NO_RESP), p.writes.map { it.second })
        assertArrayEquals(data, joined(p.writes))

        // API <= 32 notifications: only NUS TX, a copy of the shared value.
        val value = byteArrayOf(1, 2, 3)
        @Suppress("DEPRECATION")
        p.tx.value = value
        @Suppress("DEPRECATION")
        p.cb!!.onCharacteristicChanged(p.gatt, p.tx)
        value[0] = 9
        @Suppress("DEPRECATION")
        p.cb!!.onCharacteristicChanged(p.gatt, p.rx)
        @Suppress("DEPRECATION")
        p.cb!!.onCharacteristicChanged(p.gatt, BluetoothGattCharacteristic(BleLogic.NUS_TX, 0, 0)) // no value
        assertEquals(1, h.sink.data.size)
        assertEquals(1L, h.sink.data[0].first)
        assertArrayEquals(byteArrayOf(1, 2, 3), h.sink.data[0].second)

        // A "connected" callback after open changes nothing.
        p.cb!!.onConnectionStateChange(p.gatt, BluetoothGatt.GATT_SUCCESS, BluetoothProfile.STATE_CONNECTED)
        verify(p.gatt, never()).close()

        h.bridge.close(1)
        verify(p.gatt).disconnect()
        verify(p.gatt).close()
        assertTrue(msg { h.bridge.write(1, byteArrayOf(1)) }.contains("disconnected"))
        // The stack's late disconnect callback for a port Rust closed is not a link loss.
        p.cb!!.onConnectionStateChange(p.gatt, BluetoothGatt.GATT_SUCCESS, BluetoothProfile.STATE_DISCONNECTED)
        assertTrue(h.sink.disconnects.isEmpty())
    }

    @Test
    fun everyEighthChunkGoesWithResponseAtDefaultMtu() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.mtuStarts = false // MTU request refused: stays at 23
        val ms = elapsedMs { p.open() }
        assertTrue("a refused MTU request is not waited for ($ms ms)", ms < 1_000)
        val data = ByteArray(180) { it.toByte() }
        assertEquals(180, h.bridge.write(1, data))
        assertEquals(List(9) { 20 }, p.writes.map { it.first.size })
        assertEquals(List(7) { NO_RESP } + WITH_RESP + NO_RESP, p.writes.map { it.second })
        assertArrayEquals(data, joined(p.writes))
    }

    @Test
    fun peripheralMtuIsKeptOnlyOnSuccess() {
        grantBle()
        val h = Harness()
        val ok = Peer(h)
        ok.mtuReply = 100 to BluetoothGatt.GATT_SUCCESS
        ok.open(handle = 1)
        assertEquals(100, h.bridge.write(1, ByteArray(100)))
        assertEquals(listOf(97, 3), ok.writes.map { it.first.size })

        val h2 = Harness()
        val failed = Peer(h2)
        failed.mtuReply = 100 to 4
        failed.open(handle = 2)
        assertEquals(30, h2.bridge.write(2, ByteArray(30)))
        assertEquals(listOf(20, 10), failed.writes.map { it.first.size })
    }

    @Test
    fun mtuTimeoutKeepsTheDefaultAndOpenGoesOn() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.mtuReply = null // no callback: the step times out after the budget (300 ms)
        p.open(budgetMs = 300)
        assertEquals(30, h.bridge.write(1, ByteArray(30)))
        assertEquals(listOf(20, 10), p.writes.map { it.first.size })
    }

    @Test
    fun failedChunkReturnsWhatWentOutOrFails() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.mtuStarts = false
        p.open()
        p.writeStatus = { n -> if (n == 2) 133 else 0 }
        assertEquals(20, h.bridge.write(1, ByteArray(50)))
        p.writeStatus = { 133 }
        assertEquals("write failed: GATT status 133", msg { h.bridge.write(1, ByteArray(5)) })
    }

    @Test
    fun busyWriteFailsAsDisconnectedWhenTheLinkDrops() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()
        p.writeStarts = { n ->
            if (n == 3) p.cb!!.onConnectionStateChange(p.gatt, 8, BluetoothProfile.STATE_DISCONNECTED)
            false
        }
        assertEquals("BLE device disconnected", msg { h.bridge.write(1, byteArrayOf(1)) })
        assertEquals(3, p.writes.size)
        assertEquals(listOf(1L to "link lost (status 8, state 0)"), h.sink.disconnects.toList())
    }

    @Test
    fun linkLossAfterOpenIsReportedOnceAndClosesGatt() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open(handle = 4)
        p.cb!!.onConnectionStateChange(p.gatt, BluetoothGatt.GATT_SUCCESS, BluetoothProfile.STATE_DISCONNECTED)
        verify(p.gatt).close()
        assertEquals(listOf(4L to "link lost (status 0, state 0)"), h.sink.disconnects.toList())
        assertTrue(msg { h.bridge.write(4, byteArrayOf(1)) }.contains("disconnected"))
        p.cb!!.onConnectionStateChange(p.gatt, 19, BluetoothProfile.STATE_DISCONNECTED)
        assertEquals(1, h.sink.disconnects.size)
    }

    // --- open failures --------------------------------------------------------------------

    @Test
    fun transientConnectFailureIsRetriedOnce() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.connectStatuses.apply { clear(); addAll(listOf(133, 0)) }
        val ms = elapsedMs { p.open() }
        assertTrue("pauses before the retry ($ms ms)", ms >= 500)
        assertEquals(2, p.connects.get())
        verify(p.gatt, atLeastOnce()).close()
        assertTrue("a port that never opened is not reported", h.sink.disconnects.isEmpty())
        assertEquals(1, h.bridge.write(1, byteArrayOf(7)))
    }

    @Test
    fun transientFailureTwiceGivesUp() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.connectStatuses.apply { clear(); add(133) }
        assertEquals("connection failed: GATT status 133", msg { p.open() })
        assertEquals(2, p.connects.get())
        assertTrue(h.sink.disconnects.isEmpty())
        assertTrue(msg { h.bridge.write(1, byteArrayOf(1)) }.contains("disconnected"))
    }

    @Test
    fun otherConnectFailuresAreNotRetried() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.connectStatuses.apply { clear(); add(8) }
        assertEquals("connection failed: GATT status 8", msg { p.open() })
        assertEquals(1, p.connects.get())
    }

    @Test
    fun retryNeedsMoreBudgetThanThePause() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.connectStatuses.apply { clear(); addAll(listOf(133, 0)) }
        msg { p.open(budgetMs = 500) }
        assertEquals(1, p.connects.get())

        val p2 = Peer(Harness())
        p2.connectStatuses.apply { clear(); addAll(listOf(133, 0)) }
        p2.open(budgetMs = 501)
        assertEquals(2, p2.connects.get())
    }

    @Test
    fun connectGattReturningNullFails() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.connectReturnsNull = true
        assertEquals("connect to $MAC could not start (code -1)", msg { p.open() })
    }

    @Test
    fun spentBudgetFailsBeforeConnecting() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        assertEquals("connect: BLE open timed out (connect budget used up)", msg { p.open(budgetMs = 0) })
        assertEquals(0, p.connects.get())
    }

    @Test
    fun discoveryFailureClosesTheLink() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.discoveryStatuses.apply { clear(); add(5) }
        assertEquals("service discovery failed: GATT status 5", msg { p.open() })
        assertEquals(1, p.connects.get())
        verify(p.gatt).disconnect()
        verify(p.gatt).close()

        val p2 = Peer(Harness())
        p2.discoverStarts = false
        assertEquals("service discovery could not start (code -1)", msg { p2.open() })
        verify(p2.gatt).close()
    }

    @Test
    fun discoveryWithoutCallbackTimesOutWithinTheBudget() {
        grantBle()
        val p = Peer(Harness())
        p.answerDiscovery = false
        assertEquals("service discovery timed out after 200 ms", msg { p.open(budgetMs = 200) })
        verify(p.gatt).close()
    }

    @Test
    fun missingNusPartsAreClearErrors() {
        grantBle()
        val noService = Peer(Harness())
        noService.services.apply { clear(); add(null) }
        assertEquals("$MAC has no Nordic UART Service", msg { noService.open() })
        assertEquals("no refresh() on the mock: no second discovery", 1, noService.discoveries.get())
        verify(noService.gatt).close()

        assertEquals("$MAC has no NUS RX characteristic", msg { Peer(Harness(), hasRx = false).open() })
        assertEquals("$MAC has no NUS TX characteristic", msg { Peer(Harness(), hasTx = false).open() })
        assertEquals("NUS TX has no CCCD", msg { Peer(Harness(), hasCccd = false).open() })
        val noNotify = Peer(Harness())
        noNotify.notifyOk = false
        assertEquals("enable notifications failed", msg { noNotify.open() })
        verify(noNotify.gatt).close()
    }

    @Test
    fun staleGattCacheIsRefreshedAndDiscoveredAgain() {
        grantBle()
        val p = Peer(Harness())
        p.services.apply { clear(); addAll(listOf(null, p.service)) }
        val refresh = BluetoothGatt::class.java.getMethod("refresh")
        whenever(refresh.invoke(p.gatt)).thenReturn(true)
        p.open()
        assertEquals(2, p.discoveries.get())
        assertEquals(1, p.h.bridge.write(1, byteArrayOf(1)))
    }

    @Test
    fun indicateOnlyTxGetsTheIndicationCccdValue() {
        grantBle()
        val p = Peer(Harness(), txProps = BluetoothGattCharacteristic.PROPERTY_INDICATE)
        p.open()
        assertArrayEquals(BluetoothGattDescriptor.ENABLE_INDICATION_VALUE, p.cccdValues.single())
    }

    // --- pairing ----------------------------------------------------------------------------

    @Test
    fun cccdNeedingEncryptionPairsAndRetries() {
        grantBle()
        val p = Peer(Harness())
        p.cccdStatuses.apply { clear(); addAll(listOf(5, 0)) }
        whenever(p.device.createBond()).thenAnswer {
            p.bond = BluetoothDevice.BOND_BONDED
            true
        }
        p.open()
        verify(p.device).createBond()
        assertEquals(2, p.cccdValues.size)
        assertEquals(1, p.h.bridge.write(1, byteArrayOf(1)))
    }

    @Test
    fun failedPairingFailsTheOpen() {
        grantBle()
        val p = Peer(Harness())
        p.cccdStatuses.apply { clear(); add(137) }
        whenever(p.device.createBond()).thenAnswer {
            p.bond = BluetoothDevice.BOND_BONDING
            thread {
                Thread.sleep(300)
                p.bond = BluetoothDevice.BOND_NONE
            }
            true
        }
        val m = msg { p.open() }
        assertTrue(m, m.startsWith("$MAC requires pairing (GATT status 137) and pairing did not complete"))
        assertEquals(1, p.cccdValues.size)
        verify(p.gatt).close()
    }

    @Test
    fun bondedPeerStillRefusingCccdFails() {
        grantBle()
        val p = Peer(Harness())
        p.bond = BluetoothDevice.BOND_BONDED // also waits min(1.6 s, budget) before discovery
        p.cccdStatuses.apply { clear(); add(5) }
        var m = ""
        val ms = elapsedMs { m = msg { p.open(budgetMs = 300) } }
        assertEquals("enable notifications failed: GATT status 5", m)
        assertTrue("bonded: waits before discovery ($ms ms)", ms >= 300)
        verify(p.device, never()).createBond()
        assertEquals(2, p.cccdValues.size)
    }

    @Test
    fun discoveryWaitsForPairingInProgress() {
        grantBle()
        val p = Peer(Harness())
        p.bond = BluetoothDevice.BOND_BONDING
        val bondAtDiscovery = AtomicInteger(-1)
        p.onDiscover = { bondAtDiscovery.set(p.bond) }
        thread {
            Thread.sleep(250)
            p.bond = BluetoothDevice.BOND_BONDED
        }
        p.open(budgetMs = 300)
        assertEquals(BluetoothDevice.BOND_BONDED, bondAtDiscovery.get())
    }

    // --- API 33+ ----------------------------------------------------------------------------

    @Test
    @Config(sdk = [34])
    fun api33WritesAndNotificationsUseTheValueApis() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()
        assertArrayEquals(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE, p.cccdValues.single())
        val data = ByteArray(300) { it.toByte() }
        assertEquals(300, h.bridge.write(1, data))
        assertEquals(listOf(244, 56), p.writes.map { it.first.size })
        assertArrayEquals(data, joined(p.writes))

        p.cb!!.onCharacteristicChanged(p.gatt, p.tx, byteArrayOf(4, 5))
        p.cb!!.onCharacteristicChanged(p.gatt, p.rx, byteArrayOf(6))
        @Suppress("DEPRECATION")
        p.tx.value = byteArrayOf(7)
        @Suppress("DEPRECATION")
        p.cb!!.onCharacteristicChanged(p.gatt, p.tx) // the 3-arg variant fires too on 33+
        assertEquals(1, h.sink.data.size)
        assertArrayEquals(byteArrayOf(4, 5), h.sink.data[0].second)
    }

    @Test
    @Config(sdk = [34])
    fun serviceChangedDuringOpenIsIgnored() {
        grantBle()
        val p = Peer(Harness())
        p.onDiscover = { p.cb!!.onServiceChanged(p.gatt) }
        p.open()
        verify(p.gatt, never()).disconnect()
        assertEquals(1, p.h.bridge.write(1, byteArrayOf(1)))
    }

    @Test
    @Config(sdk = [34])
    fun serviceChangedAfterOpenDropsThePort() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open(handle = 5)
        p.cb!!.onServiceChanged(p.gatt)
        verify(p.gatt).disconnect()
        verify(p.gatt).close()
        assertEquals(listOf(5L to "GATT services changed; open the port again"), h.sink.disconnects.toList())
        assertTrue(msg { h.bridge.write(5, byteArrayOf(1)) }.contains("disconnected"))
    }

    // --- Bluetooth off / shutdown -------------------------------------------------------------

    @Test
    fun bluetoothOffClosesAnOpenPortOnce() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open(handle = 3)
        bluetoothState(h.ctx, BluetoothAdapter.STATE_ON)
        assertTrue(h.sink.disconnects.isEmpty())
        verify(p.gatt, never()).close()

        bluetoothState(h.ctx, BluetoothAdapter.STATE_OFF)
        verify(p.gatt).close()
        assertEquals(listOf(3L to "Bluetooth turned off"), h.sink.disconnects.toList())
        assertTrue(msg { h.bridge.write(3, byteArrayOf(1)) }.contains("disconnected"))
    }

    @Test
    fun bluetoothOffFailsAnOpenInProgressWithoutReportingIt() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.answerDiscovery = false
        val discovering = CountDownLatch(1)
        p.onDiscover = { discovering.countDown() }
        val error = AtomicReference<Throwable>()
        val t = thread {
            try {
                p.open(budgetMs = 20_000)
            } catch (e: Throwable) {
                error.set(e)
            }
        }
        assertTrue(discovering.await(5, TimeUnit.SECONDS))
        val ms = elapsedMs {
            bluetoothState(h.ctx, BluetoothAdapter.STATE_TURNING_OFF)
            t.join(5_000)
        }
        assertTrue("failed by the broadcast, not by the 10 s step timeout ($ms ms)", ms < 5_000)
        assertEquals("Bluetooth turned off", error.get()?.message)
        assertTrue(h.sink.disconnects.isEmpty())
        verify(p.gatt).disconnect()
        verify(p.gatt).close()
    }

    @Test
    fun shutdownClosesPortsAndStopsWatchingBluetooth() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open(handle = 2)
        h.bridge.shutdown()
        verify(p.gatt).disconnect()
        verify(p.gatt).close()
        assertTrue(msg { h.bridge.write(2, byteArrayOf(1)) }.contains("disconnected"))

        h.bridge.adoptReadyForTest(9)
        bluetoothState(h.ctx, BluetoothAdapter.STATE_OFF)
        assertTrue("receiver unregistered", h.sink.disconnects.isEmpty())
        h.bridge.shutdown() // twice: no error
    }

    @Test
    fun adapterMissingOrOffIsAClearError() {
        grantBle()
        assertEquals("this device has no Bluetooth adapter", msg { Harness(hasAdapter = false).bridge.connect(MAC, 1, 1_000) })
        val off = Harness()
        whenever(off.adapter.isEnabled).thenReturn(false)
        assertEquals("Bluetooth is off", msg { off.bridge.connect(MAC, 1, 1_000) })
    }

    // --- scan ---------------------------------------------------------------------------------

    @Test
    fun scanListsNusDevicesAndStopsTheScan() {
        grantBle()
        val h = Harness()
        val a = Peer(h, mac = MAC, name = "Nus_A", bonded = false)
        val b = Peer(h, mac = MAC2, bonded = false)
        val c = Peer(h, mac = "22:22:22:22:22:22", bonded = false)
        whenever(c.device.name).thenThrow(SecurityException("no BLUETOOTH_CONNECT"))
        h.onScan = { cb ->
            cb.onScanResult(ScanSettings.CALLBACK_TYPE_ALL_MATCHES, scanResult(a.device, -40))
            cb.onBatchScanResults(mutableListOf(scanResult(b.device, -70, recordName = "FromRecord"), scanResult(c.device, -90)))
        }
        val json = JSONObject(h.bridge.scanJson(50))
        assertEquals(3, json.length())
        assertEquals("Nus_A", json.getJSONObject("ble://$MAC").getString("product"))
        assertEquals("-40", json.getJSONObject("ble://$MAC").getString("rssi"))
        assertEquals("FromRecord", json.getJSONObject("ble://$MAC2").getString("product"))
        assertEquals("Unknown", json.getJSONObject("ble://22:22:22:22:22:22").getString("product"))
        assertEquals(ParcelUuid(BleLogic.NUS_SERVICE), h.scanFilters.single().single().serviceUuid)
        verify(h.scanner).stopScan(h.scanCallbacks.single())
    }

    @Test
    fun scanFailureIsReported() {
        grantBle()
        val h = Harness()
        h.onScan = { it.onScanFailed(ScanCallback.SCAN_FAILED_APPLICATION_REGISTRATION_FAILED) }
        assertEquals("BLE scan failed (ScanCallback error 2)", msg { h.bridge.scanJson(5_000) })
        verify(h.scanner).stopScan(h.scanCallbacks.single())
    }

    @Test
    fun scanWithoutScannerFails() {
        grantBle()
        val h = Harness()
        whenever(h.adapter.bluetoothLeScanner).thenReturn(null)
        assertEquals("Bluetooth LE scanner unavailable (Bluetooth off?)", msg { h.bridge.scanJson(10) })
    }

    @Test
    fun scanNeedsLocationServicesOnApi28() {
        grantBle()
        locationEnabled(false)
        val m = msg { Harness().bridge.scanJson(10) }
        assertTrue(m, m.startsWith("Location services are off"))
    }

    @Test
    @Config(sdk = [31])
    fun neverForLocationScanDoesNotNeedLocationServices() {
        grantBle()
        locationEnabled(false)
        assertEquals("{}", Harness().bridge.scanJson(10))
    }

    @Test
    @Config(sdk = [31])
    fun scanDerivingLocationNeedsLocationServices() {
        declare(arrayOf(SCAN, CONNECT, FINE))
        shadowOf(app).grantPermissions(SCAN, CONNECT, FINE)
        locationEnabled(false)
        val m = msg { Harness().bridge.scanJson(10) }
        assertTrue(m, m.startsWith("Location services are off"))
    }

    @Test
    fun sixthScanWithinThirtySecondsIsThrottled() {
        grantBle()
        val h = Harness()
        repeat(5) { assertEquals("{}", h.bridge.scanJson(1)) }
        assertEquals(
            "BLE scan throttled: Android allows 5 scan starts per 30 s per app; retry in 30 s",
            msg { h.bridge.scanJson(1) },
        )
        assertEquals(5, h.scanCallbacks.size)
    }

    // --- resolving the target -----------------------------------------------------------------

    @Test
    fun lowercaseMacResolvesABondedDeviceWithoutScanning() {
        grantBle()
        val p = Peer(Harness())
        p.open(target = MAC.lowercase())
        assertTrue(p.h.scanFilters.isEmpty())
        assertEquals(1, p.connects.get())
    }

    @Test
    fun deviceSeenInAScanIsReusedForItsMac() {
        grantBle()
        val h = Harness()
        val p = Peer(h, bonded = false)
        h.onScan = { it.onScanResult(1, scanResult(p.device, -50)) }
        h.bridge.scanJson(10)
        p.open()
        assertEquals(1, h.scanFilters.size)
        verify(h.adapter, never()).getRemoteDevice(any<String>())
    }

    @Test
    fun unknownMacIsLookedUpByAShortAddressScan() {
        grantBle()
        val h = Harness()
        val p = Peer(h, bonded = false)
        h.onScan = { it.onScanResult(1, scanResult(p.device, -50)) }
        val ms = elapsedMs { p.open(budgetMs = 20_000) }
        assertTrue("stops at the first match, not after 5 s ($ms ms)", ms < 4_000)
        assertEquals(MAC, h.scanFilters.single().single().deviceAddress)
        verify(h.adapter, never()).getRemoteDevice(any<String>())
    }

    @Test
    fun macNotFoundByTheScanConnectsAsAPublicAddress() {
        grantBle()
        val h = Harness()
        val p = Peer(h, bonded = false)
        whenever(h.adapter.getRemoteDevice(MAC)).thenReturn(p.device)
        p.open(budgetMs = 300) // the address scan gets min(5 s, budget)
        verify(h.adapter).getRemoteDevice(MAC)
        assertEquals(1, p.connects.get())
    }

    @Test
    fun failedAddressScanStillConnectsAsAPublicAddress() {
        grantBle()
        val h = Harness()
        val p = Peer(h, bonded = false)
        whenever(h.adapter.bluetoothLeScanner).thenReturn(null)
        whenever(h.adapter.getRemoteDevice(MAC)).thenReturn(p.device)
        p.open()
        verify(h.adapter).getRemoteDevice(MAC)
    }

    @Test
    fun nameIsResolvedByAScanOnceThenFromCache() {
        grantBle()
        val h = Harness()
        val other = Peer(h, mac = MAC2, name = "Other", bonded = false)
        val p = Peer(h, name = "Nordic_UART", bonded = false)
        h.onScan = { cb ->
            cb.onScanResult(1, scanResult(other.device, -80))
            cb.onScanResult(1, scanResult(p.device, -50))
        }
        val ms = elapsedMs { p.open(handle = 1, budgetMs = 20_000, target = "Nordic_UART") }
        assertTrue("stops at the name match, not after 10 s ($ms ms)", ms < 5_000)
        assertTrue("unfiltered scan", h.scanFilters.single().isEmpty())
        assertEquals(0, other.connects.get())
        h.bridge.close(1)
        p.open(handle = 2, target = "Nordic_UART")
        assertEquals("second open uses the cache", 1, h.scanFilters.size)
        assertEquals(2, p.connects.get())
    }

    @Test
    fun unknownNameIsAClearError() {
        grantBle()
        val h = Harness()
        assertEquals("no BLE device named 'Ghost' found", msg { h.bridge.connect("Ghost", 1, 200) })
        assertTrue(h.scanFilters.single().isEmpty())
    }

    // --- runtime permissions ------------------------------------------------------------------

    @Test
    fun missingPermissionWithoutActivity() {
        declare(arrayOf(FINE))
        assertEquals(
            "Bluetooth permission missing ($FINE) and no activity to ask",
            msg { Harness().bridge.scanJson(10) },
        )
    }

    @Test
    fun backgroundAppIsNotAsked() {
        declare(arrayOf(FINE))
        foreground(ActivityManager.RunningAppProcessInfo.IMPORTANCE_CACHED)
        val act = mock<Activity>()
        val m = msg { Harness(act).bridge.scanJson(10) }
        assertTrue(m, m.startsWith("Bluetooth permission missing ($FINE) and the app is in the background"))
        verify(act, never()).runOnUiThread(any())
    }

    @Test
    fun permissionGrantedInTheDialogLetsTheScanRun() {
        declare(arrayOf(FINE))
        locationEnabled(true)
        foreground(ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND)
        val act = mock<Activity>()
        lateinit var h: Harness
        doAnswer { inv ->
            inv.getArgument<Runnable>(0).run()
            h.bridge.onActivityPaused()
            shadowOf(app).grantPermissions(FINE)
            h.bridge.onActivityResumed()
            null
        }.whenever(act).runOnUiThread(any())
        h = Harness(act)
        assertEquals("{}", h.bridge.scanJson(10))
        verify(act).requestPermissions(arrayOf(FINE), 0x5E1B)
    }

    private fun deniedAfter(waitMs: Long, dialog: (BleGattBridge) -> Unit): Long {
        declare(arrayOf(FINE))
        foreground(ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND)
        val act = mock<Activity>()
        lateinit var h: Harness
        doAnswer {
            dialog(h.bridge)
            null
        }.whenever(act).runOnUiThread(any())
        h = Harness(act)
        h.bridge.permissionWaitMs = waitMs
        var m = ""
        val ms = elapsedMs { m = msg { h.bridge.scanJson(10) } }
        assertTrue(m, m.startsWith("Bluetooth permission denied ($FINE)"))
        return ms
    }

    @Test
    fun noAnswerWithinTheWaitIsDenied() {
        val ms = deniedAfter(200) {}
        assertTrue("$ms ms", ms >= 200)
    }

    @Test
    fun dialogClosedWithoutGrantEndsTheWaitEarly() {
        val ms = deniedAfter(2_000) {
            it.onActivityPaused()
            it.onActivityResumed()
        }
        assertTrue("$ms ms", ms < 1_500)
    }

    @Test
    fun resumeWithoutPauseDoesNotEndTheWait() {
        val ms = deniedAfter(600) { it.onActivityResumed() }
        assertTrue("$ms ms", ms >= 600)
    }

    @Test
    fun noDialogWithinThreeSecondsMeansPermanentlyDenied() {
        val ms = deniedAfter(60_000) {}
        assertTrue("$ms ms", ms in 3_000 until 8_000)
    }

    @Test
    @Config(sdk = [31])
    fun scanWithoutNeverForLocationAlsoNeedsLocation() {
        declare(arrayOf(SCAN, CONNECT, FINE))
        assertEquals(
            "Bluetooth permission missing ($SCAN, $CONNECT, $FINE) and no activity to ask",
            msg { Harness().bridge.scanJson(10) },
        )
    }

    @Test
    @Config(sdk = [31])
    fun neverForLocationScanNeedsNoLocation() {
        declare(arrayOf(SCAN, CONNECT, FINE), intArrayOf(PackageInfo.REQUESTED_PERMISSION_NEVER_FOR_LOCATION, 0, 0))
        assertEquals(
            "Bluetooth permission missing ($SCAN, $CONNECT) and no activity to ask",
            msg { Harness().bridge.scanJson(10) },
        )
    }

    @Test
    @Config(sdk = [31])
    fun unknownFlagsOrUndeclaredListMeanNoLocation() {
        declare(arrayOf(SCAN, CONNECT, FINE), flags = null)
        assertEquals(
            "Bluetooth permission missing ($SCAN, $CONNECT) and no activity to ask",
            msg { Harness().bridge.scanJson(10) },
        )
        declare(null)
        assertEquals(
            "Bluetooth permission missing ($SCAN, $CONNECT) and no activity to ask",
            msg { Harness().bridge.scanJson(10) },
        )
    }

    @Test
    @Config(sdk = [31])
    fun undeclaredScanPermissionIsNamed() {
        declare(arrayOf(CONNECT))
        val m = msg { Harness().bridge.scanJson(10) }
        assertTrue(m, m.startsWith("Bluetooth permission $SCAN is not declared"))
        assertFalse(m, m.contains(CONNECT))
    }

    // --- round 2: pending ops, pairing edges, budgets -----------------------------------------

    @Test
    fun connectChecksPermissionsFirst() {
        declare(arrayOf(FINE))
        val p = Peer(Harness())
        assertEquals("Bluetooth permission missing ($FINE) and no activity to ask", msg { p.open() })
        assertEquals(0, p.connects.get())
    }

    @Test
    fun cachedTableWithoutTxIsRefreshedToo() {
        grantBle()
        val p = Peer(Harness())
        val partial = BluetoothGattService(BleLogic.NUS_SERVICE, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        partial.addCharacteristic(BluetoothGattCharacteristic(BleLogic.NUS_RX, BluetoothGattCharacteristic.PROPERTY_WRITE, 0))
        p.services.apply { clear(); addAll(listOf(partial, p.service)) }
        whenever(BluetoothGatt::class.java.getMethod("refresh").invoke(p.gatt)).thenReturn(true)
        p.open()
        assertEquals(2, p.discoveries.get())
    }

    @Test
    @Config(sdk = [34])
    fun serviceChangedFailsThePendingWrite() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()
        p.writeStatus = { null }
        p.writeStarts = { p.cb!!.onServiceChanged(p.gatt); true }
        var m = ""
        val ms = elapsedMs { m = msg { h.bridge.write(1, byteArrayOf(1)) } }
        assertEquals("GATT services changed", m)
        assertTrue("not the 5 s write timeout ($ms ms)", ms < 2_000)
    }

    @Test
    fun closeFailsThePendingWrite() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()
        p.writeStatus = { null }
        p.writeStarts = { h.bridge.close(1); true }
        var m = ""
        val ms = elapsedMs { m = msg { h.bridge.write(1, byteArrayOf(1)) } }
        assertEquals("closed", m)
        assertTrue("not the 5 s write timeout ($ms ms)", ms < 2_000)
    }

    @Test
    fun busyCccdWriteFailsAsDisconnectedWhenTheLinkDrops() {
        grantBle()
        val p = Peer(Harness())
        p.cccdStarts = { n ->
            if (n == 3) p.cb!!.onConnectionStateChange(p.gatt, 8, BluetoothProfile.STATE_DISCONNECTED)
            false
        }
        assertEquals("BLE device disconnected", msg { p.open() })
        assertEquals(3, p.cccdValues.size)
        assertTrue(p.h.sink.disconnects.isEmpty())
    }

    @Test
    fun writeStillBusyAfterTheRetryBudgetFails() {
        grantBle()
        val h = Harness()
        val p = Peer(h)
        p.open()
        p.writeStarts = { false }
        thread {
            Thread.sleep(200)
            ShadowSystemClock.advanceBy(java.time.Duration.ofSeconds(60))
        }
        assertEquals("write could not start (code -2: GATT stayed busy)", msg { h.bridge.write(1, byteArrayOf(1)) })
        assertTrue(p.writes.size > 1)
    }

    private fun pairingPeer(): Peer {
        grantBle()
        val p = Peer(Harness())
        p.cccdStatuses.apply { clear(); addAll(listOf(5, 0)) }
        whenever(p.device.createBond()).thenReturn(true) // pairing starts later (system prompt)
        return p
    }

    @Test
    fun pairingThatStartsLateIsWaitedFor() {
        val p = pairingPeer()
        thread {
            Thread.sleep(150)
            p.bond = BluetoothDevice.BOND_BONDING
            Thread.sleep(250)
            p.bond = BluetoothDevice.BOND_BONDED
        }
        p.open()
        assertEquals(2, p.cccdValues.size)
    }

    @Test
    fun pairingThatStartsLateAndFailsFailsTheOpen() {
        val p = pairingPeer()
        thread {
            Thread.sleep(150)
            p.bond = BluetoothDevice.BOND_BONDING
            Thread.sleep(250)
            p.bond = BluetoothDevice.BOND_NONE
        }
        assertTrue(msg { p.open() }.contains("requires pairing (GATT status 5)"))
        assertEquals(1, p.cccdValues.size)
    }

    @Test
    fun pairingThatNeverStartsEndsWithTheBudget() {
        val p = pairingPeer()
        thread {
            Thread.sleep(300)
            ShadowSystemClock.advanceBy(java.time.Duration.ofSeconds(60))
        }
        assertTrue(msg { p.open(budgetMs = 10_000) }.contains("requires pairing (GATT status 5)"))
        assertEquals(1, p.cccdValues.size)
    }

    @Test
    fun closeDuringThePairingWaitEndsIt() {
        val p = pairingPeer()
        thread {
            Thread.sleep(300)
            p.h.bridge.close(1)
        }
        assertTrue(msg { p.open() }.contains("requires pairing (GATT status 5)"))
    }

    @Test
    fun scanKeepsResultsUntilItsTimeout() {
        grantBle()
        val h = Harness()
        val a = Peer(h, mac = MAC, bonded = false)
        val b = Peer(h, mac = MAC2, bonded = false)
        h.onScan = { cb ->
            cb.onScanResult(1, scanResult(a.device, -40))
            thread {
                Thread.sleep(50)
                cb.onScanResult(1, scanResult(b.device, -60))
            }
        }
        assertEquals(setOf("ble://$MAC", "ble://$MAC2"), JSONObject(h.bridge.scanJson(400)).keys().asSequence().toSet())
    }

    @Test
    fun permissionWaitDefaultsToAMinute() {
        assertEquals(60_000L, Harness().bridge.permissionWaitMs)
    }
}
