package app.tauri.serialplugin

/**
 * JNI entry points into the Tauri Rust library (loaded by the host app).
 * Rust implements these in [mobile_jni.rs].
 */
object MobileBridge {
    @JvmStatic
    external fun feedRx(path: String, data: ByteArray)

    @JvmStatic
    external fun onUsbError(path: String, reason: String)

    @JvmStatic
    external fun onPortListChange()

    @JvmStatic
    external fun onAppDestroy()
}
