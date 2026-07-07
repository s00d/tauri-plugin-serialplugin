package app.tauri.serialplugin.manager

import app.tauri.serialplugin.manager.UsbPortSession
import app.tauri.serialplugin.models.DataBits
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.Parity
import app.tauri.serialplugin.models.SerialPortConfig
import app.tauri.serialplugin.models.StopBits
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.IOException

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class UsbBridgeTest {

    private val path = "/dev/bus/usb/001/099"
    private lateinit var sink: RecordingRxSink
    private lateinit var fake: FakeUsbSerialPort
    private lateinit var bridge: UsbBridge

    @Before
    fun setUp() {
        sink = RecordingRxSink()
        fake = FakeUsbSerialPort(path)
        fake.openForTest()
        bridge = UsbBridge.forTesting(sink)
        bridge.adoptFakePort(
            fake,
            SerialPortConfig(
                path = path,
                baudRate = 115200,
                dataBits = DataBits.EIGHT,
                flowControl = FlowControl.NONE,
                parity = Parity.NONE,
                stopBits = StopBits.ONE,
                timeout = 1000,
            ),
        )
    }

    @After
    fun tearDown() {
        if (::bridge.isInitialized) {
            bridge.close(path)
        }
    }

    @Test
    fun open_assertsDtrAndRtsWhenEnabled() {
        assertTrue(fake.dtr)
        assertTrue(fake.rts)
    }

    @Test
    fun close_deassertsDtrAndRts() {
        bridge.close(path)
        assertFalse(fake.dtr)
        assertFalse(fake.rts)
    }

    @Test
    fun open_startsSiomAutomatically() {
        assertTrue(bridge.isSiomRunning(path))
    }

    @Test
    fun write_recordsBytesOnFake() {
        val payload = "AT\r".toByteArray()
        val n = bridge.write(path, payload)
        assertEquals(payload.size, n)
        assertArrayEquals(payload, fake.writtenBytes())
    }

    @Test
    fun pollRead_rejectedWhileSiomActive() {
        val ex = assertThrows(IOException::class.java) {
            bridge.read(path, timeout = 500, size = 64)
        }
        assertEquals(UsbPortSession.LISTEN_READ_MUTEX_MESSAGE, ex.message)
    }

    @Test
    fun pollRead_afterStopSiom_returnsEnqueuedBytes() {
        bridge.stopSiom(path)
        Thread.sleep(200)
        fake.enqueueRx("OK\r\n".toByteArray())
        val data = bridge.read(path, timeout = 500, size = 64)
        assertArrayEquals("OK\r\n".toByteArray(), data)
    }

    @Test
    fun siom_deliversRxViaSink() {
        fake.enqueueRx("RING\r\n".toByteArray())
        val rx = sink.awaitRx(path, timeoutMs = 3000)
        assertNotNull(rx)
        assertArrayEquals("RING\r\n".toByteArray(), rx)
    }

    @Test
    fun siom_atEchoRoundTrip() {
        bridge.write(path, "AT\r".toByteArray())
        val rx = sink.awaitRx(path, timeoutMs = 3000)
        assertNotNull(rx)
        assertTrue(String(rx!!, Charsets.US_ASCII).contains("OK"))
    }

    @Test
    fun siom_consecutiveWritesDoNotDeadlock() {
        repeat(5) {
            bridge.write(path, "AT\r".toByteArray())
            val rx = sink.awaitRx(path, timeoutMs = 3000)
            assertNotNull("missing RX for write #$it", rx)
        }
        assertEquals(5, fake.writtenBytes().size / 3)
    }

    @Test
    fun modemLines_readableDuringSiom() {
        fake.cts = true
        assertTrue(bridge.signal(path, "readCts", null))
        fake.cts = false
        assertFalse(bridge.signal(path, "readCts", null))
    }

    @Test
    fun hardReadError_reportsUsbError() {
        fake.failNextRead = "device exploded"
        Thread.sleep(500)
        assertFalse(sink.errors.isEmpty())
        assertTrue(sink.errors.any { it.path == path })
    }

    @Test
    fun close_stopsSiom() {
        assertTrue(bridge.isSiomRunning(path))
        bridge.close(path)
        assertFalse(bridge.isSiomRunning(path))
    }
}
