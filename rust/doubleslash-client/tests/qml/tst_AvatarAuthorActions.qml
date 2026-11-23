import QtQuick
import QtQuick.Controls
import QtTest
import DoubleSlash.Client 1.0

Item {
    width: 360
    height: 300

    QtObject {
        id: backend
        property string public_id: "own-identity="
        signal avatarConfigUpdated(string peer_id)
        function avatarSvg(peerId, configJson) { return "<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'/>" }
        function avatarTintColor(peerId, configJson) { return "#888888" }
        function avatarImageSmooth(peerId, configJson) { return true }
    }

    Avatar {
        id: author
        x: 50
        y: 50
        peerId: "room-stranger="
        blockActionEnabled: true
    }
    SignalSpy { id: blockSpy; target: author; signalName: "blockRequested" }
    SignalSpy { id: purgeSpy; target: author; signalName: "blockAndPurgeRequested" }

    TestCase {
        name: "AvatarAuthorActions"
        when: windowShown

        function initTestCase() {
            Icons.overrides = { "block.svg": Qt.resolvedUrl("../../qml/icons/block.svg"), "trash.svg": Qt.resolvedUrl("../../qml/icons/trash.svg") }
        }

        function menu() { return findChild(author, "avatarAuthorMenu") }
        function action() { return findChild(author, "avatarBlockAction") }

        function init() {
            menu().close()
            tryCompare(menu(), "visible", false)
            author.peerId = "room-stranger="
            author.blockActionEnabled = true
            author.identityBlocked = false
            blockSpy.clear()
            purgeSpy.clear()
        }

        function test_click_opens_block_for_an_untrusted_author() {
            mouseClick(author)
            tryCompare(menu(), "visible", true)
            compare(action().text, "Block")
            mouseClick(action())
            compare(blockSpy.count, 1)
            compare(blockSpy.signalArguments[0][0], true)
            compare(purgeSpy.count, 0)
        }

        function test_purge_is_separate_from_plain_block_data() {
            return [{tag: "unblocked", blocked: false, label: "Block and purge messages"},
                    {tag: "already-blocked", blocked: true, label: "Purge messages"}]
        }
        function test_purge_is_separate_from_plain_block(data) {
            author.identityBlocked = data.blocked
            mouseClick(author)
            tryCompare(menu(), "visible", true)
            var purge = findChild(author, "avatarBlockPurgeAction")
            compare(purge.text, data.label)
            mouseClick(purge)
            compare(purgeSpy.count, 1)
            compare(blockSpy.count, 0)
        }

        function test_existing_block_offers_unblock() {
            author.identityBlocked = true
            mouseClick(author)
            tryCompare(menu(), "visible", true)
            compare(action().text, "Unblock")
            mouseClick(action())
            compare(blockSpy.signalArguments[0][0], false)
        }

        function test_own_avatars_have_no_actions_data() {
            return [ { tag: "padded", id: "own-identity=" }, { tag: "unpadded", id: "own-identity" }, { tag: "missing", id: "" } ]
        }
        function test_own_avatars_have_no_actions(data) {
            author.peerId = data.id
            verify(!author.canBlock)
            mouseClick(author)
            verify(!menu().visible)
            compare(blockSpy.count, 0)
            compare(purgeSpy.count, 0)
        }

        function test_ordinary_avatars_stay_display_only() {
            author.blockActionEnabled = false
            mouseClick(author)
            verify(!menu().visible)
        }

        function test_right_click_and_keyboard_open_the_author_menu() {
            mouseClick(author, 16, 16, Qt.RightButton)
            tryCompare(menu(), "visible", true)
            menu().close()
            tryCompare(menu(), "visible", false)
            author.forceActiveFocus()
            keyClick(Qt.Key_Space)
            tryCompare(menu(), "visible", true)
        }

        function test_recycled_author_closes_the_old_menu() {
            mouseClick(author)
            tryCompare(menu(), "visible", true)
            author.peerId = "different-author="
            tryCompare(menu(), "visible", false)
            compare(blockSpy.count, 0)
        }
    }
}
