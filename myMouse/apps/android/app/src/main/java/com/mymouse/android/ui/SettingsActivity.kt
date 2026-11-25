package com.mymouse.android.ui

import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import androidx.appcompat.app.AppCompatActivity
import com.mymouse.android.bridge.SessionRegistry
import com.mymouse.android.databinding.ActivitySettingsBinding
import com.mymouse.android.service.InputRouter

class SettingsActivity : AppCompatActivity() {
    private lateinit var binding: ActivitySettingsBinding
    private val prefs by lazy { getSharedPreferences("mymouse_settings", MODE_PRIVATE) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivitySettingsBinding.inflate(layoutInflater)
        setContentView(binding.root)

        bindSwitch(binding.switchInput, "cap_input", true)
        bindSwitch(binding.switchClipboard, "cap_clipboard", true)
        bindSwitch(binding.switchFiles, "cap_files", true)
        bindSwitch(binding.switchScreen, "cap_screen", true)
        bindSwitch(binding.switchRequireApproval, "files_require_approval", true)

        binding.btnOpenAccessibility.setOnClickListener {
            startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
        }
        binding.btnOpenNotifications.setOnClickListener {
            startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
        }
        binding.btnPanicDisable.setOnClickListener {
            InputRouter.panicDisable("user pressed the panic button")
            SessionRegistry.clear() // close the pinned socket as well
            binding.settingsStatus.text =
                "PANIC: input injection disabled and pinned session closed. Re-pair explicitly to reconnect."
        }
        binding.btnResumeInput.setOnClickListener {
            InputRouter.clearPanic()
            binding.settingsStatus.text =
                "Panic cleared. Input still requires Accessibility enablement, a paired pinned host, and the input toggle."
        }
        if (InputRouter.isPanicDisabled) {
            binding.settingsStatus.text =
                "Input is currently panic-disabled (${InputRouter.panicReasonText}). Use Re-enable input to clear."
        }
    }

    private fun bindSwitch(
        switch: com.google.android.material.switchmaterial.SwitchMaterial,
        key: String,
        default: Boolean,
    ) {
        switch.isChecked = prefs.getBoolean(key, default)
        switch.setOnCheckedChangeListener { _, checked ->
            prefs.edit().putBoolean(key, checked).apply()
            binding.settingsStatus.text = "Saved $key=$checked locally."
        }
    }
}
