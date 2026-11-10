package com.asmrplayer.android.update

import android.content.Context
import androidx.work.*
import com.asmrplayer.android.data.AndroidBackend
import java.util.concurrent.TimeUnit

internal object UpdateScheduler {
    private const val CHECK = "app-update-check"
    private const val STARTUP = "app-update-check-startup"
    private const val DOWNLOAD = "app-update-download"
    private val connected = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
    fun configure(context: Context) {
        val manager = WorkManager.getInstance(context)
        if (!context.getSharedPreferences("app-updates", Context.MODE_PRIVATE).getBoolean("auto", true)) {
            manager.cancelUniqueWork(CHECK); manager.cancelUniqueWork(STARTUP); return
        }
        manager.enqueueUniquePeriodicWork(CHECK, ExistingPeriodicWorkPolicy.KEEP,
            PeriodicWorkRequestBuilder<UpdateCheckWorker>(24, TimeUnit.HOURS).setInitialDelay(24, TimeUnit.HOURS).setConstraints(connected).build())
        manager.enqueueUniqueWork(STARTUP, ExistingWorkPolicy.KEEP,
            OneTimeWorkRequestBuilder<UpdateCheckWorker>().setConstraints(connected).build())
    }
    fun download(context: Context) {
        WorkManager.getInstance(context).enqueueUniqueWork(DOWNLOAD, ExistingWorkPolicy.KEEP,
            OneTimeWorkRequestBuilder<UpdateDownloadWorker>().setConstraints(connected).build())
    }
    fun cancelDownload(context: Context) { WorkManager.getInstance(context).cancelUniqueWork(DOWNLOAD) }
}

class UpdateCheckWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val result = AndroidBackend.get(applicationContext).updates.check(manual = false)
        return if (result.phase == UpdatePhase.ERROR && runAttemptCount < 3) Result.retry() else Result.success()
    }
}
class UpdateDownloadWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result =
        if (AndroidBackend.get(applicationContext).updates.download()) Result.success() else Result.failure()
}
