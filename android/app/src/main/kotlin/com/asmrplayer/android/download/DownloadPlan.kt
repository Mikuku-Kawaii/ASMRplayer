package com.asmrplayer.android.download

import com.asmrplayer.android.data.*
import com.asmrplayer.android.online.*
import kotlinx.coroutines.flow.first

data class DownloadPlanItem(val id: String, val path: String, val format: String, val bytes: Long?,
    val attachment: Boolean, val local: Boolean = false, val taskState: String? = null) {
    val downloaded get() = local || taskState == "COMPLETED"
    val queued get() = taskState != null && taskState !in setOf("COMPLETED", "FAILED", "REMOVED")
    val selectable get() = !downloaded && !queued
}

data class DownloadPlan(val workId: String, val title: String, val account: ProviderAccountState,
    val items: List<DownloadPlanItem>) {
    fun withTasks(tasks: List<DownloadTask>): DownloadPlan {
        val states = tasks.associate { (it.assetId ?: it.versionId) to it.state }
        return copy(items = items.map { it.copy(taskState = states[it.id]) })
    }
}

/** Preview imports provider identities only. User membership and queue change on confirmation. */
class DownloadPlanRepository(private val db: LibraryDatabase, private val online: OnlineRepository,
    private val downloads: DownloadRepository) {
    suspend fun prepare(workId: String? = null, origin: String? = null, sourceId: String? = null): DownloadPlan {
        val account = online.account.state.value
        val meta = workId?.let { db.metadata().forWork(it) }
        val reference = workId?.let { id -> db.library().observeVersions(id).first().firstNotNullOfOrNull { db.library().remote(it.id) } }
        val base = origin ?: meta?.origin?.takeIf { it.isNotBlank() } ?: reference?.origin
        val source = sourceId ?: meta?.sourceId?.takeIf { it.isNotBlank() } ?: reference?.sourceWorkId
        require(base == account.apiBase) { "请先切换到此作品的在线来源" }
        require(!source.isNullOrBlank()) { "作品没有可下载的在线来源" }
        val imported = online.importWork(source, addToLibrary = false, reuseDetail = true)
        check(online.account.state.value == account) { "来源已切换，请重新打开下载选择" }
        downloads.awaitReady()
        val dao = db.library()
        val assets = dao.observeAssets(imported.workId).first().filter { it.available }.map { it.id }.toSet()
        val items = imported.versionIds.map { id ->
            val ref = requireNotNull(dao.remote(id))
            DownloadPlanItem(id, ref.relativePath, ref.format, ref.byteSize, false,
                dao.sources(id).any { it.available && it.kind in setOf("local_imported", "local_downloaded") })
        } + dao.observeRemoteAssets(imported.workId).first().filter { it.available }.map { ref ->
            DownloadPlanItem(ref.id, if (ref.kind == "cover") "作品封面" else ref.relativePath,
                if (ref.kind == "cover") "封面" else ref.format, ref.byteSize, true, ref.id in assets)
        }
        check(online.account.state.value == account) { "来源已切换，请重新打开下载选择" }
        return DownloadPlan(imported.workId, requireNotNull(dao.work(imported.workId)).title, account, items)
            .withTasks(dao.downloadTasks())
    }

    suspend fun enqueue(plan: DownloadPlan, selected: Set<String>): Int {
        check(online.account.state.value == plan.account) { "来源已切换，请重新打开下载选择" }
        val eligible = plan.withTasks(db.library().downloadTasks()).items.filter { it.id in selected && it.selectable }
        if (eligible.isEmpty()) return 0
        val ids = downloads.enqueueSelection(eligible.filterNot { it.attachment }.map { it.id },
            eligible.filter { it.attachment }.map { it.id }, skipExisting = true)
        if (ids.isNotEmpty()) db.library().membership(plan.workId, true)
        return ids.size
    }
}

/** Full relative paths are keys, so folders with the same name remain independent. */
internal data class DownloadFolder(val path: String, val files: List<DownloadPlanItem>, val children: List<DownloadFolder>) {
    val allFiles: List<DownloadPlanItem> = files + children.flatMap { it.allFiles }
}

internal fun downloadFolders(items: List<DownloadPlanItem>, prefix: String = ""): List<DownloadFolder> {
    val groups = items.groupBy { it.path.removePrefix(prefix).substringBefore('/', "") }
    return groups.map { (name, files) ->
        val path = prefix + name
        if (name.isEmpty()) DownloadFolder(prefix, files, emptyList())
        else {
            val nested = downloadFolders(files, "$path/")
            DownloadFolder(path, nested.filter { it.path == "$path/" }.flatMap { it.files }, nested.filterNot { it.path == "$path/" })
        }
    }
}
