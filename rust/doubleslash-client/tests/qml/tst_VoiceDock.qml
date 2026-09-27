import QtQuick
import QtTest
import DoubleSlash.Client 1.0

// The voice dock: where the call is, who in it is sharing video unwatched,
// and the controls — and the Peers | Rooms toggle that sits by the logo.
Item {
    width: 300
    height: 300

    VoiceDock {
        id: dock
        width: 280
        inRoom: true
        contextName: "Gaming › Raid Night"
        connectionMode: "direct"
    }

    SidebarToggle {
        id: toggle
        y: 250
    }

    SignalSpy { id: muteSpy; target: dock; signalName: "muteToggled" }
    SignalSpy { id: openSpy; target: dock; signalName: "openSessionRequested" }
    SignalSpy { id: toggleSpy; target: toggle; signalName: "activated" }

    TestCase {
        name: "VoiceDock"
        when: windowShown

        function init() {
            dock.muted = false
            dock.durationSecs = 0
            dock.unwatchedStreamers = []
            muteSpy.clear()
            openSpy.clear()
            toggleSpy.clear()
        }

        function textShown(item, text) {
            if (item.text === text && item.visible)
                return true
            var kids = item.children || []
            for (var i = 0; i < kids.length; i++) {
                if (textShown(kids[i], text))
                    return true
            }
            return false
        }

        function test_duration_reads_as_a_clock() {
            dock.durationSecs = 754
            compare(dock.durationText, "12:34")
            dock.durationSecs = 3725
            compare(dock.durationText, "01:02:05")
        }

        function test_names_the_room_and_opens_it() {
            verify(textShown(dock, "Gaming › Raid Night"))
            verify(textShown(dock, "Voice connected"))
        }

        function test_unwatched_streamers_are_announced() {
            verify(!textShown(dock, "Sam is sharing video"))
            dock.unwatchedStreamers = ["Sam"]
            verify(textShown(dock, "Sam is sharing video"))
            dock.unwatchedStreamers = ["Sam", "Ruth"]
            verify(textShown(dock, "Sam, Ruth are sharing video"))
        }

        function test_direct_call_says_so() {
            dock.inRoom = false
            dock.callState = "in_call"
            verify(textShown(dock, "In call"))
            dock.callState = "connecting"
            verify(textShown(dock, "Connecting…"))
            dock.inRoom = true
            dock.callState = "idle"
        }

        function test_toggle_reports_the_picked_list() {
            toggle.currentIndex = 0
            // The second segment is the right half.
            mouseClick(toggle, toggle.width * 0.75, toggle.height / 2)
            compare(toggleSpy.count, 1)
            compare(toggleSpy.signalArguments[0][0], 1)
        }
    }
}
