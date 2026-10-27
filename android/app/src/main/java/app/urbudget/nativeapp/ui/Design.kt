package app.urbudget.nativeapp.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R

/** The phone's side margin (the desktop's `--gutter` on phones). */
internal val Gutter = 16.dp

/** Room a list keeps at its end so its last row clears the floating tab bar and add button. */
internal val DockRoom = 168.dp

/** The window's height, for sizing sheets and pickers. */
@Composable
internal fun screenHeight(): Dp = with(androidx.compose.ui.platform.LocalDensity.current) {
    androidx.compose.ui.platform.LocalWindowInfo.current.containerSize.height.toDp()
}

/**
 * Shows a short message above the tab bar; with `undo`, it offers Undo for the last change, as the
 * desktop's toasts do after moving money, quick assign, hiding or deleting.
 */
internal val LocalToast = staticCompositionLocalOf<(String, Boolean) -> Unit> { { _, _ -> } }

/** Whether content sits in a sheet, where cards are grey on the sheet's white rather than white on grey. */
internal val LocalSheet = staticCompositionLocalOf { false }

/** A card sets the fill its fields use, so a field always stands out from what it sits on. */
internal val LocalFieldFill = staticCompositionLocalOf<Color?> { null }

/** Fields are grey on white (sheets, cards) and white on the grey page. */
@Composable
internal fun fieldFill(): Color = LocalFieldFill.current ?: if (LocalSheet.current) ur.surface2 else ur.surface

/** Sheets, menus and the drawer rise over a dimmed page; on the dark theme only an edge, no shadow. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun UrSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    // The sheet stops below the status bar however tall its content grows.
    ModalBottomSheet(onDismiss, Modifier.windowInsetsPadding(WindowInsets.statusBars), sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp), containerColor = ur.surface, contentColor = ur.text,
        scrimColor = ur.scrim, tonalElevation = 0.dp,
        dragHandle = { Box(Modifier.padding(top = 8.dp, bottom = 4.dp).size(36.dp, 5.dp).clip(CircleShape).background(ur.borderStrong)) }) {
        FollowLanguage { CompositionLocalProvider(LocalSheet provides true) { Column(Modifier.fillMaxWidth().imePadding(), content = content) } }
    }
}

/** Cancel, the title, and the sheet's action, as a phone's sheets put them. */
@Composable
internal fun SheetHeader(title: String, onCancel: (() -> Unit)?, confirm: String?, onConfirm: (() -> Unit)?, enabled: Boolean = true,
                         onBack: (() -> Unit)? = null, cancelFirst: Boolean = false) {
    Box(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(horizontal = 6.dp)) {
        when {
            onBack != null -> IconButton(onBack, Modifier.align(Alignment.CenterStart)) { Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, stringResource(R.string.common_back), tint = ur.accent) }
            onCancel != null && (confirm != null || cancelFirst) -> TextButton(onCancel, Modifier.align(Alignment.CenterStart)) { Text(stringResource(R.string.common_cancel), color = ur.accent, fontWeight = FontWeight.Normal) }
        }
        Text(Money.own(title), Modifier.align(Alignment.Center).padding(horizontal = 96.dp), style = MaterialTheme.typography.titleMedium, color = ur.text,
            maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
        if (confirm != null && onConfirm != null) TextButton(onConfirm, Modifier.align(Alignment.CenterEnd), enabled = enabled) {
            Text(confirm, color = if (enabled) ur.accent else ur.muted, fontWeight = FontWeight.Bold)
        } else if (onCancel != null && !cancelFirst) TextButton(onCancel, Modifier.align(Alignment.CenterEnd)) { Text(stringResource(R.string.common_done), color = ur.accent, fontWeight = FontWeight.Bold) }
    }
}

@Composable
internal fun PrimaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
                           color: Color = ur.accent, icon: ImageVector? = null) {
    Button(onClick, modifier.heightIn(min = 48.dp), enabled = enabled, shape = RoundedCornerShape(24.dp),
        colors = ButtonDefaults.buttonColors(containerColor = color, contentColor = if (ur.dark) ur.bg else Color.White,
            disabledContainerColor = ur.zeroSoft, disabledContentColor = ur.muted),
        contentPadding = PaddingValues(horizontal = 22.dp)) {
        if (icon != null) { Icon(icon, null, Modifier.size(20.dp)); Spacer(Modifier.width(8.dp)) }
        Text(text, fontWeight = FontWeight.Bold, fontSize = 16.sp)
    }
}

@Composable
internal fun SecondaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true,
                             icon: ImageVector? = null, color: Color = ur.text) {
    OutlinedButton(onClick, modifier.heightIn(min = 46.dp), enabled = enabled, shape = RoundedCornerShape(23.dp),
        border = BorderStroke(1.dp, ur.borderStrong), colors = ButtonDefaults.outlinedButtonColors(containerColor = ur.surface, contentColor = color),
        contentPadding = PaddingValues(horizontal = 18.dp)) {
        if (icon != null) { Icon(icon, null, Modifier.size(19.dp)); Spacer(Modifier.width(8.dp)) }
        Text(text, fontWeight = FontWeight.SemiBold)
    }
}

/** The top bar's round buttons: 42dp, the surface colour, a hairline ring. */
@Composable
internal fun RoundIconButton(icon: ImageVector, label: String, onClick: () -> Unit, modifier: Modifier = Modifier,
                             tint: Color = ur.text, enabled: Boolean = true, dot: Boolean = false) {
    Box(modifier.size(42.dp)) {
        Box(Modifier.fillMaxSize().clip(CircleShape).background(ur.surface).border(1.dp, ur.border, CircleShape)
            .clickable(enabled = enabled, role = Role.Button, onClickLabel = label, onClick = onClick), contentAlignment = Alignment.Center) {
            Icon(icon, label, Modifier.size(21.dp), tint = if (enabled) tint else ur.muted)
        }
        if (dot) Box(Modifier.align(Alignment.TopEnd).padding(2.dp).size(10.dp).clip(CircleShape).background(ur.accent).border(2.dp, ur.bg, CircleShape))
    }
}

/**
 * A page's top bar on the page's own background: its title large on the left, or something of
 * its own in the middle (the month, on the budget), and round buttons on the right.
 */
@Composable
internal fun PhoneTopBar(title: String? = null, subtitle: String? = null, onBack: (() -> Unit)? = null,
                         leading: (@Composable () -> Unit)? = null, center: (@Composable () -> Unit)? = null,
                         actions: @Composable RowScope.() -> Unit = {}) {
    Box(Modifier.fillMaxWidth().background(ur.bg).statusBarsPadding().height(64.dp).padding(horizontal = 12.dp)) {
        Row(Modifier.align(Alignment.CenterStart), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (onBack != null) RoundIconButton(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, stringResource(R.string.common_back), onBack)
            leading?.invoke()
            if (center == null && title != null) Column(Modifier.padding(start = if (onBack == null && leading == null) 4.dp else 2.dp)) {
                Text(Money.own(title), style = if (onBack == null) MaterialTheme.typography.headlineLarge else MaterialTheme.typography.headlineSmall,
                    color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1) }
            }
        }
        if (center != null) Box(Modifier.align(Alignment.Center)) { center() }
        Row(Modifier.align(Alignment.CenterEnd), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp), content = actions)
    }
}

/** What a category's state looks like: fill and text for the phone's solid pill. */
internal data class StatusColors(val fill: Color, val content: Color, val ink: Color)

@Composable
internal fun statusColors(status: String): StatusColors {
    val onSolid = if (ur.dark) ur.bg else Color.White
    return when (status) {
        "positive" -> StatusColors(ur.positive, onSolid, ur.positive)
        "underfunded", "credit" -> StatusColors(ur.warning, onSolid, ur.warning)
        "negative" -> StatusColors(ur.negative, onSolid, ur.negative)
        else -> StatusColors(ur.zeroSoft, ur.text2, ur.muted)
    }
}

/** Available as a solid pill: green, amber, red or grey. Tapping it moves money. */
@Composable
internal fun AvailablePill(amount: Long, currency: String, status: String, modifier: Modifier = Modifier, onClick: (() -> Unit)? = null,
                           label: String? = null) {
    val colors = statusColors(status)
    Row(modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(colors.fill)
        .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClickLabel = label, onClick = onClick) else Modifier)
        .padding(horizontal = 11.dp), verticalAlignment = Alignment.CenterVertically) {
        if (status == "credit") { Icon(Icons.Outlined.CreditCard, null, Modifier.size(13.dp), tint = colors.content); Spacer(Modifier.width(4.dp)) }
        Text(money(amount, currency), color = colors.content, fontSize = 15.sp, fontWeight = FontWeight.Bold, style = Tabular, maxLines = 1)
    }
}

/** A target's 4dp bar: amber while short, a lighter green once funded, red when overspent. */
@Composable
internal fun TargetBar(progress: Int, status: String, modifier: Modifier = Modifier) {
    val fill = when {
        status == "negative" -> ur.negative
        progress >= 100 -> ur.positive.copy(alpha = .55f)
        else -> ur.warning
    }
    Box(modifier.height(4.dp).clip(RoundedCornerShape(2.dp)).background(ur.text.copy(alpha = .08f))) {
        Box(Modifier.fillMaxHeight().fillMaxWidth(progress.coerceIn(0, 100) / 100f).clip(RoundedCornerShape(2.dp)).background(fill))
    }
}

/** A white card of rows on the grey page, rows divided by hairlines. */
@Composable
internal fun ListCard(modifier: Modifier = Modifier, fill: Color = if (LocalSheet.current) ur.surface2 else ur.surface, content: @Composable ColumnScope.() -> Unit) {
    CompositionLocalProvider(LocalFieldFill provides if (fill == ur.surface) ur.surface2 else ur.surface) {
        Column(modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(fill)
            .then(if (ur.dark) Modifier.border(1.dp, ur.border, RoundedCornerShape(18.dp)) else Modifier), content = content)
    }
}

@Composable
internal fun Hairline(modifier: Modifier = Modifier, start: Dp = 0.dp) {
    HorizontalDivider(modifier.padding(start = start), thickness = 0.5.dp, color = ur.border)
}

/** A row of a card: an optional round icon, the title over a detail line, a value, and a chevron. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
internal fun ListRow(title: String, modifier: Modifier = Modifier, subtitle: String? = null, value: String? = null, valueColor: Color = ur.text,
                     icon: ImageVector? = null, iconTint: Color = ur.accent, iconFill: Color = ur.accentSoft, titleColor: Color = ur.text,
                     onClick: (() -> Unit)? = null, onLongClick: (() -> Unit)? = null, chevron: Boolean = onClick != null,
                     divider: Boolean = false, trailing: (@Composable () -> Unit)? = null) {
    Column(modifier) {
        Row(Modifier.fillMaxWidth()
            .then(if (onClick != null || onLongClick != null) Modifier.combinedClickable(onClick = { onClick?.invoke() }, onLongClick = onLongClick) else Modifier)
            .heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            if (icon != null) {
                Box(Modifier.size(38.dp).clip(CircleShape).background(iconFill), contentAlignment = Alignment.Center) {
                    Icon(icon, null, Modifier.size(20.dp), tint = iconTint)
                }
                Spacer(Modifier.width(14.dp))
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                // A name the person gave (an account, a payee) keeps its own direction in Arabic.
                Text(Money.own(title), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = titleColor, maxLines = 2, overflow = TextOverflow.Ellipsis)
                subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 3, overflow = TextOverflow.Ellipsis) }
            }
            if (value != null) Text(value, Modifier.padding(start = 10.dp), style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = valueColor, maxLines = 1)
            trailing?.let { Box(Modifier.padding(start = 10.dp)) { it() } }
            if (chevron) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.padding(start = 4.dp).size(20.dp), tint = ur.muted)
        }
        if (divider) Hairline(start = if (icon != null) 68.dp else 16.dp)
    }
}

/** A row with a switch, for settings in a card. */
@Composable
internal fun SwitchRow(title: String, checked: Boolean, onChange: (Boolean) -> Unit, subtitle: String? = null, enabled: Boolean = true, divider: Boolean = false) {
    ListRow(title, subtitle = subtitle, onClick = if (enabled) ({ onChange(!checked) }) else null, chevron = false, divider = divider) {
        Switch(checked, onChange, enabled = enabled, colors = SwitchDefaults.colors(checkedTrackColor = ur.positive, checkedThumbColor = Color.White,
            uncheckedTrackColor = ur.zeroSoft, uncheckedBorderColor = ur.zeroSoft, uncheckedThumbColor = Color.White))
    }
}

/** A checkbox with its words, aligned on one line. */
@Composable
internal fun CheckRow(text: String, checked: Boolean, onChange: (Boolean) -> Unit, enabled: Boolean = true) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable(enabled) { onChange(!checked) }.padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Checkbox(checked, onChange, enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
        Text(text, style = MaterialTheme.typography.bodyMedium, color = ur.text)
    }
}

/** The desktop's segmented tabs: a grey track with the chosen option raised. */
@Composable
internal fun Segmented(options: List<Pair<String, String>>, selected: String, onSelect: (String) -> Unit, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(if (ur.dark) Color(0xFF0A0C15) else Color(0xFFE4E4EA)).padding(3.dp)) {
        options.forEach { (id, label) ->
            val chosen = id == selected
            Box(Modifier.weight(1f).height(36.dp).clip(RoundedCornerShape(9.dp))
                .then(if (chosen) Modifier.background(if (ur.dark) Color(0xFF2A2F47) else Color.White) else Modifier)
                .clickable(role = Role.Tab) { onSelect(id) }, contentAlignment = Alignment.Center) {
                Text(label, fontWeight = if (chosen) FontWeight.Bold else FontWeight.Medium, color = if (chosen) ur.text else ur.text2, fontSize = 15.sp, maxLines = 1)
            }
        }
    }
}

/** A category chip under a transaction: 26dp, grey; yellow when it still needs a category. */
@Composable
internal fun TagChip(text: String, modifier: Modifier = Modifier, fill: Color = ur.zeroSoft, color: Color = ur.text, icon: ImageVector? = null) {
    Row(modifier.heightIn(min = 26.dp).clip(RoundedCornerShape(8.dp)).background(fill).padding(horizontal = 9.dp), verticalAlignment = Alignment.CenterVertically) {
        if (icon != null) { Icon(icon, null, Modifier.size(14.dp), tint = color); Spacer(Modifier.width(4.dp)) }
        Text(Money.own(text), color = color, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** A small rounded action in a row, such as a schedule's Enter and Skip. */
@Composable
internal fun SmallAction(text: String, onClick: () -> Unit, primary: Boolean = false) {
    Box(Modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(if (primary) ur.accentSoft else ur.zeroSoft)
        .clickable(role = Role.Button, onClick = onClick).padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
        Text(text, color = if (primary) ur.accent else ur.text2, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** A filter chip that shows whether it's on. */
@Composable
internal fun FilterPill(text: String, active: Boolean, onClick: () -> Unit, icon: ImageVector? = null) {
    Row(Modifier.heightIn(min = 34.dp).clip(RoundedCornerShape(17.dp)).background(if (active) ur.accentSoft else ur.surface)
        .border(1.dp, if (active) ur.accent.copy(alpha = .35f) else ur.border, RoundedCornerShape(17.dp))
        .clickable(onClick = onClick).padding(horizontal = 13.dp), verticalAlignment = Alignment.CenterVertically) {
        if (icon != null) { Icon(icon, null, Modifier.size(16.dp), tint = if (active) ur.accent else ur.text2); Spacer(Modifier.width(5.dp)) }
        Text(text, color = if (active) ur.accent else ur.text2, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** Uncleared is a dashed ring, cleared a green disc with a tick, reconciled a lock. */
@Composable
internal fun ClearedMark(status: String, modifier: Modifier = Modifier, onClick: (() -> Unit)? = null) {
    val clickable = if (onClick != null) Modifier.clip(CircleShape).clickable(role = Role.Checkbox, onClickLabel = stringResource(R.string.cleared_toggle), onClick = onClick) else Modifier
    Box(modifier.size(28.dp).then(clickable), contentAlignment = Alignment.Center) {
        when (status) {
            "cleared" -> Box(Modifier.size(19.dp).clip(CircleShape).background(ur.positive), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Check, stringResource(R.string.status_cleared), Modifier.size(13.dp), tint = ur.surface)
            }
            "reconciled" -> Box(Modifier.size(20.dp).clip(CircleShape).background(ur.positiveSoft), contentAlignment = Alignment.Center) {
                Icon(Icons.Outlined.Lock, stringResource(R.string.status_reconciled), Modifier.size(12.dp), tint = ur.positive)
            }
            else -> {
                val ring = ur.muted
                Canvas(Modifier.size(18.dp)) {
                    val r = size.minDimension / 2 - 1.dp.toPx()
                    val dash = (2 * Math.PI * r / 20).toFloat()
                    drawCircle(ring, r, style = Stroke(1.4.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(dash, dash))))
                }
            }
        }
    }
}

/** A search box: grey, rounded, with the magnifier and a clear button. */
@Composable
internal fun SearchField(value: String, onChange: (String) -> Unit, placeholder: String, modifier: Modifier = Modifier, inputModifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().height(44.dp).clip(RoundedCornerShape(12.dp)).background(if (ur.dark) ur.surface2 else Color(0xFFE6E6EB)).padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Outlined.Search, null, Modifier.size(20.dp), tint = ur.muted)
        Spacer(Modifier.width(8.dp))
        Box(Modifier.weight(1f)) {
            if (value.isEmpty()) Text(placeholder, color = ur.muted, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            BasicTextField(value, onChange, inputModifier.fillMaxWidth(), singleLine = true, textStyle = MaterialTheme.typography.bodyLarge.copy(color = ur.text),
                cursorBrush = SolidColor(ur.accent))
        }
        if (value.isNotEmpty()) Icon(Icons.Outlined.Cancel, stringResource(R.string.common_clear_search), Modifier.size(20.dp).clip(CircleShape).clickable { onChange("") }, tint = ur.muted)
    }
}

/** A coloured round dot for a flag. */
@Composable
internal fun FlagDot(color: String, modifier: Modifier = Modifier, size: Dp = 12.dp) {
    Box(modifier.size(size).clip(CircleShape).background(ur.flags[color] ?: ur.muted))
}

/** A label over a figure, for summary strips. */
@Composable
internal fun Figure(label: String, value: String, modifier: Modifier = Modifier, color: Color = ur.text, alignEnd: Boolean = false) {
    Column(modifier, horizontalAlignment = if (alignEnd) Alignment.End else Alignment.Start) {
        Text(Money.own(label), style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1)
        Text(value, style = MaterialTheme.typography.titleMedium.merge(Tabular), color = color, maxLines = 1)
    }
}

/** A notice card: amber for attention, red for a problem. */
@Composable
internal fun Notice(text: String, modifier: Modifier = Modifier, tone: String = "attention", action: String? = null, onAction: (() -> Unit)? = null,
                    icon: ImageVector = Icons.Outlined.Info) {
    val fill = when (tone) { "negative" -> ur.negativeSoft; "positive" -> ur.positiveSoft; "info" -> ur.accentSoft; else -> ur.warningSoft }
    val ink = when (tone) { "negative" -> ur.negative; "positive" -> ur.positive; "info" -> ur.accent; else -> ur.warning }
    Row(modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(fill).padding(start = 14.dp, end = 8.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, Modifier.size(20.dp), tint = ink)
        Spacer(Modifier.width(10.dp))
        Text(text, Modifier.weight(1f).padding(vertical = 4.dp), style = MaterialTheme.typography.bodyMedium, color = ur.text)
        if (action != null && onAction != null) TextButton(onAction) { Text(action, color = ink, fontWeight = FontWeight.Bold) }
    }
}

/**
 * A screen of its own over the app (adding or editing a transaction takes the whole screen). It is a
 * layer of the main window rather than a dialog window, so it draws under the system bars in the
 * app's theme; its pickers and confirmations still rise above it as sheets and dialogs.
 */
@Composable
internal fun FullScreenDialog(onDismiss: () -> Unit, content: @Composable () -> Unit) {
    androidx.activity.compose.BackHandler(onBack = onDismiss)
    val shown = remember { androidx.compose.animation.core.MutableTransitionState(false).apply { targetState = true } }
    androidx.compose.animation.AnimatedVisibility(shown, enter = androidx.compose.animation.slideInVertically(androidx.compose.animation.core.tween(240)) { it / 6 } +
        androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(200))) {
        Box(Modifier.fillMaxSize().background(ur.bg).clickable(remember { androidx.compose.foundation.interaction.MutableInteractionSource() }, null) {}) { content() }
    }
}

/** The shadow floating elements cast on the light theme; dark gets an edge instead. */
internal fun Modifier.floating(shape: androidx.compose.ui.graphics.Shape, dark: Boolean, edge: Color) =
    if (dark) this.border(1.dp, edge, shape) else this.shadow(14.dp, shape, ambientColor = Color(0x33000000), spotColor = Color(0x33000000))
