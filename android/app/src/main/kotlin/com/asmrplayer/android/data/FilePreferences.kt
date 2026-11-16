package com.asmrplayer.android.data

import java.text.Normalizer
import java.util.Locale

enum class EffectPreference(val key: String, val label: String) {
    Any("any", "不指定"), With("with", "有效果音"), Without("without", "无效果音");
    companion object { fun from(key: String?) = entries.firstOrNull { it.key == key } ?: Any }
}

enum class AudioFormatPreference(val key: String, val label: String) {
    Any("any", "不指定"), Mp3("mp3", "MP3"), Wav("wav", "WAV");
    companion object { fun from(key: String?) = entries.firstOrNull { it.key == key } ?: Any }
}

data class FilePreferences(
    val effects: EffectPreference = EffectPreference.Any,
    val format: AudioFormatPreference = AudioFormatPreference.Any,
) {
    val summary: String get() = listOfNotNull(
        effects.takeUnless { it == EffectPreference.Any }?.label,
        format.takeUnless { it == AudioFormatPreference.Any }?.label,
    ).joinToString(" · ").ifEmpty { "不指定版本与格式" }
}

private val noEffects = Regex("se(?:なし|無し|ナシ|無)|[无無]\\s*(?:se|效果音|音效|効果音)|(?:効果音|效果音|音效)(?:なし|無し|ナシ)|(?<![a-z])(?:no|without)[\\s_.-]*se(?:$|[^a-z])|なし版|無し版")
private val withEffects = Regex("se(?:あり|有り|アリ|有)|[有含带帶]\\s*(?:se|效果音|音效|効果音)|(?:効果音|效果音|音效)(?:あり|有り|アリ)|(?<![a-z])with[\\s_.-]*se(?:$|[^a-z])|あり版|有り版")
private val supplementary = Regex("試聽|试听|試聴|体験版|體驗版|サンプル|過往作品|过往作品|特典|おまけ|额外|額外|フリートーク|自由谈话|自由談話|ng片段|ng集|予告|预告|預告|(?<![a-z])(?:sample|preview|trailer|bonus|extra|free[ _-]*talk)(?![a-z])")

internal fun directoryEffects(path: String): EffectPreference {
    // Inspect the nearest directory first: an outer work name may itself mention SE.
    val segments = Normalizer.normalize(path, Normalizer.Form.NFKC).lowercase(Locale.ROOT)
        .split('/', '\\').asReversed()
    for (segment in segments) {
        val text = segment.replace(Regex("(?i)(se|効果音|效果音|音效)\\s+"), "$1")
        if (noEffects.containsMatchIn(text)) return EffectPreference.Without
        if (withEffects.containsMatchIn(text)) return EffectPreference.With
    }
    return EffectPreference.Any
}

private fun directoryPreferenceScore(group: Pair<String, String>, preferences: FilePreferences, relativePath: String): Int {
    val effects = directoryEffects(group.first)
    val effectScore = when {
        preferences.effects == EffectPreference.Any -> 0
        effects == preferences.effects -> 4
        effects == EffectPreference.Any -> 2
        else -> 0
    }
    val formatScore = if (preferences.format != AudioFormatPreference.Any &&
        group.second.trim().removePrefix(".").equals(preferences.format.key, ignoreCase = true)) 1 else 0
    return (if (supplementary.containsMatchIn(relativePath.lowercase(Locale.ROOT))) 0 else 8) + effectScore + formatScore
}

private fun preferenceScores(groups: List<Pair<String, String>>, preferences: FilePreferences): Map<Pair<String, String>, Int> {
    val paths = groups.map { it.first.replace('\\', '/').split('/') }
    // Ignore the common work folder, whose title can mention bonus content or previews.
    var prefix = 0
    while (paths.isNotEmpty() && paths.all { it.size > prefix } && paths.map { it[prefix] }.distinct().size == 1) prefix++
    return groups.mapIndexed { index, group -> group to directoryPreferenceScore(group, preferences, paths[index].drop(prefix).joinToString("/")) }.toMap()
}

/** Stable ordering: main content precedes extras; unknown SE remains unknown. Keep every directory. */
internal fun preferredDirectoryOrder(groups: List<Pair<String, String>>, preferences: FilePreferences): List<Pair<String, String>> {
    val scores = preferenceScores(groups, preferences)
    return groups.sortedWith(compareByDescending<Pair<String, String>> { scores[it] }.thenBy { directoryDepth(it.first) })
}

private fun directoryDepth(path: String) = path.split('/', '\\').count { it.isNotBlank() }

internal fun preferredDirectory(
    groups: List<Pair<String, String>>, preferences: FilePreferences, resume: Pair<String, String>?,
): Pair<String, String>? {
    val scores = preferenceScores(groups, preferences)
    val best = preferredDirectoryOrder(groups, preferences).firstOrNull() ?: return null
    return resume?.takeIf { it in groups && scores[it] == scores[best] && directoryDepth(it.first) == directoryDepth(best.first) } ?: best
}
