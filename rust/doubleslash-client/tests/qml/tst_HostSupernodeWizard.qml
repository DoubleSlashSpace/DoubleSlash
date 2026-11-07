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
        function cleanup() { wizard.close(); tryCompare(wizard, "visible", false) }

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
