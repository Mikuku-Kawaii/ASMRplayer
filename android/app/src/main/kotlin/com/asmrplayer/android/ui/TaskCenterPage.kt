package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import com.asmrplayer.android.data.AndroidBackend
import kotlinx.coroutines.*

private data class TaskRow(val id:String,val title:String,val category:String,val state:String,val detail:String,
    val progress:Float?=null,val busy:Boolean=false,val action:String?=null,val run:()->Unit={})
private val runningStates=setOf("QUEUED","DOWNLOADING","VERIFYING","RUNNING")

@UnstableApi
@Composable
internal fun TaskCenterPage(state:PlayerScreenState,model:PlayerViewModel,open:(String)->Unit,initialDownloads:Boolean=false) {
    var downloads by rememberSaveable(initialDownloads) {mutableStateOf(initialDownloads)}
    val holder=androidx.compose.runtime.saveable.rememberSaveableStateHolder()
    Column(Modifier.fillMaxSize()) {
        Box(Modifier.fillMaxWidth().padding(horizontal=16.dp,vertical=8.dp)) {
            PageTabs(downloads,listOf(false to "全部任务",true to "离线下载")){downloads=it}
        }
        Box(Modifier.weight(1f)) {
            holder.SaveableStateProvider(downloads) {
                if(downloads) {
                    val data by model.online.state.collectAsStateWithLifecycle()
                    DownloadPage(data,state,model,open)
                } else TaskOverview(state,model,open)
            }
        }
    }
}

@UnstableApi
@Composable
private fun TaskOverview(state:PlayerScreenState,model:PlayerViewModel,open:(String)->Unit) {
    val data by model.online.state.collectAsStateWithLifecycle()
    val library by model.tools.state.collectAsStateWithLifecycle()
    val sync by model.syncStatus.collectAsStateWithLifecycle()
    val context=androidx.compose.ui.platform.LocalContext.current
    val scope=rememberCoroutineScope()
    var syncing by remember {mutableStateOf(false)}
    var filter by rememberSaveable {mutableStateOf("all")}
    val rows=mutableListOf<TaskRow>()
    sync?.takeIf {it.connected}?.let { status ->
        val running=syncing || status.phase.isNotBlank()
        rows.add(TaskRow("sync","跨设备同步","云端同步",if(running)"RUNNING" else if(status.autoError.isNotBlank())"FAILED" else if(status.lastSync>0)"COMPLETED" else "PAUSED",
            status.phase.ifBlank {status.autoError.ifBlank {"${status.records} 条记录 · ${status.pending} 条待匹配"}},busy=running,
            action=if(running)null else if(status.autoError.isNotBlank())"重试" else "立即同步",run={
                syncing=true
                scope.launch {try {withContext(Dispatchers.IO){AndroidBackend.get(context).sync.sync(status.provider)}}
                    catch(e:CancellationException){throw e}catch(e:Exception){model.showError(e.message ?: "同步未完成")}
                    finally{syncing=false}}
            }))
    }
    library.scans.forEach { (id,scan) ->
        rows.add(TaskRow("scan:$id",library.roots.find{it.id==id}?.title ?: "作品目录","目录扫描",
            if(scan.running)"RUNNING" else if(scan.error!=null)"FAILED" else "COMPLETED",
            scan.error ?: "已读取 ${scan.documents} 个文件",action=if(scan.running)"停止" else "重新扫描",
            run={if(scan.running)model.tools.cancelScan(id) else model.tools.scan(id)}))
    }
    data.tasks.forEach { task ->
        val asset=data.assets[task.assetId]
        val work=state.lookup.forVersion(task.versionId) ?: state.lookup.byId[asset?.workId]
        val version=work?.versions?.find {it.id==task.versionId}
        val hidden=state.privateMetadata || work?.let{!visibleRating(it.rating,state)}==true
        rows.add(TaskRow(task.id,if(hidden)"内容已隐藏" else version?.title ?: asset?.relativePath?.substringAfterLast('/') ?: "下载文件",
            if(hidden)"下载" else work?.title?.let{"下载 · $it"} ?: "下载",task.state,
            "${byteSize(task.downloadedBytes)} / ${byteSize(task.totalBytes)}" + (task.errorCode?.let { " · ${downloadError(it)}" } ?: ""),
            task.totalBytes?.takeIf{it>0}?.let{(task.downloadedBytes.toFloat()/it).coerceIn(0f,1f)},task.id in data.taskBusy,
            when (task.state) {
                "QUEUED", "DOWNLOADING", "VERIFYING" -> "暂停下载"
                "PAUSED", "FAILED" -> downloadResumeLabel(task.state, task.errorCode)
                else -> null
            },
            run={model.online.taskAction(task.id,if(task.state in runningStates)"pause" else "resume")}))
    }
    val shown=rows.filter {when(filter){"active"->it.state in runningStates;"attention"->it.state in setOf("FAILED","PAUSED");"done"->it.state=="COMPLETED";else->true}}
        .sortedBy { if(it.state in runningStates)0 else if(it.state=="FAILED")1 else 2 }
    LazyColumn(Modifier.fillMaxSize().navigationBarsPadding(),contentPadding=PageSpace.padding,verticalArrangement=Arrangement.spacedBy(10.dp)) {
        item { MaintenanceHint("${rows.count{it.state in runningStates}} 项进行中 · ${rows.count{it.state=="FAILED"}} 项失败")
            Row(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                MotionTextButton({open("sync")}){Text("同步明细")}
            }
        }
        sync?.let { if(it.resume!=null) item {SyncResumeCard(it,model)} }
        item {PageTabs(filter,listOf("all" to "全部","active" to "进行中","attention" to "待处理","done" to "已完成")){filter=it}}
        if(!data.tasksReady || !library.rootsReady) item {LoadingRing()}
        data.tasksError?.let { error -> item {MaintenanceStatus(error,error=true);MotionTextButton(model.online::observeTasks){Text("重新读取下载")}} }
        if(shown.isEmpty() && data.tasksReady) item {EmptyContent("此分类暂无任务","任务会在开始后显示于这里。",Glyph.Download)}
        items(shown,key={it.id}) { row -> MaintenanceCard {
            MaintenanceHint(row.category)
            Text(row.title,style=MaterialTheme.typography.titleSmall,maxLines=2,overflow=TextOverflow.Ellipsis)
            MaintenanceHint("${if(row.state=="RUNNING")"进行中" else downloadLabel(row.state)} · ${row.detail}",error=row.state=="FAILED")
            if(row.state in runningStates) {
                if(row.progress!=null) LinearProgressIndicator(progress={row.progress},modifier=Modifier.fillMaxWidth()) else LoadingRing()
            }
            row.action?.let { label -> MotionTextButton(row.run,enabled=!row.busy){Text(label)} }
        } }
    }
}
