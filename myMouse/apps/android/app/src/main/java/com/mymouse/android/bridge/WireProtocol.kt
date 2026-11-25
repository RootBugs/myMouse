package com.mymouse.android.bridge

import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharsetDecoder
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

/**
 * Exact Kotlin mirror of the Rust v2 wire format. Do not "improve" this file
 * without updating `shared/android-bridge/src/transport.rs` golden tests.
 *
 * Source of truth (read, not changed): `shared/protocol/src/lib.rs`:
 *   encode() = u16 LE PROTOCOL_VERSION (2) || bincode::serialize(Message)
 *   decode() rejects a bad version header, a body > 16 MiB, and a Hello whose
 *            inner `version` field is not 2.
 * bincode 1.3's `serialize()` uses fixint encoding, little endian:
 *   enum variant index -> u32 LE
 *   String / Vec / slice length -> u64 LE, then raw items
 *   Option<T> -> u8 (0 = None, 1 = Some) then T
 *   bool -> u8 (0 / 1)
 *   struct fields -> declaration order, no names or padding
 *   nested enum (ClipboardKind) -> u32 LE variant index
 * The frame wrapper in the Rust bridge/backend is a u32 BIG-endian length
 * followed by exactly that payload (MAX_FRAME_LEN = 1 MiB).
 */
object WireProtocol {
    const val PROTOCOL_VERSION: Int = 2
    const val MAX_FRAME_LEN: Int = 1024 * 1024
    private const val MAX_BODY_LEN: Int = 16 * 1024 * 1024
    private const val PROTOCOL_HEADER_LEN: Int = 2

    // Message variant indices, in declaration order in mymouse_protocol::Message.
    private const val V_HELLO = 0
    private const val V_PAIR_CODE = 1
    private const val V_PAIR_APPROVE = 2
    private const val V_PAIR_REJECT = 3
    private const val V_INPUT_EVENT = 4
    private const val V_STATUS = 5
    private const val V_ERROR = 6
    private const val V_CLIPBOARD_UPDATE = 7
    private const val V_FILE_OFFER = 8
    private const val V_FILE_ACCEPT = 9
    private const val V_FILE_REJECT = 10
    private const val V_FILE_CHUNK = 11
    private const val V_FILE_COMPLETE = 12
    private const val V_FILE_CANCEL = 13

    fun encode(message: WireMessage): ByteArray {
        val out = Sink(MAX_BODY_LEN)
        out.u16(PROTOCOL_VERSION)
        when (message) {
            is WireMessage.Hello -> {
                out.u32(V_HELLO)
                out.u16(message.version)
                out.string(message.deviceId)
                out.string(message.name)
            }
            is WireMessage.PairCode -> {
                out.u32(V_PAIR_CODE)
                out.string(message.code)
            }
            WireMessage.PairApprove -> out.u32(V_PAIR_APPROVE)
            WireMessage.PairReject -> out.u32(V_PAIR_REJECT)
            is WireMessage.InputEvent -> {
                out.u32(V_INPUT_EVENT)
                out.string(message.target)
                out.bytes(message.payload)
            }
            is WireMessage.Status -> {
                out.u32(V_STATUS)
                out.u8(if (message.connected) 1 else 0)
                out.u64(message.devices.size.toLong())
                message.devices.forEach { out.string(it) }
            }
            is WireMessage.Error -> {
                out.u32(V_ERROR)
                out.string(message.message)
            }
            is WireMessage.ClipboardUpdate -> {
                out.u32(V_CLIPBOARD_UPDATE)
                val p = message.payload
                out.string(p.deviceId)
                out.u32(if (p.kind == ClipboardKind.TEXT) 0 else 1)
                out.optionString(p.text)
                out.optionString(p.dataBase64)
                out.optionString(p.mime)
                out.u64(p.timestampMs)
                out.u64(p.seq)
            }
            is WireMessage.FileTransferOffer -> {
                out.u32(V_FILE_OFFER)
                val o = message.offer
                out.string(o.transferId)
                out.string(o.fromDeviceId)
                out.string(o.name)
                out.u64(o.size)
                out.string(o.sha256)
                out.optionString(o.mime)
            }
            is WireMessage.FileTransferAccept -> {
                out.u32(V_FILE_ACCEPT)
                out.string(message.transferId)
            }
            is WireMessage.FileTransferReject -> {
                out.u32(V_FILE_REJECT)
                out.string(message.transferId)
                out.optionString(message.reason)
            }
            is WireMessage.FileTransferChunk -> {
                out.u32(V_FILE_CHUNK)
                val c = message.chunk
                out.string(c.transferId)
                out.u64(c.offset)
                out.bytes(c.data)
                out.u8(if (c.last) 1 else 0)
            }
            is WireMessage.FileTransferComplete -> {
                out.u32(V_FILE_COMPLETE)
                out.string(message.transferId)
            }
            is WireMessage.FileTransferCancel -> {
                out.u32(V_FILE_CANCEL)
                out.string(message.transferId)
                out.optionString(message.reason)
            }
        }
        return out.toByteArray()
    }

    fun decode(bytes: ByteArray): WireMessage {
        val input = Source(bytes)
        val version = input.u16()
        if (version != PROTOCOL_VERSION) throw WireProtocolException.VersionMismatch
        if (input.remaining() > MAX_BODY_LEN) throw WireProtocolException.TooLarge
        val message = when (input.u32()) {
            V_HELLO -> {
                val helloVersion = input.u16()
                val message = WireMessage.Hello(helloVersion, input.string(), input.string())
                if (helloVersion != PROTOCOL_VERSION) throw WireProtocolException.VersionMismatch
                message
            }
            V_PAIR_CODE -> WireMessage.PairCode(input.string())
            V_PAIR_APPROVE -> WireMessage.PairApprove
            V_PAIR_REJECT -> WireMessage.PairReject
            V_INPUT_EVENT -> WireMessage.InputEvent(input.string(), input.bytes())
            V_STATUS -> {
                val connected = input.u8() != 0
                val count = input.u64()
                if (count > input.remaining()) throw WireProtocolException.InvalidPayload
                val devices = ArrayList<String>(count.toInt())
                repeat(count.toInt()) { devices.add(input.string()) }
                WireMessage.Status(connected, devices)
            }
            V_ERROR -> WireMessage.Error(input.string())
            V_CLIPBOARD_UPDATE -> {
                val deviceId = input.string()
                val kind = when (input.u32()) {
                    0 -> ClipboardKind.TEXT
                    1 -> ClipboardKind.IMAGE
                    else -> throw WireProtocolException.InvalidPayload
                }
                WireMessage.ClipboardUpdate(
                    ClipboardPayload(
                        deviceId = deviceId,
                        kind = kind,
                        text = input.optionString(),
                        dataBase64 = input.optionString(),
                        mime = input.optionString(),
                        timestampMs = input.u64(),
                        seq = input.u64(),
                    )
                )
            }
            V_FILE_OFFER -> WireMessage.FileTransferOffer(
                FileOffer(
                    transferId = input.string(),
                    fromDeviceId = input.string(),
                    name = input.string(),
                    size = input.u64(),
                    sha256 = input.string(),
                    mime = input.optionString(),
                )
            )
            V_FILE_ACCEPT -> WireMessage.FileTransferAccept(input.string())
            V_FILE_REJECT -> WireMessage.FileTransferReject(input.string(), input.optionString())
            V_FILE_CHUNK -> WireMessage.FileTransferChunk(
                FileChunk(input.string(), input.u64(), input.bytes(), input.u8() != 0)
            )
            V_FILE_COMPLETE -> WireMessage.FileTransferComplete(input.string())
            V_FILE_CANCEL -> WireMessage.FileTransferCancel(input.string(), input.optionString())
            else -> throw WireProtocolException.InvalidPayload
        }
        if (input.remaining() != 0) throw WireProtocolException.InvalidPayload
        return message
    }

    /** Write one Rust-compatible frame: u32 BE payload length + payload. */
    fun writeFrame(output: DataOutputStream, message: WireMessage) {
        val payload = encode(message)
        if (payload.size > MAX_FRAME_LEN) throw WireProtocolException.TooLarge
        output.writeInt(payload.size) // DataOutputStream writes big-endian.
        output.write(payload)
        output.flush()
    }

    /** Read one Rust-compatible frame. Blocks until a full frame is available. */
    fun readFrame(input: DataInputStream): WireMessage {
        val length = input.readInt() // DataInputStream reads big-endian.
        if (length <= 0 || length > MAX_FRAME_LEN) throw WireProtocolException.InvalidPayload
        val payload = ByteArray(length)
        input.readFully(payload)
        return decode(payload)
    }

    /** Lowercase hex SHA-256 style comparison used by the pin check. */
    fun constantTimeEquals(a: String, b: String): Boolean {
        if (a.length != b.length) return false
        var diff = 0
        for (i in a.indices) diff = diff or (a[i].code xor b[i].code)
        return diff == 0
    }

    private class Sink(private val max: Int) {
        private val data = java.io.ByteArrayOutputStream()
        private fun ensure(n: Int) {
            if (n < 0 || data.size() + n > max) throw WireProtocolException.TooLarge
        }
        fun u8(v: Int) { ensure(1); data.write(v and 0xff) }
        fun u16(v: Int) { u8(v); u8(v shr 8) }
        fun u32(v: Int) { repeat(4) { u8(v shr (it * 8)) } }
        fun u64(v: Long) { repeat(8) { u8((v shr (it * 8)).toInt()) } }
        fun bytes(v: ByteArray) { ensure(8 + v.size); u64(v.size.toLong()); data.write(v, 0, v.size) }
        fun string(v: String) {
            val raw = v.toByteArray(StandardCharsets.UTF_8)
            ensure(8 + raw.size)
            u64(raw.size.toLong())
            data.write(raw, 0, raw.size)
        }
        fun optionString(v: String?) {
            if (v == null) u8(0) else { u8(1); string(v) }
        }
        fun toByteArray(): ByteArray = data.toByteArray()
    }

    private class Source(private val bytes: ByteArray) {
        private var pos = 0
        fun remaining(): Int = bytes.size - pos
        private fun take(n: Int): ByteArray {
            if (n < 0 || n > remaining()) throw WireProtocolException.InvalidPayload
            val out = bytes.copyOfRange(pos, pos + n)
            pos += n
            return out
        }
        fun u8(): Int {
            if (remaining() < 1) throw WireProtocolException.InvalidPayload
            return bytes[pos++].toInt() and 0xff
        }
        fun u16(): Int = u8() or (u8() shl 8)
        fun u32(): Int = (0 until 4).fold(0) { acc, i -> acc or (u8() shl (i * 8)) }
        fun u64(): Long {
            var value = 0L
            for (i in 0 until 8) value = value or ((u8().toLong() and 0xff) shl (i * 8))
            if (value < 0) throw WireProtocolException.InvalidPayload // u64 > Long.MAX_VALUE
            return value
        }
        fun bytes(): ByteArray {
            val len = u64()
            if (len > remaining()) throw WireProtocolException.InvalidPayload
            return take(len.toInt())
        }
        fun string(): String {
            val raw = bytes()
            val decoder: CharsetDecoder = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
            return runCatching { decoder.decode(ByteBuffer.wrap(raw)).toString() }
                .getOrElse { throw WireProtocolException.InvalidPayload }
        }
        fun optionString(): String? = when (u8()) {
            0 -> null
            1 -> string()
            else -> throw WireProtocolException.InvalidPayload
        }
    }
}

/** Mirrors `mymouse_protocol::ProtocolError` (plus local framing errors). */
sealed class WireProtocolException(message: String) : Exception(message) {
    object TooShort : WireProtocolException("message shorter than protocol header")
    object VersionMismatch : WireProtocolException("unsupported protocol version")
    object TooLarge : WireProtocolException("message body exceeds 16 MiB / frame exceeds 1 MiB")
    object InvalidPayload : WireProtocolException("invalid message payload")
}

enum class ClipboardKind { TEXT, IMAGE }

data class ClipboardPayload(
    val deviceId: String,
    val kind: ClipboardKind,
    val text: String?,
    val dataBase64: String?,
    val mime: String?,
    val timestampMs: Long,
    val seq: Long,
)

data class FileOffer(
    val transferId: String,
    val fromDeviceId: String,
    val name: String,
    val size: Long,
    val sha256: String,
    val mime: String?,
)

data class FileChunk(
    val transferId: String,
    val offset: Long,
    val data: ByteArray,
    val last: Boolean,
)

/** Full v2 message set, mirroring `mymouse_protocol::Message`. */
sealed class WireMessage {
    data class Hello(val version: Int, val deviceId: String, val name: String) : WireMessage()
    data class PairCode(val code: String) : WireMessage()
    object PairApprove : WireMessage()
    object PairReject : WireMessage()
    data class InputEvent(val target: String, val payload: ByteArray) : WireMessage()
    data class Status(val connected: Boolean, val devices: List<String>) : WireMessage()
    data class Error(val message: String) : WireMessage()
    data class ClipboardUpdate(val payload: ClipboardPayload) : WireMessage()
    data class FileTransferOffer(val offer: FileOffer) : WireMessage()
    data class FileTransferAccept(val transferId: String) : WireMessage()
    data class FileTransferReject(val transferId: String, val reason: String?) : WireMessage()
    data class FileTransferChunk(val chunk: FileChunk) : WireMessage()
    data class FileTransferComplete(val transferId: String) : WireMessage()
    data class FileTransferCancel(val transferId: String, val reason: String?) : WireMessage()
}
