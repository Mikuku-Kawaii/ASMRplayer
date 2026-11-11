package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** The same tonal surface as settings; glass remains on floating navigation and controls. */
@Composable
internal fun MaintenanceCard(title: String? = null, content: @Composable ColumnScope.() -> Unit) {
    Surface(Modifier.fillMaxWidth(), shape = RoundedCornerShape(14.dp), color = MaterialTheme.colorScheme.surfaceContainerLow,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant.copy(alpha = .55f))) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            title?.let { Text(it, style = MaterialTheme.typography.titleMedium) }
            content()
        }
    }
}

@Composable
internal fun MaintenanceHint(text: String, error: Boolean = false) {
    Text(text, style = MaterialTheme.typography.bodySmall,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
internal fun MaintenanceStatus(text: String, error: Boolean = false, dismiss: (() -> Unit)? = null) {
    Surface(Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite }, shape = RoundedCornerShape(16.dp),
        color = if (error) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(text, style = MaterialTheme.typography.bodyMedium)
            if (dismiss != null) MotionTextButton(dismiss, Modifier.align(Alignment.End)) { Text("关闭提示") }
        }
    }
}

@Composable
internal fun MaintenanceValue(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f).alignByBaseline(), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, Modifier.widthIn(max=180.dp).alignByBaseline(), style = MaterialTheme.typography.titleSmall, textAlign=androidx.compose.ui.text.style.TextAlign.End)
    }
}

/** A compact setting/action row. The text can grow without shrinking its action target. */
@Composable
internal fun MaintenanceRow(title: String, summary: String? = null, action: @Composable () -> Unit) {
    val label: @Composable () -> Unit = {
        Text(title, style = MaterialTheme.typography.titleSmall)
        summary?.let { MaintenanceHint(it) }
    }
    BoxWithConstraints(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(vertical = 4.dp), contentAlignment = Alignment.CenterStart) {
        if (maxWidth < 270.dp * androidx.compose.ui.platform.LocalDensity.current.fontScale) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                label()
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) { action() }
            }
        } else {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) { label() }
                action()
            }
        }
    }
}
