package com.asmrplayer.android

import android.graphics.Bitmap
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModelProvider
import androidx.media3.common.util.UnstableApi
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import com.asmrplayer.android.sync.*
import com.asmrplayer.android.ui.*
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File

@UnstableApi
class SyncHandoffTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun persistedOfferOnlyPlaysOnClickAndStaleOffersAreRejected():Unit = runBlocking {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend=AndroidBackend.get(context)
        val db=backend.database
        val wav=File(context.filesDir,"handoff-silence.wav")
        val bytes=48000*120*2
        val header=java.nio.ByteBuffer.allocate(44).order(java.nio.ByteOrder.LITTLE_ENDIAN)
        header.put("RIFF".toByteArray()).putInt(bytes+36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(1).putInt(48000).putInt(96000).putShort(2).putShort(16).put("data".toByteArray()).putInt(bytes)
        wav.outputStream().use {out->out.write(header.array());repeat(120){out.write(ByteArray(96000))}}
        db.library().putWork(LibraryWork("handoff-work",null,"","雨夜来信 · 合成演示作品","DEMO",null,false,"SAFE"))
        db.library().putTrack(LibraryTrack("handoff-track","handoff-work","今晚也辛苦了.wav","WAV"))
        db.library().putVersion(TrackVersion("visual-v1","handoff-track","wav"))
        db.library().putSource(MediaSource("handoff-source","visual-v1",null,"今晚也辛苦了.wav",android.net.Uri.fromFile(wav).toString(),wav.length()))
        val provider=SyncScheduler.selected(context)
        val time=System.currentTimeMillis()
        val offer=JSONObject().put("id","synthetic-desktop:1").put("versionId","visual-v1")
            .put("title","雨夜来信 · 今晚也辛苦了").put("positionMs",42000L).put("updatedMs",time).put("deviceName","Windows 电脑")
        fun saveOffer() {
            db.openHelper.writableDatabase.execSQL("INSERT OR REPLACE INTO device_sync_state(id,json) VALUES(1,?)",arrayOf(
                JSONObject().put("device","synthetic-phone").put("doc",SyncProtocol.wire(SyncProtocol.empty()))
                    .put("baseline",JSONObject()).put("resume_${provider.name}",offer).toString()))
        }
        db.library().putPosition(PlaybackPosition("visual-v1",42000,120000,updatedMs=time))
        saveOffer()
        val reopened=SyncRepository(context,db,backend.online)
        assertEquals(42000L,reopened.resumeCandidate(provider,offer.getString("id"))!!.getLong("positionMs"))
        assertTrue(runCatching {reopened.resumeCandidate(provider,"old-offer")}.isFailure)
        db.library().putPosition(PlaybackPosition("visual-v1",43000,120000,updatedMs=time+1))
        assertNull(reopened.resumeCandidate(provider,null))
        db.library().putPosition(PlaybackPosition("visual-v1",42000,120000,updatedMs=time))

        val summary=JSONArray()
        listOf("音频库作品","喜欢作品","收藏音轨","收听进度","时间标记","歌单").forEachIndexed {i,label ->
            summary.put(JSONObject().put("kind",i.toString()).put("label",label).put("total",i+2).put("received",1).put("pending",0))
        }
        val status=SyncStatus(provider,true,true,27,0,emptyList(),time,AutoSyncPolicy(),"",
            details=JSONObject().put("summary",summary).put("pendingItems",JSONArray()),resume=offer)
        lateinit var model:PlayerViewModel
        ui.runOnUiThread {
            model=ViewModelProvider(ui.activity)[PlayerViewModel::class.java]
            ui.activity.setContent {
                MaterialTheme(colorScheme=playerColors(true),typography=PlayerTypography) {
                    Surface(Modifier.fillMaxSize()) { Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                        Text("跨设备同步",style=MaterialTheme.typography.headlineSmall)
                        SyncResumeCard(status,model)
                        SyncResultDetails(status)
                    } }
                }
            }
        }
        ui.waitForIdle()
        assertFalse(model.screenState.value.playing)
        capture("安卓接续与同步明细")
        ui.onNodeWithText("继续收听").performClick()
        ui.waitUntil(10000) {model.screenState.value.playing && model.screenState.value.queue.getOrNull(model.screenState.value.index)?.id=="visual-v1"}
        assertTrue(model.positionMs.value in 41000..55000)
        ui.waitUntil(5000) {reopened.resumeCandidate(provider,null)==null}
        ui.runOnUiThread {model.togglePlayback()}
        ui.runOnUiThread { ui.activity.setContent {
            val state by model.screenState.collectAsState()
            MaterialTheme(colorScheme=playerColors(true),typography=PlayerTypography) {
                Surface(Modifier.fillMaxSize()) {Column { Text("任务中心",Modifier.padding(16.dp),style=MaterialTheme.typography.headlineSmall)
                    TaskCenterPage(state,model,{}) }
                }
            }
        } }
        ui.onNodeWithText("待处理").performClick()
        ui.onNodeWithText("此分类暂无任务").assertIsDisplayed()
        db.library().putDownload(DownloadTask("synthetic-paused","visual-v1","PAUSED",downloadedBytes=1_000_000,totalBytes=5_000_000))
        ui.waitUntil(5000) {ui.onAllNodesWithText("今晚也辛苦了.wav").fetchSemanticsNodes().isNotEmpty()}
        ui.onNodeWithText("继续").assertIsEnabled()
        ui.onNodeWithText("已完成").performClick()
        ui.onNodeWithText("今晚也辛苦了.wav").assertDoesNotExist()
        ui.onNodeWithText("全部").performClick()
        ui.onNodeWithText("今晚也辛苦了.wav").assertIsDisplayed()
        capture("安卓任务中心")
        ui.onNodeWithText("离线下载").performClick()
        ui.onNodeWithText("存储").assertIsDisplayed()
        ui.onNodeWithText("1 组下载").assertIsDisplayed()
        capture("安卓任务中心离线下载")
        ui.onNodeWithText("全部任务").performClick()
        ui.onNodeWithText("今晚也辛苦了.wav").assertIsDisplayed()
    }
    private fun capture(name:String) {
        if(InstrumentationRegistry.getArguments().getString("visualFixture")!="true")return
        val dir=File(InstrumentationRegistry.getInstrumentation().targetContext.getExternalFilesDir(null),"sync-tasks").apply {mkdirs()}
        ui.onRoot().captureToImage().asAndroidBitmap().let {image->File(dir,"$name.png").outputStream().use {image.compress(Bitmap.CompressFormat.PNG,100,it)}}
    }
}
