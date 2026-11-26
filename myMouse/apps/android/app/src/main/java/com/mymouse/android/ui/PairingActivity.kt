package com.mymouse.android.ui

import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import com.google.zxing.integration.android.IntentIntegrator
import com.mymouse.android.MyMouseApp
import com.mymouse.android.bridge.BridgeConnectRequest
import com.mymouse.android.bridge.BridgeProvider
import com.mymouse.android.bridge.DEFAULT_MYMOUSE_PORT
import com.mymouse.android.bridge.SessionRegistry
import com.mymouse.android.data.Capabilities
import com.mymouse.android.data.TrustedDevice
import com.mymouse.android.databinding.ActivityPairingBinding
import kotlinx.coroutines.launch

class PairingActivity : AppCompatActivity() {
    private lateinit var binding: ActivityPairingBinding
    private val bridge = BridgeProvider.get()
    private var pendingCode: String? = null
    private var pendingHostId: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityPairingBinding.inflate(layoutInflater)
        setContentView(binding.root)

        if (bridge.implementationName().startsWith("STUB")) {
            binding.pairingResult.text =
                "Network pairing is unavailable: ${bridge.implementationName()}. No trust entry was created."
        }

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
            if (!validCode(code) || hostId.isBlank()) {
                binding.pairingResult.text = "Enter a 6-digit code and host device id."
                return@setOnClickListener
            }
            pendingCode = code
            pendingHostId = hostId
            binding.pairingResult.text =
                "Code captured. Enter the host address, port and certificate SHA-256 fingerprint, then connect to pair."
        }

        binding.btnApprove.setOnClickListener { startPairing() }

        intent.getStringExtra(EXTRA_PAIRING_URI)?.let { handleUri(it) }
    }

    private fun startPairing() {
        val code = pendingCode ?: binding.codeInput.text?.toString()?.trim().orEmpty()
        val hostId = pendingHostId ?: binding.hostIdInput.text?.toString()?.trim().orEmpty()
        val address = binding.hostAddressInput.text?.toString()?.trim().orEmpty()
        val port = binding.hostPortInput.text?.toString()?.trim()?.toIntOrNull() ?: DEFAULT_MYMOUSE_PORT
        val rawFingerprint = binding.fingerprintInput.text?.toString()?.trim().orEmpty()
        val fingerprint = bridge.normalizeFingerprint(rawFingerprint)

        if (!validCode(code) || hostId.isBlank() || address.isBlank()) {
            binding.pairingResult.text = "Fill in code, host id and host address before pairing."
            return
        }
        if (fingerprint == null) {
            binding.pairingResult.text =
                "A valid 64-hex SHA-256 certificate fingerprint is required. No connection was attempted."
            return
        }
        if (!bridge.verifyCode(code, code)) {
            binding.pairingResult.text = "The pairing code must be six digits."
            return
        }

        pendingCode = code
        pendingHostId = hostId
        binding.pairingResult.text = "Connecting to $address:$port with certificate pinning..."
        val localId = Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID)
            ?.takeIf { it.isNotBlank() } ?: "android-device"
        val localName = Build.MODEL.takeIf { it.isNotBlank() } ?: "Android"
        val prefs = getSharedPreferences("mymouse_settings", MODE_PRIVATE)
        val caps = Capabilities(
            input = prefs.getBoolean("cap_input", true),
            clipboard = prefs.getBoolean("cap_clipboard", true),
            files = prefs.getBoolean("cap_files", true),
            screen = prefs.getBoolean("cap_screen", true),
        )

        lifecycleScope.launch {
            try {
                val session = bridge.connectAndPair(
                    BridgeConnectRequest(
                        host = address,
                        port = port,
                        peerId = hostId,
                        peerName = "Paired host ($hostId)",
                        pinnedFingerprint = fingerprint,
                        pairingCode = code,
                        deviceId = localId,
                        deviceName = localName,
                    )
                )
                (application as MyMouseApp).trustStore.upsert(
                    TrustedDevice(
                        deviceId = hostId,
                        name = "Paired host ($hostId)",
                        capabilities = caps,
                        fingerprint = fingerprint,
                    )
                )
                SessionRegistry.install(session)
                binding.pairingResult.text =
                    "Paired with $hostId over pinned TLS (${bridge.implementationName()}). " +
                        "Host approved the code; fingerprint $fingerprint is now stored."
            } catch (e: Exception) {
                SessionRegistry.clear()
                binding.pairingResult.text =
                    "Pairing failed (no trust entry stored): ${e.javaClass.simpleName}: ${e.message}"
            }
        }
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

    private fun handleUri(raw: String) {
        val parsed = bridge.parsePairingUri(raw)
        if (parsed == null) {
            binding.pairingResult.text = "Unsupported QR payload: $raw"
            return
        }
        val (code, id) = parsed
        binding.codeInput.setText(code)
        binding.hostIdInput.setText(id)
        pendingCode = code
        pendingHostId = id

        // Android-side deep-link hints; ignored by the Rust pairing_uri() parser.
        val uri = Uri.parse(raw)
        if (uri.scheme != "mymouse" || uri.host != "pair") {
            binding.pairingResult.text = "QR payload is not a myMouse pairing URI."
            return
        }
        uri.getQueryParameter("addr")?.let { binding.hostAddressInput.setText(it) }
        uri.getQueryParameter("port")?.toIntOrNull()?.let { binding.hostPortInput.setText(it.toString()) }
        uri.getQueryParameter("fp")?.let { binding.fingerprintInput.setText(it) }
        binding.pairingResult.text =
            "QR parsed for host $id. Verify the fingerprint on the host, then connect to pair."
    }

    private fun validCode(code: String): Boolean =
        code.length == 6 && code.all { it in '0'..'9' }

    companion object {
        const val EXTRA_PAIRING_URI = "pairing_uri"
    }
}
