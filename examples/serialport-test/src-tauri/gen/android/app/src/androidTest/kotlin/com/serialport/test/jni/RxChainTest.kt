package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4

@RunWith(AndroidJUnit4::class)
class RxChainTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun feedRx_direct_reaches_hub() {
        val payload = "RING\r\n".toByteArray()
        MobileBridge.feedRx(JniChainFixture.PATH, payload)
        assertTrue(MobileBridge.testHubBufferedLen(JniChainFixture.PATH) >= payload.size)
    }

    @Test
    fun siom_full_chain_reaches_hub() {
        JniChainFixture.fake.enqueueRx("AT\r\r\nOK\r\n".toByteArray())
        val len = JniChainFixture.waitForHubBytes(minLen = 2)
        assertTrue("expected bytes in Rust hub, got len=$len", len >= 2)
        val idle = MobileBridge.testHubTakeIdle(JniChainFixture.PATH)
        val text = String(idle, Charsets.US_ASCII)
        assertTrue("expected OK in hub idle buffer: $text", text.contains("OK"))
    }

    @Test
    fun coalescing_then_jni() {
        JniChainFixture.fake.enqueueRx("A".toByteArray())
        JniChainFixture.fake.enqueueRx("B".toByteArray())
        JniChainFixture.fake.enqueueRx("C".toByteArray())
        val len = JniChainFixture.waitForHubBytes(minLen = 3)
        assertTrue(len >= 3)
        val idle = MobileBridge.testHubTakeIdle(JniChainFixture.PATH)
        assertArrayEquals("ABC".toByteArray(), idle)
    }
}
