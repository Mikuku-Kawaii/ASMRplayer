package com.asmrplayer.android.data

import androidx.room.Entity
import androidx.room.Index
import androidx.room.PrimaryKey
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

@Entity(tableName = "remote_media")
data class RemoteMedia(@PrimaryKey val versionId: String, val origin: String, val sourceWorkId: String,
    val remoteFileId: String, val relativePath: String, val byteSize: Long?, val format: String)

@Entity(tableName = "remote_assets", indices = [Index("workId")])
data class RemoteAsset(@PrimaryKey val id: String, val workId: String, val origin: String, val sourceWorkId: String,
    val remoteFileId: String, val relativePath: String, val kind: String, val format: String,
    val byteSize: Long?, val available: Boolean = true)

@Entity(tableName = "download_tasks", indices = [Index(value = ["versionId"], unique = true), Index(value = ["assetId"], unique = true)])
data class DownloadTask(@PrimaryKey val id: String, val versionId: String?, val state: String = "QUEUED",
    val downloadedBytes: Long = 0, val totalBytes: Long? = null, val etag: String? = null, val lastModified: String? = null,
    val sha256: String? = null, val errorCode: String? = null, val attempts: Int = 0,
    val createdMs: Long = System.currentTimeMillis(), val updatedMs: Long = System.currentTimeMillis(), val assetId: String? = null)

/** AN006: additive migration; favorites, positions, bookmarks and session rows stay untouched. */
val MIGRATION_1_2 = object : Migration(1, 2) {
    override fun migrate(db: SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS `remote_media` (`versionId` TEXT NOT NULL, `origin` TEXT NOT NULL, `sourceWorkId` TEXT NOT NULL, `remoteFileId` TEXT NOT NULL, `relativePath` TEXT NOT NULL, `byteSize` INTEGER, `format` TEXT NOT NULL, PRIMARY KEY(`versionId`))")
        db.execSQL("CREATE TABLE IF NOT EXISTS `download_tasks` (`id` TEXT NOT NULL, `versionId` TEXT NOT NULL, `state` TEXT NOT NULL, `downloadedBytes` INTEGER NOT NULL, `totalBytes` INTEGER, `etag` TEXT, `lastModified` TEXT, `sha256` TEXT, `errorCode` TEXT, `attempts` INTEGER NOT NULL, `createdMs` INTEGER NOT NULL, `updatedMs` INTEGER NOT NULL, PRIMARY KEY(`id`))")
        db.execSQL("CREATE UNIQUE INDEX IF NOT EXISTS `index_download_tasks_versionId` ON `download_tasks` (`versionId`)")
    }
}

/** AN006 attachments use their own identity, never a fabricated audio version. */
val MIGRATION_2_3 = object : Migration(2, 3) {
    override fun migrate(db: SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS `remote_assets` (`id` TEXT NOT NULL, `workId` TEXT NOT NULL, `origin` TEXT NOT NULL, `sourceWorkId` TEXT NOT NULL, `remoteFileId` TEXT NOT NULL, `relativePath` TEXT NOT NULL, `kind` TEXT NOT NULL, `format` TEXT NOT NULL, `byteSize` INTEGER, `available` INTEGER NOT NULL, PRIMARY KEY(`id`))")
        db.execSQL("CREATE INDEX IF NOT EXISTS `index_remote_assets_workId` ON `remote_assets` (`workId`)")
        db.execSQL("CREATE TABLE `download_tasks_new` (`id` TEXT NOT NULL, `versionId` TEXT, `state` TEXT NOT NULL, `downloadedBytes` INTEGER NOT NULL, `totalBytes` INTEGER, `etag` TEXT, `lastModified` TEXT, `sha256` TEXT, `errorCode` TEXT, `attempts` INTEGER NOT NULL, `createdMs` INTEGER NOT NULL, `updatedMs` INTEGER NOT NULL, `assetId` TEXT, PRIMARY KEY(`id`))")
        db.execSQL("INSERT INTO download_tasks_new SELECT id,versionId,state,downloadedBytes,totalBytes,etag,lastModified,sha256,errorCode,attempts,createdMs,updatedMs,NULL FROM download_tasks")
        db.execSQL("DROP TABLE download_tasks")
        db.execSQL("ALTER TABLE download_tasks_new RENAME TO download_tasks")
        db.execSQL("CREATE UNIQUE INDEX `index_download_tasks_versionId` ON `download_tasks` (`versionId`)")
        db.execSQL("CREATE UNIQUE INDEX `index_download_tasks_assetId` ON `download_tasks` (`assetId`)")
    }
}
