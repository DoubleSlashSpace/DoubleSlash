// SidebarToggle.qml — Peers | Rooms, beside the D:// logo in the title bar.
//
// The one switch between the two lists, at the top on every platform: the
// phone's app bar carries the same control in the same place, so neither
// client has a tab bar of its own at the bottom.
//
// Drawn as an elongated hexagon rather than a pill: the UI is angular
// throughout. Each segment's highlight is pointed on its outer end and flat
// where the two segments meet.

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
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

    /// Gap between the track and the segments.
    readonly property int inset: 2

    implicitWidth: segmentRow.implicitWidth + inset * 2
    implicitHeight: Theme.controlHeight

    /// How far a pointed end reaches in, for a 120° corner as in a regular
    /// hexagon: half the height times tan 30°.
    function pointDepth(h) { return h / (2 * Math.sqrt(3)) }

    /// A hexagon, or half of one when an end is flat.
    component Hexagon: Canvas {
        property color fillColor: "transparent"
        property color strokeColor: "transparent"
        property bool pointLeft: true
        property bool pointRight: true

        onFillColorChanged: requestPaint()
        onStrokeColorChanged: requestPaint()
        onPointLeftChanged: requestPaint()
        onPointRightChanged: requestPaint()
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            ctx.reset()
            var w = width, h = height
            if (w <= 0 || h <= 0)
                return
            // Half a pixel in, so a 1px stroke stays crisp and inside.
            var s = strokeColor.a > 0 ? 0.5 : 0
            var d = root.pointDepth(h)
            ctx.beginPath()
            ctx.moveTo(pointLeft ? d : s, s)
            ctx.lineTo(pointRight ? w - d : w - s, s)
            if (pointRight)
                ctx.lineTo(w - s, h / 2)
            ctx.lineTo(pointRight ? w - d : w - s, h - s)
            ctx.lineTo(pointLeft ? d : s, h - s)
            if (pointLeft)
                ctx.lineTo(s, h / 2)
            ctx.closePath()
            if (fillColor.a > 0) {
                ctx.fillStyle = fillColor
                ctx.fill()
            }
            if (strokeColor.a > 0) {
                ctx.lineWidth = 1
                ctx.lineJoin = "miter"
                ctx.strokeStyle = strokeColor
                ctx.stroke()
            }
        }
    }

    Hexagon {
        anchors.fill: parent
        fillColor: Theme.bg2
        strokeColor: Theme.divider
    }

    Row {
        id: segmentRow
        anchors.centerIn: parent
        spacing: 2

        Repeater {
            model: root.segments

            delegate: Item {
                id: segment
                required property int index
                required property var modelData
                readonly property bool selected: !root.dimmed && root.currentIndex === index
                readonly property bool first: index === 0
                readonly property bool last: index === root.segments.length - 1
                readonly property real depth: root.pointDepth(height)

                width: segmentContent.implicitWidth + Theme.spacingMd * 2
                    + (first ? depth : 0) + (last ? depth : 0)
                height: root.height - root.inset * 2

                Hexagon {
                    anchors.fill: parent
                    pointLeft: segment.first
                    pointRight: segment.last
                    fillColor: segment.selected ? Theme.accent
                        : (segmentHover.hovered ? Theme.bg3 : "transparent")
                }

                Row {
                    id: segmentContent
                    anchors.centerIn: parent
                    // Centre on the flat body, not on the point.
                    anchors.horizontalCenterOffset:
                        ((segment.first ? segment.depth : 0) - (segment.last ? segment.depth : 0)) / 2
                    spacing: Theme.spacingXs

                    Image {
                        anchors.verticalCenter: parent.verticalCenter
                        source: segment.modelData.icon
                        sourceSize.width: 16; sourceSize.height: 16
                        width: 16; height: 16
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
