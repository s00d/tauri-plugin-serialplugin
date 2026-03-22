package app.tauri.serialplugin.manager

import org.junit.Assert.assertEquals
import org.junit.Test

class BufferedEmitterTest {
    @Test
    fun pendingByteCount_tracks_data_before_flush() {
        // Max flush interval (2000 ms) so the first scheduled flush is late; we only assert immediately.
        val emitter = BufferedEmitter("/dev/usbX", 2000L) { _ -> }
        try {
            assertEquals(0, emitter.pendingByteCount())
            emitter.addData(byteArrayOf(10, 20, 30))
            assertEquals(3, emitter.pendingByteCount())
        } finally {
            emitter.stop()
        }
    }
}
