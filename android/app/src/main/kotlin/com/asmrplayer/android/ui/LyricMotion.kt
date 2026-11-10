package com.asmrplayer.android.ui

import androidx.compose.animation.core.LinearOutSlowInEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.lazy.LazyListState

/** Align the active cue to the list's leading content padding. A seek skips intervening cues. */
internal suspend fun LazyListState.followLyric(target: Int, animate: Boolean) {
    val visible = layoutInfo.visibleItemsInfo.firstOrNull { it.index == target }
    if (!animate || visible == null) scrollToItem(target)
    else animateScrollBy(visible.offset.toFloat(), tween(220, easing = LinearOutSlowInEasing))
}
