package com.asmrplayer.android.sync

import android.content.Context
import com.asmrplayer.android.data.LibraryDatabase
import com.asmrplayer.android.online.OnlineRepository
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import org.json.JSONObject

/** Receive identities first, then complete source data using this device's configured source. */
internal class SyncSourceEnrichment(
    context:Context,
    private val database:LibraryDatabase,
    private val online:OnlineRepository,
) {
    private val prefs=context.getSharedPreferences("sync-source-enrichment",Context.MODE_PRIVATE)
    fun pending()=prefs.getInt("pending",0)
    fun error()=prefs.getString("error","").orEmpty()

    fun complete()=runBlocking(Dispatchers.IO) {
        val base=online.account.state.value.apiBase
        val done=prefs.getStringSet("completed:$base",emptySet()).orEmpty().toMutableSet()
        val sources=database.openHelper.readableDatabase.query(
            "SELECT workId,sourceId FROM catalog_metadata WHERE origin<>'' AND sourceId<>''"
        ).use { cursor -> buildList {
            while(cursor.moveToNext()){
                val work=cursor.getString(0)
                val source=cursor.getString(1)
                if(source !in done) add(work to source)
            }
        } }
        prefs.edit().putInt("pending",sources.size).putString("error","").commit()
        val semaphore=Semaphore(4)
        val results=sources.map { (work,source) -> async { semaphore.withPermit {
            try {
                withTimeout(45_000) { online.refreshMetadata(work,source) }
                source to ""
            } catch(e:TimeoutCancellationException) { source to "来源请求超时" }
            catch(e:CancellationException) { throw e }
            catch(e:Exception) { source to (e.message ?: "来源信息暂不可用") }
        } } }.awaitAll()
        results.filter { it.second.isEmpty() }.forEach { done.add(it.first) }
        val failures=results.filter { it.second.isNotEmpty() }
        check(prefs.edit().putStringSet("completed:$base",done).putInt("pending",failures.size)
            .putString("error",failures.firstOrNull()?.second.orEmpty()).commit())
    }
}
