// Glyph.qml — a packaged icon, tinted.
//
// The SVG files are drawn in #C8C8C8, so a raw Image reads on the dark skins
// and disappears on the light ones. This tints the shape, from its alpha, to
// `color`, the same way a button icon is tinted. A pack icon is a solid shape
// on a transparent background and takes the same colour. The logo stays an Image.
import QtQuick
import QtQuick.Controls.impl
import DoubleSlash.Client 1.0

IconImage {
    color: Theme.text
    fillMode: Image.PreserveAspectFit
}
