import QtQuick
import QtQuick.Controls
import QtTest
import ConquerD.Client 1.0

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
            property string public_id: ""
            property bool backup_busy: false
            property string backup_result: ""
            property var requests: []
            function backupCommand(raw) {
                requests.push(JSON.parse(raw));
                backup_busy = true;
                backup_result = "";
            }
            function lockIdentityAndQuit() {
            }
            function finish(reply) {
                backup_busy = false;
                backup_result = JSON.stringify(reply);
            }
        }
        BackupWizard {
            id: wizard
            appBackend: backend
        }
    }

    TestCase {
        name: "BackupWizard"
        when: windowShown

        function init() {
            wizard.close();
            tryCompare(wizard, "visible", false);
            backend.backup_busy = false;
            backend.backup_result = "";
            backend.public_id = "";
            backend.requests = [];
            window.width = 720;
            window.height = 760;
        }
        function cleanup() {
            wizard.close();
            tryCompare(wizard, "visible", false);
        }
        function openWizard() {
            wizard.open();
            tryCompare(wizard, "opened", true);
        }

        function test_locked_profiles_and_preview() {
            openWizard();
            compare(backend.requests[0].cmd, "profile.list");
            backend.finish({
                ok: true,
                profiles: [
                    {
                        profile: "original",
                        public_id: "example-identity"
                    }
                ]
            });
            compare(wizard.profiles.length, 1);
            wizard.page = "import";
            wizard.filePath = "C:/example.dbackup";
            var password = findChild(wizard, "backupPassword");
            var action = findChild(wizard, "backupPrimaryAction");
            verify(password !== null && action !== null);
            password.text = "backup passphrase";
            tryCompare(action, "enabled", true);
            action.clicked();
            compare(backend.requests[1].cmd, "backup.inspect");
            tryCompare(action, "enabled", false);
            backend.finish({
                ok: true,
                token: "preview-token",
                summary: {
                    public_id: "example-identity",
                    messages: 12,
                    attachments: 2,
                    missing_attachments: 1
                }
            });
            compare(wizard.page, "preview");
            compare(wizard.token, "preview-token");
            compare(password.text, "");
            password.text = "new local passphrase";
            findChild(wizard, "backupConfirmation").text = "different passphrase";
            tryCompare(action, "enabled", false);
            findChild(wizard, "backupConfirmation").text = password.text;
            tryCompare(action, "enabled", true);
            action.clicked();
            compare(backend.requests[2].cmd, "backup.restore");
            compare(backend.requests[2].token, "preview-token");
            backend.finish({
                ok: true,
                summary: {
                    messages: 12,
                    attachments: 2
                }
            });
            compare(wizard.page, "done");
            compare(password.text, "");
        }

        function test_export_errors_allow_retry() {
            backend.public_id = "example-identity";
            openWizard();
            compare(backend.requests.length, 0);
            wizard.page = "export";
            wizard.filePath = "C:/example.dbackup";
            var password = findChild(wizard, "backupPassword");
            var confirm = findChild(wizard, "backupConfirmation");
            var action = findChild(wizard, "backupPrimaryAction");
            password.text = "backup passphrase";
            confirm.text = password.text;
            action.clicked();
            backend.finish({
                ok: false,
                error: "Disk full"
            });
            compare(wizard.page, "export");
            compare(wizard.errorText, "Disk full");
            tryCompare(action, "enabled", true);
            action.clicked();
            compare(wizard.errorText, "");
            backend.finish({
                ok: true,
                summary: {
                    messages: 12,
                    attachments: 2,
                    missing_attachments: 0
                }
            });
            compare(wizard.page, "done");
            compare(password.text, "");
            compare(confirm.text, "");
        }

        function test_compact_window_and_secret_cleanup() {
            window.width = 390;
            window.height = 640;
            backend.public_id = "example-identity";
            openWizard();
            wait(50);
            verify(wizard.width <= window.width);
            verify(wizard.height <= window.height);
            wizard.page = "export";
            findChild(wizard, "backupPassword").text = "backup passphrase";
            wizard.close();
            tryCompare(findChild(wizard, "backupPassword"), "text", "");
        }
    }
}
