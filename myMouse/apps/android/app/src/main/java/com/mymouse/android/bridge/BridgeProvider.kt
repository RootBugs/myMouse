package com.mymouse.android.bridge

import android.util.Log

/**
 * Implementation order:
 *   1. A generated UniFFI/JNI binding, IF the native .so is packaged AND a
 *      generated class implementing [MyMouseBridge] is present on the classpath.
 *      Nothing is fabricated here: without a real generated class we do not
 *      invent JNI signatures or claim the native path works.
 *   2. The real Kotlin pinned-TLS bridge (always available in this source tree).
 *   3. The explicitly-labelled stub, only if even the Kotlin bridge cannot load.
 */
object BridgeProvider {
    private const val TAG = "MyMouseBridge"
    private const val NATIVE_LIBRARY = "mymouse_android_bridge"
    private val GENERATED_CLASSES = listOf(
        "com.mymouse.android.bridge.generated.GeneratedMyMouseBridge",
        "com.mymouse.android.bridge.generated.MyMouseBridgeNative",
        "com.mymouse.android.bridge.generated.MymouseBridge",
    )

    @Volatile
    private var cached: MyMouseBridge? = null

    fun get(): MyMouseBridge = cached ?: synchronized(this) {
        cached ?: resolve().also { cached = it }
    }

    private fun resolve(): MyMouseBridge {
        tryNativeBinding()?.let { native ->
            Log.i(TAG, "Using native bridge binding: ${native.implementationName()}")
            return native
        }
        val kotlinBridge = runCatching { PinnedTlsBridge() }.getOrNull()
        if (kotlinBridge != null) return kotlinBridge
        Log.w(TAG, "Kotlin bridge unavailable; falling back to LAST-RESORT STUB (no network)")
        return StubMyMouseBridge()
    }

    private fun tryNativeBinding(): MyMouseBridge? {
        val loaded = runCatching { System.loadLibrary(NATIVE_LIBRARY) }.isSuccess
        if (!loaded) {
            Log.i(TAG, "No native $NATIVE_LIBRARY in this build; using Kotlin bridge")
            return null
        }
        for (className in GENERATED_CLASSES) {
            val type = runCatching { Class.forName(className) }.getOrNull() ?: continue
            val instance = runCatching { type.getDeclaredConstructor().newInstance() }.getOrNull()
            if (instance is MyMouseBridge) return instance
            Log.w(TAG, "$className does not implement MyMouseBridge; skipped")
        }
        Log.w(
            TAG,
            "Native library loaded but no compatible generated binding was found. " +
                "Generate UniFFI bindings and add them under bridge/generated/ to use the native path.",
        )
        return null
    }
}
