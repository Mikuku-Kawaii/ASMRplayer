package com.asmrplayer.android

import androidx.activity.compose.setContent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.ui.CardAction
import com.asmrplayer.android.ui.CardActions
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

@UnstableApi
class CardGestureTest {
    @get:Rule val ui = createAndroidComposeRule<MainActivity>()

    @Test fun nestedTapLongPressAndScrollStayIndependent() {
        var tapped = -1
        var acted = -1
        ui.runOnUiThread { ui.activity.setContent {
            LazyColumn {
                items(30, key = { it }) { index ->
                    CardActions("作品 $index", menu = { close ->
                        CardAction("加入队列", close) { acted = index }
                    }) {
                        Text("爱心 $index", Modifier.fillMaxWidth().height(96.dp).clickable { tapped = index })
                    }
                }
            }
        } }
        val first = ui.onNodeWithText("爱心 0")
        first.performTouchInput { longClick() }
        ui.onNodeWithText("加入队列").performClick()
        ui.runOnIdle { assertEquals(0, acted); assertEquals(-1, tapped) }
        first.performTouchInput { click() }
        ui.runOnIdle { assertEquals(0, tapped) }
        val second = ui.onNodeWithText("爱心 1")
        second.performTouchInput { swipeUp() }
        ui.onNodeWithText("加入队列").assertDoesNotExist()
        val target = ui.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.OnLongClick))[0]
        target.performSemanticsAction(SemanticsActions.OnLongClick)
        ui.onNodeWithText("加入队列").assertIsDisplayed()
    }
}
