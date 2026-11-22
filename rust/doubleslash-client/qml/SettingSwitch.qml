import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Item {
    id: root

    property string title: ""
    property string description: ""
    property bool checked: false
    property bool enabledState: true
    signal changed(bool checked)

    Layout.fillWidth: true
    implicitHeight: Math.max(Theme.touchTarget, row.implicitHeight)
    enabled: enabledState
    opacity: enabled ? 1.0 : 0.5
    Accessible.role: Accessible.CheckBox
    Accessible.name: root.title
    Accessible.description: root.description

    RowLayout {
        id: row
        anchors.fill: parent
        spacing: Theme.spacingMd

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            Label {
                Layout.fillWidth: true
                text: root.title
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                // Wrapped, not elided: with large text or a narrow window an
                // ellipsis hid what the switch does.
                wrapMode: Text.WordWrap
            }

            Label {
                visible: root.description.length > 0
                Layout.fillWidth: true
                text: root.description
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                wrapMode: Text.WordWrap
            }
        }

        SquareSwitch {
            checked: root.checked
            enabled: root.enabled
            // The switch is what takes focus, so it carries the name a
            // screen reader announces.
            Accessible.name: root.title
            Accessible.description: root.description
            Layout.alignment: Qt.AlignVCenter
            onToggled: root.changed(checked)
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: root.enabled
        cursorShape: Qt.PointingHandCursor
        onClicked: root.changed(!root.checked)
    }
}
