package com.mymouse.android.service

import android.os.SystemClock
import android.util.Log
import com.mymouse.android.bridge.InputEventCodec
import com.mymouse.android.bridge.InputEventDecodeException
import com.mymouse.android.bridge.WireProtocol

/**
 * Security gate in front of accessibility injection. No input is injected
 * unless it arrives from the currently authorized pinned session, the user has
 * enabled the accessibility service, the local capability toggle is on, and a
 * rate limiter permits it. Panic disable is sticky and requires an explicit
 * user action to clear.
 */
object InputRouter {
    private const val TAG = "MyMouseInput"
    private const val MAX_PER_SECOND = 60
    private const val BURST = 8

    @Volatile
    private var authorizedPeer: String? = null

    @Volatile
    private var authorizedFingerprint: String? = null

    @Volatile
    private var panicDisabled = false

    @Volatile
    private var panicReason = ""

    @Volatile
    var boundService: MyMouseAccessibilityService? = null

    private val rateLock = Any()
    private var tokens = BURST.toDouble()
    private var lastRefillMs = SystemClock.elapsedRealtime()

    val isPanicDisabled: Boolean get() = panicDisabled
    val panicReasonText: String get() = panicReason

    fun authorize(peerId: String, fingerprint: String) {
        synchronized(this) {
            authorizedPeer = peerId
            authorizedFingerprint = fingerprint
        }
        Log.i(TAG, "Input authorized for pinned peer $peerId")
    }

    fun revoke(peerId: String? = null) {
        synchronized(this) {
            if (peerId == null || peerId == authorizedPeer) {
                authorizedPeer = null
                authorizedFingerprint = null
            }
        }
    }

    /** Panic path: immediately stop accepting input; clearing needs explicit user action. */
    fun panicDisable(reason: String) {
        panicReason = reason
        panicDisabled = true
        revoke()
        boundService?.cancelPendingGesture()
        Log.w(TAG, "PANIC disable: $reason")
    }

    fun clearPanic() {
        panicDisabled = false
        panicReason = ""
        Log.i(TAG, "Panic cleared by explicit user action")
    }

    fun isAuthorized(peerId: String, fingerprint: String): Boolean {
        if (panicDisabled) return false
        val expectedPeer = authorizedPeer ?: return false
        val expectedFingerprint = authorizedFingerprint ?: return false
        return peerId == expectedPeer &&
            WireProtocol.constantTimeEquals(fingerprint, expectedFingerprint)
    }

    private fun tryAcquire(): Boolean = synchronized(rateLock) {
        val now = SystemClock.elapsedRealtime()
        val elapsed = (now - lastRefillMs).coerceAtLeast(0)
        lastRefillMs = now
        tokens = (tokens + elapsed * MAX_PER_SECOND / 1000.0).coerceAtMost(BURST.toDouble())
        if (tokens >= 1.0) {
            tokens -= 1.0
            true
        } else {
            false
        }
    }

    /** Called from the TLS reader; performs decode and every safety check. */
    fun onHostInput(target: String, payload: ByteArray, fingerprint: String) {
        if (panicDisabled) {
            Log.w(TAG, "Panic active ($panicReason); discarding input for $target")
            return
        }
        val peer = authorizedPeer
        if (peer == null) {
            Log.w(TAG, "No authorized pinned session; discarding input")
            return
        }
        if (!isAuthorized(peer, fingerprint)) {
            Log.w(TAG, "Input fingerprint does not match the trusted peer; discarding")
            return
        }
        val event = try {
            InputEventCodec.decode(payload)
        } catch (e: InputEventDecodeException) {
            Log.w(TAG, "Rejected malformed input: ${e.message}")
            return
        }
        if (!tryAcquire()) {
            Log.w(TAG, "Input rate limit exceeded; discarding")
            return
        }
        val service = boundService
        if (service == null) {
            Log.w(TAG, "Accessibility service not enabled; discarding input")
            return
        }
        service.injectAuthorized(event)
    }
}
