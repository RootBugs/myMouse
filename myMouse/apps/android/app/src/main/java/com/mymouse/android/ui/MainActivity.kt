package com.mymouse.android.ui

import android.content.Intent
import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import com.mymouse.android.bridge.BridgeProvider
import com.mymouse.android.bridge.SessionRegistry
import com.mymouse.android.databinding.ActivityMainBinding
import com.mymouse.android.service.InputRouter

class MainActivity : AppCompatActivity() {
    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        val bridge = BridgeProvider.get()
        val session = SessionRegistry.current
        binding.status.text = buildString {
            append("Bridge: ${bridge.implementationName()} · protocol v${bridge.protocolVersion()}\n")
            if (session?.isConnected == true) {
                append("Connected to ${session.peerId} (pinned ${session.fingerprint.take(12)}…)\n")
            } else {
                append("Disconnected — pair with a host to enable control\n")
            }
            if (InputRouter.isPanicDisabled) append("INPUT PANIC-DISABLED — re-enable in Settings\n")
            append("No APK built / no device verification in this environment.")
        }

        binding.btnPairing.setOnClickListener { startActivity(Intent(this, PairingActivity::class.java)) }
        binding.btnTrusted.setOnClickListener { startActivity(Intent(this, TrustedDevicesActivity::class.java)) }
        binding.btnFiles.setOnClickListener { startActivity(Intent(this, FileTransferActivity::class.java)) }
        binding.btnRemote.setOnClickListener { startActivity(Intent(this, RemoteScreenActivity::class.java)) }
        binding.btnSettings.setOnClickListener { startActivity(Intent(this, SettingsActivity::class.java)) }

        handlePairingIntent(intent)
    }

    override fun onResume() {
        super.onResume()
        // Session state can change while another activity is in front.
        val session = SessionRegistry.current
        binding.status.text = buildString {
            append("Bridge: ${BridgeProvider.get().implementationName()}\n")
            append(
                if (session?.isConnected == true) "Connected to ${session.peerId}\n"
                else "Disconnected — pair with a host to enable control\n"
            )
            if (InputRouter.isPanicDisabled) append("INPUT PANIC-DISABLED\n")
        }
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
