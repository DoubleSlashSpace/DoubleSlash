// VoiceDock.qml — The live call, at the foot of the sidebar.
//
// Whatever the sidebar is showing — Peers, Rooms or Settings — a live voice
// session stays in view here: where it is, how it is connected, who in it is
// sharing video, and the controls that drive it. The Rooms tree is where the
// people are; this is only the session.
//
// Mobile-first: the same block sits at the foot of the phone's screens, so the
// two clients keep one layout for "you are in a call".

import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root
    color: Theme.bg2
    implicitHeight: dockColumn.implicitHeight + Theme.spacingSm * 2

    // ── Public API ────────────────────────────────────────────────────────

    /// "idle" | "connecting" | "in_call" — the direct-call state.
    property string callState: "idle"
    /// An SFU room session rather than a direct call.
    property bool inRoom: false
    /// Where the session is: the room's path, or the peer in a direct call.
    property string contextName: ""
    /// Local microphone muted.
    property bool muted: false
    /// We are sharing video.
    property bool videoOn: false
    /// Audio actually going out with the video (what the backend achieved).
    property bool shareAudioOn: false
    /// Capture sources as `[{ id, name }]`, "Default camera" first.
    property var videoSources: []
    /// Main capture source id — `video_input_device`.
    property string videoSourceId: ""
    /// Picture-in-picture layout, verbatim `video_overlays_json`.
    property string videoOverlaysJson: "[]"
    /// Why video cannot be shared, or "".
    property string videoUnavailableReason: ""
    /// The failure is the platform/encoder rather than the hardware.
    property bool videoEncoderMissing: false
    /// Audio mode the share menu pre-selects: "auto" | "system" | "off".
    property string contentAudioMode: "auto"
    /// Elapsed session seconds.
    property int durationSecs: 0
    /// "direct" | "relay" | "offline" | "error".
    property string connectionMode: "offline"
    /// Names of people in the session sharing video that we are not watching.
    property var unwatchedStreamers: []
    /// Last trust-invite failure to explain, or "".
    property string inviteNotice: ""

    signal endCallRequested()
    signal muteToggled(bool muted)
    /// Start sharing, with `audioMode` one of "auto", "system", "off".
    signal shareRequested(string audioMode)
    signal stopShareRequested()
    /// The share menu is opening — re-enumerate capture sources.
    signal shareOptionsOpened()
    signal videoSourceSelected(string sourceId)
    signal videoOverlaysEdited(string overlaysJson)
    /// The session's room (or peer chat) should be brought into view.
    signal openSessionRequested()
    /// Watch everyone in the session who is sharing video.
    signal watchStreamersRequested()

    function pad(n) { return n < 10 ? "0" + n : n.toString() }

    readonly property string durationText: {
        var s = root.durationSecs
        var h = Math.floor(s / 3600)
        var m = Math.floor((s % 3600) / 60)
        var sec = s % 60
        return h > 0 ? root.pad(h) + ":" + root.pad(m) + ":" + root.pad(sec)
                     : root.pad(m) + ":" + root.pad(sec)
    }

    readonly property bool connecting: !root.inRoom && root.callState === "connecting"
    readonly property color statusColor: root.connecting ? Theme.warn
        : Theme.connectionModeColor(root.connectionMode)

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

    // Top divider
    Rectangle {
        anchors { top: parent.top; left: parent.left; right: parent.right }
        height: 1
        color: Theme.divider
    }

    ColumnLayout {
        id: dockColumn
        anchors {
            left: parent.left
            right: parent.right
            top: parent.top
            leftMargin: Theme.spacingMd
            rightMargin: Theme.spacingMd
            topMargin: Theme.spacingSm
        }
        spacing: Theme.spacingXs

        // ── Status ────────────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            // Signal bars in the connection's colour.
            Row {
                Layout.alignment: Qt.AlignVCenter
                spacing: 2
                Repeater {
                    model: [5, 8, 11]
                    delegate: Rectangle {
                        required property var modelData
                        width: 3
                        height: modelData
                        anchors.bottom: parent ? parent.bottom : undefined
                        color: root.statusColor
                    }
                }
            }

            Text {
                text: root.connecting ? qsTr("Connecting…")
                    : (root.inRoom ? qsTr("Voice connected") : qsTr("In call"))
                color: root.statusColor
                font.pixelSize: Theme.fontSizeCaption + 1
                font.bold: true
            }

            Rectangle {
                visible: !root.connecting
                implicitWidth: modeText.implicitWidth + Theme.spacingSm
                implicitHeight: modeText.implicitHeight + 2
                color: Theme.semanticTint(Theme.connectionModeColor(root.connectionMode), 0.18)
                Text {
                    id: modeText
                    anchors.centerIn: parent
                    text: Theme.connectionModeLabel(root.connectionMode)
                    color: Theme.connectionModeColor(root.connectionMode)
                    font.pixelSize: Theme.fontSizeCaption
                    font.bold: true
                }
            }

            Item { Layout.fillWidth: true }

            Text {
                visible: !root.connecting
                text: root.durationText
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                font.family: "monospace"
            }
        }

        // Where the session is. Clicking it goes there.
        Text {
            id: contextLink
            Layout.fillWidth: true
            text: root.contextName
            color: Theme.text
            font.pixelSize: Theme.fontSizeCaption + 1
            font.underline: contextHover.hovered
            elide: Text.ElideRight
            HoverHandler { id: contextHover; cursorShape: Qt.PointingHandCursor }
            TapHandler { onTapped: root.openSessionRequested() }
            ToolTip.text: root.inRoom ? qsTr("Open this room") : qsTr("Open this chat")
            ToolTip.visible: contextHover.hovered
            ToolTip.delay: 500
        }

        // Someone here is sharing video and it is not on screen: say so, in
        // the one place that is always visible, with the way to see it.
        RowLayout {
            visible: root.unwatchedStreamers.length > 0
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            Image {
                source: "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg"
                sourceSize.width: 12; sourceSize.height: 12
                Layout.preferredWidth: 12
                Layout.preferredHeight: 12
                Layout.alignment: Qt.AlignVCenter
            }
            Text {
                Layout.fillWidth: true
                text: root.unwatchedStreamers.length === 1
                    ? qsTr("%1 is sharing video").arg(root.unwatchedStreamers[0])
                    : qsTr("%1 are sharing video").arg(root.unwatchedStreamers.join(", "))
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                elide: Text.ElideRight
            }
            StyledButton {
                primary: true
                text: qsTr("Watch")
                onClicked: root.watchStreamersRequested()
            }
        }

        // Our own share, so it is never running unnoticed.
        RowLayout {
            visible: root.videoOn
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            Rectangle {
                Layout.preferredWidth: 7
                Layout.preferredHeight: 7
                radius: 4
                color: Theme.danger
                Layout.alignment: Qt.AlignVCenter
            }
            Text {
                Layout.fillWidth: true
                text: root.shareAudioOn ? qsTr("You are sharing video and audio")
                                        : qsTr("You are sharing video")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                elide: Text.ElideRight
            }
        }

        // Why a trust invite from the tree did not go.
        Text {
            visible: root.inviteNotice !== ""
            Layout.fillWidth: true
            text: root.inviteNotice
            color: Theme.warn
            font.pixelSize: Theme.fontSizeMicro + 1
            wrapMode: Text.WordWrap
        }

        // ── Controls ──────────────────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 2
            spacing: Theme.spacingSm

            // Mute toggle
            Rectangle {
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                radius: Theme.radiusPill
                color: root.muted ? Theme.danger : Theme.bg3

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

                ToolTip.text: root.muted ? qsTr("Unmute microphone") : qsTr("Mute microphone")
                ToolTip.visible: muteHover.hovered
                HoverHandler { id: muteHover }
            }

            // One control for sharing: the audio is an option of sharing,
            // chosen when it starts.
            Rectangle {
                id: shareButton
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                radius: Theme.radiusPill
                color: root.videoOn ? Theme.accent : Theme.bg3
                // Greyed out only when this build cannot encode at all; the
                // menu re-enumerates, so a missing camera must not disable it.
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

                // Audio-included badge: the only sign that the audio half of a
                // share actually started.
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
                    // Stopping needs no menu; starting asks about audio.
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
            }

            Item { Layout.fillWidth: true }

            // Leave / End
            Rectangle {
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                radius: Theme.radiusPill
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

                ToolTip.text: root.inRoom ? qsTr("Leave voice") : qsTr("End call")
                ToolTip.visible: endHover.hovered
                HoverHandler { id: endHover }
            }
        }
    }

    Popup {
        id: sharePopup
        // Opens upward from the dock's left edge: the menu is wider than the
        // sidebar, so it spills right over the content, not off the window.
        x: Theme.spacingSm
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
