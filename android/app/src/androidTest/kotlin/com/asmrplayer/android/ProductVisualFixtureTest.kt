package com.asmrplayer.android

import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.data.*
import kotlinx.coroutines.runBlocking
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Explicit opt-in fixture, restricted to the separate .uitest application. Contains only silence and authored text. */
@RunWith(AndroidJUnit4::class)
class ProductVisualFixtureTest {
    @Test fun seedVisualReview() = runBlocking {
        org.junit.Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("visualFixture") == "true")
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val db=AndroidBackend.get(context).database
        db.clearAllTables()
        val dao=db.library()
        val wav=File(context.filesDir,"visual-silence.wav")
        val bytes=48000*120*4
        val header=ByteBuffer.allocate(44).order(ByteOrder.LITTLE_ENDIAN)
        header.put("RIFF".toByteArray()).putInt(bytes+36).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(2).putInt(48000).putInt(192000).putShort(4).putShort(16).put("data".toByteArray()).putInt(bytes)
        wav.outputStream().use {out->out.write(header.array());repeat(120){out.write(ByteArray(192000))}}
        val subtitle=File(context.filesDir,"visual-lines.srt")
        val lines=listOf("夜晚慢慢安静下来。","把耳机调到舒适的音量。","窗外的雨声，轻轻落在屋檐上。","放慢呼吸，给自己一点休息的时间。","这一段台词用于检查长句的换行、阅读间距，以及播放时的自动跟随。","接下来，只需要听见声音。")
        subtitle.writeText(lines.mapIndexed {i,t->"${i+1}\n00:%02d:%02d,000 --> 00:%02d:%02d,000\n%s".format((i*20)/60,(i*20)%60,((i+1)*20)/60,((i+1)*20)%60,t)}.joinToString("\n\n"))
        val cover=File(context.filesDir,"visual-cover.png")
        val bitmap=android.graphics.Bitmap.createBitmap(800,600,android.graphics.Bitmap.Config.ARGB_8888)
        val canvas=android.graphics.Canvas(bitmap)
        val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
        paint.shader=android.graphics.LinearGradient(0f,0f,800f,800f,0xffc8b4d8.toInt(),0xff485d83.toInt(),android.graphics.Shader.TileMode.CLAMP)
        canvas.drawRect(0f,0f,800f,800f,paint);paint.shader=null;paint.color=0x55ffffff;canvas.drawCircle(580f,220f,180f,paint)
        paint.color=0x44ffffff;canvas.drawRoundRect(80f,430f,720f,850f,220f,220f,paint)
        cover.outputStream().use {bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        dao.putWork(LibraryWork("visual-work",null,"","雨夜来信 · 合成演示作品","DEMO",Uri.fromFile(cover).toString(),true,"SAFE"))
        db.metadata().put(CatalogMetadata("visual-work","演示工作室","[\"示例声优\"]","[\"雨声\",\"轻声\",\"助眠\"]","这是用于界面验收的合成作品。音频内容为静音，台词为原创测试文字。\n\n检查封面、作品信息、音轨分组、附件和台词阅读。", "2026-11-03",true))
        val names=listOf("01 雨声与问候.wav","02 今晚也辛苦了.wav","03 长标题与多行换行的显示检查 · 闭上眼睛慢慢休息.wav")
        names.forEachIndexed {i,name->
            dao.putTrack(LibraryTrack("visual-t$i","visual-work",name,"WAV"))
            dao.putVersion(TrackVersion("visual-v$i","visual-t$i","wav"))
            dao.putSource(MediaSource("visual-s$i","visual-v$i",null,"WAV/$name",Uri.fromFile(wav).toString(),wav.length()))
            dao.putSubtitle(SubtitleSource("visual-sub$i","visual-v$i",null,"台词.srt",Uri.fromFile(subtitle).toString(),"台词.srt","srt"))
            dao.putSubtitlePreference(SubtitlePreference("visual-v$i","visual-sub$i"))
        }
        dao.putAsset(WorkAsset("visual-image","visual-work","","封面.png",Uri.fromFile(cover).toString(),"png"))
        dao.putAsset(WorkAsset("visual-text","visual-work","","台词.srt",Uri.fromFile(subtitle).toString(),"srt"))
        dao.putPosition(PlaybackPosition("visual-v0",25000,120000))
        dao.putBookmark(Bookmark("visual-bookmark","visual-v0",40000,"雨声落在屋檐上"))
        dao.putSession(SessionRecord(queueJson="[\"visual-v0\",\"visual-v1\",\"visual-v2\"]",index=0,positionMs=25000))
        db.queueGroups().put(QueueGroup("visual-group","雨夜助眠",QueueGroup.encode(listOf("visual-v0","visual-v1","visual-v2"))))
        db.queueGroups().put(QueueGroup("visual-group-empty","周末慢慢听",sortOrder=1))
        dao.putPreferences(LibraryPreference())
    }
}
