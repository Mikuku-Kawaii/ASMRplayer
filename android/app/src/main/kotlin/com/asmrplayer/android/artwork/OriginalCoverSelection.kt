package com.asmrplayer.android.artwork

/** Only explicit jacket/cover files; promotional panels and phone wallpapers are not covers. */
internal fun originalCoverPriority(path: String): Int {
    val normalized=path.replace('\\','/').lowercase()
    val name=normalized.substringAfterLast('/')
    if(name.substringAfterLast('.') !in setOf("png","jpg","jpeg","webp"))return 0
    if(listOf("wallpaper","壁紙","壁纸","iphone","android","紹介","介绍","sample","thumb").any{normalized.contains(it)})return 0
    val explicit=listOf("ジャケット","jacket","cover","封面","メインビジュアル","表紙","パッケージ","package","ロゴ","サムネ").any{name.contains(it)}
    val stem=name.substringBeforeLast('.')
    val main=Regex("(?i)(?:(?:rj|bj|vj)\\d{6,8}[_ -]?)?img[_ -]?main").matches(stem) || stem in setOf("folder","front","main") || Regex("(?:rj|bj|vj)\\d{6,8}").matches(stem)
    val inCoverDirectory=normalized.substringBeforeLast('/',"").split('/').any{part->listOf("ジャケット","jacket","covers","封面").any{part.contains(it)}}
    val numbered=Regex("\\d{1,3}(?:[_ -](?:logo|ロゴ))?").matches(stem)
    if(!explicit&&!main&&!(inCoverDirectory&&numbered))return 0
    val withoutLogo=listOf("ロゴなし","ロゴ無し","文字なし","文字無し","no_logo","nologo","no logo","无字","無字","package_off").any{name.contains(it)}
    val logo=!withoutLogo && (name.contains("ロゴ") || name.contains("logo"))
    return 10 + (if(logo)20 else 0) + (if(name.contains("メイン") || name.contains("main"))5 else 0) - (if(withoutLogo)5 else 0)
}
