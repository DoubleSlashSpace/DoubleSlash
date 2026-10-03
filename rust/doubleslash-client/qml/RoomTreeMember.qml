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
    signal copyIdRequested()

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

    implicitHeight: rowHeight + (root.expanded ? actions.implicitHeight : 0)
    clip: true

    Behavior on implicitHeight {
        NumberAnimation { duration: Theme.animFast; easing.type: Easing.OutQuad }
    }

    // Open row background, reaching under the actions.
    Rectangle {
        anchors.fill: parent
        color: root.expanded ? Theme.bg2 : (rowHover.hovered ? Theme.bg2 : "transparent")
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
                    radius: width / 2
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
                    width: 26; height: 26; radius: 13
                    color: "transparent"
                    border.width: 2
                    border.color: Theme.danger
                    opacity: 0.75
                }

                Rectangle {
                    visible: root.isMuted && root.inSession
                    anchors { right: parent.right; bottom: parent.bottom; rightMargin: -2; bottomMargin: -2 }
                    width: 12; height: 12; radius: 6
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
                font.bold: root.speaking
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
                radius: 10
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
        }
    }

    // ── Actions, under the row ──────────────────────────────────────────────
    ColumnLayout {
        id: actions
        x: root.indent + 30
        y: root.rowHeight
        width: root.width - x - Theme.spacingSm
        spacing: Theme.spacingXs
        visible: root.expanded

        // Volume, for someone whose voice reaches us through this session.
        RowLayout {
            visible: root.inSession && !root.isSelf
            Layout.fillWidth: true
            Layout.topMargin: 2
            spacing: Theme.spacingXs

            Image {
                source: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg"
                sourceSize.width: 12; sourceSize.height: 12
                Layout.preferredWidth: 12
                Layout.preferredHeight: 12
                Layout.alignment: Qt.AlignVCenter
                opacity: 0.8
            }
            Slider {
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

        Flow {
            Layout.fillWidth: true
            Layout.bottomMargin: Theme.spacingSm
            spacing: Theme.spacingXs

            StyledButton {
                visible: root.canWatch
                primary: !root.watching
                text: root.watching ? qsTr("Stop watching") : qsTr("Watch video")
                onClicked: root.watchToggled()
            }
            StyledButton {
                visible: root.inSession && !root.isSelf && root.videoActive
                text: qsTr("Pop out")
                onClicked: root.popoutRequested()
            }
            StyledButton {
                visible: root.inSession && !root.isSelf
                text: root.locallyMuted ? qsTr("Unmute for me") : qsTr("Mute for me")
                onClicked: root.localAudioChanged(!root.locallyMuted,
                    root.localVolume > 0 ? root.localVolume : 100)
            }
            // Trusted peers can be messaged; anyone else can be offered trust,
            // from a room we are in. Only one of the two ever shows.
            StyledButton {
                visible: root.trusted && root.listPeerId !== ""
                text: qsTr("Message")
                onClicked: root.messageRequested()
            }
            StyledButton {
                visible: !root.trusted && root.canInvite
                primary: root.inviteState === ""
                enabled: root.inviteState === ""
                text: root.inviteState === "sent" ? qsTr("Invite sent")
                    : root.inviteState === "pending" ? qsTr("Sending invite…")
                    : qsTr("Invite to trusted peers")
                onClicked: root.inviteRequested()
            }
            StyledButton {
                text: qsTr("Copy peer ID")
                onClicked: root.copyIdRequested()
            }
        }
    }
}
