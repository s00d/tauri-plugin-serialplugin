package app.tauri.serialplugin.manager

import org.junit.Assert.assertEquals
import org.junit.Test

class CoalescingRxSinkTest {

  @Test
  fun onPortClosed_clears_pending_buffers() {
    val delegate = RecordingRxSink()
    val sink = CoalescingRxSink(delegate, immediate = false)
    val path = "/dev/bus/usb/001/099"

    sink.feedRx(path, "partial".toByteArray())
    sink.onPortClosed(path)

    sink.feedRx(path, "after-close".toByteArray())
    Thread.sleep(20)
    val batches = delegate.drainRx(path)
    assertEquals(1, batches.size)
    assertEquals("after-close", String(batches.single()))
  }
}
