package com.mymouse.android.data

data class Capabilities(
    val input: Boolean = true,
    val clipboard: Boolean = true,
    val files: Boolean = true,
    val screen: Boolean = true,
)

data class TrustedDevice(
    val deviceId: String,
    val name: String,
    val capabilities: Capabilities = Capabilities(),
    val fingerprint: String? = null,
    val pairedAtUnixMs: Long = System.currentTimeMillis(),
)

data class PairingSessionUiState(
    val hostDeviceId: String = "",
    val code: String = "",
    val approved: Boolean = false,
    val message: String = "",
)
