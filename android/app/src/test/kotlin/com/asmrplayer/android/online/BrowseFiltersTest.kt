package com.asmrplayer.android.online

import org.junit.Assert.*
import org.junit.Test

class BrowseFiltersTest {
    @Test fun structuredConditionsAreAndedAndPreserveUnicode() {
        assertEquals("睡眠 \$tag:耳かき\$ \$-tag:大声\$ \$va:声優 A\$ \$circle:森\$ \$age:adult\$",
            BrowseFilters(tags=listOf("耳かき"),excludedTags=listOf("大声"),voices=listOf("声優 A"),circle="森",rating="adult").query(" 睡眠 "))
    }
    @Test fun operatorsAndExcessConditionsAreRejected() {
        listOf(BrowseFilters(tags=listOf("a\$age:adult\$")),BrowseFilters(tags=(1..6).map{it.toString()}),BrowseFilters(rating="unknown")).forEach {
            assertThrows(IllegalArgumentException::class.java){it.query("")}
        }
        assertThrows(IllegalArgumentException::class.java){BrowseFilters().query("a\nb")}
        assertEquals("",BrowseFilters(subtitleOnly=true).query(""))
    }
}
