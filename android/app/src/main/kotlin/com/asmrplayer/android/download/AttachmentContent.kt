package com.asmrplayer.android.download

import android.graphics.BitmapFactory
import com.asmrplayer.android.data.RemoteAsset
import com.asmrplayer.android.subtitles.SubtitleParser
import java.io.File

/** Validate before rename/publication. Decode images at bounded resolution, never full artwork size. */
internal object AttachmentContent {
    fun limit(asset: RemoteAsset): Long = when (asset.kind) {
        "subtitle" -> 8L * 1024 * 1024
        "image", "cover" -> 32L * 1024 * 1024
        else -> Long.MAX_VALUE
    }
    fun validate(file: File, asset: RemoteAsset) {
        require(file.length() in 1..limit(asset)) { "附件大小不受支持" }
        if (asset.kind == "subtitle") {
            SubtitleParser.decode(file.readBytes(), asset.format)
        } else if (asset.kind in setOf("image", "cover")) {
            val options = BitmapFactory.Options().apply { inJustDecodeBounds = true; inSampleSize = 1 }
            BitmapFactory.decodeFile(file.path, options)
            require(options.outWidth > 0 && options.outHeight > 0) { "附件不是可解码图片" }
            while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 256) options.inSampleSize *= 2
            options.inJustDecodeBounds = false
            val bitmap = BitmapFactory.decodeFile(file.path, options)
            requireNotNull(bitmap) { "图片数据损坏" }
            bitmap.recycle()
        } else if (asset.format.lowercase() !in setOf("html", "htm")) {
            val head = file.inputStream().use { input ->
                val bytes = ByteArray(512)
                val count = input.read(bytes)
                String(bytes, 0, count.coerceAtLeast(0), Charsets.UTF_8).trimStart().lowercase()
            }
            require(!head.startsWith("<!doctype html") && !head.startsWith("<html")) { "来源返回了错误页面" }
        }
    }
}
