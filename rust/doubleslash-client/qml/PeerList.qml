// PeerList.qml — Navigation rail showing connected peers and rooms.

import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root
    color: Theme.bg1

    property int peerCount: 0
    property var peerModel: null
    property string selectedPeerId: ""
    /// Blocked peers are hidden unless this is on — the Peers header's
    /// "Show N hidden", matching the phone.
    property bool showBlocked: false
    property string sortMode: "online"
    /// How many peers are blocked, so an all-blocked list can say so.
    property int blockedCount: 0
    /// Our own row, pinned above the list and outside its Online/Offline
    /// sections. Opening it is self-chat, which reaches every device of ours.
    property string selfPeerId: ""
    property string selfAvatarId: ""
    property string selfHandle: ""
    property bool selfOnline: false
    /// Our other devices online now.
    property int ownDevicesOnline: 0
    property int selfUnread: 0
    signal selfSelected()
    signal peerSelected(string peerId, string handle)
    signal startCallRequested(string peerId)
    signal removePeerRequested(string peerId)
    signal copyPeerIdRequested(string peerId)
    signal blockPeerRequested(string peerId)
    signal unblockPeerRequested(string peerId)
    signal clearHistoryRequested(string peerId)

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Our own row ──────────────────────────────────────────────────
        Rectangle {
            id: selfRow
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(56, Theme.fontSizeBody + Theme.fontSizeCaption + Theme.spacingLg)
            readonly property bool selected: root.selfPeerId !== "" && root.selfPeerId === root.selectedPeerId
            color: selfRow.selected
                ? Theme.selectedFill()
                : (selfMouse.containsMouse ? Theme.bg3 : "transparent")
            Accessible.role: Accessible.Button
            Accessible.name: qsTr("Message myself")
            Accessible.description: selfStatus.text

            ToolTip.visible: selfMouse.containsMouse
            ToolTip.text: qsTr("Message myself")
            ToolTip.delay: 500

            Behavior on color { ColorAnimation { duration: Theme.animNormal } }

            Rectangle {
                visible: selfRow.selected || root.selfUnread > 0
                width: 3
                anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                color: Theme.accent
            }

            RowLayout {
                anchors {
                    fill: parent
                    leftMargin: Theme.spacingMd
                    rightMargin: Theme.spacingSm
                    topMargin: Theme.spacingXs
                    bottomMargin: Theme.spacingXs
                }
                spacing: Theme.spacingSm

                Avatar {
                    id: selfAvatar
                    peerId: root.selfAvatarId
                    size: 36
                    showRing: true
                    ringColor: root.selfOnline ? Theme.online : selfAvatar.tintColor
                    Layout.alignment: Qt.AlignVCenter
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2

                    Text {
                        text: qsTr("%1 (you)").arg(root.selfHandle !== "" ? root.selfHandle : qsTr("Me"))
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeBody
                        font.bold: root.selfUnread > 0
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }

                    Text {
                        id: selfStatus
                        text: !root.selfOnline
                            ? qsTr("Offline")
                            : root.ownDevicesOnline === 0
                                ? qsTr("Online · this device only")
                                : root.ownDevicesOnline === 1
                                    ? qsTr("Online · 1 other device")
                                    : qsTr("Online · %1 other devices").arg(root.ownDevicesOnline)
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeCaption
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }

                Rectangle {
                    visible: root.selfUnread > 0
                    width: Math.max(20, selfBadge.implicitWidth + 8)
                    height: Math.max(20, selfBadge.implicitHeight + 4)
                    radius: Theme.radiusPill
                    color: Theme.danger
                    Layout.alignment: Qt.AlignVCenter

                    Text {
                        id: selfBadge
                        anchors.centerIn: parent
                        text: root.selfUnread > 99 ? "99+" : root.selfUnread.toString()
                        color: Theme.textInv
                        font.pixelSize: Theme.fontSizeCaption
                        font.bold: true
                    }
                }
            }

            MouseArea {
                id: selfMouse
                anchors.fill: parent
                hoverEnabled: true
                onClicked: root.selfSelected()
            }
        }

        // Peer list backed by PeerListModel — with section grouping by online status
        ListView {
            id: peerListView
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.peerModel

            // Section grouping: Online peers first, then Offline
            section.property: root.sortMode === "online" ? "online" : ""
            section.criteria: ViewSection.FullString
            section.delegate: Rectangle {
                width: peerListView.width
                height: Math.max(22, Theme.fontSizeCaption + 8)
                color: "transparent"

                Text {
                    anchors {
                        verticalCenter: parent.verticalCenter
                        left: parent.left
                        leftMargin: Theme.spacingMd
                    }
                    text: (section === "true" ? "Online" : "Offline").toUpperCase()
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 1.2
                    font.bold: true
                }
            }

            EmptyState {
                anchors.centerIn: parent
                readonly property bool allHidden: !root.showBlocked
                    && peerListView.count > 0 && root.blockedCount >= peerListView.count
                visible: peerListView.count === 0 || allHidden
                width: Math.min(parent.width - Theme.spacingXl, 170)
                iconSource: "qrc:/qt/qml/DoubleSlash/Client/icons/peers.svg"
                iconSize: 32
                title: allHidden ? qsTr("All peers are blocked") : qsTr("No peers yet")
                subtitle: allHidden ? qsTr("Use Show hidden in the list header to show them.")
                                    : qsTr("Paste an invite above to add a trusted peer.")
            }

            delegate: Rectangle {
                id: delegateItem
                width: ListView.view.width
                // Blocked peers leave the list until "Show blocked" is on.
                visible: !delegateItem.blocked || root.showBlocked
                // 56 at the designed size: name plus preview. Grows with those two lines.
                height: visible ? Math.max(56, Theme.fontSizeBody + Theme.fontSizeCaption + Theme.spacingLg) : 0
                color: delegateItem.selected
                    ? Theme.selectedFill()
                    : (mouseArea.containsMouse ? Theme.bg3 : "transparent")

                required property string peerId
                readonly property bool selected: delegateItem.peerId === root.selectedPeerId
                required property string handle
                required property bool online
                required property bool inCall
                required property int unreadCount
                required property string lastPreview
                required property bool isTyping
                required property bool blocked

                Rectangle {
                    visible: delegateItem.selected || delegateItem.unreadCount > 0 || delegateItem.inCall
                    width: 3
                    anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                    color: delegateItem.inCall ? Theme.online : Theme.accent
                    radius: 0
                }

                Behavior on color { ColorAnimation { duration: Theme.animNormal } }

                RowLayout {
                    anchors {
                        fill: parent
                        leftMargin: Theme.spacingMd
                        rightMargin: Theme.spacingSm
                        topMargin: Theme.spacingXs
                        bottomMargin: Theme.spacingXs
                    }
                    spacing: Theme.spacingSm

                    // Identity-derived avatar with status ring
                    // (red = blocked, green = online, grey = offline)
                    Avatar {
                        id: peerAvatar
                        peerId: delegateItem.peerId
                        size: 36
                        showRing: true
                        ringColor: delegateItem.blocked ? Theme.danger
                                 : delegateItem.online   ? Theme.online
                                 : peerAvatar.tintColor
                        Layout.alignment: Qt.AlignVCenter

                        ToolTip.visible: delegateItem.blocked && hoverHandler.hovered
                        ToolTip.text: "Blocked"
                        HoverHandler { id: hoverHandler }
                    }

                    // Name + preview column
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        // Display name row
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 4

                            Text {
                                text: delegateItem.handle || delegateItem.peerId || ""
                                color: Theme.text
                                font.pixelSize: Theme.fontSizeBody
                                font.bold: delegateItem.unreadCount > 0
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            // In-call phone icon
                            Image {
                                visible: delegateItem.inCall
                                source: "qrc:/qt/qml/DoubleSlash/Client/icons/phone.svg"
                                sourceSize.width: 12
                                sourceSize.height: 12
                                width: 12
                                height: 12
                                fillMode: Image.PreserveAspectFit
                            }
                        }

                        // Preview / typing row
                        Text {
                            visible: delegateItem.isTyping || delegateItem.lastPreview !== ""
                            text: delegateItem.isTyping
                                ? "typing\u2026"
                                : delegateItem.lastPreview
                            color: delegateItem.isTyping ? Theme.accent : Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                            font.italic: delegateItem.isTyping
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                    }

                    // Unread badge
                    Rectangle {
                        visible: delegateItem.unreadCount > 0
                        width: Math.max(20, badgeText.implicitWidth + 8)
                        height: Math.max(20, badgeText.implicitHeight + 4)
                        radius: Theme.radiusPill
                        color: Theme.danger
                        Layout.alignment: Qt.AlignVCenter

                        Text {
                            id: badgeText
                            anchors.centerIn: parent
                            text: delegateItem.unreadCount > 99
                                ? "99+"
                                : delegateItem.unreadCount.toString()
                            color: Theme.textInv
                            font.pixelSize: Theme.fontSizeCaption
                            font.bold: true
                        }
                    }
                }

                MouseArea {
                    id: mouseArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton

                    onClicked: function(mouse) {
                        if (mouse.button === Qt.LeftButton) {
                            root.peerSelected(delegateItem.peerId, delegateItem.handle || delegateItem.peerId)
                        } else if (mouse.button === Qt.RightButton) {
                            peerContextMenu.targetPeerId = delegateItem.peerId
                            peerContextMenu.targetHandle = delegateItem.handle || delegateItem.peerId
                            peerContextMenu.targetBlocked = delegateItem.blocked
                            peerContextMenu.popup()
                        }
                    }
                }
            }
        }
    }

    // ── Per-peer context menu ─────────────────────────────────────────────
    Menu {
        id: peerContextMenu
        property string targetPeerId: ""
        property string targetHandle: ""
        property bool targetBlocked: false

        MenuItem {
            text: "Start Call"
            onTriggered: root.startCallRequested(peerContextMenu.targetPeerId)
        }

        MenuItem {
            text: "Copy Peer ID"
            onTriggered: root.copyPeerIdRequested(peerContextMenu.targetPeerId)
        }

        MenuSeparator {}

        MenuItem {
            text: "Remove Peer"
            onTriggered: root.removePeerRequested(peerContextMenu.targetPeerId)
        }

        MenuItem {
            text: peerContextMenu.targetBlocked ? "Unblock Peer" : "Block Peer"
            onTriggered: {
                if (peerContextMenu.targetBlocked)
                    root.unblockPeerRequested(peerContextMenu.targetPeerId)
                else
                    root.blockPeerRequested(peerContextMenu.targetPeerId)
            }
        }

        MenuSeparator {}

        MenuItem {
            text: "Clear Chat History"
            onTriggered: root.clearHistoryRequested(peerContextMenu.targetPeerId)
        }
    }
}
