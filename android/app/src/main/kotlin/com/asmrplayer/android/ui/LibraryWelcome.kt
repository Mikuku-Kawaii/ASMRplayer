package com.asmrplayer.android.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.asmrplayer.android.R

@Composable
internal fun LibraryWelcome(importAudio: () -> Unit, browseOnline: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val typography = MaterialTheme.typography
    Box(Modifier.fillMaxWidth().padding(top = 16.dp), contentAlignment = Alignment.TopCenter) {
        Surface(
            modifier = Modifier.widthIn(max = 440.dp).fillMaxWidth(),
            shape = MaterialTheme.shapes.extraLarge,
            color = colors.surfaceContainer,
        ) {
            Column(
                Modifier.padding(horizontal = 20.dp, vertical = 28.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.size(116.dp).background(colors.primaryContainer, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Image(painterResource(R.drawable.brand_mark), contentDescription = null, modifier = Modifier.size(96.dp))
                }
                Spacer(Modifier.height(20.dp))
                Text(
                    "开始收听",
                    Modifier.semantics { heading() },
                    style = typography.headlineLarge,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "音频库还没有作品。",
                    style = typography.bodyLarge,
                    color = colors.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(24.dp))
                Column(
                    Modifier.widthIn(max = 280.dp).fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    MotionButton(importAudio, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        AppGlyph(Glyph.Folder, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("导入本地音频", textAlign = TextAlign.Center)
                    }
                    MotionOutlinedButton(browseOnline, Modifier.fillMaxWidth().heightIn(min = 48.dp)) {
                        AppGlyph(Glyph.Discover, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("浏览在线作品", textAlign = TextAlign.Center)
                    }
                }
            }
        }
    }
}
