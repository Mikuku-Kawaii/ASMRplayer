package com.asmrplayer.android.playback

import android.os.Bundle
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionCommand
import androidx.media3.session.SessionResult
import com.google.common.util.concurrent.ListenableFuture

/** AN005 controller contract. Commands execute in PlaybackService, never in a UI timer. */
@UnstableApi
object LibraryPlayback {
    const val GET_STATE = "com.asmrplayer.LIBRARY_STATE"
    const val PLAY_VERSIONS = "com.asmrplayer.PLAY_VERSIONS"
    const val SEEK_POSITION = "com.asmrplayer.SEEK_POSITION"
    const val SET_AB = "com.asmrplayer.SET_AB"
    const val SET_SLEEP = "com.asmrplayer.SET_SLEEP"
    const val ADD_BOOKMARK = "com.asmrplayer.ADD_BOOKMARK"
    const val SAVE_SESSION = "com.asmrplayer.SAVE_SESSION"
    const val PREPARE_UPDATE = "com.asmrplayer.PREPARE_UPDATE"
    const val ENQUEUE = "com.asmrplayer.ENQUEUE"
    const val REMOVE_QUEUE_ITEM = "com.asmrplayer.REMOVE_QUEUE_ITEM"
    const val UNDO_QUEUE_REMOVE = "com.asmrplayer.UNDO_QUEUE_REMOVE"
    const val WIDGET_ACTION = "com.asmrplayer.WIDGET_ACTION"
    val commands = listOf(WIDGET_ACTION, REMOVE_QUEUE_ITEM, UNDO_QUEUE_REMOVE, GET_STATE, PLAY_VERSIONS, SEEK_POSITION, ENQUEUE, SET_AB, SET_SLEEP, ADD_BOOKMARK, SAVE_SESSION, PREPARE_UPDATE)

    fun play(controller: MediaController, versions: List<String>, index: Int = 0, resume: Boolean = true, startPositionMs: Long? = null): ListenableFuture<SessionResult> =
        send(controller, PLAY_VERSIONS, Bundle().apply {
            putStringArrayList("versionIds", ArrayList(versions)); putInt("index", index); putBoolean("resume", resume)
            startPositionMs?.let { putLong("positionMs", it) }
        })
    fun setAb(controller: MediaController, versionId: String, aMs: Long?, bMs: Long?): ListenableFuture<SessionResult> =
        send(controller, SET_AB, Bundle().apply {
            putString("versionId", versionId)
            if (aMs != null && bMs != null) { putLong("aMs", aMs); putLong("bMs", bMs) }
        })
    fun setSleep(controller: MediaController, deadlineEpochMs: Long? = null, endOfTrack: Boolean = false, fadeMs: Long? = null): ListenableFuture<SessionResult> =
        send(controller, SET_SLEEP, Bundle().apply {
            deadlineEpochMs?.let { putLong("deadlineMs", it) }; putBoolean("endOfTrack", endOfTrack)
            fadeMs?.let { putLong("fadeMs", it) }
        })
    fun extendSleep(controller: MediaController, minutes: Int): ListenableFuture<SessionResult> =
        send(controller, SET_SLEEP, Bundle().apply { putInt("extendMinutes", minutes) })
    fun bookmark(controller: MediaController, note: String = ""): ListenableFuture<SessionResult> =
        send(controller, ADD_BOOKMARK, Bundle().apply { putString("note", note) })
    fun state(controller: MediaController) = send(controller, GET_STATE)
    fun save(controller: MediaController) = send(controller, SAVE_SESSION)
    fun prepareUpdate(controller: MediaController) = send(controller, PREPARE_UPDATE)
    private fun send(controller: MediaController, action: String, args: Bundle = Bundle.EMPTY) =
        controller.sendCustomCommand(SessionCommand(action, Bundle.EMPTY), args)
}
