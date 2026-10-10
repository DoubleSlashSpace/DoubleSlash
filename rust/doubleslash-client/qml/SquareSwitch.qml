// SquareSwitch.qml — A Switch with a rectangular track and a square thumb.
//
// Material's is a pill with a round thumb; the app is angular throughout.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

Switch {
    id: control

    indicator: Rectangle {
        implicitWidth: 40
        implicitHeight: 20
        // Placed as Material places its own indicator.
        x: control.text
            ? (control.mirrored ? control.width - width - control.rightPadding : control.leftPadding)
            : control.leftPadding + (control.availableWidth - width) / 2
        y: control.topPadding + (control.availableHeight - height) / 2
        color: control.checked ? Theme.accent : "transparent"
        border.width: 1
        border.color: control.checked ? Theme.accent : Theme.muted
        opacity: control.enabled ? 1.0 : 0.5

        Rectangle {
            width: parent.height - 8
            height: width
            y: 4
            x: control.checked ? parent.width - width - 4 : 4
            color: control.checked ? Theme.textInv : Theme.muted
            Behavior on x { NumberAnimation { duration: Theme.animFast } }
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
