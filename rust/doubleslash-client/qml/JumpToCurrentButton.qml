import QtQuick
import DoubleSlash.Client 1.0

/*!
    A "jump to current" affordance for a chat \l ListView.

    Appears once the view has been scrolled far enough from the newest message
    that an arriving message would land unseen below the fold, and returns to
    the latest message when clicked.

    Purely an affordance: the panel decides for itself whether to keep
    auto-scrolling. This only reports that the reader has scrolled away and
    offers a way back, so reading history is never interrupted by a message
    yanking the view down.

    The caller anchors it — normally bottom-centre over the list — because only
    the caller knows what else is competing for that corner.
*/
StyledButton {
    id: control

    /*! The chat list this returns to the bottom of. */
    property ListView list: null

    /*!
        How far from the newest message counts as "away", in pixels.

        Roughly three message rows. Small enough that a message never arrives
        unseen behind the fold, large enough that nudging the wheel one notch
        does not flash a button at you.
    */
    property int threshold: 96

    /*!
        How far the newest message sits below the bottom of the viewport.

        Measured from \c originY, not from zero. A chat list grows at both
        ends - older history is inserted at row 0 - and Qt keeps the visible
        rows still across such an insert by moving the content's origin rather
        than every row, so the scrollable range is \c originY to
        \c {originY + contentHeight - height}. Treating it as starting at zero
        reads as "away" or "at the end" by whatever the origin has drifted to,
        which is how this button used to hide at the top of a long history and
        linger at the bottom of one.
    */
    readonly property real distanceFromLatest: control.list === null
        ? 0
        : Math.max(0, control.list.originY + control.list.contentHeight
            - control.list.height - control.list.contentY)

    /*! True while the list is scrolled past \l threshold from the newest message. */
    readonly property bool awayFromLatest: control.list !== null
        && control.list.contentHeight > control.list.height
        // Both ends come from the view's own reckoning rather than from the
        // arithmetic above, because with variable-height delegates
        // `contentHeight` is an estimate until every row has been built: at
        // the very end there is nothing to jump to whatever it estimates, and
        // at the very start of a list taller than its viewport there always
        // is.
        && !control.list.atYEnd
        && (control.list.atYBeginning || control.distanceFromLatest > control.threshold)

    text: qsTr("Jump to current")
    primary: true
    icon.source: "qrc:/qt/qml/DoubleSlash/Client/icons/arrow-down.svg"

    // Driven by opacity so the button fades rather than blinking, and folded
    // back into `visible` so a fully faded button cannot be clicked or take
    // focus.
    opacity: control.awayFromLatest ? 1 : 0
    visible: opacity > 0
    Behavior on opacity {
        NumberAnimation {
            duration: 120
            easing.type: Easing.OutQuad
        }
    }

    onClicked: {
        if (control.list)
            control.list.positionViewAtEnd()
    }
}
