package com.asmrplayer.android.update

import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.AtomicFile
import androidx.core.content.FileProvider
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionResult
import com.asmrplayer.android.BuildConfig
import com.asmrplayer.android.playback.LibraryPlayback
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File
import java.util.concurrent.Executor
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

enum class UpdatePhase { IDLE, CHECKING, UP_TO_DATE, AVAILABLE, QUEUED, DOWNLOADING, VERIFYING, READY, ERROR }
data class UpdateSnapshot(
    val phase: UpdatePhase = UpdatePhase.IDLE, val release: UpdateRelease? = null,
    val downloadedBytes: Long = 0, val checkedAt: Long? = null,
    val autoCheck: Boolean = true, val skippedVersion: String? = null, val error: String? = null,
)
sealed interface UpdateInstallAction {
    data class PermissionRequired(val intent: Intent) : UpdateInstallAction
    data class Installer(val intent: Intent) : UpdateInstallAction
}

/** Process owner. WorkManager owns download lifetime; only the UI may launch a returned install Intent. */
class UpdateRepository internal constructor(
    private val context: Context,
    private val remote: UpdateRemote = HttpUpdateRemote(),
    private val publicKey: String = BuildConfig.UPDATE_PUBLIC_KEY,
    private val verifyPackage: (File, UpdateRelease) -> Unit = ApkIdentity(context)::verify,
) {
    private val prefs = context.getSharedPreferences("app-updates", Context.MODE_PRIVATE)
    private val mutable = MutableStateFlow(UpdateSnapshot(autoCheck = prefs.getBoolean("auto", true), skippedVersion = prefs.getString("skip", null)))
    val state: StateFlow<UpdateSnapshot> = mutable.asStateFlow()
    private val lock = Mutex()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val identity = ApkIdentity(context)
    private val directory = File(context.filesDir, "updates")
    private val apk = File(directory, "ready/update.apk")
    private val part = File(directory, "download.part")
    private val saved = AtomicFile(File(directory, "ready.json"))
    private var manifest: String? = null
    @Volatile private var downloadJob: Job? = null
    private val restored = scope.launch {
        lock.withLock {
            part.delete()
            if (saved.baseFile.exists()) try {
                val raw = saved.openRead().bufferedReader().use { it.readText() }
                val release = requireNotNull(UpdateManifest.parse(raw))
                verify(release)
                manifest = raw
                mutable.update { it.copy(phase = UpdatePhase.READY, release = release, downloadedBytes = release.size) }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { saved.delete(); apk.delete() }
        }
    }

    fun setAutoCheck(enabled: Boolean) {
        prefs.edit().putBoolean("auto", enabled).apply(); mutable.update { it.copy(autoCheck = enabled) }
        UpdateScheduler.configure(context)
    }
    fun skipVersion(version: String?) {
        require(version == null || version == state.value.release?.version)
        prefs.edit().putString("skip", version).apply(); mutable.update { it.copy(skippedVersion = version) }
    }

    suspend fun check(manual: Boolean = true): UpdateSnapshot {
        restored.join()
        if (!lock.tryLock()) return state.value
        try {
            if (!manual && !state.value.autoCheck || state.value.phase == UpdatePhase.READY ||
                state.value.phase == UpdatePhase.QUEUED && state.value.release != null) return state.value
            mutable.update { it.copy(phase = UpdatePhase.CHECKING, error = null) }
            val raw = remote.manifest()
            val release = UpdateManifest.parse(raw)
            val installed = identity.installed()
            val candidate = release?.takeIf { UpdateManifest.eligible(it, requireNotNull(installed.versionName), identity.code(installed), Build.VERSION.SDK_INT) }
                ?.takeUnless { !manual && it.version == state.value.skippedVersion }
            manifest = raw
            mutable.update { it.copy(phase = if (candidate == null) UpdatePhase.UP_TO_DATE else UpdatePhase.AVAILABLE,
                release = candidate, downloadedBytes = 0, checkedAt = System.currentTimeMillis()) }
        } catch (e: CancellationException) {
            mutable.update { it.copy(phase = if (it.release == null) UpdatePhase.IDLE else UpdatePhase.AVAILABLE) }; throw e
        } catch (e: Exception) { fail(e) }
        finally { lock.unlock() }
        return state.value
    }

    /** Enqueue after an explicit user action; survives Activity recreation. */
    fun startDownload() {
        if (state.value.phase in setOf(UpdatePhase.QUEUED, UpdatePhase.DOWNLOADING, UpdatePhase.VERIFYING, UpdatePhase.READY)) return
        mutable.update { it.copy(phase = UpdatePhase.QUEUED, error = null) }
        try { UpdateScheduler.download(context) } catch (e: Exception) { fail(e); throw e }
    }
    fun cancelDownload() {
        UpdateScheduler.cancelDownload(context)
        downloadJob?.cancel()
        mutable.update { if (it.phase == UpdatePhase.QUEUED) it.copy(phase = UpdatePhase.AVAILABLE) else it }
    }

    internal suspend fun download(): Boolean {
        restored.join()
        // A worker restarted after process death obtains a fresh manifest, never trusts a .part file.
        if (state.value.release == null) check(manual = true)
        if (!lock.tryLock()) return false
        try {
            if (state.value.phase == UpdatePhase.READY) return true
            val release = requireNotNull(state.value.release) { "请先检查更新" }
            val raw = requireNotNull(manifest)
            downloadJob = currentCoroutineContext()[Job]
            mutable.update { it.copy(phase = UpdatePhase.DOWNLOADING, downloadedBytes = 0, error = null) }
            directory.mkdirs(); part.delete()
            remote.download(release, part) { total -> mutable.update { it.copy(downloadedBytes = total) } }
            mutable.update { it.copy(phase = UpdatePhase.VERIFYING) }
            withContext(Dispatchers.IO) {
                UpdaterSignature.verify(part, release.signature, publicKey, release.version, release.sha256, release.size)
                verifyPackage(part, release)
                currentCoroutineContext().ensureActive()
                apk.parentFile!!.mkdirs(); apk.delete()
                kotlin.check(part.renameTo(apk)) { "无法保存 APK" }
                val stream = saved.startWrite()
                try { stream.write(raw.toByteArray(Charsets.UTF_8)); saved.finishWrite(stream) }
                catch (e: Exception) { saved.failWrite(stream); throw e }
            }
            mutable.update { it.copy(phase = UpdatePhase.READY) }
            return true
        } catch (e: CancellationException) {
            mutable.update { it.copy(phase = UpdatePhase.AVAILABLE, downloadedBytes = 0, error = null) }; throw e
        } catch (e: Exception) { fail(e); return false }
        finally { part.delete(); downloadJob = null; lock.unlock() }
    }

    /** Call from the install button. Permission requests do not pause playback; retry after permission is granted. */
    @UnstableApi
    suspend fun prepareInstall(controller: MediaController): UpdateInstallAction {
        restored.join()
        return lock.withLock {
            try {
                kotlin.check(state.value.phase == UpdatePhase.READY) { "更新尚未就绪" }
                val release = requireNotNull(state.value.release)
                withContext(Dispatchers.IO) { verify(release) }
                if (Build.VERSION.SDK_INT >= 26 && !context.packageManager.canRequestPackageInstalls()) {
                    return@withLock UpdateInstallAction.PermissionRequired(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}")))
                }
                withContext(Dispatchers.Main.immediate) {
                    val future = LibraryPlayback.prepareUpdate(controller)
                    val result = withTimeout(30_000) {
                        suspendCancellableCoroutine { continuation ->
                            future.addListener({
                                try { continuation.resume(future.get()) } catch (e: Exception) { continuation.resumeWithException(e) }
                            }, Executor { it.run() })
                            continuation.invokeOnCancellation { future.cancel(false) }
                        }
                    }
                    kotlin.check(result.resultCode == SessionResult.RESULT_SUCCESS) { "保存播放状态失败，请重试" }
                }
                val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", apk)
                UpdateInstallAction.Installer(Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "application/vnd.android.package-archive")
                    clipData = ClipData.newRawUri("ASMR Player update", uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                })
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { fail(e); throw e }
        }
    }

    private fun verify(release: UpdateRelease) {
        UpdaterSignature.verify(apk, release.signature, publicKey, release.version, release.sha256, release.size)
        verifyPackage(apk, release)
    }
    private fun fail(error: Exception) { mutable.update { it.copy(phase = UpdatePhase.ERROR, error = error.message ?: "更新失败") } }
}
