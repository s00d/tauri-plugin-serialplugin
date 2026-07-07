package app.tauri.serialplugin.manager

import app.tauri.serialplugin.models.DataBits
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.Parity
import app.tauri.serialplugin.models.SerialPortConfig
import app.tauri.serialplugin.models.StopBits
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [28])
class UsbPortSessionTest {

    private val path = "/dev/bus/usb/001/099"
    private lateinit var fake: FakeUsbSerialPort
    private lateinit var session: UsbPortSession

    @Before
    fun setUp() {
        fake = FakeUsbSerialPort(path)
        fake.openForTest()
        session = UsbPortSession(
            path = path,
            port = fake,
            config = SerialPortConfig(
                path = path,
                baudRate = 115200,
                dataBits = DataBits.EIGHT,
                flowControl = FlowControl.NONE,
                parity = Parity.NONE,
                stopBits = StopBits.ONE,
                timeout = 1000,
            ),
            onRx = {},
            onError = {},
            onRecover = { session.restartReadLoop() },
        )
    }

    @After
    fun tearDown() {
        session.shutdown()
        session.close()
    }

    @Test
    fun shutdown_preventsRestart() {
        session.startReadLoop()
        assertTrue(session.isReading)
        session.shutdown()
        session.restartReadLoop()
        assertFalse(session.isReading)
    }

    @Test
    fun close_preventsRestart() {
        session.startReadLoop()
        session.close()
        session.restartReadLoop()
        assertFalse(session.isReading)
    }
}
