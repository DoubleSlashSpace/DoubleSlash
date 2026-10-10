package com.doubleslash.client.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.unit.dp

/**
 * The app theme: the DoubleSlash palette (see [DsColors]) with the user's skin
 * laid over it, mapped onto Material 3's slots.
 *
 * The same palette and skins as the desktop, rather than Material You's
 * wallpaper colours: the two clients are one product, and a skin copied from
 * one has to look the same on the other. Colours Material has no slot for —
 * online, warning, the four background layers — come from [LocalDsColors].
 */
@Composable
fun DoubleSlashTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    skinJson: String = "",
    content: @Composable () -> Unit,
) {
    val palette = remember(darkTheme, skinJson) { paletteFor(darkTheme, skinJson) }
    val scheme = remember(palette, darkTheme) { schemeFor(palette, darkTheme) }
    CompositionLocalProvider(LocalDsColors provides palette) {
        MaterialTheme(colorScheme = scheme, shapes = SquareShapes, content = content)
    }
}

private fun schemeFor(p: DsColors, dark: Boolean) = if (dark) {
    darkColorScheme(
        primary = p.accent,
        onPrimary = p.textInv,
        primaryContainer = p.accent.copy(alpha = 0.28f).compositeOver(p.bg2),
        onPrimaryContainer = p.text,
        secondary = p.muted,
        onSecondary = p.bg1,
        secondaryContainer = p.bg3,
        onSecondaryContainer = p.text,
        tertiary = p.online,
        onTertiary = Color.White,
        background = p.bg1,
        onBackground = p.text,
        surface = p.bg1,
        onSurface = p.text,
        surfaceVariant = p.bg2,
        onSurfaceVariant = p.muted,
        surfaceContainerLowest = p.bg0,
        surfaceContainerLow = p.bg1,
        surfaceContainer = p.bg2,
        surfaceContainerHigh = p.bg2,
        surfaceContainerHighest = p.bg3,
        inverseSurface = p.text,
        inverseOnSurface = p.bg1,
        error = p.danger,
        onError = Color.White,
        outline = p.divider,
        outlineVariant = p.border,
    )
} else {
    lightColorScheme(
        primary = p.accent,
        onPrimary = p.textInv,
        primaryContainer = p.accent.copy(alpha = 0.18f).compositeOver(p.bg1),
        onPrimaryContainer = p.text,
        secondary = p.muted,
        onSecondary = p.bg1,
        secondaryContainer = p.bg3,
        onSecondaryContainer = p.text,
        tertiary = p.online,
        onTertiary = Color.White,
        background = p.bg1,
        onBackground = p.text,
        surface = p.bg1,
        onSurface = p.text,
        surfaceVariant = p.bg2,
        onSurfaceVariant = p.muted,
        surfaceContainerLowest = p.bg1,
        surfaceContainerLow = p.bg0,
        surfaceContainer = p.bg2,
        surfaceContainerHigh = p.bg2,
        surfaceContainerHighest = p.bg3,
        inverseSurface = p.text,
        inverseOnSurface = p.bg1,
        error = p.danger,
        onError = Color.White,
        outline = p.divider,
        outlineVariant = p.border,
    )
}

/**
 * Square corners at every size: text fields, dialogs, menus, cards and
 * tooltips take their shape from these. The UI is angular throughout, as on
 * the desktop, whose Material controls are set to `NotRounded`.
 */
private val SquareShapes = RoundedCornerShape(0.dp).let { square ->
    Shapes(
        extraSmall = square,
        small = square,
        medium = square,
        large = square,
        extraLarge = square,
    )
}
