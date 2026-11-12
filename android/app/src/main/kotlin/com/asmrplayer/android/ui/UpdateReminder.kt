package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.BuildConfig
import com.asmrplayer.android.online.HttpTransport
import com.asmrplayer.android.update.UpdatePhase
import com.asmrplayer.android.update.UpdateSnapshot

@Composable
internal fun UpdateReminder(update: UpdateSnapshot, canShow: Boolean, startUpdate: () -> Unit) {
    var dismissedVersion by rememberSaveable { mutableStateOf<String?>(null) }
    if (!canShow || !update.autoCheck) return
    val ready = when (update.phase) {
        UpdatePhase.AVAILABLE -> false
        UpdatePhase.READY -> true
        else -> return
    }
    val release = update.release ?: return
    if (release.version == update.skippedVersion || release.version == dismissedVersion) return

    val dismiss = { dismissedVersion = release.version }
    PlayerAlertDialog(
        onDismissRequest = dismiss,
        title = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("软件更新", Modifier.semantics { heading() })
                Text(
                    "v${BuildConfig.VERSION_NAME} → v${release.version}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("更新日志", Modifier.semantics { heading() }, style = MaterialTheme.typography.titleSmall)
                UpdateReleaseLog(
                    release.notes,
                    Modifier.weight(1f, fill = false).heightIn(max = 280.dp).verticalScroll(rememberScrollState()),
                )
                Text(
                    if (ready) "安装包已校验，可以安装" else "安装包 ${storageBytes(release.size)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        dismissButton = { MotionTextButton(dismiss) { Text("稍后") } },
        confirmButton = {
            MotionButton({
                dismiss()
                startUpdate()
            }) { Text("立即更新") }
        },
    )
}

private val releaseHeading = Regex("""^#{1,6}\s+(.+)$""")
private val releaseBullet = Regex("""^(\s*)[-*+]\s+""")
private val releaseInline = Regex(
    """(<img\b[^>]*>)|(!?)\[([^\]]*)\]\(([^\s)]+)(?:[ \t]+"[^"]*")?\)|\*\*([^*]+)\*\*|`([^`]+)`""",
    RegexOption.IGNORE_CASE,
)
private val releaseImageAttribute = Regex("""\s(src|alt)\s*=\s*(?:"([^"]*)"|'([^']*)')""", RegexOption.IGNORE_CASE)
private const val RELEASE_LOG_LIMIT = 50_000
private val releaseImageTransport = HttpTransport()

private data class ReleaseImage(val address: String, val label: String)

@Composable
internal fun UpdateReleaseLog(notes: String, modifier: Modifier = Modifier) {
    var selectedImage by remember(notes) { mutableStateOf<ReleaseImage?>(null) }
    val linkColor = MaterialTheme.colorScheme.primary
    val log = remember(notes, linkColor) {
        buildAnnotatedString {
            fun appendImage(address: String, label: String) {
                val url = address.removeSurrounding("<", ">").replace("&amp;", "&")
                val image = runCatching { releaseImageTransport.validate(url) }.getOrNull()
                    ?.let { ReleaseImage(it.toString(), label.ifBlank { "更新日志图片" }) }
                if (image == null) {
                    append(label.ifBlank { "图片不可用" })
                } else {
                    withLink(LinkAnnotation.Clickable(
                        tag = "release-image",
                        styles = TextLinkStyles(SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline)),
                        linkInteractionListener = { selectedImage = image },
                    )) { append(if (label.isBlank()) "查看图片" else "查看图片 · $label") }
                }
            }
            val content = notes.trim()
            content.take(RELEASE_LOG_LIMIT).ifBlank { "此版本未提供更新日志。" }.lineSequence().forEachIndexed { index, line ->
                if (index > 0) append('\n')
                val heading = releaseHeading.matchEntire(line.trimStart())
                if (heading != null) {
                    withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(heading.groupValues[1]) }
                } else {
                    val text = releaseBullet.replaceFirst(line, "$1• ")
                    var offset = 0
                    releaseInline.findAll(text).forEach { part ->
                        append(text.substring(offset, part.range.first))
                        when {
                            part.groups[1] != null -> {
                                val attrs = releaseImageAttribute.findAll(part.groupValues[1]).associate {
                                    it.groupValues[1].lowercase() to it.groupValues[2].ifEmpty { it.groupValues[3] }
                                }
                                appendImage(attrs["src"].orEmpty(), attrs["alt"].orEmpty())
                            }
                            part.groupValues[2] == "!" -> appendImage(part.groupValues[4], part.groupValues[3])
                            part.groups[3] != null -> append(part.groupValues[3])
                            part.groups[5] != null -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(part.groupValues[5]) }
                            else -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) { append(part.groupValues[6]) }
                        }
                        offset = part.range.last + 1
                    }
                    append(text.substring(offset))
                }
            }
            if (content.length > RELEASE_LOG_LIMIT) append("\n\n更新日志过长，已显示前 50,000 个字符。")
        }
    }
    Text(log, modifier, style = MaterialTheme.typography.bodyMedium)
    selectedImage?.let { image ->
        FullscreenArtwork(image.address, label = image.label) { selectedImage = null }
    }
}
