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
    fun onUsbError_clears_registry() {
        assertTrue(MobileBridge.testRegistryHasPort(JniChainFixture.PATH))
        JniChainFixture.fake.failNextRead = "device exploded"
        Thread.sleep(800)
        assertFalse(
            "registry should drop port after SIOM error → onUsbError",
            MobileBridge.testRegistryHasPort(JniChainFixture.PATH),
        )
    }
}
