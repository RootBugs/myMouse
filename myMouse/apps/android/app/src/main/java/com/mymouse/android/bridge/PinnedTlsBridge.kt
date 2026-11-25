package com.mymouse.android.bridge

import android.os.Build
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.EOFException
import java.net.SocketException
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.CopyOnWriteArrayList
import javax.net.ssl.SNIHostName
import javax.net.ssl.SNIServerName
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/** Fingerprint rules shared with `shared/android-bridge/src/transport.rs`. */
object Fingerprints {
    /** Accepts colon/space/hyphen separated uppercase hex; emits 64 lowercase hex chars. */
    fun normalize(value: String): String? {
        val digits = value.filterNot { c -> c == ':' || c == ' ' || c == '-' || c.isWhitespace() }
        if (digits.length != FINGERPRINT_HEX_LENGTH) return null
        if (digits.any { c -> !c.isAsciiHexDigit() }) return null
        return digits.lowercase()
    }

    /** SHA-256 over the DER end-entity certificate, lowercase hex (backend parity). */
    fun sha256Hex(der: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256").digest(der)
        return digest.joinToString("") { "%02x".format(it) }
    }

    private fun Char.isAsciiHexDigit(): Boolean =
        this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'
}

/**
 * Real Kotlin bridge: TLS with an explicit end-entity SHA-256 pin, using the
 * project's exact v2 wire framing. Hostname is deliberately NOT trusted as an
 * alternative identity: the backend also pins only the certificate digest.
 */
class PinnedTlsBridge : MyMouseBridge {
    private val secureRandom = SecureRandom()

    override fun protocolVersion(): Int = WireProtocol.PROTOCOL_VERSION
    override fun implementationName(): String = "Kotlin javax.net.ssl + pinned SHA-256 (v2 framing)"

    override fun generatePairingCode(): String =
        secureRandom.nextInt(1_000_000).toString().padStart(6, '0')

    override fun buildPairingUri(code: String, peerId: String): String =
        "mymouse://pair?code=$code&id=$peerId"

    override fun parsePairingUri(uri: String): Pair<String, String>? {
        val rest = uri.removePrefix("mymouse://pair?")
        if (rest == uri) return null
        var code: String? = null
        var id: String? = null
        for (part in rest.split('&')) {
            val pieces = part.split('=', limit = 2)
            if (pieces.size != 2) return null
            when (pieces[0]) {
                "code" -> code = pieces[1]
                "id" -> id = pieces[1]
                else -> Unit // Rust parser also ignores unknown keys.
            }
        }
        val foundCode = code ?: return null
        val foundId = id ?: return null
        return foundCode to foundId
    }

    override fun verifyCode(expected: String, entered: String): Boolean =
        expected.length == 6 && expected.all { it in '0'..'9' } && expected == entered

    override fun normalizeFingerprint(value: String): String? = Fingerprints.normalize(value)

    override suspend fun connect(request: BridgeConnectRequest): BridgeSession =
        withContext(Dispatchers.IO) {
            val pin = normalizeFingerprint(request.pinnedFingerprint)
                ?: throw BridgeException("A valid 64-hex SHA-256 certificate fingerprint is required")
            if (request.host.isBlank()) throw BridgeException("Host address is required")
            if (request.port !in 1..65535) throw BridgeException("Invalid host port")

            val context = SSLContext.getInstance("TLS")
            val trustManager = PinnedTrustManager(pin)
            context.init(null, arrayOf<TrustManager>(trustManager), secureRandom)

            val socket = context.socketFactory.createSocket(request.host, request.port) as? SSLSocket
                ?: throw BridgeException("TLS factory did not return an SSLSocket")
            socket.soTimeout = request.timeoutMs.coerceIn(1_000, 120_000).toInt()
            val params = socket.sslParameters
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                // Same SNI as the Windows client. Pinning, not DNS/SNI, is the identity.
                params.serverNames = mutableListOf<SNIServerName>(SNIHostName("mymouse.local"))
                socket.sslParameters = params
            }
            try {
                socket.startHandshake()
                val certificate = socket.session.peerCertificates.firstOrNull() as? X509Certificate
                    ?: throw BridgeException("Peer presented no certificate")
                val observed = Fingerprints.sha256Hex(certificate.encoded)
                if (!WireProtocol.constantTimeEquals(observed, pin)) {
                    throw BridgeException("unpinned peer certificate")
                }
                PinnedTlsSession(socket, request.peerId, pin).also { it.start() }
            } catch (t: Throwable) {
                runCatching { socket.close() }
                throw t
            }
        }

    override suspend fun connectAndPair(request: BridgeConnectRequest): BridgeSession {
        val session = connect(request)
        try {
            session.send(WireMessage.Hello(WireProtocol.PROTOCOL_VERSION, request.deviceId, request.deviceName))
            session.send(WireMessage.PairCode(request.pairingCode))
            val answer = session.awaitMessage(
                { it is WireMessage.PairApprove || it is WireMessage.PairReject || it is WireMessage.Error },
                request.timeoutMs,
            )
            when (answer) {
                is WireMessage.PairApprove -> return session
                is WireMessage.PairReject -> throw BridgeException("Host rejected the pairing code")
                is WireMessage.Error -> throw BridgeException("Host error: ${answer.message}")
                else -> throw BridgeException("Unexpected pairing response")
            }
        } catch (t: Throwable) {
            session.close()
            throw t
        }
    }

    /** Only the first certificate in the chain is pinned, exactly like the backend verifier. */
    private class PinnedTrustManager(private val expectedFingerprint: String) : X509TrustManager {
        override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
            throw CertificateException("myMouse never accepts client certificates")
        }

        override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
            val endEntity = chain.firstOrNull() ?: throw CertificateException("empty certificate chain")
            val observed = Fingerprints.sha256Hex(endEntity.encoded)
            if (!WireProtocol.constantTimeEquals(observed, expectedFingerprint)) {
                throw CertificateException("unpinned peer certificate")
            }
        }

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }
}

/** Routes decoded host input to the accessibility service (installed by it at bind time). */
object InputEventSink {
    @Volatile
    var handler: ((target: String, payload: ByteArray, fingerprint: String) -> Unit)? = null

    fun receive(target: String, payload: ByteArray, fingerprint: String) {
        handler?.invoke(target, payload, fingerprint)
    }
}

/** One pinned connection; one reader coroutine; serialized writes. */
private class PinnedTlsSession(
    private val socket: SSLSocket,
    override val peerId: String,
    override val fingerprint: String,
) : BridgeSession {
    private val input = DataInputStream(socket.getInputStream())
    private val output = DataOutputStream(socket.getOutputStream())
    private val writeLock = Any()
    private val listeners = CopyOnWriteArrayList<(WireMessage) -> Unit>()
    private val closeListeners = CopyOnWriteArrayList<() -> Unit>()
    private val waiters = CopyOnWriteArrayList<Waiter>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile
    override var isConnected: Boolean = true
        private set

    @Volatile
    private var closed = false

    private class Waiter(
        val predicate: (WireMessage) -> Boolean,
        val deferred: CompletableDeferred<WireMessage>,
    )

    fun start() {
        scope.launch {
            try {
                while (!closed) {
                    val message = WireProtocol.readFrame(input)
                    if (message is WireMessage.InputEvent) {
                        InputEventSink.receive(message.target, message.payload, fingerprint)
                    }
                    for (listener in listeners) runCatching { listener(message) }
                    val matched = waiters.filter { it.predicate(message) }
                    for (waiter in matched) waiter.deferred.complete(message)
                    waiters.removeAll(matched.toSet())
                }
            } catch (e: EOFException) {
                // Clean peer close.
            } catch (e: SocketException) {
                if (!closed) android.util.Log.w(TAG, "Socket closed: ${e.message}")
            } catch (e: Exception) {
                if (!closed) android.util.Log.w(TAG, "Reader stopped: ${e.javaClass.simpleName}: ${e.message}")
            } finally {
                markClosed()
            }
        }
    }

    override suspend fun send(message: WireMessage) = withContext(Dispatchers.IO) {
        if (!isConnected) throw BridgeException("Session is closed")
        synchronized(writeLock) { WireProtocol.writeFrame(output, message) }
    }

    override fun addMessageListener(listener: (WireMessage) -> Unit) {
        if (listener !in listeners) listeners.add(listener)
    }

    override fun removeMessageListener(listener: (WireMessage) -> Unit) {
        listeners.remove(listener)
    }

    override fun addCloseListener(listener: () -> Unit) {
        if (listener !in closeListeners) closeListeners.add(listener)
    }

    override suspend fun awaitMessage(predicate: (WireMessage) -> Boolean, timeoutMs: Long): WireMessage {
        if (!isConnected) throw BridgeException("Session is closed")
        val waiter = Waiter(predicate, CompletableDeferred())
        waiters.add(waiter)
        return try {
            withTimeout(timeoutMs) { waiter.deferred.await() }
        } finally {
            waiters.remove(waiter)
        }
    }

    override fun close() {
        if (closed) return
        closed = true
        scope.cancel()
        markClosed()
    }

    private fun markClosed() {
        closed = true
        isConnected = false
        runCatching { socket.close() }
        for (listener in closeListeners) runCatching { listener() }
        for (waiter in waiters) waiter.deferred.cancel()
        waiters.clear()
    }

    companion object {
        private const val TAG = "MyMouseTls"
    }
}
