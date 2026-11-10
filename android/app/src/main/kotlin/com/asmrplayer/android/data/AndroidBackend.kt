package com.asmrplayer.android.data

import android.content.Context
import com.asmrplayer.android.library.LibraryRepository
import com.asmrplayer.android.subtitles.SubtitleRepository
import com.asmrplayer.android.playback.SessionPersistence
import com.asmrplayer.android.online.*
import com.asmrplayer.android.download.DownloadRepository

/** Process-scoped data services. The MediaSessionService remains the sole playback owner. */
class AndroidBackend private constructor(context: Context) {
    internal val database = LibraryDatabase.open(context)
    internal val sessions = SessionPersistence(database)
    private val transport = HttpTransport(context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE != 0)
    val online = OnlineRepository(database, AsmrOneProvider(ProviderAccount(context, transport), transport))
    val library = LibraryRepository(context, database, online::resolve)
    val queueGroups = QueueGroupRepository(database)
    val subtitles = SubtitleRepository(context, database, online.provider)
    val downloads = DownloadRepository(context, database, online.provider)
    val backups = com.asmrplayer.android.backup.BackupRepository(context, database)
    val sync by lazy { com.asmrplayer.android.sync.SyncRepository(context, database, online) }
    val updates by lazy { com.asmrplayer.android.update.UpdateRepository(context) }
    companion object {
        @Volatile private var instance: AndroidBackend? = null
        fun get(context: Context): AndroidBackend = instance ?: synchronized(this) {
            instance ?: AndroidBackend(context.applicationContext).also { instance = it }
        }
    }
}
