package com.asmrplayer.android.artwork

import com.asmrplayer.android.data.CacheMetrics
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.ProviderAccountState
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** Bounded, cancellable cover loading; no account Token is attached to image/CDN requests. */
internal object CoverArtwork {
    private val slots = Semaphore(3)
    data class Key(val reference: String?, val workId: String?, val revision: Long, val edge: Int, val scope: String)
    data class Request(val address: String?, val key: Key)
    private data class CachedCover(val bitmap: Bitmap, val validUntil: Long = Long.MAX_VALUE)
    private data class Selection(val bitmap: Bitmap?, val retryUpgrade: Boolean = false)
    private val cache = object : LruCache<Key, CachedCover>(32 * 1024 * 1024) {
        override fun sizeOf(key: Key, value: CachedCover) = value.bitmap.allocationByteCount
    }
    private val failures = LruCache<Key, Long>(256)
    private val largeImageSlot = Mutex()

    private val locks = Array(32) { Mutex() }
    private val revisions = LruCache<String, Long>(1024)
    private var counter = 0L
    private val matchedPaths = LruCache<String, String>(64)

    @Synchronized fun invalidate(workId: String) { revisions.remove(workId) }

    @Synchronized fun request(context: Context, uri: String?, workId: String?, edge: Int): Request {
        val identity = workId ?: uri.orEmpty()
        val revision = revisions.get(identity) ?: (++counter).also { revisions.put(identity, it) }
        val account = AndroidBackend.get(context).online.account.state.value
        val reference = if (workId == null || uri == null) uri else stableAddress(uri)
        return Request(uri, Key(reference, workId, revision, edge, "${account.apiBase}:${account.cacheScope}"))
    }

    fun trimMemory() { cache.trimToSize(cache.maxSize() / 4) }

    fun peek(request: Request): Bitmap? = cache.get(request.key)
        ?.takeIf { it.validUntil > android.os.SystemClock.elapsedRealtime() }?.bitmap
    fun preview(request: Request): Bitmap? = listOf(request.key.edge, 768, 384, 1536, 2048, 3072).firstNotNullOfOrNull {
        cache.get(request.key.copy(edge = it))?.bitmap
    }
    fun recentlyFailed(request: Request) =
        (failures.get(request.key) ?: 0L) > android.os.SystemClock.elapsedRealtime()

    private fun stableAddress(address: String): String {
        if (!address.startsWith("http") || '?' !in address) return address
        val uri = Uri.parse(address)
        val stable = uri.buildUpon().clearQuery()
        uri.queryParameterNames.filterNot { it.lowercase() in setOf("signature", "token", "expires", "sig")
            || it.startsWith("X-Amz-", ignoreCase = true) }.sorted().forEach { name ->
            uri.getQueryParameters(name).forEach { stable.appendQueryParameter(name, it) }
        }
        return stable.build().toString()
    }

    suspend fun best(context: Context, uri: String?, workId: String?, maxEdge: Int) =
        best(context, request(context, uri, workId, maxEdge))

    suspend fun best(context: Context, request: Request): Bitmap? = withContext(Dispatchers.IO) {
        peek(request)?.let { return@withContext it }
        val key = request.key
        locks[(key.hashCode() and Int.MAX_VALUE) % locks.size].withLock {
            peek(request)?.let { return@withLock it }
            if (recentlyFailed(request)) return@withLock null
            val result = try {
                if (key.edge > 1024) largeImageSlot.withLock { slots.withPermit { select(context, request) } }
                else slots.withPermit { select(context, request) }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { Selection(null) }
            val bitmap = result.bitmap
            if (bitmap == null) failures.put(key, android.os.SystemClock.elapsedRealtime() + 20_000)
            else if (bitmap.allocationByteCount <= cache.maxSize()) {
                val validUntil = if (result.retryUpgrade) android.os.SystemClock.elapsedRealtime() + 30_000 else Long.MAX_VALUE
                cache.put(key, CachedCover(bitmap, validUntil))
            }
            bitmap
        }
    }

    private suspend fun select(context: Context, request: Request): Selection {
        val key = request.key
        val owner = key.workId?.let {
            val epoch = context.getSharedPreferences("artwork-revisions", Context.MODE_PRIVATE).getLong(it, 0)
            "$it:${key.scope}:$epoch"
        }
        val backend = AndroidBackend.get(context)
        val meta = key.workId?.let { backend.database.metadata().forWork(it) }
        val account = backend.online.account.state.value
        val source = meta?.takeIf { it.origin == account.apiBase &&
            "${account.apiBase}:${account.cacheScope}" == key.scope && it.sourceId.matches(Regex("[0-9]{1,12}")) }
        val reference = request.address ?: source?.let { "${it.origin}/api/cover/${it.sourceId}.jpg?type=main" }
        val workId = key.workId ?: return Selection(reference?.let { readUri(context, it, key.edge, null) })
        val onlineWork = source?.takeIf { workId == LibraryIndex.id("asmr_one", it.origin, it.sourceId) }
        return loadOriginal(context, workId, key.edge, reference, key.revision, requireNotNull(owner),
            onlineWork?.sourceId, account)
    }

    fun refresh(context: Context, workId: String) {
        context.getSharedPreferences("artwork-revisions", Context.MODE_PRIVATE).edit()
            .putLong(workId, System.currentTimeMillis()).apply()
        invalidate(workId)
    }

    private suspend fun readUri(context: Context, address: String, maxEdge: Int, owner: String? = null): Bitmap? {
        val uri = Uri.parse(address)
        if (uri.scheme in setOf("https", "http")) {
            val identity = if (owner == null) address else "$owner:${stableAddress(address)}"
            return CoverDiskCache.read(context, identity, maxEdge) { address }
        }
        val bounds=BitmapFactory.Options().apply{inJustDecodeBounds=true}
        context.contentResolver.openInputStream(uri)?.use{BitmapFactory.decodeStream(it,null,bounds)}
        val options = artworkDecodeOptions(bounds.outWidth, bounds.outHeight, maxEdge)
        CacheMetrics.imageDecodes.incrementAndGet()
        return context.contentResolver.openInputStream(uri)?.use{BitmapFactory.decodeStream(it,null,options)}
    }
    private fun sameCover(reference:Bitmap,candidate:Bitmap):Boolean {
        fun pixels(source:Bitmap):IntArray {
            val small=Bitmap.createScaledBitmap(source,32,24,true)
            val result=IntArray(768);small.getPixels(result,0,32,0,0,32,24)
            if(small!==source)small.recycle()
            return result
        }
        return matchingCoverSamples(pixels(reference),pixels(candidate),reference.width.toFloat()/reference.height,candidate.width.toFloat()/candidate.height)
    }
    private data class Candidate(val path: String, val bytes: Long?, val read: suspend () -> Bitmap?) {
        val priority = originalCoverPriority(path)
    }

    /** Resolve attachment identities on demand; do not persist expiring media URLs. */
    private suspend fun loadOriginal(context: Context, workId: String, maxEdge: Int, referenceUri: String?,
        revision: Long, owner: String, sourceId: String?, account: ProviderAccountState): Selection {
        val backend=AndroidBackend.get(context)
        val dao=backend.database.library()
        val local=dao.observeAssets(workId).first().filter { it.available && coverComparisonCandidate(it.relativePath) }
        val remote=dao.observeRemoteAssets(workId).first().filter { it.available && it.kind=="image" && coverComparisonCandidate(it.relativePath) }
        val matchKey="$workId:$revision:$referenceUri"
        val previous = matchedPaths.get(matchKey)
        val highResolution = maxEdge >= 1024
        var retryUpgrade = false
        suspend fun attempt(read: suspend () -> Bitmap?): Bitmap? = try { read() }
        catch (e: CancellationException) { throw e }
        catch (_: Exception) { retryUpgrade = true; null }

        var referenceBitmap: Bitmap? = null
        var referenceLoaded = false
        suspend fun referenceCover(): Bitmap? {
            if (!referenceLoaded) {
                referenceLoaded = true
                referenceBitmap = referenceUri?.let { attempt { readUri(context, it, maxEdge, owner) } }
            }
            return referenceBitmap
        }
        fun edge(bitmap: Bitmap?) = bitmap?.let { maxOf(it.width, it.height) } ?: 0
        var best: Bitmap? = null
        fun consider(bitmap: Bitmap?) {
            if (edge(bitmap) > edge(best)) best = bitmap
        }
        suspend fun scan(candidates: List<Candidate>) {
            val ordered = candidates.sortedWith(compareByDescending<Candidate> { it.path == previous }
                .thenByDescending { it.priority }.thenByDescending { it.bytes ?: 0 }.thenBy { it.path })
            val (named, unnamed) = ordered.partition { it.path == previous || it.priority > 0 }
            for (candidate in named.take(4) + unnamed.take(if (highResolution) 6 else 4)) {
                currentCoroutineContext().ensureActive()
                val bitmap = attempt(candidate.read) ?: continue
                if (candidate in unnamed && referenceCover()?.let { sameCover(it, bitmap) } != true) continue
                if (candidate in unnamed) matchedPaths.put(matchKey, candidate.path)
                consider(bitmap)
                if (edge(best) >= maxEdge) break
            }
        }
        val downloadedPaths = mutableSetOf<String>()
        scan(local.map { asset -> Candidate(asset.relativePath, null) {
            readUri(context, asset.uri, maxEdge)?.also { downloadedPaths += asset.relativePath }
        } })
        if (highResolution && edge(best) < maxEdge) consider(referenceCover())
        if (edge(best) < maxEdge) {
            val detail = if (highResolution && sourceId != null) try {
                withTimeoutOrNull(8_000) {
                    if (backend.online.account.state.value != account) null
                    else backend.online.detail(sourceId).takeIf { backend.online.account.state.value == account }
                }.also { if (it == null) retryUpgrade = true }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { retryUpgrade = true; null }
            else null
            if (detail != null) {
                consider(detail.work.coverUrl?.takeUnless { it == referenceUri }?.let { attempt { readUri(context, it, maxEdge, owner) } })
                if (edge(best) < maxEdge) scan(detail.files.asSequence()
                    .filter { it.kind == "image" && it.path !in downloadedPaths && coverComparisonCandidate(it.path) }
                    .mapNotNull { file ->
                        val url = file.downloadUrl ?: file.streamUrl ?: return@mapNotNull null
                        val id = LibraryIndex.id(workId, "asset", file.id, file.path)
                        Candidate(file.path, file.bytes) {
                            CoverDiskCache.read(context, "$owner:${account.cacheScope}:$id:${file.bytes}", maxEdge) { url }
                        }
                    }.toList())
            } else {
                val cachedOnly = highResolution && sourceId != null
                scan(remote.filterNot { it.relativePath in downloadedPaths }.map { asset -> Candidate(asset.relativePath, asset.byteSize) {
                    CoverDiskCache.read(context, "$owner:${backend.online.account.cacheScope(asset.origin)}:${asset.id}:${asset.byteSize}", maxEdge, cachedOnly) {
                        backend.online.provider.resolve(asset).url
                    }
                } })
            }
        }
        if (highResolution || best == null) consider(referenceCover())
        return Selection(best, highResolution && retryUpgrade && edge(best) < maxEdge)
    }

}
