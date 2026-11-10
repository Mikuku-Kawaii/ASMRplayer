package com.asmrplayer.android.artwork

import kotlin.math.abs

/** Compare spatial colour samples; matching a palette alone must not select another illustration. */
internal fun matchingCoverSamples(reference:IntArray,candidate:IntArray,referenceAspect:Float,candidateAspect:Float):Boolean {
    if(reference.size!=768||candidate.size!=768||referenceAspect<=0||candidateAspect<=0)return false
    if(abs(referenceAspect/candidateAspect-1f)>.12f)return false
    fun channel(c:Int,shift:Int)=(c shr shift) and 255
    fun light(c:Int)=(channel(c,16)*3+channel(c,8)*6+channel(c,0))/10
    val errors=DoubleArray(reference.size){i->listOf(16,8,0).sumOf{shift->abs(channel(reference[i],shift)-channel(candidate[i],shift))}/765.0}
    // Lettering/compression may differ in a small part of the image.
    errors.sort()
    if(errors.average()>.16||errors.take(653).average()>.095)return false
    var edges=0;var aligned=0
    for(y in 0 until 24)for(x in 0 until 32) {
        val i=y*32+x
        for(j in listOfNotNull(if(x<31)i+1 else null,if(y<23)i+32 else null)) {
            val a=light(reference[i])-light(reference[j]);val b=light(candidate[i])-light(candidate[j])
            if(abs(a)>=12){edges++;if(a*b>0)aligned++}
        }
    }
    return edges>=24&&aligned.toDouble()/edges>=.72
}

internal fun coverComparisonCandidate(path:String):Boolean {
    val p=path.replace('\\','/').lowercase()
    return p.substringAfterLast('.') in setOf("png","jpg","jpeg","webp")&&
        listOf("wallpaper","壁紙","壁纸","iphone","android","紹介","介绍","sample","thumb","キャラクター","character","トラックリスト","tracklist").none{p.contains(it)}
}
