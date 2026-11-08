import QtQuick
import QtQuick.Controls
import QtTest
import DoubleSlash.Client 1.0

Item {
    width: 720
    height: 760
    ApplicationWindow {
        id: window
        width: 720
        height: 760
        visible: true
        QtObject {
            id: backend
            property string supernode_admin_state: '{"notice_accepted":true}'
            property var requests: []
            signal supernodeAdminEvent(string event_json)
            function supernodeCommand(raw) { requests = requests.concat([JSON.parse(raw)]) }
        }
        HostSupernodeWizard { id: wizard; appBackend: backend }
    }
    TestCase {
        name: "HostSupernodeDddns"
        when: windowShown
        // Repeater delegates in a Popup are visual children of its content,
        // while their QObject ownership can live in the delegate model.
        function visualChild(item, name) {
            if (item.objectName === name) return item
            var children = item.children || []
            for (var i = 0; i < children.length; i++) {
                var found = visualChild(children[i], name)
                if (found) return found
            }
            return null
        }
        function init() {
            wizard.openFor("", "")
            tryCompare(wizard, "opened", true)
            var hostname = findChild(wizard, "setup-hostname")
            hostname.text = "node.doubleslash.space"
            hostname.textEdited()
            var lookup = findChild(wizard, "setup-hostname-lookup")
            lookup.currentIndex = 0
            lookup.activated(0)
            backend.requests = []
        }
        function cleanup() { wizard.close(); tryCompare(wizard, "visible", false); backend.supernode_admin_state = '{"notice_accepted":true}' }

        function test_token_only_travels_with_setup_and_is_cleared() {
            var token = findChild(wizard, "setup-dddns-token")
            verify(token.visible)
            compare(token.echoMode, TextInput.Password)
            compare(findChild(wizard, "setup-dddns-update-url").text,
                    "https://users.doubleslash.space/v1/update")
            verify(findChild(wizard, "setup-dddns-update-url").readOnly)
            token.text = "ddt_test_secret"
            wizard.startSetup()
            var request = backend.requests[0]
            compare(request.dddns_token, "ddt_test_secret")
            compare(request.hostname, "node.doubleslash.space")
            compare(token.text, "")
            backend.supernodeAdminEvent(JSON.stringify({
                kind: "result", request_id: request.request_id, op: "deploy", ok: true,
                value: { warning: "Server ready, DDDNS update needs retry" }
            }))
            compare(wizard.page, "done")
            var result = findChild(wizard, "setup-dddns-result")
            verify(result.visible)
            compare(result.text, "Server ready, DDDNS update needs retry")
            token.text = "ddt_another_secret"
            wizard.close()
            compare(token.text, "")
        }

        function test_import_scans_then_selects_without_deploy_or_credentials_in_commit() {
            wizard.close()
            tryCompare(wizard, "visible", false)
            backend.supernode_admin_state = '{"notice_accepted":false}'
            wizard.openImport()
            tryCompare(wizard, "opened", true)
            compare(wizard.page, "server")
            verify(!findChild(wizard, "setup-hostname").visible)
            verify(!wizard.hostnameUsesDddns)
            findChild(wizard, "setup-server-address").text = "198.51.100.7"
            var password = findChild(wizard, "setup-ssh-password")
            password.text = "test-password"
            wizard.hostKey = "ssh-ed25519 confirmed"
            wizard.page = "hostkey"
            backend.requests = []
            findChild(wizard, "confirm-server-fingerprint").clicked()
            compare(wizard.page, "scan")
            compare(password.text, "")
            var scan = backend.requests[0]
            compare(scan.op, "discover_existing")
            compare(scan.ssh_password, "test-password")
            verify(!("hardening" in scan))
            verify(!("dddns_token" in scan))
            backend.supernodeAdminEvent(JSON.stringify({kind:"result",op:scan.op,request_id:scan.request_id,ok:true,
                value:{discovery_id:"snapshot",instances:[
                    {unit:"doubleslash-supernode@a.service",instance_id:"a",public_host:"198.51.100.7",ws_port:41000,relay_port:42000,data_dir:"/srv/a",cluster_id:"fleet",cluster_members:3},
                    {unit:"doubleslash-supernode@b.service",instance_id:"b",public_host:"198.51.100.7",ws_port:41001,relay_port:42001,data_dir:"/srv/b",cluster_id:"fleet",cluster_members:3}
                ]}}))
            compare(wizard.page, "preview")
            compare(wizard.selectedUnits.length, 2)
            tryVerify(function() { return visualChild(wizard.contentItem, "import-unit-a") !== null })
            var selected = visualChild(wizard.contentItem, "import-unit-a")
            verify(selected.checked)
            selected.checked = false
            selected.toggled()
            findChild(wizard, "import-selected-instances").clicked()
            var commit = backend.requests[1]
            compare(commit.op, "import_existing")
            compare(commit.discovery_id, "snapshot")
            compare(commit.units.length, 1)
            compare(commit.units[0], "doubleslash-supernode@b.service")
            verify(!("ssh_password" in commit))
            backend.supernodeAdminEvent(JSON.stringify({kind:"result",op:commit.op,request_id:commit.request_id,ok:true,value:{imported_instances:1}}))
            compare(wizard.page, "done")
            compare(backend.requests.length, 2)
            backend.supernode_admin_state = '{"notice_accepted":true}'
        }

        function test_foreign_provider_needs_an_explicit_update_url() {
            var hostname = findChild(wizard, "setup-hostname")
            var token = findChild(wizard, "setup-dddns-token")
            token.text = "ddt_discard"
            hostname.text = "node.names.example"
            hostname.textEdited()
            var lookup = findChild(wizard, "setup-hostname-lookup")
            lookup.currentIndex = 2
            lookup.activated(2)
            compare(token.text, "")
            compare(findChild(wizard, "setup-dddns-update-url").text, "")
            token.text = "ddt_test"
            findChild(wizard, "setup-dddns-update-url").text = "https://users.names.example/v1/update"
            compare(wizard.serverRequest().dddns_update_url, "https://users.names.example/v1/update")
            lookup.currentIndex = 1
            lookup.activated(1)
            verify(!("dddns_token" in wizard.serverRequest()))
        }
    }
}
