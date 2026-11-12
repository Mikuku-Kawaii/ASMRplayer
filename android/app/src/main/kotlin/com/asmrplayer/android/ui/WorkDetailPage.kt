package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.AlignmentLine
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.unit.Constraints
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
    val groups = if (remote != null) audioGroups.keys.toList() else localGroups.keys.toList()
    var group by rememberSaveable(workId){mutableStateOf("")}
    var format by rememberSaveable(workId){mutableStateOf("")}
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
        ?: groupsByVersion[lastInWork?.versionId] ?: groups.firstOrNull()
    val versions = localGroups[selected].orEmpty()
    val files = audioGroups[selected].orEmpty()
    val versionsById = remember(localTracks) { localTracks.associateBy { it.id } }
    val tasksByVersion = remember(online.tasks) {
        buildMap { online.tasks.forEach { task -> task.versionId?.let { putIfAbsent(it, task) } } }
    }
    val favoriteIds = remember(collection.favorites) { collection.favorites.mapTo(HashSet()) { it.versionId } }
    val listState = rememberLazyListState()
    var page by rememberSaveable(workId){mutableStateOf("tracks")}
    var image by remember {mutableStateOf<String?>(null)}
    var fileInfo by remember {mutableStateOf<String?>(null)}
    var binding by remember {mutableStateOf<WorkAsset?>(null)}
    val available=versions.filter{it.available}.map{it.id}
    val groupVersions=if(remote!=null) files.map { it.versionId(workId) } else available
    val latest=groupVersions.mapNotNull(positions::get).latestUnfinished()
    val current=state.queue.getOrNull(state.index)?.id
    val busy=detailState.busy||online.working
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
        CardAction("加入歌单", close, !busy && !waitingForTracks) {
            if (remote != null && origin != null) model.online.cardAction(origin, remote.work.sourceId, com.asmrplayer.android.OnlineTools.CardAction.Playlist)
            else model.choosePlaylist(com.asmrplayer.android.data.preferredPlaylistVersions(local?.versions.orEmpty()))
        }
        if(remote!=null||detailState.remoteId!=null) DropdownMenuItem(text={Text("下载作品")},enabled=!busy,onClick={close();download()})
        DropdownMenuItem(text={Text("加入队列")},enabled=!busy && !waitingForTracks,onClick={close();act("enqueue")})
        if(remote!=null)DropdownMenuItem(text={Text(if(local?.inLibrary!=true)"加入音频库" else "刷新作品信息")},onClick={close();if(local?.inLibrary==true&&origin!=null)model.details.refresh(remote.work.sourceId,origin) { model.online.loadDetail(origin, remote.work.sourceId) } else model.online.act("save")})
        else if(detailState.remoteId!=null&&detailState.remoteOrigin!=null)DropdownMenuItem(text={Text("刷新作品信息")},onClick={close();model.details.refresh(detailState.remoteId!!,detailState.remoteOrigin!!)})
        if(local!=null)DropdownMenuItem(text={Text(if(local.nsfw)"取消 NSFW 标记" else "标记为 NSFW")},onClick={close();model.setNsfw(workId,!local.nsfw)})
        DropdownMenuItem(text={Text("下载管理")},onClick={close();open("downloads")})
    }
    Column(Modifier.fillMaxSize()) {
        RouteHeader("作品详情", back) {
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
                    WorkHero(title, presentedWork?.coverUrl ?: local?.coverUri, presentedWork?.code ?: local?.code, circle, workId)
                }
            }
            item {
                FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                    val playable = state.connected && !busy && groupVersions.isNotEmpty()
                    MotionButton({act("play")},enabled=playable && (detailState.ready || detailState.error!=null)) {
                        AppGlyph(Glyph.Play)
                        Spacer(Modifier.width(6.dp))
                        Text(latest?.let { "继续收听 · ${time(it.positionMs)}" } ?: "播放当前目录")
                    }
                    MotionOutlinedButton({act("restart")},enabled=playable) {Text("从头播放")}
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        MotionIconButton(
                            onClick = { if (local != null) model.addToLibrary(workId) else model.online.act("save") },
                            enabled = !busy,
                            modifier = Modifier.semantics { contentDescription = if (local?.inLibrary == true) "已加入音频库" else "加入音频库" }
                        ) { AppGlyph(Glyph.Collection, filled = local?.inLibrary == true) }
                        val liked = local?.favorite == true
                        MotionIconToggleButton(liked, { if (local != null) model.toggleFavorite(workId) else model.online.act("favorite") },
                            enabled = !busy, modifier = Modifier.semantics { contentDescription = if (liked) "取消作品喜欢" else "喜欢作品" }) {
                            AppGlyph(Glyph.Heart, filled = liked, tint = if (liked) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                
                }
                if (busy) LoadingRing()
                detailState.error?.let {Text(it,color=MaterialTheme.colorScheme.error)}
            }
            if (voices.isNotEmpty()) item(key = "detail-cast") {
                DetailChipRows("CV", voices, isCast = true) { discover(voice = it) }
            }
            if (tags.isNotEmpty()) item(key = "detail-tags") {
                DetailChipRows("标签", tags, isCast = false) { discover(tag = it) }
            }
            stickyHeader {
                Surface(color=MaterialTheme.colorScheme.background) {
                    Column {
                        if(listState.firstVisibleItemIndex>0)Text(title,Modifier.padding(vertical=6.dp),maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.labelMedium)
                        val trackLabel = if (waitingForTracks) "音轨" else "音轨 ${if (remote != null) files.size else versions.size}"
                        PageTabs(page,listOf("tracks" to trackLabel,"info" to "介绍","assets" to "附件")){page=it}
                    }
                }
            }
            if(page=="info") {
                item {
                    if(circle.isNotBlank())Text(circle, style=MaterialTheme.typography.titleMedium)
                    val release=remote?.work?.release?:meta?.release.orEmpty()
                    if(release.isNotBlank())Text("发行日期 · $release",style=MaterialTheme.typography.bodyMedium)
                    val description=remote?.work?.description?:meta?.description.orEmpty()
                    Text(description.ifBlank{"暂无作品简介"},Modifier.padding(vertical=12.dp),style=MaterialTheme.typography.bodyLarge)
                    if(tags.isEmpty())Text("暂无标签",style=MaterialTheme.typography.bodyMedium)
                }
            } else if(page=="tracks") {
                if (waitingForTracks) item { LoadingRing("正在加载音轨目录") }
                if (onlineError != null) item {
                    Text(onlineError, color = MaterialTheme.colorScheme.error)
                    MotionTextButton(::retryDetail) { Text("重新加载") }
                }
                if (groups.isNotEmpty()) item {
                    GroupPicker(groups,selected){group=it.first;format=it.second}
                }
                if(remote!=null) itemsIndexed(files,key={_,it->it.path}) {index,file ->
                    val id=file.versionId(workId)
                    val version = versionsById[id]
                    val task = version?.let { tasksByVersion[it.id] }
                    val liked = id in favoriteIds
                    DetailTrackRow(file.title,listOfNotNull(progressLabel(positions[id]),file.durationMs?.let { time(it) },byteSize(file.bytes),task?.let{downloadLabel(it.state)}).joinToString(" · "),
                        { model.online.act("play", files.map { it.path }, startPath = file.path) { open("player") } },!busy,index,version!=null && version.id==current,liked,
                        {if(version!=null)model.collections.favorite(version.id,!liked) else act("favorite-track",paths=listOf(file.path))}) { close ->
                        DropdownMenuItem(text={Text("下一首播放")},onClick={close();act("next",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("加入队列")},onClick={close();act("enqueue",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("加入歌单")},onClick={close();act("playlist",paths=listOf(file.path))})
                        DropdownMenuItem(text={Text("下载音轨")},onClick={close();download(paths=setOf(file.path))})
                        DropdownMenuItem(text={Text("文件信息")},onClick={close();fileInfo=file.path})
                    }
                } else itemsIndexed(versions,key={_,it->it.id}) {index,version ->
                    val p=positions[version.id]
                    val task = tasksByVersion[version.id]
                    val favorite = version.id in favoriteIds
                    DetailTrackRow(version.title,listOfNotNull(progressLabel(p),p?.durationMs?.takeIf{it>0}?.let{time(it)},if(!version.available)"文件不可用" else task?.let{downloadLabel(it.state)}).joinToString(" · "),
                        { model.playGroup(available, startVersion = version.id); open("player") },version.available,index,version.id==current,favorite,{model.collections.favorite(version.id,!favorite)}) { close ->
                        DropdownMenuItem(text={Text("下一首播放")},enabled=version.available,onClick={close();model.enqueue(listOf(version.id),true)})
                        DropdownMenuItem(text={Text("加入队列")},enabled=version.available,onClick={close();model.enqueue(listOf(version.id))})
                        DropdownMenuItem(text={Text("加入歌单")},onClick={close();model.choosePlaylist(listOf(version.id))})
                        if(detailState.remoteId!=null)DropdownMenuItem(text={Text("下载音轨")},onClick={close();download(ids=setOf(version.id))})
                        DropdownMenuItem(text={Text("文件信息")},onClick={close();fileInfo="${version.groupPath}/${version.title}"})
                    }
                }
            } else {
                val remoteAssets=remote?.files.orEmpty().filter{it.kind!="audio"}
                val pictures=detailState.assets.filter {it.available&&it.format in LibraryIndex.imageExtensions}
                items(pictures.chunked(2),key={"pictures:${it.first().id}"}) { row ->
                    Row(horizontalArrangement=Arrangement.spacedBy(12.dp)) {
                        row.forEach { asset ->
                            CardActions(asset.relativePath.substringAfterLast('/'), modifier = Modifier.weight(1f), menu = { close ->
                                CardAction("查看图片", close) { image = asset.uri }
                                CardAction("文件信息", close) { fileInfo = asset.relativePath }
                            }) {
                                Column(Modifier.fillMaxWidth().clickable{image=asset.uri}) {
                                    AudioArtwork(Modifier.fillMaxWidth().aspectRatio(4f/3f),asset.uri)
                                    Text(asset.relativePath.substringAfterLast('/'),Modifier.padding(vertical=8.dp),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall)
                                }
                            }
                        }
                        if(row.size==1)Spacer(Modifier.weight(1f))
                    }
                }
                if(detailState.ready&&remoteAssets.isEmpty()&&detailState.assets.isEmpty()&&detailState.remoteAssets.none{it.kind!="cover"})item{EmptyContent("暂无附件","字幕、图片与文档会出现在这里。",Glyph.Library)}
                if(remote?.work?.coverUrl!=null)item{MotionTextButton({download(ids=setOf(LibraryIndex.id(workId,"cover")))},enabled=!busy){Text("缓存作品封面")}}
                items(remoteAssets.filter { f -> pictures.none { it.relativePath==f.path } },key={"remote:${it.path}"}) {file ->
                    val asset=detailState.assets.firstOrNull{it.relativePath==file.path&&it.available}
                    CardActions(file.title, menu = { close ->
                        CardAction("文件信息", close) { fileInfo = file.path }
                        if (asset == null) {
                            CardAction("下载附件", close, !busy) { download(paths=setOf(file.path)) }
                            if (file.kind == "image" && (file.downloadUrl ?: file.streamUrl) != null)
                                CardAction("查看图片", close) { image = file.downloadUrl ?: file.streamUrl }
                        } else LocalAssetMenu(asset, close, { image = it }, model.details::preview, { binding = it })
                    }) {
                        Column {
                            Text(file.title,style=MaterialTheme.typography.titleSmall)
                            Text("${file.format.uppercase()} · ${byteSize(file.bytes)}",style=MaterialTheme.typography.bodySmall)
                            VideoAttachmentHint(file.format)
                            Row {
                                if(asset==null) {
                                    if(file.kind=="image"&&(file.downloadUrl?:file.streamUrl)!=null)MotionTextButton({image=file.downloadUrl?:file.streamUrl}){Text("查看图片")}
                                    MotionTextButton({download(paths=setOf(file.path))},enabled=!busy){Text("下载附件")}
                                }
                                else AssetActions(asset,{image=it},{model.details.preview(it)},{binding=it})
                            }
                            HorizontalDivider()
                        }
                    }
                }
                if(remote==null)items(detailState.remoteAssets.filter{r->r.kind!="cover"&&detailState.assets.none{it.relativePath==r.relativePath}},key={"reference:${it.id}"}) {asset->
                    CardActions(asset.relativePath.substringAfterLast('/'), menu = { close ->
                        CardAction("文件信息", close) { fileInfo = asset.relativePath }
                        if (asset.kind == "image") CardAction("查看图片", close, !busy) { model.details.previewImage(asset) { image = it } }
                        val task = online.tasks.firstOrNull { it.assetId == asset.id }
                        CardAction("下载附件", close, !busy && task?.state !in setOf("QUEUED", "DOWNLOADING", "VERIFYING")) { download(ids=setOf(asset.id)) }
                    }) {
                        Column {
                            Text(asset.relativePath.substringAfterLast('/'),style=MaterialTheme.typography.titleSmall)
                            val task=online.tasks.firstOrNull{it.assetId==asset.id}
                            Text("${asset.format.uppercase()} · ${task?.let{downloadLabel(it.state)}?:"未下载"}",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                            VideoAttachmentHint(asset.format)
                            Row {
                                if(asset.kind=="image")MotionTextButton({model.details.previewImage(asset){image=it}},enabled=!busy){Text("查看图片")}
                                MotionTextButton({download(ids=setOf(asset.id))},enabled=!busy&&task?.state !in setOf("QUEUED","DOWNLOADING","VERIFYING")){Text("下载附件")}
                            }
                            HorizontalDivider()
                        }
                    }
                }
                items(detailState.assets.filter {a->a !in pictures&&remoteAssets.none{it.path==a.relativePath}},key={it.id}) {asset ->
                    CardActions(asset.relativePath.substringAfterLast('/'), menu = { close ->
                        CardAction("文件信息", close) { fileInfo = asset.relativePath }
                        if (asset.available) LocalAssetMenu(asset, close, { image = it }, model.details::preview, { binding = it })
                    }) {
                        Column {
                            Text(asset.relativePath.substringAfterLast('/'),style=MaterialTheme.typography.titleSmall)
                            VideoAttachmentHint(asset.format)
                            if (!asset.available) Text("文件不可用")
                            else if (!LibraryIndex.isVideoFormat(asset.format)) Row {
                                AssetActions(asset,{image=it},{model.details.preview(it)},{binding=it})
                            }
                            HorizontalDivider()
                        }
                    }
                }
            }
        }
        image?.let { uri -> FullscreenArtwork(uri, label = "附件图片") { image = null } }
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
private fun DetailChipRows(label: String, values: List<String>, isCast: Boolean, select: (String) -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top) {
        Box(Modifier.alignByBaseline().widthIn(min = 36.dp), contentAlignment = Alignment.CenterStart) {
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        BoxWithConstraints(Modifier.weight(1f).alignByBaseline()) {
            val viewportWidth = constraints.maxWidth
            Layout(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                content = {
                    values.forEach { name ->
                        Surface(
                            onClick = { select(name) },
                            shape = MaterialTheme.shapes.small,
                            color = if (isCast) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                            contentColor = if (isCast) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurfaceVariant
                        ) {
                            Box(Modifier.heightIn(min = 36.dp).padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                                Text(name, style = MaterialTheme.typography.labelLarge)
                            }
                        }
                    }
                }
            ) { measurables, bounds ->
                val chips = measurables.map { it.measure(Constraints(maxWidth = viewportWidth)) }
                val gap = 6.dp.roundToPx()
                val rowHeight = chips.maxOfOrNull { it.height } ?: 0
                val rowStep = rowHeight + 4.dp.roundToPx()
                var pageStart = 0
                var row = 0
                var x = 0
                var width = 0
                var height = rowHeight
                val positions = chips.map { chip ->
                    if (!isCast && x > 0 && x + chip.width > viewportWidth) {
                        if (row == 1) pageStart += viewportWidth + gap
                        row = 1 - row
                        x = 0
                    }
                    val left = pageStart + x
                    width = maxOf(width, left + chip.width)
                    height = maxOf(height, row * rowStep + chip.height)
                    x += chip.width + gap
                    left to row * rowStep
                }
                val baselines = chips.firstOrNull()?.let { mapOf<AlignmentLine, Int>(FirstBaseline to it[FirstBaseline]) }.orEmpty()
                layout(width.coerceAtLeast(bounds.minWidth), height, alignmentLines = baselines) {
                    chips.forEachIndexed { index, chip ->
                        val (left, top) = positions[index]
                        chip.placeRelative(left, top)
                    }
                }
            }
        }
    }
}

@Composable
private fun VideoAttachmentHint(format: String) {
    if (LibraryIndex.isVideoFormat(format)) MaintenanceHint("暂不支持预览")
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
private fun AssetActions(asset:WorkAsset,image:(String)->Unit,preview:(WorkAsset)->Unit,bind:(WorkAsset)->Unit) {
    if(asset.format in LibraryIndex.imageExtensions)MotionTextButton({image(asset.uri)}){Text("查看图片")}
    else if(asset.format in LibraryIndex.subtitleExtensions) {MotionTextButton({preview(asset)}){Text("预览字幕")};MotionTextButton({bind(asset)}){Text("关联音轨")}}
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
        Row(Modifier.fillMaxWidth().heightIn(min=56.dp).padding(vertical=3.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                if(current)AppGlyph(Glyph.Play,Modifier.width(28.dp),tint=MaterialTheme.colorScheme.primary)
                else Text("${index+1}",Modifier.width(28.dp).alignByBaseline(),style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                Column(Modifier.weight(1f).alignByBaseline().clickable(enabled=enabled,onClick=play).heightIn(min=48.dp), verticalArrangement=Arrangement.Center) {
                    Text(trackDisplayTitle(title),style=MaterialTheme.typography.titleSmall,maxLines=2,overflow=TextOverflow.Ellipsis,color=if(current)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    if(subtitle.isNotBlank())Text(subtitle,Modifier.padding(top=4.dp),style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            MotionIconToggleButton(favorite,{like()},Modifier.semantics{contentDescription=if(favorite)"取消书签" else "添加书签"}){AppGlyph(Glyph.TrackBookmark,filled=favorite,tint=if(favorite)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)}
            DetailActions("音轨操作",menu)
        }
    }
    HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.4f))
}

internal fun List<PlaybackPosition>.latestUnfinished(): PlaybackPosition? =
    filter { it.positionMs > 0 && !it.completed }.maxByOrNull { it.updatedMs }

private fun progressLabel(position: PlaybackPosition?): String? = when {
    position == null -> null
    position.completed -> "已听完"
    position.positionMs > 0 -> "听到 ${time(position.positionMs)}"
    else -> null
}
