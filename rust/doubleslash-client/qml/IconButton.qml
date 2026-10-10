// IconButton.qml — A ToolButton on a square tile.
//
// Material draws an icon-only ToolButton's highlight as a circle, and only on
// hover. The app is angular throughout, and every control sits on a tile that
// contrasts with what is behind it, so every tool button uses this instead.
// The button keeps Material's size, so swapping one in moves nothing; the
// tile is at most the bottom bars' action size, centred in it.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

ToolButton {
    id: control

    /// The tile behind it, for a control with a colour of its own.
    property color tileColor: "transparent"
    /// An outline in that colour, or none.
    property color tileBorder: "transparent"

    background: Item {
        implicitWidth: control.Material.touchTarget
        implicitHeight: control.Material.touchTarget

        Rectangle {
            readonly property real side: Math.min(parent.width, parent.height, Theme.barActionSize)
            anchors.centerIn: parent
            // A text button keeps its width; an icon button gets a square.
            width: control.text !== "" && control.display !== AbstractButton.IconOnly
                ? parent.width : side
            height: side
            // A tint of the text colour reads on every surface and skin;
            // stronger while hovered, checked or pressed.
            color: control.tileColor.a > 0 ? control.tileColor
                : Qt.rgba(Theme.text.r, Theme.text.g, Theme.text.b,
                          !control.enabled ? 0.05
                          : control.down ? 0.26
                          : control.checked ? 0.20
                          : control.hovered ? 0.16 : 0.10)
            border.width: control.visualFocus || control.tileBorder.a > 0 ? 1 : 0
            border.color: control.visualFocus ? Theme.accent : control.tileBorder

            // Hover lifts a coloured tile too.
            Rectangle {
                anchors.fill: parent
                visible: control.tileColor.a > 0 && control.enabled
                    && (control.hovered || control.down)
                color: Qt.rgba(Theme.text.r, Theme.text.g, Theme.text.b, control.down ? 0.14 : 0.08)
            }
        }
    }
}
