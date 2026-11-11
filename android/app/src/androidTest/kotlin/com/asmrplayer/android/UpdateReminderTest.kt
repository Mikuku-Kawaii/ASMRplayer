package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.media3.common.util.UnstableApi
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
        val release = UpdateRelease("99.1.0", 100, 23, "", "", "", 1024, "", "")
        var update by mutableStateOf(UpdateSnapshot(phase = UpdatePhase.AVAILABLE, release = release))
        var canShow by mutableStateOf(false)
        var opened = 0
        ui.runOnUiThread {
            ui.activity.setContent {
                MaterialTheme { UpdateReminder(update, canShow) { opened++ } }
            }
        }

        val available = ui.onNodeWithText("发现新版本")
        val ready = ui.onNodeWithText("更新已就绪")
        available.assertDoesNotExist()
        ui.runOnIdle { canShow = true; update = update.copy(autoCheck = false) }
        available.assertDoesNotExist()
        ui.runOnIdle { update = update.copy(autoCheck = true, skippedVersion = release.version) }
        available.assertDoesNotExist()
        ui.runOnIdle { update = update.copy(skippedVersion = null) }
        available.assertIsDisplayed()

        ui.onNodeWithText("稍后").performClick()
        available.assertDoesNotExist()
        ui.runOnIdle {
            assertEquals(0, opened)
            update = update.copy(phase = UpdatePhase.READY)
        }
        ready.assertDoesNotExist()

        ui.runOnIdle { update = update.copy(release = release.copy(version = "99.2.0", versionCode = 101)) }
        ready.assertIsDisplayed()
        ui.onNodeWithText("查看更新").performClick()
        ready.assertDoesNotExist()
        ui.runOnIdle { assertEquals(1, opened) }
    }
}
