package app.tauri.serialplugin.models

import org.junit.Assert.assertEquals
import org.junit.Test

class SerialModelsTest {
    @Test
    fun clearBuffer_fromValue_isCaseInsensitive() {
        assertEquals(ClearBuffer.INPUT, ClearBuffer.fromValue("input"))
        assertEquals(ClearBuffer.OUTPUT, ClearBuffer.fromValue("OUTPUT"))
        assertEquals(ClearBuffer.ALL, ClearBuffer.fromValue("All"))
    }

    @Test
    fun dataBits_fromValue_roundTrip() {
        assertEquals(DataBits.EIGHT, DataBits.fromValue(8))
        assertEquals(DataBits.FIVE, DataBits.fromValue(5))
    }

    @Test
    fun flowControl_fromValue() {
        assertEquals(FlowControl.NONE, FlowControl.fromValue(0))
        assertEquals(FlowControl.SOFTWARE, FlowControl.fromValue(1))
        assertEquals(FlowControl.HARDWARE, FlowControl.fromValue(2))
    }

    @Test
    fun serialPortConfig_defaultTimeout() {
        val c = SerialPortConfig(path = "/dev/usb", baudRate = 115200)
        assertEquals(1000, c.timeout)
    }

    @Test
    fun dataBits_fromValue_unknownDefaultsToEight() {
        assertEquals(DataBits.EIGHT, DataBits.fromValue(999))
    }

    @Test
    fun parity_roundTrip_and_unknownDefaultsToNone() {
        Parity.entries.forEach { p ->
            assertEquals(p, Parity.fromValue(p.value))
        }
        assertEquals(Parity.NONE, Parity.fromValue(-1))
    }

    @Test
    fun stopBits_roundTrip_and_unknownDefaultsToOne() {
        StopBits.entries.forEach { s ->
            assertEquals(s, StopBits.fromValue(s.value))
        }
        assertEquals(StopBits.ONE, StopBits.fromValue(-99))
    }

    @Test
    fun clearBuffer_fromNumericValue() {
        assertEquals(ClearBuffer.INPUT, ClearBuffer.fromValue(0))
        assertEquals(ClearBuffer.OUTPUT, ClearBuffer.fromValue(1))
        assertEquals(ClearBuffer.ALL, ClearBuffer.fromValue(2))
    }

    @Test
    fun clearBuffer_unknownDefaultsToInput() {
        assertEquals(ClearBuffer.INPUT, ClearBuffer.fromValue("not-a-buffer"))
    }
}
