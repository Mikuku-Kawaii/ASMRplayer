package com.asmrplayer.android.data

import com.asmrplayer.android.online.ProviderAccount
import kotlinx.coroutines.flow.first

internal fun collectionChapters(item: CollectionWork, versions: List<VersionDetail>, preferences: FilePreferences, resumeVersion: String?): List<String> {
    val candidates = versions.filter { it.available && (item.directory == null || it.groupPath == item.directory) &&
        (item.format == null || it.format.equals(item.format, ignoreCase = true)) }
    val groups = candidates.groupBy { it.groupPath to it.format.lowercase() }
    val resume = candidates.firstOrNull { it.id == resumeVersion }
    val selected = preferredDirectory(groups.keys.toList(), preferences, resume?.let { it.groupPath to it.format.lowercase() })
    return groups[selected].orEmpty().distinctBy { it.trackId }.map { it.id }
}

internal suspend fun AndroidBackend.playlistVersions(id: String, preferences: FilePreferences): List<String> {
    val group = requireNotNull(database.queueGroups().get(id))
    val content = library.preferences.first()
    val visible = library.works.first().filter { (it.rating != "NSFW" || content.includeNsfw) &&
        (it.rating != "UNRATED" || content.includeUnrated) }.associateBy { it.id }
    if (!group.isWorks) {
        val available = library.versionsByWork.first().filterKeys(visible::containsKey).values.flatten()
            .filter { it.available }.mapTo(hashSetOf()) { it.id }
        return group.ids().filter(available::contains)
    }
    val positions = library.positions.first().filter { !it.completed && it.positionMs > 0 }.associateBy { it.versionId }
    val offline = database.metadata().offlineWorks().first().toHashSet()
    val chapters = mutableListOf<String>()
    for (item in group.works()) {
        if (item.id !in visible) continue
        var versions = library.versions(item.id).first()
        val metadata = database.metadata().forWork(item.id)
        if (metadata != null && (versions.isEmpty() || (item.id !in offline && visible[item.id]?.rootId == null))) {
            val base = online.account.state.value.apiBase
            if (metadata.sourceId.isBlank() || !(metadata.origin == base || (metadata.origin in ProviderAccount.OFFICIAL_BASES && base in ProviderAccount.OFFICIAL_BASES))) continue
            val imported = online.importWork(metadata.sourceId, addToLibrary = false, reuseDetail = true)
            val work = library.work(imported.workId) ?: continue
            if ((work.rating == "NSFW" && !content.includeNsfw) || (work.rating == "UNRATED" && !content.includeUnrated)) continue
            versions = library.versions(imported.workId).first()
        }
        val last = versions.mapNotNull { positions[it.id] }.maxByOrNull { it.updatedMs }
        chapters += collectionChapters(item, versions, preferences, last?.versionId)
    }
    return chapters
}
