// Icons.qml — packaged SVG icons, with an optional folder of replacements.
//
// Call sites keep the packaged URL:
//
//   source: Icons.resolve("qrc:/qt/qml/DoubleSlash/Client/icons/phone.svg")
//
// `resolve` reads `generation`, so a binding that calls it updates when a
// pack is applied or cleared. A name the pack does not supply stays the
// packaged icon. A pack file may be that SVG, or a PNG with the same stem.
// `scripts/export_icon_pack.mjs` writes a starter folder.
pragma Singleton
import QtQuick

QtObject {
    readonly property string prefix: "qrc:/qt/qml/DoubleSlash/Client/icons/"

    /// Built-in file name → `file:` URL. Replaced as a whole when a pack loads.
    property var overrides: ({})
    property string packName: ""
    property int replaced: 0
    property int builtinCount: 0
    /// Why the current folder is not in use. Empty when the built-in set or a
    /// pack is showing.
    property string problem: ""
    /// A pack loaded, but at least one of its files was left out.
    property string note: ""
    property int generation: 0

    /// The URL to draw for a packaged icon URL. Anything else is unchanged,
    /// including a file URL a test sets by hand.
    function resolve(source) {
        var text = source === undefined || source === null ? "" : ("" + source)
        // `generation` is what makes an Image binding reload after a pack loads.
        if (generation < 0)
            return text
        var at = text.indexOf(prefix)
        if (at < 0)
            return source
        var name = text.substring(at + prefix.length)
        var cut = name.indexOf("?")
        if (cut >= 0)
            name = name.substring(0, cut)
        cut = name.indexOf("#")
        if (cut >= 0)
            name = name.substring(0, cut)
        var hit = overrides[name]
        if (hit !== undefined && hit !== "")
            return hit
        return source
    }

    /// Apply the JSON from `SettingsModel.loadIconPack`.
    function applyDescribed(jsonText) {
        var data = null
        try {
            data = JSON.parse(jsonText)
        } catch (e) {
            data = null
        }
        if (!data || data.ok !== true) {
            overrides = ({})
            packName = ""
            replaced = 0
            builtinCount = data && data.builtin ? data.builtin : builtinCount
            problem = data && data.error ? data.error : qsTr("Could not read that icon pack.")
            note = ""
            generation = generation + 1
            return false
        }
        overrides = data.icons || ({})
        packName = data.name || ""
        replaced = data.count || 0
        builtinCount = data.builtin || 0
        problem = ""
        note = data.note || ""
        generation = generation + 1
        return true
    }
}
