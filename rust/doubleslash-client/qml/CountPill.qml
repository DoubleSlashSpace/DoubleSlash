// CountPill.qml — A small icon-and-number badge for a Rooms-tree row: voice
// or chat occupancy when a room is folded, or "—" when its node is offline.

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
    property string tip: ""

    Layout.alignment: Qt.AlignVCenter
    implicitWidth: content.implicitWidth + 10
    implicitHeight: Math.max(20, content.implicitHeight + 6)
    radius: height / 2
    color: root.active ? Theme.semanticTint(root.tint, 0.16) : Theme.bg2
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
