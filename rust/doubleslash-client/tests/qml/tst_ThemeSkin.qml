import QtQuick
import QtTest
import DoubleSlash.Client 1.0

// Skins: the portable colour JSON both clients read. A bad skin must never
// leave the app unreadable, and a partial one fills the rest from the base.
TestCase {
    name: "ThemeSkin"

    function cleanup() {
        Theme.applySkinJson("")
        Theme.isDark = true
    }

    function test_garbage_is_not_a_skin() {
        compare(Theme.parseSkin(""), null)
        compare(Theme.parseSkin("not json"), null)
        compare(Theme.parseSkin("{\"name\":\"x\"}"), null)
        compare(Theme.parseSkin("[1,2]"), null)
    }

    function test_unknown_tokens_and_bad_colours_are_dropped() {
        var skin = Theme.parseSkin(JSON.stringify({
            v: 1, name: "Mixed", base: "dark",
            colors: { accent: "#12ab34", bg0: "red", wobble: "#FFFFFF", text: "#FF00FF00" }
        }))
        verify(skin !== null)
        compare(skin.name, "Mixed")
        compare(skin.base, "dark")
        compare(skin.colors.accent, "#12AB34")
        compare(skin.colors.text, "#FF00FF00")
        verify(skin.colors.bg0 === undefined)
        verify(skin.colors.wobble === undefined)
    }

    function test_applied_skin_overrides_only_what_it_names() {
        Theme.applySkinJson(Theme.skinJson("Accent only", "", { accent: "#FF8800" }))
        compare(Theme.toHex(Theme.accent), "#ff8800")
        compare(Theme.toHex(Theme.bg1), Theme.basePalettes.dark.bg1.toLowerCase())
        // Switching theme keeps the override and swaps the rest.
        Theme.isDark = false
        compare(Theme.toHex(Theme.accent), "#ff8800")
        compare(Theme.toHex(Theme.bg1), Theme.basePalettes.light.bg1.toLowerCase())
    }

    function test_empty_skin_restores_the_base() {
        Theme.applySkinJson(Theme.skinJson("x", "", { bg0: "#000000" }))
        Theme.applySkinJson("")
        compare(Theme.toHex(Theme.bg0), Theme.basePalettes.dark.bg0.toLowerCase())
    }

    function test_every_preset_round_trips() {
        for (var i = 0; i < Theme.presets.length; i++) {
            var p = Theme.presets[i]
            var skin = Theme.parseSkin(Theme.skinJson(p.name, p.base, p.colors))
            verify(skin !== null, p.id)
            compare(Object.keys(skin.colors).length, Object.keys(p.colors).length, p.id)
        }
    }
}
