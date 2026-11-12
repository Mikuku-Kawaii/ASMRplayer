package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalConfiguration
import kotlinx.coroutines.delay
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val cardInfoGap = 4.dp

@Composable
private fun cardTagStyle() = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.SemiBold)

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
    score:Double?=null,nsfw:Boolean=false,hasSubtitle:Boolean=false) {
    val status = badges.filter { it != "正在播放" }
    val hasActionLine = save != null || like != null || status.isNotEmpty()
    val metadataHeight = cardTextHeight(MaterialTheme.typography.bodySmall, padding = 0.dp)
    val tagHeight = cardTextHeight(cardTagStyle(), padding = 4.dp)
    Surface(shape=RoundedCornerShape(16.dp),color=MaterialTheme.colorScheme.surfaceContainerLow) {
        Column {
            Box {
                CardArtwork(cover,Modifier.fillMaxWidth().sharedWorkArtwork(workId).aspectRatio(4f/3f).clickable(onClick=open).semantics{contentDescription="打开 $title"},workId,hasSubtitle,nsfw)
                if("正在播放" in badges)Box(Modifier.align(Alignment.BottomStart).padding(6.dp)){WorkBadge("正在播放",true)}
            }
            Column(
                Modifier.fillMaxWidth().padding(top = cardInfoGap, bottom = if (hasActionLine) 0.dp else cardInfoGap),
                verticalArrangement = Arrangement.spacedBy(cardInfoGap),
            ) {
                Column(Modifier.fillMaxWidth().padding(horizontal = 9.dp), verticalArrangement = Arrangement.spacedBy(cardInfoGap)) {
                    CardTitle(title, Modifier.clickable(onClick = open))
                    Box(Modifier.fillMaxWidth().heightIn(min = metadataHeight).testTag("work-card-metadata"),
                        contentAlignment = Alignment.CenterStart) {
                        if (voices.isNotEmpty()) CardVoices(voices, onVoice)
                        else if (metadata.isNotBlank()) CardMetadata(metadata, onMetadata)
                    }
                    Box(Modifier.fillMaxWidth().heightIn(min = tagHeight).testTag("work-card-tags"),
                        contentAlignment = Alignment.CenterStart) {
                        if (tags.isNotEmpty()) CardFooter(tags, onTag)
                    }
                    if (!hasActionLine) CardCodeScore(code, score)
                }
                if (hasActionLine) WorkCardActionLine(title, inLibrary, save, saving, favorite, like) {
                    Column(verticalArrangement = Arrangement.spacedBy(cardInfoGap)) {
                        Text(code.orEmpty(), Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                            maxLines = 1, softWrap = false, style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                        WorkCardStatus(status)
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
        voices.forEach { voice -> Text(voice,Modifier.alignByBaseline().then(if(choose!=null)Modifier.clickable{choose(voice)}else Modifier),style=MaterialTheme.typography.bodySmall,color=if(accent)MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,maxLines=1) }
    }
}

/** Font-scaled slots keep mixed CJK/Latin titles and missing metadata aligned. */
@Composable
private fun cardTextHeight(style: androidx.compose.ui.text.TextStyle, lines: Int = 1,
    padding: androidx.compose.ui.unit.Dp = 4.dp): androidx.compose.ui.unit.Dp {
    val measurer = cardMeasurer()
    // Include fallback glyphs: system CJK/bold fonts can exceed the nominal line height.
    val measured = remember(style, lines, measurer) {
        measurer.measure(AnnotatedString(List(lines) { "国あア漢Ag♡☆é" }.joinToString("\n")), style = style, maxLines = lines)
    }
    return with(LocalDensity.current) { measured.size.height.toDp() } + padding
}

@Composable
internal fun CardTitle(title: String, modifier: Modifier = Modifier) {
    val style = MaterialTheme.typography.titleSmall.copy(fontSize = 14.sp, lineHeight = 18.sp,
        fontWeight=androidx.compose.ui.text.font.FontWeight.SemiBold,
        platformStyle = PlatformTextStyle(includeFontPadding = false),
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None))
    // Android's CJK fallback may be taller than the requested 18sp line height.
    // Measure a shared two-line sample instead of capping the paragraph at 36sp,
    // which makes Android ellipsize after the first line on some vendor fonts.
    Text(title, modifier.fillMaxWidth().height(cardTextHeight(style, 2, padding = 0.dp)),
        minLines = 2, maxLines = 2, overflow = TextOverflow.Ellipsis, style = style)
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
internal fun CardFooter(tags: List<String>, onTag: ((String) -> Unit)? = null) {
    var expanded by rememberSaveable(tags){mutableStateOf(false)}
    val style = cardTagStyle()
    val colors = MaterialTheme.colorScheme
    val light = colors.surface.luminance() > .5f
    val shape = RoundedCornerShape(6.dp)
    val fill = if (light) lerp(colors.surfaceContainerLow, colors.primary, .14f) else colors.surfaceContainerHighest
    val pill = Modifier.clip(shape).background(fill)
        .then(if (light) Modifier.border(1.dp, colors.outline.copy(alpha = .35f), shape) else Modifier)
    val measurer=cardMeasurer();val density=LocalDensity.current
    Column {
        CardWidth(Modifier.fillMaxWidth(), inset = 18.dp) { maxWidth, _ ->
            val width=with(density){maxWidth.toPx()};val gap=with(density){8.dp.toPx()}
            val pillPadding=with(density){12.dp.toPx()}
            val count = remember(tags, style, measurer, width, pillPadding, gap) {
                var used = 0f
                var fit = 0
                for ((i, tag) in tags.withIndex()) {
                    val tagWidth = measurer.measure(AnnotatedString(tag), style).size.width + pillPadding
                    val reserve = if (i < tags.lastIndex)
                        measurer.measure(AnnotatedString("+${tags.size-i-1}"), style).size.width + gap + pillPadding else 0f
                    if (used + tagWidth + reserve > width) break
                    used += tagWidth + gap
                    fit++
                }
                fit
            }
            run {
                val visibleCount=if(tags.isEmpty())0 else count.coerceAtLeast(1)
                val hidden=tags.size-visibleCount
                val reserved=if(hidden>0)with(density){measurer.measure(AnnotatedString("+$hidden"),style).size.width.toDp()}+20.dp else 0.dp
                val maxTagWidth=(maxWidth-reserved).coerceAtLeast(0.dp)
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp),verticalAlignment=Alignment.CenterVertically) {
                    tags.take(visibleCount).forEach { tag ->
                        Text(tag,Modifier.widthIn(max=maxTagWidth)
                            .then(pill)
                            .then(if(onTag!=null)Modifier.clickable{onTag(tag)}else Modifier).padding(horizontal=6.dp,vertical=2.dp),
                            maxLines=1,overflow=TextOverflow.Ellipsis,style=style,color=colors.onSurface)
                    }
                    if(hidden>0)Text("+$hidden",pill
                        .clickable { expanded = true }.padding(horizontal=6.dp,vertical=2.dp).semantics{contentDescription="展开其余 $hidden 个标签"},
                        maxLines=1,style=style,color=colors.onSurface)
                }
            }
        }
    }
    if (expanded) PlayerSheet("作品标签", { expanded = false }) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            tags.forEach { tag ->
                SuggestionChip(onClick = { expanded = false; onTag?.invoke(tag) }, label = { Text(tag) }, enabled = onTag != null)
            }
        }
    }
}

@Composable
internal fun CardCodeScore(code:String?,score:Double?) {
    val style = MaterialTheme.typography.labelSmall
    val measurer = cardMeasurer()
    val density = LocalDensity.current
    val widths = remember(style, measurer) {
        measurer.measure(AnnotatedString("RJ00000000"), style).size.width to
            measurer.measure(AnnotatedString("5.00"), style).size.width
    }
    CardWidth(Modifier.fillMaxWidth(), inset = 18.dp) { maxWidth, _ ->
      val codeWidth = with(density) { widths.first.toDp() } + 2.dp
      val ratingWidth = if (score == null) 0.dp else with(density) { widths.second.toDp() } + 22.dp
      Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(4.dp)) {
        Text(code.orEmpty(), Modifier.width(minOf(codeWidth, (maxWidth - ratingWidth).coerceAtLeast(0.dp))).alignByBaseline(), maxLines = 1, overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if(score!=null) {
            AppGlyph(Glyph.Star,Modifier.size(12.dp),filled=true,
                tint=if(MaterialTheme.colorScheme.surface.luminance()>.5f)Color(0xFFB98200) else Color(0xFFFFD166))
            Text(String.format(java.util.Locale.ROOT,"%.2f",score),maxLines=1,style=MaterialTheme.typography.labelSmall,
                color=MaterialTheme.colorScheme.onSurfaceVariant,modifier=Modifier.alignByBaseline().semantics{contentDescription="评分 $score，满分 5 分"})
        }
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
    val viewportHeight = LocalConfiguration.current.screenHeightDp.dp
    LaunchedEffect(copied) { if(copied) { delay(1200); copied=false } }
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val colors=MaterialTheme.colorScheme
        val typography=MaterialTheme.typography
        val informationHeight=cardTextHeight(typography.titleMedium,3) +
            cardTextHeight(typography.bodySmall) + cardTextHeight(typography.labelSmall)
        val coverWidth = minOf(maxWidth * .46f, informationHeight * (4f / 3f), viewportHeight / 3)
        val stacked = maxWidth < 300.dp || LocalDensity.current.fontScale >= 1.5f
        val artwork: @Composable (Modifier) -> Unit = { modifier ->
            AudioArtwork(modifier.sharedWorkArtwork(workId, hero = true).aspectRatio(4f/3f)
                .clickable { showCover=true }.semantics { contentDescription="查看作品封面" },
                cover,ContentScale.Fit,highResolution=true,workId=workId)
        }
        val info: @Composable (Modifier) -> Unit = { modifier ->
            Column(modifier,verticalArrangement=Arrangement.spacedBy(3.dp)) {
                Text(title,Modifier.then(if(overflow) Modifier.clickable(onClickLabel="查看完整标题"){fullTitle=true} else Modifier),
                    maxLines=3,overflow=TextOverflow.Ellipsis,style=typography.titleMedium,
                    onTextLayout={overflow=it.hasVisualOverflow})
                if(summary.isNotBlank())Text(summary,style=typography.bodySmall,color=colors.onSurfaceVariant,maxLines=1,overflow=TextOverflow.Ellipsis)
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
                val compactCoverWidth = minOf(116.dp, viewportHeight * .18f) * (4f / 3f)
                artwork(Modifier.widthIn(max = compactCoverWidth).fillMaxWidth())
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
                if(LocalUiMotion.current) androidx.compose.animation.core.tween(240, easing = MotionEase) else androidx.compose.animation.core.snap(),label="disclosure arrow")
            AppGlyph(Glyph.ChevronDown,Modifier.padding(start=4.dp).size(18.dp).rotate(angle),MaterialTheme.colorScheme.primary)
        }
        MotionReveal(expanded) { Column(Modifier.padding(bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = content) }
    }
}

@Composable
internal fun SettingsGroup(title: String, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(title, Modifier.padding(start = 2.dp, top = 6.dp, bottom = 2.dp), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Surface(shape = RoundedCornerShape(14.dp), color = MaterialTheme.colorScheme.surfaceContainerLow,
            border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = .55f))) {
            Column(Modifier.fillMaxWidth(), content = content)
        }
    }
}

@Composable
internal fun SettingsLink(title: String, summary: String, action: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = action).heightIn(min = 56.dp).padding(horizontal = 14.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            if(summary.isNotBlank()) Text(summary, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        AppGlyph(Glyph.ChevronRight, Modifier.size(18.dp), MaterialTheme.colorScheme.onSurfaceVariant)
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
    var expanded by rememberSaveable { mutableStateOf(false) }
    val current = selected?.takeIf { it in groups } ?: groups.first()
    fun name(group: Pair<String, String>) = group.first.ifBlank { "根目录" }
    if (groups.size == 1) {
        val format = current.second.uppercase()
        val directory = name(current)
        val summary = if (format.isBlank() || directory.equals(format, ignoreCase = true)) directory else "$directory · $format"
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AppGlyph(Glyph.Folder, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Text("目录 · $summary", maxLines = 1, overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        return
    }
    fun label(group: Pair<String, String>) = if (groups.count { it.first == group.first } > 1)
        "${name(group)} · ${group.second.uppercase()}" else name(group)
    MotionOutlinedButton({ expanded = true }, Modifier.fillMaxWidth().heightIn(min = 48.dp)
        .semantics { contentDescription = "目录 · ${label(current)}" }) {
        AppGlyph(Glyph.Folder, Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text("目录 · ${label(current)}", Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.width(8.dp))
        AppGlyph(Glyph.ChevronDown, Modifier.size(18.dp))
    }
    if (expanded) {
        val scroll = androidx.compose.foundation.lazy.rememberLazyListState(initialFirstVisibleItemIndex = groups.indexOf(current))
        PlayerAlertDialog(onDismissRequest = { expanded = false }, title = { Text("选择音轨目录") },
            confirmButton = { MotionTextButton({ expanded = false }) { Text("关闭") } }, text = {
                LazyColumn(Modifier.fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp * .5f).dp),
                    state = scroll, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    items(groups) { entry ->
                        val active = entry == current
                        Row(Modifier.fillMaxWidth()
                            .background(if (active) MaterialTheme.colorScheme.primaryContainer else Color.Transparent, RoundedCornerShape(12.dp))
                            .selectable(selected = active, role = Role.RadioButton, onClick = { expanded = false; choose(entry) })
                            .padding(horizontal = 12.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Text(name(entry), style = MaterialTheme.typography.bodyLarge,
                                    color = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface)
                                Text(entry.second.uppercase(), style = MaterialTheme.typography.labelMedium,
                                    color = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            Spacer(Modifier.width(12.dp))
                            if (active) AppGlyph(Glyph.Check, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onPrimaryContainer)
                            else Spacer(Modifier.size(20.dp))
                        }
                    }
                }
            })
    }
}
