pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import DoubleSlash.Client 1.0

Dialog {
    id: root
    required property var appBackend
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(620, parent.width - 32)
    height: Math.min(660, parent.height - 32)
    modal: true
    closePolicy: Popup.NoAutoClose
    title: "Devices & Backups"
    property string page: "choose"
    property string operation: ""
    property string filePath: ""
    property string errorText: ""
    property string token: ""
    property var summary: ({})
    property var profiles: []
    readonly property bool unlocked: appBackend.public_id !== ""
    readonly property bool busy: appBackend.backup_busy

    function run(request) {
        errorText = ""
        operation = request.cmd
        appBackend.backupCommand(JSON.stringify(request))
    }
    function localPath(url) {
        var text = url.toString()
        return decodeURIComponent(Qt.platform.os === "windows" ? text.slice(8) : text.slice(7))
    }
    onOpened: {
        page = "choose"; errorText = ""; token = ""; filePath = ""
        if (!unlocked) run({cmd: "profile.list"})
    }
    onClosed: { password.clear(); confirmPassword.clear(); token = "" }

    Connections {
        target: root.appBackend
        function onBackup_resultChanged() {
            if (!root.opened || !root.appBackend.backup_result) return
            var result
            try { result = JSON.parse(root.appBackend.backup_result) } catch (e) { return }
            if (!result.ok) { root.errorText = result.error; return }
            if (root.operation === "profile.list") { root.profiles = result.profiles; return }
            if (root.operation === "backup.cancel") return
            root.summary = result.summary || ({})
            password.clear(); confirmPassword.clear()
            if (root.operation === "backup.inspect") { root.token = result.token; root.page = "preview" }
            else root.page = "done"
        }
    }
    FileDialog {
        id: exportPicker
        title: "Save encrypted backup"
        fileMode: FileDialog.SaveFile
        nameFilters: ["DoubleSlash backup (*.dbackup)"]
        defaultSuffix: "dbackup"
        onAccepted: { root.filePath = root.localPath(selectedFile); root.page = "export" }
    }
    FileDialog {
        id: importPicker
        title: "Open encrypted backup"
        nameFilters: ["DoubleSlash backup (*.dbackup)", "All files (*)"]
        onAccepted: { root.filePath = root.localPath(selectedFile); root.page = "import" }
    }
    contentItem: ScrollView {
        clip: true
        ColumnLayout {
            width: root.availableWidth
            spacing: 16
            Label {
                Layout.fillWidth: true; wrapMode: Text.WordWrap
                text: "Your backup and its password can restore your identity, contacts, rooms and local history on desktop or Android. Keep the password somewhere safe; DoubleSlash cannot recover it."
            }
            ColumnLayout {
                visible: root.page === "choose"
                Layout.fillWidth: true
                Button { text: "Create encrypted backup"; enabled: root.unlocked && !root.busy; onClicked: exportPicker.open() }
                Button { text: "Restore from backup"; enabled: !root.unlocked && !root.busy; onClicked: importPicker.open() }
                Label {
                    Layout.fillWidth: true; wrapMode: Text.WordWrap
                    text: root.unlocked ? "To restore or switch identities, lock and quit, reopen DoubleSlash, then choose Restore backup on the unlock screen." : "Restoring creates a separate profile. Your existing identities remain available below."
                }
                Button { visible: root.unlocked; text: "Lock and quit"; enabled: !root.busy; onClicked: root.appBackend.lockIdentityAndQuit() }
                Label {
                    Layout.fillWidth: true; wrapMode: Text.WordWrap
                    text: "Move to your phone: create a backup, transfer the file, then restore it in the Android app. Quit this device before connecting the other. Simultaneous use and ongoing sync are not supported yet."
                }
                Repeater {
                    model: root.unlocked ? [] : root.profiles
                    Button {
                        required property var modelData
                        text: "Load " + (modelData.profile === "original" ? "original identity " : "restored identity ") + String(modelData.public_id).slice(0, 12) + "…"
                        enabled: !root.busy
                        onClicked: root.run({cmd: "profile.select", profile: modelData.profile})
                    }
                }
            }
            Label { visible: root.page === "export" || root.page === "import"; Layout.fillWidth: true; wrapMode: Text.WrapAnywhere; text: root.filePath }
            CheckBox { id: attachments; visible: root.page === "export"; checked: true; text: "Include available attachment files"; enabled: !root.busy }
            Label {
                visible: root.page === "preview"
                Layout.fillWidth: true; wrapMode: Text.WordWrap
                text: "Verified identity: " + (root.summary.public_id || "") + "\nMessages: " + (root.summary.messages || 0) + "\nAttachments: " + (root.summary.attachments || 0) + "\nMissing attachments: " + (root.summary.missing_attachments || 0) + "\n\nChoose a new passphrase for unlocking this device. Your original password and keyfile are not required."
            }
            TextField {
                id: password; Layout.fillWidth: true
                objectName: "backupPassword"
                visible: ["export", "import", "preview"].indexOf(root.page) >= 0
                enabled: !root.busy; echoMode: TextInput.Password
                placeholderText: root.page === "preview" ? "New local passphrase (at least 12 characters)" : "Backup password"
            }
            TextField {
                id: confirmPassword; Layout.fillWidth: true
                objectName: "backupConfirmation"
                visible: root.page === "export" || root.page === "preview"
                enabled: !root.busy; echoMode: TextInput.Password
                placeholderText: "Confirm passphrase"
            }
            Button {
                visible: root.page === "export" || root.page === "import" || root.page === "preview"
                objectName: "backupPrimaryAction"
                text: root.page === "export" ? "Create and verify backup" : root.page === "import" ? "Verify and preview" : "Restore into new profile"
                enabled: !root.busy && password.text.length > 0 && (root.page === "import" || (password.text.length >= 12 && password.text === confirmPassword.text))
                onClicked: {
                    if (root.page === "export") root.run({cmd: "backup.export", path: root.filePath, password: password.text, attachments: attachments.checked})
                    else if (root.page === "import") root.run({cmd: "backup.inspect", path: root.filePath, password: password.text})
                    else root.run({cmd: "backup.restore", token: root.token, local_password: password.text})
                }
            }
            Label {
                visible: root.page === "done"; Layout.fillWidth: true; wrapMode: Text.WordWrap
                text: root.operation === "backup.export"
                    ? "Backup saved and verified.\n" + root.summary.messages + " messages, " + root.summary.attachments + " attachments.\nMissing attachments: " + root.summary.missing_attachments + ".\nKeep this file and its password to restore on another device."
                    : "Profile selected. Close and reopen DoubleSlash to load it. Quit any other device using this identity before connecting."
            }
            BusyIndicator { running: root.busy; visible: running }
            Label { visible: root.busy; text: "Processing and verifying… Large backups can take a few minutes."; Layout.fillWidth: true; wrapMode: Text.WordWrap }
            Label { text: root.errorText; visible: text !== ""; Layout.fillWidth: true; wrapMode: Text.WordWrap; color: Theme.danger }
            Button {
                enabled: !root.busy
                text: root.page === "done" && root.operation !== "backup.export" ? "Close DoubleSlash" : "Close"
                onClicked: {
                    if (root.page === "done" && root.operation !== "backup.export") root.appBackend.lockIdentityAndQuit()
                    else { root.run({cmd: "backup.cancel"}); root.close() }
                }
            }
        }
    }
}
