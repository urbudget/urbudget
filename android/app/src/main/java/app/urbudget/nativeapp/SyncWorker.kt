package app.urbudget.nativeapp

import android.content.Context
import androidx.work.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.concurrent.TimeUnit

/** A persisted wake-up. The native coordinator owns durable pending work and retry state. */
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        if (!NativePreferences(applicationContext).automaticSync) { configure(applicationContext, false); return@withContext Result.success() }
        try {
            NativeServices.initialize(applicationContext)
            val status = NativeServices.call("cloud_status") as org.json.JSONObject
            if (!syncBackgroundEligible(status)) { configure(applicationContext, false); return@withContext Result.success() }
            repeat(2) {
                val generation = applicationContext.getSharedPreferences("sync-wakes", Context.MODE_PRIVATE).getLong("generation", 0)
                val report = NativeServices.syncAutomatically() ?: return@withContext Result.retry()
                if (report.optString("state") in setOf("offline", "error")) return@withContext Result.retry()
                if (generation == applicationContext.getSharedPreferences("sync-wakes", Context.MODE_PRIVATE).getLong("generation", 0)) return@withContext Result.success()
            }
            Result.retry()
        } catch (_: Exception) { Result.retry() }
    }

    companion object {
        private val network = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
        @Volatile private var configured: Boolean? = null
        @Synchronized fun configure(context: Context, eligible: Boolean) {
            val enabled = eligible && NativePreferences(context).automaticSync
            if (configured == enabled) return
            configured = enabled
            if (!enabled) {
                WorkManager.getInstance(context).cancelUniqueWork("account-sync-periodic")
                WorkManager.getInstance(context).cancelUniqueWork("account-sync")
                return
            }
            WorkManager.getInstance(context).enqueueUniquePeriodicWork("account-sync-periodic", ExistingPeriodicWorkPolicy.KEEP,
                PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES).setConstraints(network)
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build())
            enqueue(context)
        }
        @Synchronized fun enqueue(context: Context) {
            if (!NativePreferences(context).automaticSync) { configure(context, false); return }
            if (configured != true) return
            val wakes = context.getSharedPreferences("sync-wakes", Context.MODE_PRIVATE)
            wakes.edit().putLong("generation", wakes.getLong("generation", 0) + 1).commit()
            WorkManager.getInstance(context).enqueueUniqueWork("account-sync", ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<SyncWorker>().setConstraints(network).setInitialDelay(2, TimeUnit.SECONDS)
                    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build())
        }
    }
}
