package com.asmrplayer.android

import android.os.Bundle
import android.content.Intent
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.SystemBarStyle
import androidx.activity.viewModels
import androidx.media3.common.util.UnstableApi
import com.asmrplayer.android.ui.PlayerApp

@UnstableApi
class MainActivity : ComponentActivity() {
    private var playerRequest by mutableIntStateOf(0)
    private val model: PlayerViewModel by viewModels()
    private fun receive(value: Intent) {
        when (value.action) {
            OPEN_PLAYER -> playerRequest++
            // Old launcher PendingIntents can survive an upgrade; never replay a stale operation.
            com.asmrplayer.android.widget.PlaybackWidget.ACTION -> { playerRequest++; value.action=OPEN_PLAYER }
            else -> model.incoming.receive(value)
        }
    }
    override fun onStart() { super.onStart(); model.setForeground(true); com.asmrplayer.android.sync.SyncScheduler.foreground(this) }
    override fun onStop() { model.setForeground(false); super.onStop() }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) receive(intent)
        enableEdgeToEdge(statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT), navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
        setContent { PlayerApp(model, playerRequest) }
    }
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        receive(intent)
    }
    companion object { const val OPEN_PLAYER = "com.asmrplayer.OPEN_PLAYER" }
}
