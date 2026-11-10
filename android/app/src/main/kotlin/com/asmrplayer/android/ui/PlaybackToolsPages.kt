package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.*
import java.text.DateFormat
import java.util.Date

internal fun hiddenCurrent(state: PlayerScreenState) = state.currentWork?.let { !visibleRating(it.rating, state) } == true

internal fun stableCurrent(state: PlayerScreenState) = state.session.versionId?.takeIf {
    state.session.ready && it == state.queue.getOrNull(state.index)?.id
}

@UnstableApi
@Composable
internal fun BookmarksPage(state: PlayerScreenState, data: LibraryToolsState, model: PlayerViewModel, inlineEdit: Boolean = false) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val version = stableCurrent(state)
    var note by rememberSaveable(version) { mutableStateOf("") }
    var editing by remember { mutableStateOf<com.asmrplayer.android.data.Bookmark?>(null) }
    val hidden = hiddenCurrent(state)
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            PageIntro("当前位置 · ${time(position)}",trackDisplayTitle(displayTitle(state)))
            if (version == null) Text("请先从音频库选择音轨。标记绑定具体音轨版本，在线直链暂不支持。")
            else {
                DetailDisclosure("添加备注", note.ifBlank { "可选，便于之后找到这一刻" }) {
                    OutlinedTextField(note, { if (it.length <= 4096) note = it }, label = { Text("标记备注") }, modifier = Modifier.fillMaxWidth())
                }
                MotionButton({ model.addBookmark(version, note) }, enabled = !state.session.busy) { Text("保存当前位置 · ${time(position)}") }
                if (!state.seekable) Text("开始播放后可跳转到标记位置。", style = MaterialTheme.typography.bodySmall)
            }
        }
        if (data.bookmarks.isEmpty() && version != null) item { Text("当前音轨还没有标记。") }
        items(data.bookmarks, key = { it.id }) { bookmark ->
            val actions: CardMenu = { close ->
                CardAction("跳转 ${time(bookmark.positionMs)}", close, state.seekable && bookmark.versionId == version) {
                    model.seekBookmark(bookmark.versionId, bookmark.positionMs)
                }
                CardAction("编辑备注", close) { editing = bookmark }
                CardAction("删除标记", close) { model.collections.removeBookmark(bookmark.id) }
            }
            CardActions("时间标记 · ${time(bookmark.positionMs)}", enabled = editing?.id != bookmark.id, menu = actions) {
                MaintenanceCard {
                    Text(if (hidden) "内容已隐藏" else bookmark.note.ifBlank { "未填写备注" })
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        MotionTextButton({ model.seekBookmark(bookmark.versionId, bookmark.positionMs) }, enabled = state.seekable && bookmark.versionId == version) { Text("跳转 ${time(bookmark.positionMs)}") }
                        if(inlineEdit) {
                            GlyphButton(Glyph.Details,"编辑标记备注",{editing=if(editing?.id==bookmark.id)null else bookmark})
                            GlyphButton(Glyph.Close,"删除标记",{model.collections.removeBookmark(bookmark.id)})
                        } else MoreActions("标记操作", content = actions)
                    }
                    if(inlineEdit && editing?.id==bookmark.id) {
                        var draft by remember(bookmark.id){mutableStateOf(bookmark.note)}
                        OutlinedTextField(draft,{draft=it.take(4096)},label={Text("标记备注")},modifier=Modifier.fillMaxWidth())
                        Row {
                            MotionTextButton({model.collections.editBookmark(bookmark.id,draft);editing=null}){Text("保存")}
                            MotionTextButton({editing=null}){Text("取消")}
                        }
                    }
                    HorizontalDivider()
                }
            }
        }
    }
    if(!inlineEdit)editing?.let { bookmark ->
        var draft by remember(bookmark.id) { mutableStateOf(bookmark.note) }
        PlayerAlertDialog(onDismissRequest={editing=null},title={Text("标记备注")},text={OutlinedTextField(draft,{draft=it.take(4096)},modifier=Modifier.fillMaxWidth())},
            confirmButton={MotionTextButton({model.collections.editBookmark(bookmark.id,draft);editing=null}){Text("保存")}},dismissButton={MotionTextButton({editing=null}){Text("取消")}})
    }

}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun AbLoopPage(state: PlayerScreenState, model: PlayerViewModel) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val version = stableCurrent(state)
    val active = version != null && state.session.abVersionId == version
    var start by rememberSaveable(version, state.session.aMs) { mutableStateOf(state.session.aMs?.let(::secondsText).orEmpty()) }
    var end by rememberSaveable(version, state.session.bMs) { mutableStateOf(state.session.bMs?.let(::secondsText).orEmpty()) }
    val a = secondsToMillis(start)
    val b = secondsToMillis(end)
    val valid = a != null && b != null && a >= 0 && b > a && b <= state.duration
    val enabled = version != null && state.seekable && !state.session.busy
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        item { PageIntro("循环收听",displayTitle(state),Glyph.Repeat) }
        item { Text(if (active) "循环中 · ${time(state.session.aMs ?: 0)} → ${time(state.session.bMs ?: 0)}" else "未开启 A–B 循环") }
        if (version == null || !state.seekable) item { EmptyContent("当前音轨暂不可设置循环","播放可定位的音轨后即可设置 A、B 两点。",Glyph.Repeat) }
        if (version != null && state.seekable) {
        item {
            SettingsGroup("循环范围") {
                Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                    Text("A 起点 · ${a?.let(::time) ?: "尚未设置"}",style=MaterialTheme.typography.titleMedium)
                    MotionOutlinedButton({start=secondsText(position)},enabled=enabled){Text("用当前位置设为 A")}
                    HorizontalDivider()
                    Text("B 终点 · ${b?.let(::time) ?: "尚未设置"}",style=MaterialTheme.typography.titleMedium)
                    MotionOutlinedButton({
                        val next=position
                        end=secondsText(next)
                        if(a!=null&&next>a&&next<=state.duration&&!state.session.endOfTrack)model.setAb(version!!,a,next)
                    },enabled=enabled){Text("用当前位置设为 B")}
                    DetailDisclosure("精确输入","按秒调整起点和终点") {
                        OutlinedTextField(start,{start=it},label={Text("A 点（秒）")},singleLine=true,modifier=Modifier.fillMaxWidth())
                        OutlinedTextField(end,{end=it},label={Text("B 点（秒）")},singleLine=true,modifier=Modifier.fillMaxWidth())
                    }
                }
            }
            Text("先设起点，再设终点。切换音轨会关闭循环。",style=MaterialTheme.typography.bodySmall)
            if(start.isNotBlank()&&end.isNotBlank()&&!valid)Text("终点需晚于起点，并且不超过 ${time(state.duration)}。",color=MaterialTheme.colorScheme.error)
            if (state.session.endOfTrack) Text("请先取消“播完本轨停止”，再开启 A–B 循环。")
        }
        item { FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MotionButton({ model.setAb(version, a, b) }, enabled = enabled && valid && !state.session.endOfTrack) { Text("应用精确时间") }
            MotionOutlinedButton({ model.setAb(version, null, null) }, enabled = active && !state.session.busy) { Text("关闭循环") }
        } }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun SleepPage(state: PlayerScreenState, model: PlayerViewModel, night: Boolean, onNight: (Boolean)->Unit, quick: Boolean = false) {
    var minutes by rememberSaveable { mutableStateOf("30") }
    var appliedPreset by rememberSaveable { mutableStateOf<Int?>(null) }
    var fade by rememberSaveable(state.session.fadeMs) { mutableStateOf(state.session.fadeMs) }
    val value = minutes.toIntOrNull()?.takeIf { it in 1..1440 }
    val enabled = state.connected && state.session.ready && !state.session.busy
    val active = state.session.deadlineMs != null || state.session.endOfTrack
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if(!quick) item { ToggleRow("夜间操作", "此页使用深色表面和大按钮。", night, true, onNight) }
        item { PageIntro(when {
            state.session.endOfTrack -> "已设置：播完本轨停止"
            state.session.deadlineMs != null -> "将在 ${DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(state.session.deadlineMs))} 暂停播放"
            else -> "尚未设置睡眠定时"
        }, "让声音陪你入睡",Glyph.Sleep) }
        
        item { FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(15, 30, 45, 60).forEach { count -> ChoiceChip(active&&!state.session.endOfTrack&&appliedPreset==count, { minutes = count.toString();appliedPreset=count;model.setSleep(count,fadeMs=fade) }, { Text("$count 分钟") },enabled=enabled) }
        } }
        item {
            DetailDisclosure("自定义时长", "$minutes 分钟") {
                OutlinedTextField(minutes,{minutes=it},label={Text("分钟 · 1–1440")},singleLine=true,modifier=Modifier.fillMaxWidth(),isError=value==null,
                    keyboardOptions=androidx.compose.foundation.text.KeyboardOptions(keyboardType=androidx.compose.ui.text.input.KeyboardType.Number))
                MotionButton({ value?.let { model.setSleep(it, fadeMs = fade) } }, enabled = enabled && value != null) { Text("应用自定义时长") }
            }
        }
        item {
            HorizontalDivider()
            MotionOutlinedButton({ model.setSleep(endOfTrack = true, fadeMs = fade) }, enabled = enabled && state.queue.isNotEmpty() && state.session.abVersionId == null) { Text("播完本轨停止") }
            if (state.session.abVersionId != null) Text("先关闭 A–B 循环，再设置播完本轨停止。")
            Text("本轨结束后暂停，不切到下一轨。此选项会替换分钟定时。", style = MaterialTheme.typography.bodySmall)
        }
        item { Text("结束前渐弱", style = MaterialTheme.typography.titleSmall)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(0L, 15_000L, 30_000L, 60_000L, 120_000L).forEach { duration ->
                    ChoiceChip(fade == duration, { fade = duration; if(active)model.setSleepFade(duration) }, { Text(if (duration == 0L) "关闭" else "${duration / 1000} 秒") })
                }
            }
            MaintenanceHint("取消定时后恢复原音量。")
        }
        if (state.session.deadlineMs != null) item {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                MotionOutlinedButton({ model.extendSleep(5) }, enabled = enabled, modifier = Modifier.heightIn(min = 56.dp)) { Text("延长 5 分钟") }
                MotionOutlinedButton({ model.extendSleep(15) }, enabled = enabled, modifier = Modifier.heightIn(min = 56.dp)) { Text("延长 15 分钟") }
            }
        }
        if (active) item { MotionTextButton({ model.setSleep() }, enabled = enabled) { Text("取消定时") } }
    }
}
