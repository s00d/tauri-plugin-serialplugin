package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import app.tauri.serialplugin.UsbNative
import app.tauri.serialplugin.manager.UsbFdBridge

/** Sets up FakeTransport CDC + production fd bridge + Rust test harness for one port. */
object JniChainFixture {
    const val DEVICE_NAME: String = "/dev/bus/usb/001/099"

    lateinit var bridge: UsbFdBridge
        private set
    lateinit var sessionPath: String
        private set

    private var libraryLoaded = false

    fun setUp() {
        loadNativeLibrary()
        MobileBridge.testHarnessReset()
        bridge = UsbFdBridge.forIntegrationTest()
        UsbNative.bind(bridge)
        sessionPath = MobileBridge.testOpenFakePort(DEVICE_NAME)
        check(!sessionPath.startsWith("ERR:")) { "testOpenFakePort: $sessionPath" }
    }

    fun tearDown() {
        if (::bridge.isInitialized) {
            runCatching { bridge.closeDeviceFd(DEVICE_NAME) }
        }
        MobileBridge.testHarnessReset()
        MobileBridge.onAppDestroy()
    }

    private fun loadNativeLibrary() {
        if (libraryLoaded) return
        System.loadLibrary("tauri_app_lib")
        libraryLoaded = true
    }

    fun waitForHubBytes(minLen: Long = 1, timeoutMs: Long = 10000): Long {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            val len = MobileBridge.testHubBufferedLen(sessionPath)
            if (len >= minLen) return len
            Thread.sleep(10)
        }
        return MobileBridge.testHubBufferedLen(sessionPath)
    }
}
