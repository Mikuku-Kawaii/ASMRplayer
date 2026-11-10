package com.asmrplayer.android

import com.asmrplayer.android.download.ResumePolicy
import org.junit.Assert.*
import org.junit.Test

class ResumePolicyTest {
    @Test fun partialResponseMustMatchRequestedOffsetAndLength() {
        val plan = ResumePolicy.evaluate(206, 100, "bytes 100-199/200", 100)
        assertTrue(plan.append); assertEquals(200L, plan.total); assertEquals(100L, plan.expectedBody)
    }
    @Test(expected = IllegalArgumentException::class) fun wrongRangeNeverAppends() { ResumePolicy.evaluate(206, 100, "bytes 0-99/200", 100) }
    @Test(expected = IllegalArgumentException::class) fun inconsistentLengthNeverAppends() { ResumePolicy.evaluate(206, 100, "bytes 100-199/200", 99) }
    @Test fun ignoredRangeAlwaysTruncates() { assertFalse(ResumePolicy.evaluate(200, 100, null, 200).append) }
    @Test fun rangeNotSatisfiableOnlyCompletesExactSize() { assertTrue(ResumePolicy.evaluate(416, 200, "bytes */200", null).alreadyComplete) }
    @Test(expected = IllegalArgumentException::class) fun incomplete416IsNotSuccess() { ResumePolicy.evaluate(416, 100, "bytes */200", null) }
}
