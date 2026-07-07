package app.tauri.serialplugin.manager

import com.hoho.android.usbserial.driver.ProbeTable
import com.hoho.android.usbserial.driver.UsbSerialProber

/** Fallback prober for VID/PID not in the default table (host can extend later). */
internal object CustomProber {
    fun getCustomProber(): UsbSerialProber {
        val table = ProbeTable()
        // Example: table.addProduct(0x1234, 0xabcd, FtdiSerialDriver::class.java)
        return UsbSerialProber(table)
    }
}
