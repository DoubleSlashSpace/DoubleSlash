import QtQuick
import QtTest
import DoubleSlash.Client 1.0

// The Rooms tree's rows: voice and text lists, the live voice session, and
// the connector columns that draw the tree lines. A list appears only when
// its leaf flag is true. Selecting a room or being in its voice does neither.
TestCase {
    name: "RoomTree"

    function member(id, extra) {
        var m = { id: id, name: id, trusted: false, list_peer_id: "", is_self: false }
        for (var k in extra || {}) m[k] = extra[k]
        return m
    }

    function ctx(extra) {
        var c = {
            collapsed: {}, leaves: {}, overflow: {},
            selectedNode: "", selectedRoom: "",
            voiceNode: "", voiceRoom: "",
            limit: 8
        }
        for (var k in extra || {}) c[k] = extra[k]
        return c
    }

    // `leaves[n:<id>:voice|text] === true` is the only way that list opens.
    function shown() {
        var leaves = {}
        for (var i = 0; i < arguments.length; i++)
            leaves["n:" + arguments[i]] = true
        return leaves
    }

    function kinds(rows) {
        return rows.map(function (r) {
            if (r.row_kind === "room") return "room:" + r.room_id
            if (r.row_kind === "group") return "group:" + r.room_id + ":" + r.group
            if (r.row_kind === "member") return "member:" + r.id
            if (r.row_kind === "session") return "session:" + r.room_id
            return r.row_kind
        })
    }

    // Lobby (root, voice + text), Gaming (root) > Raid (voice) > Strategy.
    function sample() {
        return [
            { room_id: "lobby", name: "Lobby", parent_id: "",
              voice_members: [member("mara"), member("theo")],
              text_members: [member("jonah")] },
            { room_id: "gaming", name: "Gaming", parent_id: "",
              voice_members: [], text_members: [] },
            { room_id: "raid", name: "Raid", parent_id: "gaming",
              voice_members: [member("sam")], text_members: [] },
            { room_id: "strat", name: "Strategy", parent_id: "raid",
              voice_members: [member("ade")] }
        ]
    }

    function test_leaves_come_before_sub_rooms() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ leaves: shown("lobby:voice", "raid:voice",
                                                     "strat:voice", "lobby:text") }))
        compare(kinds(rows), [
            "room:gaming",
            "room:raid", "group:raid:voice", "member:sam",
            "room:strat", "group:strat:voice", "member:ade",
            "room:lobby", "group:lobby:voice", "member:mara", "member:theo",
            "group:lobby:text", "member:jonah"
        ])
    }

    function test_text_members_stay_shut_until_their_flag_opens_them() {
        // Selecting the room opens neither list. Both counts stay on the row.
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        var k = kinds(rows)
        verify(k.indexOf("member:mara") === -1)
        verify(k.indexOf("member:jonah") === -1)
        compare(rows[k.indexOf("room:lobby")].room_chat, 1)
        compare(rows[k.indexOf("room:lobby")].voice_count, 2)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        // The voice flag lists voice people and keeps the count, so it can close.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ leaves: shown("lobby:voice") }))
        k = kinds(rows)
        verify(k.indexOf("member:mara") !== -1)
        verify(k.indexOf("member:jonah") === -1)
        var lobby = rows[k.indexOf("room:lobby")]
        compare(lobby.room_chat, 1)
        compare(lobby.voice_count, 2)
        compare(lobby.subtree_voice, 0)
        verify(!lobby.collapsed)
        // The text count's flag lists them, even while the voice list stays shut.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ leaves: shown("lobby:text") }))
        k = kinds(rows)
        verify(k.indexOf("member:jonah") !== -1)
        verify(k.indexOf("member:mara") === -1)
        compare(rows[k.indexOf("room:lobby")].room_chat, 0)
        compare(rows[k.indexOf("room:lobby")].voice_count, 2)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        // Another node's chat does not open this room.
        rows = RoomTree.rows(sample(), "other", true,
                             ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        verify(kinds(rows).indexOf("member:jonah") === -1)
    }

    function test_selecting_a_room_does_not_open_its_lists() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        var k = kinds(rows)
        verify(k.indexOf("member:jonah") === -1)
        verify(k.indexOf("member:mara") === -1)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        // A stored false is the same as no flag: the list stays shut.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ selectedNode: "n", selectedRoom: "lobby",
                                   leaves: { "n:lobby:voice": false } }))
        verify(kinds(rows).indexOf("member:mara") === -1)
        verify(kinds(rows).indexOf("member:jonah") === -1)
        // Opening another room's chat does not open its voice list either.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ selectedNode: "n", selectedRoom: "raid" }))
        k = kinds(rows)
        verify(k.indexOf("member:jonah") === -1)
        verify(k.indexOf("member:sam") === -1)
        verify(k.indexOf("room:strat") !== -1)
        verify(rows[k.indexOf("room:raid")].collapsed)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
    }

    function test_voice_flag_toggles_and_text_flag_stays_independent() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ leaves: shown("lobby:voice", "lobby:text") }))
        var k = kinds(rows)
        verify(k.indexOf("member:mara") !== -1)
        verify(k.indexOf("member:jonah") !== -1)
        compare(rows[k.indexOf("room:lobby")].voice_count, 2)
        verify(!rows[k.indexOf("room:lobby")].collapsed)
        // Closing the voice list leaves the text list where it was.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ leaves: { "n:lobby:voice": false, "n:lobby:text": true } }))
        k = kinds(rows)
        verify(k.indexOf("member:mara") === -1)
        verify(k.indexOf("member:jonah") !== -1)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        compare(rows[k.indexOf("room:lobby")].voice_count, 2)
    }

    // An open list is one box in the colour of the count box that opened it:
    // the leaf row starts it, every row under it carries it at the leaf's
    // column, and the last row ends it.
    function test_open_list_is_one_box_in_its_count_colour() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ leaves: shown("lobby:voice", "lobby:text") }))
        var k = kinds(rows)
        var voice = rows[k.indexOf("group:lobby:voice")]
        compare(voice.box, "voice")
        verify(voice.box_start)
        verify(!voice.box_end)
        var mara = rows[k.indexOf("member:mara")]
        compare(mara.box, "voice")
        compare(mara.box_depth, voice.box_depth)
        verify(!mara.box_start)
        verify(!mara.box_end)
        verify(rows[k.indexOf("member:theo")].box_end)
        // Text gets its own box, which starts again at its leaf.
        var text = rows[k.indexOf("group:lobby:text")]
        compare(text.box, "text")
        verify(text.box_start)
        compare(rows[k.indexOf("member:jonah")].box, "text")
        verify(rows[k.indexOf("member:jonah")].box_end)
        // Rooms are never inside a box.
        verify(!rows[k.indexOf("room:lobby")].box)

        // The live session stands for the voice members, so it ends that box.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid", leaves: shown("raid:voice") }))
        k = kinds(rows)
        compare(rows[k.indexOf("session:raid")].box, "voice")
        verify(rows[k.indexOf("session:raid")].box_end)

        // A folded list draws no box; "+N more" ends one that overflows.
        var many = []
        for (var i = 0; i < 12; i++) many.push(member("p" + i))
        rows = RoomTree.rows([{ room_id: "big", name: "Big", parent_id: "", voice_members: many }],
                             "n", true, ctx({ limit: 5, leaves: shown("big:voice") }))
        var last = rows[rows.length - 1]
        compare(last.row_kind, "more")
        compare(last.box, "voice")
        verify(last.box_end)
        rows.forEach(function (r) {
            if (r.row_kind === "member")
                verify(!r.box_end, r.id)
        })
    }

    function test_shut_voice_list_keeps_sub_rooms_and_its_own_count() {
        var rows = RoomTree.rows(sample(), "n", true, ctx())
        var k = kinds(rows)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("room:strat") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(k.indexOf("member:ade") === -1)
        var raid = rows[k.indexOf("room:raid")]
        verify(raid.collapsed)
        compare(raid.subtree_voice, 1)
        compare(raid.voice_count, 1)
        compare(raid.stack_ids, ["sam"])
        // Ade counts on Strategy, not on Raid.
        compare(rows[k.indexOf("room:strat")].subtree_voice, 1)
        compare(rows[k.indexOf("room:strat")].voice_count, 1)
    }

    function test_roster_fields_keep_a_known_list_and_omit_an_unknown_one() {
        var kept = RoomTree.rosterFields({
            voice_members: [member("sam", { is_self: true })],
            text_members: []
        })
        compare(kept.voice_members.length, 1)
        compare(kept.voice_members[0].id, "sam")
        verify(kept.voice_members[0].is_self)
        compare(kept.text_members.length, 0)
        var unknown = RoomTree.rosterFields({ room_id: "lobby", voice_count: 1 })
        verify(!unknown.hasOwnProperty("voice_members"))
        verify(!unknown.hasOwnProperty("text_members"))
        compare(Object.keys(RoomTree.rosterFields(null)).length, 0)
    }

    function test_session_row_stands_in_for_the_voice_room_members() {
        // Being in the room does not open its list. The row still offers a call.
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid" }))
        var k = kinds(rows)
        verify(k.indexOf("session:raid") === -1)
        verify(rows[k.indexOf("room:raid")].collapsed)
        verify(rows[k.indexOf("room:raid")].show_call)
        verify(rows[k.indexOf("room:raid")].voice_here)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid",
                                   leaves: shown("raid:voice", "lobby:voice") }))
        k = kinds(rows)
        verify(k.indexOf("session:raid") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(k.indexOf("member:mara") !== -1)
        // The call icon sits on the room row. In the voice room we are in it
        // ends the call; anywhere else it joins.
        verify(rows[k.indexOf("room:raid")].show_call)
        verify(rows[k.indexOf("room:raid")].voice_here)
        verify(!rows[k.indexOf("room:raid")].collapsed)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
        verify(!rows[k.indexOf("room:lobby")].voice_here)
        verify(rows[k.indexOf("room:lobby")].show_call)
        verify(!rows[k.indexOf("group:lobby:voice")].hasOwnProperty("show_join"))
    }

    function test_rooms_start_collapsed_with_voice_and_text_counts() {
        var rows = RoomTree.rows(sample(), "n", true, ctx())
        compare(kinds(rows), ["room:gaming", "room:raid", "room:strat", "room:lobby"])
        function room(id) {
            for (var i = 0; i < rows.length; i++)
                if (rows[i].row_kind === "room" && rows[i].room_id === id)
                    return rows[i]
            return null
        }
        var lobby = room("lobby")
        verify(lobby.collapsed)
        compare(lobby.subtree_voice, 2)
        compare(lobby.voice_count, 2)
        compare(lobby.room_chat, 1)
        compare(lobby.stack_ids, ["mara", "theo"])
        // Gaming has nobody of its own: it is the row its sub-rooms hang from.
        var gaming = room("gaming")
        verify(!gaming.has_children)
        compare(gaming.subtree_voice, 0)
        compare(gaming.voice_count, 0)
        var raid = room("raid")
        verify(raid.collapsed)
        compare(raid.subtree_voice, 1)
        compare(raid.voice_count, 1)
        compare(raid.room_chat, 0)
        var strat = room("strat")
        verify(strat.collapsed)
        compare(strat.subtree_voice, 1)
        compare(strat.voice_count, 1)
    }

    function test_voice_room_stays_shut_until_its_voice_flag_opens_it() {
        var rooms = sample()
        rooms[2].text_members = [member("nia")]
        var rows = RoomTree.rows(rooms, "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid" }))
        compare(kinds(rows), [
            "room:gaming", "room:raid", "room:strat", "room:lobby"
        ])
        var k = kinds(rows)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        verify(rows[k.indexOf("room:raid")].collapsed)
        verify(rows[k.indexOf("room:strat")].collapsed)
        verify(rows[k.indexOf("room:raid")].voice_here)
        verify(rows[k.indexOf("room:lobby")].show_call)
        compare(rows[k.indexOf("room:raid")].room_chat, 1)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
        verify(k.indexOf("member:nia") === -1)
        verify(k.indexOf("member:ade") === -1)
        verify(k.indexOf("session:raid") === -1)
        // The text count adds those people and leaves the voice list shut.
        rows = RoomTree.rows(rooms, "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid",
                                   leaves: shown("raid:text") }))
        k = kinds(rows)
        verify(k.indexOf("member:nia") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(k.indexOf("session:raid") === -1)
        compare(rows[k.indexOf("room:raid")].room_chat, 0)
        verify(rows[k.indexOf("room:raid")].collapsed)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
        // The voice flag is what shows the live session, and the count stays.
        rows = RoomTree.rows(rooms, "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid",
                                   leaves: shown("raid:voice") }))
        compare(kinds(rows), [
            "room:gaming",
            "room:raid", "group:raid:voice", "session:raid",
            "room:strat",
            "room:lobby"
        ])
        k = kinds(rows)
        verify(!rows[k.indexOf("room:raid")].collapsed)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
        compare(rows[k.indexOf("room:raid")].subtree_voice, 0)
        compare(rows[k.indexOf("room:raid")].room_chat, 1)
    }

    function test_old_collapse_flag_does_not_open_the_voice_room() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid",
                                       collapsed: { "n:raid": false } }))
        var k = kinds(rows)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("room:strat") !== -1)
        verify(k.indexOf("session:raid") === -1)
        verify(rows[k.indexOf("room:raid")].collapsed)
        verify(rows[k.indexOf("room:raid")].voice_here)
        compare(rows[k.indexOf("room:raid")].voice_count, 1)
    }

    function test_invites_only_from_rooms_we_are_in() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ leaves: shown("lobby:voice", "strat:voice"),
                                       selectedNode: "n", selectedRoom: "lobby" }))
        var k = kinds(rows)
        verify(rows[k.indexOf("member:mara")].can_invite)
        verify(!rows[k.indexOf("member:ade")].can_invite)
    }

    function test_offline_node_draws_no_leaves() {
        var rows = RoomTree.rows(sample(), "n", false, ctx())
        kinds(rows).forEach(function (k) { verify(k.indexOf("room:") === 0, k) })
        // Nor any call control: there is no node to join or leave through.
        rows.forEach(function (r) { verify(!r.show_call, r.room_id) })
    }

    function test_busy_leaf_folds_into_more() {
        var many = []
        for (var i = 0; i < 12; i++) many.push(member("p" + i))
        var rooms = [{ room_id: "big", name: "Big", parent_id: "", voice_members: many }]
        var rows = RoomTree.rows(rooms, "n", true,
                                 ctx({ limit: 5, leaves: shown("big:voice") }))
        var members = rows.filter(function (r) { return r.row_kind === "member" })
        compare(members.length, 4)
        compare(rows[rows.length - 1].row_kind, "more")
        compare(rows[rows.length - 1].label, "+8 more")
        // The count stays at the full roster while the leaf is folded to a few.
        compare(rows[0].voice_count, 12)
        rows = RoomTree.rows(rooms, "n", true,
                             ctx({ limit: 5, leaves: shown("big:voice"),
                                   overflow: { "n:big:voice": true } }))
        compare(rows.filter(function (r) { return r.row_kind === "member" }).length, 12)
        compare(rows[rows.length - 1].label, "Show fewer")
    }

    function test_guides_follow_the_parent_not_the_grandparent() {
        // Gaming is not the last root, but Raid is Gaming's last child, so the
        // column Raid's line runs in must be blank below Raid — the bug the old
        // builder had, which read the grandparent's siblings instead.
        var rooms = [
            { room_id: "gaming", name: "Gaming", parent_id: "" },
            { room_id: "raid", name: "Raid", parent_id: "gaming" },
            { room_id: "strat", name: "Strategy", parent_id: "raid" },
            { room_id: "zzz", name: "Zzz", parent_id: "" }
        ]
        var rows = RoomTree.rows(rooms, "n", true, ctx())
        var k = kinds(rows)
        compare(rows[k.indexOf("room:raid")].guide_cols, [2])
        compare(rows[k.indexOf("room:strat")].guide_cols, [0, 2])
    }

    function test_member_guides_run_under_their_room() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ leaves: shown("raid:voice") }))
        var k = kinds(rows)
        // Raid is Gaming's last child, so nothing runs on in Raid's column
        // (0). Under Raid: the Voice leaf, then Strategy — the leaf is ├, and
        // the leaf's column carries on past Sam to reach Strategy.
        compare(rows[k.indexOf("group:raid:voice")].guide_cols, [0, 3])
        compare(rows[k.indexOf("member:sam")].guide_cols, [0, 1, 2])
    }

    function test_parent_cycle_still_lists_every_room() {
        var rooms = [
            { room_id: "a", name: "A", parent_id: "b" },
            { room_id: "b", name: "B", parent_id: "a" }
        ]
        // Each is the other's child. Neither is dropped.
        var rows = RoomTree.rows(rooms, "n", true, ctx())
        compare(rows.filter(function (r) { return r.row_kind === "room" }).length, 2)
    }

    function roomIds(rows) {
        return rows.filter(function (r) { return r.row_kind === "room" })
            .map(function (r) { return r.room_id })
    }

    function test_sort_by_name_and_by_people() {
        var desc = RoomTree.rows(sample(), "n", true, ctx({ order: { mode: "name_desc" } }))
        compare(roomIds(desc), ["lobby", "gaming", "raid", "strat"])
        // Lobby has three people and Gaming has none, so the roots swap.
        var most = RoomTree.rows(sample(), "n", true, ctx({ order: { mode: "peers_desc" } }))
        compare(roomIds(most), ["lobby", "gaming", "raid", "strat"])
        var fewest = RoomTree.rows(sample(), "n", true, ctx({ order: { mode: "peers_asc" } }))
        compare(roomIds(fewest), ["gaming", "raid", "strat", "lobby"])
        var rooms = sample()
        rooms.push({
            room_id: "chill", name: "Chill", parent_id: "gaming",
            voice_members: [member("a"), member("b"), member("c")]
        })
        var under = RoomTree.rows(rooms, "n", true, ctx({ order: { mode: "peers_desc" } }))
        var ids = roomIds(under)
        verify(ids.indexOf("chill") < ids.indexOf("raid"))
        verify(ids.indexOf("chill") > ids.indexOf("gaming"))
    }

    function test_natural_order_pins_and_manual() {
        var numbered = [
            { room_id: "r10", name: "Room 10", parent_id: "" },
            { room_id: "r2", name: "Room 2", parent_id: "" },
            { room_id: "r1", name: "Room 1", parent_id: "" }
        ]
        compare(roomIds(RoomTree.rows(numbered, "n", true, ctx())), ["r1", "r2", "r10"])

        var pinned = RoomTree.togglePin({ mode: "name_asc" }, "n:lobby")
        compare(pinned.pinned, ["n:lobby"])
        compare(roomIds(RoomTree.rows(sample(), "n", true, ctx({ order: pinned })))[0], "lobby")
        // A pinned sub-room stays under its parent.
        var pinStrat = RoomTree.togglePin({}, "n:strat")
        compare(roomIds(RoomTree.rows(sample(), "n", true, ctx({ order: pinStrat }))),
                ["gaming", "raid", "strat", "lobby"])
        compare(RoomTree.canMove(sample(), "n", "lobby", 1, pinned), false)

        var manual = RoomTree.withMode({}, "manual", [{ nodeId: "n", rooms: sample() }])
        compare(manual.mode, "manual")
        compare(manual.manual, ["n:gaming", "n:raid", "n:strat", "n:lobby"])
        var kept = RoomTree.withMode(
            { mode: "name_desc", manual: ["n:lobby"] }, "manual",
            [{ nodeId: "n", rooms: sample() }])
        compare(kept.manual, ["n:lobby"])
        var moved = RoomTree.moveRoom(sample(), "n", "lobby", -1, manual)
        verify(moved.manual.indexOf("n:lobby") < moved.manual.indexOf("n:gaming"))
        compare(RoomTree.canMove(sample(), "n", "lobby", -1, manual), true)
        compare(RoomTree.canMove(sample(), "n", "gaming", -1, manual), false)

        var both = RoomTree.togglePin(RoomTree.togglePin({}, "n:gaming"), "n:lobby")
        // The room pinned last is first.
        compare(both.pinned[0], "n:lobby")
        var swapped = RoomTree.moveRoom(sample(), "n", "gaming", -1, both)
        compare(swapped.pinned[0], "n:gaming")
    }

    function test_voice_room_keeps_a_text_count_of_who_is_here() {
        // Raid's text list is empty. Being in its voice must not drop the
        // text count: that count is how the row opens onto who is here, and
        // it lists the voice roster. Lobby still lists only Jonah.
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid" }))
        var k = kinds(rows)
        var raid = rows[k.indexOf("room:raid")]
        compare(raid.room_chat, 1)
        verify(raid.text_lists_room)
        compare(raid.voice_count, 1)
        compare(rows[k.indexOf("room:lobby")].room_chat, 1)
        verify(!rows[k.indexOf("room:lobby")].text_lists_room)
        compare(rows[k.indexOf("room:gaming")].room_chat, 0)
        compare(rows[k.indexOf("room:strat")].room_chat, 0)
        verify(k.indexOf("member:sam") === -1)
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid",
                                   leaves: shown("raid:text") }))
        k = kinds(rows)
        verify(k.indexOf("member:sam") !== -1)
        verify(k.indexOf("session:raid") === -1)
        compare(rows[k.indexOf("group:raid:text")].room_roster, true)
        compare(rows[k.indexOf("room:raid")].room_chat, 0)
        // A real text-only member stays the text list. Sam is not added to it.
        var rooms = sample()
        rooms[2].text_members = [member("nia")]
        rows = RoomTree.rows(rooms, "n", true,
                             ctx({ voiceNode: "n", voiceRoom: "raid",
                                   leaves: shown("raid:text") }))
        k = kinds(rows)
        verify(k.indexOf("member:nia") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(!rows[k.indexOf("room:raid")].text_lists_room)
        verify(!rows[k.indexOf("group:raid:text")].room_roster)
    }

    function test_roster_counts_follow_the_member_lists() {
        var counts = RoomTree.rosterCounts({
            voice_members: [member("a"), member("b")],
            text_members: [member("c")]
        })
        compare(counts.voice, 2)
        compare(counts.text, 1)
        compare(counts.voiceKnown, true)
        compare(counts.textKnown, true)

        var empty = RoomTree.rosterCounts({ voice_members: [], text_members: [] })
        compare(empty.voice, 0)
        compare(empty.text, 0)
        compare(empty.voiceKnown, true)
        compare(empty.textKnown, true)

        var unknown = RoomTree.rosterCounts({})
        compare(unknown.voice, 0)
        compare(unknown.text, 0)
        compare(unknown.voiceKnown, false)
        compare(unknown.textKnown, false)
        compare(RoomTree.rosterCounts(null).voiceKnown, false)

        // In this room's voice, an empty text list keeps a count of who is here.
        var here = RoomTree.rosterCounts({
            voice_members: [member("sam")],
            text_members: []
        }, true)
        compare(here.text, 1)
        verify(here.textListsRoom)
        var away = RoomTree.rosterCounts({
            voice_members: [member("sam")],
            text_members: []
        }, false)
        compare(away.text, 0)
        verify(!away.textListsRoom)
        var nia = RoomTree.rosterCounts({
            voice_members: [member("sam")],
            text_members: [member("nia")]
        }, true)
        compare(nia.text, 1)
        verify(!nia.textListsRoom)
    }
}
