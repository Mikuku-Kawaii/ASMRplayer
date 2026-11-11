package com.asmrplayer.android.ui

import android.graphics.BitmapFactory
import android.net.Uri
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.animation.core.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToLong
import androidx.media3.common.util.UnstableApi
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.asmrplayer.android.PlayerScreenState
import com.asmrplayer.android.PlayerViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CancellationException

internal fun displayTitle(state:PlayerScreenState):String {
    val item=state.queue.getOrNull(state.index)?:return "还没有正在播放的音频"
    return if(hiddenCurrent(state))"内容已隐藏" else item.title
}
internal fun time(ms:Long):String="%d:%02d".format(ms/60000,ms/1000%60)
private fun playbackLabel(state:PlayerScreenState)=when {
    state.buffering->"正在缓冲…";state.ended->"已播放完";state.playRequested->"正在播放";else->"已暂停"
}
internal fun currentCover(state:PlayerScreenState):String? = state.currentWork?.takeIf { visibleRating(it.rating, state) }?.coverUri

@Composable
internal fun EmptyContent(title:String,detail:String,glyph:Glyph) {
    Surface(Modifier.fillMaxWidth(), shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainerLow) {
        Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
            AppGlyph(glyph, Modifier.size(24.dp), MaterialTheme.colorScheme.primary)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium)
                Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
internal fun AudioArtwork(modifier:Modifier=Modifier,uri:String?=null,contentScale:ContentScale=ContentScale.Crop,highResolution:Boolean=false,workId:String?=null, shape: androidx.compose.ui.graphics.Shape = RoundedCornerShape(18.dp)) {
    val colors=MaterialTheme.colorScheme
    val backdrop = remember(colors) { Brush.linearGradient(listOf(colors.primaryContainer,colors.surfaceContainer,colors.primary.copy(alpha=.18f))) }
    CardWidth(modifier.clip(shape).background(backdrop)) { maxWidth, maxHeight ->
        Box(Modifier.fillMaxSize(), contentAlignment=Alignment.Center) {
            val pixels = with(androidx.compose.ui.platform.LocalDensity.current) { maxWidth.toPx() }
            val loaded = rememberArtwork(uri, highResolution, workId, thumbnailEdge = if (pixels <= 384) 384 else 768)
            val artwork = loaded.image
            val loading = loaded.loading
            val showLabel = maxWidth >= 100.dp && maxHeight >= 80.dp
            artwork?.let{Image(it,null,Modifier.fillMaxSize(),contentScale=contentScale)}
                ?:Column(horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(8.dp)) {
                    if (loading) CircularProgressIndicator(Modifier.size(minOf(24.dp, maxWidth, maxHeight)),strokeWidth=2.dp)
                    else AppGlyph(Glyph.Audio,Modifier.size(28.dp),colors.primary.copy(alpha=.8f))
                    if (showLabel && !loading) Text(if(uri == null) "暂无封面" else "封面加载失败",style=MaterialTheme.typography.labelSmall,color=colors.onSurfaceVariant)
                }
        }
    }
}

@Composable
internal fun AudioCard(item:LibraryAudio,open:()->Unit,favorite:()->Unit, badges:List<String> = emptyList()) {
    WorkCard(item.title,item.coverUri,item.code,badges,
        "${item.versions.map { it.trackId }.distinct().size} 首音轨 · ${item.versions.map { it.format.uppercase() }.distinct().joinToString(" / ")}",
        open=open,favorite=item.favorite,like=favorite,workId=item.id,nsfw=item.nsfw)
}

private val miniBarInset = 8.dp
private val miniCoverSize = 44.dp
private val miniTitleGap = 10.dp

@UnstableApi
@Composable
internal fun MiniPlayer(source: GlassBackdrop, state: PlayerScreenState, model: PlayerViewModel,
    open: () -> Unit, modifier: Modifier = Modifier) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    MiniPlayerDock(source, state, position, open, model::togglePlayback, model::next, model::seek, modifier)
}

@Composable
internal fun MiniPlayerDock(source: GlassBackdrop, state: PlayerScreenState, position: Long,
    open: () -> Unit, playPause: () -> Unit, next: () -> Unit, seek: (Long) -> Unit,
    modifier: Modifier = Modifier) {
    val duration = state.duration
    val target = if (duration > 0) (position.toDouble() / duration).coerceIn(0.0, 1.0).toFloat() else 0f
    val canSeek = state.connected && duration > 0 && (state.seekable || state.canPrepareSeek)
    val motion = dockMotionEnabled(state.lowEffects)
    key(state.queue.getOrNull(state.index)?.id) {
        val progress = animateFloatAsState(target,
            if (motion && state.playing && !state.buffering) tween(500, easing = LinearEasing) else snap(), label = "mini player progress")
        var preview by remember(duration, canSeek) { mutableStateOf<Float?>(null) }
        val commit by rememberUpdatedState(seek)
        fun positionAt(fraction: Float) = (fraction.coerceIn(0f, 1f).toDouble() * duration).roundToLong()

        val openInteraction = remember { MutableInteractionSource() }
        val colors = MaterialTheme.colorScheme
        val fill = colors.primary.copy(alpha = if (colors.background.luminance() < .5f) .14f else .10f)
        GlassSurface(source, state.lowEffects, modifier.fillMaxWidth().testTag("mini-player")
            .pointerInput(duration, canSeek) {
                if (!canSeek) return@pointerInput
                fun fractionAt(x: Float) = (x / size.width.coerceAtLeast(1)).coerceIn(0f, 1f)
                detectHorizontalDragGestures(
                    onDragStart = { preview = fractionAt(it.x) },
                    onDragCancel = { preview = null },
                    onDragEnd = {
                        val fraction = preview
                        preview = null
                        fraction?.let { commit(positionAt(it)) }
                    },
                    onHorizontalDrag = { change, _ -> preview = fractionAt(change.position.x) },
                )
            }
            .semantics {
                if (duration > 0) progressBarRangeInfo = ProgressBarRangeInfo(preview ?: target, 0f..1f)
                if (canSeek) setProgress { fraction ->
                    if (!fraction.isFinite()) false
                    else { commit(positionAt(fraction)); true }
                }
            }, baseTint = colors.surfaceContainerHigh) {
            Canvas(Modifier.matchParentSize()) {
                drawRect(fill, size = Size(size.width * (preview ?: progress.value), size.height))
            }
            Row(
                Modifier.align(Alignment.Center).fillMaxWidth().heightIn(min = 64.dp)
                    .padding(horizontal = miniBarInset, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(
                    Modifier.weight(1f).clip(RoundedCornerShape(24.dp))
                        .clickable(interactionSource = openInteraction, indication = null, onClick = open)
                        .dockPress(openInteraction, state.lowEffects).heightIn(min = 48.dp).padding(end = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(miniTitleGap)
                ) {
                    AudioArtwork(Modifier.size(miniCoverSize), currentCover(state),
                        workId = state.currentWork?.takeIf { visibleRating(it.rating, state) }?.id)
                    Column(Modifier.weight(1f)) {
                        Text(displayTitle(state), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleSmall)
                        Text("${playbackLabel(state)} · ${time(preview?.let(::positionAt) ?: position)}",
                            maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelSmall,
                            color = colors.onSurfaceVariant)
                    }
                }
                DockButton(if (state.playRequested) Glyph.Pause else Glyph.Play, if (state.playRequested) "暂停" else "播放",
                    state.lowEffects, state.connected, playPause)
                DockButton(Glyph.Next, "下一轨", state.lowEffects, state.canNext, next)
            }
        }
    }
}
