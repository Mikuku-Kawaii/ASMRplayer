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
    @Query("SELECT * FROM catalog_metadata WHERE workId=:id") suspend fun forWork(id:String):CatalogMetadata?
    @Upsert suspend fun put(value:CatalogMetadata)
    @Query("SELECT * FROM catalog_metadata") fun observe():Flow<List<CatalogMetadata>>
    @Query("SELECT DISTINCT t.workId FROM media_sources s JOIN track_versions v ON v.id=s.versionId JOIN tracks t ON t.id=v.trackId WHERE s.available=1 AND s.kind IN ('local_imported','local_downloaded')") fun offlineWorks():Flow<List<String>>
}
val MIGRATION_4_5=object:Migration(4,5) {
    override fun migrate(db:SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS catalog_metadata (workId TEXT NOT NULL PRIMARY KEY, circle TEXT NOT NULL, voicesJson TEXT NOT NULL, tagsJson TEXT NOT NULL, description TEXT NOT NULL, release TEXT NOT NULL, hasSubtitle INTEGER NOT NULL, origin TEXT NOT NULL, sourceId TEXT NOT NULL)")
    }
}
