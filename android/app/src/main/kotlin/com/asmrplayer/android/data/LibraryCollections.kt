package com.asmrplayer.android.data

import androidx.room.*
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import kotlinx.coroutines.flow.Flow

/** Explicit false rows preserve an un-favorite when merging portable user data. */
@Entity(tableName = "version_preferences", foreignKeys = [ForeignKey(entity = TrackVersion::class,
    parentColumns = ["id"], childColumns = ["versionId"])])
data class VersionPreference(@PrimaryKey val versionId: String, val favorite: Boolean,
    val updatedMs: Long = System.currentTimeMillis())

/** Only stable identity and action state are exposed to the desktop widget. */
data class WidgetCollectionState(val versionId: String, val workId: String, val favorite: Boolean, val inLibrary: Boolean)

data class WorkSummary(@Embedded val work: LibraryWork, val versionCount: Int,
    val playedCount: Int, val completedCount: Int, val importedCount: Int,
    val downloadedCount: Int, val onlineCount: Int, val lastListenedMs: Long?, val hasSubtitle: Boolean)

data class VersionCollectionEntry(val versionId: String, val workId: String, val workTitle: String,
    val workCode: String?, val rating: String, val title: String, val groupPath: String, val format: String,
    val available: Boolean, val imported: Boolean, val downloaded: Boolean, val online: Boolean,
    val favorite: Boolean, val favoriteUpdatedMs: Long?, val positionMs: Long, val durationMs: Long,
    val completed: Boolean, val lastListenedMs: Long?)

data class BookmarkCollectionEntry(@Embedded val bookmark: Bookmark, val workId: String,
    val workTitle: String, val workCode: String?, val rating: String, val title: String,
    val groupPath: String, val format: String, val available: Boolean)

private const val VERSION_ENTRIES = """SELECT v.id AS versionId,t.workId,w.title AS workTitle,w.code AS workCode,w.rating,
    t.title,t.groupPath,v.format,
    EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.available=1) AS available,
    EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.kind='local_imported') AS imported,
    EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.kind='local_downloaded' AND s.available=1) AS downloaded,
    EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.kind='online_provider' AND s.available=1) AS online,
    COALESCE(f.favorite,0) AS favorite,f.updatedMs AS favoriteUpdatedMs,
    COALESCE(p.positionMs,0) AS positionMs,COALESCE(p.durationMs,0) AS durationMs,
    COALESCE(p.completed,0) AS completed,p.updatedMs AS lastListenedMs
    FROM track_versions v JOIN tracks t ON t.id=v.trackId JOIN works w ON w.id=t.workId
    LEFT JOIN version_preferences f ON f.versionId=v.id LEFT JOIN playback_positions p ON p.versionId=v.id"""

@Dao
interface LibraryCollectionsDao {
    @Query("""SELECT v.id AS versionId,t.workId,COALESCE(f.favorite,0) AS favorite,w.inLibrary
        FROM track_versions v JOIN tracks t ON t.id=v.trackId JOIN works w ON w.id=t.workId
        LEFT JOIN version_preferences f ON f.versionId=v.id WHERE v.id=:versionId""")
    fun observeWidgetCollection(versionId: String): Flow<WidgetCollectionState?>
    @Upsert suspend fun putPreference(value: VersionPreference)
    @Query("SELECT * FROM version_preferences WHERE versionId=:id") suspend fun preference(id: String): VersionPreference?
    @Query("SELECT w.*,COALESCE(c.versionCount,0) AS versionCount,COALESCE(c.playedCount,0) AS playedCount," +
        "COALESCE(c.completedCount,0) AS completedCount,COALESCE(c.importedCount,0) AS importedCount," +
        "COALESCE(c.downloadedCount,0) AS downloadedCount,COALESCE(c.onlineCount,0) AS onlineCount,c.lastListenedMs, " +
        "(COALESCE(m.hasSubtitle,0) OR sub.workId IS NOT NULL) AS hasSubtitle " +
        "FROM (" + WORK_WITH_COVER + ") w LEFT JOIN catalog_metadata m ON m.workId=w.id " +
        "LEFT JOIN (SELECT DISTINCT t.workId FROM subtitles s JOIN track_versions v ON v.id=s.versionId " +
        "JOIN tracks t ON t.id=v.trackId WHERE s.available=1) sub ON sub.workId=w.id " +
        "LEFT JOIN (SELECT workId,COUNT(*) AS versionCount," +
        "SUM(CASE WHEN lastListenedMs IS NULL THEN 0 ELSE 1 END) AS playedCount,SUM(completed) AS completedCount," +
        "SUM(imported) AS importedCount,SUM(downloaded) AS downloadedCount,SUM(online) AS onlineCount,MAX(lastListenedMs) AS lastListenedMs " +
        "FROM (" + VERSION_ENTRIES + ") GROUP BY workId) c ON c.workId=w.id")
    fun observeCatalog(): Flow<List<WorkSummary>>
    @Query(VERSION_ENTRIES + " WHERE f.favorite=1 ORDER BY f.updatedMs DESC,v.id")
    fun observeFavorites(): Flow<List<VersionCollectionEntry>>
    @Query(VERSION_ENTRIES + " WHERE p.versionId IS NOT NULL ORDER BY p.updatedMs DESC,v.id")
    fun observeHistory(): Flow<List<VersionCollectionEntry>>
    @Query("""SELECT b.*,t.workId,w.title AS workTitle,w.code AS workCode,w.rating,t.title,t.groupPath,v.format,
        EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId=v.id AND s.available=1) AS available
        FROM bookmarks b JOIN track_versions v ON v.id=b.versionId JOIN tracks t ON t.id=v.trackId
        JOIN works w ON w.id=t.workId ORDER BY b.createdMs DESC,b.id""")
    fun observeBookmarks(): Flow<List<BookmarkCollectionEntry>>
}

val MIGRATION_3_4 = object : Migration(3, 4) {
    override fun migrate(db: SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS `version_preferences` (`versionId` TEXT NOT NULL, `favorite` INTEGER NOT NULL, `updatedMs` INTEGER NOT NULL, PRIMARY KEY(`versionId`), FOREIGN KEY(`versionId`) REFERENCES `track_versions`(`id`) ON UPDATE NO ACTION ON DELETE NO ACTION)")
    }
}
