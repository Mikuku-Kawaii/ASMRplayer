package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import kotlinx.coroutines.launch

@Composable
private fun NewPlaylistDialog(busy: Boolean, error: String?, create: (String) -> Unit, cancel: () -> Unit) {
    var name by rememberSaveable { mutableStateOf("") }
    val canCreate = name.isNotBlank() && !busy
    val submit = { if (canCreate) create(name.trim()) }
    PlayerAlertDialog(
        onDismissRequest = { if (!busy) cancel() },
        title = { Text("新建歌单") },
        text = {
            OutlinedTextField(name, { name = it.take(80) }, Modifier.fillMaxWidth(), enabled = !busy, singleLine = true,
                label = { Text("歌单名称") }, isError = error != null,
                supportingText = if (error != null) ({ Text(error) }) else null,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }))
        },
        confirmButton = { MotionTextButton(submit, enabled = canCreate) { Text("创建歌单") } },
        dismissButton = { MotionTextButton(cancel, enabled = !busy) { Text("取消") } }
    )
}

@Composable
private fun GroupFeedback(data: QueueGroupState) {
    data.error?.let { Text(it,color=MaterialTheme.colorScheme.error,style=MaterialTheme.typography.bodySmall) }

}

@UnstableApi
@Composable
private fun PlaylistArt(works:List<LibraryAudio>, modifier:Modifier) {
    val colors=MaterialTheme.colorScheme
    Box(modifier.clip(MaterialTheme.shapes.medium)
        .background(Brush.linearGradient(listOf(colors.primaryContainer,colors.tertiaryContainer))),contentAlignment=Alignment.Center) {
        when {
            works.size>=4 -> Column {
                for (row in 0..1) Row(Modifier.weight(1f)) {
                    for(col in 0..1) {
                        val w=works[row*2+col]
                        AudioArtwork(Modifier.weight(1f).fillMaxHeight(),w.coverUri,workId=w.id,shape=RectangleShape)
                    }
                }
            }
            works.isNotEmpty() -> AudioArtwork(Modifier.fillMaxSize(),works.first().coverUri,workId=works.first().id,shape=RectangleShape)
            else -> AppGlyph(Glyph.Queue,Modifier.fillMaxSize(.42f),colors.onPrimaryContainer)
        }
    }
}

@Composable
private fun playlistWorks(ids:List<String>,state:PlayerScreenState):List<LibraryAudio> =
    remember(ids,state.lookup,state.showNsfw,state.includeUnrated) {
        ids.asSequence().mapNotNull(state.lookup::forVersion).filter{visibleRating(it.rating,state)}.distinctBy{it.id}.take(4).toList()
    }

@UnstableApi
@Composable
internal fun QueueGroupsPage(state: PlayerScreenState, model: PlayerViewModel, bottom: Dp, open: (String)->Unit) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    var creating by rememberSaveable { mutableStateOf(false) }
    if (creating) NewPlaylistDialog(data.busy, data.error, { name ->
        model.queueGroups.create(name) { id -> creating = false; open("queue-group:$id") }
    }, { creating = false })
    LazyVerticalGrid(GridCells.Adaptive(145.dp * LocalDensity.current.fontScale.coerceIn(1f, 1.8f)),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = bottom),
        horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if(data.error!=null) item(span = { GridItemSpan(maxLineSpan) }) { GroupFeedback(data) }
        item(span = { GridItemSpan(maxLineSpan) }) {
            CardActions("正在播放", menu = { close ->
                CardAction("打开当前队列", close) { open("queue-current") }
                CardAction("打开播放器", close, state.queue.isNotEmpty()) { open("player") }
            }) {
                Surface(shape=MaterialTheme.shapes.large,color=MaterialTheme.colorScheme.primaryContainer) {
                    Row(Modifier.fillMaxWidth().clickable{open("queue-current")}.padding(16.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(12.dp)) {
                        PlaylistArt(listOfNotNull(state.currentWork?.takeIf{visibleRating(it.rating,state)}),Modifier.size(76.dp))
                        Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                            Text("当前队列 · ${state.queue.size} 首",style=MaterialTheme.typography.labelMedium,color=MaterialTheme.colorScheme.onPrimaryContainer)
                            Text(if(state.queue.isEmpty())"队列为空" else trackDisplayTitle(displayTitle(state)),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleMedium)
                            if(state.index in state.queue.indices) {
                                val status=when {state.buffering->"正在缓冲";state.ended->"已播放完";state.playRequested->"正在播放";else->"已暂停"}
                                Text("$status · 第 ${state.index+1} 首",style=MaterialTheme.typography.bodySmall)
                            }
                        }
                        AppGlyph(Glyph.Back,Modifier.rotate(180f))
                    }
                }
            }
        }
        item(span = { GridItemSpan(maxLineSpan) }) { Row(Modifier.fillMaxWidth().padding(top=8.dp),verticalAlignment=Alignment.CenterVertically) {
            Text("我的歌单",Modifier.weight(1f).semantics { heading() },style=MaterialTheme.typography.titleLarge)
            MotionTextButton({creating=true},enabled=!data.busy && !creating){AppGlyph(Glyph.Plus);Text("新建歌单")}
        } }

        if(data.ready && data.groups.isEmpty() && !creating) item(span = { GridItemSpan(maxLineSpan) }) { MaintenanceHint("新建歌单后，可以从音频库挑选音轨并保存播放顺序。") }
        items(data.groups,key={it.id}) { group ->
            val ids=remember(group){group.ids()}
            CardActions(group.name, menu = { close ->
                CardAction("打开歌单", close) { open("queue-group:${group.id}") }
                CardAction("播放歌单", close, state.connected && group.ids().isNotEmpty()) { model.playGroup(group.ids()) }
                CardAction("加入播放队列", close, state.connected && group.ids().isNotEmpty()) { model.enqueue(group.ids()) }
            }) {
                Column(Modifier.workListCard().clickable(onClickLabel = "打开歌单") { open("queue-group:${group.id}") }) {
                    PlaylistArt(playlistWorks(ids,state),Modifier.fillMaxWidth().aspectRatio(4f / 3f))
                    Row(Modifier.fillMaxWidth().padding(start=9.dp,end=4.dp,top=4.dp,bottom=4.dp),
                        verticalAlignment=Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(group.name,maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
                            Text("${ids.size} 首音轨",style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if(ids.isNotEmpty()) GlyphButton(Glyph.Play,"播放歌单 ${group.name}",{model.playGroup(ids)},state.connected)
                    }
                }
            }
        }

    }
}

@UnstableApi
@Composable
internal fun SavedQueuePage(id: String, state: PlayerScreenState, model: PlayerViewModel, bottom: Dp, back: ()->Unit) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    val group=data.groups.firstOrNull{it.id==id}
    if(group==null) { Column(Modifier.padding(20.dp)){GroupFeedback(data);if(data.ready)EmptyContent("歌单不存在","返回队列页查看其他歌单。",Glyph.Folder) else if(data.error==null)LoadingRing()};return }
    val ids=group.ids()
    var picker by remember { mutableStateOf(false) }
    var options by remember { mutableStateOf(false) }
    var editing by rememberSaveable(id) { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    var name by remember(group.name) { mutableStateOf(group.name) }
    val list=rememberLazyListState();val scope=rememberCoroutineScope()
    var dragging by remember {mutableIntStateOf(-1)};var target by remember {mutableIntStateOf(-1)};var dragY by remember{mutableFloatStateOf(0f)}
    Column {
        Column(Modifier.padding(horizontal=20.dp)) {
            GroupFeedback(data)
            if(renaming)Row(verticalAlignment=Alignment.CenterVertically) {
                OutlinedTextField(name,{name=it.take(80)},Modifier.weight(1f),singleLine=true,label={Text("歌单名称")})
                GlyphButton(Glyph.Check,"保存名称",{model.queueGroups.rename(id,name){renaming=false}},name.isNotBlank()&&!data.busy)
                GlyphButton(Glyph.Close,"取消修改",{renaming=false;name=group.name})
            } else Row(Modifier.fillMaxWidth().padding(vertical=8.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(16.dp)){
                PlaylistArt(playlistWorks(ids,state),Modifier.size(76.dp))
                Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(4.dp)){
                    Text(group.name,style=MaterialTheme.typography.titleLarge,maxLines=2,overflow=TextOverflow.Ellipsis)
                    Text("${ids.size} 首音轨",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Row(verticalAlignment=Alignment.CenterVertically) {
                if(ids.isEmpty()) MotionButton({picker=true},enabled=!data.busy){AppGlyph(Glyph.Plus);Text("添加音轨")}
                else MotionButton({model.playGroup(ids)},enabled=state.connected){AppGlyph(Glyph.Play);Text("播放全部")}
                Spacer(Modifier.weight(1f))
                if (ids.isNotEmpty()) MotionTextButton({ editing = !editing }) { Text(if (editing) "完成" else "编辑") }
                if(ids.isNotEmpty()) GlyphButton(Glyph.GroupAdd,"添加音轨",{picker=true})
                GlyphButton(Glyph.More,"歌单操作",{options=true})
            }
        }
        LazyColumn(Modifier.weight(1f),state=list,contentPadding=PaddingValues(start=20.dp,end=20.dp,bottom=bottom),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            if(ids.isEmpty())item { EmptyContent("还没有音轨","从音频库选几首加入这个歌单。",Glyph.Queue) }
            itemsIndexed(ids,key={_,version->version}) { index,version ->
                val work=state.lookup.forVersion(version)
                val entry=work?.versions?.firstOrNull{it.id==version};val hidden=work!=null&&!visibleRating(work.rating,state)
                val active=state.queue.getOrNull(state.index)?.id==version
                CardActions(if (hidden) "音轨操作" else entry?.title ?: "音轨操作", enabled = !editing, menu = { close ->
                    val playable = state.connected && entry?.available == true
                    CardAction("从此音轨播放", close, playable) { model.playGroup(ids, startVersion = version) }
                    CardAction("下一首播放", close, playable) { model.enqueue(listOf(version), true) }
                    CardAction("加入歌单", close, entry != null && !hidden) { model.choosePlaylist(listOf(version)) }
                    CardAction("编辑歌单", close) { editing = true }
                }) {
                    Surface(shape=MaterialTheme.shapes.medium,color=if(active)MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface) {
                        Row(Modifier.fillMaxWidth().padding(start=8.dp),verticalAlignment=Alignment.CenterVertically) {
                            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                                Text("${index+1}",Modifier.width(28.dp).alignByBaseline(),style=MaterialTheme.typography.labelSmall)
                                Column(Modifier.weight(1f).alignByBaseline().clickable(enabled=state.connected&&entry?.available==true){model.playGroup(ids,startVersion=version)}.padding(vertical=14.dp)) {
                                    Text(if(hidden)"内容已隐藏" else trackDisplayTitle(entry?.title?:"音轨不可用"),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
                                    Text(if(dragging==index)"松开移至第 ${target+1} 首" else if(active)if(state.playRequested)"正在播放" else "当前音轨 · 已暂停" else if(hidden)"" else work?.title.orEmpty(),maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                            if (editing) GlyphButton(Glyph.Close,"移出歌单",{model.queueGroups.remove(id,version)},!data.busy)
                            if(editing && ids.size>1)Box(Modifier.size(48.dp).semantics {
                                contentDescription="按住拖动调整顺序"
                                customActions=listOfNotNull(if(index>0)CustomAccessibilityAction("上移"){model.queueGroups.move(id,version,index-1,ids);true}else null,if(index<ids.lastIndex)CustomAccessibilityAction("下移"){model.queueGroups.move(id,version,index+1,ids);true}else null)
                            }.pointerInput(ids,data.busy){if(!data.busy)detectDragGesturesAfterLongPress(onDragStart={dragging=index;target=index;dragY=list.layoutInfo.visibleItemsInfo.firstOrNull{it.index==index}?.let{it.offset+it.size/2f}?:0f},onDragCancel={dragging=-1},onDragEnd={if(target!=index)model.queueGroups.move(id,version,target,ids);dragging=-1},onDrag={change,amount->change.consume();dragY+=amount.y;list.layoutInfo.visibleItemsInfo.firstOrNull{dragY>=it.offset&&dragY<=it.offset+it.size}?.let{target=it.index.coerceIn(ids.indices)};if(dragY>list.layoutInfo.viewportEndOffset-64)scope.launch{list.scrollBy(24f)}else if(dragY<64)scope.launch{list.scrollBy(-24f)}})},contentAlignment=Alignment.Center){AppGlyph(Glyph.Drag,tint=MaterialTheme.colorScheme.onSurfaceVariant)}
                        }
                    }
                }
            }
        }
    }
    if(picker)PlayerSheet("添加音轨",{picker=false},contentScrolls=true){GroupTrackPicker(state,ids,data.busy){selected->model.queueGroups.add(id,selected){picker=false}}}
    if(options)PlayerSheet("歌单操作",{options=false}) {
        SettingsLink("重命名",""){options=false;renaming=true}
        if(data.groups.indexOf(group)>0)SettingsLink("上移歌单",""){model.queueGroups.moveGroup(id,-1);options=false}
        if(data.groups.indexOf(group)<data.groups.lastIndex)SettingsLink("下移歌单",""){model.queueGroups.moveGroup(id,1);options=false}
        SettingsLink("删除歌单","保留音频文件与当前播放队列"){model.queueGroups.delete(group){options=false;back()}}
    }
}

@UnstableApi
@Composable
private fun GroupTrackPicker(state: PlayerScreenState, existing: List<String>, busy: Boolean, add: (List<String>)->Unit) {
    var query by rememberSaveable {mutableStateOf("")};var selected by rememberSaveable{mutableStateOf(listOf<String>())}
    val tracks=state.library.filter{it.inLibrary&&visibleRating(it.rating,state)}.flatMap{work->work.versions.filter{it.available}.map{work to it}}.filter{(work,track)->query.isBlank()||track.title.contains(query,true)||work.title.contains(query,true)}
    Column(Modifier.fillMaxWidth().padding(horizontal=20.dp)) {
        PageSearch(query,{query=it},"搜索音轨或作品")
        LazyColumn(Modifier.weight(1f, fill = false)) {items(tracks,key={it.second.id}){(work,track)->
            val included=track.id in existing
            Row(Modifier.fillMaxWidth().clickable(enabled=!included&&!busy){selected=if(track.id in selected)selected-track.id else selected+track.id}.padding(vertical=8.dp),verticalAlignment=Alignment.CenterVertically) {
                Checkbox(included||track.id in selected,null,enabled=!included&&!busy)
                Column(Modifier.weight(1f)){Text(trackDisplayTitle(track.title),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodyMedium);Text(if(included)"已在歌单中" else work.title,maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall)}
            }
        }}
        MotionButton({add(selected)},Modifier.fillMaxWidth().padding(vertical=12.dp),enabled=selected.isNotEmpty()&&!busy){Text("添加 ${selected.size} 首")}
    }
}

@UnstableApi
@Composable
internal fun AddCurrentToGroup(state: PlayerScreenState, model: PlayerViewModel) {
    val version = stableCurrent(state)
    AddTracksToGroup(listOfNotNull(version), model)
}

@UnstableApi
@Composable
internal fun AddTracksToGroup(ids: List<String>, model: PlayerViewModel) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    var creating by rememberSaveable(ids) { mutableStateOf(false) }
    GroupFeedback(data)
    if (ids.isEmpty()) Text("没有可加入的音轨。", style=MaterialTheme.typography.bodyMedium)
    else {
        Text("已选择 ${ids.size} 首音轨", style=MaterialTheme.typography.bodySmall, color=MaterialTheme.colorScheme.onSurfaceVariant)
        data.groups.forEach { group ->
            val included = ids.all { it in group.ids() }
            Row(Modifier.fillMaxWidth().heightIn(min=56.dp).clickable(enabled=!included&&!data.busy) {
                model.queueGroups.add(group.id, ids) { model.feedback("已加入「${group.name}」") }
            }, verticalAlignment=Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) { Text(group.name, style=MaterialTheme.typography.titleSmall);Text("${group.ids().size} 首", style=MaterialTheme.typography.bodySmall) }
                AppGlyph(if(included)Glyph.Check else Glyph.GroupAdd, tint=if(included)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        if (creating) NewPlaylistDialog(data.busy, data.error, { name ->
            model.queueGroups.create(name, ids) { creating = false; model.feedback("已加入「$name」") }
        }, { creating = false })
        else MotionOutlinedButton({creating=true}, Modifier.fillMaxWidth(), enabled=!data.busy) { AppGlyph(Glyph.GroupAdd);Spacer(Modifier.width(8.dp));Text("新建歌单并加入") }
    }
}
