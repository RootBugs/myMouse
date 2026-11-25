package com.mymouse.android.bridge

import com.mymouse.android.service.InputRouter

/** The one live pinned session; UI reads it instead of pretending to be connected. */
object SessionRegistry {
    @Volatile
    var current: BridgeSession? = null
        private set

    fun install(session: BridgeSession) {
        current?.close()
        current = session
        session.addCloseListener {
            if (current === session) {
                current = null
                InputRouter.revoke(session.peerId)
            }
        }
        InputRouter.authorize(session.peerId, session.fingerprint)
    }

    fun clear() {
        current?.close()
        current = null
        InputRouter.revoke()
    }
}
