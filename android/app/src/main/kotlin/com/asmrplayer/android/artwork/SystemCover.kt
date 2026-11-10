package com.asmrplayer.android.artwork

import android.graphics.Bitmap
import java.io.ByteArrayOutputStream
import kotlin.math.roundToInt

/** Preserve aspect ratio and do not recycle the bitmap shared with the app artwork cache. */
internal fun systemCoverBytes(source:Bitmap):ByteArray {
    val scale=minOf(1f,1536f/maxOf(source.width,source.height))
    val sized=if(scale<1f)Bitmap.createScaledBitmap(source,(source.width*scale).roundToInt().coerceAtLeast(1),(source.height*scale).roundToInt().coerceAtLeast(1),true)else source
    try {
        fun encode(format:Bitmap.CompressFormat,quality:Int)=ByteArrayOutputStream().use{stream->sized.compress(format,quality,stream);stream.toByteArray()}
        var bytes=encode(Bitmap.CompressFormat.PNG,100)
        if(bytes.size<=768*1024)return bytes
        for(quality in listOf(92,85,75,65,50,35,20)) {
            bytes=encode(Bitmap.CompressFormat.JPEG,quality)
            if(bytes.size<=768*1024)return bytes
        }
        check(bytes.size<=768*1024)
        return bytes
    } finally {if(sized!==source)sized.recycle()}
}
