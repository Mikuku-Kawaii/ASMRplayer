package com.asmrplayer.android.download

import android.content.Context
import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.*
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

data class DownloadStorageItem(val taskId: String, val kind: String, val state: String,
    val fileBytes: Long, val partialBytes: Long)
data class DownloadStorageUsage(val items: List<DownloadStorageItem>, val freeBytes: Long,
    val artworkBytes: Long = 0, val playbackBytes: Long = 0, val loudnessBytes: Long = 0) {
    val totalBytes: Long get() = items.sumOf { it.fileBytes + it.partialBytes }
    val partialBytes: Long get() = items.sumOf { it.partialBytes }
    fun bytesFor(kind: String): Long = items.filter { it.kind == kind }.sumOf { it.fileBytes + it.partialBytes }
    val offlineCoverBytes get() = items.filter { it.kind == "cover" && it.state == "COMPLETED" }.sumOf { it.fileBytes }
    val downloadBytes get() = totalBytes - offlineCoverBytes
    val coverCacheBytes get() = offlineCoverBytes + artworkBytes
    val cacheBytes get() = coverCacheBytes + playbackBytes + loudnessBytes
}
data class CoverCacheClearResult(val removedIds: List<String>, val failedIds: List<String>)

class DownloadRepository(private val context: Context, private val db: LibraryDatabase,
    private val provider: AsmrOneProvider, private val schedule: suspend () -> Unit = { DownloadScheduler.schedule(context) }) {
    private val dao = db.library()
    private val directory = File(context.noBackupFilesDir, "offline")
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val running = ConcurrentHashMap<String, Job>()
    private val runLock = Mutex()
    private val mutation = Mutex()
    private val recovery = scope.async { reconcile() }
    val tasks = dao.observeDownloads()
    suspend fun awaitReady() { recovery.await() }
    /** Actual private download bytes, never the size of user-owned SAF media or database records. */
    suspend fun storageUsage(): DownloadStorageUsage {
        awaitReady()
        return mutation.withLock { withContext(Dispatchers.IO) {
            val items = dao.downloadTasks().filter { it.state != "REMOVED" }.map { task ->
                DownloadStorageItem(task.id, task.assetId?.let { dao.remoteAsset(it)?.kind ?: "attachment" } ?: "audio",
                    task.state, file(task.id, false).length(), file(task.id, true).length())
            }
            val cache = context.cacheDir
            DownloadStorageUsage(items, context.noBackupFilesDir.usableSpace,
                com.asmrplayer.android.artwork.CoverDiskCache.storedBytes(context),
                File(cache, "audio-spans-v1").walkTopDown().filter { it.isFile && it.extension == "exo" }.sumOf { it.length() },
                File(cache, "loudness").listFiles().orEmpty().filter { it.isFile && it.extension == "json" }.sumOf { it.length() })
        } }
    }
    /** Explicit user action. Completed covers only; audio, subtitles and image attachments stay intact. */
    suspend fun clearCompletedCoverCache(): CoverCacheClearResult {
        awaitReady()
        val ids = mutation.withLock { dao.downloadTasks().filter { task ->
            task.state == "COMPLETED" && task.assetId?.let { dao.remoteAsset(it)?.kind } == "cover"
        }.map { it.id } }
        val removed = mutableListOf<String>(); val failed = mutableListOf<String>()
        ids.forEach { id ->
            try { remove(id); removed.add(id) }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { failed.add(id) }
        }
        return CoverCacheClearResult(removed, failed)
    }
    internal suspend fun schedulingFailed() {
        awaitReady()
        mutation.withLock { dao.downloadTasks().filter { it.state == "QUEUED" }.forEach { dao.downloadState(it.id, "PAUSED", "SCHEDULING") } }
    }

    /** Call from a visible user action on Android 14+ (UIDT scheduling requirement). */
    suspend fun enqueue(versionIds: List<String>): List<String> = enqueueTargets(versionIds, false)

    suspend fun enqueueAssets(assetIds: List<String>): List<String> = enqueueTargets(assetIds, true)

    /** Cache the imported work's primary cover using the same durable queue as other attachments. */
    suspend fun cacheCover(workId: String): String {
        val asset = dao.remoteAsset(LibraryIndex.id(workId, "cover"))
            ?: throw OnlineException(OnlineError.NOT_FOUND, "作品没有可缓存封面，请先刷新作品")
        return enqueueAssets(listOf(asset.id)).single()
    }

    private suspend fun enqueueTargets(targetIds: List<String>, attachments: Boolean): List<String> {
        require(targetIds.isNotEmpty() && targetIds.size <= 500)
        return enqueueSelection(if (attachments) emptyList() else targetIds, if (attachments) targetIds else emptyList())
    }

    /** A confirmed file selection is validated and queued in one transaction, then scheduled once. */
    suspend fun enqueueSelection(versionIds: List<String>, assetIds: List<String>, skipExisting: Boolean = false): List<String> {
        awaitReady()
        val targets = versionIds.distinct().map { it to false } + assetIds.distinct().map { it to true }
        require(targets.isNotEmpty() && targets.size <= 50_001)
        val ids = mutation.withLock { db.withTransaction {
            targets.forEach { (targetId, attachments) ->
                if (attachments) require(dao.remoteAsset(targetId)?.available == true) { "附件没有可用在线来源" }
                else requireNotNull(dao.remote(targetId)) { "版本没有在线来源" }
            }
            targets.mapNotNull { (targetId, attachments) ->
                val id = LibraryIndex.id(if (attachments) "download-asset" else "download", targetId)
                val stored = dao.download(id)
                val old = if (skipExisting && stored != null) resetFailedFile(stored) else stored
                if (skipExisting && old != null && old.state !in setOf("REMOVED", "FAILED")) return@mapNotNull null
                if (old == null || old.state !in setOf("COMPLETED", "DOWNLOADING", "VERIFYING"))
                    dao.putDownload(old?.copy(state = "QUEUED", errorCode = null, attempts = 0, updatedMs = System.currentTimeMillis())
                        ?: DownloadTask(id, if (attachments) null else targetId, assetId = if (attachments) targetId else null))
                id
            }
        } }
        if (ids.none { id -> dao.download(id)?.state == "QUEUED" || (dao.download(id)?.state == "VERIFYING" && running[id] == null) }) return ids
        try { schedule() } catch (e: CancellationException) { throw e } catch (e: Exception) {
            mutation.withLock { ids.forEach { id -> if (dao.download(id)?.state == "QUEUED") dao.downloadState(id, "PAUSED", "SCHEDULING") } }
            throw OnlineException(OnlineError.NETWORK, "后台任务未能启动，请在应用前台重试")
        }
        return ids
    }
    suspend fun pause(id: String) {
        awaitReady()
        mutation.withLock { if (dao.download(id)?.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING")) dao.downloadState(id, "PAUSED") }
        running[id]?.cancel()
    }
    suspend fun resume(id: String) {
        awaitReady()
        val task = mutation.withLock {
            resetFailedFile(requireNotNull(dao.download(id)))
        }
        if (task.assetId != null) enqueueAssets(listOf(task.assetId)) else enqueue(listOf(requireNotNull(task.versionId)))
    }
    private suspend fun resetFailedFile(task: DownloadTask): DownloadTask {
        if (task.state != "FAILED" || task.errorCode !in setOf("SOURCE_CHANGED", "INTEGRITY", "INVALID_CONTENT", "MISSING_FILE")) return task
        return withContext(Dispatchers.IO) {
            val part = file(task.id, true); check(!part.exists() || part.delete())
            if (task.errorCode != "SOURCE_CHANGED") { val final = file(task.id, false); check(!final.exists() || final.delete()) }
            task.copy(downloadedBytes = 0, etag = null, lastModified = null, sha256 = null).also { dao.putDownload(it) }
        }
    }
    suspend fun remove(id: String) {
        awaitReady(); pause(id); running[id]?.cancelAndJoin()
        mutation.withLock { withContext(Dispatchers.IO) {
            val task = requireNotNull(dao.download(id))
            listOf(file(id, false), file(id, true)).forEach { f -> check(!f.exists() || f.delete()) { "无法删除下载文件" } }
            db.withTransaction {
                unpublish(task)
                dao.putDownload(task.copy(state = "REMOVED", downloadedBytes = 0, sha256 = null, updatedMs = System.currentTimeMillis()))
            }
        } }
    }

    /** Used by both UIDT JobService and legacy WorkManager; never requires a screen or playback service. */
    suspend fun runQueue(onProgress: (DownloadTask) -> Unit = {}): Boolean = runLock.withLock {
        awaitReady()
        reconcile()
        var retry = false
        val pending = dao.downloadTasks().filter { it.state == "QUEUED" }
        for (task in pending) {
            currentCoroutineContext().ensureActive()
            try { transfer(task.id, onProgress) }
            catch (e: CancellationException) { currentCoroutineContext().ensureActive() }
            catch (_: Exception) {
                currentCoroutineContext().ensureActive()
                retry = retry || dao.download(task.id)?.state == "QUEUED"
            }
        }
        retry || dao.downloadTasks().any { it.state == "QUEUED" }
    }

    private suspend fun transfer(id: String, progress: (DownloadTask) -> Unit) = coroutineScope {
        val job = currentCoroutineContext().job
        running[id] = job
        try {
            var task = mutation.withLock {
                val value = requireNotNull(dao.download(id))
                if (value.state != "QUEUED") return@coroutineScope
                value.copy(state = "DOWNLOADING", errorCode = null, attempts = value.attempts + 1).also { dao.putDownload(it) }
            }
            val asset = task.assetId?.let { requireNotNull(dao.remoteAsset(it)) }
            val ref = task.versionId?.let { requireNotNull(dao.remote(it)) }
            require((asset != null) != (ref != null))
            val part = file(id, true)
            withContext(Dispatchers.IO) { check(directory.isDirectory || directory.mkdirs()) }
            var finished = false
            var refreshed = false
            while (!finished) {
                currentCoroutineContext().ensureActive()
                val address = if (asset != null) provider.resolve(asset) else provider.resolve(requireNotNull(ref), download = true)
                var offset = withContext(Dispatchers.IO) { part.length() }
                if (offset > 0 && task.etag == null && task.lastModified == null) {
                    withContext(Dispatchers.IO) { FileOutputStream(part).use { } }; offset = 0
                }
                val headers = buildMap {
                    if (offset > 0) {
                        put("Range", "bytes=$offset-")
                        (task.etag ?: task.lastModified)?.let { put("If-Range", it) }
                    }
                }
                provider.http.response(address.url, headers) { response ->
                    val code = response.responseCode
                    if (code in setOf(401, 403) && !refreshed) { refreshed = true; return@response }
                    if (code !in setOf(200, 206, 416)) throw HttpStatusException(code)
                    val etag = response.getHeaderField("ETag")?.takeUnless { it.startsWith("W/") }
                    val modified = response.getHeaderField("Last-Modified")
                    val plan = ResumePolicy.evaluate(code, offset, response.getHeaderField("Content-Range"),
                        response.getHeaderField("Content-Length")?.toLongOrNull()?.takeIf { it >= 0 })
                    if (plan.alreadyComplete && !((task.etag != null && task.etag == etag) || (task.lastModified != null && task.lastModified == modified))) {
                        FileOutputStream(part).use { }; task = task.copy(etag = null, lastModified = null); return@response
                    }
                    require(response.getHeaderField("Content-Encoding").let { it == null || it == "identity" })
                    require(address.bytes == null || plan.total == null || address.bytes == plan.total) { "来源大小已变化" }
                    if (plan.append && !plan.alreadyComplete) {
                        require(task.etag == null || etag == null || task.etag == etag) { "来源校验标识已变化" }
                        require(task.etag != null || task.lastModified == null || modified == null || modified == task.lastModified)
                    }
                    val start = if (plan.append) offset else 0L
                    val total = plan.total ?: address.bytes
                    if (asset != null && total != null && total > AttachmentContent.limit(asset)) throw InvalidAttachment()
                    if (total != null && directory.usableSpace < total - start + 65536) throw OnlineException(OnlineError.STORAGE, "存储空间不足")
                    if (!plan.append) FileOutputStream(part).use { } // Truncate BEFORE saving replacement validators.
                    task = task.copy(downloadedBytes = start, totalBytes = total, etag = etag ?: if (plan.append) task.etag else null,
                        lastModified = modified ?: if (plan.append) task.lastModified else null)
                    saveActive(task); progress(task)
                    if (!plan.alreadyComplete) {
                        var bytes = 0L; var lastReport = System.nanoTime()
                        response.inputStream.use { input -> FileOutputStream(part, plan.append).use { output ->
                            val buffer = ByteArray(64 * 1024)
                            while (true) {
                                currentCoroutineContext().ensureActive()
                                val count = input.read(buffer); if (count < 0) break
                                if (asset != null && start + bytes + count > AttachmentContent.limit(asset)) throw InvalidAttachment()
                                output.write(buffer, 0, count); bytes += count
                                require(plan.expectedBody == null || bytes <= plan.expectedBody)
                                require(total == null || start + bytes <= total)
                                if (System.nanoTime() - lastReport >= 500_000_000L) {
                                    task = task.copy(downloadedBytes = start + bytes, updatedMs = System.currentTimeMillis())
                                    saveActive(task); progress(task); lastReport = System.nanoTime()
                                }
                            }
                            output.fd.sync()
                        } }
                        if (plan.expectedBody != null && bytes != plan.expectedBody) throw IOException("响应中断")
                    }
                    val size = part.length()
                    if (total != null && size < total && code == 206) {
                        require(task.etag != null || task.lastModified != null) { "来源未提供续传校验标识" }
                        task = task.copy(downloadedBytes = size, updatedMs = System.currentTimeMillis())
                        saveActive(task); progress(task); return@response
                    }
                    if (total != null && size != total) throw IOException("文件未完整下载")
                    require(size > 0)
                    if (asset != null) validateAttachment(part, asset)
                    val digest = sha256(part)
                    task = task.copy(state = "VERIFYING", downloadedBytes = size, totalBytes = size, sha256 = digest)
                    saveActive(task); progress(task)
                    currentCoroutineContext().ensureActive()
                    mutation.withLock {
                        requireActive(id)
                        val destination = file(id, false)
                        check(!destination.exists() && part.renameTo(destination)) { "下载文件无法完成更名" }
                        complete(task, destination)
                    }
                    finished = true
                }
            }
        } catch (e: CancellationException) { throw e }
        catch (e: Exception) {
            currentCoroutineContext().ensureActive()
            mutation.withLock {
                val task = dao.download(id)
                if (task != null && task.state in setOf("DOWNLOADING", "VERIFYING") && !(task.state == "VERIFYING" && file(id, false).exists())) {
                    val code = when (e) {
                        is OnlineException -> e.code.name
                        is HttpStatusException -> if (e.status in setOf(401, 403)) "AUTH_REQUIRED" else if (e.status == 404) "NOT_FOUND" else "NETWORK"
                        is InvalidAttachment -> "INVALID_CONTENT"
                        is IllegalArgumentException -> "SOURCE_CHANGED"
                        is android.database.sqlite.SQLiteException, is SecurityException, is IllegalStateException -> "STORAGE"
                        else -> if (generateSequence(e as Throwable?) { it.cause }.any {
                            it is android.system.ErrnoException && it.errno in setOf(android.system.OsConstants.ENOSPC, android.system.OsConstants.EROFS, android.system.OsConstants.EACCES)
                        }) "STORAGE" else "NETWORK"
                    }
                    val retry = code == "NETWORK" && task.attempts < 3
                    dao.putDownload(task.copy(state = if (retry) "QUEUED" else "FAILED", errorCode = code,
                        downloadedBytes = file(id, true).length(), updatedMs = System.currentTimeMillis()))
                }
            }
            throw e
        } finally {
            withContext(NonCancellable) { mutation.withLock {
                val task = dao.download(id)
                if (task?.state == "DOWNLOADING" || (task?.state == "VERIFYING" && !file(id, false).exists())) dao.downloadState(id, "QUEUED")
            } }
            running.remove(id, job)
        }
    }
    private suspend fun saveActive(task: DownloadTask) = mutation.withLock { requireActive(task.id); dao.putDownload(task) }
    private suspend fun requireActive(id: String) { if (dao.download(id)?.state !in setOf("DOWNLOADING", "VERIFYING")) throw CancellationException("下载已暂停") }
    private suspend fun complete(task: DownloadTask, destination: File) = db.withTransaction {
        val uri = Uri.fromFile(destination).toString()
        if (task.assetId != null) {
            val asset = requireNotNull(dao.remoteAsset(task.assetId))
            dao.putAsset(WorkAsset(asset.id, asset.workId, "online:${asset.workId}", asset.relativePath, uri, asset.format))
            dao.relinkImportedSubtitles("unbound://asset/${Uri.encode(asset.id)}", uri)
            dao.subtitleUriAvailable(uri, true)
            if (asset.kind in setOf("cover", "image")) com.asmrplayer.android.artwork.CoverArtwork.invalidate(asset.workId)
        } else {
            val versionId = requireNotNull(task.versionId)
            dao.putSource(MediaSource(sourceId(versionId), versionId, null, destination.name, uri,
                destination.length(), System.currentTimeMillis(), kind = "local_downloaded"))
        }
        dao.putDownload(task.copy(state = "COMPLETED", errorCode = null, updatedMs = System.currentTimeMillis()))
    }
    private suspend fun unpublish(task: DownloadTask) {
        if (task.assetId != null) {
            dao.assetAvailable(task.assetId, false)
            dao.asset(task.assetId)?.let {
                dao.subtitleUriAvailable(it.uri, false)
                com.asmrplayer.android.artwork.CoverArtwork.invalidate(it.workId)
            }
        } else dao.setSourceAvailable(sourceId(requireNotNull(task.versionId)), false)
    }
    private class InvalidAttachment : IOException("附件格式或大小不受支持")
    private fun validateAttachment(file: File, asset: RemoteAsset) {
        try { AttachmentContent.validate(file, asset) }
        catch (_: IllegalArgumentException) { throw InvalidAttachment() }
        catch (_: java.nio.charset.CharacterCodingException) { throw InvalidAttachment() }
    }
    private suspend fun reconcile() = mutation.withLock {
        for (task in dao.downloadTasks()) {
            val final = file(task.id, false); val part = file(task.id, true)
            if (task.state in setOf("VERIFYING", "QUEUED", "PAUSED") && final.exists() && task.totalBytes == final.length() && task.sha256 != null && sha256(final) == task.sha256) {
                try {
                    task.assetId?.let { validateAttachment(final, requireNotNull(dao.remoteAsset(it))) }
                    complete(task, final)
                } catch (_: InvalidAttachment) {
                    db.withTransaction { unpublish(task); dao.downloadState(task.id, "FAILED", "INVALID_CONTENT") }
                }
            } else if ((task.state == "VERIFYING" && final.exists()) || (task.state == "COMPLETED" && (!final.exists() || task.totalBytes != final.length()))) {
                db.withTransaction { unpublish(task); dao.downloadState(task.id, "FAILED", if (final.exists()) "INTEGRITY" else "MISSING_FILE") }
            } else if (task.state in setOf("DOWNLOADING", "VERIFYING")) {
                dao.putDownload(task.copy(state = "PAUSED", downloadedBytes = part.length(), updatedMs = System.currentTimeMillis()))
            }
        }
    }
    private fun file(id: String, partial: Boolean): File {
        require(id.matches(Regex("[a-f0-9-]{36}")))
        val root = directory.canonicalFile
        val target = File(root, id + if (partial) ".part" else ".media").canonicalFile
        check(target.parentFile == root)
        return target
    }
    private fun sourceId(versionId: String) = LibraryIndex.id(versionId, "downloaded")
    private suspend fun sha256(file: File): String = withContext(Dispatchers.IO) {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) { currentCoroutineContext().ensureActive(); val size = input.read(buffer); if (size < 0) break; digest.update(buffer, 0, size) }
        }
        digest.digest().joinToString("") { "%02x".format(it) }
    }
}
