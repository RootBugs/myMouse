package com.mymouse.android.data

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Local trusted device storage. Scaffold implementation uses SharedPreferences + JSON.
 * Production should use encrypted storage and align with the Rust bridge trust model.
 */
class TrustedDeviceStore(context: Context) {
    private val prefs = context.getSharedPreferences("mymouse_trust_store", Context.MODE_PRIVATE)

    fun list(): List<TrustedDevice> {
        val raw = prefs.getString(KEY_DEVICES, "[]") ?: "[]"
        val arr = JSONArray(raw)
        return (0 until arr.length()).mapNotNull { idx ->
            runCatching { fromJson(arr.getJSONObject(idx)) }.getOrNull()
        }.sortedBy { it.name }
    }

    fun upsert(device: TrustedDevice) {
        val devices = list().filterNot { it.deviceId == device.deviceId } + device
        save(devices)
    }

    fun revoke(deviceId: String) {
        save(list().filterNot { it.deviceId == deviceId })
    }

    fun get(deviceId: String): TrustedDevice? = list().firstOrNull { it.deviceId == deviceId }

    private fun save(devices: List<TrustedDevice>) {
        val arr = JSONArray()
        devices.forEach { arr.put(toJson(it)) }
        prefs.edit().putString(KEY_DEVICES, arr.toString()).apply()
    }

    private fun toJson(d: TrustedDevice): JSONObject = JSONObject().apply {
        put("deviceId", d.deviceId)
        put("name", d.name)
        put("fingerprint", d.fingerprint)
        put("pairedAtUnixMs", d.pairedAtUnixMs)
        put("capabilities", JSONObject().apply {
            put("input", d.capabilities.input)
            put("clipboard", d.capabilities.clipboard)
            put("files", d.capabilities.files)
            put("screen", d.capabilities.screen)
        })
    }

    private fun fromJson(o: JSONObject): TrustedDevice {
        val caps = o.optJSONObject("capabilities") ?: JSONObject()
        return TrustedDevice(
            deviceId = o.getString("deviceId"),
            name = o.optString("name", o.getString("deviceId")),
            fingerprint = if (o.has("fingerprint") && !o.isNull("fingerprint")) o.getString("fingerprint") else null,
            pairedAtUnixMs = o.optLong("pairedAtUnixMs", System.currentTimeMillis()),
            capabilities = Capabilities(
                input = caps.optBoolean("input", true),
                clipboard = caps.optBoolean("clipboard", true),
                files = caps.optBoolean("files", true),
                screen = caps.optBoolean("screen", true),
            ),
        )
    }

    companion object {
        private const val KEY_DEVICES = "trusted_devices"
    }
}
