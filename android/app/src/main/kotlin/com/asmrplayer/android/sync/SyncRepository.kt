package com.asmrplayer.android.sync

import android.content.Context
import com.asmrplayer.android.data.LibraryDatabase
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

data class SyncConflict(val key: String, val choices: List<JSONObject>, val label: String)
data class SyncStatus(val provider:Storage, val configured:Boolean, val connected: Boolean, val records: Int, val pending: Int, val conflicts: List<SyncConflict>, val lastSync: Long,
    val autoPolicy: AutoSyncPolicy, val autoError: String, val metadataPending:Int=0, val metadataError:String="", val details:JSONObject=JSONObject(), val resume:JSONObject?=null, val phase:String="")
class SyncRepository(private val context: Context, private val database: LibraryDatabase,
    private val online:com.asmrplayer.android.online.OnlineRepository) {
    @Volatile private var phase = ""
    private val sourceInfo=SyncSourceEnrichment(context,database,online)
    init {
        // Upgrade cleanup for the retired account. Preserve the shared Room journal.
        val legacy=context.getSharedPreferences("google-drive-account",Context.MODE_PRIVATE)
        if(legacy.all.isNotEmpty()) legacy.edit().clear().apply()
        runCatching {
            val store=java.security.KeyStore.getInstance("AndroidKeyStore").apply {load(null)}
            if(store.containsAlias("asmr-google-drive-refresh-v1"))store.deleteEntry("asmr-google-drive-refresh-v1")
        }
    }
    private val providers = mapOf(Storage.GITHUB to GitHubGistProvider(context), Storage.ONEDRIVE to OneDriveProvider(context), Storage.WEBDAV to WebDavProvider(context), Storage.SUPABASE to SupabaseProvider(context))
    private fun selected() = SyncScheduler.selected(context)
    private fun provider(id:Storage) = providers.getValue(id)
    internal fun authorizationRequest(id:Storage) = requireNotNull(provider(id).authorizationRequest())
    fun select(id:Storage) = runGate.withLock {
        SyncScheduler.select(context,id)
        status()
    }
    private val gate = ReentrantLock()
    private val runGate = ReentrantLock()
    fun connected() = provider(selected()).getStatus().connected
    fun logout(id:Storage) = runGate.withLock {
        check(id==selected()); provider(id).logout()
        gate.withLock { val j=journal(); j.remove("resume_${id.name}")
            j.put("doc",SyncProtocol.wire(j.getJSONObject("doc")))
            database.openHelper.writableDatabase.execSQL("INSERT OR REPLACE INTO device_sync_state(id,json) VALUES(1,?)",arrayOf(j.toString())) }
        status()
    }
    private fun journal(): JSONObject = database.openHelper.readableDatabase.query("SELECT json FROM device_sync_state WHERE id=1").use {
        if (it.moveToFirst()) JSONObject(it.getString(0)).also { journal->SyncProtocol.expand(journal.getJSONObject("doc")) } else JSONObject().put("device", UUID.randomUUID().toString()).put("doc", SyncProtocol.empty()).put("baseline", JSONObject()).put("cache", JSONObject())
    }
    fun status(): SyncStatus = gate.withLock {
        val j = journal()
        val records = j.getJSONObject("doc").getJSONObject("records")
        val names = j.optJSONObject("names") ?: JSONObject()
        val conflicts = records.keys().asSequence().filter { records.getJSONArray(it).length() > 1 }.map {
            SyncConflict(it, SyncProtocol.rows(records.getJSONArray(it)), names.optString(it.split('|')[1], "尚未匹配的内容"))
        }.toList()
        val id=selected()
        val binding=provider(id).getStatus()
        SyncStatus(id,binding.configured,binding.connected, records.length(), j.optInt("pending"), conflicts, j.optLong(lastKey(id)), SyncScheduler.policy(context,id), SyncScheduler.error(context,id),sourceInfo.pending(),sourceInfo.error(),j.optJSONObject("details_${id.name}") ?: JSONObject(),if(binding.connected) resumeCandidate(id,null) else null,phase)
    }
    private fun lastKey(id:Storage) = "lastSync_${id.name}"
    private fun exchange(remote: JSONObject? = null, resolution: Triple<String, String, Long>? = null, synced: Storage? = null): JSONObject = gate.withLock {
        val j = journal(); val cache = j.optJSONObject("cache") ?: JSONObject(); val catalog = SyncCatalog.load(context, database, cache)
        database.runInTransaction {
            val db = database.openHelper.writableDatabase
            catalog.useVerifiedKeys(j.getJSONObject("doc"))
            SyncProtocol.capture(j, catalog.snapshot(db), catalog.targets)
            val records=j.getJSONObject("doc").getJSONObject("records")
            for(key in records.keys()) if(key.startsWith("position|")) for(row in SyncProtocol.rows(records.getJSONArray(key))) {
                if(row.getString("device")==j.getString("device")) row.getJSONObject("value").put("deviceName","Android · ${android.os.Build.MODEL}".take(80))
            }
            val before=catalog.snapshot(db)
            catalog.describeLikes(db,j.getJSONObject("doc"))
            remote?.let { SyncProtocol.merge(j.getJSONObject("doc"), it) }
            resolution?.let { (key, device, seq) ->
                val rows = j.getJSONObject("doc").getJSONObject("records").getJSONArray(key)
                val chosen = SyncProtocol.rows(rows).find { it.getString("device") == device && it.getLong("seq") == seq }
                requireNotNull(chosen) { "记录已变化，请重新同步后选择" }
                SyncProtocol.edit(j, key, chosen.get("value"))
            }
            val doc = j.getJSONObject("doc"); SyncProtocol.validate(doc)
            val selected=SyncProtocol.selected(doc)
            val base=context.getSharedPreferences("online-account",Context.MODE_PRIVATE).getString("base",null) ?: com.asmrplayer.android.online.ProviderAccount.DEFAULT_BASE
            catalog.restoreLikes(db, doc, selected, base)
            catalog.apply(db, selected)
            val targets=catalog.targets
            j.put("baseline", catalog.snapshot(db)).put("cache", cache).put("names", JSONObject(catalog.names))
            j.put("pending", doc.getJSONObject("records").keys().asSequence().count { !it.startsWith("playlist|") && it.split('|')[1] !in targets } + SyncPlaylists.referenced(selected).count { it !in targets })
            for (key in selected.keys().asSequence().filter { it.startsWith("playlist|") && !selected.isNull(it) }) catalog.names[key.split('|')[1]]="歌单 · ${selected.getJSONObject(key).getString("name")}"
            j.put("names",JSONObject(catalog.names))
            if (remote != null || resolution != null) {
                val (details,resume)=SyncInsights.describe(doc,before,catalog,j.getString("device"))
                val id=synced ?: this.selected()
                j.put("details_${id.name}",details)
                if(resume!=null) j.put("resume_${id.name}",resume)
            }
            if (synced != null) j.put(lastKey(synced), System.currentTimeMillis())
            db.execSQL("INSERT OR REPLACE INTO device_sync_state(id,json) VALUES(1,?)", arrayOf(j.put("doc",SyncProtocol.wire(doc)).toString()))
            j.put("doc",doc)
        }
        j.getJSONObject("doc")
    }
    fun resolve(key: String, device: String, seq: Long): SyncStatus { exchange(resolution = Triple(key, device, seq)); return status() }
    fun sync(id:Storage = selected()): SyncStatus = runSync(id,null)
    internal fun syncAuthorized(id:Storage,input:LoginInput): SyncStatus = runSync(id,input)
    private fun runSync(id:Storage,input:LoginInput?): SyncStatus {
        check(runGate.tryLock()) { "同步正在进行" }
        try {
            check(id==selected()) { "存储方式已切换，请重试" }
            SyncScheduler.confirm(context,id)
            val provider=provider(id)
            SyncScheduler.attempted(context,id)
            phase="正在读取凭据"
            val token = if(input==null) provider.accessToken() else {
                provider.login(input).also { SyncScheduler.onLogin(context,id) }
            }
            phase="正在整理本机数据"
            val local = exchange()
            val device = gate.withLock { journal().getString("device") }
            val merged = JSONObject(local.toString())
            phase="正在上传与拉取"
            provider.exchangeSyncData(token,device,merged)
            phase="正在应用同步数据"
            exchange(merged, synced = id)
            sourceInfo.complete()
            SyncScheduler.result(context,true,id)
            phase=""
            return status()
        } catch(e:Exception) {
            SyncScheduler.result(context,false,id)
            throw e
        } finally { phase=""; runGate.unlock() }
    }
    fun resumeCandidate(id:Storage, expectedId:String?):JSONObject? = gate.withLock {
        check(id==selected()) { "存储方式已切换" }
        val value=journal().optJSONObject("resume_${id.name}") ?: return null
        if(expectedId!=null) check(value.optString("id")==expectedId) { "收听位置已更新，请重试" }
        val current=database.openHelper.readableDatabase.query("SELECT completed,updatedMs FROM playback_positions WHERE versionId=?",arrayOf<Any>(value.getString("versionId"))).use {
            it.moveToFirst() && it.getInt(0)==0 && it.getLong(1)<=value.getLong("updatedMs")
        }
        value.takeIf { current }
    }
    fun dismissResume(id:Storage, expectedId:String) = gate.withLock {
        check(id==selected())
        val j=journal()
        if(j.optJSONObject("resume_${id.name}")?.optString("id")==expectedId) {
            j.remove("resume_${id.name}")
            j.put("doc",SyncProtocol.wire(j.getJSONObject("doc")))
            database.openHelper.writableDatabase.execSQL("INSERT OR REPLACE INTO device_sync_state(id,json) VALUES(1,?)",arrayOf(j.toString()))
        }
    }
    fun setAutoPolicy(policy: AutoSyncPolicy, id:Storage): SyncStatus = runGate.withLock {
        check(id==selected())
        if (policy.foreground || policy.periodic) check(connected()) { "请先绑定当前存储" }
        SyncScheduler.save(context, policy,id)
        return status()
    }
}
