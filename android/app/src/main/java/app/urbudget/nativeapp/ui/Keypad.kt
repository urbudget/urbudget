package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Backspace
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.AmountPad
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.LayoutDirection

/**
 * What the keypad holds. A seeded amount (an existing assignment, a balance to move) is replaced by
 * the first digit, as YNAB's keypad does, while an operator first keeps it: "+ 25" adds 25.
 */
@Stable
internal class PadState(initialCents: Long?) {
    var text by mutableStateOf(initialCents?.let(AmountPad::show).orEmpty())
    var fresh by mutableStateOf(initialCents != null)
    fun press(key: String) {
        text = if (fresh && key.length == 1 && key[0].isDigit()) AmountPad.press("", key) else AmountPad.press(text, key)
        fresh = false
    }
    fun set(cents: Long) { text = AmountPad.show(cents); fresh = true }
    // Parsed once per change of text, however many places read them.
    private val parsed by derivedStateOf { if (text.isBlank()) 0L else AmountPad.value(text) }
    private val sum by derivedStateOf { AmountPad.isSum(text) }
    /** The amount in cents, or null while it's a sum that doesn't work out (ends on an operator). */
    val cents: Long? get() = parsed
    val isSum: Boolean get() = sum
    /** The text with the budget's decimal mark. */
    fun display(): String = text.replace(".", Money.decimalMark())
}

@Composable
internal fun rememberPad(initialCents: Long?, vararg keys: Any?) = remember(*keys) { PadState(initialCents) }

/** The phone's keypad for amounts: digits, the four operators, =, ⌫ and Done. */
@Composable
internal fun AmountKeypad(pad: PadState, onDone: () -> Unit, modifier: Modifier = Modifier, doneLabel: String = stringResource(R.string.common_done)) {
    val haptics = LocalHapticFeedback.current
    val delete = stringResource(R.string.keypad_delete)
    val minus = stringResource(R.string.keypad_minus)
    val times = stringResource(R.string.keypad_times)
    val divide = stringResource(R.string.keypad_divide)
    @Composable
    fun RowScope.Key(key: String, weight: Float = 1f, label: String = key, kind: String = "digit", action: (() -> Unit)? = null) {
        val fill = when (kind) { "op" -> ur.zeroSoft; "done" -> ur.accent; else -> ur.surface }
        val ink = when (kind) { "done" -> if (ur.dark) ur.bg else Color.White; "op" -> ur.accent; else -> ur.text }
        Box(Modifier.weight(weight).height(52.dp).clip(RoundedCornerShape(12.dp)).background(fill)
            .then(if (kind == "digit") Modifier.border(0.5.dp, ur.border, RoundedCornerShape(12.dp)) else Modifier)
            .clickable(role = Role.Button, onClickLabel = when (key) { "⌫" -> delete; "−" -> minus; "×" -> times; "÷" -> divide; else -> label }) {
                haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                if (action != null) action() else pad.press(key)
            }, contentAlignment = Alignment.Center) {
            if (key == "⌫") Icon(Icons.AutoMirrored.Outlined.Backspace, delete, tint = ink)
            else Text(label, color = ink, fontSize = if (kind == "done") 17.sp else 24.sp, fontWeight = if (kind == "digit") FontWeight.Normal else FontWeight.SemiBold)
        }
    }
    // A keypad keeps its keys where calculators have them, left to right, in Arabic too.
    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
        Column(modifier.fillMaxWidth().background(ur.surface2).padding(horizontal = 8.dp, vertical = 8.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) { Key("7"); Key("8"); Key("9"); Key("×", kind = "op"); Key("÷", kind = "op") }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) { Key("4"); Key("5"); Key("6"); Key("+", kind = "op"); Key("−", kind = "op") }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) { Key("1"); Key("2"); Key("3"); Key("=", 2.06f, kind = "op") }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Spacer(Modifier.weight(1f)); Key("0"); Key("⌫", kind = "op")
                Key("done", 2.06f, doneLabel, "done") { if (pad.isSum) pad.press("="); onDone() }
            }
        }
    }
}

/** The amount, large, as the keypad has it: the sum while it's being typed, else the money. */
@Composable
internal fun PadAmount(pad: PadState, currency: String, modifier: Modifier = Modifier, color: Color = ur.text, sign: String = "") {
    val text = when {
        pad.text.isBlank() -> money(0, currency)
        pad.isSum -> pad.display()
        else -> money(pad.cents ?: 0, currency)
    }
    Text(Money.ltr(if (sign.isNotEmpty() && !pad.isSum && (pad.cents ?: 0) != 0L) "$sign$text" else text), modifier, color = color,
        fontSize = if (pad.isSum) 30.sp else 40.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, maxLines = 1)
}
