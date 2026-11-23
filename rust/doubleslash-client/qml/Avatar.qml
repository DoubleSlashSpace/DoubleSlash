// Avatar.qml - Identity-derived symmetric identicon.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root

    // Peer identity string: base64url Ed25519 public key or any unique id.
    property string peerId: ""
    /// Room-message authors opt in; ordinary avatars remain display-only.
    property bool blockActionEnabled: false
    property bool identityBlocked: false
    signal blockRequested(bool blocked)
    signal blockAndPurgeRequested()
    readonly property bool canBlock: blockActionEnabled && peerId !== ""
        && peerId.replace(/=+$/, "") !== (backend.public_id || "").replace(/=+$/, "")

    activeFocusOnTab: canBlock
    Accessible.role: canBlock ? Accessible.Button : Accessible.NoRole
    Accessible.name: canBlock ? qsTr("Room message author actions") : ""
    Accessible.onPressAction: if (root.canBlock) authorMenu.popup()
    Keys.onReturnPressed: if (root.canBlock) authorMenu.popup()
    Keys.onSpacePressed: if (root.canBlock) authorMenu.popup()

    // When non-empty, passed verbatim to avatarSvg() and used instead of the
    // trust-tier lookup. Use this for the own-avatar live preview in Settings.
    property string configJson: ""

    property int size: 32
    property bool showRing: false
    property color ringColor: _tintHex
    property bool speaking: false
    property real audioLevel: 0.0
    property string _svgSource: ""
    property string _tintHex: Theme.toHex(Theme.muted)
    property bool _imageSmooth: true
    readonly property color tintColor: _tintHex
    readonly property bool _ringVisible: root.showRing || root.speaking
    readonly property int _ringWidth: {
        if (!_ringVisible) return 0
        return root.speaking ? Math.max(5, Math.round(root.size * 0.22))
                             : Math.max(4, Math.round(root.size * 0.17))
    }

    implicitWidth: size
    implicitHeight: size
    width: size
    height: size

    function _refresh() {
        if (!peerId) {
            _svgSource = ""
            _tintHex = Theme.toHex(Theme.muted)
            _imageSmooth = true
            return
        }
        var svgStr = backend.avatarSvg(peerId, configJson)
        _svgSource = "data:image/svg+xml;base64," + Qt.btoa(svgStr)
        _tintHex = backend.avatarTintColor(peerId, configJson)
        _imageSmooth = backend.avatarImageSmooth(peerId, configJson)
    }

    Component.onCompleted: _refresh()
    onPeerIdChanged: {
        if (authorMenu) authorMenu.close()
        _refresh()
    }
    onCanBlockChanged: if (!canBlock && authorMenu) authorMenu.close()
    onConfigJsonChanged: _refresh()

    Connections {
        target: backend
        function onAvatarConfigUpdated(peer_id) {
            if (peer_id === root.peerId) _refresh()
        }
    }

    Rectangle {
        id: statusRing
        anchors.fill: parent
        color: "transparent"
        border.color: root.ringColor
        border.width: root.showRing && !root.speaking ? root._ringWidth : 0
        visible: border.width > 0

        Behavior on border.color {
            ColorAnimation { duration: Theme.animMicro }
        }
    }

    Rectangle {
        id: body
        anchors.centerIn: parent
        width: root.size - (root._ringWidth > 0 ? root._ringWidth * 2 : 0)
        height: width
        color: "transparent"
        clip: true

        Image {
            id: img
            anchors.fill: parent
            source: root._svgSource
            sourceSize.width: root.size
            sourceSize.height: root.size
            fillMode: Image.Stretch
            smooth: root._imageSmooth
            mipmap: root._imageSmooth && root.size > 32
            visible: root._svgSource !== ""
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.canBlock
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.PointingHandCursor
        onClicked: authorMenu.popup()
        onPressAndHold: authorMenu.popup()
    }

    Menu {
        id: authorMenu
        objectName: "avatarAuthorMenu"
        MenuItem {
            objectName: "avatarBlockAction"
            text: root.identityBlocked ? qsTr("Unblock") : qsTr("Block")
            icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/block.svg")
            enabled: root.canBlock
            onTriggered: if (root.canBlock) root.blockRequested(!root.identityBlocked)
        }
        MenuItem {
            objectName: "avatarBlockPurgeAction"
            text: root.identityBlocked ? qsTr("Purge messages") : qsTr("Block and purge messages")
            icon.source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg")
            enabled: root.canBlock
            onTriggered: if (root.canBlock) root.blockAndPurgeRequested()
        }
    }
}
