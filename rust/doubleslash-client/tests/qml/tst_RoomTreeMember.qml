import QtQuick
import QtTest
import DoubleSlash.Client 1.0

// A member row in the Rooms tree: what its inline actions offer depends on
// whether they are in our voice session, whether we trust them, and whether
// we are in their room at all.
Item {
    width: 300
    height: 400

    // Avatar asks the backend for its picture.
    QtObject {
        id: backend
        signal avatarConfigUpdated(string peer_id)
        function avatarSvg(peerId, configJson) { return "<svg xmlns='http://www.w3.org/2000/svg'/>" }
        function avatarTintColor(peerId, configJson) { return "#888888" }
        function avatarImageSmooth(peerId, configJson) { return true }
    }

    RoomTreeMember {
        id: member
        width: 300
        peerId: "peer-a"
        displayName: "Jonah"
        guides: [1, 3]
    }

    SignalSpy { id: toggleSpy; target: member; signalName: "toggleRequested" }
    SignalSpy { id: audioSpy; target: member; signalName: "localAudioChanged" }
    SignalSpy { id: inviteSpy; target: member; signalName: "inviteRequested" }
    SignalSpy { id: blockSpy; target: member; signalName: "blockRequested" }
    SignalSpy { id: purgeSpy; target: member; signalName: "blockAndPurgeRequested" }
    SignalSpy { id: popoutSpy; target: member; signalName: "popoutRequested" }
    SignalSpy { id: fullscreenSpy; target: member; signalName: "fullscreenRequested" }

    TestCase {
        name: "RoomTreeMember"
        when: windowShown

        function initTestCase() {
            var icons = {}
            ;["block", "trash", "video", "video-off", "speaker", "speaker-off", "speech", "invite", "check", "popout", "expand"].forEach(function(name) {
                icons[name + ".svg"] = Qt.resolvedUrl("../../qml/icons/" + name + ".svg")
            })
            Icons.overrides = icons
        }

        function init() {
            member.expanded = false
            member.inSession = false
            member.videoActive = false
            member.watching = false
            member.trusted = false
            member.blocked = false
            member.listPeerId = ""
            member.canInvite = false
            member.isSelf = false
            member.locallyMuted = false
            member.localVolume = 100
            toggleSpy.clear()
            audioSpy.clear()
            inviteSpy.clear()
            blockSpy.clear()
            purgeSpy.clear()
            popoutSpy.clear()
            fullscreenSpy.clear()
        }

        // Actions are SVG icons named by their tooltip, which is also their
        // accessible name; a text button is found by its label.
        function findButton(text) {
            return findChildMatching(member, function (item) {
                return (item.text === text || item.tip === text)
                    && item.visible && item.clicked !== undefined
            })
        }

        function findChildMatching(item, pred) {
            if (pred(item))
                return item
            var kids = item.children || []
            for (var i = 0; i < kids.length; i++) {
                var hit = findChildMatching(kids[i], pred)
                if (hit)
                    return hit
            }
            return null
        }

        function test_row_is_one_line_until_opened() {
            // Closing animates. A previous test leaves the row open, so the
            // height is mid-animation until that finishes. The settled closed
            // row is still exactly one line.
            tryCompare(member, "implicitHeight", member.rowHeight)
            member.expanded = true
            tryVerify(function () { return member.implicitHeight > member.rowHeight })
        }

        function test_clicking_the_row_asks_to_open() {
            mouseClick(member, 150, member.rowHeight / 2)
            compare(toggleSpy.count, 1)
        }

        function test_our_own_row_does_not_open() {
            member.isSelf = true
            mouseClick(member, 150, member.rowHeight / 2)
            compare(toggleSpy.count, 0)
        }

        function test_audio_controls_only_in_our_session() {
            member.expanded = true
            verify(findButton("Mute for me") === null)
            member.inSession = true
            var mute = findButton("Mute for me")
            verify(mute !== null)
            mute.clicked()
            compare(audioSpy.count, 1)
            compare(audioSpy.signalArguments[0][0], true)
        }

        function test_trusted_members_get_message_not_invite() {
            member.expanded = true
            member.canInvite = true
            member.trusted = true
            member.listPeerId = "list-a"
            verify(findButton("Message") !== null)
            verify(findButton("Invite to trusted peers") === null)
        }

        function test_strangers_can_be_blocked_without_trusting_them() {
            member.expanded = true
            var button = findButton("Block")
            verify(button !== null)
            button.clicked()
            compare(blockSpy.count, 1)
            compare(blockSpy.signalArguments[0][0], true)
            compare(purgeSpy.count, 0)
        }

        function test_purge_is_available_before_and_after_blocking() {
            member.expanded = true
            var purge = findButton("Block and purge messages")
            verify(purge !== null)
            purge.clicked()
            compare(purgeSpy.count, 1)
            compare(blockSpy.count, 0)
            member.blocked = true
            purge = findButton("Purge messages")
            verify(purge !== null)
            purge.clicked()
            compare(purgeSpy.count, 2)
            compare(blockSpy.count, 0)
        }

        function test_blocked_members_offer_unblock_and_no_contact_or_video() {
            member.expanded = true
            member.blocked = true
            member.canInvite = true
            member.inSession = true
            member.videoActive = true
            member.trusted = true
            member.listPeerId = "list-a"
            verify(findButton("Message") === null)
            verify(findButton("Invite to trusted peers") === null)
            verify(findButton("Watch video") === null)
            var button = findButton("Unblock")
            verify(button !== null)
            button.clicked()
            compare(blockSpy.signalArguments[0][0], false)
        }

        function test_own_identity_cannot_be_blocked() {
            member.isSelf = true
            member.expanded = true
            verify(findButton("Block") === null)
            verify(findButton("Block and purge messages") === null)
            verify(findButton("Purge messages") === null)
        }

        function test_invite_needs_a_room_we_are_in() {
            member.expanded = true
            verify(findButton("Invite to trusted peers") === null)
            member.canInvite = true
            var invite = findButton("Invite to trusted peers")
            verify(invite !== null)
            invite.clicked()
            compare(inviteSpy.count, 1)
        }

        // Nothing accepts a pasted peer ID, so the tree offers no copy.
        function test_no_copy_peer_id_action() {
            member.expanded = true
            member.inSession = true
            member.trusted = true
            member.listPeerId = "list-a"
            verify(findButton("Copy peer ID") === null)
        }

        function test_watching_needs_the_session_and_a_camera() {
            member.expanded = true
            member.videoActive = true
            verify(findButton("Watch video") === null)
            member.inSession = true
            verify(findButton("Watch video") !== null)
            member.watching = true
            verify(findButton("Stop watching") !== null)
        }

        function test_popout_and_fullscreen_need_a_live_camera() {
            member.expanded = true
            member.inSession = true
            verify(findButton("Pop out video") === null)
            verify(findButton("Full screen") === null)
            member.videoActive = true
            var popout = findButton("Pop out video")
            var full = findButton("Full screen")
            verify(popout !== null)
            verify(full !== null)
            popout.clicked()
            full.clicked()
            compare(popoutSpy.count, 1)
            compare(fullscreenSpy.count, 1)
        }
    }
}
