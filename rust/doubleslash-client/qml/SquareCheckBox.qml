// SquareCheckBox.qml — A CheckBox with a square box and a mitred tick.
//
// Material's indicator has 2 px corners hard-coded; the app is angular
// throughout.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

CheckBox {
    id: control

    indicator: Rectangle {
        implicitWidth: 18
        implicitHeight: 18
        // Placed as Material places its own indicator.
        x: control.text
            ? (control.mirrored ? control.width - width - control.rightPadding : control.leftPadding)
            : control.leftPadding + (control.availableWidth - width) / 2
        y: control.topPadding + (control.availableHeight - height) / 2
        color: control.checked ? Theme.accent : "transparent"
        border.width: control.checked ? 0 : 2
        border.color: Theme.muted
        opacity: control.enabled ? 1.0 : 0.5

        // The tick from icons/check.svg, drawn so it takes the skin's
        // text-on-accent colour.
        Canvas {
            id: tick
            anchors.fill: parent
            visible: control.checked
            readonly property color stroke: Theme.textInv
            onStrokeChanged: requestPaint()
            onPaint: {
                var ctx = getContext("2d")
                ctx.reset()
                ctx.strokeStyle = stroke
                ctx.lineWidth = 2
                ctx.lineJoin = "miter"
                ctx.lineCap = "square"
                ctx.beginPath()
                ctx.moveTo(width * 0.22, height * 0.5)
                ctx.lineTo(width * 0.42, height * 0.72)
                ctx.lineTo(width * 0.8, height * 0.28)
                ctx.stroke()
            }
        }

        Rectangle {
            visible: control.visualFocus
            anchors.fill: parent
            anchors.margins: -3
            color: "transparent"
            border.width: 1
            border.color: Theme.accent
        }
    }
}
