// TrustInviteDialog.qml — A room member asks to become trusted peers.
//
// Nothing is trusted until Accept, which redeems the invite the offer carried
// exactly as a pasted invite link would. Offers queue rather than replace each
// other, so a second one arriving cannot swap the face under a click aimed at
// the first.

import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root
    visible: queue.count > 0

    /// Accept the offer: redeem `inviteUrl`.
    signal accepted(string inviteUrl)

    ListModel { id: queue }

    readonly property var current: queue.count > 0 ? queue.get(0) : null

    function enqueue(senderId, handle, roomName, inviteUrl) {
        for (var i = 0; i < queue.count; i++) {
            if (queue.get(i).senderId === senderId) {
                // A newer offer from the same person replaces theirs in place:
                // the older invite may already have expired.
                queue.set(i, { senderId: senderId, handle: handle,
                               roomName: roomName, inviteUrl: inviteUrl,
                               receivedAt: Date.now() })
                return
            }
        }
        queue.append({ senderId: senderId, handle: handle, roomName: roomName,
                       inviteUrl: inviteUrl, receivedAt: Date.now() })
    }

    function answer(accept) {
        if (!root.current)
            return
        var url = root.current.inviteUrl
        queue.remove(0)
        if (accept)
            root.accepted(url)
    }

    // The invite inside lives 15 minutes; an offer left unanswered past that
    // could only fail when accepted, so it goes quietly instead.
    Timer {
        interval: 30000
        repeat: true
        running: queue.count > 0
        onTriggered: {
            var cutoff = Date.now() - 14 * 60 * 1000
            for (var i = queue.count - 1; i >= 0; i--) {
                if (queue.get(i).receivedAt < cutoff)
                    queue.remove(i)
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.overlayScrim
        opacity: 0.72
    }

    // Swallow clicks on the scrim so nothing behind the prompt is pressed.
    MouseArea { anchors.fill: parent }

    Rectangle {
        anchors.centerIn: parent
        width: 340
        height: cardColumn.implicitHeight + Theme.spacingXl * 2
        color: Theme.bg2
        radius: Theme.radiusMd
        border.color: Theme.border
        border.width: 1

        ColumnLayout {
            id: cardColumn
            anchors {
                fill: parent
                margins: Theme.spacingXl
            }
            spacing: Theme.spacingMd

            Avatar {
                Layout.alignment: Qt.AlignHCenter
                peerId: root.current ? root.current.senderId : ""
                size: 56
                showRing: true
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("Become trusted peers?")
                font.pixelSize: Theme.fontSizeTitle
                font.bold: true
                color: Theme.text
                horizontalAlignment: Text.AlignHCenter
            }

            Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                // The name is the one they chose; the avatar and id below are
                // what actually identify them.
                text: root.current
                    ? ((root.current.handle !== "" ? root.current.handle : qsTr("A room member"))
                       + qsTr(" in ") + root.current.roomName
                       + qsTr(" asked to add you as a trusted peer."))
                    : ""
            }

            Label {
                Layout.fillWidth: true
                text: root.current ? root.current.senderId : ""
                font.pixelSize: Theme.fontSizeMicro
                color: Theme.muted
                elide: Text.ElideMiddle
                horizontalAlignment: Text.AlignHCenter
            }

            Label {
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                text: qsTr("Trusted peers can message you, call you and send you files outside the room.")
                font.pixelSize: Theme.fontSizeCaption
                color: Theme.muted
            }

            Label {
                Layout.fillWidth: true
                visible: queue.count > 1
                horizontalAlignment: Text.AlignHCenter
                text: qsTr("%1 more waiting").arg(queue.count - 1)
                font.pixelSize: Theme.fontSizeMicro
                color: Theme.muted
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacingMd

                StyledButton {
                    Layout.fillWidth: true
                    text: qsTr("Not now")
                    onClicked: root.answer(false)
                }

                StyledButton {
                    Layout.fillWidth: true
                    text: qsTr("Accept")
                    success: true
                    onClicked: root.answer(true)
                }
            }
        }
    }
}
