package com.doubleslash.client

import com.doubleslash.client.ui.TreeFold
import com.doubleslash.client.ui.TreeRow
import com.doubleslash.client.ui.buildRoomTree
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Covers what the Rooms tree says about each member: in voice or not, whether
 * their video is ours to watch, and whether their actions offer a chat or a
 * trust invite.
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
    fun `a room invite can be bound to a person, not a block or a node`() {
        val revoked = Peer(peerId = "revoked", identityPub = "UmV2b2tlZA", revoked = true)
        val contacts = (state.peers + revoked).inviteContacts()
        assertEquals(listOf(friend), contacts)
    }

    @Test
    fun `blocked peers, supernodes and strangers are not trusted`() {
        assertNull(state.trustedPeer("QmxvY2tlZA"))
        assertNull(state.trustedPeer("Tm9kZQ"))
        assertNull(state.trustedPeer("U3RyYW5nZXI"))
    }

    private val room = Room(roomId = "room", roomName = "Room", supernodeId = "nodeA")

    /** The member rows the tree shows for [room], with the text leaf open. */
    private fun members(
        voice: List<String>,
        text: List<String>,
        voiceRoom: VoiceRoom?,
    ): List<TreeRow.Member> = buildRoomTree(
        rooms = listOf(room),
        voiceRosters = mapOf("nodeA:room" to voice),
        textRosters = mapOf("nodeA:room" to text),
        reading = room,
        voiceRoom = voiceRoom,
        fold = TreeFold(expanded = setOf(room.key)),
    ).filterIsInstance<TreeRow.Member>()

    @Test
    fun `a text-only member is listed apart and is not in our session`() {
        val rows = members(voice = emptyList(), text = listOf("RnJpZW5k"), voiceRoom = VoiceRoom("nodeA", "room", "Room"))
        val m = rows.single()
        assertFalse(m.voice)
        assertFalse(m.inSession)
    }

    @Test
    fun `a voice member is in our session only when our voice is in that room`() {
        val voice = listOf("RnJpZW5k==")
        val here = members(voice, voice, VoiceRoom("nodeA", "room", "Room")).single()
        val reading = members(voice, voice, voiceRoom = null).single()
        assertTrue(here.voice && here.inSession)
        assertTrue(reading.voice)
        assertFalse(reading.inSession)
    }

    @Test
    fun `someone in voice is not listed again under text only`() {
        // The chat roster carries voice members too, padded differently here.
        val rows = members(voice = listOf("RnJpZW5k=="), text = listOf("RnJpZW5k", "UmVhZGVy"), voiceRoom = null)
        assertEquals(listOf(true, false), rows.map { it.voice })
        assertEquals("UmVhZGVy", rows.last().id)
    }

    @Test
    fun `invites are offered only from a room we are in`() {
        val rows = members(voice = listOf("U3RyYW5nZXI"), text = emptyList(), voiceRoom = null)
        assertTrue(rows.single().canInvite)
        val elsewhere = buildRoomTree(
            rooms = listOf(room),
            voiceRosters = mapOf("nodeA:room" to listOf("U3RyYW5nZXI")),
            textRosters = emptyMap(),
            reading = null,
            voiceRoom = null,
            fold = TreeFold(expanded = setOf(room.key)),
        ).filterIsInstance<TreeRow.Member>().single()
        assertFalse(elsewhere.canInvite)
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
    fun `a stranger is not trusted whatever the padding`() {
        assertNull(state.trustedPeer("U3RyYW5nZXI="))
        assertNull(state.trustedPeer("U3RyYW5nZXI"))
    }
}
