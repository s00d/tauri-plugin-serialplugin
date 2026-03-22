package app.tauri.serialplugin.manager

import java.io.ByteArrayOutputStream

/**
 * Thread-safe byte buffer for coalescing serial chunks before emitting to JS.
 * Pure JVM logic — unit-testable without Robolectric.
 */
internal class SerialByteAccumulator {
    private val lock = Any()
    private val stream = ByteArrayOutputStream()

    fun append(data: ByteArray) {
        if (data.isEmpty()) return
        synchronized(lock) {
            stream.write(data)
        }
    }

    /**
     * Returns all accumulated bytes and clears the buffer.
     * Returns an empty array if nothing was pending.
     */
    fun drain(): ByteArray {
        synchronized(lock) {
            if (stream.size() == 0) return ByteArray(0)
            val b = stream.toByteArray()
            stream.reset()
            return b
        }
    }

    fun pendingByteCount(): Int = synchronized(lock) { stream.size() }
}
