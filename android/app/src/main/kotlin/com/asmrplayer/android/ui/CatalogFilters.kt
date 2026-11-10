package com.asmrplayer.android.ui

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import com.asmrplayer.android.PlayerScreenState
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.library.*
import com.asmrplayer.android.online.BrowseFilters
import com.asmrplayer.android.data.CatalogMetadata
import org.json.JSONObject
import org.json.JSONArray

internal fun LibraryQuery.encode()=JSONObject().put("source",source.name).put("listening",listening.name).put("sort",sort.name).put("descending",descending)
    .put("tags",JSONArray(tags)).put("voices",JSONArray(voices)).put("circle",circle).put("subtitle",subtitleOnly).put("offline",offlineOnly).put("rating",rating).put("favorite",favorite).toString()
internal fun decodeLibraryQuery(raw:String):LibraryQuery=runCatching {
    val o=JSONObject(raw)
    fun list(k:String)=o.optJSONArray(k)?.let {a->List(a.length()){a.getString(it)}}.orEmpty()
    LibraryQuery(source=LibrarySourceFilter.valueOf(o.getString("source")),listening=ListeningFilter.valueOf(o.getString("listening")),sort=LibrarySort.valueOf(o.getString("sort")),descending=o.optBoolean("descending"),tags=list("tags"),voices=list("voices"),circle=o.optString("circle"),subtitleOnly=o.optBoolean("subtitle"),offlineOnly=o.optBoolean("offline"),rating=o.optString("rating"),favorite=o.optInt("favorite"))
}.getOrDefault(LibraryQuery())

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun <T> FilterChoices(title:String,value:T,options:List<Pair<T,String>>,change:(T)->Unit) {
    Column {if(title.isNotBlank())Text(title,style=MaterialTheme.typography.titleSmall)
        FlowRow(horizontalArrangement=Arrangement.spacedBy(8.dp)) { options.forEach {(key,label)->ChoiceChip(value==key,{change(key)},{Text(label)})} }
    }
}
internal fun LibraryQuery.clearConditions() = LibraryQuery(favoritesOnly = favoritesOnly, sort = sort, descending = descending)
internal val LibraryQuery.conditionCount get() = tags.size + voices.size + listOf(circle.isNotBlank(), source != LibrarySourceFilter.ALL,
    listening != ListeningFilter.ALL, offlineOnly, subtitleOnly, favorite != 0, rating.isNotBlank()).count { it }

private val listeningChoices = listOf(ListeningFilter.ALL to "全部", ListeningFilter.UNPLAYED to "未听", ListeningFilter.IN_PROGRESS to "未听完", ListeningFilter.COMPLETED to "已听完")
private val sortChoices = listOf(LibrarySort.TITLE to "标题", LibrarySort.CODE to "作品编号", LibrarySort.RECENTLY_PLAYED to "最近收听")

private fun toggleName(names: List<String>, name: String, limit: Int): List<String> {
    val existing = names.firstOrNull { it.equals(name, true) }
    return if (existing != null) names - existing else if (names.size < limit) names + name else names
}

@Composable
internal fun LibraryFilterSheet(value: LibraryQuery, state: PlayerScreenState, keyword: String,
    preview: (LibraryQuery) -> Flow<Int>, close: () -> Unit, apply: (LibraryQuery) -> Unit) {
    var draft by remember { mutableStateOf(value) }
    var section by rememberSaveable { mutableStateOf("") }
    val scoped by produceState(emptyList<CatalogMetadata>(), state.lookup, state.metadata, state.showNsfw, state.includeUnrated, value.favoritesOnly) {
        this.value = withContext(Dispatchers.Default) {
            state.library.filter { visibleRating(it.rating, state) && (if (value.favoritesOnly) it.favorite else it.inLibrary) }
                .mapNotNull { state.metadata[it.id] }
        }
    }
    val request = draft.copy(text = keyword)
    val count = key(request) {
        val flow = remember { preview(request).catch { /* Keep the apply action available if the preview fails. */ } }
        flow.collectAsStateWithLifecycle(initialValue = null).value
    }
    FilterPanel(if (value.favoritesOnly) "喜欢筛选" else "音频库筛选", section, { section = "" }, close,
        { draft = draft.clearConditions() }, { apply(draft); close() }, count?.let { "查看 $it 部作品" } ?: "查看结果") {
        when (section) {
            "标签", "声优", "社团" -> {
                val options by produceState(emptyList<FilterOption>(), scoped, section) {
                    val field = section
                    this.value = withContext(Dispatchers.Default) {
                        scoped.flatMap { row ->
                            when (field) { "标签" -> row.tags(); "声优" -> row.voices(); else -> listOf(row.circle) }
                                .map { it.trim() }.filter { it.isNotEmpty() }.distinctBy { it.lowercase() }
                        }.groupBy { it.lowercase() }.values.map { FilterOption(it.first(), count = if (field == "标签") it.size else null) }
                            .sortedWith(compareByDescending<FilterOption> { it.count ?: 0 }.thenBy { it.label })
                    }
                }
                val selected = when (section) { "标签" -> draft.tags; "声优" -> draft.voices; else -> listOf(draft.circle).filter { it.isNotBlank() } }
                FilterPicker(section, options, selected, { name ->
                    draft = when (section) {
                        "标签" -> draft.copy(tags = toggleName(draft.tags, name, 5))
                        "声优" -> draft.copy(voices = toggleName(draft.voices, name, 16))
                        else -> draft.copy(circle = if (draft.circle == name) "" else name)
                    }
                }, limit = when (section) { "标签" -> 5; "声优" -> 16; else -> 1 }, common = section == "标签")
            }
            "更多条件" -> FilterBody {
                FilterChoices("来源",draft.source,listOf(LibrarySourceFilter.ALL to "全部",LibrarySourceFilter.LOCAL to "本地导入",LibrarySourceFilter.DOWNLOADED to "已下载",LibrarySourceFilter.ONLINE to "在线")){draft=draft.copy(source=it)}
                if (!value.favoritesOnly) FilterChoices("喜欢",draft.favorite,listOf(0 to "不限",1 to "已喜欢",2 to "未喜欢")){draft=draft.copy(favorite=it)}
                FilterChoices("分级",draft.rating,listOf("" to "不限","SAFE" to "全年龄","NSFW" to "R18","UNRATED" to "未分级")){draft=draft.copy(rating=it)}
            }
            "排序方式" -> FilterBody {
                FilterChoices("排序", draft.sort, sortChoices) { draft = draft.copy(sort = it) }
                FilterChoices("方向",draft.descending,listOf(false to "升序",true to "降序")){draft=draft.copy(descending=it)}
            }
            else -> FilterBody {
                FilterChoices("收听状态", draft.listening, listeningChoices) { draft = draft.copy(listening = it) }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    ChoiceChip(draft.offlineOnly, { draft = draft.copy(offlineOnly = !draft.offlineOnly) }, { Text("仅离线可用") })
                    ChoiceChip(draft.subtitleOnly, { draft = draft.copy(subtitleOnly = !draft.subtitleOnly) }, { Text("仅带字幕") })
                }
                HorizontalDivider()
                FilterLink("标签", draft.tags.joinToString("、")) { section = "标签" }
                FilterLink("声优", draft.voices.joinToString("、")) { section = "声优" }
                FilterLink("社团", draft.circle) { section = "社团" }
                val extra = listOf(draft.source != LibrarySourceFilter.ALL, draft.rating.isNotBlank(), draft.favorite != 0).count { it }
                FilterLink("更多条件", if (extra > 0) "已选 $extra 项" else if (value.favoritesOnly) "来源、分级" else "来源、喜欢、分级") { section = "更多条件" }
                HorizontalDivider()
                val sort = sortChoices.first { it.first == draft.sort }.second
                FilterLink("排序方式", sort + if (draft.descending) " ↓" else " ↑") { section = "排序方式" }
            }
        }
    }
}

@Composable
internal fun OnlineFilterSheet(value: BrowseFilters, options: List<String>, counts: Map<String, Int>, notice: String?,
    voices: List<String>, circles: List<String>, order: String, ascending: Boolean, sortOptions: List<Pair<String, String>>,
    close: () -> Unit, apply: (BrowseFilters, String, Boolean) -> Unit) {
    var draft by remember { mutableStateOf(value) }
    var draftOrder by rememberSaveable { mutableStateOf(order) }
    var draftAscending by rememberSaveable { mutableStateOf(ascending) }
    var section by rememberSaveable { mutableStateOf("") }
    var excluding by rememberSaveable { mutableStateOf(false) }
    FilterPanel("发现筛选", section, { section = "" }, close, { draft = BrowseFilters() },
        { apply(draft, draftOrder, draftOrder != "popular" && draftAscending); close() }, "应用筛选") {
        when (section) {
            "标签" -> {
                notice?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                val candidates = remember(options, counts) { options.distinct().map { FilterOption(it, count = counts[it]) } }
                fun remove(name: String) {
                    draft = draft.copy(tags = draft.tags.filterNot { it.equals(name, true) },
                        excludedTags = draft.excludedTags.filterNot { it.equals(name, true) })
                }
                FilterPicker("标签", candidates, draft.tags, { name ->
                    val sameSide = (if (excluding) draft.excludedTags else draft.tags).any { it.equals(name, true) }
                    remove(name)
                    if (!sameSide && draft.tags.size + draft.excludedTags.size < 5) {
                        draft = if (excluding) draft.copy(excludedTags = draft.excludedTags + name) else draft.copy(tags = draft.tags + name)
                    }
                }, limit = 5, providerTerms = true, common = true, excluded = draft.excludedTags, excluding = excluding, setExcluding = { excluding = it }, remove = ::remove)
            }
            "声优" -> FilterPicker("声优", voices.map { FilterOption(it) }, draft.voices, { draft = draft.copy(voices = toggleName(draft.voices, it, 5)) }, limit = 5, providerTerms = true)
            "社团" -> FilterPicker("社团", circles.map { FilterOption(it) }, listOf(draft.circle).filter { it.isNotBlank() }, { draft = draft.copy(circle = if (it == draft.circle) "" else it) }, providerTerms = true)
            "排序方式" -> FilterBody {
                FilterChoices("排序", draftOrder, sortOptions) { draftOrder = it }
                if (draftOrder != "popular") {
                    FilterChoices("方向", draftAscending, listOf(true to "升序", false to "降序")) { draftAscending = it }
                }
            }
            else -> FilterBody {
                FilterLink("标签", (draft.tags.map { "包含 $it" } + draft.excludedTags.map { "排除 $it" }).joinToString("、")) { section = "标签" }
                FilterChoices("分级",draft.rating,listOf("" to "不限","general" to "全年龄","adult" to "R18")){draft=draft.copy(rating=it)}
                FilterChoices("字幕", draft.subtitleOnly, listOf(false to "不限", true to "仅带字幕")) { draft = draft.copy(subtitleOnly = it) }
                HorizontalDivider()
                FilterLink("声优", draft.voices.joinToString("、")) { section = "声优" }
                FilterLink("社团", draft.circle) { section = "社团" }
                HorizontalDivider()
                val label = sortOptions.firstOrNull { it.first == draftOrder }?.second.orEmpty()
                val direction = if (draftOrder == "popular") "" else if (draftAscending) " ↑" else " ↓"
                FilterLink("排序方式", label + direction) { section = "排序方式" }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun LibraryFilterSummary(value:LibraryQuery,change:(LibraryQuery)->Unit) {
    if (value.conditionCount == 0) return
    FilterStrip {
        value.tags.forEach {v->RemovableChip({change(value.copy(tags=value.tags-v))},{Text("$v")})}
        value.voices.forEach {v->RemovableChip({change(value.copy(voices=value.voices-v))},{Text("声优：$v")})}
        if(value.circle.isNotBlank())RemovableChip({change(value.copy(circle=""))},{Text("${value.circle}")})
        if(value.source!=LibrarySourceFilter.ALL)RemovableChip({change(value.copy(source=LibrarySourceFilter.ALL))},{Text(when(value.source){LibrarySourceFilter.LOCAL->"本地导入";LibrarySourceFilter.DOWNLOADED->"已下载";else->"在线"})})
        if(value.listening!=ListeningFilter.ALL)RemovableChip({change(value.copy(listening=ListeningFilter.ALL))},{Text(when(value.listening){ListeningFilter.UNPLAYED->"未听";ListeningFilter.IN_PROGRESS->"未听完";else->"听过"})})
        if(value.offlineOnly)RemovableChip({change(value.copy(offlineOnly=false))},{Text("离线可用")})
        if(value.subtitleOnly)RemovableChip({change(value.copy(subtitleOnly=false))},{Text("带字幕")})
        if(value.favorite!=0)RemovableChip({change(value.copy(favorite=0))},{Text(if(value.favorite==1)"已喜欢" else "未喜欢")})
        if(value.rating.isNotBlank())RemovableChip({change(value.copy(rating=""))},{Text(when(value.rating){"SAFE"->"全年龄";"NSFW"->"R18";else->"未分级"})})
        MotionTextButton({ change(value.clearConditions()) }) { Text("清空条件") }
    }
}
