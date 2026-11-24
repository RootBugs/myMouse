package com.mymouse.android

import android.app.Application
import com.mymouse.android.data.TrustedDeviceStore

class MyMouseApp : Application() {
    lateinit var trustStore: TrustedDeviceStore
        private set

    override fun onCreate() {
        super.onCreate()
        trustStore = TrustedDeviceStore(this)
    }
}
