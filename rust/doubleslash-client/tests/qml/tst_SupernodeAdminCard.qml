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
