package app.tauri.serialplugin.manager

import com.hoho.android.usbserial.driver.UsbSerialPort

internal fun setModemLines(port: UsbSerialPort, dtr: Boolean, rts: Boolean) {
    try {
        port.dtr = dtr
    } catch (_: Exception) {
    }
    try {
        port.rts = rts
    } catch (_: Exception) {
    }
}
