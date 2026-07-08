package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4

@RunWith(AndroidJUnit4::class)
class UsbErrorChainTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun fake_read_error_clears_registry() {
        assertTrue(MobileBridge.testRegistryHasPort(JniChainFixture.sessionPath))
        MobileBridge.testFakeInjectError(JniChainFixture.DEVICE_NAME, "device exploded")
        Thread.sleep(1500)
        assertFalse(
            "registry should drop port after reader error → onUsbError",
            MobileBridge.testRegistryHasPort(JniChainFixture.sessionPath),
        )
    }
}
