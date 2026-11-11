package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.ui.input.pointer.pointerInput
import kotlinx.coroutines.launch
import androidx.compose.foundation.background
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.foundation.clickable
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.semantics.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import com.asmrplayer.android.data.VersionCollectionEntry
import com.asmrplayer.android.library.*
import java.text.DateFormat
import java.util.Date

internal fun visibleRating(rating: String, state: PlayerScreenState) =
    (rating != "NSFW" || state.showNsfw) && (rating != "UNRATED" || state.includeUnrated)

@Composable
internal fun <T> ChoiceMenu(label: String, value: T, choices: List<Pair<T, String>>, choose: (T) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        MotionOutlinedButton({ expanded = true }) { Text((if(label.isBlank())"" else "$label：")+choices.first { it.first == value }.second) }
        DropdownMenu(expanded, { expanded = false }) {
            choices.forEach { (key, title) -> DropdownMenuItem(text = { Text(title) }, onClick = { expanded = false; choose(key) }) }
        }
    }
}

@Composable
internal fun CollectionFeedback(data: CollectionScreenState, retry: () -> Unit, showLoading: Boolean = true) {
    if (data.error != null) {
        Text(data.error, color = MaterialTheme.colorScheme.error)
        MotionTextButton(retry) { Text("重试读取") }
    } else if (!data.ready && showLoading) CircularProgressIndicator(Modifier.size(24.dp))
}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun CollectionPage(favorites: Boolean, state: PlayerScreenState, model: PlayerViewModel, query: String,
    changeQuery: (String) -> Unit, bottom: Dp, importAudio: () -> Unit, browseOnline: () -> Unit,
    open: (String) -> Unit, active: Boolean) {
    val data by (if (favorites) model.collections.favoriteCatalog else model.collections.state).collectAsStateWithLifecycle()
    var category by rememberSaveable { mutableStateOf("works") }
    var encoded by rememberSaveable { mutableStateOf(LibraryQuery().encode()) }
    var workSearch by rememberSaveable { mutableStateOf("") }
    var trackSearch by rememberSaveable { mutableStateOf("") }
    var markerSearch by rememberSaveable { mutableStateOf("") }
    var searching by rememberSaveable { mutableStateOf(false) }
    val recordState = rememberSaveableStateHolder()
    val effectiveQuery = when {
        !favorites -> query
        category == "tracks" -> trackSearch
        category == "bookmarks" -> markerSearch
        else -> workSearch
    }
    fun updateSearch(text: String) {
        when (category) {
            "tracks" -> trackSearch = text
            "bookmarks" -> markerSearch = text
            else -> workSearch = text
        }
    }
    val condition=remember(encoded,favorites){decodeLibraryQuery(encoded).copy(favoritesOnly=favorites)}
    var filters by remember { mutableStateOf(false) }
    val gridState=rememberLazyGridState()
    var anchor by remember { mutableStateOf<Pair<String, Int>?>(null) }
    fun applyFilter(value: LibraryQuery) {
        val next = value.encode()
        if (next == encoded) return
        val first = gridState.layoutInfo.visibleItemsInfo.firstOrNull { it.index == gridState.firstVisibleItemIndex }
        anchor = (first?.key as? String)?.let { it to gridState.firstVisibleItemScrollOffset } ?: anchor
        encoded = next
    }
    val requested = condition.copy(text = if (favorites) workSearch else query)
    LaunchedEffect(requested) {
        if (favorites) model.collections.queryFavoriteWorks(requested) else model.collections.query(requested)
    }
    val trackFilters = rememberRecordFilters()
    val markerFilters = rememberRecordFilters()
    val recordFilters = when (category) { "tracks" -> trackFilters; "bookmarks" -> markerFilters; else -> null }
    val catalog = if (data.query == requested) data else CollectionScreenState(query = requested)
    val works = remember(catalog.works, state.showNsfw, state.includeUnrated) { catalog.works.filter { visibleRating(it.work.rating, state) } }
    val libraryIsEmpty = remember(state.lookup) { state.library.none { it.inLibrary } }
    val showWelcome = !favorites && catalog.ready && catalog.error == null &&
        works.isEmpty() && state.libraryReady && libraryIsEmpty && query.isBlank() && condition.conditionCount == 0
    Column {
        val searchOpen = favorites && (searching || effectiveQuery.isNotEmpty())
        val hint = when (category) {
            "tracks" -> "搜索音轨或作品"
            "bookmarks" -> "搜索备注、作品或音轨"
            else -> "搜索作品、声优或标签"
        }
        HomeHeader {
            if (searchOpen) {
                val focus = remember { FocusRequester() }
                androidx.activity.compose.BackHandler(enabled = active) { updateSearch(""); searching = false }
                HomeSearch(effectiveQuery, ::updateSearch, hint, Modifier.weight(1f).focusRequester(focus))
                LaunchedEffect(category, searching) { if (searching && active) focus.requestFocus() }
                GlyphButton(Glyph.Back, "结束搜索", { updateSearch(""); searching = false })
            } else {
                val title = if (favorites) "喜欢" else works.size.takeIf { catalog.ready && !showWelcome }?.let { "音频库 · $it" } ?: "音频库"
                Text(title, Modifier.weight(1f)
                    .semantics { heading() }, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (favorites) GlyphButton(Glyph.Search, "搜索当前分栏", { searching = true })
                else {
                    GlyphButton(Glyph.History, "收听记录", { open("history") })
                    GlyphButton(Glyph.Plus, "导入作品", importAudio)
                }
                if (favorites) FilterButton("筛选与排序", recordFilters?.count ?: condition.conditionCount) {
                    if (recordFilters != null) recordFilters.open.value = true else filters = true
                }
                GlyphButton(Glyph.Settings, "设置", { open("settings") })
            }
        }
        if (favorites) {
            val options = remember { listOf("works" to "作品", "tracks" to "收藏音轨", "bookmarks" to "时间标记") }
            PageTabs(category, options, Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                category = it; searching = false
            }
        }
        if (favorites && recordFilters != null) {
            recordState.SaveableStateProvider(category) {
                CollectionList(category, state, model, effectiveQuery, bottom, open, recordFilters)
            }
        } else {
            val showSummary = condition.conditionCount > 0
            val loadingUnfilteredLibrary = !favorites && !catalog.ready && catalog.error == null &&
                query.isBlank() && !showSummary && libraryIsEmpty
            val hasHeader = (!favorites && !showWelcome && !loadingUnfilteredLibrary) || showSummary || catalog.error != null
            LaunchedEffect(catalog.ready, catalog.query, anchor) {
                val saved = anchor ?: return@LaunchedEffect
                if (catalog.ready) {
                    val row = works.indexOfFirst { it.work.id == saved.first }
                    if (row >= 0) gridState.scrollToItem(row + (if (hasHeader) 1 else 0), saved.second)
                    anchor = null
                }
            }
            CatalogGrid(gridState, bottom) {
                if (hasHeader) item(span = { GridItemSpan(maxLineSpan) }) {
                    Column {
                        if (!favorites) {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                HomeSearch(query, changeQuery, "搜索作品、声优或标签", Modifier.weight(1f))
                                FilterButton("筛选作品", condition.conditionCount) { filters = true }
                            }
                        }
                        LibraryFilterSummary(condition){applyFilter(it)}
                        CollectionFeedback(catalog, if (favorites) model.collections::retryFavoriteWorks else model.collections::retry, showLoading = false)
                    }
                }
                if (loadingUnfilteredLibrary) item(span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth().padding(vertical = 28.dp), contentAlignment = Alignment.Center) {
                        CollectionFeedback(catalog, model.collections::retry)
                    }
                } else if (showWelcome) item(span = { GridItemSpan(maxLineSpan) }) {
                    LibraryWelcome(importAudio, browseOnline)
                } else if (catalog.ready && catalog.error == null && works.isEmpty()) item(span = { GridItemSpan(maxLineSpan) }) {
                    Column {
                        EmptyContent("没有匹配的作品", "可更换搜索与筛选条件，或检查内容显示设置。", Glyph.Library)
                        if (!favorites) MotionButton(importAudio) { Text("导入作品") }
                    }
                }
                items(works, key = { it.work.id }, contentType = { "work" }) { summary ->
                    val work = summary.work
                    val meta=state.metadata[work.id]
                    val versions = state.lookup.byId[work.id]?.versions.orEmpty()
                    CardActions(work.title, menu = { close ->
                        val first = versions.firstOrNull { it.available }
                        val ids = versions.filter { it.available && it.groupPath == first?.groupPath && it.format == first?.format }.map { it.id }
                        val playable = state.connected && ids.isNotEmpty()
                        CardAction("作品详情", close) { open("audio:${work.id}") }
                        CardAction("播放作品", close, playable) { model.playGroup(ids) }
                        CardAction("下一首播放", close, playable) { model.enqueue(ids, true) }
                        CardAction("加入播放队列", close, playable) { model.enqueue(ids) }
                        CardAction("加入歌单", close, ids.isNotEmpty()) { model.choosePlaylist(com.asmrplayer.android.data.preferredPlaylistVersions(versions)) }
                        CardAction(if (work.favorite) "取消喜欢" else "喜欢作品", close) { model.toggleFavorite(work.id) }
                        CardAction(if (work.inLibrary) "移出音频库" else "加入音频库", close) {
                            model.setLibraryMembership(work.id, !work.inLibrary)
                        }
                    }) {
                        WorkCard(work.title,work.coverUri,work.code,
                            listOf(if(state.playRequested && state.currentWork?.id == work.id)"正在播放" else if(summary.completedCount==summary.versionCount&&summary.versionCount>0)"听过" else if(summary.playedCount>0)"未听完" else "未听",
                                if(summary.importedCount>0)"本地" else if(summary.downloadedCount>0)"已下载" else "在线"),
                            if(meta?.voices().isNullOrEmpty())"${summary.versionCount} 首音轨" else "",meta?.tags().orEmpty(),
                            {open("audio:${work.id}")},onTag={tag->applyFilter(condition.copy(tags=(condition.tags+tag).distinct().take(5)))},
                            favorite=work.favorite,like={model.toggleFavorite(work.id)},nsfw=work.rating=="NSFW",hasSubtitle=summary.hasSubtitle,
                            inLibrary=work.inLibrary,save={ model.setLibraryMembership(work.id, !work.inLibrary) },workId=work.id,voices=meta?.voices().orEmpty(),onVoice={voice->applyFilter(condition.copy(voices=(condition.voices+voice).distinct()))})
                    }

                }
            }
        }
    }
    if (filters) LibraryFilterSheet(condition, state, effectiveQuery, model.collections::previewCount, { filters = false }) { next ->
        applyFilter(next)
    }

}

internal class RecordFilters(val available: MutableState<Boolean>, val work: MutableState<String>,
    val reverse: MutableState<Boolean>, val open: MutableState<Boolean>) {
    val count get() = (if (available.value) 1 else 0) + (if (work.value.isNotBlank()) 1 else 0)
}

@Composable
private fun rememberRecordFilters(): RecordFilters {
    val available = rememberSaveable { mutableStateOf(false) }
    val work = rememberSaveable { mutableStateOf("") }
    val reverse = rememberSaveable { mutableStateOf(false) }
    return remember { RecordFilters(available, work, reverse, mutableStateOf(false)) }
}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun CollectionList(category: String, state: PlayerScreenState, model: PlayerViewModel, query: String,
    bottom: Dp, open: (String) -> Unit, sharedFilters: RecordFilters? = null) {
    val data by model.collections.state.collectAsStateWithLifecycle()
    var editing by remember { mutableStateOf<com.asmrplayer.android.data.Bookmark?>(null) }
    val controls = sharedFilters ?: rememberRecordFilters()
    var onlyAvailable by controls.available
    var workFilter by controls.work
    var reverse by controls.reverse
    var listFilters by controls.open
    if (listFilters) {
        val options = remember(state.lookup, state.showNsfw, state.includeUnrated) {
            state.library.filter { visibleRating(it.rating, state) }.map { FilterOption(it.id, it.title) }
        }
        RecordFilterSheet(onlyAvailable, workFilter, reverse, options, { listFilters = false }) { available, work, ascending ->
            onlyAvailable = available; workFilter = work; reverse = ascending
        }
    }
    val term = query.trim()
    fun matches(vararg values: String?) = term.isEmpty() || values.any { it?.contains(term, ignoreCase = true) == true }
    val versions = (if (category == "history") data.history else data.favorites).filter {
        visibleRating(it.rating, state) && (!onlyAvailable||it.available) && (workFilter.isEmpty()||it.workId==workFilter) && matches(it.title, it.workTitle, it.workCode, it.groupPath)
    }
    val bookmarks = data.bookmarks.filter { visibleRating(it.rating, state) && (!onlyAvailable||it.available) && (workFilter.isEmpty()||it.workId==workFilter) && matches(it.title, it.workTitle, it.workCode, it.bookmark.note) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 20.dp, end = 20.dp, top = 8.dp, bottom = bottom),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (sharedFilters == null || onlyAvailable || workFilter.isNotBlank() || data.error != null) item {
            if (sharedFilters == null) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    MotionTextButton({ listFilters = true }) { Text("筛选记录") }
                    MotionTextButton({ reverse = !reverse }) { Text(if (reverse) "时间升序" else "时间降序") }
                }
            }
            if (onlyAvailable || workFilter.isNotBlank()) FilterStrip {
                if (onlyAvailable) RemovableChip({ onlyAvailable = false }) { Text("仅可用文件") }
                if (workFilter.isNotBlank()) RemovableChip({ workFilter = "" }) { Text(state.lookup.byId[workFilter]?.title ?: "所属作品", maxLines = 1) }
                MotionTextButton({ onlyAvailable = false; workFilter = "" }) { Text("清空条件") }
            }
            CollectionFeedback(data, model.collections::retry, showLoading = category == "history")
        }
        if (data.ready && (if (category == "bookmarks") bookmarks.isEmpty() else versions.isEmpty())) item {
            val filtered = term.isNotEmpty() || workFilter.isNotBlank() || onlyAvailable
            val (title, detail) = when {
                filtered -> "没有匹配的记录" to "可以更换关键词或调整筛选条件。"
                category == "bookmarks" -> "暂无时间标记" to "听到想再听的片段时，可以在播放器里保存标记。"
                category == "tracks" -> "暂无收藏音轨" to "在作品详情里点音轨旁的书签，即可在这里回听。"
                else -> "暂无收听记录" to "播放过的音轨会显示在这里。"
            }
            EmptyContent(title, detail, if(category=="bookmarks")Glyph.Marker else if(category=="history")Glyph.History else Glyph.TrackBookmark)
        }
        if (category == "bookmarks") items(if(reverse)bookmarks.reversed() else bookmarks, key = { it.bookmark.id }) { entry ->
            val actions: CardMenu = { close ->
                CardAction("加入歌单", close) { model.choosePlaylist(listOf(entry.bookmark.versionId)) }
                DropdownMenuItem(text={Text("编辑备注")},onClick={close();editing=entry.bookmark})
                DropdownMenuItem(text={Text("作品详情")},onClick={close();open("audio:${entry.workId}")})
                DropdownMenuItem(text={Text("删除标记")},enabled=entry.bookmark.id !in data.busyIds,onClick={close();model.collections.removeBookmark(entry.bookmark.id)})
            }
            CardActions(entry.bookmark.note.ifBlank { trackDisplayTitle(entry.title) }, menu = actions) {
                Row(Modifier.workListCard().heightIn(min = 64.dp).padding(start = 10.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Row(Modifier.weight(1f).clickable(enabled = state.connected && entry.available, onClickLabel = "从标记播放") {
                        model.playBookmark(entry.bookmark.versionId, entry.bookmark.positionMs); open("player")
                    }.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(time(entry.bookmark.positionMs), style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.primary, modifier = Modifier.widthIn(min = 52.dp).alignByBaseline())
                        Column(Modifier.alignByBaseline()) {
                            val track = trackDisplayTitle(entry.title)
                            Text(entry.bookmark.note.ifBlank { track }, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(if (entry.bookmark.note.isBlank()) entry.workTitle else "${entry.workTitle} · $track", style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            if (!entry.available) Text("来源不可用", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
                        }
                    }
                    MoreActions("标记操作", content = actions)
                }
            }
        } else itemsIndexed(if(reverse)versions.reversed() else versions, key = { _,entry -> entry.versionId }) { index,entry ->
            val ordered=if(reverse)versions.reversed() else versions
            val date=collectionDate(entry.lastListenedMs)
            if(category=="history"&&(index==0||collectionDate(ordered[index-1].lastListenedMs)!=date))Text(date,Modifier.padding(top=12.dp,bottom=6.dp),style=MaterialTheme.typography.labelLarge,color=MaterialTheme.colorScheme.onSurfaceVariant)
            val liked = data.favorites.any { it.versionId == entry.versionId }
            val playable = state.connected && entry.available
            CardActions(trackDisplayTitle(entry.title), menu = { close ->
                CardAction("播放音轨", close, playable) { model.playVersion(entry.versionId); open("player") }
                CardAction("下一首播放", close, playable) { model.enqueue(listOf(entry.versionId), true) }
                CardAction("加入播放队列", close, playable) { model.enqueue(listOf(entry.versionId)) }
                CardAction("加入歌单", close) { model.choosePlaylist(listOf(entry.versionId)) }
                CardAction(if (liked) "取消收藏音轨" else "收藏音轨", close, entry.versionId !in data.busyIds) {
                    model.collections.favorite(entry.versionId, !liked)
                }
                CardAction("作品详情", close) { open("audio:${entry.workId}") }
            }) {
                VersionCollectionCard(entry, state.connected, liked, entry.versionId in data.busyIds,
                    { model.playVersion(entry.versionId); open("player") }, { model.collections.favorite(entry.versionId, it) }, { open("audio:${entry.workId}") },state.lookup.byId[entry.workId]?.coverUri)
            }
        }
    }
    editing?.let { bookmark ->
        var note by remember(bookmark.id) { mutableStateOf(bookmark.note) }
        PlayerAlertDialog(onDismissRequest={editing=null},title={Text("标记备注")},text={OutlinedTextField(note,{note=it.take(4096)},modifier=Modifier.fillMaxWidth())},confirmButton={MotionTextButton({model.collections.editBookmark(bookmark.id,note);editing=null}){Text("保存")}},dismissButton={MotionTextButton({editing=null}){Text("取消")}})
    }

}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun VersionCollectionCard(entry: VersionCollectionEntry, connected: Boolean, favorite: Boolean, busy: Boolean,
    play: () -> Unit, like: (Boolean) -> Unit, details: () -> Unit, cover:String?) {
    Row(Modifier.workListCard().padding(start=10.dp,end=4.dp,top=8.dp,bottom=8.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
        AudioArtwork(Modifier.width(64.dp).aspectRatio(4f/3f).clickable(onClick=details),cover,workId=entry.workId)
        Column(Modifier.weight(1f).clickable(enabled=connected&&entry.available,onClick=play),verticalArrangement=Arrangement.spacedBy(4.dp)) {
            Text(trackDisplayTitle(entry.title),style=MaterialTheme.typography.titleSmall,maxLines=2,overflow=TextOverflow.Ellipsis)
            Text(entry.workTitle,style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant,maxLines=1,overflow=TextOverflow.Ellipsis)
            if (entry.completed || entry.positionMs > 0) Text(if(entry.completed)"已听完" else "上次听到 ${time(entry.positionMs)}",style=MaterialTheme.typography.labelMedium,color=MaterialTheme.colorScheme.primary)
            if(entry.durationMs>0 && entry.positionMs>0)LinearProgressIndicator(progress={(entry.positionMs.toFloat()/entry.durationMs).coerceIn(0f,1f)},modifier=Modifier.fillMaxWidth().height(2.dp))
            if(!entry.available) Text("来源不可用",style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.error)
        }
        MotionIconToggleButton(favorite,{like(it)},enabled=!busy,modifier=Modifier.semantics{contentDescription=if(favorite)"取消书签" else "添加书签"}){AppGlyph(Glyph.TrackBookmark,filled=favorite,tint=if(favorite)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)}
    }
}

@UnstableApi
@Composable
internal fun HistoryPage(state: PlayerScreenState, model: PlayerViewModel, open: (String) -> Unit) {
    var query by rememberSaveable { mutableStateOf("") }
    Column(Modifier.navigationBarsPadding()) {
        PageSearch(query,{query=it},"搜索收听记录",Modifier.padding(PageSpace.padding))
        CollectionList("history", state, model, query, 24.dp, open)
    }
}

@UnstableApi
@Composable
internal fun QueuePage(state: PlayerScreenState, model: PlayerViewModel, bottom: Dp, locateCurrent: Boolean = false, directControls: Boolean = false) {
    val list = androidx.compose.foundation.lazy.rememberLazyListState()
    val scope=rememberCoroutineScope()
    var editing by rememberSaveable { mutableStateOf(false) }
    val expected = remember(state.queue) { state.queue.map { it.id } }
    var located by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(locateCurrent,state.queue.isNotEmpty()) {
        if(locateCurrent&&!located&&state.index>=0) { list.scrollToItem(state.index);located=true }
    }
    var dragIndex by remember { mutableIntStateOf(-1) }
    var targetIndex by remember { mutableIntStateOf(-1) }
    var dragY by remember { mutableFloatStateOf(0f) }
    Column {
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("${state.queue.size} 首音轨", Modifier.weight(1f), style = MaterialTheme.typography.labelLarge)
            MotionTextButton({ model.playbackOrder(state.playbackOrder.next()) }, enabled = state.connected) {
                AppGlyph(state.playbackOrder.glyph, Modifier.size(18.dp))
                Text(state.playbackOrder.label, Modifier.padding(start = 4.dp))
            }
            if (state.queue.isNotEmpty()) MotionTextButton({ editing = !editing; dragIndex = -1 }) { Text(if (editing) "完成" else "编辑") }
        }
        LazyColumn(state=list,contentPadding=PaddingValues(start=20.dp,end=20.dp,top=4.dp,bottom=bottom),verticalArrangement=Arrangement.spacedBy(4.dp)) {
            if(state.queue.isEmpty())item { EmptyContent("队列还是空的","从音频库选择音轨开始收听。",Glyph.Queue) }
            itemsIndexed(state.queue,key={index,entry->"$index:${entry.id}"}) { index,entry ->
                val work=state.lookup.forVersion(entry.id)
                val hidden=work!=null&&!visibleRating(work.rating,state)
                CardActions(if (hidden) "音轨操作" else entry.title, enabled = !editing, menu = { close ->
                    CardAction("播放此音轨", close, state.connected) { model.select(index) }
                    CardAction("加入歌单", close, !hidden && work != null) { model.choosePlaylist(listOf(entry.id)) }
                    CardAction("编辑队列", close) { editing = true }
                }) {
                    Surface(shape=MaterialTheme.shapes.medium,color=if(index==state.index)MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerLow) {
                        Row(Modifier.fillMaxWidth().padding(start=10.dp,end=2.dp,top=4.dp,bottom=4.dp),verticalAlignment=Alignment.CenterVertically) {
                            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                                Text("${index + 1}", Modifier.widthIn(min = 28.dp).padding(end = 4.dp),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = if (index == state.index) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                                Column(Modifier.weight(1f).clickable(enabled=state.connected && !editing){model.select(index)}.padding(vertical=10.dp)) {
                                    Text(if(hidden)"内容已隐藏" else trackDisplayTitle(work?.versions?.firstOrNull{it.id==entry.id}?.title?.takeIf{it.isNotBlank()}?:entry.title),fontWeight=if(index==state.index)FontWeight.SemiBold else FontWeight.Medium,style=MaterialTheme.typography.titleSmall,maxLines=2,overflow=TextOverflow.Ellipsis)
                                    val showWork = index == 0 || state.lookup.forVersion(state.queue[index-1].id)?.id != work?.id
                                    if (index == state.index || dragIndex == index || (!hidden && showWork && work != null)) Text(if(dragIndex==index)"松开移至第 ${targetIndex+1} 首" else if(index==state.index)if(state.playRequested)"正在播放" else "当前音轨 · 已暂停" else if(hidden)"" else work?.title.orEmpty(),
                                        maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                            if (editing) CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurface.copy(alpha=if(directControls).58f else 1f)) {
                                GlyphButton(Glyph.Close,"移出队列：${if(hidden)"内容已隐藏" else entry.title}",{model.removeQueue(index,expected)},state.connected)
                            }
                            if(editing && state.queue.size>1)Box(Modifier.size(48.dp).semantics {
                                contentDescription="按住拖动调整顺序"
                                if(directControls)customActions=listOfNotNull(
                                    if(state.connected&&index>0)CustomAccessibilityAction("上移"){model.moveQueue(index,index-1,expected);true} else null,
                                    if(state.connected&&index<state.queue.lastIndex)CustomAccessibilityAction("下移"){model.moveQueue(index,index+1,expected);true} else null)
                            }
                                .pointerInput(expected,state.connected) { if(state.connected)detectDragGesturesAfterLongPress(
                                    onDragStart={dragIndex=index;targetIndex=index;dragY=list.layoutInfo.visibleItemsInfo.firstOrNull { it.index==index }?.let {it.offset+it.size/2f}?:0f},
                                    onDragCancel={dragIndex=-1},onDragEnd={if(targetIndex!=index&&targetIndex in expected.indices)model.moveQueue(index,targetIndex,expected);dragIndex=-1},
                                    onDrag={change,amount->
                                        change.consume();dragY+=amount.y
                                        list.layoutInfo.visibleItemsInfo.firstOrNull {dragY>=it.offset&&dragY<=it.offset+it.size}?.let { targetIndex=it.index.coerceIn(expected.indices) }
                                        if(dragY>list.layoutInfo.viewportEndOffset-64)scope.launch {list.scrollBy(24f)}
                                        else if(dragY<64)scope.launch {list.scrollBy(-24f)}
                                    }) },contentAlignment=Alignment.Center) {
                                if(directControls)AppGlyph(Glyph.Drag,tint=MaterialTheme.colorScheme.onSurface.copy(alpha=.58f))
                                else MoreActions("长按拖动调整顺序 ${index+1}",Glyph.Drag) { close ->
                                    DropdownMenuItem(text={Text("上移")},enabled=state.connected&&index>0,onClick={close();model.moveQueue(index,index-1,expected)})
                                    DropdownMenuItem(text={Text("下移")},enabled=state.connected&&index<state.queue.lastIndex,onClick={close();model.moveQueue(index,index+1,expected)})
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun collectionDate(timestamp: Long?): String {
    if(timestamp==null)return "更早"
    val day=java.util.Calendar.getInstance().apply { timeInMillis=timestamp }
    val today=java.util.Calendar.getInstance()
    fun same()=day.get(java.util.Calendar.YEAR)==today.get(java.util.Calendar.YEAR)&&day.get(java.util.Calendar.DAY_OF_YEAR)==today.get(java.util.Calendar.DAY_OF_YEAR)
    if(same())return "今天"
    today.add(java.util.Calendar.DATE,-1)
    return if(same())"昨天" else java.text.DateFormat.getDateInstance().format(java.util.Date(timestamp))
}
