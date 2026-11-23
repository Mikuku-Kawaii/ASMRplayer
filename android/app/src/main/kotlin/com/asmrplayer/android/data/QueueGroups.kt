package com.asmrplayer.android.data

import androidx.room.*
import kotlinx.coroutines.flow.Flow
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

data class CollectionWork(val id: String, val directory: String? = null, val format: String? = null, val fromSite: Boolean = false) {
    fun json() = JSONObject().put("id", id).apply { directory?.let { put("directory", it) }; format?.let { put("format", it) }; if(fromSite) put("fromSite",true) }
    fun valid() = id.isNotBlank() && id.length <= 128 &&
        (directory == null || (directory.toByteArray().size <= 4096 && !directory.startsWith('/') && !directory.startsWith('\\') && ':' !in directory && '\u0000' !in directory && ".." !in directory.split('/', '\\'))) &&
        (format == null || format.matches(Regex("[A-Za-z0-9]{1,16}")))
    companion object {
        fun parse(row: JSONObject) = CollectionWork(row.getString("id"), row.opt("directory") as? String, row.opt("format") as? String, row.optBoolean("fromSite")).also { require(it.valid()) }
        fun encode(items: List<CollectionWork>) = JSONArray().apply { items.distinctBy { it.id }.forEach { require(it.valid()); put(it.json()) } }.toString()
    }
}

@Entity(tableName = "queue_groups")
data class QueueGroup(@PrimaryKey val id: String, val name: String, val versionIds: String = "[]", val sortOrder: Long = 0, val workItems: String? = null) {
    @get:Ignore val isWorks: Boolean get() = workItems != null
    @delegate:Ignore private val members by lazy { workItems?.let { raw -> JSONArray(raw).let { a -> List(a.length()) { CollectionWork.parse(a.getJSONObject(it)) } } } }
    fun works() = members.orEmpty()
    @delegate:Ignore private val parsed by lazy { decode(versionIds) }
    fun ids(): List<String> = members?.map { it.id } ?: parsed
    companion object {
        private val siteId = Regex("asmr-list-[0-9a-f]{64}")

        fun validName(id: String, name: String): Boolean {
            if (name.isBlank()) return false
            return if (id.matches(siteId)) name.length <= 8192 else name.codePointCount(0, name.length) <= 80
        }

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

/** Saved lists hold stable work or version IDs. Playback state belongs to the media session. */
class QueueGroupRepository(private val db: LibraryDatabase) {
    val groups = db.queueGroups().observe()
    suspend fun create(name: String, ids: List<String> = emptyList(), works: List<CollectionWork>? = null): String = db.withTransaction {
        val title = name.trim(); require(title.isNotBlank() && title.length <= 80)
        validate(ids)
        if (works != null) { require(ids.isEmpty()); validateWorks(works) }
        val id = UUID.randomUUID().toString()
        db.queueGroups().put(QueueGroup(id, title, QueueGroup.encode(ids), (db.queueGroups().all().maxOfOrNull { it.sortOrder } ?: -1) + 1, works?.let(CollectionWork::encode)))
        id
    }
    suspend fun rename(id: String, name: String) = db.withTransaction {
        require(name.trim().isNotBlank() && name.trim().length <= 80)
        db.queueGroups().get(id)?.let { db.queueGroups().put(it.copy(name = name.trim())) }
    }
    private suspend fun validateWorks(items: List<CollectionWork>) {
        for (item in items) require(item.valid() && db.library().work(item.id) != null)
    }
    suspend fun addWorks(id: String, items: List<CollectionWork>): List<String> = db.withTransaction {
        val group = requireNotNull(db.queueGroups().get(id)).also { require(it.isWorks) }
        validateWorks(items)
        val seen = group.ids().toMutableSet()
        val added = items.filter { seen.add(it.id) }
        db.queueGroups().put(group.copy(workItems = CollectionWork.encode(group.works() + added)))
        added.map { it.id }
    }
    suspend fun add(id: String, ids: List<String>): List<String> = db.withTransaction {
        validate(ids)
        val group = requireNotNull(db.queueGroups().get(id))
        require(!group.isWorks)
        val added = ids.distinct() - group.ids().toSet()
        db.queueGroups().put(group.copy(versionIds = QueueGroup.encode(group.ids() + added)))
        added
    }
    suspend fun remove(id: String, version: String) = removeItems(id, setOf(version))
    suspend fun removeItems(id: String, ids: Set<String>) = db.withTransaction {
        db.queueGroups().get(id)?.let { g ->
            db.queueGroups().put(if (g.isWorks) g.copy(workItems = CollectionWork.encode(g.works().filterNot { it.id in ids }))
                else g.copy(versionIds = QueueGroup.encode(g.ids().filterNot(ids::contains))))
        }
    }
    suspend fun move(id: String, version: String, target: Int, expected: List<String>) = db.withTransaction {
        val group = requireNotNull(db.queueGroups().get(id)); require(group.ids() == expected)
        val ordered = expected.toMutableList(); require(ordered.remove(version) && target in 0..ordered.size)
        ordered.add(target, version)
        val members = group.works().associateBy { it.id }
        db.queueGroups().put(if (group.isWorks) group.copy(workItems = CollectionWork.encode(ordered.map(members::getValue))) else group.copy(versionIds = QueueGroup.encode(ordered)))
    }
    suspend fun moveGroup(id: String, delta: Int) = db.withTransaction {
        val all = db.queueGroups().all(); val kind = all.firstOrNull { it.id == id }?.isWorks ?: return@withTransaction
        val groups = all.filter { it.isWorks == kind }.toMutableList(); val index = groups.indexOfFirst { it.id == id }
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

val MIGRATION_8_9 = object : androidx.room.migration.Migration(8, 9) {
    override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
        db.execSQL("ALTER TABLE queue_groups ADD COLUMN workItems TEXT")
        db.execSQL("ALTER TABLE track_versions ADD COLUMN durationMs INTEGER")
    }
}

val MIGRATION_7_8 = object : androidx.room.migration.Migration(7, 8) {
    override fun migrate(db: androidx.sqlite.db.SupportSQLiteDatabase) {
        db.execSQL("CREATE TABLE IF NOT EXISTS queue_groups (id TEXT NOT NULL, name TEXT NOT NULL, versionIds TEXT NOT NULL, sortOrder INTEGER NOT NULL, PRIMARY KEY(id))")
    }
}
