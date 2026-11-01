package com.doubleslash.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Covers the JSON layer between the core and the UI.
 *
 * These are the pure parts of the boundary — everything that does not need a
 * running core or an Android context — and they encode claims the rest of the
 * app relies on: that a reply is only "ok" when the core said so, that an
 * unknown field cannot break an older build, and that the display fallbacks
 * never surface a raw 44-character id.
 */
class CoreModelsTest {

    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    private fun obj(raw: String): JsonObject = json.parseToJsonElement(raw).jsonObject

    // ── Reply helpers ──────────────────────────────────────────────────────

    @Test
    fun `ok is true only for a boolean true`() {
        assertTrue(obj("""{"ok":true}""").ok)
        assertFalse(obj("""{"ok":false}""").ok)
    }

    @Test
    fun `a reply with no ok field is not treated as success`() {
        // A malformed or truncated reply must fail closed. Reading a missing
        // field as success would make every parse error look like it worked.
        assertFalse(obj("""{}""").ok)
    }

    @Test
    fun `a non-boolean ok does not throw`() {
        // The core always sends a boolean, but a strict accessor here would
        // turn a wire change into a crash rather than a failed command.
        assertFalse(obj("""{"ok":"yes"}""").ok)
        assertFalse(obj("""{"ok":1}""").ok)
    }

    @Test
    fun `error text is read from a failed reply`() {
        assertEquals("no running session", obj("""{"ok":false,"error":"no running session"}""").errorText)
        assertNull(obj("""{"ok":true}""").errorText)
    }

    @Test
    fun `string reads only strings`() {
        val reply = obj("""{"a":"x","b":3}""")
        assertEquals("x", reply.string("a"))
        assertNull(reply.string("b"))
        assertEquals("", reply.stringOrEmpty("missing"))
    }

    @Test
    fun `number falls back to zero rather than throwing`() {
        assertEquals(1.5, obj("""{"t":1.5}""").number("t"), 0.0001)
        assertEquals(0.0, obj("""{}""").number("t"), 0.0001)
    }

    @Test
    fun `string list reads arrays and tolerates absence`() {
        assertEquals(listOf("a", "b"), obj("""{"m":["a","b"]}""").stringList("m"))
        assertEquals(emptyList<String>(), obj("""{}""").stringList("m"))
    }

    @Test
    fun `event name is empty for a command reply`() {
        assertEquals("chat_message", obj("""{"event":"chat_message"}""").eventName())
        assertEquals("", obj("""{"ok":true}""").eventName())
    }

    // ── Models ─────────────────────────────────────────────────────────────

    @Test
    fun `an unknown field does not break decoding`() {
        // Forward compatibility: a core that gained a field must not stop an
        // older app build from reading the rest.
        val peer = json.decodeFromString<Peer>(
            """{"peer_id":"abc","display_name":"Bobert","brand_new_field":42}""",
        )
        assertEquals("abc", peer.peerId)
        assertEquals("Bobert", peer.displayName)
    }

    @Test
    fun `peer label prefers display name then handle then a short id`() {
        val full = Peer(peerId = "0123456789abcdef", displayName = "Bobert", handle = "bob")
        assertEquals("Bobert", full.label)

        val handleOnly = full.copy(displayName = "")
        assertEquals("bob", handleOnly.label)

        // Never the full id: these are 44 characters and unreadable in a row.
        val idOnly = full.copy(displayName = "", handle = "")
        assertEquals("0123456789ab", idOnly.label)
        assertEquals(12, idOnly.label.length)
    }

    @Test
    fun `room key distinguishes the same room id on different supernodes`() {
        val a = Room(roomId = "r1", supernodeId = "node-a")
        val b = Room(roomId = "r1", supernodeId = "node-b")
        assertFalse("list keys must not collide", a.key == b.key)
    }

    @Test
    fun `room hidden defaults to false when the core omits it`() {
        // Hidden is a separate tombstone list on the core side; a room list
        // that omits the flag must not make everything vanish.
        val room = json.decodeFromString<Room>("""{"room_id":"r1","room_name":"General"}""")
        assertFalse(room.hidden)
    }

    @Test
    fun `chat message decodes the fields the UI renders`() {
        val message = json.decodeFromString<ChatMessage>(
            """{"id":"m1","peer_id":"p","body":"hi","timestamp":1.5,"is_self":true,"status":"failed","status_note":"not delivered"}""",
        )
        assertEquals("m1", message.id)
        assertTrue(message.isSelf)
        assertEquals("failed", message.status)
        assertEquals("not delivered", message.statusNote)
    }

    @Test
    fun `a room roster keeps voice participants and chat members apart`() {
        // `members` is the voice rail; `chat_members` is the supernode's full
        // key-group roster (participants + text subscribers). Counting the
        // former as "members" hides every peer reading the room without
        // joining voice - which is what made the header disagree with the
        // desktop's member panel.
        val event = obj(
            """{"event":"room_members_changed","room_id":"r1",
                "members":["me"],"chat_members":["me","bobert"]}""",
        )
        assertEquals(listOf("me"), event.stringList("members"))
        assertEquals(listOf("me", "bobert"), event.stringList("chat_members"))
    }

    @Test
    fun `a roster with nobody in voice still has chat members`() {
        // The all-text case: an empty voice rail must not read as an empty room.
        val event = obj(
            """{"event":"room_members_changed","room_id":"r1",
                "members":[],"chat_members":["me","bobert"]}""",
        )
        assertTrue(event.stringList("members").isEmpty())
        assertEquals(2, event.stringList("chat_members").size)
    }

    @Test
    fun `connection stats read the nested sample and match the desktop chip`() {
        val parsed = obj(
            """{"event":"connection_stats","peer_id":"nodeA=",
                "stats":{"rtt_ms":24,"packet_loss_pct":0.4,"jitter_ms":3,"relay":true,"bandwidth_kbps":120}}""",
        ).connectionStats()
        assertEquals("nodeA=", parsed?.first)
        val stats = parsed!!.second
        assertEquals(24.0, stats.rttMs, 0.01)
        assertEquals(LinkQuality.GOOD, stats.quality)
        assertEquals("24 ms", stats.pingLabel)
        assertEquals("QUIC", stats.pathLabel)
        assertEquals(
            "Ping: 24 ms\nPacket loss: 0.4% · Jitter: 3 ms",
            stats.tooltip(),
        )
        assertNull(obj("""{"event":"connection_stats","peer_id":"n"}""").connectionStats())
        assertEquals(LinkQuality.FAIR, NodeConnectionStats(rttMs = 160.0).quality)
        assertEquals(LinkQuality.BAD, NodeConnectionStats(rttMs = 301.0).quality)
        assertEquals(LinkQuality.BAD, NodeConnectionStats(rttMs = 20.0, packetLossPct = 4.1).quality)
        assertEquals(LinkQuality.FAIR, NodeConnectionStats(rttMs = 20.0, packetLossPct = 1.6).quality)
        assertEquals(LinkQuality.NONE, NodeConnectionStats().quality)
        assertEquals("Excellent", NodeConnectionStats(rttMs = 24.0, packetLossPct = 0.4, jitterMs = 3.0).qualityTierLabel)
        assertEquals("Good", NodeConnectionStats(rttMs = 100.0, jitterMs = 20.0).qualityTierLabel)
        assertEquals("Fair", NodeConnectionStats(rttMs = 160.0).qualityTierLabel)
        assertEquals("Poor", NodeConnectionStats(rttMs = 301.0).qualityTierLabel)
    }

    @Test
    fun `a supernode row shows ping only while that node is up`() {
        val fast = NodeConnectionStats(rttMs = 24.0)
        val node = RoomNodeInfo(nodeId = "nodeA=", members = listOf("nodeA", "nodeB"))
        val state = AppState(
            connectedSupernodes = setOf("nodeA", "nodeB"),
            roomNodes = listOf(node),
            nodeStats = mapOf("nodeA" to fast, "nodeB" to NodeConnectionStats(rttMs = 80.0)),
        )
        assertEquals(fast, state.statsForNode(node))
        assertNull(state.copy(connectedSupernodes = emptySet()).statsForNode(node))
        assertFalse(state.knowsSupernode("bob"))
        assertTrue(state.knowsSupernode("nodeA="))
    }

    @Test
    fun `supernode status line includes ping when connected`() {
        val node = SupernodeInfo(peerId = "peer", identityPub = "nodeA=", clusterMembers = listOf("nodeB"))
        val state = AppState(
            connectedSupernodes = setOf("nodeA"),
            nodeStats = mapOf("nodeA" to NodeConnectionStats(rttMs = 24.0)),
        )
        assertEquals("Connected · 24 ms · cluster of 2", supernodeStatusLine(node, state))
        assertEquals(
            "Offline · cluster of 2",
            supernodeStatusLine(node, state.copy(connectedSupernodes = emptySet())),
        )
    }

    @Test
    fun `a direct peer uses its own ping and is not labeled relayed`() {
        val own = NodeConnectionStats(rttMs = 18.0)
        val node = NodeConnectionStats(rttMs = 40.0, relay = true)
        val state = AppState(
            peers = listOf(Peer(peerId = "bob", identityPub = "pub=", displayName = "Bob")),
            directPeers = setOf("bob"),
            relayPresentPeers = setOf("bob"),
            onlinePeers = setOf("bob"),
            connectedSupernodes = setOf("nodeA"),
            roomNodes = listOf(RoomNodeInfo(nodeId = "nodeA", displayName = "acdc")),
            nodeStats = mapOf("nodeA" to node),
            peerStats = mapOf("pub" to own),
        )
        val link = state.peerLink("bob")
        assertEquals(PeerRoute.DIRECT, link?.route)
        assertEquals("Direct", link?.routeLabel)
        assertEquals(own, link?.stats)
        // The session id may arrive padded. It is still this peer.
        assertEquals(PeerRoute.DIRECT, state.copy(directPeers = setOf("bob=")).peerLink("bob")?.route)
    }

    @Test
    fun `a relayed peer names the supernode whose relay is up`() {
        val websocket = NodeConnectionStats(rttMs = 10.0, relay = false)
        val quic = NodeConnectionStats(rttMs = 40.0, relay = true)
        val state = AppState(
            peers = listOf(Peer(peerId = "bob", displayName = "Bob")),
            relayPresentPeers = setOf("bob"),
            onlinePeers = setOf("bob"),
            connectedSupernodes = setOf("plain", "acdc"),
            roomNodes = listOf(
                RoomNodeInfo(nodeId = "plain", displayName = "plain"),
                RoomNodeInfo(nodeId = "acdc", displayName = "acdc"),
            ),
            nodeStats = mapOf("plain" to websocket, "acdc" to quic),
            // A leftover direct sample must not become this peer's ping.
            peerStats = mapOf("bob" to NodeConnectionStats(rttMs = 5.0)),
        )
        val link = state.peerLink("bob")
        assertEquals(PeerRoute.RELAYED, link?.route)
        assertEquals("Relayed · acdc", link?.routeLabel)
        assertEquals(quic, link?.stats)
        assertNull(state.peerLink("offline"))
    }

    @Test
    fun `a relayed peer with no quic relay still names a connected supernode`() {
        val websocket = NodeConnectionStats(rttMs = 55.0, relay = false)
        val state = AppState(
            relayPresentPeers = setOf("bob"),
            onlinePeers = setOf("bob"),
            connectedSupernodes = setOf("nodeA"),
            roomNodes = listOf(RoomNodeInfo(nodeId = "nodeA", displayName = "acdc")),
            nodeStats = mapOf("nodeA" to websocket),
        )
        val link = state.peerLink("bob")
        assertEquals(PeerRoute.RELAYED, link?.route)
        assertEquals("Relayed · acdc", link?.routeLabel)
        assertEquals(websocket, link?.stats)
    }

    @Test
    fun `a quiet or offline peer shows no ping`() {
        val quiet = AppState(
            directPeers = setOf("bob"),
            onlinePeers = setOf("bob"),
            peerStats = mapOf("bob" to NodeConnectionStats(rttMs = 0.0)),
        )
        val link = quiet.peerLink("bob")
        assertEquals(PeerRoute.DIRECT, link?.route)
        assertEquals("Direct", link?.routeLabel)
        assertNull(link?.stats)

        val stored = NodeConnectionStats(rttMs = 12.0)
        val offline = AppState(peerStats = mapOf("bob" to stored))
        assertNull(offline.peerLink("bob"))

        val unnamed = AppState(
            relayPresentPeers = setOf("bob"),
            onlinePeers = setOf("bob"),
        )
        assertEquals("Relayed", unnamed.peerLink("bob")?.routeLabel)
    }

    @Test
    fun `an unknown id is not stored as a peer`() {
        val people = AppState(peers = listOf(Peer(peerId = "bob", displayName = "Bob")))
        assertTrue(people.knowsPeer("bob"))
        assertTrue(people.knowsPeer("bob="))
        assertFalse(people.knowsPeer("stranger"))
        assertFalse(people.copy(peers = listOf(Peer(peerId = "bob", isSupernode = true))).knowsPeer("bob"))
        assertFalse(people.knowsSupernode("bob"))
    }

    @Test
    fun `a cluster counts the node as well as its siblings`() {
        // The roster names the siblings only; padding does not make a new node.
        val node = SupernodeInfo(identityPub = "aaa=", clusterMembers = listOf("bbb", "ccc=", "ddd", "aaa"))
        assertEquals(4, node.clusterSize)
        assertEquals(1, SupernodeInfo(identityPub = "aaa=").clusterSize)
        // Green while any member is up; connected ids arrive unpadded.
        assertTrue(node.isConnected(setOf("ccc")))
        assertTrue(node.isConnected(setOf("aaa")))
        assertFalse(node.isConnected(setOf("zzz")))
    }
}
