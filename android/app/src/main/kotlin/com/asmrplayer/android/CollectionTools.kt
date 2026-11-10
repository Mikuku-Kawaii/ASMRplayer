package com.asmrplayer.android

import android.content.Context
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class CollectionScreenState(
    val query: LibraryQuery = LibraryQuery(), val works: List<WorkSummary> = emptyList(),
    val favorites: List<VersionCollectionEntry> = emptyList(), val history: List<VersionCollectionEntry> = emptyList(),
    val bookmarks: List<BookmarkCollectionEntry> = emptyList(), val ready: Boolean = false,
    val refreshing: Boolean = false, val busyIds: Set<String> = emptySet(), val error: String? = null,
)

@OptIn(ExperimentalCoroutinesApi::class)
class CollectionTools(context: Context, private val scope: CoroutineScope, private val feedback: (String, String?, (() -> Unit)?) -> Unit = { _, _, _ -> }) {
    private val library = AndroidBackend.get(context).library
    fun previewCount(query: LibraryQuery) = library.collections.catalog(query).map { it.size }
    private val mutable = MutableStateFlow(CollectionScreenState())
    val state = mutable.asStateFlow()
    private val query = MutableStateFlow(LibraryQuery())
    // Both catalogue pages can be visible during a drag. Each owns its filters and last result.
    private val favoriteRequest = MutableStateFlow(LibraryQuery(favoritesOnly = true) to 0)
    private val favoriteReload = MutableStateFlow(false)
    val refreshingFavorites = favoriteReload.asStateFlow()
    val favoriteCatalog = favoriteRequest.flatMapLatest { (requested, _) ->
        library.collections.catalog(requested).map { works ->
            CollectionScreenState(query = requested, works = works, ready = true)
        }.catch { emit(CollectionScreenState(query = requested, error = "资料列表读取失败，请重试。")) }
    }.onEach { if (it.query == favoriteRequest.value.first) favoriteReload.value = false }.stateIn(scope, SharingStarted.WhileSubscribed(5_000), CollectionScreenState())

    fun queryFavoriteWorks(requested: LibraryQuery) {
        if (favoriteRequest.value.first != requested) {
            favoriteReload.value = true
            favoriteRequest.update { requested to it.second }
        }
    }
    fun retryFavoriteWorks() { favoriteReload.value = true; favoriteRequest.update { it.first to it.second + 1 } }

    private var reader: Job? = null
    init { retry() }
    fun query(value: LibraryQuery) {
        if (query.value == value) return
        mutable.update { it.copy(query = value, works = emptyList(), ready = false) }
        query.value = value
    }
    fun retry() {
        reader?.cancel()
        mutable.update { it.copy(error = null, refreshing = true) }
        reader = scope.launch {
            try {
                combine(query.flatMapLatest { requested -> library.collections.catalog(requested).map { requested to it } }, library.collections.favoriteVersions(),
                    library.collections.history(), library.collections.bookmarks()) { catalog, favorites, history, bookmarks ->
                    CollectionScreenState(query = catalog.first, works = catalog.second, favorites = favorites, history = history, bookmarks = bookmarks)
                }.collect { result -> mutable.update { it.copy(works = if (result.query == it.query) result.works else emptyList(), favorites = result.favorites,
                    history = result.history, bookmarks = result.bookmarks, ready = result.query == it.query, refreshing = false) } }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { mutable.update { it.copy(error = "资料列表读取失败，请重试。", refreshing = false) } }
        }
    }
    fun favorite(id: String, value: Boolean) = change(id) {
        library.collections.setVersionFavorite(id, value)
        feedback(if(value) "已添加书签" else "已取消书签", "撤销") { change(id) { library.collections.setVersionFavorite(id, !value) } }
    }
    fun removeBookmark(id: String) = change(id) {
        val old = library.bookmark(id) ?: return@change
        library.removeBookmark(id)
        feedback("标记已删除", "撤销") { change(id) { library.restoreBookmark(old) } }
    }
    fun editBookmark(id: String, note: String) = change(id) {
        val old = library.bookmark(id) ?: return@change
        library.restoreBookmark(old.copy(note = note.take(4096)))
    }
    private fun change(id: String, block: suspend () -> Unit) {
        if (id in mutable.value.busyIds) return
        mutable.update { it.copy(busyIds = it.busyIds + id, error = null) }
        scope.launch {
            try { block() }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { mutable.update { it.copy(error = "修改未保存，请重试。") } }
            finally { mutable.update { it.copy(busyIds = it.busyIds - id) } }
        }
    }
}
