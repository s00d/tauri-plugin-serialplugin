package app.tauri.serialplugin.manager

import java.io.ByteArrayOutputStream
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * Merges rapid SIOM RX chunks before JNI (SimpleUsbTerminal SerialService pattern).
 * Disabled in test mode ([immediate] = true).
 */
internal class CoalescingRxSink(
    private val delegate: SerialRxSink,
    private val immediate: Boolean,
) : SerialRxSink {
    private val buffers = ConcurrentHashMap<String, ByteArrayOutputStream>()
    private val flushTasks = ConcurrentHashMap<String, ScheduledFuture<*>>()
    private val scheduler = Executors.newSingleThreadScheduledExecutor { r ->
        Thread(r, "usb-rx-coalesce").apply { isDaemon = true }
    }

    override fun feedRx(path: String, data: ByteArray) {
        if (immediate || data.isEmpty()) {
            if (data.isNotEmpty()) delegate.feedRx(path, data)
            return
        }
        val buf = buffers.computeIfAbsent(path) { ByteArrayOutputStream() }
        val flushNow: Boolean
        synchronized(buf) {
            buf.write(data)
            flushNow = buf.size() >= MAX_BATCH
        }
        if (flushNow) {
            cancelFlush(path)
            flush(path)
        } else {
            scheduleFlush(path)
        }
    }

    override fun onUsbError(path: String, reason: String) {
        cancelFlush(path)
        buffers.remove(path)
        delegate.onUsbError(path, reason)
    }

    override fun onPortListChange() = delegate.onPortListChange()

    private fun scheduleFlush(path: String) {
        flushTasks.compute(path) { _, existing ->
            existing?.cancel(false)
            scheduler.schedule({ flush(path) }, FLUSH_MS, TimeUnit.MILLISECONDS)
        }
    }

    private fun cancelFlush(path: String) {
        flushTasks.remove(path)?.cancel(false)
    }

    private fun flush(path: String) {
        flushTasks.remove(path)
        val buf = buffers.remove(path) ?: return
        val bytes = synchronized(buf) { buf.toByteArray() }
        if (bytes.isNotEmpty()) delegate.feedRx(path, bytes)
    }

    companion object {
        private const val FLUSH_MS = 8L
        private const val MAX_BATCH = 512
    }
}
