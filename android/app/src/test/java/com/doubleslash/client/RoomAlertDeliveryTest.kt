package com.doubleslash.client

import org.junit.Assert.*
import org.junit.Test

class RoomAlertDeliveryTest {
    @Test
    fun `session delivery needs no activity and deduplicates cluster copies`() {
        val delivery = RoomAlertDelivery()
        val ticket = requireNotNull(delivery.begin("room", "first"))
        assertEquals(1, delivery.finish("room", ticket, true))
        assertNull(delivery.begin("room", "first"))
        val next = requireNotNull(delivery.begin("room", "second"))
        assertEquals(2, delivery.finish("room", next, true))
    }

    @Test
    fun `opening or muting cancels an alert during metadata lookup`() {
        val delivery = RoomAlertDelivery()
        val ticket = requireNotNull(delivery.begin("room", "first"))
        delivery.cancel("room")
        assertNull(delivery.finish("room", ticket, true))
        assertNull(delivery.begin("room", "first"))
        val next = requireNotNull(delivery.begin("room", "second"))
        assertEquals(1, delivery.finish("room", next, true))
    }

    @Test
    fun `disconnect invalidates pending alerts and starts a fresh session`() {
        val delivery = RoomAlertDelivery()
        val ticket = requireNotNull(delivery.begin("room", "first"))
        delivery.reset()
        assertNull(delivery.finish("room", ticket, true))
        val next = requireNotNull(delivery.begin("room", "first"))
        assertEquals(1, delivery.finish("room", next, true))
    }

    @Test
    fun `eligibility excludes own devices muted rooms and foreground`() {
        assertTrue(roomAlertEligible("other", "me=", true, false))
        assertFalse(roomAlertEligible("me", "me=", true, false))
        assertFalse(roomAlertEligible("other", "me", false, false))
        assertFalse(roomAlertEligible("other", "me", true, true))
        assertFalse(roomAlertEligible("other", "", true, false))
    }
}
