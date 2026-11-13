package app.tauri.serialplugin.manager

import android.app.PendingIntent
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.ArgumentMatchers.anyInt
import org.mockito.Mockito.mockStatic
import org.mockito.kotlin.any
import org.mockito.kotlin.doAnswer
import org.mockito.kotlin.eq
import org.mockito.kotlin.mock
import org.mockito.kotlin.never
import org.mockito.kotlin.times
import org.mockito.kotlin.verify
import org.mockito.kotlin.whenever
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.io.IOException
import android.os.Looper
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicInteger

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class UsbFdBridgeTest {

    private companion object {
        const val DEVICE = "/dev/bus/usb/001/001"
        const val ACTION_USB_PERMISSION = "app.tauri.serialplugin.USB_PERMISSION"
    }

    private class RecordingSink : UsbFdBridge.NativeSink {
        val portListChanges = AtomicInteger(0)
        val detached = mutableListOf<String>()

        override fun onPortListChange() {
            portListChanges.incrementAndGet()
        }

        override fun onDeviceDetached(deviceName: String) {
            detached.add(deviceName)
        }
    }

    private data class UsbHarness(
        val bridge: UsbFdBridge,
        val usbManager: UsbManager,
        val context: Context,
        val sink: RecordingSink,
    )

    private fun usbHarness(sink: RecordingSink = RecordingSink()): UsbHarness {
        val base = ApplicationProvider.getApplicationContext<Context>()
        val usbManager = mock<UsbManager>()
        val context = object : ContextWrapper(base) {
            override fun getSystemService(name: String): Any? =
                if (Context.USB_SERVICE == name) usbManager else super.getSystemService(name)
        }
        val bridge = UsbFdBridge.forTesting(context, sink)
        return UsbHarness(bridge, usbManager, context, sink)
    }

    private fun mockDevice(name: String = DEVICE): UsbDevice {
        val device = mock<UsbDevice>()
        whenever(device.deviceName).thenReturn(name)
        whenever(device.vendorId).thenReturn(0x0403)
        whenever(device.productId).thenReturn(0x6001)
        whenever(device.interfaceCount).thenReturn(1)
        val iface = mock<android.hardware.usb.UsbInterface>()
        whenever(iface.id).thenReturn(2)
        whenever(iface.interfaceClass).thenReturn(255)
        whenever(iface.interfaceSubclass).thenReturn(66)
        whenever(iface.interfaceProtocol).thenReturn(1)
        whenever(device.getInterface(0)).thenReturn(iface)
        return device
    }

    private fun mockConn(fd: Int): UsbDeviceConnection {
        val conn = mock<UsbDeviceConnection>()
        whenever(conn.fileDescriptor).thenReturn(fd)
        return conn
    }

    private fun stubDeviceList(usbManager: UsbManager, device: UsbDevice) {
        whenever(usbManager.deviceList).thenReturn(hashMapOf("key" to device))
    }

    @Test
    fun enumerateJson_emptyWithoutUsbManager() {
        val bridge = UsbFdBridge.forTesting()
        val json = bridge.enumerateJson()
        assertTrue(json.contains("\"ports\""))
    }

    @Test
    fun enumerateJson_includesInterfaces() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        val json = harness.bridge.enumerateJson()
        assertTrue(json.contains("\"interfaces\""))
        assertTrue(json.contains("\"class\":255"))
    }

    @Test
    fun adoptConnection_roundtripFd() {
        val bridge = UsbFdBridge.forTesting()
        val conn = mockConn(99)
        bridge.adoptConnectionForTest(DEVICE, conn)
        assertEquals(99, bridge.openDeviceFd(DEVICE))
        bridge.closeDeviceFd(DEVICE)
    }

    @Test
    fun openDeviceFd_permissionGranted() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        val conn = mockConn(42)
        whenever(harness.usbManager.openDevice(device)).thenReturn(conn)
        doAnswer {
            harness.bridge.completePermissionForTest(DEVICE, true)
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())

        assertEquals(42, harness.bridge.openDeviceFd(DEVICE))
    }

    @Test
    fun openDeviceFd_permissionDenied() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        doAnswer {
            harness.bridge.completePermissionForTest(DEVICE, false)
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())

        val err = assertThrows(IOException::class.java) {
            harness.bridge.openDeviceFd(DEVICE)
        }
        assertTrue(err.message!!.contains("permission denied"))
    }

    @Test
    fun openDeviceFd_permissionTimeout() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)

        val err = assertThrows(IOException::class.java) {
            harness.bridge.openDeviceFd(DEVICE)
        }
        assertTrue(err.message!!.contains("permission denied"))
        verify(harness.usbManager, never()).openDevice(device)
    }

    @Test
    @Config(sdk = [31])
    fun requestPermission_usesMutablePendingIntentOnApi31() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        val conn = mockConn(7)
        whenever(harness.usbManager.openDevice(device)).thenReturn(conn)

        mockStatic(PendingIntent::class.java).use { mocked ->
            mocked.`when`<PendingIntent> {
                PendingIntent.getBroadcast(any(), anyInt(), any(), anyInt())
            }.thenAnswer { inv ->
                val flags = inv.arguments[3] as Int
                assertEquals(PendingIntent.FLAG_MUTABLE, flags and PendingIntent.FLAG_MUTABLE)
                mock<PendingIntent>()
            }
            doAnswer {
                harness.bridge.completePermissionForTest(DEVICE, true)
                null
            }.whenever(harness.usbManager).requestPermission(eq(device), any())

            assertEquals(7, harness.bridge.openDeviceFd(DEVICE))
        }
    }

    @Test
    fun onDeviceDetached_notifiesSinkAndClosesConnection() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        val device = mockDevice()
        val conn = mockConn(11)
        harness.bridge.adoptConnectionForTest(DEVICE, conn)

        harness.bridge.deliverBroadcastForTest(
            Intent(UsbManager.ACTION_USB_DEVICE_DETACHED).apply {
                putExtra(UsbManager.EXTRA_DEVICE, device)
            },
        )

        assertEquals(listOf(DEVICE), sink.detached)
        assertTrue(sink.portListChanges.get() >= 1)
        verify(conn).close()
    }

    @Test
    fun onDeviceAttached_notifiesPortListChange() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        val before = sink.portListChanges.get()

        harness.bridge.deliverBroadcastForTest(Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED))

        assertEquals(before + 1, sink.portListChanges.get())
        assertTrue(sink.detached.isEmpty())
    }

    @Test
    fun shutdown_unregistersReceiver() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        broadcast(harness.context, Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED))
        assertEquals("registered: the system broadcast arrives", 1, sink.portListChanges.get())
        val conn = mockConn(3)
        harness.bridge.adoptConnectionForTest(DEVICE, conn)

        harness.bridge.shutdown()

        verify(conn).close()
        broadcast(harness.context, Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED))
        assertEquals(1, sink.portListChanges.get())
        assertFalse(shadowOf(ApplicationProvider.getApplicationContext<android.app.Application>())
            .hasReceiverForIntent(Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED)))
        assertTrue(assertThrows(IOException::class.java) { harness.bridge.openDeviceFd(DEVICE) }.message!!.contains("shut down"))
        assertThrows(IOException::class.java) { harness.bridge.runOnIoSync {} }
    }

    private fun broadcast(ctx: Context, intent: Intent) {
        ctx.sendBroadcast(intent)
        shadowOf(Looper.getMainLooper()).idle()
    }

    private fun ports(json: String): JSONObject = JSONObject(json).getJSONObject("ports")

    @Test
    fun enumerateJson_reportsIdsNamesAndInterfaces() {
        val harness = usbHarness()
        val device = mockDevice()
        whenever(device.manufacturerName).thenReturn("FTDI")
        whenever(device.productName).thenReturn("FT232R")
        whenever(device.serialNumber).thenReturn("A10K")
        stubDeviceList(harness.usbManager, device)
        val port = ports(harness.bridge.enumerateJson()).getJSONObject(DEVICE)
        assertEquals("Usb", port.getString("type"))
        assertEquals("0x0403", port.getString("vid"))
        assertEquals("0x6001", port.getString("pid"))
        assertEquals("FTDI", port.getString("manufacturer"))
        assertEquals("FT232R", port.getString("product"))
        assertEquals("A10K", port.getString("serial_number"))
        val iface = port.getJSONArray("interfaces").getJSONObject(0)
        assertEquals(2, iface.getInt("id"))
        assertEquals(255, iface.getInt("class"))
        assertEquals(66, iface.getInt("subclass"))
        assertEquals(1, iface.getInt("protocol"))
    }

    @Test
    fun enumerateJson_missingOrDeniedNamesAreEmpty() {
        val harness = usbHarness()
        val device = mockDevice()
        whenever(device.manufacturerName).thenReturn(null)
        whenever(device.productName).thenThrow(SecurityException("no permission"))
        whenever(device.serialNumber).thenThrow(SecurityException("no permission"))
        stubDeviceList(harness.usbManager, device)
        val port = ports(harness.bridge.enumerateJson()).getJSONObject(DEVICE)
        assertEquals("", port.getString("manufacturer"))
        assertEquals("", port.getString("product"))
        assertEquals("", port.getString("serial_number"))
    }

    @Test
    fun enumerateJson_skipsADeviceThatFails() {
        val harness = usbHarness()
        val good = mockDevice()
        val bad = mockDevice("/dev/bus/usb/001/002")
        whenever(bad.getInterface(0)).thenThrow(IllegalStateException("gone"))
        whenever(harness.usbManager.deviceList).thenReturn(hashMapOf("a" to good, "b" to bad))
        val ports = ports(harness.bridge.enumerateJson())
        assertEquals(listOf(DEVICE), ports.keys().asSequence().toList())
    }

    @Test
    fun systemBroadcasts_reachTheReceiver() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        val device = mockDevice()
        val conn = mockConn(12)
        harness.bridge.adoptConnectionForTest(DEVICE, conn)
        broadcast(harness.context, Intent(UsbManager.ACTION_USB_DEVICE_DETACHED).putExtra(UsbManager.EXTRA_DEVICE, device))
        assertEquals(listOf(DEVICE), sink.detached)
        assertEquals(1, sink.portListChanges.get())
        verify(conn).close()

        // Permission result as a real broadcast while openDeviceFd waits for it.
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        val opened = mockConn(13)
        whenever(harness.usbManager.openDevice(device)).thenReturn(opened)
        doAnswer {
            broadcast(
                harness.context,
                Intent(ACTION_USB_PERMISSION)
                    .putExtra(UsbManager.EXTRA_DEVICE, device)
                    .putExtra(UsbManager.EXTRA_PERMISSION_GRANTED, true),
            )
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())
        assertEquals(13, harness.bridge.openDeviceFd(DEVICE))
    }

    @Test
    fun permissionBroadcast_deniedOrWithoutExtraIsADenial() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        doAnswer {
            harness.bridge.deliverBroadcastForTest(Intent(ACTION_USB_PERMISSION).putExtra(UsbManager.EXTRA_DEVICE, device))
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())
        val ms = System.nanoTime()
        val err = assertThrows(IOException::class.java) { harness.bridge.openDeviceFd(DEVICE) }
        assertEquals("USB permission denied for $DEVICE", err.message)
        assertTrue("answered by the broadcast, not the timeout", (System.nanoTime() - ms) / 1_000_000 < 150)
        verify(harness.usbManager, never()).openDevice(device)
    }

    @Test
    fun detachWithoutDevice_onlyRefreshesThePortList() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        val conn = mockConn(5)
        harness.bridge.adoptConnectionForTest(DEVICE, conn)
        harness.bridge.deliverBroadcastForTest(Intent(UsbManager.ACTION_USB_DEVICE_DETACHED))
        assertTrue(sink.detached.isEmpty())
        assertEquals(1, sink.portListChanges.get())
        verify(conn, never()).close()
        harness.bridge.deliverBroadcastForTest(Intent("some.other.ACTION"))
        assertEquals(1, sink.portListChanges.get())
    }

    @Test
    @Config(sdk = [34])
    fun detach_onApi33PlusReadsTheTypedExtra() {
        val sink = RecordingSink()
        val harness = usbHarness(sink)
        harness.bridge.deliverBroadcastForTest(
            Intent(UsbManager.ACTION_USB_DEVICE_DETACHED).putExtra(UsbManager.EXTRA_DEVICE, mockDevice()),
        )
        assertEquals(listOf(DEVICE), sink.detached)
    }

    @Test
    fun openDeviceFd_errors() {
        assertEquals("no UsbManager", assertThrows(IOException::class.java) { UsbFdBridge.forTesting().openDeviceFd(DEVICE) }.message)
        val harness = usbHarness()
        whenever(harness.usbManager.deviceList).thenReturn(hashMapOf())
        assertEquals("device not found: $DEVICE", assertThrows(IOException::class.java) { harness.bridge.openDeviceFd(DEVICE) }.message)
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(true)
        assertEquals("open failed: $DEVICE", assertThrows(IOException::class.java) { harness.bridge.openDeviceFd(DEVICE) }.message)
    }

    @Test
    fun productionBridge_runsIoOnItsOwnThreadAndShutsDown() {
        val app = ApplicationProvider.getApplicationContext<android.app.Application>()
        val bridge = UsbFdBridge(app)
        val attached = Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED)
        assertTrue(shadowOf(app).hasReceiverForIntent(attached))
        assertEquals("usb-fd-io", bridge.runOnIoSync<String> { Thread.currentThread().name })
        var ran = false
        bridge.runOnIoSync { ran = true }
        assertTrue(ran)

        bridge.shutdown()
        val end = System.currentTimeMillis() + 3_000
        while (shadowOf(app).hasReceiverForIntent(attached) && System.currentTimeMillis() < end) Thread.sleep(10)
        assertFalse("unregistered on the IO thread", shadowOf(app).hasReceiverForIntent(attached))
        assertEquals("USB fd bridge shut down", assertThrows(IOException::class.java) { bridge.runOnIoSync {} }.message)
        val pool = UsbFdBridge::class.java.getDeclaredField("ioExecutor").apply { isAccessible = true }
            .get(bridge) as java.util.concurrent.ExecutorService
        assertTrue("IO thread pool stopped", pool.awaitTermination(3, java.util.concurrent.TimeUnit.SECONDS))
        bridge.shutdown() // pool already shut down: the rejected path, no error
    }

    @Test
    fun runOnIoSync_testModeRunsInline() {
        val bridge = UsbFdBridge.forTesting()
        val caller = Thread.currentThread()
        assertEquals(5, bridge.runOnIoSync<Int> { assertTrue(Thread.currentThread() === caller); 5 })
    }

    @Test
    fun openDeviceFd_reusesExistingConnection() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(true)
        val conn = mockConn(55)
        whenever(harness.usbManager.openDevice(device)).thenReturn(conn)

        assertEquals(55, harness.bridge.openDeviceFd(DEVICE))
        assertEquals(55, harness.bridge.openDeviceFd(DEVICE))
        verify(harness.usbManager, times(1)).openDevice(device)
    }

    @Test
    fun openDeviceFd_doubleGrantWhileWaiting() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        val conn = mockConn(77)
        whenever(harness.usbManager.openDevice(device)).thenReturn(conn)
        doAnswer {
            harness.bridge.completePermissionForTest(DEVICE, true)
            harness.bridge.completePermissionForTest(DEVICE, true)
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())

        assertEquals(77, harness.bridge.openDeviceFd(DEVICE))
    }

    @Test
    fun openDeviceFd_afterDeny_retrySucceeds() {
        val harness = usbHarness()
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)
        val conn = mockConn(88)
        whenever(harness.usbManager.openDevice(device)).thenReturn(conn)

        var grant = false
        doAnswer {
            harness.bridge.completePermissionForTest(DEVICE, grant)
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())

        val denied = assertThrows(IOException::class.java) {
            harness.bridge.openDeviceFd(DEVICE)
        }
        assertTrue(denied.message!!.contains("permission denied"))

        grant = true
        assertEquals(88, harness.bridge.openDeviceFd(DEVICE))
    }

    @Test
    fun openDeviceFd_shutdownDuringPermissionWait() {
        val harness = usbHarness()
        harness.bridge.setPermissionWaitMsForTest(30_000)
        val device = mockDevice()
        stubDeviceList(harness.usbManager, device)
        whenever(harness.usbManager.hasPermission(device)).thenReturn(false)

        val err = java.util.concurrent.atomic.AtomicReference<Throwable>()
        val started = java.util.concurrent.CountDownLatch(1)
        doAnswer {
            started.countDown()
            null
        }.whenever(harness.usbManager).requestPermission(eq(device), any())

        val t = Thread {
            try {
                harness.bridge.openDeviceFd(DEVICE)
            } catch (e: Throwable) {
                err.set(e)
            }
        }
        t.start()
        assertTrue(started.await(2, java.util.concurrent.TimeUnit.SECONDS))
        val t0 = System.nanoTime()
        harness.bridge.shutdown()
        t.join(3000)
        val elapsedMs = (System.nanoTime() - t0) / 1_000_000
        assertTrue("worker should finish from shutdown alone", !t.isAlive)
        assertTrue(
            "shutdown should unblock well under the 30s permission wait (took ${elapsedMs}ms)",
            elapsedMs < 2000,
        )
        assertTrue(err.get() is IOException)
        val msg = err.get()!!.message!!
        assertTrue(
            "expected shutdown or permission denial, got: $msg",
            msg.contains("permission denied") || msg.contains("shut down"),
        )
    }
}
