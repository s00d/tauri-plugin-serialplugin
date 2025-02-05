package app.tauri.serialplugin.manager

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Build
import com.hoho.android.usbserial.driver.UsbSerialPort
import com.hoho.android.usbserial.driver.UsbSerialProber
import com.hoho.android.usbserial.util.SerialInputOutputManager
import app.tauri.serialplugin.models.*
import java.util.concurrent.Executors
import java.io.IOException
import android.util.Log
import androidx.annotation.RequiresApi

class SerialPortManager(private val context: Context) {
    private val usbManager: UsbManager = context.getSystemService(Context.USB_SERVICE) as UsbManager
    private val portMap = mutableMapOf<String, UsbSerialPort>()
    private val ioManagerMap = mutableMapOf<String, SerialInputOutputManager>()
    private val executor = Executors.newCachedThreadPool()
    
    private val ACTION_USB_PERMISSION = "app.tauri.serialplugin.USB_PERMISSION"
    
    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (ACTION_USB_PERMISSION == intent.action) {
                synchronized(this) {
                    val device: UsbDevice? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
                    } else {
                        intent.getParcelableExtra(UsbManager.EXTRA_DEVICE) as UsbDevice?
                    }
                    if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) {
                        device?.let {
                            // Permission granted, proceed with connection
                        }
                    }
                }
            }
        }
    }

    fun registerReceiver() {
        val filter = IntentFilter(ACTION_USB_PERMISSION)

        // Для Android O (API 26) и выше используем 3 параметра
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.registerReceiver(usbReceiver, filter, Context.RECEIVER_EXPORTED)
        } else {
            // Для более старых версий (до Android O) регистрируем без третьего параметра
            context.registerReceiver(usbReceiver, filter)
        }
    }

    fun unregisterReceiver() {
        try {
            context.unregisterReceiver(usbReceiver)
        } catch (e: IllegalArgumentException) {
            // Получите ошибку, если ресивер не зарегистрирован
        }
    }
    
    init {

    }

    fun getAvailablePorts(): Map<String, Map<String, String>> {
        val result = mutableMapOf<String, Map<String, String>>()
        val availableDrivers = UsbSerialProber.getDefaultProber().findAllDrivers(usbManager)

        Log.d("SerialPortManager", "Available drivers: ${availableDrivers.size}")

        availableDrivers.forEach { driver ->
            val device = driver.device
            Log.d("SerialPortManager", "Found device: ${device.deviceName}, Vendor ID: ${device.vendorId}, Product ID: ${device.productId}")

            result[device.deviceName] = mapOf(
                "type" to "USB",
                "vid" to device.vendorId.toString(),
                "pid" to device.productId.toString(),
                "manufacturer" to (device.manufacturerName ?: "Unknown"),
                "product" to (device.productName ?: "Unknown"),
                "serial_number" to (device.serialNumber ?: "Unknown")
            )

            Log.d("SerialPortManager", "Device Info: ${result[device.deviceName]}")
        }

        Log.d("SerialPortManager", "Total available ports: ${result.size}")

        return result
    }

    fun getManagedPorts(): List<String> {
        return portMap.keys.toList()
    }

    fun openPort(config: SerialPortConfig): Boolean {
        try {
            val availableDrivers = UsbSerialProber.getDefaultProber().findAllDrivers(usbManager)
            val driver = availableDrivers.find { it.device.deviceName == config.path }
                ?: throw IOException("Device not found")
            
            if (!usbManager.hasPermission(driver.device)) {
                val permissionIntent = PendingIntent.getBroadcast(
                    context,
                    0,
                    Intent(ACTION_USB_PERMISSION),
                    PendingIntent.FLAG_IMMUTABLE
                )
                usbManager.requestPermission(driver.device, permissionIntent)
                return false
            }
            
            val connection = usbManager.openDevice(driver.device)
                ?: throw IOException("Failed to open device")
            
            val port = driver.ports[0]
            
            port.open(connection)
            port.setParameters(
                config.baudRate,
                config.dataBits.value,
                config.stopBits.value,
                config.parity.value
            )
            
            when (config.flowControl) {
                FlowControl.HARDWARE -> {
                    port.setDTR(true)
                    port.setRTS(true)
                }
                FlowControl.SOFTWARE -> {
                    // Software flow control implementation
                }
                FlowControl.NONE -> {}
            }
            
            portMap[config.path] = port
            return true
        } catch (e: Exception) {
            throw IOException("Failed to open port: ${e.message}")
        }
    }

    private fun startIoManager(path: String, port: UsbSerialPort, onDataReceived: (ByteArray) -> Unit) {
        val ioManager = SerialInputOutputManager(port, object : SerialInputOutputManager.Listener {
            override fun onNewData(data: ByteArray) {
                onDataReceived(data)
            }

            override fun onRunError(e: Exception) {
                closePort(path)
            }
        })

        ioManagerMap[path] = ioManager

        // Wrap the ioManager's run method in a Runnable
        executor.submit(Runnable { ioManager.start() })
    }

    fun writeToPort(path: String, data: ByteArray) {
        try {
            portMap[path]?.write(data, 1000) ?: throw IOException("Port not found")
        } catch (e: Exception) {
            throw IOException("Failed to write data: ${e.message}")
        }
    }

    fun closePort(path: String) {
        try {
            ioManagerMap[path]?.stop()
            ioManagerMap.remove(path)
            portMap[path]?.close()
            portMap.remove(path)
        } catch (e: Exception) {
            throw IOException("Failed to close port: ${e.message}")
        }
    }

    fun closeAllPorts() {
        val exceptions = mutableListOf<Exception>()
        
        portMap.keys.toList().forEach { path ->
            try {
                closePort(path)
            } catch (e: Exception) {
                exceptions.add(e)
            }
        }
        
        if (exceptions.isNotEmpty()) {
            throw IOException("Failed to close all ports: ${exceptions.joinToString(", ") { it.message ?: "" }}")
        }
    }

    fun setPortParameters(path: String, config: SerialPortConfig): Boolean {
        return try {
            portMap[path]?.let { port ->
                port.setParameters(
                    config.baudRate,
                    config.dataBits.value,
                    config.stopBits.value,
                    config.parity.value
                )
                true
            } ?: false
        } catch (e: Exception) {
            false
        }
    }

   fun readFromPort(path: String, timeout: Int, size: Int?): ByteArray {
       return try {
           val port = portMap[path] ?: throw IOException("Port not found")

           val targetSize = size ?: 1024

           // Получаем рекомендованный размер буфера
           val maxPacketSize = port.getReadEndpoint().getMaxPacketSize()
           val bufferSize = minOf(targetSize, maxPacketSize) // Берем минимум из запрашиваемого размера и maxPacketSize

           val buffer = ByteArray(bufferSize)
           val bytesRead = port.read(buffer, timeout.coerceAtLeast(200)) // Минимальный таймаут 200 мс

           if (bytesRead > 0) {
               buffer.copyOf(bytesRead)
           } else {
               throw IOException("Read timeout: no data received within $timeout ms")
           }
       } catch (e: Exception) {
           throw IOException("Failed to read data: ${e.message}")
       }
   }

   fun readFullyFromPort(path: String, timeout: Int, size: Int?): ByteArray {
       val port = portMap[path] ?: throw IOException("Port not found")
       val buffer = mutableListOf<Byte>()
       val startTime = System.currentTimeMillis()

        val targetSize = size ?: 1024

       // Получаем оптимальный размер пакета
       val maxPacketSize = port.getReadEndpoint().getMaxPacketSize()

       while (buffer.size < targetSize && (System.currentTimeMillis() - startTime) < timeout) {
           val remainingTime = timeout - (System.currentTimeMillis() - startTime).toInt()
           if (remainingTime <= 0) break

           val chunkSize = minOf(targetSize - buffer.size, maxPacketSize) // Читаем не больше maxPacketSize
           val tempBuffer = ByteArray(chunkSize)
           val bytesRead = port.read(tempBuffer, remainingTime.coerceAtLeast(200))

           if (bytesRead > 0) {
               buffer.addAll(tempBuffer.copyOf(bytesRead).toList())
           } else {
               throw IOException("Read timeout: no data received within $timeout ms")
           }
       }

       return if (buffer.isEmpty()) {
           throw IOException("Read timeout: no data received within $timeout ms")
       } else {
           buffer.toByteArray()
       }
   }

    fun setBaudRate(path: String, baudRate: Int): Boolean {
        return try {
            Log.d("setBaudRate", path)
            false
            //portMap[path]?.setBaudRate(baudRate) ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun setDataBits(path: String, dataBits: DataBits): Boolean {
        return try {
            Log.d("setDataBits", path)
            false
            //portMap[path]?.setDataBits(dataBits.value) ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun setFlowControl(path: String, flowControl: FlowControl): Boolean {
        return try {
            when (flowControl) {
                FlowControl.HARDWARE -> {
                    portMap[path]?.setDTR(true)
                    portMap[path]?.setRTS(true)
                }
                FlowControl.SOFTWARE -> {
                    // Software flow control implementation
                }
                FlowControl.NONE -> {}
            }
            true
        } catch (e: Exception) {
            false
        }
    }

    fun setParity(path: String, parity: Parity): Boolean {
        return try {
            Log.d("setParity", path)
            false
            //portMap[path]?.setParity(parity.value) ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun setStopBits(path: String, stopBits: StopBits): Boolean {
        return try {
            Log.d("setStopBits", path)
            false
            //portMap[path]?.setStopBits(stopBits.value) ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun setTimeout(path: String, timeout: Int): Boolean {
        return try {
            Log.d("setTimeout", path)
            false
            //portMap[path]?.setReadTimeout(timeout) ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun writeRequestToSend(path: String, level: Boolean): Boolean {
        return try {
            portMap[path]?.setRTS(level)
            true
        } catch (e: Exception) {
            false
        }
    }

    fun writeDataTerminalReady(path: String, level: Boolean): Boolean {
        return try {
            portMap[path]?.setDTR(level)
            true
        } catch (e: Exception) {
            false
        }
    }

    fun readClearToSend(path: String): Boolean {
        return try {
            portMap[path]?.getCTS() ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun readDataSetReady(path: String): Boolean {
        return try {
            portMap[path]?.getDSR() ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun readRingIndicator(path: String): Boolean {
        return try {
            portMap[path]?.getRI() ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun readCarrierDetect(path: String): Boolean {
        return try {
            portMap[path]?.getCD() ?: false
        } catch (e: Exception) {
            false
        }
    }

    fun bytesToRead(path: String): Int {
        return try {
            Log.d("bytesToRead", path)
            //portMap[path]?.bytesAvailable() ?: 0
            0
        } catch (e: Exception) {
            0
        }
    }

    fun bytesToWrite(path: String): Int {
        return try {
            Log.d("bytesToWrite", path)
            //portMap[path]?.bytesToWrite() ?: 0
            0
        } catch (e: Exception) {
            0
        }
    }

    fun clearBuffer(path: String, bufferType: String): Boolean {
        return try {
            Log.d("clearBuffer", path)
            Log.d("clearBuffer", bufferType)
            //when (bufferType) {
            //    "input" -> portMap[path]?.clearInputBuffer()
            //    "output" -> portMap[path]?.clearOutputBuffer()
            //    else -> throw IOException("Invalid buffer type")
            //}
            false
        } catch (e: Exception) {
            false
        }
    }

    fun setBreak(path: String): Boolean {
        return try {
            portMap[path]?.setBreak(true)
            true
        } catch (e: Exception) {
            false
        }
    }

    fun clearBreak(path: String): Boolean {
        return try {
            portMap[path]?.setBreak(false)
            true
        } catch (e: Exception) {
            false
        }
    }

    fun startListening(path: String, onDataReceived: (ByteArray) -> Unit) {
        val port = portMap[path] ?: throw IOException("Port not found")
        startIoManager(path, port, onDataReceived)
    }

    fun stopListening(path: String) {
        ioManagerMap[path]?.stop()
        ioManagerMap.remove(path)
    }
}