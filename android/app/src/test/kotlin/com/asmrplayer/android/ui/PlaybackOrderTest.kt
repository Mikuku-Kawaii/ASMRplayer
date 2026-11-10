package com.asmrplayer.android.ui

import com.asmrplayer.android.PlaybackOrder
import org.junit.Assert.*
import org.junit.Test

class PlaybackOrderTest {
    @Test fun fourModesUseDistinctIconsAndReturnToSequential() {
        var order=PlaybackOrder.SEQUENTIAL
        val icons=mutableSetOf<Glyph>()
        repeat(4){icons+=order.glyph;order=order.next()}
        assertEquals(4,icons.size)
        assertEquals(PlaybackOrder.SEQUENTIAL,order)
        assertEquals(PlaybackOrder.SHUFFLE_LOOP,PlaybackOrder.from(2,true))
        assertEquals(PlaybackOrder.LIST_LOOP,PlaybackOrder.from(2,false))
        assertEquals(PlaybackOrder.SINGLE_LOOP,PlaybackOrder.from(1,false))
    }
}
