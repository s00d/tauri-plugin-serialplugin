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
import org.robolectric.annotation.Config
import java.io.IOException
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
        whenever(iface.id).thenReturn(0)
        whenever(iface.interfaceClass).thenReturn(255)
        whenever(iface.interfaceSubclass).thenReturn(255)
        whenever(iface.interfaceProtocol).thenReturn(255)
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
        val before = sink.portListChanges.get()

        harness.bridge.shutdown()

        harness.context.sendBroadcast(Intent(UsbManager.ACTION_USB_DEVICE_ATTACHED))
        assertEquals(before, sink.portListChanges.get())
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
}
