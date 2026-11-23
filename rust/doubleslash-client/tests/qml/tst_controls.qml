import QtQuick
import QtQuick.Controls.Material
import QtTest
import DoubleSlash.Client 1.0

Item {
    width: 640
    height: 360

    StyledButton {
        id: button
        x: 20
        y: 20
        width: 120
        text: "A deliberately long action label"
    }

    StyledTextField {
        id: field
        x: 20
        y: 80
        width: 240
        placeholderText: "Peer name"
    }

    // 36 is the voice dock's control at the designed size. The room row uses
    // the same mark at 26. Both were cropped when the buffer grew with the
    // screen scale and Qt kept showing only the item-sized window.
    HangupSquare {
        id: hangup
        x: 420
        y: 40
        width: 36
        height: 36
        fill: "#ff2b40"
    }

    TestCase {
        name: "SharedControls"
        when: windowShown

        SignalSpy {
            id: clickSpy
            target: button
            signalName: "clicked"
        }

        SignalSpy {
            id: hangupPainted
            target: hangup
            signalName: "painted"
        }

        function cleanup() {
            mouseRelease(button, button.width / 2, button.height / 2)
            button.enabled = true
            button.primary = false
            button.text = "A deliberately long action label"
            button.icon.source = ""
            field.enabled = true
            Theme.isDark = true
            clickSpy.clear()
            field.forceActiveFocus(Qt.OtherFocusReason)
        }

        function luminance(color) {
            var channels = [color.r, color.g, color.b].map(function(value) {
                return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4)
            })
            return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
        }

        function test_secondaryTextContrast() {
            for (var mode = 0; mode < 2; mode++) {
                Theme.isDark = mode === 0
                var foreground = luminance(Theme.muted)
                var surfaces = [Theme.bg0, Theme.bg1, Theme.bg2, Theme.bg3]
                for (var surface = 0; surface < surfaces.length; surface++) {
                    var background = luminance(surfaces[surface])
                    var ratio = (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05)
                    verify(ratio >= 4.5, "Secondary text contrast: " + ratio)
                }
            }
        }

        function test_iconLayout() {
            button.icon.source = Qt.resolvedUrl("../../qml/icons/invite.svg")
            var row = button.contentItem.children[0]
            var image = row.children[0]
            var label = row.children[1]
            tryCompare(image, "status", Image.Ready)
            // The glyph is tinted to the text colour, including on the light skin.
            // The file itself stays the baked grey.
            Theme.isDark = true
            compare(image.color, Theme.text)
            Theme.isDark = false
            compare(image.color, Theme.text)
            button.primary = true
            compare(image.color, Theme.textInv)
            button.primary = false
            tryVerify(function() { return label.x >= image.x + image.width })
            verify(label.x + label.width <= row.width)
            button.text = ""
            tryVerify(function() { return Math.abs(image.x + image.width / 2 - row.width / 2) < 1 })
        }

        function test_keyboardActivation() {
            button.forceActiveFocus(Qt.TabFocusReason)
            keyClick(Qt.Key_Space)
            compare(clickSpy.count, 1)
            button.enabled = false
            keyClick(Qt.Key_Space)
            compare(clickSpy.count, 1)
        }

        function test_verticalCentering() {
            var row = button.contentItem.children[0]
            var image = row.children[0]
            var label = row.children[1]
            compare(button.verticalPadding, 0)
            compare(button.topPadding, 0)
            compare(button.bottomPadding, 0)
            compare(button.height, Theme.controlHeight)
            button.text = "Invite"
            button.icon.source = Qt.resolvedUrl("../../qml/icons/invite.svg")
            tryCompare(image, "status", Image.Ready)
            tryVerify(function() {
                return Math.abs(label.mapToItem(button, 0, label.height / 2).y - button.height / 2) <= 1
            }, 1000, "Invite label must be vertically centered")
            verify(Math.abs(image.mapToItem(button, 0, image.height / 2).y - button.height / 2) <= 1,
                   "Invite icon must be vertically centered")
            button.text = "Accept invite"
            button.icon.source = ""
            tryVerify(function() {
                return Math.abs(label.mapToItem(button, 0, label.height / 2).y - button.height / 2) <= 1
            }, 1000, "Text-only label must be vertically centered")
            // Even if a layout stretches the control to title-bar height,
            // icon and label stay in the vertical center — not the bottom.
            button.height = Theme.titleBarHeight
            button.text = "Invite"
            button.icon.source = Qt.resolvedUrl("../../qml/icons/invite.svg")
            tryCompare(image, "status", Image.Ready)
            tryVerify(function() {
                return Math.abs(label.mapToItem(button, 0, label.height / 2).y - button.height / 2) <= 1
            }, 1000, "Invite label must stay centered in a title-bar-tall button")
            verify(Math.abs(image.mapToItem(button, 0, image.height / 2).y - button.height / 2) <= 1,
                   "Invite icon must stay centered in a title-bar-tall button")
            button.height = Theme.controlHeight
        }

        function test_buttonLabelFits() {
            var row = button.contentItem.children[0]
            var label = row.children[1]
            compare(row.width, button.availableWidth)
            verify(label.truncated)
            verify(label.x + label.width <= row.width)
            compare(button.height, Theme.controlHeight)
            compare(button.background.height, button.height)
        }

        function test_neutralPressedState() {
            mousePress(button, button.width / 2, button.height / 2)
            verify(button.down)
            tryCompare(button.background, "color", Theme.selectedFill())
        }

        function test_keyboardFocus() {
            button.forceActiveFocus(Qt.TabFocusReason)
            verify(button.visualFocus)
            compare(button.background.border.width, 2)
            tryCompare(button.background.border, "color", Theme.text)
            keyClick(Qt.Key_Tab)
            verify(field.activeFocus)
            compare(field.background.border.width, 2)
        }

        function test_fieldThemes() {
            for (var mode = 0; mode < 2; mode++) {
                Theme.isDark = mode === 0
                compare(field.background.color, Theme.bg2)
                // Material's floating placeholder is hidden; the hint drawn in
                // its place stays in the field, in the muted colour.
                compare(field.placeholderTextColor.a, 0)
                var hint = findChild(field, "placeholderHint")
                verify(hint !== null)
                compare(hint.color, Theme.muted)
                compare(hint.text, field.placeholderText)
                field.text = "typed"
                verify(!hint.visible)
                field.text = ""
                verify(hint.visible)
                compare(field.selectionColor, Theme.accent)
                field.enabled = false
                compare(field.background.color, Theme.bg1)
                compare(field.color, Theme.muted)
                field.enabled = true
            }
        }

        // getImageData asks for device pixels. Qt's canvas image is the item
        // size times the screen scale, so a logical point is scaled first.
        function hangupPixel(logicalX, logicalY) {
            var ratio = Screen.devicePixelRatio
            var sample = hangup.context.getImageData(
                Math.round(logicalX * ratio), Math.round(logicalY * ratio), 1, 1).data
            return { r: sample[0], g: sample[1], b: sample[2], a: sample[3] }
        }

        function test_hangupSquareFillsItsControl() {
            // The buffer Qt shows is the item. Growing it for the screen scale
            // turns on the unfinished canvas tiling and crops the mark.
            tryVerify(function() {
                return Math.abs(hangup.canvasSize.width - hangup.width) < 0.01
                    && Math.abs(hangup.canvasSize.height - hangup.height) < 0.01
                    && Math.abs(hangup.canvasWindow.width - hangup.width) < 0.01
                    && Math.abs(hangup.canvasWindow.height - hangup.height) < 0.01
                    && Math.abs(hangup.canvasWindow.x) < 0.01
                    && Math.abs(hangup.canvasWindow.y) < 0.01
            })
            hangupPainted.clear()
            hangup.requestPaint()
            tryVerify(function() { return hangupPainted.count > 0 && hangup.context })
            // Inside the corner margin (about 9% of the side) the fill is solid.
            var corner = hangupPixel(34, 34)
            var origin = hangupPixel(1, 1)
            verify(corner.a > 200,
                   "bottom-right of the square must stay filled, alpha " + corner.a
                   + " origin " + origin.a + " scale " + Screen.devicePixelRatio)
            verify(corner.r > 200 && corner.g < 80 && corner.b < 120,
                   "bottom-right must be the danger fill")
            var hole = hangupPixel(18, 18)
            verify(hole.a < 20, "center of the X must be open, alpha " + hole.a)
            var arm = hangupPixel(12, 12)
            verify(arm.a < 20, "an arm of the X must be open, alpha " + arm.a)
        }
    }
}
