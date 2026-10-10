import QtQuick
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root

    signal sendMessage(string message)
    signal sendFile(string fileUrl)
    signal composing(bool active)
    signal askAi(string prompt)

    property string targetName: ""
    property bool enabledForTarget: true
    property bool fileTransferEnabled: true
    property string fileTransferTooltip: "Attach file"
    property bool aiEnabled: false
    property bool aiStreaming: false

    Layout.fillWidth: true
    implicitHeight: composerLayout.implicitHeight + Theme.barPadding * 2
    color: Theme.bg3
    radius: Theme.radiusMd

    function selectedText() {
        return composer.selectedText || ""
    }

    function wrapSelection(prefix, suffix) {
        var start = composer.selectionStart
        var end = composer.selectionEnd
        var before = composer.text.slice(0, start)
        var middle = composer.text.slice(start, end)
        var after = composer.text.slice(end)
        if (middle.length === 0) middle = "text"
        composer.text = before + prefix + middle + suffix + after
        composer.select(start + prefix.length, start + prefix.length + middle.length)
        composer.forceActiveFocus()
    }

    function insertLink() {
        var start = composer.selectionStart
        var end = composer.selectionEnd
        var label = composer.text.slice(start, end) || "link"
        var insert = label + " https://"
        composer.text = composer.text.slice(0, start) + insert + composer.text.slice(end)
        composer.cursorPosition = start + insert.length
        composer.forceActiveFocus()
    }

    function submit() {
        var value = composer.text.trim()
        if (value.length === 0 || !root.enabledForTarget) return
        root.sendMessage(value)
        composer.clear()
    }

    FileDialog {
        id: fileDialog
        title: "Attach File"
        onAccepted: root.sendFile(selectedFile.toString())
    }

    /// One formatting control. Square at the control height, so it grows with
    /// the font-size setting along with its label.
    component FormatButton: IconButton {
        property string tip: ""
        enabled: root.enabledForTarget
        flat: true
        implicitWidth: Theme.barTopRowHeight
        implicitHeight: Theme.barTopRowHeight
        padding: 0
        font.pixelSize: Theme.fontSizeTitle
        icon.width: 18
        icon.height: 18
        icon.color: Theme.muted
        Material.foreground: Theme.muted
        Accessible.name: tip
        ToolTip.text: tip
        ToolTip.visible: hovered
    }

    /// The input row's actions: attach on the left, send on the right.
    component ActionButton: IconButton {
        property string tip: ""
        flat: true
        implicitWidth: Theme.barActionSize
        implicitHeight: Theme.barActionSize
        padding: 0
        icon.width: 20
        icon.height: 20
        Layout.alignment: Qt.AlignBottom
        Accessible.name: tip
        ToolTip.text: tip
        ToolTip.visible: hovered
    }

    ColumnLayout {
        id: composerLayout
        anchors.fill: parent
        anchors.margins: Theme.barPadding
        spacing: Theme.barRowGap

        RowLayout {
            Layout.fillWidth: true
            spacing: 2

            FormatButton {
                text: "B"
                font.bold: true
                tip: "Bold"
                onClicked: root.wrapSelection("**", "**")
            }
            FormatButton {
                text: "I"
                font.italic: true
                tip: "Italic"
                onClicked: root.wrapSelection("*", "*")
            }
            FormatButton {
                text: "U"
                font.underline: true
                tip: "Underline"
                onClicked: root.wrapSelection("__", "__")
            }
            FormatButton {
                text: "</>"
                implicitWidth: Theme.barTopRowHeight + Theme.spacingSm
                font.pixelSize: Theme.fontSizeBody
                font.bold: true
                tip: "Code"
                onClicked: root.wrapSelection("`", "`")
            }

            Rectangle {
                Layout.preferredWidth: 1
                Layout.preferredHeight: Theme.barTopRowHeight - Theme.spacingSm * 2
                Layout.leftMargin: Theme.spacingXs
                Layout.rightMargin: Theme.spacingXs
                color: Theme.muted
                opacity: 0.35
            }

            FormatButton {
                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/chain.svg"
                tip: "Link"
                onClicked: root.insertLink()
            }

            Item { Layout.fillWidth: true }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            ActionButton {
                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/attach.svg"
                icon.color: enabled ? Theme.text : Theme.muted
                enabled: root.enabledForTarget && root.fileTransferEnabled
                tip: root.fileTransferEnabled ? root.fileTransferTooltip : "Room file transfer is not available yet"
                onClicked: fileDialog.open()
            }

            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(92, Math.max(Theme.barActionSize, composer.implicitHeight))

                TextArea {
                    id: composer
                    anchors.fill: parent
                    enabled: root.enabledForTarget
                    background: Item {}
                    // The attach button already sets the text off from the
                    // edge; Material's own inset left a wide gap after it.
                    leftPadding: Theme.spacingXs
                    rightPadding: Theme.spacingXs
                    color: Theme.text
                    wrapMode: TextEdit.Wrap
                    selectByMouse: true
                    onTextChanged: root.composing(text.length > 0)
                    Keys.onPressed: function(event) {
                        if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                                && !(event.modifiers & Qt.ShiftModifier)) {
                            root.submit()
                            event.accepted = true
                        }
                    }
                }

                Text {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: composer.leftPadding
                    visible: composer.text.length === 0
                    text: root.enabledForTarget ? "Message..." : "Select a chat"
                    color: Theme.muted
                    font.pixelSize: composer.font.pixelSize
                    elide: Text.ElideRight
                }
            }

            ActionButton {
                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/send.svg"
                icon.color: enabled ? Theme.accent : Theme.muted
                enabled: composer.text.trim() !== "" && root.enabledForTarget
                tip: "Send"
                onClicked: root.submit()
            }

            Button {
                Layout.alignment: Qt.AlignBottom
                text: root.aiStreaming ? "..." : "AI"
                visible: root.aiEnabled
                enabled: !root.aiStreaming
                    && composer.text.trim() !== ""
                    && root.enabledForTarget
                flat: true
                Material.foreground: Theme.accent
                ToolTip.text: "Ask local AI"
                ToolTip.visible: hovered
                onClicked: {
                    var value = composer.text.trim()
                    if (value.length === 0) return
                    root.askAi(value)
                    composer.clear()
                    root.composing(false)
                }
            }
        }
    }

    DropArea {
        anchors.fill: parent
        keys: ["text/uri-list"]
        enabled: root.enabledForTarget && root.fileTransferEnabled
        onDropped: function(drop) {
            if (drop.hasUrls && drop.urls.length > 0) {
                root.sendFile(drop.urls[0].toString())
            }
        }
    }
}
