// SupernodeAdminCard.qml - the supernodes this profile set up on its own
// servers: status, updates, logs, restart, clustering, and removal.
//
// Every server action goes through `supernodeCommand`; results come back on
// `supernodeAdminEvent`, matched by request id.
pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import DoubleSlash.Client 1.0

SettingsCard {
    id: card
    required property var appBackend
    // The Rooms sidebar's node list, to show whether a managed node is live.
    property ListModel supernodeModel: null

    // Opens the wizard: empty ids for a new node, or a node to add a server to.
    signal hostRequested(string nodeId, string nodeName)

    title: "Your servers"
    subtitle: "Supernodes you set up on your own servers from this device."

    readonly property var adminState: {
        try { return JSON.parse(appBackend.supernode_admin_state) } catch (e) { return ({}) }
    }
    readonly property var nodes: adminState.nodes || []
    readonly property bool busy: appBackend.supernode_admin_busy

    // Per node id: { status: [...], updateAvailable, output, logs, working }.
    property var nodeInfo: ({})
    property string pendingRemoveId: ""
    property bool showNotice: false
    property var pending: ({})

    function send(nodeId, request) {
        var id = "card-" + request.op + "-" + Date.now()
        request.request_id = id
        if (nodeId) request.node_id = nodeId
        var p = Object.assign({}, pending)
        p[id] = nodeId
        pending = p
        patch(nodeId, { working: request.op, output: "" })
        appBackend.supernodeCommand(JSON.stringify(request))
    }

    function patch(nodeId, fields) {
        if (!nodeId) return
        var all = Object.assign({}, nodeInfo)
        all[nodeId] = Object.assign({}, all[nodeId] || {}, fields)
        nodeInfo = all
    }

    function info(nodeId) { return nodeInfo[nodeId] || ({}) }

    // A managed node is live when the sidebar lists one of its servers'
    // identities as connected. Ids compare without base64 padding.
    function connected(node) {
        if (!supernodeModel) return false
        var ids = node.hosts.map(function(h) { return (h.node_identity || "").replace(/=+$/, "") })
        for (var i = 0; i < supernodeModel.count; i++) {
            var row = supernodeModel.get(i)
            if (row.connected && ids.indexOf((row.node_id || "").replace(/=+$/, "")) >= 0)
                return true
        }
        return false
    }

    function statusLine(node) {
        var parts = [connected(node) ? "Connected" : "Not connected"]
        parts.push(node.hosts.length === 1 ? node.hosts[0].public_host
                                           : node.hosts.length + " servers")
        parts.push(node.update_mode === "server-timer" ? "updates automatically"
                                                       : "updates when you choose")
        return parts.join(" · ")
    }

    Component.onCompleted: appBackend.supernodeCommand(JSON.stringify({ op: "state" }))

    Connections {
        target: card.appBackend
        function onSupernodeAdminEvent(event_json) {
            var event
            try { event = JSON.parse(event_json) } catch (e) { return }
            if (event.kind !== "result" || !(event.request_id in card.pending)) return
            var nodeId = card.pending[event.request_id]
            var p = Object.assign({}, card.pending)
            delete p[event.request_id]
            card.pending = p
            if (!event.ok) {
                card.patch(nodeId, { working: "", output: event.error, failed: true })
                return
            }
            var v = event.value || {}
            var fields = { working: "", failed: false }
            if (event.op === "status") fields.status = v.hosts
            else if (event.op === "check_update") {
                fields.updateAvailable = v.update_available
                fields.output = v.update_available ? "A newer nightly build is available."
                                                   : "Up to date."
            } else if (event.op === "logs") fields.logs = v.text
            else if (v.lines) fields.output = v.lines.join("\n")
            card.patch(nodeId, fields)
        }
    }

    StyledButton {
        primary: true
        text: "Host a supernode"
        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
        enabled: !card.busy
        onClicked: card.hostRequested("", "")
    }

    Label {
        Layout.fillWidth: true
        visible: card.nodes.length === 0
        wrapMode: Text.WordWrap
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        text: "Rent a small Linux server from any hosting provider, then let DoubleSlash "
            + "install and secure the supernode on it. You choose how it is updated and how "
            + "SSH is locked down."
    }

    Repeater {
        model: card.nodes
        delegate: ColumnLayout {
            id: nodeColumn
            required property var modelData
            readonly property var node: modelData
            readonly property var info: card.info(node.id)
            readonly property bool working: (info.working || "") !== ""
            readonly property bool confirming: card.pendingRemoveId === node.id
            Layout.fillWidth: true
            spacing: Theme.spacingSm

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Theme.bg3
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacingMd
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Label {
                        Layout.fillWidth: true
                        text: nodeColumn.node.name
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeBody
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        Layout.fillWidth: true
                        text: card.statusLine(nodeColumn.node)
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeCaption
                        elide: Text.ElideRight
                    }
                }
                SquareBusyIndicator {
                    Layout.preferredWidth: 18
                    Layout.preferredHeight: 18
                    running: nodeColumn.working
                    visible: running
                }
            }

            // One line per server, with the last status check.
            Repeater {
                model: nodeColumn.node.hosts
                delegate: Label {
                    id: hostLine
                    required property var modelData
                    required property int index
                    readonly property var live: (nodeColumn.info.status || [])[index] || null
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: hostLine.live && hostLine.live.reachable === false ? Theme.danger
                                                                              : Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                    text: {
                        var parts = [modelData.public_host, modelData.os || modelData.platform]
                        if (hostLine.live) {
                            if (hostLine.live.reachable === false) parts.push("unreachable: " + hostLine.live.error)
                            else parts.push((hostLine.live.active ? "running " : hostLine.live.state + " ")
                                            + hostLine.live.version)
                        }
                        parts.push("SSH key " + modelData.host_key_fingerprint.substring(0, 20) + "…")
                        return parts.join(" · ")
                    }
                }
            }

            Flow {
                Layout.fillWidth: true
                spacing: Theme.spacingSm
                visible: !nodeColumn.confirming

                StyledButton {
                    text: "Check status"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "status" })
                }
                StyledButton {
                    text: "Check for update"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "check_update" })
                }
                StyledButton {
                    text: "Update now"
                    primary: nodeColumn.info.updateAvailable === true
                    enabled: !nodeColumn.working && !card.busy
                    onClicked: card.send(nodeColumn.node.id, { op: "update_now" })
                }
                StyledButton {
                    text: "Restart"
                    enabled: !nodeColumn.working && !card.busy
                    onClicked: card.send(nodeColumn.node.id, { op: "restart" })
                }
                StyledButton {
                    text: "Logs"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/logs.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "logs", lines: 200 })
                }
                StyledButton {
                    text: "Add a server"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
                    enabled: !card.busy
                    onClicked: card.hostRequested(nodeColumn.node.id, nodeColumn.node.name)
                }
                StyledButton {
                    text: "Remove"
                    danger: true
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.pendingRemoveId = nodeColumn.node.id
                }
            }

            SettingSwitch {
                Layout.fillWidth: true
                visible: !nodeColumn.confirming
                title: "Update automatically on the server"
                description: "Off: the node changes only when you choose Update now."
                checked: nodeColumn.node.update_mode === "server-timer"
                enabledState: !nodeColumn.working && !card.busy
                onChanged: function(value) {
                    card.send(nodeColumn.node.id, {
                        op: "set_update_mode",
                        mode: value ? "server-timer" : "client"
                    })
                }
            }

            // Remove: forget locally, or uninstall from the servers too.
            ColumnLayout {
                visible: nodeColumn.confirming
                Layout.fillWidth: true
                spacing: Theme.spacingSm
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: Theme.danger
                    font.pixelSize: Theme.fontSizeCaption
                    text: "Are you sure? Uninstall stops the supernode and removes it from the "
                        + "server (the firewall and security settings stay). Delete its data "
                        + "too, and the node's identity is gone: everyone who uses it has to "
                        + "accept a new invite. Forget only removes it from this list; the "
                        + "server keeps running."
                }
                SettingSwitch {
                    id: purgeSwitch
                    Layout.fillWidth: true
                    title: "Also delete the node's data and identity"
                    checked: false
                    onChanged: function(value) { purgeSwitch.checked = value }
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: Theme.spacingSm
                    StyledButton {
                        text: "Uninstall"
                        danger: true
                        enabled: !card.busy
                        onClicked: {
                            card.pendingRemoveId = ""
                            card.send(nodeColumn.node.id, { op: "uninstall", purge: purgeSwitch.checked })
                        }
                    }
                    StyledButton {
                        text: "Forget"
                        onClicked: {
                            card.pendingRemoveId = ""
                            card.send(nodeColumn.node.id, { op: "forget" })
                        }
                    }
                    StyledButton {
                        text: "Cancel"
                        onClicked: card.pendingRemoveId = ""
                    }
                }
            }

            Label {
                Layout.fillWidth: true
                visible: (nodeColumn.info.output || "") !== ""
                text: nodeColumn.info.output || ""
                wrapMode: Text.WordWrap
                color: nodeColumn.info.failed ? Theme.danger : Theme.muted
                font.pixelSize: Theme.fontSizeCaption
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: 220
                visible: (nodeColumn.info.logs || "") !== ""
                clip: true
                TextArea {
                    readOnly: true
                    text: nodeColumn.info.logs || ""
                    font.family: "monospace"
                    font.pixelSize: Theme.fontSizeCaption
                    color: Theme.text
                    wrapMode: TextEdit.NoWrap
                    background: Rectangle { color: Theme.bg1; border.color: Theme.divider; border.width: 1 }
                }
            }
        }
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 1
        color: Theme.bg3
    }

    Label {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        text: card.showNotice
            ? (card.adminState.notice || "")
            : "Your servers are your responsibility. DoubleSlash automates setup, not ongoing administration."
    }
    StyledButton {
        text: card.showNotice ? "Hide the notice" : "Read the full notice"
        onClicked: card.showNotice = !card.showNotice
    }
}
