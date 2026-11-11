package app.urbudget.nativeapp

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.*
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import org.json.JSONObject
import java.util.concurrent.TimeUnit

/**
 * Reads new bank texts soon after they arrive, for the open budget when it asked to: in the
 * background, with the permission given on the Bank messages page. Texts that are only codes and
 * alerts leave without a word; anything about money waits in the review, counted in a notification
 * that opens the page.
 */
class SmsWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val context = applicationContext
        val inbox = SmsInbox.get(context)
        val asked = withContext(smsStorageLane) { inbox.reading() }
        if (asked.isEmpty() || !SmsReader.canRead(context)) return Result.success()
        return try {
            withContext(Dispatchers.IO) { NativeServices.initialize(context) }
            // Only the open budget can be read: it's the one the assistant is given.
            val identity = (withContext(Dispatchers.IO) { NativeServices.call("native_budget_status") } as JSONObject).optString("databasePath")
            if (identity !in asked) return Result.success()
            val connection = withContext(Dispatchers.IO) { NativeServices.call("assistant_status") } as JSONObject
            if (!connection.optBoolean("ready") || !smsConsented(withContext(smsStorageLane) { inbox.state(identity, "consent") }, connection)) return Result.success()
            val (base, waiting) = withContext(smsStorageLane) {
                SmsReader.refresh(context, identity)
                val base = SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) }
                val inReview = base?.messages?.map { it.id }?.toSet().orEmpty()
                base to inbox.pending(identity).filter { it.id !in inReview }
            }
            if (waiting.isEmpty()) return Result.success()
            // Busy reading on the page: that reading takes these too, or the next arrival does.
            val read = SmsReadings.read(context, NativeSmsAnalyzer, identity, connection, waiting, "", base) { SmsReader.canRead(context) }
                ?: return Result.success()
            val fresh = read.analysis.rows("items").drop(base?.analysis?.rows("items")?.size ?: 0)
            val money = fresh.count { it.text("disposition") != "ignore" }
            if (money == 0 && base == null) {
                // Only codes and alerts: done, as the page would have done it.
                val skipped = JSONObject(fresh.groupingBy { it.text("skip").ifBlank { "other" } }.eachCount())
                withContext(smsStorageLane) {
                    inbox.finish(identity, read.messages.map { it.id })
                    inbox.saveState(identity, "last", JSONObject().put("at", System.currentTimeMillis()).put("skipped", skipped).toString())
                }
            } else if (money > 0) notify(context, money)
            Result.success()
        } catch (cancel: kotlinx.coroutines.CancellationException) { throw cancel
        } catch (_: Exception) {
            // A connection that's down tries again later, once: a reading already tries a request that's
            // too much again in smaller ones, so what still fails waits for the page rather than being
            // sent, and paid for, over and over.
            if (runAttemptCount < 2) Result.retry() else Result.success()
        }
    }

    private fun notify(context: Context, count: Int) {
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return
        val manager = context.getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel(CHANNEL, context.getString(R.string.sms_notify_channel), NotificationManager.IMPORTANCE_DEFAULT)
            .apply { description = context.getString(R.string.sms_notify_channel_detail) })
        val open = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java).putExtra(PAGE, "Bank messages")
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification = NotificationCompat.Builder(context, CHANNEL).setSmallIcon(R.drawable.ic_notification_messages)
            .setContentTitle(context.resources.getQuantityString(R.plurals.sms_notify_title, count, count.toString()))
            .setContentText(context.getString(R.string.sms_notify_text)).setContentIntent(open).setAutoCancel(true).build()
        runCatching { NotificationManagerCompat.from(context).notify(NOTIFICATION, notification) }
    }

    companion object {
        private const val CHANNEL = "bank-messages"
        private const val NOTIFICATION = 7001
        /** The page an intent opens: a notification's way into the app. */
        const val PAGE = "app.urbudget.page"
        private val network = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

        /** After texts arrive; a short wait gathers a burst into one reading. */
        fun enqueue(context: Context) {
            WorkManager.getInstance(context).enqueueUniqueWork("sms-reading", ExistingWorkPolicy.APPEND_OR_REPLACE,
                OneTimeWorkRequestBuilder<SmsWorker>().setConstraints(network).setInitialDelay(20, TimeUnit.SECONDS)
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 1, TimeUnit.MINUTES).build())
        }
    }
}

/** A page the app opens when it comes forward, asked for by a notification. */
internal object PageRequests {
    val page = MutableStateFlow<String?>(null)
}
