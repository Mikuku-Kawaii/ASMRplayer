package com.asmrplayer.android.incoming

import org.junit.Assert.*
import org.junit.Test

class WorkLinkTest {
    @Test fun supportedLinksPreserveWorkIdentity() {
        assertEquals("123456", WorkLink.id("https://asmr.one/work/123456"))
        assertEquals("01234567", WorkLink.id("https://asmr-200.com/work/RJ01234567?from=share"))
        assertEquals("123456", WorkLink.id("https://www.dlsite.com/maniax/work/=/product_id/RJ123456.html"))
    }
    @Test fun unrelatedOrAmbiguousLinksAreNotImported() {
        listOf("http://asmr.one/work/123456", "https://asmr.one.evil.test/work/123456",
            "https://user@asmr.one/work/123456", "https://asmr.one:8443/work/123456",
            "https://asmr.one/work/123456/extra", "https://dlsite.com/RJ123456", "not a link")
            .forEach { assertNull(it, WorkLink.id(it)) }
    }
}
