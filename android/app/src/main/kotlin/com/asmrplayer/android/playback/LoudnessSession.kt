package com.asmrplayer.android.playback

import android.content.Context
import android.net.Uri
import android.os.Bundle
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.core.DspConfig
import com.asmrplayer.core.LoudnessMeasurement
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.Locale
import java.util.concurrent.atomic.AtomicReference

/** Service owns scans and effective per-version gain; preferences never contain a track's gain. */
@UnstableApi
internal class LoudnessSession(private val context: Context, private val player: Player,
    private val config: AtomicReference<DspConfig>, private val gain: AtomicReference<Double>) {
    private val scope = CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)
    private val mutex = Mutex()
    private var job: Job? = null
    @Volatile private var generation = 0
    private var key: Pair<String?, Boolean>? = null
    private var measured: LoudnessMeasurement? = null
    private var status = "开启后自动分析本地或已下载的当前音轨。"
    @Volatile private var progress = -1
    private var busy = false
    private val listener = object : Player.Listener {
        override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) { refresh() }
    }
    init { player.addListener(listener); refresh() }
    fun refresh() {
        val cfg = config.get()
        val next = player.currentMediaItem?.mediaId to (cfg.normalize && !cfg.bypass)
        if (key != next) { key = next; start(false) } else applyGain()
    }
    fun cancel() { generation++; job?.cancel(); busy = false; progress = -1; status = "分析已取消，可重新分析。" }
    fun analyze() { start(true) }
    private fun applyGain() {
        val cfg = config.get()
        gain.set(if (cfg.normalize && !cfg.bypass) measured?.gainDb(cfg.targetLufs, cfg.maxBoostDb) ?: 0.0 else 0.0)
    }
    private fun start(force: Boolean) {
        val token = ++generation; job?.cancel(); measured = null; gain.set(0.0); busy = false; progress = -1
        val id = player.currentMediaItem?.mediaId
        if (id == null) { status = "选择音轨后可分析整轨响度。"; return }
        if (!force && (!config.get().normalize || config.get().bypass)) { status = "音轨响度统一已关闭。"; return }
        status = "准备分析整轨响度…"; busy = true
        job = scope.launch {
            try {
                val result = withContext(Dispatchers.IO) { mutex.withLock {
                    val backend = AndroidBackend.get(context)
                    val sources = backend.database.library().sources(id)
                    check(sources.any { it.available && it.kind != "online_provider" }) { "local_required" }
                    val source = backend.library.resolve(id)
                    val uri = Uri.parse(source.uri)
                    check(uri.scheme in setOf("content", "file")) { "local_required" }
                    val row = requireNotNull(backend.database.library().source(source.sourceId))
                    val stamp = if (uri.scheme == "file") File(requireNotNull(uri.path)).let { "${it.length()}:${it.lastModified()}" } else "${row.size}:${row.modifiedMs}"
                    val fingerprint = digest("1|$id|${row.id}|${row.uri}|$stamp")
                    val folder = File(context.cacheDir, "loudness").apply { mkdirs() }
                    val file = File(folder, digest(id)+".json")
                    val cached = if (!force) read(file, fingerprint) else null
                    cached ?: LoudnessScanner(context).scan(uri) { if (generation == token) progress = it }.also { m ->
                        currentCoroutineContext().ensureActive()
                        val json = JSONObject().put("fingerprint", fingerprint).put("lufs", m.lufs ?: JSONObject.NULL).put("peak", m.peak).put("frames", m.frames)
                        file.writeText(json.toString())
                    }
                } }
                if (generation != token) return@launch
                measured = result; applyGain()
                status = if (result.lufs == null) "音轨过短或低于测量门限，保留原始增益。" else String.format(Locale.ROOT, "整轨响度 %.1f LUFS", result.lufs)
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                if (generation == token) status = if (e.message == "local_required") "此音轨尚未下载。下载后可分析并统一响度；实时均衡仍可使用。"
                    else "响度分析未完成，请检查文件授权或格式后重试。"
            } finally { if (generation == token) { busy = false; progress = -1 } }
        }
    }
    fun state() = Bundle().apply {
        putString("loudnessStatus", status); putBoolean("loudnessBusy", busy); putInt("loudnessProgress", progress)
        putDouble("normalizationGainDb", gain.get())
    }
    fun close() { player.removeListener(listener); generation++; scope.cancel(); gain.set(0.0) }
    private fun digest(text: String) = MessageDigest.getInstance("SHA-256").digest(text.toByteArray()).joinToString("") { "%02x".format(it) }
    private fun read(file: File, fingerprint: String): LoudnessMeasurement? = try {
        val o = JSONObject(file.readText())
        if (o.getString("fingerprint") != fingerprint) null else LoudnessMeasurement(
            if (o.isNull("lufs")) null else o.getDouble("lufs"), o.getDouble("peak"), o.getLong("frames")).takeIf {
            (it.lufs?.isFinite() != false) && it.peak.isFinite() && it.peak >= 0 && it.frames > 0
        }
    } catch (_: Exception) { null }
}
