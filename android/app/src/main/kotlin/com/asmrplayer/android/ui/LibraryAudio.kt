package com.asmrplayer.android.ui

import com.asmrplayer.android.data.VersionDetail

/** A presentation of the shared Room work; playback always uses version IDs. */
data class LibraryAudio(
    val id: String,
    val title: String,
    val favorite: Boolean,
    val rating: String,
    val versions: List<VersionDetail>,
    val coverUri: String? = null,
    val code: String? = null,
    val inLibrary: Boolean = true,
) {
    val nsfw get() = rating == "NSFW"
    fun containsVersion(id: String?) = versions.any { it.id == id }
}

/** Built once per library emission. Progress ticks and lazy list rows only look up stable IDs. */
class LibraryLookup(val works: List<LibraryAudio> = emptyList()) {
    val byId = works.associateBy { it.id }
    private val byVersion = buildMap { for (work in works) for (version in work.versions) put(version.id, work) }
    fun forVersion(id: String?) = byVersion[id]
}
