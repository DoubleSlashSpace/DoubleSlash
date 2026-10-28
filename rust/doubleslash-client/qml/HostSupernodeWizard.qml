// HostSupernodeWizard.qml - set up a supernode on the user's own server, or
// add a server to one they already manage.
//
// Notice -> server -> host key -> choices -> live checklist -> done. Every
// server step is idempotent, so a failure is retried from the top with the
// same answers. The password is sent once with the setup request and cleared
// from the form when the dialog closes.
pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Material
import QtQuick.Layouts
import DoubleSlash.Client 1.0

Dialog {
    id: root
    required property var appBackend
    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(640, parent.width - 32)
    height: Math.min(700, parent.height - 32)
    modal: true
    closePolicy: Popup.NoAutoClose
    title: root.addToNodeId !== "" ? "Add a server to " + root.addToNodeName : "Host a supernode"

    // The app's own surfaces, like the other dialogs: square, themed, and a
    // dark scrim behind instead of the style's default.
    Overlay.modal: Rectangle {
        color: Theme.withAlpha(Theme.overlayScrim, 0.72)
    }
    background: Rectangle {
        color: Theme.bg1
        border.color: Theme.bg3
        border.width: 1
    }
    header: Label {
        text: root.title
        color: Theme.text
        font.pixelSize: Theme.fontSizeDialog
        font.bold: true
        padding: Theme.spacingLg
        elide: Text.ElideRight
    }

    // Empty for a new node; a managed node's id to add a cluster member.
    property string addToNodeId: ""
    property string addToNodeName: ""

    property string page: "notice"
    property string errorText: ""
    property string requestId: ""
    property string hostKey: ""
    property string fingerprint: ""
    property var steps: []
    property var outcome: ({})

    property string access: "client-key-only"
    property string updateMode: "server-timer"

    readonly property var adminState: {
        try { return JSON.parse(appBackend.supernode_admin_state) } catch (e) { return ({}) }
    }
    readonly property bool busy: appBackend.supernode_admin_busy

    function openFor(nodeId, nodeName) {
        addToNodeId = nodeId || ""
        addToNodeName = nodeName || ""
        open()
    }

    function send(request) {
        requestId = request.op + "-" + Date.now()
        request.request_id = requestId
        errorText = ""
        appBackend.supernodeCommand(JSON.stringify(request))
    }

    function serverRequest() {
        var request = {
            op: addToNodeId !== "" ? "add_server" : "deploy",
            name: nameField.text.trim(),
            host: hostField.text.trim(),
            user: userField.text.trim() || "root",
            port: parseInt(portField.text) || 22,
            public_host: publicField.text.trim(),
            host_key: hostKey,
            password: passwordField.text,
            access: access,
            hardening: {
                firewall: firewallSwitch.checked,
                automatic_security_updates: updatesSwitch.checked,
                automatic_reboot: rebootSwitch.checked,
                fail2ban: fail2banSwitch.checked
            }
        }
        if (addToNodeId !== "") request.node_id = addToNodeId
        else request.update_mode = updateMode
        return request
    }

    function startSetup() {
        steps = []
        outcome = ({})
        page = "run"
        send(serverRequest())
    }

    onOpened: {
        errorText = ""; hostKey = ""; fingerprint = ""; steps = []
        page = adminState.notice_accepted ? "server" : "notice"
        send({ op: "state" })
    }
    onClosed: passwordField.clear()

    Connections {
        target: root.appBackend
        function onSupernodeAdminEvent(event_json) {
            var event
            try { event = JSON.parse(event_json) } catch (e) { return }
            if (!root.opened || event.request_id !== root.requestId) return
            if (event.kind === "plan") {
                root.steps = event.steps.map(function(title) {
                    return { title: title, state: "pending", lines: [], error: "" }
                })
                return
            }
            if (event.kind === "step") {
                var copy = root.steps.slice()
                while (copy.length <= event.index)
                    copy.push({ title: event.title, state: "pending", lines: [], error: "" })
                copy[event.index] = {
                    title: event.title,
                    state: event.state,
                    lines: event.lines || [],
                    error: event.error || ""
                }
                root.steps = copy
                return
            }
            if (event.kind !== "result") return
            if (!event.ok) { root.errorText = event.error; return }
            if (event.op === "accept_notice") root.page = "server"
            else if (event.op === "fetch_host_key") {
                root.hostKey = event.value.host_key
                root.fingerprint = event.value.fingerprint
                root.page = "hostkey"
            } else if (event.op === "deploy" || event.op === "add_server") {
                root.outcome = event.value
                passwordField.clear()
                root.page = "done"
            }
        }
    }

    // A square, selectable choice: the desktop draws no radio circles.
    component ChoiceTile: Rectangle {
        id: tile
        property string title: ""
        property string description: ""
        property bool selected: false
        signal picked()
        Layout.fillWidth: true
        implicitHeight: tileColumn.implicitHeight + Theme.spacingMd * 2
        color: tile.selected ? Theme.selectedFill() : (tileMouse.containsMouse ? Theme.bg3 : Theme.bg2)
        border.width: 1
        border.color: tile.selected ? Theme.accent : Theme.divider
        ColumnLayout {
            id: tileColumn
            anchors.fill: parent
            anchors.margins: Theme.spacingMd
            spacing: 2
            Label {
                Layout.fillWidth: true
                text: tile.title
                color: Theme.text
                font.pixelSize: Theme.fontSizeBody
                font.bold: tile.selected
                wrapMode: Text.WordWrap
            }
            Label {
                Layout.fillWidth: true
                visible: tile.description !== ""
                text: tile.description
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                wrapMode: Text.WordWrap
            }
        }
        MouseArea {
            id: tileMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: tile.picked()
        }
    }

    component FieldLabel: Label {
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
    }

    contentItem: ScrollView {
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            width: root.availableWidth
            spacing: Theme.spacingLg

            // ── Responsibility notice ────────────────────────────────────
            ColumnLayout {
                visible: root.page === "notice"
                Layout.fillWidth: true
                spacing: Theme.spacingMd

                Label {
                    Layout.fillWidth: true
                    text: "Before you start"
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeTitle
                    font.bold: true
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    text: root.adminState.notice || ""
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    text: "You need a Linux server running Debian 11 or later, or Ubuntu 20.04 or "
                        + "later, that you can log in to as root (or as a user with passwordless "
                        + "sudo). A fresh server used only for DoubleSlash is best: the firewall "
                        + "blocks anything else that is listening."
                }
                SettingSwitch {
                    id: noticeSwitch
                    Layout.fillWidth: true
                    title: "I understand that the server is my responsibility"
                    onChanged: function(value) { noticeSwitch.checked = value }
                }
            }

            // ── Server ───────────────────────────────────────────────────
            GridLayout {
                visible: root.page === "server"
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.spacingMd
                rowSpacing: Theme.spacingSm

                FieldLabel { text: "Name"; visible: root.addToNodeId === "" }
                StyledTextField {
                    id: nameField
                    visible: root.addToNodeId === ""
                    Layout.fillWidth: true
                    placeholderText: "My supernode"
                }
                FieldLabel { text: "Server address" }
                StyledTextField {
                    id: hostField
                    Layout.fillWidth: true
                    placeholderText: "203.0.113.10 or node.example.com"
                }
                FieldLabel { text: "SSH port" }
                StyledTextField {
                    id: portField
                    Layout.preferredWidth: 100
                    text: "22"
                    validator: IntValidator { bottom: 1; top: 65535 }
                }
                FieldLabel { text: "Log in as" }
                StyledTextField {
                    id: userField
                    Layout.fillWidth: true
                    text: "root"
                }
                FieldLabel { text: "Password" }
                StyledTextField {
                    id: passwordField
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: "Used once during setup, never saved"
                }
                FieldLabel { text: "Public address" }
                StyledTextField {
                    id: publicField
                    Layout.fillWidth: true
                    placeholderText: "Same as the server address"
                }
                Label {
                    Layout.columnSpan: 2
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    text: "Your hosting provider shows the address and root password when you "
                        + "create the server. Leave the password empty if you log in with your "
                        + "own SSH key."
                }
            }

            // ── Host key ─────────────────────────────────────────────────
            ColumnLayout {
                visible: root.page === "hostkey"
                Layout.fillWidth: true
                spacing: Theme.spacingMd

                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    text: "The server identified itself with this key fingerprint:"
                }
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: fingerprintLabel.implicitHeight + Theme.spacingMd * 2
                    color: Theme.bg2
                    border.width: 1
                    border.color: Theme.divider
                    Label {
                        id: fingerprintLabel
                        anchors.fill: parent
                        anchors.margins: Theme.spacingMd
                        text: root.fingerprint
                        color: Theme.text
                        font.family: "monospace"
                        font.pixelSize: Theme.fontSizeBody
                        wrapMode: Text.WrapAnywhere
                    }
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    text: "If your provider shows the server's SSH fingerprint (often in its "
                        + "console), it should match. DoubleSlash remembers this key and will "
                        + "refuse to connect if the server ever presents a different one."
                }
            }

            // ── Choices ──────────────────────────────────────────────────
            ColumnLayout {
                visible: root.page === "options"
                Layout.fillWidth: true
                spacing: Theme.spacingSm

                SettingsSectionHeader { title: "SSH access" }
                ChoiceTile {
                    title: "This device's key, password login off (recommended)"
                    description: "DoubleSlash installs its own SSH key, checks it works, then "
                        + "turns password login off. If you lose this device and its backup, "
                        + "you will need your provider's console to get back in."
                    selected: root.access === "client-key-only"
                    onPicked: root.access = "client-key-only"
                }
                ChoiceTile {
                    title: "This device's key, password login left on"
                    description: "DoubleSlash installs its key but leaves SSH settings alone."
                    selected: root.access === "client-key"
                    onPicked: root.access = "client-key"
                }
                ChoiceTile {
                    title: "My own SSH key"
                    description: "Use the key in your ~/.ssh folder. Nothing is installed and "
                        + "SSH settings are left alone. The key must not need a passphrase."
                    selected: root.access === "own-key"
                    onPicked: root.access = "own-key"
                }

                SettingsSectionHeader {
                    title: "Supernode updates"
                    visible: root.addToNodeId === ""
                }
                ChoiceTile {
                    visible: root.addToNodeId === ""
                    title: "Automatically on the server (recommended)"
                    description: "The server checks for a new nightly build every day, installs "
                        + "it, and rolls back if the node fails to start."
                    selected: root.updateMode === "server-timer"
                    onPicked: root.updateMode = "server-timer"
                }
                ChoiceTile {
                    visible: root.addToNodeId === ""
                    title: "Only when I choose Update"
                    description: "Nothing changes until you update from Settings. An unattended "
                        + "server falls behind."
                    selected: root.updateMode === "client"
                    onPicked: root.updateMode = "client"
                }

                SettingsSectionHeader { title: "Server security" }
                SettingSwitch {
                    id: firewallSwitch
                    Layout.fillWidth: true
                    title: "Firewall"
                    description: "Block incoming connections except SSH and the supernode's ports."
                    checked: true
                    onChanged: function(value) { firewallSwitch.checked = value }
                }
                SettingSwitch {
                    id: updatesSwitch
                    Layout.fillWidth: true
                    title: "Automatic security updates"
                    description: "Install the operating system's security updates daily."
                    checked: true
                    onChanged: function(value) { updatesSwitch.checked = value }
                }
                SettingSwitch {
                    id: rebootSwitch
                    Layout.fillWidth: true
                    title: "Restart for updates at 04:00"
                    description: "Restart the server when an update needs it. The node is "
                        + "offline for about a minute."
                    checked: true
                    enabledState: updatesSwitch.checked
                    onChanged: function(value) { rebootSwitch.checked = value }
                }
                SettingSwitch {
                    id: fail2banSwitch
                    Layout.fillWidth: true
                    title: "Block repeated failed logins"
                    description: "Ban addresses that keep failing to log in over SSH."
                    checked: true
                    onChanged: function(value) { fail2banSwitch.checked = value }
                }
            }

            // ── Live checklist ───────────────────────────────────────────
            ColumnLayout {
                visible: root.page === "run"
                Layout.fillWidth: true
                spacing: Theme.spacingSm

                Label {
                    Layout.fillWidth: true
                    visible: root.steps.length === 0 && root.errorText === ""
                    text: "Connecting…"
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeBody
                }

                Repeater {
                    model: root.steps
                    delegate: RowLayout {
                        id: stepRow
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: Theme.spacingSm

                        Item {
                            Layout.preferredWidth: 18
                            Layout.preferredHeight: 18
                            Layout.alignment: Qt.AlignTop
                            SquareBusyIndicator {
                                anchors.fill: parent
                                running: stepRow.modelData.state === "running"
                                visible: running
                            }
                            Image {
                                anchors.fill: parent
                                visible: stepRow.modelData.state === "done"
                                    || stepRow.modelData.state === "failed"
                                source: stepRow.modelData.state === "done"
                                    ? "qrc:/qt/qml/DoubleSlash/Client/icons/check.svg"
                                    : "qrc:/qt/qml/DoubleSlash/Client/icons/x-circle.svg"
                                sourceSize: Qt.size(18, 18)
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Label {
                                Layout.fillWidth: true
                                text: stepRow.modelData.title
                                wrapMode: Text.WordWrap
                                font.pixelSize: Theme.fontSizeBody
                                color: stepRow.modelData.state === "pending" ? Theme.muted
                                    : stepRow.modelData.state === "failed" ? Theme.danger
                                    : Theme.text
                            }
                            Repeater {
                                model: stepRow.modelData.lines
                                delegate: Label {
                                    required property string modelData
                                    Layout.fillWidth: true
                                    text: modelData
                                    wrapMode: Text.WordWrap
                                    color: Theme.muted
                                    font.pixelSize: Theme.fontSizeCaption
                                }
                            }
                        }
                    }
                }
            }

            // ── Done ─────────────────────────────────────────────────────
            ColumnLayout {
                visible: root.page === "done"
                Layout.fillWidth: true
                spacing: Theme.spacingMd

                Label {
                    Layout.fillWidth: true
                    text: root.addToNodeId !== "" ? "The server joined your node." : "Your supernode is ready."
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeTitle
                    font.bold: true
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.text
                    font.pixelSize: Theme.fontSizeBody
                    text: root.addToNodeId !== ""
                        ? "Clients connected to your node can now fail over to this server."
                        : "DoubleSlash is connecting to it through its invite. It appears under "
                          + "Rooms, and you can manage it from Settings › Network."
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    readonly property var ports: root.outcome.ports || ({})
                    text: "The server's own firewall is set up. If your hosting provider also has "
                        + "a firewall in its control panel, allow UDP " + (ports.relay_udp || 3478)
                        + " and TCP " + (ports.signaling_tcp || 34935) + " there"
                        + (root.addToNodeId !== ""
                            ? ", and UDP " + (ports.cluster_udp || 4478) + " between your servers."
                            : ".")
                }
            }

            // ── Error ────────────────────────────────────────────────────
            Label {
                Layout.fillWidth: true
                visible: root.errorText !== ""
                text: root.errorText
                color: Theme.danger
                wrapMode: Text.WordWrap
                font.pixelSize: Theme.fontSizeCaption
            }
            Label {
                Layout.fillWidth: true
                visible: root.page === "run" && root.errorText !== ""
                wrapMode: Text.WordWrap
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                text: "Nothing was left half-done that a retry cannot finish: each step checks "
                    + "what is already in place. Password login stays on until the very last "
                    + "SSH step succeeds."
            }
        }
    }

    // A plain row, not DialogButtonBox: the box kept laying out the buttons
    // hidden on the current page, which pushed Done past the dialog's edge.
    footer: Rectangle {
        color: Theme.bg1
        implicitHeight: footerRow.implicitHeight + Theme.spacingMd * 2

        RowLayout {
            id: footerRow
            anchors.fill: parent
            anchors.margins: Theme.spacingMd
            spacing: Theme.spacingSm

            StyledButton {
                text: "Cancel"
                visible: root.page !== "run" && root.page !== "done"
                onClicked: root.close()
            }
            StyledButton {
                text: "Back"
                visible: root.page === "hostkey" || root.page === "options"
                onClicked: root.page = root.page === "options" ? "hostkey" : "server"
            }
            // Setup keeps running on the backend; Settings › Network shows
            // the node once it finishes.
            StyledButton {
                text: "Hide"
                visible: root.page === "run" && root.errorText === ""
                onClicked: root.close()
            }
            StyledButton {
                visible: root.page === "run" && root.errorText !== ""
                text: "Change answers"
                onClicked: { root.errorText = ""; root.page = "options" }
            }
            StyledButton {
                visible: root.page === "run" && root.errorText !== ""
                text: "Close"
                onClicked: root.close()
            }

            Item { Layout.fillWidth: true }

            StyledButton {
                visible: root.page === "notice"
                primary: true
                text: "Continue"
                enabled: noticeSwitch.checked
                onClicked: root.send({ op: "accept_notice" })
            }
            StyledButton {
                visible: root.page === "server"
                primary: true
                text: "Next"
                enabled: hostField.text.trim() !== ""
                onClicked: root.send({
                    op: "fetch_host_key",
                    host: hostField.text.trim(),
                    port: parseInt(portField.text) || 22
                })
            }
            StyledButton {
                visible: root.page === "hostkey"
                primary: true
                text: "The key matches"
                onClicked: root.page = "options"
            }
            StyledButton {
                visible: root.page === "options"
                primary: true
                text: root.addToNodeId !== "" ? "Add server" : "Set up supernode"
                enabled: !root.busy
                onClicked: root.startSetup()
            }
            StyledButton {
                visible: root.page === "run" && root.errorText !== ""
                primary: true
                text: "Retry"
                enabled: !root.busy
                onClicked: root.startSetup()
            }
            StyledButton {
                visible: root.page === "done"
                primary: true
                text: "Done"
                onClicked: root.close()
            }
        }
    }
}
