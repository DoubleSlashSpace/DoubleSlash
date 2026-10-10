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

    // How sibling rooms are ordered. The phone stores the same JSON.
    //
    // `mode` is name_asc (default), name_desc, peers_asc, peers_desc, or
    // manual. `pinned` room keys (`nodeId:roomId`) stay first among their
    // siblings, in the order they were pinned. `manual` is that same key
    // order, used only while mode is manual. A pin does not pull a sub-room
    // out from under its parent.
    function normalizeMode(mode) {
        if (mode === "name_desc" || mode === "peers_asc" || mode === "peers_desc" || mode === "manual")
            return mode
        return "name_asc"
    }

    function asKeyList(value) {
        if (!value || typeof value.length !== "number")
            return []
        var out = []
        for (var i = 0; i < value.length; i++)
            if (typeof value[i] === "string" && value[i] !== "")
                out.push(value[i])
        return out
    }

    function normalizeOrder(order) {
        var src = order || {}
        return {
            mode: root.normalizeMode(src.mode),
            pinned: root.asKeyList(src.pinned),
            manual: root.asKeyList(src.manual)
        }
    }

    function sortMode(order) {
        return root.normalizeOrder(order).mode
    }

    function isPinned(order, key) {
        return root.indexIn(root.normalizeOrder(order).pinned, key) !== -1
    }

    function indexIn(list, key) {
        for (var i = 0; i < list.length; i++)
            if (list[i] === key)
                return i
        return -1
    }

    // Case-insensitive, and "Room 2" before "Room 10". The phone uses the
    // same split so the two lists agree.
    function compareAlphanumeric(a, b) {
        var left = String(a || "").toLowerCase()
        var right = String(b || "").toLowerCase()
        var i = 0
        var j = 0
        function digit(s, at) {
            var c = s.charAt(at)
            return c >= "0" && c <= "9"
        }
        function trimZeros(s) {
            var k = 0
            while (k < s.length - 1 && s.charAt(k) === "0")
                k++
            return s.substring(k)
        }
        while (i < left.length && j < right.length) {
            var aDigit = digit(left, i)
            var bDigit = digit(right, j)
            if (aDigit && bDigit) {
                var iStart = i
                var jStart = j
                while (i < left.length && digit(left, i))
                    i++
                while (j < right.length && digit(right, j))
                    j++
                var aNum = trimZeros(left.substring(iStart, i))
                var bNum = trimZeros(right.substring(jStart, j))
                if (aNum.length !== bNum.length)
                    return aNum.length < bNum.length ? -1 : 1
                if (aNum !== bNum)
                    return aNum < bNum ? -1 : 1
            } else {
                if (left.charAt(i) !== right.charAt(j))
                    return left.charAt(i) < right.charAt(j) ? -1 : 1
                i++
                j++
            }
        }
        if (left.length === right.length)
            return 0
        return left.length < right.length ? -1 : 1
    }

    function roomLabel(room) {
        var name = room && room.name
        if (name === undefined || name === null || String(name) === "")
            return String(room && room.room_id || "")
        return String(name)
    }

    /// Voice members plus text-only members. A list the bridge has not sent
    /// yet counts as nobody, the same as an empty roster.
    function peerCount(room) {
        var voice = root.memberList(room && room.voice_members)
        var text = root.memberList(room && room.text_members)
        return (voice ? voice.length : 0) + (text ? text.length : 0)
    }

    function compareRooms(a, b, nodeId, order) {
        var o = root.normalizeOrder(order)
        var ka = nodeId + ":" + (a && a.room_id || "")
        var kb = nodeId + ":" + (b && b.room_id || "")
        var pa = root.indexIn(o.pinned, ka)
        var pb = root.indexIn(o.pinned, kb)
        if ((pa !== -1) !== (pb !== -1))
            return pa !== -1 ? -1 : 1
        if (pa !== -1 && pb !== -1 && pa !== pb)
            return pa - pb
        if (o.mode === "manual") {
            var ma = root.indexIn(o.manual, ka)
            var mb = root.indexIn(o.manual, kb)
            if (ma < 0) ma = 1000000000
            if (mb < 0) mb = 1000000000
            if (ma !== mb)
                return ma - mb
        } else if (o.mode === "peers_asc" || o.mode === "peers_desc") {
            var diff = root.peerCount(a) - root.peerCount(b)
            if (diff !== 0)
                return o.mode === "peers_asc" ? diff : -diff
        } else if (o.mode === "name_desc") {
            var desc = root.compareAlphanumeric(root.roomLabel(b), root.roomLabel(a))
            if (desc !== 0)
                return desc
            return root.compareAlphanumeric(b && b.room_id, a && a.room_id)
        }
        var asc = root.compareAlphanumeric(root.roomLabel(a), root.roomLabel(b))
        if (asc !== 0)
            return asc
        return root.compareAlphanumeric(a && a.room_id, b && b.room_id)
    }

    /// Roots, each parent's children, and rooms a cycle kept out of both.
    function partition(rooms) {
        var byId = {}
        var childrenOf = {}
        var roots = []
        var i
        if (!Array.isArray(rooms))
            return { childrenOf: childrenOf, roots: roots, tail: [] }
        for (i = 0; i < rooms.length; i++)
            if (rooms[i] && rooms[i].room_id)
                byId[rooms[i].room_id] = rooms[i]
        for (i = 0; i < rooms.length; i++) {
            var r = rooms[i]
            if (!r || !r.room_id)
                continue
            var pid = r.parent_id || ""
            if (pid !== "" && pid !== r.room_id && byId.hasOwnProperty(pid)) {
                if (!childrenOf[pid])
                    childrenOf[pid] = []
                childrenOf[pid].push(r)
            } else {
                roots.push(r)
            }
        }
        var seen = {}
        function walk(room) {
            if (!room || seen[room.room_id])
                return
            seen[room.room_id] = true
            var kids = childrenOf[room.room_id] || []
            for (var k = 0; k < kids.length; k++)
                walk(kids[k])
        }
        for (i = 0; i < roots.length; i++)
            walk(roots[i])
        var tail = []
        for (i = 0; i < rooms.length; i++)
            if (rooms[i] && rooms[i].room_id && !seen[rooms[i].room_id])
                tail.push(rooms[i])
        return { childrenOf: childrenOf, roots: roots, tail: tail }
    }

    function siblingGroup(rooms, roomId) {
        var part = root.partition(rooms)
        var i
        for (i = 0; i < part.tail.length; i++)
            if (part.tail[i].room_id === roomId)
                return part.tail
        for (i = 0; i < part.roots.length; i++)
            if (part.roots[i].room_id === roomId)
                return part.roots
        for (var pid in part.childrenOf) {
            if (!part.childrenOf.hasOwnProperty(pid))
                continue
            var list = part.childrenOf[pid]
            for (i = 0; i < list.length; i++)
                if (list[i].room_id === roomId)
                    return list
        }
        return []
    }

    function roomKeysInOrder(rooms, nodeId, order) {
        var rows = root.rows(rooms, nodeId, true, {
            collapsed: {}, leaves: {}, overflow: {},
            selectedNode: "", selectedRoom: "",
            voiceNode: "", voiceRoom: "",
            limit: 8,
            order: order
        })
        var keys = []
        for (var i = 0; i < rows.length; i++)
            if (rows[i].row_kind === "room")
                keys.push(nodeId + ":" + rows[i].room_id)
        return keys
    }

    /// Switch mode. The first time manual is chosen, freeze the order on screen.
    function withMode(order, mode, groups) {
        var o = root.normalizeOrder(order)
        var nextMode = root.normalizeMode(mode)
        var manual = o.manual.slice()
        if (nextMode === "manual" && manual.length === 0 && groups && groups.length) {
            for (var g = 0; g < groups.length; g++) {
                var group = groups[g]
                if (!group || !group.nodeId)
                    continue
                var keys = root.roomKeysInOrder(group.rooms, group.nodeId, o)
                for (var i = 0; i < keys.length; i++)
                    manual.push(keys[i])
            }
        }
        return { mode: nextMode, pinned: o.pinned.slice(), manual: manual }
    }

    function togglePin(order, key) {
        var o = root.normalizeOrder(order)
        var pinned = o.pinned.slice()
        var at = root.indexIn(pinned, key)
        if (at === -1)
            pinned.unshift(key)
        else
            pinned.splice(at, 1)
        return { mode: o.mode, pinned: pinned, manual: o.manual.slice() }
    }

    function relocate(list, key, neighborKey, after) {
        var next = []
        var i
        for (i = 0; i < list.length; i++)
            if (list[i] !== key)
                next.push(list[i])
        var at = -1
        for (i = 0; i < next.length; i++) {
            if (next[i] === neighborKey) {
                at = i
                break
            }
        }
        if (at < 0)
            return list.slice()
        next.splice(after ? at + 1 : at, 0, key)
        return next
    }

    function placeMissing(manual, rooms, nodeId, order) {
        var keys = root.roomKeysInOrder(rooms, nodeId, order)
        var next = manual.slice()
        var cursor = -1
        for (var i = 0; i < keys.length; i++) {
            var at = root.indexIn(next, keys[i])
            if (at < 0) {
                next.splice(cursor + 1, 0, keys[i])
                cursor = cursor + 1
            } else {
                cursor = at
            }
        }
        return next
    }

    /// Move a room one place among its siblings. Pins move inside the pin
    /// band; other rooms move only in manual order, and not across a pin.
    function moveRoom(rooms, nodeId, roomId, delta, order) {
        var o = root.normalizeOrder(order)
        if (delta !== -1 && delta !== 1)
            return o
        var group = root.siblingGroup(rooms, roomId)
        if (group.length < 2)
            return o
        var sorted = group.slice()
        sorted.sort(function (a, b) { return root.compareRooms(a, b, nodeId, o) })
        var i = -1
        for (var n = 0; n < sorted.length; n++)
            if (sorted[n].room_id === roomId)
                i = n
        var j = i + delta
        if (i < 0 || j < 0 || j >= sorted.length)
            return o
        var aKey = nodeId + ":" + sorted[i].room_id
        var bKey = nodeId + ":" + sorted[j].room_id
        var aPin = root.indexIn(o.pinned, aKey) !== -1
        var bPin = root.indexIn(o.pinned, bKey) !== -1
        if (aPin !== bPin)
            return o
        if (aPin)
            return {
                mode: o.mode,
                pinned: root.relocate(o.pinned, aKey, bKey, delta > 0),
                manual: o.manual.slice()
            }
        if (o.mode !== "manual")
            return o
        var manual = o.manual.slice()
        if (root.indexIn(manual, aKey) < 0 || root.indexIn(manual, bKey) < 0)
            manual = root.placeMissing(manual, rooms, nodeId, o)
        return {
            mode: o.mode,
            pinned: o.pinned.slice(),
            manual: root.relocate(manual, aKey, bKey, delta > 0)
        }
    }

    function sameOrder(a, b) {
        a = root.normalizeOrder(a)
        b = root.normalizeOrder(b)
        if (a.mode !== b.mode || a.pinned.length !== b.pinned.length || a.manual.length !== b.manual.length)
            return false
        var i
        for (i = 0; i < a.pinned.length; i++)
            if (a.pinned[i] !== b.pinned[i])
                return false
        for (i = 0; i < a.manual.length; i++)
            if (a.manual[i] !== b.manual[i])
                return false
        return true
    }

    function canMove(rooms, nodeId, roomId, delta, order) {
        return !root.sameOrder(order, root.moveRoom(rooms, nodeId, roomId, delta, order))
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
    //
    // Siblings follow `ctx.order` (see normalizeOrder). With no order, that is
    // name A–Z.
    function rows(rooms, nodeId, connected, ctx) {
        if (!Array.isArray(rooms)) return []
        var part = root.partition(rooms)
        var order = root.normalizeOrder(ctx && ctx.order)
        function byOrder(a, b) { return root.compareRooms(a, b, nodeId, order) }
        part.roots.sort(byOrder)
        var childrenOf = part.childrenOf
        var roots = part.roots
        var pid
        for (pid in childrenOf)
            if (childrenOf.hasOwnProperty(pid))
                childrenOf[pid].sort(byOrder)
        part.tail.sort(byOrder)
        var i

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
                guide_cols: codes(pass, isLast)
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
                // The call icon on the row: joining is explicit, not only a
                // double-click. Not while we are already in this voice room.
                item.show_call = connected && !isVoice
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
        for (i = 0; i < part.tail.length; i++)
            emitRoom(part.tail[i], 0, [], i === part.tail.length - 1, true)
        return out
    }
}
