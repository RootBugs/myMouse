package com.mymouse.android.ui

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import com.mymouse.android.databinding.ActivityRemoteScreenBinding

/**
 * Phase 3 scaffold placeholder.
 * Real implementation: connect to trusted host over pinned TLS, request LAN screen
 * stream, decode H.264 frames, render to SurfaceView/TextureView, and send touch/key
 * input back through protocol messages.
 */
class RemoteScreenActivity : AppCompatActivity() {
    private lateinit var binding: ActivityRemoteScreenBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityRemoteScreenBinding.inflate(layoutInflater)
        setContentView(binding.root)
    }
}
