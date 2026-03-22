package app.tauri.serialplugin.manager

import app.tauri.plugin.JSObject

/**
 * Payload fields for the `serialData` plugin event (mirrors what we put on [JSObject]).
 * Kept as a plain Kotlin type so tests do not need [JSObject].
 */
internal data class SerialDataEmitFields(
    val path: String,
    val dataAsString: String,
    val size: Int,
)

internal fun serialDataPayloadFromChunk(path: String, chunk: ByteArray): SerialDataEmitFields =
    SerialDataEmitFields(
        path = path,
        dataAsString = String(chunk),
        size = chunk.size,
    )

internal fun SerialDataEmitFields.applyToJSObject(target: JSObject) {
    target.put("path", path)
    target.put("data", dataAsString)
    target.put("size", size)
}

/**
 * Drains [accumulator] and invokes [emitFields] once if there was data.
 */
internal fun flushAccumulatorToEmit(
    path: String,
    accumulator: SerialByteAccumulator,
    emitFields: (SerialDataEmitFields) -> Unit,
) {
    val chunk = accumulator.drain()
    if (chunk.isEmpty()) return
    emitFields(serialDataPayloadFromChunk(path, chunk))
}
