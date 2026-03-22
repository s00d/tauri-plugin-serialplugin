package app.tauri.serialplugin.manager

import app.tauri.plugin.JSObject
import org.junit.Assert.assertEquals
import org.junit.Test

class SerialDataEmitFieldsTest {
    @Test
    fun payload_from_chunk_preserves_utf8_string() {
        val chunk = "café 🚀".toByteArray(Charsets.UTF_8)
        val p = serialDataPayloadFromChunk("/dev/usb", chunk)
        assertEquals("/dev/usb", p.path)
        assertEquals("café 🚀", p.dataAsString)
        assertEquals(chunk.size, p.size)
    }

    @Test
    fun payload_from_chunk_preserves_nul_and_binary_bytes() {
        val chunk = byteArrayOf(0x48, 0x00, 0x01.toByte())
        val p = serialDataPayloadFromChunk("/dev/usb", chunk)
        assertEquals(3, p.size)
        assertEquals(3, p.dataAsString.length)
        assertEquals('\u0000', p.dataAsString[1])
        assertEquals(1.toChar(), p.dataAsString[2])
    }

    @Test
    fun applyToJSObject_sets_path_data_size() {
        val p = SerialDataEmitFields(
            path = "/dev/tty",
            dataAsString = "ab",
            size = 2,
        )
        val o = JSObject()
        p.applyToJSObject(o)
        assertEquals("/dev/tty", o.getString("path"))
        assertEquals("ab", o.getString("data"))
        assertEquals(2, o.getInteger("size"))
    }

    @Test
    fun flush_accumulator_invokes_emit_once_with_merged_bytes() {
        val acc = SerialByteAccumulator()
        acc.append(byteArrayOf(65, 66))
        acc.append(byteArrayOf(67))
        val captured = mutableListOf<SerialDataEmitFields>()
        flushAccumulatorToEmit("COM1", acc) { captured.add(it) }
        assertEquals(1, captured.size)
        assertEquals("COM1", captured[0].path)
        assertEquals("ABC", captured[0].dataAsString)
        assertEquals(3, captured[0].size)
    }

    @Test
    fun flush_accumulator_skips_emit_when_empty() {
        val acc = SerialByteAccumulator()
        assertEquals(0, acc.pendingByteCount())
        assertEquals(0, acc.drain().size)
        val captured = mutableListOf<SerialDataEmitFields>()
        flushAccumulatorToEmit("COM1", acc) { captured.add(it) }
        assertEquals(0, captured.size)
    }
}
