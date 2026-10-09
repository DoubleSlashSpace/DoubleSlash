package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OwnPeerTest {
    private val me = IdentityInfo(
        publicId = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ=",
        peerId = "myhex",
    )

    @Test
    fun ownRecordIsRecognisedUnderEitherSpelling() {
        assertTrue(Peer(peerId = "myhex").isOwnIdentity(me))
        assertTrue(Peer(peerId = "other", identityPub = me.publicId.trimEnd('=')).isOwnIdentity(me))
        assertFalse(Peer(peerId = "friend", identityPub = "friendpub").isOwnIdentity(me))
        // Before the identity loads nothing is ours, least of all a blank record.
        assertFalse(Peer(peerId = "").isOwnIdentity(IdentityInfo()))
    }

    @Test
    fun statusLineMatchesTheDesktop() {
        assertEquals("Offline", ownStatusLabel(online = false, otherDevices = 2))
        assertEquals("Online · this device only", ownStatusLabel(online = true, otherDevices = 0))
        assertEquals("Online · 1 other device", ownStatusLabel(online = true, otherDevices = 1))
        assertEquals("Online · 3 other devices", ownStatusLabel(online = true, otherDevices = 3))
    }
}
