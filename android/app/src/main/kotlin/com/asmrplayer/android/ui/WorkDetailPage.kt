package com.asmrplayer.android.ui

import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.delay
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import com.asmrplayer.android.data.WorkAsset
import com.asmrplayer.android.library.LibraryIndex
import com.asmrplayer.android.online.versionId
import com.asmrplayer.android.data.PlaybackPosition
import com.asmrplayer.android.data.preferredDirectory
import com.asmrplayer.android.data.preferredDirectoryOrder
import com.asmrplayer.android.online.BrowseFilters

@OptIn(ExperimentalLayoutApi::class, ExperimentalFoundationApi::class)
@UnstableApi
@Composable
internal fun WorkDetailPage(state:PlayerScreenState,model:PlayerViewModel,workId:String,open:(String)->Unit,back:()->Unit,origin:String?=null,sourceId:String?=null,bottom:androidx.compose.ui.unit.Dp=24.dp) {
    val observedOnline by model.online.state.collectAsStateWithLifecycle()
    val online = routeSnapshot(observedOnline)
    val observedDetail by model.details.state.collectAsStateWithLifecycle()
    val detailState = routeSnapshot(observedDetail)
    val collection by model.collections.state.collectAsStateWithLifecycle()
    val active = LocalRouteActive.current
    if (active) DisposableEffect(workId,origin,sourceId) {
        model.details.select(workId)
        if(origin!=null&&sourceId!=null)model.online.loadDetail(origin,sourceId)
        onDispose {
            if (model.details.state.value.workId == workId) model.details.select(null)
            if (origin != null && model.online.state.value.detailOrigin == origin && model.online.state.value.detailId == sourceId) model.online.leaveDetail()
        }
    }
    val local=state.lookup.byId[workId]
    val isOnline = origin != null && sourceId != null
    val requestedDetail = isOnline && online.detailOrigin == origin && online.detailId == sourceId
    val remote = online.detail.takeIf { requestedDetail }
    val onlineError = online.detailError.takeIf { requestedDetail }
    val waitingForTracks = isOnline && remote == null && onlineError == null
    val localTracks = if (waitingForTracks) emptyList() else local?.versions.orEmpty()
    fun retryDetail() {
        if (origin != null && sourceId != null) model.online.loadDetail(origin, sourceId, force = true)
    }
    val meta=state.metadata[workId]
    // The browse card already has the hero metadata. Show it during the network request so
    // the cover has a destination on the first navigation frame.
    val preview = online.page?.items?.firstOrNull { origin == online.account.apiBase && it.sourceId == sourceId }
    val presentedWork = remote?.work ?: preview
    val title=presentedWork?.title?:local?.title
    val audioGroups = remember(remote?.files) {
        remote?.files.orEmpty().filter { it.kind == "audio" }
            .groupBy { it.path.substringBeforeLast('/', "") to it.format }
    }
    val localGroups = remember(localTracks) { localTracks.groupBy { it.groupPath to it.format } }
    val groups = remember(remote != null, audioGroups, localGroups, state.filePreferences) {
        preferredDirectoryOrder(if (remote != null) audioGroups.keys.toList() else localGroups.keys.toList(), state.filePreferences)
    }
    var group by rememberSaveable(workId, state.filePreferences){mutableStateOf("")}
    var format by rememberSaveable(workId, state.filePreferences){mutableStateOf("")}
    LaunchedEffect(detailState.notice) { detailState.notice?.let { model.feedback(it); model.details.clearNotice() } }
    val positions = remember(detailState.positions) { detailState.positions.associateBy { it.versionId } }
    val groupsByVersion = remember(remote != null, audioGroups, localGroups, workId) {
        buildMap {
            if (remote != null) {
                audioGroups.forEach { (groupKey, tracks) -> tracks.forEach { put(it.versionId(workId), groupKey) } }
            } else {
                localGroups.forEach { (groupKey, tracks) -> tracks.forEach { if (it.available) put(it.id, groupKey) } }
            }
        }
    }
    val lastInWork=groupsByVersion.keys.mapNotNull(positions::get).latestUnfinished()
    val selected=groups.firstOrNull { it.first==group && it.second==format }
        ?: preferredDirectory(groups, state.filePreferences, groupsByVersion[lastInWork?.versionId])
    val versions = localGroups[selected].orEmpty()
    val files = audioGroups[selected].orEmpty()
    val versionsById = remember(localTracks) { localTracks.associateBy { it.id } }
    val tasksByVersion = remember(online.tasks) {
        buildMap { online.tasks.forEach { task -> task.versionId?.let { putIfAbsent(it, task) } } }
    }
    val favoriteIds = remember(collection.favorites) { collection.favorites.mapTo(HashSet()) { it.versionId } }
    val listState = rememberLazyListState()
    val compactHeader by remember { derivedStateOf { listState.firstVisibleItemIndex > 0 } }
    val attachmentMeasurer = rememberTextMeasurer()
    var page by rememberSaveable(workId){mutableStateOf("tracks")}
    var image by remember {mutableStateOf<String?>(null)}
    var pickingTracks by rememberSaveable(workId) { mutableStateOf(false) }
    var fileInfo by remember {mutableStateOf<String?>(null)}
    var metadataInput by rememberSaveable(workId) { mutableStateOf<String?>(null) }
    var binding by remember {mutableStateOf<WorkAsset?>(null)}
    val available=versions.filter{it.available}.map{it.id}
    val groupVersions=if(remote!=null) files.map { it.versionId(workId) } else available
    val latest=groupVersions.mapNotNull(positions::get).latestUnfinished()
    val current=state.queue.getOrNull(state.index)?.id
    val busy=detailState.busy||online.working
    val attachments = remember(workId, detailState.assets, remote?.files, detailState.remoteAssets) {
        detailAttachments(workId, detailState.assets, remote?.files.orEmpty(), detailState.remoteAssets)
    }
    val tasksByAsset = remember(online.tasks) { online.tasks.filter { it.assetId != null }.associateBy { it.assetId } }
    val playable = state.connected && !busy && groupVersions.isNotEmpty()
    fun download(ids: Set<String>? = null, paths: Set<String>? = null) {
        model.downloadPlan.open(workId, origin ?: detailState.remoteOrigin, sourceId ?: detailState.remoteId,
            initialIds = ids, initialPaths = paths)
    }
    fun act(action:String,ids:List<String> = available,paths:List<String> = files.map{it.path}) {
        if(remote!=null) {
            val startPath=if(action=="play") files.firstOrNull { it.versionId(workId)==latest?.versionId }?.path else null
            model.online.act(action,paths,startPath,completed=if(action in setOf("play","restart")) {{open("player")}} else null)
            return
        }
        else when(action) {
            "play" -> model.playGroup(ids,startVersion=latest?.versionId)
            "restart" -> model.playGroup(ids,true)
            "playlist" -> model.choosePlaylist(ids)
            "enqueue","next" -> model.enqueue(ids,action=="next")
        }
        if(action in setOf("play","restart"))open("player")
    }
    val voices=presentedWork?.voices?:meta?.voices().orEmpty()
    val tags=presentedWork?.tags?:meta?.tags().orEmpty()
    val circle=presentedWork?.circle?:meta?.circle.orEmpty()
    fun discover(tag:String?=null,voice:String?=null) {
        model.online.search("",conditions=BrowseFilters(tags=listOfNotNull(tag),voices=listOfNotNull(voice)));open("discover")
    }
    val workActions: CardMenu = {close->
        DropdownMenuItem(text={Text("从头播放")},enabled=playable,onClick={close();act("restart")})
        CardAction("加入作品集", close, !busy && (presentedWork != null || local != null)) {
            if (presentedWork != null && origin != null) model.online.chooseWork(presentedWork,origin,selected?.first,selected?.second)
            else model.chooseWork(workId,selected?.first,selected?.second)
        }
        if(remote!=null||detailState.remoteId!=null) DropdownMenuItem(text={Text("下载作品")},enabled=!busy,onClick={close();download()})
        DropdownMenuItem(text={Text("加入队列")},enabled=!busy && !waitingForTracks,onClick={close();act("enqueue")})
        if(remote!=null)DropdownMenuItem(text={Text(if(local?.inLibrary!=true)"加入音频库" else "刷新作品信息")},onClick={close();if(local?.inLibrary==true&&origin!=null)model.details.refresh(remote.work.sourceId,origin) { model.online.loadDetail(origin, remote.work.sourceId) } else model.online.act("save")})
        else if(detailState.remoteId!=null&&detailState.remoteOrigin!=null)DropdownMenuItem(text={Text("刷新作品信息")},onClick={close();model.details.refresh(detailState.remoteId!!,detailState.remoteOrigin!!)})
        else if (!isOnline && local != null) CardAction("同步元数据", close, !busy) {
            val input = meta?.takeIf { it.origin == online.account.apiBase }?.sourceId?.takeIf { it.isNotBlank() }
                ?: local.code?.takeIf { it.isNotBlank() } ?: LibraryIndex.workCode(local.title)
                ?: local.versions.firstNotNullOfOrNull { LibraryIndex.workCode(it.title) }
            if (input == null) metadataInput = "" else model.details.syncLocalMetadata(workId, input)
        }
        if(local!=null)DropdownMenuItem(text={Text(if(local.nsfw)"取消 NSFW 标记" else "标记为 NSFW")},onClick={close();model.setNsfw(workId,!local.nsfw)})
        DropdownMenuItem(text={Text("下载管理")},onClick={close();open("downloads")})
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp).heightIn(min = 56.dp),
            verticalAlignment = Alignment.CenterVertically) {
            GlyphButton(Glyph.Back, "返回", back)
            Crossfade(compactHeader, Modifier.weight(1f),
                animationSpec = if (LocalUiMotion.current) tween(220, easing = MotionEase) else snap(),
                label = "detail header title") { compact ->
                Text(if (compact) title ?: "作品详情" else "作品详情",
                    Modifier.fillMaxWidth().then(if (compact == compactHeader) Modifier.semantics { heading() }
                        else Modifier.clearAndSetSemantics { }),
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                    style = if (compact) MaterialTheme.typography.titleMedium else MaterialTheme.typography.titleLarge)
            }
            if (title != null && (local == null || visibleRating(local.rating, state))) DetailActions("作品操作", workActions)
        }
        if(title==null) {
            Column(Modifier.padding(24.dp)) {
                if (waitingForTracks) LoadingRing("正在加载音轨目录")
                onlineError?.let { Text(it); MotionTextButton(::retryDetail) { Text("重新加载") } }
            };return@Column
        }
        if(local!=null&&!visibleRating(local.rating,state)){EmptyContent("内容已隐藏","可在内容显示设置中调整。",Glyph.Eye);return@Column}
        LazyColumn(Modifier.navigationBarsPadding(),state=listState,contentPadding=PaddingValues(start=20.dp,end=20.dp,top=8.dp,bottom=bottom),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            item {
                CardActions(title, menu = workActions) {
                    WorkHero(title, presentedWork?.coverUrl ?: local?.coverUri, workId)
                }
            }
            item {
                WorkDetailActionRow(
                    resumeTime = latest?.let { time(it.positionMs) },
                    collected = local?.inLibrary == true,
                    liked = local?.favorite == true,
                    playEnabled = playable && (detailState.ready || detailState.error != null),
                    actionsEnabled = !busy,
                    onPlay = { act("play") },
                    onCollect = { if (local != null) model.toggleLibraryMembership(workId) else model.online.act("save") },
                    onLike = { if (local != null) model.toggleFavorite(workId) else model.online.act("favorite") },
                )
                if (busy) LoadingRing()
                detailState.error?.let {Text(it,color=MaterialTheme.colorScheme.error)}
            }
            stickyHeader {
                Surface(color=MaterialTheme.colorScheme.background) {
                    Column {
                        val trackLabel = if (waitingForTracks) "音轨" else "音轨 ${if (remote != null) files.size else versions.size}"
                        PageTabs(page,listOf("tracks" to trackLabel,"info" to "介绍","assets" to "附件")){page=it}
                    }
                }
            }
            if(page=="info") {
                val code = presentedWork?.code ?: local?.code
                if (!code.isNullOrBlank()) item { DetailCodeRow(code) }
                if (circle.isNotBlank()) item { DetailInfoRow("社团", circle) }
                if (voices.isNotEmpty()) item { DetailChipRows("CV", voices, isCast = true) { discover(voice = it) } }
                val release=remote?.work?.release?:meta?.release.orEmpty()
                if (release.isNotBlank()) item { DetailInfoRow("发行", release) }
                if (tags.isNotEmpty()) item { DetailChipRows("标签", tags, isCast = false) { discover(tag = it) } }
                item {
                    HorizontalDivider(Modifier.padding(vertical = 4.dp), color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = .4f))
                    val description=remote?.work?.description?:meta?.description.orEmpty()
                    Text(description.ifBlank{"暂无作品简介"},Modifier.padding(vertical=8.dp),style=MaterialTheme.typography.bodyLarge)
                }
            } else if(page=="tracks") {
                if (waitingForTracks) item { LoadingRing("正在加载音轨目录") }
                if (onlineError != null) item {
                    Text(onlineError, color = MaterialTheme.colorScheme.error)
                    MotionTextButton(::retryDetail) { Text("重新加载") }
                }
                if (groups.isNotEmpty()) item {
                    Column {
                        GroupPicker(groups,selected){group=it.first;format=it.second}
                        MotionTextButton({pickingTracks=true}, enabled=!busy && groupVersions.isNotEmpty()){ Text("选择音轨") }
                    }
                }
                if(remote!=null) itemsIndexed(files,key={_,it->it.path}) {index,file ->
                    val id=file.versionId(workId)
                    val version = versionsById[id]
                    val task = version?.let { tasksByVersion[it.id] }
                    val liked = id in favoriteIds
                    DetailTrackRow(file.title,listOfNotNull(trackProgress(positions[id], file.durationMs),task?.let{downloadLabel(it.state)}).joinToString(" · "),
                        { model.online.act("play", files.map { it.path }, startPath = file.path) { open("player") } },!busy,index,version!=null && version.id==current,liked,
                        {if(version!=null)model.collections.favorite(version.id,!liked) else act("favorite-track",paths=listOf(file.path))}) { close ->
                        DropdownMenuItem(text={Text("下一首播放")},onClick={close();act("next",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("加入队列")},onClick={close();act("enqueue",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("加入音轨歌单")},onClick={close();act("playlist",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("下载音轨")},onClick={close();download(paths=setOf(file.path))})
                        DropdownMenuItem(text={Text("文件信息")},onClick={close();fileInfo="${file.path}\n${file.format.uppercase()} · ${byteSize(file.bytes)}"})
                    }
                } else itemsIndexed(versions,key={_,it->it.id}) {index,version ->
                    val p=positions[version.id]
                    val task = tasksByVersion[version.id]
                    val favorite = version.id in favoriteIds
                    DetailTrackRow(version.title,listOfNotNull(trackProgress(p, p?.durationMs),if(!version.available)"文件不可用" else task?.let{downloadLabel(it.state)}).joinToString(" · "),
                        { model.playGroup(available, startVersion = version.id); open("player") },version.available,index,version.id==current,favorite,{model.collections.favorite(version.id,!favorite)}) { close ->
                        DropdownMenuItem(text={Text("下一首播放")},enabled=version.available,onClick={close();model.enqueue(listOf(version.id),true)})
                        DropdownMenuItem(text={Text("加入队列")},enabled=version.available,onClick={close();model.enqueue(listOf(version.id))})
                        DropdownMenuItem(text={Text("加入音轨歌单")},onClick={close();model.choosePlaylist(listOf(version.id))})
                        if(detailState.remoteId!=null)DropdownMenuItem(text={Text("下载音轨")},onClick={close();download(ids=setOf(version.id))})
                        DropdownMenuItem(text={Text("文件信息")},onClick={close();fileInfo="${version.groupPath}/${version.title}"})
                    }
                }
            } else {
                val pictures = attachments.filter { it.picture }
                val documents = attachments.filterNot { it.picture }
                fun downloadAttachment(entry: DetailAttachment) {
                    if (entry.remote != null) download(paths = setOf(entry.path))
                    else if (entry.reference != null) download(ids = setOf(entry.reference.id))
                }
                if (detailState.ready && attachments.isEmpty()) item {
                    EmptyContent("暂无附件", "字幕、图片与文档会出现在这里。", Glyph.Library)
                }
                if (remote?.work?.coverUrl != null) item {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("附件 ${attachments.size}", style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                        MotionOutlinedButton({ download(ids = setOf(LibraryIndex.id(workId, "cover"))) }, enabled = !busy) {
                            AppGlyph(Glyph.Download, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("缓存封面")
                        }
                    }
                }
                items(pictures.chunked(2), key = { "pictures:${it.first().id}" }) { row ->
                    BoxWithConstraints(Modifier.fillMaxWidth()) {
                    val textWidth = with(LocalDensity.current) { ((maxWidth - 10.dp) / 2 - 20.dp).roundToPx().coerceAtLeast(1) }
                    val titleLines = row.maxOf { entry ->
                        attachmentMeasurer.measure(entry.title, style = MaterialTheme.typography.titleSmall,
                            constraints = Constraints(maxWidth = textWidth), maxLines = 2).lineCount
                    }.coerceIn(1, 2)
                    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Max), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        row.forEach { entry ->
                            key(entry.id) {
                                AttachmentImageCard(entry, tasksByAsset[entry.id], busy,
                                    modifier = Modifier.weight(1f).fillMaxHeight(),
                                    titleLines = titleLines,
                                    resolve = model.details::attachmentImageUrl,
                                    preview = { image = it },
                                    download = { downloadAttachment(entry) }, info = { fileInfo = entry.path })
                            }
                        }
                        if (row.size == 1) Spacer(Modifier.weight(1f))
                    }
                    }
                }
                if (pictures.isNotEmpty() && documents.isNotEmpty()) item {
                    Text("其他附件", Modifier.padding(top = 8.dp), style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                items(documents, key = { "attachment:${it.id}" }) { entry ->
                    AttachmentFileRow(entry, tasksByAsset[entry.id], busy,
                        download = { downloadAttachment(entry) },
                        previewSubtitle = { entry.local?.let(model.details::preview) },
                        menu = { close ->
                            CardAction("文件信息", close) { fileInfo = entry.path }
                            if (entry.available) entry.local?.let { asset ->
                                LocalAssetMenu(asset, close, { image = it }, model.details::preview, { binding = it })
                            } else if (entry.downloadable) {
                                val running = tasksByAsset[entry.id]?.state in setOf("QUEUED", "DOWNLOADING", "VERIFYING")
                                CardAction("下载附件", close, !busy && !running) { downloadAttachment(entry) }
                            }
                        })
                }
            }
        }
        image?.let { uri -> FullscreenArtwork(uri, label = "附件图片") { image = null } }
        if(pickingTracks) PlayerSheet("选择音轨",{pickingTracks=false},contentScrolls=true) {
            val choices = if(remote != null) files.map { PlaylistChoice(it.path,it.title,it.format.uppercase()) }
                else versions.filter { it.available }.map { PlaylistChoice(it.id,it.title,it.format.uppercase()) }
            PlaylistMemberChoices(choices,emptyList(),busy,"加入音轨歌单",500) { chosen ->
                pickingTracks=false
                if(remote!=null) act("playlist",paths=chosen) else model.choosePlaylist(chosen)
            }
        }
        metadataInput?.let { input ->
            PlayerAlertDialog(
                onDismissRequest = { metadataInput = null },
                title = { Text("同步元数据") },
                text = { OutlinedTextField(input, { metadataInput = it }, singleLine = true,
                    label = { Text("作品编号") }, placeholder = { Text("RJ、VJ、BJ 编号或站内 ID") }) },
                confirmButton = { MotionTextButton({
                    metadataInput = null
                    model.details.syncLocalMetadata(workId, input.trim())
                }, enabled = input.isNotBlank()) { Text("同步") } },
                dismissButton = { MotionTextButton({ metadataInput = null }) { Text("取消") } },
            )
        }
        fileInfo?.let {text->PlayerSheet("文件信息",{fileInfo=null}){Text(text)}}
        detailState.preview?.let {text->PlayerSheet("字幕预览",model.details::closePreview,contentScrolls=true){LazyColumn(Modifier.fillMaxWidth().padding(horizontal=20.dp),verticalArrangement=Arrangement.spacedBy(12.dp)){items(text.split("\n\n")){Text(it)}}}}
        binding?.let {asset->
            var selectedVersion by remember(asset.id){mutableStateOf(versions.firstOrNull()?.id.orEmpty())}
            PlayerSheet("关联字幕",{binding=null},contentScrolls=true){Column(Modifier.fillMaxWidth().padding(horizontal=20.dp)) {
                Text(asset.relativePath.substringAfterLast('/'),style=MaterialTheme.typography.bodySmall)
                LazyColumn(Modifier.weight(1f, fill = false)){items(versions,key={it.id}){v->Row(Modifier.fillMaxWidth().clickable{selectedVersion=v.id},verticalAlignment=Alignment.CenterVertically){RadioButton(selectedVersion==v.id,{selectedVersion=v.id});Text(v.title,style=MaterialTheme.typography.bodyMedium)}}}
                Row {
                    MotionTextButton({model.details.bind(asset,selectedVersion,false);binding=null},enabled=selectedVersion.isNotBlank()){Text("设为原文")}
                    MotionTextButton({model.details.bind(asset,selectedVersion,true);binding=null},enabled=selectedVersion.isNotBlank()){Text("设为译文")}
                }
            }}
        }
    }
}
@Composable
private fun DetailCodeRow(code: String) {
    val context = LocalContext.current
    var copied by remember(code) { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1200); copied = false } }
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp)
        .clickable(onClickLabel = "复制作品编号") {
            val clipboard = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("RJ", code))
            copied = true
        }, horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text("编号", Modifier.width(40.dp), style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(code, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.primary)
        AppGlyph(if (copied) Glyph.Check else Glyph.Copy, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
    }
}

@Composable
private fun DetailInfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, Modifier.width(40.dp), style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DetailChipRows(label: String, values: List<String>, isCast: Boolean, select: (String) -> Unit) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(label, Modifier.width(40.dp).padding(top = 13.dp), style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
        FlowRow(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            values.forEach { name ->
                Surface(onClick = { select(name) }, shape = MaterialTheme.shapes.small,
                    modifier = Modifier.padding(vertical = 4.dp).heightIn(min = 40.dp),
                    color = if (isCast) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh) {
                    Box(Modifier.padding(horizontal = 10.dp, vertical = 6.dp), contentAlignment = Alignment.Center) {
                        Text(name, style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        }
    }
}

@Composable
private fun LocalAssetMenu(asset: WorkAsset, close: () -> Unit, image: (String) -> Unit, preview: (WorkAsset) -> Unit, bind: (WorkAsset) -> Unit) {
    if (asset.format in LibraryIndex.imageExtensions) CardAction("查看图片", close) { image(asset.uri) }
    if (asset.format in LibraryIndex.subtitleExtensions) {
        CardAction("预览字幕", close) { preview(asset) }
        CardAction("关联音轨", close) { bind(asset) }
    }
}

@Composable
private fun DetailActions(title:String, menu:@Composable ColumnScope.(()->Unit)->Unit) {
    var showing by remember {mutableStateOf(false)}
    GlyphButton(Glyph.More,title,{showing=true})
    if(showing)PlayerSheet(title,{showing=false}){Column{menu{showing=false}}}
}
@Composable
private fun DetailTrackRow(title:String,subtitle:String,play:()->Unit,enabled:Boolean=true,index:Int,current:Boolean,favorite:Boolean,like:()->Unit,menu:@Composable ColumnScope.(()->Unit)->Unit) {
    CardActions(trackDisplayTitle(title), menu = menu) {
        Row(Modifier.fillMaxWidth()
            .background(if (current) MaterialTheme.colorScheme.primaryContainer.copy(alpha = .35f) else androidx.compose.ui.graphics.Color.Transparent,
                androidx.compose.foundation.shape.RoundedCornerShape(12.dp))
            .heightIn(min = 64.dp).padding(vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(Modifier.weight(1f).clickable(enabled = enabled, onClick = play).heightIn(min = 48.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Box(Modifier.width(22.dp), contentAlignment = Alignment.Center) {
                    if (current) AppGlyph(Glyph.Play, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                    else Text("${index+1}", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(trackDisplayTitle(title), style = MaterialTheme.typography.titleSmall, maxLines = 1,
                        overflow = TextOverflow.Ellipsis, color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    if (subtitle.isNotBlank()) Text(subtitle, style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            MotionIconToggleButton(favorite,{like()},Modifier.semantics{contentDescription=if(favorite)"取消书签" else "添加书签"}) {
                AppGlyph(Glyph.TrackBookmark,Modifier.size(20.dp),filled=favorite,
                    tint=if(favorite)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
            }
            DetailActions("音轨操作",menu)
        }
    }
    HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.25f))
}

internal fun List<PlaybackPosition>.latestUnfinished(): PlaybackPosition? =
    filter { it.positionMs > 0 && !it.completed }.maxByOrNull { it.updatedMs }

private fun trackProgress(position: PlaybackPosition?, durationMs: Long?): String? {
    val duration = durationMs?.takeIf { it > 0 } ?: position?.durationMs?.takeIf { it > 0 }
    return when {
        position?.completed == true -> listOfNotNull("已听完", duration?.let(::time)).joinToString(" · ")
        position != null && position.positionMs > 0 -> "听到 ${time(position.positionMs)}" + (duration?.let { " / ${time(it)}" } ?: "")
        else -> duration?.let(::time)
    }
}
