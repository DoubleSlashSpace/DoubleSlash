package com.doubleslash.client.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/**
 * An icon control on a square tile, the app's stand-in for Material's
 * IconButton (whose background and ripple are circles). The UI is angular
 * throughout, and every control sits on a tile that contrasts with what is
 * behind it, as on the desktop.
 *
 * The tile is a tint of the text colour unless [tile] says otherwise, so it
 * reads on every surface and in every skin. [border] outlines it, for a
 * control with a colour of its own, such as the green call icon.
 */
@Composable
internal fun SquareIconButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tile: Color? = null,
    border: Color? = null,
    content: @Composable () -> Unit,
) {
    val ds = LocalDsColors.current
    val fill = tile ?: ds.text.copy(alpha = if (enabled) 0.10f else 0.05f)
    Box(
        contentAlignment = Alignment.Center,
        modifier = modifier
            .sizeIn(minWidth = 40.dp, minHeight = 40.dp)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(2.dp)
            .background(fill, RectangleShape)
            .then(if (border != null) Modifier.border(1.dp, border, RectangleShape) else Modifier),
    ) {
        // Dimmed when disabled, as Material dims its own icon buttons.
        val tint = LocalContentColor.current
        CompositionLocalProvider(
            LocalContentColor provides if (enabled) tint else tint.copy(alpha = 0.38f),
        ) {
            content()
        }
    }
}

/**
 * A text button on a square tile: Material's TextButton has no background and
 * a pill-shaped ripple. The tile is a tint of the text colour, as on
 * [SquareIconButton], so the control reads as one on every surface.
 */
@Composable
internal fun SquareTextButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    val ds = LocalDsColors.current
    TextButton(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled,
        shape = RectangleShape,
        colors = ButtonDefaults.textButtonColors(
            containerColor = ds.text.copy(alpha = 0.10f),
            disabledContainerColor = ds.text.copy(alpha = 0.05f),
        ),
        content = content,
    )
}

/** Material's filled Button with square corners: its own is a pill. */
@Composable
internal fun SquareButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    Button(
        onClick = onClick,
        modifier = modifier,
        enabled = enabled,
        shape = RectangleShape,
        content = content,
    )
}

/**
 * A popup with the desktop dialog's chrome: a dark header, a bordered square
 * body, and a footer of square buttons.
 *
 * Material's alert dialog is the same shape as everything else now, but its
 * title, surface and button row still read as a stock card. This is the one
 * the phone's questions use.
 */
@Composable
internal fun DsDialog(
    onDismissRequest: () -> Unit,
    confirmButton: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    dismissButton: @Composable (() -> Unit)? = null,
    icon: @Composable (() -> Unit)? = null,
    title: @Composable (() -> Unit)? = null,
    text: @Composable (() -> Unit)? = null,
) {
    val ds = LocalDsColors.current
    Dialog(
        onDismissRequest = onDismissRequest,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier.fillMaxSize().padding(24.dp),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                modifier
                    .widthIn(min = 280.dp, max = 440.dp)
                    .fillMaxWidth()
                    .background(ds.bg1, RectangleShape)
                    .border(1.dp, ds.border, RectangleShape),
            ) {
                if (title != null || icon != null) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .background(ds.bg0)
                            .padding(horizontal = 16.dp, vertical = 14.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        if (icon != null) {
                            icon()
                            if (title != null) Spacer(Modifier.height(8.dp))
                        }
                        if (title != null) {
                            ProvideTextStyle(
                                MaterialTheme.typography.titleMedium.copy(
                                    fontWeight = FontWeight.Bold,
                                    color = ds.text,
                                ),
                            ) {
                                title()
                            }
                        }
                    }
                    Box(Modifier.fillMaxWidth().height(1.dp).background(ds.divider))
                }
                if (text != null) {
                    Box(
                        Modifier
                            .heightIn(max = 420.dp)
                            .verticalScroll(rememberScrollState())
                            .padding(16.dp)
                            .fillMaxWidth(),
                    ) {
                        ProvideTextStyle(
                            MaterialTheme.typography.bodyMedium.copy(color = ds.text),
                        ) {
                            text()
                        }
                    }
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(ds.divider))
                Row(
                    Modifier
                        .fillMaxWidth()
                        .background(ds.bg0)
                        .padding(horizontal = 12.dp, vertical = 10.dp),
                    horizontalArrangement = Arrangement.End,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    dismissButton?.invoke()
                    if (dismissButton != null) Spacer(Modifier.width(8.dp))
                    confirmButton()
                }
            }
        }
    }
}

/**
 * A QR symbol from rows of `1` (dark) and `0` (light), including its quiet zone.
 *
 * Drawn on white: a scanner needs that contrast in every skin.
 */
@Composable
internal fun QrImage(rows: List<String>, modifier: Modifier = Modifier) {
    Canvas(
        modifier
            .aspectRatio(1f)
            .background(Color.White, RectangleShape)
            .border(1.dp, LocalDsColors.current.border, RectangleShape),
    ) {
        if (rows.isEmpty()) return@Canvas
        val n = rows[0].length
        if (n == 0 || rows.any { it.length != n }) return@Canvas
        val cell = size.minDimension / n
        val drawn = cell * n
        val originX = (size.width - drawn) / 2f
        val originY = (size.height - drawn) / 2f
        val dark = Color(0xFF111214)
        rows.forEachIndexed { y, row ->
            row.forEachIndexed { x, bit ->
                if (bit == '1') {
                    drawRect(
                        dark,
                        Offset(originX + x * cell, originY + y * cell),
                        Size(cell, cell),
                    )
                }
            }
        }
    }
}
