package app.tauri.serialplugin.manager

import app.tauri.serialplugin.MobileBridge

/** Injectable sink for RX bytes and USB lifecycle (production = JNI). */
internal interface SerialRxSink {
    fun feedRx(path: String, data: ByteArray)
    fun onUsbError(path: String, reason: String)
    fun onPortListChange()
    fun onPortClosed(path: String) {}
    fun shutdown() {}
}

internal object JniSerialRxSink : SerialRxSink {
    override fun feedRx(path: String, data: ByteArray) = MobileBridge.feedRx(path, data)
    override fun onUsbError(path: String, reason: String) = MobileBridge.onUsbError(path, reason)
    override fun onPortListChange() = MobileBridge.onPortListChange()

    override fun onPortClosed(path: String) {}

    override fun shutdown() {}
}
