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
class UsbDetachChainTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun detach_clears_registry() {
        assertTrue(MobileBridge.testRegistryHasPort(JniChainFixture.sessionPath))
        MobileBridge.onDeviceDetached(JniChainFixture.DEVICE_NAME)
        assertFalse(
            "registry should drop port after USB detach",
            MobileBridge.testRegistryHasPort(JniChainFixture.sessionPath),
        )
    }
}
