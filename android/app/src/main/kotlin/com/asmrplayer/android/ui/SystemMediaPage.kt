package com.asmrplayer.android.ui

import android.app.NotificationManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel
import com.asmrplayer.android.playback.PlaybackService

@UnstableApi
@Composable
internal fun SystemMediaPage(state: PlayerScreenState, model: PlayerViewModel) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    var notifications by remember { mutableStateOf(true) }
    var channel by remember { mutableStateOf<String?>(null) }
    fun refresh() {
        notifications = NotificationManagerCompat.from(context).areNotificationsEnabled()
        channel = if (Build.VERSION.SDK_INT >= 26) context.getSystemService(NotificationManager::class.java)
            .getNotificationChannel(PlaybackService.MEDIA_CHANNEL)?.let { if (it.importance == NotificationManager.IMPORTANCE_NONE) "已关闭" else "已开启" } else "由系统统一管理"
    }
    fun settings(intent: Intent) {
        try { context.startActivity(intent) }
        catch (_: android.content.ActivityNotFoundException) {
            try { context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))) }
            catch (_: android.content.ActivityNotFoundException) { model.showError("无法打开设置，请从系统设置中找到 ASMR Player。") }
        }
    }
    DisposableEffect(owner, context) {
        refresh()
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) refresh() }
        owner.lifecycle.addObserver(observer)
        onDispose { owner.lifecycle.removeObserver(observer) }
    }
    LazyColumn(Modifier.navigationBarsPadding(), contentPadding = PageSpace.padding, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        item { MaintenanceCard("锁屏与通知") {
            ToggleRow("隐藏系统媒体标题与封面", "同时应用于通知、锁屏、桌面小组件和系统媒体卡片。", state.privateMetadata, state.connected, model::setPrivacy)
            HorizontalDivider()
            DetailDisclosure("连接状态",if(notifications)"应用通知已开启" else "应用通知已关闭") {
            MaintenanceValue("媒体会话",if(state.connected)"已连接" else "正在连接")
            MaintenanceValue("应用通知",if(notifications)"已开启" else "已关闭")
            MaintenanceValue("播放通知",channel?:"首次播放后创建")
            }
            MotionOutlinedButton({ settings(if (Build.VERSION.SDK_INT >= 26) Intent(Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName).putExtra(Settings.EXTRA_CHANNEL_ID, PlaybackService.MEDIA_CHANNEL)
                else Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))) }) { Text("播放通知设置") }
        } }
        item { MaintenanceCard("桌面播放小组件") {
            MaintenanceHint("在桌面查看当前音轨，控制播放和切歌。")
            MotionOutlinedButton({
                val manager=android.appwidget.AppWidgetManager.getInstance(context)
                if(Build.VERSION.SDK_INT>=26 && manager.isRequestPinAppWidgetSupported) {
                    manager.requestPinAppWidget(android.content.ComponentName(context,com.asmrplayer.android.widget.PlaybackWidget::class.java),null,null)
                } else model.tools.message("请长按手机桌面，在小组件列表中选择 ASMR Player。")
            }) { Text("添加到桌面") }
            MotionTextButton({ context.startActivity(Intent(context,com.asmrplayer.android.widget.WidgetConfigurationActivity::class.java)) }) { Text("组件外观") }
        } }
        item { DetailDisclosure("厂商胶囊与灵动岛","查看本机兼容说明") {
            MaintenanceHint("显示由系统版本、机型和支持名单决定；专属卡片尚未接入厂商审核接口。")
            val maker=android.os.Build.MANUFACTURER.lowercase()
            MaintenanceHint(when {
                maker.contains("oppo")||maker.contains("oneplus")->"ColorOS：可在通知与控制中心的流体云中查看支持应用；未列出时使用系统媒体通知与锁屏控制。"
                maker.contains("xiaomi")||maker.contains("redmi")->"HyperOS：当前提供标准媒体控制，超级岛专属卡片需厂商准入。"
                maker.contains("samsung")->"One UI：支持机型可在 Now bar 使用音乐控制，显示由系统决定。"
                else->"提供标准通知与锁屏媒体控制；厂商专属胶囊由相应系统支持。"
            })
        } }
        item { MotionTextButton({ settings(Intent(Settings.ACTION_SETTINGS)) }) { Text("打开系统设置") } }
    }
}
