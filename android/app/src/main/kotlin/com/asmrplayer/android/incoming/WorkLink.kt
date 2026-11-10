package com.asmrplayer.android.incoming

import java.net.URI

internal object WorkLink {
    fun id(text: String): String? = runCatching {
        val uri = URI(text.trim())
        if (uri.scheme != "https" || uri.userInfo != null || uri.port !in listOf(-1, 443)) return null
        val host = uri.host?.lowercase() ?: return null
        val path = uri.path.orEmpty()
        when (host) {
            "asmr.one", "www.asmr.one", "asmr-100.com", "asmr-200.com", "asmr-300.com" ->
                Regex("^/work/(?:RJ)?([0-9]{4,12})/?$", RegexOption.IGNORE_CASE).find(path)?.groupValues?.get(1)
            "www.dlsite.com", "dlsite.com" -> Regex("/product_id/RJ([0-9]{4,12})\\.html$", RegexOption.IGNORE_CASE).find(path)?.groupValues?.get(1)
            else -> null
        }
    }.getOrNull()
}
