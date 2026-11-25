package com.mymouse.android.ui

import android.net.Uri
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import com.mymouse.android.MyMouseApp
import com.mymouse.android.bridge.BridgeSession
import com.mymouse.android.bridge.FileChunk
import com.mymouse.android.bridge.FileOffer
import com.mymouse.android.bridge.SessionRegistry
import com.mymouse.android.bridge.WireMessage
import com.mymouse.android.databinding.ActivityFileTransferBinding
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.launch
import java.io.ByteArrayOutputStream
import java.io.File
import java.security.MessageDigest
import java.util.UUID

/**
 * Real v2 file transfer over the current pinned session. There is no "simulate"
 * button: both directions refuse to run unless a trusted, pinned, files-enabled
 * host is connected. Chunking, SHA-256 verification and path sanitization are
 * performed locally.
 */
class FileTransferActivity : AppCompatActivity() {
    private lateinit var binding: ActivityFileTransferBinding
    private val maxBytes = 64L * 1024 * 1024
    private val chunkSize = 32 * 1024

    private val pickFile = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) sendFile(uri) else binding.fileStatus.text = "No file selected; nothing sent."
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityFileTransferBinding.inflate(layoutInflater)
        setContentView(binding.root)

        binding.btnPickSend.setOnClickListener { pickFile.launch(arrayOf("*/*")) }
        binding.btnAcceptReceive.setOnClickListener { startReceive() }
        updateConnectionHint()
    }

    override fun onResume() {
        super.onResume()
        updateConnectionHint()
    }

    private fun updateConnectionHint() {
        val session = SessionRegistry.current
        if (session == null || !session.isConnected) {
            binding.fileStatus.text =
                "No pinned connection. Pair with a host first; no transfer will be attempted."
        }
    }

    private fun requireSession(): BridgeSession? {
        val session = SessionRegistry.current
        if (session == null || !session.isConnected) {
            binding.fileStatus.text = "No pinned connection. Pair with a host first."
            return null
        }
        val device = (application as MyMouseApp).trustStore.get(session.peerId)
        if (device == null || device.fingerprint != session.fingerprint || !device.capabilities.files) {
            binding.fileStatus.text =
                "Connected host is not trusted for file transfer with this fingerprint. Nothing sent."
            return null
        }
        return session
    }

    private fun localDeviceId(): String =
        android.provider.Settings.Secure.getString(contentResolver, android.provider.Settings.Secure.ANDROID_ID)
            ?: "android-device"

    private fun sendFile(uri: Uri) {
        val session = requireSession() ?: return
        lifecycleScope.launch {
            try {
                val data = ByteArrayOutputStream()
                val digest = MessageDigest.getInstance("SHA-256")
                contentResolver.openInputStream(uri)?.use { input ->
                    val buffer = ByteArray(chunkSize)
                    while (true) {
                        val read = input.read(buffer)
                        if (read <= 0) break
                        if (data.size() + read > maxBytes) throw Exception("File exceeds 64 MiB limit")
                        data.write(buffer, 0, read)
                        digest.update(buffer, 0, read)
                    }
                } ?: throw Exception("Could not open the selected file")
                val bytes = data.toByteArray()
                if (bytes.isEmpty()) throw Exception("Empty files are not sent")
                val name = uri.lastPathSegment?.take(120)?.replace(Regex("[^A-Za-z0-9._-]"), "_")
                    ?: "file.bin"
                val transferId = UUID.randomUUID().toString()
                val offer = FileOffer(
                    transferId = transferId,
                    fromDeviceId = localDeviceId(),
                    name = name,
                    size = bytes.size.toLong(),
                    sha256 = hex(digest.digest()),
                    mime = contentResolver.getType(uri),
                )
                session.send(WireMessage.FileTransferOffer(offer))
                binding.fileStatus.text = "Offered $name (${bytes.size} bytes); waiting for host acceptance..."
                val answer = session.awaitMessage(
                    {
                        it is WireMessage.FileTransferAccept && it.transferId == transferId ||
                            it is WireMessage.FileTransferReject && it.transferId == transferId ||
                            it is WireMessage.FileTransferCancel && it.transferId == transferId
                    },
                    30_000,
                )
                when (answer) {
                    is WireMessage.FileTransferAccept -> Unit
                    is WireMessage.FileTransferReject -> {
                        binding.fileStatus.text = "Host rejected transfer: ${answer.reason ?: "no reason given"}"
                        return@launch
                    }
                    is WireMessage.FileTransferCancel -> {
                        binding.fileStatus.text = "Host cancelled transfer: ${answer.reason ?: "no reason given"}"
                        return@launch
                    }
                    else -> throw Exception("Unexpected response to file offer")
                }
                var offset = 0
                while (offset < bytes.size) {
                    val end = minOf(offset + chunkSize, bytes.size)
                    session.send(
                        WireMessage.FileTransferChunk(
                            FileChunk(transferId, offset.toLong(), bytes.copyOfRange(offset, end), end == bytes.size)
                        )
                    )
                    binding.fileStatus.text = "Sending $name: $end / ${bytes.size} bytes"
                    offset = end
                }
                session.send(WireMessage.FileTransferComplete(transferId))
                binding.fileStatus.text = "Sent $name to ${session.peerId} (SHA-256 verified by host)."
            } catch (e: TimeoutCancellationException) {
                binding.fileStatus.text = "Transfer timed out; no completion claimed."
            } catch (e: Exception) {
                binding.fileStatus.text = "Send failed: ${e.javaClass.simpleName}: ${e.message}"
            }
        }
    }

    private fun startReceive() {
        val session = requireSession() ?: return
        binding.btnAcceptReceive.isEnabled = false
        binding.fileStatus.text = "Waiting up to 60 s for an incoming file offer..."
        lifecycleScope.launch {
            try {
                val first = session.awaitMessage({ it is WireMessage.FileTransferOffer }, 60_000)
                val offer = (first as WireMessage.FileTransferOffer).offer
                if (offer.fromDeviceId != session.peerId || offer.size > maxBytes) {
                    session.send(WireMessage.FileTransferReject(offer.transferId, "untrusted sender or too large"))
                    binding.fileStatus.text = "Rejected offer from ${offer.fromDeviceId}."
                    return@launch
                }
                session.send(WireMessage.FileTransferAccept(offer.transferId))
                val payload = ByteArrayOutputStream()
                var received = 0L
                while (true) {
                    val message = session.awaitMessage(
                        {
                            it is WireMessage.FileTransferChunk && it.chunk.transferId == offer.transferId ||
                                it is WireMessage.FileTransferComplete && it.transferId == offer.transferId ||
                                it is WireMessage.FileTransferCancel && it.transferId == offer.transferId
                        },
                        60_000,
                    )
                    when (message) {
                        is WireMessage.FileTransferChunk -> {
                            val chunk = message.chunk
                            if (chunk.offset != received || received + chunk.data.size > maxBytes) {
                                throw Exception("Invalid chunk offset or oversized transfer")
                            }
                            payload.write(chunk.data)
                            received += chunk.data.size
                            binding.fileStatus.text = "Receiving ${offer.name}: $received / ${offer.size} bytes"
                            if (chunk.last) break
                        }
                        is WireMessage.FileTransferComplete -> break
                        is WireMessage.FileTransferCancel -> {
                            throw Exception("Sender cancelled: ${message.reason ?: "no reason given"}")
                        }
                        else -> throw Exception("Unexpected transfer message")
                    }
                }
                val bytes = payload.toByteArray()
                if (received != offer.size || hex(MessageDigest.getInstance("SHA-256").digest(bytes)) != offer.sha256) {
                    throw Exception("Size or SHA-256 mismatch; file was not saved")
                }
                val safeName = offer.name.take(120).replace(Regex("[^A-Za-z0-9._-]"), "_")
                val dir = getExternalFilesDir("received") ?: filesDir
                val output = File(dir, "${offer.transferId.take(8)}-$safeName")
                output.writeBytes(bytes)
                session.send(WireMessage.FileTransferComplete(offer.transferId))
                binding.fileStatus.text = "Received and verified ${output.name} ($received bytes)."
            } catch (e: TimeoutCancellationException) {
                binding.fileStatus.text = "Receive timed out; no file was saved."
            } catch (e: Exception) {
                binding.fileStatus.text = "Receive failed: ${e.javaClass.simpleName}: ${e.message}"
            } finally {
                binding.btnAcceptReceive.isEnabled = true
            }
        }
    }

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }
}
