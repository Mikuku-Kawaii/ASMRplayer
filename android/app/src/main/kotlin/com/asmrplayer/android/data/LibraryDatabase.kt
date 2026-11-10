package com.asmrplayer.android.data

import android.content.Context
import androidx.room.*
import kotlinx.coroutines.flow.Flow

/** AN004/005: Android owns its schema; desktop database files are never opened here. */
@Entity(tableName = "library_roots", indices = [Index(value = ["treeUri"], unique = true)])
data class LibraryRoot(@PrimaryKey val id: String, val treeUri: String, val title: String,
    val mode: String = "AUTO", val available: Boolean = true, val lastScanMs: Long = 0)

@Entity(tableName = "works", indices = [Index("rootId")])
data class LibraryWork(@PrimaryKey val id: String, val rootId: String?, val relativePath: String,
    val title: String, val code: String? = null, val coverUri: String? = null,
    val favorite: Boolean = false, val rating: String = "UNRATED",
    @ColumnInfo(defaultValue = "1") val inLibrary: Boolean = true)

@Entity(tableName = "tracks", foreignKeys = [ForeignKey(entity = LibraryWork::class,
    parentColumns = ["id"], childColumns = ["workId"])], indices = [Index("workId")])
data class LibraryTrack(@PrimaryKey val id: String, val workId: String, val title: String, val groupPath: String)

@Entity(tableName = "track_versions", foreignKeys = [ForeignKey(entity = LibraryTrack::class,
    parentColumns = ["id"], childColumns = ["trackId"])], indices = [Index("trackId")])
data class TrackVersion(@PrimaryKey val id: String, val trackId: String, val format: String)

@Entity(tableName = "media_sources", foreignKeys = [ForeignKey(entity = TrackVersion::class,
    parentColumns = ["id"], childColumns = ["versionId"])], indices = [Index("versionId"), Index("rootId"), Index("uri")])
data class MediaSource(@PrimaryKey val id: String, val versionId: String, val rootId: String?,
    val relativePath: String, val uri: String, val size: Long = 0, val modifiedMs: Long = 0,
    val available: Boolean = true, val kind: String = "local_imported")

@Entity(tableName = "subtitles", indices = [Index("versionId"), Index("rootId")])
data class SubtitleSource(@PrimaryKey val id: String, val versionId: String, val rootId: String?,
    val relativePath: String, val uri: String, val title: String, val format: String,
    val available: Boolean = true, val manual: Boolean = false)

@Entity(tableName = "work_assets", indices = [Index("workId"), Index("rootId")])
data class WorkAsset(@PrimaryKey val id: String, val workId: String, val rootId: String,
    val relativePath: String, val uri: String, val format: String, val available: Boolean = true)

@Entity(tableName = "subtitle_preferences")
data class SubtitlePreference(@PrimaryKey val versionId: String, val originalId: String? = null,
    val translationId: String? = null, val offsetMs: Long = 0, val translationOffsetMs: Long = 0)

@Entity(tableName = "playback_positions")
data class PlaybackPosition(@PrimaryKey val versionId: String, val positionMs: Long,
    val durationMs: Long, val completed: Boolean = false, val updatedMs: Long = System.currentTimeMillis())

@Entity(tableName = "bookmarks", indices = [Index("versionId")])
data class Bookmark(@PrimaryKey val id: String, val versionId: String, val positionMs: Long,
    val note: String = "", val createdMs: Long = System.currentTimeMillis())

@Entity(tableName = "playback_session")
data class SessionRecord(@PrimaryKey val id: Int = 1, val queueJson: String, val index: Int,
    val positionMs: Long, val abVersionId: String? = null, val aMs: Long? = null, val bMs: Long? = null,
    val sleepDeadlineMs: Long? = null, val sleepEndOfTrack: Boolean = false)

@Entity(tableName = "library_preferences")
data class LibraryPreference(@PrimaryKey val id: Int = 1, val includeNsfw: Boolean = false, val includeUnrated: Boolean = true)

data class VersionDetail(val id: String, val trackId: String, val workId: String, val title: String,
    val groupPath: String, val format: String, val available: Boolean)

private const val VERSION_ROWS = "SELECT v.id, v.trackId, t.workId, t.title, t.groupPath, v.format, EXISTS(SELECT 1 FROM media_sources s WHERE s.versionId = v.id AND s.available = 1) AS available FROM track_versions v JOIN tracks t ON t.id = v.trackId"
internal const val WORK_WITH_COVER = "SELECT w.id,w.rootId,w.relativePath,w.title,w.code,COALESCE((SELECT a.uri FROM work_assets a JOIN remote_assets r ON a.id = r.id WHERE r.workId = w.id AND r.kind = 'cover' AND a.available = 1 LIMIT 1),w.coverUri) AS coverUri,w.favorite,w.rating,w.inLibrary FROM works w"

@Dao
interface LibraryDao {
    @Upsert suspend fun putRemoteAsset(value: RemoteAsset)
    @Query("SELECT * FROM remote_assets WHERE id = :id") suspend fun remoteAsset(id: String): RemoteAsset?
    @Query("SELECT * FROM remote_assets") fun observeAllRemoteAssets(): Flow<List<RemoteAsset>>
    @Query("SELECT * FROM remote_assets WHERE workId = :id ORDER BY relativePath") fun observeRemoteAssets(id: String): Flow<List<RemoteAsset>>
    @Query("UPDATE remote_assets SET available = 0 WHERE workId = :id") suspend fun invalidateRemoteAssets(id: String)
    @Upsert suspend fun putRemote(value: RemoteMedia)
    @Query("SELECT * FROM remote_media WHERE versionId = :id") suspend fun remote(id: String): RemoteMedia?
    @Query("UPDATE media_sources SET available = 0 WHERE kind = 'online_provider' AND versionId IN (SELECT v.id FROM track_versions v JOIN tracks t ON v.trackId = t.id WHERE t.workId = :workId)") suspend fun invalidateOnlineWork(workId: String)
    @Upsert suspend fun putDownload(value: DownloadTask)
    @Query("SELECT * FROM download_tasks WHERE id = :id") suspend fun download(id: String): DownloadTask?
    @Query("SELECT * FROM download_tasks ORDER BY createdMs") suspend fun downloadTasks(): List<DownloadTask>
    @Query("SELECT * FROM download_tasks WHERE state != 'REMOVED' ORDER BY createdMs DESC") fun observeDownloads(): Flow<List<DownloadTask>>
    @Query("UPDATE download_tasks SET state = :state, errorCode = :error, updatedMs = :now WHERE id = :id") suspend fun downloadState(id: String, state: String, error: String? = null, now: Long = System.currentTimeMillis())
    @Query("SELECT * FROM library_roots ORDER BY title") fun observeRoots(): Flow<List<LibraryRoot>>
    @Query("SELECT * FROM library_roots WHERE id = :id") suspend fun root(id: String): LibraryRoot?
    @Query("SELECT * FROM library_roots WHERE treeUri = :uri") suspend fun rootByUri(uri: String): LibraryRoot?
    @Upsert suspend fun putRoot(root: LibraryRoot)
    @Query("DELETE FROM library_roots WHERE id = :id") suspend fun forgetRoot(id: String)
    @Query("DELETE FROM media_sources WHERE rootId = :id") suspend fun forgetRootSources(id: String)
    @Query("DELETE FROM work_assets WHERE rootId = :id") suspend fun forgetRootAssets(id: String)
    @Query("UPDATE subtitle_preferences SET originalId = NULL WHERE originalId IN (SELECT id FROM subtitles WHERE rootId = :id)") suspend fun unlinkOriginals(id: String)
    @Query("UPDATE subtitle_preferences SET translationId = NULL WHERE translationId IN (SELECT id FROM subtitles WHERE rootId = :id)") suspend fun unlinkTranslations(id: String)
    @Query("DELETE FROM subtitles WHERE rootId = :id") suspend fun forgetRootSubtitles(id: String)
    @Query("""UPDATE works SET rootId = NULL, coverUri = NULL, inLibrary = inLibrary AND EXISTS
        (SELECT 1 FROM tracks t JOIN track_versions v ON v.trackId = t.id JOIN media_sources s ON s.versionId = v.id
         WHERE t.workId = works.id AND s.available = 1) WHERE rootId = :id""")
    suspend fun detachRootWorks(id: String)
    @Query(WORK_WITH_COVER + " ORDER BY w.title") fun observeWorks(): Flow<List<LibraryWork>>
    @Query(WORK_WITH_COVER + " WHERE w.id = :id") suspend fun work(id: String): LibraryWork?
    @Upsert suspend fun putWork(work: LibraryWork)
    @Query("UPDATE works SET inLibrary = :value WHERE id = :id") suspend fun membership(id: String, value: Boolean): Int
    @Query("SELECT * FROM bookmarks WHERE id = :id") suspend fun bookmark(id: String): Bookmark?
    @Upsert suspend fun putTrack(track: LibraryTrack)
    @Upsert suspend fun putVersion(version: TrackVersion)
    @Upsert suspend fun putSource(source: MediaSource)
    @Query("SELECT * FROM media_sources WHERE rootId = :rootId") suspend fun sourcesForRoot(rootId: String): List<MediaSource>
    @Query("SELECT * FROM media_sources WHERE versionId = :id ORDER BY available DESC, id") suspend fun sources(id: String): List<MediaSource>
    @Query("SELECT * FROM media_sources WHERE uri = :uri LIMIT 1") suspend fun sourceByUri(uri: String): MediaSource?
    @Query("SELECT * FROM media_sources WHERE id = :id") suspend fun source(id: String): MediaSource?
    @Query("SELECT * FROM media_sources WHERE rootId IS NULL AND kind = 'local_imported'") suspend fun standaloneSources(): List<MediaSource>
    @Query("SELECT * FROM subtitles WHERE rootId IS NULL") suspend fun standaloneSubtitles(): List<SubtitleSource>
    @Query("UPDATE media_sources SET available = 0 WHERE rootId = :id") suspend fun invalidateRoot(id: String)
    @Query("UPDATE subtitles SET available = 0 WHERE rootId = :id") suspend fun invalidateSubtitles(id: String)
    @Query("SELECT * FROM subtitles WHERE rootId = :id AND manual = 1") suspend fun manualSubtitles(id: String): List<SubtitleSource>
    @Query("UPDATE media_sources SET available = :available WHERE id = :id") suspend fun setSourceAvailable(id: String, available: Boolean)
    @Query("UPDATE works SET favorite = :favorite WHERE id = :id") suspend fun favorite(id: String, favorite: Boolean): Int
    @Query("UPDATE works SET rating = :rating WHERE id = :id") suspend fun rating(id: String, rating: String): Int
    @Query(VERSION_ROWS)
    fun observeAllVersions(): Flow<List<VersionDetail>>
    @Query(VERSION_ROWS + " WHERE t.workId = :workId")
    fun observeVersions(workId: String): Flow<List<VersionDetail>>
    @Query(VERSION_ROWS + " WHERE v.id = :id")
    suspend fun version(id: String): VersionDetail?
    @Query("SELECT * FROM subtitles WHERE versionId = :versionId ORDER BY title") fun observeSubtitles(versionId: String): Flow<List<SubtitleSource>>
    @Query("SELECT * FROM subtitles WHERE id = :id") suspend fun subtitle(id: String): SubtitleSource?
    @Upsert suspend fun putSubtitle(source: SubtitleSource)
    @Upsert suspend fun putAsset(asset: WorkAsset)
    @Query("SELECT * FROM work_assets WHERE id = :id") suspend fun asset(id: String): WorkAsset?
    @Query("UPDATE work_assets SET available = :available WHERE id = :id") suspend fun assetAvailable(id: String, available: Boolean)
    @Query("UPDATE subtitles SET available = :available WHERE uri = :uri") suspend fun subtitleUriAvailable(uri: String, available: Boolean)
    @Query("UPDATE subtitles SET uri = :uri, available = 1 WHERE uri = :placeholder") suspend fun relinkImportedSubtitles(placeholder: String, uri: String)
    @Query("SELECT * FROM work_assets WHERE uri = :uri LIMIT 1") suspend fun assetByUri(uri: String): WorkAsset?
    @Query("SELECT * FROM work_assets WHERE workId = :id ORDER BY relativePath") fun observeAssets(id: String): Flow<List<WorkAsset>>
    @Query("UPDATE work_assets SET available = 0 WHERE rootId = :id") suspend fun invalidateAssets(id: String)
    @Query("SELECT * FROM subtitle_preferences WHERE versionId = :id") suspend fun subtitlePreference(id: String): SubtitlePreference?
    @Query("SELECT * FROM subtitle_preferences WHERE versionId = :id") fun observeSubtitlePreference(id: String): Flow<SubtitlePreference?>
    @Upsert suspend fun putSubtitlePreference(value: SubtitlePreference)
    @Query("SELECT * FROM playback_positions WHERE versionId = :id") suspend fun position(id: String): PlaybackPosition?
    @Query("SELECT * FROM playback_positions ORDER BY updatedMs DESC") fun observePositions(): Flow<List<PlaybackPosition>>
    @Upsert suspend fun putPosition(value: PlaybackPosition)
    @Query("SELECT * FROM bookmarks WHERE versionId = :id ORDER BY positionMs, createdMs") fun observeBookmarks(id: String): Flow<List<Bookmark>>
    @Upsert suspend fun putBookmark(value: Bookmark)
    @Query("DELETE FROM bookmarks WHERE id = :id") suspend fun deleteBookmark(id: String)
    @Query("SELECT * FROM playback_session WHERE id = 1") suspend fun session(): SessionRecord?
    @Upsert suspend fun putSession(value: SessionRecord)
    @Query("SELECT * FROM library_preferences WHERE id = 1") fun observePreferences(): Flow<LibraryPreference?>
    @Upsert suspend fun putPreferences(value: LibraryPreference)
}

@Database(entities = [LibraryRoot::class, LibraryWork::class, LibraryTrack::class, TrackVersion::class,
    MediaSource::class, SubtitleSource::class, WorkAsset::class, SubtitlePreference::class, PlaybackPosition::class,
    Bookmark::class, SessionRecord::class, LibraryPreference::class, RemoteMedia::class, RemoteAsset::class, DownloadTask::class, VersionPreference::class, CatalogMetadata::class, com.asmrplayer.android.sync.SyncState::class, QueueGroup::class], version = 8, exportSchema = true)
abstract class LibraryDatabase : RoomDatabase() {
    abstract fun library(): LibraryDao
    abstract fun metadata(): CatalogMetadataDao
    abstract fun collections(): LibraryCollectionsDao
    abstract fun queueGroups(): QueueGroupDao
    companion object {
        fun open(context: Context): LibraryDatabase = Room.databaseBuilder(context.applicationContext,
            LibraryDatabase::class.java, "asmr-library.db").addMigrations(MIGRATION_1_2, MIGRATION_2_3, MIGRATION_3_4, MIGRATION_4_5, com.asmrplayer.android.sync.MIGRATION_5_6, MIGRATION_6_7, MIGRATION_7_8).build()
    }
}

val MIGRATION_6_7 = object : androidx.room.migration.Migration(6, 7) {
    override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
        db.execSQL("ALTER TABLE works ADD COLUMN inLibrary INTEGER NOT NULL DEFAULT 1")
    }
}
