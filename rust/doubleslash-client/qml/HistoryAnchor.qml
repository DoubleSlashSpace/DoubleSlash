import QtQuick

/*!
    Keeps a reader's place in a chat \l ListView across an insert at the front.

    Paging older history in means every row the reader was looking at is
    renumbered, and the view has to be put back on the message they were
    actually reading.

    It does that by index rather than by pixels, because how Qt absorbs an
    insert at row 0 depends on whether row 0 had been realized. With the first
    row on screen Qt holds \c originY still and pushes the rows below it down,
    so the reader stays on the same pixel and therefore lands on older content.
    With the first row scrolled off Qt moves \c originY instead, leaving the
    reader exactly where they were and needing no correction at all. A pixel
    correction is right for one of those and a whole page of history out for
    the other, and \c contentHeight is only an estimate anyway while
    variable-height delegates are still being built. The row is the same
    message under either.

    Usage is capture-then-restore around the model change:

    \qml
    var anchor = historyAnchor.capture()
    chatModel.prependMessages(json)
    historyAnchor.restore(anchor, rows.length)
    \endqml
*/
QtObject {
    id: control

    /*! The chat list whose reading position is preserved. */
    property ListView list: null

    /*!
        Where the reader is now: the row at the top of the viewport and how far
        above the fold it starts.

        \c row is -1 when there was nothing to anchor on - an empty list, or no
        list at all - and \l restore then leaves the view alone.

        \c indexAt answers -1 in the \c spacing gap between two rows, so this
        probes a few points down the viewport rather than trusting one.
        Successive probes sit further apart than \c spacing, so no two can land
        in the same gap.
    */
    function capture() {
        if (control.list === null)
            return { row: -1, offset: 0 }
        var step = Math.max(4, control.list.spacing + 1)
        for (var n = 0; n < 8; ++n) {
            var row = control.list.indexAt(4, control.list.contentY + 1 + n * step)
            if (row >= 0) {
                var item = control.list.itemAtIndex(row)
                return { row: row, offset: item ? item.y - control.list.contentY : 0 }
            }
        }
        return { row: -1, offset: 0 }
    }

    /*!
        Puts the reader back on \a anchor after \a inserted rows were added at
        the front of the model.

        \c forceLayout first because the insert has to be laid out before the
        view can be positioned on a row within it.
    */
    function restore(anchor, inserted) {
        if (control.list === null || !anchor || anchor.row < 0)
            return
        control.list.forceLayout()
        control.list.positionViewAtIndex(anchor.row + inserted, ListView.Beginning)
        control.list.contentY -= anchor.offset
    }
}
