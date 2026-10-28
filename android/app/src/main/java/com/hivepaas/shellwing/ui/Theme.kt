package com.hivepaas.shellwing.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * The desktop's app colors (crates/app/src/colors.rs, Dusk), by what each
 * one is for, as the iOS app has them (ios/Shellwing/Theme.swift).
 */
object Dusk {
    val chrome = rgb(0x21272f)
    val panel = rgb(0x252b34)
    val page = rgb(0x272e38)
    val surface = rgb(0x2b323d)
    val raised = rgb(0x2e3541)
    val chip = rgb(0x343c48)
    val field = rgb(0x1c2129)
    val line = rgb(0x363e4a)
    val fieldEdge = rgb(0x424b58)
    val edge = rgb(0x46505e)
    val button = rgb(0x2f3641)

    val text = rgb(0xe7ebf0)
    val soft = rgb(0xc9d0d9)
    val muted = rgb(0xa0aab7)

    val accent = rgb(0x4cc4f9)
    val onAccent = rgb(0x04121c)
    /** Material's tonal container: the selected destination, a chip on. */
    val accentContainer = rgb(0x1f4a5f)
    val onAccentContainer = rgb(0xcdefff)

    val ok = rgb(0x22c55e)
    val amber = rgb(0xf59e0b)
    val lost = rgb(0xf97316)
    val idle = rgb(0x7b8595)
    val bad = rgb(0xf87171)
}

/** `0xrrggbb`, opaque. */
fun rgb(hex: Long) = Color(0xff000000 or hex)

/** `0xrrggbb` as the core gives colors. */
fun rgb(hex: UInt) = rgb(hex.toLong())

private val scheme = darkColorScheme(
    primary = Dusk.accent,
    onPrimary = Dusk.onAccent,
    primaryContainer = Dusk.accentContainer,
    onPrimaryContainer = Dusk.onAccentContainer,
    secondary = Dusk.accent,
    onSecondary = Dusk.onAccent,
    secondaryContainer = Dusk.accentContainer,
    onSecondaryContainer = Dusk.onAccentContainer,
    background = Dusk.panel,
    onBackground = Dusk.text,
    surface = Dusk.panel,
    onSurface = Dusk.text,
    surfaceVariant = Dusk.chip,
    onSurfaceVariant = Dusk.muted,
    surfaceContainerLowest = Dusk.field,
    surfaceContainerLow = Dusk.surface,
    surfaceContainer = Dusk.chrome,
    surfaceContainerHigh = Dusk.raised,
    surfaceContainerHighest = Dusk.chip,
    outline = rgb(0x6b7584),
    outlineVariant = Dusk.line,
    error = Dusk.bad,
)

@Composable
fun ShellwingTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = scheme, content = content)
}
