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
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

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
        val wrong=Bitmap.createBitmap(1600,1200,Bitmap.Config.ARGB_8888).apply{eraseColor(0xffcc88cc.toInt())}
        try {
            val uri=save(thumbnail,"thumbnail.png")
            val dao=AndroidBackend.get(context).database.library()
            dao.putWork(LibraryWork(id,null,"","Synthetic cover",coverUri=uri))
            dao.putAsset(WorkAsset("$id-wrong",id,"","a_illustration.png",save(wrong,"wrong.png"),"png"))
            dao.putAsset(WorkAsset("$id-original",id,"","b_特典画像.png",save(original,"original.png"),"png"))
            val selected=CoverArtwork.best(context,uri,id,3072)!!
            assertEquals(1600,selected.width);assertEquals(1200,selected.height)
            assertEquals(1536, CoverArtwork.best(context, uri, id, 1536)!!.width)
            val bytes=systemCoverBytes(selected)
            assertTrue(bytes.size<=768*1024);assertFalse(selected.isRecycled)
            val decoded=BitmapFactory.decodeByteArray(bytes,0,bytes.size)
            assertEquals(1536,decoded.width);assertEquals(1152,decoded.height);decoded.recycle()
        } finally {original.recycle();thumbnail.recycle();wrong.recycle();files.forEach{it.delete()}}
    }
    @Test fun smallCoversAreNotUpscaledForTheSystem() {
        val bitmap=Bitmap.createBitmap(320,240,Bitmap.Config.ARGB_8888)
        try {
            val bytes=systemCoverBytes(bitmap);val decoded=BitmapFactory.decodeByteArray(bytes,0,bytes.size)
            assertEquals(320,decoded.width);assertEquals(240,decoded.height);assertFalse(bitmap.isRecycled);decoded.recycle()
        } finally {bitmap.recycle()}
    }
}
