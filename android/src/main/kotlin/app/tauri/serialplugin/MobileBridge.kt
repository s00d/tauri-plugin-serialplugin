package app.tauri.serialplugin

/**
 * JNI entry points into the Tauri Rust library (loaded by the host app).
 * RX is polled via Rust USB reader → [PortRxHub] (see [android/jni.rs]).
 */
object MobileBridge {
    @JvmStatic
    external fun onUsbError(path: String, reason: String)

    @JvmStatic
    external fun onDeviceDetached(deviceName: String)

    @JvmStatic
    external fun onPortListChange()

    @JvmStatic
    external fun onAppDestroy()

    /** Debug-only: reset Rust port registry between integration tests. */
    @JvmStatic
    external fun testHarnessReset()

    /** Debug-only: register path in Rust registry with a fresh PortRxHub. */
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

    /** Debug-only: invoke Rust TX path (driver_host write). Returns bytes written or -1. */
    @JvmStatic
    external fun testInvokeWrite(path: String, data: ByteArray): Long

    /** Debug-only: inject CDC fake device, open port, register hub. Returns session path. */
    @JvmStatic
    external fun testOpenFakePort(deviceName: String): String

    /** Debug-only: push scripted bulk-IN bytes into the fake transport. */
    @JvmStatic
    external fun testFakeInjectRx(deviceName: String, data: ByteArray): Boolean

    /** Debug-only: take bytes written to the fake bulk-OUT endpoint. */
    @JvmStatic
    external fun testFakeTakeTx(deviceName: String): ByteArray

    /** Debug-only: fail the next bulk-IN read (reader → onUsbError). */
    @JvmStatic
    external fun testFakeInjectError(deviceName: String, reason: String): Boolean
}
