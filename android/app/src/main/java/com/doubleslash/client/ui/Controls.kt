package com.doubleslash.client.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp

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
