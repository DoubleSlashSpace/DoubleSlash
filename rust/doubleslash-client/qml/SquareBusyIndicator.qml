// SquareBusyIndicator.qml — A BusyIndicator drawn as a turning square.
//
// Material's is a spinning arc; the app is angular throughout.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

BusyIndicator {
    id: control

    contentItem: Item {
        implicitWidth: 32
        implicitHeight: 32

        Rectangle {
            anchors.centerIn: parent
            width: Math.round(Math.min(parent.width, parent.height) * 0.55)
            height: width
            color: "transparent"
            border.width: 2
            border.color: Theme.accent
            visible: control.running

            RotationAnimator on rotation {
                from: 0
                to: 360
                duration: 1200
                loops: Animation.Infinite
                running: control.running && control.visible
            }
        }
    }
}
