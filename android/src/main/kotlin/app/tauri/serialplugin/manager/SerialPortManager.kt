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
import com.hoho.android.usbserial.driver.ProbeTable
import app.tauri.serialplugin.models.*
import java.util.concurrent.Executors
import java.io.IOException
import android.util.Log
import androidx.core.content.ContextCompat
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

data class ManagedPort (
    val port: UsbSerialPort,
    val config: SerialPortConfig
)

class SerialPortManager(private val context: Context) {
    private val usbManager: UsbManager = context.getSystemService(Context.USB_SERVICE) as UsbManager
    private val portMap = mutableMapOf<String, ManagedPort>()
    private val ioManagerMap = mutableMapOf<String, SerialInputOutputManager>()
    private val executor = Executors.newCachedThreadPool()
    private val permissionFutures = mutableMapOf<String, CompletableFuture<Boolean>>()
    
    private val ACTION_USB_PERMISSION = "app.tauri.serialplugin.USB_PERMISSION"
    
    // Custom prober for unknown devices (только для кастомных VID/PID)
    private val customProber: UsbSerialProber by lazy {
        val customTable = ProbeTable()
        
        // Добавляем только устройства с кастомными VID/PID, которые не поддерживаются по умолчанию
        // Например, если у вас есть устройство с VID=0x1234 и PID=0x0001, которое совместимо с FTDI
        // customTable.addProduct(0x1234, 0x0001, FtdiSerialDriver::class.java)
        
        UsbSerialProber(customTable)
    }

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (ACTION_USB_PERMISSION == intent.action) {
                synchronized(this) {
                    val device: UsbDevice? = if (Build.VERSION.SDK_INT >= 33) {
                        intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
                    } else {
                        @Suppress("DEPRECATION")
                        intent.getParcelableExtra(UsbManager.EXTRA_DEVICE) as UsbDevice?
                    }
                    
                    val permissionGranted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
                    val deviceName = device?.deviceName
                    
                    Log.d("SerialPortManager", "USB permission result for $deviceName: $permissionGranted")
                    
                    deviceName?.let { name ->
                        permissionFutures[name]?.complete(permissionGranted)
                        permissionFutures.remove(name)
                    }
                }
            }
        }
    }

    fun registerReceiver() {
        val filter = IntentFilter(ACTION_USB_PERMISSION)

        if (Build.VERSION.SDK_INT >= 33) {
            context.registerReceiver(usbReceiver, filter, Context.RECEIVER_EXPORTED)
        } else {
            ContextCompat.registerReceiver(
                context,
                usbReceiver,
                filter,
                ContextCompat.RECEIVER_NOT_EXPORTED
            )
        }
    }

    fun unregisterReceiver() {
        try {
            context.unregisterReceiver(usbReceiver)
        } catch (_: IllegalArgumentException) {
            Log.w("SerialPortManager", "Receiver not registered")
        }
    }
    
    init {
        registerReceiver()
    }

    fun getAvailablePorts(): Map<String, Map<String, String>> {
        val result = mutableMapOf<String, Map<String, String>>()
        
        try {
            // Use default prober first
            val availableDrivers = UsbSerialProber.getDefaultProber().findAllDrivers(usbManager)
            Log.d("SerialPortManager", "Available drivers (default prober): ${availableDrivers.size}")

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

            // Also check for custom prober devices
            val customDrivers = customProber.findAllDrivers(usbManager)
            Log.d("SerialPortManager", "Available drivers (custom prober): ${customDrivers.size}")

            customDrivers.forEach { driver ->
                val device = driver.device
                if (!result.containsKey(device.deviceName)) {
                    Log.d("SerialPortManager", "Found custom device: ${device.deviceName}, Vendor ID: ${device.vendorId}, Product ID: ${device.productId}")

                    result[device.deviceName] = mapOf(
                        "type" to "USB (Custom)",
                        "vid" to device.vendorId.toString(),
                        "pid" to device.productId.toString(),
                        "manufacturer" to (device.manufacturerName ?: "Unknown"),
                        "product" to (device.productName ?: "Unknown"),
                        "serial_number" to (device.serialNumber ?: "Unknown")
                    )
                }
            }
            
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Error getting available ports: ${e.message}", e)
        }

        Log.d("SerialPortManager", "Total available ports: ${result.size}")
        return result
    }

    fun getManagedPorts(): List<String> {
        return portMap.keys.toList()
    }

    fun openPort(config: SerialPortConfig): Boolean {
        try {
            Log.d("SerialPortManager", "Opening port: ${config.path}")
            
            // Find the device by name
            val device = findDeviceByName(config.path)
                ?: throw IOException("Device not found: ${config.path}")
            
            // Probe for driver using default prober first
            var driver = UsbSerialProber.getDefaultProber().probeDevice(device)
            
            // If no driver found, try custom prober
            if (driver == null) {
                driver = customProber.probeDevice(device)
                Log.d("SerialPortManager", "Device found via custom prober: ${device.deviceName}")
            }
            
            if (driver == null) {
                throw IOException("No driver found for device: ${config.path}")
            }
            
            // Check permissions
            if (!usbManager.hasPermission(device)) {
                Log.d("SerialPortManager", "Requesting USB permission for device: ${device.deviceName}")
                
                val permissionFuture = CompletableFuture<Boolean>()
                permissionFutures[device.deviceName] = permissionFuture
                
                val flags =
                    PendingIntent.FLAG_IMMUTABLE

                val permissionIntent = PendingIntent.getBroadcast(
                    context,
                    0,
                    Intent(ACTION_USB_PERMISSION),
                    flags
                )
                usbManager.requestPermission(device, permissionIntent)
                
                // Wait for permission result with timeout
                val permissionGranted = permissionFuture.get(10, TimeUnit.SECONDS)
                if (!permissionGranted) {
                    throw IOException("USB permission denied for device: ${config.path}")
                }
            }
            
            // Open connection
            val connection = usbManager.openDevice(device)
                ?: throw IOException("Failed to open device: ${config.path}")
            
            // Get port (most devices have just one port)
            val port = driver.ports[0]
            
            // Open port
            port.open(connection)
            Log.d("SerialPortManager", "Setting port parameters: baudRate=${config.baudRate}, dataBits=${config.dataBits.value}, stopBits=${config.stopBits.value}, parity=${config.parity.value}")
            
            try {
                port.setParameters(
                    config.baudRate,
                    config.dataBits.value,
                    config.stopBits.value,
                    config.parity.value
                )
                Log.d("SerialPortManager", "Port parameters set successfully")
            } catch (_: UnsupportedOperationException) {
                Log.w("SerialPortManager", "setParameters not supported for this device, using default settings")
                // Some devices don't support parameter changes, continue with defaults
            } catch (e: Exception) {
                Log.w("SerialPortManager", "Failed to set parameters: ${e.message}, using default settings")
                // Continue with default parameters
            }
            
            // Handle flow control
            when (config.flowControl) {
                FlowControl.HARDWARE -> {
                    Log.d("SerialPortManager", "Enabling hardware flow control")
                    try {
                        port.dtr = true
                        port.rts = true
                        Log.d("SerialPortManager", "Hardware flow control enabled successfully")
                    } catch (_: UnsupportedOperationException) {
                        Log.w("SerialPortManager", "Hardware flow control not supported by this device")
                    } catch (e: Exception) {
                        Log.w("SerialPortManager", "Failed to enable hardware flow control: ${e.message}")
                    }
                }
                FlowControl.SOFTWARE -> {
                    Log.d("SerialPortManager", "Software flow control not implemented in this library")
                }
                FlowControl.NONE -> {
                    Log.d("SerialPortManager", "No flow control - using default settings")
                }
            }
            
            portMap[config.path] = ManagedPort(port, config)
            Log.d("SerialPortManager", "Port opened successfully: ${config.path}")
            return true
            
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to open port: ${e.message}", e)
            throw IOException("Failed to open port: ${e.message}")
        }
    }

    private fun findDeviceByName(deviceName: String): UsbDevice? {
        return usbManager.deviceList.values.find { it.deviceName == deviceName }
    }

    private fun startIoManager(path: String, port: UsbSerialPort, onDataReceived: (ByteArray) -> Unit) {
        val ioManager = SerialInputOutputManager(port, object : SerialInputOutputManager.Listener {
            override fun onNewData(data: ByteArray) {
                try {
                    Log.d("SerialPortManager", "Data received on $path: ${data.size} bytes")
                    onDataReceived(data)
                } catch (e: Exception) {
                    Log.e("SerialPortManager", "Error in data callback for $path: ${e.message}", e)
                }
            }

            override fun onRunError(e: Exception) {
                Log.e("SerialPortManager", "IO Manager error for $path: ${e.message}", e)
                
                // Try to recover from certain errors
                when (e) {
                    is IOException -> {
                        Log.w("SerialPortManager", "IO error on $path, attempting to close port")
                        closePort(path)
                    }
                    is IllegalStateException -> {
                        Log.w("SerialPortManager", "Illegal state on $path, attempting to close port")
                        closePort(path)
                    }
                    else -> {
                        Log.e("SerialPortManager", "Unknown error on $path, closing port")
                        closePort(path)
                    }
                }
            }
        })

        ioManagerMap[path] = ioManager
        
        try {
            executor.submit {
                try {
                    ioManager.start()
                    Log.d("SerialPortManager", "IO Manager started successfully for $path")
                } catch (e: Exception) {
                    Log.e(
                        "SerialPortManager",
                        "Failed to start IO Manager for $path: ${e.message}",
                        e
                    )
                    closePort(path)
                }
            }
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to submit IO Manager task for $path: ${e.message}", e)
            closePort(path)
        }
    }

    fun writeToPort(path: String, data: ByteArray): Int {
        try {
            val port = portMap[path] ?: throw IOException("Port not found")
            
            Log.d("SerialPortManager", "Writing to port $path: ${data.size} bytes")
            
            port.port.write(data, 1000) // 1 second timeout
            val bytesWritten = data.size

            return bytesWritten
        } catch (e: IOException) {
            Log.e("SerialPortManager", "Write failed: ${e.message}")
            throw e
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Unexpected error during write: ${e.message}", e)
            throw IOException("Failed to write data: ${e.message}")
        }
    }

    fun closePort(path: String) {
        try {
            ioManagerMap[path]?.stop()
            ioManagerMap.remove(path)
            portMap[path]?.port?.close()
            portMap.remove(path)
            Log.d("SerialPortManager", "Port closed: $path")
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to close port $path: ${e.message}", e)
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
                port.port.setParameters(
                    config.baudRate,
                    config.dataBits.value,
                    config.stopBits.value,
                    config.parity.value
                )
                true
            } ?: false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set port parameters: ${e.message}", e)
            false
        }
    }

   fun readFromPort(path: String, timeout: Int, size: Int?): ByteArray {
       return try {
           val port = portMap[path] ?: throw IOException("Port not found")

           val targetSize = size ?: 1024
           val maxPacketSize = port.port.readEndpoint.maxPacketSize
           val bufferSize = minOf(targetSize, maxPacketSize)

           val buffer = ByteArray(bufferSize)
           val adjustedTimeout = timeout.coerceAtLeast(200) // Minimum 200ms timeout
           
           Log.d("SerialPortManager", "Reading from port $path: bufferSize=$bufferSize, timeout=$adjustedTimeout")
           
           val bytesRead = port.port.read(buffer, adjustedTimeout)

           if (bytesRead > 0) {
               Log.d("SerialPortManager", "Read successful: $bytesRead bytes")
               buffer.copyOf(bytesRead)
           } else {
               Log.w("SerialPortManager", "Read timeout: no data received within $adjustedTimeout ms")
               throw IOException("Read timeout: no data received within $adjustedTimeout ms")
           }
       } catch (e: IOException) {
           Log.e("SerialPortManager", "Read failed: ${e.message}")
           throw e
       } catch (e: Exception) {
           Log.e("SerialPortManager", "Unexpected error during read: ${e.message}", e)
           throw IOException("Failed to read data: ${e.message}")
       }
   }

   fun readFullyFromPort(path: String, timeout: Int, size: Int?): ByteArray {
       val port = portMap[path] ?: throw IOException("Port not found")
       val buffer = mutableListOf<Byte>()
       val startTime = System.currentTimeMillis()

        val targetSize = size ?: 1024
       val maxPacketSize = port.port.readEndpoint.maxPacketSize

       while (buffer.size < targetSize && (System.currentTimeMillis() - startTime) < timeout) {
           val remainingTime = timeout - (System.currentTimeMillis() - startTime).toInt()
           if (remainingTime <= 0) break

           val chunkSize = minOf(targetSize - buffer.size, maxPacketSize)
           val tempBuffer = ByteArray(chunkSize)
           val bytesRead = port.port.read(tempBuffer, remainingTime.coerceAtLeast(200))

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
            val port = portMap[path] ?: return false
            port.config.baudRate = baudRate
            port.port.setParameters(port.config.baudRate, port.config.dataBits.value, port.config.stopBits.value, port.config.parity.value)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set baud rate: ${e.message}", e)
            false
        }
    }

    fun setDataBits(path: String, dataBits: DataBits): Boolean {
        return try {
            val port = portMap[path] ?: return false
            port.config.dataBits = dataBits
            port.port.setParameters(port.config.baudRate, port.config.dataBits.value, port.config.stopBits.value, port.config.parity.value)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set data bits: ${e.message}", e)
            false
        }
    }

    fun setFlowControl(path: String, flowControl: FlowControl): Boolean {
        return try {
            when (flowControl) {
                FlowControl.HARDWARE -> {
                    portMap[path]?.port?.dtr = true
                    portMap[path]?.port?.rts = true
                }
                FlowControl.SOFTWARE -> {
                    // Software flow control implementation
                }
                FlowControl.NONE -> {}
            }
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set flow control: ${e.message}", e)
            false
        }
    }

    fun setParity(path: String, parity: Parity): Boolean {
        return try {
            val port = portMap[path] ?: return false
            port.config.parity = parity
            port.port.setParameters(port.config.baudRate, port.config.dataBits.value, port.config.stopBits.value, port.config.parity.value)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set parity: ${e.message}", e)
            false
        }
    }

    fun setStopBits(path: String, stopBits: StopBits): Boolean {
        return try {
            val port = portMap[path] ?: return false
            port.config.stopBits = stopBits
            port.port.setParameters(port.config.baudRate, port.config.dataBits.value, port.config.stopBits.value, port.config.parity.value)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set stop bits: ${e.message}", e)
            false
        }
    }

    fun setTimeout(path: String, timeout: Int): Boolean {
        return try {
            // Note: UsbSerialPort doesn't have a direct timeout setter
            // The timeout is used in read operations
            Log.d("SerialPortManager", "Timeout set to $timeout ms for port $path")
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set timeout: ${e.message}", e)
            false
        }
    }

    fun writeRequestToSend(path: String, level: Boolean): Boolean {
        return try {
            portMap[path]?.port?.rts = level
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set RTS: ${e.message}", e)
            false
        }
    }

    fun writeDataTerminalReady(path: String, level: Boolean): Boolean {
        return try {
            portMap[path]?.port?.dtr = level
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set DTR: ${e.message}", e)
            false
        }
    }

    fun readClearToSend(path: String): Boolean {
        return try {
            portMap[path]?.port?.cts ?: false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to read CTS: ${e.message}", e)
            false
        }
    }

    fun readDataSetReady(path: String): Boolean {
        return try {
            portMap[path]?.port?.dsr ?: false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to read DSR: ${e.message}", e)
            false
        }
    }

    fun readRingIndicator(path: String): Boolean {
        return try {
            portMap[path]?.port?.ri ?: false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to read RI: ${e.message}", e)
            false
        }
    }

    fun readCarrierDetect(path: String): Boolean {
        return try {
            portMap[path]?.port?.cd ?: false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to read CD: ${e.message}", e)
            false
        }
    }

    fun bytesToRead(path: String): Int {
        return try {
            // Note: UsbSerialPort doesn't provide bytesAvailable method
            // Return 0 as fallback
            0
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to get bytes to read: ${e.message}", e)
            0
        }
    }

    fun bytesToWrite(path: String): Int {
        return try {
            // Note: UsbSerialPort doesn't provide bytesToWrite method
            // Return 0 as fallback
            0
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to get bytes to write: ${e.message}", e)
            0
        }
    }

    fun clearBuffer(path: String, bufferType: String): Boolean {
        return try {
            // Note: UsbSerialPort doesn't provide buffer clearing methods
            // Return false as fallback
            Log.d("SerialPortManager", "Buffer clearing not supported for USB serial ports")
            false
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to clear buffer: ${e.message}", e)
            false
        }
    }

    fun setBreak(path: String): Boolean {
        return try {
            portMap[path]?.port?.setBreak(true)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to set break: ${e.message}", e)
            false
        }
    }

    fun clearBreak(path: String): Boolean {
        return try {
            portMap[path]?.port?.setBreak(false)
            true
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to clear break: ${e.message}", e)
            false
        }
    }

    fun startListening(path: String, onDataReceived: (ByteArray) -> Unit) {
        val port = portMap[path] ?: throw IOException("Port not found")
        startIoManager(path, port.port, onDataReceived)
    }

    fun stopListening(path: String) {
        try {
            ioManagerMap[path]?.stop()
            ioManagerMap.remove(path)
            Log.d("SerialPortManager", "Stopped listening on port: $path")
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to stop listening: ${e.message}", e)
        }
    }
    
    fun cleanup() {
        try {
            closeAllPorts()
            unregisterReceiver()
            executor.shutdown()
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Error during cleanup: ${e.message}", e)
        }
    }
}
