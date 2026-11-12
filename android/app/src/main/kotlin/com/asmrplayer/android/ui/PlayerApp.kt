package com.asmrplayer.android.ui

import android.animation.ValueAnimator
import android.os.Build
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerDefaults
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.animation.core.snap
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Alignment
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerViewModel
import com.asmrplayer.android.UiFeedback
import com.asmrplayer.android.R
import com.asmrplayer.android.update.UpdatePhase
import kotlin.math.abs
import kotlin.math.roundToInt

internal enum class Destination(val title: String, val glyph: Glyph) {
    Library("音频库",Glyph.Library), Discover("发现",Glyph.Discover), Favorites("喜欢",Glyph.Heart), Queue("队列",Glyph.Queue)
}

@OptIn(ExperimentalMaterial3Api::class)
@UnstableApi
@Composable
fun PlayerApp(model: PlayerViewModel, playerRequest: Int = 0, refraction: Boolean = true) {
    val state by model.screenState.collectAsStateWithLifecycle()
    val playlistTarget by model.playlistTarget.collectAsStateWithLifecycle()
    val downloadPlan by model.downloadPlan.state.collectAsStateWithLifecycle()
    val toolState by model.tools.state.collectAsStateWithLifecycle()
    val onlineState by model.online.state.collectAsStateWithLifecycle()
    val update by model.maintenance.update.collectAsStateWithLifecycle()
    val incoming by model.incoming.state.collectAsStateWithLifecycle()
    val notices = remember { NoticeHost() }
    NoticeTimer(notices)
    val lifecycleState by LocalLifecycleOwner.current.lifecycle.currentStateFlow.collectAsState()
    val pager = rememberPagerState { Destination.entries.size }
    var filterPage by remember { mutableStateOf<Destination?>(null) }
    val tab = filterPage ?: Destination.entries[pager.settledPage]
    LaunchedEffect(filterPage, pager.settledPage) {
        filterPage?.let { if (pager.settledPage != it.ordinal) pager.requestScrollToPage(it.ordinal) }
    }
    val motionEnabled = rememberUiMotion(state.lowEffects)
    val scope = rememberCoroutineScope()
    var tabScroll by remember { mutableStateOf<Job?>(null) }
    fun chooseTab(next: Destination) {
        tabScroll?.cancel()
        tabScroll = scope.launch {
            if (motionEnabled) pager.animateScrollToPage(next.ordinal)
            else pager.scrollToPage(next.ordinal)
        }
    }
    val pageState=rememberSaveableStateHolder()
    var routes by rememberSaveable { mutableStateOf(listOf<String>()) }
    fun leaveHome() {
        tabScroll?.cancel()
        if (routes.isEmpty() && pager.isScrollInProgress) pager.requestScrollToPage(pager.currentPage)
    }
    LaunchedEffect(playerRequest) {
        if (playerRequest > 0) { leaveHome(); routes = listOf("player") }
    }
    val route=routes.lastOrNull()
    LaunchedEffect(model) { model.feedback.collect(notices::show) }
    LaunchedEffect(toolState.error, toolState.notice) {
        (toolState.error ?: toolState.notice)?.let { notices.show(UiFeedback(it)); model.tools.dismissMessage() }
    }
    LaunchedEffect(onlineState.actionError, onlineState.notice) {
        (onlineState.actionError ?: onlineState.notice)?.let { notices.show(UiFeedback(it)); model.online.dismiss() }
    }
    LaunchedEffect(state.error) {
        state.error?.let { notices.show(UiFeedback(it)); model.showError(null) }
    }
    val maintenance by model.maintenance.state.collectAsStateWithLifecycle()
    LaunchedEffect(maintenance.error, maintenance.notice) {
        (maintenance.error ?: maintenance.notice)?.let { notices.show(UiFeedback(it)); model.maintenance.dismiss() }
    }
    LaunchedEffect(onlineState.accountNotice) {
        onlineState.accountNotice?.let { notices.show(UiFeedback(it)); model.online.dismissAccountFeedback() }
    }
    fun open(page: String) {
        if (page == "discover") {
            tabScroll?.cancel()
            pager.requestScrollToPage(Destination.Discover.ordinal)
            routes = emptyList()
        } else {
            leaveHome()
            routes = routes + page
        }
    }
    fun back() { routes=routes.dropLast(1) }
    var librarySearch by rememberSaveable { mutableStateOf("") }
    var streamDialog by rememberSaveable { mutableStateOf(false) }
    var address by rememberSaveable { mutableStateOf("") }
    var importDialog by rememberSaveable { mutableStateOf(false) }
    var rootToReplace by rememberSaveable { mutableStateOf<String?>(null) }
    var subtitleTarget by rememberSaveable { mutableStateOf<String?>(null) }
    var subtitleTranslation by rememberSaveable { mutableStateOf(false) }
    val picker=rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments(),model::openDocuments)
    val directoryPicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        uri?.let {
            val root = rootToReplace
            if (root != null) {
                model.maintenance.previewRoot(root, it)
                if (routes.lastOrNull() != "backup") open("backup")
            } else {
                model.tools.prepareRoot(it, null)
                if (routes.lastOrNull() != "directories") open("directories")
            }
        }
        rootToReplace=null
    }
    val subtitlePicker=rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val version=subtitleTarget
        if(uri!=null&&version!=null) model.tools.attachSubtitle(version,subtitleTranslation,uri)
        subtitleTarget=null
    }
    val chooseDirectory:(String?)->Unit={id->
        rootToReplace=id
        directoryPicker.launch(toolState.roots.firstOrNull{it.id==id}?.treeUri?.let(Uri::parse))
    }
    val importAudio={importDialog=true}
    BackHandler(filterPage == null && (streamDialog || importDialog || route != null || librarySearch.isNotEmpty() || tab != Destination.Library || pager.isScrollInProgress)) {
        when { streamDialog->streamDialog=false;importDialog->importDialog=false;route!=null->back();tab==Destination.Library && librarySearch.isNotEmpty()->librarySearch="";else->chooseTab(Destination.Library) }
    }
    val sleepUi=androidx.compose.ui.platform.LocalContext.current.getSharedPreferences("sleep-ui",0)
    var sleepNight by rememberSaveable { mutableStateOf(sleepUi.getBoolean("night",false)) }
    val dark=if(route=="sleep"&&sleepNight) true else when(state.theme){"light"->false;"dark"->true;else->isSystemInDarkTheme()}
    val colors=motionColors(playerColors(dark,state.colorPreset),motionEnabled)
    val activity=LocalActivity.current as? ComponentActivity
    LaunchedEffect(activity,dark,route) {
        val style=if(dark||route in setOf("player","player-queue")) SystemBarStyle.dark(android.graphics.Color.TRANSPARENT) else SystemBarStyle.light(android.graphics.Color.TRANSPARENT,android.graphics.Color.TRANSPARENT)
        activity?.enableEdgeToEdge(statusBarStyle=style,navigationBarStyle=style)
    }
    MaterialTheme(colorScheme=colors, typography=PlayerTypography) {
        CompositionLocalProvider(LocalContentColor provides colors.onSurface, LocalUiMotion provides motionEnabled,
            LocalNoticeHost provides notices) {
        val backdrop=rememberGlassBackdrop(refraction)
        val depth = routes.size
        val density=LocalDensity.current
        val dockVisible=route==null&&WindowInsets.ime.getBottom(density)==0
        val detailRoute=route?.startsWith("audio:")==true||route?.startsWith("online:")==true
        val detailMini=detailRoute&&state.queue.isNotEmpty()&&WindowInsets.ime.getBottom(density)==0
        val dockState = remember { androidx.compose.animation.core.MutableTransitionState(dockVisible || detailMini) }
        dockState.targetState = dockVisible || detailMini
        fun routeTitle(route: String?): String = when {route=="queue-current"->"当前队列";route?.startsWith("queue-group:")==true->"歌单详情";route=="player"->"正在播放";route=="settings"->"设置";route=="appearance"->"外观";route=="content-settings"->"内容显示";route=="effects"->"音效与均衡器";route=="downloads"->"任务中心";route=="tasks"->"任务中心";
            route=="sync"->"跨设备同步";route=="history"->"收听记录";route=="backup"->"资料备份与恢复";route=="storage"->"存储管理";route in setOf("updates", "updates/install")->"应用更新";
            route=="headphones"->"耳机音效偏好";route=="system-media"->"系统媒体与灵动岛";
            route=="online-source"->"在线来源";route?.startsWith("online:")==true->"作品详情";
            route=="directories"->"作品目录";route=="subtitles"->"字幕";route=="subtitle-settings"->"字幕设置";route=="bookmarks"->"标记";route=="ab"->"A–B 循环";route=="sleep"->"睡眠定时";
            route?.startsWith("audio:")==true->"作品详情";else->tab.title}
        DisposableEffect(route == null && tab == Destination.Discover) {
            model.online.browsing(route == null && tab == Destination.Discover)
            onDispose { model.online.browsing(false) }
        }
        BoxWithConstraints(Modifier.fillMaxSize().background(colors.background)) {
            val shortWide=maxWidth>=600.dp && maxHeight<600.dp
            var controlHeightPx by remember(density.density,density.fontScale,shortWide) { mutableIntStateOf(0) }
            val controlHeight=with(density) { controlHeightPx.toDp() }.coerceAtLeast(64.dp)
            val measureControl: (androidx.compose.ui.unit.IntSize)->Unit = { if(it.height>controlHeightPx) controlHeightPx=it.height }
            RouteMotionHost(MotionDestination(route, depth), Modifier.fillMaxSize()
                .recordBackdrop(backdrop, (dockState.currentState || dockState.targetState) && !state.lowEffects && Build.VERSION.SDK_INT >= 31, colors.background)) { destination ->
              val route = destination.route
              val screenIdentity = route ?: "home"
              val immersive = route in setOf("player", "player-queue")
              val detailRoute = route?.startsWith("audio:") == true || route?.startsWith("online:") == true
              val title = routeTitle(route)
              val navigationInset = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
              val bottom = if (route == null || detailRoute) navigationInset + 24.dp + controlHeight +
                  (if (route == null && state.queue.isNotEmpty() && !shortWide) controlHeight + 12.dp else 0.dp) + 16.dp else 24.dp
              Column(Modifier.widthIn(max=if(immersive) 1600.dp else if(route==null||detailRoute)1200.dp else 720.dp).fillMaxSize()
                .then(if(immersive) Modifier else Modifier.windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top+WindowInsetsSides.Horizontal)))) {
                if (route != null && !immersive && !detailRoute) RouteHeader(title, ::back)
                if(state.opening && route != null) CircularProgressIndicator(Modifier.size(24.dp))
                if (route == null) {
                    HorizontalPager(state = pager, modifier = Modifier.weight(1f).fillMaxWidth().imePadding(),
                        key = { Destination.entries[it].name }, beyondViewportPageCount = 1,
                        userScrollEnabled = WindowInsets.ime.getBottom(density) == 0 && !streamDialog && !importDialog && filterPage == null,
                        flingBehavior = if (motionEnabled) PagerDefaults.flingBehavior(pager)
                            else PagerDefaults.flingBehavior(pager, snapAnimationSpec = snap())) { index ->
                        val page = Destination.entries[index]
                        pageState.SaveableStateProvider("home:${page.name}") {
                            // Offscreen neighbors stay ready to draw but expose their controls only once selected.
                            val accessibility = if (page == tab) Modifier else Modifier.clearAndSetSemantics { }
                            Column(Modifier.fillMaxSize().then(accessibility).testTag("home-page-${page.name}")) {
                                if (page == Destination.Discover || page == Destination.Queue) {
                                    HomeHeader {
                                        Text(page.title, Modifier.weight(1f).semantics { heading() }, style = MaterialTheme.typography.titleLarge, maxLines = 1)
                                        GlyphButton(Glyph.Settings, "设置", { open("settings") })
                                    }
                                }
                                val filterVisibility: (Boolean) -> Unit = { showing ->
                                    if (showing) filterPage = page
                                    else if (filterPage == page) {
                                        pager.requestScrollToPage(page.ordinal)
                                        filterPage = null
                                    }
                                }
                                CompositionLocalProvider(LocalFilterVisibility provides filterVisibility, LocalSharedArtworkEnabled provides (page == tab)) {
                                    HomeRefresh(page, model, onlineState.loading, state.opening && page == tab) {
                                        when (page) {
                                            Destination.Library, Destination.Favorites -> CollectionPage(
                                                page == Destination.Favorites, state, model, librarySearch, { librarySearch = it }, bottom, importAudio,
                                                browseOnline = { chooseTab(Destination.Discover) }, open = ::open,
                                                active = page == tab && LocalRouteActive.current)
                                            Destination.Discover -> DiscoverPage(onlineState, model.online, bottom, state,
                                                active = page == tab && LocalRouteActive.current,
                                                download = { origin, id -> model.downloadPlan.open(origin = origin, sourceId = id) }) { origin, id -> open("online:${Uri.encode(origin)}:$id") }
                                            Destination.Queue -> QueueGroupsPage(state, model, bottom, ::open)
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else Box(Modifier.weight(1f).fillMaxWidth().imePadding()) {
                    pageState.SaveableStateProvider("screen:$screenIdentity") { when {
                        route=="effects" -> Box(Modifier.navigationBarsPadding()){DspPanel(state.dsp,state.connected,model::updateDsp,model::showError,{open("headphones")},
                            state.loudnessStatus,state.loudnessBusy,state.loudnessProgress,state.normalizationGainDb,state.queue.getOrNull(state.index)!=null,model::analyzeLoudness)}
                        route=="headphones" -> HeadphonePage(state,model,{open("effects")})
                        route=="system-media" -> SystemMediaPage(state,model)
                        route=="settings" -> SettingsPage(state,::open)
                        route=="appearance" -> AppearancePage(state,model)
                        route=="content-settings" -> ContentPreferencesPage(state,model)
                        route=="queue-current" -> QueuePage(state,model,24.dp,locateCurrent=true,directControls=true)
                        route?.startsWith("queue-group:")==true -> SavedQueuePage(route.substringAfter(":"),state,model,24.dp,::back)
                        route=="history" -> HistoryPage(state,model,::open)
                        route=="backup" -> BackupPage(model.maintenance,chooseDirectory)
                        route=="sync" -> SyncPage(model)
                        route=="tasks" -> TaskCenterPage(state,model,::open)
                        route=="storage" -> StoragePage(model.maintenance,{open("downloads")})
                        route in setOf("updates", "updates/install") -> UpdatePage(model, confirmInstallOnOpen = route == "updates/install")
                        route=="online-source" -> OnlineSourcePage(onlineState,model.online)
                        route?.startsWith("online:")==true -> {
                            val origin=Uri.decode(route.removePrefix("online:").substringBeforeLast(':'))
                            val id=route.substringAfterLast(':')
                            WorkDetailPage(state,model,com.asmrplayer.android.library.LibraryIndex.id("asmr_one",origin,id),::open,::back,origin,id,bottom)
                        }
                        route=="directories" -> DirectoryPage(toolState,model.tools,chooseDirectory)
                        route=="subtitles" -> SubtitlePage(state,toolState,model,{open("subtitle-settings")})
                        route=="subtitle-settings" -> SubtitleSettingsPage(state,toolState,model.tools) { version,translation ->
                            subtitleTarget=version;subtitleTranslation=translation;subtitlePicker.launch(arrayOf("*/*"))
                        }
                        route=="bookmarks" -> BookmarksPage(state,toolState,model)
                        route=="ab" -> AbLoopPage(state,model)
                        route=="sleep" -> SleepPage(state,model,sleepNight,{ sleepNight=it;sleepUi.edit().putBoolean("night",it).apply() })
                        route=="downloads" -> TaskCenterPage(state,model,::open,initialDownloads=true)
                        route in setOf("player","player-queue") -> FullPlayer(state,model,::open,::back, { version,translation -> subtitleTarget=version;subtitleTranslation=translation;subtitlePicker.launch(arrayOf("*/*")) })
                        route?.startsWith("audio:")==true -> {
                            val workId=route.removePrefix("audio:")
                            val online = state.metadata[workId]?.takeIf {
                                it.sourceId.isNotBlank() && it.origin.isNotBlank() &&
                                    (workId == com.asmrplayer.android.library.LibraryIndex.id("asmr_one", it.origin, it.sourceId) ||
                                        state.lookup.byId[workId]?.versions.isNullOrEmpty())
                            }
                            WorkDetailPage(state,model,workId,::open,::back,online?.origin,online?.sourceId,bottom)
                        }
                    } }
                }
            }
            }
            AnimatedVisibility(dockState, Modifier.align(Alignment.BottomCenter),
                enter = if (motionEnabled) fadeIn(tween(180)) + slideInVertically(tween(280, easing = MotionEase)) { it / 3 } else EnterTransition.None,
                exit = if (motionEnabled) fadeOut(tween(160)) + slideOutVertically(tween(240, easing = MotionEase)) { it / 3 } else ExitTransition.None) {
              Box(Modifier.widthIn(max=840.dp).fillMaxWidth().glassDockBounds(backdrop)
                .background(androidx.compose.ui.graphics.Brush.verticalGradient(listOf(colors.background.copy(alpha=0f),colors.background.copy(alpha=.72f),colors.background.copy(alpha=.96f))))
                .windowInsetsPadding(WindowInsets.navigationBars.only(WindowInsetsSides.Bottom+WindowInsetsSides.Horizontal))
                .padding(horizontal=16.dp,vertical=12.dp)) {
                val navigation: @Composable (Modifier) -> Unit = { modifier ->
                    LiquidNavigation(backdrop, state.lowEffects, pager, ::chooseTab,
                        modifier.heightIn(min = controlHeight).onSizeChanged(measureControl))
                }
                val player: @Composable (Modifier) -> Unit = { modifier ->
                    MiniPlayer(backdrop, state, model, { open("player") },
                        modifier.heightIn(min = controlHeight).onSizeChanged(measureControl))
                }
                if (state.queue.isEmpty()) {
                    navigation(Modifier.fillMaxWidth())
                } else {
                    PlayerDockLayout(dockVisible, shortWide, Modifier.fillMaxWidth(),
                        player = { player(Modifier.fillMaxWidth()) }, navigation = { navigation(Modifier.fillMaxWidth()) })
                }
              }
            }
        }

        NoticeOverlay()
        UpdateReminder(
            update = update,
            canShow = lifecycleState == Lifecycle.State.RESUMED && dockVisible &&
                filterPage == null && !pager.isScrollInProgress && !importDialog && !streamDialog &&
                !incoming.visible && !downloadPlan.visible && playlistTarget == null,
            startUpdate = {
                val phase = model.maintenance.update.value.phase
                if (phase == UpdatePhase.AVAILABLE) model.maintenance.downloadUpdate()
                open(if (phase == UpdatePhase.READY) "updates/install" else "updates")
            },
        )
        DownloadSelectionDialog(downloadPlan, model.downloadPlan::toggle, model.downloadPlan::close, model.downloadPlan::retry) {
            model.downloadPlan.confirm { open("downloads") }
        }
        playlistTarget?.let { ids -> PlayerSheet("加入歌单", model::closePlaylistPicker) { AddTracksToGroup(ids, model) } }
        IncomingDialog(incoming,model.incoming,state) { id -> open("online:${Uri.encode(onlineState.account.apiBase)}:$id") }
        if(streamDialog) PlayerAlertDialog(onDismissRequest={streamDialog=false},title={Text("播放在线音频")},text={
            OutlinedTextField(address,{address=it},label={Text("HTTPS 音频地址")},modifier=Modifier.fillMaxWidth())
        },confirmButton={MotionTextButton({if(model.openStream(address)){streamDialog=false;address="";open("player")}}){Text("播放链接")}},dismissButton={MotionTextButton({streamDialog=false}){Text("取消")}})
        if(importDialog) PlayerSheet("导入作品",{importDialog=false}) {
            SettingsLink("导入作品目录","选择文件夹，预览后加入音频库"){importDialog=false;chooseDirectory(null)}
            SettingsLink("打开音频文件","选择文件后直接播放"){importDialog=false;picker.launch(arrayOf("audio/*"))}
            SettingsLink("播放链接","输入 HTTPS 音频地址"){importDialog=false;streamDialog=true}
            SettingsLink("管理已授权目录",""){importDialog=false;open("directories")}
        }

        }
    }
}

@Composable
internal fun GlyphButton(glyph:Glyph,label:String,action:()->Unit,enabled:Boolean=true) {
    MotionIconButton(action,enabled=enabled,modifier=Modifier.sizeIn(minWidth=48.dp,minHeight=48.dp).semantics{contentDescription=label}){AppGlyph(glyph)}
}

@Composable
fun ToggleRow(title:String,detail:String?,checked:Boolean,enabled:Boolean,onChange:(Boolean)->Unit) {
    Row(Modifier.fillMaxWidth().toggleable(value=checked,enabled=enabled,role=Role.Switch,onValueChange=onChange).heightIn(min=52.dp).padding(vertical=6.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)){Text(title,style=MaterialTheme.typography.titleSmall);detail?.let{Text(it,style=MaterialTheme.typography.bodySmall,color=MaterialTheme.colorScheme.onSurfaceVariant)}}
        Switch(checked,null,enabled=enabled)
    }
}

@Composable
internal fun RouteHeader(title: String, back: () -> Unit, actions: @Composable () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(horizontal=16.dp, vertical=4.dp).heightIn(min=56.dp),
        verticalAlignment=Alignment.CenterVertically) {
        GlyphButton(Glyph.Back, "返回", back)
        Text(title, Modifier.weight(1f).semantics { heading() }, maxLines=2, overflow=androidx.compose.ui.text.style.TextOverflow.Ellipsis, style=MaterialTheme.typography.titleLarge)
        actions()
    }
}
