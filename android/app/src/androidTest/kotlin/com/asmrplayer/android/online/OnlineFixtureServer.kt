package com.asmrplayer.android.online

import java.io.Closeable
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicInteger
import kotlin.concurrent.thread

/** Loopback-only synthetic API/audio. No account, network service or user media is accessed. */
class OnlineFixtureServer(durationSeconds: Int = 3, sampleRate: Int = 48000, port: Int = 0) : Closeable {
    data class Request(val path: String, val headers: Map<String, String>, val method:String="GET")
    private val socket = ServerSocket(port, 16, InetAddress.getByName("127.0.0.1"))
    val base = "http://127.0.0.1:${socket.localPort}"
    val requests = ConcurrentLinkedQueue<Request>()
    val treeRequests = AtomicInteger()
    val mediaRequests = AtomicInteger()
    @Volatile var mode = "normal"
    @Volatile var redirect: String? = null
    @Volatile var attachments = false
    @Volatile var secondTrack = false
    @Volatile var hideAttachments = false
    @Volatile var invalidAttachment = false
    @Volatile var requireWebMetadataRequest = false
    val subtitleBytes = (1..12_000).joinToString("\n\n") { "$it\n00:00:00,000 --> 00:00:01,000\nSynthetic subtitle $it" }.toByteArray()
    val imageBytes: ByteArray = java.io.ByteArrayOutputStream().use { out ->
        val bitmap = android.graphics.Bitmap.createBitmap(16, 16, android.graphics.Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(android.graphics.Color.BLUE)
        bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, out); bitmap.recycle(); out.toByteArray()
    }
    val bytes: ByteArray = ByteBuffer.allocate(44 + sampleRate * 4 * durationSeconds).order(ByteOrder.LITTLE_ENDIAN).apply {
        put("RIFF".toByteArray()).putInt(capacity() - 8).put("WAVEfmt ".toByteArray()).putInt(16)
            .putShort(1).putShort(2).putInt(sampleRate).putInt(sampleRate * 4).putShort(4).putShort(16)
            .put("data".toByteArray()).putInt(capacity() - 44)
    }.array()
    private val worker = thread(isDaemon = true) {
        try {
            while (!socket.isClosed) {
                val client = socket.accept()
                thread(isDaemon = true) { client.use { peer ->
                    try {
                        val reader = peer.getInputStream().bufferedReader()
                        val requestLine=reader.readLine()?.split(' ')?:return@use
                        val target=requestLine.getOrNull(1)?:return@use
                        val headers = mutableMapOf<String, String>()
                        while (true) {
                            val line = reader.readLine() ?: break
                            if (line.isEmpty()) break
                            headers[line.substringBefore(':').lowercase()] = line.substringAfter(':').trim()
                        }
                        if(requestLine[0]=="POST") {
                            val count=headers["content-length"]?.toIntOrNull()?:0
                            var remaining=count
                            while(remaining>0){val buffer=CharArray(remaining);val n=reader.read(buffer);if(n<0)break;remaining-=n}
                        }
                        requests.add(Request(target, headers,requestLine[0]))
                        var code = 200; val responseHeaders = linkedMapOf<String, String>(); var body: ByteArray
                        val nextRedirect = redirect
                        when {
                            nextRedirect != null -> { code = 302; responseHeaders["Location"] = nextRedirect; body = byteArrayOf() }
                            mode == "unauthorized" -> { code = 401; body = byteArrayOf() }
                            target.startsWith("/api/workInfo/") -> {
                                if(mode=="metadata-unavailable" || (requireWebMetadataRequest && (target!="/api/workInfo/123456" ||
                                    headers["origin"]!="https://asmr.one" || headers["referer"]!="https://asmr.one/" ||
                                    headers["accept"]!="application/json, text/plain, */*"))) {
                                    code=504; body=byteArrayOf()
                                } else body = work().toByteArray()
                            }
                            target.startsWith("/api/tracks/") -> {
                                val number = treeRequests.incrementAndGet()
                                val extra = if (attachments && !hideAttachments) """,{"type":"text","title":"01.srt","hash":"fixture-subtitle","size":${subtitleBytes.size},"mediaDownloadUrl":"$base/media/subtitle?signature=$number"},{"type":"image","title":"art.png","hash":"fixture-image","size":${imageBytes.size},"mediaDownloadUrl":"$base/media/image?signature=$number"}""" else ""
                                val second = if (secondTrack) """,{"type":"audio","title":"02.wav","hash":"fixture-02","size":${bytes.size},"mediaStreamUrl":"$base/media?signature=$number"}""" else ""
                                body = """[{"type":"folder","title":"WAV","children":[{"type":"audio","title":"01.wav","hash":"fixture-01","size":${bytes.size},"mediaStreamUrl":"$base/media?signature=$number","mediaDownloadUrl":"$base/media?signature=$number"}$second$extra]}]""".toByteArray()
                            }
                            target.startsWith("/api/recommender/popular") && mode == "popular-unavailable" -> { code = 503; body = byteArrayOf() }
                            target.startsWith("/api/recommender/popular") || target.startsWith("/api/search/") || target.startsWith("/api/works") -> body = """{"works":[${work()},{"id":123457,"title":"Adult fixture","age_category_string":"r18"}],"pagination":{"currentPage":1,"pageSize":20,"totalCount":2}}""".toByteArray()
                            else -> {
                                val count = mediaRequests.incrementAndGet()
                                if (mode == "expired" && count == 1) { code = 403; body = byteArrayOf() }
                                else {
                                    val payload = when {
                                        target.startsWith("/media/subtitle") -> subtitleBytes
                                        target.startsWith("/media/image") || target.startsWith("/media/cover") -> imageBytes
                                        else -> bytes
                                    }.let { if (invalidAttachment && target.startsWith("/media/")) "<html>not an attachment</html>".toByteArray().copyOf(it.size) else it }
                                    val offset = headers["range"]?.substringAfter("bytes=")?.substringBefore('-')?.toInt() ?: 0
                                    responseHeaders["Content-Type"] = if (target.startsWith("/media/")) "application/octet-stream" else "audio/wav"
                                    responseHeaders["ETag"] = "\"fixture-v1\""
                                    when {
                                        mode == "ignore-range" || !headers.containsKey("range") -> body = payload
                                        offset >= payload.size -> { code = 416; responseHeaders["Content-Range"] = "bytes */${payload.size}"; body = byteArrayOf() }
                                        else -> {
                                            val end = if (mode == "capped-range") minOf(offset + 65536, payload.size) else payload.size
                                            code = 206; body = payload.copyOfRange(offset, end)
                                            responseHeaders["Content-Range"] = "bytes ${if (mode == "bad-range") 0 else offset}-${end - 1}/${payload.size}"
                                        }
                                    }
                                }
                            }
                        }
                        val out = peer.getOutputStream()
                        val preamble = buildString {
                            append("HTTP/1.1 $code Fixture\r\nContent-Length: ${body.size}\r\nConnection: close\r\n")
                            responseHeaders.forEach { (k, v) -> append("$k: $v\r\n") }; append("\r\n")
                        }
                        out.write(preamble.toByteArray())
                        if (target.startsWith("/media") && mode == "drop") out.write(body, 0, minOf(32768, body.size))
                        else if (target.startsWith("/media") && mode == "slow") {
                            var offset = 0
                            while (offset < body.size) { val size = minOf(4096, body.size - offset); out.write(body, offset, size); out.flush(); offset += size; Thread.sleep(10) }
                        } else out.write(body)
                        out.flush()
                    } catch (_: java.io.IOException) { /* Cancellation closes the synthetic peer. */ }
                } }
            }
        } catch (e: SocketException) { if (!socket.isClosed) throw e }
    }
    private fun work(): String {
        val cover = if (attachments && !hideAttachments) """, "mainCoverUrl":"$base/media/cover?signature=${treeRequests.get()}"""" else ""
        return """{"id":123456,"title":"Synthetic online work","circle":{"name":"Fixture"},"vas":[{"name":"Voice"}],"tags":[{"name":"General"}],"age_category_string":"general"$cover}"""
    }
    override fun close() { socket.close(); worker.join(2000) }
}
