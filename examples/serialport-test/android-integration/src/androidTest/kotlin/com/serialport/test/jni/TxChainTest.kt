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
    fun rust_write_hits_fake_transport() {
        val payload = "AT\r".toByteArray()
        val n = MobileBridge.testInvokeWrite(JniChainFixture.sessionPath, payload)
        assertEquals(payload.size.toLong(), n)
        assertArrayEquals(payload, MobileBridge.testFakeTakeTx(JniChainFixture.DEVICE_NAME))
    }

    @Test
    fun roundtrip_write_then_inject_rx_reaches_hub() {
        val payload = "AT\r".toByteArray()
        val n = MobileBridge.testInvokeWrite(JniChainFixture.sessionPath, payload)
        assertEquals(payload.size.toLong(), n)
        MobileBridge.testFakeInjectRx(JniChainFixture.DEVICE_NAME, "OK\r\n".toByteArray())
        val len = JniChainFixture.waitForHubBytes(minLen = 2, timeoutMs = 5000)
        assertTrue("expected OK in hub, len=$len", len >= 2)
        val idle = MobileBridge.testHubTakeIdle(JniChainFixture.sessionPath)
        val text = String(idle, Charsets.US_ASCII)
        assertTrue("expected OK from injected RX: $text", text.contains("OK"))
    }
}
