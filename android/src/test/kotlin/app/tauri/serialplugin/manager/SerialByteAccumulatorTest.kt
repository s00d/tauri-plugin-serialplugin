package app.tauri.serialplugin.manager

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

class SerialByteAccumulatorTest {
    @Test
    fun append_empty_is_noop() {
        val acc = SerialByteAccumulator()
        acc.append(ByteArray(0))
        assertEquals(0, acc.pendingByteCount())
        assertArrayEquals(ByteArray(0), acc.drain())
    }

    @Test
    fun append_then_drain_preserves_order() {
        val acc = SerialByteAccumulator()
        acc.append(byteArrayOf(1, 2))
        acc.append(byteArrayOf(3))
        assertEquals(3, acc.pendingByteCount())
        assertArrayEquals(byteArrayOf(1, 2, 3), acc.drain())
        assertEquals(0, acc.pendingByteCount())
        assertArrayEquals(ByteArray(0), acc.drain())
    }

    @Test
    fun drain_when_empty_returns_empty() {
        val acc = SerialByteAccumulator()
        assertArrayEquals(ByteArray(0), acc.drain())
    }

    @Test
    fun concurrent_appends_merge_consistently() {
        val acc = SerialByteAccumulator()
        val threads = List(8) { i ->
            Thread {
                repeat(100) {
                    acc.append(byteArrayOf(i.toByte()))
                }
            }
        }
        threads.forEach { it.start() }
        threads.forEach { it.join() }
        val out = acc.drain()
        assertEquals(800, out.size)
        // All bytes are in 0..7 — count occurrences
        val counts = IntArray(8)
        for (b in out) {
            counts[b.toInt() and 0xff]++
        }
        for (i in 0 until 8) {
            assertEquals(100, counts[i])
        }
    }
}
