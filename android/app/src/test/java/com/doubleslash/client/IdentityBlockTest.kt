package com.doubleslash.client

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class IdentityBlockTest {
    @Test
    fun `blocking a stranger does not add a trusted peer`() {
        val state = AppState(blockedPeople = listOf(BlockedPerson("public", "Room member")))
        assertTrue(state.isIdentityBlocked("public="))
        assertNull(state.trustedPeer("public="))
        assertTrue(state.peers.isEmpty())
        assertFalse(state.copy(blockedPeople = emptyList()).isIdentityBlocked("public"))
        assertNull(state.copy(blockedPeople = emptyList()).trustedPeer("public"))
    }

    @Test
    fun `stored peer ids and padded public ids resolve to the same block`() {
        val state = AppState(
            peers = listOf(Peer(peerId = "hex", identityPub = "public=")),
            blockedPeople = listOf(BlockedPerson("public")),
        )
        assertTrue(state.isIdentityBlocked("hex"))
        assertTrue(state.isIdentityBlocked("public="))
        assertFalse(state.isIdentityBlocked("other"))
    }
}
