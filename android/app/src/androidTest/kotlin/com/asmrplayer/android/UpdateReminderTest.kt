package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performFirstLinkClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeUp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.ui.FullscreenArtwork
import com.asmrplayer.android.ui.UpdateReminder
import com.asmrplayer.android.update.UpdatePhase
import com.asmrplayer.android.update.UpdateRelease
import com.asmrplayer.android.update.UpdateSnapshot
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

@UnstableApi
class UpdateReminderTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun defersUntilHomeAndHonorsDismissedAndSkippedVersions() {
        val notes = "## 播放改进\n" + (1..30).joinToString("\n") { "- **修复**项目 $it" }
        val release = UpdateRelease("99.1.0", 100, 23, "", "", "", 1024, notes, "")
        var update by mutableStateOf(UpdateSnapshot(phase = UpdatePhase.AVAILABLE, release = release))
        var canShow by mutableStateOf(false)
        val started = mutableListOf<UpdatePhase>()
        ui.runOnUiThread {
            ui.activity.setContent {
                MaterialTheme { UpdateReminder(update, canShow) { started += update.phase } }
            }
        }

        val dialog = ui.onNodeWithText("软件更新")
        val primary = ui.onNodeWithText("立即更新")
        dialog.assertDoesNotExist()
        ui.runOnIdle { canShow = true; update = update.copy(autoCheck = false) }
        dialog.assertDoesNotExist()
        ui.runOnIdle { update = update.copy(autoCheck = true, skippedVersion = release.version) }
        dialog.assertDoesNotExist()
        ui.runOnIdle { update = update.copy(skippedVersion = null) }
        dialog.assertIsDisplayed()
        ui.onNodeWithText("→ v99.1.0", substring = true).assertIsDisplayed()
        ui.onNodeWithText("播放改进\n• 修复项目 1", substring = true).performTouchInput { swipeUp() }
        primary.assertIsDisplayed()

        ui.onNodeWithText("稍后").performClick()
        dialog.assertDoesNotExist()
        ui.runOnIdle {
            assertEquals(emptyList<UpdatePhase>(), started)
            update = update.copy(phase = UpdatePhase.READY)
        }
        dialog.assertDoesNotExist()

        ui.runOnIdle { update = update.copy(release = release.copy(version = "99.2.0", versionCode = 101)) }
        ui.onNodeWithText("安装包已校验，可以安装").assertIsDisplayed()
        primary.performClick()
        dialog.assertDoesNotExist()

        ui.runOnIdle { update = update.copy(phase = UpdatePhase.AVAILABLE, release = release.copy(version = "99.3.0")) }
        primary.performClick()
        dialog.assertDoesNotExist()
        ui.runOnIdle { assertEquals(listOf(UpdatePhase.READY, UpdatePhase.AVAILABLE), started) }
    }

    @Test fun releaseImageAttachmentsOpenViewerAndReturnToUpdateDialog() {
        val release = UpdateRelease("99.1.0", 100, 23, "", "", "", 1024, "", "")
        var notes by mutableStateOf("![播放截图](https://example.invalid/screen.png)")
        ui.runOnUiThread {
            ui.activity.setContent {
                MaterialTheme {
                    UpdateReminder(UpdateSnapshot(phase = UpdatePhase.AVAILABLE, release = release.copy(notes = notes)), true) {}
                }
            }
        }

        val close = ui.onNodeWithContentDescription("关闭图片")
        ui.onNodeWithText("查看图片 · 播放截图").performFirstLinkClick { true }
        close.assertIsDisplayed().performClick()
        close.assertDoesNotExist()
        ui.onNodeWithText("立即更新").assertIsDisplayed()

        ui.runOnIdle { notes = """<img alt="界面截图" width="500" src="https://example.invalid/layout.png">""" }
        ui.onNodeWithText("查看图片 · 界面截图").performFirstLinkClick { true }
        close.assertIsDisplayed().performClick()
        close.assertDoesNotExist()
    }

    @Test fun fullscreenViewerRendersLocalImage() {
        ui.runOnUiThread {
            val image = "android.resource://${ui.activity.packageName}/${R.drawable.brand_mark}"
            ui.activity.setContent {
                MaterialTheme { FullscreenArtwork(image, label = "附件测试图片") {} }
            }
        }
        val image = ui.onNodeWithContentDescription("附件测试图片")
        ui.waitUntil(5_000) { ui.onAllNodes(androidx.compose.ui.test.hasContentDescription("附件测试图片")).fetchSemanticsNodes().isNotEmpty() }
        image.assertIsDisplayed()
    }
}
