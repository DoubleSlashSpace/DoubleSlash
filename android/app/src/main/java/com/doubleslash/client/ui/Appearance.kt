package com.doubleslash.client.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.doubleslash.client.AppSettings

/**
 * Theme, skin and colours — the desktop's Settings › General › Appearance.
 *
 * The skin is kept as the portable JSON both clients read, so what this
 * writes is exactly what "Copy skin" hands to the desktop, and a skin copied
 * there pastes here.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AppearanceSection(
    theme: String,
    skinJson: String,
    onSetTheme: (String) -> Unit,
    onSetSkin: (String) -> Unit,
) {
    val ds = LocalDsColors.current
    val clipboard = LocalClipboardManager.current
    var editing by remember { mutableStateOf<String?>(null) }
    var paste by remember { mutableStateOf("") }
    var notice by remember { mutableStateOf("") }
    val skin = parseSkin(skinJson)
    val presetIndex = presetIndexOf(skinJson)
    val dark = when (theme) {
        AppSettings.THEME_DARK -> true
        AppSettings.THEME_LIGHT -> false
        else -> isSystemInDarkTheme()
    }

    Text("Theme", style = MaterialTheme.typography.labelLarge)
    listOf(
        AppSettings.THEME_SYSTEM to "Follow the system",
        AppSettings.THEME_DARK to "Dark",
        AppSettings.THEME_LIGHT to "Light",
    ).forEach { (value, label) ->
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { onSetTheme(value) }
                .padding(vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RadioButton(selected = theme == value, onClick = { onSetTheme(value) })
            Text(label, Modifier.padding(start = 8.dp))
        }
    }

    Spacer(Modifier.height(12.dp))
    Text("Skin", style = MaterialTheme.typography.labelLarge)
    Spacer(Modifier.height(6.dp))
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.horizontalScroll(rememberScrollState()),
    ) {
        SKIN_PRESETS.forEachIndexed { i, preset ->
            FilterChip(
                selected = presetIndex == i,
                onClick = {
                    onSetSkin(if (preset.id == "default") "" else skinToJson(preset.toSkin()))
                    // A skin made for one theme reads badly on the other.
                    if (preset.base.isNotEmpty()) onSetTheme(preset.base)
                },
                label = { Text(preset.name) },
            )
        }
        if (presetIndex < 0) {
            FilterChip(selected = true, onClick = {}, label = { Text(skin?.name?.ifBlank { null } ?: "Custom") })
        }
    }

    Spacer(Modifier.height(12.dp))
    Text("Colours", style = MaterialTheme.typography.labelLarge)
    Spacer(Modifier.height(6.dp))
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TOKENS.forEach { token ->
            val overridden = skin?.colors?.containsKey(token) == true
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .width(160.dp)
                    .height(48.dp)
                    .clickable { editing = token }
                    .semantics { contentDescription = "Change ${TOKEN_LABELS[token]} colour" },
            ) {
                Box(
                    Modifier
                        .size(28.dp)
                        .background(ds[token], CircleShape)
                        .border(1.dp, ds.divider, CircleShape),
                )
                Spacer(Modifier.width(8.dp))
                Column {
                    Text(
                        TOKEN_LABELS[token] ?: token,
                        fontSize = 13.sp,
                        fontWeight = if (overridden) FontWeight.Bold else FontWeight.Normal,
                    )
                    Text(ds[token].toSkinHex(), fontSize = 11.sp, color = ds.muted, fontFamily = FontFamily.Monospace)
                }
            }
        }
    }

    Spacer(Modifier.height(8.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        SquareTextButton(onClick = { onSetSkin("") }, enabled = skinJson.isNotEmpty()) { Text("Reset colours") }
        SquareTextButton(onClick = {
            val out = skin ?: Skin("DoubleSlash", if (dark) "dark" else "light", emptyMap())
            clipboard.setText(AnnotatedString(skinToJson(out)))
            notice = "Skin copied. Paste it into another DoubleSlash to use it there."
        }) { Text("Copy skin") }
    }
    OutlinedTextField(
        value = paste,
        onValueChange = { paste = it },
        placeholder = { Text("Paste a skin…") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
        trailingIcon = {
            SquareTextButton(
                enabled = paste.isNotBlank(),
                onClick = {
                    val pasted = parseSkin(paste.trim())
                    if (pasted == null) {
                        notice = "That is not a DoubleSlash skin."
                    } else {
                        onSetSkin(skinToJson(pasted))
                        if (pasted.base.isNotEmpty()) onSetTheme(pasted.base)
                        paste = ""
                        notice = "Skin applied."
                    }
                },
            ) { Text("Apply") }
        },
    )
    if (notice.isNotEmpty()) {
        Text(notice, fontSize = 12.sp, color = ds.muted, modifier = Modifier.padding(top = 4.dp))
    }

    editing?.let { token ->
        SkinColourDialog(
            token = token,
            current = ds[token],
            onDismiss = { editing = null },
            onPick = { color ->
                editing = null
                val colors = (skin?.colors ?: emptyMap()) + (token to color)
                val base = skin?.base?.ifEmpty { null } ?: if (dark) "dark" else "light"
                onSetSkin(skinToJson(Skin("Custom", base, colors)))
            },
        )
    }
}

/** A few starting points; any hex can be typed instead. */
private val SUGGESTED = listOf(
    "#5865F2", "#3B82F6", "#2F9E6E", "#3BA55D", "#B4532A", "#E07A5F", "#FAA61A", "#FF2B40",
    "#A78BFA", "#E07A9A", "#5FB3B3", "#8C9BFF", "#000000", "#111214", "#1E1F22", "#2B2D31",
    "#383A40", "#A6A9B0", "#DCDDDE", "#FFFFFF", "#F2F3F5", "#FBF8F2", "#2B2620", "#0F141A",
)

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SkinColourDialog(
    token: String,
    current: Color,
    onDismiss: () -> Unit,
    onPick: (Color) -> Unit,
) {
    val ds = LocalDsColors.current
    var hex by remember { mutableStateOf(current.toSkinHex()) }
    val parsed = parseSkinColor(hex.trim().let { if (it.startsWith("#")) it else "#$it" }.uppercase())
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(TOKEN_LABELS[token] ?: token) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(40.dp)
                            .background(parsed ?: current, CircleShape)
                            .border(1.dp, ds.divider, CircleShape),
                    )
                    Spacer(Modifier.width(12.dp))
                    OutlinedTextField(
                        value = hex,
                        onValueChange = { hex = it },
                        singleLine = true,
                        isError = parsed == null,
                        label = { Text("Hex") },
                        modifier = Modifier.weight(1f),
                    )
                }
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    SUGGESTED.forEach { suggestion ->
                        val c = parseSkinColor(suggestion) ?: return@forEach
                        Box(
                            Modifier
                                .size(32.dp)
                                .background(c, CircleShape)
                                .border(if (suggestion == hex.uppercase()) 3.dp else 1.dp, ds.divider, CircleShape)
                                .clickable { hex = suggestion }
                                .semantics { contentDescription = suggestion },
                        )
                    }
                }
            }
        },
        confirmButton = {
            SquareTextButton(enabled = parsed != null, onClick = { parsed?.let(onPick) }) { Text("Use colour") }
        },
        dismissButton = { SquareTextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
