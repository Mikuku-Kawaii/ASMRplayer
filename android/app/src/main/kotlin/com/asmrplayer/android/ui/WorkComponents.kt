package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.delay
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

@Composable
internal fun WorkBadge(text: String, accent: Boolean = false) {
    Surface(shape = RoundedCornerShape(6.dp), color = if (accent) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer,
        contentColor = if (accent) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant) {
        Text(text, Modifier.padding(horizontal = 7.dp, vertical = 3.dp), style = MaterialTheme.typography.labelSmall)
    }
}

/** Shared cover-first card; metadata and actions never obscure artwork. */
@Composable
internal fun WorkCard(title: String, cover: String?, code: String?, badges: List<String>, metadata: String,
    tags: List<String> = emptyList(), open: () -> Unit, favorite: Boolean? = null, like: (() -> Unit)? = null,
    onTag:((String)->Unit)?=null, onMetadata:(()->Unit)?=null, voices:List<String> = emptyList(),
    onVoice:((String)->Unit)?=null, inLibrary:Boolean?=null, save:(()->Unit)?=null,saving:Boolean=false,workId:String?=null,
    discovery:Boolean=false,score:Double?=null,nsfw:Boolean=false,hasSubtitle:Boolean=false) {
    Surface(shape=RoundedCornerShape(16.dp),color=MaterialTheme.colorScheme.surfaceContainerLow,
        border=if(discovery)BorderStroke(1.dp,MaterialTheme.colorScheme.outlineVariant.copy(alpha=.45f)) else null) {
        Column {
            Box {
                CardArtwork(cover,Modifier.fillMaxWidth().aspectRatio(4f/3f).clickable(onClick=open).semantics{contentDescription="打开 $title"},workId,hasSubtitle,nsfw)
                if("正在播放" in badges)Box(Modifier.align(Alignment.BottomStart).padding(6.dp)){WorkBadge("正在播放",true)}
            }
            Column(Modifier.fillMaxWidth().padding(top = 6.dp, bottom = 3.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Column(Modifier.fillMaxWidth().padding(horizontal = 9.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    CardTitle(title, Modifier.clickable(onClick = open))
                    if (voices.isNotEmpty()) CardVoices(voices, onVoice, accent = discovery)
                    else if (metadata.isNotBlank()) CardMetadata(metadata, onMetadata)
                    if (tags.isNotEmpty()) CardFooter("", tags, onTag, staticPills = discovery)
                }
                if (discovery) {
                    Box(Modifier.padding(horizontal = 9.dp)) { CardCodeScore(code, score) }
                } else {
                    WorkCardActionLine(title, inLibrary, save, saving, favorite, like) {
                        Column {
                            if (!code.isNullOrBlank()) Text(code, Modifier.horizontalScroll(rememberScrollState()),
                                maxLines = 1, softWrap = false, style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant)
                            WorkCardStatus(badges.filter { it != "正在播放" })
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun CardVoices(voices:List<String>, choose:((String)->Unit)?,accent:Boolean=false) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        Text("CV",Modifier.alignByBaseline(),style=MaterialTheme.typography.labelSmall,color=MaterialTheme.colorScheme.onSurfaceVariant)
        voices.forEach { voice -> Text(voice,Modifier.alignByBaseline().then(if(choose!=null)Modifier.clickable{choose(voice)}else Modifier).padding(vertical=1.dp),style=MaterialTheme.typography.bodySmall,color=if(accent)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,maxLines=1) }
    }
}

/** Font-scaled slots keep mixed CJK/Latin titles and missing metadata aligned. */
@Composable
private fun cardTextHeight(style: androidx.compose.ui.text.TextStyle, lines: Int = 1): androidx.compose.ui.unit.Dp {
    val measurer = cardMeasurer()
    // Include fallback glyphs: system CJK/bold fonts can exceed the nominal line height.
    val measured = remember(style, lines, measurer) {
        measurer.measure(AnnotatedString(List(lines) { "国あAg" }.joinToString("\n")), style = style, maxLines = lines)
    }
    return with(LocalDensity.current) { measured.size.height.toDp() } + 4.dp
}

@Composable
internal fun CardTitle(title: String, modifier: Modifier = Modifier) {
    val style = MaterialTheme.typography.titleSmall.copy(fontSize = 14.sp, lineHeight = 18.sp, fontWeight=androidx.compose.ui.text.font.FontWeight.Medium)
    Text(title, modifier,
        maxLines = 2, overflow = TextOverflow.Ellipsis, style = style)
}

@Composable
internal fun CardMetadata(metadata: String, onClick: (() -> Unit)? = null) {
    Text(metadata, Modifier.fillMaxWidth()
        .horizontalScroll(rememberScrollState())
        .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier),
        maxLines = 1, softWrap = false, style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun CardFooter(summary: String, tags: List<String>, onTag: ((String) -> Unit)? = null,staticPills:Boolean=false) {
    var expanded by rememberSaveable(tags){mutableStateOf(false)}
    val style=MaterialTheme.typography.labelSmall
    val measurer=cardMeasurer();val density=LocalDensity.current
    Column {
        if(summary.isNotBlank())Text(summary,style=style,color=MaterialTheme.colorScheme.onSurfaceVariant,maxLines=1,overflow=TextOverflow.Ellipsis)
        CardWidth(Modifier.fillMaxWidth(), inset = 18.dp) { maxWidth, _ ->
            val width=with(density){maxWidth.toPx()};val gap=with(density){8.dp.toPx()}
            val pillPadding=with(density){if(staticPills)12.dp.toPx() else 0f}
            val count = remember(tags, style, measurer, width, staticPills) {
                var used = 0f
                var fit = 0
                for ((i, tag) in tags.withIndex()) {
                    val tagWidth = measurer.measure(AnnotatedString(tag), style).size.width + pillPadding
                    val reserve = if (i < tags.lastIndex)
                        measurer.measure(AnnotatedString("＋${tags.size-i-1}"), style).size.width + gap + pillPadding else 0f
                    if (used + tagWidth + reserve > width) break
                    used += tagWidth + gap
                    fit++
                }
                fit
            }
            if(staticPills) {
                val visibleCount=if(tags.isEmpty())0 else count.coerceAtLeast(1)
                val hidden=tags.size-visibleCount
                val reserved=if(hidden>0)with(density){measurer.measure(AnnotatedString("+$hidden"),style).size.width.toDp()}+20.dp else 0.dp
                val maxTagWidth=(maxWidth-reserved).coerceAtLeast(0.dp)
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp),verticalAlignment=Alignment.CenterVertically) {
                    tags.take(visibleCount).forEach { tag ->
                        Text(tag,Modifier.widthIn(max=maxTagWidth)
                            .background(MaterialTheme.colorScheme.surfaceContainerHighest,RoundedCornerShape(6.dp))
                            .then(if(onTag!=null)Modifier.clickable{onTag(tag)}else Modifier).padding(horizontal=6.dp,vertical=2.dp),
                            maxLines=1,overflow=TextOverflow.Ellipsis,style=style,color=MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if(hidden>0)Text("+$hidden",Modifier.background(MaterialTheme.colorScheme.surfaceContainerHighest,RoundedCornerShape(6.dp))
                        .padding(horizontal=6.dp,vertical=2.dp).semantics{contentDescription="另有 $hidden 个标签"},
                        maxLines=1,style=style,color=MaterialTheme.colorScheme.onSurfaceVariant)
                }
            } else FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp),verticalArrangement=Arrangement.spacedBy(4.dp)) {
                (if(expanded)tags else tags.take(count)).forEach{tag->Text(tag,Modifier.then(if(onTag!=null)Modifier.clickable{onTag(tag)}else Modifier).padding(vertical=2.dp),style=style,color=MaterialTheme.colorScheme.primary)}
                if(tags.size>count)Text(if(expanded)"收起" else "＋${tags.size-count}",Modifier.clickable{expanded=!expanded}.padding(vertical=2.dp).semantics{contentDescription=if(expanded)"收起标签" else "展开其余 ${tags.size-count} 个标签"},style=style,color=MaterialTheme.colorScheme.primary)
            }
        }
    }
}

@Composable
internal fun CardCodeScore(code:String?,score:Double?) {
    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(4.dp)) {
        if (!code.isNullOrBlank()) Text(code, Modifier.weight(1f, fill = false).alignByBaseline(), maxLines = 1, overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if(score!=null) {
            AppGlyph(Glyph.Star,Modifier.size(12.dp),filled=true,
                tint=if(MaterialTheme.colorScheme.surface.luminance()>.5f)Color(0xFFB98200) else Color(0xFFFFD166))
            Text(String.format(java.util.Locale.ROOT,"%.2f",score),maxLines=1,style=MaterialTheme.typography.labelSmall,
                color=MaterialTheme.colorScheme.onSurfaceVariant,modifier=Modifier.alignByBaseline().semantics{contentDescription="评分 $score，满分 5 分"})
        }
    }
}

@Composable
internal fun WorkHero(title: String, cover: String?, code: String?, summary: String, workId:String?=null) {
    var fullTitle by rememberSaveable(title) { mutableStateOf(false) }
    var showCover by remember { mutableStateOf(false) }
    var overflow by remember(title) { mutableStateOf(false) }
    var copied by remember(code) { mutableStateOf(false) }
    val context = LocalContext.current
    LaunchedEffect(copied) { if(copied) { delay(1200); copied=false } }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val colors=MaterialTheme.colorScheme
        val typography=MaterialTheme.typography
        val informationHeight=cardTextHeight(typography.titleMedium,3) +
            cardTextHeight(typography.bodySmall) + cardTextHeight(typography.labelSmall)
        val coverWidth=minOf(maxWidth*.46f, informationHeight * (4f/3f))
        val stacked = maxWidth < 300.dp || androidx.compose.ui.platform.LocalDensity.current.fontScale >= 1.5f
        val artwork: @Composable (Modifier) -> Unit = { modifier ->
            AudioArtwork(modifier.aspectRatio(4f/3f)
                .clickable { showCover=true }.semantics { contentDescription="查看作品封面" },
                cover,ContentScale.Fit,highResolution=true,workId=workId)
        }
        val info: @Composable (Modifier) -> Unit = { modifier ->
            Column(modifier,verticalArrangement=Arrangement.spacedBy(3.dp)) {
                Text(title,Modifier.then(if(overflow) Modifier.clickable(onClickLabel="查看完整标题"){fullTitle=true} else Modifier),
                    maxLines=3,overflow=TextOverflow.Ellipsis,style=typography.titleMedium,
                    onTextLayout={overflow=it.hasVisualOverflow})
                if(summary.isNotBlank())Text(summary,style=typography.bodySmall,maxLines=1,overflow=TextOverflow.Ellipsis)
                if(!code.isNullOrBlank()) {
                    Row(Modifier.clickable(onClickLabel="复制 RJ 号") {
                        val clipboard=context.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                        clipboard.setPrimaryClip(android.content.ClipData.newPlainText("RJ",code))
                        copied=true
                    }.padding(vertical=3.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(5.dp)) {
                        Text(code,style=typography.labelSmall,color=colors.primary)
                        AppGlyph(if(copied)Glyph.Check else Glyph.Copy,Modifier.size(15.dp),tint=colors.primary)
                    }
                }
            }
        }
        if (stacked) {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                artwork(Modifier.widthIn(max = 300.dp).fillMaxWidth())
                info(Modifier.fillMaxWidth())
            }
        } else Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
            artwork(Modifier.width(coverWidth))
            info(Modifier.weight(1f))
        }
    }
    if(fullTitle)PlayerAlertDialog(onDismissRequest={fullTitle=false},text={
        SelectionContainer { Text(title,Modifier.heightIn(max=320.dp).verticalScroll(rememberScrollState()),style=MaterialTheme.typography.titleMedium) }
    },confirmButton={TextButton(onClick={fullTitle=false}){Text("关闭")}})
    if (showCover) FullscreenArtwork(cover,workId=workId) { showCover = false }

}

@Composable
internal fun DetailDisclosure(title: String, summary: String? = null, content: @Composable ColumnScope.() -> Unit) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable { expanded = !expanded }
            .semantics { stateDescription = if (expanded) "已展开" else "已收起" }, verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleSmall)
                summary?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
            Text(if (expanded) "收起" else "展开", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            val angle by androidx.compose.animation.core.animateFloatAsState(if(expanded) 0f else -90f,
                if(LocalUiMotion.current) androidx.compose.animation.core.tween(160) else androidx.compose.animation.core.snap(),label="disclosure arrow")
            AppGlyph(Glyph.ChevronDown,Modifier.padding(start=4.dp).size(18.dp).rotate(angle),MaterialTheme.colorScheme.primary)
        }
        MotionReveal(expanded) { Column(Modifier.padding(bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = content) }
    }
}

@Composable
internal fun SettingsGroup(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, Modifier.padding(start = 4.dp, top = 8.dp, bottom = 4.dp), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Surface(shape = RoundedCornerShape(20.dp), color = MaterialTheme.colorScheme.surfaceContainerLow) {
            Column(Modifier.fillMaxWidth(), content = content)
        }
    }
}

@Composable
internal fun SettingsLink(title: String, summary: String, action: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = action).heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            if(summary.isNotBlank()) Text(summary, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        AppGlyph(Glyph.Back, Modifier.rotate(180f), MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
internal fun MoreActions(label: String, glyph: Glyph = Glyph.More, content: @Composable ColumnScope.(close: () -> Unit) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        GlyphButton(glyph, label, { expanded = true })
        DropdownMenu(expanded, { expanded = false }) { content { expanded = false } }
    }
}

@Composable
internal fun GroupPicker(groups: List<Pair<String, String>>, selected: Pair<String, String>?, choose: (Pair<String, String>) -> Unit) {
    if (groups.isEmpty()) return
    fun label(group: Pair<String, String>) = if (group.first.equals(group.second, true)) "${group.second.uppercase()} 格式"
        else "${group.first.ifBlank { "根目录" }} · ${group.second.uppercase()}"
    PageTabs(selected, groups.map { it to label(it) }) { choose(it) }
}
