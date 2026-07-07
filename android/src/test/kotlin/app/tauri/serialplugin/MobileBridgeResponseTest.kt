package app.tauri.serialplugin

import org.junit.Assert.assertEquals
import org.junit.Test
import java.nio.charset.Charset

class MobileBridgeResponseTest {
    @Test
    fun readIso88591PreservesHighBytes() {
        val data = byteArrayOf(0x48, 0x65, 0x6C, 0x6C, 0x6F, 0xFF.toByte(), 0xFE.toByte())
        val text = String(data, Charset.forName("ISO-8859-1"))
        assertEquals(7, text.length)
        assertEquals(0xFF, text[5].code)
        assertEquals(0xFE, text[6].code)
    }
}
