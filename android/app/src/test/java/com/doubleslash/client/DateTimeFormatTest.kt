package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Test
import java.time.ZoneOffset
import java.util.Locale

class DateTimeFormatTest {
    /** 2026-10-09 15:45:00 UTC. */
    private val at = 1_791_560_700.0

    @Test
    fun unknownIdsFallBackToLocalAmPm() {
        assertEquals(DateTimeFormats.DEFAULT, "ampm")
        assertEquals("ampm", DateTimeFormats.normalize(null))
        assertEquals("ampm", DateTimeFormats.normalize(""))
        assertEquals("ampm", DateTimeFormats.normalize("24h"))
        assertEquals("military", DateTimeFormats.normalize("military"))
    }

    @Test
    fun eachLayoutRendersTheSameInstant() {
        val zone = ZoneOffset.UTC
        val locale = Locale.US
        fun shown(id: String) = DateTimeFormats.format(at, id, zone, locale)

        assertEquals("Oct 9, 2026 3:45 PM", shown("ampm"))
        assertEquals("Oct 9, 2026 15:45", shown("military"))
        assertEquals("10/9/2026 3:45 PM", shown("us"))
        assertEquals("09/10/2026 15:45", shown("uk"))
        assertEquals("09.10.2026 15:45", shown("eu"))
        assertEquals("2026-10-09 15:45", shown("iso"))
        assertEquals("2026/10/09 15:45", shown("east_asia"))
        assertEquals("", DateTimeFormats.format(0.0, "ampm", zone, locale))
    }
}
