package com.doubleslash.client.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
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
import com.doubleslash.client.roomMembersUnion
import com.doubleslash.client.roomSenderName
import com.doubleslash.client.trustedPeer
import com.doubleslash.client.videoKey
import com.doubleslash.client.watching

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
    ) : TreeRow

    data class Leaf(
        override val key: String,
        override val guides: List<Int>,
        val room: Room,
        val voice: Boolean,
        val count: Int,
        val expanded: Boolean,
        val showJoin: Boolean,
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
): List<TreeRow> {
    val byId = rooms.associateBy { it.roomId }
    // Empty, self-referential, the Space itself or a room we do not have all
    // mean "top level", as in layOutSpaceTree before it.
    val children = rooms.groupBy { r ->
        val p = r.parentId
        if (p.isBlank() || p == r.roomId || p == r.spaceId || p !in byId) "" else p
    }.mapValues { (_, list) -> list.sortedBy { it.roomName.lowercase() } }

    val out = mutableListOf<TreeRow>()
    val seen = mutableSetOf<String>()

    fun voiceOf(r: Room) = voiceRosters.roomMembersUnion(r.roomId)
    fun textOf(r: Room, voice: List<String>): List<String> {
        val inVoice = voice.map { it.videoKey() }.toSet()
        return textRosters.roomMembersUnion(r.roomId).filter { it.videoKey() !in inVoice }
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
            showJoin = voice && !isSession,
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
    rooms.filter { it.roomId !in seen }.sortedBy { it.roomName.lowercase() }
        .forEach { emitRoom(it, 0, emptyList(), true, true) }
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
                fontWeight = if (reading || voiceHere) FontWeight.Bold else FontWeight.Normal,
                fontSize = 15.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
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
                            Box(Modifier.offset(x = 12.dp * i).size(18.dp).clip(CircleShape)) {
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
        }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            DropdownMenuItem(
                text = { Text("Create room inside ${room.label}...") },
                onClick = {
                    menuOpen = false
                    actions.onCreateSubRoom(room)
                },
            )
            DropdownMenuItem(
                text = { Text(if (room.hidden) "Show in list" else "Hide from list") },
                onClick = {
                    actions.onSetHidden(room, !room.hidden)
                    menuOpen = false
                },
            )
        }
    }
}

/** A room's name, or a short id when it never had one. */
private val Room.label: String get() = roomName.ifBlank { roomId.take(12) }

@Composable
private fun CountBadge(icon: Int, count: Int, tint: Color, what: String) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier
            .background(tint.copy(alpha = 0.16f), RoundedCornerShape(11.dp))
            .border(1.dp, tint, RoundedCornerShape(11.dp))
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
            Box(Modifier.width(STEP), contentAlignment = Alignment.Center) {
                Chevron(row.expanded, 10.dp, ds.muted)
            }
            Spacer(Modifier.width(6.dp))
            Icon(
                painterResource(if (row.voice) R.drawable.ds_headphone else R.drawable.ds_speech),
                contentDescription = null,
                tint = if (row.voice) ds.online else ds.linkPeer,
                modifier = Modifier.size(14.dp),
            )
            Spacer(Modifier.width(6.dp))
            Text("${row.count}", color = ds.muted, fontSize = 12.sp)
        }
        if (row.showJoin) {
            OutlinedButton(
                onClick = { actions.onJoinVoice(row.room) },
                contentPadding = PaddingValues(horizontal = 14.dp),
                colors = ButtonDefaults.outlinedButtonColors(
                    containerColor = ds.online.copy(alpha = 0.16f),
                    contentColor = ds.online,
                ),
                border = androidx.compose.foundation.BorderStroke(1.dp, ds.online),
                shape = RoundedCornerShape(0.dp),
                modifier = Modifier.height(32.dp),
            ) { Text("Join", fontWeight = FontWeight.Bold, fontSize = 13.sp) }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MemberRow(row: TreeRow.Member, state: AppState, fold: TreeFold, actions: RoomTreeActions) {
    val ds = LocalDsColors.current
    val clipboard = LocalClipboardManager.current
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

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(if (open) ds.bg2 else Color.Transparent)
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
                Box(Modifier.size(32.dp).border(2.5.dp, ring, CircleShape))
                val art = state.avatars[row.id]
                if (art != null) {
                    Box(Modifier.size(26.dp).clip(CircleShape)) { Avatar(art, Modifier.size(26.dp)) }
                } else {
                    Box(Modifier.size(26.dp).background(ds.bg3, CircleShape))
                }
                if (isSelf && row.inSession && state.muted) {
                    Box(
                        contentAlignment = Alignment.Center,
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .offset(x = 2.dp, y = 2.dp)
                            .size(14.dp)
                            .background(ds.danger, CircleShape),
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
                fontWeight = if (speaking) FontWeight.Bold else FontWeight.Normal,
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
                            .background(if (watching) ds.accent else Color.Transparent, CircleShape)
                            .border(1.dp, ds.accent, CircleShape),
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
        }

        if (open) {
            Column(
                verticalArrangement = Arrangement.spacedBy(10.dp),
                modifier = Modifier.padding(start = indent(row.guides) + 44.dp, end = 12.dp, bottom = 12.dp),
            ) {
                if (row.inSession) {
                    var volume by remember(key) { mutableFloatStateOf((audio?.volume ?: 100).toFloat()) }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            painterResource(R.drawable.ds_speaker),
                            contentDescription = null,
                            tint = ds.muted,
                            modifier = Modifier.size(16.dp),
                        )
                        Slider(
                            value = volume,
                            onValueChange = { volume = it },
                            // Moving it off zero unmutes, so the slider and the
                            // mute never disagree about whether you hear them.
                            onValueChangeFinished = {
                                val v = volume.toInt()
                                actions.onPeerAudio(row.id, v == 0, v)
                            },
                            valueRange = 0f..200f,
                            steps = 39,
                            modifier = Modifier.weight(1f).padding(horizontal = 8.dp)
                                .semantics { contentDescription = "Volume for $name" },
                        )
                        Text("${volume.toInt()}%", color = ds.muted, fontSize = 12.sp, modifier = Modifier.width(40.dp))
                    }
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (canWatch) {
                        ActionButton(if (watching) "Stop watching" else "Watch video", primary = !watching) {
                            actions.members.onToggleWatch(row.id)
                        }
                    }
                    if (row.inSession) {
                        ActionButton(if (locallyMuted) "Unmute for me" else "Mute for me") {
                            actions.onPeerAudio(row.id, !locallyMuted, audio?.volume?.takeIf { it > 0 } ?: 100)
                        }
                    }
                    if (trusted != null) {
                        ActionButton("Message") { actions.members.onMessage(trusted) }
                    } else if (row.canInvite) {
                        ActionButton(
                            when (inviteState) {
                                "sent" -> "Invite sent"
                                "pending" -> "Sending invite..."
                                else -> "Invite to trusted peers"
                            },
                            primary = inviteState.isEmpty(),
                            enabled = inviteState.isEmpty(),
                        ) { actions.members.onInvite(row.room.roomId, row.id) }
                    }
                    ActionButton("Copy ID") { clipboard.setText(AnnotatedString(row.id)) }
                }
            }
        }
    }
}

@Composable
private fun ActionButton(label: String, primary: Boolean = false, enabled: Boolean = true, onClick: () -> Unit) {
    val ds = LocalDsColors.current
    Button(
        onClick = onClick,
        enabled = enabled,
        shape = RoundedCornerShape(0.dp),
        contentPadding = PaddingValues(horizontal = 12.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = if (primary) ds.accent else ds.bg3,
            contentColor = if (primary) ds.textInv else ds.text,
        ),
        modifier = Modifier.height(36.dp),
    ) { Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold) }
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
        TextButton(onClick = {
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
 * Peers | Rooms, beside the logo in the top bar.
 *
 * The desktop's title-bar toggle on the phone: the one switch between the two
 * lists, in the same place on both, so neither has a tab bar at the bottom.
 */
@Composable
internal fun ListToggle(rooms: Boolean, onSelect: (rooms: Boolean) -> Unit) {
    val ds = LocalDsColors.current
    Row(
        modifier = Modifier
            .border(1.dp, ds.divider, RoundedCornerShape(50))
            .background(ds.bg2, RoundedCornerShape(50))
            .padding(2.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        listOf(false to "Peers", true to "Rooms").forEach { (isRooms, label) ->
            val selected = rooms == isRooms
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .height(36.dp)
                    .background(if (selected) ds.accent else Color.Transparent, RoundedCornerShape(50))
                    .clickable { onSelect(isRooms) }
                    .padding(horizontal = 12.dp)
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
