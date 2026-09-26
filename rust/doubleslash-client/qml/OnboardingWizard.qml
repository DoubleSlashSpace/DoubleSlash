// OnboardingWizard.qml - first-run setup for the native client.
//
// Pages: display name, then connect (paste an invite, usually a supernode's),
// then local integration on Windows only. Connectivity keeps the Settings
// defaults; port and firewall choices live in Settings, not here.

import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Dialog {
    id: root

    required property var settingsModel
    required property var appBackend

    property int step: 0
    readonly property bool isWindows: Qt.platform.os === "windows"
    readonly property int stepCount: isWindows ? 3 : 2
    property string generatedInvite: ""
    property bool copiedInvite: false
    property bool inviteSent: false

    title: "Welcome to DoubleSlash"
    modal: true
    closePolicy: Dialog.NoAutoClose
    width: 600
    height: 480
    padding: 0

    function saveHandle() {
        if (!root.settingsModel) return
        root.settingsModel.local_handle = handleField.text.trim()
        root.settingsModel.save()
    }

    function acceptInvite() {
        var invite = incomingInviteField.text.trim()
        if (invite.length === 0 || !root.appBackend) return
        root.appBackend.pasteInvite(invite)
        incomingInviteField.text = ""
        root.inviteSent = true
    }

    function finishWizard() {
        root.saveHandle()
        root.acceptInvite()

        if (root.isWindows && root.appBackend) {
            if (uriOption.checked) root.appBackend.registerUriScheme()
            if (shortcutOption.checked) root.appBackend.createDesktopShortcuts()
        }

        if (root.settingsModel) {
            root.settingsModel.onboarding_complete = true
            root.settingsModel.save()
        }

        root.close()
    }

    background: Rectangle {
        color: Theme.bg1
        radius: 0
        border.color: Theme.bg3
        border.width: 1
    }

    contentItem: ColumnLayout {
        spacing: 0

        Rectangle {
            Layout.fillWidth: true
            height: 64
            color: Theme.bg0

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.spacingLg
                anchors.rightMargin: Theme.spacingLg
                spacing: Theme.spacingMd

                Image {
                    source: "qrc:/qt/qml/DoubleSlash/Client/icons/logo.svg"
                    sourceSize.width: 32
                    sourceSize.height: 32
                    Layout.preferredWidth: 32
                    Layout.preferredHeight: 32
                    fillMode: Image.PreserveAspectFit
                }

                Text {
                    text: root.step === 0 ? "Welcome"
                        : root.step === 1 ? "Get connected"
                        : "Finishing touches"
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeTitle
                    font.bold: true
                    Layout.fillWidth: true
                }

                Text {
                    text: (root.step + 1) + " / " + root.stepCount
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: root.step

            // Step 0: Welcome and display name.
            ColumnLayout {
                spacing: Theme.spacingMd
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.margins: Theme.spacingXl

                Item { Layout.fillHeight: true }

                Avatar {
                    peerId: root.appBackend && root.appBackend.public_id
                        ? String(root.appBackend.public_id) : ""
                    size: 72
                    showRing: true
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    text: "Private by default"
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody + 5
                    font.bold: true
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    text: "Your identity stays on this device. You add people with invite links."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeBody
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                    Layout.maximumWidth: 440
                    Layout.alignment: Qt.AlignHCenter
                }

                ColumnLayout {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: 320
                    spacing: Theme.spacingXs

                    Text {
                        text: "Display name"
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeCaption
                        font.bold: true
                    }

                    StyledTextField {
                        id: handleField
                        Layout.fillWidth: true
                        placeholderText: "What should people call you?"
                        maximumLength: 64
                        focus: true
                        text: root.settingsModel ? root.settingsModel.local_handle : ""
                        onTextChanged: if (root.settingsModel) root.settingsModel.local_handle = text
                        Keys.onReturnPressed: if (text.trim().length > 0) nextBtn.clicked()
                    }
                }

                Item { Layout.fillHeight: true }
            }

            // Step 1: Get connected.
            ColumnLayout {
                spacing: Theme.spacingMd
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.margins: Theme.spacingXl

                Text {
                    text: "Paste the invite link you were given. Most people start with a supernode's invite, which gives you rooms and keeps you reachable."
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSm

                    StyledTextField {
                        id: incomingInviteField
                        Layout.fillWidth: true
                        placeholderText: "https://doubleslash.space/i#..."
                        onTextChanged: if (text.length > 0) root.inviteSent = false
                        Keys.onReturnPressed: root.acceptInvite()
                    }

                    StyledButton {
                        text: "Join"
                        primary: true
                        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/connect.svg"
                        enabled: incomingInviteField.text.trim().length > 0
                        onClicked: root.acceptInvite()
                    }
                }

                Text {
                    visible: root.inviteSent
                    text: "Invite accepted. You can add more later from the sidebar."
                    color: Theme.online
                    font.pixelSize: Theme.fontSizeCaption
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: Theme.spacingSm
                    height: 1
                    color: Theme.bg3
                }

                Text {
                    text: "Or invite a friend"
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSm

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: Theme.touchTarget
                        color: Theme.bg2
                        border.color: Theme.bg3
                        border.width: 1

                        Text {
                            anchors.fill: parent
                            anchors.leftMargin: Theme.spacingSm
                            anchors.rightMargin: Theme.spacingSm
                            verticalAlignment: Text.AlignVCenter
                            color: root.generatedInvite !== "" ? Theme.text : Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                            text: root.generatedInvite !== "" ? root.generatedInvite : "Create a link to send them"
                            elide: Text.ElideMiddle
                        }
                    }

                    StyledButton {
                        text: root.generatedInvite === "" ? "Create"
                            : root.copiedInvite ? "Copied" : "Copy"
                        icon.source: root.generatedInvite === ""
                            ? "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
                            : "qrc:/qt/qml/DoubleSlash/Client/icons/clipboard.svg"
                        success: root.copiedInvite
                        onClicked: {
                            if (!root.appBackend) return
                            if (root.generatedInvite === "") {
                                root.generatedInvite = root.appBackend.generateInvite()
                                root.copiedInvite = false
                            } else {
                                root.appBackend.copyToClipboard(root.generatedInvite)
                                root.copiedInvite = true
                            }
                        }
                    }
                }

                Item { Layout.fillHeight: true }

                Text {
                    text: "No invite yet? Skip this; you can paste one any time."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }

            // Step 2 (Windows only): local integration.
            ColumnLayout {
                spacing: Theme.spacingMd
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.margins: Theme.spacingXl

                Text {
                    text: "You can change these later in Settings."
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeBody
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }

                CheckBox {
                    id: uriOption
                    text: "Open DoubleSlash invite links in the app"
                    checked: root.isWindows
                    font.pixelSize: Theme.fontSizeBody
                    Material.foreground: Theme.text
                }

                CheckBox {
                    id: shortcutOption
                    text: "Create Desktop and Start Menu shortcuts"
                    checked: root.isWindows
                    font.pixelSize: Theme.fontSizeBody
                    Material.foreground: Theme.text
                }

                Item { Layout.fillHeight: true }
            }
        }
    }

    footer: ColumnLayout {
        spacing: Theme.spacingSm

        Row {
            Layout.alignment: Qt.AlignHCenter
            spacing: Theme.spacingSm

            Repeater {
                model: root.stepCount

                Rectangle {
                    width: index === root.step ? 22 : 8
                    height: 8
                    radius: 0
                    color: index === root.step ? Theme.accent : Theme.bg3

                    Behavior on width { NumberAnimation { duration: Theme.animFast } }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.spacingLg
            Layout.rightMargin: Theme.spacingLg
            Layout.bottomMargin: Theme.spacingMd
            spacing: Theme.spacingSm

            StyledButton {
                text: "Back"
                visible: root.step > 0
                onClicked: root.step = Math.max(0, root.step - 1)
            }

            Item { Layout.fillWidth: true }

            StyledButton {
                id: nextBtn
                text: root.step === root.stepCount - 1 ? "Finish"
                    : root.step === 1 && !root.inviteSent && root.generatedInvite === ""
                        && incomingInviteField.text.trim().length === 0 ? "Skip"
                    : "Next"
                primary: true
                enabled: root.step !== 0 || handleField.text.trim().length > 0
                onClicked: {
                    if (root.step === 0) root.saveHandle()
                    if (root.step === 1) root.acceptInvite()
                    if (root.step < root.stepCount - 1) {
                        root.step += 1
                    } else {
                        root.finishWizard()
                    }
                }
            }
        }
    }
}
