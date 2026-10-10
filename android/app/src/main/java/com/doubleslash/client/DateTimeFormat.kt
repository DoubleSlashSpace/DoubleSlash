package com.doubleslash.client

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Chat date and time layouts shared with the desktop.
 *
 * The stored value is an id, not a pattern, so both clients agree. Times are
 * the device's local clock. An unknown id is [DEFAULT].
 */
object DateTimeFormats {
    const val DEFAULT = "ampm"

    val all: List<Format> = listOf(
        Format("ampm", "Local, AM/PM", "MMM d, yyyy h:mm a"),
        Format("military", "Military, 24-hour", "MMM d, yyyy HH:mm"),
        Format("us", "United States", "M/d/yyyy h:mm a"),
        Format("uk", "United Kingdom", "dd/MM/yyyy HH:mm"),
        Format("eu", "Europe", "dd.MM.yyyy HH:mm"),
        Format("iso", "ISO 8601", "yyyy-MM-dd HH:mm"),
        Format("east_asia", "East Asia", "yyyy/MM/dd HH:mm"),
    )

    fun normalize(raw: String?): String =
        all.firstOrNull { it.id == raw }?.id ?: DEFAULT

    fun format(
        epochSeconds: Double,
        id: String,
        zone: ZoneId = ZoneId.systemDefault(),
        locale: Locale = Locale.getDefault(),
    ): String {
        if (epochSeconds <= 0.0) return ""
        val pattern = all.first { it.id == normalize(id) }.pattern
        val formatter = DateTimeFormatter.ofPattern(pattern, locale)
        val instant = Instant.ofEpochMilli((epochSeconds * 1000.0).toLong())
        return formatter.format(instant.atZone(zone))
    }

    data class Format(val id: String, val label: String, val pattern: String)
}
