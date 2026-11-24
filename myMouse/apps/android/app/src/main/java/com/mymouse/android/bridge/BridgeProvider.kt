package com.mymouse.android.bridge

/**
 * Provides the bridge implementation. In production, attempt to load UniFFI native
 * library and return generated binding wrapper; fallback to StubMyMouseBridge for dev.
 */
object BridgeProvider {
    private val stub by lazy { StubMyMouseBridge() }

    fun get(): MyMouseBridge {
        // TODO(phase3): Load native libmymouse_android_bridge.so and return UniFFI wrapper.
        // System.loadLibrary("mymouse_android_bridge")
        return stub
    }
}
