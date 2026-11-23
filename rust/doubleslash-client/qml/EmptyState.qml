import QtQuick
import QtQuick.Layouts
import DoubleSlash.Client 1.0

ColumnLayout {
    id: root

    property string iconSource: ""
    property int iconSize: 32
    property string title: ""
    property string subtitle: ""
    /// When above zero, the empty state keeps within this height: with large
    /// text in a short panel the icon goes first, then the subtitle is cut
    /// short with an ellipsis, instead of running off the window.
    property real availableHeight: 0

    readonly property real _iconBlock: Theme.iconPx(iconSize) + spacing
    readonly property real _titleBlock: title !== "" ? titleText.implicitHeight + spacing : 0
    readonly property real _subtitleLine: Math.ceil(Theme.fontSizeCaption * 1.34 * subtitleText.lineHeight)
    readonly property real _fullHeight: _iconBlock + _titleBlock + subtitleText.implicitHeight
    readonly property bool _showIcon: availableHeight <= 0 || _fullHeight <= availableHeight

    spacing: Theme.spacingSm

    visible: title.length > 0 || subtitle.length > 0

    Glyph {
        visible: root.iconSource !== "" && root._showIcon
        source: Icons.resolve(root.iconSource)
        sourceSize.width: Theme.iconPx(root.iconSize)
        sourceSize.height: Theme.iconPx(root.iconSize)
        Layout.preferredWidth: Theme.iconPx(root.iconSize)
        Layout.preferredHeight: Theme.iconPx(root.iconSize)
        Layout.alignment: Qt.AlignHCenter
        opacity: 0.72
    }

    Text {
        id: titleText
        visible: root.title !== ""
        text: root.title
        horizontalAlignment: Text.AlignHCenter
        color: Theme.text
        font.pixelSize: Theme.fontSizeBody
        font.bold: true
        // Wraps like the subtitle; the panels give an empty state a fixed
        // width, which large text outgrows.
        wrapMode: Text.WordWrap
        Layout.fillWidth: true
    }

    Text {
        id: subtitleText
        visible: root.subtitle !== ""
        text: root.subtitle
        horizontalAlignment: Text.AlignHCenter
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        wrapMode: Text.WordWrap
        lineHeight: 1.3
        // Measured without the icon, which is the first thing dropped.
        maximumLineCount: root.availableHeight > 0
            ? Math.max(1, Math.floor((root.availableHeight - root._titleBlock) / root._subtitleLine))
            : 1000
        elide: Text.ElideRight
        Layout.fillWidth: true
    }
}
