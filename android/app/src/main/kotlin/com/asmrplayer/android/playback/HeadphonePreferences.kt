package com.asmrplayer.android.playback

import android.content.SharedPreferences
import com.asmrplayer.core.DspConfig
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

data class SoundProfile(val id: String, val name: String, val config: DspConfig)
data class OutputBinding(val key: String, val label: String, val profileId: String)
data class OutputRoute(val deviceId: Int, val label: String, val key: String?, val sharedPort: Boolean = false)
data class HeadphoneState(
    val automatic: Boolean = false, val profiles: List<SoundProfile> = emptyList(),
    val bindings: List<OutputBinding> = emptyList(), val activeId: String? = null,
    val route: OutputRoute? = null, val modified: Boolean = false, val warning: String? = null,
)

/** Service-owned preferences. Selecting a profile only changes the existing atomic DSP snapshot. */
class HeadphonePreferences(private val prefs: SharedPreferences, initial: DspConfig, private val apply: (DspConfig) -> Unit) {
    private var common = initial
    private var effective = initial
    private var value = HeadphoneState()
    val state get() = value.copy(modified = (value.profiles.firstOrNull { it.id == value.activeId }?.config ?: common) != effective)

    init {
        prefs.getString("headphones", null)?.let { raw ->
            try {
                val root = JSONObject(raw)
                require(root.getInt("version") == 1)
                val profiles = root.getJSONArray("profiles").let { list ->
                    require(list.length() <= 32)
                    List(list.length()) { i -> list.getJSONObject(i).let { SoundProfile(it.getString("id"), it.getString("name"), DspSettings.decode(it.getString("config"))) } }
                }
                require(profiles.map { it.id }.distinct().size == profiles.size)
                val bindings = root.getJSONArray("bindings").let { list -> List(list.length()) { i -> list.getJSONObject(i).let {
                    OutputBinding(it.getString("key"), it.getString("label"), it.getString("profile"))
                } } }.filter { b -> profiles.any { it.id == b.profileId } }
                common = DspSettings.decode(root.getString("common"))
                value = HeadphoneState(root.getBoolean("automatic"), profiles, bindings, root.optString("active").takeIf { id -> profiles.any { it.id == id } })
                if (value.automatic) use(null) // Never restore a headphone-specific sound onto an unknown startup output.
            } catch (_: Exception) { value = HeadphoneState(warning = "耳机偏好无法读取，当前音效保留；可重新保存方案。") }
        }
    }

    fun edited(config: DspConfig) { effective = config }
    fun routeChanged(route: OutputRoute?) {
        val oldKey = value.route?.key
        val oldId = value.route?.deviceId
        value = value.copy(route = route)
        if (value.automatic && (oldKey != route?.key || oldId != route?.deviceId)) applyRoute()
    }
    fun automatic(enabled: Boolean) {
        value = value.copy(automatic = enabled)
        if (enabled) applyRoute()
        persist()
    }
    fun save(name: String, id: String? = null) {
        val clean = name.trim(); require(clean.isNotEmpty() && clean.length <= 80)
        if (id == null) require(value.profiles.size < 32) else require(value.profiles.any { it.id == id })
        val profile = SoundProfile(id ?: UUID.randomUUID().toString(), clean, effective)
        value = value.copy(profiles = value.profiles.filterNot { it.id == profile.id } + profile, activeId = profile.id)
        persist()
    }
    private fun use(id: String?) {
        val next = if (id == null) common else value.profiles.first { it.id == id }.config
        value = value.copy(activeId = id)
        effective = next; apply(next)
    }
    fun select(id: String?) { use(id); persist() }
    fun saveCommon() { common = effective; value = value.copy(activeId = null); persist() }
    fun bind(id: String, expectedRoute: String) {
        val route = requireNotNull(value.route)
        require(route.key != null && route.key == expectedRoute)
        require(value.profiles.any { it.id == id })
        value = value.copy(bindings = value.bindings.filterNot { it.key == route.key } + OutputBinding(route.key, route.label, id))
        if (value.automatic) applyRoute()
        persist()
    }
    fun unbind(key: String) {
        value = value.copy(bindings = value.bindings.filterNot { it.key == key })
        if (value.automatic && value.route?.key == key) applyRoute()
        persist()
    }
    fun remove(id: String) {
        val active = value.activeId == id
        value = value.copy(profiles = value.profiles.filterNot { it.id == id }, bindings = value.bindings.filterNot { it.profileId == id })
        if (active) use(null)
        persist()
    }
    private fun applyRoute() = use(value.bindings.firstOrNull { it.key == value.route?.key }?.profileId)
    private fun persist() {
        val json = JSONObject().put("version", 1).put("automatic", value.automatic).put("common", DspSettings.encode(common))
            .put("active", value.activeId).put("profiles", JSONArray().apply { value.profiles.forEach {
                put(JSONObject().put("id", it.id).put("name", it.name).put("config", DspSettings.encode(it.config)))
            } }).put("bindings", JSONArray().apply { value.bindings.forEach {
                put(JSONObject().put("key", it.key).put("label", it.label).put("profile", it.profileId))
            } })
        prefs.edit().putString("headphones", json.toString()).apply()
        value = value.copy(warning = null)
    }
}

object HeadphoneCommands {
    const val GET = "com.asmrplayer.GET_HEADPHONES"
    const val EDIT = "com.asmrplayer.EDIT_HEADPHONES"
    fun encode(state: HeadphoneState): String = JSONObject().apply {
        put("automatic", state.automatic); put("active", state.activeId); put("modified", state.modified); put("warning", state.warning)
        put("profiles", JSONArray().apply { state.profiles.forEach { put(JSONObject().put("id", it.id).put("name", it.name).put("config", DspSettings.encode(it.config))) } })
        put("bindings", JSONArray().apply { state.bindings.forEach { put(JSONObject().put("key", it.key).put("label", it.label).put("profile", it.profileId)) } })
        state.route?.let { put("route", JSONObject().put("id", it.deviceId).put("label", it.label).put("key", it.key).put("sharedPort", it.sharedPort)) }
    }.toString()
    fun decode(raw: String): HeadphoneState = JSONObject(raw).let { root ->
        HeadphoneState(root.getBoolean("automatic"), root.getJSONArray("profiles").let { a -> List(a.length()) { i -> a.getJSONObject(i).let {
            SoundProfile(it.getString("id"), it.getString("name"), DspSettings.decode(it.getString("config")))
        } } }, root.getJSONArray("bindings").let { a -> List(a.length()) { i -> a.getJSONObject(i).let {
            OutputBinding(it.getString("key"), it.getString("label"), it.getString("profile"))
        } } }, root.optString("active").takeIf { it.isNotEmpty() }, root.optJSONObject("route")?.let {
            OutputRoute(it.getInt("id"), it.getString("label"), it.optString("key").takeIf { key -> key.isNotEmpty() }, it.getBoolean("sharedPort"))
        }, root.getBoolean("modified"), root.optString("warning").takeIf { it.isNotEmpty() })
    }
}
