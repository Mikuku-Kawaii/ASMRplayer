package com.asmrplayer.android.playback

import com.asmrplayer.android.data.CacheMetrics
import android.content.Context
import android.net.Uri
import androidx.media3.common.util.UnstableApi
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.*
import androidx.media3.datasource.cache.Cache
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.library.LibraryIndex
import kotlinx.coroutines.runBlocking
import java.io.File
import java.io.IOException

/** Only cache misses reach this source. A signed URL never becomes a disk key or redirect record. */
@UnstableApi
private class StreamUpstream(private val backend: AndroidBackend, private val http: DataSource.Factory) : DataSource {
    private val listeners = mutableListOf<TransferListener>()
    private var source: DataSource? = null
    private var stableUri: Uri? = null
    override fun addTransferListener(listener: TransferListener) { listeners.add(listener) }

    override fun open(dataSpec: DataSpec): Long {
        val versioned = dataSpec.uri.scheme == "asmr"
        stableUri = dataSpec.uri
        repeat(2) { attempt ->
            val target = if (versioned) {
                val id = requireNotNull(dataSpec.uri.lastPathSegment)
                Uri.parse(runBlocking { backend.online.resolveStream(id, refresh = attempt != 0) }.uri)
            } else dataSpec.uri
            val next = http.createDataSource()
            listeners.forEach(next::addTransferListener)
            source = next
            try {
                return next.open(dataSpec.withUri(target))
            } catch (e: HttpDataSource.InvalidResponseCodeException) {
                next.close()
                source = null
                if (!versioned || attempt != 0 || e.responseCode !in setOf(401, 403)) throw e
            }
        }
        error("unreachable")
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        val count = requireNotNull(source).read(buffer, offset, length)
        if (count > 0) CacheMetrics.networkBytes.addAndGet(count.toLong())
        return count
    }
    override fun getUri(): Uri? = stableUri
    override fun getResponseHeaders() = source?.responseHeaders.orEmpty()
    override fun close() { try { source?.close() } finally { stableUri = null; source = null } }
}

/** Keep local source preference at open time. Remote byte ranges share an application-owned store. */
@UnstableApi
internal class VersionDataSource private constructor(
    private val backend: AndroidBackend, private val local: DataSource.Factory,
    private val upstream: DataSource.Factory, private val cached: () -> DataSource,
) : DataSource {
    private val listeners = mutableListOf<TransferListener>()
    private var source: DataSource? = null
    private var onlineSpec: DataSpec? = null
    private var bytesRead = 0L
    override fun addTransferListener(listener: TransferListener) { listeners.add(listener) }

    private fun attach(next: DataSource): DataSource {
        listeners.forEach(next::addTransferListener)
        source = next
        return next
    }

    override fun open(dataSpec: DataSpec): Long {
        if (dataSpec.uri.scheme == "https" &&
            dataSpec.uri.lastPathSegment?.substringAfterLast('.')?.lowercase() in LibraryIndex.audioExtensions) {
            val finite = dataSpec.buildUpon().setFlags(dataSpec.flags or DataSpec.FLAG_DONT_CACHE_IF_LENGTH_UNKNOWN).build()
            return openCached(finite, LibraryIndex.id(directSession, dataSpec.uri.toString()))
        }
        if (dataSpec.uri.scheme != "asmr") return attach(local.createDataSource()).open(dataSpec)
        require(dataSpec.uri.host == "version" && dataSpec.uri.pathSegments.size == 1)
        val id = requireNotNull(dataSpec.uri.lastPathSegment)
        val resolved = runBlocking { backend.library.resolveSource(id, deferOnline = true) }
        val uri = Uri.parse(resolved.uri)
        if (uri.scheme != "asmr") return attach(local.createDataSource()).open(dataSpec.withUri(uri))

        val key = runBlocking {
            val dao = backend.database.library()
            val remote = requireNotNull(dao.remote(id))
            val sourceInfo = requireNotNull(dao.source(resolved.sourceId))
            LibraryIndex.id(id, remote.origin, backend.online.account.cacheScope(remote.origin), remote.remoteFileId,
                remote.format, remote.byteSize.toString(), sourceInfo.modifiedMs.toString())
        }
        return openCached(dataSpec, key)
    }

    private fun openCached(dataSpec: DataSpec, key: String): Long {
        CacheMetrics.streamOpens.incrementAndGet()
        val spec = dataSpec.buildUpon().setKey(key)
            .setFlags(dataSpec.flags or DataSpec.FLAG_ALLOW_CACHE_FRAGMENTATION).build()
        onlineSpec = spec
        bytesRead = 0
        return try { attach(cached()).open(spec) }
        catch (e: Cache.CacheException) { bypassCache(spec) }
    }

    private fun bypassCache(spec: DataSpec): Long {
        onlineSpec = null
        try { source?.close() } catch (_: IOException) { /* Failed cache write. */ }
        return attach(upstream.createDataSource()).open(spec.subrange(bytesRead))
    }

    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        val count = try { requireNotNull(source).read(buffer, offset, length) }
        catch (e: Cache.CacheException) {
            bypassCache(onlineSpec ?: throw e)
            requireNotNull(source).read(buffer, offset, length)
        }
        if (count > 0) bytesRead += count
        return count
    }

    override fun getUri(): Uri? = source?.uri
    override fun getResponseHeaders() = source?.responseHeaders.orEmpty()
    override fun close() {
        try { source?.close() } catch (_: Cache.CacheException) { /* Optional cache commit failed. */ }
        finally { source = null; onlineSpec = null; bytesRead = 0 }
    }

    class Factory(context: Context, private val backend: AndroidBackend) : DataSource.Factory {
        private val app = context.applicationContext
        private val http = DefaultHttpDataSource.Factory().setUserAgent("ASMRPlayer-Android/0.1")
            .setConnectTimeoutMs(15_000).setReadTimeoutMs(20_000)
        private val local = DefaultDataSource.Factory(app, http)
        private val upstream = DataSource.Factory { StreamUpstream(backend, http) }
        private val cached by lazy {
            CacheDataSource.Factory().setCache(store(app)).setUpstreamDataSourceFactory(upstream)
                .setEventListener(object : CacheDataSource.EventListener {
                    override fun onCachedBytesRead(cacheSizeBytes: Long, cachedBytesRead: Long) {
                        CacheMetrics.diskAudioBytes.addAndGet(cachedBytesRead)
                    }
                    override fun onCacheIgnored(reason: Int) = Unit
                })
                .setFlags(CacheDataSource.FLAG_IGNORE_CACHE_ON_ERROR)
        }
        override fun createDataSource(): DataSource = VersionDataSource(backend, local, upstream) { cached.createDataSource() }
    }

    private companion object {
        private val directSession = java.util.UUID.randomUUID().toString()
        private var cache: SimpleCache? = null
        @Synchronized fun store(context: Context): SimpleCache = cache ?: SimpleCache(
            File(context.cacheDir, "audio-spans-v1"), LeastRecentlyUsedCacheEvictor(minOf(512L * 1024 * 1024, context.cacheDir.usableSpace / 8)),
            StandaloneDatabaseProvider(context)
        ).also { cache = it }
    }
}
