// CreateRoomDialog.qml — Dialog to create a public or private SFU voice room.
//
// Open via createRoomDialog.openForNode(supernodeId, "public"|"private")
// or createRoomDialog.open() to pick a supernode from the list.

import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Dialog {
    id: root

    // The live node list model from MainWindow (used when no supernode is preset).
    required property ListModel nodeListModel
    /// Offer hidden rooms as parents too. Follows the Rooms list: a room the
    /// sidebar is not showing is not offered to nest under.
    property bool includeHidden: false

    // When set, the supernode picker is hidden and this node is used on accept.
    property string targetSupernodeId: ""
    // "public" or "private" — drives title and the createRoom wire shape.
    property string roomType: "public"
    // When set, the new room nests under this room in the Space tree (sub-room).
    property string parentRoomId: ""
    // Display name of the parent room, shown as a subtitle.
    property string parentRoomName: ""
    // The title-bar + : one dialog with a private checkbox, an optional
    // parent, and a host picker only when there is more than one supernode.
    // Context-menu creates leave this false and keep the preset type.
    property bool compose: false

    readonly property bool supernodePreset: targetSupernodeId !== ""
    readonly property bool isSubRoom: parentRoomId !== ""
    readonly property bool privateRoom: compose ? privateSwitch.checked : roomType === "private"
    readonly property string resolvedSupernodeId: {
        if (targetSupernodeId !== "")
            return targetSupernodeId
        if (nodeListModel.count === 1)
            return nodeListModel.get(0).node_id || ""
        if (supernodeBox.currentIndex >= 0 && nodeListModel.count > 0)
            return nodeListModel.get(supernodeBox.currentIndex).node_id || ""
        return ""
    }

    title: compose
        ? qsTr("New room")
        : (isSubRoom
            ? qsTr("Create Sub-room")
            : (roomType === "private" ? qsTr("Create Private Room") : qsTr("Create Public Room")))
    modal: true
    standardButtons: Dialog.Ok | Dialog.Cancel
    closePolicy: Dialog.CloseOnEscape
    width: 400
    padding: Theme.spacingXl
    implicitHeight: header.height + contentColumn.implicitHeight + buttonBox.implicitHeight
                    + topPadding + bottomPadding

    function openNew() {
        compose = true
        targetSupernodeId = ""
        roomType = "public"
        parentRoomId = ""
        parentRoomName = ""
        open()
    }

    function openForNode(supernodeId, type) {
        compose = false
        targetSupernodeId = supernodeId || ""
        roomType = (type === "private") ? "private" : "public"
        parentRoomId = ""
        parentRoomName = ""
        open()
    }

    // Open to create a sub-room nested under `parentId` on `supernodeId`.
    function openForParent(supernodeId, type, parentId, parentName) {
        compose = false
        targetSupernodeId = supernodeId || ""
        roomType = (type === "private") ? "private" : "public"
        parentRoomId = parentId || ""
        parentRoomName = parentName || ""
        open()
    }

    // Rooms on the chosen host, for the "Inside" picker. The first row is
    // always "top level".
    function refillParents() {
        parentChoices.clear()
        parentChoices.append({ room_id: "", name: qsTr("Nothing — top level") })
        var snId = resolvedSupernodeId
        if (snId === "")
            return
        for (var i = 0; i < nodeListModel.count; i++) {
            var node = nodeListModel.get(i)
            if ((node.node_id || "") !== snId)
                continue
            var rooms = []
            try { rooms = JSON.parse(node.rooms_json || "[]") } catch (e) { rooms = [] }
            for (var r = 0; r < rooms.length; r++) {
                var room = rooms[r]
                if (!room || !room.room_id)
                    continue
                if (room.hidden && !root.includeHidden)
                    continue
                parentChoices.append({
                    room_id: room.room_id,
                    name: room.name || room.room_name || room.room_id
                })
            }
            break
        }
    }

    onOpened: {
        roomNameField.text = ""
        membersCanInviteSwitch.checked = false
        privateSwitch.checked = false
        if (!supernodePreset && nodeListModel.count > 1)
            supernodeBox.currentIndex = 0
        if (compose)
            refillParents()
        roomNameField.forceActiveFocus()
    }

    onAccepted: {
        var name = roomNameField.text.trim()
        if (name === "") return
        var snId = resolvedSupernodeId
        if (snId === "") return
        var type = privateRoom ? "private" : "public"
        var invitePolicy = (privateRoom && membersCanInviteSwitch.checked)
            ? "members" : "owner"
        var parentId = parentRoomId
        if (compose && parentBox.currentIndex > 0 && parentChoices.count > parentBox.currentIndex)
            parentId = parentChoices.get(parentBox.currentIndex).room_id || ""
        if (parentId !== "")
            backend.createSubRoom(snId, name, type, parentId, invitePolicy)
        else
            backend.createRoom(snId, name, type, invitePolicy)
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
        radius: 0
        Text {
            anchors.centerIn: parent
            text: root.title
            color: Theme.text
            font.pixelSize: Theme.fontSizeBody
            font.bold: true
        }
    }

    contentItem: ColumnLayout {
        id: contentColumn
        spacing: Theme.spacingMd

        Text {
            visible: root.isSubRoom
            text: qsTr("Nested under “%1”.").arg(root.parentRoomName || root.parentRoomId)
            color: Theme.muted
            font.pixelSize: Theme.fontSizeCaption
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            visible: root.compose
            Layout.fillWidth: true
            spacing: Theme.spacingSm

            ColumnLayout {
                spacing: 2
                Layout.fillWidth: true

                Text {
                    text: qsTr("Private")
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                }
                Text {
                    text: root.privateRoom
                        ? qsTr("Only people you invite can join.")
                        : qsTr("Anyone on this supernode can find and join it.")
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }

            SquareSwitch {
                id: privateSwitch
                checked: false
            }
        }

        Text {
            visible: root.privateRoom && !root.compose
            text: qsTr("Private rooms require an invite token to join. You'll receive one after creation.")
            color: Theme.muted
            font.pixelSize: Theme.fontSizeCaption
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            visible: root.privateRoom
            Layout.fillWidth: true
            spacing: Theme.spacingSm

            ColumnLayout {
                spacing: 2
                Layout.fillWidth: true

                Text {
                    text: qsTr("Members can invite")
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                }
                Text {
                    text: qsTr("Let any current member mint invite tokens, not just you.")
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }

            SquareSwitch {
                id: membersCanInviteSwitch
                checked: false
            }
        }

        ColumnLayout {
            spacing: 4
            Layout.fillWidth: true

            Text {
                text: qsTr("Room name")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
            }

            TextField {
                id: roomNameField
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.controlHeight
                placeholderText: qsTr("e.g. Game Night")
                maximumLength: 64
                background: Rectangle {
                    color: Theme.bg2
                    radius: 0
                    border.color: roomNameField.activeFocus ? Theme.accent : Theme.bg3
                    border.width: 1
                }
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                Keys.onReturnPressed: root.accept()
            }
        }

        ColumnLayout {
            visible: root.compose && parentChoices.count > 1
            spacing: 4
            Layout.fillWidth: true

            Text {
                text: qsTr("Inside")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
            }

            ComboBox {
                id: parentBox
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.controlHeight
                model: parentChoices
                textRole: "name"

                background: Rectangle {
                    color: Theme.bg2
                    radius: 0
                    border.color: parentBox.activeFocus ? Theme.accent : Theme.bg3
                    border.width: 1
                }

                contentItem: Text {
                    leftPadding: 8
                    text: {
                        if (parentBox.currentIndex < 0 || parentBox.currentIndex >= parentChoices.count)
                            return ""
                        var row = parentChoices.get(parentBox.currentIndex)
                        return row ? (row.name || "") : ""
                    }
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }
            }
        }

        ListModel { id: parentChoices }

        ColumnLayout {
            visible: root.compose ? root.nodeListModel.count > 1 : !root.supernodePreset
            spacing: 4
            Layout.fillWidth: true

            Text {
                text: qsTr("Host supernode")
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
            }

            ComboBox {
                id: supernodeBox
                Layout.fillWidth: true
                Layout.preferredHeight: Theme.controlHeight
                model: root.nodeListModel
                textRole: "title"
                onCurrentIndexChanged: if (root.compose && root.opened) root.refillParents()

                background: Rectangle {
                    color: Theme.bg2
                    radius: 0
                    border.color: supernodeBox.activeFocus ? Theme.accent : Theme.bg3
                    border.width: 1
                }

                contentItem: Text {
                    leftPadding: 8
                    text: {
                        if (supernodeBox.currentIndex < 0) return supernodeBox.displayText
                        var row = nodeListModel.get(supernodeBox.currentIndex)
                        return (row.title && row.title !== "") ? row.title : row.node_id
                    }
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    verticalAlignment: Text.AlignVCenter
                }

                delegate: ItemDelegate {
                    required property int index
                    required property string title
                    required property string node_id
                    width: supernodeBox.width
                    contentItem: Text {
                        text: (title && title !== "") ? title : node_id
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeBody
                    }
                    highlighted: supernodeBox.highlightedIndex === index
                    background: Rectangle {
                        color: highlighted ? Theme.accent : Theme.bg2
                    }
                }

                popup: Popup {
                    y: supernodeBox.height + 2
                    width: supernodeBox.width
                    implicitHeight: contentItem.implicitHeight
                    padding: 0

                    contentItem: ListView {
                        clip: true
                        implicitHeight: contentHeight
                        model: supernodeBox.delegateModel
                        ScrollBar.vertical: ScrollBar {}
                    }

                    background: Rectangle {
                        color: Theme.bg2
                        radius: 0
                        border.color: Theme.bg3
                        border.width: 1
                    }
                }
            }

            Text {
                visible: root.nodeListModel.count === 0
                text: qsTr("No supernodes connected. Connect to a supernode first.")
                color: Theme.danger
                font.pixelSize: Theme.fontSizeCaption
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }

    footer: DialogButtonBox {
        id: buttonBox
        standardButtons: root.standardButtons
        background: Rectangle {
            color: Theme.bg0
            radius: 0
            Rectangle {
                anchors { left: parent.left; right: parent.right; top: parent.top }
                height: 10
                color: Theme.bg0
            }
        }
        delegate: Button {
            enabled: DialogButtonBox.buttonRole !== DialogButtonBox.AcceptRole
                  || (roomNameField.text.trim() !== "" && root.resolvedSupernodeId !== "")
        }
    }
}