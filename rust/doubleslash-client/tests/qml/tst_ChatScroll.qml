import QtQuick
import QtTest
import DoubleSlash.Client 1.0

/*!
    Scroll behaviour of the chat lists: when the jump-to-current affordance is
    offered, and where a reader ends up when older history is paged in.

    Both used to be computed from \c contentY as though the scrollable range
    started at zero. It does not: a ListView's range runs from \c originY, and
    inserting at row 0 is exactly what moves that origin. Everything here fails
    on the arithmetic those two paths used before.
*/
Item {
    id: app
    width: 320
    height: 400

    // Rows deliberately vary in height, the way real messages do: Qt's origin
    // handling leans on an *average* row size, so uniform rows hide the cases
    // where an estimate is all it has.
    Component {
        id: viewComp
        ListView {
            width: 320
            height: 400
            spacing: 2
            model: ListModel {}
            delegate: Rectangle { width: 320; height: model.h; color: "grey" }
        }
    }

    Component {
        id: buttonComp
        JumpToCurrentButton {}
    }

    Component {
        id: anchorComp
        HistoryAnchor {}
    }

    TestCase {
        name: "ChatScroll"
        when: windowShown

        function rowHeight(i, vary) { return vary ? (30 + (i * 37) % 90) : 40 }

        function makeView(n, vary) {
            var lv = viewComp.createObject(app)
            for (var i = 0; i < n; ++i)
                lv.model.append({ h: rowHeight(i, vary) })
            lv.forceLayout()
            return lv
        }

        function prepend(lv, count, vary) {
            for (var j = 0; j < count; ++j)
                lv.model.insert(0, { h: rowHeight(j * 7, vary) })
        }

        // ---- JumpToCurrentButton -------------------------------------------

        function test_button_hidden_when_the_list_fits() {
            var lv = makeView(3, false)
            var btn = buttonComp.createObject(app, { list: lv })
            verify(!btn.awayFromLatest, "nothing to jump to when it all fits")
            btn.destroy(); lv.destroy()
        }

        function test_button_hidden_at_the_newest_message() {
            var lv = makeView(200, true)
            lv.positionViewAtEnd()
            lv.forceLayout()
            verify(!btn_away(lv), "offered a jump while already on the newest message")
        }

        function btn_away(lv) {
            var btn = buttonComp.createObject(app, { list: lv })
            var away = btn.awayFromLatest
            btn.destroy()
            return away
        }

        function test_button_shown_at_the_oldest_message() {
            var lv = makeView(200, true)
            lv.positionViewAtBeginning()
            lv.forceLayout()
            verify(btn_away(lv), "no way back offered from the top of the history")
            lv.destroy()
        }

        // The regression proper: an insert at row 0 moves originY, and the old
        // zero-based comparison then read the wrong answer at both ends.
        function test_button_tracks_both_ends_after_a_prepend() {
            var lv = makeView(200, true)
            lv.positionViewAtEnd()
            lv.forceLayout()
            prepend(lv, 50, true)
            lv.forceLayout()
            verify(lv.originY !== 0,
                "precondition: the prepend should have moved the origin")

            lv.positionViewAtEnd()
            lv.forceLayout()
            verify(!btn_away(lv), "button lingered at the newest message")

            lv.positionViewAtBeginning()
            lv.forceLayout()
            verify(btn_away(lv), "button hid at the oldest message")
            lv.destroy()
        }

        function test_button_appears_once_past_its_threshold() {
            var lv = makeView(200, false)
            lv.positionViewAtEnd()
            lv.forceLayout()
            var btn = buttonComp.createObject(app, { list: lv })

            // A nudge smaller than the threshold must not flash the button.
            lv.contentY -= 20
            lv.forceLayout()
            verify(!btn.awayFromLatest, "flashed after a single wheel notch")

            lv.contentY -= btn.threshold
            lv.forceLayout()
            verify(btn.awayFromLatest, "stayed hidden well past the threshold")
            btn.destroy(); lv.destroy()
        }

        // ---- HistoryAnchor --------------------------------------------------

        function test_history_anchor_never_moves_the_reader_data() {
            var rows = []
            var offsets = [0, 1, 10, 24, 47, 48, 49, 120, 124, 125, 400, 900, 3000]
            var pages = [1, 13, 50]
            for (var v = 0; v < 2; ++v)
                for (var p = 0; p < pages.length; ++p)
                    for (var i = 0; i < offsets.length; ++i)
                        rows.push({ fromTop: offsets[i], page: pages[p], vary: v === 1 })
            return rows
        }

        /*!
            Whatever the reader was doing when a page of history landed, they
            must come out of it on the same message, at the same height in the
            viewport.

            The offsets sweep the whole list, including the ones that fall in
            the gap between two rows and the ones either side of the point
            where Qt switches from pushing rows down to moving the origin.
        */
        function test_history_anchor_never_moves_the_reader(data) {
            var lv = makeView(100, data.vary)
            var anchor = anchorComp.createObject(app, { list: lv })

            lv.contentY = lv.originY + data.fromTop
            lv.forceLayout()
            var before = anchor.capture()

            prepend(lv, data.page, data.vary)
            anchor.restore(before, data.page)
            lv.forceLayout()

            var after = anchor.capture()
            compare(after.row, before.row + data.page,
                "reader left the message they were on")
            fuzzyCompare(after.offset, before.offset, 0.5,
                "message shifted within the viewport")
            anchor.destroy(); lv.destroy()
        }

        function test_history_anchor_survives_an_empty_list() {
            var lv = viewComp.createObject(app)
            var anchor = anchorComp.createObject(app, { list: lv })
            lv.forceLayout()
            var captured = anchor.capture()
            compare(captured.row, -1, "an empty list has nothing to anchor on")
            anchor.restore(captured, 0)   // must not throw
            anchor.destroy(); lv.destroy()
        }

        function test_history_anchor_without_a_list_is_inert() {
            var anchor = anchorComp.createObject(app)
            compare(anchor.capture().row, -1, "no list means no anchor")
            anchor.restore({ row: 5, offset: 0 }, 10)   // must not throw
            anchor.destroy()
        }
    }
}
