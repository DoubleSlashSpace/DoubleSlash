// Theme.qml — DoubleSlash palette singleton: dark/light base plus skins.
//
// Usage: Theme.bg0, Theme.accent, etc.
// Toggle dark/light: Theme.isDark = false
// Skin: Theme.applySkinJson(json) — colour overrides on top of the base.
// All QML files in DoubleSlash.Client 1.0 can reference this without extra imports.
//
// A skin is portable JSON, the same format the Android client reads, so one
// can be copied from one device and pasted into the other:
//
//   {"v":1,"name":"Midnight","base":"dark","colors":{"bg0":"#000000",…}}
//
// `colors` may name any of `tokenNames`; anything it leaves out comes from the
// base palette. `base` says which theme the skin was made for: choosing a skin
// switches the theme to it, but an applied skin never forces the theme.
pragma Singleton
import QtQuick

QtObject {
    id: theme

    // ── Dark/light toggle ────────────────────────────────────────────────────
    property bool isDark: true
    onIsDarkChanged: _applyPalette()

    /// Colour overrides from the active skin, `{ token: "#rrggbb" }`.
    /// Reassigned whole (applySkinJson does it) so the palette re-applies.
    property var skinColors: ({})
    onSkinColorsChanged: _applyPalette()

    /// Every colour a skin can set, in the order a colour editor lists them.
    readonly property var tokenNames: [
        "accent", "bg0", "bg1", "bg2", "bg3", "text", "muted", "textInv",
        "border", "divider", "online", "danger", "warn", "linkMine", "linkPeer"
    ]

    /// What each token is for, as a colour editor labels it.
    readonly property var tokenLabels: ({
        accent: qsTr("Accent"), bg0: qsTr("Title bar"), bg1: qsTr("Background"),
        bg2: qsTr("Panels"), bg3: qsTr("Raised"), text: qsTr("Text"),
        muted: qsTr("Muted text"), textInv: qsTr("Text on accent"),
        border: qsTr("Borders"), divider: qsTr("Dividers"), online: qsTr("Online"),
        danger: qsTr("Danger"), warn: qsTr("Warning"),
        linkMine: qsTr("Links (yours)"), linkPeer: qsTr("Links (theirs)")
    })

    readonly property var basePalettes: ({
        dark: {
            bg0: "#111214", bg1: "#1E1F22", bg2: "#2B2D31", bg3: "#383A40",
            text: "#DCDDDE", muted: "#A6A9B0", textInv: "#FFFFFF",
            border: "#1E1F22", divider: "#383A40",
            accent: "#5865F2", online: "#3BA55D", danger: "#FF2B40", warn: "#FAA61A",
            linkMine: "#DCE0FF", linkPeer: "#8EA7FF"
        },
        light: {
            bg0: "#F2F3F5", bg1: "#FFFFFF", bg2: "#E9EAEC", bg3: "#D8D9DD",
            text: "#2E3338", muted: "#5B5F68", textInv: "#FFFFFF",
            border: "#E3E5E8", divider: "#C8C9CC",
            accent: "#5865F2", online: "#3BA55D", danger: "#FF2B40", warn: "#FAA61A",
            linkMine: "#3C45A5", linkPeer: "#5865F2"
        }
    })

    /// Built-in skins. "default" is the base palette with nothing changed.
    /// Kept in step with the Android client's list.
    readonly property var presets: [
        { id: "default", name: qsTr("DoubleSlash"), base: "", colors: {} },
        { id: "midnight", name: qsTr("Midnight"), base: "dark", colors: {
            bg0: "#000000", bg1: "#08090B", bg2: "#121418", bg3: "#1E2127",
            border: "#08090B", divider: "#1E2127" } },
        { id: "slate", name: qsTr("Slate"), base: "dark", colors: {
            bg0: "#0F141A", bg1: "#151C24", bg2: "#1D2630", bg3: "#2A3542",
            border: "#151C24", divider: "#2A3542", accent: "#3B82F6", linkPeer: "#7FB0FF" } },
        { id: "forest", name: qsTr("Forest"), base: "dark", colors: {
            bg0: "#0E1411", bg1: "#141D18", bg2: "#1C2821", bg3: "#28372E",
            border: "#141D18", divider: "#28372E", accent: "#2F9E6E", linkPeer: "#7FD1A8" } },
        { id: "contrast", name: qsTr("High contrast"), base: "dark", colors: {
            bg0: "#000000", bg1: "#000000", bg2: "#0D0D0D", bg3: "#262626",
            text: "#FFFFFF", muted: "#D6D6D6", border: "#8A8A8A", divider: "#8A8A8A",
            accent: "#8C9BFF", linkPeer: "#AFC0FF" } },
        { id: "paper", name: qsTr("Paper"), base: "light", colors: {
            bg0: "#E9E4DA", bg1: "#FBF8F2", bg2: "#F1ECE3", bg3: "#E0D9CC",
            text: "#2B2620", muted: "#625A4F", border: "#E0D9CC", divider: "#CFC6B6",
            accent: "#B4532A", linkMine: "#7A3417", linkPeer: "#B4532A" } }
    ]

    /// `#RRGGBB` or `#AARRGGBB` only (alpha first, as Qt and Android both read
    /// it): what every skin writes and both clients parse.
    function isSkinColor(value) {
        return typeof value === "string" && /^#([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$/.test(value)
    }

    /// A skin from its JSON, or null when it is not one. Unknown tokens and
    /// malformed colours are dropped rather than rejecting the whole skin, so a
    /// skin from a newer client still applies what this one understands.
    function parseSkin(json) {
        if (!json || typeof json !== "string" || json.trim() === "")
            return null
        var obj
        try { obj = JSON.parse(json) } catch (e) { return null }
        if (!obj || typeof obj !== "object" || !obj.colors || typeof obj.colors !== "object")
            return null
        var colors = {}
        for (var i = 0; i < theme.tokenNames.length; i++) {
            var t = theme.tokenNames[i]
            if (theme.isSkinColor(obj.colors[t]))
                colors[t] = obj.colors[t].toUpperCase()
        }
        return {
            name: typeof obj.name === "string" ? obj.name : "",
            base: obj.base === "dark" || obj.base === "light" ? obj.base : "",
            colors: colors
        }
    }

    /// The portable JSON for a skin.
    function skinJson(name, base, colors) {
        return JSON.stringify({ v: 1, name: name || "", base: base || "", colors: colors || {} })
    }

    /// Apply a skin's colours; an empty or unreadable skin restores the base.
    function applySkinJson(json) {
        var skin = theme.parseSkin(json)
        theme.skinColors = skin ? skin.colors : ({})
    }

    /// The colour a token has in the base palette for the current theme.
    function baseColor(token) {
        return theme.basePalettes[theme.isDark ? "dark" : "light"][token]
    }

    function _applyPalette() {
        var base = theme.basePalettes[theme.isDark ? "dark" : "light"]
        for (var i = 0; i < theme.tokenNames.length; i++) {
            var t = theme.tokenNames[i]
            var v = theme.skinColors[t]
            theme[t] = theme.isSkinColor(v) ? v : base[t]
        }
    }

    // ── Background layers (dark values as initial literals) ──────────────────
    property color bg0:  "#111214"
    property color bg1:  "#1E1F22"
    property color bg2:  "#2B2D31"
    property color bg3:  "#383A40"

    // ── Text ─────────────────────────────────────────────────────────────────
    property color text:    "#DCDDDE"
    property color muted:   "#A6A9B0"
    property color textInv: "#FFFFFF"

    // ── Semantic colours (skinnable, same in both base palettes) ──────────────
    property color accent:  "#5865F2"
    property color online:  "#3BA55D"
    property color danger:  "#FF2B40"
    property color warn:    "#FAA61A"

    // ── Borders / dividers ────────────────────────────────────────────────────
    property color border:  "#1E1F22"
    property color divider: "#383A40"

    // ── Typography ────────────────────────────────────────────────────────────
    // Multiplier for the sizes below. 1 is the designed size. Settings ›
    // General owns this (font_scale_percent, −50%…+200% → 0.5…3). A skin
    // cannot set it: it is not a colour token and applySkinJson ignores it.
    property real fontScale: 1.0
    onFontScaleChanged: {
        var clamped = Math.max(0.5, Math.min(3.0, fontScale))
        if (fontScale !== clamped)
            fontScale = clamped
    }

    readonly property int fontSizeBody:    Math.max(1, Math.round(13 * fontScale))
    readonly property int fontSizeCaption: Math.max(1, Math.round(11 * fontScale))
    readonly property int fontSizeTitle:   Math.max(1, Math.round(15 * fontScale))

    // Chat stamps. Settings › General owns `timeFormat`. The ids match the
    // phone. A skin cannot set this. The default is 12-hour with AM/PM;
    // military is the previous 24-hour stamp.
    property string timeFormat: "ampm"
    readonly property var timeFormats: [
        { "id": "ampm", "label": qsTr("Local, AM/PM"), "date": "MMM d, yyyy", "dateTime": "MMM d, yyyy h:mm AP" },
        { "id": "military", "label": qsTr("Military, 24-hour"), "date": "MMM d, yyyy", "dateTime": "MMM d, yyyy HH:mm" },
        { "id": "us", "label": qsTr("United States"), "date": "M/d/yyyy", "dateTime": "M/d/yyyy h:mm AP" },
        { "id": "uk", "label": qsTr("United Kingdom"), "date": "dd/MM/yyyy", "dateTime": "dd/MM/yyyy HH:mm" },
        { "id": "eu", "label": qsTr("Europe"), "date": "dd.MM.yyyy", "dateTime": "dd.MM.yyyy HH:mm" },
        { "id": "iso", "label": qsTr("ISO 8601"), "date": "yyyy-MM-dd", "dateTime": "yyyy-MM-dd HH:mm" },
        { "id": "east_asia", "label": qsTr("East Asia"), "date": "yyyy/MM/dd", "dateTime": "yyyy/MM/dd HH:mm" }
    ]

    function normalizeTimeFormat(id) {
        for (var i = 0; i < timeFormats.length; i++) {
            if (timeFormats[i].id === id)
                return id
        }
        return "ampm"
    }

    function timeFormatById(id) {
        var want = normalizeTimeFormat(id)
        for (var i = 0; i < timeFormats.length; i++) {
            if (timeFormats[i].id === want)
                return timeFormats[i]
        }
        return timeFormats[0]
    }

    function formatChatDate(date) {
        return Qt.formatDate(date, timeFormatById(timeFormat).date)
    }

    function formatChatDateTime(date) {
        return Qt.formatDateTime(date, timeFormatById(timeFormat).dateTime)
    }

    // ── Geometry ──────────────────────────────────────────────────────────────
    // Angular throughout: no rounded corners, circles or pills. Icons are SVG
    // drawn with square ends and mitred corners. Material's round controls
    // have square stand-ins (IconButton, SquareSwitch, SquareSlider,
    // SquareBusyIndicator); the Peers | Rooms toggle is a hexagon.
    readonly property int radiusSm: 0
    readonly property int radiusMd: 0
    readonly property int radiusLg: 0

    readonly property int spacingXs: 4
    readonly property int spacingSm: 8
    readonly property int spacingMd: 12
    readonly property int spacingLg: 16
    readonly property int spacingXl: 24

    // Grow with the text so a larger font size still fits. At the designed
    // size (fontScale 1) these stay 32 and 44.
    readonly property int controlHeight: Math.max(32, fontSizeBody + spacingSm * 2)
    readonly property int touchTarget: Math.max(44, fontSizeTitle + spacingMd * 2)
    // Wide enough for the Rooms tree: rooms, their Voice / Text-only leaves
    // and members nest three or four levels deep.
    readonly property int sidebarWidth: 280
    readonly property int titleBarHeight: Math.max(44, touchTarget)
    readonly property int bannerHeight: Math.max(32, fontSizeCaption + spacingSm * 2)

    // ── Bottom bars ───────────────────────────────────────────────────────────
    // The message box and the voice dock sit side by side at the foot of the
    // window. Both are built from these rows, so their tops, rows and buttons
    // line up across it: the call status beside the formatting row, the call
    // buttons beside the input row.
    /// Around the message box, between it and the panel's edges.
    readonly property int barMargin: spacingSm
    /// Inside either bar, around its rows.
    readonly property int barPadding: spacingSm
    readonly property int barRowGap: spacingXs
    /// Formatting buttons; call status.
    readonly property int barTopRowHeight: controlHeight
    /// Attach and send; mute, share and leave.
    readonly property int barActionSize: controlHeight + spacingXs
    /// Either bar's two rows with their padding, before the text box grows.
    readonly property int barHeight: barPadding * 2 + barTopRowHeight + barRowGap + barActionSize

    // ── Typography extras ─────────────────────────────────────────────────────
    readonly property int fontSizeDialog: Math.max(1, Math.round(18 * fontScale))
    readonly property int fontSizeMicro: Math.max(1, Math.round(9 * fontScale))

    // Chat link colours (updated per palette in _applyPalette)
    property color linkMine: "#DCE0FF"
    property color linkPeer: "#8EA7FF"

    // ── Motion (DESIGN.md: ≤ 300 ms) ──────────────────────────────────────────
    readonly property int animMicro: 80
    readonly property int animFast: 160
    readonly property int animNormal: 250
    readonly property int animSlow: 300

    // ── Overlays ──────────────────────────────────────────────────────────────
    readonly property color overlayScrim: "#000000"

    // ── Colour helpers ────────────────────────────────────────────────────────
    function withAlpha(c, a) {
        return Qt.rgba(c.r, c.g, c.b, a)
    }

    function selectedFill() {
        return withAlpha(accent, 0.15)
    }

    function semanticTint(c, strength) {
        return withAlpha(c, strength !== undefined ? strength : 0.12)
    }

    function connectionModeColor(mode) {
        switch (mode) {
            case "direct": return online
            case "relay":  return warn
            case "error":  return danger
            default:       return muted
        }
    }

    function connectionModeLabel(mode) {
        switch (mode) {
            case "direct": return "Direct"
            case "relay":  return "Relay"
            case "error":  return "Error"
            default:       return "Offline"
        }
    }

    function connectionModeTint(mode) {
        return semanticTint(connectionModeColor(mode), 0.06)
    }

    function toHex(c) {
        function channel(v) {
            var n = Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16)
            return n.length === 1 ? "0" + n : n
        }
        return "#" + channel(c.r) + channel(c.g) + channel(c.b)
    }

    function qualityTierColor(tier) {
        switch (tier) {
            case "Excellent": return online
            case "Good":      return online
            case "Fair":      return warn
            default:          return danger
        }
    }

    function rttSparklineColor(maxMs) {
        if (maxMs > 300) return danger
        if (maxMs > 150) return warn
        return online
    }
}
