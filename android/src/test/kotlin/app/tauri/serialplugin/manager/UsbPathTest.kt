package app.tauri.serialplugin.manager

import org.junit.Assert.assertEquals
import org.junit.Test

class UsbPathTest {
    @Test
    fun sessionKey_singlePort_usesDeviceName() {
        assertEquals("/dev/bus/usb/001/002", UsbPath.sessionKey("/dev/bus/usb/001/002", 0, 1))
    }

    @Test
    fun sessionKey_multiPort_appendsIndex() {
        assertEquals("/dev/bus/usb/001/002#1", UsbPath.sessionKey("/dev/bus/usb/001/002", 1, 2))
    }

    @Test
    fun parse_plainPath_portZero() {
        assertEquals("/dev/bus/usb/001/002" to 0, UsbPath.parse("/dev/bus/usb/001/002"))
    }

    @Test
    fun parse_withPortSuffix() {
        assertEquals("/dev/bus/usb/001/002" to 1, UsbPath.parse("/dev/bus/usb/001/002#1"))
    }

    @Test
    fun deviceName_stripsPortSuffix() {
        assertEquals("/dev/bus/usb/001/002", UsbPath.deviceName("/dev/bus/usb/001/002#0"))
    }
}
