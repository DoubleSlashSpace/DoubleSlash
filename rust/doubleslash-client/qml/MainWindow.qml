// MainWindow.qml — DoubleSlash native client main window (Phase 3 scaffold)
//
// Hosts the navigation rail, chat panel, call panel, and session banner.
// Binds to AppBridge (exposed from Rust via cxx-qt) for live state.

import QtQuick
import QtQuick.Window
import QtQuick.Controls.Material
import QtQuick.Layouts
import Qt.labs.platform as Platform
import DoubleSlash.Client 1.0

ApplicationWindow {
    id: root
    title: "DoubleSlash"
    width: 1100
    height: 700
    visible: false
    minimumWidth: 768
    minimumHeight: 640
    // CustomizeWindowHint hides Qt's default title-bar widgets. On Windows,
    // window_chrome.cpp re-applies WS_CAPTION|WS_THICKFRAME and handles
    // WM_NCCALCSIZE so DWM Aero Snap / drag-to-edge works with our QML
    // TitleBar (startSystemMove). Without that helper, CustomizeWindowHint
    // alone omits WS_CAPTION and snap is broken.
    flags: Qt.Window | Qt.CustomizeWindowHint

    // Inherited by controls that do not set their own pixel size, so menus,
    // combo boxes and the like follow the General font-size setting.
    font.pixelSize: Theme.fontSizeBody
    Material.theme: Theme.isDark ? Material.Dark : Material.Light
    // The skin's accent, so Material controls (switches, sliders, focus
    // lines) follow a custom colour rather than a fixed blue.
    Material.accent: Theme.accent
    // Square corners on Material's buttons, combo boxes, menus, popups and
    // dialogs: the UI is angular throughout. Inherited by every child.
    Material.roundedScale: Material.NotRounded

    function applyThemePreference(value) {
        var useDark = true
        if (value === "light") {
            useDark = false
        } else if (value === "system") {
            useDark = Qt.styleHints.colorScheme === Qt.ColorScheme.Dark
        }
        Theme.isDark = useDark
        Material.theme = useDark ? Material.Dark : Material.Light
    }

    /// `percent` is the General setting: −50…+200, where 0 is the designed size.
    function applyFontScale(percent) {
        var p = Math.max(-50, Math.min(200, Math.round(percent)))
        Theme.fontScale = (100 + p) / 100
    }

    function applyTimeFormat(id) {
        Theme.timeFormat = Theme.normalizeTimeFormat(id)
    }

    function showFilePreview(path) {
        if (!path || path === "") return
        // FilePreviewPanel requires the webengine feature. Fall back to the
        // system app when the panel module is not linked into this build.
        function openInSystem() {
            var n = ("" + path).replace(/\\/g, "/")
            if (n.charAt(0) !== "/") n = "/" + n
            Qt.openUrlExternally("file://" + n)
        }
        if (filePreviewLoader.item) {
            filePreviewLoader.item.filePath = path
            filePreviewLoader.visible = true
            return
        }
        filePreviewLoader.active = true
        filePreviewLoader.setSource(Qt.resolvedUrl("FilePreviewPanel.qml"), {
            "filePath": path
        })
        // setSource is async; check after a tick. On failure, open externally.
        Qt.callLater(function() {
            if (filePreviewLoader.status === Loader.Ready && filePreviewLoader.item) {
                filePreviewLoader.visible = true
            } else {
                filePreviewLoader.active = false
                filePreviewLoader.source = ""
                openInSystem()
            }
        })
    }

    function canonicalNodeId(nodeId) {
        if (!nodeId || nodeId === "") return ""
        if (!backend.isKnownSupernode(nodeId)) return ""
        var resolved = backend.resolveSupernodeNodeId(nodeId)
        if (resolved === "") resolved = nodeId
        // Fold every member of a cluster to its stable representative id so the
        // sidebar shows one logical node (one row, one avatar, one green dot,
        // one merged room list) regardless of which member currently hosts us.
        var rep = backend.clusterRepresentative(resolved)
        return rep !== "" ? rep : resolved
    }

    function pruneNonSupernodeEntries() {
        for (var j = nodeListModel.count - 1; j >= 0; j--) {
            if (!backend.isKnownSupernode(nodeListModel.get(j).node_id))
                nodeListModel.remove(j)
        }
    }

    function findNodeIndex(nodeId) {
        var canon = canonicalNodeId(nodeId)
        if (canon === "") return -1
        for (var j = 0; j < nodeListModel.count; j++) {
            var existing = nodeListModel.get(j).node_id
            if (existing === canon || canonicalNodeId(existing) === canon)
                return j
        }
        return -1
    }

    function findNodeIndexRaw(nodeId) {
        if (!nodeId || nodeId === "") return -1
        for (var j = 0; j < nodeListModel.count; j++) {
            if (nodeListModel.get(j).node_id === nodeId)
                return j
        }
        return -1
    }

    function supernodeHandleFor(nodeId) {
        var idx = findNodeIndex(nodeId)
        if (idx < 0) return ""
        var handle = nodeListModel.get(idx).title || ""
        return handle
    }

    // Per-supernode transport stats keyed by canonical node id (from connectionStats).
    property var nodeConnectionStats: ({})

    function upsertNodeConnectionStats(peerId, stats) {
        var canon = canonicalNodeId(peerId)
        if (canon === "") return
        var next = Object.assign({}, nodeConnectionStats)
        next[canon] = stats
        nodeConnectionStats = next
    }

    function clearNodeConnectionStats(nodeId) {
        var canon = canonicalNodeId(nodeId)
        if (canon === "" && nodeId) canon = nodeId
        if (canon === "" || !nodeConnectionStats[canon]) return
        var next = Object.assign({}, nodeConnectionStats)
        delete next[canon]
        nodeConnectionStats = next
    }

    function peerHandleFor(peerId) {
        if (!peerId || peerId === "") return ""
        for (var row = 0; row < peerModel.rowCount(); row++) {
            var idx = peerModel.index(row, 0)
            if (peerModel.data(idx, 256).toString() === peerId)
                return peerModel.data(idx, 257).toString()
        }
        if (peerId.length > 12)
            return peerId.substring(0, 12) + "…"
        return peerId
    }

    function activeCallPeerHandle() {
        var peerId = root.activeCallPeerId || chatPanel.selectedPeerId
        if (!peerId || peerId === "") return ""
        var handle = root.peerHandleFor(peerId)
        if (chatPanel.selectedPeerId === peerId && chatPanel.selectedPeerName !== "")
            return chatPanel.selectedPeerName
        return handle
    }

    function trackDirectCall(peerId) {
        root.activeCallPeerId = peerId || ""
    }

    /// Dial `peerId` 1:1, from wherever the user pressed call.
    ///
    /// `start_call` leaves any live voice room first — room audio and a direct
    /// call cannot both run. The expanded tiles and popout windows belong to
    /// that room's video, so collapse them here, the same housekeeping the
    /// Leave and End buttons do; the bridge cannot close QML windows.
    function beginDirectCall(peerId) {
        if (backend.voice_in_room) {
            root.closeAllVideoPopouts()
            root.expandedVideoPeers = []
        }
        root.trackDirectCall(peerId)
        backend.startCall(peerId)
    }

    function refreshDirectCallModel() {
        directCallModel.clear()
        var remoteId = root.activeCallPeerId || chatPanel.selectedPeerId
        if (!remoteId || remoteId === "") return
        directCallModel.append({
            peerId: backend.public_id,
            handle: (settingsModel.local_handle && settingsModel.local_handle !== "")
                ? settingsModel.local_handle
                : backend.public_id,
            muted: voiceDock.muted,
            audioLevel: 0.0,
            isSelf: true
        })
        directCallModel.append({
            peerId: remoteId,
            handle: root.activeCallPeerHandle(),
            muted: false,
            audioLevel: 0.0,
            isSelf: false
        })
    }

    function numericRoomCount(value) {
        var n = Number(value)
        return isNaN(n) ? 0 : Math.max(0, n)
    }

    // Room fold, keyed `node_id:room_id`. Reassigned (not mutated) on toggle so
    // the `roomsTree` bindings re-evaluate. `true` shuts a room's member lists
    // (and wins over the live voice room and the open chat), `false` opens
    // them, and no entry leaves them collapsed unless this is the voice room
    // or the chat on screen. Sub-rooms stay listed either way.
    property var collapsedRooms: ({})
    // The voice room whose explicit collapse has already been released, so a
    // later fold stays shut until the user leaves that room and joins again.
    property string releasedVoiceKey: ""

    readonly property string voiceTreeKey: (backend.voice_in_room && root.voiceRoomId !== "")
        ? root.voiceSupernodeId + ":" + root.voiceRoomId
        : ""
    onVoiceTreeKeyChanged: root.forgetCollapseOnVoiceRoom()

    function copyCollapseFlags() {
        var next = {}
        for (var k in root.collapsedRooms)
            if (root.collapsedRooms.hasOwnProperty(k)) next[k] = root.collapsedRooms[k]
        return next
    }

    function roomIsOpen(nodeId, roomId) {
        var flag = root.collapsedRooms[nodeId + ":" + roomId]
        if (flag === true)
            return false
        if (flag === false)
            return true
        if (root.voiceTreeKey !== "" && nodeId === root.voiceSupernodeId
                && roomId === root.voiceRoomId)
            return true
        return nodeId === roomPanel.supernodeId && roomId === roomPanel.roomId
    }

    function toggleRoomCollapse(nodeId, roomId) {
        var next = root.copyCollapseFlags()
        next[nodeId + ":" + roomId] = root.roomIsOpen(nodeId, roomId) ? true : false
        root.collapsedRooms = next
    }

    /// Drop an explicit collapse of the voice room once per join.
    function forgetCollapseOnVoiceRoom() {
        var voiceKey = root.voiceTreeKey
        if (voiceKey === "") {
            root.releasedVoiceKey = ""
            return
        }
        if (voiceKey === root.releasedVoiceKey)
            return
        root.releasedVoiceKey = voiceKey
        var next = root.copyCollapseFlags()
        if (next[voiceKey] === true) {
            delete next[voiceKey]
            root.collapsedRooms = next
        }
    }

    /// Clear a shut flag so selecting this chat can open it, without leaving
    /// it open after the chat changes.
    function releaseRoomCollapse(nodeId, roomId) {
        var key = nodeId + ":" + roomId
        if (root.collapsedRooms[key] !== true)
            return
        var next = root.copyCollapseFlags()
        delete next[key]
        root.collapsedRooms = next
    }

    // Voice / Text-only leaves opened or closed by hand, keyed
    // `node_id:room_id:voice|text`. Absent means open: an expanded room shows
    // both lists.
    property var memberLeafState: ({})
    // Leaves showing every member rather than the first few.
    property var memberOverflowOpen: ({})
    // The member whose actions are open under their row, or "". One at a time.
    property string openMemberKey: ""
    // Members a leaf lists before folding the rest under "+N more".
    readonly property int treeMemberLimit: 8

    function setTreeFlag(which, key, value) {
        var src = which === "leaf" ? root.memberLeafState : root.memberOverflowOpen
        var next = {}
        for (var k in src)
            if (src.hasOwnProperty(k)) next[k] = src[k]
        next[key] = value
        if (which === "leaf") root.memberLeafState = next
        else root.memberOverflowOpen = next
    }

    function toggleOpenMember(key) {
        root.openMemberKey = root.openMemberKey === key ? "" : key
    }

    /// Bring a room's members into view: Rooms tab, that room's lists unfolded.
    /// The room panel's member count asks for this. Sub-rooms are already listed.
    function revealRoomMembers(nodeId, roomId) {
        if (nodeId === "" || roomId === "")
            return
        root.sidebarTab = 1
        var next = root.copyCollapseFlags()
        next[nodeId + ":" + roomId] = false
        root.collapsedRooms = next
        root.setTreeFlag("leaf", nodeId + ":" + roomId + ":voice", true)
        root.setTreeFlag("leaf", nodeId + ":" + roomId + ":text", true)
    }

    // Rooms list order, the same JSON the phone stores. Name A–Z until changed.
    readonly property var roomListOrder: {
        var raw = settingsModel ? (settingsModel.room_list_order_json || "") : ""
        try {
            return JSON.parse(raw || "{}")
        } catch (e) {
            return {}
        }
    }

    // Room ids whose text alerts. Empty until the user enables a room.
    // Same JSON array the phone stores. Keyed by room id, not the host.
    readonly property var roomAlertIds: {
        var raw = settingsModel ? (settingsModel.room_message_alerts_json || "[]") : "[]"
        try {
            var parsed = JSON.parse(raw)
            return Array.isArray(parsed) ? parsed : []
        } catch (e) {
            return []
        }
    }

    property var roomUnread: ({})

    // Selection survives navigation; only visible foreground text is read.
    readonly property bool readingRoomText: navIndex === 1 && root.active
    onReadingRoomTextChanged: backend.setRoomChatVisible(readingRoomText)
    readonly property bool readingDirectText: navIndex === 0 && root.active
    onReadingDirectTextChanged: backend.setDirectChatVisible(readingDirectText)

    function roomAlertsEnabled(roomId) {
        return roomId !== "" && root.roomAlertIds.indexOf(roomId) >= 0
    }

    function roomUnreadCount(roomId) {
        var n = root.roomUnread[roomId]
        return n > 0 ? n : 0
    }

    function setRoomUnread(roomId, count) {
        if (!roomId)
            return
        var next = {}
        var cur = root.roomUnread || {}
        for (var k in cur) {
            if (Object.prototype.hasOwnProperty.call(cur, k))
                next[k] = cur[k]
        }
        if (count > 0)
            next[roomId] = count
        else
            delete next[roomId]
        root.roomUnread = next
    }

    function setRoomMessageAlerts(roomId, enabled) {
        if (!settingsModel || !roomId)
            return
        var ids = root.roomAlertIds.slice()
        var at = ids.indexOf(roomId)
        if (enabled && at < 0)
            ids.push(roomId)
        if (!enabled && at >= 0)
            ids.splice(at, 1)
        settingsModel.room_message_alerts_json = JSON.stringify(ids)
        settingsModel.save()
    }

    function roomNameForAlert(roomId) {
        var host = root.roomHostForAlert(roomId)
        return host ? (host.name || "") : ""
    }

    /// Sidebar node and display name for a room id, including a hidden room.
    function roomHostForAlert(roomId) {
        if (!roomId)
            return null
        for (var i = 0; i < nodeListModel.count; i++) {
            var node = nodeListModel.get(i)
            if (!node)
                continue
            var rooms = []
            try {
                rooms = JSON.parse(node.rooms_json || "[]")
            } catch (e) {
                continue
            }
            for (var r = 0; r < rooms.length; r++) {
                if (rooms[r] && rooms[r].room_id === roomId)
                    return {
                        nodeId: node.node_id,
                        roomId: roomId,
                        name: rooms[r].name || rooms[r].room_id
                    }
            }
        }
        return null
    }

    function commitRoomOrder(order) {
        if (!settingsModel)
            return
        var o = RoomTree.normalizeOrder(order)
        settingsModel.room_list_order_json = JSON.stringify({
            mode: o.mode,
            pinned: o.pinned,
            manual: o.manual
        })
        settingsModel.save()
    }

    function roomGroupsForOrder() {
        var groups = []
        for (var i = 0; i < nodeListModel.count; i++) {
            var node = nodeListModel.get(i)
            if (!node || !backend.isKnownSupernode(node.node_id))
                continue
            var parsed = []
            try {
                parsed = JSON.parse(node.rooms_json || "[]")
            } catch (e) {
                parsed = []
            }
            var rooms = []
            for (var r = 0; r < parsed.length; r++) {
                if (!parsed[r])
                    continue
                if (!root.showHiddenRooms && parsed[r].hidden)
                    continue
                rooms.push(parsed[r])
            }
            groups.push({ nodeId: node.node_id, rooms: rooms })
        }
        return groups
    }

    function setRoomSort(mode) {
        root.commitRoomOrder(RoomTree.withMode(root.roomListOrder, mode, root.roomGroupsForOrder()))
    }

    // Everything the tree's shape depends on besides the rooms themselves, as
    // one value so a change to any of it rebuilds the rows.
    readonly property var treeContext: ({
        collapsed: root.collapsedRooms,
        leaves: root.memberLeafState,
        overflow: root.memberOverflowOpen,
        selectedNode: roomPanel.supernodeId,
        selectedRoom: roomPanel.roomId,
        voiceNode: backend.voice_in_room ? root.voiceSupernodeId : "",
        voiceRoom: backend.voice_in_room ? root.voiceRoomId : "",
        limit: root.treeMemberLimit,
        order: root.roomListOrder
    })

    /// Open a room's text from the tree (a single click on it).
    function openRoomFromTree(nodeId, roomId, name) {
        if (!nodeId || !roomId)
            return
        // The open chat expands this room only while it stays selected.
        root.releaseRoomCollapse(nodeId, roomId)
        roomPanel.switchToRoom(name || roomId, roomId, nodeId)
        backend.subscribeRoomChat(nodeId, roomId)
        navIndex = 1
    }

    /// Join a room's voice from the tree (double-click, or a Voice leaf's Join).
    function joinVoiceFromTree(nodeId, roomId, name) {
        if (!nodeId || !roomId)
            return
        root.releaseRoomCollapse(nodeId, roomId)
        roomPanel.switchToRoom(name || roomId, roomId, nodeId)
        backend.joinRoomWithVoice(nodeId, roomId)
        root.voiceRoomName = name || roomId
        root.voiceSupernodeId = nodeId
        root.voiceRoomId = roomId
        navIndex = 1
    }

    /// Open the 1:1 chat with a trusted member, by Peers-list id.
    function messagePeer(listPeerId, name) {
        if (!listPeerId)
            return
        chatPanel.selectedPeerId = listPeerId
        chatPanel.selectedPeerName = name
        backend.selectPeer(listPeerId)
        peerModel.setPeerUnread(listPeerId, 0)
        root.sidebarTab = 0
        navIndex = 0
    }

    /// Watch a peer in the centre region, or stop watching wherever they are.
    function toggleWatching(peerId) {
        if (root.watchedVideoPeers.indexOf(peerId) !== -1)
            root.stopWatchingVideo(peerId)
        else
            root.toggleVideoExpanded(peerId)
    }

    // The content page to return to from Settings. There is no Chat/Settings
    // switch any more: the title-bar avatar opens Settings, and its Back
    // button or the Peers | Rooms toggle leaves it.
    property int lastContentNav: 0
    // The sidebar's list: 0 = Peers, 1 = Rooms. Driven by the title-bar toggle.
    property int sidebarTab: 0
    onNavIndexChanged: if (navIndex !== 2) root.lastContentNav = navIndex

    function leaveSettings() {
        if (navIndex === 2)
            navIndex = root.lastContentNav
    }

    /// The voice room as "Parent › Room", from the sidebar's own room list.
    function voiceRoomPath() {
        var name = root.voiceRoomName || qsTr("Voice room")
        var idx = root.findNodeIndex(root.voiceSupernodeId)
        if (idx < 0 || root.voiceRoomId === "")
            return name
        var rooms = []
        try { rooms = JSON.parse(nodeListModel.get(idx).rooms_json || "[]") } catch (e) { return name }
        var byId = {}
        for (var i = 0; i < rooms.length; i++)
            if (rooms[i] && rooms[i].room_id) byId[rooms[i].room_id] = rooms[i]
        var parts = []
        var cur = byId[root.voiceRoomId]
        var guard = 0
        while (cur && guard++ < 16) {
            parts.unshift(cur.name || cur.room_id)
            cur = cur.parent_id ? byId[cur.parent_id] : null
        }
        return parts.length > 0 ? parts.join(" › ") : name
    }

    // Bumped whenever the voice roster is replaced, so bindings that ask
    // roomModel about a peer (it has no per-peer change signal) re-run.
    property int sessionRosterStamp: 0

    /// People in the live session sharing video that we are not watching, as
    /// `[{id, name}]` — the voice dock's "is sharing video · Watch" line.
    readonly property var unwatchedStreamers: {
        root.sessionRosterStamp
        var out = []
        if (!backend.voice_active)
            return out
        for (var pid in root.videoActivePeers) {
            if (!root.videoActivePeers[pid] || pid === backend.public_id)
                continue
            if (root.watchedVideoPeers.indexOf(pid) !== -1)
                continue
            var name = backend.voice_in_room
                ? roomModel.handleFor(pid)
                : (pid === root.activeCallPeerId ? root.activeCallPeerHandle() : "")
            if (name === "")
                continue
            out.push({ id: pid, name: name })
        }
        return out
    }

    function roomVoiceCount(room) {
        if (room.voice_count !== undefined && room.voice_count !== null)
            return root.numericRoomCount(room.voice_count)
        if (room.count !== undefined && room.count !== null)
            return root.numericRoomCount(room.count)
        if (room.member_count !== undefined && room.member_count !== null)
            return root.numericRoomCount(room.member_count)
        return 0
    }

    function roomHasVoiceCount(room) {
        return (room.voice_count !== undefined && room.voice_count !== null)
            || (room.count !== undefined && room.count !== null)
            || (room.member_count !== undefined && room.member_count !== null)
    }

    // Text-chat occupancy (voice participants + chat-only subscribers) —
    // distinct from roomVoiceCount, which is voice-only. Server sends this
    // alongside voice_count/member_count on a full room list. Incremental
    // voice-roster patches (SfuMembers/PeerJoined/PeerLeft — fired on every
    // voice join/leave, including your own SfuSubscribe when you click a
    // room) never carry it, and DO carry a real voice_count, so gating on
    // the shared `count_known` would treat those patches as authoritative
    // for chat_count too and zero it out. Use a dedicated
    // `chat_count_known` instead (see mergeRoomEntry).
    function roomChatCount(room) {
        if (room.chat_count !== undefined && room.chat_count !== null)
            return root.numericRoomCount(room.chat_count)
        return 0
    }

    function roomHasChatCount(room) {
        return room.chat_count !== undefined && room.chat_count !== null
    }

    // Normalize a list of peer names that may arrive either as a real JS array
    // (JSON.parse of a room patch) or as a delegate `var` role. A QVariantList
    // reaches QML as a sequence wrapper: it indexes and has `.length`, but
    // `Array.isArray()` is false for it, so testing with Array.isArray() silently
    // discards a perfectly good roster. Walk `.length` instead.
    function nameList(value) {
        if (value === undefined || value === null)
            return []
        if (typeof value === "string")
            return value.trim() === "" ? [] : [value.trim()]
        if (typeof value.length !== "number")
            return []
        var out = []
        for (var i = 0; i < value.length; i++) {
            var name = String(value[i] || "").trim()
            if (name !== "")
                out.push(name)
        }
        return out
    }

    function roomKnownPeers(room) {
        return root.nameList(room.known_peers)
    }

    function roomUnknownPeerCount(room, voiceCount, knownPeers) {
        if (room.unknown_peers !== undefined && room.unknown_peers !== null)
            return root.numericRoomCount(room.unknown_peers)
        return Math.max(0, voiceCount - knownPeers.length)
    }

    // Rebuild the Rooms sidebar from the trusted peer store, preserving
    // per-node room snapshots and live connected/sfu flags where possible.
    function syncRoomsSidebar(nodesJson) {
        try {
            var desired = JSON.parse(nodesJson || "[]")
            var keepIds = {}
            for (var d = 0; d < desired.length; d++) {
                if (desired[d].node_id)
                    keepIds[desired[d].node_id] = true
            }
            for (var j = nodeListModel.count - 1; j >= 0; j--) {
                if (!keepIds[nodeListModel.get(j).node_id])
                    nodeListModel.remove(j)
            }
            for (var i = 0; i < desired.length; i++) {
                var node = desired[i]
                var nid = node.node_id || ""
                if (nid === "") continue
                var idx = findNodeIndexRaw(nid)
                if (idx >= 0) {
                    if (node.title !== undefined && node.title !== "")
                        nodeListModel.setProperty(idx, "title", node.title)
                    if (node.homepage_url !== undefined && node.homepage_url !== "")
                        nodeListModel.setProperty(idx, "homepage_url", node.homepage_url)
                } else {
                    nodeListModel.append({
                        node_id: nid,
                        connected: node.connected || false,
                        homepage_url: node.homepage_url || "",
                        title: node.title || "",
                        sfu_enabled: node.sfu_enabled || false,
                        public_rooms_enabled: node.public_rooms_enabled || false,
                        rooms_json: "[]"
                    })
                }
            }
        } catch (e) {
            console.warn("syncRoomsSidebar parse error:", e)
        }
    }

    function mergeRoomEntry(existing, incoming) {
        var merged = {}
        for (var k in existing) {
            if (existing.hasOwnProperty(k))
                merged[k] = existing[k]
        }
        for (var j in incoming) {
            if (!incoming.hasOwnProperty(j))
                continue
            var v = incoming[j]
            if (v === undefined || v === null)
                continue
            if ((j === "name" || j === "room_name" || j === "kind" || j === "room_type") && v === "")
                continue
            if ((j === "name" || j === "room_name")
                    && existing[j]
                    && incoming.room_id
                    && v === incoming.room_id)
                continue
            if ((j === "kind" || j === "room_type")
                    && existing[j]
                    && v === "voice"
                    && incoming.room_id
                    && !incoming.creator_id
                    && incoming.is_default === undefined)
                continue
            if ((j === "voice_count" || j === "member_count" || j === "count"
                    || j === "known_peers" || j === "unknown_peers")
                    && incoming.count_known === false)
                continue
            if (j === "count_known" && v === false && existing.count_known === true)
                continue
            // chat_count has its own staleness flag — a voice-only patch
            // (incoming.count_known === true, since it has a real
            // voice_count) must not zero out the last known chat_count.
            if (j === "chat_count" && incoming.chat_count_known === false)
                continue
            if (j === "chat_count_known" && v === false && existing.chat_count_known === true)
                continue
            // Keep a known Space parent when a supernode-sourced update (which
            // has no tree metadata) would otherwise blank it out.
            if (j === "parent_id" && v === "" && existing.parent_id)
                continue
            merged[j] = v
        }
        return merged
    }

    // Union two room snapshots by room_id (used when deduping alias node rows).
    function mergeRoomLists(a, b) {
        var byId = {}
        for (var i = 0; i < a.length; i++)
            if (a[i].room_id) byId[a[i].room_id] = a[i]
        for (var j = 0; j < b.length; j++)
            if (b[j].room_id)
                byId[b[j].room_id] = byId.hasOwnProperty(b[j].room_id)
                    ? root.mergeRoomEntry(byId[b[j].room_id], b[j])
                    : b[j]
        var out = []
        for (var k in byId) {
            if (byId.hasOwnProperty(k))
                out.push(byId[k])
        }
        return out
    }

    // Merge duplicate sidebar entries created when the same supernode was
    // keyed once by hex peer_id and once by base64url identity_pub.
    function dedupeNodeList() {
        var canonToIdx = {}
        var toRemove = []
        for (var j = 0; j < nodeListModel.count; j++) {
            var entry = nodeListModel.get(j)
            var canon = canonicalNodeId(entry.node_id)
            if (canon === "") continue
            if (canonToIdx.hasOwnProperty(canon)) {
                var keep = canonToIdx[canon]
                var keepEntry = nodeListModel.get(keep)
                var keepRooms = []
                var dupRooms = []
                try { keepRooms = JSON.parse(keepEntry.rooms_json || "[]") } catch (e) {}
                try { dupRooms = JSON.parse(entry.rooms_json || "[]") } catch (e) {}
                if (dupRooms.length > 0) {
                    var mergedRooms = root.mergeRoomLists(keepRooms, dupRooms)
                    nodeListModel.setProperty(keep, "rooms_json", JSON.stringify(mergedRooms))
                }
                if (!keepEntry.connected && entry.connected)
                    nodeListModel.setProperty(keep, "connected", true)
                if (!keepEntry.sfu_enabled && entry.sfu_enabled)
                    nodeListModel.setProperty(keep, "sfu_enabled", true)
                if (!keepEntry.public_rooms_enabled && entry.public_rooms_enabled)
                    nodeListModel.setProperty(keep, "public_rooms_enabled", true)
                if (!keepEntry.title && entry.title)
                    nodeListModel.setProperty(keep, "title", entry.title)
                if (!keepEntry.homepage_url && entry.homepage_url)
                    nodeListModel.setProperty(keep, "homepage_url", entry.homepage_url)
                nodeListModel.setProperty(keep, "node_id", canon)
                toRemove.push(j)
            } else {
                canonToIdx[canon] = j
                if (entry.node_id !== canon)
                    nodeListModel.setProperty(j, "node_id", canon)
            }
        }
        toRemove.sort(function(a, b) { return b - a })
        for (var k = 0; k < toRemove.length; k++)
            nodeListModel.remove(toRemove[k])
        pruneNonSupernodeEntries()
    }

    function upsertSfuRoomGroup(supernodeId, rooms, replaceRooms) {
        var canon = canonicalNodeId(supernodeId)
        if (canon === "") return

        var normalized = []
        for (var i = 0; i < rooms.length; i++) {
            var r = rooms[i]
            var voiceCount = root.roomVoiceCount(r)
            var countKnown = root.roomHasVoiceCount(r)
            var knownPeers = root.roomKnownPeers(r)
            var entry = {
                room_id: r.room_id || "",
                name: r.name || r.room_name || r.room_id || "Room",
                kind: r.kind || r.room_type || "voice",
                voice_count: voiceCount,
                chat_count: root.roomChatCount(r),
                chat_count_known: root.roomHasChatCount(r),
                known_peers: knownPeers,
                unknown_peers: root.roomUnknownPeerCount(r, voiceCount, knownPeers),
                count_known: countKnown,
                creator_id: r.creator_id || "",
                is_default: r.is_default === true || r.room_id === "default",
                // Space-tree parent (carried from the local store) for sidebar
                // indent. Supernode-sourced rooms omit it → "" → top-level.
                parent_id: r.parent_id || ""
            }
            // Absent means "this update does not know", so a voice patch
            // must not clear a tombstone the sidebar is still holding.
            if (r.hidden === true || r.hidden === false)
                entry.hidden = r.hidden === true
            // Who is in the room. Without these the tree still paints the
            // headphone on a room we have joined, and nothing under it.
            var rosters = RoomTree.rosterFields(r)
            for (var rk in rosters) {
                if (rosters.hasOwnProperty(rk))
                    entry[rk] = rosters[rk]
            }
            normalized.push(entry)
        }

        var nodeIdx = findNodeIndex(canon)
        if (nodeIdx >= 0) {
            var existing = []
            try { existing = JSON.parse(nodeListModel.get(nodeIdx).rooms_json || "[]") } catch (e) {}
            var merged = existing
            if (replaceRooms === true)
                merged = normalized
            else if (normalized.length > 0)
                merged = root.mergeRoomLists(existing, normalized)
            nodeListModel.setProperty(nodeIdx, "node_id", canon)
            nodeListModel.setProperty(nodeIdx, "rooms_json", JSON.stringify(merged))
        } else if (normalized.length > 0) {
            var roomsJson = JSON.stringify(normalized)
            nodeListModel.append({
                node_id: canon,
                connected: false,
                homepage_url: "",
                title: "",
                sfu_enabled: false,
                public_rooms_enabled: false,
                rooms_json: roomsJson
            })
        }
        dedupeNodeList()
        root.recountRooms()
        if (canon === root.voiceSupernodeId)
            root.forgetCollapseOnVoiceRoom()
    }

    // Local sidebar tombstones. Hidden rooms stay in the model; the tree
    // draws them only while this is on, the same way the phone's list header does.
    property bool showHiddenRooms: false
    // Blocked peers are the Peers list's hidden ones, on both clients.
    property bool showBlockedPeers: false
    property int blockedPeerCount: 0
    property int hiddenRoomCount: 0
    property int visibleRoomCount: 0

    function recountRooms() {
        var hidden = 0
        var visible = 0
        for (var i = 0; i < nodeListModel.count; i++) {
            var rooms = []
            try { rooms = JSON.parse(nodeListModel.get(i).rooms_json || "[]") } catch (e) {}
            for (var r = 0; r < rooms.length; r++) {
                if (rooms[r] && rooms[r].hidden)
                    hidden++
                else
                    visible++
            }
        }
        root.hiddenRoomCount = hidden
        root.visibleRoomCount = visible
    }

    function setRoomHiddenFlag(supernodeId, roomId, hidden) {
        var nodeIdx = root.findNodeIndex(supernodeId)
        if (nodeIdx < 0)
            return
        var rooms = []
        try { rooms = JSON.parse(nodeListModel.get(nodeIdx).rooms_json || "[]") } catch (e) {}
        var changed = false
        for (var i = 0; i < rooms.length; i++) {
            if (rooms[i] && rooms[i].room_id === roomId && rooms[i].hidden !== hidden) {
                rooms[i].hidden = hidden
                changed = true
            }
        }
        if (changed)
            nodeListModel.setProperty(nodeIdx, "rooms_json", JSON.stringify(rooms))
        root.recountRooms()
    }

    function unhideRoom(supernodeId, roomId) {
        backend.unhideRoom(supernodeId, roomId)
        root.setRoomHiddenFlag(supernodeId, roomId, false)
    }

    // ── Custom frameless title bar with embedded logo + invite controls ─────
    TitleBar {
        id: customTitleBar
        z: 200
        anchors {
            top: parent.top
            left: parent.left
            right: parent.right
        }
        appWindow: root

        Image {
            id: logoImage
            Layout.preferredWidth: 48
            Layout.preferredHeight: 22
            Layout.alignment: Qt.AlignVCenter
            fillMode: Image.PreserveAspectFit
            source: "qrc:/qt/qml/DoubleSlash/Client/icons/logo.svg"
        }

        // Peers | Rooms — the one switch between the two lists, where the
        // phone has it too. Picking either also leaves Settings.
        SidebarToggle {
            id: sidebarToggle
            Layout.alignment: Qt.AlignVCenter
            Layout.leftMargin: Theme.spacingSm
            currentIndex: root.sidebarTab
            dimmed: navIndex === 2
            onActivated: (index) => {
                root.sidebarTab = index
                root.leaveSettings()
            }
        }

        // Invite / peer-ID paste field
        StyledTextField {
            id: inviteField
            Layout.preferredWidth: 160
            Layout.preferredHeight: Theme.controlHeight
            Layout.maximumHeight: Theme.controlHeight
            Layout.alignment: Qt.AlignVCenter
            placeholderText: "Paste invite\u2026"
            Accessible.name: "Invite link or peer ID"
            Keys.onReturnPressed: {
                if (text.trim().length > 0) {
                    backend.pasteInvite(text.trim())
                    text = ""
                }
            }
        }

        // Connect button (→)
        StyledButton {
            id: connectBtn
            enabled: inviteField.text.trim().length > 0
            Layout.preferredWidth: Theme.controlHeight
            leftPadding: 6
            rightPadding: 6
            Layout.preferredHeight: Theme.controlHeight
            Layout.maximumHeight: Theme.controlHeight
            Layout.alignment: Qt.AlignVCenter
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/invite-submit.svg"
            Accessible.name: "Accept invite"
            ToolTip.text: "Connect to peer / accept invite"
            ToolTip.visible: hovered || visualFocus
            ToolTip.delay: Theme.animSlow
            onClicked: {
                var u = inviteField.text.trim()
                if (u.length > 0) {
                    backend.pasteInvite(u)
                    inviteField.text = ""
                }
            }
        }

        // New Invite button
        StyledButton {
            id: newInviteBtn
            Layout.preferredWidth: Theme.controlHeight
            leftPadding: 6
            rightPadding: 6
            primary: true
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
            Layout.preferredHeight: Theme.controlHeight
            Layout.maximumHeight: Theme.controlHeight
            Layout.alignment: Qt.AlignVCenter
            Accessible.name: "Copy new invite link"
            ToolTip.text: "Copy new invite link to clipboard (Ctrl+N)"
            ToolTip.visible: hovered || visualFocus
            ToolTip.delay: Theme.animSlow
            onClicked: {
                backend.copyInvite()
                // Empty when none could be minted; the session banner says why.
                if (backend.invite_url !== "")
                    invitePopup.visible = true
            }
            Shortcut {
                sequence: "Ctrl+N"
                onActivated: newInviteBtn.clicked()
            }
        }

        Item { Layout.fillWidth: true }

        // Discord-style update affordance: present but unobtrusive until a
        // release is ready. The installer owns shutdown, install, and relaunch.
        IconButton {
            id: updateIndicator
            visible: tag !== ""
            Layout.preferredWidth: 30
            Layout.preferredHeight: 30
            Layout.alignment: Qt.AlignVCenter
            padding: 6

            property string tag: ""
            property bool installing: false
            property string errorMessage: ""

            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
            icon.width: 18
            icon.height: 18
            icon.color: Theme.textInv

            background: Rectangle {
                color: updateIndicator.errorMessage !== ""
                    ? Theme.danger
                    : Theme.online
                opacity: updateIndicator.down
                    ? 0.75
                    : (updateIndicator.hovered ? 0.9 : 1.0)
                Behavior on opacity {
                    NumberAnimation { duration: Theme.animMicro }
                }
            }

            contentItem: Item {
                Image {
                    anchors.centerIn: parent
                    visible: !updateIndicator.installing
                    source: updateIndicator.icon.source
                    sourceSize.width: 18
                    sourceSize.height: 18
                    width: 18
                    height: 18
                    fillMode: Image.PreserveAspectFit
                }
                SquareBusyIndicator {
                    anchors.centerIn: parent
                    width: 20
                    height: 20
                    visible: updateIndicator.installing
                    running: visible
                    Material.accent: Theme.textInv
                }
            }

            ToolTip.visible: hovered
            ToolTip.delay: 300
            ToolTip.text: errorMessage !== ""
                ? "Update failed to start: " + errorMessage + "\nClick to retry"
                : (installing
                    ? "Installing " + tag + "\u2026 DoubleSlash will restart"
                    : "Update " + tag + " ready \u2014 click to update and restart")

            onClicked: {
                if (installing)
                    return
                errorMessage = ""
                installing = true
                backend.applyUpdate()
            }
        }

        // Own avatar — tooltip shows peer ID; click opens Avatar settings tab
        Avatar {
            Layout.alignment: Qt.AlignVCenter
            Layout.rightMargin: 4
            size: 28
            showRing: true
            peerId: backend.public_id
            configJson: settingsModel.avatar_config_json
            ToolTip.text: "Your peer ID: " + backend.public_id
            ToolTip.visible: ownAvatarHover.hovered
            ToolTip.delay: 500
            HoverHandler { id: ownAvatarHover }
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                // 2 = Identity, where the avatar editor lives. Shifted from 1
                // when the Video section was inserted after Audio.
                onClicked: { navIndex = 2; settingsTab = settingsPage.tabIdentity }
            }
        }
    }

    // Active navigation index: 0=chat, 1=room, 2=settings
    property int navIndex: 0

    // Name of the room we are currently in voice for (set only by joinRoomWithVoice).
    // Kept separate from roomPanel.roomName so browsing chat rooms doesn't
    // change the voice-rail label.
    property string voiceRoomName: ""
    // Hosting supernode for the active voice room (paired with voiceRoomName).
    property string voiceSupernodeId: ""
    // Room id of the active voice room. Names are display strings and can
    // collide across nodes, so anything deciding "is voice active in *this*
    // room" must compare the id + supernode, not the name.
    property string voiceRoomId: ""
    // Remote peer for an active direct P2P voice call.
    property string activeCallPeerId: ""

    // Settings section index (0=Audio … 9=About), in the order SettingsSidebar
    // lists them. Drives SettingsPage.currentTab; the names are on SettingsPage.
    property int settingsTab: 0

    // ── Video expand / popout state ──────────────────────────────────────────

    /// Peers shown in the shared region above chat, in insertion order.
    property var expandedVideoPeers: []
    /// Peer ids currently sending video, as a set (`{peerId: true}`).
    ///
    /// Held here rather than read back off the participant model, because
    /// neither model can answer the question outside a delegate: `RoomModel` is
    /// a QAbstractListModel whose roles only reach delegates, and
    /// `directCallModel` is a plain ListModel with no videoActive field at all.
    /// Anything iterating peer *ids* (the video region does) therefore saw
    /// "not streaming" for everyone and rendered the camera-off placeholder
    /// permanently. A plain map is model-independent and, reassigned whole,
    /// re-triggers the bindings that depend on it.
    property var videoActivePeers: ({})

    /// Record a peer's camera state. Rebuilds the map so QML sees a new
    /// identity and re-evaluates; mutating in place would not notify.
    function setPeerVideoActive(peerId, active) {
        if (!peerId || peerId === "")
            return
        var next = {}
        for (var k in root.videoActivePeers)
            next[k] = true
        if (active)
            next[peerId] = true
        else
            delete next[peerId]
        root.videoActivePeers = next
    }

    /// Peers whose video stopped arriving while their camera is still on.
    /// Same shape and same reassign-whole rule as `videoActivePeers`.
    property var videoStalledPeers: ({})

    function setPeerVideoStalled(peerId, stalled) {
        if (!peerId || peerId === "")
            return
        var next = {}
        for (var k in root.videoStalledPeers)
            next[k] = true
        if (stalled)
            next[peerId] = true
        else
            delete next[peerId]
        root.videoStalledPeers = next
    }
    /// Fraction of the content area the region occupies (persisted).
    property real videoRegionRatio: 0.4
    /// Live popout windows keyed by peer id.
    property var videoPopouts: ({})

    /// Peers whose video is on screen right now — expanded tiles plus popouts.
    ///
    /// Two things are gated on this, for the same underlying reason:
    ///
    /// * **Shared application audio**, because that audio is one half of a
    ///   picture — playing it to someone who never opened the tile gives them a
    ///   noise with no context and no visible control to stop it.
    /// * **Which senders the supernode forwards video from**, because a tile
    ///   nobody opened is a stream nobody decodes, and in a room of 1080p
    ///   senders that is most of a member's downlink spent on nothing.
    ///
    /// Derived rather than maintained by hand. Both inputs are reassigned
    /// wholesale on every change, so one binding cannot miss an update the way
    /// the five call sites that mutate them could — and a missed *removal* is
    /// the failure that matters, since it leaves a closed tile audible and its
    /// stream still arriving.
    readonly property var watchedVideoPeers: {
        var out = root.expandedVideoPeers.slice()
        for (var pid in root.videoPopouts) {
            if (root.videoPopouts[pid] && out.indexOf(pid) === -1)
                out.push(pid)
        }
        return out
    }

    onWatchedVideoPeersChanged: root.publishWatchedVideoPeers()

    /// Push the watched set to the backend: what to play, and what to receive.
    ///
    /// Guarded because this runs during component completion, which can precede
    /// the backend object being constructed.
    ///
    /// Also called on completion with an empty set, which is deliberate and not
    /// a no-op: to the supernode "no subscription yet" means *forward
    /// everything*, so a member who opens no tiles only stops paying for the
    /// room's video once they have actually said so.
    function publishWatchedVideoPeers() {
        if (typeof backend === "undefined" || !backend)
            return
        var json = JSON.stringify(root.watchedVideoPeers)
        backend.setContentAudioViewers(json)
        backend.setVideoSubscriptions(json)
    }


    function isVideoExpanded(peerId) {
        return root.expandedVideoPeers.indexOf(peerId) !== -1
    }

    /// Toggle a peer in the shared region.
    ///
    /// Reassigns the array rather than mutating it: QML only re-evaluates
    /// bindings on assignment, so an in-place push would leave the grid stale.
    function toggleVideoExpanded(peerId) {
        if (!peerId)
            return
        var next = root.expandedVideoPeers.slice()
        var i = next.indexOf(peerId)
        if (i === -1)
            next.push(peerId)
        else
            next.splice(i, 1)
        root.expandedVideoPeers = next
    }

    function collapseVideo(peerId) {
        var next = root.expandedVideoPeers.slice()
        var i = next.indexOf(peerId)
        if (i !== -1) {
            next.splice(i, 1)
            root.expandedVideoPeers = next
        }
    }

    /// Move a peer from the shared region into its own window.
    function popoutVideo(peerId) {
        if (!peerId)
            return
        // Already popped out — raise the existing window instead of opening a
        // second one showing the same stream.
        if (root.videoPopouts[peerId]) {
            root.videoPopouts[peerId].raise()
            root.videoPopouts[peerId].requestActivate()
            return
        }
        var comp = Qt.createComponent(
            "qrc:/qt/qml/DoubleSlash/Client/qml/VideoPopoutWindow.qml")
        if (comp.status === Component.Error) {
            console.warn("[video] popout unavailable:", comp.errorString())
            return
        }
        var win = comp.createObject(null, {
            peerId: peerId,
            displayName: root.videoPeerName(peerId)
        })
        if (!win) {
            console.warn("[video] popout window could not be created")
            return
        }
        // Bound after creation, not passed in: createObject's property map sets
        // static values, and this one has to keep tracking the peer's camera.
        win.streaming = Qt.binding(() => root.videoActivePeers[peerId] === true)
        win.stalled = Qt.binding(() => root.videoStalledPeers[peerId] === true)
        var map = root.videoPopouts
        map[peerId] = win
        root.videoPopouts = map
        win.closed.connect(function() { root.forgetVideoPopout(peerId) })
        win.contentAudioChanged.connect(function(pid, muted, volume) {
            backend.setContentAudioPref(pid, muted, volume)
        })
        // Leaving it in the region too would decode the same stream into two
        // sinks for no benefit; popping out is a move, not a copy.
        root.collapseVideo(peerId)
    }

    function forgetVideoPopout(peerId) {
        var map = root.videoPopouts
        if (map[peerId]) {
            delete map[peerId]
            root.videoPopouts = map
        }
    }

    /// Close every popout. Called before quitting so no detached window
    /// outlives the main one and keeps the process alive.
    function closeAllVideoPopouts() {
        for (var pid in root.videoPopouts) {
            if (root.videoPopouts[pid])
                root.videoPopouts[pid].close()
        }
        root.videoPopouts = ({})
    }

    /// Take a peer's video off screen, wherever it is showing.
    function stopWatchingVideo(peerId) {
        root.collapseVideo(peerId)
        if (root.videoPopouts[peerId])
            root.videoPopouts[peerId].close()
    }

    // ── Trust invites ────────────────────────────────────────────────────────

    /// Our trust invites by member id: "pending" | "sent". Session-only — it
    /// exists so the menu does not offer a second invite to someone who has
    /// one waiting, not as a record of who was asked.
    property var trustInviteStates: ({})
    /// Why the last invite was not sent, shown in the rail; "" when fine.
    property string trustInviteNotice: ""

    function setTrustInviteState(memberId, state) {
        var next = {}
        for (var k in root.trustInviteStates)
            next[k] = root.trustInviteStates[k]
        if (state === "")
            delete next[memberId]
        else
            next[memberId] = state
        root.trustInviteStates = next
    }

    /// Offer trust to a member of `roomId`, a room we are in: the receiver only
    /// honours an invite from someone in the room it names.
    function sendTrustInvite(roomId, memberId) {
        if (!memberId || !roomId)
            return
        root.setTrustInviteState(memberId, "pending")
        root.trustInviteNotice = ""
        backend.sendTrustInvite(roomId, memberId)
    }

    Timer {
        id: trustInviteNoticeTimer
        interval: 8000
        onTriggered: root.trustInviteNotice = ""
    }

    /// Resolve a peer's display name from the active voice roster.
    function videoPeerName(peerId) {
        var model = backend.voice_in_room ? roomModel : directCallModel
        if (model && model.rowCount) {
            for (var i = 0; i < model.rowCount(); i++) {
                var row = model.get ? model.get(i) : null
                if (row && row.peerId === peerId)
                    return row.handle || peerId
            }
        }
        return peerId
    }

    /// Capture sources the share menu offers, as `[{ id, name }]`.
    ///
    /// The empty-id entry first is the "default camera" selection an empty
    /// `video_input_device` means — the same convention `SourceSpec` reads.
    property var shareCaptureSources: [{ id: "", name: qsTr("Default camera") }]

    /// Whether this build can encode video at all.
    ///
    /// A static platform fact (VP8 is vendored everywhere, H.264 is added on
    /// Windows), so it is resolved once. If it is ever false there is no point
    /// offering to share — and previously the attempt just failed in silence.
    property bool videoEncoderAvailable: true

    /// Real capture sources, excluding the synthetic "Default camera" entry
    /// that `shareCaptureSources` always leads with.
    readonly property int realCaptureSourceCount: root.shareCaptureSources.length - 1

    /// Why video cannot be shared right now, or "" when it can.
    ///
    /// Capture is re-enumerated every time the share menu opens, so a camera
    /// plugged in after launch is picked up without restarting.
    readonly property string videoUnavailableReason:
        !root.videoEncoderAvailable
            ? qsTr("Video unavailable on this platform — no encoder.")
            : (root.realCaptureSourceCount <= 0
                ? qsTr("No camera or screen detected.")
                : "")

    function refreshVideoEncoderAvailable() {
        try {
            var res = JSON.parse(backend.listVideoCodecs())
            root.videoEncoderAvailable = (res.codecs || []).length > 0
        } catch (e) {
            // Assume available: a failed probe must not lock the user out of a
            // feature that may well work.
            console.warn("could not list video codecs:", e)
            root.videoEncoderAvailable = true
        }
    }

    /// Re-enumerate capture sources for the share menu.
    ///
    /// Cameras, monitors and windows share one list because any of them can be
    /// either the main source or an inset over it.
    function refreshShareCaptureSources() {
        var out = [{ id: "", name: qsTr("Default camera") }]
        try {
            var res = JSON.parse(backend.listVideoDevices())
            var groups = [
                { items: res.cameras || [], prefix: "Camera — " },
                { items: res.screens || [], prefix: "" },
                { items: res.windows || [], prefix: "Window — " }
            ]
            for (var g = 0; g < groups.length; g++) {
                var items = groups[g].items
                for (var i = 0; i < items.length; i++) {
                    out.push({
                        id: items[i].id,
                        name: groups[g].prefix + (items[i].name || items[i].id)
                    })
                }
            }
        } catch (e) {
            // Leaves the default-camera entry standing: enumeration failing is
            // no reason to be unable to share at all.
            console.warn("share menu: could not list video devices:", e)
        }
        root.shareCaptureSources = out
    }

    // Debounced persist of the region ratio, mirroring the window-geometry
    // timer below: a drag emits a value per frame and each save rewrites the
    // settings file.
    Timer {
        id: videoRegionRatioSaveTimer
        interval: 600
        onTriggered: {
            if (settingsModel) {
                settingsModel.video_region_ratio = root.videoRegionRatio
                settingsModel.save()
            }
        }
    }

    // Auto-switch to room tab when voice join succeeds. Leaving voice must not
    // kick the user off the room text panel when a text room is still selected.
    Connections {
        target: backend
        function onIn_roomChanged() {
            if (backend.in_room) navIndex = 1
            else if (navIndex === 1 && !roomPanel.roomId) navIndex = 0
        }
    }

    // Rust backend singleton (injected by cxx-qt)
    AppBridge {
        id: backend
    }

    // Settings model
    SettingsModel {
        id: settingsModel
    }

    Connections {
        target: settingsModel
        function onThemeChanged() {
            applyThemePreference(settingsModel.theme)
        }
        function onSkin_jsonChanged() {
            Theme.applySkinJson(settingsModel.skin_json)
        }
        function onFont_scale_percentChanged() {
            applyFontScale(settingsModel.font_scale_percent)
        }
        function onTime_formatChanged() {
            applyTimeFormat(settingsModel.time_format)
        }
        // Keep the bridge's applied avatar config in lockstep with settings so
        // every self-avatar site (voice rail, own room messages, …) resolves to
        // the same config as the Settings preview — including after a profile
        // reload or reset, not just after an in-page avatar edit.
        function onAvatar_config_jsonChanged() {
            backend.setAvatarConfigJson(settingsModel.avatar_config_json)
        }
        function onUpdate_check_enabledChanged() {
            backend.setAutomaticUpdateChecks(settingsModel.update_check_enabled)
        }
        function onRoom_message_alerts_jsonChanged() {
            backend.setRoomMessageAlerts(settingsModel.room_message_alerts_json || "[]")
        }
    }

    Connections {
        target: Qt.styleHints
        function onColorSchemeChanged() {
            if (settingsModel.theme === "system")
                applyThemePreference("system")
        }
    }

    // Live-data models
    PeerListModel     { id: peerModel }
    ChatModel         { id: chatModel }
    // Voice-rail participants (active voice room only).
    RoomModel         { id: roomModel }
    // The open room's whole roster — chat recipients and voice participants,
    // each marked in or out of voice. What the rail lists while a room is open.
    RoomModel         { id: textRoomModel }
    FileTransferModel { id: fileTransferModel }

    // Sidebar: supernodes with grouped SFU rooms (nodesUpdated + sfuRoomsUpdated)
    ListModel { id: nodeListModel }

    Component.onCompleted: {
        settingsModel.load()
        backend.setAutomaticUpdateChecks(settingsModel.update_check_enabled)
        applyThemePreference(settingsModel.theme)
        Theme.applySkinJson(settingsModel.skin_json)
        applyFontScale(settingsModel.font_scale_percent)
        applyTimeFormat(settingsModel.time_format)
        root.refreshVideoEncoderAvailable()

        // Announce the (empty) watched set. Not a no-op: the supernode treats
        // "never subscribed" as "forward everything", so this is what starts
        // the saving for a member who opens no tiles.
        root.publishWatchedVideoPeers()

        // ── Push saved avatar config into the bridge so avatarSvg() uses it ─
        backend.setAvatarConfigJson(settingsModel.avatar_config_json)

        // Restore the video region split, clamped to the same bounds the drag
        // handler enforces so a hand-edited settings file cannot wedge the
        // region open or shut.
        if (settingsModel.video_region_ratio > 0)
            root.videoRegionRatio = Math.max(0.2, Math.min(0.7, settingsModel.video_region_ratio))

        // Replay listener-local per-peer mute/volume into the mixer, so
        // choices made in an earlier session apply to peers we have not
        // interacted with yet this run.
        backend.applyPeerAudioPrefs(settingsModel.peer_audio_prefs_json)

        // Restore the last normal geometry. Ignore a position that no longer
        // overlaps a connected screen so monitor changes cannot strand us.
        root._restoringGeometry = true
        var savedWidth = settingsModel.window_width > 0
            ? Math.max(root.minimumWidth, settingsModel.window_width)
            : root.width
        var savedHeight = settingsModel.window_height > 0
            ? Math.max(root.minimumHeight, settingsModel.window_height)
            : root.height
        var restoreScreen = null
        if (settingsModel.window_position_saved) {
            for (var screenIndex = 0; screenIndex < Qt.application.screens.length; screenIndex++) {
                var candidate = Qt.application.screens[screenIndex]
                var overlapsX = settingsModel.window_x + savedWidth > candidate.virtualX
                    && settingsModel.window_x < candidate.virtualX + candidate.width
                var overlapsY = settingsModel.window_y + savedHeight > candidate.virtualY
                    && settingsModel.window_y < candidate.virtualY + candidate.height
                if (overlapsX && overlapsY) {
                    restoreScreen = candidate
                    break
                }
            }
        }
        if (restoreScreen) {
            root.width = Math.min(savedWidth, restoreScreen.width)
            root.height = Math.min(savedHeight, restoreScreen.height)
            root.x = Math.max(restoreScreen.virtualX,
                Math.min(settingsModel.window_x,
                    restoreScreen.virtualX + restoreScreen.width - root.width))
            root.y = Math.max(restoreScreen.virtualY,
                Math.min(settingsModel.window_y,
                    restoreScreen.virtualY + restoreScreen.height - root.height))
        } else {
            root.width = savedWidth
            root.height = savedHeight
        }
        root._windowWasMaximized = settingsModel.window_maximized
        root._restoringGeometry = false

        // ── Present the main window ───────────────────────────────────────
        // start_minimized only when the tray icon is available; otherwise
        // users would see a process with no visible UI.
        if (settingsModel.start_minimized && trayIcon.available) {
            root.showMinimized()
        } else {
            root.visible = true
            if (settingsModel.window_maximized)
                root.showMaximized()
            else
                root.show()
            root.raise()
            root.requestActivate()
        }

        backend.peersUpdated.connect(peerModel.setPeers)
        backend.peersUpdated.connect(function(json) {
            var n = 0
            try {
                var list = JSON.parse(json || "[]")
                for (var i = 0; i < list.length; i++)
                    if (list[i] && list[i].blocked) n++
            } catch (e) {}
            root.blockedPeerCount = n
        })
        backend.chatMessageReceived.connect(function(msgJson) {
            try {
                var msg = JSON.parse(msgJson)
                if (!msg.peer_id || msg.peer_id === chatPanel.selectedPeerId)
                    chatModel.appendMessage(msgJson)
            } catch (e) {
                chatModel.appendMessage(msgJson)
            }
        })
        backend.chatHistoryLoaded.connect(function(json) {
            chatModel.setMessages(json)
            chatPanel._historyPage = 0
            chatPanel._hasMoreHistory = true
            chatPanel._loadingHistory = false
        })
        backend.chatHistoryPrepended.connect(chatPanel.onHistoryPrepended)
        backend.messageStatusChanged.connect(chatModel.updateMessageStatus)
        backend.participantsUpdated.connect(roomModel.setParticipants)
        backend.participantsUpdated.connect(function() { root.sessionRosterStamp++ })
        backend.textMembersUpdated.connect(textRoomModel.setParticipants)
        // Live audio state goes to the room's member list too while it shows
        // the room we are in voice for — that list is where the rail reads it
        // from then. Never otherwise: a peer can be in both rooms, and the
        // room merely being browsed must not light up with this call's levels.
        backend.localSpeakingChanged.connect(function(speaking) {
            roomModel.updateParticipant(backend.public_id, speaking, false)
            if (roomPanel.voiceActiveHere)
                textRoomModel.updateParticipant(backend.public_id, speaking, false)
        })
        backend.peerSpeakingChanged.connect(function(peerId, speaking) {
            roomModel.updateParticipant(peerId, speaking, false)
            if (roomPanel.voiceActiveHere)
                textRoomModel.updateParticipant(peerId, speaking, false)
        })
        backend.peerLevelChanged.connect(function(peerId, level) {
            roomModel.setAudioLevel(peerId, level)
            if (roomPanel.voiceActiveHere)
                textRoomModel.setAudioLevel(peerId, level)
        })
        backend.trustInviteReceived.connect(function(senderId, handle, roomName, inviteUrl) {
            trustInviteDialog.enqueue(senderId, handle, roomName, inviteUrl)
        })
        backend.trustInviteResult.connect(function(memberId, error) {
            root.setTrustInviteState(memberId, error === "" ? "sent" : "")
            root.trustInviteNotice = error === "" ? "" : qsTr("Invite not sent: ") + error
            if (error !== "")
                trustInviteNoticeTimer.restart()
        })
        backend.peerVideoStateChanged.connect(function(peerId, active) {
            roomModel.setVideoActive(peerId, active)
            textRoomModel.setVideoActive(peerId, active)
            // Drives the video region, which cannot read model roles.
            root.setPeerVideoActive(peerId, active)
            // A camera that just turned on or off cannot also be stalled; the
            // backend retracts its own report, but not before the tile would
            // have flashed the badge over a fresh stream.
            if (!active)
                root.setPeerVideoStalled(peerId, false)
        })
        backend.peerVideoStalledChanged.connect(function(peerId, stalled) {
            root.setPeerVideoStalled(peerId, stalled)
        })
        backend.cameraCaptureFailed.connect(function(reason) {
            // The toggle is already off by the time this arrives — the backend
            // turned it off, because the capture is gone either way.
            console.warn("[video] camera stopped:", reason)
        })
        // File transfer model wiring
        backend.fileOffered.connect(fileTransferModel.upsertTransfer)
        backend.fileProgress.connect(fileTransferModel.setProgress)
        backend.fileComplete.connect(function(json) {
            try {
                var o = JSON.parse(json)
                fileTransferModel.markComplete(o.transfer_id)
                var mid = "xfer-" + o.transfer_id
                if (o.saved_path)
                    chatModel.updateAttachment(mid, o.saved_path, o.size_str || "")
                if (o.saved_path)
                    roomPanel.updateAttachment(mid, o.saved_path, o.size_str || "")
            } catch(e) {}
        })
        backend.fileFailed.connect(function(json) {
            try {
                var o = JSON.parse(json)
                fileTransferModel.markFailed(o.transfer_id, o.reason || "failed")
                fileTransferModel.setRetryable(o.transfer_id, !!o.retryable)
            } catch(e) {}
        })
        // Wire peer list badge + preview + typing from bridge signals
        backend.unreadChanged.connect(peerModel.setPeerUnread)
        backend.previewChanged.connect(peerModel.setPeerPreview)
        backend.typingChanged.connect(function(peerId, isTyping) {
            peerModel.setTyping(peerId, isTyping)
        })
        backend.passphraseRequired.connect(function(isNew) {
            passphraseDialog.isNew = isNew
            passphraseDialog.errorText = backend.session_banner
            passphraseDialog.visible = true
        })
        backend.incomingCall.connect(function(peerId) {
            incomingCallDialog.show(peerId)
        })
        backend.incomingCallCancelled.connect(function(peerId) {
            incomingCallDialog.cancel(peerId)
        })
        backend.updateAvailable.connect(function(tag, url) {
            updateIndicator.tag = tag
            updateIndicator.installing = false
            updateIndicator.errorMessage = ""
        })
        backend.updateInstallFailed.connect(function(message) {
            updateIndicator.installing = false
            updateIndicator.errorMessage = message
        })

        // Merge room list updates per supernode into grouped sidebar model.
        backend.sfuRoomsUpdated.connect(function(json) {
            try {
                var obj = JSON.parse(json)
                root.upsertSfuRoomGroup(obj.supernode_id || "", obj.rooms || [], obj.replace === true)
            } catch(e) { console.warn("sfuRoomsUpdated parse error:", e) }
        })

        backend.roomsSidebarSync.connect(root.syncRoomsSidebar)

        backend.connectionStats.connect(function(json) {
            try {
                var stats = JSON.parse(json)
                if (!stats.peer_id) return
                if (!backend.isKnownSupernode(stats.peer_id)) return
                root.upsertNodeConnectionStats(stats.peer_id, stats)
            } catch (e) {}
        })

        // Wire node connect/disconnect into nodeListModel (upsert by node_id)
        backend.nodesUpdated.connect(function(json) {
            try {
                var patches = JSON.parse(json)
                for (var i = 0; i < patches.length; i++) {
                    var p = patches[i]
                    var canon = root.canonicalNodeId(p.node_id || "")
                    if (canon === "") continue
                    var nodeIdx = root.findNodeIndex(canon)
                    if (nodeIdx >= 0) {
                        nodeListModel.setProperty(nodeIdx, "node_id", canon)
                        if (p.connected !== undefined && p.connected !== null) {
                            nodeListModel.setProperty(nodeIdx, "connected", p.connected)
                            if (!p.connected)
                                root.clearNodeConnectionStats(canon)
                        }
                        if (p.homepage_url !== undefined)
                            nodeListModel.setProperty(nodeIdx, "homepage_url", p.homepage_url)
                        if (p.title !== undefined)
                            nodeListModel.setProperty(nodeIdx, "title", p.title)
                        if (p.sfu_enabled !== undefined && p.sfu_enabled !== null)
                            nodeListModel.setProperty(nodeIdx, "sfu_enabled", p.sfu_enabled)
                        if (p.public_rooms_enabled !== undefined && p.public_rooms_enabled !== null)
                            nodeListModel.setProperty(nodeIdx, "public_rooms_enabled", p.public_rooms_enabled)
                    } else {
                        nodeListModel.append({
                            node_id:              canon,
                            connected:            p.connected || false,
                            homepage_url:         p.homepage_url || "",
                            title:                p.title || "",
                            sfu_enabled:          p.sfu_enabled || false,
                            public_rooms_enabled: p.public_rooms_enabled || false,
                            rooms_json:           "[]"
                        })
                    }
                }
                root.dedupeNodeList()
                root.pruneNonSupernodeEntries()
            } catch(e) { console.warn("nodesUpdated parse error:", e) }
        })

        // PTT: start polling thread if enabled in settings
        if (settingsModel.push_to_talk) {
            backend.enablePtt(settingsModel.ptt_key)
        }

        // Wire message deletion: remove from in-memory model when backend
        // confirms. Both panels are told — a message id is unique across the
        // store, so whichever holds it drops it and the other no-ops.
        backend.messageDeleted.connect(function(msgId) {
            chatModel.removeMessage(msgId)
            roomPanel.removeMessage(msgId)
        })
        // Wire peer history clear: wipe the in-memory model
        backend.peerHistoryCleared.connect(function(peerId) {
            if (chatPanel.selectedPeerId === peerId) {
                chatModel.clearMessages()
            }
        })

        backend.initializeBackend()
        backend.setRoomChatVisible(root.readingRoomText)
        backend.setDirectChatVisible(root.readingDirectText)
        // Also covers default settings that emitted no change signal on load.
        backend.setRoomMessageAlerts(settingsModel.room_message_alerts_json || "[]")
        if (!settingsModel.onboarding_complete)
            Qt.callLater(function() {
                if (backend.public_id && backend.public_id !== "")
                    onboardingWizard.open()
            })
        // Drop any stale non-supernode rows left from older builds.
        Qt.callLater(root.pruneNonSupernodeEntries)
    }

    // ── Passphrase dialog — shown when identity needs unlocking/creation ──
    OnboardingWizard {
        id: onboardingWizard
        anchors.centerIn: parent
        z: 120
        settingsModel: settingsModel
        appBackend: backend
    }

    PassphraseDialog {
        id: passphraseDialog
        onBackupsRequested: backupWizard.open()
        onSubmitted: function(passphrase, filePath, remember) {
            passphraseDialog.visible = false
            backend.unlockWithPassphraseAndFile(passphrase, filePath, remember)
        }
    }

    BackupWizard {
        id: backupWizard
        appBackend: backend
    }

    // Listen for "Incorrect passphrase" banner to re-show dialog with error
    Connections {
        target: backend
        function onPublic_idChanged() {
            if (!settingsModel.onboarding_complete
                    && backend.public_id && backend.public_id !== ""
                    && !onboardingWizard.opened)
                onboardingWizard.open()
        }
        function onSession_bannerChanged() {
            const txt = backend.session_banner
            if (txt === "Incorrect passphrase \u2014 try again.") {
                passphraseDialog.errorText = txt
                passphraseDialog.visible = true
            }
        }
        function onTypingChanged(peerId, isTyping) {
            chatPanel.typingPeerId = isTyping ? peerId : ""
            chatPanel.peerIsTyping = isTyping
        }
        function onRoomChatReceived(msgJson) {
            // Bridge already filters to the selected text room; RoomPanel also
            // checks room_id so history loads / edge races cannot cross-paint.
            roomPanel.appendRoomChat(msgJson)
        }
        function onRoomUnreadChanged(roomId, count) {
            root.setRoomUnread(roomId, count)
        }
        // Same tray balloon as a direct message, for rooms the user enabled.
        // The bridge only emits this when that room's alerts are on.
        function onRoomMessageAlert(roomId, sender, body, supernodeId, messageId) {
            if (root.active || !trayIcon.available || !settingsModel.notifications_enabled)
                return
            var title = root.roomNameForAlert(roomId)
            if (!title)
                title = qsTr("Room")
            var who = sender || qsTr("Someone")
            var text = who + ": " + (body || qsTr("New message"))
            root.showTrayNotice(title, text.substring(0, 80),
                                Platform.SystemTrayIcon.Information, 4000, {
                                    kind: "room",
                                    roomId: roomId,
                                    supernodeId: supernodeId || "",
                                    messageId: messageId || ""
                                })
        }
        // Show a tray balloon when a message arrives while the window is not active.
        function onChatMessageReceived(msgJson) {
            if (!root.active && trayIcon.available && settingsModel.notifications_enabled) {
                try {
                    var msg = JSON.parse(msgJson)
                    if (!msg.mine) {
                        var sender = msg.sender || qsTr("DoubleSlash")
                        var body = (msg.body || qsTr("New message")).substring(0, 80)
                        root.showTrayNotice(sender, body,
                                            Platform.SystemTrayIcon.Information, 4000, null)
                    }
                } catch(e) {}
            }
        }
        // Show a tray balloon when a missed call is recorded.
        function onMissed_callsChanged() {
            if (backend.missed_calls > 0 && trayIcon.available) {
                root.showTrayNotice(qsTr("DoubleSlash"),
                                    qsTr("Missed call"),
                                    Platform.SystemTrayIcon.Warning, 5000, null)
            }
        }
        // Auto-update nodes list with portal info when supernode responds.
        // (The nodesUpdated signal already patches the ListModel; this handler
        //  is a no-op placeholder kept for future expansion.)
        function onSupernodeInfoReceived(nodeId, url, title) {
            // nodesUpdated already patched homepage_url + title in the model.
        }
        // When relay access requires a portal visit: open the browser,
        // switch to the portal view, and log an event.
        function onRelayPortalRequired(supernodeId, portalUrl) {
            if (portalUrl.startsWith("https://") || portalUrl.startsWith("http://")) {
                Qt.openUrlExternally(portalUrl)
                backend.logEvent("[relay] Portal visit required — opened browser: " + portalUrl)
            }
        }
        // Open a supernode's in-app portal: navigate the embedded browser
        // to the doubleslash:// URL served over the QUIC relay connection.
        function onNavigateNodePortal(supernodeId, url) {
            console.log("[portal] onNavigateNodePortal sn=" + supernodeId + " url=" + url)
            browserPanel.portalActive = true   // ensure Loader fires
            browserPanel.nodeMode = true
            browserPanel.navigateTo(url)
            navIndex = 3
        }
        function onSupernodeRemoved(nodeId) {
            if (!nodeId || nodeId === "") return
            root.clearNodeConnectionStats(nodeId)
            // Peer store is already updated when this fires; canonicalNodeId()
            // would return "" because isKnownSupernode() is false.
            for (var j = nodeListModel.count - 1; j >= 0; j--) {
                if (nodeListModel.get(j).node_id === nodeId)
                    nodeListModel.remove(j)
            }
            root.pruneNonSupernodeEntries()
            if (roomPanel.supernodeId === nodeId)
                roomPanel.switchToRoom("", "", "")
        }
        function onRoomRemoved(supernodeId, roomId) {
            if (roomPanel.supernodeId === supernodeId && roomPanel.roomId === roomId)
                roomPanel.switchToRoom("", "", "")
            // Keep the row, marked hidden, so + can show it again.
            root.setRoomHiddenFlag(supernodeId, roomId, true)
        }
        function onRoomCreated(supernodeId, roomId, roomName, roomType, inviteToken) {
            roomPanel.switchToRoom(roomName, roomId, supernodeId)
            backend.joinRoomWithVoice(supernodeId, roomId)
            root.voiceRoomName = roomName
            root.voiceSupernodeId = supernodeId
            root.voiceRoomId = roomId
            navIndex = 1
            if (roomType === "private" && inviteToken !== "") {
                // Prefer a self-contained invite URL (embeds the supernode
                // address) so recipients on any/no supernode can just paste it;
                // fall back to the bare token if the URL can't be built.
                var inviteUrl = backend.generateRoomInvite(supernodeId, roomId, roomName)
                backend.copyToClipboard(inviteUrl !== "" ? inviteUrl : inviteToken)
                root.showTrayNotice(
                    qsTr("Private room created"),
                    inviteUrl !== ""
                        ? qsTr("Invite link copied to clipboard.")
                        : qsTr("Invite token copied to clipboard."),
                    Platform.SystemTrayIcon.Information, 5000, null)
            }
        }
        function onRoomInviteReady(supernodeId, roomId, roomName) {
            roomPanel.switchToRoom(roomName, roomId, supernodeId)
            backend.joinRoomWithVoice(supernodeId, roomId)
            root.voiceRoomName = roomName
            root.voiceSupernodeId = supernodeId
            root.voiceRoomId = roomId
            navIndex = 1
        }
    }

    // ── Incoming call overlay ─────────────────────────────────────────────
    IncomingCallDialog {
        id: incomingCallDialog
        anchors.centerIn: parent
        z: 100
        onAccepted: function(peerId) {
            root.trackDirectCall(peerId)
            backend.acceptCall(peerId)
        }
        onRejected: function(peerId) { backend.rejectCall(peerId) }
    }

    // A room member offering to become trusted peers. Accepting redeems the
    // invite they sealed to us, through the same path as a pasted link.
    TrustInviteDialog {
        id: trustInviteDialog
        anchors.fill: parent
        z: 99
        onAccepted: (inviteUrl) => backend.pasteInvite(inviteUrl)
    }

    // Paste a peer or room invite. The phone's + opens this same box.
    Dialog {
        id: acceptInviteDialog
        title: qsTr("Accept an invite")
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        closePolicy: Dialog.CloseOnEscape
        width: 420
        padding: Theme.spacingXl
        z: 100

        onAboutToShow: acceptInviteField.forceActiveFocus()
        onOpened: acceptInviteField.text = ""
        onAccepted: {
            var url = acceptInviteField.text.trim()
            if (url !== "")
                backend.pasteInvite(url)
        }

        background: Rectangle {
            color: Theme.bg1
            radius: 0
            border.color: Theme.bg3
            border.width: 1
        }

        header: Rectangle {
            color: Theme.bg0
            height: 48
            Text {
                anchors.centerIn: parent
                text: acceptInviteDialog.title
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                font.bold: true
            }
        }

        contentItem: ColumnLayout {
            spacing: Theme.spacingSm

            Text {
                text: qsTr("Invite link")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
            }

            TextField {
                id: acceptInviteField
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.controlHeight * 2
                placeholderText: qsTr("doubleslash://…")
                wrapMode: Text.Wrap
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                background: Rectangle {
                    color: Theme.bg2
                    radius: 0
                    border.color: acceptInviteField.activeFocus ? Theme.accent : Theme.bg3
                    border.width: 1
                }
                Keys.onReturnPressed: acceptInviteDialog.accept()
            }
        }

        footer: DialogButtonBox {
            standardButtons: acceptInviteDialog.standardButtons
            background: Rectangle { color: Theme.bg0 }
            delegate: Button {
                enabled: DialogButtonBox.buttonRole !== DialogButtonBox.AcceptRole
                      || acceptInviteField.text.trim() !== ""
            }
        }
    }

    // ── Join Room dialog ──────────────────────────────────────────────────
    JoinRoomDialog {
        id: joinRoomDialog
        anchors.centerIn: parent
        z: 100
        onJoinRequested: function(supernodeId, roomId, inviteToken) {
            roomPanel.switchToRoom(roomId, roomId, supernodeId)
            // Persist + validate the invite token before joinRoomWithVoice runs
            // join_room (which reads the token from the room store).
            if ((inviteToken || "").trim() !== "")
                backend.joinRoomWithInvite(supernodeId, roomId, inviteToken)
            backend.joinRoomWithVoice(supernodeId, roomId)
            root.voiceRoomName = roomId
            root.voiceSupernodeId = supernodeId
            root.voiceRoomId = roomId
            navIndex = 1
        }
    }

    CreateRoomDialog {
        id: createRoomDialog
        anchors.centerIn: parent
        z: 100
        nodeListModel: nodeListModel
        includeHidden: root.showHiddenRooms
    }

    // ── Topbar removed: logo + invite field now live inside the TitleBar ─

    // ── Invite URL popup — shown after "New Invite" ───────────────────────
    Popup {
        id: invitePopup
        x: parent.width / 2 - width / 2
        y: customTitleBar.height + 8
        width: 460
        height: 100
        modal: false
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        z: 200

        background: Rectangle {
            color: Theme.bg2
            radius: 0
            border.color: Theme.border
            border.width: 1
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 8

            Label {
                text: "Invite link copied to clipboard:"
                color: Theme.text
                font.pixelSize: Theme.fontSizeCaption
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacingXs

                StyledTextField {
                    Layout.fillWidth: true
                    text: backend.invite_url
                    readOnly: true
                }

                Button {
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/clipboard.svg"
                    icon.width: 30
                    icon.height: 30
                    icon.color: Theme.text
                    implicitHeight: 30
                    implicitWidth: 30
                    flat: true
                    Material.foreground: Theme.text
                    ToolTip.text: "Copy to clipboard"
                    ToolTip.visible: hovered
                    onClicked: {
                        // Copy exactly the link shown (peer or room invite),
                        // rather than minting a fresh peer invite.
                        backend.copyToClipboard(backend.invite_url)
                        invitePopup.visible = false
                    }
                }
            }
        }
    }

    // ── Synthetic participant model for direct P2P calls ─────────────────
    // Populated from bridge when call_state changes; cleared on idle.
    ListModel { id: directCallModel }

    Connections {
        target: backend
        function onCall_stateChanged() {
            var cs = backend.call_state
            if (cs === "connecting" || cs === "in_call") {
                root.refreshDirectCallModel()
            } else {
                root.activeCallPeerId = ""
                directCallModel.clear()
            }
        }
    }

    // ── Main body ─────────────────────────────────────────────────────────
    RowLayout {
        anchors {
            top: customTitleBar.bottom
            left: parent.left
            right: parent.right
            bottom: parent.bottom
        }
        spacing: 0

        // ── Left sidebar: Peers | Rooms tabs ─────────────────────────────────
        ColumnLayout {
            Layout.preferredWidth: Theme.sidebarWidth
            Layout.minimumWidth: Theme.sidebarWidth
            Layout.maximumWidth: Theme.sidebarWidth
            Layout.fillHeight: true
            clip: true
            spacing: 0

            // Compact actions for the selected list.
            RowLayout {
                visible: navIndex !== 2
                Layout.fillWidth: true
                Layout.margins: Theme.spacingXs
                spacing: Theme.spacingXs
                IconButton {
                    icon.source: root.sidebarTab === 1 ? "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg" : "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
                    icon.color: Theme.text
                    icon.width: 18; icon.height: 18
                    Layout.preferredWidth: 32; Layout.preferredHeight: 32
                    padding: 6
                    Accessible.name: root.sidebarTab === 1 ? qsTr("Create Room") : qsTr("Create an Invite")
                    ToolTip.text: Accessible.name
                    ToolTip.visible: hovered || visualFocus
                    onClicked: root.sidebarTab === 1 ? createRoomDialog.openNew() : newInviteBtn.clicked()
                    enabled: root.sidebarTab === 0 || nodeListModel.count > 0
                }
                IconButton {
                    id: hiddenToggle
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/eye.svg"
                    icon.color: Theme.text
                    icon.width: 18; icon.height: 18
                    Layout.preferredWidth: 32; Layout.preferredHeight: 32
                    padding: 6
                    Accessible.name: (showing ? qsTr("Hide %1 hidden") : qsTr("Show %1 hidden")).arg(hiddenCount)
                    ToolTip.text: Accessible.name
                    ToolTip.visible: hovered || visualFocus
                    onClicked: {
                        if (root.sidebarTab === 1) root.showHiddenRooms = !root.showHiddenRooms
                        else root.showBlockedPeers = !root.showBlockedPeers
                    }
                    readonly property int hiddenCount: root.sidebarTab === 1 ? root.hiddenRoomCount : root.blockedPeerCount
                    readonly property bool showing: root.sidebarTab === 1 ? root.showHiddenRooms : root.showBlockedPeers
                    highlighted: showing
                    enabled: hiddenCount > 0
                    Text {
                        anchors.right: parent.right
                        anchors.top: parent.top
                        text: hiddenToggle.hiddenCount
                        visible: hiddenToggle.hiddenCount > 0
                        color: Theme.text
                        font.pixelSize: Math.max(1, Math.round(10 * Theme.fontScale))
                    }
                }
                IconButton {
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/sort.svg"
                    icon.color: Theme.text
                    icon.width: 18; icon.height: 18
                    Layout.preferredWidth: 32; Layout.preferredHeight: 32
                    padding: 6
                    Accessible.name: root.sidebarTab === 1 ? qsTr("Sort Rooms") : qsTr("Sort Peers")
                    ToolTip.text: Accessible.name
                    ToolTip.visible: hovered || visualFocus
                    onClicked: root.sidebarTab === 1 ? roomSortMenu.popup() : peerSortMenu.popup()
                    Menu {
                        id: roomSortMenu
                        title: qsTr("Sort rooms")

                        /// The SVG check beside the mode in use, or no icon.
                        function mark(mode) {
                            return RoomTree.sortMode(root.roomListOrder) === mode
                                ? "qrc:/qt/qml/DoubleSlash/Client/icons/check.svg" : ""
                        }

                        MenuItem {
                            text: qsTr("Name (A\u2013Z)")
                            icon.source: roomSortMenu.mark("name_asc")
                            onTriggered: root.setRoomSort("name_asc")
                        }
                        MenuItem {
                            text: qsTr("Name (Z\u2013A)")
                            icon.source: roomSortMenu.mark("name_desc")
                            onTriggered: root.setRoomSort("name_desc")
                        }
                        MenuItem {
                            text: qsTr("Fewest people")
                            icon.source: roomSortMenu.mark("peers_asc")
                            onTriggered: root.setRoomSort("peers_asc")
                        }
                        MenuItem {
                            text: qsTr("Most people")
                            icon.source: roomSortMenu.mark("peers_desc")
                            onTriggered: root.setRoomSort("peers_desc")
                        }
                        MenuItem {
                            text: qsTr("Manual order")
                            icon.source: roomSortMenu.mark("manual")
                            onTriggered: root.setRoomSort("manual")
                        }
                    }
                    Menu {
                        id: peerSortMenu

                        /// The SVG check beside the mode in use, as in the room sort.
                        function mark(mode) {
                            return peerList.sortMode === mode
                                ? "qrc:/qt/qml/DoubleSlash/Client/icons/check.svg" : ""
                        }

                        MenuItem { text: qsTr("Name (A\u2013Z)"); icon.source: peerSortMenu.mark("name_asc"); onTriggered: { peerList.sortMode = "name_asc"; peerModel.setSortMode("name_asc") } }
                        MenuItem { text: qsTr("Name (Z\u2013A)"); icon.source: peerSortMenu.mark("name_desc"); onTriggered: { peerList.sortMode = "name_desc"; peerModel.setSortMode("name_desc") } }
                        MenuItem { text: qsTr("Online first"); icon.source: peerSortMenu.mark("online"); onTriggered: { peerList.sortMode = "online"; peerModel.setSortMode("online") } }
                    }
                }
                Item { Layout.fillWidth: true }
                IconButton {
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
                    icon.color: Theme.text
                    icon.width: 18; icon.height: 18
                    Layout.preferredWidth: 32; Layout.preferredHeight: 32
                    padding: 6
                    Accessible.name: qsTr("Refresh")
                    ToolTip.text: Accessible.name
                    ToolTip.visible: hovered || visualFocus
                    onClicked: root.sidebarTab === 1 ? backend.refreshRooms() : backend.refreshPeers()

                }
            }

            // Tab content (hidden while settings nav is active)
            StackLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: navIndex !== 2
                currentIndex: root.sidebarTab

                // ── Tab 0: Peers ──────────────────────────────────────────
                PeerList {
                    id: peerList
                    peerCount: backend.peer_count
                    peerModel: peerModel
                    selectedPeerId: chatPanel.selectedPeerId
                    showBlocked: root.showBlockedPeers
                    blockedCount: root.blockedPeerCount
                    // Re-read once the identity loads; a bare call never re-evaluates.
                    selfPeerId: backend.public_id !== "" ? backend.selfChatPeerId() : ""
                    selfAvatarId: backend.public_id
                    selfHandle: settingsModel.local_handle
                    selfOnline: backend.self_online
                    ownDevicesOnline: backend.own_devices_online
                    selfUnread: backend.self_chat_unread
                    onSelfSelected: {
                        chatPanel.selectedPeerId = backend.selfChatPeerId()
                        chatPanel.selectedPeerName = qsTr("Message myself")
                        backend.selectPeer(chatPanel.selectedPeerId)
                        navIndex = 0
                    }
                    onPeerSelected: function(peerId, handle) {
                        chatPanel.selectedPeerId = peerId
                        chatPanel.selectedPeerName = handle
                        backend.selectPeer(peerId)
                        navIndex = 0
                        peerModel.setPeerUnread(peerId, 0)
                    }
                    onStartCallRequested: function(peerId) {
                        root.beginDirectCall(peerId)
                    }
                    onRemovePeerRequested: (peerId) => backend.removePeer(peerId)
                    onCopyPeerIdRequested: (peerId) => backend.copyPeerId(peerId)
                    onBlockPeerRequested: (peerId) => backend.blockPeer(peerId)
                    onUnblockPeerRequested: (peerId) => backend.unblockPeer(peerId)
                    onClearHistoryRequested: function(peerId) {
                        backend.clearPeerHistory(peerId)
                    }
                }

                // ── Tab 1: Rooms (supernodes + grouped rooms) ─────────────
                ColumnLayout {
                    spacing: 0

                    Menu {
                        id: roomContextMenu
                        property string targetSupernodeId: ""
                        property string targetRoomId: ""
                        property string targetRoomName: ""
                        property bool targetCanRemove: false
                        property bool targetHidden: false
                        property var targetRooms: []
                        readonly property string targetKey: targetSupernodeId + ":" + targetRoomId
                        readonly property bool targetPinned: RoomTree.isPinned(root.roomListOrder, targetKey)
                        readonly property bool targetManual: RoomTree.sortMode(root.roomListOrder) === "manual"
                        property bool targetAlerts: false

                        MenuItem {
                            text: qsTr("Join Voice Room")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/phone.svg"
                            onTriggered: {
                                roomPanel.switchToRoom(
                                    roomContextMenu.targetRoomName,
                                    roomContextMenu.targetRoomId,
                                    roomContextMenu.targetSupernodeId)
                                backend.joinRoomWithVoice(
                                    roomContextMenu.targetSupernodeId,
                                    roomContextMenu.targetRoomId)
                                root.voiceRoomName = roomContextMenu.targetRoomName
                                root.voiceSupernodeId = roomContextMenu.targetSupernodeId
                                root.voiceRoomId = roomContextMenu.targetRoomId
                                navIndex = 1
                            }
                        }
                        MenuItem {
                            text: qsTr("Copy Room Invite")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/clipboard.svg"
                            onTriggered: {
                                var url = backend.generateRoomInvite(
                                    roomContextMenu.targetSupernodeId,
                                    roomContextMenu.targetRoomId,
                                    roomContextMenu.targetRoomName)
                                if (url !== "") {
                                    backend.copyToClipboard(url)
                                    invitePopup.visible = true
                                } else {
                                    root.showTrayNotice(
                                        qsTr("Room invite"),
                                        qsTr("Couldn't build the invite — connect to the room's supernode first."),
                                        Platform.SystemTrayIcon.Warning, 5000, null)
                                }
                            }
                        }
                        // Per-contact invite: embeds an owner-signed SpaceGrant
                        // bound to the chosen peer, so a private room admits them
                        // durably by proof+grant (survives supernode restarts).
                        Menu {
                            id: inviteContactMenu
                            title: qsTr("Invite Contact to Room")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
                            enabled: contactInviteInstantiator.count > 0
                            Instantiator {
                                id: contactInviteInstantiator
                                model: peerModel
                                delegate: MenuItem {
                                    text: (handle && handle !== "") ? handle : peerId
                                    onTriggered: {
                                        var url = backend.generateRoomInviteForPeer(
                                            roomContextMenu.targetSupernodeId,
                                            roomContextMenu.targetRoomId,
                                            roomContextMenu.targetRoomName,
                                            peerId)
                                        if (url !== "") {
                                            backend.copyToClipboard(url)
                                            invitePopup.visible = true
                                        } else {
                                            root.showTrayNotice(
                                                qsTr("Room invite"),
                                                qsTr("Couldn't build the invite — you must own this room's Space and be connected to its supernode."),
                                                Platform.SystemTrayIcon.Warning, 5000, null)
                                        }
                                    }
                                }
                                onObjectAdded: (index, object) => inviteContactMenu.insertItem(index, object)
                                onObjectRemoved: (index, object) => inviteContactMenu.removeItem(object)
                            }
                        }
                        MenuSeparator {}
                        MenuItem {
                            text: qsTr("Create Public Sub-room…")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/globe.svg"
                            onTriggered: createRoomDialog.openForParent(
                                roomContextMenu.targetSupernodeId,
                                "public",
                                roomContextMenu.targetRoomId,
                                roomContextMenu.targetRoomName)
                        }
                        MenuItem {
                            text: qsTr("Create Private Sub-room…")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/lock.svg"
                            onTriggered: createRoomDialog.openForParent(
                                roomContextMenu.targetSupernodeId,
                                "private",
                                roomContextMenu.targetRoomId,
                                roomContextMenu.targetRoomName)
                        }
                        MenuSeparator {}
                        MenuItem {
                            text: roomContextMenu.targetPinned
                                ? qsTr("Stop keeping at top")
                                : qsTr("Keep at top")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/pin.svg"
                            onTriggered: root.commitRoomOrder(
                                RoomTree.togglePin(root.roomListOrder, roomContextMenu.targetKey))
                        }
                        MenuItem {
                            text: qsTr("Move up")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/arrow-up.svg"
                            visible: roomContextMenu.targetManual || roomContextMenu.targetPinned
                            height: visible ? implicitHeight : 0
                            enabled: RoomTree.canMove(
                                roomContextMenu.targetRooms,
                                roomContextMenu.targetSupernodeId,
                                roomContextMenu.targetRoomId,
                                -1,
                                root.roomListOrder)
                            onTriggered: root.commitRoomOrder(RoomTree.moveRoom(
                                roomContextMenu.targetRooms,
                                roomContextMenu.targetSupernodeId,
                                roomContextMenu.targetRoomId,
                                -1,
                                root.roomListOrder))
                        }
                        MenuItem {
                            text: qsTr("Move down")
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/arrow-down.svg"
                            visible: roomContextMenu.targetManual || roomContextMenu.targetPinned
                            height: visible ? implicitHeight : 0
                            enabled: RoomTree.canMove(
                                roomContextMenu.targetRooms,
                                roomContextMenu.targetSupernodeId,
                                roomContextMenu.targetRoomId,
                                1,
                                root.roomListOrder)
                            onTriggered: root.commitRoomOrder(RoomTree.moveRoom(
                                roomContextMenu.targetRooms,
                                roomContextMenu.targetSupernodeId,
                                roomContextMenu.targetRoomId,
                                1,
                                root.roomListOrder))
                        }
                        MenuItem {
                            text: roomContextMenu.targetAlerts
                                ? qsTr("Mute message alerts")
                                : qsTr("Enable message alerts")
                            icon.source: roomContextMenu.targetAlerts
                                ? "qrc:/qt/qml/DoubleSlash/Client/icons/bell-off.svg"
                                : "qrc:/qt/qml/DoubleSlash/Client/icons/bell.svg"
                            onTriggered: root.setRoomMessageAlerts(
                                roomContextMenu.targetRoomId,
                                !roomContextMenu.targetAlerts)
                        }
                        MenuSeparator {
                            visible: roomContextMenu.targetCanRemove
                        }
                        MenuItem {
                            text: roomContextMenu.targetHidden
                                ? qsTr("Show in list")
                                : qsTr("Hide Room")
                            icon.source: roomContextMenu.targetHidden
                                ? "qrc:/qt/qml/DoubleSlash/Client/icons/eye.svg"
                                : "qrc:/qt/qml/DoubleSlash/Client/icons/eye-off.svg"
                            visible: roomContextMenu.targetCanRemove
                            onTriggered: {
                                if (roomContextMenu.targetHidden)
                                    root.unhideRoom(
                                        roomContextMenu.targetSupernodeId,
                                        roomContextMenu.targetRoomId)
                                else
                                    backend.removeRoom(
                                        roomContextMenu.targetSupernodeId,
                                        roomContextMenu.targetRoomId)
                            }
                        }
                    }

                    ListView {
                        id: roomsListView
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        model: nodeListModel
                        clip: true
                        spacing: Theme.spacingXs

                        EmptyState {
                            anchors.centerIn: parent
                            visible: nodeListModel.count === 0
                            width: Math.min(parent.width - Theme.spacingXl, 170)
                            iconSource: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                            iconSize: 30
                            title: "No rooms"
                            subtitle: "Accept a supernode invite to see the rooms it hosts."
                        }

                        EmptyState {
                            anchors.centerIn: parent
                            visible: nodeListModel.count > 0
                                && root.visibleRoomCount === 0
                                && root.hiddenRoomCount > 0
                                && !root.showHiddenRooms
                            width: Math.min(parent.width - Theme.spacingXl, 170)
                            iconSource: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                            iconSize: 30
                            title: "All rooms are hidden"
                            subtitle: "Use Show hidden in the list header to show them."
                        }

                        delegate: Item {
                            id: roomGroup
                            required property string node_id
                            required property bool connected
                            required property string title
                            required property string rooms_json

                            readonly property var rooms: {
                                var showHidden = root.showHiddenRooms
                                var parsed = []
                                try {
                                    parsed = JSON.parse(roomGroup.rooms_json || "[]")
                                } catch (e) {
                                    return []
                                }
                                if (showHidden)
                                    return parsed
                                var out = []
                                for (var i = 0; i < parsed.length; i++) {
                                    if (!parsed[i] || !parsed[i].hidden)
                                        out.push(parsed[i])
                                }
                                return out
                            }

                            // Rooms with their Voice / Text-only leaves and
                            // members, flattened parent-first (RoomTree.rows).
                            readonly property var roomsTree:
                                RoomTree.rows(roomGroup.rooms, roomGroup.node_id,
                                              roomGroup.connected, root.treeContext)

                            visible: backend.isKnownSupernode(roomGroup.node_id)
                            width: roomsListView.width
                            height: visible ? roomColumn.implicitHeight + Theme.spacingXs * 2 : 0

                            // One group per supernode is still how rooms arrive,
                            // but nothing marks the boundary: the list reads as
                            // one tree, and nodes are managed in Settings › Network.
                            Column {
                                id: roomColumn
                                y: Theme.spacingXs
                                width: parent.width
                                spacing: 0

                                Label {
                                    visible: roomGroup.rooms.length === 0
                                    width: roomColumn.width
                                    height: Math.max(40, Theme.fontSizeCaption + 16)
                                    verticalAlignment: Text.AlignVCenter
                                    // Named: a bare "No rooms" would not say which
                                    // supernode is the empty one.
                                    text: roomGroup.title !== ""
                                        ? qsTr("No rooms on %1").arg(roomGroup.title)
                                        : qsTr("No rooms")
                                    color: Theme.muted
                                    font.pixelSize: Theme.fontSizeCaption
                                    leftPadding: Theme.spacingMd
                                }

                                Repeater {
                                    model: roomGroup.roomsTree

                                    delegate: Loader {
                                        id: treeRow
                                        required property var modelData
                                        readonly property var row: treeRow.modelData
                                        width: roomColumn.width
                                        height: item ? item.implicitHeight : 0
                                        sourceComponent: row.row_kind === "room" ? roomRowComp
                                            : row.row_kind === "group" ? groupRowComp
                                            : row.row_kind === "member" ? memberRowComp
                                            : row.row_kind === "session" ? sessionRowComp
                                            : moreRowComp

                                        // ── Room ────────────────────────────────
                                        Component {
                                            id: roomRowComp

                                            ItemDelegate {
                                                id: roomDelegate
                                                readonly property var row: treeRow.row
                                                readonly property string roomName: row.name || row.room_id
                                                readonly property int treeStep: Theme.spacingLg
                                                readonly property int treeIndent:
                                                    Theme.spacingSm + row.guide_cols.length * treeStep
                                                readonly property bool canRemove:
                                                    row.is_default !== true && row.room_id !== "default"
                                                readonly property bool roomSelected:
                                                    roomPanel.supernodeId !== ""
                                                    && roomPanel.supernodeId === roomGroup.node_id
                                                    && roomPanel.roomId === row.room_id
                                                readonly property bool voiceHere:
                                                    backend.voice_in_room
                                                    && root.voiceRoomId === row.room_id
                                                    && root.voiceSupernodeId === roomGroup.node_id
                                                readonly property bool isPrivate:
                                                    String(row.kind || "").toLowerCase() === "private"

                                                // 34 at the designed size; taller once the name no longer fits.
                                                implicitHeight: Math.max(34, Theme.fontSizeBody + 8)
                                                padding: 0

                                                background: Rectangle {
                                                    color: roomDelegate.roomSelected
                                                        ? Theme.selectedFill()
                                                        : (roomDelegate.hovered ? Theme.bg3 : "transparent")
                                                    Behavior on color {
                                                        ColorAnimation { duration: Theme.animNormal }
                                                    }

                                                    Rectangle {
                                                        visible: roomDelegate.roomSelected
                                                        width: 3
                                                        anchors {
                                                            left: parent.left
                                                            top: parent.top
                                                            bottom: parent.bottom
                                                        }
                                                        color: Theme.accent
                                                    }
                                                }

                                                onClicked: root.openRoomFromTree(
                                                    roomGroup.node_id, row.room_id, roomDelegate.roomName)
                                                onDoubleClicked: root.joinVoiceFromTree(
                                                    roomGroup.node_id, row.room_id, roomDelegate.roomName)

                                                MouseArea {
                                                    anchors.fill: parent
                                                    acceptedButtons: Qt.RightButton
                                                    onClicked: (mouse) => {
                                                        roomContextMenu.targetSupernodeId = roomGroup.node_id
                                                        roomContextMenu.targetRoomId = row.room_id
                                                        roomContextMenu.targetRoomName = roomDelegate.roomName
                                                        roomContextMenu.targetCanRemove = roomDelegate.canRemove
                                                        roomContextMenu.targetHidden = row.hidden === true
                                                        roomContextMenu.targetRooms = roomGroup.rooms
                                                        roomContextMenu.targetAlerts = root.roomAlertsEnabled(row.room_id)
                                                        roomContextMenu.popup()
                                                    }
                                                }

                                                TreeGuides {
                                                    x: Theme.spacingSm
                                                    height: parent.height
                                                    guides: row.guide_cols
                                                }

                                                RowLayout {
                                                    anchors {
                                                        left: parent.left
                                                        right: parent.right
                                                        verticalCenter: parent.verticalCenter
                                                        leftMargin: roomDelegate.treeIndent
                                                        rightMargin: Theme.spacingSm
                                                    }
                                                    spacing: Theme.spacingXs

                                                    // Expand/collapse: an SVG caret rotated in
                                                    // place, so it never depends on the UI font.
                                                    Item {
                                                        Layout.preferredWidth: 18
                                                        Layout.preferredHeight: 18
                                                        Layout.alignment: Qt.AlignVCenter

                                                        // A square behind the caret on hover,
                                                        // so it reads as its own control.
                                                        Rectangle {
                                                            anchors.fill: parent
                                                            visible: row.has_children && caretArea.containsMouse
                                                            color: Qt.rgba(Theme.text.r, Theme.text.g, Theme.text.b, 0.12)
                                                        }

                                                        Image {
                                                            anchors.centerIn: parent
                                                            width: 10
                                                            height: 10
                                                            sourceSize.width: 20
                                                            sourceSize.height: 20
                                                            fillMode: Image.PreserveAspectFit
                                                            smooth: true
                                                            visible: row.has_children
                                                            source: "qrc:/qt/qml/DoubleSlash/Client/icons/chevron.svg"
                                                            rotation: row.collapsed ? 0 : 90
                                                            // Open reads at full strength, folded recedes.
                                                            opacity: row.collapsed ? 0.6 : 1.0
                                                            Behavior on rotation {
                                                                NumberAnimation { duration: Theme.animNormal }
                                                            }
                                                        }

                                                        MouseArea {
                                                            id: caretArea
                                                            anchors.fill: parent
                                                            anchors.margins: -4
                                                            enabled: row.has_children
                                                            hoverEnabled: true
                                                            cursorShape: Qt.PointingHandCursor
                                                            onClicked: root.toggleRoomCollapse(
                                                                roomGroup.node_id, row.room_id)
                                                        }
                                                    }

                                                    Image {
                                                        visible: roomDelegate.isPrivate
                                                        source: "qrc:/qt/qml/DoubleSlash/Client/icons/lock.svg"
                                                        sourceSize.width: 12; sourceSize.height: 12
                                                        Layout.preferredWidth: 12
                                                        Layout.preferredHeight: 12
                                                        Layout.alignment: Qt.AlignVCenter
                                                        opacity: 0.7
                                                    }

                                                    Label {
                                                        Layout.fillWidth: true
                                                        text: roomDelegate.roomName
                                                        color: row.hidden
                                                            ? Theme.muted
                                                            : (roomDelegate.voiceHere ? Theme.online : Theme.text)
                                                        font.pixelSize: Theme.fontSizeBody
                                                        font.bold: roomDelegate.roomSelected
                                                            || roomDelegate.voiceHere
                                                            || root.roomUnreadCount(row.room_id) > 0
                                                        elide: Text.ElideRight
                                                    }

                                                    Rectangle {
                                                        visible: root.roomUnreadCount(row.room_id) > 0
                                                        width: Math.max(20, roomUnreadText.implicitWidth + 8)
                                                        height: Math.max(20, roomUnreadText.implicitHeight + 4)
                                                        color: Theme.danger
                                                        Layout.alignment: Qt.AlignVCenter

                                                        Text {
                                                            id: roomUnreadText
                                                            anchors.centerIn: parent
                                                            text: root.roomUnreadCount(row.room_id) > 99
                                                                ? "99+"
                                                                : root.roomUnreadCount(row.room_id).toString()
                                                            color: Theme.textInv
                                                            font.pixelSize: Theme.fontSizeCaption
                                                            font.bold: true
                                                        }
                                                    }

                                                    Label {
                                                        visible: RoomTree.isPinned(
                                                            root.roomListOrder,
                                                            roomGroup.node_id + ":" + row.room_id)
                                                        text: qsTr("top")
                                                        color: Theme.accent
                                                        font.pixelSize: Theme.fontSizeCaption
                                                        font.bold: true
                                                        Layout.alignment: Qt.AlignVCenter
                                                    }

                                                    Image {
                                                        visible: roomDelegate.voiceHere
                                                        source: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                                                        sourceSize.width: 13; sourceSize.height: 13
                                                        Layout.preferredWidth: 13
                                                        Layout.preferredHeight: 13
                                                        Layout.alignment: Qt.AlignVCenter
                                                        ToolTip.text: qsTr("You are in voice here")
                                                        ToolTip.visible: youHereHover.hovered
                                                        HoverHandler { id: youHereHover }
                                                    }

                                                    // Folded: who is inside, without unfolding.
                                                    Row {
                                                        visible: row.collapsed && row.stack_ids.length > 0
                                                        spacing: -5
                                                        Layout.alignment: Qt.AlignVCenter
                                                        Repeater {
                                                            model: row.stack_ids
                                                            delegate: Avatar {
                                                                required property var modelData
                                                                peerId: modelData
                                                                size: 16
                                                            }
                                                        }
                                                    }

                                                    CountPill {
                                                        // A node that is gone has no live roster:
                                                        // "—" rather than a stale number.
                                                        visible: !roomGroup.connected
                                                            || (row.collapsed && row.subtree_voice > 0)
                                                        iconSource: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                                                        text: roomGroup.connected ? String(row.subtree_voice) : "—"
                                                        tint: roomGroup.connected ? Theme.online : Theme.muted
                                                        active: roomGroup.connected
                                                        tip: roomGroup.connected
                                                            ? qsTr("In voice")
                                                            : qsTr("Supernode offline — counts may be stale")
                                                    }

                                                    CountPill {
                                                        visible: roomGroup.connected && row.collapsed && row.room_chat > 0
                                                        iconSource: "qrc:/qt/qml/DoubleSlash/Client/icons/speech.svg"
                                                        text: String(row.room_chat)
                                                        tint: Theme.accent
                                                        active: true
                                                        tip: qsTr("In text only")
                                                    }

                                                    // Join voice, explicitly. A double-click on
                                                    // the row still does the same.
                                                    // Join voice: solid green on a green square, so
                                                    // joining stands out. In the voice room we are in
                                                    // it turns into hang-up: the handset inverted, in
                                                    // red, leaving as the voice dock's Leave does.
                                                    IconButton {
                                                        readonly property bool endsCall: row.voice_here === true
                                                        readonly property color callColor:
                                                            endsCall ? Theme.danger : Theme.online
                                                        visible: row.show_call === true
                                                        implicitWidth: 26
                                                        implicitHeight: 26
                                                        padding: 0
                                                        Layout.alignment: Qt.AlignVCenter
                                                        icon.source: endsCall
                                                            ? "qrc:/qt/qml/DoubleSlash/Client/icons/phone-hangup.svg"
                                                            : "qrc:/qt/qml/DoubleSlash/Client/icons/phone.svg"
                                                        icon.width: 16
                                                        icon.height: 16
                                                        icon.color: callColor
                                                        tileColor: Theme.semanticTint(callColor, 0.18)
                                                        tileBorder: callColor
                                                        Accessible.name: endsCall ? qsTr("Leave voice") : qsTr("Join voice")
                                                        ToolTip.text: endsCall
                                                            ? qsTr("Leave voice")
                                                            : qsTr("Join voice — or double-click the room")
                                                        ToolTip.visible: hovered
                                                        ToolTip.delay: 400
                                                        onClicked: {
                                                            if (endsCall)
                                                                voiceDock.endCallRequested()
                                                            else
                                                                root.joinVoiceFromTree(
                                                                    roomGroup.node_id, row.room_id, roomDelegate.roomName)
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // ── Voice / Text-only leaf ─────────────
                                        Component {
                                            id: groupRowComp

                                            Item {
                                                id: groupRow
                                                readonly property var row: treeRow.row
                                                readonly property bool isVoice: row.group === "voice"
                                                implicitHeight: Math.max(26, Theme.fontSizeCaption + 10)

                                                Rectangle {
                                                    anchors.fill: parent
                                                    color: groupHover.hovered ? Theme.bg2 : "transparent"
                                                }

                                                TreeGuides {
                                                    x: Theme.spacingSm
                                                    height: parent.height
                                                    guides: row.guide_cols
                                                }

                                                RowLayout {
                                                    anchors {
                                                        left: parent.left
                                                        right: parent.right
                                                        verticalCenter: parent.verticalCenter
                                                        leftMargin: Theme.spacingSm + row.guide_cols.length * Theme.spacingLg
                                                        rightMargin: Theme.spacingSm
                                                    }
                                                    spacing: Theme.spacingXs

                                                    Item {
                                                        id: leafToggle
                                                        Layout.fillWidth: true
                                                        Layout.preferredHeight: Math.max(26, Theme.fontSizeCaption + 10)

                                                        RowLayout {
                                                            anchors.fill: parent
                                                            spacing: Theme.spacingXs

                                                            Item {
                                                                Layout.preferredWidth: 16
                                                                Layout.preferredHeight: 16
                                                                Image {
                                                                    anchors.centerIn: parent
                                                                    width: 8; height: 8
                                                                    sourceSize.width: 16; sourceSize.height: 16
                                                                    source: "qrc:/qt/qml/DoubleSlash/Client/icons/chevron.svg"
                                                                    rotation: row.expanded ? 90 : 0
                                                                    opacity: row.expanded ? 1.0 : 0.6
                                                                    Behavior on rotation {
                                                                        NumberAnimation { duration: Theme.animFast }
                                                                    }
                                                                }
                                                            }
                                                            Image {
                                                                id: leafIcon
                                                                // A folded list recedes; an open one reads in full.
                                                                opacity: row.expanded ? 1.0 : 0.6
                                                                source: groupRow.isVoice
                                                                    ? "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                                                                    : "qrc:/qt/qml/DoubleSlash/Client/icons/speech.svg"
                                                                sourceSize.width: 14; sourceSize.height: 14
                                                                Layout.preferredWidth: 14
                                                                Layout.preferredHeight: 14
                                                                Layout.alignment: Qt.AlignVCenter
                                                                Accessible.name: groupRow.isVoice ? qsTr("Voice") : qsTr("Text only")
                                                                ToolTip.text: groupRow.isVoice ? qsTr("Voice") : qsTr("Text only")
                                                                ToolTip.visible: leafIconHover.hovered
                                                                ToolTip.delay: Theme.animSlow
                                                                HoverHandler { id: leafIconHover }
                                                            }
                                                            Text {
                                                                text: row.count
                                                                color: row.expanded ? Theme.text : Theme.muted
                                                                font.bold: row.expanded
                                                                font.pixelSize: Theme.fontSizeCaption
                                                            }
                                                            Item { Layout.fillWidth: true }
                                                        }

                                                        HoverHandler { id: groupHover; cursorShape: Qt.PointingHandCursor }
                                                        TapHandler {
                                                            onTapped: root.setTreeFlag("leaf", row.key, !row.expanded)
                                                        }
                                                    }
                                                }
                                            }
                                        }

                                        // ── A roster member ────────────────────
                                        Component {
                                            id: memberRowComp

                                            RoomTreeMember {
                                                readonly property var row: treeRow.row
                                                peerId: row.id
                                                displayName: row.name
                                                isSelf: row.is_self
                                                inVoice: row.group === "voice"
                                                inSession: false
                                                videoActive: row.group === "voice" && row.on_roster
                                                    && root.videoActivePeers[row.id] === true
                                                watching: root.watchedVideoPeers.indexOf(row.id) !== -1
                                                trusted: row.trusted
                                                listPeerId: row.list_peer_id
                                                inviteState: root.trustInviteStates[row.id] || ""
                                                canInvite: row.can_invite
                                                expanded: root.openMemberKey === row.key
                                                guides: row.guide_cols
                                                onToggleRequested: root.toggleOpenMember(row.key)
                                                onMessageRequested: root.messagePeer(row.list_peer_id, row.name)
                                                onInviteRequested: root.sendTrustInvite(row.room_id, row.id)
                                            }
                                        }

                                        // ── The live voice session ─────────────
                                        //
                                        // Drawn from roomModel rather than the
                                        // roster, because that is where levels,
                                        // their mute and "muted for me" live.
                                        Component {
                                            id: sessionRowComp

                                            Column {
                                                id: sessionColumn
                                                readonly property var row: treeRow.row

                                                Repeater {
                                                    id: sessionRepeater
                                                    model: roomModel

                                                    delegate: RoomTreeMember {
                                                        id: sessionMember
                                                        required property var model
                                                        required property int index
                                                        readonly property string memberKey:
                                                            roomGroup.node_id + ":" + sessionColumn.row.room_id
                                                            + ":voice:" + (model.peerId || "")

                                                        width: sessionColumn.width
                                                        peerId: model.peerId || ""
                                                        displayName: model.handle || ""
                                                        isSelf: model.isSelf === true
                                                        inVoice: true
                                                        inSession: true
                                                        isMuted: model.muted === true
                                                        audioLevel: model.isSelf ? backend.mic_level : (model.audioLevel || 0.0)
                                                        videoActive: model.videoActive === true
                                                            || root.videoActivePeers[model.peerId] === true
                                                        watching: root.watchedVideoPeers.indexOf(model.peerId) !== -1
                                                        locallyMuted: model.localMuted === true
                                                        localVolume: model.localVolume === undefined ? 100 : model.localVolume
                                                        trusted: model.trusted === true
                                                        listPeerId: model.listPeerId || ""
                                                        inviteState: root.trustInviteStates[model.peerId] || ""
                                                        canInvite: sessionColumn.row.can_invite
                                                        expanded: root.openMemberKey === sessionMember.memberKey
                                                        guides: sessionColumn.row.pass_cols.concat(
                                                            [sessionMember.index === sessionRepeater.count - 1 ? 2 : 3])

                                                        onToggleRequested: root.toggleOpenMember(sessionMember.memberKey)
                                                        onWatchToggled: root.toggleWatching(sessionMember.peerId)
                                                        onPopoutRequested: root.popoutVideo(sessionMember.peerId)
                                                        onLocalAudioChanged: function(muted, volume) {
                                                            backend.setPeerAudioPref(sessionMember.peerId, muted, volume)
                                                            roomModel.setLocalAudio(sessionMember.peerId, muted, volume)
                                                        }
                                                        onMessageRequested: root.messagePeer(
                                                            sessionMember.listPeerId, sessionMember.displayName)
                                                        onInviteRequested: root.sendTrustInvite(
                                                            sessionColumn.row.room_id, sessionMember.peerId)
                                                    }
                                                }
                                            }
                                        }

                                        // ── "+N more" / "Show fewer" ───────────
                                        Component {
                                            id: moreRowComp

                                            Item {
                                                readonly property var row: treeRow.row
                                                implicitHeight: Math.max(24, Theme.fontSizeCaption + 8)

                                                TreeGuides {
                                                    x: Theme.spacingSm
                                                    height: parent.height
                                                    guides: row.guide_cols
                                                }

                                                Text {
                                                    x: Theme.spacingSm + row.guide_cols.length * Theme.spacingLg + 4
                                                    anchors.verticalCenter: parent.verticalCenter
                                                    text: row.label
                                                    color: Theme.linkPeer
                                                    font.pixelSize: Theme.fontSizeCaption
                                                    font.underline: moreHover.hovered
                                                    HoverHandler { id: moreHover; cursorShape: Qt.PointingHandCursor }
                                                    TapHandler {
                                                        onTapped: root.setTreeFlag("overflow", row.key,
                                                            root.memberOverflowOpen[row.key] !== true)
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                }
            }                    // end sidebar StackLayout

            // ── Settings section navigation ──────────────────────────────────
            // Replaces the peer/rooms area when Settings is the active nav.
            SettingsSidebar {
                id: settingsSidebar
                visible: navIndex === 2
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: settingsTab
                dirty: settingsModel ? settingsModel.dirty : false
                onSectionActivated: (index) => settingsTab = index
                onBackRequested: root.leaveSettings()
                onSaveRequested: if (settingsModel) settingsModel.save()

                // Polled rather than pushed: settings are written from around a
                // hundred places, most of which save immediately and some of
                // which do not, so a flag each writer had to set would be wrong
                // the first time one was added. The model compares itself with
                // what is on disk instead, which cannot drift.
                //
                // Only while Settings is on screen — this is for the button.
                Timer {
                    interval: 300
                    repeat: true
                    running: settingsSidebar.visible && settingsModel !== null
                    triggeredOnStart: true
                    onTriggered: settingsModel.refreshDirty()
                }
            }

            // ── The live call ────────────────────────────────────────────────
            // At the foot of the sidebar whatever it shows, so the session and
            // its controls never leave the screen.
            VoiceDock {
                id: voiceDock
                Layout.fillWidth: true
                Layout.preferredHeight: implicitHeight
                visible: backend.voice_active

                callState: backend.call_state
                inRoom: backend.voice_in_room
                contextName: backend.voice_in_room
                    ? root.voiceRoomPath()
                    : (root.activeCallPeerHandle() || chatPanel.selectedPeerName || qsTr("Call"))
                connectionMode: backend.connection_mode
                durationSecs: backend.call_duration_secs
                unwatchedStreamers: root.unwatchedStreamers.map(function (s) { return s.name })
                inviteNotice: root.trustInviteNotice

                onOpenSessionRequested: {
                    if (backend.voice_in_room) {
                        root.sidebarTab = 1
                        root.openRoomFromTree(root.voiceSupernodeId, root.voiceRoomId, root.voiceRoomName)
                    } else if (root.activeCallPeerId !== "") {
                        root.sidebarTab = 0
                        navIndex = 0
                    }
                }
                onWatchStreamersRequested: {
                    var list = root.unwatchedStreamers
                    for (var i = 0; i < list.length; i++)
                        root.toggleVideoExpanded(list[i].id)
                }

                onEndCallRequested: {
                    // Collapse expand/popout UI before the session ends so tiles
                    // unregister and we do not keep a detached window on a stream
                    // that is about to stop.
                    root.closeAllVideoPopouts()
                    root.expandedVideoPeers = []
                    if (backend.voice_in_room) {
                        backend.leaveRoom()
                        // Stay on the room text panel when a text room is still selected.
                        if (!roomPanel.roomId)
                            navIndex = 0
                    } else {
                        backend.endCall()
                    }
                }
                onMuteToggled: (m) => backend.setMuted(m)

                videoOn: backend.video_active
                shareAudioOn: backend.content_audio_active

                videoSources: root.shareCaptureSources
                videoSourceId: settingsModel.video_input_device
                videoOverlaysJson: settingsModel.video_overlays_json
                contentAudioMode: settingsModel.content_audio_mode

                videoUnavailableReason: root.videoUnavailableReason
                videoEncoderMissing: !root.videoEncoderAvailable

                onShareOptionsOpened: root.refreshShareCaptureSources()
                // Written straight to settings, so the menu and Settings › Video
                // are two views of one choice rather than two choices that can
                // disagree. Not saved here: the settings page's Save button owns
                // that, exactly as the audio mode below has always worked.
                onVideoSourceSelected: (sourceId) => settingsModel.video_input_device = sourceId
                onVideoOverlaysEdited: (json) => settingsModel.video_overlays_json = json

                /// Start sharing video, and the audio that belongs with it.
                ///
                /// Order matters: video creates the session clock, and the content
                /// audio is timestamped against it. Starting audio first would
                /// stamp it against a clock that does not exist yet.
                onShareRequested: (audioMode) => {
                    settingsModel.content_audio_mode = audioMode

                    var got = backend.setVideoEnabled(
                        true,
                        settingsModel.video_input_device,
                        settingsModel.video_quality,
                        settingsModel.video_overlays_json,
                        settingsModel.videoEncoderJson())
                    if (!got) {
                        console.warn("[video] could not start sharing")
                        return
                    }
                    // Reflect our own state on our own row straight away; remote
                    // members learn about it from the SfuVideoState announcement.
                    // `public_id` is the exposed Q_PROPERTY — `my_public_id` is the
                    // internal Rust field name and reads as undefined from QML.
                    if (roomModel && roomModel.setVideoActive && backend.public_id) {
                        roomModel.setVideoActive(backend.public_id, true)
                        textRoomModel.setVideoActive(backend.public_id, true)
                    }
                    root.setPeerVideoActive(backend.public_id, true)

                    if (audioMode === "off")
                        return
                    // Audio failing is not a reason to abandon the video that is
                    // already running — the platform may simply have no loopback.
                    var audioOk = backend.setContentAudioEnabled(
                        true,
                        settingsModel.video_input_device,
                        audioMode)
                    if (audioOk)
                        return
                    // "auto" on a camera resolves to no audio by design — your
                    // microphone already carries you — so it is not a failure and
                    // must not be logged as one, or the real failures below stop
                    // being worth reading.
                    var dev = settingsModel.video_input_device
                    var isScreen = dev.indexOf("window:") === 0
                        || dev.indexOf("monitor:") === 0
                    if (audioMode === "auto" && !isScreen)
                        return
                    console.warn("[content-audio] sharing video without audio: "
                        + "no capture endpoint for mode '" + audioMode + "'")
                }

                /// Stop both. Content audio cannot outlive the clock it is stamped
                /// against, so it is stopped first and explicitly.
                onStopShareRequested: {
                    if (backend.content_audio_active)
                        backend.setContentAudioEnabled(
                            false,
                            settingsModel.video_input_device,
                            settingsModel.content_audio_mode)
                    backend.setVideoEnabled(
                        false,
                        settingsModel.video_input_device,
                        settingsModel.video_quality,
                        settingsModel.video_overlays_json,
                        settingsModel.videoEncoderJson())
                    if (roomModel && roomModel.setVideoActive && backend.public_id) {
                        roomModel.setVideoActive(backend.public_id, false)
                        textRoomModel.setVideoActive(backend.public_id, false)
                    }
                    root.setPeerVideoActive(backend.public_id, false)
                }
            }
        }

        // Content area (chat / rooms / settings)
        // Using visibility-based switching rather than StackLayout so that
        // each child always has anchors.fill: parent — they get the correct
        // size at the first layout pass with no lazy-init timing dependency.
        Item {
            id: contentArea
            Layout.fillWidth: true
            Layout.minimumWidth: 200
            Layout.fillHeight: true

            // Shared video area. Only chat and room views yield space to it —
            // settings and the portal keep the full area, so the region never
            // overlaps a page that has nothing to do with a call.
            Loader {
                id: videoRegionLoader
                anchors { top: parent.top; left: parent.left; right: parent.right }
                z: 40
                // Loaded lazily: a build without Qt Multimedia has no
                // VideoRegion in the qrc at all, and MainWindow must still parse.
                active: root.expandedVideoPeers.length > 0
                source: "qrc:/qt/qml/DoubleSlash/Client/qml/VideoRegion.qml"

                readonly property bool showing:
                    active && (navIndex === 0 || navIndex === 1)
                visible: showing
                height: showing
                    ? Math.round(contentArea.height * root.videoRegionRatio)
                    : 0

                Behavior on height {
                    NumberAnimation { duration: Theme.animFast; easing.type: Easing.InOutQuad }
                }

                onLoaded: {
                    item.expandedPeers = Qt.binding(() => root.expandedVideoPeers)
                    item.videoActivePeers = Qt.binding(() => root.videoActivePeers)
                    item.videoStalledPeers = Qt.binding(() => root.videoStalledPeers)
                    item.participantModel = Qt.binding(() =>
                        backend.voice_in_room ? roomModel : directCallModel)
                    item.heightRatio = Qt.binding(() => root.videoRegionRatio)
                    item.collapseRequested.connect(root.collapseVideo)
                    item.popoutRequested.connect(root.popoutVideo)
                    // `backend` is only in scope here, so the tile's request
                    // arrives as a signal and is applied at this level.
                    item.contentAudioChanged.connect(function(peerId, muted, volume) {
                        backend.setContentAudioPref(peerId, muted, volume)
                    })
                    item.ratioChanged.connect(function(r) {
                        root.videoRegionRatio = r
                        videoRegionRatioSaveTimer.restart()
                    })
                }
            }

            ChatPanel {
                id: chatPanel
                anchors {
                    top: videoRegionLoader.bottom
                    left: parent.left
                    right: parent.right
                    bottom: parent.bottom
                }
                visible: navIndex === 0
                chatModel: chatModel
                fileTransferModel: fileTransferModel
                settingsModel: settingsModel
                youtubePreviewEnabled: settingsModel ? settingsModel.youtube_preview_enabled : true
                youtubeInlineAck: settingsModel ? settingsModel.youtube_inline_ack : false
                // `call_state` moves only on the direct-call paths — room voice
                // never touches it — so this is a 1:1 with the selected peer and
                // not a room they happen to share.
                callActiveWithPeer: backend.call_state !== "idle"
                                    && root.activeCallPeerId !== ""
                                    && root.activeCallPeerId === chatPanel.selectedPeerId
                onSendMessage: (peerId, msg) => backend.sendChat(peerId, msg)
                onStartCall: function(peerId) {
                    root.beginDirectCall(peerId)
                }
                onSendFile: (peerId, fileUrl) => backend.sendFile(peerId, fileUrl)
                onOpenAttachment: (path) => root.showFilePreview(path)
                Component.onCompleted: chatPanel.onActiveFocusChanged.connect(function() {
                    if (chatPanel.activeFocus) backend.clearUnread()
                })
            }

            RoomPanel {
                id: roomPanel
                anchors {
                    top: videoRegionLoader.bottom
                    left: parent.left
                    right: parent.right
                    bottom: parent.bottom
                }
                visible: navIndex === 1
                // Text members only — never the active voice roster (voice rail
                // owns roomModel). Peers who share this room's chat space.
                roomModel: textRoomModel
                fileTransferModel: fileTransferModel
                settingsModel: settingsModel
                youtubePreviewEnabled: settingsModel ? settingsModel.youtube_preview_enabled : true
                youtubeInlineAck: settingsModel ? settingsModel.youtube_inline_ack : false
                // Compare id + supernode, not the display name: two nodes can
                // host rooms with the same name. `voice_in_room` is what keeps
                // the never-cleared voiceRoomId from reporting a stale match.
                voiceActiveHere: backend.voice_in_room
                                 && root.voiceRoomId !== ""
                                 && root.voiceRoomId === roomPanel.roomId
                                 && root.voiceSupernodeId === roomPanel.supernodeId
                onLeaveRoom: {
                    root.closeAllVideoPopouts()
                    root.expandedVideoPeers = []
                    backend.leaveRoom()
                    navIndex = 0
                }
                onJoinVoiceRequested: {
                    // Same path as a sidebar double-click.
                    backend.joinRoomWithVoice(roomPanel.supernodeId, roomPanel.roomId)
                    root.voiceRoomName = roomPanel.roomName
                    root.voiceSupernodeId = roomPanel.supernodeId
                    root.voiceRoomId = roomPanel.roomId
                }
                onOpenAttachment: (path) => root.showFilePreview(path)
                onMembersRequested: root.revealRoomMembers(roomPanel.supernodeId, roomPanel.roomId)
            }

            // Full-size local media / document preview (images, video, PDF, …).
            // Loaded lazily so non-webengine builds still parse MainWindow.qml;
            // showFilePreview() falls back to the system app when unavailable.
            Loader {
                id: filePreviewLoader
                anchors.fill: parent
                visible: false
                z: 50
                active: false
                onLoaded: {
                    if (item && item.closeRequested) {
                        item.closeRequested.connect(function() {
                            filePreviewLoader.visible = false
                            if (filePreviewLoader.item)
                                filePreviewLoader.item.filePath = ""
                        })
                    }
                }
            }

            SettingsPage {
                id: settingsPage
                onBackupsRequested: { settingsModel.save(); backupWizard.open() }
                anchors.fill: parent
                visible: navIndex === 2
                settings: settingsModel
                currentTab: settingsTab
                // Named differently from the id so the binding cannot resolve
                // to the page's own property instead of the model.
                supernodeModel: nodeListModel
                supernodeStats: root.nodeConnectionStats
                // React to PTT setting changes at runtime
                Connections {
                    target: settingsModel
                    function onPush_to_talkChanged() {
                        if (settingsModel.push_to_talk) {
                            backend.enablePtt(settingsModel.ptt_key)
                        } else {
                            backend.disablePtt()
                        }
                    }
                    function onPtt_keyChanged() {
                        if (settingsModel.push_to_talk) {
                            backend.enablePtt(settingsModel.ptt_key)
                        }
                    }
                    function onVoice_activationChanged() {
                        backend.setVoiceActivation(settingsModel.voice_activation)
                    }
                }
            }

            // ── Portal / Browser panel ────────────────────────────────────────
            // Occupies the full right content area (navIndex === 3).
            // Loaded lazily so QtWebEngine is not required at parse time.
            // The panel loads when a doubleslash:// portal is active:
            //   portalActive = true (locked to the secure scheme over the QUIC relay).
            Item {
                id: browserPanel
                anchors.fill: parent
                visible: navIndex === 3

                property bool nodeMode: false
                property bool portalActive: false
                // URL buffered while the Loader is still instantiating.
                property string pendingUrl: ""

                function navigateTo(url) {
                    console.log("[portal] browserPanel.navigateTo url=" + url + " loaderItem=" + _bpLoader.item)
                    if (_bpLoader.item) {
                        _bpLoader.item.navigateTo(url)
                    } else {
                        pendingUrl = url
                    }
                }
                onNodeModeChanged: {
                    if (_bpLoader.item) _bpLoader.item.nodeMode = nodeMode
                }

                Loader {
                    id: _bpLoader
                    anchors.fill: parent
                    source: browserPanel.portalActive
                        ? Qt.resolvedUrl("BrowserPanel.qml")
                        : ""
                    onItemChanged: {
                        if (item) {
                            console.log("[portal] Loader item ready, flushing pending=" + browserPanel.pendingUrl)
                            item.nodeMode = browserPanel.nodeMode
                            if (browserPanel.pendingUrl !== "") {
                                item.navigateTo(browserPanel.pendingUrl)
                                browserPanel.pendingUrl = ""
                            }
                        }
                    }
                }
            }
        }  // end contentArea

    }

    // ── System tray icon (port of client_desktop/taskbar_badge.py setup_tray) ─
    Platform.SystemTrayIcon {
        id: trayIcon
        visible: true
        icon.source: "qrc:/assets/doubleslash.ico"
        tooltip: backend.session_banner.length > 0 ? backend.session_banner : "DoubleSlash"

        menu: Platform.Menu {
            Platform.MenuItem {
                text: qsTr("Show DoubleSlash")
                onTriggered: root.showFromTray()
            }
            Platform.MenuItem {
                text: qsTr("Mute microphone")
                checkable: true
                onTriggered: backend.setMuted(checked)
            }
            Platform.MenuSeparator { }
            Platform.MenuItem {
                text: qsTr("Quit")
                onTriggered: {
                    geometrySaveTimer.stop()
                    root.persistWindowGeometry()
                    Qt.quit()
                }
            }
        }

        onActivated: function(reason) {
            // On Windows, single-click = Trigger; double-click = DoubleClick.
            if (reason === Platform.SystemTrayIcon.Trigger
                || reason === Platform.SystemTrayIcon.DoubleClick) {
                if (root.visible) {
                    root.raise()
                    root.requestActivate()
                } else {
                    root.showFromTray()
                }
            }
        }

        // Windows delivers a balloon click here, not as a tray-icon activation.
        // The hidden tray HWND is what the shell activates, so the main window
        // has to be shown and pointed at the message ourselves.
        onMessageClicked: root.openTrayNotice()
    }

    // The balloon a click will open. Only a room message navigates; the next
    // balloon replaces it, so a click cannot open a room the user was not shown.
    property var trayNotice: null

    function showTrayNotice(title, text, icon, msec, notice) {
        root.trayNotice = notice || null
        if (trayIcon.available)
            trayIcon.showMessage(title, text, icon, msec)
    }

    function openTrayNotice() {
        var notice = root.trayNotice
        root.trayNotice = null
        if (!root.visible || root.visibility === Window.Minimized
                || root.visibility === Window.Hidden)
            root.showFromTray()
        else {
            root.raise()
            root.requestActivate()
        }
        if (!notice || notice.kind !== "room" || !notice.roomId)
            return
        var nodeId = notice.supernodeId || ""
        var name = root.roomNameForAlert(notice.roomId)
        if (!nodeId) {
            var host = root.roomHostForAlert(notice.roomId)
            if (host) {
                nodeId = host.nodeId
                if (!name)
                    name = host.name
            }
        }
        if (!name)
            name = notice.roomId
        if (!nodeId)
            return
        roomPanel.armMessageReveal(notice.messageId || "")
        root.sidebarTab = 1
        root.openRoomFromTree(nodeId, notice.roomId, name)
        roomPanel.revealArmedMessage()
    }

    // Guard to suppress geometry saves during initial restore.
    property bool _restoringGeometry: false
    property bool _windowWasMaximized: false

    function showFromTray() {
        if (root._windowWasMaximized)
            root.showMaximized()
        else
            root.showNormal()
        root.raise()
        root.requestActivate()
    }

    function persistWindowGeometry() {
        if (root._restoringGeometry)
            return
        if (root.visibility === Window.Windowed) {
            settingsModel.window_x = root.x
            settingsModel.window_y = root.y
            settingsModel.window_width = root.width
            settingsModel.window_height = root.height
            settingsModel.window_position_saved = true
        }
        settingsModel.window_maximized = root._windowWasMaximized
        settingsModel.save()
    }

    // Debounce saves while the user moves or resizes the normal window.
    Timer {
        id: geometrySaveTimer
        interval: 600
        onTriggered: root.persistWindowGeometry()
    }
    onXChanged:      if (!root._restoringGeometry && root.visibility === Window.Windowed) geometrySaveTimer.restart()
    onYChanged:      if (!root._restoringGeometry && root.visibility === Window.Windowed) geometrySaveTimer.restart()
    onWidthChanged:  if (!root._restoringGeometry) geometrySaveTimer.restart()
    onHeightChanged: if (!root._restoringGeometry) geometrySaveTimer.restart()

    // True once we've explained (via a tray balloon) that the window was hidden
    // to the tray rather than closed — shown only on the first hide per session
    // so users aren't surprised by a process with no visible window.
    property bool _trayHintShown: false

    // Hide the window into the system tray and, the first time, tell the user
    // the app is still running and how to get it back.
    function hideToTray() {
        root.hide()
        if (!root._trayHintShown && trayIcon.available) {
            root.showTrayNotice(
                qsTr("DoubleSlash is still running"),
                qsTr("The window was minimized to the tray. Click the tray icon to restore it, or use Quit to exit."),
                Platform.SystemTrayIcon.Information, 5000, null)
            root._trayHintShown = true
        }
    }

    // Closing the window quits the application — unless "Minimize to tray" is
    // enabled and a tray icon is available, in which case the window is hidden
    // into the tray and DoubleSlash keeps running in the background. (Without the
    // setting, hiding on close surprised users who had no visible indication
    // the process was still alive.) The tray icon's Quit / Show items always
    // provide explicit control.
    onClosing: function(close) {
        geometrySaveTimer.stop()
        root.persistWindowGeometry()
        if (settingsModel.minimize_to_tray && trayIcon.available) {
            close.accepted = false
            hideToTray()
        } else {
            // Close popouts before quitting. They are separate top-level
            // windows, so leaving them open would both keep the process alive
            // and leave their HWNDs in the chrome tracking set during teardown.
            root.closeAllVideoPopouts()
            Qt.quit()
        }
    }

    // Minimizing the window also tucks it into the tray when the setting is on.
    onVisibilityChanged: function(visibility) {
        if (!root._restoringGeometry) {
            if (visibility === Window.Maximized)
                root._windowWasMaximized = true
            else if (visibility === Window.Windowed)
                root._windowWasMaximized = false
            if (visibility === Window.Maximized || visibility === Window.Windowed)
                geometrySaveTimer.restart()
        }
        if (visibility === Window.Minimized
                && settingsModel.minimize_to_tray
                && trayIcon.available) {
            hideToTray()
        }
    }

    // ── Global keyboard shortcuts ─────────────────────────────────────────
    Shortcut {
        sequence: "Ctrl+Q"
        context: Qt.ApplicationShortcut
        onActivated: Qt.quit()
    }
    Shortcut {
        sequence: "Ctrl+W"
        context: Qt.ApplicationShortcut
        onActivated: {
            if (trayIcon.available) root.hide()
            else Qt.quit()
        }
    }
    Shortcut {
        sequence: "Ctrl+,"
        context: Qt.ApplicationShortcut
        onActivated: navIndex = 2
    }
    Shortcut {
        sequence: "Ctrl+K"
        context: Qt.ApplicationShortcut
        onActivated: {
            navIndex = 0
            inviteField.forceActiveFocus()
            inviteField.selectAll()
        }
    }
}
