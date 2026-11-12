package com.asmrplayer.android.online

import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.library.ResolvedVersion
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

class AsmrOneProvider(val account: ProviderAccount, val http: HttpTransport,
    private val metadataNodes:List<String> = ProviderAccount.OFFICIAL_BASES) {
    suspend fun browse(query: String = "", page: Int = 1, order: String = "release", ascending: Boolean = false,
        includeNsfw: Boolean = false, includeUnrated: Boolean = true, filters:BrowseFilters=BrowseFilters()): OnlinePage {
        require(page in 1..100_000 && query.length <= 1024)
        require(order in setOf("popular", "release", "dl_count", "rate_average_2dp", "id", "create_date", "review_count"))
        val base = account.state.value.apiBase
        if(!includeNsfw&&filters.rating=="adult") return OnlinePage(emptyList(),page,20,0)
        val terms=filters.query(query)
        val effective = if (includeNsfw || filters.rating == "general") terms else "${terms} \$age:general\$".trim()
        val route = if (effective.isEmpty()) "/api/works" else "/api/search/${Uri.encode(effective)}"
        val sub=if(filters.subtitleOnly)1 else 0
        val body=if(order=="popular" && query.isBlank()) {
            val payload=JSONObject().put("keyword",effective.ifBlank { " " }).put("page",page).put("subtitle",sub)
                .put("localSubtitledWorks",org.json.JSONArray()).put("withPlaylistStatus",org.json.JSONArray())
            request(base, "/api/recommender/popular", payload.toString())
        } else {
            // A free-text search uses the source's search index. Only an explicitly selected ranking sets the order.
            val orderParam = if (order == "popular") "" else "order=$order&"
            request(base, "$route?${orderParam}sort=${if (ascending) "asc" else "desc"}&page=$page&subtitle=$sub")
        }
        val result = parse { AsmrJson.page(body, page).also { p -> p.items.forEach { it.coverUrl?.let(http::validate) } } }
        if (account.state.value.apiBase != base) throw CancellationException("来源已切换")
        return result.copy(items = result.items.filter { work ->
            (includeNsfw || work.rating != "NSFW") && (includeUnrated || work.rating != "UNRATED") &&
                when(filters.rating) {
                    "adult" -> work.rating == "NSFW"
                    "general" -> work.rating == "SAFE"
                    else -> true
                }
        })
    }
    suspend fun detail(rawId: String, base: String = account.state.value.apiBase): OnlineDetail = coroutineScope {
        val id = AsmrJson.workId(rawId)
        val work = async { work(id,base) }
        val files = async { tree(base, id) }
        OnlineDetail(work.await(), files.await())
    }
    suspend fun work(rawId:String, base:String=account.state.value.apiBase):OnlineWork {
        val id=AsmrJson.workId(rawId)
        val route="/api/workInfo/${AsmrJson.workInfoId(id)}"
        val body=try { metadataRequest(base,route,true) } catch(error:OnlineException) {
            if(error.code!=OnlineError.NETWORK || base !in metadataNodes) throw error
            var recovered:String?=null
            for(mirror in metadataNodes.filter { it!=base }.take(2)) {
                try { recovered=metadataRequest(mirror,route,false); break }
                catch(e:CancellationException) { throw e }
                catch(_:OnlineException) { /* Try the next public node without sharing account credentials. */ }
            }
            recovered ?: throw error
        }
        return parse { readWork(body) }.also {
            check(it.sourceId==id) { "来源返回的作品身份不一致" }
        }
    }
    private suspend fun metadataRequest(base:String,route:String,useAccount:Boolean):String = try {
        withTimeout(12_000) { request(base,route,useAccount=useAccount) }
    } catch(_:TimeoutCancellationException) {
        throw OnlineException(OnlineError.NETWORK,"来源作品资料请求超时")
    }
    suspend fun tree(base: String, id: String): List<OnlineFile> = parse { AsmrJson.tree(request(base, "/api/tracks/${AsmrJson.workId(id)}?v=2"), id) }
    suspend fun resolve(reference: RemoteAsset): RemoteAddress {
        if (reference.kind == "cover") {
            val work = work(reference.sourceWorkId,reference.origin)
            val url = work.coverUrl ?: throw OnlineException(OnlineError.NOT_FOUND, "来源没有封面")
            http.validate(url)
            return RemoteAddress(url, null)
        }
        val matches = tree(reference.origin, reference.sourceWorkId).filter { it.id == reference.remoteFileId && it.kind == reference.kind }
        val file = matches.firstOrNull { it.path == reference.relativePath } ?: matches.singleOrNull()
            ?: throw OnlineException(OnlineError.SOURCE_CHANGED, "远端附件已变化，请刷新作品")
        if (file.format != reference.format) throw OnlineException(OnlineError.SOURCE_CHANGED, "附件格式已变化")
        val url = file.downloadUrl ?: file.streamUrl ?: throw OnlineException(OnlineError.SCHEMA_CHANGED, "来源没有附件地址")
        http.validate(url)
        return RemoteAddress(url, file.bytes)
    }
    suspend fun resolve(reference: RemoteMedia, download: Boolean = false): RemoteAddress {
        // Signed URLs are resolved at use time and are never saved to Room or logs.
        val matches = tree(reference.origin, reference.sourceWorkId).filter { it.id == reference.remoteFileId }
        val file = matches.firstOrNull { it.path == reference.relativePath } ?: matches.singleOrNull()
            ?: throw OnlineException(OnlineError.SOURCE_CHANGED, "远端音轨已变化，请刷新作品后选择版本")
        val url = (if (download) file.downloadUrl ?: file.streamUrl else file.streamUrl ?: file.downloadUrl)
            ?: throw OnlineException(OnlineError.SCHEMA_CHANGED, "远端没有可用音频地址")
        http.validate(url)
        return RemoteAddress(url, file.bytes)
    }
    private suspend fun request(base: String, route: String, jsonBody: String? = null, useAccount:Boolean=true): String {
        http.validate(base)
        repeat(3) { attempt ->
            try { return http.text(base + route, mapOf("Accept" to "application/json, text/plain, */*",
                "Content-Type" to "application/json","Origin" to "https://asmr.one","Referer" to "https://asmr.one/") +
                (if(useAccount) withContext(Dispatchers.IO) { account.headers(base) } else emptyMap()), jsonBody) }
            catch (e: CancellationException) { throw e }
            catch (e: OnlineException) { throw e }
            catch (e: Exception) {
                currentCoroutineContext().ensureActive()
                val status = (e as? HttpStatusException)?.status
                val failure = when (status) {
                    401, 403 -> OnlineException(OnlineError.AUTH_REQUIRED, "来源需要认证或权限不足，请检查 Token")
                    404 -> OnlineException(OnlineError.NOT_FOUND, "来源未找到作品或文件")
                    429 -> OnlineException(OnlineError.RATE_LIMITED, "来源请求过于频繁，请稍后重试")
                    else -> OnlineException(OnlineError.NETWORK, "来源请求失败，请检查网络后重试")
                }
                if (attempt == 2 || (status != null && status !in 500..599 && status != 429)) throw failure
                delay(300L * (1 shl attempt))
            }
        }
        error("unreachable")
    }
    private fun readWork(text: String) = AsmrJson.work(JSONObject(text)).also { it.coverUrl?.let(http::validate) }
    private suspend fun <T> parse(block: suspend () -> T): T = withContext(Dispatchers.Default) {
        try { block() } catch (e: CancellationException) { throw e } catch (e: OnlineException) { throw e }
        catch (_: Exception) { throw OnlineException(OnlineError.SCHEMA_CHANGED, "来源响应格式已变化") }
    }
}

data class ImportedOnlineWork(val workId: String, val versionsByPath: Map<String, String>, val assetIds: List<String> = emptyList(), val coverAssetId: String? = null) {
    val versionIds get() = versionsByPath.values.toList()
}

class OnlineRepository(private val db: LibraryDatabase, val provider: AsmrOneProvider) {
    private val dao = db.library()
    val account get() = provider.account
    fun assets(workId: String) = dao.observeRemoteAssets(workId)
    suspend fun browse(query: String = "", page: Int = 1, order: String = "release", ascending: Boolean = false, filters:BrowseFilters=BrowseFilters()): OnlinePage {
        val prefs = dao.observePreferences().first() ?: LibraryPreference()
        return provider.browse(query, page, order, ascending, prefs.includeNsfw, prefs.includeUnrated, filters)
    }
    private data class DetailKey(val account: ProviderAccountState, val id: String)
    private data class DetailEntry(val value: OnlineDetail, val loadedMs: Long, val fetchedAt: Long)
    private val detailCache = object : android.util.LruCache<DetailKey, DetailEntry>(8000) {
        override fun sizeOf(key: DetailKey, value: DetailEntry) = 128 + value.value.files.size
    }
    private val detailLocks = Array(16) { kotlinx.coroutines.sync.Mutex() }

    internal fun cachedDetail(origin: String, id: String): OnlineDetail? {
        val current = account.state.value
        if (current.apiBase != origin) return null
        val normalized = runCatching { AsmrJson.workId(id) }.getOrNull() ?: return null
        return detailCache.get(DetailKey(current, normalized))?.value
    }

    suspend fun detail(id: String, force: Boolean = false): OnlineDetail {
        val key = DetailKey(account.state.value, AsmrJson.workId(id))
        return detailLocks[(key.hashCode() and Int.MAX_VALUE) % detailLocks.size].withLock {
            val now = android.os.SystemClock.elapsedRealtime()
            val cached = detailCache.get(key)
            if (!force && cached != null && now - cached.loadedMs < 300_000) return@withLock cached.value
            val fresh = try { provider.detail(key.id, key.account.apiBase) }
            catch (e: OnlineException) {
                if (e.code in setOf(OnlineError.AUTH_REQUIRED, OnlineError.NOT_FOUND)) detailCache.remove(key)
                throw e
            }
            if (account.state.value != key.account) throw CancellationException("来源已切换")
            detailCache.put(key, DetailEntry(fresh, android.os.SystemClock.elapsedRealtime(), System.currentTimeMillis()))
            fresh
        }
    }
    suspend fun importWork(id: String, addToLibrary: Boolean = true, reuseDetail: Boolean = false): ImportedOnlineWork {
        val selectedAccount = account.state.value
        val base = selectedAccount.apiBase
        val detail = detail(id, force = !reuseDetail)
        if (account.state.value != selectedAccount) throw CancellationException("来源已切换")
        val revision = detailCache.get(DetailKey(selectedAccount, detail.work.sourceId))
            ?.takeIf { it.value === detail }?.fetchedAt ?: System.currentTimeMillis()
        var artworkChanged = !reuseDetail
        return db.withTransaction {
            val workId = LibraryIndex.id("asmr_one", base, detail.work.sourceId)
            val old = dao.work(workId)
            artworkChanged = artworkChanged || old == null || old.coverUri != detail.work.coverUrl
            dao.putWork(LibraryWork(workId, null, "", detail.work.title, detail.work.code, detail.work.coverUrl,
                old?.favorite ?: false, old?.rating ?: detail.work.rating, old?.inLibrary == true || addToLibrary))
            db.metadata().put(CatalogMetadata(workId, detail.work.circle, org.json.JSONArray(detail.work.voices).toString(),
                org.json.JSONArray(detail.work.tags).toString(),detail.work.description,detail.work.release,
                detail.work.hasSubtitle||detail.files.any{it.kind=="subtitle"},base,detail.work.sourceId))
            dao.invalidateOnlineWork(workId)
            val versionsByPath = detail.files.filter { it.kind == "audio" }.associate { file ->
                val trackId = LibraryIndex.id(workId, "track", file.path.substringBeforeLast('.'))
                val versionId = file.versionId(workId)
                dao.putTrack(LibraryTrack(trackId, workId, file.title, file.path.substringBeforeLast('/', "")))
                dao.putVersion(TrackVersion(versionId, trackId, file.format))
                dao.putRemote(RemoteMedia(versionId, base, detail.work.sourceId, file.id, file.path, file.bytes, file.format))
                dao.putSource(MediaSource(LibraryIndex.id(versionId, "online"), versionId, null, file.path,
                    "asmr-online://$versionId", file.bytes ?: 0, modifiedMs = if (file.id.startsWith("${detail.work.sourceId}:path:")) revision else 0, kind = "online_provider"))
                file.path to versionId
            }
            dao.invalidateRemoteAssets(workId)
            val assetIds = detail.files.filter { it.kind != "audio" }.map { file ->
                val assetId = LibraryIndex.id(workId, "asset", file.id, file.path)
                dao.putRemoteAsset(RemoteAsset(assetId, workId, base, detail.work.sourceId, file.id, file.path, file.kind, file.format, file.bytes))
                assetId
            }
            val coverId = detail.work.coverUrl?.let {
                LibraryIndex.id(workId, "cover").also { assetId ->
                    dao.putRemoteAsset(RemoteAsset(assetId, workId, base, detail.work.sourceId, "cover", "cover", "cover", "image", null))
                }
            }
            ImportedOnlineWork(workId, versionsByPath, assetIds, coverId)
        }.also { if (artworkChanged) com.asmrplayer.android.artwork.CoverArtwork.invalidate(it.workId) }
    }
    /** Refresh only provider metadata. Collection membership and track identities stay owned by the library. */
    suspend fun refreshMetadata(workId:String, sourceId:String) {
        val selectedAccount=account.state.value
        val work=provider.work(sourceId,selectedAccount.apiBase)
        check(account.state.value==selectedAccount) { "来源已切换，请重试" }
        db.withTransaction {
            val old=requireNotNull(dao.work(workId)) { "作品已移除" }
            dao.putWork(old.copy(title=work.title,code=work.code,coverUri=work.coverUrl ?: old.coverUri,
                rating=if(old.rating=="UNRATED") work.rating else old.rating))
            db.metadata().put(CatalogMetadata(workId,work.circle,org.json.JSONArray(work.voices).toString(),
                org.json.JSONArray(work.tags).toString(),work.description,work.release,work.hasSubtitle,
                selectedAccount.apiBase,work.sourceId))
        }
        com.asmrplayer.android.artwork.CoverArtwork.invalidate(workId)
    }
    private data class StreamKey(val media: RemoteMedia, val accountScope: String, val revision: Long)
    private data class StreamAddress(val value: RemoteAddress, val resolvedAt: Long)
    private val addresses = android.util.LruCache<StreamKey, StreamAddress>(32)

    internal suspend fun resolve(versionId: String): ResolvedVersion = resolveStream(versionId, false)

    internal suspend fun resolveStream(versionId: String, refresh: Boolean): ResolvedVersion {
        val ref = dao.remote(versionId) ?: throw OnlineException(OnlineError.NOT_FOUND, "没有远端来源")
        val sourceId = LibraryIndex.id(versionId, "online")
        val revision = if (ref.remoteFileId.startsWith("${ref.sourceWorkId}:path:")) dao.source(sourceId)?.modifiedMs ?: 0 else 0
        val key = StreamKey(ref, account.cacheScope(ref.origin), revision)
        val now = android.os.SystemClock.elapsedRealtime()
        val recent = if (refresh) null else addresses.get(key)?.takeIf { now - it.resolvedAt < 120_000 }
        val accountState = account.state.value
        val entry = if (!refresh && accountState.apiBase == ref.origin)
            detailCache.get(DetailKey(accountState, ref.sourceWorkId)) else null
        val file = entry?.takeIf { android.os.SystemClock.elapsedRealtime() - it.loadedMs < 300_000 }
            ?.value?.files?.find { it.id == ref.remoteFileId && it.path == ref.relativePath && it.kind == "audio" }
        val cachedUrl = file?.let { it.streamUrl ?: it.downloadUrl }
        val address = recent?.value ?: (if (cachedUrl == null) provider.resolve(ref) else {
            provider.http.validate(cachedUrl)
            RemoteAddress(cachedUrl, file.bytes)
        }).also { addresses.put(key, StreamAddress(it, now)) }
        return ResolvedVersion(versionId, sourceId, address.url, dao.version(versionId)?.title ?: "音频")
    }
}
