package com.asmrplayer.android.artwork

import com.asmrplayer.android.data.CacheMetrics
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.io.File
import java.io.IOException

/** Encoded images are shared by every display size. Only complete, decodable files are published. */
internal object CoverDiskCache {
    private const val budget = 96L * 1024 * 1024
    private const val maxEntries = 1024
    private const val weekMs = 7L * 86_400_000
    private const val maxFileBytes = 32L * 1024 * 1024
    private val locks = Array(32) { Mutex() }
    private val trimLock = Any()

    // Published entries have UUID filenames; temporary downloads must never be cleared mid-write.
    private fun entries(context: Context) = File(context.cacheDir, "artwork-v1").listFiles().orEmpty()
        .filter { it.isFile && it.extension.isEmpty() }
    fun storedBytes(context: Context): Long = synchronized(trimLock) { entries(context).sumOf { it.length() } }
    fun clearStored(context: Context): Pair<Int, Int> = synchronized(trimLock) {
        val files = entries(context)
        val removed = files.count { it.delete() }
        removed to (files.size - removed)
    }

    suspend fun read(context: Context, identity: String, edge: Int, address: suspend () -> String): Bitmap? {
        val key = LibraryIndex.id(identity)
        return locks[(key.hashCode() and Int.MAX_VALUE) % locks.size].withLock {
            val dir = File(context.cacheDir, "artwork-v1").apply { mkdirs() }
            val file = File(dir, key)
            if (file.exists() && System.currentTimeMillis() - file.lastModified() < weekMs) {
                val bitmap = decode(file, edge)
                if (bitmap != null) {
                    return@withLock bitmap
                }
                file.delete()
            }
            val temp = File.createTempFile("cover-", ".part", dir)
            try {
                CacheMetrics.imageFetches.incrementAndGet()
                AndroidBackend.get(context).online.provider.http.response(address()) { response ->
                    if (response.responseCode !in 200..299) throw IOException("封面请求失败")
                    response.inputStream.use { input -> temp.outputStream().buffered().use { output ->
                        val buffer = ByteArray(32 * 1024)
                        var total = 0L
                        while (true) {
                            currentCoroutineContext().ensureActive()
                            val count = input.read(buffer)
                            if (count < 0) break
                            total += count
                            if (total > maxFileBytes) throw IOException("封面过大")
                            output.write(buffer, 0, count)
                        }
                    } }
                }
                val bitmap = decode(temp, edge) ?: return@withLock null
                synchronized(trimLock) {
                    if (temp.renameTo(file)) trim(dir)
                }
                bitmap
            } catch (failure: IOException) {
                // An expired but intact copy is still useful during a network outage.
                if (file.isFile) decode(file, edge) ?: throw failure else throw failure
            } finally { temp.delete() }
        }
    }

    private fun decode(file: File, edge: Int): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
        CacheMetrics.imageDecodes.incrementAndGet()
        return BitmapFactory.decodeFile(file.path, artworkDecodeOptions(bounds.outWidth, bounds.outHeight, edge))
    }

    private fun trim(dir: File) {
        val files = dir.listFiles().orEmpty()
        val cutoff = System.currentTimeMillis() - 86_400_000
        files.filter { it.extension == "part" && it.lastModified() < cutoff }.forEach { it.delete() }
        val complete = files.filter { it.extension != "part" }.sortedBy { it.lastModified() }
        var total = complete.sumOf { it.length() }
        val availableBudget = minOf(budget, (dir.usableSpace + total) / 8)
        for ((index, file) in complete.withIndex()) {
            if (total <= availableBudget && complete.size - index <= maxEntries) break
            val size = file.length()
            if (file.delete()) total -= size
        }
    }
}
