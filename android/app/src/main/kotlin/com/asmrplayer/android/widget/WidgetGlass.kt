package com.asmrplayer.android.widget

import android.graphics.*
import kotlin.math.min
import kotlin.math.roundToInt

internal data class GlassFrame(val bitmap:Bitmap,val ink:Int,val secondary:Int)

/** Opaque blurred cover ambience, with a neutral fallback when no cover is public. */
internal object WidgetGlass {
    fun render(widthDp:Float,heightDp:Float,config:WidgetGlassConfig,cover:Bitmap?=null):GlassFrame {
        val scale=minOf(2f,720f/widthDp,400f/heightDp)
        val w=(widthDp*scale).toInt().coerceAtLeast(1);val h=(heightDp*scale).toInt().coerceAtLeast(1)
        val dark=cover!=null || config.mode!="light"
        val ink=if(dark)Color.rgb(250,250,252) else Color.rgb(24,24,28)
        val secondary=if(dark)Color.rgb(235,235,240) else Color.rgb(46,46,52)
        val bitmap=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);val canvas=Canvas(bitmap)
        val paint=Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
        val inset=3f*scale;val rect=RectF(inset,inset,w-inset,h-inset);val radius=min(24f*scale,rect.height()/3)
        val shape=Path().apply{addRoundRect(rect,radius,radius,Path.Direction.CW)}
        val tint=if(dark)Color.rgb(8,10,14) else Color.rgb(248,249,252)
        fun tintAt(opacity:Float)=(tint and 0x00ffffff) or ((opacity.coerceIn(0f,1f)*255).roundToInt() shl 24)
        // Keep the cast shadow outside the surface.
        canvas.save()
        @Suppress("DEPRECATION")
        canvas.clipPath(shape,Region.Op.DIFFERENCE)
        paint.color=Color.BLACK
        paint.setShadowLayer(2f*scale,0f,.8f*scale,0x24000000)
        canvas.drawRoundRect(rect,radius,radius,paint);paint.clearShadowLayer();canvas.restore()
        if(config.solid){paint.color=tint;canvas.drawRoundRect(rect,radius,radius,paint);return GlassFrame(bitmap,ink,secondary)}
        // The entire surface is opaque, including covers with transparent source pixels.
        paint.color=if(dark)Color.rgb(47,47,55) else Color.rgb(236,237,242)
        canvas.drawRoundRect(rect,radius,radius,paint)
        if(cover!=null){
            val blurred=blurCover(cover,rect.width()/rect.height())
            canvas.save();canvas.clipPath(shape)
            paint.color=Color.WHITE
            paint.colorFilter=ColorMatrixColorFilter(ColorMatrix().apply{setSaturation(1.65f)})
            canvas.drawBitmap(blurred,null,rect,paint)
            paint.colorFilter=null
            canvas.restore();blurred.recycle()
        }
        val opacity=if(cover!=null)(.27f+config.opacity*.55f).coerceIn(.31f,.47f) else config.opacity.coerceIn(.08f,.36f)
        val shades=if(cover!=null)intArrayOf(tintAt(opacity),tintAt(opacity-.07f),tintAt(opacity+.04f))
            else intArrayOf(tintAt(opacity*1.1f),tintAt(opacity*.86f),tintAt(opacity))
        paint.shader=LinearGradient(0f,rect.top,0f,rect.bottom,
            shades,floatArrayOf(0f,.52f,1f),Shader.TileMode.CLAMP)
        canvas.drawRoundRect(rect,radius,radius,paint)
        canvas.save();canvas.clipPath(shape)
        // One contour shares the cover's silhouette. An inset ring or a full-face
        // white wash makes the cover look trapped under a separate plastic shell.
        paint.style=Paint.Style.STROKE;paint.strokeWidth=.6f*scale
        paint.shader=LinearGradient(rect.left,rect.top,rect.right,rect.bottom,
            intArrayOf(0x38ffffff,0x08ffffff,0x00ffffff,0x20ffffff),floatArrayOf(0f,.20f,.70f,1f),Shader.TileMode.CLAMP)
        val edgeInset=paint.strokeWidth/2
        canvas.drawRoundRect(RectF(rect).apply{inset(edgeInset,edgeInset)},radius-edgeInset,radius-edgeInset,paint)
        canvas.restore()
        return GlassFrame(bitmap,ink,secondary)
    }
    fun decodeCover(bytes:ByteArray):Bitmap? {
        val bounds=BitmapFactory.Options().apply{inJustDecodeBounds=true}
        BitmapFactory.decodeByteArray(bytes,0,bytes.size,bounds)
        if(bounds.outWidth<=0||bounds.outHeight<=0)return null
        var sample=1
        while(maxOf(bounds.outWidth,bounds.outHeight)/sample>256)sample*=2
        return BitmapFactory.decodeByteArray(bytes,0,bytes.size,BitmapFactory.Options().apply{inSampleSize=sample})
    }
    private fun blurCover(source:Bitmap,aspect:Float):Bitmap {
        // Bounded working surface; blur cost does not grow with the original cover.
        val w=160;val h=(w/aspect).roundToInt().coerceIn(40,160)
        val target=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888)
        val scale=maxOf(w.toFloat()/source.width,h.toFloat()/source.height)
        val dx=(source.width*scale-w)/2;val dy=(source.height*scale-h)/2
        Canvas(target).drawBitmap(source,null,RectF(-dx,-dy,w+dx,h+dy),Paint(Paint.FILTER_BITMAP_FLAG))
        var pixels=IntArray(w*h);target.getPixels(pixels,0,w,0,0,w,h)
        fun pass(input:IntArray,horizontal:Boolean):IntArray {
            val out=IntArray(input.size);val outer=if(horizontal)h else w;val inner=if(horizontal)w else h;val r=3
            fun index(a:Int,b:Int)=if(horizontal)a*w+b else b*w+a
            for(a in 0 until outer){
                var red=0;var green=0;var blue=0
                fun add(b:Int,sign:Int){val c=input[index(a,b.coerceIn(0,inner-1))];red+=Color.red(c)*sign;green+=Color.green(c)*sign;blue+=Color.blue(c)*sign}
                for(b in -r..r)add(b,1)
                for(b in 0 until inner){out[index(a,b)]=Color.rgb(red/(2*r+1),green/(2*r+1),blue/(2*r+1));add(b-r,-1);add(b+r+1,1)}
            };return out
        }
        repeat(3){pixels=pass(pass(pixels,true),false)}
        target.setPixels(pixels,0,w,0,0,w,h);return target
    }
}
