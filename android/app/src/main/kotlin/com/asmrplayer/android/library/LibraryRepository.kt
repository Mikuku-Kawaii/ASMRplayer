package com.asmrplayer.android.library

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.IOException
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

data class ScanState(val running: Boolean = false, val documents: Int = 0, val error: String? = null)
data class ResolvedVersion(val versionId: String, val sourceId: String, val uri: String, val title: String)
data class ReauthorizationNeeds(val directoryIds: List<String>, val audioSourceIds: List<String>, val subtitleSourceIds: List<String>)
data class ReauthorizationItem(val id: String, val kind: String, val title: String)
data class RootRelinkPreview(val rootId: String, val directoryTitle: String, val mode: RootMode,
    val matchedAudio: Int, val missingPaths: List<String>, val newAudio: Int)

/** UI calls suspend intents and collects Flows. No Activity/Compose dependencies or player ownership. */
class LibraryRepository(private val context: Context, private val db: LibraryDatabase,
    private val remoteResolver: (suspend (String) -> ResolvedVersion)? = null) {
    private val dao = db.library()
    val collections = LibraryCollections(db)
    private val resolver = context.contentResolver
    private val reader = SafTreeReader(resolver)
    private val locks = ConcurrentHashMap<String, Mutex>()
    private val scans = ConcurrentHashMap<String, Job>()
    private val mutableScans = MutableStateFlow<Map<String, ScanState>>(emptyMap())
    val scanStates = mutableScans.asStateFlow()
    val roots = dao.observeRoots()
    val preferences = dao.observePreferences().map { it ?: LibraryPreference() }
    val works = dao.observeWorks().map { it.sortedWith(compareBy(NaturalOrder) { w -> w.title }) }
    val visibleWorks = combine(works, preferences) { works, prefs -> works.filter {
        when (it.rating) { "NSFW" -> prefs.includeNsfw; "UNRATED" -> prefs.includeUnrated; else -> true }
    } }
    val positions = dao.observePositions()
    private val versionOrder = compareBy<VersionDetail, String>(NaturalOrder) { it.groupPath }
        .thenComparator { a, b -> NaturalOrder.compare(a.title, b.title) }.thenBy { it.format }
    fun versions(workId: String) = dao.observeVersions(workId).map { it.sortedWith(versionOrder) }
    internal val versionsByWork = dao.observeAllVersions().map { rows ->
        rows.groupBy { it.workId }.mapValues { (_, versions) -> versions.sortedWith(versionOrder) }
    }.flowOn(Dispatchers.Default)
    fun assets(workId: String) = dao.observeAssets(workId)
    fun bookmarks(versionId: String) = dao.observeBookmarks(versionId)
    suspend fun work(id: String) = dao.work(id)
    suspend fun position(versionId: String) = dao.position(versionId)
    suspend fun setMembership(workId: String, value: Boolean) { require(dao.membership(workId, value) == 1) }
    suspend fun bookmark(id: String) = dao.bookmark(id)
    suspend fun restoreBookmark(value: Bookmark) = dao.putBookmark(value)
    suspend fun setFavorite(workId: String, favorite: Boolean) { require(dao.favorite(workId, favorite) == 1) }
    suspend fun setRating(workId: String, rating: String) {
        require(rating in setOf("UNRATED", "SAFE", "NSFW")); require(dao.rating(workId, rating) == 1)
    }
    suspend fun setContentFilter(includeNsfw: Boolean, includeUnrated: Boolean) = dao.putPreferences(
        LibraryPreference(includeNsfw = includeNsfw, includeUnrated = includeUnrated))

    /** Pass the existing ID when reauthorizing the SAME logical tree; relative-path identities survive URI changes. */
    suspend fun authorizeRoot(treeUri: Uri, mode: RootMode = RootMode.AUTO, existingRootId: String? = null): LibraryRoot = withContext(Dispatchers.IO) {
        require(androidx.core.provider.DocumentsContractCompat.isTreeUri(treeUri))
        resolver.takePersistableUriPermission(treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        val id = existingRootId ?: dao.rootByUri(treeUri.toString())?.id ?: UUID.randomUUID().toString()
        scans[id]?.cancelAndJoin()
        locks.getOrPut(id) { Mutex() }.withLock {
            val old = dao.root(id)
            if (existingRootId != null) requireNotNull(old) { "目录记录不存在" }
            val duplicate = dao.rootByUri(treeUri.toString())
            require(duplicate == null || duplicate.id == id) { "目录已导入" }
            val root = LibraryRoot(id, treeUri.toString(), old?.title ?: "已授权目录", mode.name, false, old?.lastScanMs ?: 0)
            db.withTransaction {
                dao.putRoot(root)
                dao.invalidateRoot(id); dao.invalidateSubtitles(id); dao.invalidateAssets(id)
            }
            root
        }
    }

    suspend fun preview(treeUri: Uri, mode: RootMode = RootMode.AUTO): ScanPreview {
        val (title, files) = reader.read(treeUri)
        return withContext(Dispatchers.Default) { LibraryIndex.build(title, files, mode) }
    }

    fun cancelScan(rootId: String) { scans[rootId]?.cancel() }

    suspend fun removeRoot(id: String) = withContext(Dispatchers.IO) {
        scans[id]?.cancelAndJoin()
        locks.getOrPut(id) { Mutex() }.withLock {
            val root = db.withTransaction {
                val root = requireNotNull(dao.root(id)) { "目录已移除" }
                dao.unlinkOriginals(id); dao.unlinkTranslations(id)
                dao.forgetRootSubtitles(id); dao.forgetRootAssets(id); dao.forgetRootSources(id)
                // Preserve logical tracks and versions for likes, playlists, bookmarks and history.
                dao.detachRootWorks(id); dao.forgetRoot(id)
                root
            }
            mutableScans.update { it - id }
            val tree = Uri.parse(root.treeUri)
            if (tree.scheme == "content" && resolver.persistedUriPermissions.any { it.uri == tree && it.isReadPermission }) {
                try { resolver.releasePersistableUriPermission(tree, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
                catch (_: SecurityException) { /* Already revoked by the document provider. */ }
            }
        }
    }

    suspend fun reauthorizationNeeds(): ReauthorizationNeeds = withContext(Dispatchers.IO) {
        val grants = resolver.persistedUriPermissions.filter { it.isReadPermission }.map { it.uri.toString() }.toSet()
        fun grantedOrOwned(value: String): Boolean {
            if(value in grants) return true
            val uri=Uri.parse(value)
            if(uri.scheme!="content" || uri.authority!="${context.packageName}.imports") return false
            return runCatching { resolver.openAssetFileDescriptor(uri,"r")?.use { true } ?: false }.getOrDefault(false)
        }
        ReauthorizationNeeds(roots.first().filter { !it.available || it.treeUri !in grants }.map { it.id },
            dao.standaloneSources().filter { !it.available || !grantedOrOwned(it.uri) }.map { it.id },
            dao.standaloneSubtitles().filter { it.uri.startsWith("unbound://subtitle/") || (it.uri.startsWith("content:") && (!it.available || !grantedOrOwned(it.uri))) }.map { it.id })
    }
    suspend fun reauthorizationItems(): List<ReauthorizationItem> {
        val needs = reauthorizationNeeds()
        return needs.directoryIds.map { ReauthorizationItem(it, "directory", dao.root(it)?.title ?: "作品目录") } +
            needs.audioSourceIds.map { id -> ReauthorizationItem(id, "audio", dao.source(id)?.let { dao.version(it.versionId)?.title } ?: "音频文件") } +
            needs.subtitleSourceIds.map { ReauthorizationItem(it, "subtitle", dao.subtitle(it)?.title ?: "字幕文件") }
    }

    /** Read-only comparison. Caller supplies a tree selected through OpenDocumentTree. */
    suspend fun previewReauthorization(rootId: String, treeUri: Uri): RootRelinkPreview {
        require(androidx.core.provider.DocumentsContractCompat.isTreeUri(treeUri))
        val root = requireNotNull(dao.root(rootId))
        val (title, files) = reader.read(treeUri)
        val preview = LibraryIndex.build(title, files, relinkMode(root))
        return compareRelink(rootId, title, preview)
    }

    /** Re-read and commit only a complete matching tree. Failure leaves the old binding untouched. */
    suspend fun reauthorizeRoot(rootId: String, treeUri: Uri, allowMissing: Boolean = false): RootRelinkPreview = coroutineScope {
        require(androidx.core.provider.DocumentsContractCompat.isTreeUri(treeUri))
        scans[rootId]?.cancelAndJoin()
        val job = currentCoroutineContext().job
        check(scans.putIfAbsent(rootId, job) == null) { "目录正在扫描" }
        try { locks.getOrPut(rootId) { Mutex() }.withLock {
            val root = requireNotNull(dao.root(rootId))
            val duplicate = dao.rootByUri(treeUri.toString())
            require(duplicate == null || duplicate.id == rootId) { "目录已绑定其他根目录" }
            mutableScans.update { it + (rootId to ScanState(running = true)) }
            val (title, files) = reader.read(treeUri) { count -> mutableScans.update { it + (rootId to ScanState(true, count)) } }
            val preview = withContext(Dispatchers.Default) { LibraryIndex.build(title, files, relinkMode(root)) }
            val compared = compareRelink(rootId, title, preview)
            require(compared.matchedAudio > 0 || dao.sourcesForRoot(rootId).isEmpty()) { "所选目录没有匹配的原音频，请选择同一逻辑目录" }
            require(allowMissing || compared.missingPaths.isEmpty()) { "部分原音频缺失，请先预览并明确允许缺失" }
            withContext(Dispatchers.IO) { resolver.takePersistableUriPermission(treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            currentCoroutineContext().ensureActive()
            commit(root.copy(treeUri = treeUri.toString(), title = title, mode = preview.mode.name), preview)
            mutableScans.update { it + (rootId to ScanState(documents = files.size)) }
            compared
        } } catch (e: Exception) {
            mutableScans.update { it + (rootId to ScanState(error = "重新授权未完成，保留原目录记录")) }; throw e
        } finally { scans.remove(rootId, job) }
    }
    private suspend fun relinkMode(root: LibraryRoot): RootMode {
        if (root.mode != "AUTO") return RootMode.valueOf(root.mode)
        val paths = works.first().filter { it.rootId == root.id }.map { it.relativePath }
        return when { paths.isEmpty() -> RootMode.AUTO; paths.any { it.isEmpty() } -> RootMode.WORK; else -> RootMode.COLLECTION }
    }
    private suspend fun compareRelink(id: String, title: String, preview: ScanPreview): RootRelinkPreview {
        val expected = dao.sourcesForRoot(id).map { it.relativePath }.toSet()
        val found = preview.works.flatMap { it.audio }.map { it.path }.toSet()
        return RootRelinkPreview(id, title, preview.mode, (expected intersect found).size, (expected - found).sorted(), (found - expected).size)
    }
    suspend fun reauthorizeAudioSource(sourceId: String, uri: Uri): ResolvedVersion = withContext(Dispatchers.IO) {
        val source = requireNotNull(dao.source(sourceId))
        require(source.rootId == null && source.kind == "local_imported" && uri.scheme == "content")
        if (uri.authority != "${context.packageName}.imports") resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        resolver.openAssetFileDescriptor(uri, "r")?.use { } ?: throw IOException("文件不可读")
        db.withTransaction {
            val duplicate = dao.sourceByUri(uri.toString())
            require(duplicate == null || duplicate.id == sourceId) { "文件已绑定其他来源" }
            dao.putSource(source.copy(uri = uri.toString(), available = true))
        }
        ResolvedVersion(source.versionId, source.id, uri.toString(), dao.version(source.versionId)!!.title)
    }

    suspend fun scan(rootId: String): ScanPreview = coroutineScope {
        val job = currentCoroutineContext().job
        check(scans.putIfAbsent(rootId, job) == null) { "目录正在扫描" }
        try {
            locks.getOrPut(rootId) { Mutex() }.withLock {
                val root = requireNotNull(dao.root(rootId))
                mutableScans.update { it + (rootId to ScanState(running = true)) }
                try {
                    val (title, files) = reader.read(Uri.parse(root.treeUri)) { count ->
                        mutableScans.update { it + (rootId to ScanState(true, count)) }
                    }
                    val result = withContext(Dispatchers.Default) { LibraryIndex.build(title, files, RootMode.valueOf(root.mode)) }
                    commit(root.copy(title = title), result)
                    mutableScans.update { it + (rootId to ScanState(documents = files.size)) }
                    result
                } catch (e: CancellationException) {
                    mutableScans.update { it + (rootId to ScanState(error = "扫描已取消，保留原有库记录")) }; throw e
                } catch (e: Exception) {
                    currentCoroutineContext().ensureActive()
                    if (e is SecurityException || e is java.io.FileNotFoundException) db.withTransaction {
                        dao.putRoot(root.copy(available = false)); dao.invalidateRoot(rootId)
                        dao.invalidateSubtitles(rootId); dao.invalidateAssets(rootId)
                    }
                    mutableScans.update { it + (rootId to ScanState(error = "无法完成扫描，请检查目录授权或提供者后重试")) }
                    throw e
                }
            }
        } finally {
            scans.remove(rootId, job)
            if (job.isCancelled) mutableScans.update { it + (rootId to ScanState(error = "扫描已取消，保留原有库记录")) }
        }
    }

    /** Only a COMPLETE enumeration reaches this transaction. Unchanged source rows retain stable IDs. */
    internal suspend fun commit(root: LibraryRoot, preview: ScanPreview) = db.withTransaction {
        val previous = dao.sourcesForRoot(root.id).associateBy { it.id }
        val manualSubtitles = dao.manualSubtitles(root.id).groupBy { it.relativePath }
        dao.invalidateRoot(root.id); dao.invalidateSubtitles(root.id); dao.invalidateAssets(root.id)
        preview.works.forEach { work ->
            currentCoroutineContext().ensureActive()
            val workId = LibraryIndex.id(root.id, "work", work.path)
            val old = dao.work(workId)
            dao.putWork(LibraryWork(workId, root.id, work.path, work.title, work.code, work.cover?.uri,
                old?.favorite ?: false, old?.rating ?: "UNRATED", old?.inLibrary ?: true))
            work.assets.forEach { asset ->
                dao.putAsset(WorkAsset(LibraryIndex.id(root.id, "asset", asset.path), workId,
                    root.id, asset.path, asset.uri, asset.extension))
                manualSubtitles[asset.path].orEmpty().forEach { sub -> dao.putSubtitle(sub.copy(uri = asset.uri, available = true)) }
            }
            work.audio.forEach { audio ->
                val relative = audio.path.removePrefix(work.path.takeIf { it.isNotEmpty() }?.plus('/') ?: "")
                val trackId = LibraryIndex.id(workId, "track", relative.substringBeforeLast('.'))
                val versionId = LibraryIndex.id(workId, "version", relative)
                dao.putTrack(LibraryTrack(trackId, workId, audio.stem, relative.substringBeforeLast('/', "")))
                dao.putVersion(TrackVersion(versionId, trackId, audio.extension))
                val source = MediaSource(LibraryIndex.id(root.id, "source", audio.path), versionId, root.id,
                    audio.path, audio.uri, audio.size, audio.modifiedMs)
                // No audio decoding, hashing or whole-file reads during an incremental scan.
                val unchanged = previous[source.id] == source
                if (unchanged) dao.setSourceAvailable(source.id, true) else dao.putSource(source)
                work.subtitles[audio.path].orEmpty().forEach { sub -> dao.putSubtitle(SubtitleSource(
                    LibraryIndex.id(versionId, "subtitle", sub.path), versionId, root.id, sub.path, sub.uri, sub.name, sub.extension)) }
            }
        }
        dao.putRoot(root.copy(available = true, lastScanMs = System.currentTimeMillis()))
    }.also {
        preview.works.forEach { com.asmrplayer.android.artwork.CoverArtwork.invalidate(LibraryIndex.id(root.id, "work", it.path)) }
    }

    /** Single-document imports never enumerate parents. External pickers must provide persistable permission; owned share copies need none. */
    suspend fun importDocument(uri: Uri, title: String): ResolvedVersion = withContext(Dispatchers.IO) {
        require(uri.scheme == "content")
        if (uri.authority != "${context.packageName}.imports") resolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        resolver.openAssetFileDescriptor(uri, "r")?.use { } ?: throw IOException("文件不可读")
        db.withTransaction {
            val existing = dao.sourceByUri(uri.toString())
            if (existing != null) {
                dao.setSourceAvailable(existing.id, true)
                ResolvedVersion(existing.versionId, existing.id, existing.uri, dao.version(existing.versionId)?.title ?: title)
            } else {
                val versionId = UUID.randomUUID().toString(); val workId = UUID.randomUUID().toString(); val trackId = UUID.randomUUID().toString()
                val source = MediaSource(UUID.randomUUID().toString(), versionId, null, title, uri.toString())
                dao.putWork(LibraryWork(workId, null, "", title))
                dao.putTrack(LibraryTrack(trackId, workId, title, ""))
                dao.putVersion(TrackVersion(versionId, trackId, title.substringAfterLast('.', "").lowercase()))
                dao.putSource(source)
                ResolvedVersion(versionId, source.id, source.uri, title)
            }
        }
    }

    suspend fun resolve(versionId: String): ResolvedVersion = resolveSource(versionId, deferOnline = false)

    internal suspend fun resolveSource(versionId: String, deferOnline: Boolean): ResolvedVersion = withContext(Dispatchers.IO) {
        val version = requireNotNull(dao.version(versionId)) { "音轨版本不存在" }
        val sources = dao.sources(versionId).filter { it.available }.sortedBy {
            when (it.kind) { "local_imported" -> 0; "local_downloaded" -> 1; else -> 2 }
        }
        for (source in sources) {
            if (source.kind == "online_provider" && deferOnline) {
                return@withContext ResolvedVersion(versionId, source.id, "asmr://version/$versionId", version.title)
            }
            if (source.kind == "online_provider") return@withContext requireNotNull(remoteResolver) { "在线来源尚未配置" }(versionId)
            try {
                val uri = Uri.parse(source.uri)
                if (source.kind == "local_downloaded") {
                    val root = java.io.File(context.noBackupFilesDir, "offline").canonicalFile
                    val file = java.io.File(requireNotNull(uri.path)).canonicalFile
                    if (uri.scheme != "file" || file.parentFile != root || file.name.endsWith(".part") || !file.isFile) throw IOException("下载文件不存在")
                } else resolver.openAssetFileDescriptor(uri, "r")?.use { } ?: throw IOException("文件不存在")
                return@withContext ResolvedVersion(versionId, source.id, source.uri, version.title)
            } catch (e: SecurityException) { dao.setSourceAvailable(source.id, false) }
            catch (e: IOException) { dao.setSourceAvailable(source.id, false) }
        }
        throw IOException("音频不可用，请重新授权或扫描目录")
    }

    /** Online-capable queues keep a stable placeholder; the Media3 data source resolves at each open/seek. */
    internal suspend fun playbackReference(versionId: String): ResolvedVersion {
        val version = requireNotNull(dao.version(versionId))
        val online = dao.sources(versionId).firstOrNull { it.available && it.kind == "online_provider" }
        return if (online != null) ResolvedVersion(versionId, online.id, "asmr://version/$versionId", version.title) else resolve(versionId)
    }

    suspend fun addBookmark(versionId: String, positionMs: Long, note: String = ""): Bookmark {
        requireNotNull(dao.version(versionId)); require(positionMs >= 0 && note.length <= 4096)
        return Bookmark(UUID.randomUUID().toString(), versionId, positionMs, note).also { dao.putBookmark(it) }
    }
    suspend fun removeBookmark(id: String) = dao.deleteBookmark(id)
}
