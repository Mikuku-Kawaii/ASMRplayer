package com.asmrplayer.android.sync

import androidx.activity.compose.setContent
import androidx.compose.ui.test.*
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import com.asmrplayer.android.MainActivity
import com.asmrplayer.android.data.AndroidBackend
import com.asmrplayer.android.ui.SyncPage
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test

@androidx.media3.common.util.UnstableApi
class StorageSettingsTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun switchesActiveStoreAndDiscardsUnsubmittedGistToken() {
        assumeTrue(ui.activity.packageName.endsWith(".drivetest"))
        val repo = AndroidBackend.get(ui.activity).sync
        ui.activity.getSharedPreferences("sync-selection",android.content.Context.MODE_PRIVATE)
            .edit().putString("provider","GOOGLE").commit()
        org.junit.Assert.assertEquals(Storage.GITHUB,repo.status().provider)
        repo.select(Storage.WEBDAV)
        try {
            ui.runOnUiThread {
                val model=androidx.lifecycle.ViewModelProvider(ui.activity)[com.asmrplayer.android.PlayerViewModel::class.java]
                ui.activity.setContent { SyncPage(model) }
            }
            fun choose(from: String, to: String) {
                ui.onNodeWithText("存储方式：$from").performClick()
                ui.onNodeWithText(to).performClick()
                ui.waitUntil(5000) { ui.onAllNodesWithText("存储方式：$to").fetchSemanticsNodes().isNotEmpty() }
            }
            choose("WebDAV","GitHub Gist")
            ui.waitUntil(5000) { ui.onAllNodes(hasText("使用访问令牌") and isEnabled()).fetchSemanticsNodes().size==1 }
            ui.onNodeWithText("使用访问令牌").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
            ui.waitUntil(5000) {ui.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size==1}
            ui.onNode(hasSetTextAction()).performScrollTo().performTextInput("fixture")
            ui.onNodeWithText("绑定 GitHub 并同步").assertIsEnabled()
            choose("GitHub Gist","OneDrive")
            ui.onNodeWithText("GitHub 访问令牌").assertDoesNotExist()
            choose("OneDrive","GitHub Gist")
            ui.onNodeWithText("登录 GitHub 并同步").assertIsEnabled()
            ui.waitUntil(5000) { ui.onAllNodes(hasText("使用访问令牌") and isEnabled()).fetchSemanticsNodes().size==1 }
            ui.onNodeWithText("使用访问令牌").performScrollTo().performSemanticsAction(SemanticsActions.OnClick)
            ui.waitUntil(5000) {ui.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size==1}
            ui.onNodeWithText("绑定 GitHub 并同步").assertIsNotEnabled()
        } finally { repo.select(Storage.WEBDAV) }
    }
}
