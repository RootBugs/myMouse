package com.mymouse.android.service

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.util.Log
import android.view.accessibility.AccessibilityEvent

/**
 * Receives myMouse input events and injects them into Android.
 *
 * SCAFFOLD: This service declares the required capability and documents the injection path.
 * Real implementation must:
 *  - authenticate the connected host (trusted device + pinned TLS),
 *  - decode protocol InputEvent payloads,
 *  - map to gestures/key events with rate limiting and safety checks,
 *  - expose a clear "panic disable" path and never run without explicit user enablement.
 */
class MyMouseAccessibilityService : AccessibilityService() {

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        // Not used for injection. We do not inspect window content in scaffold.
    }

    override fun onInterrupt() {
        Log.i(TAG, "Accessibility service interrupted")
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        Log.i(TAG, "myMouse Accessibility service connected. Waiting for trusted host input.")
    }

    fun injectTap(x: Float, y: Float) {
        val path = Path().apply { moveTo(x, y) }
        val stroke = GestureDescription.StrokeDescription(path, 0, 50)
        val gesture = GestureDescription.Builder().addStroke(stroke).build()
        dispatchGesture(gesture, null, null)
    }

    companion object {
        private const val TAG = "MyMouseA11y"
    }
}
