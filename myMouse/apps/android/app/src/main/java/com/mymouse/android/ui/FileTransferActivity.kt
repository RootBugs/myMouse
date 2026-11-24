package com.mymouse.android.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import com.mymouse.android.databinding.ActivityFileTransferBinding

class FileTransferActivity : AppCompatActivity() {
    private lateinit var binding: ActivityFileTransferBinding

    private val pickFile = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        binding.fileStatus.text = if (uri != null) {
            "Selected $uri. Scaffold: real flow would chunk, encrypt, and send over pinned TLS after capability/approval checks."
        } else {
            "No file selected."
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityFileTransferBinding.inflate(layoutInflater)
        setContentView(binding.root)

        binding.btnPickSend.setOnClickListener {
            pickFile.launch(arrayOf("*/*"))
        }
        binding.btnAcceptReceive.setOnClickListener {
            binding.fileStatus.text = "Scaffold: receive approval recorded. Real flow must verify sender trust, write to user-selected location, and report transfer status."
        }
    }
}
