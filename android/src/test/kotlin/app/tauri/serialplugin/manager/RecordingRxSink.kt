package app.tauri.serialplugin.manager

import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.TimeUnit

/** In-memory [SerialRxSink] for JVM unit tests. */
class RecordingRxSink : SerialRxSink {
    data class RxEvent(val path: String, val data: ByteArray)
    data class ErrorEvent(val path: String, val reason: String)

    val rx = ConcurrentLinkedQueue<RxEvent>()
    val errors = ConcurrentLinkedQueue<ErrorEvent>()
    @Volatile var portListChanges: Int = 0

    override fun feedRx(path: String, data: ByteArray) {
        rx.add(RxEvent(path, data.copyOf()))
    }

    override fun onUsbError(path: String, reason: String) {
        errors.add(ErrorEvent(path, reason))
    }

    override fun onPortListChange() {
        portListChanges++
    }

    fun awaitRx(path: String, timeoutMs: Long = 3000): ByteArray? {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            rx.forEach { event ->
                if (event.path == path) {
                    rx.remove(event)
                    return event.data
                }
            }
            Thread.sleep(10)
        }
        return null
    }

    fun drainRx(path: String): List<ByteArray> {
        val out = mutableListOf<ByteArray>()
        val it = rx.iterator()
        while (it.hasNext()) {
            val event = it.next()
            if (event.path == path) {
                out.add(event.data)
                it.remove()
            }
        }
        return out
    }

    fun clear() {
        rx.clear()
        errors.clear()
        portListChanges = 0
    }
}
