import QtQuick
import QtTest
import DoubleSlash.Client 1.0

// The Rooms tree's rows: leaves under each room, fold state, the live voice
// session, and the connector columns that draw the tree lines.
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

    // `collapsed[key] === false` is an explicit open. Rooms otherwise start shut.
    function opened() {
        var collapsed = {}
        for (var i = 0; i < arguments.length; i++)
            collapsed["n:" + arguments[i]] = false
        return collapsed
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
                                 ctx({ collapsed: opened("lobby", "gaming", "raid", "strat") }))
        compare(kinds(rows), [
            "room:gaming",
            "room:raid", "group:raid:voice", "member:sam",
            "room:strat", "group:strat:voice", "member:ade",
            "room:lobby", "group:lobby:voice", "member:mara", "member:theo",
            "group:lobby:text", "member:jonah"
        ])
    }

    function test_text_leaf_opens_for_the_room_being_read() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ collapsed: opened("lobby"),
                                       selectedNode: "n", selectedRoom: "lobby" }))
        verify(kinds(rows).indexOf("member:jonah") !== -1)
        // Opening the chat expands that room on its own.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        verify(kinds(rows).indexOf("member:jonah") !== -1)
        // An open room lists its text people even when the chat on screen
        // belongs to another node.
        rows = RoomTree.rows(sample(), "other", true,
                             ctx({ collapsed: { "other:lobby": false },
                                   selectedNode: "n", selectedRoom: "lobby" }))
        verify(kinds(rows).indexOf("member:jonah") !== -1)
        // That other node's chat does not open this node's room.
        rows = RoomTree.rows(sample(), "other", true,
                             ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        verify(kinds(rows).indexOf("member:jonah") === -1)
    }

    function test_reading_a_room_expands_it_until_another_is_open() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ selectedNode: "n", selectedRoom: "lobby" }))
        var k = kinds(rows)
        verify(k.indexOf("member:jonah") !== -1)
        verify(k.indexOf("member:mara") !== -1)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("member:sam") === -1)
        // A shut flag still wins over the open chat.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ selectedNode: "n", selectedRoom: "lobby",
                                   collapsed: { "n:lobby": true } }))
        verify(kinds(rows).indexOf("member:jonah") === -1)
        // Leaving the chat folds it again and opens the next one.
        rows = RoomTree.rows(sample(), "n", true,
                             ctx({ selectedNode: "n", selectedRoom: "raid" }))
        k = kinds(rows)
        verify(k.indexOf("member:jonah") === -1)
        verify(k.indexOf("member:sam") !== -1)
        verify(k.indexOf("room:strat") !== -1)
    }

    function test_leaves_fold_by_hand() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ collapsed: opened("lobby"),
                                       leaves: { "n:lobby:voice": false, "n:lobby:text": true } }))
        var k = kinds(rows)
        verify(k.indexOf("member:mara") === -1)
        verify(k.indexOf("member:jonah") !== -1)
    }

    function test_collapsed_room_keeps_its_sub_rooms_and_counts_its_own() {
        var rows = RoomTree.rows(sample(), "n", true, ctx({ collapsed: { "n:raid": true } }))
        var k = kinds(rows)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("room:strat") !== -1)
        verify(k.indexOf("member:sam") === -1)
        verify(k.indexOf("member:ade") === -1)
        var raid = rows[k.indexOf("room:raid")]
        verify(raid.collapsed)
        compare(raid.subtree_voice, 1)
        compare(raid.stack_ids, ["sam"])
        // Ade counts on Strategy, not on Raid.
        compare(rows[k.indexOf("room:strat")].subtree_voice, 1)
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
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid",
                                       collapsed: opened("lobby") }))
        var k = kinds(rows)
        verify(k.indexOf("session:raid") !== -1)
        verify(k.indexOf("member:sam") === -1)
        // The call icon sits on the room row. In the voice room we are in it
        // ends the call; anywhere else it joins.
        verify(rows[k.indexOf("room:raid")].show_call)
        verify(rows[k.indexOf("room:raid")].voice_here)
        verify(!rows[k.indexOf("room:lobby")].voice_here)
        // A room we opened by hand still offers the call.
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
        compare(lobby.room_chat, 1)
        compare(lobby.stack_ids, ["mara", "theo"])
        // Gaming has nobody of its own: it is the row its sub-rooms hang from.
        var gaming = room("gaming")
        verify(!gaming.has_children)
        compare(gaming.subtree_voice, 0)
        var raid = room("raid")
        verify(raid.collapsed)
        compare(raid.subtree_voice, 1)
        compare(raid.room_chat, 0)
        var strat = room("strat")
        verify(strat.collapsed)
        compare(strat.subtree_voice, 1)
    }

    function test_voice_room_shows_voice_and_text_people_together() {
        var rooms = sample()
        rooms[2].text_members = [member("nia")]
        var rows = RoomTree.rows(rooms, "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid" }))
        compare(kinds(rows), [
            "room:gaming",
            "room:raid", "group:raid:voice", "session:raid",
            "group:raid:text", "member:nia",
            "room:strat",
            "room:lobby"
        ])
        var k = kinds(rows)
        verify(rows[k.indexOf("room:lobby")].collapsed)
        verify(!rows[k.indexOf("room:raid")].collapsed)
        verify(rows[k.indexOf("room:strat")].collapsed)
        verify(rows[k.indexOf("room:raid")].voice_here)
        verify(rows[k.indexOf("room:lobby")].show_call)
        verify(rows[k.indexOf("group:raid:text")].expanded)
        verify(k.indexOf("member:ade") === -1)
    }

    function test_explicit_collapse_keeps_the_voice_room_shut() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ voiceNode: "n", voiceRoom: "raid",
                                       collapsed: { "n:raid": true } }))
        var k = kinds(rows)
        verify(k.indexOf("room:raid") !== -1)
        verify(k.indexOf("room:strat") !== -1)
        verify(k.indexOf("session:raid") === -1)
        verify(rows[k.indexOf("room:raid")].collapsed)
    }

    function test_invites_only_from_rooms_we_are_in() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ collapsed: opened("lobby", "gaming", "raid", "strat"),
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
                                 ctx({ limit: 5, collapsed: { "n:big": false } }))
        var members = rows.filter(function (r) { return r.row_kind === "member" })
        compare(members.length, 4)
        compare(rows[rows.length - 1].row_kind, "more")
        compare(rows[rows.length - 1].label, "+8 more")
        rows = RoomTree.rows(rooms, "n", true,
                             ctx({ limit: 5, collapsed: { "n:big": false },
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
        var rows = RoomTree.rows(rooms, "n", true,
                                 ctx({ collapsed: { "n:gaming": false, "n:raid": false } }))
        var k = kinds(rows)
        compare(rows[k.indexOf("room:raid")].guide_cols, [2])
        compare(rows[k.indexOf("room:strat")].guide_cols, [0, 2])
    }

    function test_member_guides_run_under_their_room() {
        var rows = RoomTree.rows(sample(), "n", true,
                                 ctx({ collapsed: opened("gaming", "raid") }))
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
        // Each is the other's child. Folded or open, neither is dropped.
        var rows = RoomTree.rows(rooms, "n", true,
                                 ctx({ collapsed: { "n:a": false, "n:b": false } }))
        compare(rows.filter(function (r) { return r.row_kind === "room" }).length, 2)
        var folded = RoomTree.rows(rooms, "n", true, ctx())
        compare(folded.filter(function (r) { return r.row_kind === "room" }).length, 2)
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
}
