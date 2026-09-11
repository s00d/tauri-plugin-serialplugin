package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4

/**
 * Regression for Android ClassNotFoundException when the first USB JNI call runs on a
 * Rust-attached worker thread (system class loader) instead of the Java binder thread.
 */
@RunWith(AndroidJUnit4::class)
class EnumerateWorkerThreadTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun enumerate_json_from_rust_worker_thread_after_bind() {
        val result = MobileBridge.testEnumerateJsonFromWorkerThread()
        assertNotNull(result)
        assertFalse(
            "worker enumerate must not fail after UsbNative.bind; got: $result",
            result.startsWith("ERR:"),
        )
        // Fake bridge returns a JSON object/array; empty list is fine.
        assertTrue(
            "expected JSON enumerate payload, got: $result",
            result.trimStart().startsWith("{") || result.trimStart().startsWith("["),
        )
    }
}
