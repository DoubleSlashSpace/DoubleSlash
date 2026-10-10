import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Rectangle {
    id: root

    property int currentIndex: 0
    /// Whether the live settings differ from the file. Drives the Save button's
    /// colour and whether it is clickable at all.
    property bool dirty: false
    signal sectionActivated(int index)
    signal saveRequested()
    /// Leave Settings for the page it was opened from. The title-bar avatar
    /// is the way in; there is no Chat/Settings switch to toggle back.
    signal backRequested()

    Layout.preferredWidth: Theme.sidebarWidth
    Layout.fillHeight: true
    color: Theme.bg1

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.fillWidth: true
            height: Theme.touchTarget
            color: Theme.bg2

            IconButton {
                id: backButton
                anchors.left: parent.left
                anchors.leftMargin: Theme.spacingXs
                anchors.verticalCenter: parent.verticalCenter
                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/chevron.svg"
                icon.width: 14
                icon.height: 14
                icon.color: Theme.text
                // The chevron points right; flipped, it reads as "back".
                rotation: 180
                flat: true
                onClicked: root.backRequested()
                ToolTip.text: qsTr("Back")
                ToolTip.visible: hovered
                Accessible.name: qsTr("Back")
            }

            Label {
                anchors.centerIn: parent
                text: qsTr("Settings")
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                font.bold: true
            }
        }

        ListView {
            id: navList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: ListModel {
                ListElement { label: "Audio"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/headphone.svg" }
                ListElement { label: "Video"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/video.svg" }
                ListElement { label: "Identity"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/person.svg" }
                ListElement { label: "General"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/gear.svg" }
                ListElement { label: "AI"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/lightning.svg" }
                ListElement { label: "Network"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/globe.svg" }
                ListElement { label: "Security"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/lock.svg" }
                ListElement { label: "Privacy"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/key.svg" }
                ListElement { label: "Diagnostics"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/logs.svg" }
                ListElement { label: "About"; icon: "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg" }
            }

            delegate: Item {
                id: navItem
                width: navList.width
                height: Theme.touchTarget
                property bool highlighted: index === root.currentIndex
                property bool hovered: navMouse.containsMouse

                Rectangle {
                    anchors.fill: parent
                    color: navItem.hovered ? Theme.bg3 : "transparent"

                    Rectangle {
                        anchors.fill: parent
                        color: Theme.selectedFill()
                        visible: navItem.highlighted
                    }

                    Rectangle {
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        width: 3
                        height: parent.height - Theme.spacingMd
                        radius: Theme.radiusSm
                        color: Theme.accent
                        visible: navItem.highlighted
                    }

                    Behavior on color { ColorAnimation { duration: Theme.animFast } }
                }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.spacingLg
                    anchors.rightMargin: Theme.spacingSm
                    spacing: Theme.spacingSm

                    Image {
                        source: model.icon
                        sourceSize.width: 20
                        sourceSize.height: 20
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 20
                        fillMode: Image.PreserveAspectFit
                        opacity: navItem.highlighted ? 1.0 : 0.72
                    }

                    Label {
                        Layout.fillWidth: true
                        text: model.label
                        color: navItem.highlighted ? Theme.text : Theme.muted
                        font.pixelSize: Theme.fontSizeBody
                        font.bold: navItem.highlighted
                        verticalAlignment: Text.AlignVCenter
                        elide: Text.ElideRight
                    }
                }

                MouseArea {
                    id: navMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.sectionActivated(index)
                }

                Accessible.role: Accessible.Button
                Accessible.name: model.label
            }
        }

        Rectangle {
            Layout.fillWidth: true
            height: 1
            color: Theme.bg3
        }

        Rectangle {
            Layout.fillWidth: true
            height: 72
            color: Theme.bg0

            StyledButton {
                anchors.fill: parent
                anchors.margins: Theme.spacingMd
                // Red only while there is something to save, and unclickable
                // once there is not — the state is the point of the button, so
                // it says which of the two it is rather than looking the same
                // either way.
                danger: root.dirty
                enabled: root.dirty
                compact: false
                font.pixelSize: Theme.fontSizeTitle
                font.bold: root.dirty
                text: root.dirty ? "Save Settings" : "Settings Saved"
                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/save.svg"
                onClicked: root.saveRequested()

                Accessible.description: root.dirty
                    ? "There are unsaved settings"
                    : "All settings are saved"
            }
        }
    }
}
