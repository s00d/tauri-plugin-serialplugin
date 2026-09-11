package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AtModemSimTest {

    @Before
    fun setUp() {
        JniChainFixture.setUp()
        assertTrue(MobileBridge.testFakeEnableAtModem(JniChainFixture.DEVICE_NAME))
    }

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun write_at_reaches_hub_with_ok() {
        val n = MobileBridge.testInvokeWrite(JniChainFixture.sessionPath, "AT\r".toByteArray())
        assertTrue("write failed: $n", n > 0)
        val len = JniChainFixture.waitForHubBytes(minLen = 2)
        assertTrue("expected OK in hub, len=$len", len >= 2)
        val idle = String(MobileBridge.testHubTakeIdle(JniChainFixture.sessionPath), Charsets.US_ASCII)
        assertTrue("expected OK: $idle", idle.contains("OK"))
    }

    @Test
    fun exchange_at_csq_via_modem_sim() {
        assertTrue(MobileBridge.testExchangeBegin(JniChainFixture.sessionPath, "AT+CSQ"))
        val n = MobileBridge.testInvokeWrite(JniChainFixture.sessionPath, "AT+CSQ\r".toByteArray())
        assertTrue("write failed: $n", n > 0)
        val result = MobileBridge.testExchangeWait(JniChainFixture.sessionPath, 10_000)
        assertTrue("expected OK exchange, got $result", result.startsWith("OK:"))
    }
}
