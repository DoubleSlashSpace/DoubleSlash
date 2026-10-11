// QrCode.qml — A QR symbol drawn from rows of `1` (dark) and `0` (light).
//
// The rows come from `backend.qrRows`. The symbol stays on a white square:
// a scanner needs that contrast whatever skin is active. An empty `rows`
// draws nothing.

import QtQuick
import DoubleSlash.Client 1.0

Item {
    id: root

    property string rows: ""

    implicitWidth: 220
    implicitHeight: 220

    Rectangle {
        anchors.fill: parent
        color: "white"
        border.color: Theme.border
        border.width: 1
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        anchors.margins: 8

        onPaint: {
            var ctx = getContext("2d")
            ctx.clearRect(0, 0, width, height)
            var lines = root.rows.split("\n")
            var kept = []
            var n = 0
            for (var i = 0; i < lines.length; i++) {
                if (lines[i].length > 0) {
                    kept.push(lines[i])
                    n = lines[i].length
                }
            }
            if (kept.length === 0 || n === 0)
                return
            var cell = Math.floor(Math.min(width, height) / n)
            if (cell < 1)
                return
            var drawn = cell * n
            var originX = Math.floor((width - drawn) / 2)
            var originY = Math.floor((height - drawn) / 2)
            ctx.fillStyle = "#111214"
            for (var y = 0; y < kept.length; y++) {
                var row = kept[y]
                for (var x = 0; x < n; x++) {
                    if (row.charAt(x) === "1")
                        ctx.fillRect(originX + x * cell, originY + y * cell, cell, cell)
                }
            }
        }

        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
    }

    onRowsChanged: canvas.requestPaint()
}
