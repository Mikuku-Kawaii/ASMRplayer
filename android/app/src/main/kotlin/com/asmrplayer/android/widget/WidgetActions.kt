package com.asmrplayer.android.widget

import android.content.ComponentName
import android.content.Context
import android.os.Bundle
import android.widget.Toast
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import androidx.media3.session.SessionToken
import com.asmrplayer.android.playback.LibraryPlayback
import com.asmrplayer.android.playback.PlaybackService
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.*
import java.util.concurrent.Executor
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** Short-lived controller only; PlaybackService owns restore, playback and library writes. */
@UnstableApi
internal object WidgetActions {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var toast: Toast? = null
    private fun showNotice(context: Context, message: String) {
        toast?.cancel()
        val next = Toast.makeText(context, message, Toast.LENGTH_LONG)
        toast = next
        next.show()
        scope.launch {
            delay(com.asmrplayer.android.ui.NOTICE_DURATION_MS)
            if (toast === next) { next.cancel(); toast = null }
        }
    }
    fun dispatch(context: Context, operation: String?, version: String?, favorite: Boolean?, finished: () -> Unit) {
        val app = context.applicationContext
        scope.launch {
            try {
                val result = execute(app, operation, version, favorite)
                val message = result.extras.getString("widgetMessage")
                    ?: if (result.resultCode != SessionResult.RESULT_SUCCESS) "操作未完成，请重试" else null
                message?.let { showNotice(app, it) }
            } catch (_: Exception) {
                showNotice(app, "播放器尚未就绪，请重试")
            } finally { PlaybackWidget.refresh(app); finished() }
        }
    }

    internal suspend fun execute(context: Context, operation: String?, version: String?, favorite: Boolean?): SessionResult =
        withContext(Dispatchers.Main.immediate) {
            require(operation in setOf("play", "pause", "previous", "next", "favorite", "library"))
            val connection = MediaController.Builder(context,
                SessionToken(context, ComponentName(context, PlaybackService::class.java))).buildAsync()
            try {
                withTimeout(8_000) {
                    connection.await().sendCustomCommand(SessionCommand(LibraryPlayback.WIDGET_ACTION, Bundle.EMPTY), Bundle().apply {
                        putString("operation", operation); putString("versionId", version)
                        favorite?.let { putBoolean("favorite", it) }
                    }).await()
                }
            } finally { MediaController.releaseFuture(connection) }
        }

    private suspend fun <T> ListenableFuture<T>.await(): T = suspendCancellableCoroutine { continuation ->
        addListener({
            try { continuation.resume(get()) }
            catch (e: Exception) { continuation.resumeWithException(e) }
        }, Executor { it.run() })
        continuation.invokeOnCancellation { cancel(false) }
    }
}
