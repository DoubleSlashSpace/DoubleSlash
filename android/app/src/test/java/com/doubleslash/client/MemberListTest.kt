package com.doubleslash.client

import com.doubleslash.client.ui.memberView
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Covers what the room's member list says about each person: in voice or not,
 * whether their video is ours to watch, and whether the menu offers a chat or
 * a trust invite.
 *
 * Rosters carry base64 `public_id`s, padded on some paths and not on others,
 * while the peer list is keyed by hex id. A miss in either direction offers an
 * invite to someone already trusted, or a chat with someone who is not.
 */
class MemberListTest {

    private val friend = Peer(peerId = "0a1b2c", identityPub = "RnJpZW5k==")
    private val blocked = Peer(peerId = "0d0e0f", identityPub = "QmxvY2tlZA", blocked = true)
    private val node = Peer(peerId = "node", identityPub = "Tm9kZQ", isSupernode = true)

    private val state = AppState(
        identity = IdentityInfo(publicId = "TWU="),
        peers = listOf(friend, blocked, node),
    )

    @Test
    fun `a trusted peer is found across the padding difference`() {
        assertEquals(friend, state.trustedPeer("RnJpZW5k"))
        assertEquals(friend, state.trustedPeer("RnJpZW5k=="))
    }

    @Test
    fun `blocked peers, supernodes and strangers are not trusted`() {
        assertNull(state.trustedPeer("QmxvY2tlZA"))
        assertNull(state.trustedPeer("Tm9kZQ"))
        assertNull(state.trustedPeer("U3RyYW5nZXI"))
    }

    @Test
    fun `a text-only member cannot be watched even with the camera on`() {
        val s = state.copy(streamingPeers = setOf("RnJpZW5k"))
        val m = s.memberView("RnJpZW5k", "room", voiceMembers = emptyList(), voiceHere = true)
        assertFalse(m.inVoice)
        assertFalse(m.inSession)
        assertTrue(m.cameraOn)
    }

    @Test
    fun `a voice member is watchable only when our voice is in that room`() {
        val voice = listOf("RnJpZW5k==")
        val here = state.memberView("RnJpZW5k", "room", voice, voiceHere = true)
        val reading = state.memberView("RnJpZW5k", "room", voice, voiceHere = false)
        assertTrue(here.inVoice && here.inSession)
        assertTrue(reading.inVoice)
        assertFalse(reading.inSession)
    }

    @Test
    fun `a room's members are unioned across every node that reported them`() {
        // A cluster: the phone joined via node A, the streaming desktop via B.
        val rosters = mapOf(
            "nodeA:room" to listOf("UGhvbmU=", "RnJpZW5k"),
            "nodeB:room" to listOf("RnJpZW5k==", "RGVza3RvcA"),
            "nodeA:other" to listOf("U3RyYW5nZXI"),
        )
        assertEquals(
            listOf("UGhvbmU=", "RnJpZW5k", "RGVza3RvcA"),
            rosters.roomMembersUnion("room"),
        )
    }

    @Test
    fun `our own row is marked as us and our invite state is carried`() {
        val s = state.copy(trustInvites = mapOf("U3RyYW5nZXI" to "sent"))
        assertTrue(s.memberView("TWU", "room", emptyList(), false).isSelf)
        val stranger = s.memberView("U3RyYW5nZXI=", "room", emptyList(), false)
        assertNull(stranger.trustedPeer)
        assertEquals("sent", stranger.inviteState)
    }
}
