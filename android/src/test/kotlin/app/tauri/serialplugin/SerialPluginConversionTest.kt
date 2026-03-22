package app.tauri.serialplugin

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * Tests for package-level [Map.toJSObject] and [List.toJSArray] helpers used by [SerialPlugin].
 */
class SerialPluginConversionTest {
    @Test
    fun toJSObject_flatPrimitives() {
        val m = mapOf<String, Any?>(
            "n" to 42,
            "s" to "hello",
            "b" to true,
        )
        val o = m.toJSObject()
        assertEquals(42, o.getInteger("n"))
        assertEquals("hello", o.getString("s"))
        assertEquals(true, o.getBoolean("b"))
    }

    @Test
    fun toJSObject_nestedMap() {
        val inner = mapOf<String, Any?>("k" to "v", "x" to 1)
        val m = mapOf<String, Any?>("outer" to inner)
        val o = m.toJSObject()
        val innerObj = o.getJSObject("outer")
        assertNotNull(innerObj)
        assertEquals("v", innerObj!!.getString("k"))
        assertEquals(1, innerObj.getInteger("x"))
    }

    @Test
    fun toJSArray_listOfMaps() {
        val list: List<Any?> = listOf(
            mapOf<String, Any?>("a" to 1),
            mapOf<String, Any?>("b" to 2),
        )
        val arr = list.toJSArray()
        assertEquals(2, arr.length())
        val first = arr.getJSONObject(0)
        assertEquals(1, first.getInt("a"))
    }

    @Test
    fun toJSArray_mixedPrimitives() {
        val list: List<Any?> = listOf("x", 3, false)
        val arr = list.toJSArray()
        assertEquals(3, arr.length())
        assertEquals("x", arr.getString(0))
        assertEquals(3, arr.getInt(1))
        assertEquals(false, arr.getBoolean(2))
    }

    @Test
    fun toJSArray_nestedListOfInts() {
        val inner: List<Any?> = listOf(1, 2)
        val outer: List<Any?> = listOf(inner)
        val arr = outer.toJSArray()
        assertEquals(1, arr.length())
        val nested = arr.getJSONArray(0)
        assertEquals(2, nested.length())
        assertEquals(1, nested.getInt(0))
        assertEquals(2, nested.getInt(1))
    }
}
