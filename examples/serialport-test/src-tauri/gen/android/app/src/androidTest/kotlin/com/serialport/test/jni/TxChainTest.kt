package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4

@RunWith(AndroidJUnit4::class)
class TxChainTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun rust_write_hits_fake_port() {
        val payload = "AT\r".toByteArray()
        val n = MobileBridge.testInvokeWrite(JniChainFixture.PATH, payload)
        assertEquals(payload.size.toLong(), n)
        assertArrayEquals(payload, JniChainFixture.fake.writtenBytes())
    }

    @Test
    fun roundtrip_at_echo_reaches_hub() {
        JniChainFixture.fake.clearWritten()
        val payload = "AT\r".toByteArray()
        val n = MobileBridge.testInvokeWrite(JniChainFixture.PATH, payload)
        assertEquals(payload.size.toLong(), n)
        val len = JniChainFixture.waitForHubBytes(minLen = 2, timeoutMs = 5000)
        assertTrue("expected AT echo in hub, len=$len", len >= 2)
        val idle = MobileBridge.testHubTakeIdle(JniChainFixture.PATH)
        val text = String(idle, Charsets.US_ASCII)
        assertTrue("expected OK from auto echo: $text", text.contains("OK"))
    }
}
