package com.asmrplayer.android.download

import org.junit.Assert.assertEquals
import org.junit.Test

class DownloadStorageUsageTest {
    @Test fun partialDownloadsRemainDownloadsAndFinishedCoversCountOnlyOnce() {
        val usage = DownloadStorageUsage(listOf(
            DownloadStorageItem("audio", "audio", "COMPLETED", 80, 0),
            DownloadStorageItem("image", "image", "COMPLETED", 10, 0),
            DownloadStorageItem("cover", "cover", "COMPLETED", 20, 0),
            DownloadStorageItem("pending", "cover", "PAUSED", 0, 7),
            DownloadStorageItem("queued", "audio", "QUEUED", 0, 0)),
            freeBytes = 1000, artworkBytes = 30, playbackBytes = 40, loudnessBytes = 2)
        assertEquals(97L, usage.downloadBytes)
        assertEquals(50L, usage.coverCacheBytes)
        assertEquals(92L, usage.cacheBytes)
        assertEquals(usage.totalBytes + 72, usage.downloadBytes + usage.cacheBytes)
    }
}
