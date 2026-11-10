package com.asmrplayer.android.playback

import com.asmrplayer.android.data.CacheMetrics
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.media3.common.*
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import androidx.media3.exoplayer.audio.ForwardingAudioSink
import androidx.media3.session.*
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.R
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.core.DspConfig
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import java.util.concurrent.atomic.AtomicReference

@UnstableApi
class PlaybackService : MediaSessionService() {
    private lateinit var player: ExoPlayer
    private var session: MediaSession? = null
    private val config = AtomicReference(DspConfig())
    private val normalization = AtomicReference(0.0)
    private var loudness: LoudnessSession? = null
    private lateinit var processor: DspAudioProcessor
    private val preferences by lazy { getSharedPreferences("playback", MODE_PRIVATE) }
    private var privateMetadata = false
    private val titles = mutableMapOf<String, String>()
    private var settingsError: String? = null
    private lateinit var librarySession: LibrarySessionCoordinator
    private lateinit var headphones: HeadphonePreferences
    private lateinit var output: OutputRouteMonitor
    private lateinit var artwork: SessionArtwork

    private var revision = 0L
    private fun changed(kind: String) {
        session?.setSessionExtras(Bundle().apply { putString("changed", kind); putLong("revision", ++revision) })
    }

    override fun onCreate() {
        super.onCreate()
        preferences.getString("dsp", null)?.let { saved ->
            try { config.set(DspSettings.decode(saved)) }
            catch (_: Exception) { settingsError = "音效设置无法读取，当前使用默认音效，请重新保存。" }
        }
        privateMetadata = preferences.getBoolean("privateMetadata", false)
        headphones = HeadphonePreferences(preferences, config.get()) { next ->
            config.set(next)
            loudness?.refresh()
            preferences.edit().putString("dsp", DspSettings.encode(next)).apply()
        }
        output = OutputRouteMonitor(this) { route -> headphones.routeChanged(route); changed("sound") }
        processor = DspAudioProcessor(config, normalization)
        val renderers = object : DefaultRenderersFactory(this) {
            override fun buildAudioSink(context: Context, enableFloatOutput: Boolean, enableAudioTrackPlaybackParams: Boolean): AudioSink =
                object : ForwardingAudioSink(DefaultAudioSink.Builder(context)
                    .setEnableFloatOutput(false)
                    .setEnableAudioTrackPlaybackParams(false)
                    .setAudioProcessors(arrayOf(processor))
                    .setAudioTrackProvider(object : DefaultAudioSink.AudioTrackProvider {
                        override fun getAudioTrack(audioTrackConfig: AudioSink.AudioTrackConfig, audioAttributes: AudioAttributes,
                            audioSessionId: Int, context: Context?): android.media.AudioTrack =
                            DefaultAudioSink.AudioTrackProvider.DEFAULT.getAudioTrack(audioTrackConfig, audioAttributes, audioSessionId, context)
                                .also(output::attached)
                    })
                    .build()) {
                    // Route changes must never enable encoded passthrough around the DSP.
                    override fun supportsFormat(format: Format): Boolean =
                        format.sampleMimeType == MimeTypes.AUDIO_RAW && super.supportsFormat(format)
                    override fun getFormatSupport(format: Format): Int =
                        if (format.sampleMimeType == MimeTypes.AUDIO_RAW) super.getFormatSupport(format) else AudioSink.SINK_FORMAT_UNSUPPORTED
                }
        }
        player = ExoPlayer.Builder(this, renderers)
            .setMediaSourceFactory(androidx.media3.exoplayer.source.DefaultMediaSourceFactory(VersionDataSource.Factory(this, AndroidBackend.get(this))))
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(), true)
            .setHandleAudioBecomingNoisy(true)
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .setSeekBackIncrementMs(5000)
            .setSeekForwardIncrementMs(5000)
            .build()
        player.repeatMode = preferences.getInt("repeatMode", Player.REPEAT_MODE_OFF).coerceIn(0,2)
        player.shuffleModeEnabled = preferences.getBoolean("shuffle", false)
        if(player.shuffleModeEnabled)player.repeatMode=Player.REPEAT_MODE_ALL
        player.addListener(object : Player.Listener {
            override fun onRepeatModeChanged(repeatMode: Int) { preferences.edit().putInt("repeatMode", repeatMode).apply() }
            override fun onShuffleModeEnabledChanged(shuffleModeEnabled:Boolean) { preferences.edit().putBoolean("shuffle",shuffleModeEnabled).apply() }
        })
        player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
            .setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, true)
            .setAudioOffloadPreferences(TrackSelectionParameters.AudioOffloadPreferences.Builder()
                .setAudioOffloadMode(TrackSelectionParameters.AudioOffloadPreferences.AUDIO_OFFLOAD_MODE_DISABLED).build())
            .build()
        val activity = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java).setAction(MainActivity.OPEN_PLAYER)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        session = MediaSession.Builder(this, player).setSessionActivity(activity).setCallback(Callback()).build()
        setMediaNotificationProvider(DefaultMediaNotificationProvider.Builder(this)
            .setChannelId(MEDIA_CHANNEL).setChannelName(R.string.media_channel_name).build().apply { setSmallIcon(R.drawable.ic_media_notification) })
        artwork = SessionArtwork(this, player, { privateMetadata }, ::metadata)
        librarySession = LibrarySessionCoordinator(player, AndroidBackend.get(this), titles, ::metadata, getSharedPreferences("sleep", MODE_PRIVATE)) { changed("session") }
        player.addListener(object : Player.Listener {
            private var bufferingSince: Long? = null
            override fun onPlaybackStateChanged(playbackState: Int) {
                val now = android.os.SystemClock.elapsedRealtime()
                if (playbackState == Player.STATE_BUFFERING) bufferingSince = now
                else bufferingSince?.let { CacheMetrics.bufferingMs.addAndGet(now - it); bufferingSince = null }
                CacheMetrics.report()
            }

            override fun onEvents(player: Player, events: Player.Events) {
                if (events.containsAny(Player.EVENT_MEDIA_METADATA_CHANGED, Player.EVENT_TIMELINE_CHANGED, Player.EVENT_PLAY_WHEN_READY_CHANGED, Player.EVENT_PLAYBACK_STATE_CHANGED, Player.EVENT_POSITION_DISCONTINUITY, Player.EVENT_IS_PLAYING_CHANGED))
                    com.asmrplayer.android.widget.PlaybackWidget.publish(this@PlaybackService, player)
            }
        })
        com.asmrplayer.android.widget.PlaybackWidget.publish(this, player)
        loudness = LoudnessSession(this, player, config, normalization)
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    private fun metadata(item: MediaItem): MediaItem = item.buildUpon().setMediaMetadata(
        MediaMetadata.Builder().setTitle(if (privateMetadata) "ASMR Audio Track" else titles[item.mediaId] ?: "音频")
            .setArtist("ASMR Player").setMediaType(MediaMetadata.MEDIA_TYPE_MUSIC).setIsPlayable(true)
            .setExtras(Bundle().apply { putBoolean("hasWorkArtwork", artwork.hasCover(item.mediaId)) })
            .setArtworkData(artwork.forItem(item.mediaId), MediaMetadata.PICTURE_TYPE_FRONT_COVER).build()
    ).build()

    private inner class Callback : MediaSession.Callback {
        override fun onConnect(session: MediaSession, controller: MediaSession.ControllerInfo): MediaSession.ConnectionResult {
            val own = controller.packageName == packageName
            val commands = MediaSession.ConnectionResult.DEFAULT_SESSION_COMMANDS.buildUpon()
            if (own) (listOf(DspSettings.GET_DSP, DspSettings.SET_DSP, DspSettings.SET_PRIVACY, DspSettings.ANALYZE, DspSettings.CANCEL_ANALYSIS, HeadphoneCommands.GET, HeadphoneCommands.EDIT) + LibraryPlayback.commands).forEach {
                commands.add(SessionCommand(it, Bundle.EMPTY))
            }
            val playerCommands = MediaSession.ConnectionResult.DEFAULT_PLAYER_COMMANDS.buildUpon()
            if (!own) {
                playerCommands.remove(Player.COMMAND_CHANGE_MEDIA_ITEMS)
                playerCommands.remove(Player.COMMAND_SET_MEDIA_ITEM)
            }
            return MediaSession.ConnectionResult.AcceptedResultBuilder(session)
                .setAvailableSessionCommands(commands.build()).setAvailablePlayerCommands(playerCommands.build()).build()
        }

        override fun onAddMediaItems(session: MediaSession, controller: MediaSession.ControllerInfo, mediaItems: List<MediaItem>): ListenableFuture<List<MediaItem>> {
            if (controller.packageName != packageName) return Futures.immediateFailedFuture(SecurityException("Unsupported controller"))
            return try {
                require(mediaItems.size <= 500)
                mediaItems.forEach { item ->
                    val uri = requireNotNull(item.localConfiguration?.uri)
                    val debugLoopback = applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE != 0 && uri.scheme == "http" && uri.host == "127.0.0.1"
                    require(uri.scheme in setOf("content", "https", "asmr") || debugLoopback)
                    require(item.mediaId.isNotBlank())
                }
                librarySession.add(mediaItems) { uri -> contentResolver.persistedUriPermissions.any { it.uri == uri && it.isReadPermission } }
            } catch (e: IllegalArgumentException) { Futures.immediateFailedFuture(e) }
        }

        @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
        override fun onPlayerCommandRequest(session: MediaSession, controller: MediaSession.ControllerInfo, playerCommand: Int): Int {
            if (playerCommand == Player.COMMAND_PLAY_PAUSE) librarySession.playbackIntent()
            if (playerCommand in setOf(Player.COMMAND_SET_MEDIA_ITEM, Player.COMMAND_CHANGE_MEDIA_ITEMS,
                    Player.COMMAND_STOP, Player.COMMAND_SEEK_TO_MEDIA_ITEM, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM,
                    Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)) librarySession.userIntent()
            return SessionResult.RESULT_SUCCESS
        }

        override fun onCustomCommand(session: MediaSession, controller: MediaSession.ControllerInfo, customCommand: SessionCommand, args: Bundle): ListenableFuture<SessionResult> {
            if (controller.packageName != packageName) return Futures.immediateFuture(SessionResult(SessionError.ERROR_PERMISSION_DENIED))
            if (customCommand.customAction in LibraryPlayback.commands) return librarySession.command(customCommand.customAction, args)
            return try {
                when (customCommand.customAction) {
                    DspSettings.SET_DSP -> {
                        val next = DspSettings.decode(requireNotNull(args.getString(DspSettings.JSON)))
                        config.set(next)
                        loudness?.refresh()
                        headphones.edited(next)
                        preferences.edit().putString("dsp", DspSettings.encode(next)).apply()
                        settingsError = null
                    }
                    DspSettings.SET_PRIVACY -> {
                        privateMetadata = args.getBoolean("private", privateMetadata)
                        preferences.edit().putBoolean("privateMetadata", privateMetadata).apply()
                        artwork.invalidate()
                        for (i in 0 until player.mediaItemCount) player.replaceMediaItem(i, metadata(player.getMediaItemAt(i)))
                        artwork.refresh()
                    }
                    HeadphoneCommands.GET -> Unit
                    HeadphoneCommands.EDIT -> when (args.getString("operation")) {
                        "save" -> headphones.save(requireNotNull(args.getString("name")), args.getString("id"))
                        "select" -> headphones.select(args.getString("id"))
                        "common" -> headphones.saveCommon()
                        "automatic" -> headphones.automatic(args.getBoolean("enabled"))
                        "bind" -> headphones.bind(requireNotNull(args.getString("id")), requireNotNull(args.getString("route")))
                        "unbind" -> headphones.unbind(requireNotNull(args.getString("route")))
                        "remove" -> headphones.remove(requireNotNull(args.getString("id")))
                        else -> throw IllegalArgumentException("Unknown headphone action")
                    }
                    DspSettings.GET_DSP -> Unit
                    DspSettings.ANALYZE -> loudness?.analyze()
                    DspSettings.CANCEL_ANALYSIS -> loudness?.cancel()
                    else -> return Futures.immediateFuture(SessionResult(SessionError.ERROR_NOT_SUPPORTED))
                }
                if (customCommand.customAction !in setOf(HeadphoneCommands.GET, DspSettings.GET_DSP)) changed("sound")
                Futures.immediateFuture(SessionResult(SessionResult.RESULT_SUCCESS, Bundle().apply {
                    putString(DspSettings.JSON, DspSettings.encode(config.get()))
                    loudness?.state()?.let { putAll(it) }
                    putString("headphones", HeadphoneCommands.encode(headphones.state))
                    putBoolean("private", privateMetadata)
                    putString("warning", settingsError)
                    putLong("processedFrames", processor.processedFrames)
                    putInt("sampleRate", processor.sampleRate)
                    putInt("channels", processor.channels)
                    putStringArrayList("titleIds", ArrayList(titles.keys))
                    putStringArrayList("titles", ArrayList(titles.values))
                }))
            } catch (_: Exception) { Futures.immediateFuture(SessionResult(SessionError.ERROR_BAD_VALUE)) }
        }
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        if (!isPlaybackOngoing) { player.stop(); stopSelf() }
    }

    override fun onDestroy() {
        loudness?.close()
        librarySession.close()
        artwork.close()
        output.close()
        session?.release()
        session = null
        player.release()
        com.asmrplayer.android.widget.PlaybackWidget.publish(this, null)
        super.onDestroy()
    }

    companion object { const val MEDIA_CHANNEL = "asmr_playback" }
}
