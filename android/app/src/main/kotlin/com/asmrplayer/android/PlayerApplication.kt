package com.asmrplayer.android

import android.app.Application
import androidx.work.Configuration

class PlayerApplication : Application(), Configuration.Provider {
    @androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
    override fun onConfigurationChanged(newConfig: android.content.res.Configuration) {
        super.onConfigurationChanged(newConfig)
        com.asmrplayer.android.widget.PlaybackWidget.refresh(this)
    }
    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        if (level >= TRIM_MEMORY_RUNNING_LOW) com.asmrplayer.android.artwork.CoverArtwork.trimMemory()
    }

    override fun onCreate() {
        super.onCreate()
        com.asmrplayer.android.sync.SyncScheduler.configure(this)
        // Debug builds use explicit checks; instrumentation never contacts the release server.
        if (!BuildConfig.DEBUG) com.asmrplayer.android.update.UpdateScheduler.configure(this)
    }
    override val workManagerConfiguration: Configuration
        get() = Configuration.Builder()
            // Keep WorkManager jobs separate from DownloadScheduler's UIDT job 6006.
            .setJobSchedulerJobIdRange(10_000, 19_999)
            .build()
}
