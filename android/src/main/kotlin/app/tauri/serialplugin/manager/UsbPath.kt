package app.tauri.serialplugin.manager

/** Session path ↔ USB device name + port index (`deviceName#1` for multi-port). */
internal object UsbPath {
    private const val SEP = '#'

    fun sessionKey(deviceName: String, portIndex: Int, portCount: Int): String =
        if (portCount <= 1) deviceName else "$deviceName$SEP$portIndex"

    fun parse(path: String): Pair<String, Int> {
        val idx = path.lastIndexOf(SEP)
        if (idx < 0) return path to 0
        val device = path.substring(0, idx)
        val port = path.substring(idx + 1).toIntOrNull() ?: 0
        return device to port
    }

    fun deviceName(sessionPath: String): String = parse(sessionPath).first
}
