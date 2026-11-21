import QtQuick
import QtQuick.Layouts
import DoubleSlash.Client 1.0

ColumnLayout {
    id: root

    property string iconSource: ""
    property int iconSize: 32
    property string title: ""
    property string subtitle: ""

    spacing: Theme.spacingSm

    visible: title.length > 0 || subtitle.length > 0

    Image {
        visible: root.iconSource !== ""
        source: Icons.resolve(root.iconSource)
        sourceSize.width: Theme.iconPx(root.iconSize)
        sourceSize.height: Theme.iconPx(root.iconSize)
        Layout.preferredWidth: Theme.iconPx(root.iconSize)
        Layout.preferredHeight: Theme.iconPx(root.iconSize)
        Layout.alignment: Qt.AlignHCenter
        fillMode: Image.PreserveAspectFit
        opacity: 0.72
    }

    Text {
        visible: root.title !== ""
        text: root.title
        horizontalAlignment: Text.AlignHCenter
        color: Theme.text
        font.pixelSize: Theme.fontSizeBody
        font.bold: true
        Layout.fillWidth: true
    }

    Text {
        visible: root.subtitle !== ""
        text: root.subtitle
        horizontalAlignment: Text.AlignHCenter
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        wrapMode: Text.WordWrap
        lineHeight: 1.3
        Layout.fillWidth: true
    }
}