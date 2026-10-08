package com.doubleslash.client

import kotlinx.serialization.json.JsonObject

/** Session-owned notification state. Accessed on the main thread. */
internal class RoomAlertDelivery {
    private val seen = ArrayDeque<String>()
    private val pending = mutableMapOf<String, Any>()
    private val counts = mutableMapOf<String, Int>()

    fun begin(roomId: String, key: String): Any? {
        if (roomId.isEmpty() || key in seen) return null
        seen.addLast(key)
        while (seen.size > 256) seen.removeFirst()
        return Any().also { pending[roomId] = it }
    }

    fun finish(roomId: String, ticket: Any, eligible: Boolean): Int? {
        if (pending[roomId] !== ticket) return null
        pending.remove(roomId)
        if (!eligible) return null
        val count = (counts[roomId] ?: 0) + 1
        counts[roomId] = count
        return count
    }

    fun cancel(roomId: String) {
        pending.remove(roomId)
        counts.remove(roomId)
    }

    fun reset() {
        seen.clear()
        pending.clear()
        counts.clear()
    }
}

internal fun roomMessageKey(event: JsonObject): String {
    val room = event.stringOrEmpty("room_id")
    val id = event.stringOrEmpty("message_id")
    if (id.isNotEmpty()) return "$room\u0000$id"
    return room + "\u0000" + event.stringOrEmpty("sender_id") + "\u0000" +
        event.number("timestamp").toString() + "\u0000" + event.stringOrEmpty("body")
}

internal fun roomAlertEligible(
    sender: String,
    ownId: String,
    enabled: Boolean,
    foreground: Boolean,
): Boolean = enabled && !foreground && ownId.isNotEmpty() &&
    sender.isNotEmpty() && sender.trimEnd('=') != ownId.trimEnd('=')
