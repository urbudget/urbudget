package app.urbudget.nativeapp

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Keeps UrBudget running while ChatGPT sign-in waits in the browser. The browser comes back to the
 * app's own server on 127.0.0.1 (services `assistant/auth.rs`), and Android pauses an app that sits
 * behind the browser (Samsung's quickly): the page then spins on "Allow" until the user comes back by
 * hand. A short foreground service keeps the app answering, and its notification is another way back.
 * It ends with the sign-in, or when Android's few minutes for a short service are up.
 */
class SignInService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var watching: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            val notification = notification(this)
            if (Build.VERSION.SDK_INT >= 34) startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE)
            else startForeground(NOTIFICATION, notification)
        } catch (_: Exception) {
            // Not allowed to run in front now: sign-in still works, the user just comes back by hand.
            stopSelf()
            return START_NOT_STICKY
        }
        if (watching?.isActive != true) watching = scope.launch {
            // Wait for the attempt to begin (it starts just after this), then for it to end.
            var seen = false
            var checks = 0
            while (isActive) {
                val now = signingIn()
                if (now) seen = true else if (seen || checks >= 20) break
                checks++
                delay(500)
            }
            stopSelf()
        }
        return START_NOT_STICKY
    }

    /** Android's time for a short service is up (API 34). */
    override fun onTimeout(startId: Int) = stopSelf()

    /** The same, from API 35. */
    override fun onTimeout(startId: Int, fgsType: Int) = stopSelf()

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private fun signingIn(): Boolean =
        runCatching { (NativeServices.call("assistant_status") as JSONObject).optBoolean("signingIn") }.getOrDefault(false)

    companion object {
        private const val CHANNEL = "sign-in"
        private const val NOTIFICATION = 7301

        /** Called from the sign-in button, while the app is in front, as Android requires. */
        fun start(context: Context) {
            runCatching { ContextCompat.startForegroundService(context, Intent(context, SignInService::class.java)) }
        }

        private fun notification(context: Context): android.app.Notification {
            if (Build.VERSION.SDK_INT >= 26) context.getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, context.getString(R.string.sign_in_notify_channel), NotificationManager.IMPORTANCE_LOW)
                    .apply { description = context.getString(R.string.sign_in_notify_channel_detail) })
            // Tapping it brings the app back over the browser tab, as the page's own link does.
            val open = PendingIntent.getActivity(context, NOTIFICATION, Intent(context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            // Without permission to notify, Android still runs the service; the notification just isn't shown.
            val quiet = Build.VERSION.SDK_INT >= 33 &&
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            return NotificationCompat.Builder(context, CHANNEL).setSmallIcon(R.drawable.ic_notification_sign_in)
                .setContentTitle(context.getString(R.string.sign_in_notify_title))
                .setContentText(context.getString(R.string.sign_in_notify_text))
                .setContentIntent(open).setOngoing(true).setSilent(true).setOnlyAlertOnce(true)
                .setPriority(if (quiet) NotificationCompat.PRIORITY_MIN else NotificationCompat.PRIORITY_LOW)
                .build()
        }
    }
}
