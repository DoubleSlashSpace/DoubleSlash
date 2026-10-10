package com.doubleslash.client

import com.doubleslash.client.ui.DarkPalette
import com.doubleslash.client.ui.SKIN_PRESETS
import com.doubleslash.client.ui.TreeFold
import com.doubleslash.client.ui.TreeRow
import com.doubleslash.client.ui.RoomListOrder
import com.doubleslash.client.ui.buildRoomLeaves
import com.doubleslash.client.ui.buildRoomTree
import com.doubleslash.client.ui.canMoveRoom
import com.doubleslash.client.ui.moveRoomInOrder
import com.doubleslash.client.ui.parseRoomListOrder
import com.doubleslash.client.ui.toJson
import com.doubleslash.client.ui.roomOrderWithMode
import com.doubleslash.client.ui.withRoomPinToggled
import com.doubleslash.client.ui.paletteFor
import com.doubleslash.client.ui.parseSkin
import com.doubleslash.client.ui.parseSkinColor
import com.doubleslash.client.ui.presetIndexOf
import com.doubleslash.client.ui.skinToJson
import com.doubleslash.client.ui.toSkinHex
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The Rooms tree's rows, matching the desktop's (tests/qml/tst_RoomTree.qml),
 * and the skin format both clients share.
 */
class RoomTreeTest {

    private fun room(id: String, parent: String = "") =
        Room(roomId = id, roomName = id.replaceFirstChar { it.uppercase() }, supernodeId = "n", parentId = parent)

    private val rooms = listOf(room("lobby"), room("gaming"), room("raid", "gaming"), room("strat", "raid"))
    private val voice = mapOf(
        "n:lobby" to listOf("mara", "theo"),
        "n:raid" to listOf("sam"),
        "n:strat" to listOf("ade"),
    )
    private val text = mapOf("n:lobby" to listOf("mara", "jonah"))

    private fun kinds(rows: List<TreeRow>) = rows.map {
        when (it) {
            is TreeRow.RoomNode -> "room:${it.room.roomId}"
            is TreeRow.Leaf -> "leaf:${it.room.roomId}:${if (it.voice) "voice" else "text"}"
            is TreeRow.Member -> "member:${it.id}"
            is TreeRow.More -> "more"
        }
    }

    private fun tree(fold: TreeFold = TreeFold(), reading: Room? = null, voiceRoom: VoiceRoom? = null) =
        buildRoomTree(rooms, voice, text, reading, voiceRoom, fold)

    private fun open(vararg ids: String) = TreeFold(expanded = ids.map { "n/$it" }.toSet())

    @Test
    fun `an open room lists voice and text people before its sub-rooms`() {
        assertEquals(
            listOf(
                "room:gaming",
                "room:raid", "leaf:raid:voice", "member:sam",
                "room:strat", "leaf:strat:voice", "member:ade",
                "room:lobby", "leaf:lobby:voice", "member:mara", "member:theo",
                "leaf:lobby:text", "member:jonah",
            ),
            kinds(tree(open("gaming", "raid", "strat", "lobby"))),
        )
    }

    @Test
    fun `the room being read opens its text leaf`() {
        val rows = kinds(tree(open("lobby"), reading = rooms[0]))
        assertTrue("member:jonah" in rows)
        // Mara is in voice, so she is not listed again under text only.
        assertEquals(1, rows.count { it == "member:mara" })
    }

    @Test
    fun `folding a room keeps its sub-rooms and counts only its own people`() {
        val rows = tree(TreeFold(collapsed = setOf(rooms[2].key)))
        val names = kinds(rows)
        assertTrue("room:raid" in names)
        assertTrue("room:strat" in names)
        assertTrue("member:sam" !in names)
        assertTrue("member:ade" !in names)
        val raid = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertTrue(raid.collapsed)
        assertEquals(1, raid.subtreeVoice)
        assertEquals(listOf("sam"), raid.stack)
        val strat = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "strat" }
        assertEquals(1, strat.subtreeVoice)
    }

    @Test
    fun `guides follow the parent, not the grandparent`() {
        // Raid is Gaming's last child: nothing runs on in Raid's column below it.
        val rows = tree(open("gaming", "raid"))
        val byKind = kinds(rows).zip(rows).toMap()
        assertEquals(listOf(2), byKind.getValue("room:raid").guides)
        assertEquals(listOf(0, 3), byKind.getValue("leaf:raid:voice").guides)
        assertEquals(listOf(0, 1, 2), byKind.getValue("member:sam").guides)
        assertEquals(listOf(0, 2), byKind.getValue("room:strat").guides)
    }

    @Test
    fun `the voice room's row offers no call and its members are in session`() {
        val rows = tree(open("lobby"), voiceRoom = VoiceRoom("n", "raid", "Raid"))
        // The call icon is on the room row, as on the desktop, not on a leaf.
        val nodes = rows.filterIsInstance<TreeRow.RoomNode>().associateBy { it.room.roomId }
        assertTrue(!nodes.getValue("raid").showCall)
        assertTrue(nodes.getValue("lobby").showCall)
        val sam = rows.filterIsInstance<TreeRow.Member>().first { it.id == "sam" }
        assertTrue(sam.inSession && sam.canInvite)
        val mara = rows.filterIsInstance<TreeRow.Member>().first { it.id == "mara" }
        assertTrue(!mara.inSession && !mara.canInvite)
    }

    @Test
    fun `a busy leaf folds the rest under more`() {
        val many = (0 until 12).map { "p$it" }
        val big = listOf(room("big"))
        val opened = TreeFold(expanded = setOf("n/big"))
        val rows = buildRoomTree(big, mapOf("n:big" to many), emptyMap(), null, null, opened, limit = 5)
        assertEquals(4, rows.count { it is TreeRow.Member })
        assertEquals("+8 more", (rows.last() as TreeRow.More).label)
        val open = buildRoomTree(
            big, mapOf("n:big" to many), emptyMap(), null, null,
            opened.copy(overflow = setOf("n/big:voice")), limit = 5,
        )
        assertEquals(12, open.count { it is TreeRow.Member })
    }

    @Test
    fun `the members sheet is one room's leaves at the edge`() {
        val rows = buildRoomLeaves(rooms[0], voice, text, null, TreeFold())
        assertEquals(listOf("leaf:lobby:voice", "member:mara", "member:theo", "leaf:lobby:text", "member:jonah"), kinds(rows))
        // The leaves sit at the sheet's edge; their members hang off them.
        assertEquals(emptyList<Int>(), rows.first().guides)
        assertEquals(listOf(3), rows[1].guides)
    }

    @Test
    fun `rooms start collapsed and counts split voice from text`() {
        val rows = tree()
        assertEquals(listOf("room:gaming", "room:raid", "room:strat", "room:lobby"), kinds(rows))
        val gaming = rows[0] as TreeRow.RoomNode
        assertFalse(gaming.hasChildren)
        assertEquals(0, gaming.subtreeVoice)
        val raid = rows[1] as TreeRow.RoomNode
        assertTrue(raid.collapsed)
        assertEquals(1, raid.subtreeVoice)
        assertEquals(listOf("sam"), raid.stack)
        assertEquals(0, raid.roomChat)
        val strat = rows[2] as TreeRow.RoomNode
        assertTrue(strat.collapsed)
        assertEquals(1, strat.subtreeVoice)
        val lobby = rows[3] as TreeRow.RoomNode
        assertTrue(lobby.collapsed)
        assertEquals(2, lobby.subtreeVoice)
        assertEquals(1, lobby.roomChat)
    }

    @Test
    fun `the voice room shows its voice and text people together`() {
        val rows = buildRoomTree(
            rooms,
            voice,
            text + ("n:raid" to listOf("nia")),
            null,
            VoiceRoom("n", "raid", "Raid"),
            TreeFold(),
        )
        assertEquals(
            listOf(
                "room:gaming",
                "room:raid", "leaf:raid:voice", "member:sam",
                "leaf:raid:text", "member:nia",
                "room:strat",
                "room:lobby",
            ),
            kinds(rows),
        )
        val textLeaf = rows.filterIsInstance<TreeRow.Leaf>().first { !it.voice && it.room.roomId == "raid" }
        assertTrue(textLeaf.expanded)
        val strat = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "strat" }
        assertTrue(strat.collapsed)
        val lobby = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "lobby" }
        assertTrue(lobby.collapsed)
    }

    @Test
    fun `reading a room expands it until another is open`() {
        val lobby = kinds(tree(reading = rooms[0]))
        assertTrue("member:jonah" in lobby)
        assertTrue("member:mara" in lobby)
        assertTrue("room:raid" in lobby)
        assertTrue("member:sam" !in lobby)
        val shut = kinds(tree(TreeFold(collapsed = setOf(rooms[0].key)), reading = rooms[0]))
        assertTrue("member:jonah" !in shut)
        val raid = kinds(tree(reading = rooms[2]))
        assertTrue("member:sam" in raid)
        assertTrue("member:jonah" !in raid)
        assertTrue("room:strat" in raid)
    }

    @Test
    fun `an explicit collapse keeps the voice room shut`() {
        val rows = tree(TreeFold(collapsed = setOf(rooms[2].key)), voiceRoom = VoiceRoom("n", "raid", "Raid"))
        val names = kinds(rows)
        assertTrue("room:raid" in names)
        assertTrue("room:strat" in names)
        assertTrue("leaf:raid:voice" !in names)
    }

    @Test
    fun `a parent cycle still lists every room`() {
        val cycle = listOf(room("a", "b"), room("b", "a"))
        // Each is the other's child. Folded or open, neither is dropped.
        val rows = buildRoomTree(
            cycle, emptyMap(), emptyMap(), null, null,
            TreeFold(expanded = setOf("n/a", "n/b")),
        )
        assertEquals(2, rows.count { it is TreeRow.RoomNode })
        val folded = buildRoomTree(cycle, emptyMap(), emptyMap(), null, null, TreeFold())
        assertEquals(2, folded.count { it is TreeRow.RoomNode })
    }

    private fun roomIds(rows: List<TreeRow>) =
        rows.filterIsInstance<TreeRow.RoomNode>().map { it.room.roomId }

    private fun ordered(order: RoomListOrder) =
        buildRoomTree(rooms, voice, text, null, null, TreeFold(), order = order)

    @Test
    fun `name descending and peer counts reorder siblings`() {
        assertEquals(listOf("lobby", "gaming", "raid", "strat"), roomIds(ordered(RoomListOrder(mode = "name_desc"))))
        // Lobby has three people and Gaming has none, so the roots swap.
        assertEquals(listOf("lobby", "gaming", "raid", "strat"), roomIds(ordered(RoomListOrder(mode = "peers_desc"))))
        assertEquals(listOf("gaming", "raid", "strat", "lobby"), roomIds(ordered(RoomListOrder(mode = "peers_asc"))))
        val withChill = rooms + room("chill", "gaming")
        val voice3 = voice + ("n:chill" to listOf("a", "b", "c"))
        val ids = buildRoomTree(
            withChill, voice3, text, null, null, TreeFold(),
            order = RoomListOrder(mode = "peers_desc"),
        ).let(::roomIds)
        assertTrue(ids.indexOf("chill") < ids.indexOf("raid"))
        assertTrue(ids.indexOf("chill") > ids.indexOf("gaming"))
    }

    @Test
    fun `natural names, pins, and manual order match the desktop`() {
        val numbered = listOf(
            Room(roomId = "r10", roomName = "Room 10", supernodeId = "n"),
            Room(roomId = "r2", roomName = "Room 2", supernodeId = "n"),
            Room(roomId = "r1", roomName = "Room 1", supernodeId = "n"),
        )
        assertEquals(
            listOf("r1", "r2", "r10"),
            roomIds(buildRoomTree(numbered, emptyMap(), emptyMap(), null, null, TreeFold())),
        )
        val pinned = withRoomPinToggled(RoomListOrder(), "n:lobby")
        assertEquals(listOf("n:lobby"), pinned.pinned)
        assertEquals("lobby", roomIds(ordered(pinned)).first())
        val pinStrat = withRoomPinToggled(RoomListOrder(), "n:strat")
        assertEquals(listOf("gaming", "raid", "strat", "lobby"), roomIds(ordered(pinStrat)))
        assertFalse(canMoveRoom(pinned, rooms, rooms[0], 1, voice, text))

        val manual = roomOrderWithMode(RoomListOrder(), "manual", rooms, voice, text)
        assertEquals("manual", manual.mode)
        assertEquals(listOf("n:gaming", "n:raid", "n:strat", "n:lobby"), manual.manual)
        val kept = roomOrderWithMode(
            RoomListOrder(mode = "name_desc", manual = listOf("n:lobby")),
            "manual",
            rooms,
            voice,
            text,
        )
        assertEquals(listOf("n:lobby"), kept.manual)
        val moved = moveRoomInOrder(manual, rooms, rooms[0], -1, voice, text)
        assertTrue(moved.manual.indexOf("n:lobby") < moved.manual.indexOf("n:gaming"))
        assertTrue(canMoveRoom(manual, rooms, rooms[0], -1, voice, text))
        assertFalse(canMoveRoom(manual, rooms, rooms[1], -1, voice, text))

        val both = withRoomPinToggled(withRoomPinToggled(RoomListOrder(), "n:gaming"), "n:lobby")
        assertEquals("n:lobby", both.pinned.first())
        val swapped = moveRoomInOrder(both, rooms, rooms[1], -1, voice, text)
        assertEquals("n:gaming", swapped.pinned.first())
        assertEquals("name_asc", parseRoomListOrder("nope").mode)
        assertEquals(listOf("n:lobby"), parseRoomListOrder(pinned.toJson()).pinned)
    }

    // ── Skins ───────────────────────────────────────────────────────────────

    @Test
    fun `garbage is not a skin`() {
        assertNull(parseSkin(""))
        assertNull(parseSkin("not json"))
        assertNull(parseSkin("{\"name\":\"x\"}"))
    }

    @Test
    fun `unknown tokens and bad colours are dropped, the rest kept`() {
        val skin = parseSkin(
            """{"v":1,"name":"Mixed","base":"dark","colors":{"accent":"#12ab34","bg0":"red","wobble":"#FFFFFF"}}""",
        )!!
        assertEquals("Mixed", skin.name)
        assertEquals("dark", skin.base)
        assertEquals(setOf("accent"), skin.colors.keys)
        assertEquals("#12AB34", skin.colors.getValue("accent").toSkinHex())
    }

    @Test
    fun `a skin overrides only what it names`() {
        val palette = paletteFor(true, """{"v":1,"name":"x","base":"","colors":{"accent":"#FF8800"}}""")
        assertEquals("#FF8800", palette.accent.toSkinHex())
        assertEquals(DarkPalette.bg1, palette.bg1)
    }

    @Test
    fun `presets round-trip and are recognised`() {
        SKIN_PRESETS.forEachIndexed { i, preset ->
            val json = skinToJson(preset.toSkin())
            assertEquals(preset.colors.size, parseSkin(json)!!.colors.size)
            if (preset.colors.isNotEmpty()) assertEquals(i, presetIndexOf(json))
        }
        assertEquals(0, presetIndexOf(""))
    }

    @Test
    fun `colours read and write in both forms`() {
        assertEquals("#5865F2", parseSkinColor("#5865f2")!!.toSkinHex())
        assertEquals("#805865F2", parseSkinColor("#805865F2")!!.toSkinHex())
        assertNull(parseSkinColor("#12345"))
    }
}
