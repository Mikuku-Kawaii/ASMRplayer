package com.asmrplayer.android.data

import androidx.room.*
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import kotlinx.coroutines.flow.Flow
import org.json.JSONArray

/** Refreshable provider metadata, separate from user favorites and listening records. */
@Entity(tableName="catalog_metadata")
data class CatalogMetadata(@PrimaryKey val workId:String, val circle:String="", val voicesJson:String="[]", val tagsJson:String="[]",
    val description:String="", val release:String="", val hasSubtitle:Boolean=false, val origin:String="",val sourceId:String="") {
    @delegate:Ignore private val voiceNames by lazy(LazyThreadSafetyMode.PUBLICATION) { strings(voicesJson) }
    @delegate:Ignore private val tagNames by lazy(LazyThreadSafetyMode.PUBLICATION) { strings(tagsJson) }
    fun voices() = voiceNames
    fun tags() = tagNames
    private fun strings(json:String)=runCatching {val a=JSONArray(json);List(a.length()){a.getString(it)}}.getOrDefault(emptyList())
}
@Dao
interface CatalogMetadataDao {
    @Query("SELECT w.id,w.code,w.rootId,w.inLibrary,m.origin,m.sourceId FROM works w LEFT JOIN catalog_metadata m ON m.workId=w.id")
    suspend fun accountLibraryEntries(): List<AccountLibraryEntry>
    @Query("SELECT * FROM catalog_metadata WHERE workId=:id") suspend fun forWork(id:String):CatalogMetadata?
    @Query("SELECT m.workId FROM catalog_metadata m JOIN works w ON w.id=m.workId WHERE w.rootId=:rootId AND m.sourceId<>''")
    suspend fun syncedWorksForRoot(rootId: String): List<String>
    @Upsert suspend fun put(value:CatalogMetadata)
    @Query("SELECT * FROM catalog_metadata") fun observe():Flow<List<CatalogMetadata>>
    @Query("SELECT DISTINCT t.workId FROM media_sources s JOIN track_versions v ON v.id=s.versionId JOIN tracks t ON t.id=v.trackId WHERE s.available=1 AND s.kind IN ('local_imported','local_downloaded')") fun offlineWorks():Flow<List<String>>
}
data class AccountLibraryEntry(val id: String, val code: String?, val rootId: String?, val inLibrary: Boolean,
    val origin: String?, val sourceId: String?)
val MIGRATION_4_5=object:Migration(4,5) {
    override fun migrate(db:SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS catalog_metadata (workId TEXT NOT NULL PRIMARY KEY, circle TEXT NOT NULL, voicesJson TEXT NOT NULL, tagsJson TEXT NOT NULL, description TEXT NOT NULL, release TEXT NOT NULL, hasSubtitle INTEGER NOT NULL, origin TEXT NOT NULL, sourceId TEXT NOT NULL)")
    }
}
