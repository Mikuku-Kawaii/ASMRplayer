package com.asmrplayer.android.online

import kotlinx.coroutines.*
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import java.io.ByteArrayOutputStream

internal class HttpStatusException(val status: Int) : java.io.IOException("HTTP $status")

/** Credentials are scoped to the initial origin. Redirects never receive credentials for a different origin. */
class HttpTransport(private val debugLoopback: Boolean = false) {
    fun validate(address: String): URI {
        val uri = URI(address)
        require((uri.scheme == "https" || (debugLoopback && uri.scheme == "http" && uri.host == "127.0.0.1")) &&
            !uri.host.isNullOrBlank() && uri.rawUserInfo == null && uri.fragment == null) { "需要 HTTPS 地址" }
        return uri
    }
    suspend fun <T> response(address: String, headers: Map<String, String> = emptyMap(), jsonBody: String? = null, block: suspend (HttpURLConnection) -> T): T = withContext(Dispatchers.IO) {
        val initial = validate(address)
        var next = initial
        repeat(6) { redirect ->
            currentCoroutineContext().ensureActive()
            val connection = next.toURL().openConnection() as HttpURLConnection
            connection.instanceFollowRedirects = false
            connection.connectTimeout = 15_000; connection.readTimeout = 20_000
            connection.setRequestProperty("Accept-Encoding", "identity")
            connection.setRequestProperty("User-Agent", "ASMRPlayer-Android/0.1")
            headers.forEach { (key, value) ->
                if (!key.equals("Authorization", true) || origin(initial) == origin(next)) connection.setRequestProperty(key, value)
            }
            val watcher = launch(Dispatchers.Default, start = CoroutineStart.UNDISPATCHED) {
                try { awaitCancellation() } finally { connection.disconnect() }
            }
            try {
                if(jsonBody!=null) {
                    require(origin(initial)==origin(next)) { "POST redirect changed origin" }
                    connection.requestMethod="POST";connection.doOutput=true
                    connection.setRequestProperty("Content-Type","application/json")
                    connection.outputStream.use {it.write(jsonBody.toByteArray(Charsets.UTF_8))}
                }
                val code = connection.responseCode
                if (code in setOf(301, 302, 303, 307, 308)) {
                    require(redirect < 5)
                    next = validate(next.resolve(connection.getHeaderField("Location") ?: error("无重定向地址")).toString())
                } else return@withContext block(connection)
            } finally { watcher.cancel(); connection.disconnect() }
        }
        error("重定向次数过多")
    }
    suspend fun text(address: String, headers: Map<String, String> = emptyMap(), jsonBody:String?=null): String = response(address, headers, jsonBody) { connection ->
        if (connection.responseCode !in 200..299) throw HttpStatusException(connection.responseCode)
        connection.inputStream.use { stream ->
            val out = ByteArrayOutputStream(); val buffer = ByteArray(8192)
            while (true) {
                currentCoroutineContext().ensureActive()
                val size = stream.read(buffer); if (size < 0) break
                require(out.size() + size <= 16 * 1024 * 1024) { "响应过大" }
                out.write(buffer, 0, size)
            }
            out.toString("UTF-8")
        }
    }
    private fun origin(uri: URI) = Triple(uri.scheme, uri.host, uri.port)
}
