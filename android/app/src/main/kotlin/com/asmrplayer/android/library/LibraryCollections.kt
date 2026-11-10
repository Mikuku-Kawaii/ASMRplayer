package com.asmrplayer.android.library

import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.*

enum class LibrarySourceFilter { ALL, LOCAL, DOWNLOADED, ONLINE }
enum class ListeningFilter { ALL, UNPLAYED, IN_PROGRESS, COMPLETED }
enum class LibrarySort { TITLE, CODE, RECENTLY_PLAYED }
data class LibraryQuery(val text: String = "", val favoritesOnly: Boolean = false,
    val source: LibrarySourceFilter = LibrarySourceFilter.ALL, val listening: ListeningFilter = ListeningFilter.ALL,
    val sort: LibrarySort = LibrarySort.TITLE, val descending: Boolean = false,
    val tags:List<String> = emptyList(), val voices:List<String> = emptyList(), val circle:String="",
    val subtitleOnly:Boolean=false, val offlineOnly:Boolean=false, val rating:String="", val favorite:Int=0)

/** Joins and user preferences are owned here, not reconstructed by individual pages. */
class LibraryCollections internal constructor(private val db: LibraryDatabase) {
    private val dao = db.collections()
    private val preferences = db.library().observePreferences().map { it ?: LibraryPreference() }
    fun catalog(query: LibraryQuery = LibraryQuery()): Flow<List<WorkSummary>> {
        val metadata = db.metadata().observe().map { list -> list.associateBy { it.workId } }
        val offline = if (query.offlineOnly) db.metadata().offlineWorks().map { it.toSet() } else flowOf(emptySet())
        return combine(dao.observeCatalog(), preferences, metadata, offline) { rows, prefs, meta, offlineIds ->
            val term = query.text.trim()
            val filtered = rows.filter { row -> visible(row.work.rating, prefs) && (if(query.favoritesOnly) row.work.favorite else row.work.inLibrary) &&
                (term.isEmpty() || matches(term, row.work.title, row.work.code, meta[row.work.id]?.circle, meta[row.work.id]?.voices()?.joinToString(" "))) &&
                (query.favorite==0 || row.work.favorite==(query.favorite==1)) &&
                (query.rating.isEmpty()||row.work.rating==query.rating) &&
                (!query.offlineOnly || row.work.id in offlineIds) &&
                (!query.subtitleOnly || row.hasSubtitle) &&
                query.tags.all {tag->meta[row.work.id]?.tags()?.any{it.equals(tag,true)}==true} &&
                query.voices.all {voice->meta[row.work.id]?.voices()?.any{it.equals(voice,true)}==true} &&
                (query.circle.isBlank()||meta[row.work.id]?.circle?.contains(query.circle,true)==true) &&
                when (query.source) {
                    LibrarySourceFilter.ALL -> true
                    LibrarySourceFilter.LOCAL -> row.importedCount > 0
                    LibrarySourceFilter.DOWNLOADED -> row.downloadedCount > 0
                    LibrarySourceFilter.ONLINE -> row.onlineCount > 0
                } && when (query.listening) {
                    ListeningFilter.ALL -> true
                    ListeningFilter.UNPLAYED -> row.playedCount == 0
                    ListeningFilter.IN_PROGRESS -> row.playedCount > 0 && row.completedCount < row.versionCount
                    ListeningFilter.COMPLETED -> row.versionCount > 0 && row.completedCount == row.versionCount
                }
            }
            val order = when (query.sort) {
                LibrarySort.TITLE -> compareBy<WorkSummary, String>(NaturalOrder) { it.work.title }
                LibrarySort.CODE -> compareBy<WorkSummary, String>(NaturalOrder) { it.work.code.orEmpty() }
                LibrarySort.RECENTLY_PLAYED -> compareBy<WorkSummary> { it.lastListenedMs ?: Long.MIN_VALUE }
            }.let { comparator -> if (query.descending) Comparator { a, b -> comparator.compare(b, a) } else comparator }
                .thenBy(NaturalOrder) { it.work.title }.thenBy { it.work.id }
            filtered.sortedWith(order)
        }.flowOn(Dispatchers.Default)
    }

    fun favoriteVersions(text: String = ""): Flow<List<VersionCollectionEntry>> = visibleVersions(dao.observeFavorites(), text)
    /** One latest position per stable version, not a fabricated count of listening sessions. */
    fun history(text: String = ""): Flow<List<VersionCollectionEntry>> = visibleVersions(dao.observeHistory(), text)
    fun bookmarks(text: String = ""): Flow<List<BookmarkCollectionEntry>> =
        combine(dao.observeBookmarks(), preferences) { rows, prefs -> rows.filter {
            visible(it.rating, prefs) && matches(text.trim(), it.title, it.workTitle, it.workCode, it.bookmark.note)
        } }.flowOn(Dispatchers.Default)

    suspend fun setVersionFavorite(versionId: String, favorite: Boolean) = db.withTransaction {
        requireNotNull(db.library().version(versionId)) { "音轨版本不存在" }
        if (dao.preference(versionId)?.favorite != favorite) dao.putPreference(VersionPreference(versionId, favorite))
    }
    private fun visibleVersions(source: Flow<List<VersionCollectionEntry>>, text: String) =
        combine(source, preferences) { rows, prefs -> rows.filter {
            visible(it.rating, prefs) && matches(text.trim(), it.title, it.workTitle, it.workCode, it.groupPath)
        } }.flowOn(Dispatchers.Default)
    private fun visible(rating: String, preferences: LibraryPreference) = when (rating) {
        "NSFW" -> preferences.includeNsfw
        "UNRATED" -> preferences.includeUnrated
        else -> true
    }
    private fun matches(term: String, vararg fields: String?) = term.isEmpty() || fields.any { it?.contains(term, ignoreCase = true) == true }
}
