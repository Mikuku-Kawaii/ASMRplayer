package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel

@UnstableApi
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun HeadphonePage(state: PlayerScreenState, model: PlayerViewModel, effects: () -> Unit) {
    val data = state.headphones
    val enabled = state.connected && state.headphonesReady && !state.headphonesBusy
    var name by rememberSaveable { mutableStateOf("") }
    var deleting by rememberSaveable { mutableStateOf<String?>(null) }
    var replacing by rememberSaveable { mutableStateOf<String?>(null) }
    val active = data.profiles.firstOrNull { it.id == data.activeId }
    var creating by rememberSaveable { mutableStateOf(false) }
    BackHandler(creating && !state.headphonesBusy) { creating = false }
    if (creating) PlayerAlertDialog(onDismissRequest={if(!state.headphonesBusy)creating=false}, title={Text("保存耳机方案")}, text={
        Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
            MaintenanceHint("保存当前均衡器、音效、响度和平衡设置。")
            OutlinedTextField(name,{name=it.take(80)},label={Text("方案名称")},singleLine=true,modifier=Modifier.fillMaxWidth(),enabled=!state.headphonesBusy)
        }
    },confirmButton={MotionTextButton({model.headphoneAction("save",name=name.trim(),onSuccess={name="";creating=false})},enabled=enabled&&name.isNotBlank()&&data.profiles.size<32){Text("保存")}},
        dismissButton={MotionTextButton({creating=false},enabled=!state.headphonesBusy){Text("取消")}})
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            MaintenanceCard {
                Text("当前生效", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
                Text(if (state.headphonesReady && state.connected) active?.name ?: "通用音效" else "等待播放服务",
                    style = MaterialTheme.typography.headlineSmall)
                MaintenanceValue(if (data.route != null && !state.playing) "最近输出" else "输出设备",
                    if (state.connected) data.route?.label ?: "播放后确认" else "未连接")
                if (state.headphonesReady && data.modified) MaintenanceHint("当前调整尚未保存到方案")
                MotionOutlinedButton(effects) { Text("调整当前音效") }
            }
        }
        if (!state.headphonesReady || state.headphonesBusy) item {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                LoadingRing()
                MaintenanceHint(if (!state.headphonesReady) "正在连接耳机偏好…" else "正在更新耳机偏好…")
            }
        }
        data.warning?.let { message -> item {
            MaintenanceStatus(message, error = message.startsWith("未完成"))
        } }
        item {
            MaintenanceRow("已保存的方案", "通用音效 · ${data.profiles.size} 个自建方案") {
                MotionTextButton({ creating = true }, enabled = enabled && data.profiles.size < 32) { Text("保存为新方案") }
            }
            MaintenanceCard { MaintenanceRow("通用音效", "未关联设备时的默认方案") {
                if (enabled && data.activeId == null) WorkBadge(if (data.modified) "已调整" else "使用中", accent = true)
                MoreActions("通用音效选项") { close ->
                    DropdownMenuItem(text = { Text("应用通用音效") }, enabled = enabled, onClick = { close(); model.headphoneAction("select") })
                    DropdownMenuItem(text = { Text("将当前音效设为通用") }, enabled = enabled, onClick = { close(); model.headphoneAction("common") })
                }
            } }
            if (data.profiles.size >= 32) MaintenanceHint("已保存 32 个自建方案，可更新或删除已有方案。")
        }
        items(data.profiles, key = { it.id }) { profile ->
            val actions: CardMenu = { close ->
                DropdownMenuItem(text = { Text("用当前音效更新") }, enabled = enabled, onClick = { close(); replacing = profile.id })
                DropdownMenuItem(text = { Text("关联当前输出") }, enabled = enabled && data.route?.key != null,
                    onClick = { close(); model.headphoneAction("bind", id = profile.id, route = data.route?.key) })
                DropdownMenuItem(text = { Text("删除方案", color = MaterialTheme.colorScheme.error) }, enabled = enabled, onClick = { close(); deleting = profile.id })
            }
            CardActions(profile.name, menu = actions) {
                MaintenanceCard {
                    Row(Modifier.fillMaxWidth(), verticalAlignment = androidx.compose.ui.Alignment.Top) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(profile.name, style = MaterialTheme.typography.titleMedium)
                            if (state.connected && profile.id == data.activeId) WorkBadge(if (data.modified) "使用中 · 已调整" else "使用中", accent = true)
                        }
                        if (profile.id != data.activeId || data.modified) MotionTextButton({ model.headphoneAction("select", id = profile.id) }, enabled = enabled) {
                            Text(if (profile.id == data.activeId && data.modified) "还原" else "应用")
                        }
                        MoreActions("方案选项 ${profile.name}", content = actions)
                    }
                }
            }
        }
        item { SettingsGroup("设备关联") {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                ToggleRow("按设备自动切换", "切换输出时应用关联方案", data.automatic, enabled) {
                    model.headphoneAction("automatic", enabled = it)
                }
                if (data.automatic) {
                    val binding = data.bindings.firstOrNull { it.key == data.route?.key }
                    val target = data.profiles.firstOrNull { it.id == binding?.profileId }
                    MaintenanceHint(when {
                        data.route == null -> "播放后可关联输出设备，未关联时使用通用音效。"
                        data.route.key == null -> "系统未提供此输出的标识，可在上方手动应用方案。"
                        else -> "${data.route.label} → ${target?.name ?: "通用音效"}"
                    })
                    if (data.route?.sharedPort == true) MaintenanceHint("同一接口上的耳机共用关联方案。")
                }
                if (data.bindings.isNotEmpty()) DetailDisclosure("已关联设备", "${data.bindings.size} 个设备或接口") {
                    data.bindings.forEach { binding ->
                        MaintenanceRow(binding.label, data.profiles.firstOrNull { it.id == binding.profileId }?.name) {
                            MotionTextButton({ model.headphoneAction("unbind", route = binding.key) }, enabled = enabled) { Text("解除关联") }
                        }
                    }
                } else if (data.automatic && data.profiles.isNotEmpty()) MaintenanceHint("在已保存方案的菜单中选择“关联当前输出”。")
            }
        } }
    }

    data.profiles.firstOrNull { it.id == deleting }?.let { profile -> PlayerAlertDialog(onDismissRequest = { deleting = null },
        title = { Text("删除“${profile.name}”？") }, text = { Text("同时移除此方案的设备关联。若正在使用，会切换到通用音效。") },
        confirmButton = { MotionTextButton({ model.headphoneAction("remove", id = profile.id); deleting = null }) { Text("删除") } },
        dismissButton = { MotionTextButton({ deleting = null }) { Text("取消") } }) }
    data.profiles.firstOrNull { it.id == replacing }?.let { profile -> PlayerAlertDialog(onDismissRequest = { replacing = null },
        title = { Text("更新“${profile.name}”？") }, text = { Text("用当前完整音效覆盖此方案，保留已有设备关联。") },
        confirmButton = { MotionTextButton({ model.headphoneAction("save", id = profile.id, name = profile.name); replacing = null }) { Text("更新方案") } },
        dismissButton = { MotionTextButton({ replacing = null }) { Text("取消") } }) }
}
