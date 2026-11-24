package com.mymouse.android.bridge

/**
 * Kotlin-side contract mirroring the Rust android-bridge API.
 *
 * When UniFFI-generated bindings are available (see shared/android-bridge/README.md),
 * replace this file with generated bindings and wire the real native library.
 */
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

interface MyMouseBridge {
    fun protocolVersion(): Int
    fun generatePairingCode(): String
    fun buildPairingUri(code: String, peerId: String): String
    fun parsePairingUri(uri: String): Pair<String, String>? // (code, peerId)
    fun verifyCode(expected: String, entered: String): Boolean
}

/**
 * Scaffold fallback bridge used when native UniFFI library is not bundled.
 * It preserves behavior needed for UI/dev flows and makes the integration point explicit.
 */
class StubMyMouseBridge : MyMouseBridge {
    override fun protocolVersion(): Int = 1

    override fun generatePairingCode(): String =
        (0..999999).random().toString().padStart(6, '0')

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
        return code to id
    }

    override fun verifyCode(expected: String, entered: String): Boolean =
        expected.length == 6 && expected == entered
}
