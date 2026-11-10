package com.asmrplayer.android.ui

import com.asmrplayer.android.PlaybackOrder
import com.asmrplayer.android.PlayerScreenState

internal val PlayerScreenState.playbackOrder get()=PlaybackOrder.from(repeatMode,shuffle)
internal val PlaybackOrder.glyph get()=when(this) {
    PlaybackOrder.SEQUENTIAL->Glyph.Sequence
    PlaybackOrder.LIST_LOOP->Glyph.Repeat
    PlaybackOrder.SINGLE_LOOP->Glyph.RepeatOne
    PlaybackOrder.SHUFFLE_LOOP->Glyph.Shuffle
}
