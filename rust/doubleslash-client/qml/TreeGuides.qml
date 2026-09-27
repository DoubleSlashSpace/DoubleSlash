// TreeGuides.qml — The connector lines in a Rooms-tree row's indent.
//
// One cell per indent column, from `guides`: 0 blank, 1 pass-through │,
// 2 └ (last child), 3 ├ (a sibling follows). Only the last column can hold an
// elbow; the others carry an ancestor's line past this row.

import QtQuick
import DoubleSlash.Client 1.0

Row {
    id: root

    property var guides: []
    readonly property int step: Theme.spacingLg

    Repeater {
        model: root.guides

        delegate: Item {
            id: cell
            required property var modelData
            readonly property int code: modelData
            width: root.step
            height: root.height

            Rectangle {
                visible: cell.code !== 0
                width: 1
                color: Theme.divider
                x: Math.floor(cell.width / 2)
                height: cell.code === 1 || cell.code === 3 ? cell.height
                    : (cell.code === 2 ? Math.floor(cell.height / 2) : 0)
            }
            Rectangle {
                visible: cell.code === 2 || cell.code === 3
                height: 1
                width: cell.width / 2
                color: Theme.divider
                x: Math.floor(cell.width / 2)
                y: Math.floor(cell.height / 2)
            }
        }
    }
}
