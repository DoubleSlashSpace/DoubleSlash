// RoomTreeMember.qml — One person in the Rooms tree, under their room's Voice
// or Text-only leaf.
//
// The tree is the room's member list: there is no separate members rail. So
// everything the rail's menu offered — watching video, muting someone for
// yourself, their volume, a message or a trust invite — opens here, inline
// under the row, when the row is clicked.
//
// Live state (levels, their mute, "muted for me", volume) exists only for the
// voice session we are in. Everyone else is a roster entry: a name, whether we
// trust them, and whether their camera is on.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root

    property string peerId: ""
    property string displayName: ""
    property bool isSelf: false
    /// Under the Voice leaf rather than Text-only.
    property bool inVoice: true
    /// Part of the voice session we are in, so the audio controls apply.
    property bool inSession: false
    /// Their own microphone state (session members only).
    property bool isMuted: false
    property real audioLevel: 0.0
    property bool videoActive: false
    /// Their video is on screen here.
    property bool watching: false
    property bool locallyMuted: false
    property int localVolume: 100
    property bool trusted: false
    property string listPeerId: ""
    /// "" | "pending" | "sent" — our trust invite to them.
    property string inviteState: ""
    /// A trust invite can be sent from this room: we are in it ourselves.
    property bool canInvite: false
    /// Actions shown under the row.
    property bool expanded: false

    /// Tree connector codes, one per indent column; the last is this row's
    /// own: 0 blank, 1 pass-through, 2 last child, 3 child with a sibling below.
    property var guides: []
    readonly property int treeStep: Theme.spacingLg
    // 28 at the designed size. The name is caption+1, so the row grows with it.
    readonly property int rowHeight: Math.max(28, Theme.fontSizeCaption + 1 + Theme.spacingSm)

    signal toggleRequested()
    signal watchToggled()
    signal popoutRequested()
    signal localAudioChanged(bool muted, int volume)
    signal messageRequested()
    signal inviteRequested()

    readonly property real visualLevel:
        (!inSession || isMuted) ? 0.0 : Math.max(0.0, Math.min(1.0, audioLevel))
    readonly property real quantizedLevel: Math.round(visualLevel * 32) / 32
    readonly property bool speaking: quantizedLevel > 0.05
    readonly property color activityColor: {
        var lv = root.quantizedLevel
        if (lv < 0.55) {
            var t = lv / 0.55
            return Qt.rgba((48 + (87 - 48) * t) / 255, (204 + (242 - 204) * t) / 255,
                           (255 + (135 - 255) * t) / 255, 1)
        }
        var f = (lv - 0.55) / 0.45
        return Qt.rgba((87 + (254 - 87) * f) / 255, (242 + (231 - 242) * f) / 255,
                       (135 + (92 - 135) * f) / 255, 1)
    }

    readonly property string label: {
        var n = root.displayName !== "" ? root.displayName
              : (root.peerId.length > 12 ? root.peerId.substring(0, 12) + "…" : root.peerId)
        return root.isSelf ? n + qsTr(" (you)") : n
    }

    readonly property bool canWatch:
        root.inSession && !root.isSelf && (root.videoActive || root.watching)
    readonly property bool canPopout: root.inSession && !root.isSelf && root.videoActive
    // Trusted peers can be messaged; anyone else can be offered trust, from a
    // room we are in. Only one of the two ever shows.
    readonly property bool canMessage: root.trusted && root.listPeerId !== ""
    readonly property bool canOfferTrust: !root.trusted && root.canInvite
    // Decided from the conditions, not the icons' own visibility: a child of a
    // hidden row reads as hidden, so the row could never turn itself on.
    readonly property bool hasActionIcons:
        root.canWatch || root.canPopout || root.canMessage || root.canOfferTrust

    implicitHeight: rowHeight + (root.expanded ? actions.implicitHeight + Theme.spacingSm : 0)
    clip: true

    Behavior on implicitHeight {
        NumberAnimation { duration: Theme.animFast; easing.type: Easing.OutQuad }
    }

    /// One action under an open row: an SVG icon, named by its tooltip.
    component ActionIcon: IconButton {
        property string tip: ""
        implicitWidth: 28
        implicitHeight: 28
        padding: 0
        icon.width: 16
        icon.height: 16
        icon.color: Theme.text
        Accessible.name: tip
        ToolTip.text: tip
        ToolTip.visible: hovered
        ToolTip.delay: 300
    }

    // Open row background, reaching under the actions.
    Rectangle {
        anchors.fill: parent
        color: root.expanded ? Theme.semanticTint(Theme.accent, 0.10)
            : (rowHover.hovered ? Theme.bg2 : "transparent")
        Behavior on color { ColorAnimation { duration: Theme.animFast } }
    }

    // Open: an accent edge down the row and its actions, so it is plain which
    // person the panel belongs to.
    Rectangle {
        visible: root.expanded
        x: root.indent - 4
        width: 2
        height: root.height
        color: Theme.accent
    }

    // Tree connectors: ancestor columns run the full height (through the
    // actions too); this row's own column stops at its elbow when it is last.
    Row {
        id: guideRow
        x: Theme.spacingSm
        height: root.height

        Repeater {
            model: root.guides
            delegate: Item {
                id: cell
                required property int index
                required property var modelData
                readonly property int code: modelData
                readonly property bool own: index === root.guides.length - 1
                width: root.treeStep
                height: guideRow.height

                Rectangle {
                    width: 1
                    color: Theme.divider
                    x: Math.floor(cell.width / 2)
                    height: cell.code === 1 || cell.code === 3 ? cell.height
                        : (cell.code === 2 ? root.rowHeight / 2 : 0)
                    visible: cell.code !== 0
                }
                Rectangle {
                    visible: cell.own && (cell.code === 2 || cell.code === 3)
                    height: 1
                    width: cell.width / 2
                    color: Theme.divider
                    x: Math.floor(cell.width / 2)
                    y: Math.floor(root.rowHeight / 2)
                }
            }
        }
    }

    readonly property int indent: Theme.spacingSm + root.guides.length * root.treeStep

    // ── The row ─────────────────────────────────────────────────────────────
    Item {
        id: row
        x: root.indent
        width: root.width - root.indent - Theme.spacingSm
        height: root.rowHeight
        opacity: root.inVoice ? 1.0 : 0.6

        HoverHandler { id: rowHover; cursorShape: root.isSelf ? Qt.ArrowCursor : Qt.PointingHandCursor }
        TapHandler {
            enabled: !root.isSelf
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onTapped: root.toggleRequested()
        }

        RowLayout {
            anchors.fill: parent
            spacing: Theme.spacingSm

            Item {
                Layout.preferredWidth: 24
                Layout.preferredHeight: 24
                Layout.leftMargin: 2
                Layout.alignment: Qt.AlignVCenter

                Avatar {
                    anchors.centerIn: parent
                    peerId: root.peerId
                    size: 20
                }

                // Level ring — the session only, where levels exist.
                Rectangle {
                    visible: root.inSession
                    anchors.centerIn: parent
                    width: 24 + root.quantizedLevel * 3
                    height: width
                    color: "transparent"
                    border.width: 2
                    border.color: root.activityColor
                    opacity: root.speaking ? Math.min(1.0, 0.36 + root.quantizedLevel * 0.64) : 0.0
                    Behavior on width { NumberAnimation { duration: Theme.animMicro; easing.type: Easing.OutQuad } }
                    Behavior on opacity { NumberAnimation { duration: Theme.animMicro; easing.type: Easing.OutQuad } }
                }

                // "I muted them": a ring, so it reads apart from their own mute.
                Rectangle {
                    visible: root.locallyMuted && root.inSession
                    anchors.centerIn: parent
                    width: 26; height: 26
                    color: "transparent"
                    border.width: 2
                    border.color: Theme.danger
                    opacity: 0.75
                }

                Rectangle {
                    visible: root.isMuted && root.inSession
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: -2; bottomMargin: -2 }
                    width: 12; height: 12
                    color: Theme.danger

                    Image {
                        anchors.centerIn: parent
                        source: "qrc:/qt/qml/DoubleSlash/Client/icons/mic-off.svg"
                        sourceSize.width: 8; sourceSize.height: 8
                        width: 8; height: 8
                        fillMode: Image.PreserveAspectFit
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                text: root.label
                color: root.trusted || root.isSelf ? Theme.text : Theme.muted
                font.pixelSize: Theme.fontSizeCaption + 1
                font.bold: root.speaking || root.expanded
                // Not someone we trust: the name is only what the room says.
                font.italic: !root.trusted && !root.isSelf
                elide: Text.ElideRight
            }

            // Camera badge: present while they stream, filled while you watch.
            Rectangle {
                visible: root.videoActive || root.watching
                Layout.preferredWidth: 20
                Layout.preferredHeight: 20
                Layout.alignment: Qt.AlignVCenter
                color: root.watching ? Theme.accent : "transparent"
                border.color: Theme.accent
                border.width: 1
                opacity: root.videoActive ? 1.0 : 0.5

                Image {
                    anchors.centerIn: parent
                    source: "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg"
                    sourceSize.width: 11; sourceSize.height: 11
                    width: 11; height: 11
                    fillMode: Image.PreserveAspectFit
                }

                // A MouseArea, not a TapHandler: it takes the press, so the row
                // does not open its actions as well.
                MouseArea {
                    id: videoArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: root.canWatch ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: if (root.canWatch) root.watchToggled()
                }

                ToolTip.text: root.isSelf ? qsTr("You are sharing video")
                    : root.watching ? qsTr("Watching — click to stop")
                    : root.inSession ? qsTr("Streaming video — click to watch")
                    : qsTr("Streaming video — join voice to watch")
                ToolTip.visible: videoArea.containsMouse
                ToolTip.delay: 300
            }

            // Expand state: points right while shut, down while open. Shown
            // on hover too, so a row reads as something that opens.
            Image {
                visible: !root.isSelf
                source: "qrc:/qt/qml/DoubleSlash/Client/icons/chevron.svg"
                sourceSize.width: 20; sourceSize.height: 20
                Layout.preferredWidth: 10
                Layout.preferredHeight: 10
                Layout.alignment: Qt.AlignVCenter
                Layout.rightMargin: 2
                rotation: root.expanded ? 90 : 0
                opacity: root.expanded ? 1.0 : (rowHover.hovered ? 0.7 : 0.0)
                Behavior on rotation { NumberAnimation { duration: Theme.animFast } }
                Behavior on opacity { NumberAnimation { duration: Theme.animFast } }
            }
        }
    }

    // ── Actions, under the row ──────────────────────────────────────────────
    // A framed panel: the volume first, for someone whose voice reaches us
    // through this session, then one icon per action, named by its tooltip.
    Rectangle {
        id: actions
        x: root.indent + 30
        y: root.rowHeight
        width: root.width - x - Theme.spacingSm
        implicitHeight: actionColumn.implicitHeight + Theme.spacingXs * 2
        visible: root.expanded
        color: Theme.bg1
        border.width: 1
        border.color: Theme.divider

        ColumnLayout {
            id: actionColumn
            anchors {
                left: parent.left
                right: parent.right
                top: parent.top
                margins: Theme.spacingXs
            }
            spacing: 2

            RowLayout {
                visible: root.inSession && !root.isSelf
                Layout.fillWidth: true
                spacing: Theme.spacingXs

                // Mute for me, beside the volume it zeroes.
                ActionIcon {
                    icon.source: root.locallyMuted
                        ? "qrc:/qt/qml/DoubleSlash/Client/icons/speaker-off.svg"
                        : "qrc:/qt/qml/DoubleSlash/Client/icons/speaker.svg"
                    icon.color: root.locallyMuted ? Theme.danger : Theme.text
                    checked: root.locallyMuted
                    tip: root.locallyMuted ? qsTr("Unmute for me") : qsTr("Mute for me")
                    onClicked: root.localAudioChanged(!root.locallyMuted,
                        root.localVolume > 0 ? root.localVolume : 100)
                }
                SquareSlider {
                    id: volumeSlider
                    Layout.fillWidth: true
                    Layout.preferredHeight: 24
                    from: 0; to: 200; stepSize: 5
                    value: root.locallyMuted ? 0 : root.localVolume
                    // Moving it off zero unmutes, so the slider and the mute never
                    // disagree about whether you can hear them.
                    onMoved: root.localAudioChanged(value === 0, Math.round(value))
                    Accessible.name: qsTr("Volume for %1").arg(root.label)
                }
                Text {
                    text: Math.round(volumeSlider.value) + "%"
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeMicro + 1
                    Layout.preferredWidth: 30
                    horizontalAlignment: Text.AlignRight
                }
            }

            Row {
                id: actionIcons
                // Nothing to do here when every action is hidden, e.g. an
                // untrusted member of a room we are not in.
                visible: root.hasActionIcons
                spacing: 2

                ActionIcon {
                    id: watchIcon
                    visible: root.canWatch
                    icon.source: root.watching
                        ? "qrc:/qt/qml/DoubleSlash/Client/icons/video-off.svg"
                        : "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg"
                    icon.color: root.watching ? Theme.text : Theme.accent
                    checked: root.watching
                    tip: root.watching ? qsTr("Stop watching") : qsTr("Watch video")
                    onClicked: root.watchToggled()
                }
                ActionIcon {
                    id: popoutIcon
                    visible: root.canPopout
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/popout.svg"
                    tip: qsTr("Pop out video")
                    onClicked: root.popoutRequested()
                }
                // Trusted peers can be messaged; anyone else can be offered
                // trust, from a room we are in. Only one of the two ever shows.
                ActionIcon {
                    id: messageIcon
                    visible: root.canMessage
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/speech.svg"
                    tip: qsTr("Message")
                    onClicked: root.messageRequested()
                }
                ActionIcon {
                    id: inviteIcon
                    visible: root.canOfferTrust
                    enabled: root.inviteState === ""
                    icon.source: root.inviteState === "sent"
                        ? "qrc:/qt/qml/DoubleSlash/Client/icons/check.svg"
                        : "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
                    icon.color: root.inviteState === "" ? Theme.accent : Theme.muted
                    tip: root.inviteState === "sent" ? qsTr("Invite sent")
                        : root.inviteState === "pending" ? qsTr("Sending invite…")
                        : qsTr("Invite to trusted peers")
                    // A disabled button shows no tooltip of its own; say why here.
                    ToolTip.visible: hovered || (inviteHover.hovered && !enabled)
                    HoverHandler { id: inviteHover }
                    onClicked: root.inviteRequested()
                }
            }

            // Nothing applies: say so rather than open an empty panel.
            Text {
                visible: !root.hasActionIcons && !(root.inSession && !root.isSelf)
                Layout.fillWidth: true
                Layout.margins: Theme.spacingXs
                text: qsTr("Open this room to offer them trust.")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                wrapMode: Text.WordWrap
            }
        }
    }
}
