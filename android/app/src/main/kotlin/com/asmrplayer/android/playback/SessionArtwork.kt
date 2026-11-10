package com.asmrplayer.android.playback

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import com.asmrplayer.android.artwork.CoverArtwork
import com.asmrplayer.android.artwork.systemCoverBytes
import com.asmrplayer.android.R
import com.asmrplayer.android.data.AndroidBackend
import kotlinx.coroutines.*
import java.io.ByteArrayOutputStream

/** Only the current cover is kept. No URLs, credentials or private artwork leave through metadata. */
internal class SessionArtwork(
    private val context: Context, private val player: Player, private val private: () -> Boolean,
    private val metadata: (MediaItem) -> MediaItem,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var job: Job? = null
    private var generation = 0L
    private var currentId: String? = null
    private var cover: ByteArray? = null
    private val neutral = BitmapFactory.decodeResource(context.resources, R.drawable.app_icon,
        BitmapFactory.Options().apply { inSampleSize = 4 })?.let(::encode)
    private val listener = object : Player.Listener {
        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) { refresh() }
    }
    init { player.addListener(listener) }
    fun forItem(id: String): ByteArray? = if (!private() && currentId == id) cover ?: neutral else neutral
    fun hasCover(id: String): Boolean = !private() && currentId == id && cover != null
    private var encodedCover: Pair<CoverArtwork.Key, ByteArray>? = null
    fun invalidate() { encodedCover = null; ++generation; job?.cancel(); currentId = null; cover = null }
    fun refresh() {
        if (private()) { invalidate(); return }
        val previous = encodedCover
        invalidate()
        val id = player.currentMediaItem?.mediaId ?: return
        val stamp = generation
        job = scope.launch {
            val bytes = try {
                withContext(Dispatchers.IO) {
                    val backend = AndroidBackend.get(context)
                    val version = backend.database.library().version(id) ?: return@withContext null
                    val address = backend.database.library().work(version.workId)?.coverUri
                    val request = CoverArtwork.request(context, address, version.workId, 1536)
                    if (previous?.first == request.key) return@withContext previous
                    val bitmap = CoverArtwork.best(context, request) ?: return@withContext null
                    ensureActive()
                    request.key to systemCoverBytes(bitmap)
                }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { null } // Missing cover must not interrupt playback.
            if (generation != stamp || private() || player.currentMediaItem?.mediaId != id) return@launch
            encodedCover = bytes
            currentId = id; cover = bytes?.second
            val item = player.currentMediaItem ?: return@launch
            player.replaceMediaItem(player.currentMediaItemIndex, metadata(item))
        }
    }
    private fun encode(bitmap: Bitmap): ByteArray = ByteArrayOutputStream().use { stream ->
        bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream); bitmap.recycle(); stream.toByteArray()
    }
    fun close() { player.removeListener(listener); invalidate(); scope.cancel() }
}
