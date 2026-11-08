package app.tauri.serialplugin.manager

import android.bluetooth.BluetoothAdapter
import android.content.Context
import android.content.Intent
import android.os.Looper
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.util.Collections
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class BleGattBridgeTest {

    @Test
    fun chunksFollowNegotiatedMtu() {
        assertEquals(20, BleLogic.chunkSize(23))
        assertEquals(20, BleLogic.chunkSize(0)) // never below the BLE default
        assertEquals(244, BleLogic.chunkSize(247))
        // Attribute values are at most 512 bytes; Android 14+ always negotiates 517.
        assert(BleLogic.chunkSize(517) == 512)

        val data = ByteArray(50) { it.toByte() }
        val parts = BleLogic.chunks(data, 23)
        assertEquals(listOf(20, 20, 10), parts.map { it.size })
        assertArrayEquals(data, parts.fold(ByteArray(0)) { acc, p -> acc + p })
        assertEquals(1, BleLogic.chunks(data, 247).size)
        assertTrue(BleLogic.chunks(ByteArray(0), 23).isEmpty())
    }

    @Test
    fun writeQueueKeepsEachCallersChunksTogetherAndInOrder() {
        val q = BleWriteQueue()
        val sent = Collections.synchronizedList(mutableListOf<ByteArray>())
        val a = ByteArray(100) { 'a'.code.toByte() }
        val b = ByteArray(100) { 'b'.code.toByte() }
        val go = CountDownLatch(1)
        val threads = listOf(a, b).map { data ->
            Thread {
                go.await()
                assertEquals(100, q.write(data, 23) { chunk ->
                    sent.add(chunk)
                    Thread.sleep(2) // widen the race window
                })
            }.apply { start() }
        }
        go.countDown()
        threads.forEach { it.join() }

        assertEquals(10, sent.size)
        val out = ByteArrayOutputStream().apply { sent.forEach { write(it) } }.toString(Charsets.US_ASCII.name())
        assertTrue("interleaved writes: $out", out == "a".repeat(100) + "b".repeat(100) || out == "b".repeat(100) + "a".repeat(100))
    }

    @Test
    fun permissionsDependOnApiLevel() {
        val scanConnect = listOf("android.permission.BLUETOOTH_SCAN", "android.permission.BLUETOOTH_CONNECT")
        assertEquals(scanConnect, BleLogic.requiredPermissions(31))
        assertEquals(scanConnect, BleLogic.requiredPermissions(35))
        assertEquals(listOf("android.permission.ACCESS_FINE_LOCATION"), BleLogic.requiredPermissions(30))
        assertEquals(listOf("android.permission.ACCESS_FINE_LOCATION"), BleLogic.requiredPermissions(24))

        assertEquals(
            listOf("android.permission.BLUETOOTH_CONNECT"),
            BleLogic.missingPermissions(33) { it == "android.permission.BLUETOOTH_SCAN" },
        )
        assertTrue(BleLogic.missingPermissions(29) { true }.isEmpty())
    }

    @Test
    fun scanJsonMatchesDesktopShape() {
        val json = JSONObject(
            BleLogic.scanJson(
                listOf(
                    BleLogic.ScanEntry("AA:BB:CC:DD:EE:FF", "Nordic_UART", -61),
                    BleLogic.ScanEntry("11:22:33:44:55:66", null, -90),
                ),
            ),
        )
        assertEquals(2, json.length())
        val a = json.getJSONObject("ble://AA:BB:CC:DD:EE:FF")
        assertEquals("BLE", a.getString("type"))
        assertEquals("Nordic_UART", a.getString("product"))
        assertEquals("AA:BB:CC:DD:EE:FF", a.getString("serial_number"))
        assertEquals("-61", a.getString("rssi"))
        for (k in listOf("manufacturer", "vid", "pid")) assertEquals("Unknown", a.getString(k))
        assertEquals("Unknown", json.getJSONObject("ble://11:22:33:44:55:66").getString("product"))
        assertEquals("{}", BleLogic.scanJson(emptyList()))
    }

    @Test
    fun deniedPermissionIsAClearErrorWithoutActivity() {
        // Robolectric grants nothing and the library manifest declares no BLE permission
        // (build.rs adds them to the app): a clear "not declared" error, no hang.
        val ctx = ApplicationProvider.getApplicationContext<Context>()
        val bridge = BleGattBridge(ctx, null, object : BleGattBridge.NativeSink {
            override fun onBleData(handle: Long, data: ByteArray) {}
            override fun onBleDisconnected(handle: Long, reason: String) {}
        })
        val e = assertThrows(IOException::class.java) { bridge.scanJson(500) }
        assertTrue(e.message!!, e.message!!.contains("ACCESS_FINE_LOCATION"))
        assertTrue(e.message!!, e.message!!.contains("not declared"))
        assertThrows(IOException::class.java) { bridge.write(1, byteArrayOf(1)) }
        bridge.close(1) // unknown handle: no-op
        bridge.shutdown()
    }

    private class RecordingSink : BleGattBridge.NativeSink {
        val disconnects = Collections.synchronizedList(mutableListOf<Pair<Long, String>>())
        override fun onBleData(handle: Long, data: ByteArray) {}
        override fun onBleDisconnected(handle: Long, reason: String) {
            disconnects.add(handle to reason)
        }
    }

    @Test
    fun bluetoothTurnedOffDisconnectsOpenPortsOnce() {
        val ctx = ApplicationProvider.getApplicationContext<Context>()
        val sink = RecordingSink()
        val bridge = BleGattBridge(ctx, null, sink)
        bridge.adoptReadyForTest(7)
        val off = Intent(BluetoothAdapter.ACTION_STATE_CHANGED)
            .putExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.STATE_TURNING_OFF)
        ctx.sendBroadcast(off)
        ctx.sendBroadcast(Intent(off).putExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.STATE_OFF))
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(listOf(7L to "Bluetooth turned off"), sink.disconnects.toList())
        assertThrows(IOException::class.java) { bridge.write(7, byteArrayOf(1)) }
        bridge.shutdown()
        ctx.sendBroadcast(off) // unregistered: nothing more
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(1, sink.disconnects.size)
    }

    // --- GATT sequencing (M1): one pending op, completed only by its own callback ---------

    @Test
    fun unsolicitedMtuDuringDiscoveryDoesNotCompleteIt() {
        val slot = GattOpSlot()
        val discovery = slot.begin(GattEvent.SERVICES_DISCOVERED)
        assertFalse("peripheral-initiated MTU", slot.deliver(GattEvent.MTU_CHANGED, null, 0))
        assertFalse(discovery.result.isDone)
        assertTrue(slot.deliver(GattEvent.SERVICES_DISCOVERED, null, 0))
        assertEquals(0, discovery.result.get(0, TimeUnit.MILLISECONDS))
        assertFalse("second (Service Changed) discovery is a no-op", slot.deliver(GattEvent.SERVICES_DISCOVERED, null, 0))
        slot.end(discovery)
        assertFalse("nothing pending", slot.deliver(GattEvent.SERVICES_DISCOVERED, null, 0))
    }

    @Test
    fun lateCallbacksOfTimedOutStepsAreIgnored() {
        val slot = GattOpSlot()
        val mtu = slot.begin(GattEvent.MTU_CHANGED)
        slot.end(mtu) // timed out
        val cccd = slot.begin(GattEvent.DESCRIPTOR_WRITE, BleLogic.CCCD)
        assertFalse("late MTU", slot.deliver(GattEvent.MTU_CHANGED, null, 0))
        assertFalse("other descriptor", slot.deliver(GattEvent.DESCRIPTOR_WRITE, UUID.randomUUID(), 0))
        assertFalse(cccd.result.isDone)
        assertTrue(slot.deliver(GattEvent.DESCRIPTOR_WRITE, BleLogic.CCCD, 0))
        slot.end(cccd)
        val write = slot.begin(GattEvent.CHARACTERISTIC_WRITE, BleLogic.NUS_RX)
        assertFalse("late CCCD write", slot.deliver(GattEvent.DESCRIPTOR_WRITE, BleLogic.CCCD, 0))
        assertFalse("write to another characteristic", slot.deliver(GattEvent.CHARACTERISTIC_WRITE, BleLogic.NUS_TX, 0))
        assertTrue(slot.deliver(GattEvent.CHARACTERISTIC_WRITE, BleLogic.NUS_RX, 0))
        slot.end(write)
    }

    @Test
    fun linkLossFailsWhateverIsPending() {
        val slot = GattOpSlot()
        val p = slot.begin(GattEvent.CHARACTERISTIC_WRITE, BleLogic.NUS_RX)
        slot.fail(IOException("link lost"))
        assertTrue(p.result.isCompletedExceptionally)
        val stale = slot.begin(GattEvent.CONNECTED)
        slot.end(p) // ending an older op must not clear the newer one
        assertTrue(slot.deliver(GattEvent.CONNECTED, null, 0))
        assertEquals(0, stale.result.get(0, TimeUnit.MILLISECONDS))
    }

    private class FakeClock {
        var t = 0L
        val now = { t }
        val sleep = { ms: Long -> t += ms }
    }

    @Test
    fun busyStartsRetryWhileAliveThenGiveUp() {
        val c = FakeClock()
        var tries = 0
        val started = BleLogic.startWhileBusy(2_000, { true }, {
            tries++
            if (tries < 3) BleLogic.ERROR_GATT_WRITE_REQUEST_BUSY else 0
        }, c.now, c.sleep)
        assertEquals(0, started)
        assertEquals(3, tries)

        val stuck = BleLogic.startWhileBusy(100, { true }, { BleLogic.PRE33_REFUSED }, c.now, c.sleep)
        assertEquals("budget used up: report busy", BleLogic.PRE33_REFUSED, stuck)

        val e = assertThrows(IOException::class.java) {
            BleLogic.startWhileBusy(2_000, { false }, { BleLogic.PRE33_REFUSED }, c.now, c.sleep)
        }
        assertTrue("link gone is not busy: ${e.message}", e.message!!.contains("disconnected"))

        var once = 0
        assertEquals(-1, BleLogic.startWhileBusy(2_000, { true }, { once++; -1 }, c.now, c.sleep))
        assertEquals("non-busy failure is not retried", 1, once)
    }

    @Test
    fun deadlineCapsEachStepAndFailsWhenSpent() {
        val c = FakeClock()
        val d = Deadline.after(15_000, c.now)
        assertEquals(10_000L, d.cap(10_000, "discovery"))
        c.t = 12_000
        assertEquals(3_000L, d.cap(10_000, "discovery"))
        assertEquals(3_000L, d.cap(Long.MAX_VALUE, "connect"))
        c.t = 15_000
        assertThrows(IOException::class.java) { d.cap(5_000, "enable notifications") }
    }

    @Test
    fun scanThrottleAllowsFivePerThirtySeconds() {
        val t = ScanThrottle()
        repeat(5) { assertEquals(0L, t.acquire(1_000L + it)) }
        assertEquals(21_000L, t.acquire(10_000))
        assertEquals("first start ages out", 0L, t.acquire(31_000))
        assertTrue("still 5 in the window", t.acquire(31_000) > 0)
    }

    @Test
    fun partialWriteReportsBytesSentAndFailsOnlyWhenNothingWent() {
        val q = BleWriteQueue()
        val data = ByteArray(50)
        var calls = 0
        val n = q.write(data, 23) {
            calls++
            if (calls == 3) throw IOException("write failed: GATT status 133")
        }
        assertEquals("two 20-byte chunks went out", 40, n)
        assertThrows(IOException::class.java) {
            q.write(data, 23) { throw IOException("BLE device disconnected") }
        }
    }

    @Test
    fun locationIsNeededWhenScanIsNotNeverForLocation() {
        assertEquals(
            listOf(
                "android.permission.BLUETOOTH_SCAN",
                "android.permission.BLUETOOTH_CONNECT",
                "android.permission.ACCESS_FINE_LOCATION",
            ),
            BleLogic.requiredPermissions(33, scanDerivesLocation = true),
        )
        assertEquals(listOf("android.permission.ACCESS_FINE_LOCATION"), BleLogic.requiredPermissions(30, true))
        assertTrue(BleLogic.needsLocationServices(30, false))
        assertFalse(BleLogic.needsLocationServices(31, false))
        assertTrue(BleLogic.needsLocationServices(34, true))
    }

    @Test
    fun everyEighthChunkIsWrittenWithResponseWhenRxAllowsBoth() {
        val both = android.bluetooth.BluetoothGattCharacteristic.PROPERTY_WRITE or
            android.bluetooth.BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE
        val noResp = android.bluetooth.BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        val withResp = android.bluetooth.BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        val types = (1..16).map { BleLogic.writeTypeFor(it, both) }
        assertEquals((1..16).map { if (it % BleLogic.WRITE_PACE_EVERY == 0) withResp else noResp }, types)
        val onlyNoResp = android.bluetooth.BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE
        assertTrue((1..16).all { BleLogic.writeTypeFor(it, onlyNoResp) == noResp })
        val onlyResp = android.bluetooth.BluetoothGattCharacteristic.PROPERTY_WRITE
        assertTrue((1..16).all { BleLogic.writeTypeFor(it, onlyResp) == withResp })
    }

    @Test
    fun cccdUsesIndicationWhenTxCannotNotify() {
        assertArrayEquals(
            android.bluetooth.BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE,
            BleLogic.cccdValue(android.bluetooth.BluetoothGattCharacteristic.PROPERTY_NOTIFY or
                android.bluetooth.BluetoothGattCharacteristic.PROPERTY_INDICATE),
        )
        assertArrayEquals(
            android.bluetooth.BluetoothGattDescriptor.ENABLE_INDICATION_VALUE,
            BleLogic.cccdValue(android.bluetooth.BluetoothGattCharacteristic.PROPERTY_INDICATE),
        )
    }

    @Test
    fun bondedDevicesWaitBeforeServiceDiscovery() {
        assertEquals(1600L, BleLogic.discoveryDelayMs(android.bluetooth.BluetoothDevice.BOND_BONDED))
        assertEquals(0L, BleLogic.discoveryDelayMs(android.bluetooth.BluetoothDevice.BOND_NONE))
    }
}
