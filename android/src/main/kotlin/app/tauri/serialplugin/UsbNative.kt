package app.tauri.serialplugin

import app.tauri.serialplugin.manager.UsbBridge
import app.tauri.serialplugin.models.ClearBuffer
import app.tauri.serialplugin.models.DataBits
import app.tauri.serialplugin.models.FlowControl
import app.tauri.serialplugin.models.Parity
import app.tauri.serialplugin.models.SerialPortConfig
import app.tauri.serialplugin.models.StopBits
import org.json.JSONObject

/** Static JNI facade: Rust calls these instead of Tauri @Command handlers. */
object UsbNative {
    @Volatile
    private var bridge: UsbBridge? = null

    @JvmStatic
    fun bind(usb: UsbBridge) {
        bridge = usb
        nativeInit()
    }

    @JvmStatic
    private external fun nativeInit()

    private fun usb(): UsbBridge =
        bridge ?: throw IllegalStateException("UsbNative not bound")

    @JvmStatic
    fun enumerateJson(): String = usb().runOnIoSync {
        val ports = JSONObject()
        usb().enumerate().forEach { (path, info) ->
            ports.put(path, JSONObject(info))
        }
        JSONObject().put("ports", ports).toString()
    }

    @JvmStatic
    fun open(
        path: String,
        baudRate: Int,
        dataBits: Int,
        flowControl: Int,
        parity: Int,
        stopBits: Int,
        timeout: Int,
    ): String = usb().runOnIoSync {
        usb().open(
            SerialPortConfig(
                path = path,
                baudRate = baudRate,
                dataBits = DataBits.fromValue(dataBits),
                flowControl = FlowControl.fromValue(flowControl),
                parity = Parity.fromValue(parity),
                stopBits = StopBits.fromValue(stopBits),
                timeout = timeout,
            ),
        )
    }

    @JvmStatic
    fun close(path: String?) {
        usb().runOnIoSync { usb().close(path) }
    }

    @JvmStatic
    fun write(path: String, data: ByteArray): Int =
        usb().runOnIoSync { usb().write(path, data) }

    @JvmStatic
    fun ctl(
        path: String,
        op: String,
        baudRate: Int,
        timeout: Int,
        dataBits: Int,
        flowControl: Int,
        parity: Int,
        stopBits: Int,
        bufferType: Int,
    ): Boolean = usb().runOnIoSync {
        val buf = ClearBuffer.fromValue(bufferType).name.lowercase()
        when (val r = usb().ctl(
            path,
            op,
            mapOf(
                "baudRate" to baudRate,
                "timeout" to timeout,
                "dataBits" to dataBits,
                "flowControl" to flowControl,
                "parity" to parity,
                "stopBits" to stopBits,
                "bufferType" to buf,
            ),
        )) {
            is Boolean -> r
            is Number -> true
            else -> false
        }
    }

    @JvmStatic
    fun ctlBytesToWrite(path: String): Int = usb().runOnIoSync {
        when (val r = usb().ctl(path, "bytesToWrite", emptyMap())) {
            is Number -> r.toInt()
            else -> 0
        }
    }

    @JvmStatic
    fun signal(path: String, op: String, level: Boolean): Boolean =
        usb().runOnIoSync { usb().signal(path, op, level) }
}
