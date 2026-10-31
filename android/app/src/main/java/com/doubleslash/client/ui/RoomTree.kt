package com.doubleslash.client.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.GenericShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.layout
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.doubleslash.client.AppState
import com.doubleslash.client.R
import com.doubleslash.client.Room
import com.doubleslash.client.RoomNodeInfo
import com.doubleslash.client.VoiceRoom
import com.doubleslash.client.cameraOn
import com.doubleslash.client.inviteContacts
import com.doubleslash.client.roomMembersUnion
import com.doubleslash.client.roomSenderName
import com.doubleslash.client.trustedPeer
import com.doubleslash.client.videoKey
import com.doubleslash.client.watching
import kotlin.math.sqrt
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

// ── The rows ────────────────────────────────────────────────────────────────
//
// The Rooms tree as the desktop draws it (RoomTree.qml): under each room its
// Voice leaf, its Text-only leaf, then its sub-rooms, each leaf listing its
// members. Built as flat rows so a LazyColumn can draw it; `guides` are the
// connector codes per indent column — 0 blank, 1 pass-through, 2 last child,
// 3 a sibling follows — and only the last column can hold an elbow.

/**
 * What the tree has folded or opened, kept by the screen.
 *
 * Sub-rooms stay listed. A room's voice and text lists both start shut.
 * [leaves] opens them, keyed `"<room key>:voice"` or `":text"`. The voice
 * count opens the voice list and hides once it is open. The chevron still
 * toggles it. The text count opens the text list. Selecting
 * the room or being in its voice does neither. Voice includes anyone also on
 * video.
 */
internal data class TreeFold(
    /** Leaves opened or closed by hand, `"<room key>:voice|text"`. Absent means shut. */
    val leaves: Map<String, Boolean> = emptyMap(),
    /** Leaves listing every member rather than the first few. */
    val overflow: Set<String> = emptySet(),
    /** The member whose actions are open under their row, or "". */
    val openMember: String = "",
)

internal sealed interface TreeRow {
    val key: String
    val guides: List<Int>

    data class RoomNode(
        override val key: String,
        override val guides: List<Int>,
        val room: Room,
        val hasChildren: Boolean,
        val collapsed: Boolean,
        /** Voice members. The row badge hides this once the list is open; the leaf still shows it. */
        val voiceCount: Int = 0,
        val roomChat: Int,
        /**
         * The text count is listing the voice roster: we are in this room's
         * voice and nobody is left text-only.
         */
        val textListsRoom: Boolean = false,
        /** The call icon on the row: it joins this room's voice, or hangs up when [inVoice]. */
        val showCall: Boolean = false,
        /** We are in this room's voice, so its call icon ends the call. */
        val inVoice: Boolean = false,
    ) : TreeRow

    data class Leaf(
        override val key: String,
        override val guides: List<Int>,
        val room: Room,
        val voice: Boolean,
        val count: Int,
        val expanded: Boolean,
        /**
         * This text leaf is listing the voice roster, because we are in the
         * room's voice and nobody is left text-only.
         */
        val listsRoom: Boolean = false,
        /** Set while open: the leaf row starts its list's box. */
        val box: ListBox? = null,
    ) : TreeRow

    data class Member(
        override val key: String,
        override val guides: List<Int>,
        val room: Room,
        val id: String,
        val voice: Boolean,
        /** Their voice is part of the session we are in. */
        val inSession: Boolean,
        /** We are on this room's roster, so its camera state reaches us. */
        val onRoster: Boolean,
        /** A trust invite can be sent from this room: we are in it. */
        val canInvite: Boolean,
        /** The open list's box this row is part of. */
        val box: ListBox? = null,
    ) : TreeRow

    data class More(
        override val key: String,
        override val guides: List<Int>,
        val label: String,
        val overflowKey: String,
        val box: ListBox? = null,
    ) : TreeRow
}

/**
 * An open voice or text list, drawn as one box in the colours of the count
 * box that stands for it, as on the desktop: green for voice, blue for text.
 * Every row of the list carries it. [depth] is the leaf's column, where the
 * box's edge sits; the leaf row [start]s it and the last row [end]s it.
 */
internal data class ListBox(val voice: Boolean, val depth: Int, val start: Boolean, val end: Boolean) {
    /** One column further out, for the members sheet, whose rows drop one. */
    fun outdented(): ListBox = copy(depth = (depth - 1).coerceAtLeast(0))
}

/**
 * Draw [box]'s part of an open list behind a row: the fill and both sides on
 * every row, the top on its leaf row and the bottom on its last row.
 */
private fun Modifier.listBox(box: ListBox?, ds: DsColors): Modifier =
    if (box == null) {
        this
    } else {
        drawBehind {
            val tint = if (box.voice) ds.online else ds.accent
            val x = (EDGE + STEP * box.depth - 4.dp).toPx()
            // Ends where the rows' content does, in line with the call tiles.
            val w = size.width - BOX_END_INSET.toPx() - x
            // A gap above and below the box, so two open lists in a row, or a
            // list and the room under it, read as separate boxes.
            val top = if (box.start) BOX_GAP.toPx() else 0f
            val bottom = if (box.end) size.height - BOX_GAP.toPx() else size.height
            val h = bottom - top
            val line = 1.dp.toPx()
            drawRect(tint.copy(alpha = 0.16f), topLeft = Offset(x, top), size = Size(w, h))
            drawRect(tint, topLeft = Offset(x, top), size = Size(line, h))
            drawRect(tint, topLeft = Offset(x + w - line, top), size = Size(line, h))
            if (box.start) drawRect(tint, topLeft = Offset(x, top), size = Size(w, line))
            if (box.end) drawRect(tint, topLeft = Offset(x, bottom - line), size = Size(w, line))
        }
    }

/** What sits under a room, in order: its leaves, then its sub-rooms. */
private sealed interface TreeItem {
    data class LeafItem(
        val voice: Boolean,
        val members: List<String>,
        val listsRoom: Boolean = false,
    ) : TreeItem
    data class SubRoom(val room: Room) : TreeItem
}

/** Members a leaf lists before folding the rest under "+N more". */
internal const val TREE_MEMBER_LIMIT = 8

/** Rooms-list sort, the same JSON the desktop stores in `room_list_order_json`. */
internal const val ROOM_SORT_NAME_ASC = "name_asc"
internal const val ROOM_SORT_NAME_DESC = "name_desc"
internal const val ROOM_SORT_PEERS_ASC = "peers_asc"
internal const val ROOM_SORT_PEERS_DESC = "peers_desc"
internal const val ROOM_SORT_MANUAL = "manual"

internal const val DEFAULT_ROOM_LIST_ORDER_JSON =
    """{"mode":"name_asc","pinned":[],"manual":[]}"""

/** Labels in the list header sort menu, in the same order as the desktop's Sort rooms menu. */
internal val ROOM_SORT_OPTIONS = listOf(
    ROOM_SORT_NAME_ASC to "Name (A\u2013Z)",
    ROOM_SORT_NAME_DESC to "Name (Z\u2013A)",
    ROOM_SORT_PEERS_ASC to "Fewest people",
    ROOM_SORT_PEERS_DESC to "Most people",
    ROOM_SORT_MANUAL to "Manual order",
)

/**
 * Saved rooms-list order.
 *
 * [pinned] keys (`supernodeId:roomId`) stay first among their siblings.
 * [manual] is that same key order, used only while [mode] is [ROOM_SORT_MANUAL].
 * A pin does not pull a sub-room out from under its parent.
 * [collapsed] lists the supernode ids whose rooms are folded under their
 * node row.
 */
data class RoomListOrder(
    val mode: String = ROOM_SORT_NAME_ASC,
    val pinned: List<String> = emptyList(),
    val manual: List<String> = emptyList(),
    val collapsed: List<String> = emptyList(),
) {
    fun normalized(): RoomListOrder = copy(
        mode = normalizeRoomSortMode(mode),
        pinned = pinned.filter { it.isNotEmpty() },
        manual = manual.filter { it.isNotEmpty() },
        collapsed = collapsed.filter { it.isNotEmpty() },
    )

    fun isPinned(room: Room): Boolean = room.listOrderKey() in pinned

    fun isNodeCollapsed(nodeId: String): Boolean = nodeId in collapsed
}

internal fun withNodeCollapsed(order: RoomListOrder, nodeId: String, collapsed: Boolean): RoomListOrder {
    val o = order.normalized()
    val rest = o.collapsed.filter { it != nodeId }
    return o.copy(collapsed = if (collapsed && nodeId.isNotEmpty()) rest + nodeId else rest)
}

/** A supernode row and the rooms listed under it. */
internal data class RoomNodeGroup(val node: RoomNodeInfo, val rooms: List<Room>)

/**
 * The Rooms list's top level: one group per supernode, a cluster counting as
 * one, named A–Z. A node with no rooms still gets its row. A room naming a
 * node the core did not describe gets a row of its own.
 */
internal fun groupRoomsByNode(rooms: List<Room>, nodes: List<RoomNodeInfo>): List<RoomNodeGroup> {
    val byNode = rooms.groupBy { it.listNodeId }
    val described = nodes.filter { it.nodeId.isNotEmpty() }.distinctBy { it.nodeId }
    val known = described.map { it.nodeId }.toSet()
    val extra = byNode.keys.filter { it !in known }.map { RoomNodeInfo(nodeId = it) }
    return (described + extra)
        .sortedWith { a, b ->
            compareAlphanumeric(a.label, b.label).takeIf { it != 0 } ?: a.nodeId.compareTo(b.nodeId)
        }
        .map { RoomNodeGroup(it, byNode[it.nodeId].orEmpty()) }
}

/** Voice members across a node's rooms, for its folded row. */
internal fun nodeVoiceCount(rooms: List<Room>, voiceRosters: Map<String, List<String>>): Int =
    rooms.sumOf { voiceRosters.roomMembersUnion(it.roomId).size }

internal fun Room.listOrderKey(): String = "$supernodeId:$roomId"

internal fun normalizeRoomSortMode(mode: String): String = when (mode) {
    ROOM_SORT_NAME_DESC, ROOM_SORT_PEERS_ASC, ROOM_SORT_PEERS_DESC, ROOM_SORT_MANUAL -> mode
    else -> ROOM_SORT_NAME_ASC
}

internal fun parseRoomListOrder(raw: String?): RoomListOrder {
    if (raw.isNullOrBlank()) return RoomListOrder()
    return try {
        val obj = Json.parseToJsonElement(raw).jsonObject
        fun strings(name: String): List<String> {
            val arr = obj[name] as? JsonArray ?: return emptyList()
            return arr.mapNotNull { (it as? JsonPrimitive)?.content }
        }
        RoomListOrder(
            mode = normalizeRoomSortMode(obj["mode"]?.jsonPrimitive?.contentOrNull ?: ROOM_SORT_NAME_ASC),
            pinned = strings("pinned"),
            manual = strings("manual"),
            collapsed = strings("collapsed"),
        ).normalized()
    } catch (_: Exception) {
        RoomListOrder()
    }
}

internal fun RoomListOrder.toJson(): String {
    val o = normalized()
    return buildJsonObject {
        put("mode", o.mode)
        put("pinned", JsonArray(o.pinned.map { JsonPrimitive(it) }))
        put("manual", JsonArray(o.manual.map { JsonPrimitive(it) }))
        put("collapsed", JsonArray(o.collapsed.map { JsonPrimitive(it) }))
    }.toString()
}

/** Voice members plus text-only members. */
internal fun roomPeerCount(
    room: Room,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): Int {
    val voice = voiceRosters.roomMembersUnion(room.roomId)
    val inVoice = voice.map { it.videoKey() }.toSet()
    val text = textRosters.roomMembersUnion(room.roomId).count { it.videoKey() !in inVoice }
    return voice.size + text
}

private fun roomSortName(room: Room): String = room.roomName.ifBlank { room.roomId }

/** Case-insensitive, and "Room 2" before "Room 10". Matches [compareAlphanumeric] in RoomTree.qml. */
internal fun compareAlphanumeric(a: String, b: String): Int {
    val left = a.lowercase()
    val right = b.lowercase()
    var i = 0
    var j = 0
    fun digit(s: String, at: Int) = s[at] in '0'..'9'
    fun trimZeros(s: String): String {
        var k = 0
        while (k < s.length - 1 && s[k] == '0') k++
        return s.substring(k)
    }
    while (i < left.length && j < right.length) {
        val aDigit = digit(left, i)
        val bDigit = digit(right, j)
        if (aDigit && bDigit) {
            val iStart = i
            val jStart = j
            while (i < left.length && digit(left, i)) i++
            while (j < right.length && digit(right, j)) j++
            val aNum = trimZeros(left.substring(iStart, i))
            val bNum = trimZeros(right.substring(jStart, j))
            if (aNum.length != bNum.length) return aNum.length.compareTo(bNum.length)
            if (aNum != bNum) return aNum.compareTo(bNum)
        } else {
            if (left[i] != right[j]) return left[i].compareTo(right[j])
            i++
            j++
        }
    }
    return left.length.compareTo(right.length)
}

internal fun compareRooms(
    a: Room,
    b: Room,
    order: RoomListOrder,
    peerCount: (Room) -> Int,
): Int {
    val o = order.normalized()
    val ka = a.listOrderKey()
    val kb = b.listOrderKey()
    val pa = o.pinned.indexOf(ka)
    val pb = o.pinned.indexOf(kb)
    if ((pa >= 0) != (pb >= 0)) return if (pa >= 0) -1 else 1
    if (pa >= 0 && pb >= 0 && pa != pb) return pa - pb
    if (o.mode == ROOM_SORT_MANUAL) {
        val ma = o.manual.indexOf(ka).let { if (it < 0) Int.MAX_VALUE else it }
        val mb = o.manual.indexOf(kb).let { if (it < 0) Int.MAX_VALUE else it }
        if (ma != mb) return ma.compareTo(mb)
    } else if (o.mode == ROOM_SORT_PEERS_ASC || o.mode == ROOM_SORT_PEERS_DESC) {
        val diff = peerCount(a) - peerCount(b)
        if (diff != 0) return if (o.mode == ROOM_SORT_PEERS_ASC) diff else -diff
    } else if (o.mode == ROOM_SORT_NAME_DESC) {
        val byName = compareAlphanumeric(roomSortName(b), roomSortName(a))
        if (byName != 0) return byName
        return compareAlphanumeric(b.roomId, a.roomId)
    }
    val byName = compareAlphanumeric(roomSortName(a), roomSortName(b))
    if (byName != 0) return byName
    return compareAlphanumeric(a.roomId, b.roomId)
}

private fun relocate(list: List<String>, key: String, neighbor: String, after: Boolean): List<String> {
    val next = list.filter { it != key }.toMutableList()
    val at = next.indexOf(neighbor)
    if (at < 0) return list
    next.add(if (after) at + 1 else at, key)
    return next
}

internal fun withRoomPinToggled(order: RoomListOrder, key: String): RoomListOrder {
    val o = order.normalized()
    val pinned = if (key in o.pinned) o.pinned.filter { it != key } else listOf(key) + o.pinned
    return o.copy(pinned = pinned)
}

/** Switch mode. The first time manual is chosen, freeze the order on screen. */
internal fun roomOrderWithMode(
    order: RoomListOrder,
    mode: String,
    rooms: List<Room>,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): RoomListOrder {
    val current = order.normalized()
    val nextMode = normalizeRoomSortMode(mode)
    val manual = if (nextMode == ROOM_SORT_MANUAL && current.manual.isEmpty()) {
        // One node at a time: room ids repeat across nodes.
        rooms.groupBy { it.listNodeId }.values.flatMap { nodeRooms ->
            buildRoomTree(
                rooms = nodeRooms,
                voiceRosters = voiceRosters,
                textRosters = textRosters,
                reading = null,
                voiceRoom = null,
                fold = TreeFold(),
                order = current,
            ).mapNotNull { (it as? TreeRow.RoomNode)?.room?.listOrderKey() }
        }
    } else {
        current.manual
    }
    return current.copy(mode = nextMode, manual = manual)
}

private fun orderedSiblings(
    rooms: List<Room>,
    room: Room,
    order: RoomListOrder,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): List<Room> {
    val byId = rooms.associateBy { it.roomId }
    fun parentOf(r: Room): String {
        val p = r.parentId
        return if (p.isBlank() || p == r.roomId || p == r.spaceId || p !in byId) "" else p
    }
    val grouped = rooms.groupBy { parentOf(it) }
    val seen = mutableSetOf<String>()
    fun walk(r: Room) {
        if (!seen.add(r.roomId)) return
        grouped[r.roomId].orEmpty().forEach(::walk)
    }
    grouped[""].orEmpty().forEach(::walk)
    val tail = rooms.filter { it.roomId !in seen }
    val group = if (tail.any { it.roomId == room.roomId && it.supernodeId == room.supernodeId }) {
        tail
    } else {
        grouped[parentOf(room)].orEmpty()
    }
    return group.sortedWith { a, b ->
        compareRooms(a, b, order) { roomPeerCount(it, voiceRosters, textRosters) }
    }
}

private fun placeMissing(
    manual: List<String>,
    rooms: List<Room>,
    order: RoomListOrder,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): List<String> {
    val keys = buildRoomTree(
        rooms = rooms,
        voiceRosters = voiceRosters,
        textRosters = textRosters,
        reading = null,
        voiceRoom = null,
        fold = TreeFold(),
        order = order,
    ).mapNotNull { (it as? TreeRow.RoomNode)?.room?.listOrderKey() }
    val next = manual.toMutableList()
    var cursor = -1
    for (key in keys) {
        val at = next.indexOf(key)
        if (at < 0) {
            next.add(cursor + 1, key)
            cursor += 1
        } else {
            cursor = at
        }
    }
    return next
}

/** Move a room one place among its siblings. See RoomTree.qml `moveRoom`. */
internal fun moveRoomInOrder(
    order: RoomListOrder,
    allRooms: List<Room>,
    room: Room,
    delta: Int,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): RoomListOrder {
    val o = order.normalized()
    if (delta != -1 && delta != 1) return o
    // Siblings are under the same node: another node's room of the same id
    // is not one of them.
    val rooms = allRooms.filter { it.listNodeId == room.listNodeId }
    val sorted = orderedSiblings(rooms, room, o, voiceRosters, textRosters)
    val i = sorted.indexOfFirst { it.roomId == room.roomId && it.supernodeId == room.supernodeId }
    val j = i + delta
    if (i < 0 || j !in sorted.indices) return o
    val aKey = sorted[i].listOrderKey()
    val bKey = sorted[j].listOrderKey()
    val aPin = aKey in o.pinned
    val bPin = bKey in o.pinned
    if (aPin != bPin) return o
    if (aPin) return o.copy(pinned = relocate(o.pinned, aKey, bKey, delta > 0))
    if (o.mode != ROOM_SORT_MANUAL) return o
    var manual = o.manual
    if (aKey !in manual || bKey !in manual) {
        manual = placeMissing(manual, rooms, o, voiceRosters, textRosters)
    }
    return o.copy(manual = relocate(manual, aKey, bKey, delta > 0))
}

internal fun canMoveRoom(
    order: RoomListOrder,
    rooms: List<Room>,
    room: Room,
    delta: Int,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): Boolean = moveRoomInOrder(order, rooms, room, delta, voiceRosters, textRosters) != order.normalized()

private fun codes(pass: List<Boolean>, last: Boolean): List<Int> =
    pass.map { if (it) 1 else 0 } + (if (last) 2 else 3)

/** Build the Rooms tree's rows. Pure, so it can be tested on its own. */
internal fun buildRoomTree(
    rooms: List<Room>,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
    reading: Room?,
    voiceRoom: VoiceRoom?,
    fold: TreeFold,
    limit: Int = TREE_MEMBER_LIMIT,
    order: RoomListOrder = RoomListOrder(),
): List<TreeRow> {
    val byId = rooms.associateBy { it.roomId }
    // Empty, self-referential, the Space itself or a room we do not have all
    // mean "top level", as in layOutSpaceTree before it.
    val grouped = rooms.groupBy { r ->
        val p = r.parentId
        if (p.isBlank() || p == r.roomId || p == r.spaceId || p !in byId) "" else p
    }

    val out = mutableListOf<TreeRow>()
    val seen = mutableSetOf<String>()

    fun voiceOf(r: Room) = voiceRosters.roomMembersUnion(r.roomId)
    /** Text-only people, or the voice roster when [inThisVoice] and that list is empty. */
    fun textOf(r: Room, voice: List<String>, inThisVoice: Boolean): Pair<List<String>, Boolean> {
        val inVoice = voice.map { it.videoKey() }.toSet()
        val textOnly = textRosters.roomMembersUnion(r.roomId).filter { it.videoKey() !in inVoice }
        if (textOnly.isNotEmpty()) return textOnly to false
        if (inThisVoice && voice.isNotEmpty()) return voice to true
        return textOnly to false
    }
    val orderNorm = order.normalized()
    val children = grouped.mapValues { (_, list) ->
        list.sortedWith { a, b ->
            compareRooms(a, b, orderNorm) { roomPeerCount(it, voiceRosters, textRosters) }
        }
    }
    fun emitLeaf(
        r: Room,
        voice: Boolean,
        members: List<String>,
        pass: List<Boolean>?,
        last: Boolean,
        listsRoom: Boolean = false,
    ) {
        val leafKey = r.key + ":" + if (voice) "voice" else "text"
        val isReading = reading != null && r.roomId == reading.roomId && r.supernodeId == reading.supernodeId
        val inThisVoice = voiceRoom != null && r.roomId == voiceRoom.roomId && r.supernodeId == voiceRoom.supernodeId
        val isSession = voice && inThisVoice
        // A list is emitted only when its flag is already true, so it arrives open.
        val expanded = true
        // `pass == null`: the leaf is itself top level (the members sheet).
        val leafGuides = if (pass == null) emptyList() else codes(pass, last)
        val depth = leafGuides.size
        out += TreeRow.Leaf(
            key = leafKey,
            guides = leafGuides,
            room = r,
            voice = voice,
            count = members.size,
            expanded = expanded,
            listsRoom = listsRoom,
            // A list with nobody in it ends with its leaf row.
            box = if (expanded) ListBox(voice, depth, start = true, end = members.isEmpty()) else null,
        )
        if (!expanded) return
        val gPass = if (pass == null) emptyList() else pass + !last
        val overflowOpen = leafKey in fold.overflow
        val shown = if (members.size > limit && !overflowOpen) members.take(limit - 1) else members
        val more = when {
            members.size <= limit -> ""
            overflowOpen -> "Show fewer"
            else -> "+${members.size - shown.size} more"
        }
        shown.forEachIndexed { i, id ->
            out += TreeRow.Member(
                key = "$leafKey:$id",
                guides = codes(gPass, i == shown.lastIndex && more.isEmpty()),
                room = r,
                id = id,
                voice = voice,
                inSession = isSession,
                onRoster = isReading || inThisVoice,
                // The receiver only honours an invite from someone in the room
                // it names, so only rooms we are in can send one.
                canInvite = isReading || inThisVoice,
                box = ListBox(voice, depth, start = false, end = i == shown.lastIndex && more.isEmpty()),
            )
        }
        if (more.isNotEmpty()) {
            out += TreeRow.More(
                "$leafKey:more", codes(gPass, true), more, leafKey,
                box = ListBox(voice, depth, start = false, end = true),
            )
        }
    }

    fun emitRoom(r: Room, depth: Int, pass: List<Boolean>, last: Boolean, visible: Boolean) {
        if (!seen.add(r.roomId)) return
        val kids = children[r.roomId].orEmpty()
        val voice = voiceOf(r)
        // The chevron toggles this room's voice list. Sub-rooms stay listed
        // either way, so a room with nobody in voice has nothing to fold.
        val voiceKey = r.key + ":voice"
        val textKey = r.key + ":text"
        val isVoiceRoom = voiceRoom != null && r.roomId == voiceRoom.roomId &&
            r.supernodeId == voiceRoom.supernodeId
        val (text, listsRoom) = textOf(r, voice, isVoiceRoom)
        val voiceOpen = voice.isNotEmpty() && fold.leaves[voiceKey] == true
        val textOpen = text.isNotEmpty() && fold.leaves[textKey] == true
        val collapsed = voice.isNotEmpty() && !voiceOpen
        if (visible) {
            out += TreeRow.RoomNode(
                key = r.key,
                guides = if (depth > 0) codes(pass, last) else emptyList(),
                room = r,
                hasChildren = voice.isNotEmpty(),
                collapsed = collapsed,
                voiceCount = voice.size,
                roomChat = if (textOpen) 0 else text.size,
                textListsRoom = listsRoom && !textOpen,
                showCall = true,
                inVoice = isVoiceRoom,
            )
        }
        val childPass = if (depth > 0) pass + !last else emptyList()
        // Leaves first, then sub-rooms: who is here reads before where to go.
        val items = buildList<TreeItem> {
            if (voiceOpen) add(TreeItem.LeafItem(true, voice))
            if (textOpen) add(TreeItem.LeafItem(false, text, listsRoom))
            kids.forEach { add(TreeItem.SubRoom(it)) }
        }
        items.forEachIndexed { i, item ->
            val isLast = i == items.lastIndex
            when (item) {
                is TreeItem.SubRoom -> emitRoom(item.room, depth + 1, childPass, isLast, visible)
                is TreeItem.LeafItem ->
                    if (visible) emitLeaf(r, item.voice, item.members, childPass, isLast, item.listsRoom)
            }
        }
    }

    children[""].orEmpty().forEachIndexed { i, r ->
        emitRoom(r, 0, emptyList(), i == children[""].orEmpty().lastIndex, true)
    }
    // Anything a parent cycle kept out still gets shown, flat.
    val tail = rooms.filter { it.roomId !in seen }.sortedWith { a, b ->
        compareRooms(a, b, orderNorm) { roomPeerCount(it, voiceRosters, textRosters) }
    }
    tail.forEachIndexed { i, room ->
        emitRoom(room, 0, emptyList(), i == tail.lastIndex, true)
    }
    return out
}

/** One room's Voice and Text-only leaves, top level: the members sheet. */
internal fun buildRoomLeaves(
    room: Room,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
    voiceRoom: VoiceRoom?,
    fold: TreeFold,
): List<TreeRow> {
    // The same builder with this room as the only one, then its own row
    // dropped: the sheet is titled with the room already.
    // The sheet is this room's people. A list the sidebar has not touched yet
    // is shown here; one the user has shut stays shut.
    val voiceLeaf = room.key + ":voice"
    val textLeaf = room.key + ":text"
    val leaves = fold.leaves.toMutableMap()
    if (voiceLeaf !in leaves) leaves[voiceLeaf] = true
    if (textLeaf !in leaves) leaves[textLeaf] = true
    val rows = buildRoomTree(
        rooms = listOf(room.copy(parentId = "")),
        voiceRosters = voiceRosters,
        textRosters = textRosters,
        reading = room,
        voiceRoom = voiceRoom,
        fold = fold.copy(leaves = leaves),
    )
    return rows.drop(1).map { row ->
        // One column less: the leaves sit at the sheet's edge.
        when (row) {
            is TreeRow.Leaf -> row.copy(guides = row.guides.drop(1), box = row.box?.outdented())
            is TreeRow.Member -> row.copy(guides = row.guides.drop(1), box = row.box?.outdented())
            is TreeRow.More -> row.copy(guides = row.guides.drop(1), box = row.box?.outdented())
            is TreeRow.RoomNode -> row
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────────

private val STEP = 16.dp
private val EDGE = 8.dp
private val ROOM_ROW = 48.dp
private val NODE_ROW = 48.dp
private val LEAF_ROW = 40.dp
private val MEMBER_ROW = 44.dp
/** The open-list box stops this far from the right, where row content ends. */
private val BOX_END_INSET = 12.dp
/** Space above and below an open-list box. */
private val BOX_GAP = 3.dp

/** What the tree's rows can do, supplied once by the screen. */
internal class RoomTreeActions(
    val onFold: (TreeFold) -> Unit,
    val onOpenRoom: (Room) -> Unit,
    /** Join a room's voice: runs the microphone explanation first. */
    val onJoinVoice: (Room) -> Unit,
    val onSetHidden: (Room, Boolean) -> Unit,
    /** Delete the room on this device. The flag also deletes its chat history. */
    val onDeleteRoom: (Room, Boolean) -> Unit = { _, _ -> },
    /** Create a room nested inside this one, with the parent already chosen. */
    val onCreateSubRoom: (Room) -> Unit,
    val members: MemberActions,
    val onPeerAudio: (peerId: String, muted: Boolean, volume: Int) -> Unit,
    /** Keep this room first among its siblings, or stop doing that. */
    val onTogglePin: (Room) -> Unit = {},
    /** Move this room one place among its siblings. `delta` is -1 or 1. */
    val onMoveRoom: (Room, Int) -> Unit = { _, _ -> },
    /** Copy a shareable link for this room. */
    val onCopyInvite: (Room) -> Unit = {},
    /** Pick a contact and copy a link bound to them. */
    val onInviteContact: (Room) -> Unit = {},
    /** Leave the voice room we are in: the hang-up on its row, as the voice dock's Leave. */
    val onLeaveVoice: () -> Unit = {},
    /** Turn message alerts on or off for this room. */
    val onSetMessageAlerts: (Room, Boolean) -> Unit = { _, _ -> },
)

/** Connector lines in the indent, drawn behind the row. */
private fun Modifier.treeGuides(guides: List<Int>, elbowY: Dp, color: Color): Modifier =
    if (guides.isEmpty()) {
        this
    } else {
        drawBehind {
            val step = STEP.toPx()
            val edge = EDGE.toPx()
            val y = elbowY.toPx()
            val w = 1.dp.toPx()
            guides.forEachIndexed { i, code ->
                val x = edge + i * step + step / 2
                when (code) {
                    1 -> drawLine(color, Offset(x, 0f), Offset(x, size.height), w)
                    2, 3 -> {
                        drawLine(color, Offset(x, 0f), Offset(x, if (code == 3) size.height else y), w)
                        drawLine(color, Offset(x, y), Offset(x + step / 2, y), w)
                    }
                }
            }
        }
    }

private fun indent(guides: List<Int>): Dp = EDGE + STEP * guides.size

/** A touch target wider than the 16dp slot it sits in, centred on it. */
private fun Modifier.slot(width: Dp, touch: Dp): Modifier = layout { measurable, constraints ->
    val placeable = measurable.measure(constraints.copy(minWidth = touch.roundToPx(), maxWidth = touch.roundToPx()))
    layout(width.roundToPx(), placeable.height) {
        placeable.place((width.roundToPx() - touch.roundToPx()) / 2, 0)
    }
}

/** Add [rows] to a LazyColumn. The tree and the members sheet both use this. */
internal fun LazyListScope.roomTreeItems(
    rows: List<TreeRow>,
    state: AppState,
    fold: TreeFold,
    actions: RoomTreeActions,
) {
    items(rows, key = { it.key }) { row ->
        when (row) {
            is TreeRow.RoomNode -> RoomNodeRow(row, state, fold, actions)
            is TreeRow.Leaf -> LeafRow(row, fold, actions)
            is TreeRow.Member -> MemberRow(row, state, fold, actions)
            is TreeRow.More -> MoreRow(row, fold, actions)
        }
    }
}

/**
 * A supernode's row, heading its rooms: a cluster is one node. Tapping it
 * folds the rooms away; long-press has the rest. As on the desktop.
 */
internal fun LazyListScope.roomNodeItem(
    group: RoomNodeGroup,
    state: AppState,
    collapsed: Boolean,
    onToggle: () -> Unit,
    onOpenPortal: () -> Unit,
    onCreateRoom: () -> Unit,
) {
    item(key = "node:" + group.node.nodeId) {
        NodeRow(group, state, collapsed, onToggle, onOpenPortal, onCreateRoom)
    }
    if (!collapsed && group.rooms.isEmpty()) {
        item(key = "node-empty:" + group.node.nodeId) {
            val ds = LocalDsColors.current
            Text(
                "No rooms",
                color = ds.muted,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth().padding(start = EDGE + STEP, top = 10.dp, bottom = 10.dp),
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun NodeRow(
    group: RoomNodeGroup,
    state: AppState,
    collapsed: Boolean,
    onToggle: () -> Unit,
    onOpenPortal: () -> Unit,
    onCreateRoom: () -> Unit,
) {
    val ds = LocalDsColors.current
    val node = group.node
    val connected = node.isConnected(state.connectedSupernodes)
    val art = state.avatars[node.peerId.ifBlank { node.nodeId }]
    var menuOpen by remember(node.nodeId) { mutableStateOf(false) }
    Box {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .height(NODE_ROW)
                .background(ds.bg2)
                .combinedClickable(onClick = onToggle, onLongClick = { menuOpen = true })
                .semantics {
                    contentDescription =
                        (if (collapsed) "Show rooms on " else "Hide rooms on ") + node.label
                }
                .padding(start = EDGE, end = 12.dp),
        ) {
            Box(contentAlignment = Alignment.Center, modifier = Modifier.width(STEP)) {
                Chevron(!collapsed, 12.dp, ds.muted)
            }
            Spacer(Modifier.width(6.dp))
            // Green ring while connected.
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(30.dp)
                    .border(3.dp, if (connected) ds.online else ds.muted, RectangleShape)
                    .alpha(if (connected) 1f else 0.6f),
            ) {
                if (art != null) {
                    Avatar(art, Modifier.size(22.dp))
                } else {
                    Box(Modifier.size(22.dp).background(ds.bg3))
                }
            }
            Spacer(Modifier.width(10.dp))
            Text(
                node.label,
                color = if (connected) ds.text else ds.muted,
                fontWeight = FontWeight.Bold,
                fontSize = 15.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            // Folded, the row still says what is going on under it.
            if (collapsed) {
                val unread = group.rooms.sumOf { state.roomUnread[it.roomId] ?: 0 }
                if (unread > 0) {
                    Spacer(Modifier.width(6.dp))
                    Text(
                        if (unread > 99) "99+" else unread.toString(),
                        color = Color.White,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier
                            .background(ds.danger, RectangleShape)
                            .padding(horizontal = 6.dp, vertical = 1.dp)
                            .semantics { contentDescription = "$unread unread" },
                    )
                }
                val voice = nodeVoiceCount(group.rooms, state.roomVoiceRosters)
                if (connected && voice > 0) {
                    Spacer(Modifier.width(6.dp))
                    CountBadge(R.drawable.ds_headphone, voice, ds.online, "in voice", onClick = onToggle)
                }
            }
        }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            TreeMenuItem(
                if (collapsed) R.drawable.ds_eye else R.drawable.ds_eye_off,
                if (collapsed) "Show rooms" else "Hide rooms",
            ) {
                menuOpen = false
                onToggle()
            }
            TreeMenuItem(R.drawable.ds_globe, "Open Portal", enabled = connected) {
                menuOpen = false
                onOpenPortal()
            }
            HorizontalDivider()
            TreeMenuItem(R.drawable.ds_plus, "Create room here...", enabled = connected) {
                menuOpen = false
                onCreateRoom()
            }
        }
    }
}

@Composable
private fun Chevron(open: Boolean, size: Dp, tint: Color) {
    Icon(
        painterResource(R.drawable.ds_chevron),
        contentDescription = null,
        tint = tint,
        modifier = Modifier.size(size).rotate(if (open) 90f else 0f),
    )
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RoomNodeRow(row: TreeRow.RoomNode, state: AppState, fold: TreeFold, actions: RoomTreeActions) {
    val ds = LocalDsColors.current
    val room = row.room
    // By host as well as id: two nodes each list a `default` room.
    val reading = (state.screen as? com.doubleslash.client.Screen.RoomChat)?.room?.key == room.key
    val voiceHere = row.inVoice
    val unread = state.roomUnread[room.roomId] ?: 0
    val alertsOn = room.roomId in state.prefs.roomMessageAlerts
    var menuOpen by remember(room.key) { mutableStateOf(false) }
    var confirmHide by remember(room.key) { mutableStateOf(false) }
    var confirmDelete by remember(room.key) { mutableStateOf(false) }
    Box {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .height(ROOM_ROW)
                .background(if (reading) ds.selectedFill else Color.Transparent)
                .treeGuides(row.guides, ROOM_ROW / 2, ds.divider)
                .combinedClickable(
                    onClick = { actions.onOpenRoom(room) },
                    onLongClick = { menuOpen = true },
                )
                .padding(start = indent(row.guides), end = 12.dp),
        ) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .slot(STEP, 40.dp)
                    .height(ROOM_ROW)
                    .clickable(enabled = row.hasChildren) {
                        val key = room.key + ":voice"
                        actions.onFold(fold.copy(leaves = fold.leaves + (key to (fold.leaves[key] != true))))
                    }
                    .semantics {
                        contentDescription = if (row.collapsed) "Show voice members" else "Hide voice members"
                    },
            ) {
                if (row.hasChildren) Chevron(!row.collapsed, 12.dp, ds.muted)
            }
            Spacer(Modifier.width(6.dp))
            if (room.roomType.equals("private", ignoreCase = true)) {
                Icon(
                    painterResource(R.drawable.ds_lock),
                    contentDescription = "Private",
                    tint = ds.muted,
                    modifier = Modifier.size(14.dp),
                )
                Spacer(Modifier.width(6.dp))
            }
            // The name and its markers take the free width, so what follows
            // sits flush right whatever the name's length.
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.weight(1f)) {
                Text(
                    room.label,
                    color = if (voiceHere) ds.online else ds.text,
                    fontWeight = if (reading || voiceHere || unread > 0) FontWeight.Bold else FontWeight.Normal,
                    fontSize = 15.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (unread > 0) {
                    Spacer(Modifier.width(6.dp))
                    Text(
                        if (unread > 99) "99+" else unread.toString(),
                        color = Color.White,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Bold,
                        modifier = Modifier
                            .background(ds.danger, RectangleShape)
                            .padding(horizontal = 6.dp, vertical = 1.dp)
                            .semantics { contentDescription = "$unread unread" },
                    )
                }
                if (state.prefs.roomListOrder.isPinned(room)) {
                    Spacer(Modifier.width(6.dp))
                    Text("top", color = ds.accent, fontSize = 11.sp, fontWeight = FontWeight.Bold)
                }
                if (room.hidden) {
                    Spacer(Modifier.width(6.dp))
                    Text("hidden", color = ds.muted, fontSize = 12.sp)
                }
            }
            if (row.collapsed && row.voiceCount > 0) {
                CountBadge(
                    R.drawable.ds_headphone,
                    row.voiceCount,
                    ds.online,
                    "Show voice members",
                ) {
                    val key = room.key + ":voice"
                    actions.onFold(fold.copy(leaves = fold.leaves + (key to (fold.leaves[key] != true))))
                }
            }
            if (row.roomChat > 0) {
                Spacer(Modifier.width(6.dp))
                CountBadge(
                    R.drawable.ds_speech,
                    row.roomChat,
                    ds.accent,
                    if (row.textListsRoom) "Show who's in the room" else "Show text members",
                ) {
                    actions.onFold(fold.copy(leaves = fold.leaves + (room.key + ":text" to true)))
                }
            }
            // Join voice, as on the desktop. The green handset is on the open
            // room; this list has no hover. The hang-up on the live voice room
            // stays. Other rows join from the long-press menu.
            if (row.showCall && state.connectedSupernodes.isNotEmpty() &&
                (row.inVoice || (reading && !room.hidden))
            ) {
                Spacer(Modifier.width(4.dp))
                // Solid green on a green square, so joining stands out. In the
                // voice room we are in it turns into hang-up: the handset
                // inverted, in red, leaving as the voice dock's Leave does.
                val callColor = if (row.inVoice) ds.danger else ds.online
                TreeIcon(
                    if (row.inVoice) R.drawable.ds_phone_hangup else R.drawable.ds_phone,
                    if (row.inVoice) "Leave voice" else "Join voice",
                    tint = callColor,
                    tile = callColor.copy(alpha = 0.18f),
                    border = callColor,
                ) {
                    if (row.inVoice) actions.onLeaveVoice() else actions.onJoinVoice(room)
                }
            }
        }
        val order = state.prefs.roomListOrder
        val pinned = order.isPinned(room)
        val showMove = order.mode == ROOM_SORT_MANUAL || pinned
        val visibleRooms = state.rooms.filter { state.showHiddenRooms || !it.hidden }
        val canInviteContact = state.peers.inviteContacts().isNotEmpty()
        val listed = !room.hidden
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            TreeMenuItem(R.drawable.ds_phone, "Join Voice Room", enabled = listed) {
                menuOpen = false
                actions.onJoinVoice(room)
            }
            TreeMenuItem(R.drawable.ds_clipboard, "Copy Room Invite", enabled = listed) {
                menuOpen = false
                actions.onCopyInvite(room)
            }
            TreeMenuItem(
                R.drawable.ic_invite,
                "Invite Contact to Room",
                enabled = listed && canInviteContact,
            ) {
                menuOpen = false
                actions.onInviteContact(room)
            }
            HorizontalDivider()
            TreeMenuItem(R.drawable.ds_plus, "Create room inside ${room.label}...", enabled = listed) {
                menuOpen = false
                actions.onCreateSubRoom(room)
            }
            HorizontalDivider()
            TreeMenuItem(
                R.drawable.ds_pin,
                if (pinned) "Stop keeping at top" else "Keep at top",
                enabled = listed,
            ) {
                menuOpen = false
                actions.onTogglePin(room)
            }
            if (showMove) {
                TreeMenuItem(
                    R.drawable.ds_arrow_up,
                    "Move up",
                    enabled = listed && canMoveRoom(
                        order, visibleRooms, room, -1, state.roomVoiceRosters, state.roomTextRosters,
                    ),
                ) {
                    menuOpen = false
                    actions.onMoveRoom(room, -1)
                }
                TreeMenuItem(
                    R.drawable.ds_arrow_down,
                    "Move down",
                    enabled = listed && canMoveRoom(
                        order, visibleRooms, room, 1, state.roomVoiceRosters, state.roomTextRosters,
                    ),
                ) {
                    menuOpen = false
                    actions.onMoveRoom(room, 1)
                }
            }
            TreeMenuItem(
                if (alertsOn) R.drawable.ds_bell_off else R.drawable.ds_bell,
                if (alertsOn) "Mute message alerts" else "Enable message alerts",
                enabled = listed,
            ) {
                menuOpen = false
                actions.onSetMessageAlerts(room, !alertsOn)
            }
            // Hide only takes the room off this list. Remove deletes it here.
            // The built-in default can be hidden and has no Remove. A hidden
            // room keeps Show in list and Remove, and greys out everything above.
            val canRemove = room.roomId != "default"
            HorizontalDivider()
            if (room.hidden) {
                TreeMenuItem(R.drawable.ds_eye, "Show in list") {
                    actions.onSetHidden(room, false)
                    menuOpen = false
                }
            } else {
                TreeMenuItem(R.drawable.ds_eye_off, "Hide room") {
                    menuOpen = false
                    confirmHide = true
                }
            }
            if (canRemove) {
                TreeMenuItem(
                    R.drawable.ds_trash,
                    "Remove room",
                    color = ds.danger,
                ) {
                    menuOpen = false
                    confirmDelete = true
                }
            }
        }
        if (confirmHide) {
            DsDialog(
                onDismissRequest = { confirmHide = false },
                title = { Text("Are you sure?") },
                text = {
                    Text(
                        "This hides ${room.label} from your list on this device. " +
                            "The room stays on the supernode. Show hidden brings it back.",
                    )
                },
                confirmButton = {
                    SquareTextButton(onClick = {
                        confirmHide = false
                        actions.onSetHidden(room, true)
                    }) { Text("Hide", color = ds.danger) }
                },
                dismissButton = {
                    SquareTextButton(onClick = { confirmHide = false }) { Text("Cancel") }
                },
            )
        }
        if (confirmDelete) {
            DsDialog(
                onDismissRequest = { confirmDelete = false },
                title = { Text("Are you sure?") },
                text = {
                    Text(
                        "This removes ${room.label} from this device. Other people still " +
                            "have the room, and an invite can bring it back here. If you are " +
                            "in its voice, that call ends. Remove the chat history stored on " +
                            "this device as well, or only the room?",
                    )
                },
                // Stacked: "Only the room", "Room and history", and Cancel
                // do not fit one footer row on a phone.
                confirmButton = {
                    Column(
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                        horizontalAlignment = Alignment.End,
                    ) {
                        SquareTextButton(onClick = {
                            confirmDelete = false
                            actions.onDeleteRoom(room, false)
                        }) { Text("Only the room", color = ds.danger) }
                        SquareTextButton(onClick = {
                            confirmDelete = false
                            actions.onDeleteRoom(room, true)
                        }) { Text("Room and history", color = ds.danger) }
                        SquareTextButton(onClick = { confirmDelete = false }) { Text("Cancel") }
                    }
                },
            )
        }
    }
}

/** A room's name, or a short id when it never had one. */
private val Room.label: String get() = roomName.ifBlank { roomId.take(12) }

/**
 * An SVG icon action in the tree, named by its tooltip (long-press) and
 * content description. [checked] fills it, for a state that is on.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TreeIcon(
    icon: Int,
    label: String,
    tint: Color? = null,
    checked: Boolean = false,
    enabled: Boolean = true,
    /** The tile behind it, when the control has a colour of its own. */
    tile: Color? = null,
    border: Color? = null,
    onClick: () -> Unit,
) {
    val ds = LocalDsColors.current
    TooltipBox(
        positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        SquareIconButton(
            onClick = onClick,
            enabled = enabled,
            // A state that is on reads as a stronger tile.
            tile = tile ?: if (checked) ds.text.copy(alpha = 0.22f) else null,
            border = border,
            modifier = Modifier.size(40.dp),
        ) {
            Icon(
                painterResource(icon),
                contentDescription = label,
                tint = if (!enabled) ds.muted else tint ?: ds.text,
                modifier = Modifier.size(20.dp),
            )
        }
    }
}

/** A long-press menu entry with its SVG icon, as on the desktop's room menu. */
@Composable
private fun TreeMenuItem(
    icon: Int,
    label: String,
    enabled: Boolean = true,
    color: Color? = null,
    onClick: () -> Unit,
) {
    val ds = LocalDsColors.current
    val tint = when {
        !enabled -> ds.muted
        color != null -> color
        else -> ds.text
    }
    DropdownMenuItem(
        text = { Text(label, color = tint) },
        leadingIcon = {
            Icon(
                painterResource(icon),
                contentDescription = null,
                tint = tint,
                modifier = Modifier.size(20.dp),
            )
        },
        enabled = enabled,
        onClick = onClick,
    )
}

@Composable
internal fun CountBadge(
    icon: Int,
    count: Int,
    tint: Color,
    what: String,
    checked: Boolean = false,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        modifier = modifier
            .background(tint.copy(alpha = if (checked) 0.36f else 0.16f), RectangleShape)
            .border(1.dp, tint, RectangleShape)
            .clickable(onClick = onClick)
            .padding(horizontal = 7.dp, vertical = 2.dp)
            .semantics { contentDescription = "$count $what" },
    ) {
        Icon(painterResource(icon), contentDescription = null, tint = tint, modifier = Modifier.size(12.dp))
        Text("$count", color = tint, fontSize = 12.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun LeafRow(row: TreeRow.Leaf, fold: TreeFold, actions: RoomTreeActions) {
    val ds = LocalDsColors.current
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .height(LEAF_ROW)
            .listBox(row.box, ds)
            .treeGuides(row.guides, LEAF_ROW / 2, ds.divider)
            .padding(start = indent(row.guides), end = 12.dp),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .weight(1f)
                .height(LEAF_ROW)
                .clickable {
                    actions.onFold(fold.copy(leaves = fold.leaves + (row.key to !row.expanded)))
                }
                .semantics {
                    contentDescription = (if (row.expanded) "Collapse " else "Expand ") +
                        (when {
                            row.voice -> "voice"
                            row.listsRoom -> "room"
                            else -> "text-only"
                        }) + " members"
                },
        ) {
            // An open list reads in full; a folded one recedes.
            val strength = if (row.expanded) 1f else 0.6f
            Box(Modifier.width(STEP).alpha(strength), contentAlignment = Alignment.Center) {
                Chevron(row.expanded, 10.dp, if (row.expanded) ds.text else ds.muted)
            }
            Spacer(Modifier.width(6.dp))
            Icon(
                painterResource(if (row.voice) R.drawable.ds_headphone else R.drawable.ds_speech),
                contentDescription = null,
                tint = if (row.voice) ds.online else ds.linkPeer,
                modifier = Modifier.size(14.dp).alpha(strength),
            )
            Spacer(Modifier.width(6.dp))
            Text(
                "${row.count}",
                color = if (row.expanded) ds.text else ds.muted,
                fontWeight = if (row.expanded) FontWeight.Bold else FontWeight.Normal,
                fontSize = 12.sp,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemberRow(row: TreeRow.Member, state: AppState, fold: TreeFold, actions: RoomTreeActions) {
    val ds = LocalDsColors.current
    val key = row.id.videoKey()
    val isSelf = key == state.identity.publicId.videoKey()
    val trusted = state.trustedPeer(row.id)
    // We are not in our own peer list, so a lookup there gave our own row a
    // shortened key instead of our name.
    val name = if (isSelf) {
        state.identity.handle.ifEmpty { state.peers.roomSenderName(row.id, "") } + " (you)"
    } else {
        state.peers.roomSenderName(row.id, "")
    }
    val speaking = row.inSession && key in state.speakingPeers && !(isSelf && state.muted)
    val cameraOn = row.voice && row.onRoster && state.cameraOn(row.id)
    val watching = state.watching(row.id)
    val canWatch = row.inSession && !isSelf && (cameraOn || watching)
    val audio = state.peerAudio[key]
    val locallyMuted = row.inSession && audio?.muted == true
    val inviteState = state.trustInvites[key].orEmpty()
    val open = fold.openMember == row.key && !isSelf
    // Trusted peers can be messaged; anyone else can be offered trust, from a
    // room we are in. Only one of the two ever shows.
    val canOfferTrust = trusted == null && row.canInvite
    val hasIcons = canWatch || trusted != null || canOfferTrust
    val edgeX = indent(row.guides) - 2.dp

    Column(
        modifier = Modifier
            .fillMaxWidth()
            // Open: tinted, with an accent edge down the row and its panel, so
            // it is plain which person the panel belongs to. As on the desktop.
            .listBox(row.box, ds)
            .background(if (open) ds.accent.copy(alpha = 0.10f) else Color.Transparent)
            .drawBehind {
                if (open) {
                    drawRect(
                        ds.accent,
                        topLeft = Offset(edgeX.toPx(), 0f),
                        size = Size(2.dp.toPx(), size.height),
                    )
                }
            }
            .treeGuides(row.guides, MEMBER_ROW / 2, ds.divider),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .height(MEMBER_ROW)
                .clickable(enabled = !isSelf) {
                    actions.onFold(fold.copy(openMember = if (open) "" else row.key))
                }
                .padding(start = indent(row.guides) + 2.dp, end = 4.dp)
                .semantics { contentDescription = if (isSelf) name else "Actions for $name" },
        ) {
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier.size(32.dp).alpha(if (row.voice) 1f else 0.6f),
            ) {
                val ring = when {
                    locallyMuted -> ds.danger
                    speaking -> ds.online
                    else -> Color.Transparent
                }
                Box(Modifier.size(32.dp).border(2.5.dp, ring, RectangleShape))
                val art = state.avatars[row.id]
                if (art != null) {
                    Box(Modifier.size(26.dp).clip(RectangleShape)) { Avatar(art, Modifier.size(26.dp)) }
                } else {
                    Box(Modifier.size(26.dp).background(ds.bg3, RectangleShape))
                }
                if (isSelf && row.inSession && state.muted) {
                    Box(
                        contentAlignment = Alignment.Center,
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .offset(x = 2.dp, y = 2.dp)
                            .size(14.dp)
                            .background(ds.danger, RectangleShape),
                    ) {
                        Icon(
                            painterResource(R.drawable.ds_mic_off),
                            contentDescription = "Muted",
                            tint = Color.White,
                            modifier = Modifier.size(9.dp),
                        )
                    }
                }
            }
            Spacer(Modifier.width(10.dp))
            Text(
                name,
                color = if (trusted == null && !isSelf) ds.muted else ds.text,
                fontWeight = if (speaking || open) FontWeight.Bold else FontWeight.Normal,
                // Not someone we trust: the name is only what the room says.
                fontStyle = if (trusted == null && !isSelf) FontStyle.Italic else FontStyle.Normal,
                fontSize = 14.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).alpha(if (row.voice) 1f else 0.6f),
            )
            if (cameraOn || watching) {
                Box(
                    contentAlignment = Alignment.Center,
                    modifier = Modifier
                        .size(44.dp)
                        .clickable(enabled = canWatch) { actions.members.onToggleWatch(row.id) }
                        .semantics {
                            contentDescription = when {
                                watching -> "Watching $name. Tap to stop"
                                canWatch -> "$name is streaming. Tap to watch"
                                else -> "$name is streaming. Join voice to watch"
                            }
                        },
                ) {
                    Box(
                        contentAlignment = Alignment.Center,
                        modifier = Modifier
                            .size(28.dp)
                            .background(if (watching) ds.accent else Color.Transparent, RectangleShape)
                            .border(1.dp, ds.accent, RectangleShape),
                    ) {
                        Icon(
                            painterResource(R.drawable.ds_video),
                            contentDescription = null,
                            tint = if (watching) ds.textInv else ds.text,
                            modifier = Modifier.size(15.dp),
                        )
                    }
                }
            }
            // Expand state: right while shut, down while open. Our own row
            // does not open, so it has none.
            if (!isSelf) {
                Box(Modifier.size(24.dp).alpha(if (open) 1f else 0.4f), contentAlignment = Alignment.Center) {
                    Chevron(open, 10.dp, ds.text)
                }
            }
        }

        if (open) {
            // A framed panel: the volume first, then one icon per action,
            // named by its tooltip and content description.
            Column(
                verticalArrangement = Arrangement.spacedBy(2.dp),
                modifier = Modifier
                    .padding(start = indent(row.guides) + 44.dp, end = 12.dp, bottom = 10.dp)
                    .fillMaxWidth()
                    .background(ds.bg1, RectangleShape)
                    .border(1.dp, ds.divider, RectangleShape)
                    .padding(4.dp),
            ) {
                if (row.inSession) {
                    var volume by remember(key) { mutableFloatStateOf((audio?.volume ?: 100).toFloat()) }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        // Mute for me, beside the volume it zeroes.
                        TreeIcon(
                            if (locallyMuted) R.drawable.ds_speaker_off else R.drawable.ds_speaker,
                            if (locallyMuted) "Unmute for me" else "Mute for me",
                            tint = if (locallyMuted) ds.danger else ds.text,
                            checked = locallyMuted,
                        ) {
                            actions.onPeerAudio(row.id, !locallyMuted, audio?.volume?.takeIf { it > 0 } ?: 100)
                        }
                        Slider(
                            value = if (locallyMuted) 0f else volume,
                            onValueChange = { volume = it },
                            // Moving it off zero unmutes, so the slider and the
                            // mute never disagree about whether you hear them.
                            onValueChangeFinished = {
                                val v = volume.toInt()
                                actions.onPeerAudio(row.id, v == 0, v)
                            },
                            valueRange = 0f..200f,
                            steps = 39,
                            // Square, like the desktop's: the UI is angular.
                            thumb = {
                                Box(Modifier.size(18.dp).background(ds.accent, RectangleShape))
                            },
                            modifier = Modifier
                                .weight(1f)
                                .padding(horizontal = 4.dp)
                                .semantics { contentDescription = "Volume for $name" },
                        )
                        Text(
                            "${if (locallyMuted) 0 else volume.toInt()}%",
                            color = ds.muted,
                            fontSize = 12.sp,
                            modifier = Modifier.width(40.dp),
                        )
                    }
                }
                if (hasIcons) {
                    Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                        if (canWatch) {
                            TreeIcon(
                                if (watching) R.drawable.ds_video_off else R.drawable.ds_video,
                                if (watching) "Stop watching" else "Watch video",
                                tint = if (watching) ds.text else ds.accent,
                                checked = watching,
                            ) { actions.members.onToggleWatch(row.id) }
                        }
                        if (trusted != null) {
                            TreeIcon(R.drawable.ds_speech, "Message") { actions.members.onMessage(trusted) }
                        } else if (canOfferTrust) {
                            TreeIcon(
                                if (inviteState == "sent") R.drawable.ds_check else R.drawable.ic_invite,
                                when (inviteState) {
                                    "sent" -> "Invite sent"
                                    "pending" -> "Sending invite..."
                                    else -> "Invite to trusted peers"
                                },
                                tint = if (inviteState.isEmpty()) ds.accent else ds.muted,
                                enabled = inviteState.isEmpty(),
                            ) { actions.members.onInvite(row.room.roomId, row.id) }
                        }
                    }
                } else if (!row.inSession) {
                    // Nothing applies: say so rather than open an empty panel.
                    Text(
                        "Open this room to offer them trust.",
                        color = ds.muted,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(6.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun MoreRow(row: TreeRow.More, fold: TreeFold, actions: RoomTreeActions) {
    val ds = LocalDsColors.current
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .height(LEAF_ROW)
            .listBox(row.box, ds)
            .treeGuides(row.guides, LEAF_ROW / 2, ds.divider)
            .padding(start = indent(row.guides)),
    ) {
        SquareTextButton(onClick = {
            val open = row.overflowKey in fold.overflow
            actions.onFold(
                fold.copy(overflow = if (open) fold.overflow - row.overflowKey else fold.overflow + row.overflowKey),
            )
        }) { Text(row.label, color = ds.linkPeer, fontSize = 13.sp) }
    }
}

/** The voice room as the `Room` the tree and screens use, when we know it. */
internal fun AppState.voiceRoomEntry(): Room? {
    val v = voiceRoom ?: return null
    return rooms.firstOrNull { it.roomId == v.roomId && it.supernodeId == v.supernodeId }
        ?: rooms.firstOrNull { it.roomId == v.roomId }
        ?: Room(roomId = v.roomId, roomName = v.roomName, supernodeId = v.supernodeId)
}

/** What the member actions can do, supplied once by the screen. */
internal class MemberActions(
    val onToggleWatch: (String) -> Unit,
    val onMessage: (com.doubleslash.client.Peer) -> Unit,
    val onInvite: (roomId: String, memberId: String) -> Unit,
)

/**
 * An elongated hexagon, or half of one when an end is flat: the Peers | Rooms
 * toggle, drawn as on the desktop. The UI is angular throughout. A pointed end
 * has a 120° corner, as in a regular hexagon: it reaches in half the height
 * times tan 30°.
 */
internal fun hexagonShape(pointLeft: Boolean, pointRight: Boolean): Shape = GenericShape { size, _ ->
    val h = size.height
    val d = h / (2f * sqrt(3f))
    moveTo(if (pointLeft) d else 0f, 0f)
    lineTo(if (pointRight) size.width - d else size.width, 0f)
    if (pointRight) lineTo(size.width, h / 2f)
    lineTo(if (pointRight) size.width - d else size.width, h)
    lineTo(if (pointLeft) d else 0f, h)
    if (pointLeft) lineTo(0f, h / 2f)
    close()
}

/**
 * Peers | Rooms, beside the logo in the top bar.
 *
 * The desktop's title-bar toggle on the phone: the one switch between the two
 * lists, in the same place on both, so neither has a tab bar at the bottom.
 */
@Composable
internal fun ListToggle(rooms: Boolean, onSelect: (rooms: Boolean) -> Unit) {
    val ds = LocalDsColors.current
    val segmentHeight = 36.dp
    // How far a pointed end reaches in, so the label clears it.
    val depth = segmentHeight / (2f * sqrt(3f))
    Row(
        modifier = Modifier
            .border(1.dp, ds.divider, hexagonShape(pointLeft = true, pointRight = true))
            .background(ds.bg2, hexagonShape(pointLeft = true, pointRight = true))
            .padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        listOf(false to "Peers", true to "Rooms").forEach { (isRooms, label) ->
            val selected = rooms == isRooms
            // Pointed on its outer end, flat where the two segments meet.
            val shape = hexagonShape(pointLeft = !isRooms, pointRight = isRooms)
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .height(segmentHeight)
                    .background(if (selected) ds.accent else Color.Transparent, shape)
                    .clickable { onSelect(isRooms) }
                    .padding(
                        start = 12.dp + if (isRooms) 0.dp else depth,
                        end = 12.dp + if (isRooms) depth else 0.dp,
                    )
                    .semantics { contentDescription = if (selected) "$label, selected" else label },
            ) {
                Icon(
                    painterResource(if (isRooms) R.drawable.ds_headphone else R.drawable.ds_peers),
                    contentDescription = null,
                    tint = if (selected) ds.textInv else ds.text,
                    modifier = Modifier.size(16.dp),
                )
                Spacer(Modifier.width(6.dp))
                Text(
                    label,
                    color = if (selected) ds.textInv else ds.text,
                    fontSize = 14.sp,
                    fontWeight = if (selected) FontWeight.Bold else FontWeight.Normal,
                )
            }
        }
    }
}

/**
 * Watched video when its room is not on screen.
 *
 * The room screen shows watched video full width at its top; anywhere else
 * the first watched stream floats here, so a stream you opened never quietly
 * disappears behind navigation. Tapping it goes back to the room.
 */
@Composable
internal fun FloatingVideo(state: AppState, onOpen: () -> Unit, modifier: Modifier = Modifier) {
    val voice = state.voiceRoom ?: return
    val onVoiceRoom = (state.screen as? com.doubleslash.client.Screen.RoomChat)?.room?.roomId == voice.roomId
    if (onVoiceRoom) return
    val myKey = state.identity.publicId.videoKey()
    val shown = state.roomVoiceRosters.roomMembersUnion(voice.roomId)
        .firstOrNull { it.videoKey() != myKey && state.watching(it) && state.cameraOn(it) } ?: return
    VideoTile(
        peerId = shown,
        name = state.peers.roomSenderName(shown, ""),
        size = state.videoSizes[shown.videoKey()],
        stalled = shown.videoKey() in state.stalledVideo,
        modifier = modifier.width(176.dp).border(1.dp, LocalDsColors.current.bg3),
        onClick = onOpen,
    )
}
