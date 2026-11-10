package com.asmrplayer.android.artwork

import com.asmrplayer.android.data.CacheMetrics
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import com.asmrplayer.android.data.AndroidBackend
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
    private val cache = object : LruCache<Key, Bitmap>(32 * 1024 * 1024) {
        override fun sizeOf(key: Key, value: Bitmap) = value.allocationByteCount
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
    fun preview(request: Request): Bitmap? = listOf(768, 384, 1536, 3072).firstNotNullOfOrNull {
        cache.get(request.key.copy(edge = it))
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
            val bitmap = try {
                if (key.edge > 1024) largeImageSlot.withLock { slots.withPermit { select(context, request) } }
                else slots.withPermit { select(context, request) }
            } catch (e: CancellationException) { throw e }
            catch (_: Exception) { null }
            if (bitmap == null) failures.put(key, android.os.SystemClock.elapsedRealtime() + 20_000)
            else if (bitmap.allocationByteCount <= cache.maxSize()) cache.put(key, bitmap)
            bitmap
        }
    }

    private suspend fun select(context: Context, request: Request): Bitmap? {
        val key = request.key
        val owner = key.workId?.let {
            val epoch = context.getSharedPreferences("artwork-revisions", Context.MODE_PRIVATE).getLong(it, 0)
            "$it:${key.scope}:$epoch"
        }
        val original = key.workId?.let {
            try { loadOriginal(context, it, key.edge, request.address, key.revision, requireNotNull(owner)) }
            catch (e: CancellationException) { throw e }
            catch (_: Exception) { null }
        }
        if (original != null) return original
        if (request.address != null) return readUri(context, request.address, key.edge, owner)
        // A sync-created work has a provider identity even before its full metadata is available.
        val backend = AndroidBackend.get(context)
        val meta = key.workId?.let { backend.database.metadata().forWork(it) } ?: return null
        val account = backend.online.account.state.value
        if (meta.origin != account.apiBase || "${account.apiBase}:${account.cacheScope}" != key.scope ||
            !meta.sourceId.matches(Regex("[0-9]{1,12}"))) return null
        return readUri(context, "${meta.origin}/api/cover/${meta.sourceId}.jpg?type=main", key.edge, owner)
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
    /** Resolve attachment identities on demand; do not persist expiring media URLs. */
    private suspend fun loadOriginal(context: Context, workId: String, maxEdge:Int, referenceUri:String?, revision:Long, owner: String): Bitmap? = withContext(Dispatchers.IO) {
        val backend=AndroidBackend.get(context)
        val dao=backend.database.library()
        val local=dao.observeAssets(workId).first().filter { it.available && coverComparisonCandidate(it.relativePath) }
        val remote=dao.observeRemoteAssets(workId).first().filter { it.available && it.kind=="image" && coverComparisonCandidate(it.relativePath) }
        val all=(local.map{it.relativePath}+remote.map{it.relativePath}).distinct()
        val named=all.filter{originalCoverPriority(it)>0}.sortedWith(compareByDescending<String>(::originalCoverPriority).thenBy{it})
        val matchKey="$workId:$revision:$referenceUri"
        val previous=matchedPaths.get(matchKey)?.takeIf{it in all}
        // Bound discovery of unnamed bonus images instead of downloading an entire illustration collection.
        val paths=(listOfNotNull(previous)+named+all.filter{it !in named}.sorted().take(4)).distinct()
        var reference:Bitmap?=null;var referenceLoaded=false
        var best:Bitmap?=null
        for(path in paths) {
            currentCoroutineContext().ensureActive()
            val localAsset=local.firstOrNull{it.relativePath==path}
            val remoteAsset=remote.firstOrNull{it.relativePath==path}
            var bitmap = localAsset?.let {
                try { readUri(context, it.uri, maxEdge) }
                catch (e: CancellationException) { throw e }
                catch (_: Exception) { null }
            }
            if (bitmap == null && remoteAsset != null) {
                try {
                    bitmap = CoverDiskCache.read(context, "$owner:${backend.online.account.cacheScope(remoteAsset.origin)}:${remoteAsset.id}:${remoteAsset.byteSize}", maxEdge) {
                        backend.online.provider.resolve(remoteAsset).url
                    }
                } catch (e: CancellationException) { throw e }
                catch (_: Exception) { /* Continue with the remaining candidates. */ }
            }
            bitmap?.let {
                if(originalCoverPriority(path)==0&&path!=previous) {
                    if(!referenceLoaded) {
                        referenceLoaded=true
                        reference=referenceUri?.let{address->try{readUri(context,address,768,owner)}catch(e:CancellationException){throw e}catch(_:Exception){null}}
                    }
                    if(reference==null||!sameCover(reference!!,it))return@let
                    matchedPaths.put(matchKey,path)
                }
                if(maxOf(it.width,it.height)>=minOf(maxEdge,1024))return@withContext it
                if(best==null||it.width.toLong()*it.height>best!!.width.toLong()*best!!.height)best=it
            }
        }
        best
    }

}
