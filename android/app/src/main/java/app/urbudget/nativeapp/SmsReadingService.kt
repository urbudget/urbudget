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
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Keeps UrBudget running while it reads bank messages the user asked it to. A first reading takes
 * minutes, a request at a time, and Android pauses an app that's out of sight within seconds: the
 * request under way then waits until it times out, and its texts are read (and paid for) again.
 * This service runs in front for as long as [SmsReadings] has a reading under way, and ends with
 * it. Its notification is the way back to Bank messages.
 *
 * Only readings started from the page use it: Android lets an app start one while it's in front.
 * The background reader ([SmsWorker]) runs under WorkManager, which keeps it going itself.
 */
class SmsReadingService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var watching: Job? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        try {
            val notification = notification(this)
            if (Build.VERSION.SDK_INT >= 29) startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            else startForeground(NOTIFICATION, notification)
        } catch (_: Exception) {
            // Not allowed to run in front now: the reading carries on while the app is open, as before.
            stopSelf()
            return START_NOT_STICKY
        }
        if (watching?.isActive != true) watching = scope.launch {
            // The reading begins just after this is started; wait for it, then for it to end.
            withTimeoutOrNull(START_WAIT) { SmsReadings.state.first { it != null } }
            // One reading can follow another (the page reads what arrived meanwhile): a short wait tells them apart from the end.
            do {
                SmsReadings.state.first { it == null }
                delay(END_WAIT)
            } while (SmsReadings.state.value != null)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    /** Android's time for this kind of service is up (API 35): the reading goes on only while the app is open. */
    override fun onTimeout(startId: Int, fgsType: Int) = stopSelf()

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        private const val CHANNEL = "bank-reading"
        private const val NOTIFICATION = 7002
        private const val START_WAIT = 10_000L
        private const val END_WAIT = 1_500L

        /** Called as a reading starts from the page, while the app is in front, as Android requires. */
        fun start(context: Context) {
            runCatching { ContextCompat.startForegroundService(context, Intent(context, SmsReadingService::class.java)) }
        }

        private fun notification(context: Context): android.app.Notification {
            if (Build.VERSION.SDK_INT >= 26) context.getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, context.getString(R.string.sms_reading_service_channel), NotificationManager.IMPORTANCE_LOW))
            val open = PendingIntent.getActivity(context, NOTIFICATION, Intent(context, MainActivity::class.java).putExtra(SmsWorker.PAGE, "Bank messages")
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            // Without permission to notify, Android still runs the service; the notification just isn't shown.
            val quiet = Build.VERSION.SDK_INT >= 33 &&
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            return NotificationCompat.Builder(context, CHANNEL).setSmallIcon(R.drawable.ic_notification_messages)
                .setContentTitle(context.getString(R.string.sms_reading_service_title))
                .setContentText(context.getString(R.string.sms_reading_service_text))
                .setContentIntent(open).setOngoing(true).setSilent(true).setOnlyAlertOnce(true).setProgress(0, 0, true)
                .setPriority(if (quiet) NotificationCompat.PRIORITY_MIN else NotificationCompat.PRIORITY_LOW)
                .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE).build()
        }
    }
}
