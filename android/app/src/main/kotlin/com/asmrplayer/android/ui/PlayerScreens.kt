package com.asmrplayer.android.ui

import android.graphics.BitmapFactory
import android.net.Uri
import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.grid.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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

@Composable
internal fun DockWithCat(
    state: PlayerScreenState,
    modifier: Modifier = Modifier,
    toggle: () -> Unit,
    content: @Composable () -> Unit
) {
    val size = 56.dp
    // Anchor to the title column even when the content filter replaces the track name.
    val titleStart = miniBarInset + miniCoverSize + miniTitleGap
    val titleInset = with(androidx.compose.ui.platform.LocalDensity.current) {
        MaterialTheme.typography.titleSmall.fontSize.toDp() * 2
    }
    val start = if (state.queue.isEmpty()) 12.dp else titleStart + titleInset - size / 2
    Box(modifier) {
        // The tilted cube drops below its side corners. Seat those corners on the bar edge.
        Box(Modifier.fillMaxWidth().padding(top = size - 12.dp)) { content() }
        Box(Modifier.padding(start = start)) {
            NsfwDockButton(state.lowEffects, state.showNsfw, size, toggle)
        }
    }
}

@UnstableApi
@Composable
internal fun MiniPlayer(source:GlassBackdrop,state:PlayerScreenState,model:PlayerViewModel,open:()->Unit,modifier:Modifier=Modifier) {
    val position by model.positionMs.collectAsStateWithLifecycle()
    val openInteraction=remember { MutableInteractionSource() }
    GlassSurface(source,state.lowEffects,modifier.fillMaxWidth(),baseTint=MaterialTheme.colorScheme.surfaceContainerHigh) {
        Row(Modifier.align(Alignment.Center).fillMaxWidth().heightIn(min=64.dp).padding(horizontal=miniBarInset,vertical=6.dp),verticalAlignment=Alignment.CenterVertically) {
            Row(Modifier.weight(1f).clip(RoundedCornerShape(24.dp)).clickable(interactionSource=openInteraction,indication=null,onClick=open)
                .dockPress(openInteraction,state.lowEffects).heightIn(min=48.dp).padding(end=6.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(miniTitleGap)) {
                AudioArtwork(Modifier.size(miniCoverSize),currentCover(state),workId=state.currentWork?.takeIf { visibleRating(it.rating,state) }?.id)
                Column(Modifier.weight(1f)) {
                    Text(displayTitle(state),maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.titleSmall)
                    Text("${playbackLabel(state)} · ${time(position)}",maxLines=1,overflow=TextOverflow.Ellipsis,style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            DockButton(if(state.playRequested)Glyph.Pause else Glyph.Play,if(state.playRequested)"暂停" else "播放",state.lowEffects,state.connected,model::togglePlayback)
            DockButton(Glyph.Next,"下一轨",state.lowEffects,state.canNext,model::next)
        }
    }
}

