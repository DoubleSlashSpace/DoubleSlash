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
 * Sub-rooms stay listed. A room's own voice and text lists start collapsed.
 * [expanded] opens those lists. [collapsed] shuts them, and that wins over the
 * live voice room and the open chat. An open room shows its voice people,
 * including anyone also on video, and its text-only people together. Folding
 * the voice room again stays shut until the next join. The open chat expands
 * its room only while that chat is selected.
 */
internal data class TreeFold(
    /** Rooms the user folded shut, by [Room.key]. Wins over voice auto-open. */
    val collapsed: Set<String> = emptySet(),
    /** Rooms the user opened, by [Room.key]. */
    val expanded: Set<String> = emptySet(),
    /** Leaves opened or closed by hand, `"<room key>:voice|text"`. */
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
        /** Who is inside a folded room, for its stacked avatars. */
        val stack: List<String>,
        val subtreeVoice: Int,
        val roomChat: Int,
        /** The call icon that joins this room's voice: not while we are in it. */
        val showCall: Boolean = false,
    ) : TreeRow

    data class Leaf(
        override val key: String,
        override val guides: List<Int>,
        val room: Room,
        val voice: Boolean,
        val count: Int,
        val expanded: Boolean,
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
    ) : TreeRow

    data class More(
        override val key: String,
        override val guides: List<Int>,
        val label: String,
        val overflowKey: String,
    ) : TreeRow
}

/** What sits under a room, in order: its leaves, then its sub-rooms. */
private sealed interface TreeItem {
    data class LeafItem(val voice: Boolean, val members: List<String>) : TreeItem
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
 */
data class RoomListOrder(
    val mode: String = ROOM_SORT_NAME_ASC,
    val pinned: List<String> = emptyList(),
    val manual: List<String> = emptyList(),
) {
    fun normalized(): RoomListOrder = copy(
        mode = normalizeRoomSortMode(mode),
        pinned = pinned.filter { it.isNotEmpty() },
        manual = manual.filter { it.isNotEmpty() },
    )

    fun isPinned(room: Room): Boolean = room.listOrderKey() in pinned
}

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
        buildRoomTree(
            rooms = rooms,
            voiceRosters = voiceRosters,
            textRosters = textRosters,
            reading = null,
            voiceRoom = null,
            fold = TreeFold(),
            order = current,
        ).mapNotNull { (it as? TreeRow.RoomNode)?.room?.listOrderKey() }
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
    rooms: List<Room>,
    room: Room,
    delta: Int,
    voiceRosters: Map<String, List<String>>,
    textRosters: Map<String, List<String>>,
): RoomListOrder {
    val o = order.normalized()
    if (delta != -1 && delta != 1) return o
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
    fun textOf(r: Room, voice: List<String>): List<String> {
        val inVoice = voice.map { it.videoKey() }.toSet()
        return textRosters.roomMembersUnion(r.roomId).filter { it.videoKey() !in inVoice }
    }
    val orderNorm = order.normalized()
    val children = grouped.mapValues { (_, list) ->
        list.sortedWith { a, b ->
            compareRooms(a, b, orderNorm) { roomPeerCount(it, voiceRosters, textRosters) }
        }
    }
    fun emitLeaf(r: Room, voice: Boolean, members: List<String>, pass: List<Boolean>?, last: Boolean) {
        val leafKey = r.key + ":" + if (voice) "voice" else "text"
        val isReading = reading != null && r.roomId == reading.roomId && r.supernodeId == reading.supernodeId
        val inThisVoice = voiceRoom != null && r.roomId == voiceRoom.roomId && r.supernodeId == voiceRoom.supernodeId
        val isSession = voice && inThisVoice
        // The room is already open, so both lists show their people. A stored
        // flag is only a hand fold of that one list.
        val expanded = fold.leaves[leafKey] ?: true
        out += TreeRow.Leaf(
            key = leafKey,
            // `pass == null`: the leaf is itself top level (the members sheet).
            guides = if (pass == null) emptyList() else codes(pass, last),
            room = r,
            voice = voice,
            count = members.size,
            expanded = expanded,
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
            )
        }
        if (more.isNotEmpty()) {
            out += TreeRow.More("$leafKey:more", codes(gPass, true), more, leafKey)
        }
    }

    fun emitRoom(r: Room, depth: Int, pass: List<Boolean>, last: Boolean, visible: Boolean) {
        if (!seen.add(r.roomId)) return
        val kids = children[r.roomId].orEmpty()
        val voice = voiceOf(r)
        val text = textOf(r, voice)
        // The chevron folds this room's member lists. Sub-rooms stay listed
        // either way, so a room of only sub-rooms has nothing to fold.
        val hasMembers = voice.isNotEmpty() || text.isNotEmpty()
        val forcedShut = r.key in fold.collapsed
        val forcedOpen = r.key in fold.expanded
        val isVoiceRoom = voiceRoom != null && r.roomId == voiceRoom.roomId &&
            r.supernodeId == voiceRoom.supernodeId
        val isReading = reading != null && r.roomId == reading.roomId && r.supernodeId == reading.supernodeId
        // A shut flag wins. Otherwise an explicit open, the live voice room,
        // or the open chat shows this room's lists.
        val listsOpen = !forcedShut && (forcedOpen || isVoiceRoom || isReading)
        val collapsed = hasMembers && !listsOpen
        if (visible) {
            val ownVoice = if (collapsed) voice else emptyList()
            out += TreeRow.RoomNode(
                key = r.key,
                guides = if (depth > 0) codes(pass, last) else emptyList(),
                room = r,
                hasChildren = hasMembers,
                collapsed = collapsed,
                stack = ownVoice.take(3),
                subtreeVoice = ownVoice.size,
                roomChat = if (collapsed) text.size else 0,
                showCall = !isVoiceRoom,
            )
        }
        val childPass = if (depth > 0) pass + !last else emptyList()
        // Leaves first, then sub-rooms: who is here reads before where to go.
        val items = buildList<TreeItem> {
            if (listsOpen && voice.isNotEmpty()) add(TreeItem.LeafItem(true, voice))
            if (listsOpen && text.isNotEmpty()) add(TreeItem.LeafItem(false, text))
            kids.forEach { add(TreeItem.SubRoom(it)) }
        }
        items.forEachIndexed { i, item ->
            val isLast = i == items.lastIndex
            when (item) {
                is TreeItem.SubRoom -> emitRoom(item.room, depth + 1, childPass, isLast, visible)
                is TreeItem.LeafItem ->
                    if (visible) emitLeaf(r, item.voice, item.members, childPass, isLast)
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
    val rows = buildRoomTree(
        rooms = listOf(room.copy(parentId = "")),
        voiceRosters = voiceRosters,
        textRosters = textRosters,
        reading = room,
        voiceRoom = voiceRoom,
        // The sheet is this room's people, so the room itself stays open even
        // when the sidebar has it folded.
        fold = fold.copy(collapsed = fold.collapsed - room.key, expanded = fold.expanded + room.key),
    )
    return rows.drop(1).map { row ->
        // One column less: the leaves sit at the sheet's edge.
        when (row) {
            is TreeRow.Leaf -> row.copy(guides = row.guides.drop(1))
            is TreeRow.Member -> row.copy(guides = row.guides.drop(1))
            is TreeRow.More -> row.copy(guides = row.guides.drop(1))
            is TreeRow.RoomNode -> row
        }
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────────

private val STEP = 16.dp
private val EDGE = 8.dp
private val ROOM_ROW = 48.dp
private val LEAF_ROW = 40.dp
private val MEMBER_ROW = 44.dp

/** What the tree's rows can do, supplied once by the screen. */
internal class RoomTreeActions(
    val onFold: (TreeFold) -> Unit,
    val onOpenRoom: (Room) -> Unit,
    /** Join a room's voice: runs the microphone explanation first. */
    val onJoinVoice: (Room) -> Unit,
    val onSetHidden: (Room, Boolean) -> Unit,
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
    val reading = (state.screen as? com.doubleslash.client.Screen.RoomChat)?.room?.roomId == room.roomId
    val voiceHere = state.voiceRoom?.roomId == room.roomId
    val unread = state.roomUnread[room.roomId] ?: 0
    val alertsOn = room.roomId in state.prefs.roomMessageAlerts
    var menuOpen by remember(room.key) { mutableStateOf(false) }
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
                        val next = if (row.collapsed) {
                            fold.copy(
                                collapsed = fold.collapsed - room.key,
                                expanded = fold.expanded + room.key,
                            )
                        } else {
                            fold.copy(
                                expanded = fold.expanded - room.key,
                                collapsed = fold.collapsed + room.key,
                            )
                        }
                        actions.onFold(next)
                    }
                    .semantics { contentDescription = if (row.collapsed) "Expand ${room.label}" else "Collapse ${room.label}" },
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
            if (voiceHere) {
                Spacer(Modifier.width(6.dp))
                Icon(
                    painterResource(R.drawable.ds_headphone),
                    contentDescription = "You are in voice here",
                    tint = ds.online,
                    modifier = Modifier.size(16.dp),
                )
            }
            if (room.hidden) {
                Spacer(Modifier.width(6.dp))
                Text("hidden", color = ds.muted, fontSize = 12.sp)
            }
            Spacer(Modifier.weight(1f))
            if (row.collapsed && row.stack.isNotEmpty()) {
                // Overlapped, so a folded room shows who is inside in little room.
                Box(Modifier.width(18.dp + 12.dp * (row.stack.size - 1)).height(18.dp)) {
                    row.stack.forEachIndexed { i, id ->
                        state.avatars[id]?.let {
                            Box(Modifier.offset(x = 12.dp * i).size(18.dp).clip(RectangleShape)) {
                                Avatar(it, Modifier.size(18.dp))
                            }
                        }
                    }
                }
                Spacer(Modifier.width(6.dp))
            }
            if (row.collapsed && row.subtreeVoice > 0) {
                CountBadge(R.drawable.ds_headphone, row.subtreeVoice, ds.online, "in voice")
            }
            if (row.collapsed && row.roomChat > 0) {
                Spacer(Modifier.width(6.dp))
                CountBadge(R.drawable.ds_speech, row.roomChat, ds.accent, "in text only")
            }
            // Join voice, explicitly, as on the desktop: a call icon on the row
            // rather than a button under its Voice list.
            if (row.showCall && state.connectedSupernodes.isNotEmpty()) {
                Spacer(Modifier.width(4.dp))
                // Solid green on a green square, so joining stands out.
                TreeIcon(
                    R.drawable.ds_phone,
                    "Join voice",
                    tint = ds.online,
                    tile = ds.online.copy(alpha = 0.18f),
                    border = ds.online,
                ) {
                    actions.onJoinVoice(room)
                }
            }
        }
        val order = state.prefs.roomListOrder
        val pinned = order.isPinned(room)
        val showMove = order.mode == ROOM_SORT_MANUAL || pinned
        val visibleRooms = state.rooms.filter { state.showHiddenRooms || !it.hidden }
        val canInviteContact = state.peers.inviteContacts().isNotEmpty()
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            TreeMenuItem(R.drawable.ds_phone, "Join Voice Room") {
                menuOpen = false
                actions.onJoinVoice(room)
            }
            TreeMenuItem(R.drawable.ds_clipboard, "Copy Room Invite") {
                menuOpen = false
                actions.onCopyInvite(room)
            }
            TreeMenuItem(R.drawable.ic_invite, "Invite Contact to Room", enabled = canInviteContact) {
                menuOpen = false
                actions.onInviteContact(room)
            }
            HorizontalDivider()
            TreeMenuItem(R.drawable.ds_plus, "Create room inside ${room.label}...") {
                menuOpen = false
                actions.onCreateSubRoom(room)
            }
            HorizontalDivider()
            TreeMenuItem(R.drawable.ds_pin, if (pinned) "Stop keeping at top" else "Keep at top") {
                menuOpen = false
                actions.onTogglePin(room)
            }
            if (showMove) {
                TreeMenuItem(
                    R.drawable.ds_arrow_up,
                    "Move up",
                    enabled = canMoveRoom(
                        order, visibleRooms, room, -1, state.roomVoiceRosters, state.roomTextRosters,
                    ),
                ) {
                    menuOpen = false
                    actions.onMoveRoom(room, -1)
                }
                TreeMenuItem(
                    R.drawable.ds_arrow_down,
                    "Move down",
                    enabled = canMoveRoom(
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
            ) {
                menuOpen = false
                actions.onSetMessageAlerts(room, !alertsOn)
            }
            HorizontalDivider()
            TreeMenuItem(
                if (room.hidden) R.drawable.ds_eye else R.drawable.ds_eye_off,
                if (room.hidden) "Show in list" else "Hide from list",
            ) {
                actions.onSetHidden(room, !room.hidden)
                menuOpen = false
            }
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
private fun TreeMenuItem(icon: Int, label: String, enabled: Boolean = true, onClick: () -> Unit) {
    val ds = LocalDsColors.current
    DropdownMenuItem(
        text = { Text(label) },
        leadingIcon = {
            Icon(
                painterResource(icon),
                contentDescription = null,
                tint = if (enabled) ds.text else ds.muted,
                modifier = Modifier.size(20.dp),
            )
        },
        enabled = enabled,
        onClick = onClick,
    )
}

@Composable
private fun CountBadge(icon: Int, count: Int, tint: Color, what: String) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier
            .background(tint.copy(alpha = 0.16f), RectangleShape)
            .border(1.dp, tint, RectangleShape)
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
                        (if (row.voice) "voice" else "text-only") + " members"
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
    val name = state.peers.roomSenderName(row.id, "").let { if (isSelf) "$it (you)" else it }
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
