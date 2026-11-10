package com.asmrplayer.android.playback

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.AudioTrack
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicLong

/** Observes the actual Media3 AudioTrack, never guesses the active device from connected-device order. */
@Suppress("DEPRECATION")
class OutputRouteMonitor(context: Context, private val changed: (OutputRoute?) -> Unit) {
    private val manager = context.getSystemService(AudioManager::class.java)
    private val handler = Handler(Looper.getMainLooper())
    private val generation = AtomicLong()
    private var track: AudioTrack? = null
    private var route: OutputRoute? = null
    private var closed = false
    private val listener = AudioTrack.OnRoutingChangedListener { current -> if (current === track) read(current) }
    private val devices = object : AudioDeviceCallback() {
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
            if (removed.any { it.id == route?.deviceId }) { route = null; changed(null) }
        }
    }
    init { manager.registerAudioDeviceCallback(devices, handler) }
    fun attached(created: AudioTrack) {
        val stamp = generation.incrementAndGet()
        handler.post {
            if (closed || stamp != generation.get()) return@post
            detach()
            track = created
            runCatching { created.addOnRoutingChangedListener(listener, handler) }
            read(created)
        }
    }
    private fun read(current: AudioTrack) {
        val device = runCatching { current.routedDevice }.getOrNull() ?: return
        route = describe(device)
        changed(route)
    }
    private fun detach() { track?.let { runCatching { it.removeOnRoutingChangedListener(listener) } }; track = null }
    fun close() { closed = true; generation.incrementAndGet(); manager.unregisterAudioDeviceCallback(devices); detach() }

    companion object {
        fun describe(device: AudioDeviceInfo): OutputRoute {
            val type = device.type
            val port = type in setOf(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioDeviceInfo.TYPE_BUILTIN_EARPIECE,
                AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET)
            val kind = when (type) {
                AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "手机扬声器"
                AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "听筒"
                AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET -> "有线耳机接口"
                AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLE_SPEAKER, AudioDeviceInfo.TYPE_BLE_BROADCAST -> "蓝牙音频"
                AudioDeviceInfo.TYPE_HEARING_AID -> "助听设备"
                AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_USB_ACCESSORY, AudioDeviceInfo.TYPE_USB_HEADSET -> "USB 音频"
                AudioDeviceInfo.TYPE_HDMI, AudioDeviceInfo.TYPE_HDMI_ARC, AudioDeviceInfo.TYPE_HDMI_EARC -> "HDMI 音频"
                else -> "音频输出"
            }
            val name = runCatching { device.productName.toString() }.getOrNull()?.takeIf { it.isNotBlank() }
            val address = if (Build.VERSION.SDK_INT >= 28) runCatching { device.address }.getOrNull()?.trim()?.takeIf {
                it.isNotBlank() && it !in setOf("00:00:00:00:00:00", "02:00:00:00:00:00")
            } else null
            val key = if (port) "port:$type" else address?.let {
                MessageDigest.getInstance("SHA-256").digest("$type:$it".toByteArray()).joinToString("") { b -> "%02x".format(b) }
            }
            return OutputRoute(device.id, if (port || name == null) kind else "$name · $kind", key, port)
        }
    }
}
