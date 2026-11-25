package com.mymouse.android.bridge

import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Minimal deterministic InputEvent payload format for protocol v2.
 *
 * The Rust protocol only defines `InputEvent { target: String, payload: Vec<u8> }`;
 * it deliberately does not interpret the bytes. Android and any host therefore
 * use this 20-byte, versioned, fixed-size layout ("MINP/1"):
 *
 *   offset  size  field
 *   0       4     magic bytes 'M' 'I' 'N' 'P'
 *   4       1     format version = 1
 *   5       1     kind: 1 MOVE, 2 DOWN, 3 UP, 4 SWIPE,
 *                       5 HOME, 6 BACK, 7 RECENTS
 *   6       1     pointerId 0..9 (0 = primary)
 *   7       1     button bitmask 0..0x0f (bit 0 = primary/left)
 *   8       4     x, signed i32 little-endian, device pixels
 *   12      4     y, signed i32 little-endian, device pixels
 *   16      4     keyCode i32 little-endian, 0 for pointer/global actions;
 *                 reserved for future key injection, decoded but NOT injected
 *                 (see MyMouseAccessibilityService).
 *
 * All integers are little-endian, matching bincode/fixint in the Rust protocol.
 * Decoding is total: malformed, truncated, oversized or unknown inputs are
 * rejected with [InputEventDecodeException] and never reach the injector.
 */
object InputEventCodec {
    const val FORMAT_VERSION: Int = 1
    const val ENCODED_LENGTH: Int = 20

    private const val MAGIC_0 = 'M'.code
    private const val MAGIC_1 = 'I'.code
    private const val MAGIC_2 = 'N'.code
    private const val MAGIC_3 = 'P'.code
    private const val MAX_COORDINATE = 100_000
    private const val MAX_KEY_CODE = 300

    enum class Kind(val code: Int) {
        MOVE(1), DOWN(2), UP(3), SWIPE(4), HOME(5), BACK(6), RECENTS(7);

        companion object {
            fun fromCode(code: Int): Kind? = values().firstOrNull { it.code == code }
        }
    }

    data class Event(
        val kind: Kind,
        val pointerId: Int,
        val buttons: Int,
        val x: Int,
        val y: Int,
        val keyCode: Int = 0,
    )

    fun encode(event: Event): ByteArray {
        require(event.pointerId in 0..9) { "pointerId out of range" }
        require(event.buttons in 0..0x0f) { "button mask out of range" }
        require(event.x in -MAX_COORDINATE..MAX_COORDINATE) { "x out of range" }
        require(event.y in -MAX_COORDINATE..MAX_COORDINATE) { "y out of range" }
        require(event.keyCode in 0..MAX_KEY_CODE) { "keyCode out of range" }
        val buffer = ByteBuffer.allocate(ENCODED_LENGTH).order(ByteOrder.LITTLE_ENDIAN)
        buffer.put(MAGIC_0.toByte()).put(MAGIC_1.toByte()).put(MAGIC_2.toByte()).put(MAGIC_3.toByte())
        buffer.put(FORMAT_VERSION.toByte())
        buffer.put(event.kind.code.toByte())
        buffer.put(event.pointerId.toByte())
        buffer.put(event.buttons.toByte())
        buffer.putInt(event.x)
        buffer.putInt(event.y)
        buffer.putInt(event.keyCode)
        return buffer.array()
    }

    fun decode(bytes: ByteArray): Event {
        if (bytes.size != ENCODED_LENGTH) throw InputEventDecodeException("wrong payload length")
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        if (buffer.get().toInt() and 0xff != MAGIC_0 ||
            buffer.get().toInt() and 0xff != MAGIC_1 ||
            buffer.get().toInt() and 0xff != MAGIC_2 ||
            buffer.get().toInt() and 0xff != MAGIC_3
        ) throw InputEventDecodeException("bad magic")
        if (buffer.get().toInt() and 0xff != FORMAT_VERSION) {
            throw InputEventDecodeException("unknown format version")
        }
        val kind = Kind.fromCode(buffer.get().toInt() and 0xff)
            ?: throw InputEventDecodeException("unknown event kind")
        val pointerId = buffer.get().toInt() and 0xff
        val buttons = buffer.get().toInt() and 0xff
        val x = buffer.getInt()
        val y = buffer.getInt()
        val keyCode = buffer.getInt()
        if (pointerId > 9) throw InputEventDecodeException("pointerId out of range")
        if (buttons > 0x0f) throw InputEventDecodeException("button mask out of range")
        if (x !in -MAX_COORDINATE..MAX_COORDINATE || y !in -MAX_COORDINATE..MAX_COORDINATE) {
            throw InputEventDecodeException("coordinates out of range")
        }
        if (keyCode !in 0..MAX_KEY_CODE) throw InputEventDecodeException("keyCode out of range")
        if (buffer.hasRemaining()) throw InputEventDecodeException("trailing bytes")
        return Event(kind, pointerId, buttons, x, y, keyCode)
    }
}

class InputEventDecodeException(message: String) : Exception(message)
