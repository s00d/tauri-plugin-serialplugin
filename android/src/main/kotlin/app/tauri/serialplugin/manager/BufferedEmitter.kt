package app.tauri.serialplugin.manager

import android.util.Log
import app.tauri.plugin.JSObject
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Coalesces high-frequency [onNewData] chunks and emits at most once per [flushIntervalMs]
 * to reduce WebView/JS pressure (backpressure).
 */
internal class BufferedEmitter(
    private val path: String,
    flushIntervalMs: Long,
    private val emit: (JSObject) -> Unit,
) {
    private val accumulator = SerialByteAccumulator()
    private val scheduler = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "serial-emit-$path").apply { isDaemon = true }
    }
    private val scheduled: ScheduledFuture<*>

    init {
        val interval = flushIntervalMs.coerceIn(10L, 2000L)
        scheduled = scheduler.scheduleAtFixedRate(
            {
                try {
                    flushOnce()
                } catch (e: Exception) {
                    Log.e("BufferedEmitter", "flush: ${e.message}", e)
                }
            },
            interval,
            interval,
            TimeUnit.MILLISECONDS,
        )
    }

    private fun flushOnce() {
        flushAccumulatorToEmit(path, accumulator) { fields ->
            val eventData = JSObject()
            fields.applyToJSObject(eventData)
            emit(eventData)
        }
    }

    fun addData(data: ByteArray) {
        accumulator.append(data)
    }

    fun stop() {
        scheduled.cancel(false)
        scheduler.shutdown()
        try {
            if (!scheduler.awaitTermination(300, TimeUnit.MILLISECONDS)) {
                scheduler.shutdownNow()
            }
        } catch (_: InterruptedException) {
            scheduler.shutdownNow()
        }
        try {
            flushAccumulatorToEmit(path, accumulator) { fields ->
                val eventData = JSObject()
                fields.applyToJSObject(eventData)
                emit(eventData)
            }
        } catch (e: Exception) {
            Log.w("BufferedEmitter", "final flush: ${e.message}")
        }
    }
}
