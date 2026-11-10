package com.asmrplayer.android.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.PlayerViewModel

@OptIn(ExperimentalMaterial3Api::class)
@UnstableApi
@Composable
internal fun HomeRefresh(page: Destination, model: PlayerViewModel, browsing: Boolean, opening: Boolean,
                        content: @Composable () -> Unit) {
    val busy = when (page) {
        Destination.Library -> {
            val data by model.collections.state.collectAsStateWithLifecycle()
            data.refreshing || (!data.ready && data.error == null)
        }
        Destination.Favorites -> {
            val data by model.collections.favoriteCatalog.collectAsStateWithLifecycle()
            val refreshing by model.collections.refreshingFavorites.collectAsStateWithLifecycle()
            val records by model.collections.state.collectAsStateWithLifecycle()
            refreshing || records.refreshing || (!data.ready && data.error == null)
        }
        Destination.Discover -> browsing
        Destination.Queue -> {
            val groups by model.queueGroups.state.collectAsStateWithLifecycle()
            groups.refreshing || groups.busy || (!groups.ready && groups.error == null)
        }
    } || opening
    fun refresh() {
        if (busy) return
        when (page) {
            Destination.Library -> model.collections.retry()
            Destination.Favorites -> { model.collections.retryFavoriteWorks(); model.collections.retry() }
            Destination.Discover -> model.online.search(page = model.online.state.value.requestedPage)
            Destination.Queue -> model.refreshQueue()
        }
    }
    PullToRefreshBox(isRefreshing = busy, onRefresh = ::refresh, modifier = Modifier.fillMaxSize().semantics {
        customActions = listOf(CustomAccessibilityAction("刷新${page.title}") { refresh(); true })
    }) { content() }
}
