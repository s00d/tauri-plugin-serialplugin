package app.tauri.serialplugin

/**
 * JNI entry points into the Tauri Rust library (loaded by the host app).
 * Rust implements these in [android/jni.rs] and [android/test_harness.rs] (debug).
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

    /** Debug-only: reset Rust port registry between integration tests. */
    @JvmStatic
    external fun testHarnessReset()

    /** Debug-only: register path in Rust registry with a fresh [MobileRxHub]. */
    @JvmStatic
    external fun testRegisterPort(path: String)

    /** Debug-only: idle + read-slot bytes buffered in the Rust hub for [path]. */
    @JvmStatic
    external fun testHubBufferedLen(path: String): Long

    /** Debug-only: take idle bytes from the Rust hub (exchange replay path). */
    @JvmStatic
    external fun testHubTakeIdle(path: String): ByteArray

    /** Debug-only: whether [path] is still registered after USB teardown. */
    @JvmStatic
    external fun testRegistryHasPort(path: String): Boolean

    /** Debug-only: invoke Rust TX path (usb_jni → UsbNative.write). Returns bytes written or -1. */
    @JvmStatic
    external fun testInvokeWrite(path: String, data: ByteArray): Long
}
