package com.mymouse.android.service

import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.util.Log

/**
 * Scaffold for notification/code-copy helper.
 *
 * Real implementation should only extract explicitly requested OTP/code patterns when
 * user enables this feature, and send them through the trusted encrypted channel.
 * It must not exfiltrate arbitrary notifications.
 */
class MyMouseNotificationListener : NotificationListenerService() {
    override fun onNotificationPosted(sbn: StatusBarNotification?) {
        // Scaffold: no-op. Avoid reading/sending notification content by default.
        Log.d(TAG, "Notification posted (scaffold no-op): ${sbn?.packageName}")
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification?) {
        // no-op
    }

    companion object {
        private const val TAG = "MyMouseNotif"
    }
}
