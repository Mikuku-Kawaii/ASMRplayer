package com.asmrplayer.android.online

import androidx.activity.compose.setContent
import com.asmrplayer.android.ui.PlayerApp
import android.content.ComponentName
import android.graphics.Bitmap
import android.net.Uri
import android.os.*
import android.view.*
import androidx.compose.ui.geometry.Offset
import android.content.Intent
import android.graphics.Rect
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.*
import androidx.room.withTransaction
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.*
import com.asmrplayer.android.playback.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import androidx.lifecycle.ViewModelProvider
import com.asmrplayer.android.PlayerViewModel
import org.junit.*
import java.io.File
import java.util.Locale
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Explicit device benchmark. Uses only a separately installed app, generated silence and loopback HTTP. */
@UnstableApi
class OperationPerformanceTest {
    private fun <T> main(block: ()->T): T {
        val result = java.util.concurrent.FutureTask(block)
        inst.runOnMainSync(result)
        return result.get()
    }
    private fun waitFor(ms: Long, condition: ()->Boolean) {
        val end=SystemClock.elapsedRealtime()+ms
        while (SystemClock.elapsedRealtime()<end) { if(condition()) return; SystemClock.sleep(60) }
        check(condition()) { "Timed out waiting for device UI" }
    }
    private fun bounds(label:String, partial:Boolean=false, bottom:Boolean=false): Rect {
        val matches=mutableListOf<Rect>()
        fun visit(n:AccessibilityNodeInfo) {
            val labels=listOf(n.text?.toString().orEmpty(),n.contentDescription?.toString().orEmpty())
            if(n.isVisibleToUser && labels.any{if(partial) it.contains(label) else it==label}) {
                val r=Rect();n.getBoundsInScreen(r);if(!r.isEmpty)matches+=r
            }
            for(i in 0 until n.childCount)n.getChild(i)?.let(::visit)
        }
        waitFor(10000){inst.uiAutomation.rootInActiveWindow?.let(::visit);matches.isNotEmpty()}
        return if(bottom)matches.maxBy{it.centerY()} else matches.first()
    }
    private fun transcriptBounds(): Rect {
        // OEM accessibility trees can omit the scroll container. Visible cue rows still bound
        // the drag surface on both builds. Locate them together in a single window snapshot.
        var region: Rect? = null
        waitFor(10000) {
            val visible = Rect()
            var rows = 0
            fun find(node: AccessibilityNodeInfo) {
                if (node.isVisibleToUser && node.text?.contains("合成台本") == true) {
                    val rect = Rect().also { node.getBoundsInScreen(it) }
                    if (!rect.isEmpty) { visible.union(rect); rows++ }
                }
                for (i in 0 until node.childCount) node.getChild(i)?.let(::find)
            }
            inst.uiAutomation.rootInActiveWindow?.let(::find)
            if (rows >= 2 && visible.height() > 100) region = visible
            region != null
        }
        return requireNotNull(region)
    }
    private fun center(r:Rect)=Offset(r.exactCenterX(),r.exactCenterY())
    private val inst = InstrumentationRegistry.getInstrumentation()
    private val context = inst.targetContext
    private val backend get() = AndroidBackend.get(context)

    private val downs = mutableListOf<Long>()
    private fun touch(action: Int, x: Float, y: Float, down: Long) {
        if (action == MotionEvent.ACTION_DOWN) downs += System.nanoTime()
        val e = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, x, y, 0).apply { source = InputDevice.SOURCE_TOUCHSCREEN }
        try { check(inst.uiAutomation.injectInputEvent(e, true)) } finally { e.recycle() }
    }
    private fun tap(point: Offset) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, point.x, point.y, down); touch(MotionEvent.ACTION_UP, point.x, point.y, down)
    }
    private fun drag(a: Offset, b: Offset) {
        val down = SystemClock.uptimeMillis()
        touch(MotionEvent.ACTION_DOWN, a.x, a.y, down)
        for (i in 1..14) {
            SystemClock.sleep(12)
            val p = a + (b - a) * (i / 14f)
            touch(MotionEvent.ACTION_MOVE, p.x, p.y, down)
        }
        SystemClock.sleep(80)
        touch(MotionEvent.ACTION_UP, b.x, b.y, down)
    }

    private suspend fun seed(server: OnlineFixtureServer): String {
        val db = backend.database
        backend.online.account.setApiBase(server.base)
        val imported = backend.online.importWork("123456")
        val version = imported.versionIds.single()
        backend.library.works.first().filter { it.id != imported.workId && it.title == "Synthetic online work" && it.inLibrary }
            .forEach { backend.library.setMembership(it.id,false) }
        val cover = File(context.filesDir, "frame-cover.png")
        val bitmap = Bitmap.createBitmap(128, 96, Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(0xff647aa4.toInt())
        cover.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        db.withTransaction {
            val dao = db.library()
            for (i in 0 until 160) {
                val id = "frame-$i"
                dao.putWork(LibraryWork(id, null, "", "作品 " + i.toString().padStart(3,'0'), "RJ$i", Uri.fromFile(cover).toString(), i % 2 == 0, "SAFE"))
                db.metadata().put(CatalogMetadata(id, "合成资料", """["声音甲","声音乙"]""", """["雨声","晚安","耳语","环境音","轻声"]"""))
                for (j in 0..2) {
                    val track = "$id-$j"
                    dao.putTrack(LibraryTrack(track,id,"音轨 $j","WAV")); dao.putVersion(TrackVersion(track,track,"wav"))
                }
            }
            for (lang in listOf("original","translation")) {
                val sub = File(context.filesDir, "$lang.lrc")
                sub.bufferedWriter().use { out -> for (i in 0 until 1200) {
                    val ms = i * 200
                    out.write(String.format(Locale.ROOT,"[%02d:%02d.%03d]合成台本 %s 第 %d 句，用于长时间轴定位。%n",ms/60000,ms/1000%60,ms%1000,lang,i))
                } }
                dao.putSubtitle(SubtitleSource(lang, version, null, sub.name, Uri.fromFile(sub).toString(),sub.name,"lrc"))
            }
            dao.putSubtitlePreference(SubtitlePreference(version,"original","translation"))
        }
        return version
    }

    @Test fun measureThreeWorkloads() {
        Assume.assumeTrue(Build.VERSION.SDK_INT >= 31 && InstrumentationRegistry.getArguments().getString("perf") == "true")
        check(context.packageName == "com.asmrplayer.android.perf.uitest")
        val report = File(context.filesDir, "operation-frames.log").apply { writeText("") }
        val server = OnlineFixtureServer(durationSeconds=300,sampleRate=8000,port=35461).apply { attachments = true }
        val thread = HandlerThread("frame-samples").apply { start() }
        context.startActivity(Intent(context,MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        fun activity() = main { ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull() }
        waitFor(12000){activity()!=null}
        val window = requireNotNull(activity()).window
        data class Frame(val first:Long,val second:Long,val third:Long,val gpu:Long, val drop:Int)
        val samples = mutableListOf<Frame>()
        val listener = Window.OnFrameMetricsAvailableListener { _,m,dropped ->
            synchronized(samples) { samples += Frame(m.getMetric(FrameMetrics.INTENDED_VSYNC_TIMESTAMP),
                m.getMetric(FrameMetrics.TOTAL_DURATION),m.getMetric(FrameMetrics.DEADLINE),m.getMetric(FrameMetrics.GPU_DURATION),dropped) }
        }
        val future = main {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            window.addOnFrameMetricsAvailableListener(listener,Handler(thread.looper))
            MediaController.Builder(context,SessionToken(context,ComponentName(context,PlaybackService::class.java))).buildAsync()
        }
        val p = future.get(15,TimeUnit.SECONDS)
        val lost = AtomicInteger()
        val playbackListener = object: Player.Listener { override fun onIsPlayingChanged(isPlaying:Boolean) { if(!isPlaying) lost.incrementAndGet() } }
        fun measure(name:String, round:Int, action:()->Unit) {
            val before = listOf(CacheMetrics.streamOpens.get(),CacheMetrics.imageFetches.get(),CacheMetrics.imageDecodes.get(),CacheMetrics.bufferingMs.get(),lost.get().toLong())
            downs.clear()
            val start = System.nanoTime()
            action()
            val end = System.nanoTime()
            SystemClock.sleep(150)
            val frames = synchronized(samples) { samples.filter { it.first in start..end && it.second>0 && it.third>0 }.also { samples.clear() } }
            check(frames.isNotEmpty()) { "No frames: $name" }
            val times = frames.map { it.second }.sorted()
            fun pct(q:Double) = times[kotlin.math.ceil(q*times.size).toInt()-1]/1e6
            var streak=0;var longest=0
            frames.forEach { streak=if(it.second>it.third) streak+1 else 0;longest=maxOf(streak,longest) }
            val after=listOf(CacheMetrics.streamOpens.get(),CacheMetrics.imageFetches.get(),CacheMetrics.imageDecodes.get(),CacheMetrics.bufferingMs.get(),lost.get().toLong())
            val firstFrames = downs.mapNotNull { at -> frames.firstOrNull { it.first >= at }?.let { (it.first + it.second - at)/1e6 } }.sorted()
            val gpu = frames.map { it.gpu/1e6 }.sorted()
            val deadline=frames.map {it.third/1e6}.sorted()[frames.size/2]
            val line=String.format(Locale.ROOT,"scenario=%s round=%d n=%d p50=%.2f p95=%.2f p99=%.2f late=%d streak=%d deadline=%.2f gpu95=%.2f first50=%.2f drops=%d delta=%s",name,round,frames.size,pct(.5),pct(.95),pct(.99),frames.count{it.second>it.third},longest,deadline,gpu[(gpu.size*.95).toInt().coerceAtMost(gpu.lastIndex)],firstFrames.getOrElse(firstFrames.size/2){0.0},frames.sumOf{it.drop},after.zip(before){a,b->a-b}.joinToString(","))
            report.appendText(line + "\n"); android.util.Log.i("AsmrPerf",line)
            if (name in setOf("tabs", "library", "lyricScroll")) {
                check(before == after) { "Unexpected media restart, image reload or buffering during $name" }
            }
        }
        try {
            val version=runBlocking {seed(server)}
            main { LibraryPlayback.play(p,listOf(version),resume=false) }.get(12,TimeUnit.SECONDS)
            waitFor(15000) {main{p.isPlaying && p.currentPosition>300}}
            main {p.addListener(playbackListener)}
            val tabs=listOf("音频库","发现","喜欢","队列").map {center(bounds(it,bottom=true))}
            val args = InstrumentationRegistry.getArguments()
            if (args.getString("refreshCheck") == "true") {
                val model = main { ViewModelProvider(requireNotNull(ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()))[PlayerViewModel::class.java] }
                for (index in tabs.indices) {
                    tap(tabs[index]);SystemClock.sleep(850)
                    val flow = when(index) {
                        0 -> model.collections.state.map { it.refreshing }
                        1 -> model.online.state.map { it.loading }
                        2 -> model.collections.refreshingFavorites
                        else -> model.queueGroups.state.map { it.refreshing }
                    }
                    runBlocking {
                        withTimeout(10000) { flow.first { !it } }
                        // Input injection blocks this thread. Watch short Room refreshes on a separate dispatcher.
                        val done = async(Dispatchers.Default, start=CoroutineStart.UNDISPATCHED) {
                            withTimeout(8000) { flow.dropWhile { !it }.first { !it } }
                        }
                        val r=Rect().also { main {window.decorView.getWindowVisibleDisplayFrame(it)} }
                        val x=r.exactCenterX()
                        drag(Offset(x,r.top+r.height()*.36f),Offset(x,r.top+r.height()*.76f))
                        done.await()
                    }
                    report.appendText("refresh=$index completed\n")
                }
            }
            fun library(){tap(tabs[0]);SystemClock.sleep(600)}
            fun tabRun(){repeat(4){for(i in listOf(1,3,2,0)){tap(tabs[i]);SystemClock.sleep(125)}}; SystemClock.sleep(400)}
            tabs.forEach{tap(it);SystemClock.sleep(700)};library();tabRun()
            repeat(3){measure("tabs",it+1,::tabRun)}
            val rect=Rect().also { main {window.decorView.getWindowVisibleDisplayFrame(it)} }
            val a=Offset(rect.exactCenterX(),rect.top+rect.height()*.67f);val b=Offset(a.x,rect.top+rect.height()*.29f)
            fun scrollRun(){repeat(4){drag(a,b)};repeat(4){drag(b,a)};SystemClock.sleep(300)}
            scrollRun()
            repeat(3){measure("library",it+1,::scrollRun)}
            if (args.getString("glassCheck") == "true") {
                val host = requireNotNull(activity())
                val model = main { ViewModelProvider(host)[PlayerViewModel::class.java] }
                for (mode in listOf("blur","low","full")) {
                    main { model.setLowEffects(mode=="low");host.setContent { PlayerApp(model, refraction=mode!="blur") } }
                    SystemClock.sleep(700); scrollRun()
                    repeat(3){measure("glass-$mode",it+1,::scrollRun)}
                }
            }
            check(main {p.isPlaying && p.currentMediaItem?.mediaId==version})
            tap(center(bounds("正在播放 ·",partial=true)))
            tap(center(bounds("点击封面展开台本")))
            bounds("合成台本",partial=true)
            val slider=bounds("播放进度",partial=true)
            fun scrubRun(){for((u,v) in listOf(.15f to .7f,.7f to .3f,.3f to .65f,.65f to .1f)){
                drag(Offset(slider.left+slider.width()*u,slider.exactCenterY()),Offset(slider.left+slider.width()*v,slider.exactCenterY()));SystemClock.sleep(350)
            }}
            val stage = transcriptBounds()
            val textTop = Offset(stage.exactCenterX(), stage.top + stage.height() * .27f)
            val textBottom = Offset(textTop.x, stage.top + stage.height() * .76f)
            fun scrollTranscript() {
                repeat(3) { drag(textBottom, textTop) }
                repeat(3) { drag(textTop, textBottom) }
            }
            scrollTranscript()
            repeat(3) { measure("lyricScroll", it + 1, ::scrollTranscript) }
            scrubRun()
            repeat(3){measure("lyrics",it+1,::scrubRun)}
            if (args.getString("soak") == "true") {
                fun memory(): String {
                    val info=Debug.MemoryInfo();Debug.getMemoryInfo(info)
                    return "pssKiB=${info.totalPss} heapBytes=${Runtime.getRuntime().totalMemory()-Runtime.getRuntime().freeMemory()} fd=${File("/proc/self/fd").list()?.size}"
                }
                main { p.repeatMode=Player.REPEAT_MODE_ONE }
                val initialLost=lost.get()
                val start=SystemClock.elapsedRealtime()
                report.appendText("soak-start "+memory()+"\n")
                var loops=0
                while(SystemClock.elapsedRealtime()-start<20*60_000L) {
                    check(main {p.isPlaying && p.currentMediaItem?.mediaId==version && p.playerError==null})
                    SystemClock.sleep(10_000)
                    loops++
                    if (loops % 3 == 0) { drag(textBottom, textTop); drag(textTop, textBottom) }
                    if(loops%12==0) report.appendText("soak minute=${(SystemClock.elapsedRealtime()-start)/60000} "+memory()+"\n")
                    synchronized(samples){samples.clear()}
                }
                report.appendText("soak-end interruptions=${lost.get()-initialLost} "+memory()+"\n")
                measure("lyricsAfterSoak",1,::scrubRun)
            }
            check(main {p.isPlaying && p.currentMediaItem?.mediaId==version})
        } finally {
            main {p.removeListener(playbackListener);p.stop();p.clearMediaItems();MediaController.releaseFuture(future)
                window.removeOnFrameMetricsAvailableListener(listener);window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)}
            thread.quitSafely();server.close()
        }
    }
}
