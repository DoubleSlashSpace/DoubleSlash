// RoomTree.qml — The Rooms sidebar's tree, as rows.
//
// Pure logic, kept out of MainWindow so it can be tested on its own: given a
// node's room list (each room carrying `voice_members` / `text_members` from
// the bridge) and the sidebar's fold state, produce the flat list of rows the
// sidebar draws. The Android client builds the same shape in Kotlin, so the
// two trees read alike: a room, its Voice leaf, its Text-only leaf, then its
// sub-rooms.
pragma Singleton
import QtQuick

QtObject {
    id: root

    /// A room's members as the tree lists them: `[{id, name, trusted,
    /// list_peer_id, is_self}]`, or null when that roster is not known yet.
    function memberList(value) {
        if (value === undefined || value === null || typeof value.length !== "number")
            return null
        var out = []
        for (var i = 0; i < value.length; i++) {
            var m = value[i]
            if (m && m.id)
                out.push(m)
        }
        return out
    }

    /// The roster fields a sidebar room must keep. The bridge omits a list it
    /// has not heard yet; an empty list means the room really has nobody
    /// there. Copying only the keys that arrived lets a later merge keep the
    /// last known list instead of wiping it.
    function rosterFields(room) {
        var out = {}
        if (!room)
            return out
        if (room.voice_members !== undefined && room.voice_members !== null)
            out.voice_members = room.voice_members
        if (room.text_members !== undefined && room.text_members !== null)
            out.text_members = room.text_members
        return out
    }

    // Flatten a node's rooms into the rows of the Rooms tree, parents first.
    //
    // Under each room come its Voice leaf, its Text-only leaf, then its
    // sub-rooms, each leaf listing its members. Rows are plain objects with a
    // `row_kind` of "room", "group", "member", "session" (the live voice
    // session's members, drawn from roomModel) or "more".
    //
    // `guide_cols` are per-column connector codes: 0 blank, 1 pass-through │,
    // 2 └ (last child), 3 ├ (has a sibling below). A column carries on below a
    // row only while that row's ancestor at that depth has siblings to come.
    //
    // `ctx`: { collapsed, leaves, overflow } fold state keyed as in MainWindow,
    // `selectedNode`/`selectedRoom` (the room being read), `voiceNode`/
    // `voiceRoom` (the live voice session, "" when none), and `limit`, the
    // members a leaf lists before folding the rest under "+N more".
    //
    // Sub-rooms are always listed. A room's own voice and text lists start
    // collapsed, so each row shows its own counts. `collapsed[key] === false`
    // opens those lists and `=== true` shuts them, which also overrides the
    // live voice room and the open chat. With no entry, the voice room and
    // the chat being read open their lists and every other room stays shut.
    // An open room shows both lists' people; voice includes anyone also on video.
    //
    // A room is top-level when its `parent_id` is "" or points outside the list.
    // Anything unreachable (a parent cycle) is appended flat rather than hidden.
    function rows(rooms, nodeId, connected, ctx) {
        if (!Array.isArray(rooms)) return []
        var byId = {}
        var i
        for (i = 0; i < rooms.length; i++)
            if (rooms[i] && rooms[i].room_id) byId[rooms[i].room_id] = rooms[i]

        var childrenOf = {}
        var roots = []
        for (i = 0; i < rooms.length; i++) {
            var r = rooms[i]
            if (!r || !r.room_id) continue
            var pid = r.parent_id || ""
            if (pid !== "" && pid !== r.room_id && byId.hasOwnProperty(pid)) {
                if (!childrenOf[pid]) childrenOf[pid] = []
                childrenOf[pid].push(r)
            } else {
                roots.push(r)
            }
        }

        function codes(pass, isLast) {
            var cols = []
            for (var k = 0; k < pass.length; k++) cols.push(pass[k] ? 1 : 0)
            cols.push(isLast ? 2 : 3)
            return cols
        }
        function passCodes(pass) {
            var cols = []
            for (var k = 0; k < pass.length; k++) cols.push(pass[k] ? 1 : 0)
            return cols
        }
        function copy(obj) {
            var o = {}
            for (var kk in obj) if (obj.hasOwnProperty(kk)) o[kk] = obj[kk]
            return o
        }

        var selected = ctx.selectedNode === nodeId ? ctx.selectedRoom : ""
        var voiceHere = ctx.voiceNode === nodeId ? ctx.voiceRoom : ""
        var seen = {}
        var out = []

        function emitGroup(room, group, members, pass, isLast) {
            var key = nodeId + ":" + room.room_id + ":" + group
            var isSession = group === "voice" && room.room_id === voiceHere
            var reading = room.room_id === selected
            var stored = ctx.leaves[key]
            // The room is already open, so both lists show their people. A
            // stored flag is only a hand fold of that one list.
            var expanded = stored !== undefined ? stored : true
            out.push({
                row_kind: "group", group: group, key: key,
                room_id: room.room_id, room_name: room.name || room.room_id,
                count: members.length, expanded: expanded,
                guide_cols: codes(pass, isLast),
                show_join: group === "voice" && !isSession && connected
            })
            if (!expanded)
                return
            var gPass = pass.concat([!isLast])
            // A trust invite names the room it is sent from, and the receiver
            // checks we are in it: only rooms we are in can send one.
            var canInvite = reading || room.room_id === voiceHere
            if (isSession) {
                // The live session: roomModel carries its levels and controls.
                out.push({
                    row_kind: "session", room_id: room.room_id,
                    pass_cols: passCodes(gPass), can_invite: true
                })
                return
            }
            var shown = members
            var more = ""
            var oKey = key
            if (members.length > ctx.limit) {
                if (ctx.overflow[oKey] === true) {
                    more = qsTr("Show fewer")
                } else {
                    shown = members.slice(0, ctx.limit - 1)
                    more = qsTr("+%1 more").arg(members.length - shown.length)
                }
            }
            for (var m = 0; m < shown.length; m++) {
                var mem = shown[m]
                out.push({
                    row_kind: "member", group: group, room_id: room.room_id,
                    key: key + ":" + mem.id,
                    id: mem.id, name: mem.name || "",
                    trusted: mem.trusted === true, list_peer_id: mem.list_peer_id || "",
                    is_self: mem.is_self === true,
                    // Camera state reaches everyone on the room's roster.
                    on_roster: reading || room.room_id === voiceHere,
                    can_invite: canInvite,
                    guide_cols: codes(gPass, m === shown.length - 1 && more === "")
                })
            }
            if (more !== "")
                out.push({ row_kind: "more", key: oKey, label: more, guide_cols: codes(gPass, true) })
        }

        function emitRoom(room, depth, pass, isLast, visible) {
            if (seen[room.room_id]) return
            seen[room.room_id] = true
            var kids = childrenOf[room.room_id] || []
            // A disconnected node has no live roster: its leaves would be
            // guesses, so they are not drawn and the room shows "—" instead.
            var voice = connected ? root.memberList(room.voice_members) : null
            var text = connected ? root.memberList(room.text_members) : null
            var groups = []
            if (voice && voice.length > 0) groups.push({ group: "voice", members: voice })
            if (text && text.length > 0) groups.push({ group: "text", members: text })
            // The chevron folds this room's member lists. Sub-rooms stay
            // listed either way, so a room of only sub-rooms has nothing to fold.
            var hasMembers = groups.length > 0
            var foldKey = nodeId + ":" + room.room_id
            var foldFlag = ctx.collapsed[foldKey]
            var isVoice = voiceHere !== "" && room.room_id === voiceHere
            var isReading = selected !== "" && room.room_id === selected
            // true shuts the lists, false opens them, and no entry opens the
            // live voice room and the chat being read.
            var listsOpen = foldFlag === false
                || (foldFlag !== true && (isVoice || isReading))
            var isCollapsed = hasMembers && !listsOpen
            if (visible) {
                var item = copy(room)
                item.row_kind = "room"
                item.tree_depth = depth
                item.has_children = hasMembers
                item.collapsed = isCollapsed
                item.guide_cols = depth > 0 ? codes(pass, isLast) : []
                var own = isCollapsed && voice ? voice : []
                var stack = []
                for (var s = 0; s < own.length && stack.length < 3; s++)
                    stack.push(own[s].id)
                item.stack_ids = stack
                item.subtree_voice = own.length
                item.room_chat = isCollapsed && text ? text.length : 0
                out.push(item)
            }
            var childPass = depth > 0 ? pass.concat([!isLast]) : []
            var items = []
            if (listsOpen)
                for (var g = 0; g < groups.length; g++) items.push(groups[g])
            for (var c = 0; c < kids.length; c++) items.push({ room: kids[c] })
            for (var n = 0; n < items.length; n++) {
                var last = n === items.length - 1
                if (items[n].room)
                    emitRoom(items[n].room, depth + 1, childPass, last, visible)
                else if (visible)
                    emitGroup(room, items[n].group, items[n].members, childPass, last)
            }
        }

        for (var t = 0; t < roots.length; t++)
            emitRoom(roots[t], 0, [], t === roots.length - 1, true)
        for (i = 0; i < rooms.length; i++) {
            if (rooms[i] && rooms[i].room_id && !seen[rooms[i].room_id])
                emitRoom(rooms[i], 0, [], true, true)
        }
        return out
    }
}
