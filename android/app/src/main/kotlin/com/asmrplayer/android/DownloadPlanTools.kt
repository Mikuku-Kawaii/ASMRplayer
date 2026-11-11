package com.asmrplayer.android

import android.content.Context
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.download.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class DownloadPlanState(val visible: Boolean = false, val loading: Boolean = false, val busy: Boolean = false,
    val plan: DownloadPlan? = null, val selected: Set<String> = emptySet(), val error: String? = null)

class DownloadPlanTools(context: Context, private val scope: CoroutineScope, private val feedback: (String) -> Unit) {
    private val backend = AndroidBackend.get(context)
    private val repository = DownloadPlanRepository(backend.database, backend.online, backend.downloads)
    private val mutable = MutableStateFlow(DownloadPlanState())
    val state = mutable.asStateFlow()
    private var job: Job? = null
    private var retry: (() -> Unit)? = null
    init {
        scope.launch { backend.downloads.tasks.collect { tasks ->
            mutable.update { current ->
                val plan = current.plan?.withTasks(tasks) ?: return@update current
                current.copy(plan = plan, selected = current.selected.intersect(plan.items.filter { it.selectable }.map { it.id }.toSet()))
            }
        } }
    }
    fun open(workId: String? = null, origin: String? = null, sourceId: String? = null,
        initialIds: Set<String>? = null, initialPaths: Set<String>? = null) {
        if (mutable.value.busy) return
        job?.cancel()
        retry = { open(workId, origin, sourceId, initialIds, initialPaths) }
        mutable.value = DownloadPlanState(visible = true, loading = true)
        job = scope.launch {
            try {
                val plan = repository.prepare(workId, origin, sourceId)
                ensureActive()
                val selected = plan.items.filter { it.selectable &&
                    (if (initialIds != null) it.id in initialIds else if (initialPaths != null) it.path in initialPaths else true) }.map { it.id }.toSet()
                mutable.value = DownloadPlanState(visible = true, plan = plan, selected = selected)
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.update { it.copy(loading = false, error = onlineMessage(e)) } }
        }
    }
    fun retry() { retry?.invoke() }
    fun close() { if (!mutable.value.busy) { job?.cancel(); retry = null; mutable.value = DownloadPlanState() } }
    fun toggle(ids: List<String>) {
        mutable.update { current ->
            if (current.busy) return@update current
            val eligible = current.plan?.items.orEmpty().filter { it.selectable && it.id in ids }.map { it.id }.toSet()
            current.copy(selected = if (current.selected.containsAll(eligible)) current.selected - eligible else current.selected + eligible)
        }
    }
    fun confirm(done: () -> Unit) {
        val current = mutable.value
        val plan = current.plan ?: return
        if (current.busy || current.selected.isEmpty()) return
        mutable.update { it.copy(busy = true, error = null) }
        job = scope.launch {
            try {
                val count = repository.enqueue(plan, current.selected)
                mutable.value = DownloadPlanState(); retry = null
                feedback(if (count == 0) "所选文件已下载或已在队列中" else "已提交 $count 个文件的下载任务")
                if (count > 0) done()
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { mutable.update { it.copy(busy = false, error = onlineMessage(e)) } }
        }
    }
}
