package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

internal data class FilterOption(val id: String, val label: String = id, val count: Int? = null)

// A modal editor belongs to the home page that launched it, including during IME focus changes.
internal val LocalFilterVisibility = staticCompositionLocalOf<(Boolean) -> Unit> { {} }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun FilterPanel(title: String, section: String, leave: () -> Unit, close: () -> Unit,
    reset: () -> Unit, apply: () -> Unit, applyLabel: String, content: @Composable ColumnScope.() -> Unit) {
    val visibility by rememberUpdatedState(LocalFilterVisibility.current)
    DisposableEffect(Unit) {
        visibility(true)
        onDispose { visibility(false) }
    }
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val motion = LocalUiMotion.current
    fun dismiss() {
        if (!motion) close() else scope.launch { sheet.hide(); if (!sheet.isVisible) close() }
    }
    ModalBottomSheet(onDismissRequest = close, sheetState = sheet) {
        NoticeOverlay(modal = true)
        androidx.activity.compose.BackHandler(section.isNotEmpty()) { leave() }
        Column(Modifier.fillMaxWidth().imePadding().contentSheetHeight().sizeMotion().padding(horizontal = 20.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                if (section.isNotEmpty()) GlyphButton(Glyph.Back, "返回筛选", leave)
                Text(section.ifEmpty { title }, Modifier.weight(1f).semantics { heading() }, style = MaterialTheme.typography.titleLarge)
                GlyphButton(Glyph.Close, "关闭筛选", ::dismiss)
            }
            key(section) { content() }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                if (section.isEmpty()) MotionTextButton(reset) { Text("清空条件") }
                Spacer(Modifier.weight(1f))
                MotionButton(if (section.isEmpty()) apply else leave) { Text(if (section.isEmpty()) applyLabel else "完成选择") }
            }
        }
    }
}

@Composable
internal fun ColumnScope.FilterBody(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp), content = content)
}

@Composable
internal fun FilterSwitch(label: String, checked: Boolean, change: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().toggleable(value = checked, role = Role.Switch, onValueChange = change)
        .heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
internal fun FilterLink(label: String, value: String, action: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = action).heightIn(min = 48.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Text(value.ifBlank { "不限" }, Modifier.weight(1f), textAlign = TextAlign.End,
            maxLines = 1, overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        AppGlyph(Glyph.ChevronRight, Modifier.size(18.dp))
    }
}

@Composable
internal fun FilterStrip(content: @Composable RowScope.() -> Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically, content = content)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ColumnScope.FilterPicker(label: String, options: List<FilterOption>, selected: List<String>,
    choose: (String) -> Unit, limit: Int = 1, manual: Boolean = true, common: Boolean = false, providerTerms: Boolean = false,
    excluded: List<String> = emptyList(), excluding: Boolean? = null, setExcluding: (Boolean) -> Unit = {},
    remove: (String) -> Unit = choose) {
    var search by rememberSaveable { mutableStateOf("") }
    val term = search.trim()
    val filterMode = excluding != null
    val picked = selected + excluded
    fun chosen(id: String) = picked.any { it.equals(id, true) }
    val single = limit == 1
    val allowed = single || picked.size < limit
    val frequent = if (common && term.isBlank()) options.take(8) else emptyList()
    val results = remember(options, term, frequent) {
        options.asSequence().drop(frequent.size).filter { it.label.contains(term, true) }.sortedBy { it.label }.toList()
    }
    val chipOptions = label == "标签" || label == "声优" || label == "社团"
    val batches = remember(results, chipOptions) { if (chipOptions) results.chunked(32) else emptyList() }
    if (filterMode) FilterChoices("", excluding == true, listOf(false to "包含", true to "排除"), setExcluding)
    HomeSearch(search, { search = it }, "搜索$label")
    if (picked.isNotEmpty() || !single) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(if (single) "已选" else "已选 ${picked.size}/$limit", style = MaterialTheme.typography.labelMedium)
            Spacer(Modifier.width(8.dp))
            FilterStrip {
                picked.forEach { id ->
                    val name = options.firstOrNull { it.id == id }?.label ?: id
                    val prefix = if (!filterMode) "" else if (id in excluded) "排除 " else "包含 "
                    RemovableChip({ remove(id) }) { Text(prefix + name, maxLines = 1) }
                }
            }
        }
    }
    LazyColumn(Modifier.fillMaxWidth().weight(1f, fill = false), contentPadding = PaddingValues(vertical = 8.dp)) {
        if (frequent.isNotEmpty()) item {
            Text("常用标签", style = MaterialTheme.typography.titleSmall)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                frequent.forEach { option ->
                    ChoiceChip(chosen(option.id), { choose(option.id) }, {
                        Text(option.label + (option.count?.let { "  $it" } ?: ""))
                    }, enabled = allowed || chosen(option.id))
                }
            }
        }
        if (results.isNotEmpty()) item { Text(if (term.isEmpty()) "全部$label" else "搜索结果", style = MaterialTheme.typography.titleSmall) }
        if (chipOptions) {
            // Keep long provider dictionaries lazy while letting variable-width chips wrap naturally.
            items(batches, key = { it.first().id }) { batch ->
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    batch.forEach { option ->
                        ChoiceChip(chosen(option.id), { choose(option.id) }, {
                            Text(option.label + (option.count?.let { "  $it" } ?: ""), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }, enabled = allowed || chosen(option.id))
                    }
                }
            }
        } else items(results, key = { it.id }) { option ->
            val enabled = allowed || chosen(option.id)
            Row(Modifier.fillMaxWidth().clickable(enabled = enabled) { choose(option.id) }.heightIn(min = 48.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Text(option.label, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = if (enabled) 1f else .38f))
                option.count?.let { Text(it.toString(), style = MaterialTheme.typography.labelSmall) }
                if (chosen(option.id)) AppGlyph(Glyph.Check, Modifier.padding(start = 8.dp).size(16.dp))
            }
        }
        if (manual && term.isNotEmpty() && options.none { it.label.equals(term, true) }) item {
            val names = if (single) listOf(term) else term.split('，', ',', '\n').map { it.trim() }
                .filter { it.isNotEmpty() }.distinctBy { it.lowercase() }
            val valid = names.isNotEmpty() && (!providerTerms || names.all { name -> name.length <= 100 && name.none { it == '$' || it.isISOControl() } })
            MotionTextButton({
                if (names.size == 1) choose(names.first())
                else names.filterNot(::chosen).take((limit - picked.size).coerceAtLeast(0)).forEach(choose)
                search = ""
            }, enabled = valid && (allowed || names.size == 1 && chosen(names.first()))) { Text("使用「$term」") }
        } else if (results.isEmpty() && frequent.isEmpty()) item { Text("没有匹配的$label", Modifier.padding(vertical = 12.dp)) }
    }
}

@Composable
internal fun FilterButton(label: String, count: Int, open: () -> Unit) {
    val description = if (count == 0) label else "$label，已选 $count 项"
    MotionIconButton(open, Modifier.size(48.dp).semantics { contentDescription = description }) {
        BadgedBox(badge = {
            if (count > 0) Badge(
                modifier = Modifier.clearAndSetSemantics { },
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary
            ) { Text(count.toString()) }
        }) {
            AppGlyph(Glyph.Sliders)
        }
    }
}

@Composable
internal fun RecordFilterSheet(available: Boolean, work: String, ascending: Boolean, options: List<FilterOption>,
    close: () -> Unit, apply: (Boolean, String, Boolean) -> Unit) {
    var availableDraft by rememberSaveable { mutableStateOf(available) }
    var workDraft by rememberSaveable { mutableStateOf(work) }
    var ascendingDraft by rememberSaveable { mutableStateOf(ascending) }
    var selecting by rememberSaveable { mutableStateOf(false) }
    FilterPanel("筛选与排序", if (selecting) "所属作品" else "", { selecting = false }, close,
        { availableDraft = false; workDraft = "" }, { apply(availableDraft, workDraft, ascendingDraft); close() }, "查看结果") {
        if (selecting) FilterPicker("作品", options, listOf(workDraft).filter { it.isNotEmpty() }, {
            workDraft = if (workDraft == it) "" else it
            selecting = false
        }, manual = false)
        else FilterBody {
            FilterLink("所属作品", options.firstOrNull { it.id == workDraft }?.label.orEmpty()) { selecting = true }
            FilterSwitch("仅显示可用文件", availableDraft) { availableDraft = it }
            HorizontalDivider()
            FilterChoices("时间顺序", ascendingDraft, listOf(false to "从新到旧", true to "从旧到新")) { ascendingDraft = it }
        }
    }
}
