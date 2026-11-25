package com.mymouse.android.ui

import android.os.Bundle
import android.view.MotionEvent
import android.view.View
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import com.mymouse.android.MyMouseApp
import com.mymouse.android.bridge.BridgeSession
import com.mymouse.android.bridge.InputEventCodec
import com.mymouse.android.bridge.SessionRegistry
import com.mymouse.android.bridge.WireMessage
import com.mymouse.android.databinding.ActivityRemoteScreenBinding
import kotlinx.coroutines.launch

/**
 * Remote video decoding is NOT implemented in this build (no decoder/renderer
 * dependency is present). What is real here: touch gestures on this view are
 * encoded with the documented MINP/1 payload and sent as v2 InputEvent frames
 * over the currently paired pinned TLS session. No local input is silently
 * discarded while claiming a connection exists.
 */
class RemoteScreenActivity : AppCompatActivity() {
    private lateinit var binding: ActivityRemoteScreenBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityRemoteScreenBinding.inflate(layoutInflater)
        setContentView(binding.root)

        val session = SessionRegistry.current
        val device = session?.let { (application as MyMouseApp).trustStore.get(it.peerId) }
        if (session == null || !session.isConnected || device == null ||
            device.fingerprint != session.fingerprint ||
            !device.capabilities.screen || !device.capabilities.input
        ) {
            binding.remotePlaceholder.text =
                "Remote screen streaming is not implemented in this build. " +
                    "Touch input is disabled: pair with a pinned, trusted host that allows input and screen first."
            return
        }

        binding.remotePlaceholder.text =
            "Remote screen video decoding is not implemented in this build. " +
                "Touch here to send real v2 input events to ${session.peerId}."
        binding.remotePlaceholder.setOnTouchListener { _: View, event: MotionEvent ->
            val code = when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> InputEventCodec.Kind.DOWN
                MotionEvent.ACTION_MOVE -> InputEventCodec.Kind.MOVE
                MotionEvent.ACTION_UP -> InputEventCodec.Kind.UP
                else -> null
            }
            if (code != null) {
                val payload = InputEventCodec.encode(
                    InputEventCodec.Event(
                        kind = code,
                        pointerId = 0,
                        buttons = if (code == InputEventCodec.Kind.DOWN || code == InputEventCodec.Kind.UP) 1 else 0,
                        x = event.x.toInt(),
                        y = event.y.toInt(),
                    )
                )
                lifecycleScope.launch { sendInput(session, payload) }
            }
            true
        }
    }

    private suspend fun sendInput(session: BridgeSession, payload: ByteArray) {
        try {
            session.send(WireMessage.InputEvent(target = session.peerId, payload = payload))
        } catch (e: Exception) {
            binding.remotePlaceholder.text =
                "Input send failed (${e.javaClass.simpleName}: ${e.message}); connection may be closed."
        }
    }
}
