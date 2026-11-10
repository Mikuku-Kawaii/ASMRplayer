package com.asmrplayer.android.artwork

/** Keep card and full-player decoding bounded without sharing a thumbnail cache entry. */
internal fun artworkSampleSize(width: Int, height: Int, maxEdge: Int): Int {
    require(maxEdge > 0)
    var sample = 1
    while (maxOf(width, height) / sample > maxEdge) sample *= 2
    return sample
}

/** Downsample oversized sources and keep the display and lock-screen size between power-of-two steps. */
internal fun artworkDecodeOptions(width: Int, height: Int, edge: Int) = android.graphics.BitmapFactory.Options().apply {
    inSampleSize = artworkSampleSize(width, height, edge)
    val longest = maxOf(width, height)
    if (edge >= 1024 && inSampleSize > 1 && longest / inSampleSize < edge) {
        inSampleSize /= 2
        inDensity = longest / inSampleSize
        inTargetDensity = edge
        inScaled = true
    }
}
