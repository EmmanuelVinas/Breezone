package com.breezone.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Colors specific to Breezone (operation modes and states), same palette as the web UI. */
@Immutable
data class FzColors(
    val heat: Color,
    val cool: Color,
    val fan: Color,
    val dry: Color,
    val success: Color,
    val warning: Color,
    val card: Color,
    val muted: Color,
)

private val DarkFz = FzColors(
    heat = Color(0xFFF89A45),
    cool = Color(0xFF5DB7EE),
    fan = Color(0xFFB395F0),
    dry = Color(0xFF45D3B8),
    success = Color(0xFF48D08A),
    warning = Color(0xFFF2C14E),
    card = Color(0xFF151C2B),
    muted = Color(0xFF8A93A6),
)

private val LightFz = FzColors(
    heat = Color(0xFFE0701F),
    cool = Color(0xFF2F86C9),
    fan = Color(0xFF8A5FD6),
    dry = Color(0xFF1E9C86),
    success = Color(0xFF1F9D5C),
    warning = Color(0xFFC98A0E),
    card = Color(0xFFFFFFFF),
    muted = Color(0xFF6B7385),
)

private val DarkScheme = darkColorScheme(
    primary = Color(0xFF40D0D8),
    onPrimary = Color(0xFF07262B),
    secondary = Color(0xFF26304A),
    onSecondary = Color(0xFFF2F4F8),
    background = Color(0xFF0B1220),
    onBackground = Color(0xFFF2F4F8),
    surface = Color(0xFF0B1220),
    onSurface = Color(0xFFF2F4F8),
    surfaceVariant = Color(0xFF1C2435),
    onSurfaceVariant = Color(0xFF9AA3B5),
    surfaceContainer = Color(0xFF151C2B),
    surfaceContainerHigh = Color(0xFF1C2435),
    surfaceContainerLow = Color(0xFF111827),
    outline = Color(0x33FFFFFF),
    outlineVariant = Color(0x1AFFFFFF),
    error = Color(0xFFF2665A),
)

private val LightScheme = lightColorScheme(
    primary = Color(0xFF0F8F98),
    onPrimary = Color.White,
    background = Color(0xFFF6F8FB),
    surface = Color(0xFFF6F8FB),
    surfaceVariant = Color(0xFFE9EDF3),
    surfaceContainer = Color.White,
    surfaceContainerHigh = Color(0xFFEFF2F7),
    surfaceContainerLow = Color(0xFFF1F4F8),
    onSurfaceVariant = Color(0xFF5B6476),
    outline = Color(0xFFD5DBE5),
    outlineVariant = Color(0xFFE3E8EF),
)

val LocalFzColors = staticCompositionLocalOf { DarkFz }

/** Corner shapes shared by the screens. */
object FzShapes {
    /** Text fields, large buttons and framed blocks of forms. */
    val field = RoundedCornerShape(16.dp)

    /** Regular buttons. */
    val button = RoundedCornerShape(14.dp)
}

object Fz {
    val colors: FzColors @Composable get() = LocalFzColors.current
    val shapes = FzShapes
}

private val FzTypography = Typography().let { t ->
    t.copy(
        displayLarge = t.displayLarge.copy(fontWeight = FontWeight.SemiBold, letterSpacing = (-1.5).sp),
        displayMedium = t.displayMedium.copy(fontWeight = FontWeight.SemiBold, letterSpacing = (-1).sp),
        headlineMedium = t.headlineMedium.copy(fontWeight = FontWeight.SemiBold, letterSpacing = (-0.5).sp),
        titleLarge = t.titleLarge.copy(fontWeight = FontWeight.SemiBold),
        titleMedium = t.titleMedium.copy(fontWeight = FontWeight.SemiBold),
    )
}

/** Tabular numbers for temperatures. */
val TempStyle = TextStyle(fontFeatureSettings = "tnum", fontWeight = FontWeight.SemiBold)

@Composable
fun BreezoneTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalFzColors provides if (dark) DarkFz else LightFz) {
        MaterialTheme(
            colorScheme = if (dark) DarkScheme else LightScheme,
            typography = FzTypography,
            shapes = Shapes(
                small = RoundedCornerShape(12.dp),
                medium = RoundedCornerShape(18.dp),
                large = RoundedCornerShape(26.dp),
            ),
            content = content,
        )
    }
}
