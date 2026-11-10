package com.asmrplayer.android

import android.content.Context
import com.asmrplayer.android.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*

data class QueueGroupState(val groups: List<QueueGroup> = emptyList(), val ready: Boolean = false,
    val refreshing: Boolean = false, val busy: Boolean = false, val error: String? = null)

class QueueGroupTools(context: Context, private val scope: CoroutineScope,
    private val feedback: (String, String?, (() -> Unit)?) -> Unit) {
    private val repository = AndroidBackend.get(context).queueGroups
    private val mutable = MutableStateFlow(QueueGroupState())
    val state = mutable.asStateFlow()
    private var reader: Job? = null
    init { refresh() }
    fun refresh() {
        if (mutable.value.refreshing) return
        reader?.cancel()
        mutable.update { it.copy(refreshing=true, error=null) }
        reader = scope.launch {
            repository.groups.catch { mutable.update { it.copy(refreshing=false,error="歌单无法读取") } }
                .collect { groups -> mutable.update { it.copy(groups=groups, ready=true, refreshing=false) } }
        }
    }
    private fun run(block: suspend () -> Unit) {
        if(mutable.value.busy)return
        mutable.update { it.copy(busy=true,error=null) }
        scope.launch { try { block() } catch(e: CancellationException) { throw e }
            catch(_: Exception) { feedback("操作未完成，请检查音轨是否可用；每组最多 500 首。", null, null) }
            finally { mutable.update { it.copy(busy=false) } } }
    }
    fun create(name: String, ids: List<String> = emptyList(), done: (String) -> Unit = {}) = run { done(repository.create(name,ids)) }
    fun rename(id: String, name: String, done: () -> Unit = {}) = run { repository.rename(id,name);done() }
    fun add(id: String, ids: List<String>, done: () -> Unit = {}) = run { repository.add(id,ids);done() }
    fun remove(id: String, version: String) = run { repository.remove(id,version) }
    fun move(id: String, version: String, target: Int, expected: List<String>) = run { repository.move(id,version,target,expected) }
    fun moveGroup(id: String, delta: Int) = run { repository.moveGroup(id,delta) }
    fun delete(group: QueueGroup, done: () -> Unit) = run { repository.delete(group.id);feedback("已删除「${group.name}」","撤销") { run { repository.restore(group) } };done() }
}
