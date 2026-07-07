package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import app.tauri.serialplugin.UsbNative
import app.tauri.serialplugin.manager.FakeUsbSerialPort
import app.tauri.serialplugin.manager.UsbBridge
import app.tauri.serialplugin.models.DataBits
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.Parity
import app.tauri.serialplugin.models.SerialPortConfig
import app.tauri.serialplugin.models.StopBits

/** Sets up FakeUsb + production JNI sink + Rust test harness for one port. */
object JniChainFixture {
    const val PATH: String = "/dev/bus/usb/001/099"

    lateinit var bridge: UsbBridge
        private set
    lateinit var fake: FakeUsbSerialPort
        private set

    private var libraryLoaded = false

    fun setUp() {
        loadNativeLibrary()
        MobileBridge.testHarnessReset()
        bridge = UsbBridge.forIntegrationTest(immediateCoalesce = true)
        UsbNative.bind(bridge)
        fake = FakeUsbSerialPort(PATH)
        fake.openForTest()
        bridge.adoptFakePortForTest(
            fake,
            SerialPortConfig(
                path = PATH,
                baudRate = 115200,
                dataBits = DataBits.EIGHT,
                flowControl = FlowControl.NONE,
                parity = Parity.NONE,
                stopBits = StopBits.ONE,
                timeout = 1000,
            ),
        )
        MobileBridge.testRegisterPort(PATH)
    }

    fun tearDown() {
        if (::bridge.isInitialized) {
            runCatching { bridge.close(PATH) }
        }
        MobileBridge.testHarnessReset()
        MobileBridge.onAppDestroy()
    }

    private fun loadNativeLibrary() {
        if (libraryLoaded) return
        System.loadLibrary("tauri_app_lib")
        libraryLoaded = true
    }

    fun waitForHubBytes(minLen: Long = 1, timeoutMs: Long = 3000): Long {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            val len = MobileBridge.testHubBufferedLen(PATH)
            if (len >= minLen) return len
            Thread.sleep(10)
        }
        return MobileBridge.testHubBufferedLen(PATH)
    }
}
