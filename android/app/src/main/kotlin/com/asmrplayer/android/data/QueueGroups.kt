package com.asmrplayer.android.data

import androidx.room.*
import kotlinx.coroutines.flow.Flow
import org.json.JSONArray
import java.util.UUID

@Entity(tableName = "queue_groups")
data class QueueGroup(@PrimaryKey val id: String, val name: String, val versionIds: String = "[]", val sortOrder: Long = 0) {
    @delegate:Ignore private val parsed by lazy { decode(versionIds) }
    fun ids(): List<String> = parsed
    companion object {
        fun decode(value: String): List<String> {
            val array = JSONArray(value)
            require(array.length() <= 500)
            return List(array.length()) { array.getString(it).also { id -> require(id.isNotBlank() && id.length <= 128) } }.distinct()
        }
        fun encode(ids: List<String>): String {
            require(ids.size <= 500)
            return JSONArray(ids.distinct()).toString()
        }
    }
}

@Dao
interface QueueGroupDao {
    @Query("SELECT * FROM queue_groups ORDER BY sortOrder,id") fun observe(): Flow<List<QueueGroup>>
    @Query("SELECT * FROM queue_groups ORDER BY sortOrder,id") suspend fun all(): List<QueueGroup>
    @Query("SELECT * FROM queue_groups WHERE id=:id") suspend fun get(id: String): QueueGroup?
    @Upsert suspend fun put(group: QueueGroup)
    @Query("DELETE FROM queue_groups WHERE id=:id") suspend fun delete(id: String)
}

/** Saved lists refer only to stable versions; they never own a player or a playback cursor. */
class QueueGroupRepository(private val db: LibraryDatabase) {
    val groups = db.queueGroups().observe()
    suspend fun create(name: String, ids: List<String> = emptyList()): String = db.withTransaction {
        val title = name.trim(); require(title.isNotBlank() && title.length <= 80)
        validate(ids)
        val id = UUID.randomUUID().toString()
        db.queueGroups().put(QueueGroup(id, title, QueueGroup.encode(ids), (db.queueGroups().all().maxOfOrNull { it.sortOrder } ?: -1) + 1))
        id
    }
    suspend fun rename(id: String, name: String) = db.withTransaction {
        require(name.trim().isNotBlank() && name.trim().length <= 80)
        db.queueGroups().get(id)?.let { db.queueGroups().put(it.copy(name = name.trim())) }
    }
    suspend fun add(id: String, ids: List<String>) = db.withTransaction {
        validate(ids)
        val group = requireNotNull(db.queueGroups().get(id))
        db.queueGroups().put(group.copy(versionIds = QueueGroup.encode((group.ids() + ids).distinct())))
    }
    suspend fun remove(id: String, version: String) = db.withTransaction {
        db.queueGroups().get(id)?.let { db.queueGroups().put(it.copy(versionIds = QueueGroup.encode(it.ids() - version))) }
    }
    suspend fun move(id: String, version: String, target: Int, expected: List<String>) = db.withTransaction {
        val group = requireNotNull(db.queueGroups().get(id)); require(group.ids() == expected)
        val ordered = expected.toMutableList(); require(ordered.remove(version) && target in 0..ordered.size)
        ordered.add(target, version); db.queueGroups().put(group.copy(versionIds = QueueGroup.encode(ordered)))
    }
    suspend fun moveGroup(id: String, delta: Int) = db.withTransaction {
        val groups = db.queueGroups().all().toMutableList(); val index = groups.indexOfFirst { it.id == id }
        if (index >= 0 && index + delta in groups.indices) {
            val item = groups.removeAt(index); groups.add(index + delta, item)
            groups.forEachIndexed { order, group -> db.queueGroups().put(group.copy(sortOrder = order.toLong())) }
        }
    }
    suspend fun delete(id: String) = db.withTransaction { db.queueGroups().delete(id) }
    suspend fun restore(group: QueueGroup) = db.withTransaction { db.queueGroups().put(group) }
    private suspend fun validate(ids: List<String>) {
        require(ids.size <= 500 && ids.all { db.library().version(it) != null })
    }
}

val MIGRATION_7_8 = object : androidx.room.migration.Migration(7, 8) {
    override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS queue_groups (id TEXT NOT NULL, name TEXT NOT NULL, versionIds TEXT NOT NULL, sortOrder INTEGER NOT NULL, PRIMARY KEY(id))")
    }
}
