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

    /** `leaves[n/<id>:voice|text] = true` is the only way that list opens. */
    private fun shown(vararg leaves: String) = TreeFold(leaves = leaves.associate { "n/$it" to true })

    @Test
    fun `an open list is one box in its count colour`() {
        // As on the desktop: the leaf row starts the box, every row under it
        // carries it at the leaf's column, and the last row ends it.
        val rows = tree(shown("lobby:voice", "lobby:text"))
        val voiceLeaf = rows.filterIsInstance<TreeRow.Leaf>().first { it.room.roomId == "lobby" && it.voice }
        val voiceBox = voiceLeaf.box!!
        assertTrue(voiceBox.voice && voiceBox.start && !voiceBox.end)
        val voiceMembers = rows.filterIsInstance<TreeRow.Member>().filter { it.room.roomId == "lobby" && it.voice }
        assertEquals(listOf("mara", "theo"), voiceMembers.map { it.id })
        assertTrue(voiceMembers.all { it.box?.voice == true && it.box?.depth == voiceBox.depth })
        assertEquals(listOf(false, true), voiceMembers.map { it.box!!.end })
        // Text gets its own box, starting again at its leaf.
        val textLeaf = rows.filterIsInstance<TreeRow.Leaf>().first { it.room.roomId == "lobby" && !it.voice }
        assertTrue(textLeaf.box!!.start && !textLeaf.box!!.voice)
        val jonah = rows.filterIsInstance<TreeRow.Member>().first { it.id == "jonah" }
        assertTrue(jonah.box!!.end && !jonah.box!!.voice)
        // A shut voice list is absent, so it draws no box. Text stays.
        val shut = tree(shown("lobby:text"))
        assertTrue(shut.none { it is TreeRow.Leaf && it.voice && it.room.roomId == "lobby" })
        val lobby = shut.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "lobby" }
        assertEquals(2, lobby.voiceCount)
        assertTrue(lobby.collapsed)
        // The members sheet drops a column, and the box moves with it.
        val sheet = buildRoomLeaves(room("lobby"), voice, text, null, TreeFold())
        assertEquals(0, sheet.filterIsInstance<TreeRow.Leaf>().first().box!!.depth)
    }

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
            kinds(tree(shown("raid:voice", "strat:voice", "lobby:voice", "lobby:text"))),
        )
    }

    @Test
    fun `the text flag opens text members and leaves the voice list shut`() {
        val closed = kinds(tree(reading = rooms[0]))
        assertTrue("member:jonah" !in closed)
        assertTrue("member:mara" !in closed)
        val rows = kinds(tree(shown("lobby:text"), reading = rooms[0]))
        assertTrue("member:jonah" in rows)
        // Mara is in voice, so she is not listed again under text only.
        assertEquals(0, rows.count { it == "member:mara" })
        val lobby = tree(shown("lobby:text")).filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "lobby" }
        assertEquals(0, lobby.roomChat)
        assertEquals(2, lobby.voiceCount)
        assertTrue(lobby.collapsed)
    }

    @Test
    fun `a shut voice list keeps its sub-rooms and counts only its own people`() {
        val rows = tree()
        val names = kinds(rows)
        assertTrue("room:raid" in names)
        assertTrue("room:strat" in names)
        assertTrue("member:sam" !in names)
        assertTrue("member:ade" !in names)
        val raid = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertTrue(raid.collapsed)
        assertEquals(1, raid.voiceCount)
        val strat = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "strat" }
        assertEquals(1, strat.voiceCount)
    }

    @Test
    fun `guides follow the parent, not the grandparent`() {
        // Raid is Gaming's last child: nothing runs on in Raid's column below it.
        val rows = tree(shown("raid:voice"))
        val byKind = kinds(rows).zip(rows).toMap()
        assertEquals(listOf(2), byKind.getValue("room:raid").guides)
        assertEquals(listOf(0, 3), byKind.getValue("leaf:raid:voice").guides)
        assertEquals(listOf(0, 1, 2), byKind.getValue("member:sam").guides)
        assertEquals(listOf(0, 2), byKind.getValue("room:strat").guides)
    }

    @Test
    fun `the voice room's row hangs up and its members are in session`() {
        val rows = tree(shown("lobby:voice", "raid:voice"), voiceRoom = VoiceRoom("n", "raid", "Raid"))
        // The call icon is on the room row, as on the desktop, not on a leaf.
        // In the voice room we are in it ends the call; elsewhere it joins.
        val nodes = rows.filterIsInstance<TreeRow.RoomNode>().associateBy { it.room.roomId }
        assertTrue(nodes.getValue("raid").showCall && nodes.getValue("raid").inVoice)
        assertTrue(nodes.getValue("lobby").showCall && !nodes.getValue("lobby").inVoice)
        val sam = rows.filterIsInstance<TreeRow.Member>().first { it.id == "sam" }
        assertTrue(sam.inSession && sam.canInvite)
        val mara = rows.filterIsInstance<TreeRow.Member>().first { it.id == "mara" }
        assertTrue(!mara.inSession && !mara.canInvite)
    }

    @Test
    fun `a busy leaf folds the rest under more`() {
        val many = (0 until 12).map { "p$it" }
        val big = listOf(room("big"))
        val opened = shown("big:voice")
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
        assertEquals(0, gaming.voiceCount)
        val raid = rows[1] as TreeRow.RoomNode
        assertTrue(raid.collapsed)
        assertEquals(1, raid.voiceCount)
        assertEquals(0, raid.roomChat)
        val strat = rows[2] as TreeRow.RoomNode
        assertTrue(strat.collapsed)
        assertEquals(1, strat.voiceCount)
        val lobby = rows[3] as TreeRow.RoomNode
        assertTrue(lobby.collapsed)
        assertEquals(2, lobby.voiceCount)
        assertEquals(1, lobby.roomChat)
    }

    @Test
    fun `the voice room stays shut until its voice flag opens it`() {
        val withNia = text + ("n:raid" to listOf("nia"))
        val rows = buildRoomTree(
            rooms, voice, withNia, null, VoiceRoom("n", "raid", "Raid"), TreeFold(),
        )
        assertEquals(listOf("room:gaming", "room:raid", "room:strat", "room:lobby"), kinds(rows))
        val raid = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertTrue(raid.collapsed && raid.inVoice)
        assertEquals(1, raid.voiceCount)
        assertEquals(1, raid.roomChat)
        val textOnly = buildRoomTree(
            rooms, voice, withNia, null, VoiceRoom("n", "raid", "Raid"), shown("raid:text"),
        )
        assertTrue("member:nia" in kinds(textOnly))
        assertFalse("member:sam" in kinds(textOnly))
        assertTrue("leaf:raid:voice" !in kinds(textOnly))
        val voiceOpen = buildRoomTree(
            rooms, voice, withNia, null, VoiceRoom("n", "raid", "Raid"), shown("raid:voice"),
        )
        assertEquals(
            listOf(
                "room:gaming",
                "room:raid", "leaf:raid:voice", "member:sam",
                "room:strat",
                "room:lobby",
            ),
            kinds(voiceOpen),
        )
        val openRaid = voiceOpen.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertFalse(openRaid.collapsed)
        assertEquals(1, openRaid.voiceCount)
        assertEquals(1, openRaid.roomChat)
    }

    @Test
    fun `in voice with nobody text-only the text count lists who is in the room`() {
        val rows = tree(voiceRoom = VoiceRoom("n", "raid", "Raid"))
        val raid = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertEquals(1, raid.roomChat)
        assertTrue(raid.textListsRoom)
        assertEquals(1, raid.voiceCount)
        val lobby = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "lobby" }
        assertEquals(1, lobby.roomChat)
        assertFalse(lobby.textListsRoom)
        val strat = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "strat" }
        assertEquals(0, strat.roomChat)
        val open = tree(shown("raid:text"), voiceRoom = VoiceRoom("n", "raid", "Raid"))
        assertTrue("member:sam" in kinds(open))
        assertTrue("leaf:raid:voice" !in kinds(open))
        val textLeaf = open.filterIsInstance<TreeRow.Leaf>().first { it.room.roomId == "raid" && !it.voice }
        assertTrue(textLeaf.listsRoom)
        val openRaid = open.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertEquals(0, openRaid.roomChat)
    }

    @Test
    fun `selecting a room does not open its lists`() {
        val lobby = kinds(tree(reading = rooms[0]))
        assertTrue("member:jonah" !in lobby)
        assertTrue("member:mara" !in lobby)
        assertTrue("room:raid" in lobby)
        assertTrue("member:sam" !in lobby)
        val raid = kinds(tree(reading = rooms[2]))
        assertTrue("member:sam" !in raid)
        assertTrue("room:strat" in raid)
        val node = tree(reading = rooms[2]).filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertTrue(node.collapsed)
        assertEquals(1, node.voiceCount)
    }

    @Test
    fun `being in a voice room does not open its list`() {
        val rows = tree(voiceRoom = VoiceRoom("n", "raid", "Raid"))
        val names = kinds(rows)
        assertTrue("room:raid" in names)
        assertTrue("room:strat" in names)
        assertTrue("leaf:raid:voice" !in names)
        val raid = rows.filterIsInstance<TreeRow.RoomNode>().first { it.room.roomId == "raid" }
        assertTrue(raid.collapsed && raid.inVoice && raid.showCall)
    }

    @Test
    fun `a parent cycle still lists every room`() {
        val cycle = listOf(room("a", "b"), room("b", "a"))
        // Each is the other's child. Folded or open, neither is dropped.
        val rows = buildRoomTree(cycle, emptyMap(), emptyMap(), null, null, TreeFold())
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
