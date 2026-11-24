package com.mymouse.android.ui

import android.content.Intent
import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import com.mymouse.android.bridge.BridgeProvider
import com.mymouse.android.databinding.ActivityMainBinding

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        val bridge = BridgeProvider.get()
        binding.status.text = "Phase 3 scaffold. Protocol v${bridge.protocolVersion()}. " +
            "No Android device verification performed in this environment."

        binding.btnPairing.setOnClickListener { startActivity(Intent(this, PairingActivity::class.java)) }
        binding.btnTrusted.setOnClickListener { startActivity(Intent(this, TrustedDevicesActivity::class.java)) }
        binding.btnFiles.setOnClickListener { startActivity(Intent(this, FileTransferActivity::class.java)) }
        binding.btnRemote.setOnClickListener { startActivity(Intent(this, RemoteScreenActivity::class.java)) }
        binding.btnSettings.setOnClickListener { startActivity(Intent(this, SettingsActivity::class.java)) }

        handlePairingIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handlePairingIntent(intent)
    }

    private fun handlePairingIntent(intent: Intent?) {
        val data = intent?.data ?: return
        if (data.scheme == "mymouse" && data.host == "pair") {
            val i = Intent(this, PairingActivity::class.java).apply {
                putExtra(PairingActivity.EXTRA_PAIRING_URI, data.toString())
            }
            startActivity(i)
        }
    }
}
