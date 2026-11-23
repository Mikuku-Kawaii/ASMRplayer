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
import com.asmrplayer.android.data.CollectionWork
import com.asmrplayer.android.data.QueueGroup
import kotlinx.coroutines.launch

@Composable
private fun NewPlaylistDialog(label: String, busy: Boolean, error: String?, create: (String) -> Unit, cancel: () -> Unit) {
    var name by rememberSaveable { mutableStateOf("") }
    val canCreate = name.isNotBlank() && !busy
    val submit = { if (canCreate) create(name.trim()) }
    PlayerAlertDialog(
        onDismissRequest = { if (!busy) cancel() },
        title = { Text("新建$label") },
        text = {
            OutlinedTextField(name, { name = it.take(80) }, Modifier.fillMaxWidth(), enabled = !busy, singleLine = true,
                label = { Text("歌单名称") }, isError = error != null,
                supportingText = if (error != null) ({ Text(error) }) else null,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { submit() }))
        },
        confirmButton = { MotionTextButton(submit, enabled = canCreate) { Text("创建") } },
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
private fun playlistWorks(group:QueueGroup,state:PlayerScreenState):List<LibraryAudio> =
    remember(group,state.lookup,state.showNsfw,state.includeUnrated) {
        group.ids().asSequence().mapNotNull { if(group.isWorks) state.lookup.byId[it] else state.lookup.forVersion(it) }.filter{visibleRating(it.rating,state)}.distinctBy{it.id}.take(4).toList()
    }

@UnstableApi
@Composable
internal fun QueueGroupsPage(state: PlayerScreenState, model: PlayerViewModel, bottom: Dp, open: (String)->Unit) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    var creating by rememberSaveable { mutableStateOf(false) }
    var category by rememberSaveable { mutableStateOf("works") }
    val works = category=="works"
    val label = if(works) "作品集" else "音轨歌单"
    val groups = data.groups.filter { it.isWorks == works }
    if (creating) NewPlaylistDialog(label,data.busy, data.error, { name ->
        model.queueGroups.create(name,works=if(works) emptyList() else null) { id -> creating = false; open("queue-group:$id") }
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
            PageTabs(category,listOf("works" to "作品集","tracks" to "音轨歌单"),Modifier.weight(1f)) { category=it }
            MotionTextButton({ creating = true }, enabled = !data.busy && !creating) {
                AppGlyph(Glyph.Plus)
                Text("新建")
            }
        } }

        if(data.ready && groups.isEmpty() && !creating) item(span = { GridItemSpan(maxLineSpan) }) { MaintenanceHint(if(works) "从音频库或发现加入整部作品。" else "从作品详情挑选音轨，按喜欢的顺序收听。") }
        items(groups,key={it.id}) { group ->
            val ids=remember(group){group.ids()}
            CardActions(group.name, menu = { close ->
                CardAction("打开歌单", close) { open("queue-group:${group.id}") }
                CardAction("播放歌单", close, state.connected && group.ids().isNotEmpty()) { model.playSaved(group) }
                CardAction("加入播放队列", close, state.connected && group.ids().isNotEmpty()) { model.playSaved(group,append=true) }
            }) {
                Column(Modifier.workListCard().clickable(onClickLabel = "打开歌单") { open("queue-group:${group.id}") }) {
                    PlaylistArt(playlistWorks(group,state),Modifier.fillMaxWidth().aspectRatio(4f / 3f))
                    Row(Modifier.fillMaxWidth().padding(start=9.dp,end=4.dp,top=4.dp,bottom=4.dp),
                        verticalAlignment=Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(group.name,maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
                            Text("${ids.size} ${if(group.isWorks) "部作品" else "首音轨"}",style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if(ids.isNotEmpty()) GlyphButton(Glyph.Play,"播放歌单 ${group.name}",{model.playSaved(group)},state.connected)
                    }
                }
            }
        }

    }
}

@UnstableApi
@Composable
internal fun SavedQueuePage(id: String, state: PlayerScreenState, model: PlayerViewModel, bottom: Dp, back: ()->Unit, open:(String)->Unit) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    val group=data.groups.firstOrNull{it.id==id}
    if(group==null) { Column(Modifier.padding(20.dp)){GroupFeedback(data);if(data.ready)EmptyContent("歌单不存在","返回歌单页查看其他歌单。",Glyph.Folder) else if(data.error==null)LoadingRing()};return }
    val ids=remember(group){group.ids()}
    val collection by model.collections.state.collectAsStateWithLifecycle()
    val history=remember(collection.history) { collection.history.groupBy { it.workId } }
    val compatible=data.groups.filter { it.isWorks==group.isWorks }
    val itemName=if(group.isWorks) "作品" else "音轨"
    var converting by rememberSaveable(id) { mutableStateOf(false) }
    LaunchedEffect(id) { model.queueGroups.touch(id) }
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
                PlaylistArt(playlistWorks(group,state),Modifier.size(76.dp))
                Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(4.dp)){
                    Text(group.name,style=MaterialTheme.typography.titleLarge,maxLines=2,overflow=TextOverflow.Ellipsis)
                    Text("${ids.size} ${if(group.isWorks) "部作品" else "首音轨"}",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Row(verticalAlignment=Alignment.CenterVertically) {
                if(ids.isEmpty()) MotionButton({picker=true},enabled=!data.busy){AppGlyph(Glyph.Plus);Text("添加$itemName")}
                else MotionButton({model.playSaved(group)},enabled=state.connected){AppGlyph(Glyph.Play);Text("播放全部")}
                Spacer(Modifier.weight(1f))
                if (ids.isNotEmpty()) MotionTextButton({ editing = !editing }) { Text(if (editing) "完成" else "编辑") }
                if(ids.isNotEmpty()) GlyphButton(Glyph.GroupAdd,"添加$itemName",{picker=true})
                GlyphButton(Glyph.More,"歌单操作",{options=true})
            }
        }
        LazyColumn(Modifier.weight(1f),state=list,contentPadding=PaddingValues(start=20.dp,end=20.dp,bottom=bottom),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            if(ids.isEmpty())item { EmptyContent("还没有$itemName","从音频库选择要加入的$itemName。",Glyph.Queue) }
            itemsIndexed(ids,key={_,version->version}) { index,version ->
                val work=if(group.isWorks) state.lookup.byId[version] else state.lookup.forVersion(version)
                val entry=work?.versions?.firstOrNull{it.id==version};val hidden=work!=null&&!visibleRating(work.rating,state)
                val active=if(group.isWorks) state.currentWork?.id==version else state.queue.getOrNull(state.index)?.id==version
                val title=when { hidden->"内容已隐藏";group.isWorks->work?.title ?: "作品待匹配";else->entry?.title?.let(::trackDisplayTitle) ?: "音轨待匹配" }
                val heard=history[work?.id].orEmpty()
                val progress=when {
                    work==null -> "暂不可用"
                    heard.isEmpty() -> "未收听"
                    work.versions.isNotEmpty() && work.versions.map { it.trackId }.distinct().all { chapter->work.versions.any { v->v.trackId==chapter && heard.any { it.versionId==v.id && it.completed } } } -> "已听完"
                    else -> "听到 " + time(heard.first().positionMs)
                }
                val sub=when {hidden->"";group.isWorks->progress;else->listOfNotNull(work?.title,entry?.durationMs?.let(::time),entry?.format?.uppercase()).joinToString(" · ")}
                val canOpen=if(group.isWorks) work!=null && !hidden else state.connected && !hidden && entry?.available==true
                fun openItem() { if(group.isWorks) open("audio:$version") else model.playSaved(group,start=version) }
                CardActions(title, enabled = !editing, menu = { close ->
                    if(group.isWorks) CardAction("作品详情",close,canOpen) { openItem() }
                    else {
                        CardAction("从此音轨播放", close, canOpen) { openItem() }
                        CardAction("下一首播放", close, canOpen) { model.enqueue(listOf(version), true) }
                        CardAction("加入音轨歌单", close, entry != null && !hidden) { model.choosePlaylist(listOf(version)) }
                    }
                    CardAction("编辑歌单", close) { editing = true }
                }) {
                    Surface(shape=MaterialTheme.shapes.medium,color=if(active)MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface) {
                        Row(Modifier.fillMaxWidth().padding(start=8.dp),verticalAlignment=Alignment.CenterVertically) {
                            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                                Text("${index+1}",Modifier.width(28.dp).alignByBaseline(),style=MaterialTheme.typography.labelSmall)
                                if(group.isWorks && work!=null && !hidden) AudioArtwork(Modifier.padding(end=8.dp).size(44.dp),work.coverUri,workId=work.id)
                                Column(Modifier.weight(1f).clickable(enabled=canOpen){openItem()}.padding(vertical=12.dp)) {
                                    Text(title,maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
                                    Text(if(dragging==index)"松开移至第 ${target+1} 项" else sub,maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
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
    if(picker)PlayerSheet("添加$itemName",{picker=false},contentScrolls=true) {
        val choices=remember(state.lookup,state.showNsfw,state.includeUnrated,group.isWorks) {
            state.library.filter { it.inLibrary && visibleRating(it.rating,state) }.flatMap { w ->
                if(group.isWorks) listOf(PlaylistChoice(w.id,w.title,w.code.orEmpty()))
                else w.versions.filter { it.available }.map { PlaylistChoice(it.id,trackDisplayTitle(it.title),w.title+" · "+it.format.uppercase()) }
            }
        }
        PlaylistMemberChoices(choices,ids,data.busy,"添加",if(group.isWorks) Int.MAX_VALUE else 500) { selected ->
            model.queueGroups.addSelection(group,if(group.isWorks) emptyList() else selected,if(group.isWorks) selected.map(::CollectionWork) else null) { picker=false }
        }
    }
    if(converting) ConvertPlaylist(group,state,model,{converting=false}) { newId -> converting=false;open("queue-group:$newId") }
    if(options)PlayerSheet("歌单操作",{options=false}) {
        if(!group.isWorks && ids.isNotEmpty())SettingsLink("整理为作品集","按所属作品另存"){options=false;converting=true}
        if(ids.isNotEmpty())SettingsLink("加入播放队列",""){options=false;model.playSaved(group,append=true)}
        SettingsLink("重命名",""){options=false;renaming=true}
        if(compatible.indexOf(group)>0)SettingsLink("上移歌单",""){model.queueGroups.moveGroup(id,-1);options=false}
        if(compatible.indexOf(group)<compatible.lastIndex)SettingsLink("下移歌单",""){model.queueGroups.moveGroup(id,1);options=false}
        SettingsLink("删除歌单","保留音频文件与当前播放队列"){model.queueGroups.delete(group){options=false;back()}}
    }
}

internal data class PlaylistChoice(val id:String,val title:String,val subtitle:String)

@Composable
internal fun PlaylistMemberChoices(choices:List<PlaylistChoice>, existing:List<String>, busy:Boolean, verb:String, max:Int,
    initiallySelected:List<String> = emptyList(), enabled:Boolean = true, submit:(List<String>)->Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    var selected by rememberSaveable { mutableStateOf(initiallySelected) }
    val filtered=remember(query,choices) { choices.filter { it.title.contains(query.trim(),true) || it.subtitle.contains(query.trim(),true) } }
    Column(Modifier.fillMaxWidth()) {
        PageSearch(query,{query=it},"搜索名称")
        LazyColumn(Modifier.weight(1f,fill=false)) {
            if(filtered.isEmpty()) item { Text("没有符合条件的条目",Modifier.padding(vertical=12.dp)) }
            items(filtered,key={it.id}) { choice ->
                val included=choice.id in existing
                val checked=choice.id in selected
                val editable=!busy && !included && (checked || existing.size+selected.size<max)
                Row(Modifier.fillMaxWidth().clickable(enabled=editable){selected=if(checked) selected-choice.id else selected+choice.id}.padding(vertical=6.dp),verticalAlignment=Alignment.CenterVertically) {
                    Checkbox(included || checked,null,enabled=editable)
                    Column(Modifier.weight(1f)) {
                        Text(choice.title,maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodyMedium)
                        Text(if(included) "已加入" else choice.subtitle,style=MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
        MotionButton({submit(choices.map { it.id }.filter { it in selected })},Modifier.fillMaxWidth().padding(vertical=8.dp),enabled=enabled && !busy && selected.isNotEmpty()){Text("$verb（${selected.size}）")}
    }
}

@UnstableApi
@Composable
private fun ConvertPlaylist(group:QueueGroup,state:PlayerScreenState,model:PlayerViewModel,close:()->Unit,done:(String)->Unit) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    val choices=remember(group,state.lookup,state.showNsfw,state.includeUnrated) {
        group.ids().mapNotNull { id->state.lookup.forVersion(id)?.takeIf { visibleRating(it.rating,state) }?.let { it to id } }
            .groupBy({ it.first.id },{ it }).map { (id,members)->
                val work=members.first().first
                val versions=members.mapTo(hashSetOf()) { it.second }
                val tracks=work.versions.filter { it.id in versions }.distinctBy { it.trackId }.size
                PlaylistChoice(id,work.title,"已收 $tracks / ${work.versions.distinctBy { it.trackId }.size} 轨")
            }
    }
    var name by rememberSaveable(group.id) { mutableStateOf(group.name.take(74)+" · 作品集") }
    PlayerSheet("整理为作品集",close,contentScrolls=true) {
        Column {
            OutlinedTextField(name,{name=it.take(80)},Modifier.fillMaxWidth(),singleLine=true,label={ Text("新作品集名称") })
            GroupFeedback(data)
            PlaylistMemberChoices(choices,emptyList(),data.busy,"另存为作品集",Int.MAX_VALUE,choices.map { it.id },name.isNotBlank()) { ids->
                model.queueGroups.create(name,works=ids.map(::CollectionWork),done=done)
            }
        }
    }
}

@UnstableApi
@Composable
internal fun AddTracksToGroup(ids:List<String>, model:PlayerViewModel, works:List<CollectionWork>?=null, close:()->Unit={}) {
    val data by model.queueGroups.state.collectAsStateWithLifecycle()
    var creating by rememberSaveable { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    val label=if(works!=null) "作品集" else "音轨歌单"
    val recent=model.queueGroups.recent()
    val groups=data.groups.filter { it.isWorks==(works!=null) && it.name.contains(query.trim(),true) }
        .sortedBy { recent.indexOf(it.id).takeIf { i->i>=0 } ?: Int.MAX_VALUE }
    val members=works?.map { it.id } ?: ids
    Column(Modifier.fillMaxWidth()) {
        GroupFeedback(data)
        PageSearch(query,{query=it},"搜索$label")
        LazyColumn(Modifier.weight(1f,fill=false)) {
            if(data.ready && groups.isEmpty())item { Text("没有符合条件的$label",Modifier.padding(vertical=12.dp)) }
            items(groups,key={it.id}) { group->
                val included=members.all { it in group.ids() }
                Row(Modifier.fillMaxWidth().heightIn(min=56.dp).clickable(enabled=!included && !data.busy) { model.queueGroups.addSelection(group,ids,works,close) },verticalAlignment=Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) { Text(group.name,style=MaterialTheme.typography.titleSmall);Text("${group.ids().size} ${if(works!=null) "部作品" else "首音轨"}",style=MaterialTheme.typography.bodySmall) }
                    AppGlyph(if(included) Glyph.Check else Glyph.GroupAdd)
                }
            }
        }
        MotionOutlinedButton({creating=true},Modifier.fillMaxWidth(),enabled=!data.busy && members.isNotEmpty()){AppGlyph(Glyph.Plus);Text("新建并加入")}
    }
    if(creating)NewPlaylistDialog(label,data.busy,data.error,{name->model.queueGroups.create(name,ids,works){creating=false;close()}},{creating=false})
}
