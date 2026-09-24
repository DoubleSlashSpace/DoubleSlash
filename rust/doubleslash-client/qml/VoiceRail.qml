// VoiceRail.qml — The right-hand column: who is here, and the voice controls.
//
// One list for a room, not two. While a room is open it lists everyone in it —
// voice members first, then text-only members, dimmed — so who can hear you
// and who is only reading are answered in one place. Outside a room it lists
// the live voice session (a direct call, or a room browsed away from).
//
// Every per-person action starts from that person's row: watching their video,
// muting them for yourself, messaging a trusted peer or inviting a member who
// is not one yet.
//
// Width animates between 0 (hidden) and its open width so the centre panel
// expands and contracts smoothly.

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root
    color: Theme.bg2
    clip: true

    // ── Public API ────────────────────────────────────────────────────────

    /// The RoomModel (or a synthetic 2-entry ListModel for direct calls).
    /// Peer ids currently sending video, as a set (`{peerId: true}`).
    ///
    /// Needed because `videoActive` is a role on `RoomModel` only:
    /// `directCallModel` is a plain ListModel without the field, so in a
    /// direct call `model.videoActive` is always undefined and every
    /// video-gated control stayed greyed out no matter what the peer sent.
    /// Owned by MainWindow and passed down whole.
    property var videoActivePeers: ({})

    /// Whether `peerId` is sending video, from either source.
    function peerHasVideo(peerId) {
        return root.videoActivePeers[peerId] === true
    }

    /// The live voice session's participants: `RoomModel` for room voice, a
    /// synthetic 2-entry ListModel for a direct call.
    property var participantModel: null

    /// True while a room is open: the rail then lists `memberModel` (the whole
    /// room, each row marked in or out of voice) instead of the voice session.
    property bool roomView: false
    /// Every member of the open room, with `inVoice`, `trusted` and
    /// `listPeerId` roles.
    property var memberModel: null
    /// The open room, for trust invites sent from its list.
    property string roomId: ""
    /// Voice is live for the open room itself, so its voice members are the
    /// voice session and their audio controls apply.
    property bool voiceHere: false
    /// Any voice session is live. The controls bar exists only then.
    property bool voiceActive: false

    /// Peer ids whose video is on screen here — expanded or popped out.
    property var watchedPeers: []
    /// Our trust invites by member id: "pending" | "sent".
    property var inviteStates: ({})
    /// Last trust-invite failure to explain, or "".
    property string inviteNotice: ""

    /// The model the list shows.
    readonly property var listModel: root.roomView ? root.memberModel : root.participantModel

    /// Whether a member's voice is part of the session our controls drive —
    /// everyone outside a room view, and in-voice members of the room we are in
    /// voice for.
    function inSession(inVoice) {
        return !root.roomView || (root.voiceHere && inVoice)
    }

    function isWatching(pid) {
        return root.watchedPeers.indexOf(pid) !== -1
    }

    /// Watch a peer in the centre region, or stop watching wherever they are.
    function toggleWatch(pid) {
        if (root.isWatching(pid))
            root.stopWatchingRequested(pid)
        else
            root.expandVideoRequested(pid)
    }

    /// Display name shown in the header (room name or remote peer handle).
    property string contextName: ""

    /// Hosting supernode identity when `inRoom` (Ed25519 pub / peer id).
    property string supernodeId: ""

    /// Trusted-peer handle for the hosting supernode, when known.
    property string supernodeHandle: ""

    // Header avatar is room-only (supernode host). Direct P2P peers already
    // appear in the member list below — same voice rail, no duplicate row.
    readonly property string headerAvatarPeerId: root.inRoom ? root.supernodeId : ""
    readonly property string headerAvatarHandle: root.inRoom ? root.supernodeHandle : ""

    /// Call/room state passed from bridge: "idle" | "connecting" | "in_call"
    property string callState: "idle"

    /// True when in an SFU room session (controls Leave vs End label).
    property bool inRoom: false

    /// Whether the local mic is muted.
    property bool muted: false

    /// Whether we are currently sharing video.
    property bool videoOn: false
    /// True while audio is actually going out alongside the video.
    ///
    /// Reflects what the backend achieved, not what was asked for: choosing an
    /// audio option is not a guarantee that a loopback endpoint opened. The
    /// badge on the share button is the only place a user can see the
    /// difference between "sharing silently on purpose" and "audio failed".
    property bool shareAudioOn: false

    /// Capture sources the share menu offers, as `[{ id, name }]`, with the
    /// empty-id "Default camera" entry first.
    ///
    /// Supplied by the host rather than read from the backend here: `AppBridge`
    /// declares its id in MainWindow, and a QML id is scoped to the file that
    /// declares it, so it is simply not in scope in this one.
    property var videoSources: []

    /// Main capture source id — `video_input_device`. Empty means the default
    /// camera.
    property string videoSourceId: ""

    /// Picture-in-picture layout, verbatim `video_overlays_json`.
    property string videoOverlaysJson: "[]"

    /// Why video cannot be shared, or "" when it can.
    ///
    /// Non-empty greys out the share affordances and is shown verbatim, so
    /// starting a share can no longer fail with no explanation. An encoder-level
    /// problem blocks the button outright; a missing camera only blocks the
    /// commit inside the menu, because opening the menu re-enumerates and is how
    /// a camera plugged in after launch gets noticed.
    property string videoUnavailableReason: ""
    /// True when the failure is the platform/encoder rather than the hardware.
    property bool videoEncoderMissing: false

    /// Audio mode the share menu pre-selects: "auto" | "system" | "off".
    property string contentAudioMode: "auto"

    /// Elapsed call seconds (driven by bridge.call_duration_secs).
    property int durationSecs: 0

    /// Connection mode for the header pill.
    property string connectionMode: "offline"

    signal endCallRequested()
    signal muteToggled(bool muted)
    /// Camera button pressed; `on` is the requested new state.
    /// Start sharing, with `audioMode` one of "auto", "system", "off".
    ///
    /// Video and its audio start together: the audio is stamped against the
    /// video session's clock, so it cannot meaningfully exist without it.
    signal shareRequested(string audioMode)
    /// Stop sharing video and any audio that went with it.
    signal stopShareRequested()

    /// The share menu is opening — re-enumerate capture sources. Windows and
    /// cameras come and go between one share and the next, so the list is a
    /// snapshot rather than something held across sessions.
    signal shareOptionsOpened()
    /// Main capture source picked in the share menu.
    signal videoSourceSelected(string sourceId)
    /// Overlay layout edited in the share menu; carries the replacement
    /// `video_overlays_json`.
    signal videoOverlaysEdited(string overlaysJson)

    /// A peer's video should be shown in the centre expand region.
    signal expandVideoRequested(string peerId)
    /// A peer's video should be shown in its own detached window.
    signal popoutVideoRequested(string peerId)
    /// Take a peer's video off screen — collapse the tile or close its window.
    signal stopWatchingRequested(string peerId)
    /// Open the 1:1 chat with a trusted member, by Peers-list id.
    signal messagePeerRequested(string listPeerId, string name)
    /// Offer a room member we do not trust to become trusted peers.
    signal trustInviteRequested(string peerId)

    /// Peers currently expanded in the centre region, so the menu can offer
    /// "Collapse" instead of "Expand" for those already showing.
    property var expandedPeers: []

    function isExpanded(pid) {
        return root.expandedPeers.indexOf(pid) !== -1
    }

    // ── Helpers ───────────────────────────────────────────────────────────
    function pad(n) { return n < 10 ? "0" + n : n.toString() }

    // ── Share menu model ──────────────────────────────────────────────────
    //
    // Settings › Video owns the full editor, warnings and preview included.
    // What is repeated here is only what can still be decided in the moment
    // before sharing starts — which source, which insets, which audio — because
    // that is the one moment the answer is actually in question, and sending a
    // whole screen when a webcam was meant is not something a user can take
    // back afterwards.

    readonly property var shareCorners: ["top-left", "top-right", "bottom-left", "bottom-right"]
    readonly property var shareCornerLabels: [
        qsTr("Top left"), qsTr("Top right"), qsTr("Bottom left"), qsTr("Bottom right")
    ]
    readonly property var shareOverlaySizes: [10, 15, 20, 25, 30, 40, 50]
    /// Kept in step with `composite::MAX_OVERLAYS`.
    readonly property int shareMaxOverlays: 3

    /// Sources offerable as an inset: every real device, minus the "Default
    /// camera" entry, which names no device of its own.
    readonly property var shareOverlaySources:
        root.videoSources.filter(function (s) { return s && s.id !== "" })

    readonly property var shareOverlays: root.parseShareOverlays(root.videoOverlaysJson)

    /// Tolerant by design: the blob is user-editable, and a layout that failed
    /// to parse must mean "no overlays" rather than a share menu that cannot
    /// open.
    function parseShareOverlays(json) {
        var list
        try { list = JSON.parse(json || "[]") } catch (e) { return [] }
        return Array.isArray(list) ? list.slice(0, root.shareMaxOverlays) : []
    }

    function shareSourceIndex(id) {
        for (var i = 0; i < root.videoSources.length; i++) {
            if (root.videoSources[i].id === id)
                return i
        }
        return -1
    }

    function shareOverlaySourceIndex(id) {
        for (var i = 0; i < root.shareOverlaySources.length; i++) {
            if (root.shareOverlaySources[i].id === id)
                return i
        }
        return -1
    }

    /// First source not already spoken for. A device cannot be opened twice, so
    /// offering one that is already the main source (or another inset) would
    /// only add a row that fails to capture.
    function firstFreeOverlaySource() {
        var used = [root.videoSourceId]
        var list = root.shareOverlays
        for (var i = 0; i < list.length; i++)
            used.push(list[i].id)
        for (var j = 0; j < root.shareOverlaySources.length; j++) {
            var id = root.shareOverlaySources[j].id
            if (used.indexOf(id) < 0)
                return id
        }
        return ""
    }

    function editShareOverlay(index, key, value) {
        var list = root.parseShareOverlays(root.videoOverlaysJson)
        if (index < 0 || index >= list.length)
            return
        list[index][key] = value
        root.videoOverlaysEdited(JSON.stringify(list))
    }

    function removeShareOverlay(index) {
        var list = root.parseShareOverlays(root.videoOverlaysJson)
        if (index < 0 || index >= list.length)
            return
        list.splice(index, 1)
        root.videoOverlaysEdited(JSON.stringify(list))
    }

    function addShareOverlay() {
        var id = root.firstFreeOverlaySource()
        var list = root.parseShareOverlays(root.videoOverlaysJson)
        if (id === "" || list.length >= root.shareMaxOverlays)
            return
        list.push({ id: id, corner: "bottom-right", size: 25 })
        root.videoOverlaysEdited(JSON.stringify(list))
    }

    /// What "audio from the shared source" actually resolves to for the source
    /// currently selected — the one part of that option a user cannot infer.
    function shareAudioHint() {
        if (root.videoSourceId.indexOf("window:") === 0)
            return qsTr("Only that application's audio is sent.")
        if (root.videoSourceId.indexOf("monitor:") === 0)
            return qsTr("Everything this computer plays is sent.")
        return qsTr("A camera carries no audio of its own — your microphone already carries you.")
    }

    // ── Left border separator ─────────────────────────────────────────────
    Rectangle {
        anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
        width: 1
        color: Theme.divider
    }

    // ── Shared peer context menu ──────────────────────────────────────────
    //
    // One instance for the whole rail, retargeted before each popup(). Putting
    // a Menu inside the Repeater delegate would create one per participant.
    Menu {
        id: peerMenu

        property string targetPeerId: ""
        property string targetName: ""
        property bool targetLocalMuted: false
        property int targetVolume: 100
        property bool targetVideoActive: false
        property bool targetIsSelf: false
        /// Their voice is part of our session, so hearing and watching them
        /// are ours to control. False for a text-only member, or a room we
        /// are only browsing.
        property bool targetInSession: true
        property bool targetTrusted: false
        property string targetListPeerId: ""

        function openFor(m) {
            peerMenu.targetPeerId = m.peerId
            peerMenu.targetName = m.name
            peerMenu.targetLocalMuted = m.localMuted
            peerMenu.targetVolume = m.volume
            peerMenu.targetVideoActive = m.videoActive
            peerMenu.targetIsSelf = m.isSelf
            peerMenu.targetInSession = m.inSession
            peerMenu.targetTrusted = m.trusted
            peerMenu.targetListPeerId = m.listPeerId
            peerMenu.popup()
        }

        readonly property string targetInviteState:
            root.inviteStates[peerMenu.targetPeerId] || ""

        MenuItem {
            // Watching is how video starts arriving at all: nothing is received
            // or decoded for a peer until it is chosen. Stopping stays possible
            // after their camera goes off, or the tile could never be closed.
            enabled: peerMenu.targetInSession && !peerMenu.targetIsSelf
                && (peerMenu.targetVideoActive || root.isWatching(peerMenu.targetPeerId))
            text: root.isWatching(peerMenu.targetPeerId)
                ? qsTr("Stop watching")
                : (peerMenu.targetVideoActive ? qsTr("Watch video") : qsTr("Camera is off"))
            onTriggered: root.toggleWatch(peerMenu.targetPeerId)
        }

        MenuItem {
            enabled: peerMenu.targetInSession && !peerMenu.targetIsSelf
                && peerMenu.targetVideoActive
            text: qsTr("Pop out video")
            onTriggered: root.popoutVideoRequested(peerMenu.targetPeerId)
        }

        MenuSeparator {}

        MenuItem {
            // Muting yourself locally would be meaningless — you don't hear
            // your own playback — so the entry is disabled rather than absent,
            // keeping the menu's shape stable between peers.
            enabled: !peerMenu.targetIsSelf && peerMenu.targetInSession
            checkable: true
            checked: peerMenu.targetLocalMuted
            text: qsTr("Mute for me")
            onTriggered: {
                var next = !peerMenu.targetLocalMuted
                peerMenu.targetLocalMuted = next
                backend.setPeerAudioPref(peerMenu.targetPeerId, next, peerMenu.targetVolume)
                if (root.participantModel && root.participantModel.setLocalAudio)
                    root.participantModel.setLocalAudio(peerMenu.targetPeerId, next, peerMenu.targetVolume)
            }
        }

        MenuItem {
            enabled: !peerMenu.targetIsSelf && peerMenu.targetInSession
            text: qsTr("Volume…")
            onTriggered: volumePopup.openFor(
                peerMenu.targetPeerId, peerMenu.targetName, peerMenu.targetVolume)
        }

        MenuSeparator {}

        // A trusted peer can be messaged; a room member who is not one can be
        // offered that. Only one of the two is ever shown, so the menu says
        // plainly which relationship you have with this person.
        MenuItem {
            visible: peerMenu.targetTrusted && !peerMenu.targetIsSelf
            height: visible ? implicitHeight : 0
            text: qsTr("Message")
            onTriggered: root.messagePeerRequested(
                peerMenu.targetListPeerId, peerMenu.targetName)
        }

        MenuItem {
            visible: !peerMenu.targetTrusted && !peerMenu.targetIsSelf && root.roomId !== ""
            height: visible ? implicitHeight : 0
            enabled: peerMenu.targetInviteState === ""
            text: peerMenu.targetInviteState === "sent" ? qsTr("Invite sent")
                : peerMenu.targetInviteState === "pending" ? qsTr("Sending invite…")
                : qsTr("Invite to trusted peers")
            onTriggered: root.trustInviteRequested(peerMenu.targetPeerId)
        }

        MenuItem {
            text: qsTr("Copy Peer ID")
            onTriggered: backend.copyToClipboard(peerMenu.targetPeerId)
        }
    }

    PeerVolumePopup {
        id: volumePopup
        x: Math.round((root.width - width) / 2)
        y: Math.round((root.height - height) / 2)
        onVolumeChanged: function(pid, pct) {
            // Unmute implicitly when the listener raises the volume — leaving
            // someone muted while their slider reads 80% would be baffling.
            var muted = pct === 0
            backend.setPeerAudioPref(pid, muted, pct)
            if (root.participantModel && root.participantModel.setLocalAudio)
                root.participantModel.setLocalAudio(pid, muted, pct)
            if (peerMenu.targetPeerId === pid) {
                peerMenu.targetVolume = pct
                peerMenu.targetLocalMuted = muted
            }
        }
    }

    ColumnLayout {
        anchors { fill: parent; leftMargin: 1 }
        spacing: 0

        // ── Header ────────────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 52
            color: Theme.bg3

            RowLayout {
                anchors {
                    fill: parent
                    leftMargin: Theme.spacingMd
                    rightMargin: Theme.spacingSm
                    topMargin: Theme.spacingXs
                    bottomMargin: Theme.spacingXs
                }
                spacing: Theme.spacingSm

                Item {
                    visible: root.headerAvatarPeerId !== "" && !root.roomView
                    Layout.preferredWidth: 36
                    Layout.preferredHeight: 36
                    Layout.alignment: Qt.AlignVCenter

                    Avatar {
                        anchors.centerIn: parent
                        peerId: root.headerAvatarPeerId
                        size: 28
                        showRing: true
                    }

                    Rectangle {
                        visible: root.headerAvatarHandle !== ""
                        anchors {
                            horizontalCenter: parent.horizontalCenter
                            bottom: parent.bottom
                        }
                        implicitWidth: Math.min(handleLabel.implicitWidth + 6, 52)
                        width: implicitWidth
                        height: 12
                        radius: height / 2
                        color: Theme.accent

                        Text {
                            id: handleLabel
                            anchors.centerIn: parent
                            width: parent.width - 4
                            text: root.headerAvatarHandle
                            color: Theme.textInv
                            font.pixelSize: Theme.fontSizeMicro
                            font.bold: true
                            elide: Text.ElideRight
                            horizontalAlignment: Text.AlignHCenter
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    // Room members, or the voice session's room/peer name.
                    Text {
                        text: root.roomView
                            ? qsTr("Members") + " · " + memberList.count
                            : (root.contextName || (root.inRoom ? "Voice Room" : "Call"))
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeBody
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    // In a room you are not in voice for, say so instead of
                    // showing a connection pill for some other session.
                    Text {
                        visible: root.roomView && !root.voiceHere
                        text: root.voiceActive
                            ? qsTr("Your voice is in ") + (root.contextName || qsTr("another call"))
                            : qsTr("You are not in voice")
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeMicro
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    // Connection mode pill
                    Rectangle {
                        visible: root.voiceActive && (!root.roomView || root.voiceHere)
                        width: modePillText.implicitWidth + Theme.spacingSm
                        height: Theme.fontSizeCaption + Theme.spacingXs
                        radius: Theme.radiusSm
                        color: Theme.semanticTint(
                            Theme.connectionModeColor(root.connectionMode),
                            0.18
                        )

                        Text {
                            id: modePillText
                            anchors.centerIn: parent
                            text: root.callState === "connecting"
                                ? "Connecting..."
                                : Theme.connectionModeLabel(root.connectionMode)
                            color: Theme.connectionModeColor(root.connectionMode)
                            font.pixelSize: Theme.fontSizeCaption
                            font.bold: true
                        }
                    }
                }
            }
        }

        // ── Member list ───────────────────────────────────────────────────
        //
        // The list sits inside a plain Item the layout sizes, and fills it by
        // anchors. A view placed directly in the ColumnLayout reports an
        // implicit size the layout then resizes it by; with the rail animating
        // open and a fractional scale factor the two never settled, and the
        // polish pass spun forever — the window froze the moment a direct call
        // started. The connecting placeholder is a sibling for the same reason.
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ListView {
                id: memberList
                anchors.fill: parent
                anchors.topMargin: Theme.spacingXs
                clip: true
                model: root.listModel
                boundsBehavior: Flickable.StopAtBounds

                // In voice first, then text-only: the roster arrives in that
                // order, so each group is one section. Outside a room view
                // everyone listed is in voice and a heading would be noise.
                section.property: root.roomView ? "inVoice" : ""
                section.criteria: ViewSection.FullString
                section.delegate: Item {
                    required property string section
                    width: memberList.width
                    height: 22

                    Text {
                        anchors {
                            verticalCenter: parent.verticalCenter
                            left: parent.left
                            leftMargin: Theme.spacingMd
                        }
                        text: parent.section === "true" ? qsTr("In voice") : qsTr("Text only")
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeMicro
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 1.0
                        font.bold: true
                    }
                }

                delegate: MemberRow {
                    id: row
                    width: ListView.view ? ListView.view.width : 0

                    // `directCallModel` has no inVoice/trusted roles; both are
                    // undefined there, and everyone in a call is in voice.
                    readonly property bool rowInVoice: model.inVoice !== false
                    readonly property bool rowInSession: root.inSession(rowInVoice)

                    peerId: model.peerId || ""
                    displayName: model.handle || ""
                    isSelf: model.isSelf === true
                    inVoice: rowInVoice
                    isMuted: model.muted === true
                    // Levels only mean anything for the session we are in; a
                    // room we are browsing would otherwise borrow our mic.
                    audioLevel: !rowInSession ? 0.0
                        : (model.isSelf ? backend.mic_level : (model.audioLevel || 0.0))
                    videoActive: rowInSession
                        && (model.videoActive === true || root.peerHasVideo(model.peerId))
                    watching: root.isWatching(model.peerId)
                    locallyMuted: model.localMuted === true
                    trusted: model.trusted === true
                    inviteState: root.inviteStates[model.peerId] || ""

                    onMenuRequested: peerMenu.openFor({
                        peerId: model.peerId,
                        name: model.handle || model.peerId || "",
                        localMuted: model.localMuted === true,
                        volume: model.localVolume === undefined ? 100 : model.localVolume,
                        videoActive: row.videoActive,
                        isSelf: model.isSelf === true,
                        inSession: row.rowInSession,
                        trusted: model.trusted === true,
                        listPeerId: model.listPeerId || ""
                    })
                    onVideoClicked: {
                        if (!model.isSelf)
                            root.toggleWatch(model.peerId)
                    }
                }
            }

            // Connecting placeholder, until the first participant appears.
            Column {
                anchors.centerIn: parent
                spacing: 8
                visible: memberList.count === 0 && root.callState === "connecting"

                BusyIndicator {
                    anchors.horizontalCenter: parent.horizontalCenter
                    running: parent.visible
                    width: 32; height: 32
                }

                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: "Connecting..."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                }
            }
        }

        // ── Trust-invite notice ───────────────────────────────────────────
        //
        // Why an invite did not go, beside the list it was sent from. The
        // session banner is for connection state and would be overwritten by
        // the next connection event before anyone read it.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: inviteNoticeText.implicitHeight + Theme.spacingSm * 2
            visible: root.inviteNotice !== ""
            color: Theme.bg3

            Text {
                id: inviteNoticeText
                anchors { fill: parent; margins: Theme.spacingSm }
                text: root.inviteNotice
                color: Theme.warn
                font.pixelSize: Theme.fontSizeMicro
                wrapMode: Text.WordWrap
            }
        }

        // ── Duration counter ──────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            height: 24
            color: Theme.bg3
            visible: root.voiceActive && (root.callState === "in_call" || root.inRoom)

            Text {
                anchors.centerIn: parent
                text: {
                    var s = root.durationSecs
                    var h = Math.floor(s / 3600)
                    var m = Math.floor((s % 3600) / 60)
                    var sec = s % 60
                    if (h > 0) {
                        return root.pad(h) + ":" + root.pad(m) + ":" + root.pad(sec)
                    }
                    return root.pad(m) + ":" + root.pad(sec)
                }
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                font.family: "monospace"
            }

            function pad(n) { return n < 10 ? "0" + n : n.toString() }
        }

        // ── Controls bar ─────────────────────────────────────────────────
        //
        // Drives the live voice session wherever it is, so it shows only while
        // there is one — a room opened just to read has nothing to mute.
        Rectangle {
            Layout.fillWidth: true
            height: 52
            color: Theme.bg3
            visible: root.voiceActive

            // Top divider
            Rectangle {
                anchors { top: parent.top; left: parent.left; right: parent.right }
                height: 1
                color: Theme.divider
            }

            RowLayout {
                anchors {
                    fill: parent
                    leftMargin: Theme.spacingMd
                    rightMargin: Theme.spacingMd
                }
                spacing: Theme.spacingSm

                // Mute toggle
                Rectangle {
                    width: 36; height: 36; radius: Theme.radiusPill
                    color: root.muted ? Theme.danger : Theme.bg2

                    Behavior on color { ColorAnimation { duration: Theme.animFast } }

                    Image {
                        anchors.centerIn: parent
                        source: root.muted ? "qrc:/qt/qml/DoubleSlash/Client/icons/mic-off.svg" : "qrc:/qt/qml/DoubleSlash/Client/icons/mic.svg"
                        sourceSize.width: 18
                        sourceSize.height: 18
                        width: 18
                        height: 18
                        fillMode: Image.PreserveAspectFit
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            root.muted = !root.muted
                            root.muteToggled(root.muted)
                        }
                    }

                    ToolTip.text: root.muted ? "Unmute microphone" : "Mute microphone"
                    ToolTip.visible: muteHover.hovered
                    HoverHandler { id: muteHover }
                }

                // One control for sharing, not two.
                //
                // Video and the audio that belongs with it are a single act
                // from the user's point of view — "share this" — so the audio
                // is an *option of sharing*, chosen when sharing starts, rather
                // than a second toggle to remember afterwards. It also removes
                // the state that made no sense: an audio button that did
                // nothing until a camera was already running.
                Rectangle {
                    id: shareButton
                    width: 36; height: 36; radius: Theme.radiusPill
                    color: root.videoOn ? Theme.accent : Theme.bg2
                    // Greyed out when this build cannot encode at all. A missing
                    // camera does NOT disable it — the menu re-enumerates on
                    // open, so disabling here would strand anyone who plugs a
                    // camera in after launch.
                    opacity: root.videoEncoderMissing ? 0.4 : 1.0

                    Behavior on color { ColorAnimation { duration: Theme.animFast } }

                    Image {
                        anchors.centerIn: parent
                        source: root.videoOn
                            ? "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg"
                            : "qrc:/qt/qml/DoubleSlash/Client/icons/video-off.svg"
                        sourceSize.width: 18
                        sourceSize.height: 18
                        width: 18
                        height: 18
                        fillMode: Image.PreserveAspectFit
                    }

                    // Audio-included badge.
                    //
                    // Sharing is otherwise indistinguishable whether or not the
                    // audio half started, and it can fail on its own (no
                    // loopback endpoint, device in use) while the video keeps
                    // running. Without this the user's first clue is a peer
                    // saying they hear nothing.
                    Rectangle {
                        visible: root.videoOn && root.shareAudioOn
                        width: 12; height: 12; radius: 6
                        anchors { right: parent.right; bottom: parent.bottom }
                        color: Theme.bg2
                        border.color: Theme.accent
                        border.width: 1

                        Image {
                            anchors.centerIn: parent
                            source: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                            sourceSize.width: 8; sourceSize.height: 8
                            width: 8; height: 8
                            fillMode: Image.PreserveAspectFit
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        // Stopping is unambiguous, so it needs no menu. Starting
                        // asks what to do about audio, because that choice is
                        // only meaningful at the moment sharing begins.
                        onClicked: {
                            if (root.videoOn)
                                root.stopShareRequested()
                            else if (!root.videoEncoderMissing)
                                sharePopup.open()
                        }
                    }

                    ToolTip.text: root.videoEncoderMissing
                        ? root.videoUnavailableReason
                        : (!root.videoOn
                            ? qsTr("Share video")
                            : (root.shareAudioOn
                                ? qsTr("Stop sharing (video and audio)")
                                : qsTr("Stop sharing (video only)")))
                    ToolTip.visible: shareHover.hovered && !sharePopup.opened
                    HoverHandler { id: shareHover }

                    Popup {
                        id: sharePopup
                        // Right-aligned to the button. The menu is far wider
                        // than the rail it hangs off, so growing rightward
                        // would put most of it past the window edge.
                        x: shareButton.width - width
                        y: -implicitHeight - Theme.spacingXs
                        width: 320
                        padding: Theme.spacingSm
                        modal: false

                        /// Audio choice for the share that has not started yet.
                        ///
                        /// Held here rather than written through to settings
                        /// because nothing in this menu takes effect until the
                        /// start button is pressed — abandoning the menu must
                        /// leave the saved mode exactly as it was.
                        property string pendingAudioMode: root.contentAudioMode

                        // A snapshot, taken each time: windows open and close
                        // constantly, and a list built once at startup would
                        // offer things that are no longer there.
                        onAboutToShow: {
                            root.shareOptionsOpened()
                            pendingAudioMode = root.contentAudioMode
                        }
                        background: Rectangle {
                            color: Theme.bg2
                            border.color: Theme.border
                            radius: Theme.radiusSm
                        }

                        ColumnLayout {
                            anchors.fill: parent
                            spacing: Theme.spacingXs

                            Label {
                                text: qsTr("Share video with…")
                                color: Theme.text
                                font.pixelSize: Theme.fontSizeCaption
                                font.bold: true
                            }

                            // ── What is being shared ──────────────────────
                            Label {
                                text: qsTr("Source")
                                color: Theme.muted
                                font.pixelSize: Theme.fontSizeMicro
                            }

                            ComboBox {
                                id: shareSourceCombo
                                Layout.fillWidth: true
                                Layout.preferredHeight: 32
                                font.pixelSize: Theme.fontSizeCaption
                                model: root.videoSources.map(function (s) { return s.name || s.id })
                                currentIndex: root.shareSourceIndex(root.videoSourceId)
                                // -1 when the saved device is gone — an
                                // unplugged camera or a closed window is
                                // routine, and saying so beats silently
                                // sharing whatever happens to be first.
                                displayText: currentIndex >= 0
                                    ? currentText
                                    : qsTr("Source unavailable")
                                onActivated: {
                                    var s = root.videoSources[currentIndex]
                                    root.videoSourceSelected(s ? (s.id || "") : "")
                                    // Selecting broke the binding above by
                                    // writing currentIndex directly, and this
                                    // combo outlives the menu — without
                                    // rebinding, a source changed from Settings
                                    // afterwards would never show here.
                                    currentIndex = Qt.binding(function () {
                                        return root.shareSourceIndex(root.videoSourceId)
                                    })
                                }
                            }

                            // ── Overlays ──────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.spacingXs

                                Label {
                                    Layout.fillWidth: true
                                    text: qsTr("Overlays")
                                    color: Theme.muted
                                    font.pixelSize: Theme.fontSizeMicro
                                }
                                StyledButton {
                                    text: qsTr("Add")
                                    enabled: root.shareOverlays.length < root.shareMaxOverlays
                                        && root.firstFreeOverlaySource() !== ""
                                    onClicked: root.addShareOverlay()
                                }
                            }

                            Label {
                                visible: root.shareOverlays.length === 0
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                text: qsTr("None — the source fills the frame. Overlays are merged "
                                    + "into it before encoding, so peers still see one picture.")
                                color: Theme.muted
                                font.pixelSize: Theme.fontSizeMicro
                            }

                            Repeater {
                                model: root.shareOverlays

                                delegate: ColumnLayout {
                                    id: overlayRow
                                    required property int index
                                    required property var modelData

                                    Layout.fillWidth: true
                                    spacing: 2

                                    ComboBox {
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 30
                                        font.pixelSize: Theme.fontSizeCaption
                                        model: root.shareOverlaySources.map(
                                            function (s) { return s.name || s.id })
                                        currentIndex: root.shareOverlaySourceIndex(
                                            overlayRow.modelData.id)
                                        // Names the row that will be left out
                                        // of the picture, and why: a device
                                        // cannot be captured twice, so an
                                        // overlay that is also the main source
                                        // is dropped by the compositor.
                                        displayText: overlayRow.modelData.id === root.videoSourceId
                                            ? qsTr("Same as source — not shown")
                                            : (currentIndex >= 0
                                                ? currentText
                                                : qsTr("Source unavailable"))
                                        onActivated: {
                                            var s = root.shareOverlaySources[currentIndex]
                                            root.editShareOverlay(
                                                overlayRow.index, "id", s ? s.id : "")
                                        }
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: Theme.spacingXs

                                        ComboBox {
                                            Layout.fillWidth: true
                                            Layout.preferredHeight: 30
                                            font.pixelSize: Theme.fontSizeCaption
                                            model: root.shareCornerLabels
                                            currentIndex: Math.max(0, root.shareCorners.indexOf(
                                                overlayRow.modelData.corner))
                                            onActivated: root.editShareOverlay(
                                                overlayRow.index, "corner",
                                                root.shareCorners[currentIndex] || "bottom-right")
                                        }

                                        ComboBox {
                                            Layout.preferredWidth: 96
                                            Layout.preferredHeight: 30
                                            font.pixelSize: Theme.fontSizeCaption
                                            // Width only: the height follows
                                            // the source's own aspect ratio.
                                            model: root.shareOverlaySizes.map(
                                                function (s) { return s + "%" })
                                            currentIndex: Math.max(0, root.shareOverlaySizes.indexOf(
                                                Number(overlayRow.modelData.size) || 25))
                                            onActivated: root.editShareOverlay(
                                                overlayRow.index, "size",
                                                root.shareOverlaySizes[currentIndex] || 25)
                                        }

                                        StyledButton {
                                            text: qsTr("Remove")
                                            onClicked: root.removeShareOverlay(overlayRow.index)
                                        }
                                    }
                                }
                            }

                            // ── Audio ─────────────────────────────────────
                            //
                            // A selection, not a commit: everything in this
                            // menu stays editable until the start button below
                            // is pressed, so the audio row can be revisited the
                            // same way the source and overlays can.
                            Label {
                                Layout.topMargin: Theme.spacingXs
                                text: qsTr("Audio")
                                color: Theme.muted
                                font.pixelSize: Theme.fontSizeMicro
                            }

                            Repeater {
                                model: [
                                    { key: "auto",   label: qsTr("Audio from the shared source") },
                                    { key: "system", label: qsTr("This computer's audio") },
                                    { key: "off",    label: qsTr("No audio") }
                                ]
                                delegate: Rectangle {
                                    id: audioOption
                                    required property var modelData
                                    readonly property bool selected:
                                        modelData.key === sharePopup.pendingAudioMode

                                    Layout.fillWidth: true
                                    height: 30
                                    radius: Theme.radiusSm
                                    color: audioOption.selected ? Theme.accent
                                        : optHover.hovered ? Theme.bg3
                                        : "transparent"

                                    Behavior on color { ColorAnimation { duration: Theme.animFast } }

                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        anchors.left: parent.left
                                        anchors.leftMargin: Theme.spacingSm
                                        // The dot carries the same meaning as
                                        // the fill, for anyone the blue does
                                        // not reach.
                                        text: (audioOption.selected ? "• " : "   ")
                                            + audioOption.modelData.label
                                        color: audioOption.selected ? Theme.textInv : Theme.text
                                        font.pixelSize: Theme.fontSizeCaption
                                    }
                                    HoverHandler { id: optHover }
                                    MouseArea {
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: sharePopup.pendingAudioMode
                                            = audioOption.modelData.key
                                    }
                                }
                            }

                            Label {
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                text: root.shareAudioHint()
                                color: Theme.muted
                                font.pixelSize: Theme.fontSizeMicro
                            }

                            // ── Commit ────────────────────────────────────
                            //
                            // Bottom right, where the confirming action of a
                            // dialog is looked for, and green because it is the
                            // one control here that puts a picture on the wire.
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.topMargin: Theme.spacingXs
                                spacing: Theme.spacingXs

                                // Say why the commit is dead rather than letting
                                // it be pressed and silently do nothing.
                                Label {
                                    Layout.fillWidth: true
                                    visible: root.videoUnavailableReason !== ""
                                    wrapMode: Text.WordWrap
                                    text: root.videoUnavailableReason
                                    color: Theme.warn
                                    font.pixelSize: Theme.fontSizeMicro
                                }

                                Item {
                                    Layout.fillWidth: true
                                    visible: root.videoUnavailableReason === ""
                                }

                                StyledButton {
                                    success: true
                                    enabled: root.videoUnavailableReason === ""
                                    text: qsTr("Start sharing")
                                    onClicked: {
                                        sharePopup.close()
                                        root.shareRequested(sharePopup.pendingAudioMode)
                                    }
                                }
                            }
                        }
                    }
                }

                Item { Layout.fillWidth: true }

                // End / Leave button
                Rectangle {
                    width: 36; height: 36; radius: Theme.radiusPill
                    color: Theme.danger

                    Image {
                        anchors.centerIn: parent
                        source: "qrc:/qt/qml/DoubleSlash/Client/icons/x-circle.svg"
                        width: 18; height: 18
                        smooth: true
                        antialiasing: true
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.endCallRequested()
                    }

                    ToolTip.text: root.inRoom ? "Leave room" : "End call"
                    ToolTip.visible: endHover.hovered
                    HoverHandler { id: endHover }
                }
            }
        }
    }
}
