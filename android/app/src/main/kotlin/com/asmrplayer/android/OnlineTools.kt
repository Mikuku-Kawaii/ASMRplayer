package com.asmrplayer.android

import android.content.Context
import com.asmrplayer.android.data.*
import com.asmrplayer.android.online.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class OnlineScreenState(
    val account: ProviderAccountState = ProviderAccountState(ProviderAccount.DEFAULT_BASE, false),
    val filters:BrowseFilters=BrowseFilters(), val query: String = "", val order: String = "popular", val ascending: Boolean = false,
    val page: OnlinePage? = null, val requestedPage: Int = 1, val loading: Boolean = false, val error: String? = null,
    val detailId: String? = null, val detailOrigin: String? = null, val detail: OnlineDetail? = null,
    val detailLoading: Boolean = false, val detailError: String? = null,
    val savingWorks:Set<String> = emptySet(), val working: Boolean = false, val accountSaving: Boolean = false, val accountError: String? = null,
    val accountNotice: String? = null, val notice: String? = null, val actionError: String? = null,
    val tagOptions:List<String> = emptyList(), val tagCounts:Map<String, Int> = emptyMap(), val tagsError:String?=null,
    val tasks: List<DownloadTask> = emptyList(), val tasksReady: Boolean = false, val tasksError: String? = null,
    val taskBusy: Set<String> = emptySet(), val assets: Map<String, RemoteAsset> = emptyMap(),
)

/** UI orchestration only. Repository owns credentials, stable media identities and download execution. */
@OptIn(ExperimentalCoroutinesApi::class)
class OnlineTools(context: Context, private val scope: CoroutineScope, private val enqueue:(List<String>,Boolean)->Unit = {_,_->}, private val feedback: ((String,String?,(() -> Unit)?) -> Unit)? = null, private val playlist:(List<String>)->Unit = {}, private val play: (List<String>, Boolean, String?) -> Unit) {
    private val backend = AndroidBackend.get(context)
    private val mutable = MutableStateFlow(OnlineScreenState(account = backend.online.account.state.value))
    val state = mutable.asStateFlow()
    private var browseJob: Job? = null
    private var detailJob: Job? = null
    private var actionJob: Job? = null
    private var tasksJob: Job? = null
    private var browseGeneration = 0L
    private var detailGeneration = 0L
    private var browsing = false
    private var filters = LibraryPreference()

    init {
        scope.launch {
            combine(backend.online.account.state, backend.library.preferences) { account, prefs -> account to prefs }
                .collect { (account, prefs) ->
                    val changed = account != mutable.value.account || prefs != filters
                    mutable.update { it.copy(account = account, tagOptions=if(it.account.apiBase==account.apiBase)it.tagOptions else emptyList(),tagCounts=if(it.account.apiBase==account.apiBase)it.tagCounts else emptyMap(),tagsError=null) }; filters = prefs
                    if (changed) {
                        ++browseGeneration; browseJob?.cancel()
                        mutable.update { it.copy(page = null, requestedPage = 1, loading = false, error = null) }
                        if (browsing) search()
                        val current = mutable.value
                        if (current.detailId != null && current.detailOrigin != null) loadDetail(current.detailOrigin, current.detailId)
                    }
                }
        }
        observeTasks()
        scope.launch {
            backend.database.library().observeAllRemoteAssets().map { rows -> rows.associateBy { it.id } }
                .flowOn(Dispatchers.Default).catch { /* Download tasks can still be displayed. */ }
                .collect { assets -> mutable.update { it.copy(assets = assets) } }
        }
    }

    enum class CardAction { Save, Remove, Like, Unlike, Next, Queue, Download, Playlist }

    fun cardAction(origin: String, id: String, action: CardAction) {
        val account = mutable.value.account
        if (origin != account.apiBase || id in mutable.value.savingWorks) return
        mutable.update { it.copy(savingWorks = it.savingWorks + id, actionError = null) }
        scope.launch {
            try {
                if (account != backend.online.account.state.value) return@launch
                val workId = com.asmrplayer.android.library.LibraryIndex.id("asmr_one", origin, id)
                when (action) {
                    CardAction.Remove -> backend.library.setMembership(workId, false)
                    CardAction.Unlike -> backend.library.setFavorite(workId, false)
                    CardAction.Save -> backend.online.importWork(id, true, reuseDetail = true)
                    CardAction.Like -> {
                        val imported = backend.online.importWork(id, false, reuseDetail = true)
                        backend.library.setFavorite(imported.workId, true)
                    }
                    CardAction.Next, CardAction.Queue, CardAction.Download, CardAction.Playlist -> {
                        val imported = backend.online.importWork(id, action == CardAction.Download, reuseDetail = true)
                        val importedIds = imported.versionIds.toHashSet()
                        val versions = backend.library.versions(imported.workId).first().filter { it.available && it.id in importedIds }
                        val first = versions.firstOrNull() ?: error("没有可用音轨")
                        val ids = versions.filter { it.groupPath == first.groupPath && it.format == first.format }.map { it.id }
                        require(ids.size <= 500)
                        if (account != backend.online.account.state.value) return@launch
                        if (action == CardAction.Download) backend.downloads.enqueue(ids) else if (action == CardAction.Playlist) playlist(com.asmrplayer.android.data.preferredPlaylistVersions(versions)) else enqueue(ids, action == CardAction.Next)
                    }
                }
                val notice = when (action) {
                    CardAction.Save -> "已加入音频库"
                    CardAction.Remove -> "已移出音频库"
                    CardAction.Like -> "已加入喜欢"
                    CardAction.Unlike -> "已取消喜欢"
                    CardAction.Download -> "已提交音轨下载"
                    else -> null // The playback controller reports queue results.
                }
                if (notice != null) feedback?.invoke(notice, null, null)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (account == mutable.value.account) mutable.update { it.copy(actionError = onlineMessage(e)) }
            } finally {
                mutable.update { it.copy(savingWorks = it.savingWorks - id) }
            }
        }
    }

    fun loadTags() {
        val account=mutable.value.account
        val suggestions=mutable.value.page?.items.orEmpty().flatMap{it.tags}.distinct()
        mutable.update{it.copy(tagOptions=(it.tagOptions+suggestions).distinct(),tagsError=null)}
        scope.launch {
            try {
                val provider=backend.online.provider
                val a=org.json.JSONArray(provider.http.text(account.apiBase+"/api/tags/",provider.account.headers(account.apiBase)))
                val rows = List(a.length()) { a.getJSONObject(it) }.sortedByDescending { it.optLong("count") }
                val names = rows.map { it.optString("name") }.filter { it.isNotBlank() }.distinct()
                val counts = rows.mapNotNull { row -> row.optInt("count", -1).takeIf { it >= 0 }?.let { row.optString("name") to it } }.toMap()
                if (account == mutable.value.account) mutable.update { it.copy(tagOptions = names, tagCounts = counts, tagsError = null) }
            } catch(e:CancellationException){throw e}catch(_:Exception){if(account==mutable.value.account)mutable.update{it.copy(tagsError="完整标签表暂不可用，先显示当前结果中的标签；也可输入名称。")}}
        }
    }
    fun browsing(active: Boolean) {
        browsing = active
        if (active && mutable.value.page == null && !mutable.value.loading) search()
        if (!active) { ++browseGeneration; browseJob?.cancel(); mutable.update { it.copy(loading = false) } }
    }
    fun search(query: String = mutable.value.query, order: String = mutable.value.order,
        ascending: Boolean = mutable.value.ascending, page: Int = 1, conditions:BrowseFilters=mutable.value.filters) {
        if (query.length > 1024 || page !in 1..100_000) return
        val generation = ++browseGeneration
        browseJob?.cancel()
        val old = mutable.value
        val identityChanged = old.query != query.trim() || old.order != order || old.ascending != ascending || old.filters != conditions
        mutable.update { it.copy(filters=conditions, query = query.trim(), order = order, ascending = ascending, requestedPage = page,
            page = if (identityChanged) null else it.page, loading = true, error = null) }
        browseJob = scope.launch {
            try {
                val result = backend.online.browse(query.trim(), page, order, ascending, conditions)
                ensureActive()
                if (generation == browseGeneration) mutable.update { it.copy(page = result) }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                val message = when {
                    e is IllegalArgumentException -> "筛选条件无效：关键词最多 200 字，条件名称不能包含 $ 或换行。"
                    order == "popular" && query.isBlank() && e is OnlineException && e.code == OnlineError.NETWORK -> "热门榜暂时无法加载，请重试。"
                    else -> onlineMessage(e)
                }
                if (generation == browseGeneration) mutable.update { it.copy(error = message) }
            }
            finally { if (generation == browseGeneration) mutable.update { it.copy(loading = false) } }
        }
    }

    fun loadDetail(origin: String, id: String, force: Boolean = false) {
        val generation = ++detailGeneration
        detailJob?.cancel(); actionJob?.cancel()
        val cached = backend.online.cachedDetail(origin, id)?.takeIf { allowed(it.work.rating) }
        mutable.update { it.copy(detailId = id, detailOrigin = origin, detail = cached, detailLoading = cached == null,
            detailError = null, working = false, actionError = null, notice = null) }
        detailJob = scope.launch {
            try {
                require(origin == mutable.value.account.apiBase) { "来源已切换" }
                val result = backend.online.detail(id, force)
                ensureActive()
                if (generation == detailGeneration && origin == mutable.value.account.apiBase) {
                    if (allowed(result.work.rating)) mutable.update { it.copy(detail = result) }
                    else mutable.update { it.copy(detail = null, detailError = "此作品被当前内容显示设置隐藏。") }
                }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { if (generation == detailGeneration) mutable.update { it.copy(detail = backend.online.cachedDetail(origin, id)?.takeIf { work -> allowed(work.work.rating) }, detailError =
                if (origin != it.account.apiBase) "来源已切换，请返回发现页重新选择作品。" else onlineMessage(e)) } }
            finally { if (generation == detailGeneration) mutable.update { it.copy(detailLoading = false) } }
        }
    }
    private fun allowed(rating: String) = (filters.includeNsfw || rating != "NSFW") && (filters.includeUnrated || rating != "UNRATED")
    fun leaveDetail() {
        ++detailGeneration; detailJob?.cancel(); actionJob?.cancel()
        mutable.update { it.copy(detailId = null, detailOrigin = null, detail = null, detailLoading = false, working = false) }
    }

    /** Import once per explicit action; resolve selected paths against the repository's stable versions. */
    fun act(action: String, paths: List<String> = emptyList(), startPath: String? = null, completed: (() -> Unit)? = null) {
        val snapshot = mutable.value
        val detail = snapshot.detail ?: return
        if (snapshot.working || snapshot.accountSaving || !allowed(detail.work.rating)) return
        val generation = detailGeneration
        mutable.update { it.copy(working = true, actionError = null, notice = null) }
        actionJob = scope.launch {
            var succeeded = false
            try {
                val existingId = com.asmrplayer.android.library.LibraryIndex.id("asmr_one",snapshot.account.apiBase,detail.work.sourceId)
                val wasInLibrary = backend.library.work(existingId)?.inLibrary == true
                val imported = backend.online.importWork(detail.work.sourceId, action in setOf("save", "download", "assets", "cover"), reuseDetail = true)
                ensureActive()
                if (generation != detailGeneration || snapshot.detailOrigin != mutable.value.account.apiBase) return@launch
                when (action) {
                    "save" -> Unit
                    "favorite" -> backend.library.setFavorite(imported.workId, true)
                    "cover" -> backend.downloads.cacheCover(imported.workId)
                    "assets" -> {
                        val ids = backend.online.assets(imported.workId).first().filter { it.available && it.relativePath in paths }.map { it.id }
                        require(ids.size == paths.distinct().size && ids.isNotEmpty())
                        backend.downloads.enqueueAssets(ids)
                    }
                    "play", "restart", "enqueue", "next", "download", "favorite-track", "playlist" -> {
                        val orderedPaths = paths.distinct()
                        require(orderedPaths.size in 1..500)
                        val ids = orderedPaths.map { imported.versionsByPath.getValue(it) }
                        when (action) {
                            "play", "restart" -> {
                                require(startPath == null || startPath in orderedPaths)
                                play(ids, action == "restart", startPath?.let(imported.versionsByPath::getValue))
                            }
                            "playlist" -> playlist(ids)
                            "favorite-track" -> ids.forEach { backend.library.collections.setVersionFavorite(it, true) }
                            "enqueue", "next" -> enqueue(ids, action == "next")
                            else -> backend.downloads.enqueue(ids)
                        }
                    }
                    else -> error("未知操作")
                }
                succeeded = true
                val notice = when (action) {
                    "playlist" -> "请选择要加入的歌单。"
                    "enqueue","next" -> "已加入播放队列。"
                    "restart","play" -> "已发送播放请求，可打开播放器查看状态。"
                    "download", "assets", "cover" -> "下载任务已提交，可在下载管理中查看状态。"
                    "favorite", "favorite-track" -> "已加入喜欢。"
                    else -> "已保存到音频库。"
                }
                if(action=="save"&&feedback!=null&&!wasInLibrary) feedback.invoke("已加入音频库","撤销") {
                    scope.launch { try {backend.library.setMembership(imported.workId,false)} catch(e:CancellationException){throw e} catch(_:Exception){mutable.update{it.copy(actionError="撤销未完成，请重试。")}} }
                } else mutable.update { it.copy(notice=notice) }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { if (generation == detailGeneration) mutable.update { it.copy(actionError = onlineMessage(e)) } }
            finally { if (generation == detailGeneration) mutable.update { it.copy(working = false) } }
            if (succeeded && generation == detailGeneration) completed?.invoke()
        }
    }

    fun dismissAccountFeedback() {
        mutable.update { it.copy(accountError = null, accountNotice = null) }
    }
    fun saveOrigin(value: String, saved: () -> Unit = {}) = accountChange {
        backend.online.account.setApiBase(value)
        saved()
    }
    fun saveToken(origin: String, value: String?, done: () -> Unit) = accountChange {
        require(origin == backend.online.account.state.value.apiBase)
        backend.online.account.setToken(value); done()
    }
    private fun accountChange(change: suspend () -> Unit) {
        if (mutable.value.accountSaving || mutable.value.working) return
        mutable.update { it.copy(accountSaving = true, accountError = null, accountNotice = null) }
        scope.launch {
            try {
                change(); mutable.update { it.copy(accountNotice = "来源设置已保存。") }
                // Replacing a token can leave hasToken unchanged; still invalidate authenticated results.
                ++browseGeneration; browseJob?.cancel()
                mutable.update { it.copy(page = null, loading = false) }
                if (browsing) search()
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.update { it.copy(accountError = if (e is IllegalArgumentException)
                "设置未保存。请检查 HTTPS 来源地址或 Token 格式；来源切换后需重新填写。" else onlineMessage(e)) } }
            finally { mutable.update { it.copy(accountSaving = false) } }
        }
    }
    fun observeTasks() {
        tasksJob?.cancel()
        mutable.update { it.copy(tasksReady = false, tasksError = null) }
        tasksJob = scope.launch {
            try {
                backend.downloads.awaitReady()
                backend.downloads.tasks.collect { tasks -> mutable.update { it.copy(tasks = tasks, tasksReady = true) } }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.update { it.copy(tasksError = onlineMessage(e), tasksReady = true) } }
        }
    }
    fun taskAction(id: String, action: String) {
        if (id in mutable.value.taskBusy) return
        mutable.update { it.copy(taskBusy = it.taskBusy + id, actionError = null, notice = null) }
        scope.launch {
            try {
                when (action) {
                    "pause" -> backend.downloads.pause(id)
                    "resume" -> backend.downloads.resume(id)
                    "remove" -> backend.downloads.remove(id)
                }
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.update { it.copy(actionError = onlineMessage(e)) } }
            finally { mutable.update { it.copy(taskBusy = it.taskBusy - id) } }
        }
    }
    fun dismiss() { mutable.update { it.copy(actionError = null, notice = null) } }
}

internal fun onlineMessage(error: Exception): String = when ((error as? OnlineException)?.code) {
    OnlineError.AUTH_REQUIRED -> "来源需要认证或权限不足，请在在线来源设置中检查 Token。"
    OnlineError.NOT_FOUND -> "来源未找到作品或文件，请刷新作品。"
    OnlineError.RATE_LIMITED -> "来源请求过于频繁，请稍后重试。"
    OnlineError.SCHEMA_CHANGED -> "来源响应格式已变化，暂时无法读取。"
    OnlineError.SOURCE_CHANGED -> "远端文件已变化，请刷新作品后重新选择。"
    OnlineError.STORAGE -> "无法保存数据，请检查可用存储空间与文件权限。"
    OnlineError.NETWORK -> "网络或后台任务请求失败，请检查连接后重试。"
    null -> if (error is IllegalArgumentException) "选择的文件已变化或超过 500 项，请刷新作品后重新选择。" else "操作未完成，请重试。"
}
