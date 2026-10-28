package com.hivepaas.shellwing

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import com.hivepaas.shellwing.core.SessionStatus

/**
 * Keeps the process, and so its SSH connections, alive while sessions are
 * open and shellwing is not on screen: a foreground service, whose
 * notification lists them and closes them. Android would otherwise stop the
 * app soon after it leaves the screen, as iOS does.
 */
class SessionService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == CLOSE_ALL) {
            (application as ShellwingApp).model.closeAll()
            return START_NOT_STICKY
        }
        startForeground(ID, notification(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        running = true
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        running = false
        super.onDestroy()
    }

    companion object {
        private const val ID = 1
        private const val CHANNEL = "sessions"
        private const val CLOSE_ALL = "com.hivepaas.shellwing.CLOSE_ALL"
        private var running = false

        /** Starts the service, or brings its notification up to date. */
        fun show(context: Context) {
            if (running) {
                context.getSystemService(NotificationManager::class.java)?.notify(ID, notification(context))
                return
            }
            // Started when a session opens, which happens on screen: Android
            // refuses a foreground service started from the background.
            runCatching { context.startForegroundService(Intent(context, SessionService::class.java)) }
        }

        fun stop(context: Context) {
            if (running) context.stopService(Intent(context, SessionService::class.java))
            running = false
        }

        private fun notification(context: Context): Notification {
            val manager = context.getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(
                NotificationChannel(CHANNEL, tr("Sessions"), NotificationManager.IMPORTANCE_LOW),
            )
            val sessions = (context.applicationContext as ShellwingApp).model.sessions
            val live = sessions.filter { it.isLive || it.status == SessionStatus.LOST }
            val open = PendingIntent.getActivity(
                context, 0,
                Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE,
            )
            val closeAll = PendingIntent.getService(
                context, 1,
                Intent(context, SessionService::class.java).setAction(CLOSE_ALL),
                PendingIntent.FLAG_IMMUTABLE,
            )
            val lines = live.joinToString("\n") { "${it.spec} · ${statusWord(it.status)}" }
            return Notification.Builder(context, CHANNEL)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle(trN(live.size, "{n} session open", "{n} sessions open"))
                .setContentText(live.joinToString(", ") { it.spec })
                .setStyle(Notification.BigTextStyle().bigText(lines))
                .setContentIntent(open)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setCategory(Notification.CATEGORY_SERVICE)
                .setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
                .addAction(Notification.Action.Builder(null, tr("Close all"), closeAll).build())
                .build()
        }
    }
}

/** A session's status, as the desktop says it. */
fun statusWord(s: SessionStatus): String = when (s) {
    SessionStatus.CONNECTED -> tr("connected")
    SessionStatus.CONNECTING -> tr("connecting")
    SessionStatus.LOST -> tr("connection lost")
    SessionStatus.FAILED -> tr("could not connect")
    SessionStatus.CLOSED -> tr("closed")
}
