package com.asmrplayer.android

import android.app.Application
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.lifecycle.ViewModelStore
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.playback.*
import com.asmrplayer.core.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.TimeUnit

/** Dedicated package and silent synthetic media; no ActivityScenario cross-package helper. */
@UnstableApi
@RunWith(AndroidJUnit4::class)
class PlayerControlsR25Test {
    private val instrumentation=InstrumentationRegistry.getInstrumentation()
    private val context=instrumentation.targetContext
    private fun <T> main(block:()->T):T {var value:T?=null;instrumentation.runOnMainSync{value=block()};@Suppress("UNCHECKED_CAST")return value as T}
    private suspend fun await(check:()->Boolean) {withTimeout(15000){while(!check())delay(60)}}
    private fun command(p:MediaController,action:String,args:Bundle=Bundle.EMPTY)=main{p.sendCustomCommand(SessionCommand(action,Bundle.EMPTY),args)}.get(5,TimeUnit.SECONDS)

    @Test fun membershipShuffleToneAndSavedProfileUseRealService()=runBlocking {
        check(context.packageName.endsWith(".uitest"))
        context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        val id="r25-${System.nanoTime()}"
        val file=File(context.cacheDir,"$id.wav")
        val bytes=48000*30*4
        val header=ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN).apply {
            put("RIFF".toByteArray());putInt(bytes+36);put("WAVEfmt ".toByteArray());putInt(16)
            putShort(1);putShort(2);putInt(48000);putInt(192000);putShort(4);putShort(16);put("data".toByteArray());putInt(bytes)
        }
        file.outputStream().use {out->out.write(header.array());repeat(30){out.write(ByteArray(192000))}}
        val dao=AndroidBackend.get(context).database.library()
        dao.putWork(LibraryWork(id,null,"","Synthetic controls",rating="SAFE"))
        val ids=(1..5).map{"$id-$it"}
        ids.forEach { version ->
            dao.putTrack(LibraryTrack(version,id,"Synthetic track","WAV"));dao.putVersion(TrackVersion(version,version,"wav"))
            dao.putSource(MediaSource(version,version,null,"fixture.wav",Uri.fromFile(file).toString()))
        }
        val store=ViewModelStore()
        val model=main{PlayerViewModel(context.applicationContext as Application).also{store.put("r25",it)}}
        val future=main{MediaController.Builder(context,SessionToken(context,ComponentName(context,PlaybackService::class.java))).buildAsync()}
        val p=future.get(10,TimeUnit.SECONDS)
        try {
            await{model.state.value.connected&&model.state.value.libraryReady}
            main{LibraryPlayback.play(p,ids,resume=false)}.get(10,TimeUnit.SECONDS)
            await{main{p.isPlaying&&p.currentPosition>100}}
            main{p.pause()};await{!model.state.value.playRequested}
            val position=main{p.currentPosition};val current=main{p.currentMediaItem!!.mediaId}
            main{model.setLibraryMembership(id,false)}
            await{model.state.value.library.any{it.id==id&&!it.inLibrary}}
            assertTrue(file.exists());assertEquals(ids,main{model.state.value.queue.map{it.id}})
            assertEquals(current,main{p.currentMediaItem!!.mediaId});assertFalse(main{p.playWhenReady})
            main{model.setLibraryMembership(id,true)};await{model.state.value.library.any{it.id==id&&it.inLibrary}}
            main{model.playbackOrder(PlaybackOrder.SHUFFLE_LOOP)}
            await{main{p.shuffleModeEnabled&&p.repeatMode==2}&&model.state.value.canNext}
            assertTrue(kotlin.math.abs(position-main{p.currentPosition})<100)
            val visited=mutableSetOf<String>()
            repeat(5){
                val previous=main{p.currentMediaItem!!.mediaId};visited+=previous
                main{model.next()};await{main{p.currentMediaItem!!.mediaId!=previous}}
            }
            assertEquals(ids.toSet(),visited);assertEquals(current,main{p.currentMediaItem!!.mediaId})
            main{model.playbackOrder(PlaybackOrder.SINGLE_LOOP)};await{main{!p.shuffleModeEnabled&&p.repeatMode==1}}
            main{model.playbackOrder(PlaybackOrder.SEQUENTIAL)};await{main{!p.shuffleModeEnabled&&p.repeatMode==0}}
            main{p.seekTo(0,500);p.play()};await{main{p.isPlaying&&p.currentPosition>=500}}
            val discontinuities=java.util.concurrent.atomic.AtomicInteger()
            val listener=object:Player.Listener {override fun onPositionDiscontinuity(oldPosition:Player.PositionInfo,newPosition:Player.PositionInfo,reason:Int){discontinuities.incrementAndGet()}}
            main{p.addListener(listener)}
            val before=main{p.currentPosition}
            val base=DspConfig(preampDb=-2.0,bands=listOf(PeqBand(250.0,-2.0)),balanceDb=1.0,leveler=true,ceilingDb=-6.0)
            for(preset in TonePreset.entries) {
                val next=preset.applyTo(base)
                val result=command(p,DspSettings.SET_DSP,Bundle().apply{putString(DspSettings.JSON,DspSettings.encode(next))})
                assertEquals(SessionResult.RESULT_SUCCESS,result.resultCode)
                assertEquals(next,DspSettings.decode(command(p,DspSettings.GET_DSP).extras.getString(DspSettings.JSON)!!))
            }
            assertEquals(0,discontinuities.get());assertTrue(main{p.isPlaying&&p.currentPosition>=before-100})
            assertTrue(command(p,DspSettings.GET_DSP).extras.getLong("processedFrames")>0)
            main{p.removeListener(listener)}
            val warm=TonePreset.WARM.applyTo(base)
            command(p,DspSettings.SET_DSP,Bundle().apply{putString(DspSettings.JSON,DspSettings.encode(warm))})
            val saved=command(p,HeadphoneCommands.EDIT,Bundle().apply{putString("operation","save");putString("name","R25 synthetic")})
            val profile=HeadphoneCommands.decode(saved.extras.getString("headphones")!!).activeId!!
            command(p,DspSettings.SET_DSP,Bundle().apply{putString(DspSettings.JSON,DspSettings.encode(DspConfig()))})
            val restored=command(p,HeadphoneCommands.EDIT,Bundle().apply{putString("operation","select");putString("id",profile)})
            assertEquals(warm,DspSettings.decode(restored.extras.getString(DspSettings.JSON)!!))
        } finally {
            main{p.stop();p.clearMediaItems();store.clear();MediaController.releaseFuture(future)}
            file.delete()
        }
    }
}
