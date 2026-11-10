package com.asmrplayer.android.ui

import android.net.Uri
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.drawBehind
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.collectIsDraggedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun SubtitlePage(state: PlayerScreenState, data: LibraryToolsState, model: PlayerViewModel, settings: () -> Unit) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val version = data.versionId
    var preferredTimeline by rememberSaveable(version) { mutableStateOf<Boolean?>(null) }
    val translatedTimeline = preferredTimeline ?: (data.subtitles?.original?.cues.isNullOrEmpty() && !data.subtitles?.translation?.cues.isNullOrEmpty())
    var follow by rememberSaveable(version) { mutableStateOf(true) }
    val hidden = hiddenCurrent(state)
    val subtitles = data.subtitles
    val timeline = if (translatedTimeline) subtitles?.translation else subtitles?.original
    val cues = (if (translatedTimeline) subtitles?.translation else subtitles?.original)?.cues.orEmpty()

    val offset = (data.preference?.offsetMs ?: 0) + if (translatedTimeline) data.preference?.translationOffsetMs ?: 0 else 0
    val listState = rememberLazyListState()
    val dragging by listState.interactionSource.collectIsDraggedAsState()
    LaunchedEffect(dragging) { if (dragging) follow = false }
    val activeCues by remember(timeline, offset) { derivedStateOf { timeline?.active(position, offset).orEmpty() } }
    val selected by remember(timeline, offset) { derivedStateOf {
        activeCues.lastOrNull()?.let { timeline?.lastStarted(it.startMs) } ?: timeline?.lastStarted(position, offset) ?: -1
    } }
    val motion = LocalUiMotion.current
    var previous by remember(version, translatedTimeline, cues) { mutableIntStateOf(-1) }
    LaunchedEffect(version, cues, selected, follow, hidden, motion) {
        val adjacent = previous >= 0 && kotlin.math.abs(previous - selected) <= 1
        previous = if (follow && !hidden) selected else -1
        if (follow && selected >= 0 && !hidden) {
            val layout = listState.layoutInfo
            val row = layout.visibleItemsInfo.firstOrNull { it.index == selected }
            val margin = (layout.viewportEndOffset - layout.viewportStartOffset) / 10
            if (row == null || row.offset < layout.viewportStartOffset + margin ||
                row.offset + row.size > layout.viewportEndOffset - margin) listState.followLyric(selected, motion && adjacent)
        }
    }
    Column(Modifier.fillMaxSize().navigationBarsPadding().padding(horizontal = 20.dp)) {
        Text(displayTitle(state), style = MaterialTheme.typography.titleMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MotionTextButton(settings) { Text("字幕设置") }
            MotionTextButton(model::togglePlayback, enabled = state.queue.isNotEmpty()) { Text(if (state.playRequested) "暂停" else "播放") }
        }
        when {
            hidden -> EmptyContent("内容已隐藏", "开启 NSFW 显示后可以查看对应字幕。", Glyph.Eye)
            version == null -> EmptyContent("当前没有可绑定字幕的音轨", "请从音频库播放具体音轨版本。", Glyph.Audio)
            data.subtitleBusy -> LoadingRing()
            data.subtitleError != null -> {
                Text(data.subtitleError, color = MaterialTheme.colorScheme.error)
                MotionTextButton(model.tools::retrySubtitles) { Text("重新加载字幕") }
            }
            else -> {
                PageTabs(translatedTimeline, listOf(false to "原文时间轴", true to "译文时间轴")) { preferredTimeline = it }
                ChoiceChip(follow, { follow = !follow }, { Text(if (follow) "跟随播放" else "回到当前句") })
                val error = if (translatedTimeline) subtitles?.translationError else subtitles?.originalError
                if (error != null) {
                    MaintenanceHint(error, error = true)
                    MotionTextButton({ model.tools.retrySubtitle(translatedTimeline) }, enabled = !data.subtitleBusy) { Text("重试字幕") }
                } else if (cues.isEmpty()) Text("在字幕设置中选择、导入或关联作品中的字幕。")
                LazyColumn(Modifier.weight(1f), state = listState, contentPadding = PaddingValues(bottom = 24.dp)) {
                    items(cues.size, key = { it }) { index ->
                        val cue = cues[index]
                        val strength = animateFloatAsState(if (cue in activeCues) 1f else 0f,
                            if (motion) tween(180) else snap(), label = "时间轴高亮")
                        val accent = MaterialTheme.colorScheme.primary
                        Row(Modifier.fillMaxWidth().drawBehind {
                            val alpha = strength.value
                            drawRect(accent.copy(alpha = alpha * .09f))
                            val bar = 3.dp.toPx()
                            val x = if (layoutDirection == androidx.compose.ui.unit.LayoutDirection.Ltr) 0f else size.width - bar
                            drawRect(accent.copy(alpha = alpha), androidx.compose.ui.geometry.Offset(x, 0f),
                                androidx.compose.ui.geometry.Size(bar, size.height))
                        }.clickable(enabled = state.seekable) { model.seekBookmark(version, (cue.startMs + offset).coerceAtLeast(0)) }
                            .padding(horizontal = 12.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                            Text(time((cue.startMs + offset).coerceAtLeast(0)), Modifier.alignByBaseline().widthIn(min = 44.dp),
                                style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(cue.text, Modifier.weight(1f).alignByBaseline(), style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SubtitleChoice(selected: String?, data: LibraryToolsState, inlineChoices: Boolean = false, emptyLabel: String = "不显示", choose: (String?) -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Column {
        
        if(inlineChoices) {
            ChoiceChip(selected==null,{choose(null)},{Text("不显示")},enabled=!data.subtitleSaving)
            data.sources.forEach { source ->
                ChoiceChip(selected==source.id,{choose(source.id)},{Text(source.title)},enabled=source.available&&!data.subtitleSaving)
            }
        } else
        Box {
            MotionOutlinedButton({ menu = true }, enabled = !data.subtitleSaving, modifier = Modifier.fillMaxWidth()) {
                Text(data.sources.firstOrNull { it.id == selected }?.let { it.title + if (it.available) "" else "（不可用）" } ?: if (selected == null) emptyLabel else "已选文件不可用")
            }
            DropdownMenu(menu, { menu = false }) {
                DropdownMenuItem(text = { Text("不显示") }, onClick = { menu = false; choose(null) })
                data.sources.forEach { source ->
                    DropdownMenuItem(text = { Text(source.title + if (source.available) "" else "（不可用）") }, enabled = source.available,
                        onClick = { menu = false; choose(source.id) })
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SubtitleSettingsPage(state: PlayerScreenState, data: LibraryToolsState, tools: LibraryTools, inlineChoices: Boolean = false, import: (String, Boolean) -> Unit) {
    val version = data.versionId
    val pref = data.preference
    val hidden = hiddenCurrent(state)
    var originalOffset by rememberSaveable(version, pref?.offsetMs) { mutableStateOf(secondsText(pref?.offsetMs ?: 0)) }
    var translationOffset by rememberSaveable(version, pref?.translationOffsetMs) { mutableStateOf(secondsText(pref?.translationOffsetMs ?: 0)) }
    var assetRole by rememberSaveable(version) { mutableStateOf<Boolean?>(null) }
    val first = secondsToMillis(originalOffset)?.takeIf { it in -600_000..600_000 }
    val second = secondsToMillis(translationOffset)?.takeIf { it in -600_000..600_000 }
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        item { Text("当前音轨", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(displayTitle(state), style = MaterialTheme.typography.titleMedium) }
        if (version == null || hidden) item { Text(if (hidden) "内容已隐藏。开启 NSFW 显示后可管理字幕。" else "请先从音频库选择音轨版本。") }
        else {
            if (data.subtitleSaving || data.subtitleBusy) item { LoadingRing() }
            data.subtitleError?.let { error -> item {
                Text(error, color = MaterialTheme.colorScheme.error)
                MotionTextButton(tools::retrySubtitles) { Text("重新加载字幕") }
            } }
            for (translation in listOf(false, true)) item(key = if (translation) "translation" else "original") {
                val role = if (translation) "译文" else "原文"
                val selected = if (translation) pref?.translationId else pref?.originalId
                val timeline = if (translation) data.subtitles?.translation else data.subtitles?.original
                val error = if (translation) data.subtitles?.translationError else data.subtitles?.originalError
                MaintenanceCard {
                    Row(verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
                        Text("${role}字幕", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                        MaintenanceHint(when {
                            error != null -> "读取失败"
                            timeline != null -> "${timeline.cues.size} 句"
                            data.subtitleBusy && (!translation || selected != null) -> "读取中"
                            data.sources.isEmpty() -> "未找到字幕"
                            else -> "未启用"
                        }, error = error != null)
                    }
                    if (data.sources.isNotEmpty() || selected != null)
                        SubtitleChoice(selected, data, inlineChoices, if (error != null) "重新选择字幕" else "不显示") { tools.chooseSubtitle(version, translation, it) }
                    if (error != null) {
                        MaintenanceHint(error, error = true)
                        MotionTextButton({ tools.retrySubtitle(translation) }, enabled = !data.subtitleBusy) { Text("重试${role}") }
                    }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        MotionOutlinedButton({ import(version, translation) }, enabled = !data.subtitleSaving) { Text("导入${role}文件") }
                        if (data.assets.any { it.available })
                            MotionTextButton({ assetRole = translation }, enabled = !data.subtitleSaving) { Text("从作品附件选择") }
                    }
                }
            }
            if(inlineChoices && assetRole!=null) item {
                Text(if(assetRole==true)"选择作品译文" else "选择作品原文",style=MaterialTheme.typography.titleSmall)
                data.assets.filter{it.available}.forEach { asset ->
                    MotionTextButton({tools.attachSubtitle(version,assetRole==true,Uri.parse(asset.uri),asset);assetRole=null}){Text(asset.relativePath)}
                }
                MotionTextButton({assetRole=null}){Text("收起附件")}
            }
            item { MaintenanceCard { DetailDisclosure("时间对齐","整体 ${secondsText(pref?.offsetMs?:0)} 秒 · 译文 ${secondsText(pref?.translationOffsetMs?:0)} 秒") {
                OutlinedTextField(originalOffset, { originalOffset = it }, label = { Text("整体偏移（秒）") }, singleLine = true, isError = first == null,
                    modifier = Modifier.fillMaxWidth().semantics { contentDescription = "整体偏移（秒）" })
                OutlinedTextField(translationOffset, { translationOffset = it }, label = { Text("译文额外偏移（秒）") }, singleLine = true, isError = second == null,
                    modifier = Modifier.fillMaxWidth().semantics { contentDescription = "译文额外偏移（秒）" })
                Text("正值延后、负值提前；范围为 ±600 秒。译文会叠加整体偏移。", style = MaterialTheme.typography.bodySmall)
                MotionButton({ if (first != null && second != null) tools.saveOffsets(version, first, second) }, enabled = first != null && second != null && !data.subtitleSaving) { Text("保存偏移") }
            } } }
        }
    }
    if(!inlineChoices)assetRole?.let { translation ->
        PlayerAlertDialog(onDismissRequest = { assetRole = null }, title = { Text(if (translation) "选择作品译文" else "选择作品原文") }, text = {
            LazyColumn(Modifier.heightIn(max = 360.dp)) {
                items(data.assets.filter { it.available }, key = { it.id }) { asset ->
                    MotionTextButton({ if (version != null) tools.attachSubtitle(version, translation, Uri.parse(asset.uri), asset); assetRole = null }) { Text(asset.relativePath) }
                }
            }
        }, confirmButton = { MotionTextButton({ assetRole = null }) { Text("关闭") } })
    }
}
