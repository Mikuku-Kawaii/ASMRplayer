package com.asmrplayer.android.ui

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import com.asmrplayer.android.backup.ImportPolicy
import com.asmrplayer.android.library.ReauthorizationItem
import com.asmrplayer.android.update.*
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

internal fun storageBytes(value: Long): String = when {
    value >= 1024L * 1024 * 1024 -> "%.2f GiB".format(value / (1024.0 * 1024 * 1024))
    value >= 1024L * 1024 -> "%.1f MiB".format(value / (1024.0 * 1024))
    value >= 1024 -> "%.1f KiB".format(value / 1024.0)
    else -> "$value B"
}

@Composable
private fun MaintenanceFeedback(state: MaintenanceState) {
    if (state.busy) Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        LoadingRing(); MaintenanceHint("正在处理，请稍候…")
    }
    
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun BackupPage(tools: MaintenanceTools, chooseDirectory: (String?) -> Unit) {
    val state by tools.state.collectAsStateWithLifecycle()
    var policy by rememberSaveable { mutableStateOf(ImportPolicy.KEEP_EXISTING) }
    var confirmImport by remember { mutableStateOf(false) }
    var missingFiles by remember { mutableStateOf(false) }
    var allowMissing by remember(state.rootPreview) { mutableStateOf(false) }
    var pendingFile by rememberSaveable { mutableStateOf<String?>(null) }
    var pendingKind by rememberSaveable { mutableStateOf("") }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri -> uri?.let(tools::export) }
    val importPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> if (uri != null) { policy = ImportPolicy.KEEP_EXISTING; tools.prepare(uri) } }
    val relink = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val id = pendingFile
        if (uri != null && id != null) tools.relink(ReauthorizationItem(id, pendingKind, ""), uri)
        pendingFile = null
    }
    LaunchedEffect(Unit) { tools.refreshRelinks() }
    BackHandler(state.preview != null && !state.busy) { tools.cancelImport() }
    Column(Modifier.fillMaxSize().navigationBarsPadding()) {
    LazyColumn(Modifier.weight(1f), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (state.busy) item {
            MaintenanceFeedback(state)
            if (state.busy) MotionTextButton(tools::cancel) { Text("取消当前操作") }
        }
        if (state.preview == null && state.rootPreview == null) item {
            MaintenanceCard { MaintenanceRow("导出资料包", "作品、喜欢、进度与标记") {
                MotionButton({ export.launch("ASMR-library.json") }, enabled = !state.busy) { Text("导出") }
            }
            HorizontalDivider()
            MaintenanceRow("恢复资料包", "选择文件后预览，确认前不写入。") {
                MotionOutlinedButton({ importPicker.launch(arrayOf("application/json", "application/octet-stream", "text/plain")) }, enabled = !state.busy) { Text("选择文件") }
            }
            }
            DetailDisclosure("备份范围与换机说明", "资料包不包含媒体文件") {
                MaintenanceHint("明文 JSON 包含作品名称和收听记录，不含音频、图片、账号凭据和系统授权。换机时另行复制媒体文件，再重新授权。")
            }
        }
        if (state.preview != null) item {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                val preview = state.preview
                if (preview == null) {
                    MaintenanceHint("正在读取资料包…")
                } else {
                    Text("核对恢复内容", style = MaterialTheme.typography.titleLarge)
                    MaintenanceHint("已选择资料包 · 核对后确认恢复")
                    MaintenanceValue("作品", "${preview.counts["works"] ?: 0} 部")
                    MaintenanceValue("音轨版本", "${preview.counts["track_versions"] ?: 0} 个")
                    MaintenanceValue("标记", "${preview.counts["bookmarks"] ?: 0} 条")
                    DetailDisclosure("资料包信息", "版本 ${preview.version} · ${preview.existingRecords} 条记录与本机重合") {
                        MaintenanceHint("导出时间 · " + java.text.DateFormat.getDateTimeInstance().format(java.util.Date(preview.createdMs)))
                        MaintenanceHint("当前播放队列保持不变。新设备恢复后，需要重新授权媒体文件。")
                    }
                    Text("重复记录如何处理", style = MaterialTheme.typography.titleSmall)
                    listOf(ImportPolicy.KEEP_EXISTING, ImportPolicy.REPLACE_USER_DATA).forEach { choice ->
                        Row(Modifier.fillMaxWidth().selectable(policy == choice, enabled = !state.busy, role = Role.RadioButton) { policy = choice }.padding(vertical = 4.dp),
                            verticalAlignment = androidx.compose.ui.Alignment.Top) {
                            RadioButton(policy == choice, onClick = null, enabled = !state.busy, modifier = Modifier.padding(end = 12.dp, top = 4.dp))
                            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text(if (choice == ImportPolicy.KEEP_EXISTING) "保留本机资料" else "使用资料包记录", style = MaterialTheme.typography.titleSmall)
                                MaintenanceHint(if (choice == ImportPolicy.KEEP_EXISTING) "本机喜欢、进度和标记优先，只补入缺少的记录。" else "用资料包覆盖同标识的喜欢、进度和标记等资料；文件来源绑定保留。")
                            }
                        }
                    }
                }
            }
        }
        state.rootPreview?.let { preview -> item {
            MaintenanceCard("确认新的目录位置") {
                Text(preview.directoryTitle, style = MaterialTheme.typography.titleSmall)
                MaintenanceValue("匹配原音频", "${preview.matchedAudio} 个")
                MaintenanceValue("缺失音频", "${preview.missingPaths.size} 个")
                MaintenanceValue("新增音频", "${preview.newAudio} 个")
                if (preview.missingPaths.isNotEmpty()) {
                    MotionTextButton({ missingFiles = true }) { Text("查看缺失文件") }
                    ToggleRow("允许部分文件缺失", "缺失文件保留记录，播放前仍需恢复来源。", allowMissing, !state.busy) { allowMissing = it }
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MotionButton({ tools.confirmRoot(allowMissing) }, enabled = !state.busy && (preview.missingPaths.isEmpty() || allowMissing)) { Text("确认重新授权") }
                    MotionTextButton(tools::cancelRoot, enabled = !state.busy) { Text("取消预览") }
                }
            }
        } }
        if (state.preview == null) item {
            HorizontalDivider()
            MaintenanceRow("媒体授权", when {
                !state.relinksReady -> "尚未读取授权状态"
                state.relinks.isEmpty() -> "所有媒体授权正常"
                else -> "${state.relinks.size} 项需要重新选择位置"
            }) { MotionTextButton(tools::refreshRelinks, enabled = !state.busy) { Text("检查") } }
        }
        items(if (state.preview == null) state.relinks else emptyList(), key = { "${it.kind}:${it.id}" }) { item ->
            MaintenanceCard {
                WorkBadge(when (item.kind) { "directory" -> "作品目录"; "audio" -> "单独导入的音频"; else -> "字幕文件" })
                Text(item.title, style = MaterialTheme.typography.titleSmall)
                MotionOutlinedButton({
                    if (item.kind == "directory") chooseDirectory(item.id)
                    else { pendingFile = item.id; pendingKind = item.kind; relink.launch(if (item.kind == "audio") arrayOf("audio/*") else arrayOf("*/*")) }
                }, enabled = !state.busy) { Text("重新选择${if (item.kind == "directory") "目录" else "文件"}") }
            }
        }
    }
    if (state.preview != null) Surface(color = MaterialTheme.colorScheme.surfaceContainerLow) {
        FlowRow(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MotionButton({ confirmImport = true }, enabled = !state.busy) { Text("确认恢复") }
            MotionTextButton(tools::cancelImport, enabled = !state.busy) { Text("取消预览") }
        }
    }
    }
    if (missingFiles && state.rootPreview != null) PlayerAlertDialog(onDismissRequest = { missingFiles = false }, title = { Text("缺失文件") },
        text = { LazyColumn(Modifier.heightIn(max = 420.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            items(state.rootPreview!!.missingPaths) { Text(it, style = MaterialTheme.typography.bodyMedium) }
        } }, confirmButton = { MotionTextButton({ missingFiles = false }) { Text("关闭") } })
    if (confirmImport && state.preview != null) PlayerAlertDialog(onDismissRequest = { confirmImport = false }, title = { Text("导入这些资料？") },
        text = { Text(if (policy == ImportPolicy.REPLACE_USER_DATA) "将覆盖同标识记录的用户资料。建议先导出当前资料作为备份。" else "保留本机同标识记录，补入资料包中新记录。") },
        confirmButton = { MotionTextButton({ confirmImport = false; tools.applyImport(policy) }, enabled = !state.busy) { Text("导入") } },
        dismissButton = { MotionTextButton({ confirmImport = false }) { Text("取消") } })
}

@Composable
internal fun StoragePage(tools: MaintenanceTools, downloads: () -> Unit) {
    val state by tools.state.collectAsStateWithLifecycle()
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { tools.refreshStorage() }
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (state.busy) item { MaintenanceFeedback(state) }
        val usage = state.storage
        item {
            MaintenanceRow("存储位置", "应用专用空间" + (usage?.let { " · 设备可用 ${storageBytes(it.freeBytes)}" } ?: "")) {
                MotionTextButton(tools::refreshStorage, enabled = !state.busy) { Text("刷新") }
            }
        }
        item {
            MaintenanceCard {
                MaintenanceValue("下载文件", usage?.let { storageBytes(it.downloadBytes) } ?: "—")
                MaintenanceHint("主动下载的音频与附件，包含尚未完成的文件。")
                if (usage != null && usage.downloadBytes > 0) {
                    DetailDisclosure("下载明细") {
                        listOf("audio" to "音频", "subtitle" to "字幕", "image" to "图片附件").forEach { (kind, label) ->
                            val bytes = usage.bytesFor(kind)
                            if (bytes > 0) MaintenanceValue(label, storageBytes(bytes))
                        }
                        val other = usage.downloadBytes - listOf("audio", "subtitle", "image").sumOf { usage.bytesFor(it) }
                        if (other > 0) MaintenanceValue("其他文件", storageBytes(other))
                        if (usage.partialBytes > 0) MaintenanceValue("其中未完成", storageBytes(usage.partialBytes))
                    }
                }
                MotionOutlinedButton(downloads) { Text("管理下载文件") }
            }
        }
        item {
            MaintenanceCard {
                MaintenanceValue("媒体缓存", usage?.let { storageBytes(it.cacheBytes) } ?: "—")
                if (usage != null) {
                    HorizontalDivider()
                    MaintenanceValue("在线播放", storageBytes(usage.playbackBytes))
                    MaintenanceValue("作品封面", storageBytes(usage.coverCacheBytes))
                    if (usage.loudnessBytes > 0) MaintenanceValue("响度分析", storageBytes(usage.loudnessBytes))
                    if (usage.coverCacheBytes > 0) MotionTextButton({ confirm = true }, enabled = !state.busy) { Text("清理封面缓存") }
                }
            }
        }
    }
    if (confirm) PlayerAlertDialog(onDismissRequest = { confirm = false }, title = { Text("清理封面缓存？") },
        text = { Text("移除浏览时缓存的图片和已缓存的主封面。音频、字幕、下载的图片附件和收听记录会保留。") },
        confirmButton = { MotionTextButton({ confirm = false; tools.clearCovers() }) { Text("清理") } },
        dismissButton = { MotionTextButton({ confirm = false }) { Text("取消") } })
}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun UpdatePage(model: PlayerViewModel) {
    val tools = model.maintenance
    val data by tools.state.collectAsStateWithLifecycle()
    val update by tools.update.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var preparing by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf(false) }
    var launched by rememberSaveable { mutableStateOf("") }
    val installer = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        tools.notice(if (launched == "permission") {
            if (Build.VERSION.SDK_INT < 26 || context.packageManager.canRequestPackageInstalls()) "安装来源已允许，请再次点击安装更新。" else "尚未允许此来源安装，请授权后重试。"
        } else "已返回应用。系统未完成安装时，可再次点击安装更新。")
        launched = ""
    }
    val busy = update.phase in setOf(UpdatePhase.CHECKING, UpdatePhase.QUEUED, UpdatePhase.DOWNLOADING, UpdatePhase.VERIFYING)
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if (data.busy) item { MaintenanceFeedback(data) }
        item { MaintenanceCard("当前版本 ${BuildConfig.VERSION_NAME}") {
            Text(when (update.phase) {
                UpdatePhase.IDLE -> "尚未检查更新"
                UpdatePhase.CHECKING -> "正在检查更新…"
                UpdatePhase.UP_TO_DATE -> "暂无适用于此设备的新版本"
                UpdatePhase.AVAILABLE -> "可下载新版本"
                UpdatePhase.QUEUED -> "等待网络与系统调度…"
                UpdatePhase.DOWNLOADING -> "正在下载更新…"
                UpdatePhase.VERIFYING -> "正在校验安装包…"
                UpdatePhase.READY -> "安装包已校验，可以安装"
                UpdatePhase.ERROR -> update.error ?: "更新未完成，请重试"
            }, color = if (update.phase == UpdatePhase.ERROR) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
            if (busy || preparing) LoadingRing()
            if (update.phase == UpdatePhase.DOWNLOADING) Text("${storageBytes(update.downloadedBytes)} / ${storageBytes(update.release?.size ?: 0)}")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                MotionOutlinedButton(tools::checkUpdate, enabled = !busy && !data.busy && !preparing && update.phase != UpdatePhase.READY) { Text("检查更新") }
                if (update.phase in setOf(UpdatePhase.QUEUED, UpdatePhase.DOWNLOADING, UpdatePhase.VERIFYING)) MotionTextButton(tools::cancelUpdate) { Text("取消下载") }
            }
        } }
        update.release?.let { release ->
            item { MaintenanceCard {
                Text("版本 ${release.version}", style = MaterialTheme.typography.titleLarge)
                Text("安装包 ${storageBytes(release.size)}")
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (update.phase in setOf(UpdatePhase.AVAILABLE, UpdatePhase.ERROR)) MotionButton(tools::downloadUpdate, enabled = !data.busy && !preparing) { Text("下载更新") }
                    if (update.phase == UpdatePhase.READY) MotionButton({ confirm = true }, enabled = !preparing && !data.busy) { Text("安装更新") }
                    MotionTextButton({ tools.skipUpdate(if (update.skippedVersion == release.version) null else release.version) }, enabled = !preparing) {
                        Text(if (update.skippedVersion == release.version) "恢复此版本提醒" else "不再提醒此版本")
                    }
                }
            } }
            if (release.notes.isNotBlank()) item { Text("更新说明", style = MaterialTheme.typography.titleMedium); Text(release.notes) }
        }
        if (update.release == null && update.skippedVersion != null) item { MotionTextButton({ tools.skipUpdate(null) }) { Text("恢复版本 ${update.skippedVersion} 的提醒") } }
        item { MaintenanceCard {
            ToggleRow("自动检查更新", "应用启动及约每 24 小时检查，具体执行时间由系统安排。", update.autoCheck, true, tools::autoCheck)
        } }
        if(update.phase==UpdatePhase.READY)item { MaintenanceHint("安装前暂停播放并保存资料，随后由系统确认安装。") }
    }
    if (confirm) PlayerAlertDialog(onDismissRequest = { confirm = false }, title = { Text("安装更新？") }, text = { Text("将暂停播放并保存会话。系统可能先要求允许此应用安装更新。") },
        confirmButton = { MotionTextButton({
            confirm = false; preparing = true
            scope.launch {
                try {
                    val action = model.prepareUpdate()
                    when (action) {
                        is UpdateInstallAction.PermissionRequired -> { launched = "permission"; installer.launch(action.intent) }
                        is UpdateInstallAction.Installer -> { launched = "installer"; installer.launch(action.intent) }
                    }
                } catch (e: CancellationException) { throw e }
                catch (_: Exception) { tools.fail("无法打开安装入口，请检查系统授权或重新下载更新后重试。") }
                finally { preparing = false }
            }
        }) { Text("继续安装") } }, dismissButton = { MotionTextButton({ confirm = false }) { Text("取消") } })
}
