// IconButton.qml — A ToolButton with a square hover and press fill.
//
// Material draws an icon-only ToolButton's highlight as a circle. The app is
// angular throughout, so every tool button uses this instead. The size is
// Material's own, so swapping one in moves nothing.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

ToolButton {
    id: control

    background: Rectangle {
        implicitWidth: control.Material.touchTarget
        implicitHeight: control.Material.touchTarget
        visible: control.enabled
            && (control.down || control.hovered || control.visualFocus || control.checked)
        // A tint of the text colour reads on every surface the button sits on.
        color: Qt.rgba(Theme.text.r, Theme.text.g, Theme.text.b,
                       control.down ? 0.18 : (control.checked ? 0.14 : 0.08))
        border.width: control.visualFocus ? 1 : 0
        border.color: Theme.accent
    }
}
