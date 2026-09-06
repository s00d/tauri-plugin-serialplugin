package com.serialport.test.jni

import app.tauri.serialplugin.MobileBridge
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import androidx.test.ext.junit.runners.AndroidJUnit4

@RunWith(AndroidJUnit4::class)
class ExchangeChainTest {

    @Before
    fun setUp() = JniChainFixture.setUp()

    @After
    fun tearDown() = JniChainFixture.tearDown()

    @Test
    fun exchange_at_csq_with_urc_creg_completes_ok() {
        assertTrue(
            MobileBridge.testExchangeBegin(JniChainFixture.sessionPath, "AT+CSQ"),
        )
        MobileBridge.testFakeInjectRx(
            JniChainFixture.DEVICE_NAME,
            "\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n".toByteArray(),
        )
        val result = MobileBridge.testExchangeWait(JniChainFixture.sessionPath, 10_000)
        assertTrue("expected OK exchange, got $result", result.startsWith("OK:"))
    }
}
