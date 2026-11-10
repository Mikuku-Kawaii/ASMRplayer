package com.asmrplayer.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.snap
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.zIndex
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@OptIn(ExperimentalFoundationApi::class,ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun FullPlayer(state:PlayerScreenState,model:PlayerViewModel,open:(String)->Unit,close:()->Unit,importSubtitle:(String,Boolean)->Unit) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val rawData by model.tools.state.collectAsStateWithLifecycle()
    val current=state.queue.getOrNull(state.index)
    val data=if(rawData.versionId==current?.id)rawData else LibraryToolsState(subtitleBusy=true)
    val collection by model.collections.state.collectAsStateWithLifecycle()
    val work=state.currentWork
    val hidden=hiddenCurrent(state)
    val prefs=LocalContext.current.getSharedPreferences("listening-ui",0)
    var lyrics by rememberSaveable{mutableStateOf(false)}
    var playbackSettings by remember{mutableStateOf(false)}
    var sheet by rememberSaveable { mutableStateOf<String?>(null) }
    var note by rememberSaveable { mutableStateOf("") }
    var savedBookmark by remember { mutableStateOf<String?>(null) }
    var lastOriginal by rememberSaveable(current?.id) { mutableStateOf<String?>(null) }
    var lastTranslation by rememberSaveable(current?.id) { mutableStateOf<String?>(null) }
    LaunchedEffect(data.preference) {
        data.preference?.originalId?.let { lastOriginal=it }
        data.preference?.translationId?.let { lastTranslation=it }
    }
    LaunchedEffect(savedBookmark) { if(savedBookmark!=null) { delay(8000); if(sheet!="edit-note")savedBookmark=null } }
    fun subtitles() { stableCurrent(state)?.let { importSubtitle(it,false) } ?: model.feedback("请先选择音频库中的音轨") }
    var textSize by rememberSaveable{mutableFloatStateOf(prefs.getFloat("text-size",22f).coerceIn(18f,32f))}
    BackHandler(lyrics){lyrics=false}
    val artwork=rememberArtwork(currentCover(state),highResolution=true,workId=work?.id?.takeUnless{hidden})
    val scene=lerp(artwork.scene?:Color(0xFF2A1F26),Color.Black,.48f)
    val ink=Color(0xFFFFF1F6)
    val palette=darkColorScheme(primary=ink,onPrimary=scene,background=scene,surface=scene,onSurface=ink,onBackground=ink,
        onSurfaceVariant=ink.copy(alpha=.88f),surfaceContainer=lerp(scene,ink,.08f),surfaceContainerLow=lerp(scene,ink,.04f),
        primaryContainer=lerp(scene,ink,.14f),onPrimaryContainer=ink,outline=ink.copy(alpha=.35f),outlineVariant=ink.copy(alpha=.15f),
        surfaceContainerHigh=lerp(scene,ink,.10f),surfaceContainerHighest=lerp(scene,ink,.14f),secondaryContainer=lerp(scene,ink,.12f),onSecondaryContainer=ink)
    val hasLyrics=data.subtitles?.original!=null||data.subtitles?.translation!=null
    val coverTouch=remember{MutableInteractionSource()}
    val excerptTouch=remember{MutableInteractionSource()}
    val motion=LocalUiMotion.current
    val coverOpacity by animateFloatAsState(if(lyrics)0f else 1f,if(motion)tween(160,easing=androidx.compose.animation.core.LinearOutSlowInEasing) else snap(),label="cover lyric transition")
    MaterialTheme(colorScheme=palette){CompositionLocalProvider(LocalContentColor provides ink){
        BoxWithConstraints(Modifier.fillMaxSize().clipToBounds().background(scene).pointerInput(lyrics){
            if(lyrics)detectTapGestures(onTap={lyrics=false})
        }) {
            ImmersiveArtwork(artwork.image,state.lowEffects,Modifier.matchParentSize())
            val wide=maxWidth>=840.dp || (maxWidth>=600.dp && maxHeight<500.dp)
            // Extend the artwork toward the header and lyric excerpt without moving content slots.
            val upwardExtension=if(wide)0.dp else 24.dp
            val downwardExtension=if(wide)0.dp else 24.dp
            val coverTop=WindowInsets.statusBars.asPaddingValues().calculateTopPadding()+4.dp-upwardExtension
            val coverHeight=if(wide)minOf(maxHeight-coverTop,maxWidth*.46f*.75f) else maxHeight*.55f+upwardExtension+downwardExtension
            // Translate the whole portrait artwork, retaining its scale and both feathered edges.
            val coverOffset=coverTop+if(wide)((maxHeight-coverTop-coverHeight)/2).coerceAtLeast(0.dp) else 38.dp
            if(coverOpacity>0f)Box(Modifier.fillMaxWidth(if(wide).46f else 1f).offset(y=coverOffset).height(coverHeight).graphicsLayer{alpha=coverOpacity}) {
                artwork.image?.let{FusedCoverArtwork(it,state.lowEffects,if(wide)ContentScale.Fit else ContentScale.Crop,Modifier.fillMaxSize())}
                if(artwork.loading)CircularProgressIndicator(Modifier.align(Alignment.Center).size(28.dp),color=ink)
            }
            Column(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top+WindowInsetsSides.Horizontal)).navigationBarsPadding()) {
                Row(Modifier.fillMaxWidth().heightIn(min=56.dp).padding(horizontal=12.dp),verticalAlignment=Alignment.CenterVertically) {
                    MotionIconButton({if(lyrics)lyrics=false else close()},Modifier
                        .semantics{contentDescription=if(lyrics)"返回封面" else "收起播放页"}){AppGlyph(Glyph.ChevronDown)}
                    Text(if(lyrics)"台本" else "ASMR",Modifier.weight(1f),textAlign=TextAlign.Center,style=MaterialTheme.typography.titleSmall)
                    if (lyrics) GlyphButton(Glyph.List, "字幕时间轴", { open("subtitles") }) else Spacer(Modifier.size(48.dp))
                }
                state.session.warning?.let{Text(it,Modifier.padding(horizontal=24.dp),color=palette.error,style=MaterialTheme.typography.bodySmall)}
                collection.error?.let{Text(it,Modifier.padding(horizontal=24.dp),color=palette.error,style=MaterialTheme.typography.bodySmall)}
                val workInfo: @Composable (Modifier) -> Unit = { modifier ->
                    Row(modifier.fillMaxWidth().heightIn(min=76.dp),verticalAlignment=Alignment.CenterVertically) {
                        Column(Modifier.weight(1f).clickable(enabled=work!=null&&!hidden,onClickLabel="查看作品详情"){work?.id?.let{open("audio:$it")}}.padding(vertical=8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                            Text(if(hidden)"内容已隐藏" else trackDisplayTitle(work?.versions?.firstOrNull{it.id==current?.id}?.title?.takeIf{it.isNotBlank()}?:displayTitle(state)),maxLines=2,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleMedium)
                            if(!hidden)Text(work?.let{state.metadata[it.id]?.voices().orEmpty().joinToString(" / ").ifBlank{it.title}}?:"临时播放",
                                maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.bodySmall,color=palette.onSurfaceVariant)
                        }
                        val workFavorite=work?.favorite==true
                        MotionIconToggleButton(workFavorite,{work?.id?.let(model::toggleFavorite)},enabled=work!=null&&!hidden,
                            modifier=Modifier.size(48.dp).semantics{contentDescription=if(workFavorite)"取消作品喜欢" else "喜欢作品"}) {
                            AppGlyph(Glyph.Heart,Modifier.size(24.dp),filled=workFavorite,tint=if(workFavorite)Color(0xFFFF779B) else ink)
                        }
                        val favorite=collection.favorites.any{it.versionId==current?.id}
                        MotionIconToggleButton(favorite,{current?.id?.let{model.collections.favorite(it,!favorite)}},enabled=work!=null&&!hidden,
                            modifier=Modifier.size(48.dp).semantics{contentDescription=if(favorite)"取消书签" else "添加书签"}){AppGlyph(Glyph.TrackBookmark,Modifier.size(24.dp),filled=favorite,tint=if(favorite)Color(0xFFFF779B) else ink)}
                    }
                }
                val stage: @Composable (Modifier)->Unit = { stageModifier ->
                Box(stageModifier.clipToBounds()) {
                    if (lyrics || coverOpacity < 1f) Box(Modifier.fillMaxSize().zIndex(if (lyrics) 1f else 0f)
                        .graphicsLayer { alpha = 1f - coverOpacity }
                        .then(if (lyrics) Modifier else Modifier.clearAndSetSemantics {})) {
                        LyricsStage(state,data,model,textSize,{lyrics=false})
                    }
                    if (!lyrics || coverOpacity > 0f) Column(Modifier.fillMaxSize().zIndex(if (lyrics) 0f else 1f)
                        .graphicsLayer { alpha = coverOpacity }
                        .then(if (lyrics) Modifier.clearAndSetSemantics {} else Modifier), verticalArrangement=Arrangement.Bottom) {
                        Box(Modifier.weight(1f).fillMaxWidth().clickable(enabled=!lyrics,interactionSource=coverTouch,indication=null){lyrics=true}.semantics{contentDescription="点击封面展开台本"},contentAlignment=Alignment.Center) {
                            if(artwork.image==null&&!artwork.loading)AppGlyph(Glyph.Audio,Modifier.size(56.dp),ink.copy(alpha=.45f))
                        }
                        Column(Modifier.fillMaxWidth().then(if (hasLyrics) Modifier.clickable(interactionSource=excerptTouch, indication=null) { lyrics=true } else Modifier)
                            .padding(horizontal=28.dp, vertical=10.dp), horizontalAlignment=Alignment.CenterHorizontally) {
                            when {
                                hidden -> Text("内容已隐藏")
                                data.subtitleBusy -> LoadingRing()
                                hasLyrics -> key(data.versionId) { SubtitleExcerpt(requireNotNull(data.subtitles), position) }
                                else -> Text(if (data.subtitleError != null) "台本读取失败" else "暂无台本", style=MaterialTheme.typography.bodySmall)
                            }
                        }
                        if (!wide) workInfo(Modifier.padding(horizontal=20.dp))
                    }
                }
                }
                val controls: @Composable (Modifier)->Unit = { controlsModifier ->
                Column(controlsModifier.padding(horizontal=20.dp)) {
                    if (wide) workInfo(Modifier)
                    PlayerTransport(state,model,{sheet="queue"},wide,collection.history.firstOrNull{it.versionId==current?.id}?.durationMs?:0,
                        markerPositions=if(hidden)emptyList() else data.bookmarks.filter{it.versionId==current?.id}.map{it.positionMs})
                    FlowRow(Modifier.fillMaxWidth().padding(top=if(wide)6.dp else 14.dp,bottom=if(wide)8.dp else 12.dp),horizontalArrangement=Arrangement.SpaceBetween) {
                        PlayerTool(Glyph.CollectionWide,if(work?.inLibrary==true)"已收藏" else "收藏",selected=work?.inLibrary==true,enabled=work!=null&&!hidden,
                            description=if(work?.inLibrary==true)"取消收藏，移出音频库" else "收藏，加入音频库") {
                            work?.let{model.setLibraryMembership(it.id,!it.inLibrary)}
                        }
                        PlayerTool(Glyph.Sleep,"睡眠",selected=state.session.deadlineMs!=null||state.session.endOfTrack){sheet="sleep"}
                        PlayerTool(Glyph.Sliders,"音效",selected=!state.dsp.bypass){sheet="effects"}
                        PlayerTool(Glyph.GroupAdd,"歌单",enabled=work!=null&&!hidden){sheet="groups"}
                        PlayerTool(Glyph.More,"更多"){playbackSettings=true}
                    }
                }
                }
                if(wide) Row(Modifier.weight(1f).fillMaxWidth()) {
                    stage(Modifier.weight(.46f).fillMaxHeight())
                    Box(Modifier.weight(.54f).fillMaxHeight(),contentAlignment=Alignment.Center) {
                        controls(Modifier.widthIn(max=680.dp).fillMaxWidth().verticalScroll(rememberScrollState()))
                    }
                } else {
                    stage(Modifier.weight(1f).fillMaxWidth())
                    controls(Modifier.fillMaxWidth())
                }
            }
        }
        if(playbackSettings) PlayerSheet("播放工具",{playbackSettings=false}) {
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                AppGlyph(if(savedBookmark!=null)Glyph.Check else Glyph.Marker,filled=savedBookmark!=null)
                MotionTextButton({stableCurrent(state)?.let{model.addBookmark(it,""){savedBookmark=it}}},enabled=stableCurrent(state)!=null&&!hidden&&!state.session.busy){Text(if(savedBookmark!=null)"已添加标记" else "添加标记 · ${time(position)}")}
                Spacer(Modifier.weight(1f))
                if(savedBookmark!=null)MotionTextButton({note="";playbackSettings=false;sheet="edit-note"}){Text("备注")}
            }
            SettingsLink("播放速度","${state.speed}×"){playbackSettings=false;sheet="speed"}
            SettingsLink("当前音轨标记","查看、定位或编辑"){playbackSettings=false;sheet="bookmarks"}
            SettingsLink("字幕与字号","来源、字号与时间对齐"){playbackSettings=false;sheet="subtitle"}
            SettingsLink("A–B 循环",if(state.session.abVersionId!=null)"循环已开启" else "设置循环范围"){playbackSettings=false;sheet="ab"}
            SettingsLink("下载管理",""){playbackSettings=false;open("downloads")}
            MotionTextButton({playbackSettings=false;model.stop()}){Text("停止播放")}
        }
        sheet?.let { kind -> PlayerSheet(when(kind){"groups"->"加入歌单";"queue"->"播放队列 · ${state.queue.size}";"sleep"->"睡眠定时";"ab"->"A–B 循环";"bookmarks"->"当前音轨标记";"effects"->"音效";"speed"->"播放速度";"note","edit-note"->"标记备注";else->"字幕与字号"},{sheet=null},contentScrolls=kind in setOf("sleep","ab","bookmarks","subtitle-settings","effects","queue")) {
            when(kind) {
                "groups" -> AddCurrentToGroup(state,model)
                "queue" -> QueuePage(state,model,16.dp,locateCurrent=true,directControls=true)
                "sleep" -> SleepPage(state,model,false,{},quick=true)
                "ab" -> AbLoopPage(state,model)
                "bookmarks" -> BookmarksPage(state,data,model,inlineEdit=true)
                "note","edit-note" -> {
                    Text("当前位置 · ${time(position)}",style=MaterialTheme.typography.bodySmall)
                    OutlinedTextField(note,{note=it.take(4096)},label={Text("备注（可选）")},modifier=Modifier.fillMaxWidth())
                    MotionButton({if(kind=="edit-note")savedBookmark?.let{model.collections.editBookmark(it,note)} else stableCurrent(state)?.let{model.addBookmark(it,note)};note="";sheet=null},enabled=stableCurrent(state)!=null&&!state.session.busy){Text("保存备注")}
                }
                "speed" -> {
                    FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) { listOf(.5f,.75f,1f,1.25f,1.5f,2f).forEach { value -> ChoiceChip(state.speed==value,{model.speed(value)},label={Text("${value}×")}) } }
                    Text("当前 ${"%.2f".format(state.speed)}×",style=MaterialTheme.typography.bodySmall)
                    Slider(state.speed,{model.speed(it)},valueRange=.5f..2f,steps=29,modifier=Modifier.semantics { contentDescription="播放速度" })
                }
                "effects" -> PlayerEffectsSheet(state,model)
                "subtitle-settings" -> SubtitleSettingsPage(state,data,model.tools,inlineChoices=true,import=importSubtitle)
                else -> {
                    Text("台本预览",fontSize=textSize.sp,textAlign=TextAlign.Center,modifier=Modifier.fillMaxWidth())
                    Slider(textSize,{textSize=it;prefs.edit().putFloat("text-size",it).apply()},valueRange=18f..32f,steps=6,modifier=Modifier.semantics { contentDescription="台本字号" })
                    val version=stableCurrent(state)
                    val pref=data.preference
                    if(version!=null&&pref!=null) {
                        ToggleRow("显示原文",null,pref.originalId!=null,data.sources.isNotEmpty()){model.tools.chooseSubtitle(version,false,if(it)lastOriginal?.takeIf{ id->data.sources.any{it.id==id} }?:data.sources.firstOrNull()?.id else null)}
                        ToggleRow("显示译文",null,pref.translationId!=null,data.sources.any{it.id!=pref.originalId}){model.tools.chooseSubtitle(version,true,if(it)lastTranslation?.takeIf{ id->data.sources.any{it.id==id} }?:data.sources.firstOrNull { source -> source.id!=pref.originalId }?.id else null)}
                        if(data.sources.isNotEmpty())Row(verticalAlignment=Alignment.CenterVertically) {
                            MotionTextButton({model.tools.saveOffsets(version,(pref.offsetMs-500).coerceAtLeast(-600_000),pref.translationOffsetMs)}){Text("提前 0.5 秒")}
                            Text("${pref.offsetMs/1000f}s",Modifier.weight(1f),textAlign=TextAlign.Center,style=MaterialTheme.typography.bodySmall)
                            MotionTextButton({model.tools.saveOffsets(version,(pref.offsetMs+500).coerceAtMost(600_000),pref.translationOffsetMs)}){Text("延后 0.5 秒")}
                        }
                    }
                    SettingsLink("字幕时间轴",""){sheet=null;open("subtitles")}
                    SettingsLink("导入原文字幕",""){subtitles()}
                    SettingsLink("字幕文件与精确偏移",""){sheet="subtitle-settings"}
                }
            }
        } }
    }}
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PlayerTool(glyph:Glyph,label:String,longAction:(()->Unit)?=null,selected:Boolean=false,enabled:Boolean=true,description:String?=null,action:()->Unit) {
    val source=remember{MutableInteractionSource()}
    Column(Modifier.widthIn(min=48.dp).pressMotion(source,.97f).combinedClickable(interactionSource=source,indication=null,enabled=enabled,onClick=action,onLongClick=longAction).padding(vertical=6.dp).semantics(mergeDescendants=true){role=Role.Button;this.selected=selected;description?.let{contentDescription=it}},
        horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(6.dp)) {
        AppGlyph(glyph,Modifier.size(24.dp),tint=MaterialTheme.colorScheme.onSurface.copy(alpha=if(!enabled).38f else if(selected)1f else .88f),filled=selected)
        Text(label,style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@UnstableApi
@OptIn(ExperimentalMaterial3Api::class,ExperimentalFoundationApi::class)
@Composable
private fun PlayerTransport(state:PlayerScreenState,model:PlayerViewModel,queue:()->Unit,compact:Boolean=false,savedDuration:Long=0,markerPositions:List<Long> = emptyList()) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    var drag by remember(state.queue.getOrNull(state.index)?.id){mutableStateOf<Float?>(null)}
    val colors=MaterialTheme.colorScheme
    val duration=state.duration.takeIf{it>0}?:savedDuration.coerceAtLeast(0)
    val canSeek=state.connected&&duration>0&&(state.seekable||state.canPrepareSeek)
    val markers=remember(markerPositions,duration) { if(duration<=0)emptyList() else markerPositions.filter{it in 0..duration}.distinct() }
    val markerFractions=remember(markers,duration) { markers.map{(it.toDouble()/duration).toFloat()} }
    var trackWidth by remember { mutableFloatStateOf(0f) }
    val markerHitRadius=with(LocalDensity.current){10.dp.toPx()}
    val motion=LocalUiMotion.current
    val pulse=remember(state.queue.getOrNull(state.index)?.id){androidx.compose.animation.core.Animatable(0f)}
    var pulseFraction by remember(state.queue.getOrNull(state.index)?.id){mutableFloatStateOf(0f)}
    val motionScope=rememberCoroutineScope()
    val seekInteraction=remember{MutableInteractionSource()}
    val seekPressed by seekInteraction.collectIsPressedAsState()
    val seekDragged by seekInteraction.collectIsDraggedAsState()
    val seekEmphasized=canSeek&&(seekPressed||seekDragged)
    val trackHeight by animateFloatAsState(if(seekEmphasized)4f else 2f,
        if(motion)tween(if(seekEmphasized)80 else 120) else snap(),label="seek track height")
    Box(Modifier.fillMaxWidth().height(48.dp),contentAlignment=Alignment.Center) {
        Slider(value=if(duration>0)(drag?:position.toFloat()).coerceIn(0f,duration.toFloat()) else 0f,
            onValueChange={drag=it},onValueChangeFinished={
                drag?.let { requested ->
                    val nearest=markers.minByOrNull{kotlin.math.abs(it.toDouble()-requested)}
                    val target=if(nearest!=null&&trackWidth>0&&kotlin.math.abs(nearest.toDouble()-requested)/duration*trackWidth<=markerHitRadius)nearest else requested.toLong()
                    model.seek(target)
                    pulseFraction=(target.toDouble()/duration).toFloat()
                    motionScope.launch { pulse.snapTo(if(motion)1f else 0f); if(motion)pulse.animateTo(0f,tween(140)) }
                }
                drag=null
            },valueRange=0f..duration.toFloat().coerceAtLeast(1f),
            interactionSource=seekInteraction,
            enabled=canSeek,modifier=Modifier.heightIn(min=48.dp).semantics{contentDescription=(if(canSeek)"播放进度" else "播放进度，暂不可拖动")+
                if(markerFractions.isEmpty())"" else "，${markerFractions.size} 个时间标记"},
            thumb={Box(Modifier.size(8.dp))},
            track={slider->
                val fraction=if(duration>0)(slider.value/duration.toFloat()).coerceIn(0f,1f) else 0f
                Canvas(Modifier.fillMaxWidth().height(8.dp).onSizeChanged{trackWidth=it.width.toFloat()}) {
                    val y=size.height/2
                    val x=size.width*fraction
                    drawLine(colors.onSurface.copy(alpha=if(canSeek).40f else .24f),Offset(0f,y),Offset(size.width,y),trackHeight.dp.toPx())
                    drawLine(colors.onSurface,Offset(0f,y),Offset(x,y),trackHeight.dp.toPx())
                    markerFractions.forEach { marker ->
                        val center=Offset(size.width*marker,y)
                        drawCircle(colors.onSurface.copy(alpha=.25f),(2f+trackHeight/2f).dp.toPx(),center)
                        drawCircle(colors.onSurface,(1f+trackHeight/2f).dp.toPx(),center)
                    }
                    if(pulse.value>0)drawCircle(colors.onSurface.copy(alpha=.24f*pulse.value),
                        (4f+2f*(1f-pulse.value)).dp.toPx(),Offset(size.width*pulseFraction,y))
                    if(canSeek)drawCircle(colors.onSurface,(3f+trackHeight/2f).dp.toPx(),Offset(x,y))
                }
            },colors=SliderDefaults.colors(inactiveTrackColor=colors.onSurface.copy(alpha=if(canSeek).40f else .24f)))
    }
    Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween) {
        Text(time(drag?.toLong()?:position),modifier=Modifier.alignByBaseline(),style=MaterialTheme.typography.labelSmall)
        if (state.buffering) CircularProgressIndicator(Modifier.size(18.dp),strokeWidth=2.dp)
        else Text(if(state.session.abVersionId!=null)"A–B 循环" else if(state.session.endOfTrack||state.session.deadlineMs!=null)"睡眠定时已开启" else if(!canSeek)if(state.duration<=0)"播放后可拖动" else "暂不可拖动" else "",
            modifier=Modifier.alignByBaseline(),style=MaterialTheme.typography.labelSmall,color=colors.onSurfaceVariant)
        Text(if(duration>0)time(duration) else "时长待读取",modifier=Modifier.alignByBaseline(),style=MaterialTheme.typography.labelSmall)
    }
    Row(Modifier.fillMaxWidth().padding(top=if(compact)8.dp else 16.dp),horizontalArrangement=Arrangement.SpaceBetween,verticalAlignment=Alignment.CenterVertically) {
        MotionIconButton({model.playbackOrder(state.playbackOrder.next())},Modifier.size(48.dp).semantics{contentDescription="播放方式：${state.playbackOrder.label}"},enabled=state.connected) {
            AppGlyph(state.playbackOrder.glyph,Modifier.size(26.dp),tint=colors.onSurface)
        }
        GlyphButton(Glyph.Previous,"上一轨",model::previous,state.canPrevious)
        MotionIconButton(model::togglePlayback,Modifier.size(if(compact)64.dp else 72.dp).semantics{contentDescription=if(state.playRequested)"暂停" else "播放"},
            enabled=state.connected&&state.queue.isNotEmpty()){AppGlyph(if(state.playRequested)Glyph.Pause else Glyph.Play,Modifier.size(44.dp),filled=true)}
        GlyphButton(Glyph.Next,"下一轨",model::next,state.canNext)
        GlyphButton(Glyph.Queue,"播放队列",queue)
    }
}

@UnstableApi
@Composable
private fun LyricsStage(state:PlayerScreenState,data:LibraryToolsState,model:PlayerViewModel,size:Float,close:()->Unit) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val translated=data.subtitles?.original?.cues.isNullOrEmpty()
    val cues=(if(translated)data.subtitles?.translation else data.subtitles?.original)?.cues.orEmpty()
    val offset=(data.preference?.offsetMs?:0)+(if(translated)data.preference?.translationOffsetMs?:0 else 0)
    val timeline = if (translated) data.subtitles?.translation else data.subtitles?.original
    val playingCues by remember(timeline, offset) { derivedStateOf { timeline?.active(position, offset).orEmpty() } }
    val index by remember(timeline, offset) { derivedStateOf {
        playingCues.lastOrNull()?.let { timeline?.lastStarted(it.startMs) } ?: timeline?.lastStarted(position, offset) ?: -1
    } }
    val translation = data.subtitles?.translation
    val translationOffset = data.preference?.translationOffsetMs ?: 0
    val list=rememberLazyListState()
    var follow by rememberSaveable(data.versionId){mutableStateOf(true)}
    val dragging by list.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(dragging){if(dragging)follow=false}
    val motion=LocalUiMotion.current
    val currentClose by rememberUpdatedState(close)
    // Child cue taps and scroll gestures consume their events; only blank-area taps return.
    BoxWithConstraints(Modifier.fillMaxSize().pointerInput(Unit){detectTapGestures(onTap={currentClose()})}) {
        when {
            hiddenCurrent(state)->Box(Modifier.padding(24.dp)){EmptyContent("内容已隐藏","可在内容显示设置中调整。",Glyph.Eye)}
            data.subtitleBusy->LoadingRing()
            cues.isEmpty()->Column(Modifier.align(Alignment.Center).padding(24.dp),horizontalAlignment=Alignment.CenterHorizontally){
                Text(data.subtitleError?:"暂无台本",textAlign=TextAlign.Center)}
            else -> {
                val anchor = maxHeight / 3
                var previous by remember(data.versionId, cues) { mutableIntStateOf(-1) }
                LaunchedEffect(data.versionId, cues, index, follow, motion) {
                    val adjacent = previous >= 0 && kotlin.math.abs(index - previous) <= 1
                    previous = if (follow) index else -1
                    if (follow && index >= 0) list.followLyric(index, motion && adjacent)
                }
                LazyColumn(Modifier.fillMaxSize(), state=list,
                    contentPadding=PaddingValues(start=28.dp,end=28.dp,top=anchor,bottom=(maxHeight-anchor).coerceAtLeast(0.dp)),
                    verticalArrangement=Arrangement.spacedBy(16.dp),horizontalAlignment=Alignment.Start) {
                items(cues.size,key={it}){i->val cue=cues[i];val active=cue in playingCues
                    val emphasis by animateFloatAsState(if(active)1f else if(kotlin.math.abs(i-index)<=1).86f else .72f,if(motion)tween(160) else snap(),label="lyric emphasis")
                    val seek={data.versionId?.let{model.seekBookmark(it,(cue.startMs+offset).coerceAtLeast(0))};Unit}
                    Column(Modifier.fillMaxWidth().padding(vertical=6.dp).graphicsLayer { alpha=emphasis },horizontalAlignment=Alignment.Start) {
                        LyricLine(cue.text,size,MaterialTheme.colorScheme.onSurface,state.seekable,seek,close)
                        if (!translated) {
                            val paired = remember(cue, translation, translationOffset) {
                                translation?.overlapping(cue.startMs, cue.endMs, translationOffset).orEmpty()
                            }
                            paired.forEach { line ->
                                val time = position - offset - translationOffset
                                val waiting = active && follow && time < line.startMs
                                val spoken = time >= line.startMs && time < line.endMs
                                val opacity by animateFloatAsState(if (waiting) 0f else if (spoken) 1f else .78f,
                                    if (motion) tween(160) else snap(), label="译文亮度")
                                Box(Modifier.padding(top=4.dp).graphicsLayer { alpha=opacity }
                                    .then(if (waiting) Modifier.clearAndSetSemantics {} else Modifier)) {
                                    LyricLine(line.text, size*.8f, MaterialTheme.colorScheme.onSurfaceVariant,
                                        state.seekable && !waiting, seek, close)
                                }
                            }
                        }
                    }
                }
                }
                if (!follow) FilledTonalButton(onClick = { follow = true }, modifier = Modifier.align(Alignment.BottomCenter).padding(8.dp)) {
                    Text("回到当前句")
                }
            }
        }
    }
}

/** Only the text lines seek. Whitespace inside a wrapped text box also returns to the cover. */
@Composable
private fun LyricLine(text:String,size:Float,color:Color,seekable:Boolean,seek:()->Unit,close:()->Unit) {
    var layout by remember(text,size){mutableStateOf<TextLayoutResult?>(null)}
    val currentSeek by rememberUpdatedState(seek)
    val currentClose by rememberUpdatedState(close)
    Text(text,Modifier.pointerInput(text,seekable){
        detectTapGestures { point ->
            val result=layout
            val line=result?.getLineForVerticalPosition(point.y)
            val onText=result!=null&&line!=null&&point.y>=result.getLineTop(line)&&point.y<=result.getLineBottom(line)&&
                point.x>=result.getLineLeft(line)&&point.x<=result.getLineRight(line)
            if(onText){if(seekable)currentSeek()}else currentClose()
        }
    }.semantics { if(seekable){role=Role.Button;onClick(label="播放这一句"){currentSeek();true}} },
        onTextLayout={layout=it},textAlign=TextAlign.Start,fontSize=size.sp,lineHeight=(size*1.5f).sp,fontWeight=FontWeight.Medium,color=color)
}
