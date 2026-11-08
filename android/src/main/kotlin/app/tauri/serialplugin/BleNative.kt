package app.tauri.serialplugin

import android.annotation.SuppressLint
import app.tauri.serialplugin.manager.BleGattBridge

/**
 * Static JNI facade for Rust `ble://` ports (feature `ble`). Every call blocks the
 * calling Rust thread; RX and link loss come back via [MobileBridge.onBleData] /
 * [MobileBridge.onBleDisconnected].
 */
object BleNative {
    // Not a leak: the bridge holds only the application context and a weak activity ref.
    @SuppressLint("StaticFieldLeak")
    @Volatile
    private var bridge: BleGattBridge? = null

    @JvmStatic
    fun bind(ble: BleGattBridge) {
        bridge = ble
        nativeInit()
    }

    @JvmStatic
    private external fun nativeInit()

    private fun ble(): BleGattBridge =
        bridge ?: throw IllegalStateException("BleNative not bound")

    /** `{"ble://AA:BB:..": {"type": "BLE", "product": name, "rssi": "-60", ...}}` */
    @JvmStatic
    fun scanJson(timeoutMs: Long): String = ble().scanJson(timeoutMs)

    /** Returns once notifications are on; data for [handle] may follow at once. */
    @JvmStatic
    fun connect(target: String, handle: Long, timeoutMs: Long) = ble().connect(target, handle, timeoutMs)

    @JvmStatic
    fun write(handle: Long, data: ByteArray): Int = ble().write(handle, data)

    @JvmStatic
    fun close(handle: Long) = ble().close(handle)
}
