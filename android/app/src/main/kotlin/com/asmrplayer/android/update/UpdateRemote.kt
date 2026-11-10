package com.asmrplayer.android.update

import com.asmrplayer.android.online.HttpTransport
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import java.io.File

internal interface UpdateRemote {
    suspend fun manifest(): String
    suspend fun download(release: UpdateRelease, destination: File, progress: (Long) -> Unit)
}

internal class HttpUpdateRemote(
    private val transport: HttpTransport = HttpTransport(),
    private val endpoint: String = UpdateManifest.ENDPOINT,
) : UpdateRemote {
    override suspend fun manifest(): String = transport.text(endpoint)
    override suspend fun download(release: UpdateRelease, destination: File, progress: (Long) -> Unit) {
        transport.response(release.url) { connection ->
            require(connection.responseCode == 200) { "APK 下载失败：HTTP ${connection.responseCode}" }
            val expected = connection.getHeaderField("Content-Length")?.toLongOrNull()
            require(expected == null || expected == release.size) { "下载长度不匹配" }
            destination.outputStream().use { output ->
                connection.inputStream.use { input ->
                    val buffer = ByteArray(64 * 1024); var total = 0L
                    while (true) {
                        currentCoroutineContext().ensureActive()
                        val count = input.read(buffer); if (count < 0) break
                        total += count; require(total <= release.size) { "APK 超出预期大小" }
                        output.write(buffer, 0, count); progress(total)
                    }
                    require(total == release.size) { "APK 下载不完整" }
                }
                output.fd.sync()
            }
        }
    }
}
