package com.asmrplayer.android.online

import android.net.Uri
import androidx.room.withTransaction
import com.asmrplayer.android.data.AccountLibraryEntry
import com.asmrplayer.android.data.CatalogMetadata
import com.asmrplayer.android.data.LibraryDatabase
import com.asmrplayer.android.data.CollectionWork
import com.asmrplayer.android.data.QueueGroup
import com.asmrplayer.android.data.LibraryWork
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

data class SitePlaylist(val id: String, val name: String, val sources: List<String>)

data class AccountLibraryPreview(val account: ProviderAccountState, val favorites: Int, val playlists: Int,
    val playlistWorks: Int, val alreadyInLibrary: Int, val works: List<OnlineWork>,
    val lists: List<SitePlaylist> = emptyList(), val existingOwners: Map<String,String> = emptyMap())

data class AccountLibrarySelection(val workIds: Set<String> = emptySet(), val playlistIds: Set<String> = emptySet()) {
    companion object {
        fun all(preview: AccountLibraryPreview) = AccountLibrarySelection(
            preview.works.mapTo(mutableSetOf()) { it.sourceId },
            preview.lists.mapTo(mutableSetOf()) { it.id },
        )
    }
}

internal fun OnlineWork.catalogMetadata(id: String, origin: String, subtitles: Boolean = hasSubtitle) =
    CatalogMetadata(id, circle, JSONArray(voices).toString(), JSONArray(tags).toString(), description, release, subtitles, origin, sourceId)

internal data class AccountPage(val rows: JSONArray, val hasMore: Boolean)

private fun JSONObject.pageInteger(key: String): Long? {
    val value = opt(key)
    return when (value) {
        is Int -> value.toLong()
        is Long -> value
        is String -> if (value.all { it in '0'..'9' }) value.toLongOrNull() else null
        else -> null
    }?.takeIf { it >= 0 }
}

internal fun accountPage(data: JSONObject, key: String, requested: Int): AccountPage {
    val rows = data.optJSONArray(key) ?: throw JSONException("站点未返回条目列表")
    val pagination = data.optJSONObject("pagination") ?: throw JSONException("站点未返回分页信息")
    val total = pagination.pageInteger("totalCount") ?: throw JSONException("作品或歌单总数无效")
    val empty = total == 0L && requested == 1 && rows.length() == 0
    // Review and playlist endpoints use different keys for the echoed page number.
    val fields = listOf("page", "currentPage").filter(pagination::has)
    if (fields.isEmpty() && !empty) throw JSONException("站点未返回页码")
    if (fields.any { pagination.pageInteger(it) != requested.toLong() }) throw JSONException("返回的页码与请求不符")
    if (empty) return AccountPage(rows, false)
    val size = pagination.pageInteger("pageSize")?.takeIf { it > 0 } ?: throw JSONException("每页条目数无效")
    if (total == 0L || rows.length() == 0) throw JSONException("返回的条目与分页总数不符")
    return AccountPage(rows, requested <= (total - 1) / size)
}

class AccountLibraryImport(private val db: LibraryDatabase, private val provider: AsmrOneProvider) {
    private class LibraryIndexBySource(rows: List<AccountLibraryEntry>, val base: String) {
        private val byId = rows.associateBy { it.id }
        private val bySource = rows.filter { !it.sourceId.isNullOrBlank() && (it.origin == base ||
            (it.inLibrary && base in ProviderAccount.OFFICIAL_BASES && it.origin in ProviderAccount.OFFICIAL_BASES)) }
            .groupBy { it.sourceId }
        private val local = rows.filter { it.rootId != null && it.sourceId.isNullOrBlank() && !it.code.isNullOrBlank() }
            .groupBy { it.code?.uppercase() }

        fun existing(work: OnlineWork): AccountLibraryEntry? {
            val matches = bySource[work.sourceId].orEmpty()
            val cached = matches.firstOrNull { it.inLibrary } ?: matches.firstOrNull()
                ?: byId[LibraryIndex.id("asmr_one", base, work.sourceId)]
            if (cached?.inLibrary == true) return cached
            val localWork = local[work.code.uppercase()]?.singleOrNull()
            return localWork?.takeIf { it.inLibrary } ?: cached ?: localWork
        }
    }

    suspend fun cacheCard(work: OnlineWork, expected: ProviderAccountState): String = db.withTransaction {
        checkAccount(expected)
        val existing = LibraryIndexBySource(db.metadata().accountLibraryEntries(), expected.apiBase).existing(work)
        if(existing != null) return@withTransaction existing.id
        val id = LibraryIndex.id("asmr_one",expected.apiBase,work.sourceId)
        db.library().putWork(LibraryWork(id,null,"",work.title,work.code,work.coverUrl,rating=work.rating,inLibrary=false))
        db.metadata().put(work.catalogMetadata(id,expected.apiBase))
        id
    }

    private fun checkAccount(expected: ProviderAccountState) {
        if (provider.account.state.value != expected) throw OnlineException(OnlineError.SOURCE_CHANGED, "账号或镜像已更改，请重新读取。")
    }

    private suspend fun readPages(
        account: ProviderAccountState, route: String, key: String, section: String, consume: (JSONObject) -> Unit
    ) {
        var page = 1
        while (true) {
            checkAccount(account)
            fun invalidPage(reason: String) = OnlineException(OnlineError.SCHEMA_CHANGED, "无法读取「${section}」第 ${page} 页，${reason}。")
            val result = try {
                accountPage(provider.accountListPage(account, "$route&page=$page"), key, page)
            } catch (e: OnlineException) {
                if (e.code != OnlineError.SCHEMA_CHANGED) throw e
                throw invalidPage("站点未返回有效的 JSON")
            } catch (e: JSONException) {
                throw invalidPage(e.message ?: "站点未返回有效的分页信息")
            }
            try {
                for (i in 0 until result.rows.length()) consume(result.rows.getJSONObject(i))
            } catch (_: JSONException) {
                throw invalidPage("站点返回的条目缺少必要字段")
            } catch (_: IllegalArgumentException) {
                throw invalidPage("站点返回的条目编号无效")
            }
            if (!result.hasMore) return
            page++
        }
    }

    suspend fun preview(): AccountLibraryPreview = withContext(Dispatchers.IO) {
        val account = provider.account.state.value
        if (!account.hasToken) throw OnlineException(OnlineError.AUTH_REQUIRED, "请先登录 asmr.one。")
        val works = linkedMapOf<String, OnlineWork>()
        val favorites = mutableSetOf<String>()
        val playlistWorks = mutableSetOf<String>()
        val playlists = linkedMapOf<String,String>()
        val lists = mutableListOf<SitePlaylist>()
        fun add(row: JSONObject, group: MutableSet<String>): String {
            val work = AsmrJson.work(row)
            group.add(work.sourceId)
            works.putIfAbsent(work.sourceId, work)
            return work.sourceId
        }
        readPages(account, "/api/review?order=updated_at&sort=desc", "works", "我的收藏") { add(it, favorites) }
        for ((filter, section) in listOf("owned" to "我创建的歌单", "liked" to "我收藏的歌单")) {
            readPages(account, "/api/playlist/get-playlists?filterBy=$filter&pageSize=96", "playlists", section) { row ->
                val id = row.get("id")
                if ((id !is String && id !is Number) || id.toString().isBlank()) throw JSONException("Missing playlist ID")
                playlists.putIfAbsent(id.toString(), row.optString("name").ifBlank { row.optString("title").ifBlank { "官网播放列表" } })
            }
        }
        for ((id, name) in playlists) {
            val sources = linkedSetOf<String>()
            readPages(account, "/api/playlist/get-playlist-works?id=${Uri.encode(id)}&pageSize=96", "works", "歌单作品") { sources.add(add(it, playlistWorks)) }
            val namespace = if(account.apiBase in ProviderAccount.OFFICIAL_BASES) "asmr.one" else account.apiBase
            val hash = java.security.MessageDigest.getInstance("SHA-256").digest("$namespace/playlist/$id".toByteArray())
            lists.add(SitePlaylist("asmr-list-" + hash.joinToString("") { "%02x".format(it) },name,sources.toList()))
        }
        val existing = LibraryIndexBySource(db.metadata().accountLibraryEntries(), account.apiBase)
        val matched = works.values.mapNotNull { w -> existing.existing(w)?.takeIf { it.inLibrary }?.let { w.sourceId to it.id } }.toMap()
        val missing = works.values.filter { it.sourceId !in matched }
        checkAccount(account)
        AccountLibraryPreview(account, favorites.size, playlists.size, playlistWorks.size, works.size - missing.size, missing, lists, matched)
    }

    suspend fun confirm(preview: AccountLibraryPreview, keepPlaylists: Boolean = false): Int {
        val all = AccountLibrarySelection.all(preview)
        return confirm(preview, if (keepPlaylists) all else all.copy(playlistIds = emptySet()))
    }

    suspend fun confirm(
        preview: AccountLibraryPreview, selection: AccountLibrarySelection,
    ): Int = db.withTransaction {
        checkAccount(preview.account)
        require(preview.works.count { it.sourceId in selection.workIds } == selection.workIds.size &&
            preview.lists.count { it.id in selection.playlistIds } == selection.playlistIds.size) { "勾选项已变化，请重新预览。" }
        val base = preview.account.apiBase
        val existing = LibraryIndexBySource(db.metadata().accountLibraryEntries(), base)
        val owners = preview.existingOwners.toMutableMap()
        val skippedCached = mutableMapOf<String, String>()
        var added = 0
        for (work in preview.works) {
            val match = existing.existing(work)
            if (work.sourceId !in selection.workIds) {
                if (match != null) {
                    if (match.inLibrary) owners[work.sourceId] = match.id else skippedCached[work.sourceId] = match.id
                }
                continue
            }
            owners[work.sourceId] = match?.id ?: LibraryIndex.id("asmr_one", base, work.sourceId)
            if (match?.inLibrary == true) continue
            if (match == null) {
                val id = LibraryIndex.id("asmr_one", base, work.sourceId)
                db.library().putWork(LibraryWork(id, null, "", work.title, work.code, work.coverUrl, rating = work.rating))
                db.metadata().put(work.catalogMetadata(id, base))
            } else {
                db.library().membership(match.id, true)
                if (match.sourceId.isNullOrBlank()) db.metadata().put(work.catalogMetadata(match.id, base))
            }
            added++
        }
        if (selection.playlistIds.isNotEmpty()) {
            var next = (db.queueGroups().all().maxOfOrNull { it.sortOrder } ?: -1) + 1
            for (list in preview.lists) {
                if (list.id !in selection.playlistIds) continue
                val old = db.queueGroups().get(list.id)
                require(old == null || old.isWorks)
                val choices = old?.works().orEmpty().associateBy { it.id }
                val items = (list.sources.mapNotNull { source ->
                    val id = owners[source] ?: skippedCached[source]?.takeIf { it in choices } ?: return@mapNotNull null
                    (choices[id] ?: CollectionWork(id)).copy(fromSite=true)
                } + choices.values.filterNot { it.fromSite }).distinctBy { it.id }
                db.queueGroups().put(QueueGroup(list.id,list.name,sortOrder = old?.sortOrder ?: next++,workItems = CollectionWork.encode(items)))
            }
        }
        checkAccount(preview.account)
        added
    }
}
