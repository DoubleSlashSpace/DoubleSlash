// StyledButtonBox.qml — A dialog footer of StyledButtons.
//
// Material's DialogButtonBox draws pill buttons (its Button forces
// roundedScale to FullScale) on a background with 28 px corners; the desktop
// is angular throughout. Use this as `footer:` of any Dialog with
// standardButtons. The accept button is the filled one.

import QtQuick
import QtQuick.Controls.Material
import DoubleSlash.Client 1.0

DialogButtonBox {
    id: box

    /// Whether the accept-role button can be pressed, e.g. once a required
    /// field is filled. Other buttons are always enabled.
    property bool acceptEnabled: true

    Material.roundedScale: Material.NotRounded
    spacing: Theme.spacingSm
    padding: Theme.spacingMd

    background: Rectangle {
        color: Theme.bg0
    }

    delegate: StyledButton {
        readonly property bool accepts: DialogButtonBox.buttonRole === DialogButtonBox.AcceptRole
            || DialogButtonBox.buttonRole === DialogButtonBox.YesRole
        primary: accepts
        enabled: !accepts || box.acceptEnabled
    }
}
