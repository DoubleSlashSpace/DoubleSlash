// CountPill.qml — A small icon-and-number badge for a Rooms-tree row: voice
// or chat occupancy, or "—" when its node is offline. `checked` is the voice
// count while that list is open.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root

    property string iconSource: ""
    property string text: ""
    property color tint: Theme.online
    /// Live and non-zero: tinted. Otherwise neutral, reading as "no number".
    property bool active: true
    /// The list this count controls is open.
    property bool checked: false
    property string tip: ""

    Layout.alignment: Qt.AlignVCenter
    implicitWidth: content.implicitWidth + 10
    implicitHeight: Math.max(20, content.implicitHeight + 6)
    // A neighbouring control (the hang-up on a live voice row) must not
    // squeeze this badge to nothing. The name ellipsizes instead.
    Layout.minimumWidth: implicitWidth
    Layout.maximumWidth: implicitWidth
    Layout.fillWidth: false
    color: !root.active ? Theme.bg2
        : root.checked ? Theme.semanticTint(root.tint, 0.36)
        : Theme.semanticTint(root.tint, 0.16)
    border.color: root.active ? root.tint : Theme.divider
    border.width: 1

    Row {
        id: content
        anchors.centerIn: parent
        spacing: 4

        Image {
            anchors.verticalCenter: parent.verticalCenter
            source: root.iconSource
            sourceSize.width: 11; sourceSize.height: 11
            width: 11; height: 11
            fillMode: Image.PreserveAspectFit
            opacity: root.active ? 1.0 : 0.55
        }
        Label {
            anchors.verticalCenter: parent.verticalCenter
            text: root.text
            color: root.active ? root.tint : Theme.muted
            font.pixelSize: Theme.fontSizeCaption
            font.bold: root.active
        }
    }

    HoverHandler { id: hover }
    ToolTip.text: root.tip
    ToolTip.visible: hover.hovered && root.tip !== ""
    ToolTip.delay: 400
}
