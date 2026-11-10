package com.asmrplayer.android.online

data class BrowseFilters(val tags:List<String> = emptyList(),val voices:List<String> = emptyList(),val circle:String="",val excludedTags:List<String> = emptyList(),val rating:String="",val subtitleOnly:Boolean=false) {
    fun query(keyword:String):String {
        fun valid(s:String,max:Int)=s.length<=max && s.none {it=='$'||it.isISOControl()}
        require(valid(keyword,200) && tags.size+excludedTags.size<=5 && tags.size+excludedTags.size+voices.size+(if(circle.isBlank())0 else 1)+(if(rating.isBlank())0 else 1)<=12) {"最多 5 个标签、12 个条件；关键词最多 200 字。"}
        val parts=mutableListOf(keyword.trim())
        fun term(kind:String,value:String) {require(value.isNotBlank()&&valid(value,100));parts.add("\$$kind:${value.trim()}\$")}
        tags.distinct().forEach{term("tag",it)};excludedTags.distinct().forEach{term("-tag",it)}
        voices.distinct().forEach{term("va",it)};if(circle.isNotBlank())term("circle",circle)
        if(rating.isNotEmpty()){require(rating in setOf("adult","general"));term("age",rating)}
        return parts.filter{it.isNotBlank()}.joinToString(" ")
    }
    val count get()=tags.size+voices.size+excludedTags.size+(if(circle.isBlank())0 else 1)+(if(rating.isBlank())0 else 1)+(if(subtitleOnly)1 else 0)
}
