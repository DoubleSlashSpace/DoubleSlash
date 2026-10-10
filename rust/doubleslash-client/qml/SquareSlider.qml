// SquareSlider.qml — A Slider with a flat track and a square handle.
//
// Material's handle is a circle; the app is angular throughout.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

Slider {
    id: control

    background: Rectangle {
        x: control.leftPadding + (control.horizontal ? 0 : (control.availableWidth - width) / 2)
        y: control.topPadding + (control.horizontal ? (control.availableHeight - height) / 2 : 0)
        implicitWidth: control.horizontal ? 200 : 4
        implicitHeight: control.horizontal ? 4 : 200
        width: control.horizontal ? control.availableWidth : implicitWidth
        height: control.horizontal ? implicitHeight : control.availableHeight
        // A tint of the text colour reads on every surface the slider sits on.
        color: Qt.rgba(Theme.text.r, Theme.text.g, Theme.text.b, 0.22)

        // The filled part, from the start of the range to the handle.
        Rectangle {
            x: 0
            y: control.horizontal ? 0 : control.visualPosition * parent.height
            width: control.horizontal ? control.visualPosition * parent.width : parent.width
            height: control.horizontal ? parent.height : parent.height - y
            color: control.enabled ? Theme.accent : Theme.muted
        }
    }

    handle: Rectangle {
        x: control.leftPadding + (control.horizontal
            ? control.visualPosition * (control.availableWidth - width)
            : (control.availableWidth - width) / 2)
        y: control.topPadding + (control.horizontal
            ? (control.availableHeight - height) / 2
            : control.visualPosition * (control.availableHeight - height))
        implicitWidth: 14
        implicitHeight: 14
        color: !control.enabled ? Theme.muted
            : (control.pressed ? Theme.text : Theme.accent)
        border.width: control.visualFocus || control.hovered ? 2 : 0
        border.color: Theme.text
    }
}
