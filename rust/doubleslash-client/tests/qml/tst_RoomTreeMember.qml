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

    TestCase {
        name: "RoomTreeMember"
        when: windowShown

        function init() {
            member.expanded = false
            member.inSession = false
            member.videoActive = false
            member.watching = false
            member.trusted = false
            member.listPeerId = ""
            member.canInvite = false
            member.isSelf = false
            member.locallyMuted = false
            member.localVolume = 100
            toggleSpy.clear()
            audioSpy.clear()
            inviteSpy.clear()
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
            compare(member.implicitHeight, member.rowHeight)
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
    }
}
