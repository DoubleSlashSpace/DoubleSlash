package com.doubleslash.client.ui

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put

/**
 * The DoubleSlash palette, shared with the desktop's `Theme.qml`.
 *
 * Both clients name the same fifteen colours, start from the same dark and
 * light palettes and ship the same built-in skins, so a skin is portable: the
 * JSON "Copy skin" produces on one pastes into the other.
 *
 * ```
 * {"v":1,"name":"Midnight","base":"dark","colors":{"bg0":"#000000",…}}
 * ```
 *
 * `colors` may name any of [TOKENS]; the rest come from the base palette.
 * Colours are `#RRGGBB` or `#AARRGGBB`, alpha first as both Qt and Android
 * read it.
 */
@Immutable
data class DsColors(
    val accent: Color,
    val bg0: Color,
    val bg1: Color,
    val bg2: Color,
    val bg3: Color,
    val text: Color,
    val muted: Color,
    val textInv: Color,
    val border: Color,
    val divider: Color,
    val online: Color,
    val danger: Color,
    val warn: Color,
    val linkMine: Color,
    val linkPeer: Color,
) {
    operator fun get(token: String): Color = when (token) {
        "accent" -> accent
        "bg0" -> bg0
        "bg1" -> bg1
        "bg2" -> bg2
        "bg3" -> bg3
        "text" -> text
        "muted" -> muted
        "textInv" -> textInv
        "border" -> border
        "divider" -> divider
        "online" -> online
        "danger" -> danger
        "warn" -> warn
        "linkMine" -> linkMine
        "linkPeer" -> linkPeer
        else -> accent
    }

    /** This palette with [overrides] (token → colour) laid over it. */
    fun with(overrides: Map<String, Color>): DsColors = copy(
        accent = overrides["accent"] ?: accent,
        bg0 = overrides["bg0"] ?: bg0,
        bg1 = overrides["bg1"] ?: bg1,
        bg2 = overrides["bg2"] ?: bg2,
        bg3 = overrides["bg3"] ?: bg3,
        text = overrides["text"] ?: text,
        muted = overrides["muted"] ?: muted,
        textInv = overrides["textInv"] ?: textInv,
        border = overrides["border"] ?: border,
        divider = overrides["divider"] ?: divider,
        online = overrides["online"] ?: online,
        danger = overrides["danger"] ?: danger,
        warn = overrides["warn"] ?: warn,
        linkMine = overrides["linkMine"] ?: linkMine,
        linkPeer = overrides["linkPeer"] ?: linkPeer,
    )

    /** The accent laid over a panel: a tinted surface for selected rows. */
    val selectedFill: Color get() = accent.copy(alpha = 0.15f).compositeOver(bg1)
}

/** Every token a skin can set, in the order a colour editor lists them. */
val TOKENS = listOf(
    "accent", "bg0", "bg1", "bg2", "bg3", "text", "muted", "textInv",
    "border", "divider", "online", "danger", "warn", "linkMine", "linkPeer",
)

/** What each token is for, as the colour editor labels it. */
val TOKEN_LABELS = mapOf(
    "accent" to "Accent", "bg0" to "Title bar", "bg1" to "Background",
    "bg2" to "Panels", "bg3" to "Raised", "text" to "Text", "muted" to "Muted text",
    "textInv" to "Text on accent", "border" to "Borders", "divider" to "Dividers",
    "online" to "Online", "danger" to "Danger", "warn" to "Warning",
    "linkMine" to "Links (yours)", "linkPeer" to "Links (theirs)",
)

val DarkPalette = DsColors(
    accent = Color(0xFF5865F2), bg0 = Color(0xFF111214), bg1 = Color(0xFF1E1F22),
    bg2 = Color(0xFF2B2D31), bg3 = Color(0xFF383A40), text = Color(0xFFDCDDDE),
    muted = Color(0xFFA6A9B0), textInv = Color(0xFFFFFFFF), border = Color(0xFF1E1F22),
    divider = Color(0xFF383A40), online = Color(0xFF3BA55D), danger = Color(0xFFFF2B40),
    warn = Color(0xFFFAA61A), linkMine = Color(0xFFDCE0FF), linkPeer = Color(0xFF8EA7FF),
)

val LightPalette = DsColors(
    accent = Color(0xFF5865F2), bg0 = Color(0xFFF2F3F5), bg1 = Color(0xFFFFFFFF),
    bg2 = Color(0xFFE9EAEC), bg3 = Color(0xFFD8D9DD), text = Color(0xFF2E3338),
    muted = Color(0xFF5B5F68), textInv = Color(0xFFFFFFFF), border = Color(0xFFE3E5E8),
    divider = Color(0xFFC8C9CC), online = Color(0xFF3BA55D), danger = Color(0xFFFF2B40),
    warn = Color(0xFFFAA61A), linkMine = Color(0xFF3C45A5), linkPeer = Color(0xFF5865F2),
)

/** The active palette, for the colours Material's scheme has no slot for. */
val LocalDsColors = staticCompositionLocalOf { DarkPalette }

/** One skin: a name, the theme it was made for, and its colour overrides. */
data class Skin(val name: String, val base: String, val colors: Map<String, Color>)

/** A built-in skin. Kept in step with `Theme.presets` on the desktop. */
data class SkinPreset(val id: String, val name: String, val base: String, val colors: Map<String, String>) {
    fun toSkin() = Skin(name, base, colors.mapValues { parseSkinColor(it.value)!! })
}

val SKIN_PRESETS = listOf(
    SkinPreset("default", "DoubleSlash", "", emptyMap()),
    SkinPreset(
        "midnight", "Midnight", "dark",
        mapOf(
            "bg0" to "#000000", "bg1" to "#08090B", "bg2" to "#121418", "bg3" to "#1E2127",
            "border" to "#08090B", "divider" to "#1E2127",
        ),
    ),
    SkinPreset(
        "slate", "Slate", "dark",
        mapOf(
            "bg0" to "#0F141A", "bg1" to "#151C24", "bg2" to "#1D2630", "bg3" to "#2A3542",
            "border" to "#151C24", "divider" to "#2A3542", "accent" to "#3B82F6", "linkPeer" to "#7FB0FF",
        ),
    ),
    SkinPreset(
        "forest", "Forest", "dark",
        mapOf(
            "bg0" to "#0E1411", "bg1" to "#141D18", "bg2" to "#1C2821", "bg3" to "#28372E",
            "border" to "#141D18", "divider" to "#28372E", "accent" to "#2F9E6E", "linkPeer" to "#7FD1A8",
        ),
    ),
    SkinPreset(
        "contrast", "High contrast", "dark",
        mapOf(
            "bg0" to "#000000", "bg1" to "#000000", "bg2" to "#0D0D0D", "bg3" to "#262626",
            "text" to "#FFFFFF", "muted" to "#D6D6D6", "border" to "#8A8A8A", "divider" to "#8A8A8A",
            "accent" to "#8C9BFF", "linkPeer" to "#AFC0FF",
        ),
    ),
    SkinPreset(
        "paper", "Paper", "light",
        mapOf(
            "bg0" to "#E9E4DA", "bg1" to "#FBF8F2", "bg2" to "#F1ECE3", "bg3" to "#E0D9CC",
            "text" to "#2B2620", "muted" to "#625A4F", "border" to "#E0D9CC", "divider" to "#CFC6B6",
            "accent" to "#B4532A", "linkMine" to "#7A3417", "linkPeer" to "#B4532A",
        ),
    ),
)

private val HEX = Regex("^#([0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$")

/** A skin colour, or null when [value] is not `#RRGGBB` / `#AARRGGBB`. */
fun parseSkinColor(value: String?): Color? {
    if (value == null || !HEX.matches(value)) return null
    val hex = value.substring(1)
    val argb = if (hex.length == 6) "FF$hex" else hex
    return Color(argb.toLong(16).toInt())
}

/** `#RRGGBB`, or `#AARRGGBB` when not opaque — the form both clients write. */
fun Color.toSkinHex(): String {
    val argb = (alpha * 255).toInt() shl 24 or
        ((red * 255).toInt() shl 16) or
        ((green * 255).toInt() shl 8) or
        (blue * 255).toInt()
    return if (alpha >= 0.999f) {
        "#%06X".format(argb and 0xFFFFFF)
    } else {
        "#%08X".format(argb)
    }
}

private val skinJson = Json { ignoreUnknownKeys = true }

/**
 * A skin from its JSON, or null when it is not one.
 *
 * Unknown tokens and malformed colours are dropped rather than refusing the
 * skin, so one from a newer client still applies what this build understands.
 */
fun parseSkin(json: String?): Skin? {
    if (json.isNullOrBlank()) return null
    val obj = runCatching { skinJson.parseToJsonElement(json).jsonObject }.getOrNull() ?: return null
    val colors = obj["colors"] as? JsonObject ?: return null
    val parsed = buildMap {
        for (token in TOKENS) {
            val value = (colors[token] as? JsonPrimitive)?.contentOrNull
            parseSkinColor(value)?.let { put(token, it) }
        }
    }
    val base = (obj["base"] as? JsonPrimitive)?.contentOrNull?.takeIf { it == "dark" || it == "light" }.orEmpty()
    val name = (obj["name"] as? JsonPrimitive)?.contentOrNull.orEmpty()
    return Skin(name, base, parsed)
}

/** The portable JSON for a skin. */
fun skinToJson(skin: Skin): String = buildJsonObject {
    put("v", 1)
    put("name", skin.name)
    put("base", skin.base)
    put(
        "colors",
        buildJsonObject { skin.colors.forEach { (token, color) -> put(token, color.toSkinHex()) } },
    )
}.toString()

/** The palette for [dark] with [skinJson]'s colours laid over it. */
fun paletteFor(dark: Boolean, skinJson: String?): DsColors {
    val base = if (dark) DarkPalette else LightPalette
    val skin = parseSkin(skinJson) ?: return base
    return base.with(skin.colors)
}

/** Index into [SKIN_PRESETS] of [skinJson], or -1 for a custom skin. */
fun presetIndexOf(skinJson: String?): Int {
    val skin = parseSkin(skinJson)
    if (skin == null || skin.colors.isEmpty()) return 0
    return SKIN_PRESETS.indexOfFirst { preset ->
        preset.colors.isNotEmpty() && preset.toSkin().colors == skin.colors
    }
}
