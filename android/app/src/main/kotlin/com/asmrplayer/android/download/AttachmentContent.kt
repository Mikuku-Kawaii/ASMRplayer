package com.asmrplayer.android.download

import android.graphics.BitmapFactory
import com.asmrplayer.android.data.RemoteAsset
import com.asmrplayer.android.subtitles.SubtitleParser
import java.io.File

/** Validate before rename/publication. Decode images at bounded resolution, never full artwork size. */
internal object AttachmentContent {
    fun limit(asset: RemoteAsset): Long = if (asset.kind == "subtitle") 8L * 1024 * 1024 else 32L * 1024 * 1024
    fun validate(file: File, asset: RemoteAsset) {
        require(file.length() in 1..limit(asset)) { "附件大小不受支持" }
        if (asset.kind == "subtitle") {
            SubtitleParser.decode(file.readBytes(), asset.format)
        } else {
            val options = BitmapFactory.Options().apply { inJustDecodeBounds = true; inSampleSize = 1 }
            BitmapFactory.decodeFile(file.path, options)
            require(options.outWidth > 0 && options.outHeight > 0) { "附件不是可解码图片" }
            while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 256) options.inSampleSize *= 2
            options.inJustDecodeBounds = false
            val bitmap = BitmapFactory.decodeFile(file.path, options)
            requireNotNull(bitmap) { "图片数据损坏" }
            bitmap.recycle()
        }
    }
}
