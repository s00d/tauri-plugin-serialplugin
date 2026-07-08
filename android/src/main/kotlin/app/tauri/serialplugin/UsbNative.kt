package app.tauri.serialplugin

import app.tauri.serialplugin.manager.UsbFdBridge
import org.json.JSONObject

/** Static JNI facade: Rust calls fd + enumerate only. */
object UsbNative {
    @Volatile
    private var bridge: UsbFdBridge? = null

    @JvmStatic
    fun bind(usb: UsbFdBridge) {
        bridge = usb
        nativeInit()
    }

    @JvmStatic
    private external fun nativeInit()

    private fun usb(): UsbFdBridge =
        bridge ?: throw IllegalStateException("UsbNative not bound")

    @JvmStatic
    fun enumerateJson(): String = usb().runOnIoSync<String> { usb().enumerateJson() }

    @JvmStatic
    fun openDeviceFd(deviceName: String): Int =
        usb().runOnIoSync<Int> { usb().openDeviceFd(deviceName) }

    @JvmStatic
    fun closeDeviceFd(deviceName: String) {
        usb().runOnIoSync { usb().closeDeviceFd(deviceName) }
    }
}
