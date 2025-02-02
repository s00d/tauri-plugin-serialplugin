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
    var dataBits: String? = null
    val size: Int? = null
    var flowControl: String? = null
    var parity: String? = null
    var stopBits: String? = null
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
    }

    @Command
    fun availablePorts(invoke: Invoke) {
        try {
            Log.d("SerialPortManager", "Fetching available ports")
            val ports = serialPortManager.getAvailablePorts()
            Log.d("SerialPortManager", "Available ports fetched successfully")

            val result = JSObject()
            val portsObject = JSObject()

            for ((portName, portInfo) in ports) {
                val portInfoObject = JSObject()
                portInfoObject.put("type", portInfo["type"])
                portInfoObject.put("vid", portInfo["vid"])
                portInfoObject.put("pid", portInfo["pid"])
                portInfoObject.put("manufacturer", portInfo["manufacturer"])
                portInfoObject.put("product", portInfo["product"])
                portInfoObject.put("serial_number", portInfo["serial_number"])

                portsObject.put(portName, portInfoObject)
            }

            result.put("ports", portsObject)

            Log.d("SerialPortManager", "Resolving invoke with result: $result")
            invoke.resolve(result)
        } catch (e: Exception) {
            Log.e("SerialPortManager", "Failed to list ports: ${e.message}", e)
            invoke.reject("Failed to list ports: ${e.message}")
        }
    }

    @Command
    fun managedPorts(invoke: Invoke) {
        try {
            val managedPorts = serialPortManager.getManagedPorts()
            val result = JSObject()
            result.put("ports", managedPorts)
            invoke.resolve(result)
        } catch (e: Exception) {
            // В случае ошибки возвращаем сообщение об ошибке
            invoke.reject("Failed to get managed ports: ${e.message}")
        }
    }

    @Command
    fun open(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val serialConfig = SerialPortConfig(
                path = args.path,
                baudRate = args.baudRate,
                dataBits = args.dataBits?.let { DataBits.valueOf(it) } ?: DataBits.EIGHT,
                flowControl = args.flowControl?.let { FlowControl.valueOf(it) } ?: FlowControl.NONE,
                parity = args.parity?.let { Parity.valueOf(it) } ?: Parity.NONE,
                stopBits = args.stopBits?.let { StopBits.valueOf(it) } ?: StopBits.ONE,
                timeout = args.timeout
            )

            serialPortManager.openPort(serialConfig)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Failed to open port: ${e.message}")
        }
    }

    @Command
    fun write(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(WriteArgs::class.java)
            val bytesWritten = serialPortManager.writeToPort(args.path, args.value.toByteArray())
            val result = JSObject()
            result.put("bytesWritten", bytesWritten)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to write data: ${e.message}")
        }
    }

    @Command
    fun close(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            serialPortManager.closePort(args.path)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Failed to close port: ${e.message}")
        }
    }


    @Command
    fun closeAll(invoke: Invoke) {
        try {
            serialPortManager.closeAllPorts()
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Failed to close all ports: ${e.message}")
        }
    }

    @Command
    fun forceClose(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            serialPortManager.closePort(args.path)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Failed to force close port: ${e.message}")
        }
    }

    @Command
    fun writeBinary(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(WriteArgs::class.java)
            val bytesWritten = serialPortManager.writeToPort(args.path, args.value.toByteArray())
            val result = JSObject()
            result.put("bytesWritten", bytesWritten)
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to write binary data: ${e.message}")
        }
    }

    @Command
    fun read(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val data = serialPortManager.readFromPort(args.path, args.timeout, 1024)
            val result = JSObject()
            result.put("data", String(data))
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read data: ${e.message}")
        }
    }

    @Command
    fun readBinary(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(PortConfigArgs::class.java)
            val data = serialPortManager.readFullyFromPort(args.path, args.timeout, args.size)

            val result = JSObject().apply {
                put("data", data.toList())
            }
            invoke.resolve(result)
        } catch (e: Exception) {
            invoke.reject("Failed to read binary data: ${e.message}")
        }
    }

    @Command
    fun startListening(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            val listener = { data: ByteArray ->
                val eventData = JSObject()
                eventData.put("path", args.path)
                eventData.put("data", String(data))
                eventData.put("size", data.size)

                trigger("serialData", eventData)
            }

            listeners[args.path] = listener
            serialPortManager.startListening(args.path, listener)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("Failed to start listening: ${e.message}")
        }
    }

    @Command
    fun stopListening(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(CloseArgs::class.java)
            listeners.remove(args.path)
            serialPortManager.stopListening(args.path)
            invoke.resolve()
        } catch (e: Exception) {
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
            val success = serialPortManager.setDataBits(args.path, DataBits.valueOf(args.dataBits ?: "EIGHT"))
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
            val success = serialPortManager.setFlowControl(args.path, FlowControl.valueOf(args.flowControl ?: "NONE"))
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
            val success = serialPortManager.setParity(args.path, Parity.valueOf(args.parity ?: "NONE"))
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
            val success = serialPortManager.setStopBits(args.path, StopBits.valueOf(args.stopBits ?: "ONE"))
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
            val success = serialPortManager.clearBuffer(args.path, args.dataBits ?: "input")
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

    //override fun onDetach() {
    //    try {
    //        serialPortManager.closeAllPorts()
    //    } catch (e: Exception) {
    //        Log.e("SerialPortManager", "Failed to close all ports: ${e.message}", e)
    //    }
    //    super.onDetach()
    //}
}
