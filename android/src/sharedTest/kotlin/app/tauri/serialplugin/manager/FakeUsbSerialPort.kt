package app.tauri.serialplugin.manager

import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import com.hoho.android.usbserial.driver.UsbSerialDriver
import com.hoho.android.usbserial.driver.UsbSerialPort
import org.mockito.kotlin.mock
import org.mockito.kotlin.whenever
import java.io.IOException
import java.util.EnumSet
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Software UART emulator for JVM tests — same [UsbSerialPort] contract as hardware,
 * backed by in-memory RX/TX queues (similar spirit to upstream RFC-2217 device tests).
 */
class FakeUsbSerialPort(
    private val path: String = "/dev/bus/usb/001/099",
    packetSize: Int = 64,
) : UsbSerialPort {

    private val rxQueue = LinkedBlockingQueue<Byte>()
    private val written = mutableListOf<Byte>()
    private val open = AtomicBoolean(false)
    private val readEndpoint: UsbEndpoint = mockEndpoint(packetSize, IN)
    private val writeEndpoint: UsbEndpoint = mockEndpoint(packetSize, OUT)
    private val device: UsbDevice = mock<UsbDevice>().also {
        whenever(it.deviceName).thenReturn(path)
        whenever(it.vendorId).thenReturn(0x1234)
        whenever(it.productId).thenReturn(0x5678)
    }
    private val driver = FakeUsbSerialDriver(device)

    @Volatile var autoAtEcho: Boolean = true

    /** Next [read] throws this message once (SIOM error tests). */
    @Volatile var failNextRead: String? = null

    @Volatile var detached: Boolean = false

    var cts: Boolean = true
    var dsr: Boolean = true
    var ri: Boolean = false
    var cd: Boolean = true
    var dtr: Boolean = false
    var rts: Boolean = false

    private var baudRate: Int = 115200
    private var dataBits: Int = UsbSerialPort.DATABITS_8
    private var stopBits: Int = UsbSerialPort.STOPBITS_1
    private var parity: Int = UsbSerialPort.PARITY_NONE
    private var flowControl: UsbSerialPort.FlowControl = UsbSerialPort.FlowControl.NONE

    fun openForTest() {
        detached = false
        open.set(true)
    }

    fun enqueueRx(data: ByteArray) {
        data.forEach { rxQueue.offer(it) }
    }

    fun writtenBytes(): ByteArray = written.toByteArray()

    fun clearWritten() {
        written.clear()
    }

    override fun getDriver(): UsbSerialDriver = driver

    override fun getDevice(): UsbDevice = device

    override fun getPortNumber(): Int = 0

    override fun getWriteEndpoint(): UsbEndpoint = writeEndpoint

    override fun getReadEndpoint(): UsbEndpoint = readEndpoint

    override fun getSerial(): String? = "FAKE-SERIAL"

    override fun setReadQueue(bufferCount: Int, bufferSize: Int) {
        require(open.get()) { "not open" }
    }

    override fun getReadQueueBufferCount(): Int = 0

    override fun getReadQueueBufferSize(): Int = 0

    override fun open(connection: UsbDeviceConnection) {
        openForTest()
    }

    override fun close() {
        open.set(false)
    }

    override fun read(dest: ByteArray, timeout: Int): Int =
        read(dest, dest.size, timeout)

    override fun read(dest: ByteArray, length: Int, timeout: Int): Int {
        assertOpen()
        failNextRead?.let { msg ->
            failNextRead = null
            throw IOException(msg)
        }
        if (timeout == 0) {
            if (detached) throw IOException("USB device detached")
            if (!open.get()) throw IOException("Port not open")
            if (Thread.currentThread().isInterrupted) throw IOException("Interrupted")
            val first = rxQueue.poll(50, TimeUnit.MILLISECONDS) ?: return 0
            dest[0] = first
            var n = 1
            while (n < length) {
                val b = rxQueue.poll() ?: break
                dest[n++] = b
            }
            return n
        }
        val deadline = System.nanoTime() + timeout * 1_000_000L
        var total = 0
        while (total < length && System.nanoTime() < deadline) {
            val remainingMs = ((deadline - System.nanoTime()) / 1_000_000L).coerceAtLeast(1)
            val b = rxQueue.poll(remainingMs, TimeUnit.MILLISECONDS) ?: break
            dest[total++] = b
            while (total < length) {
                val next = rxQueue.poll() ?: break
                dest[total++] = next
            }
        }
        return total
    }

    override fun write(src: ByteArray, timeout: Int) {
        write(src, src.size, timeout)
    }

    override fun write(src: ByteArray, length: Int, timeout: Int) {
        assertOpen()
        val chunk = src.copyOfRange(0, length)
        written.addAll(chunk.toList())
        if (autoAtEcho) {
            maybeEchoAt(chunk)
        }
    }

    private fun maybeEchoAt(chunk: ByteArray) {
        val text = String(chunk, Charsets.US_ASCII).trim()
        when {
            text.equals("AT", ignoreCase = true) -> enqueueRx("OK\r\n".toByteArray())
            text.startsWith("AT+", ignoreCase = true) -> {
                val prefix = text.substringBefore('?').substringBefore('=')
                enqueueRx("$prefix: 0,0\r\nOK\r\n".toByteArray())
            }
        }
    }

    override fun setParameters(baudRate: Int, dataBits: Int, stopBits: Int, parity: Int) {
        assertOpen()
        this.baudRate = baudRate
        this.dataBits = dataBits
        this.stopBits = stopBits
        this.parity = parity
    }

    override fun getCD(): Boolean = cd
    override fun getCTS(): Boolean = cts
    override fun getDSR(): Boolean = dsr
    override fun getDTR(): Boolean = dtr
    override fun getRI(): Boolean = ri
    override fun getRTS(): Boolean = rts

    override fun setDTR(value: Boolean) {
        dtr = value
    }

    override fun setRTS(value: Boolean) {
        rts = value
    }

    override fun getControlLines(): EnumSet<UsbSerialPort.ControlLine> {
        val set = EnumSet.noneOf(UsbSerialPort.ControlLine::class.java)
        if (cts) set.add(UsbSerialPort.ControlLine.CTS)
        if (dsr) set.add(UsbSerialPort.ControlLine.DSR)
        if (cd) set.add(UsbSerialPort.ControlLine.CD)
        if (ri) set.add(UsbSerialPort.ControlLine.RI)
        if (rts) set.add(UsbSerialPort.ControlLine.RTS)
        if (dtr) set.add(UsbSerialPort.ControlLine.DTR)
        return set
    }

    override fun getSupportedControlLines(): EnumSet<UsbSerialPort.ControlLine> =
        EnumSet.allOf(UsbSerialPort.ControlLine::class.java)

    override fun setFlowControl(flowControl: UsbSerialPort.FlowControl) {
        this.flowControl = flowControl
    }

    override fun getFlowControl(): UsbSerialPort.FlowControl = flowControl

    override fun getSupportedFlowControl(): EnumSet<UsbSerialPort.FlowControl> =
        EnumSet.of(UsbSerialPort.FlowControl.NONE, UsbSerialPort.FlowControl.RTS_CTS)

    override fun getXON(): Boolean = true

    override fun purgeHwBuffers(purgeWriteBuffers: Boolean, purgeReadBuffers: Boolean) {
        if (purgeReadBuffers) {
            rxQueue.clear()
        }
        if (purgeWriteBuffers) {
            written.clear()
        }
    }

    override fun setBreak(value: Boolean) {
        // no-op
    }

    override fun isOpen(): Boolean = open.get() && !detached

    private fun assertOpen() {
        if (detached) throw IOException("USB device detached")
        if (!open.get()) throw IOException("Port not open")
    }

    private companion object {
        private const val IN = 0x80
        private const val OUT = 0x00

        private fun mockEndpoint(packetSize: Int, direction: Int): UsbEndpoint {
            val endpoint = mock<UsbEndpoint>()
            whenever(endpoint.maxPacketSize).thenReturn(packetSize)
            whenever(endpoint.direction).thenReturn(direction)
            return endpoint
        }
    }
}

/** Minimal driver shell so [FakeUsbSerialPort.getDriver] / [getDevice] work. */
class FakeUsbSerialDriver(
    private val usbDevice: UsbDevice,
) : UsbSerialDriver {
    override fun getDevice(): UsbDevice = usbDevice

    override fun getPorts(): List<UsbSerialPort> = emptyList()
}
