package com.asmrplayer.android.widget

import android.content.ComponentName
import android.net.Uri
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import com.asmrplayer.android.data.*
import com.asmrplayer.android.playback.PlaybackService
import com.asmrplayer.android.playback.LibraryPlayback
import com.asmrplayer.android.playback.SessionPersistence
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit

@UnstableApi
@RunWith(AndroidJUnit4::class)
class WidgetActionsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private suspend fun <T> main(block: () -> T): T = withContext(Dispatchers.Main) { block() }
    private fun stage(message:String) = instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "\n$message\n") })

    @Test fun coldRestoreAndControlsWorkWithoutActivityAndRejectStaleVersions() = runBlocking {
        // This integration test intentionally changes a session; never run it against the user's app.
        check(context.packageName.endsWith(".widgettest")) { "Use the isolated .widgettest build" }
        val backend = AndroidBackend.get(context)
        val dao = backend.database.library()
        val first = "widget-fixture-a"; val second = "widget-fixture-b"
        val audio = File(context.noBackupFilesDir, "offline/widget-fixture.wav").apply { parentFile!!.mkdirs() }
        val size = 48000 * 2 * 20
        val header = ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
            .put("RIFF".toByteArray()).putInt(size+36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(1).putInt(48000).putInt(96000).putShort(2).putShort(16)
            .put("data".toByteArray()).putInt(size)
        audio.outputStream().use { it.write(header.array()); it.write(ByteArray(size)) }
        for (id in listOf(first, second)) {
            dao.putWork(LibraryWork(id, null, id, "Widget synthetic fixture", inLibrary=false))
            dao.putTrack(LibraryTrack(id,id,id,"WAV")); dao.putVersion(TrackVersion(id,id,"wav"))
            dao.putSource(MediaSource(id,id,null,id,Uri.fromFile(audio).toString(),kind="local_downloaded"))
        }
        dao.putSession(SessionRecord(queueJson=SessionPersistence.encode(listOf(first,second)),index=0,positionMs=1250))
        suspend fun action(op:String,id:String=first,desired:Boolean?=null):SessionResult {
            stage("begin $op $id")
            return WidgetActions.execute(context,op,id,desired).also { stage("end $op ${it.resultCode}") }
        }
        stage("cold action")
        assertEquals(SessionResult.RESULT_SUCCESS,action("favorite",desired=true).resultCode)
        assertTrue(backend.database.collections().preference(first)!!.favorite)
        val connection = main { MediaController.Builder(context, SessionToken(context, ComponentName(context,PlaybackService::class.java))).buildAsync() }
        try {
            stage("connect observer")
            val player = connection.get(10,TimeUnit.SECONDS)
            // The short-lived action controller can already have unbound; wait for
            // the service's asynchronous restore before inspecting a new controller.
            main { LibraryPlayback.state(player) }.get(5,TimeUnit.SECONDS)
            stage("restored state")
            assertEquals(first,main{player.currentMediaItem?.mediaId})
            assertEquals(1250L,main{player.currentPosition})
            assertFalse(main{player.playWhenReady})
            stage("collections and track changes")
            repeat(2) { assertEquals(SessionResult.RESULT_SUCCESS,action("favorite",desired=true).resultCode) }
            assertTrue(backend.database.collections().preference(first)!!.favorite)
            assertEquals(SessionResult.RESULT_SUCCESS,action("favorite",desired=false).resultCode)
            assertFalse(backend.database.collections().preference(first)!!.favorite)
            repeat(2) { assertEquals(SessionResult.RESULT_SUCCESS,action("library").resultCode) }
            assertTrue(dao.work(first)!!.inLibrary)
            assertEquals(SessionResult.RESULT_SUCCESS,action("next").resultCode)
            assertEquals(second,main{player.currentMediaItem?.mediaId})
            assertNotEquals(SessionResult.RESULT_SUCCESS,action("favorite",desired=true).resultCode)
            assertNotEquals(SessionResult.RESULT_SUCCESS,action("next").resultCode)
            assertFalse(backend.database.collections().preference(first)!!.favorite)
            assertEquals(SessionResult.RESULT_SUCCESS,action("previous",second).resultCode)
            assertEquals(first,main{player.currentMediaItem?.mediaId})
            stage("play and pause")
            repeat(2) { assertEquals(SessionResult.RESULT_SUCCESS,action("play").resultCode) }
            assertTrue(main{player.playWhenReady})
            repeat(2) { assertEquals(SessionResult.RESULT_SUCCESS,action("pause").resultCode) }
            assertFalse(main{player.playWhenReady})
            assertTrue(main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).isEmpty() })
        } finally {
            main { if(connection.isDone && !connection.isCancelled) { connection.get().stop(); connection.get().clearMediaItems() }; MediaController.releaseFuture(connection) }
            audio.delete()
        }
    }
}
