package com.asmrplayer.android.download

import android.app.*
import android.app.job.*
import android.content.*
import android.content.pm.ServiceInfo
import android.net.NetworkRequest
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.work.*
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.data.DownloadTask
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import java.util.concurrent.TimeUnit

object DownloadScheduler {
    internal const val JOB_ID = 6006
    internal const val WORK_NAME = "asmr-offline-downloads"
    suspend fun schedule(context: Context) {
        if (Build.VERSION.SDK_INT >= 34) {
            val request = NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET).build()
            val job = JobInfo.Builder(JOB_ID, ComponentName(context, DownloadJobService::class.java))
                .setUserInitiated(true).setRequiredNetwork(request).build()
            check((context.getSystemService(Context.JOB_SCHEDULER_SERVICE) as JobScheduler).schedule(job) == JobScheduler.RESULT_SUCCESS)
        } else {
            val work = OneTimeWorkRequestBuilder<DownloadWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build()
            withContext(Dispatchers.IO) { WorkManager.getInstance(context).enqueueUniqueWork(WORK_NAME, ExistingWorkPolicy.APPEND_OR_REPLACE, work).result.get() }
        }
    }
    internal fun notification(context: Context, task: DownloadTask? = null): Notification {
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel("downloads", "离线下载", NotificationManager.IMPORTANCE_LOW))
        val pause = PendingIntent.getBroadcast(context, 6006, Intent(context, PauseDownloadsReceiver::class.java), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val text = if (task == null) "正在准备离线下载" else "已下载 ${task.downloadedBytes / 1024 / 1024} MB"
        val progress = task?.totalBytes?.takeIf { it > 0 }?.let { ((task.downloadedBytes.toDouble() / it) * 100).toInt().coerceIn(0, 100) }
        return NotificationCompat.Builder(context, "downloads").setSmallIcon(com.asmrplayer.android.R.drawable.lucide_download)
            .setContentTitle("ASMR Player 下载").setContentText(text).setOnlyAlertOnce(true).setOngoing(true)
            .setProgress(100, progress ?: 0, progress == null).addAction(com.asmrplayer.android.R.drawable.lucide_pause_filled, "暂停全部", pause)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE).build()
    }
}

/** API 23–33 fallback, independent of MediaSessionService. */
class DownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val repo = AndroidBackend.get(applicationContext).downloads
        return try {
            setForeground(info(null))
            val retry = repo.runQueue { task ->
                // Notification contents never include work names or signed URLs.
                if (Build.VERSION.SDK_INT < 33 || androidx.core.content.ContextCompat.checkSelfPermission(applicationContext,
                    android.Manifest.permission.POST_NOTIFICATIONS) == android.content.pm.PackageManager.PERMISSION_GRANTED)
                    (applicationContext.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
                        .notify(DownloadScheduler.JOB_ID, DownloadScheduler.notification(applicationContext, task))
            }
            if (retry) Result.retry() else Result.success()
        } catch (e: CancellationException) { throw e }
        catch (_: Exception) { try { repo.schedulingFailed() } catch (_: Exception) { }; Result.failure() }
    }
    override suspend fun getForegroundInfo(): ForegroundInfo = info(null)
    private fun info(task: DownloadTask?) = if (Build.VERSION.SDK_INT >= 29)
        ForegroundInfo(DownloadScheduler.JOB_ID, DownloadScheduler.notification(applicationContext, task), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        else ForegroundInfo(DownloadScheduler.JOB_ID, DownloadScheduler.notification(applicationContext, task))
}

/** API 34+: must be scheduled from a visible user action; system stop is cooperative/cancellable. */
class DownloadJobService : JobService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val jobs = mutableMapOf<Int, Job>()
    override fun onStartJob(params: JobParameters): Boolean {
        if (Build.VERSION.SDK_INT < 34) return false
        setNotification(params, DownloadScheduler.JOB_ID, DownloadScheduler.notification(this), JOB_END_NOTIFICATION_POLICY_REMOVE)
        jobs[params.jobId] = scope.launch {
            try {
                val retry = withContext(Dispatchers.IO) { AndroidBackend.get(this@DownloadJobService).downloads.runQueue { task ->
                    Handler(Looper.getMainLooper()).post {
                        if (jobs[params.jobId]?.isActive == true) setNotification(params, DownloadScheduler.JOB_ID,
                            DownloadScheduler.notification(this@DownloadJobService, task), JOB_END_NOTIFICATION_POLICY_REMOVE)
                    }
                } }
                jobFinished(params, retry)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) {
                try { AndroidBackend.get(this@DownloadJobService).downloads.schedulingFailed() } catch (_: Exception) { }
                jobFinished(params, false)
            }
            finally { if (jobs[params.jobId] === currentCoroutineContext().job) jobs.remove(params.jobId) }
        }
        return true
    }
    override fun onStopJob(params: JobParameters): Boolean { jobs.remove(params.jobId)?.cancel(); return true }
    override fun onDestroy() { scope.cancel(); super.onDestroy() }
}

class PauseDownloadsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val result = goAsync()
        CoroutineScope(SupervisorJob() + Dispatchers.IO).launch {
            try {
                val repo = AndroidBackend.get(context).downloads
                repo.tasks.first().filter { it.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING") }.forEach { repo.pause(it.id) }
                (context.getSystemService(Context.JOB_SCHEDULER_SERVICE) as JobScheduler).cancel(DownloadScheduler.JOB_ID)
                WorkManager.getInstance(context).cancelUniqueWork(DownloadScheduler.WORK_NAME)
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { /* Room remains authoritative; the app can show/retry a failed pause. */ }
            finally { result.finish() }
        }
    }
}
