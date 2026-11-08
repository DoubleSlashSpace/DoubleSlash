import QtQuick
import QtQuick.Controls
import QtTest
import DoubleSlash.Client 1.0

Item {
    width: 720
    height: 1000

    QtObject {
        id: backend
        property string supernode_admin_state: "{}"
        property bool supernode_admin_busy: false
        property var requests: []
        signal supernodeAdminEvent(string event_json)
        function supernodeCommand(raw) {
            requests = requests.concat([JSON.parse(raw)])
        }
        function copyToClipboard(text) {}
        function avatarSvg(peerId, configJson) { return "<svg xmlns='http://www.w3.org/2000/svg'/>" }
        function avatarTintColor(peerId, configJson) { return "#888888" }
        function avatarImageSmooth(peerId, configJson) { return true }
        function finish(request, value, error) {
            supernodeAdminEvent(JSON.stringify({
                kind: "result", request_id: request.request_id, op: request.op,
                ok: !error, value: value || {}, error: error || ""
            }))
        }
    }

    SupernodeAdminCard {
        id: card
        width: parent.width
        appBackend: backend
        autoStatus: false
    }

    TestCase {
        name: "SupernodeAdminTree"
        when: windowShown

        function host(name) {
            return {
                host_name: name, public_host: "192.0.2.1",
                public_ip: "192.0.2.1", ws_port: name === "second" ? 41000 : 34935,
                ipv4: "192.0.2.1", ipv6: null,
                relay_port: 3478,
                ssh_target: "root@192.0.2.1:22", os: "Debian 12",
                platform: "linux-x86_64", access: "own-key",
                host_key_fingerprint: "SHA256:test", node_identity: name
            }
        }

        function init() {
            backend.supernode_admin_state = "{}"
            card.nodeInfo = ({})
            card.pending = ({})
            card.openNodeId = ""
            card.openChosen = false
            card.pendingRemoveId = ""
            card.pendingServerRemove = ""
            card.renamingId = ""
            card.editingHostnameKey = ""
            card.hostnameDraft = ""
            card.hostnameLookupDraft = ""
            card.dddnsTokenDraft = ""
            card.dddnsUpdateUrlDraft = ""
            card.filterText = ""
            card.autoStatus = false
            card.width = 720
            backend.requests = []
            backend.supernode_admin_busy = false
            backend.supernode_admin_state = JSON.stringify({ nodes: [{
                id: "n1", name: "Supernode", channel: "nightly",
                update_mode: "client", cluster_id: "cluster",
                hosts: [host("first"), host("second"), host("third")]
            }] })
            tryCompare(card, "openNodeId", "n1")
        }

        function test_imported_hostname_targets_exact_instance_and_password_is_transient() {
            var nodes = JSON.parse(backend.supernode_admin_state).nodes
            nodes[0].imported = true
            nodes[0].hosts.forEach(function(h) { h.imported = true; h.active_at_import = true })
            backend.supernode_admin_state = JSON.stringify({nodes:nodes})
            card.autoStatus = true
            card.checkOnOpen("n1")
            compare(backend.requests.length, 0)
            verify(!findChild(card, "add-server-n1").visible)
            verify(!findChild(card, "sync-n1").visible)
            verify(!findChild(card, "remove-server-second").visible)
            compare(card.memberStatus("n1", nodes[0].hosts[1]), "Running at import")
            findChild(card, "edit-hostname-second").clicked()
            var field = findChild(card, "hostname-field-second")
            field.text = "moved.doubleslash.space"
            field.textEdited()
            compare(backend.requests.length, 0, "imported hosts do not auto-submit before password entry")
            var password = findChild(card, "hostname-ssh-password-second")
            verify(password.visible)
            compare(password.echoMode, TextInput.Password)
            password.text = "test-password"
            password.textEdited()
            card.dddnsTokenDraft = "ddt_test"
            findChild(card, "apply-hostname-second").clicked()
            var apply = backend.requests[0]
            compare(apply.op, "set_hostname")
            compare(apply.host_name, "second")
            compare(apply.ssh_password, "test-password")
            compare(apply.dddns_token, "ddt_test")
            compare(card.sshPasswordDraft, "")
            compare(password.text, "")
            compare(card.dddnsTokenDraft, "")
            backend.finish(apply, {}, "Retry")
            card.sshPasswordDraft = "discard"
            card.editingHostnameKey = ""
            compare(card.sshPasswordDraft, "")
            card.sendEach("update_now", false)
            compare(backend.requests.length, 1, "bulk updates skip imported deployments")
        }

        function test_numbered_tree_and_info_toggle() {
            var header = findChild(card, "supernode-n1")
            verify(header !== null)
            for (var i = 0; i < 3; i++) {
                var name = ["first", "second", "third"][i]
                var details = findChild(card, "details-" + name)
                verify(details !== null)
                compare(details.visible, false)
                findChild(card, "info-" + name).clicked()
                tryCompare(details, "visible", true)
                verify(details.text.indexOf("SSH: root@192.0.2.1:22") >= 0)
                findChild(card, "info-" + name).clicked()
                tryCompare(details, "visible", false)
            }
            header.clicked()
            compare(card.openNodeId, "")
            tryVerify(function() { return findChild(card, "status-first") === null })
        }

        function test_member_action_targets_only_that_server() {
            findChild(card, "status-second").clicked()
            var request = backend.requests[backend.requests.length - 1]
            compare(request.op, "status")
            compare(request.node_id, "n1")
            compare(request.host_name, "second")
            verify(card.working(card.nodes[0]))
            verify(!findChild(card, "status-first").enabled)
            backend.finish(request, { hosts: [{
                host_name: "second", reachable: true, active: true, version: "1.0.1"
            }] })
            compare(card.memberStatus("n1", host("second")), "Running")
            compare(card.memberStatus("n1", host("first")), "Not checked")
            verify(!card.working(card.nodes[0]))
        }

        function test_connection_details_are_visible_before_status_or_info() {
            var summary = findChild(card, "connection-details-second")
            verify(summary.visible)
            verify(summary.readOnly && summary.selectByMouse)
            verify(summary.text.indexOf("IPv4: 192.0.2.1") >= 0)
            verify(summary.text.indexOf("IPv6: Not checked") >= 0)
            verify(summary.text.indexOf("WebSocket signaling port (TCP): 41000") >= 0)
            verify(summary.text.indexOf("Relay port (UDP): 3478") >= 0)
            compare(card.info("n1", "second").status, undefined)
            verify(!findChild(card, "details-second").visible)
            findChild(card, "info-second").clicked()
            verify(findChild(card, "details-second").text.indexOf("TCP): 41000") >= 0)
            var missing = host("missing")
            delete missing.ws_port
            delete missing.relay_port
            verify(card.connectionDetails(missing).indexOf("TCP): Unavailable") >= 0)
        }

        function test_hostname_shows_both_families_and_read_does_not_block_apply() {
            findChild(card, "edit-hostname-second").clicked()
            var field = findChild(card, "hostname-field-second")
            field.text = "myserver.doubleslash.space"
            field.textEdited()
            var lookup = backend.requests[backend.requests.length - 1]
            compare(lookup.op, "connection_info")
            compare(lookup.host_name, "second")
            card.dddnsTokenDraft = "ddt_test"
            verify(findChild(card, "apply-hostname-second").enabled)
            verify(!findChild(card, "refresh-addresses-second").enabled)
            findChild(card, "apply-hostname-second").clicked()
            var apply = backend.requests[backend.requests.length - 1]
            backend.finish(lookup, { ipv4: "198.51.100.7", ipv6: "2606:4700::1234",
                                   ws_port: 41000, relay_port: 3478, signaling_ipv6: false })
            verify(card.working(card.nodes[0]), "read response must not release the apply task")
            var text = findChild(card, "hostname-addresses-second").text
            verify(text.indexOf("IPv4: 198.51.100.7") >= 0)
            verify(text.indexOf("IPv6: 2606:4700::1234") >= 0)
            verify(text.indexOf("signaling is IPv4 only") >= 0)
            verify(card.connectionDetails(host("second"), {
                ipv6: "2606:4700::1234", signaling_ipv6: false, dual_stack_on_apply: true,
                ws_port: 41000
            }).indexOf("Apply will enable dual stack") >= 0)
            verify(text.indexOf("TCP): 41000") >= 0)
            backend.finish(apply, {}, "Retry apply")
            findChild(card, "refresh-addresses-second").clicked()
            var refresh = backend.requests[backend.requests.length - 1]
            backend.finish(refresh, {}, "SSH unavailable")
            verify(findChild(card, "hostname-addresses-second").text.indexOf("2606:4700::1234") >= 0)
            compare(card.info("n1", "second").addressesError, "SSH unavailable")
        }

        function test_hostname_edit_targets_one_server_and_survives_errors() {
            findChild(card, "edit-hostname-second").clicked()
            var field = findChild(card, "hostname-field-second")
            verify(field.visible)
            field.text = "node.names.example"
            field.textEdited()
            var lookup = findChild(card, "hostname-lookup-second")
            lookup.currentIndex = 2
            lookup.activated(2)
            card.dddnsTokenDraft = "ddt_test"
            card.dddnsUpdateUrlDraft = "https://users.names.example/v1/update"
            findChild(card, "apply-hostname-second").clicked()
            var request = backend.requests[backend.requests.length - 1]
            compare(request.op, "set_hostname")
            compare(request.node_id, "n1")
            compare(request.host_name, "second")
            compare(request.hostname, "node.names.example")
            compare(request.hostname_lookup, "dddns")
            compare(request.dddns_token, "ddt_test")
            compare(card.dddnsTokenDraft, "")
            verify(!findChild(card, "apply-hostname-second").enabled)
            // The bridge republishes state when a task takes/releases its lock.
            var state = JSON.parse(backend.supernode_admin_state)
            state.busy_nodes = ["n1"]
            backend.supernode_admin_state = JSON.stringify(state)
            tryCompare(findChild(card, "hostname-field-second"), "text", "node.names.example")
            state.busy_nodes = []
            backend.supernode_admin_state = JSON.stringify(state)
            backend.finish(request, {}, "SSH unavailable")
            verify(findChild(card, "hostname-field-second").visible)
            compare(card.hostnameDraft, "node.names.example")
            compare(card.info("n1", "second").output, "SSH unavailable")
            card.dddnsTokenDraft = "ddt_retry"
            findChild(card, "apply-hostname-second").clicked()
            backend.finish(backend.requests[backend.requests.length - 1], { lines: ["Applied"] })
            compare(card.editingHostnameKey, "")
        }

        function test_dddns_token_is_masked_cleared_and_scoped_to_the_apply() {
            findChild(card, "edit-hostname-first").clicked()
            var hostname = findChild(card, "hostname-field-first")
            hostname.text = "node.doubleslash.space"
            hostname.textEdited()
            var token = findChild(card, "dddns-token-first")
            verify(token.visible)
            compare(token.echoMode, TextInput.Password)
            compare(findChild(card, "dddns-update-url-first").text,
                    "https://users.doubleslash.space/v1/update")
            verify(findChild(card, "dddns-update-url-first").readOnly)
            verify(!findChild(card, "apply-hostname-first").enabled)
            token.text = "ddt_test_secret"
            token.textEdited()
            verify(findChild(card, "apply-hostname-first").enabled)
            findChild(card, "apply-hostname-first").clicked()
            var sent = backend.requests[backend.requests.length - 1]
            compare(sent.dddns_token, "ddt_test_secret")
            compare(card.dddnsTokenDraft, "")
            compare(token.text, "")
            verify(JSON.stringify(card.nodeInfo).indexOf("ddt_test_secret") < 0)
            verify(backend.supernode_admin_state.indexOf("ddt_test_secret") < 0)
            backend.finish(sent, {}, "Hostname applied; DDDNS could not be confirmed")
            verify(!findChild(card, "apply-hostname-first").enabled, "retry needs token re-entry")
            findChild(card, "edit-hostname-second").clicked()
            compare(card.dddnsTokenDraft, "")
            compare(findChild(card, "dddns-token-second").text, "")
        }

        function test_hostname_load_clear_and_filter() {
            var list = [node("n1", "Supernode", ["first", "second"])]
            list[0].hosts[1].hostname = "relay.example.com"
            list[0].hosts[1].hostname_lookup = "dns"
            setNodes(list)
            tryVerify(function() { return findChild(card, "edit-hostname-second") !== null })
            findChild(card, "edit-hostname-second").clicked()
            compare(findChild(card, "hostname-field-second").text, "relay.example.com")
            compare(findChild(card, "hostname-lookup-second").currentIndex, 1)
            card.filterText = "relay.example.com"
            compare(card.listedNodes().length, 1)
            card.hostnameDraft = ""
            verify(!findChild(card, "hostname-lookup-second").enabled)
            findChild(card, "apply-hostname-second").clicked()
            compare(backend.requests[backend.requests.length - 1].hostname, "")
        }

        function test_cluster_and_member_status_merge_by_host_name() {
            card.send("n1", { op: "status" })
            var request = backend.requests[backend.requests.length - 1]
            verify(!("host_name" in request))
            backend.finish(request, { hosts: [
                { host_name: "third", reachable: false, error: "SSH unavailable" },
                { host_name: "first", reachable: true, active: true }
            ] })
            compare(card.memberStatus("n1", host("first")), "Running")
            compare(card.memberStatus("n1", host("third")), "Unreachable")
            card.send("n1", { op: "status", host_name: "second" })
            backend.finish(backend.requests[backend.requests.length - 1], { hosts: [
                { host_name: "second", reachable: true, active: false, state: "inactive" }
            ] })
            compare(card.memberStatus("n1", host("first")), "Running")
            compare(card.memberStatus("n1", host("second")), "inactive")
            verify(card.memberDetails("n1", host("third")).indexOf("SSH unavailable") >= 0)
        }

        function test_notices_errors_and_logs_stay_with_their_member() {
            card.send("n1", { op: "update_now", host_name: "second" })
            backend.finish(backend.requests[backend.requests.length - 1],
                           { lines: ["second: already running the latest build"] })
            compare(card.info("n1", "second").output, "second: already running the latest build")
            verify(!card.info("n1", "second").failed)
            compare(card.info("n1", "first").output, undefined)
            card.send("n1", { op: "restart", host_name: "third" })
            backend.finish(backend.requests[backend.requests.length - 1], {}, "Restart failed")
            verify(card.info("n1", "third").failed)
            compare(card.info("n1", "third").output, "Restart failed")
            card.send("n1", { op: "logs", host_name: "first", lines: 200 })
            backend.finish(backend.requests[backend.requests.length - 1], { text: "first log" })
            compare(card.info("n1", "first").logs, "first log")
            compare(card.info("n1", "second").logs, "")
        }

        function test_member_update_checks_preserve_other_members() {
            card.send("n1", { op: "check_update" })
            backend.finish(backend.requests[backend.requests.length - 1], {
                update_available: true, hosts: [
                    { host_name: "first", update_available: true },
                    { host_name: "second", update_available: false }
                ]
            })
            verify(card.updateAvailable(card.nodes[0]))
            card.send("n1", { op: "check_update", host_name: "second" })
            backend.finish(backend.requests[backend.requests.length - 1], {
                update_available: false, hosts: [{ host_name: "second", update_available: false }]
            })
            verify(card.updateAvailable(card.nodes[0]))
            card.send("n1", { op: "update_now", host_name: "first" })
            backend.finish(backend.requests[backend.requests.length - 1], {
                lines: ["first: update complete"],
                hosts: [{ host_name: "first", update_available: false }]
            })
            verify(!card.updateAvailable(card.nodes[0]))
        }

        function test_dense_controls_fit_narrow_card() {
            card.width = 460
            wait(50)
            var first = findChild(card, "status-first")
            verify(first.width <= 28)
            var last = findChild(card, "remove-server-first")
            verify(last.visible)
            var right = last.mapToItem(card, last.width, 0).x
            verify(right <= card.width - Theme.spacingLg, "Member controls must fit the card")
            findChild(card, "edit-hostname-first").clicked()
            var apply = findChild(card, "apply-hostname-first")
            wait(20)
            verify(apply.mapToItem(card, apply.width, 0).x <= card.width - Theme.spacingLg)
            var field = findChild(card, "hostname-field-first")
            verify(field.mapToItem(card, field.width, 0).x <= card.width - Theme.spacingLg)
        }

        function node(id, name, hosts) {
            return { id: id, name: name, channel: "nightly", update_mode: "client",
                     cluster_id: hosts.length > 1 ? "c-" + id : null,
                     hosts: hosts.map(function(h) { return host(h) }) }
        }

        function setNodes(list, extra) {
            var state = Object.assign({ nodes: list }, extra || {})
            backend.supernode_admin_state = JSON.stringify(state)
        }

        function test_names_sort_naturally() {
            setNodes([node("a", "Node 10", ["a1"]), node("b", "node 2", ["b1"]),
                      node("c", "Alpha", ["c1"])])
            compare(card.sortedNodes().map(function(n) { return n.name }),
                    ["Alpha", "node 2", "Node 10"])
        }

        function test_busy_supernode_locks_only_its_own_changes() {
            setNodes([node("a", "A", ["a1", "a2"]), node("b", "B", ["b1"])],
                     { busy_nodes: ["a"] })
            card.openNodeId = "a"
            tryVerify(function() { return findChild(card, "remove-server-a1") !== null })
            verify(!findChild(card, "remove-server-a1").enabled)
            verify(!findChild(card, "add-server-a").enabled)
            verify(findChild(card, "status-a1").enabled, "reads stay available")
            verify(findChild(card, "add-server-b").enabled)
            verify(card.statusLine(card.nodes[0]).indexOf("busy") >= 0)
            setNodes([node("a", "A", ["a1", "a2"]), node("b", "B", ["b1"])],
                     { setup_running: true })
            verify(!findChild(card, "add-server-b").enabled, "one setup at a time")
            verify(!findChild(card, "host-supernode").enabled)
            verify(findChild(card, "remove-server-a1").enabled)
        }

        function test_checks_across_supernodes_skip_busy_ones_and_stay_folded() {
            setNodes([node("a", "A", ["a1"]), node("b", "B", ["b1"]), node("c", "C", ["c1"])],
                     { busy_nodes: ["c"] })
            card.openNodeId = ""
            backend.requests = []
            verify(findChild(card, "check-all").visible)
            findChild(card, "check-all").clicked()
            compare(backend.requests.map(function(r) { return r.node_id }), ["a", "b"])
            verify(backend.requests.every(function(r) { return r.op === "status" }))
            compare(card.openNodeId, "")
            backend.requests.forEach(function(r) {
                backend.finish(r, { hosts: [{ host_name: r.node_id + "1",
                                              reachable: r.node_id === "a", active: true }] })
            })
            compare(card.health(card.nodes[0]).running, 1)
            compare(card.health(card.nodes[1]).down, 1)
            verify(card.statusLine(card.nodes[1]).indexOf("1 down") >= 0)
            compare(card.fleetSummary().indexOf("3 supernodes \u00b7 3 servers"), 0)
        }

        function test_update_all_only_sends_to_supernodes_with_updates() {
            setNodes([node("a", "A", ["a1"]), node("b", "B", ["b1"])])
            verify(!findChild(card, "update-all").enabled)
            card.patch("b", { updateAvailable: true }, "b1")
            verify(findChild(card, "update-all").enabled)
            backend.requests = []
            findChild(card, "update-all").clicked()
            compare(backend.requests.length, 1)
            compare(backend.requests[0].op, "update_now")
            compare(backend.requests[0].node_id, "b")
        }

        function test_filter_appears_for_long_lists_and_matches_addresses() {
            setNodes([node("a", "A", ["a1"]), node("b", "B", ["b1"])])
            verify(!findChild(card, "server-filter").visible)
            var many = []
            for (var i = 0; i < card.filterThreshold; i++)
                many.push(node("n" + i, "Node " + i, ["s" + i]))
            many[3].hosts[0].public_host = "203.0.113.44"
            setNodes(many)
            verify(findChild(card, "server-filter").visible)
            card.filterText = "113.44"
            compare(card.listedNodes().length, 1)
            compare(card.listedNodes()[0].id, "n3")
            card.filterText = "node"
            compare(card.listedNodes().length, card.filterThreshold)
        }

        function test_removing_a_server_confirms_then_reports_on_the_node() {
            compare(findChild(card, "remove-server-confirm-second").visible, false)
            findChild(card, "remove-server-second").clicked()
            tryCompare(findChild(card, "remove-server-confirm-second"), "visible", true)
            findChild(card, "drop-server-second").clicked()
            var request = backend.requests[backend.requests.length - 1]
            compare(request.op, "remove_server")
            compare(request.host_name, "second")
            compare(request.uninstall, false)
            compare(card.pendingServerRemove, "")
            backend.finish(request, { lines: ["cluster relinked"] })
            compare(card.info("n1").output, "cluster relinked")
            compare(card.info("n1", "second").output, undefined)
        }

        function test_last_server_has_no_remove() {
            setNodes([node("solo", "Solo", ["only"])])
            card.openNodeId = "solo"
            tryVerify(function() { return findChild(card, "remove-server-only") !== null })
            verify(!findChild(card, "remove-server-only").visible)
            verify(!findChild(card, "sync-solo").visible)
        }

        function test_rename_sends_the_new_name() {
            findChild(card, "rename-n1").clicked()
            var field = findChild(card, "rename-field-n1")
            tryCompare(field, "visible", true)
            compare(field.text, "Supernode")
            field.text = "Edge west"
            field.accepted()
            var request = backend.requests[backend.requests.length - 1]
            compare(request.op, "rename")
            compare(request.name, "Edge west")
            compare(card.renamingId, "")
        }

        function test_opening_an_unchecked_supernode_checks_it_once() {
            setNodes([node("a", "A", ["a1"]), node("b", "B", ["b1"])])
            card.openNodeId = ""
            card.autoStatus = true
            backend.requests = []
            card.toggleNode("b")
            tryVerify(function() { return backend.requests.length === 1 })
            compare(backend.requests[0].op, "status")
            compare(backend.requests[0].node_id, "b")
            backend.finish(backend.requests[0],
                           { hosts: [{ host_name: "b1", reachable: true, active: true }] })
            card.toggleNode("b")
            card.toggleNode("b")
            wait(20)
            compare(backend.requests.length, 1, "already checked")
        }
    }
}
