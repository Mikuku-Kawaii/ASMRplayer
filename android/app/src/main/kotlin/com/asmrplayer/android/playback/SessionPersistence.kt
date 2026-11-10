package com.asmrplayer.android.playback

import androidx.room.withTransaction
import com.asmrplayer.android.data.*
import org.json.JSONArray
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Process-wide ordered writer. Only stable version IDs are persisted. */
internal class SessionPersistence(private val database: LibraryDatabase) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val operations = Channel<suspend () -> Unit>(Channel.UNLIMITED)
    private val mutableWarning = MutableStateFlow<String?>(null)
    val warning = mutableWarning.asStateFlow()
    init { scope.launch { for (operation in operations) operation() } }
    suspend fun read(): SessionRecord? {
        val result = CompletableDeferred<SessionRecord?>()
        operations.send { try { result.complete(database.library().session()) } catch (e: Exception) { result.completeExceptionally(e) } }
        return result.await()
    }
    /** Queue on the service looper BEFORE returning; a recreated service's read is ordered after this write. */
    fun enqueue(session: SessionRecord, position: PlaybackPosition?, ack: CompletableDeferred<Unit>? = null) {
        check(operations.trySend {
            try {
                database.withTransaction {
                    database.library().putSession(session)
                    if (position != null) {
                        val old = database.library().position(position.versionId)
                        database.library().putPosition(if (position.durationMs == 0L && old != null) position.copy(durationMs = old.durationMs) else position)
                    }
                }
                mutableWarning.value = null
                ack?.complete(Unit)
            } catch (e: Exception) {
                mutableWarning.value = "会话未能保存，请检查存储空间后重试"
                ack?.completeExceptionally(e)
            }
        }.isSuccess)
    }
    companion object {
        fun encode(ids: List<String>) = JSONArray(ids).toString()
        fun decode(text: String): List<String> {
            require(text.length <= 64 * 1024)
            val array = JSONArray(text); require(array.length() <= 500)
            return List(array.length()) { array.getString(it) }
        }
    }
}
