package com.asmrplayer.android.widget

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WidgetGlassTest {
    @Test fun neutralMaterialDoesNotRevealHostBackgrounds() {
        for(mode in listOf("light","dark")) for(opacity in listOf(.08f,.20f,.36f)) {
            val frame=WidgetGlass.render(320f,160f,WidgetGlassConfig(mode,opacity))
            val image=frame.bitmap;val x=image.width/2;val y=image.height/2
            assertEquals(255,Color.alpha(image.getPixel(x,y)))
            assertEquals(0,Color.alpha(image.getPixel(0,0)))
            fun composite(background:Int):Int {
                val surface=Bitmap.createBitmap(image.width,image.height,Bitmap.Config.ARGB_8888)
                val canvas=Canvas(surface);canvas.drawColor(background);canvas.drawBitmap(image,0f,0f,null)
                return surface.getPixel(x,y).also{surface.recycle()}
            }
            val red=composite(Color.RED);val blue=composite(Color.BLUE)
            assertEquals("wallpaper must not affect the neutral surface",red,blue)
            image.recycle()
        }
    }
    @Test fun solidAccessibilityOptionRemainsOpaqueAndBitmapsStayBounded() {
        val frame=WidgetGlass.render(1200f,700f,WidgetGlassConfig(solid=true))
        assertTrue(frame.bitmap.width<=720);assertTrue(frame.bitmap.height<=400)
        assertEquals(255,Color.alpha(frame.bitmap.getPixel(frame.bitmap.width/2,frame.bitmap.height/2)))
        frame.bitmap.recycle()
    }
    @Test fun coverFillsTheCardAndItsColourBoundaryIsBlurred() {
        val cover=Bitmap.createBitmap(160,160,Bitmap.Config.ARGB_8888)
        val canvas=Canvas(cover)
        canvas.drawColor(Color.RED)
        canvas.drawRect(80f,0f,160f,160f,android.graphics.Paint().apply{color=Color.BLUE})
        val frame=WidgetGlass.render(320f,160f,WidgetGlassConfig(),cover)
        val mid=frame.bitmap.getPixel(frame.bitmap.width/2,frame.bitmap.height/2)
        assertEquals("cover ambience must not reveal wallpaper",255,Color.alpha(mid))
        assertTrue("blur must mix both sides of the cover",Color.red(mid)>30 && Color.blue(mid)>30)
        assertEquals(0,Color.alpha(frame.bitmap.getPixel(0,0)))
        assertTrue(Color.red(frame.bitmap.getPixel(60,frame.bitmap.height/2))>Color.blue(frame.bitmap.getPixel(60,frame.bitmap.height/2)))
        frame.bitmap.recycle();cover.recycle()
    }
}
