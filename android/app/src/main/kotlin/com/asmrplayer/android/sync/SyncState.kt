package com.asmrplayer.android.sync

import androidx.room.Entity
import androidx.room.PrimaryKey
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

@Entity(tableName = "device_sync_state")
data class SyncState(@PrimaryKey val id: Int = 1, val json: String)

val MIGRATION_5_6 = object : Migration(5, 6) {
    override fun migrate(db: SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS `device_sync_state` (`id` INTEGER NOT NULL, `json` TEXT NOT NULL, PRIMARY KEY(`id`))")
    }
}
