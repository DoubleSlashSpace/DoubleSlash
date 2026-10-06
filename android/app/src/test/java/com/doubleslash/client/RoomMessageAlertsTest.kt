package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RoomMessageAlertsTest {

    @Test
    fun `an empty or unreadable list mutes every room`() {
        assertTrue(parseRoomMessageAlerts(null).isEmpty())
        assertTrue(parseRoomMessageAlerts("").isEmpty())
        assertTrue(parseRoomMessageAlerts("nope").isEmpty())
        assertTrue(parseRoomMessageAlerts("{}").isEmpty())
    }

    @Test
    fun `room ids round-trip and blanks are dropped`() {
        val parsed = parseRoomMessageAlerts("""["lobby","","lobby","raid"]""")
        assertEquals(setOf("lobby", "raid"), parsed)
        val again = parseRoomMessageAlerts(roomMessageAlertsJson(parsed))
        assertEquals(parsed, again)
    }

    @Test
    fun `room alert notifications do not reuse the service or call ids`() {
        assertTrue(RoomMessageNotifier.notificationId("lobby") >= 0x1000)
        assertTrue(RoomMessageNotifier.notificationId("raid") >= 0x1000)
        assertEquals(
            RoomMessageNotifier.notificationId("lobby"),
            RoomMessageNotifier.notificationId("lobby"),
        )
    }
}
