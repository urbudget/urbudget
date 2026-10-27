package app.urbudget.nativeapp.ui

import android.app.Activity
import android.content.SharedPreferences
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat

/**
 * The desktop's design tokens (desktop/src/styles.css) as a phone shows them: light is iOS-style
 * grouped grey under white cards, dark is navy rather than the computer's charcoal, which reads
 * green on phone screens. The words and colours stay UrBudget's; the structure follows YNAB's app.
 */
@Immutable
data class UrColors(
    val dark: Boolean,
    val bg: Color, val surface: Color, val surface2: Color,
    val border: Color, val borderStrong: Color,
    val text: Color, val text2: Color, val muted: Color,
    val accent: Color, val accentSoft: Color,
    val positive: Color, val positiveSoft: Color,
    val negative: Color, val negativeSoft: Color,
    val warning: Color, val warningSoft: Color,
    val attention: Color, val onAttention: Color,
    val zeroSoft: Color, val tabActive: Color,
    val scrim: Color, val glass: Color,
    val flags: Map<String, Color>,
)

internal val LightUr = UrColors(
    dark = false,
    bg = Color(0xFFF2F2F7), surface = Color.White, surface2 = Color(0xFFF5F5F7),
    border = Color(0x14000000), borderStrong = Color(0x26000000),
    text = Color(0xFF1D1D1F), text2 = Color(0xFF4B4B52), muted = Color(0xFF65656C),
    accent = Color(0xFF0068DA), accentSoft = Color(0xFFE8F1FE),
    positive = Color(0xFF197A35), positiveSoft = Color(0xFFE6F6EB),
    negative = Color(0xFFCC2229), negativeSoft = Color(0xFFFFEBEA),
    warning = Color(0xFFAD4F00), warningSoft = Color(0xFFFFF1DF),
    attention = Color(0xFFFFCC00), onAttention = Color(0xFF1D1D1F),
    zeroSoft = Color(0xFFE9E9EE), tabActive = Color(0x380068DA),
    scrim = Color(0x6B000000), glass = Color(0xF4FFFFFF),
    flags = mapOf("red" to Color(0xFFFF3B30), "orange" to Color(0xFFFF9500), "yellow" to Color(0xFFFFCC00),
        "green" to Color(0xFF34C759), "blue" to Color(0xFF007AFF), "purple" to Color(0xFFAF52DE)),
)

internal val DarkUr = UrColors(
    dark = true,
    bg = Color(0xFF0C0E18), surface = Color(0xFF171A28), surface2 = Color(0xFF1D2133),
    border = Color(0xFF272B3E), borderStrong = Color(0xFF373C54),
    text = Color(0xFFE5E8EC), text2 = Color(0xFFB7BBCC), muted = Color(0xFF8C91A7),
    accent = Color(0xFF5B93FF), accentSoft = Color(0xFF1B2944),
    positive = Color(0xFF56C67F), positiveSoft = Color(0xFF15301F),
    negative = Color(0xFFFF8077), negativeSoft = Color(0xFF3B1D1B),
    warning = Color(0xFFE2B44A), warningSoft = Color(0xFF392E13),
    attention = Color(0xFFE2B44A), onAttention = Color(0xFF14161A),
    zeroSoft = Color(0xFF252A3C), tabActive = Color(0x475B93FF),
    scrim = Color(0x99080B0F), glass = Color(0xF41A1D2C),
    flags = mapOf("red" to Color(0xFFE5484D), "orange" to Color(0xFFF76B15), "yellow" to Color(0xFFE2A336),
        "green" to Color(0xFF30A46C), "blue" to Color(0xFF3E63DD), "purple" to Color(0xFF8E4EC6)),
)

/** Light by default, so components render outside the app's theme (previews and UI tests). */
internal val LocalUr = staticCompositionLocalOf { LightUr }

/** The tokens of the theme in use. */
internal val ur: UrColors @Composable @ReadOnlyComposable get() = LocalUr.current

/** Money lines up in columns, as the desktop's `tabular-nums` does. */
internal val Tabular = TextStyle(fontFeatureSettings = "tnum")

private fun scheme(c: UrColors) = (if (c.dark) darkColorScheme() else lightColorScheme()).copy(
    primary = c.accent, onPrimary = if (c.dark) Color(0xFF0C0E18) else Color.White,
    primaryContainer = c.accentSoft, onPrimaryContainer = c.accent,
    secondary = c.text2, onSecondary = c.surface, secondaryContainer = c.zeroSoft, onSecondaryContainer = c.text,
    tertiary = c.positive, tertiaryContainer = c.positiveSoft, onTertiaryContainer = c.positive,
    background = c.bg, onBackground = c.text,
    surface = c.surface, onSurface = c.text, surfaceVariant = c.surface2, onSurfaceVariant = c.muted,
    surfaceContainerLowest = c.surface, surfaceContainerLow = c.surface, surfaceContainer = c.surface,
    surfaceContainerHigh = c.surface, surfaceContainerHighest = c.surface, surfaceBright = c.surface, surfaceDim = c.bg,
    inverseSurface = if (c.dark) Color(0xFF262C34) else Color(0xFF1D1D1F), inverseOnSurface = if (c.dark) c.text else Color(0xFFF5F5F7),
    outline = if (c.dark) Color(0xFF4A5070) else Color(0x40000000), outlineVariant = c.border,
    error = c.negative, onError = if (c.dark) Color(0xFF0C0E18) else Color.White, errorContainer = c.negativeSoft, onErrorContainer = c.negative,
    scrim = c.scrim,
)

private val base = Typography()
private val UrType = Typography(
    displaySmall = base.displaySmall.copy(fontWeight = FontWeight.Bold, letterSpacing = (-0.5).sp),
    headlineLarge = base.headlineLarge.copy(fontSize = 28.sp, lineHeight = 34.sp, fontWeight = FontWeight.ExtraBold, letterSpacing = (-0.4).sp),
    headlineMedium = base.headlineMedium.copy(fontSize = 24.sp, lineHeight = 30.sp, fontWeight = FontWeight.Bold),
    headlineSmall = base.headlineSmall.copy(fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.Bold),
    titleLarge = base.titleLarge.copy(fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.Bold),
    titleMedium = base.titleMedium.copy(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold),
    titleSmall = base.titleSmall.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold),
    bodyLarge = base.bodyLarge.copy(fontSize = 16.sp, lineHeight = 22.sp, letterSpacing = 0.sp),
    bodyMedium = base.bodyMedium.copy(fontSize = 15.sp, lineHeight = 20.sp, letterSpacing = 0.sp),
    bodySmall = base.bodySmall.copy(fontSize = 13.sp, lineHeight = 18.sp, letterSpacing = 0.sp),
    labelLarge = base.labelLarge.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.sp),
    labelMedium = base.labelMedium.copy(fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.sp),
    labelSmall = base.labelSmall.copy(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.sp),
)
/**
 * Arabic is joined script: tracking would pull its letters apart, and Android measures tracked
 * Arabic wider than it draws, cutting titles short. Right-to-left text sets none.
 */
private val UrTypeRtl = with(UrType) {
    fun TextStyle.untracked() = copy(letterSpacing = 0.sp)
    Typography(displayLarge = displayLarge.untracked(), displayMedium = displayMedium.untracked(), displaySmall = displaySmall.untracked(),
        headlineLarge = headlineLarge.untracked(), headlineMedium = headlineMedium.untracked(), headlineSmall = headlineSmall.untracked(),
        titleLarge = titleLarge.untracked(), titleMedium = titleMedium.untracked(), titleSmall = titleSmall.untracked(),
        bodyLarge = bodyLarge.untracked(), bodyMedium = bodyMedium.untracked(), bodySmall = bodySmall.untracked(),
        labelLarge = labelLarge.untracked(), labelMedium = labelMedium.untracked(), labelSmall = labelSmall.untracked())
}
/** The small capitals over a list's sections are tracked out a little; Arabic is never tracked. */
internal val headerTracking: TextUnit
    @Composable get() = if (LocalLayoutDirection.current == LayoutDirection.Rtl) 0.sp else .6.sp
private val UrShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp), small = RoundedCornerShape(10.dp), medium = RoundedCornerShape(12.dp),
    large = RoundedCornerShape(18.dp), extraLarge = RoundedCornerShape(28.dp),
)

@Composable
fun UrBudgetTheme(content: @Composable () -> Unit) {
    val context = LocalContext.current
    val preferences = remember { context.getSharedPreferences("native-preferences", 0) }
    var selected by remember { mutableStateOf(preferences.getString("theme", "system")) }
    DisposableEffect(preferences) {
        val listener = SharedPreferences.OnSharedPreferenceChangeListener { prefs, key -> if (key == "theme") selected = prefs.getString("theme", "system") }
        preferences.registerOnSharedPreferenceChangeListener(listener)
        onDispose { preferences.unregisterOnSharedPreferenceChangeListener(listener) }
    }
    val dark = selected == "dark" || selected == "system" && isSystemInDarkTheme()
    val colors = if (dark) DarkUr else LightUr
    // The page draws under the system bars in its own theme, which can differ from the system's.
    val view = LocalView.current
    if (!view.isInEditMode) SideEffect {
        (view.context as? Activity)?.window?.let { window ->
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }
    CompositionLocalProvider(LocalUr provides colors) {
        // Text takes the theme's colour even where no Surface sets one (the shell draws its own pages).
        val typography = if (LocalLayoutDirection.current == LayoutDirection.Rtl) UrTypeRtl else UrType
        MaterialTheme(colorScheme = remember(colors) { scheme(colors) }, typography = typography, shapes = UrShapes) {
            CompositionLocalProvider(LocalContentColor provides colors.text, content = content)
        }
    }
}
