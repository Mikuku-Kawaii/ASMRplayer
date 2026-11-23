package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.autofill.ContentType
import androidx.compose.ui.semantics.contentType
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import com.asmrplayer.android.online.OnlineFile
import com.asmrplayer.android.online.BrowseFilters
import com.asmrplayer.android.online.RecommendationIdentity
import com.asmrplayer.android.online.ProviderAccount
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import java.util.Locale

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun DiscoverPage(data: OnlineScreenState, tools: OnlineTools, bottom: Dp, state:PlayerScreenState,
    active: Boolean = true,
    openSiteAccount: () -> Unit, download: (String, String) -> Unit, open: (String, String) -> Unit) {
    var query by rememberSaveable { mutableStateOf(data.query) }
    LaunchedEffect(data.query) { query=data.query }
    val grid = rememberLazyGridState()
    val categories = listOf(Triple("popular", "热门作品", Glyph.Flame), Triple("recommend", "推荐作品", Glyph.Discover),
        Triple("dl_count", "销量榜", Glyph.Sales),
        Triple("rate_average_2dp", "好评榜", Glyph.Star), Triple("create_date", "最新收录", Glyph.NewArrival),
        Triple("release", "最新发售", Glyph.Release), Triple("review_count", "热议榜", Glyph.Discussion))
    val categoryScroll=rememberLazyListState()
    val motion = LocalUiMotion.current
    LaunchedEffect(data.order,motion){
        val target=categories.indexOfFirst{it.first==data.order}.coerceAtLeast(0)
        if(motion) categoryScroll.animateScrollToItem(target) else categoryScroll.scrollToItem(target)
    }
    var filterOpen by remember { mutableStateOf(false) }
    var anchor by remember(data.account) { mutableStateOf<Pair<String, Int>?>(null) }
    fun filter(conditions: BrowseFilters = data.filters, ascending: Boolean = data.ascending, term: String = data.query, order: String = data.order) {
        val first = grid.layoutInfo.visibleItemsInfo.firstOrNull { it.index == grid.firstVisibleItemIndex }
        anchor = (first?.key as? String)?.let { it to grid.firstVisibleItemScrollOffset } ?: anchor
        tools.search(term, order=order, ascending=ascending, conditions=conditions)
    }
    if (filterOpen) {
        val voices = remember(data.page) { data.page?.items.orEmpty().flatMap { it.voices }.distinct() }
        val circles = remember(data.page) { data.page?.items.orEmpty().map { it.circle }.filter { it.isNotBlank() }.distinct() }
        OnlineFilterSheet(data.filters, data.tagOptions, data.tagCounts, data.tagsError, voices, circles,
            data.order, data.ascending, categories.map { (id, label, _) ->
                id to if (id == "popular" && query.isNotBlank()) "搜索结果" else label
            }, { filterOpen = false }) { conditions, order, ascending ->
            filter(conditions, ascending, query, order)
        }
    }
    fun tag(value:String) {
        val remaining = data.filters.excludedTags.filterNot { it.equals(value, true) }
        filter(data.filters.copy(tags=(data.filters.tags+value).distinct().take(5-remaining.size), excludedTags=remaining))
    }
    fun voice(value:String) { filter(data.filters.copy(voices=(data.filters.voices+value).distinct().take(5))) }
    var shownPage by rememberSaveable { mutableStateOf("") }
    val pageKey = "${data.account.apiBase}|${data.feedRevision}"
    LaunchedEffect(data.page, data.loading, data.error, pageKey) {
        if (data.loading || data.page == null) return@LaunchedEffect
        val saved = anchor
        anchor = null
        val row = saved?.let { (key, _) ->
            data.page.items.indexOfFirst { data.account.apiBase + it.sourceId == key }
        } ?: -1
        if (saved != null && row >= 0)
            grid.scrollToItem(row + 1 + (if (data.error != null) 1 else 0), saved.second)
        else if (shownPage != pageKey) grid.scrollToItem(0)
        shownPage = pageKey
    }
    LaunchedEffect(active, data.feedRevision, data.page?.page, data.loading, data.loadingMore, data.moreError, data.error) {
        if (!active || data.loading || data.loadingMore || data.moreError != null || data.error != null || !data.hasMore) return@LaunchedEffect
        snapshotFlow {
            val layout = grid.layoutInfo
            layout.totalItemsCount > 0 && (layout.visibleItemsInfo.lastOrNull()?.index ?: -1) >= layout.totalItemsCount - 5
        }.collect { nearEnd -> if (nearEnd) tools.loadNextPage() }
    }
    LaunchedEffect(active, filterOpen, data.account, data.page, data.loading, data.error) {
        if (!active || filterOpen || data.loading || data.error != null) return@LaunchedEffect
        val origin = data.account.apiBase
        if (data.page?.items.isNullOrEmpty()) return@LaunchedEffect
        snapshotFlow {
            if (grid.isScrollInProgress) emptyList()
            else grid.layoutInfo.visibleItemsInfo.asSequence()
                .mapNotNull { item -> (item.key as? String)?.takeIf { it.startsWith(origin) }?.removePrefix(origin) }
                .take(2).toList()
        }.collectLatest { ids ->
            if (ids.isNotEmpty()) {
                delay(350)
                tools.prefetchDetails(origin, ids)
            }
        }
    }
    CatalogGrid(grid, bottom) {
        item(key = "discover-controls", span = { GridItemSpan(maxLineSpan) }) {
            Column {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    HomeSearch(query, { if (it.length <= 200) query = it }, "作品名或作品编号", Modifier.weight(1f),
                        submit = { anchor = null; tools.search(query) })
                    FilterButton("筛选", data.filters.count) { filterOpen = true; tools.loadTags() }
                }
                LazyRow(Modifier.fillMaxWidth(), state = categoryScroll, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(categories, key = { it.first }) { (key, label, glyph) ->
                        val searchResults = key == "popular" && data.query.isNotBlank()
                        ChoiceChip(data.order == key, { anchor = null; tools.search(query, order = key, ascending = false) },
                            { Text(if (searchResults) "搜索结果" else label) }, glyph = if (searchResults) Glyph.Search else glyph)
                    }
                }
                if (data.order == "recommend") FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val label = when (val identity = data.page?.recommendationIdentity) {
                        RecommendationIdentity.Guest -> "访客推荐"
                        is RecommendationIdentity.Account -> identity.name?.takeIf { it.isNotBlank() }?.let { "账号推荐 · $it" } ?: "账号推荐"
                        null -> null
                    }
                    label?.let { Text(it, Modifier.align(Alignment.CenterVertically), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    MotionTextButton(openSiteAccount) { Text(if (data.page?.recommendationIdentity is RecommendationIdentity.Account) "管理站点账号" else "登录 asmr.one") }
                }
                if (data.filters.count > 0) FilterStrip {
                    data.filters.tags.forEach {v->RemovableChip({filter(data.filters.copy(tags=data.filters.tags-v))},{Text("包含 $v")})}
                    data.filters.excludedTags.forEach {v->RemovableChip({filter(data.filters.copy(excludedTags=data.filters.excludedTags-v))},{Text("排除 $v")})}
                    data.filters.voices.forEach {v->RemovableChip({filter(data.filters.copy(voices=data.filters.voices-v))},{Text("声优 $v")})}
                    if(data.filters.circle.isNotBlank())RemovableChip({filter(data.filters.copy(circle=""))},{Text("${data.filters.circle}")})
                    if(data.filters.rating.isNotBlank())RemovableChip({filter(data.filters.copy(rating=""))},{Text(if(data.filters.rating=="adult")"R18" else "全年龄")})
                    if(data.filters.subtitleOnly)RemovableChip({filter(data.filters.copy(subtitleOnly=false))},{Text("带字幕")})
                    MotionTextButton({filter(BrowseFilters())}){Text("清空条件")}
                }
            }
        }
        data.error?.let { error -> item(span = { GridItemSpan(maxLineSpan) }) {
            Column { Text(error, color = MaterialTheme.colorScheme.error)
                MotionTextButton({ tools.search(page = data.requestedPage) }) { Text("重试加载") } }
        } }
        val page = data.page
        if (page != null) {

            if (page.items.isEmpty() && !data.loading && !data.hasMore) item(span = { GridItemSpan(maxLineSpan) }) {
                EmptyContent("没有匹配的作品", "可调整搜索或内容显示设置。", Glyph.Discover)
            }
            items(page.items, key = { data.account.apiBase + it.sourceId }, contentType = { "work" }) { work ->
                val workId=com.asmrplayer.android.library.LibraryIndex.id("asmr_one",data.account.apiBase,work.sourceId)
                val local = state.lookup.byId[workId]
                val ready = work.sourceId !in data.savingWorks
                fun act(action: OnlineTools.CardAction) = tools.cardAction(data.account.apiBase, work.sourceId, action)
                CardActions(work.title, menu = { close ->
                    CardAction("作品详情", close) { open(data.account.apiBase, work.sourceId) }
                    val member = local?.inLibrary == true
                    val liked = local?.favorite == true
                    CardAction(if (member) "移出音频库" else "加入音频库", close, ready) {
                        act(if (member) OnlineTools.CardAction.Remove else OnlineTools.CardAction.Save)
                    }
                    CardAction(if (liked) "取消喜欢" else "喜欢作品", close, ready) {
                        act(if (liked) OnlineTools.CardAction.Unlike else OnlineTools.CardAction.Like)
                    }
                    CardAction("下一首播放", close, ready && state.connected) { act(OnlineTools.CardAction.Next) }
                    CardAction("加入播放队列", close, ready && state.connected) { act(OnlineTools.CardAction.Queue) }
                    CardAction("加入作品集", close, ready) { tools.chooseWork(work, data.account.apiBase) }
                    CardAction("下载作品", close, ready) { download(data.account.apiBase, work.sourceId) }
                }) {
                    WorkCard(work.title,work.coverUrl,work.code,
                        if(state.playRequested && state.currentWork?.id == workId) listOf("正在播放") else emptyList(),
                        work.circle,work.tags,{open(data.account.apiBase,work.sourceId)},onTag=::tag,
                        nsfw=work.rating=="NSFW",hasSubtitle=work.hasSubtitle,voices=work.voices,onVoice=::voice,
                        workId=workId,score=work.score)
                }
            }
            item(key = "discover-load-more", span = { GridItemSpan(maxLineSpan) }) {
                when {
                    data.loadingMore -> LoadingRing("正在加载更多作品")
                    data.moreError != null -> Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                        Text(data.moreError, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                        MotionTextButton(tools::loadNextPage) { Text("重试加载更多") }
                    }
                    !data.hasMore && page.items.isNotEmpty() -> Text("已显示全部 ${page.items.size} 部作品", Modifier.fillMaxWidth().padding(vertical = 8.dp),
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun OnlineSourcePage(data: OnlineScreenState, tools: OnlineTools, openAccount: Boolean = false) {
    var origin by rememberSaveable(data.account.apiBase) { mutableStateOf(data.account.apiBase) }
    var editing by rememberSaveable(data.account.apiBase) { mutableStateOf(false) }
    var showPresets by remember { mutableStateOf(false) }
    var accountAction by rememberSaveable { mutableStateOf("source") }
    var token by remember(data.account.apiBase) { mutableStateOf("") }
    val idle = !data.accountSaving && !data.working
    val listState = rememberLazyListState(initialFirstVisibleItemIndex = if (openAccount) 1 else 0)
    LazyColumn(Modifier.navigationBarsPadding(), state = listState, contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { MaintenanceCard("当前来源") {
            if (editing) {
                OutlinedTextField(origin, { origin = it }, label = { Text("API 来源地址") }, singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri), enabled = idle, modifier = Modifier.fillMaxWidth())
                if (accountAction == "source") data.accountError?.let { MaintenanceHint(it, error = true) }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MotionButton({
                        accountAction = "source"
                        tools.saveOrigin(origin) { editing = false }
                    }, enabled = idle && origin.isNotBlank() && origin.trim() != data.account.apiBase) { Text("保存来源") }
                    MotionTextButton({ editing = false; origin = data.account.apiBase; tools.dismissAccountFeedback() }, enabled = idle) { Text("取消") }
                }
            } else {
                androidx.compose.foundation.text.selection.SelectionContainer { Text(data.account.apiBase, style = MaterialTheme.typography.bodyLarge) }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Box {
                        MotionOutlinedButton({ showPresets = true }, enabled = idle) { Text("选择站点") }
                        DropdownMenu(expanded = showPresets, onDismissRequest = { showPresets = false }) {
                            listOf(
                                "https://api.asmr-200.com" to "镜像 200（默认）",
                                "https://api.asmr.one" to "本站",
                                "https://api.asmr-100.com" to "镜像 100",
                                "https://api.asmr-300.com" to "镜像 300",
                            ).forEach { (base, name) ->
                                DropdownMenuItem(text = {
                                    Column {
                                        Text(name + if(base == data.account.apiBase) " · 当前" else "")
                                        Text(base, style = MaterialTheme.typography.bodySmall)
                                    }
                                }, enabled = idle, onClick = {
                                    showPresets = false
                                    if(base != data.account.apiBase) {
                                        accountAction = "source"
                                        tools.saveOrigin(base)
                                    }
                                })
                            }
                        }
                    }
                    MotionOutlinedButton({ origin = data.account.apiBase; editing = true; tools.dismissAccountFeedback() }, enabled = idle) { Text("编辑地址") }
                }
                if (accountAction == "source") data.accountError?.let { MaintenanceHint(it, error = true) }
            }
        } }
        item { MaintenanceCard("asmr.one 账号") {
            val base = data.account.apiBase
            val accountName = data.account.accountName
            var name by remember(base) { mutableStateOf("") }
            var password by remember(base) { mutableStateOf("") }
            var relogin by remember(base) { mutableStateOf(false) }
            val canLogin = idle && !editing && name.isNotBlank() && password.isNotEmpty()
            fun login() {
                if (!canLogin) return
                accountAction = "login"
                tools.loginSiteAccount(base, name, password) { password = ""; name = ""; relogin = false }
            }
            if (accountName != null && !relogin) {
                Text("已连接账号 · $accountName", style = MaterialTheme.typography.bodyMedium)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MotionOutlinedButton({ name = accountName; relogin = true; tools.dismissAccountFeedback() }, enabled = idle) { Text("重新登录") }
                    MotionTextButton({ accountAction = "login"; tools.saveToken(base, null) { name = ""; password = "" } }, enabled = idle) { Text("退出登录") }
                }
            } else if (ProviderAccount.allowsLogin(base)) {
                OutlinedTextField(name, { name = it }, label = { Text("账号名") }, singleLine = true, enabled = idle && !editing,
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = ImeAction.Next),
                    modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.Username })
                OutlinedTextField(password, { password = it }, label = { Text("密码") }, singleLine = true, enabled = idle && !editing,
                    visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { login() }), modifier = Modifier.fillMaxWidth().semantics { contentType = ContentType.Password })
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MotionButton(::login, enabled = canLogin) { Text(if (data.accountSaving && accountAction == "login") "正在登录…" else "登录 asmr.one") }
                    if (relogin) MotionTextButton({ password = ""; relogin = false; tools.dismissAccountFeedback() }, enabled = idle) { Text("取消") }
                }
            } else MaintenanceHint("选择上方的 asmr.one 官方镜像即可登录。")
            if (data.account.hasToken && !relogin) {
                MotionOutlinedButton(tools::previewSiteLibrary, enabled = idle && !editing) { Text("同步收藏与播放列表") }
                data.siteLibraryNotice?.let { MaintenanceHint(it) }
            }
            if (accountAction == "login") data.accountError?.let { MaintenanceHint(it, error = true) }
        } }
        item { MaintenanceCard {
            DetailDisclosure("访问凭据", if (data.account.hasToken) "已设置 Token" else "未设置 · 可公开浏览") {
                OutlinedTextField(token, { token = it }, label = { Text("站点访问令牌") }, placeholder = { Text("输入 asmr.one 账号的 Bearer Token") }, singleLine = true,
                    visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    enabled = idle && !editing, modifier = Modifier.fillMaxWidth())
                if (accountAction == "token") data.accountError?.let { MaintenanceHint(it, error = true) }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    MotionButton({ accountAction = "token"; tools.saveToken(data.account.apiBase, token) { token = "" } },
                        enabled = idle && !editing && token.isNotBlank()) { Text("保存 Token") }
                    if (data.account.hasToken) MotionTextButton({ accountAction = "token"; tools.saveToken(data.account.apiBase, null) { token = "" } },
                        enabled = idle && !editing) { Text("清除 Token") }
                }
            }
        } }
        if (data.accountSaving) item { LoadingRing() }
    }
    data.siteLibrary?.takeIf { it.account == data.account }?.let { dialog ->
        AccountLibraryImportDialog(dialog, tools)
    }
}

internal fun byteSize(value: Long?): String = when {
    value == null -> "大小未知"
    value < 1024 -> "$value B"
    value < 1024 * 1024 -> String.format(Locale.ROOT, "%.1f KB", value / 1024.0)
    value < 1024L * 1024 * 1024 -> String.format(Locale.ROOT, "%.1f MB", value / (1024.0 * 1024))
    else -> String.format(Locale.ROOT, "%.2f GB", value / (1024.0 * 1024 * 1024))
}
