package com.asmrplayer.android.download

data class ResumePlan(val append: Boolean, val total: Long?, val expectedBody: Long?, val alreadyComplete: Boolean = false)

/** AN006: validate response BEFORE opening .part for append. */
object ResumePolicy {
    fun evaluate(status: Int, offset: Long, range: String?, contentLength: Long?): ResumePlan {
        require(offset >= 0)
        return when (status) {
            200 -> ResumePlan(false, contentLength, contentLength)
            206 -> {
                val match = Regex("bytes (\\d+)-(\\d+)/(\\d+)").matchEntire(range.orEmpty())
                    ?: throw IllegalArgumentException("无效 Content-Range")
                val (start, end, total) = match.destructured
                val a = start.toLong(); val b = end.toLong(); val size = total.toLong()
                require(a == offset && b >= a && size > b)
                val length = b - a + 1
                require(contentLength == null || contentLength == length)
                ResumePlan(offset > 0, size, length)
            }
            416 -> {
                val size = Regex("bytes \\*/(\\d+)").matchEntire(range.orEmpty())?.groupValues?.get(1)?.toLong()
                    ?: throw IllegalArgumentException("无效 Content-Range")
                require(offset > 0 && offset == size)
                ResumePlan(true, size, 0, true)
            }
            else -> throw IllegalArgumentException("下载响应不受支持")
        }
    }
}
