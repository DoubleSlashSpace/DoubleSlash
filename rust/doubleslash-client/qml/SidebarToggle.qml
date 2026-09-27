// SidebarToggle.qml — Peers | Rooms, beside the D:// logo in the title bar.
//
// The one switch between the two lists, at the top on every platform: the
// phone's app bar carries the same control in the same place, so neither
// client has a tab bar of its own at the bottom.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root

    /// 0 = Peers, 1 = Rooms.
    property int currentIndex: 0
    /// Draw no segment as selected, e.g. while Settings covers the lists.
    property bool dimmed: false
    signal activated(int index)

    readonly property var segments: [
        { label: qsTr("Peers"), icon: "qrc:/qt/qml/DoubleSlash/Client/icons/peers.svg" },
        { label: qsTr("Rooms"), icon: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg" }
    ]

    implicitWidth: segmentRow.implicitWidth + 4
    implicitHeight: Theme.controlHeight
    radius: Theme.radiusPill
    color: Theme.bg2
    border.color: Theme.divider
    border.width: 1

    Row {
        id: segmentRow
        anchors.centerIn: parent
        spacing: 2

        Repeater {
            model: root.segments

            delegate: Rectangle {
                id: segment
                required property int index
                required property var modelData
                readonly property bool selected: !root.dimmed && root.currentIndex === index

                width: segmentContent.implicitWidth + Theme.spacingMd * 2
                height: root.height - 4
                radius: Theme.radiusPill
                color: segment.selected ? Theme.accent
                    : (segmentHover.hovered ? Theme.bg3 : "transparent")
                Behavior on color { ColorAnimation { duration: Theme.animFast } }

                Row {
                    id: segmentContent
                    anchors.centerIn: parent
                    spacing: Theme.spacingXs

                    Image {
                        anchors.verticalCenter: parent.verticalCenter
                        source: segment.modelData.icon
                        sourceSize.width: 14; sourceSize.height: 14
                        width: 14; height: 14
                        opacity: segment.selected ? 1.0 : 0.75
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: segment.modelData.label
                        color: segment.selected ? Theme.textInv : Theme.text
                        font.pixelSize: Theme.fontSizeBody
                        font.bold: segment.selected
                    }
                }

                HoverHandler { id: segmentHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: root.activated(segment.index) }

                Accessible.role: Accessible.PageTab
                Accessible.name: segment.modelData.label
                Accessible.checked: segment.selected
            }
        }
    }
}
