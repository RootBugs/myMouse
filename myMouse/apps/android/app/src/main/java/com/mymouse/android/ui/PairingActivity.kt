package com.mymouse.android.ui

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import com.google.zxing.integration.android.IntentIntegrator
import com.mymouse.android.MyMouseApp
import com.mymouse.android.bridge.BridgeProvider
import com.mymouse.android.data.TrustedDevice
import com.mymouse.android.databinding.ActivityPairingBinding

class PairingActivity : AppCompatActivity() {
    private lateinit var binding: ActivityPairingBinding
    private val bridge = BridgeProvider.get()
    private var pendingCode: String? = null
    private var pendingHostId: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityPairingBinding.inflate(layoutInflater)
        setContentView(binding.root)

        binding.btnScanQr.setOnClickListener {
            IntentIntegrator(this)
                .setDesiredBarcodeFormats(IntentIntegrator.QR_CODE)
                .setPrompt("Scan myMouse pairing QR")
                .setBeepEnabled(false)
                .initiateScan()
        }

        binding.btnSubmitCode.setOnClickListener {
            val code = binding.codeInput.text?.toString()?.trim().orEmpty()
            val hostId = binding.hostIdInput.text?.toString()?.trim().orEmpty()
            if (code.length != 6 || hostId.isBlank()) {
                binding.pairingResult.text = "Enter a 6-digit code and host device id."
                return@setOnClickListener
            }
            pendingCode = code
            pendingHostId = hostId
            binding.pairingResult.text = "Code captured for host $hostId. Confirm host identity, then tap Approve pairing."
        }

        binding.btnApprove.setOnClickListener {
            val code = pendingCode ?: binding.codeInput.text?.toString()?.trim().orEmpty()
            val hostId = pendingHostId ?: binding.hostIdInput.text?.toString()?.trim().orEmpty()
            if (code.length != 6 || hostId.isBlank()) {
                binding.pairingResult.text = "No pending pairing. Scan QR or enter code first."
                return@setOnClickListener
            }
            // Scaffold: in real flow, host displays code and user confirms match after TLS/pairing handshake.
            // We record a local trusted device placeholder with explicit approval.
            val app = application as MyMouseApp
            app.trustStore.upsert(
                TrustedDevice(
                    deviceId = hostId,
                    name = "Paired host ($hostId)",
                    fingerprint = null, // Real flow pins cert fingerprint after handshake.
                )
            )
            binding.pairingResult.text =
                "Approved locally (scaffold). Stored trusted device $hostId. Real implementation must verify host code/fingerprint over pinned TLS before granting capabilities."
        }

        intent.getStringExtra(EXTRA_PAIRING_URI)?.let { handleUri(it) }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: android.content.Intent?) {
        val result = IntentIntegrator.parseActivityResult(requestCode, resultCode, data)
        if (result != null) {
            val contents = result.contents ?: return
            handleUri(contents)
            return
        }
        super.onActivityResult(requestCode, resultCode, data)
    }

    private fun handleUri(uri: String) {
        val parsed = bridge.parsePairingUri(uri)
        if (parsed == null) {
            binding.pairingResult.text = "Unsupported QR payload: $uri"
            return
        }
        val (code, id) = parsed
        binding.codeInput.setText(code)
        binding.hostIdInput.setText(id)
        pendingCode = code
        pendingHostId = id
        binding.pairingResult.text = "QR parsed for host $id. Verify host identity and approve."
    }

    companion object {
        const val EXTRA_PAIRING_URI = "pairing_uri"
    }
}
