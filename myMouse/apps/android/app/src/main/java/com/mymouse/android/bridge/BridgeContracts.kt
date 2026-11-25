package com.mymouse.android.bridge

import java.io.Closeable

const val DEFAULT_MYMOUSE_PORT: Int = 45552
const val FINGERPRINT_HEX_LENGTH: Int = 64

/** Explicit failure for features a chosen implementation cannot perform. */
open class BridgeException(message: String, cause: Throwable? = null) : Exception(message, cause)
class BridgeUnsupportedException(feature: String) :
    BridgeException("$feature is not implemented by this bridge; no action was performed")

/** Mirrors `discovery::DiscoveredPeer` in shared/android-bridge. */
data class BridgePeer(
    val id: String,
    val name: String,
    val address: String,
    val port: Int,
    val fingerprintHint: String? = null,
)

data class BridgePairingResult(
    val ok: Boolean,
    val message: String,
    val pairingUri: String? = null,
    val fingerprint: String? = null,
)

/**
 * Everything the Android UI needs from a bridge. Network operations are
 * suspending: callers must use a coroutine scope and handle failures.
 */
interface MyMouseBridge {
    fun protocolVersion(): Int

    /** Honest diagnostic shown in the UI: tells which implementation is live. */
    fun implementationName(): String

    // --- Pairing helpers (pure, match shared/android-bridge/src/pairing.rs) ---
    fun generatePairingCode(): String
    fun buildPairingUri(code: String, peerId: String): String
    fun parsePairingUri(uri: String): Pair<String, String>? // (code, peerId)
    fun verifyCode(expected: String, entered: String): Boolean

    /** Owned by trust UI: accepts colon/space separated uppercase hex, returns 64 lowercase hex chars. */
    fun normalizeFingerprint(value: String): String?

    // --- Pinned transport ---
    /** Connect only; requires a pinned fingerprint and an already-trusted peer. */
    suspend fun connect(request: BridgeConnectRequest): BridgeSession

    /** Connect, send Hello + PairCode, and require PairApprove before trust is stored. */
    suspend fun connectAndPair(request: BridgeConnectRequest): BridgeSession
}

data class BridgeConnectRequest(
    val host: String,
    val port: Int,
    val peerId: String,
    val peerName: String,
    val pinnedFingerprint: String,
    val pairingCode: String,
    val deviceId: String,
    val deviceName: String,
    val timeoutMs: Long = 15_000,
)

/**
 * A live pinned-TLS connection. All sends are serialized; receives are
 * dispatched on a single reader coroutine. Close is idempotent.
 */
interface BridgeSession : Closeable {
    val peerId: String
    val fingerprint: String
    val isConnected: Boolean

    suspend fun send(message: WireMessage)

    fun addMessageListener(listener: (WireMessage) -> Unit)
    fun removeMessageListener(listener: (WireMessage) -> Unit)
    fun addCloseListener(listener: () -> Unit)

    /** Wait for the first matching message; never returns a non-matching one. */
    suspend fun awaitMessage(predicate: (WireMessage) -> Boolean, timeoutMs: Long): WireMessage

    override fun close()
}

/**
 * LAST RESORT ONLY. Kept so the app still compiles if neither a generated
 * native binding nor the Kotlin TLS implementation can be loaded. Network
 * operations throw instead of silently pretending to work.
 */
class StubMyMouseBridge : MyMouseBridge {
    override fun protocolVersion(): Int = WireProtocol.PROTOCOL_VERSION

    override fun implementationName(): String = "STUB (last-resort fallback; no network)"

    override fun generatePairingCode(): String =
        (0 until 1_000_000).random().toString().padStart(6, '0')

    override fun buildPairingUri(code: String, peerId: String): String =
        "mymouse://pair?code=$code&id=$peerId"

    override fun parsePairingUri(uri: String): Pair<String, String>? {
        if (!uri.startsWith("mymouse://pair?")) return null
        val params = uri.removePrefix("mymouse://pair?").split("&").mapNotNull {
            val parts = it.split("=", limit = 2)
            if (parts.size == 2) parts[0] to parts[1] else null
        }.toMap()
        val code = params["code"] ?: return null
        val id = params["id"] ?: return null
        if (code.length != 6 || !code.all { c -> c in '0'..'9' } || id.isBlank()) return null
        return code to id
    }

    override fun verifyCode(expected: String, entered: String): Boolean =
        expected.length == 6 && expected.all { it in '0'..'9' } && expected == entered

    override fun normalizeFingerprint(value: String): String? {
        val digits = value.filterNot { c -> c == ':' || c == ' ' || c == '-' || c.isWhitespace() }
        return if (digits.length == FINGERPRINT_HEX_LENGTH && digits.all { it.isHexDigit() }) {
            digits.lowercase()
        } else null
    }

    override suspend fun connect(request: BridgeConnectRequest): BridgeSession =
        throw BridgeUnsupportedException("Pinned TLS connect")

    override suspend fun connectAndPair(request: BridgeConnectRequest): BridgeSession =
        throw BridgeUnsupportedException("Pairing handshake")

    private fun Char.isHexDigit(): Boolean = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
}
