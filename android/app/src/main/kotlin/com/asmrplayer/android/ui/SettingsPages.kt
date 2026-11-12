package com.asmrplayer.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel

private data class SettingEntry(
    val group: String, val title: String, val summary: String, val route: String, val keywords: String = "",
) {
    fun matches(query: String) = listOf(title, summary, group, keywords).any { it.contains(query, ignoreCase = true) }
}

private val settingsEntries=listOf(
    SettingEntry("资料库与同步","作品目录与扫描","导入、目录授权与重新扫描","directories","文件夹 本地路径"),
    SettingEntry("资料库与同步","在线来源","来源地址与访问凭据","online-source","API Token"),
    SettingEntry("资料库与同步","跨设备同步","GitHub Gist、OneDrive、WebDAV 和 Supabase","sync","局域网 自动同步"),
    SettingEntry("资料库与同步","资料备份与恢复","作品、喜欢、进度、标记与媒体授权","backup","还原 数据迁移"),
    SettingEntry("下载与存储","任务中心","离线下载、扫描、同步与双端继续收听","tasks","下载管理 失败任务"),
    SettingEntry("下载与存储","下载存储与缓存","占用统计与封面缓存","storage","清理缓存 空间不足"),
    SettingEntry("播放与字幕","音效与均衡器","预设、PEQ、AutoEQ、声道与响度","effects","EQ 左右声道 响度均衡"),
    SettingEntry("播放与字幕","耳机音效偏好","保存方案、输出设备与自动切换","headphones","蓝牙 有线耳机"),
    SettingEntry("播放与字幕","字幕设置","原文、译文、导入与时间偏移","subtitle-settings","台本 时间对齐"),
    SettingEntry("界面与系统","外观","颜色预设、明暗与低效果模式","appearance",
        "主题 配色 浅色 深色 跟随系统 关闭动效 " + ColorPreset.entries.joinToString(" ") { it.label }),
    SettingEntry("界面与系统","内容显示","NSFW 与未分级作品","content-settings","R18 SFW 成人向 全年龄"),
    SettingEntry("界面与系统","系统媒体与灵动岛","通知、锁屏与媒体隐私","system-media","隐藏封面 隐藏标题"),
    SettingEntry("界面与系统","应用更新","检查版本、下载与安装","updates","版本号 更新日志"))

@Composable
internal fun SettingsPage(state:PlayerScreenState,open:(String)->Unit) {
    var query by rememberSaveable{mutableStateOf("")}
    val groups = remember(query) {
        val terms = query.trim().split(Regex("\\s+")).filter(String::isNotBlank)
        settingsEntries.filter { entry -> terms.all(entry::matches) }.groupBy { it.group }
    }
    LazyColumn(Modifier.navigationBarsPadding(),contentPadding=PageSpace.padding,verticalArrangement=Arrangement.spacedBy(12.dp)) {
        item{PageSearch(query,{query=it},"搜索设置")}
        groups.forEach{(group,rows)->item(key=group){SettingsGroup(group){rows.forEachIndexed{index,entry->
            SettingsLink(entry.title,when(entry.route){
                "appearance"->"${ColorPreset.from(state.colorPreset).label} · ${when(state.theme){"light"->"浅色";"dark"->"深色";else->"跟随系统"}}"
                "content-settings"->if(state.showNsfw)"包含 NSFW 内容" else "隐藏 NSFW 内容"
                "effects"->if(state.dsp.bypass)"正在原声对比" else "音效已开启"
                "headphones"->state.headphones.profiles.firstOrNull{it.id==state.headphones.activeId}?.name?:"通用音效"
                "system-media"->if(state.privateMetadata)"已隐藏系统媒体标题与封面" else "显示作品信息"
                else->entry.summary
            }){open(entry.route)}
            if(index<rows.lastIndex)HorizontalDivider(Modifier.padding(horizontal=16.dp),color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.3f))
        }}}}
        if(groups.isEmpty())item{EmptyContent("没有匹配的设置","试试“字幕”“下载”或“颜色”等关键词。",Glyph.Search)}
    }
}

@OptIn(ExperimentalLayoutApi::class)
@UnstableApi
@Composable
internal fun AppearancePage(state:PlayerScreenState,model:PlayerViewModel) {
    val dark = MaterialTheme.colorScheme.background.luminance() < .5f
    LazyColumn(Modifier.navigationBarsPadding(),contentPadding=PageSpace.padding,verticalArrangement=Arrangement.spacedBy(12.dp)) {
        
        item { SettingsGroup("颜色预设") {
            BoxWithConstraints(Modifier.fillMaxWidth().padding(12.dp)) {
                val columns = if (maxWidth >= 280.dp && LocalDensity.current.fontScale <= 1.3f) 2 else 1
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    ColorPreset.entries.chunked(columns).forEach { presets ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            presets.forEach { preset ->
                                PresetOption(preset, dark, state.colorPreset == preset.key, Modifier.weight(1f)) { model.setColorPreset(preset.key) }
                            }
                            if (presets.size < columns) Spacer(Modifier.weight(1f))
                        }
                    }
                }
            }
        } }
        item{SettingsGroup("明暗模式"){Column(Modifier.padding(16.dp)){
            FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)){listOf("system" to "跟随系统","light" to "浅色","dark" to "深色").forEach{(key,label)->
                ChoiceChip(state.theme==key,{model.setTheme(key)},{Text(label)})}}
            MaintenanceHint("播放页使用当前封面的深色氛围。")
        }}}
        item{MaintenanceCard{ToggleRow("低效果模式","使用实色表面，减少视觉动效。",state.lowEffects,true,model::setLowEffects)}}
    }
}

@UnstableApi
@Composable
internal fun ContentPreferencesPage(state:PlayerScreenState,model:PlayerViewModel) {
    LazyColumn(Modifier.navigationBarsPadding(),contentPadding=PageSpace.padding,verticalArrangement=Arrangement.spacedBy(16.dp)) {
        
        item{MaintenanceCard{
            ToggleRow("显示未分级作品","未分级不代表全年龄。",state.includeUnrated,true,model::setIncludeUnrated)
            HorizontalDivider(color=MaterialTheme.colorScheme.outlineVariant.copy(alpha=.4f))
            ToggleRow("显示 NSFW 内容","只影响浏览筛选，保留当前队列与播放。",state.showNsfw,true,model::setShowNsfw)
        }}
    }
}

@Composable
private fun PresetOption(preset: ColorPreset, dark: Boolean, active: Boolean, modifier: Modifier, choose: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Surface(onClick = choose, modifier = modifier.semantics { selected = active; role = Role.RadioButton },
        shape = MaterialTheme.shapes.medium, color = if (active) colors.primaryContainer else colors.surface,
        border = if (active) androidx.compose.foundation.BorderStroke(1.dp, colors.primary) else null) {
        Row(Modifier.heightIn(min = 56.dp).padding(8.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.size(36.dp).clip(MaterialTheme.shapes.small)
                .background(if (dark) preset.background else preset.lightBackground).padding(5.dp),
                verticalArrangement = Arrangement.SpaceBetween) {
                Box(Modifier.size(12.dp).clip(RoundedCornerShape(3.dp)).background(if (dark) preset.accent else preset.lightAccent))
                Box(Modifier.fillMaxWidth().height(6.dp).background(if (dark) preset.panel else preset.lightPanel))
            }
            Text(preset.label, Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
            if (active) AppGlyph(Glyph.Check, Modifier.size(16.dp))
        }
    }
}
