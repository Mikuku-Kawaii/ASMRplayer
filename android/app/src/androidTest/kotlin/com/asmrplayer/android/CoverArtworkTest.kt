package com.asmrplayer.android

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.artwork.*
import com.asmrplayer.android.data.*
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.OnlineFixtureServer
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.ByteArrayOutputStream

@RunWith(AndroidJUnit4::class)
class CoverArtworkTest {
    @Test fun unnamedOriginalIsMatchedAndSystemReceives1536WithoutRecyclingSharedBitmap() = runBlocking {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val id="cover-match-${System.nanoTime()}"
        val files=mutableListOf<File>()
        fun save(bitmap:Bitmap,name:String):String {
            val file=File(context.cacheDir,"$id-$name");files+=file
            file.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
            return Uri.fromFile(file).toString()
        }
        val original=Bitmap.createBitmap(1600,1200,Bitmap.Config.ARGB_8888)
        val canvas=Canvas(original);val paint=Paint()
        for(y in 0 until 24)for(x in 0 until 32){paint.color=0xff000000.toInt() or ((x*7) shl 16) or ((y*10) shl 8) or (if((x/4+y/3)%2==0)220 else 40);canvas.drawRect(x*50f,y*50f,(x+1)*50f,(y+1)*50f,paint)}
        val thumbnail=Bitmap.createScaledBitmap(original,320,240,true)
        val intermediate = Bitmap.createScaledBitmap(original, 1100, 825, true)
        val wrong=Bitmap.createBitmap(1600,1200,Bitmap.Config.ARGB_8888).apply{eraseColor(0xffcc88cc.toInt())}
        try {
            val uri=save(thumbnail,"thumbnail.png")
            val dao=AndroidBackend.get(context).database.library()
            dao.putWork(LibraryWork(id,null,"","Synthetic cover",coverUri=uri))
            dao.putAsset(WorkAsset("$id-intermediate", id, "", "cover_logo.png", save(intermediate, "intermediate.png"), "png"))
            dao.putAsset(WorkAsset("$id-wrong",id,"","a_illustration.png",save(wrong,"wrong.png"),"png"))
            dao.putAsset(WorkAsset("$id-original",id,"","b_特典画像.png",save(original,"original.png"),"png"))
            val selected=CoverArtwork.best(context,uri,id,3072)!!
            assertEquals(1600,selected.width);assertEquals(1200,selected.height)
            assertEquals(1536, CoverArtwork.best(context, uri, id, 1536)!!.width)
            val bytes=systemCoverBytes(selected)
            assertTrue(bytes.size<=768*1024);assertFalse(selected.isRecycled)
            val decoded=BitmapFactory.decodeByteArray(bytes,0,bytes.size)
            assertEquals(1536,decoded.width);assertEquals(1152,decoded.height);decoded.recycle()
        } finally {original.recycle();thumbnail.recycle();intermediate.recycle();wrong.recycle();files.forEach{it.delete()}}
    }

    @Test fun fullPlayerUpgradesThumbnailFromCachedOnlineDirectoryWithoutImportingAttachments() = runBlocking {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        check(context.packageName.endsWith(".uitest"))
        val backend = AndroidBackend.get(context)
        val oldBase = backend.online.account.state.value.apiBase
        OnlineFixtureServer().use { server ->
            val original = Bitmap.createBitmap(1800, 1350, Bitmap.Config.ARGB_8888)
            server.originalArtwork = ByteArrayOutputStream().use {
                original.compress(Bitmap.CompressFormat.PNG, 100, it)
                it.toByteArray()
            }
            original.recycle()
            val workId = LibraryIndex.id("asmr_one", server.base, "123456")
            val thumbnail = "${server.base}/media/cover"
            try {
                backend.online.account.setApiBase(server.base)
                backend.database.library().putWork(LibraryWork(workId, null, "", "Synced artwork", coverUri = thumbnail))
                backend.database.metadata().put(CatalogMetadata(workId, origin = server.base, sourceId = "123456"))
                assertEquals(16, CoverArtwork.best(context, thumbnail, workId, 384)!!.width)
                assertEquals(0, server.treeRequests.get())
                backend.online.detail("123456")
                val selected = CoverArtwork.best(context, thumbnail, workId, 1536)!!
                assertEquals(1536, selected.width)
                assertEquals(1152, selected.height)
                assertSame(selected, CoverArtwork.best(context, thumbnail, workId, 1536))
                assertEquals(1, server.treeRequests.get())
                assertEquals(1, server.requests.count { it.path == "/media/original" })
            } finally {
                backend.online.account.setApiBase(oldBase)
            }
        }
    }
    @Test fun smallCoversAreNotUpscaledForTheSystem() {
        val bitmap=Bitmap.createBitmap(320,240,Bitmap.Config.ARGB_8888)
        try {
            val bytes=systemCoverBytes(bitmap);val decoded=BitmapFactory.decodeByteArray(bytes,0,bytes.size)
            assertEquals(320,decoded.width);assertEquals(240,decoded.height);assertFalse(bitmap.isRecycled);decoded.recycle()
        } finally {bitmap.recycle()}
    }
}
