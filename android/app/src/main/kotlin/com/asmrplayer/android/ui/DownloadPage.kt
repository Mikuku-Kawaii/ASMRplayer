package com.asmrplayer.android.ui

import android.Manifest
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.app.NotificationManagerCompat
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun DownloadPage(data: OnlineScreenState, state: PlayerScreenState, model: PlayerViewModel, open: (String) -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    var notifications by remember { mutableStateOf(NotificationManagerCompat.from(context).areNotificationsEnabled()) }
    var denied by rememberSaveable { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        notifications = NotificationManagerCompat.from(context).areNotificationsEnabled(); denied = !granted
    }
    DisposableEffect(owner, context) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME)
            notifications = NotificationManagerCompat.from(context).areNotificationsEnabled() }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer) }
    }
    var removing by rememberSaveable { mutableStateOf<String?>(null) }
    var notificationHint by rememberSaveable { mutableStateOf(true) }
    var details by rememberSaveable { mutableStateOf<String?>(null) }
    var filter by rememberSaveable { mutableStateOf("all") }
    val tasks = data.tasks.filter { when (filter) {
        "done" -> it.state == "COMPLETED"
        "active" -> it.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING")
        "attention" -> it.state in setOf("PAUSED", "FAILED")
        else -> true
    } }
    val grouped = tasks.groupBy { state.lookup.forVersion(it.versionId)?.id ?: data.assets[it.assetId]?.workId }
    var expanded by rememberSaveable(filter) { mutableStateOf<String?>(null) }
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(if (data.tasksReady) "${grouped.size} 组下载" else "下载文件", style = MaterialTheme.typography.titleLarge)
                    MaintenanceHint(if (filter == "all") "共 ${tasks.size} 个文件" else "全部 ${data.tasks.size} 个 · 当前筛选 ${tasks.size} 个")
                }
                MotionTextButton({ open("storage") }) { Text("存储") }
            }
            PageTabs(filter,listOf("all" to "全部","active" to "下载中","attention" to "待处理","done" to "已完成")){filter=it}
        }
        if (!notifications && notificationHint && data.tasks.any { it.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING") }) item {
            MaintenanceRow("后台通知未开启") {
                MotionTextButton({
                    if (Build.VERSION.SDK_INT >= 33 && !denied) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    else context.startActivity(if (Build.VERSION.SDK_INT >= 26)
                        Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
                        else Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, android.net.Uri.parse("package:${context.packageName}")))
                }) { Text("开启") }
                MoreActions("通知提示选项") { close -> DropdownMenuItem(text = { Text("收起提示") }, onClick = { close(); notificationHint = false }) }
            }
        }
        if (!data.tasksReady) item {
            LoadingRing(); MaintenanceHint("正在核对下载文件…")
        }
        data.tasksError?.let { error -> item {
            MaintenanceStatus(error, error = true)
            MotionTextButton(model.online::observeTasks) { Text("重新读取任务") }
        } }
        if (data.tasksReady && data.tasksError == null && tasks.isEmpty()) item {
            MaintenanceCard {
                Text(when (filter) { "active" -> "没有正在下载的文件"; "attention" -> "没有暂停或失败的任务"; "done" -> "还没有已完成的下载"; else -> "这里还没有下载任务" }, style = MaterialTheme.typography.titleMedium)
                MaintenanceHint(if (data.tasks.isEmpty()) "在发现页打开作品，选择音轨分组或单个文件下载。" else "其他文件可在“全部”中查看。")
                if (data.tasks.isEmpty()) MotionOutlinedButton({ open("discover") }) { Text("浏览发现") }
                else MotionTextButton({ filter = "all" }) { Text("查看全部") }
            }
        }
        grouped.forEach { (workId,groupTasks) ->
            val key = workId ?: "other"
            item(key="group:$key") {
                val work = state.lookup.byId[workId]
                val done = groupTasks.count { it.state == "COMPLETED" }
                val failed = groupTasks.count { it.state == "FAILED" }
                val paused = groupTasks.count { it.state == "PAUSED" }
                val active = groupTasks.firstOrNull { it.state in setOf("DOWNLOADING","VERIFYING","QUEUED") }
                val status = when {
                    failed > 0 -> "$failed 个失败"
                    active != null -> downloadLabel(active.state)
                    paused > 0 -> "$paused 个暂停"
                    else -> "已完成"
                }
                Surface(onClick = { expanded = if (expanded == key) null else key },
                    shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainerLow) {
                    Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        AppGlyph(Glyph.Folder, Modifier.size(24.dp), MaterialTheme.colorScheme.primary)
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                            Text(if (work == null) "其他文件" else if (visibleRating(work.rating,state)) work.title else "内容已隐藏",
                                style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            MaintenanceHint("$status · 完成 $done/${groupTasks.size}", error = failed > 0)
                        }
                        AppGlyph(if (expanded == key) Glyph.ChevronDown else Glyph.ChevronRight, Modifier.size(20.dp))
                    }
                }
            }
            if (expanded == key) {
                val canPause = groupTasks.filter { it.state in setOf("QUEUED","DOWNLOADING") && it.id !in data.taskBusy }
                val canResume = groupTasks.filter { it.state in setOf("PAUSED","FAILED") && it.id !in data.taskBusy }
                item(key="actions:$key") {
                    FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                        if (canPause.isNotEmpty()) MotionTextButton({canPause.forEach {model.online.taskAction(it.id,"pause")}}){Text("暂停本组")}
                        if (canResume.isNotEmpty()) MotionTextButton({canResume.forEach {model.online.taskAction(it.id,"resume")}}){Text("继续或重试本组")}
                    }
                }
        items(groupTasks, key = { it.id }) { task ->
            val asset = data.assets[task.assetId]
            val work = state.lookup.forVersion(task.versionId) ?: state.lookup.byId[asset?.workId]
            val version = work?.versions?.firstOrNull { it.id == task.versionId }
            val hidden = work != null && ((!state.showNsfw && work.nsfw) || (!state.includeUnrated && work.rating == "UNRATED"))
            val busy = task.id in data.taskBusy
            val path = version?.let { listOf(it.groupPath, it.title).filter(String::isNotBlank).joinToString(" / ") }
                ?: asset?.let { if (it.kind == "cover") "作品封面" else it.relativePath }
            val fileTitle = if (hidden) "内容已隐藏" else version?.title ?: path?.substringAfterLast('/') ?: "下载文件"
            val actions: CardMenu = { close ->
                DropdownMenuItem(text = { Text("文件详情") }, onClick = { close(); details = task.id })
                if (work != null && !hidden) DropdownMenuItem(text = { Text("查看作品") }, onClick = { close(); open("audio:${work.id}") })
                DropdownMenuItem(text = { Text("删除下载", color = MaterialTheme.colorScheme.error) }, enabled = !busy, onClick = { close(); removing = task.id })
            }
            CardActions(fileTitle, menu = actions) {
                Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f).clickable { details = task.id }.heightIn(min = 48.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(fileTitle, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)

                            val bytes = if (task.totalBytes != null && task.totalBytes > 0) "${byteSize(task.downloadedBytes)} / ${byteSize(task.totalBytes)}" else byteSize(task.downloadedBytes)
                            Text("${downloadLabel(task.state)} · $bytes", style = MaterialTheme.typography.labelSmall,
                                color = if (task.state == "FAILED") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
                        }
                        when (task.state) {
                            "QUEUED", "DOWNLOADING", "VERIFYING" -> GlyphButton(Glyph.Pause, "暂停下载 $fileTitle", { model.online.taskAction(task.id, "pause") }, !busy)
                            "PAUSED", "FAILED" -> GlyphButton(Glyph.Play, "${downloadResumeLabel(task.state, task.errorCode)} $fileTitle",
                                { model.online.taskAction(task.id, "resume") }, !busy)
                            "COMPLETED" -> if (task.versionId != null) GlyphButton(Glyph.Play, "播放离线音频 $fileTitle", { model.playVersion(task.versionId); open("player") }, state.connected && !busy)
                        }
                        MoreActions("下载选项 $fileTitle", content = actions)
                    }
                    if (task.state in setOf("DOWNLOADING", "VERIFYING") || busy) LoadingRing()
                    task.errorCode?.let { Text(downloadError(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                    HorizontalDivider(Modifier.padding(top = 8.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = .4f))
                }
            }
        }
        }

        }
    }
    data.tasks.firstOrNull { it.id == details }?.let { task ->
        val asset = data.assets[task.assetId]
        val work = state.lookup.forVersion(task.versionId) ?: state.lookup.byId[asset?.workId]
        val version = work?.versions?.firstOrNull { it.id == task.versionId }
        val hidden = work != null && !visibleRating(work.rating, state)
        PlayerAlertDialog(onDismissRequest = { details = null }, title = { Text("文件详情") }, text = {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                item { Text(if (hidden) "内容已隐藏" else work?.title.orEmpty()) }
                item { Text(if (hidden) "文件信息已隐藏" else version?.let { listOf(it.groupPath, it.title).filter(String::isNotBlank).joinToString(" / ") } ?: asset?.relativePath ?: "文件信息暂不可用") }
                item { Text("${downloadLabel(task.state)} · ${byteSize(task.downloadedBytes)} / ${task.totalBytes?.let(::byteSize) ?: "总大小未知"}") }
                task.errorCode?.let { item { MaintenanceHint(downloadError(it), error = true) } }
                if (task.state == "QUEUED") item { MaintenanceHint("等待网络与系统调度。") }
            }
        }, confirmButton = { MotionTextButton({ details = null }) { Text("关闭") } })
    }
    removing?.let { id -> PlayerAlertDialog(onDismissRequest = { removing = null }, title = { Text("删除此下载？") },
        text = { Text("移除该任务和应用保存的下载文件。作品、喜欢、进度、标记及自行导入的原文件会保留。") },
        confirmButton = { MotionTextButton({ model.online.taskAction(id, "remove"); removing = null }) { Text("删除") } },
        dismissButton = { MotionTextButton({ removing = null }) { Text("取消") } }) }
}

internal fun downloadLabel(state: String) = when (state) {
    "QUEUED" -> "等待下载"; "DOWNLOADING" -> "正在下载"; "VERIFYING" -> "正在校验"
    "PAUSED" -> "已暂停"; "FAILED" -> "下载失败"; "COMPLETED" -> "已完成"; else -> "正在核对状态"
}
internal fun downloadResumeLabel(state: String, errorCode: String?) = when {
    state == "PAUSED" -> "继续下载"
    errorCode in setOf("MISSING_FILE", "INTEGRITY", "INVALID_CONTENT") -> "重新下载"
    else -> "重试下载"
}

internal fun downloadError(code: String) = when (code) {
    "AUTH_REQUIRED" -> "访问权限不足，请检查来源 Token 后继续。"
    "NOT_FOUND" -> "文件不存在，请刷新来源作品。"
    "RATE_LIMITED" -> "请求过于频繁，请稍后重试。"
    "SCHEMA_CHANGED" -> "来源响应格式已变化。"
    "SOURCE_CHANGED" -> "远端文件已变化，请刷新作品后重试。"
    "STORAGE" -> "存储空间不足或文件无法写入。"
    "INTEGRITY", "INVALID_CONTENT" -> "下载内容校验失败，请重试。"
    "MISSING_FILE" -> "下载文件已丢失，请重新下载。"
    "SCHEDULING" -> "后台任务未启动，请保持应用在前台并点击继续。"
    else -> "网络请求失败，请检查连接后重试。"
}
