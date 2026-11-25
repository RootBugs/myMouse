package com.mymouse.android.service

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import com.mymouse.android.bridge.InputEventCodec
import com.mymouse.android.bridge.InputEventSink

/**
 * Injects host input only when:
 *  - the user explicitly enabled this accessibility service (Android binding),
 *  - the local "Allow input injection" toggle is on,
 *  - the input came from the currently pinned trusted session,
 *  - the rate limiter in [InputRouter] allows it, and
 *  - the sticky panic switch is not active.
 *
 * Public Accessibility APIs can dispatch gestures and global actions. Raw key
 * injection is NOT possible through them, so keyCode is decoded defensively but
 * deliberately not injected; a small whitelist maps to global actions instead.
 * Hardware injection is not verified in this environment.
 */
class MyMouseAccessibilityService : AccessibilityService() {

    private val gestureLock = Any()
    private var pendingDown: InputEventCodec.Event? = null
    private var lastPoint: Pair<Int, Int>? = null

    override fun onServiceConnected() {
        super.onServiceConnected()
        InputRouter.boundService = this
        InputEventSink.handler = { target, payload, fingerprint ->
            InputRouter.onHostInput(target, payload, fingerprint)
        }
        Log.i(TAG, "Accessibility service enabled; awaiting pinned trusted input")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // Window content is never read; input injection does not require it.
    }

    override fun onInterrupt() {
        cancelPendingGesture()
        Log.i(TAG, "Accessibility service interrupted")
    }

    override fun onDestroy() {
        InputEventSink.handler = null
        if (InputRouter.boundService === this) InputRouter.boundService = null
        super.onDestroy()
    }

    /** Discards any half-finished gesture; dispatched gestures cannot be revoked via public API. */
    fun cancelPendingGesture() {
        synchronized(gestureLock) {
            pendingDown = null
            lastPoint = null
        }
    }

    /** Only called by InputRouter after all authorization and rate-limit checks. */
    fun injectAuthorized(event: InputEventCodec.Event) {
        if (!isInputEnabled() || InputRouter.isPanicDisabled) {
            Log.w(TAG, "Input injection disabled by user setting or panic")
            return
        }
        if (event.keyCode != 0) {
            Log.w(TAG, "keyCode=${event.keyCode} decoded but raw key injection is not available via public API")
        }
        when (event.kind) {
            InputEventCodec.Kind.MOVE -> {
                val from = lastPoint ?: (event.x to event.y)
                dispatchStroke(from.first, from.second, event.x, event.y, 16)
                lastPoint = event.x to event.y
            }
            InputEventCodec.Kind.DOWN -> synchronized(gestureLock) { pendingDown = event }
            InputEventCodec.Kind.UP -> {
                val down = synchronized(gestureLock) {
                    val value = pendingDown
                    pendingDown = null
                    value
                }
                if (down != null) {
                    dispatchStroke(down.x, down.y, event.x, event.y, 60)
                } else {
                    dispatchStroke(event.x, event.y, event.x, event.y, 40)
                }
                lastPoint = event.x to event.y
            }
            InputEventCodec.Kind.SWIPE -> {
                val from = lastPoint ?: (event.x to event.y)
                dispatchStroke(from.first, from.second, event.x, event.y, 200)
                lastPoint = event.x to event.y
            }
            InputEventCodec.Kind.HOME -> performGlobalAction(GLOBAL_ACTION_HOME)
            InputEventCodec.Kind.BACK -> performGlobalAction(GLOBAL_ACTION_BACK)
            InputEventCodec.Kind.RECENTS -> performGlobalAction(GLOBAL_ACTION_RECENTS)
        }
    }

    private fun isInputEnabled(): Boolean =
        getSharedPreferences("mymouse_settings", MODE_PRIVATE).getBoolean("cap_input", true)

    private fun dispatchStroke(x0: Int, y0: Int, x1: Int, y1: Int, durationMs: Long): Boolean {
        val path = Path().apply {
            moveTo(x0.toFloat(), y0.toFloat())
            // A zero-length stroke is rejected by GestureDescription.
            lineTo(x1.toFloat() + 0.1f, y1.toFloat() + 0.1f)
        }
        val stroke = GestureDescription.StrokeDescription(path, 0, durationMs)
        val gesture = GestureDescription.Builder().addStroke(stroke).build()
        val accepted = dispatchGesture(gesture, null, null)
        if (!accepted) Log.w(TAG, "Gesture rejected by Android (duration/path/enablement)")
        return accepted
    }

    companion object {
        private const val TAG = "MyMouseA11y"
    }
}
