package app.tauri.serialplugin.manager

import android.util.Log
import app.tauri.serialplugin.models.ClearBuffer
import app.tauri.serialplugin.models.DataBits
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.Parity
import app.tauri.serialplugin.models.SerialPortConfig
import app.tauri.serialplugin.models.StopBits
import com.hoho.android.usbserial.driver.SerialTimeoutException
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.util.SerialInputOutputManager
import java.io.IOException

/**
 * One open USB serial port: SIOM pushes RX to [onRx], writes go straight to hardware.
 */
internal class UsbPortSession(
    val path: String,
    private val port: UsbSerialPort,
    var config: SerialPortConfig,
    private val onRx: (ByteArray) -> Unit,
    private val onError: (String) -> Unit,
    private val onRecover: () -> Unit,
) {
    private val siomLock = Any()
    private var siom: SerialInputOutputManager? = null
    @Volatile private var alive = true

    val isReading: Boolean
        get() = synchronized(siomLock) { siom != null }

    fun startReadLoop() {
        synchronized(siomLock) {
            if (!alive || siom != null || !port.isOpen) return
            startReadLoopLocked()
        }
    }

    /** Stop SIOM; further recover/restart is ignored until a new session is opened. */
    fun shutdown() {
        synchronized(siomLock) {
            alive = false
            stopReadLoopLocked()
        }
    }

    fun restartReadLoop() {
        synchronized(siomLock) {
            if (!alive || !port.isOpen) return
            stopReadLoopLocked()
            if (!alive || !port.isOpen) return
            startReadLoopLocked()
        }
    }

    fun stopReadLoop() {
        synchronized(siomLock) {
            stopReadLoopLocked()
        }
    }

    private fun startReadLoopLocked() {
        val mgr = SerialInputOutputManager(port, object : SerialInputOutputManager.Listener {
            override fun onNewData(data: ByteArray) {
                Log.d(TAG, "RX $path +${data.size}B")
                onRx(data)
            }

            override fun onRunError(e: Exception) {
                val msg = e.message ?: e.toString()
                if (isRecoverableSiomError(msg)) {
                    Log.w(TAG, "SIOM $path: spurious $msg — scheduling read loop restart")
                    onRecover()
                    return
                }
                Log.e(TAG, "SIOM $path: $msg")
                onError(msg)
            }
        })
        val packet = maxOf(port.readEndpoint.maxPacketSize, MIN_READ_BUF)
        mgr.setReadBufferSize(packet)
        mgr.setReadQueue(0)
        mgr.setReadTimeout(0) // requestWait like SimpleUsbTerminal; avoids bulkTransfer+GET_STATUS polling
        siom = mgr
        mgr.start()
        Log.i(TAG, "read loop $path buf=$packet")
    }

    private fun stopReadLoopLocked() {
        val mgr = siom ?: return
        siom = null
        try {
            mgr.stop()
        } catch (_: Exception) {
        }
    }

    private fun isRecoverableSiomError(msg: String): Boolean {
        val lower = msg.lowercase()
        if (lower.contains("null object reference") || lower.contains("connection closed")) {
            return false
        }
        return lower.contains("get_status") ||
            lower.contains("queueing usb request failed") ||
            lower.contains("waiting for usb request failed")
    }

    fun write(data: ByteArray): Int {
        val timeout = maxOf(config.timeout.coerceIn(1, 600_000), WRITE_TIMEOUT_MS)
        var offset = 0
        while (offset < data.size) {
            try {
                port.write(data.copyOfRange(offset, data.size), timeout)
                Log.d(TAG, "TX $path +${data.size}B")
                return data.size
            } catch (e: SerialTimeoutException) {
                val n = maxOf(e.bytesTransferred, 0)
                offset += n
                if (n <= 0) throw e
            } catch (e: IOException) {
                Log.e(TAG, "TX $path failed: ${e.message}")
                throw e
            }
        }
        return data.size
    }

    fun pollRead(timeout: Int, size: Int): ByteArray {
        if (isReading) throw IOException(LISTEN_READ_MUTEX_MESSAGE)
        val buf = ByteArray(maxOf(size, MIN_READ_BUF))
        val ms = maxOf(if (timeout > 0) timeout else config.timeout, 200)
        val n = port.read(buf, ms)
        if (n > 0) return buf.copyOf(n)
        throw IOException("Read timeout ($ms ms)")
    }

    fun close() {
        synchronized(siomLock) {
            alive = false
            stopReadLoopLocked()
        }
        if (config.assertDtrRts) setModemLines(port, dtr = false, rts = false)
        try {
            port.close()
        } catch (_: Exception) {
        }
        Log.i(TAG, "close $path")
    }

    fun ctl(op: String, params: Map<String, Any?>): Any? = when (op) {
        "setBaudRate" -> {
            config.baudRate = (params["baudRate"] as Number).toInt()
            applyParams()
            true
        }
        "setTimeout" -> {
            config.timeout = (params["timeout"] as Number).toInt().coerceIn(1, 600_000)
            true
        }
        "setDataBits" -> {
            config.dataBits = DataBits.fromValue((params["dataBits"] as Number).toInt())
            applyParams()
            true
        }
        "setParity" -> {
            config.parity = Parity.fromValue((params["parity"] as Number).toInt())
            applyParams()
            true
        }
        "setStopBits" -> {
            config.stopBits = StopBits.fromValue((params["stopBits"] as Number).toInt())
            applyParams()
            true
        }
        "setFlowControl" -> setFlowControl((params["flowControl"] as Number).toInt())
        "clearBuffer" -> clearBuffer(ClearBuffer.fromValue(params["bufferType"]?.toString() ?: "input"))
        "setBreak" -> setBreak(true)
        "clearBreak" -> setBreak(false)
        "cancelRead" -> true
        "bytesToWrite" -> 0
        else -> false
    }

    fun signal(op: String, level: Boolean?): Boolean = try {
        when (op) {
            "writeRts" -> {
                port.rts = level == true
                true
            }
            "writeDtr" -> {
                port.dtr = level == true
                true
            }
            "readCts" -> port.cts
            "readDsr" -> port.dsr
            "readRi" -> port.ri
            "readCd" -> port.cd
            else -> false
        }
    } catch (_: Exception) {
        false
    }

    private fun applyParams() {
        port.setParameters(
            config.baudRate,
            config.dataBits.value,
            config.stopBits.value,
            config.parity.value,
        )
    }

    private fun setFlowControl(value: Int): Boolean = try {
        val mode = when (FlowControl.fromValue(value)) {
            FlowControl.NONE -> UsbSerialPort.FlowControl.NONE
            FlowControl.HARDWARE -> UsbSerialPort.FlowControl.RTS_CTS
            FlowControl.SOFTWARE -> UsbSerialPort.FlowControl.XON_XOFF
        }
        port.setFlowControl(mode)
        config.flowControl = FlowControl.fromValue(value)
        true
    } catch (_: Exception) {
        false
    }

    private fun clearBuffer(kind: ClearBuffer): Boolean = try {
        when (kind) {
            ClearBuffer.INPUT -> port.purgeHwBuffers(false, true)
            ClearBuffer.OUTPUT -> port.purgeHwBuffers(true, false)
            ClearBuffer.ALL -> port.purgeHwBuffers(true, true)
        }
        true
    } catch (_: UnsupportedOperationException) {
        true
    } catch (_: Exception) {
        false
    }

    private fun setBreak(value: Boolean): Boolean = try {
        port.setBreak(value)
        true
    } catch (_: Exception) {
        false
    }

    companion object {
        private const val TAG = "UsbPort"
        private const val WRITE_TIMEOUT_MS = 2000
        private const val MIN_READ_BUF = 64
        const val LISTEN_READ_MUTEX_MESSAGE =
            "Cannot read while watch is active; call unwatch first"
    }
}
