import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

TextField {
    id: control

    color: enabled ? Theme.text : Theme.muted
    // Material floats its placeholder up onto the border once the field has
    // focus, over the label a form puts above the field. Its copy is hidden;
    // the one below stays where the text will go. placeholderText is still
    // set, so it remains the field's accessible description.
    placeholderTextColor: "transparent"
    selectionColor: Theme.accent
    selectedTextColor: Theme.textInv
    hoverEnabled: true
    font.pixelSize: Theme.fontSizeBody
    leftPadding: Theme.spacingMd
    rightPadding: Theme.spacingMd
    implicitHeight: Theme.controlHeight

    background: Rectangle {
        radius: Theme.radiusMd
        color: control.enabled ? Theme.bg2 : Theme.bg1
        border.width: control.activeFocus ? 2 : 1
        border.color: !control.enabled ? Theme.border
            : control.activeFocus ? Theme.accent
            : control.hovered ? Theme.muted : Theme.divider

        Behavior on border.color { ColorAnimation { duration: Theme.animFast } }
    }

    Text {
        objectName: "placeholderHint"
        // The whole height, centred as the input text is: the padded
        // interior of a 32 px field is shorter than a line.
        x: control.leftPadding
        width: control.width - control.leftPadding - control.rightPadding
        height: control.height
        visible: control.length === 0 && control.preeditText === ""
        text: control.placeholderText
        color: Theme.muted
        font: control.font
        elide: Text.ElideRight
        verticalAlignment: control.verticalAlignment
        horizontalAlignment: control.horizontalAlignment
    }
}