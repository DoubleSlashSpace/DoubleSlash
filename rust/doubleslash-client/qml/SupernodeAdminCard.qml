// SupernodeAdminCard.qml - the supernodes this profile set up on its own
// servers: status, updates, logs, restart, clustering, and removal.
//
// Every server action goes through `supernodeCommand`; results come back on
// `supernodeAdminEvent`, matched by request id. Different supernodes run
// tasks side by side; the service refuses a second task on one supernode, and
// lists the busy ones in the state's `busy_nodes`.
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
    // Check a supernode's servers when it is opened and none has been checked.
    property bool autoStatus: true
    // From this many supernodes on, the list can be filtered.
    readonly property int filterThreshold: 5

    // Opens the wizard: empty ids for a new supernode, or a managed node's id
    // and name to add a server to that supernode's cluster.
    signal hostRequested(string nodeId, string nodeName)

    component ServerButton: IconButton {
        property string tip: ""
        property bool danger: false
        property bool accent: false
        implicitWidth: 28
        implicitHeight: 28
        padding: 5
        icon.width: 16
        icon.height: 16
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

    component ResultPanel: ColumnLayout {
        id: resultPanel
        required property var result
        Layout.fillWidth: true
        spacing: Theme.spacingXs
        visible: (result.output || "") !== "" || (result.logs || "") !== ""

        Rectangle {
            id: outputNotice
            Layout.fillWidth: true
            visible: (resultPanel.result.output || "") !== ""
            readonly property bool warning: resultPanel.result.failed === true
            readonly property color noticeColor: warning ? Theme.danger : Theme.accent
            implicitHeight: outputRow.implicitHeight + Theme.spacingSm * 2
            color: Theme.semanticTint(noticeColor, 0.14)
            border.color: Theme.semanticTint(noticeColor, 0.55)
            border.width: 1

            RowLayout {
                id: outputRow
                anchors.fill: parent
                anchors.margins: Theme.spacingSm
                spacing: Theme.spacingSm
                Image {
                    Layout.preferredWidth: 18
                    Layout.preferredHeight: 18
                    Layout.alignment: Qt.AlignTop
                    sourceSize.width: 36
                    sourceSize.height: 36
                    source: outputNotice.warning
                        ? "qrc:/qt/qml/DoubleSlash/Client/icons/warning.svg"
                        : "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg"
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Theme.spacingXs
                    Label {
                        text: outputNotice.warning ? "WARNING" : "NOTICE"
                        color: outputNotice.noticeColor
                        font.pixelSize: Theme.fontSizeCaption
                        font.bold: true
                    }
                    Label {
                        Layout.fillWidth: true
                        text: resultPanel.result.output || ""
                        textFormat: Text.PlainText
                        wrapMode: Text.WordWrap
                        color: Theme.text
                        font.pixelSize: Theme.fontSizeBody
                    }
                }
            }
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: 180
            visible: (resultPanel.result.logs || "") !== ""
            clip: true
            TextArea {
                readOnly: true
                text: resultPanel.result.logs || ""
                textFormat: TextEdit.PlainText
                font.family: "monospace"
                font.pixelSize: Theme.fontSizeCaption
                color: Theme.text
                wrapMode: TextEdit.NoWrap
                background: Rectangle { color: Theme.bg1; border.color: Theme.divider; border.width: 1 }
            }
        }
    }

    title: "Your servers"
    subtitle: "Supernodes you host. Each is one server or a cluster of several; "
        + "open one to manage its servers."

    readonly property var adminState: {
        try { return JSON.parse(appBackend.supernode_admin_state) } catch (e) { return ({}) }
    }
    readonly property var nodes: adminState.nodes || []
    readonly property var busyNodes: adminState.busy_nodes || []
    readonly property bool setupRunning: adminState.setup_running === true

    // Results are scoped to a supernode or one of its servers.
    property var nodeInfo: ({})
    property string pendingRemoveId: ""
    // infoKey(node, server) of the server awaiting confirmation to remove.
    property string pendingServerRemove: ""
    property string renamingId: ""
    property string editingHostnameKey: ""
    property string hostnameDraft: ""
    property string hostnameLookupDraft: ""
    property string dddnsTokenDraft: ""
    property string dddnsUpdateUrlDraft: ""
    readonly property bool hostnameUsesDddns: {
        var name = hostnameDraft.trim().toLowerCase().replace(/\.+$/, "")
        return name !== "" && (hostnameLookupDraft === "dddns"
            || (hostnameLookupDraft === "" && /^[^.]+\.doubleslash\.space$/.test(name)))
    }

    function defaultDddnsUpdateUrl(name) {
        return /^[^.]+\.doubleslash\.space$/.test(name.trim().toLowerCase().replace(/\.+$/, ""))
            ? "https://users.doubleslash.space/v1/update" : ""
    }

    function resetDddnsCredentials() {
        dddnsTokenDraft = ""
        dddnsUpdateUrlDraft = defaultDddnsUpdateUrl(hostnameDraft)
    }

    onEditingHostnameKeyChanged: if (editingHostnameKey === "") dddnsTokenDraft = ""
    onVisibleChanged: if (!visible) { dddnsTokenDraft = ""; editingHostnameKey = "" }
    property string filterText: ""
    property bool showNotice: false
    property var pending: ({})
    // One managed supernode is open at a time. A single node starts open; a
    // longer list starts folded so the page stays a list of rows.
    property string openNodeId: ""
    property bool openChosen: false
    property int nextRequestId: 0

    // `focus` opens the node the request is for; checks across every
    // supernode leave the list as it is.
    function send(nodeId, request, focus) {
        var id = "card-" + request.op + "-" + (++nextRequestId)
        request.request_id = id
        if (nodeId) request.node_id = nodeId
        // A removed server's row goes away, so its outcome shows on the node.
        var hostName = request.op === "remove_server" ? "" : (request.host_name || "")
        var p = Object.assign({}, pending)
        p[id] = { nodeId: nodeId, hostName: hostName }
        pending = p
        patch(nodeId, request.op === "connection_info"
            ? { addressesChecking: true, addressesError: "" }
            : { working: request.op, output: "", logs: "", failed: false }, hostName)
        if (nodeId && focus !== false) {
            openNodeId = nodeId
            openChosen = true
            if (pendingRemoveId !== "" && pendingRemoveId !== nodeId)
                pendingRemoveId = ""
        }
        appBackend.supernodeCommand(JSON.stringify(request))
    }

    // Natural name order, as in the Rooms list: "Node 2" before "Node 10".
    function sortedNodes() {
        var list = nodes.slice()
        list.sort(function(a, b) {
            return RoomTree.compareAlphanumeric(a.name, b.name)
        })
        return list
    }

    function matchesFilter(node) {
        var needle = filterText.trim().toLowerCase()
        if (needle === "") return true
        var fields = [node.name || "", node.cluster_id || ""]
        ;(node.hosts || []).forEach(function(host) {
            fields.push(host.host_name || "", host.public_host || "", host.hostname || "")
        })
        return fields.some(function(field) {
            return field.toLowerCase().indexOf(needle) >= 0
        })
    }

    function listedNodes() {
        return sortedNodes().filter(matchesFilter)
    }

    function chooseInitialOpen() {
        if (openChosen || nodes.length === 0) return
        openChosen = true
        if (nodes.length === 1)
            openNodeId = nodes[0].id || ""
    }

    function toggleNode(nodeId) {
        openChosen = true
        if (openNodeId === nodeId) {
            openNodeId = ""
            if (pendingRemoveId === nodeId)
                pendingRemoveId = ""
        } else {
            openNodeId = nodeId
            if (pendingRemoveId !== "" && pendingRemoveId !== nodeId)
                pendingRemoveId = ""
        }
        pendingServerRemove = ""
    }

    function nodeById(nodeId) {
        for (var i = 0; i < nodes.length; i++)
            if (nodes[i].id === nodeId) return nodes[i]
        return null
    }

    // Opening a supernode whose servers were never checked asks them, so its
    // rows say more than "Not checked".
    function checkOnOpen(nodeId) {
        var node = nodeById(nodeId)
        if (!autoStatus || !node || working(node) || nodeBusy(nodeId)) return
        if (health(node).checked > 0) return
        send(nodeId, { op: "status" }, false)
    }
    onOpenNodeIdChanged: {
        editingHostnameKey = ""
        if (openNodeId !== "") Qt.callLater(checkOnOpen, openNodeId)
    }

    function infoKey(nodeId, hostName) {
        return hostName ? JSON.stringify([nodeId, hostName]) : nodeId
    }

    function patch(nodeId, fields, hostName) {
        if (!nodeId) return
        var all = Object.assign({}, nodeInfo)
        var key = infoKey(nodeId, hostName)
        all[key] = Object.assign({}, all[key] || {}, fields)
        nodeInfo = all
    }

    function info(nodeId, hostName) { return nodeInfo[infoKey(nodeId, hostName)] || ({}) }

    // A request from this card is waiting on the node or one of its servers.
    function working(node) {
        if (info(node.id).working) return true
        return (node.hosts || []).some(function(host) {
            return !!info(node.id, host.host_name).working
        })
    }

    // The service is running a task that changes the node, from here or from
    // the setup wizard.
    function nodeBusy(nodeId) { return busyNodes.indexOf(nodeId) >= 0 }

    function updateAvailable(node) {
        return (node.hosts || []).some(function(host) {
            return info(node.id, host.host_name).updateAvailable === true
        })
    }

    // How many servers were checked, are running, and are down.
    function health(node) {
        var out = { checked: 0, running: 0, down: 0 }
        ;(node.hosts || []).forEach(function(host) {
            var live = info(node.id, host.host_name).status
            if (!live) return
            out.checked++
            if (live.reachable !== false && live.active) out.running++
            else out.down++
        })
        return out
    }

    function memberStatus(nodeId, host) {
        var live = info(nodeId, host.host_name).status
        if (!live) return "Not checked"
        if (live.reachable === false) return "Unreachable"
        return live.active ? "Running" : live.state
    }

    function connectionDetails(host, addresses) {
        var values = addresses || host
        var missing = addresses ? "No public address detected" : "Not checked"
        return "IPv4: " + (values.ipv4 || missing)
            + "\nIPv6: " + (values.ipv6 || missing)
            + (values.ipv6 && values.signaling_ipv6 === false
                ? (values.dual_stack_on_apply ? " (Apply will enable dual stack)" : " (signaling is IPv4 only)") : "")
            + "\nWebSocket signaling port (TCP): " + (values.ws_port || "Unavailable")
            + "\nRelay port (UDP): " + (values.relay_port || "Unavailable")
    }

    function checkAddresses(nodeId, hostName, force) {
        var current = info(nodeId, hostName)
        if (current.addressesChecking || (!force && current.addresses)) return
        send(nodeId, { op: "connection_info", host_name: hostName }, false)
    }

    function memberDetails(nodeId, host) {
        var live = info(nodeId, host.host_name).status
        var lines = [host.host_name, connectionDetails(host, info(nodeId, host.host_name).addresses),
                     "SSH: " + host.ssh_target, "OS: " + (host.os || host.platform),
                     "Platform: " + host.platform, "SSH access: " + host.access,
                     "SSH host key: " + host.host_key_fingerprint,
                     "Node identity: " + host.node_identity]
        lines.push("Hostname: " + (host.hostname || "None"),
                   "Hostname lookup: " + (host.hostname_lookup || "Automatic"))
        if (live) {
            lines.push("Status: " + memberStatus(nodeId, host))
            if (live.version) lines.push("Build: " + live.version)
            if (live.error) lines.push("Error: " + live.error)
        }
        return lines.join("\n")
    }

    function bare(id) { return (id || "").replace(/=+$/, "") }

    // The sidebar row for one of the node's servers, if the client knows it.
    function sidebarRow(node) {
        if (!supernodeModel) return null
        var ids = (node.hosts || []).map(function(h) { return bare(h.node_identity) })
        var found = null
        for (var i = 0; i < supernodeModel.count; i++) {
            var row = supernodeModel.get(i)
            if (ids.indexOf(bare(row.node_id)) < 0) continue
            if (row.connected) return row
            if (!found) found = row
        }
        return found
    }

    // A managed node is live when the sidebar lists one of its servers'
    // identities as connected. Ids compare without base64 padding.
    function connected(node) {
        var row = sidebarRow(node)
        return !!row && row.connected
    }

    // The id the Rooms list draws the node's avatar from, so both match.
    function avatarId(node) {
        var row = sidebarRow(node)
        if (row) return row.node_id
        var hosts = node.hosts || []
        return hosts.length > 0 ? (hosts[0].node_identity || "") : ""
    }

    readonly property var workingWords: ({
        status: "checking", check_update: "checking for updates", update_now: "updating",
        restart: "restarting", set_update_mode: "changing updates", uninstall: "uninstalling",
        remove_server: "removing a server", sync_cluster: "syncing the cluster",
        invite: "reading the invite", logs: "reading logs", rename: "renaming",
        set_hostname: "applying hostname"
    })

    function workingWord(node) {
        var op = info(node.id).working
        if (!op) {
            var hosts = node.hosts || []
            for (var i = 0; i < hosts.length && !op; i++)
                op = info(node.id, hosts[i].host_name).working
        }
        if (op) return workingWords[op] || "working"
        return nodeBusy(node.id) ? "busy" : ""
    }

    // The folded row's line: what you need to know without opening it.
    function statusLine(node) {
        var count = (node.hosts || []).length
        var parts = [connected(node) ? "Connected" : "Not connected",
                     count + (count === 1 ? " server" : " servers")]
        var h = health(node)
        if (h.checked > 0) {
            if (h.down > 0) parts.push(h.down + " down")
            else if (h.checked === count) parts.push(count === 1 ? "running" : "all running")
            else parts.push(h.running + " of " + count + " running")
        }
        if (updateAvailable(node)) parts.push("update available")
        if (info(node.id).failed) parts.push("last action failed")
        var doing = workingWord(node)
        if (doing !== "") parts.push(doing + "…")
        return parts.join(" · ")
    }

    function statusColor(node) {
        if (health(node).down > 0 || info(node.id).failed) return Theme.danger
        if (updateAvailable(node)) return Theme.accent
        return Theme.muted
    }

    function fleetSummary() {
        var servers = 0, live = 0, updates = 0, down = 0
        nodes.forEach(function(node) {
            servers += (node.hosts || []).length
            if (connected(node)) live++
            if (updateAvailable(node)) updates++
            down += health(node).down
        })
        var parts = [nodes.length + (nodes.length === 1 ? " supernode" : " supernodes"),
                     servers + (servers === 1 ? " server" : " servers"),
                     live + " connected"]
        if (down > 0) parts.push(down + " down")
        if (updates > 0) parts.push(updates + (updates === 1 ? " has an update" : " have updates"))
        return parts.join(" · ")
    }

    // Send `op` to each listed supernode that is free. Reads run together;
    // updates run together across supernodes, one server at a time within each.
    function sendEach(op, onlyWithUpdates) {
        listedNodes().forEach(function(node) {
            if (working(node) || nodeBusy(node.id)) return
            if (onlyWithUpdates && !updateAvailable(node)) return
            send(node.id, { op: op }, false)
        })
    }

    function anyListedUpdate() {
        return listedNodes().some(function(node) {
            return updateAvailable(node) && !working(node) && !nodeBusy(node.id)
        })
    }

    Component.onCompleted: {
        chooseInitialOpen()
        appBackend.supernodeCommand(JSON.stringify({ op: "state" }))
    }
    onNodesChanged: chooseInitialOpen()

    Connections {
        target: card.appBackend
        function onSupernodeAdminEvent(event_json) {
            var event
            try { event = JSON.parse(event_json) } catch (e) { return }
            if (event.kind !== "result" || !(event.request_id in card.pending)) return
            var target = card.pending[event.request_id]
            var nodeId = target.nodeId
            var hostName = target.hostName
            var p = Object.assign({}, card.pending)
            delete p[event.request_id]
            card.pending = p
            if (event.op === "connection_info") {
                card.patch(nodeId, event.ok
                    ? { addressesChecking: false, addresses: event.value, addressesError: "" }
                    : { addressesChecking: false, addressesError: event.error || "Could not check addresses" }, hostName)
                return
            }
            if (!event.ok) {
                var failed = event.error || "Request failed"
                if (event.op === "invite" && card.info(nodeId).usedSaved)
                    failed += " The saved link is still on the clipboard."
                card.patch(nodeId, { working: "", output: failed, failed: true, usedSaved: false }, hostName)
                return
            }
            var v = event.value || {}
            if (event.op === "set_hostname"
                    && card.editingHostnameKey === card.infoKey(nodeId, hostName))
                card.editingHostnameKey = ""
            var fields = { working: "", failed: false, usedSaved: false }
            if (event.op === "check_update" || event.op === "update_now") {
                (v.hosts || []).forEach(function(host) {
                    card.patch(nodeId, { updateAvailable: host.update_available }, host.host_name)
                })
            }
            if (event.op === "status") {
                (v.hosts || []).forEach(function(host) {
                    card.patch(nodeId, { status: host }, host.host_name)
                })
            }
            else if (event.op === "invite") {
                var url = v.invite_url || ""
                if (url !== "")
                    card.appBackend.copyToClipboard(url)
                fields.output = url !== ""
                    ? "Invite copied. Anyone who accepts it adds this supernode."
                    : "The server did not return an invite."
                fields.failed = url === ""
            }
            else if (event.op === "check_update") {
                fields.output = v.update_available ? "A newer nightly build is available."
                                                   : "Up to date."
            } else if (event.op === "logs") fields.logs = v.text
            else if (v.lines) fields.output = v.lines.join("\n")
            card.patch(nodeId, fields, hostName)
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacingXs

        StyledButton {
            objectName: "host-supernode"
            text: "Host a supernode"
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
            enabled: !card.setupRunning
            onClicked: card.hostRequested("", "")
        }
        Item { Layout.fillWidth: true }
        // Across every listed supernode; a single one has its own controls.
        ServerButton {
            objectName: "check-all"
            visible: card.nodes.length > 1
            tip: "Check the status of every listed supernode"
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
            onClicked: card.sendEach("status", false)
        }
        ServerButton {
            objectName: "check-all-updates"
            visible: card.nodes.length > 1
            tip: "Check every listed supernode for updates"
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
            onClicked: card.sendEach("check_update", false)
        }
        ServerButton {
            objectName: "update-all"
            visible: card.nodes.length > 1
            tip: "Update every listed supernode that has an update"
            accent: enabled
            enabled: card.anyListedUpdate()
            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/update.svg"
            onClicked: card.sendEach("update_now", true)
        }
    }

    Label {
        objectName: "fleet-summary"
        Layout.fillWidth: true
        visible: card.nodes.length > 1
        wrapMode: Text.WordWrap
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        text: card.fleetSummary()
    }

    StyledTextField {
        objectName: "server-filter"
        Layout.fillWidth: true
        visible: card.nodes.length >= card.filterThreshold
        placeholderText: "Filter by name, server or address"
        text: card.filterText
        onTextEdited: card.filterText = text
    }

    Label {
        Layout.fillWidth: true
        visible: card.nodes.length === 0
        wrapMode: Text.WordWrap
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        text: "Rent a small Linux server from any hosting provider, then let DoubleSlash "
            + "install and secure the supernode on it. You choose how it is updated and how "
            + "SSH is locked down. Add more servers to a supernode later to make it a "
            + "cluster that clients fail over across."
    }

    Label {
        Layout.fillWidth: true
        visible: card.nodes.length > 0 && card.listedNodes().length === 0
        color: Theme.muted
        font.pixelSize: Theme.fontSizeCaption
        text: "No supernode matches."
    }

    Repeater {
        model: card.listedNodes()
        delegate: ColumnLayout {
            id: nodeColumn
            required property var modelData
            readonly property var node: modelData
            readonly property var info: card.info(node.id)
            readonly property bool working: card.working(node)
            // A task that changes the node is running, here or in the wizard.
            readonly property bool locked: working || card.nodeBusy(node.id)
            readonly property bool confirming: card.pendingRemoveId === node.id
            readonly property bool expanded: card.openNodeId === node.id
            readonly property bool renaming: card.renamingId === node.id
            readonly property bool clustered: (node.hosts || []).length > 1 || !!node.cluster_id
            property bool showDetails: false
            Layout.fillWidth: true
            spacing: Theme.spacingXs

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: Theme.bg3
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.spacingXs
                ItemDelegate {
                    id: nodeHeader
                    objectName: "supernode-" + nodeColumn.node.id
                    Layout.fillWidth: true
                    padding: Theme.spacingXs
                    hoverEnabled: true
                    Accessible.name: nodeColumn.expanded
                        ? "Hide " + nodeColumn.node.name
                        : "Show " + nodeColumn.node.name
                    background: Rectangle {
                        color: nodeHeader.hovered ? Theme.bg3 : "transparent"
                    }
                    onClicked: card.toggleNode(nodeColumn.node.id)
                    contentItem: RowLayout {
                        spacing: Theme.spacingSm
                        Image {
                            Layout.preferredWidth: 10
                            Layout.preferredHeight: 10
                            sourceSize.width: 20
                            sourceSize.height: 20
                            source: "qrc:/qt/qml/DoubleSlash/Client/icons/chevron.svg"
                            rotation: nodeColumn.expanded ? 90 : 0
                        }
                        // Ringed green while connected, as in the Rooms list.
                        Avatar {
                            id: nodeAvatar
                            peerId: card.avatarId(nodeColumn.node)
                            size: 24
                            showRing: true
                            ringColor: card.connected(nodeColumn.node) ? Theme.online : nodeAvatar.tintColor
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Label {
                                Layout.fillWidth: true
                                text: nodeColumn.node.name
                                textFormat: Text.PlainText
                                color: Theme.text
                                font.pixelSize: Theme.fontSizeBody
                                font.bold: true
                                elide: Text.ElideRight
                            }
                            Label {
                                objectName: "status-line-" + nodeColumn.node.id
                                Layout.fillWidth: true
                                text: card.statusLine(nodeColumn.node)
                                textFormat: Text.PlainText
                                color: card.statusColor(nodeColumn.node)
                                font.pixelSize: Theme.fontSizeCaption
                                elide: Text.ElideRight
                            }
                        }
                    }
                }
                SquareBusyIndicator {
                    Layout.preferredWidth: 16
                    Layout.preferredHeight: 16
                    running: nodeColumn.locked
                    visible: running
                }
                ServerButton {
                    objectName: "rename-" + nodeColumn.node.id
                    visible: nodeColumn.expanded
                    tip: "Rename"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/pencil.svg"
                    checked: nodeColumn.renaming
                    onClicked: card.renamingId = nodeColumn.renaming ? "" : nodeColumn.node.id
                }
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
                    objectName: "add-server-" + nodeColumn.node.id
                    tip: "Add a server to this supernode's cluster"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/plus.svg"
                    enabled: !nodeColumn.locked && !card.setupRunning
                    onClicked: card.hostRequested(nodeColumn.node.id, nodeColumn.node.name)
                }
                ServerButton {
                    tip: "Supernode info"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg"
                    checked: nodeColumn.showDetails
                    onClicked: {
                        card.openNodeId = nodeColumn.node.id
                        card.openChosen = true
                        nodeColumn.showDetails = !nodeColumn.showDetails
                    }
                }
                ServerButton {
                    tip: "Remove this supernode and all its servers"
                    danger: true
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                    enabled: !nodeColumn.locked
                    onClicked: {
                        card.openNodeId = nodeColumn.node.id
                        card.openChosen = true
                        card.pendingRemoveId = nodeColumn.node.id
                        card.pendingServerRemove = ""
                    }
                }
            }

            // This profile's name for the supernode. Nothing reaches a server,
            // and the Rooms list keeps its own name for the node.
            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: Theme.spacingLg
                visible: nodeColumn.renaming
                spacing: Theme.spacingXs
                StyledTextField {
                    id: renameField
                    objectName: "rename-field-" + nodeColumn.node.id
                    Layout.fillWidth: true
                    placeholderText: "Name"
                    maximumLength: 64
                    onVisibleChanged: if (visible) {
                        text = nodeColumn.node.name
                        forceActiveFocus()
                        selectAll()
                    }
                    onAccepted: saveRename.clicked()
                    Keys.onEscapePressed: card.renamingId = ""
                }
                ServerButton {
                    id: saveRename
                    tip: "Save name"
                    accent: true
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/check.svg"
                    enabled: renameField.text.trim() !== ""
                    onClicked: {
                        card.renamingId = ""
                        if (renameField.text.trim() !== nodeColumn.node.name)
                            card.send(nodeColumn.node.id, { op: "rename", name: renameField.text })
                    }
                }
                ServerButton {
                    tip: "Cancel"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/close.svg"
                    onClicked: card.renamingId = ""
                }
            }

            Label {
                Layout.fillWidth: true
                Layout.leftMargin: Theme.spacingLg
                visible: nodeColumn.expanded && nodeColumn.showDetails
                wrapMode: Text.WordWrap
                textFormat: Text.PlainText
                color: Theme.muted
                font.pixelSize: Theme.fontSizeCaption
                text: card.statusLine(nodeColumn.node)
                    + "\nChannel: " + nodeColumn.node.channel
                    + "\nUpdates: " + (nodeColumn.node.update_mode === "server-timer"
                        ? "each server updates itself daily" : "when you choose Update")
                    + (nodeColumn.node.cluster_id ? "\nCluster: " + nodeColumn.node.cluster_id : "")
                    + "\nThe servers of a supernode share its rooms, relay and portal, and "
                    + "clients fail over between them. Adding a server keeps the same invite. "
                    + "Copy invite reads the current link from a server; a room invite only "
                    + "opens that room."
            }

            Flow {
                Layout.fillWidth: true
                Layout.leftMargin: Theme.spacingLg
                spacing: Theme.spacingXs
                visible: nodeColumn.expanded && !nodeColumn.confirming
                Label {
                    height: Math.max(28, Theme.fontSizeCaption + Theme.spacingSm)
                    verticalAlignment: Text.AlignVCenter
                    text: (nodeColumn.node.hosts || []).length > 1 ? "All servers" : "Server"
                    color: Theme.muted
                    font.pixelSize: Theme.fontSizeCaption
                }
                ServerButton {
                    tip: "Check the status of every server"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "status" })
                }
                ServerButton {
                    tip: "Check every server for updates"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
                    enabled: !nodeColumn.working
                    onClicked: card.send(nodeColumn.node.id, { op: "check_update" })
                }
                ServerButton {
                    tip: "Update every server, one at a time"
                    accent: card.updateAvailable(nodeColumn.node)
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/update.svg"
                    enabled: !nodeColumn.locked
                    onClicked: card.send(nodeColumn.node.id, { op: "update_now" })
                }
                ServerButton {
                    tip: "Restart every server, one at a time"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/restart.svg"
                    enabled: !nodeColumn.locked
                    onClicked: card.send(nodeColumn.node.id, { op: "restart" })
                }
                ServerButton {
                    objectName: "sync-" + nodeColumn.node.id
                    visible: nodeColumn.clustered
                    tip: "Sync the cluster: give every server the member list again and restart "
                        + "them one at a time"
                    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/chain.svg"
                    enabled: !nodeColumn.locked
                    onClicked: card.send(nodeColumn.node.id, { op: "sync_cluster" })
                }
                SquareSwitch {
                    implicitHeight: Math.max(28, Theme.fontSizeCaption + Theme.spacingSm)
                    padding: 0
                    text: "Auto updates"
                    font.pixelSize: Theme.fontSizeCaption
                    checked: nodeColumn.node.update_mode === "server-timer"
                    enabled: !nodeColumn.locked
                    Accessible.name: "Update every server automatically on the server"
                    ToolTip.text: "Off: servers update only when you choose Update."
                    ToolTip.visible: hovered || visualFocus
                    onToggled: card.send(nodeColumn.node.id, {
                        op: "set_update_mode", mode: checked ? "server-timer" : "client"
                    })
                }
            }

            Repeater {
                model: nodeColumn.expanded ? (nodeColumn.node.hosts || []) : []
                delegate: ColumnLayout {
                    id: member
                    required property var modelData
                    required property int index
                    readonly property var info: card.info(nodeColumn.node.id, modelData.host_name)
                    readonly property bool removing: card.pendingServerRemove
                        === card.infoKey(nodeColumn.node.id, modelData.host_name)
                    readonly property string label: "server " + (index + 1)
                    property bool showDetails: false
                    readonly property bool editingHostname: card.editingHostnameKey
                        === card.infoKey(nodeColumn.node.id, modelData.host_name)
                    Layout.fillWidth: true
                    Layout.leftMargin: Theme.spacingLg
                    spacing: Theme.spacingXs

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.spacingXs
                        Item {
                            Layout.preferredWidth: 12
                            Layout.fillHeight: true
                            Rectangle { width: 1; height: parent.height / 2; color: Theme.divider }
                            Rectangle { y: parent.height / 2; width: 12; height: 1; color: Theme.divider }
                        }
                        Label {
                            id: memberName
                            Layout.fillWidth: true
                            text: (member.index + 1) + ". " + member.modelData.host_name
                                + "  " + member.modelData.public_host
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            color: Theme.text
                            font.pixelSize: Theme.fontSizeBody
                            HoverHandler { id: memberHover }
                            ToolTip.text: card.memberDetails(nodeColumn.node.id, member.modelData)
                            ToolTip.visible: memberHover.hovered
                            ToolTip.delay: 500
                        }
                        Label {
                            text: card.memberStatus(nodeColumn.node.id, member.modelData)
                            color: !member.info.status ? Theme.muted
                                : member.info.status.reachable === false || !member.info.status.active
                                ? Theme.danger : Theme.online
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        ServerButton {
                            objectName: "status-" + member.modelData.host_name
                            tip: "Check " + member.label + " status"
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/refresh.svg"
                            enabled: !nodeColumn.working
                            onClicked: card.send(nodeColumn.node.id, {
                                op: "status", host_name: member.modelData.host_name
                            })
                        }
                        ServerButton {
                            tip: "Check " + member.label + " for updates"
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/download.svg"
                            enabled: !nodeColumn.working
                            onClicked: card.send(nodeColumn.node.id, {
                                op: "check_update", host_name: member.modelData.host_name
                            })
                        }
                        ServerButton {
                            tip: "Update " + member.label
                            accent: member.info.updateAvailable === true
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/update.svg"
                            enabled: !nodeColumn.locked
                            onClicked: card.send(nodeColumn.node.id, {
                                op: "update_now", host_name: member.modelData.host_name
                            })
                        }
                        ServerButton {
                            tip: "Restart " + member.label
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/restart.svg"
                            enabled: !nodeColumn.locked
                            onClicked: card.send(nodeColumn.node.id, {
                                op: "restart", host_name: member.modelData.host_name
                            })
                        }
                        ServerButton {
                            tip: "Logs of " + member.label
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/logs.svg"
                            enabled: !nodeColumn.working
                            onClicked: card.send(nodeColumn.node.id, {
                                op: "logs", host_name: member.modelData.host_name, lines: 200
                            })
                        }
                        ServerButton {
                            objectName: "info-" + member.modelData.host_name
                            tip: "Info about " + member.label
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg"
                            checked: member.showDetails
                            onClicked: member.showDetails = !member.showDetails
                        }
                        // The last server goes with the supernode, not on its own.
                        ServerButton {
                            objectName: "remove-server-" + member.modelData.host_name
                            visible: (nodeColumn.node.hosts || []).length > 1
                            tip: "Remove " + member.label + " from the cluster"
                            danger: true
                            icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                            enabled: !nodeColumn.locked
                            onClicked: {
                                card.pendingRemoveId = ""
                                card.pendingServerRemove =
                                    card.infoKey(nodeColumn.node.id, member.modelData.host_name)
                            }
                        }
                    }
                    TextEdit {
                        objectName: "connection-details-" + member.modelData.host_name
                        Layout.fillWidth: true
                        Layout.leftMargin: 12 + Theme.spacingXs
                        visible: !member.editingHostname
                        text: card.connectionDetails(member.modelData, member.info.addresses)
                        textFormat: TextEdit.PlainText
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.Wrap
                        color: Theme.muted
                        selectionColor: Theme.accent
                        selectedTextColor: Theme.textInv
                        font.pixelSize: Theme.fontSizeCaption
                    }
                    Label {
                        objectName: "details-" + member.modelData.host_name
                        Layout.fillWidth: true
                        Layout.leftMargin: 12 + Theme.spacingXs
                        visible: member.showDetails
                        text: card.memberDetails(nodeColumn.node.id, member.modelData)
                        textFormat: Text.PlainText
                        wrapMode: Text.WrapAnywhere
                        color: Theme.muted
                        font.pixelSize: Theme.fontSizeCaption
                    }

                    StyledButton {
                        objectName: "edit-hostname-" + member.modelData.host_name
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.leftMargin: 12 + Theme.spacingXs
                        text: member.modelData.hostname ? "Hostname: " + member.modelData.hostname : "Set hostname"
                        enabled: !nodeColumn.locked
                        onClicked: {
                            card.hostnameDraft = member.modelData.hostname || ""
                            card.hostnameLookupDraft = member.modelData.hostname_lookup || ""
                            card.resetDddnsCredentials()
                            card.editingHostnameKey = member.editingHostname ? ""
                                : card.infoKey(nodeColumn.node.id, member.modelData.host_name)
                            if (member.editingHostname && card.hostnameDraft.trim() !== "")
                                card.checkAddresses(nodeColumn.node.id, member.modelData.host_name, false)
                        }
                    }
                    ColumnLayout {
                        visible: member.editingHostname
                        Layout.fillWidth: true
                        Layout.leftMargin: 12 + Theme.spacingXs
                        spacing: Theme.spacingSm
                        Label {
                            text: "Hostname (optional)"
                            color: Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        StyledTextField {
                            id: hostnameField
                            objectName: "hostname-field-" + member.modelData.host_name
                            Layout.fillWidth: true
                            placeholderText: "node.doubleslash.space or relay.example.com"
                            text: member.editingHostname ? card.hostnameDraft : ""
                            onTextEdited: {
                                card.hostnameDraft = text
                                card.resetDddnsCredentials()
                                if (text.trim() !== "")
                                    card.checkAddresses(nodeColumn.node.id, member.modelData.host_name, false)
                            }
                            enabled: !nodeColumn.locked
                        }
                        TextEdit {
                            objectName: "hostname-addresses-" + member.modelData.host_name
                            Layout.fillWidth: true
                            visible: hostnameField.text.trim() !== ""
                            text: card.connectionDetails(member.modelData, member.info.addresses)
                            textFormat: TextEdit.PlainText
                            readOnly: true
                            selectByMouse: true
                            wrapMode: TextEdit.Wrap
                            color: Theme.text
                            selectionColor: Theme.accent
                            selectedTextColor: Theme.textInv
                            font.pixelSize: Theme.fontSizeBody
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: member.info.addressesChecking === true || !!member.info.addressesError
                            text: member.info.addressesChecking ? "Checking server addresses…"
                                : member.info.addressesError || ""
                            wrapMode: Text.WordWrap
                            color: member.info.addressesError ? Theme.danger : Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        StyledButton {
                            objectName: "refresh-addresses-" + member.modelData.host_name
                            visible: hostnameField.text.trim() !== ""
                            text: "Refresh addresses"
                            enabled: !member.info.addressesChecking
                            onClicked: card.checkAddresses(nodeColumn.node.id, member.modelData.host_name, true)
                        }
                        Label {
                            text: "Hostname lookup"
                            color: Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        ComboBox {
                            id: hostnameLookup
                            objectName: "hostname-lookup-" + member.modelData.host_name
                            Layout.fillWidth: true
                            model: ["Automatic", "DNS", "DDDNS"]
                            readonly property var values: ["", "dns", "dddns"]
                            currentIndex: Math.max(0, values.indexOf(card.hostnameLookupDraft))
                            onActivated: {
                                card.hostnameLookupDraft = values[currentIndex]
                                card.resetDddnsCredentials()
                            }
                            enabled: !nodeColumn.locked && hostnameField.text.trim() !== ""
                        }
                        Label {
                            visible: card.hostnameUsesDddns
                            text: "DDDNS update token"
                            color: Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        StyledTextField {
                            id: dddnsTokenField
                            objectName: "dddns-token-" + member.modelData.host_name
                            Layout.fillWidth: true
                            visible: card.hostnameUsesDddns
                            enabled: !nodeColumn.locked
                            echoMode: TextInput.Password
                            text: member.editingHostname ? card.dddnsTokenDraft : ""
                            onTextEdited: card.dddnsTokenDraft = text
                            placeholderText: "Used once to update this hostname; never saved"
                        }
                        Label {
                            visible: card.hostnameUsesDddns
                            text: card.defaultDddnsUpdateUrl(card.hostnameDraft) !== ""
                                ? "DDDNS update endpoint (fixed)" : "DDDNS update URL"
                            color: Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        StyledTextField {
                            objectName: "dddns-update-url-" + member.modelData.host_name
                            Layout.fillWidth: true
                            visible: card.hostnameUsesDddns
                            enabled: !nodeColumn.locked
                            text: member.editingHostname ? card.dddnsUpdateUrlDraft : ""
                            onTextEdited: card.dddnsUpdateUrlDraft = text
                            placeholderText: readOnly ? "" : "Your provider's HTTPS update endpoint"
                            readOnly: card.defaultDddnsUpdateUrl(card.hostnameDraft) !== ""
                        }
                        Label {
                            Layout.fillWidth: true
                            text: (card.hostnameUsesDddns
                                   ? "Apply immediately points this name at this server's signaling addresses and port, "
                                     + "replacing the previous record. The token is sent to the HTTPS URL above and is never saved. "
                                   : "Clients try this name if the saved IP stops working. Automatic uses DDDNS "
                                     + "under doubleslash.space and DNS elsewhere. Use a name you already control. ")
                                + "Apply restarts this server and briefly interrupts its connections. "
                                + "Leave the hostname empty to remove it."
                            wrapMode: Text.WordWrap
                            color: Theme.muted
                            font.pixelSize: Theme.fontSizeCaption
                        }
                        RowLayout {
                            StyledButton {
                                objectName: "apply-hostname-" + member.modelData.host_name
                                text: card.hostnameUsesDddns ? "Apply and update DDDNS" : "Apply and restart"
                                enabled: !nodeColumn.locked && (!card.hostnameUsesDddns
                                    || (card.dddnsTokenDraft !== "" && card.dddnsUpdateUrlDraft.trim() !== ""))
                                onClicked: {
                                    var request = {
                                        op: "set_hostname", host_name: member.modelData.host_name,
                                        hostname: hostnameField.text.trim(),
                                        hostname_lookup: hostnameLookup.values[hostnameLookup.currentIndex]
                                    }
                                    if (card.hostnameUsesDddns) {
                                        request.dddns_token = card.dddnsTokenDraft
                                        request.dddns_update_url = card.dddnsUpdateUrlDraft.trim()
                                    }
                                    card.dddnsTokenDraft = ""
                                    dddnsTokenField.clear()
                                    card.send(nodeColumn.node.id, request)
                                }
                            }
                            StyledButton {
                                text: "Cancel"
                                enabled: !nodeColumn.locked
                                onClicked: card.editingHostnameKey = ""
                            }
                        }
                    }

                    // Remove one server: uninstall it, or drop one that is gone.
                    ColumnLayout {
                        objectName: "remove-server-confirm-" + member.modelData.host_name
                        visible: member.removing
                        Layout.fillWidth: true
                        Layout.leftMargin: 12 + Theme.spacingXs
                        spacing: Theme.spacingSm
                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            textFormat: Text.PlainText
                            color: Theme.danger
                            font.pixelSize: Theme.fontSizeCaption
                            text: "Are you sure? Uninstall stops the supernode on "
                                + member.modelData.host_name + " and removes it from that server "
                                + "(the firewall and security settings stay). Remove without "
                                + "contacting it is for a server you can no longer reach: it "
                                + "keeps running if it is still up. Either way the other servers "
                                + "get a member list without it and restart one at a time."
                        }
                        SettingSwitch {
                            id: serverPurgeSwitch
                            Layout.fillWidth: true
                            title: "Also delete this server's data and identity"
                            checked: false
                            onChanged: function(value) { serverPurgeSwitch.checked = value }
                        }
                        Flow {
                            Layout.fillWidth: true
                            spacing: Theme.spacingSm
                            ServerButton {
                                objectName: "uninstall-server-" + member.modelData.host_name
                                tip: "Uninstall and remove"
                                danger: true
                                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/trash.svg"
                                enabled: !nodeColumn.locked
                                onClicked: {
                                    card.pendingServerRemove = ""
                                    card.send(nodeColumn.node.id, {
                                        op: "remove_server", host_name: member.modelData.host_name,
                                        uninstall: true, purge: serverPurgeSwitch.checked
                                    })
                                }
                            }
                            ServerButton {
                                objectName: "drop-server-" + member.modelData.host_name
                                tip: "Remove without contacting it"
                                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/forget.svg"
                                enabled: !nodeColumn.locked
                                onClicked: {
                                    card.pendingServerRemove = ""
                                    card.send(nodeColumn.node.id, {
                                        op: "remove_server", host_name: member.modelData.host_name,
                                        uninstall: false
                                    })
                                }
                            }
                            ServerButton {
                                tip: "Cancel"
                                icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/close.svg"
                                onClicked: card.pendingServerRemove = ""
                            }
                        }
                    }
                    ResultPanel { result: member.info }
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
                    text: "Are you sure? Uninstall stops the supernode and removes it from "
                        + ((nodeColumn.node.hosts || []).length > 1 ? "all its servers" : "the server")
                        + " (the firewall and security settings stay). Delete its data "
                        + "too, and the node's identity is gone: everyone who uses it has to "
                        + "accept a new invite. Forget only removes it from this list; the "
                        + "servers keep running."
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
                        enabled: !nodeColumn.locked
                        onClicked: {
                            card.pendingRemoveId = ""
                            card.send(nodeColumn.node.id, { op: "uninstall", purge: purgeSwitch.checked })
                        }
                    }
                    ServerButton {
                        tip: "Forget"
                        icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/forget.svg"
                        enabled: !nodeColumn.locked
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

            ResultPanel {
                result: nodeColumn.info
                visible: nodeColumn.expanded && ((result.output || "") !== "" || (result.logs || "") !== "")
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
    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.spacingSm
        ServerButton {
            tip: card.showNotice ? "Hide the notice" : "Read the full notice"
            icon.source: card.showNotice
                ? "qrc:/qt/qml/DoubleSlash/Client/icons/close.svg"
                : "qrc:/qt/qml/DoubleSlash/Client/icons/info.svg"
            onClicked: card.showNotice = !card.showNotice
        }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.RichText
            font.pixelSize: Theme.fontSizeCaption
            color: Theme.accent
            text: "Help: <a href=\"https://doubleslash.space/support.html\"><font color=\""
                + Theme.accent + "\">doubleslash.space/support</font></a>"
            onLinkActivated: (link) => Qt.openUrlExternally(link)
            HoverHandler { cursorShape: Qt.PointingHandCursor }
        }
    }
}
