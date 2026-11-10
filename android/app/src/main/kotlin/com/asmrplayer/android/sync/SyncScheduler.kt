package com.asmrplayer.android.sync

import android.content.Context
import androidx.work.*
import com.asmrplayer.android.data.AndroidBackend
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

data class AutoSyncPolicy(
    val foreground: Boolean = false,
    val periodic: Boolean = false,
    val intervalMinutes: Int = 30,
)

object SyncScheduler {
    private const val PERIODIC = "device-sync-periodic"
    private const val FOREGROUND = "device-sync-foreground"

    private fun saved(c: Context): Storage? {
        val value =
            c.getSharedPreferences("sync-selection", Context.MODE_PRIVATE)
                .getString("provider", null)
        return Storage.entries.find { it.name == value }
    }

    fun selected(c: Context): Storage = saved(c) ?: Storage.GITHUB

    fun confirm(c: Context, id: Storage) {
        if (saved(c) == null)
            check(
                c.getSharedPreferences("sync-selection", Context.MODE_PRIVATE)
                    .edit()
                    .putString("provider", id.name)
                    .commit()
            )
    }

    fun select(c: Context, id: Storage) {
        check(
            c.getSharedPreferences("sync-selection", Context.MODE_PRIVATE)
                .edit()
                .putString("provider", id.name)
                .commit()
        )
        WorkManager.getInstance(c).cancelUniqueWork(FOREGROUND)
        configure(c)
    }

    private fun prefs(c: Context, id: Storage) =
        c.getSharedPreferences("device-sync-auto-${id.name}", Context.MODE_PRIVATE)

    fun policy(c: Context, id: Storage = selected(c)): AutoSyncPolicy {
        val p = prefs(c, id)
        return AutoSyncPolicy(
            p.getBoolean("foreground", false),
            p.getBoolean("periodic", false),
            p.getInt("interval", 30),
        )
    }

    fun error(c: Context, id: Storage) = prefs(c, id).getString("error", "") ?: ""

    fun save(c: Context, p: AutoSyncPolicy, id: Storage) {
        require(p.intervalMinutes in listOf(15, 30, 60, 180, 360))
        check(
            prefs(c, id)
                .edit()
                .putBoolean("foreground", p.foreground)
                .putBoolean("periodic", p.periodic)
                .putInt("interval", p.intervalMinutes)
                .commit()
        )
        configure(c)
        if (!p.foreground) WorkManager.getInstance(c).cancelUniqueWork(FOREGROUND)
    }

    fun onLogin(c: Context, id: Storage) {
        if (!prefs(c, id).contains("periodic")) save(c, policy(c, id).copy(periodic = true), id)
    }

    fun configure(c: Context) {
        val id = selected(c)
        val p = policy(c, id)
        val wm = WorkManager.getInstance(c)
        if (saved(c) != null && p.periodic)
            wm.enqueueUniquePeriodicWork(
                PERIODIC,
                ExistingPeriodicWorkPolicy.UPDATE,
                PeriodicWorkRequestBuilder<AutoSyncWorker>(
                        p.intervalMinutes.toLong(),
                        TimeUnit.MINUTES,
                    )
                    .setInitialDelay(p.intervalMinutes.toLong(), TimeUnit.MINUTES)
                    .setConstraints(
                        Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
                    )
                    .setInputData(workDataOf("periodic" to true, "provider" to id.name))
                    .build(),
            )
        else wm.cancelUniqueWork(PERIODIC)
    }

    fun foreground(c: Context) {
        val id = selected(c)
        if (
            saved(c) == null ||
                !policy(c, id).foreground ||
                System.currentTimeMillis() - prefs(c, id).getLong("attempt", 0) < 60_000
        )
            return
        WorkManager.getInstance(c)
            .enqueueUniqueWork(
                FOREGROUND,
                ExistingWorkPolicy.KEEP,
                OneTimeWorkRequestBuilder<AutoSyncWorker>()
                    .setInputData(workDataOf("provider" to id.name))
                    .setConstraints(
                        Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
                    )
                    .build(),
            )
    }

    internal fun attempted(c: Context, id: Storage) {
        prefs(c, id).edit().putLong("attempt", System.currentTimeMillis()).apply()
    }

    internal fun result(c: Context, success: Boolean, id: Storage) {
        prefs(c, id)
            .edit()
            .putString("error", if (success) "" else "自动同步未完成，将在下次触发时重试；可手动同步查看原因")
            .apply()
    }
}

class AutoSyncWorker(context: Context, params: WorkerParameters) :
    CoroutineWorker(context, params) {
    override suspend fun doWork(): Result =
        withContext(Dispatchers.IO) {
            val id =
                Storage.entries.find { it.name == inputData.getString("provider") }
                    ?: return@withContext Result.success()
            if (id != SyncScheduler.selected(applicationContext))
                return@withContext Result.success()
            val p = SyncScheduler.policy(applicationContext, id)
            if (!(if (inputData.getBoolean("periodic", false)) p.periodic else p.foreground))
                return@withContext Result.success()
            SyncScheduler.attempted(applicationContext, id)
            try {
                val repo = AndroidBackend.get(applicationContext).sync
                if (!repo.connected()) return@withContext Result.success()
                repo.sync(id)
                Result.success()
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (_: Exception) {
                SyncScheduler.result(applicationContext, false, id)
                if (runAttemptCount < 2) Result.retry() else Result.failure()
            }
        }
}
