package com.asmrplayer.android

/** Media3 repeat and shuffle remain the source of truth; no UI-only random queue. */
enum class PlaybackOrder(val repeatMode:Int,val shuffle:Boolean,val label:String) {
    SEQUENTIAL(0,false,"顺序播放"), LIST_LOOP(2,false,"列表循环"), SINGLE_LOOP(1,false,"单曲循环"), SHUFFLE_LOOP(2,true,"随机循环");
    fun next()=entries[(ordinal+1)%entries.size]
    companion object {
        fun from(repeatMode:Int,shuffle:Boolean)=if(shuffle)SHUFFLE_LOOP else when(repeatMode){1->SINGLE_LOOP;2->LIST_LOOP;else->SEQUENTIAL}
    }
}
