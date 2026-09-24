// MemberRow.qml - One person in the voice rail's member list.
//
// The rail lists everyone in a room, in voice or not, so each row says which:
// a voice member carries a live level ring and reads at full strength; a
// text-only member is dimmed with "Text only" beneath the name. Everything you
// can do about a person — watch their video, mute them for yourself, message or
// invite them — hangs off the one menu the avatar opens.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root

    property string peerId: ""
    property string displayName: ""
    property bool isSelf: false
    /// In the voice channel. False only in a room's list, for text-only members.
    property bool inVoice: true
    /// The member's own microphone state, visible to everyone.
    property bool isMuted: false
    property real audioLevel: 0.0
    /// Sending video right now.
    property bool videoActive: false
    /// Their video is on screen here (expanded or popped out).
    property bool watching: false
    /// Muted by this listener only.
    property bool locallyMuted: false
    /// A trusted peer rather than only a room member.
    property bool trusted: false
    /// "" | "pending" | "sent" — our trust invite to them, if any.
    property string inviteState: ""

    /// Open the member menu (avatar or row clicked, or right-click).
    signal menuRequested()
    /// The camera badge was clicked — the shortcut for watching.
    signal videoClicked()

    readonly property real visualLevel:
        (!inVoice || isMuted) ? 0.0 : Math.max(0.0, Math.min(1.0, audioLevel))
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

    readonly property string statusText: {
        if (!root.inVoice)
            return qsTr("Text only")
        var parts = [qsTr("In voice")]
        if (root.isMuted) parts.push(qsTr("muted"))
        if (root.locallyMuted) parts.push(qsTr("muted for you"))
        return parts.join(" · ")
    }

    implicitHeight: 46

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: 4
        anchors.rightMargin: 4
        radius: Theme.radiusSm
        color: hover.hovered ? Theme.bg3 : "transparent"
    }

    RowLayout {
        anchors {
            fill: parent
            leftMargin: Theme.spacingSm
            rightMargin: Theme.spacingSm
        }
        spacing: Theme.spacingSm
        // Text-only members recede; the room is still theirs, but they cannot
        // hear you, and that is the first thing a voice list has to say.
        opacity: root.inVoice ? 1.0 : 0.6

        Item {
            Layout.preferredWidth: 34
            Layout.preferredHeight: 34
            Layout.alignment: Qt.AlignVCenter

            Avatar {
                anchors.centerIn: parent
                peerId: root.peerId
                size: 28
                showRing: true
            }

            // Level ring, drawn only for voice members: a text-only member
            // makes no sound here, and an idle ring would suggest they could.
            Rectangle {
                visible: root.inVoice
                anchors.centerIn: parent
                width: 32 + root.quantizedLevel * 3
                height: width
                radius: width / 2
                color: "transparent"
                border.width: 3 + root.quantizedLevel
                border.color: root.activityColor
                opacity: root.speaking ? Math.min(1.0, 0.36 + root.quantizedLevel * 0.64) : 0.0

                Behavior on width { NumberAnimation { duration: Theme.animMicro; easing.type: Easing.OutQuad } }
                Behavior on opacity { NumberAnimation { duration: Theme.animMicro; easing.type: Easing.OutQuad } }
            }

            // "I muted them" — a ring, not a badge, so it cannot be confused
            // with their own mute below.
            Rectangle {
                visible: root.locallyMuted && root.inVoice
                anchors.centerIn: parent
                width: 34; height: 34; radius: 17
                color: "transparent"
                border.width: 2
                border.color: Theme.danger
                opacity: 0.75
            }

            Rectangle {
                visible: root.isMuted && root.inVoice
                anchors { right: parent.right; bottom: parent.bottom }
                width: 14; height: 14; radius: 7
                color: Theme.danger

                Image {
                    anchors.centerIn: parent
                    source: "qrc:/qt/qml/DoubleSlash/Client/icons/mic-off.svg"
                    sourceSize.width: 9; sourceSize.height: 9
                    width: 9; height: 9
                    fillMode: Image.PreserveAspectFit
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: 0

            Text {
                Layout.fillWidth: true
                text: root.label
                color: Theme.text
                font.pixelSize: Theme.fontSizeCaption
                font.bold: root.speaking
                elide: Text.ElideRight
            }
            Text {
                Layout.fillWidth: true
                text: root.inviteState === "sent" ? qsTr("Invite sent")
                    : root.inviteState === "pending" ? qsTr("Sending invite…")
                    : root.statusText
                color: root.inviteState !== "" ? Theme.accent : Theme.muted
                font.pixelSize: Theme.fontSizeMicro
                elide: Text.ElideRight
            }
        }

        // Camera badge: present while they stream, filled while you watch.
        // Clicking it is the shortcut for the menu's Watch entry.
        Rectangle {
            visible: root.videoActive || root.watching
            Layout.preferredWidth: 22
            Layout.preferredHeight: 22
            Layout.alignment: Qt.AlignVCenter
            radius: 11
            color: root.watching ? Theme.accent : "transparent"
            border.color: Theme.accent
            border.width: 1
            opacity: root.videoActive ? 1.0 : 0.5

            Image {
                anchors.centerIn: parent
                source: "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg"
                sourceSize.width: 12; sourceSize.height: 12
                width: 12; height: 12
                fillMode: Image.PreserveAspectFit
            }

            // A MouseArea, not a TapHandler: it accepts the press, so the row's
            // own handler never sees it and the menu does not open as well.
            MouseArea {
                id: videoArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.videoClicked()
            }

            ToolTip.text: root.watching ? qsTr("Watching — click to stop")
                                        : qsTr("Streaming video — click to watch")
            ToolTip.visible: videoArea.containsMouse
            ToolTip.delay: 300
        }
    }

    HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }

    // Left or right click: one menu either way. A left click on the camera
    // badge is taken by the badge's own handler first.
    TapHandler {
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onTapped: root.menuRequested()
    }
}
