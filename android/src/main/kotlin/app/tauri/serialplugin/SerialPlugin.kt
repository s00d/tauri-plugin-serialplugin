package app.tauri.serialplugin

import android.app.Activity
import android.content.Context
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.serialplugin.manager.SerialPortManager
import app.tauri.serialplugin.models.*
import android.webkit.WebView
import android.util.Log
import java.util.concurrent.ConcurrentHashMap

@InvokeArg
class PortConfigArgs {
    lateinit var path: String
    var baudRate: Int = 9600
    var dataBits: Any? = null
    val size: Int? = null
    var flowControl: Any? = null
    var parity: Any? = null
    var stopBits: Any? = null
    var timeout: Int = 1000
}

@InvokeArg
class WriteArgs {
    lateinit var path: String
    lateinit var value: String
}

@InvokeArg
class CloseArgs {
    lateinit var path: String
}

@TauriPlugin
class SerialPlugin(private val activity: Activity) : Plugin(activity) {
    private var webView: WebView? = null
    private lateinit var serialPortManager: SerialPortManager
    private val listeners = ConcurrentHashMap<String, (ByteArray) -> Unit>()

    override fun load(webView: WebView) {
        super.load(webView)
        serialPortManager = SerialPortManager(activity)
        this.webView = webView
        
        Log.d("SerialPlugin", "SerialPlugin loaded successfully")
    }

    override fun onDetach() {
        try {
            Log.d("SerialPlugin", "SerialPlugin detaching, cleaning up resources")
            serialPortManager.cleanup()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to cleanup: ${e.message}", e)
        }
        super.onDetach()
    }

    @Command
    fun availablePorts(invoke: Invoke) {
        try {
            Log.d("SerialPlugin", "Fetching available ports")
            val ports = serialPortManager.getAvailablePorts()
            Log.d("SerialPlugin", "Available ports fetched successfully: ${ports.size} ports")
            val result = JSObject()
            result.put("ports", ports)
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to get available ports: ${e.message}", e)
            invoke.reject("Failed to get available ports: ${e.message}")
        }
    }

    @Command
    fun managedPorts(invoke: Invoke) {
        try {
            val managedPorts = serialPortManager.getManagedPorts()
            Log.d("SerialPlugin", "Managed ports: ${managedPorts.size} ports")
            val result = JSObject()
            result.put("ports", managedPorts)
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to get managed ports: ${e.message}", e)
            invoke.reject("Failed to get managed ports: ${e.message}")
        }
    }

    @Command
    fun open(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            Log.d("SerialPlugin", "Opening port: ${args.path}")
            
            val dataBits = when (args.dataBits) {
                is String -> DataBits.valueOf(args.dataBits as String)
                is Number -> DataBits.fromValue((args.dataBits as Number).toInt())
                null -> DataBits.EIGHT
                else -> throw IllegalArgumentException("Invalid data bits type")
            }
            
            val flowControl = when (args.flowControl) {
                is String -> FlowControl.valueOf(args.flowControl as String)
                is Number -> FlowControl.fromValue((args.flowControl as Number).toInt())
                null -> FlowControl.NONE
                else -> throw IllegalArgumentException("Invalid flow control type")
            }
            
            val parity = when (args.parity) {
                is String -> Parity.valueOf(args.parity as String)
                is Number -> Parity.fromValue((args.parity as Number).toInt())
                null -> Parity.NONE
                else -> throw IllegalArgumentException("Invalid parity type")
            }
            
            val stopBits = when (args.stopBits) {
                is String -> StopBits.valueOf(args.stopBits as String)
                is Number -> StopBits.fromValue((args.stopBits as Number).toInt())
                null -> StopBits.ONE
                else -> throw IllegalArgumentException("Invalid stop bits type")
            }
            
            val serialConfig = SerialPortConfig(
                path = args.path,
                baudRate = args.baudRate,
                dataBits = dataBits,
                flowControl = flowControl,
                parity = parity,
                stopBits = stopBits,
                timeout = args.timeout
            )

            val success = serialPortManager.openPort(serialConfig)
            if (success) {
                Log.d("SerialPlugin", "Port opened successfully: ${args.path}")
                invoke.resolve()
            } else {
                Log.e("SerialPlugin", "Failed to open port: ${args.path}")
                invoke.reject("Failed to open port")
            }
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to open port: ${e.message}", e)
            invoke.reject("Failed to open port: ${e.message}")
        }
    }

    @Command
    fun write(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(WriteArgs::class.java)
            Log.d("SerialPlugin", "Writing to port: ${args.path}, data: ${args.value}")
            val bytesWritten = serialPortManager.writeToPort(args.path, args.value.toByteArray())
            val result = JSObject()
            result.put("bytesWritten", bytesWritten)
            Log.d("SerialPlugin", "Write successful: $bytesWritten bytes written")
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to write data: ${e.message}", e)
            invoke.reject("Failed to write data: ${e.message}")
        }
    }

    @Command
    fun close(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            Log.d("SerialPlugin", "Closing port: ${args.path}")
            serialPortManager.closePort(args.path)
            Log.d("SerialPlugin", "Port closed successfully: ${args.path}")
            invoke.resolve()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to close port: ${e.message}", e)
            invoke.reject("Failed to close port: ${e.message}")
        }
    }

    @Command
    fun closeAll(invoke: Invoke) {
        try {
            Log.d("SerialPlugin", "Closing all ports")
            serialPortManager.closeAllPorts()
            Log.d("SerialPlugin", "All ports closed successfully")
            invoke.resolve()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to close all ports: ${e.message}", e)
            invoke.reject("Failed to close all ports: ${e.message}")
        }
    }

    @Command
    fun forceClose(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            Log.d("SerialPlugin", "Force closing port: ${args.path}")
            serialPortManager.closePort(args.path)
            Log.d("SerialPlugin", "Port force closed successfully: ${args.path}")
            invoke.resolve()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to force close port: ${e.message}", e)
            invoke.reject("Failed to force close port: ${e.message}")
        }
    }

    @Command
    fun writeBinary(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(WriteArgs::class.java)
            Log.d("SerialPlugin", "Writing binary to port: ${args.path}")
            val bytesWritten = serialPortManager.writeToPort(args.path, args.value.toByteArray())
            val result = JSObject()
            result.put("bytesWritten", bytesWritten)
            Log.d("SerialPlugin", "Binary write successful: $bytesWritten bytes written")
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to write binary data: ${e.message}", e)
            invoke.reject("Failed to write binary data: ${e.message}")
        }
    }

    @Command
    fun read(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            Log.d("SerialPlugin", "Reading from port: ${args.path}")
            val data = serialPortManager.readFromPort(args.path, args.timeout, 1024)
            val result = JSObject()
            result.put("data", String(data))
            Log.d("SerialPlugin", "Read successful: ${data.size} bytes read")
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to read data: ${e.message}", e)
            invoke.reject("Failed to read data: ${e.message}")
        }
    }

    @Command
    fun readBinary(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            Log.d("SerialPlugin", "Reading binary from port: ${args.path}")
            val data = serialPortManager.readFullyFromPort(args.path, args.timeout, args.size)
            val result = JSObject().apply {
                put("data", data.toList())
            }
            Log.d("SerialPlugin", "Binary read successful: ${data.size} bytes read")
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to read binary data: ${e.message}", e)
            invoke.reject("Failed to read binary data: ${e.message}")
        }
    }

    @Command
    fun startListening(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            Log.d("SerialPlugin", "Starting listening on port: ${args.path}")
            
            val listener = { data: ByteArray ->
                try {
                    val eventData = JSObject()
                    eventData.put("path", args.path)
                    eventData.put("data", String(data))
                    eventData.put("size", data.size)

                    Log.d("SerialPlugin", "Data received on ${args.path}: ${data.size} bytes")
                    trigger("serialData", eventData)
                } catch (e: Exception) {
                    Log.e("SerialPlugin", "Error in listener callback: ${e.message}", e)
                }
            }

            listeners[args.path] = listener
            serialPortManager.startListening(args.path, listener)
            Log.d("SerialPlugin", "Listening started successfully on port: ${args.path}")
            invoke.resolve()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to start listening: ${e.message}", e)
            invoke.reject("Failed to start listening: ${e.message}")
        }
    }

    @Command
    fun stopListening(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            Log.d("SerialPlugin", "Stopping listening on port: ${args.path}")
            listeners.remove(args.path)
            serialPortManager.stopListening(args.path)
            Log.d("SerialPlugin", "Listening stopped successfully on port: ${args.path}")
            invoke.resolve()
        } catch (e: Exception) {
            Log.e("SerialPlugin", "Failed to stop listening: ${e.message}", e)
            invoke.reject("Failed to stop listening: ${e.message}")
        }
    }

    @Command
    fun setBaudRate(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val success = serialPortManager.setBaudRate(args.path, args.baudRate)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set baud rate")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set baud rate: ${e.message}")
        }
    }

    @Command
    fun setDataBits(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val dataBits = when (args.dataBits) {
                is String -> DataBits.valueOf(args.dataBits as String)
                is Number -> DataBits.fromValue((args.dataBits as Number).toInt())
                null -> DataBits.EIGHT
                else -> throw IllegalArgumentException("Invalid data bits type")
            }
            val success = serialPortManager.setDataBits(args.path, dataBits)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set data bits")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set data bits: ${e.message}")
        }
    }

    @Command
    fun setFlowControl(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val flowControl = when (args.flowControl) {
                is String -> FlowControl.valueOf(args.flowControl as String)
                is Number -> FlowControl.fromValue((args.flowControl as Number).toInt())
                null -> FlowControl.NONE
                else -> throw IllegalArgumentException("Invalid flow control type")
            }
            val success = serialPortManager.setFlowControl(args.path, flowControl)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set flow control")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set flow control: ${e.message}")
        }
    }

    @Command
    fun setParity(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val parity = when (args.parity) {
                is String -> Parity.valueOf(args.parity as String)
                is Number -> Parity.fromValue((args.parity as Number).toInt())
                null -> Parity.NONE
                else -> throw IllegalArgumentException("Invalid parity type")
            }
            val success = serialPortManager.setParity(args.path, parity)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set parity")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set parity: ${e.message}")
        }
    }

    @Command
    fun setStopBits(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val stopBits = when (args.stopBits) {
                is String -> StopBits.valueOf(args.stopBits as String)
                is Number -> StopBits.fromValue((args.stopBits as Number).toInt())
                null -> StopBits.ONE
                else -> throw IllegalArgumentException("Invalid stop bits type")
            }
            val success = serialPortManager.setStopBits(args.path, stopBits)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set stop bits")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set stop bits: ${e.message}")
        }
    }

    @Command
    fun setTimeout(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val success = serialPortManager.setTimeout(args.path, args.timeout)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set timeout")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set timeout: ${e.message}")
        }
    }

    @Command
    fun writeRequestToSend(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val success = serialPortManager.writeRequestToSend(args.path, args.flowControl == "HARDWARE")
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set RTS")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set RTS: ${e.message}")
        }
    }

    @Command
    fun writeDataTerminalReady(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val success = serialPortManager.writeDataTerminalReady(args.path, args.flowControl == "HARDWARE")
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set DTR")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set DTR: ${e.message}")
        }
    }

    @Command
    fun readClearToSend(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val state = serialPortManager.readClearToSend(args.path)
            val result = JSObject()
            result.put("state", state)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read CTS: ${e.message}")
        }
    }

    @Command
    fun readDataSetReady(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val state = serialPortManager.readDataSetReady(args.path)
            val result = JSObject()
            result.put("state", state)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read DSR: ${e.message}")
        }
    }

    @Command
    fun readRingIndicator(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val state = serialPortManager.readRingIndicator(args.path)
            val result = JSObject()
            result.put("state", state)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read RI: ${e.message}")
        }
    }

    @Command
    fun readCarrierDetect(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val state = serialPortManager.readCarrierDetect(args.path)
            val result = JSObject()
            result.put("state", state)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read CD: ${e.message}")
        }
    }

    @Command
    fun bytesToRead(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val bytes = serialPortManager.bytesToRead(args.path)
            val result = JSObject()
            result.put("bytes", bytes)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get bytes to read: ${e.message}")
        }
    }

    @Command
    fun bytesToWrite(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val bytes = serialPortManager.bytesToWrite(args.path)
            val result = JSObject()
            result.put("bytes", bytes)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to get bytes to write: ${e.message}")
        }
    }

    @Command
    fun clearBuffer(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val bufferType = when (args.dataBits) {
                is String -> ClearBuffer.fromValue(args.dataBits as String)
                is Number -> ClearBuffer.INPUT // By default we use INPUT for numeric values
                null -> ClearBuffer.INPUT
                else -> throw IllegalArgumentException("Invalid buffer type")
            }
            val success = serialPortManager.clearBuffer(args.path, bufferType.name.lowercase())
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to clear buffer")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to clear buffer: ${e.message}")
        }
    }

    @Command
    fun setBreak(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val success = serialPortManager.setBreak(args.path)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to set break")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to set break: ${e.message}")
        }
    }

    @Command
    fun clearBreak(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val success = serialPortManager.clearBreak(args.path)
            if (success) {
                invoke.resolve()
            } else {
                invoke.reject("Failed to clear break")
            }
        } catch (e: Exception) {
            invoke.reject("Failed to clear break: ${e.message}")
        }
    }
}
