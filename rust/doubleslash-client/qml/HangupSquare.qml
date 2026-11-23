// HangupSquare.qml — Leave / end call.
//
// A solid danger square with the X cut out of it. The hole is the mark, so
// whatever is behind the control shows through, and the same square is the
// voice dock's Leave and the hang-up on the live voice room. Square caps,
// so the arms end flat. The two ratios are shared with Android's HangupSquare.
import QtQuick
import DoubleSlash.Client 1.0

Canvas {
    id: mark

    property color fill: Theme.danger

    // Paint in device pixels. The context is not pre-scaled when canvasSize
    // is set by hand, so the transform below is the whole scale.
    readonly property real ratio: Screen.devicePixelRatio
    canvasSize: Qt.size(Math.max(1, width * ratio), Math.max(1, height * ratio))

    onFillChanged: requestPaint()
    onRatioChanged: requestPaint()

    Behavior on fill { ColorAnimation { duration: Theme.animFast } }

    onPaint: {
        var ctx = getContext("2d")
        var w = width
        var h = height
        if (w < 1 || h < 1)
            return
        ctx.reset()
        ctx.scale(ratio, ratio)
        ctx.clearRect(0, 0, w, h)
        ctx.globalCompositeOperation = "source-over"
        ctx.fillStyle = fill
        ctx.fillRect(0, 0, w, h)

        // Centerline inset 0.22 of the side, stroke 0.18, square caps.
        // The caps stay inside the square; Android uses these same fractions.
        var s = Math.min(w, h)
        var ox = (w - s) / 2
        var oy = (h - s) / 2
        var inset = s * 0.22
        ctx.globalCompositeOperation = "destination-out"
        ctx.strokeStyle = "#ffffff"
        ctx.lineWidth = s * 0.18
        ctx.lineCap = "square"
        ctx.lineJoin = "miter"
        ctx.beginPath()
        ctx.moveTo(ox + inset, oy + inset)
        ctx.lineTo(ox + s - inset, oy + s - inset)
        ctx.moveTo(ox + s - inset, oy + inset)
        ctx.lineTo(ox + inset, oy + s - inset)
        ctx.stroke()
    }
}
