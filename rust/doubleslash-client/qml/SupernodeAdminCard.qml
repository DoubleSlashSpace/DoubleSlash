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

    // Icon-only server action. The old label is the tooltip.
    component ServerButton: IconButton {
        property string tip: ""
        property bool danger: false
        property bool accent: false
        icon.width: 18
        icon.height: 18
        icon.color: !enabled ? Theme.muted
            : danger ? Theme.danger
            : accent ? Theme.accent
            : Theme.text
        tileColor: danger ? Theme.semanticTint(Theme.danger, 0.18)
            : accent ? Theme.semanticTint(Theme.accent, 0.18)
            : "transparent"
        tileBorder: danger ? Theme.danger : accent ? Theme.accent : "transparent"
        display: AbstractButton.IconOnly
        Layout.alignment: Qt.AlignVCenter
        Accessible.name: tip
        ToolTip.text: tip
        ToolTip.visible: hovered || visualFocus
        ToolTip.delay: 300
    }

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
                var failed = event.error || "Request failed"
                if (event.op === "invite" && card.info(nodeId).usedSaved)
                    failed += " The saved link is still on the clipboard."
                card.patch(nodeId, { working: "", output: failed, failed: true, usedSaved: false })
                return
            }
            var v = event.value || {}
            var fields = { working: "", failed: false, usedSaved: false }
            if (event.op === "status") fields.status = v.hosts
            else if (event.op === "invite") {
                fields.inviteUrl = v.invite_url || ""
                if (fields.inviteUrl !== "")
                    card.appBackend.copyToClipboard(fields.inviteUrl)
                fields.output = fields.inviteUrl !== ""
                    ? "Invite copied. Anyone who accepts it adds this supernode."
                    : "The server did not return an invite."
                fields.failed = fields.inviteUrl === ""
            }
            else if (event.op === "check_update") {
                fields.updateAvailable = v.update_available
                fields.output = v.update_available ? "A newer nightly build is available."
                                                   : "Up to date."
            } else if (event.op === "logs") fields.logs = v.text
            else if (v.lines) fields.output = v.lines.join("\n")
            card.patch(nodeId, fields)
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacingMd

        ServerButton {
            tip: "Host a supernode"
            accent: true
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
            enabled: !card.busy
            onClicked: card.hostRequested("", "")
        }

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.RichText
            font.pixelSize: Theme.fontSizeCaption
            color: Theme.accent
            text: "<a href=\"https://doubleslash.space/support.html\"><font color=\""
                + Theme.accent + "\">https://doubleslash.space/support.html</font></a>"
            onLinkActivated: (link) => Qt.openUrlExternally(link)
            HoverHandler { cursorShape: Qt.PointingHandCursor }
        }
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
            // The link just read from the server, or the one saved at setup.
            readonly property string shownInvite: info.inviteUrl || node.invite_url || ""
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

                ServerButton {
                    tip: "Copy invite"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/invite.svg"
                    enabled: !nodeColumn.working
                    onClicked: {
                        var saved = nodeColumn.node.invite_url || ""
                        if (saved !== "")
                            card.appBackend.copyToClipboard(saved)
                        card.patch(nodeColumn.node.id, { usedSaved: saved !== "" })
                        card.send(nodeColumn.node.id, { op: "invite" })
                    }
                }
                ServerButton {
                    tip: "Check status"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "status" })
                }
                ServerButton {
                    tip: "Check for update"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "check_update" })
                }
                ServerButton {
                    tip: "Update now"
                    accent: nodeColumn.info.updateAvailable === true
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/update.svg"
                    enabled: !nodeColumn.working && !card.busy
                    onClicked: card.send(nodeColumn.node.id, { op: "update_now" })
                }
                ServerButton {
                    tip: "Restart"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/restart.svg"
                    enabled: !nodeColumn.working && !card.busy
                    onClicked: card.send(nodeColumn.node.id, { op: "restart" })
                }
                ServerButton {
                    tip: "Logs"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/logs.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "logs", lines: 200 })
                }
                ServerButton {
                    tip: "Add a server"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
                    enabled: !card.busy
                    onClicked: card.hostRequested(nodeColumn.node.id, nodeColumn.node.name)
                }
                ServerButton {
                    tip: "Remove"
                    danger: true
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.pendingRemoveId = nodeColumn.node.id
                }
            }

            Label {
                Layout.fillWidth: true
                visible: !nodeColumn.confirming
                wrapMode: Text.WordWrap
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                text: "People who accept this link add the supernode, with the same rooms, "
                    + "relay, and portal. A room invite only opens that room. Copy invite "
                    + "reads the current link from the server."
            }

            TextArea {
                Layout.fillWidth: true
                Layout.preferredHeight: 72
                visible: !nodeColumn.confirming && nodeColumn.shownInvite !== ""
                readOnly: true
                selectByMouse: true
                text: nodeColumn.shownInvite
                wrapMode: TextEdit.WrapAnywhere
                font.family: "monospace"
                font.pixelSize: Theme.fontSizeCaption
                color: Theme.text
                background: Rectangle {
                    color: Theme.bg1
                    border.color: Theme.divider
                    border.width: 1
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
                    ServerButton {
                        tip: "Uninstall"
                        danger: true
                        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                        enabled: !card.busy
                        onClicked: {
                            card.pendingRemoveId = ""
                            card.send(nodeColumn.node.id, { op: "uninstall", purge: purgeSwitch.checked })
                        }
                    }
                    ServerButton {
                        tip: "Forget"
                        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/forget.svg"
                        onClicked: {
                            card.pendingRemoveId = ""
                            card.send(nodeColumn.node.id, { op: "forget" })
                        }
                    }
                    ServerButton {
                        tip: "Cancel"
                        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/close.svg"
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
    ServerButton {
        tip: card.showNotice ? "Hide the notice" : "Read the full notice"
        icon.source: card.showNotice
            ? "qrc:/qt/qml/DoubleSlash/Client/icons/close.svg"
            : "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg"
        onClicked: card.showNotice = !card.showNotice
    }
}
