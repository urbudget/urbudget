package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CreditCard
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.util.UUID

/**
 * Paying a credit card, as the desktop's payment form does: what's set aside in its payment
 * category, the whole balance, or another amount, from the cash account that paid it last.
 */
@Composable
internal fun CardPaymentSheet(cardId: String, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val card = remember(budget, cardId) { budget.row("accounts", cardId) }
    val balances = remember(summary) { summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty() }
    val category = remember(budget, cardId) { budget.rows("categories").firstOrNull { it.text("paymentAccountId") == cardId } }
    // The amounts are fixed when the sheet opens, so a retry pays exactly once.
    val owed = remember { maxOf(0L, -(balances[cardId] ?: 0)) }
    val aside = remember { maxOf(0L, category?.let { c -> summary?.rows("categories")?.firstOrNull { it.text("id") == c.text("id") }?.optLong("available") } ?: 0) }
    val paymentId = remember { UUID.randomUUID().toString() }
    val sources = remember(budget) { budget.rows("accounts").filter { it.text("kind") == "cash" && !it.optBoolean("archived") } }
    var from by remember {
        mutableStateOf(budget.rows("transactions").filter { t -> t.text("transferAccountId") == cardId && t.optLong("amount") < 0 && sources.any { it.text("id") == t.text("accountId") } }
            .maxByOrNull { it.text("date") }?.text("accountId")
            ?: sources.sortedWith(compareBy<JSONObject> { if (it.text("accountType") == "checking") 0 else if (it.text("accountType") == "savings" || it.text("name").contains("saving", true)) 2 else 1 }
                .thenByDescending { balances[it.text("id")] ?: 0 }).firstOrNull()?.text("id").orEmpty())
    }
    var choice by remember { mutableStateOf(if (aside > 0) "aside" else if (owed > 0) "balance" else "other") }
    val pad = rememberPad(null, cardId)
    var keypad by remember { mutableStateOf(choice == "other") }
    var date by remember { mutableStateOf(LocalDate.now().toString()) }
    var memo by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var dating by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    val amount = when (choice) { "aside" -> aside; "balance" -> owed; else -> pad.cents ?: 0 }
    if (card == null) { close(); return }
    val pay = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            require(from.isNotBlank()) { Words[R.string.loan_payment_account_required] }
            require(amount > 0) { Words[R.string.loan_payment_amount_required] }
            vm.saveTransaction(JSONObject().put("id", paymentId).put("accountId", from).put("date", date).put("payee", "").put("memo", memo)
                .put("amount", -amount).put("status", "uncleared").put("approved", true).put("flag", JSONObject.NULL)
                .put("transferAccountId", cardId).put("transferStatus", "uncleared")
                .put("splits", JSONArray().put(JSONObject().put("categoryId", JSONObject.NULL).put("unassigned", false).put("amount", -amount))))
            toast(Words[R.string.card_paid_toast, money(amount, currency), card.text("name")], true)
        }.onSuccess { close() }.onFailure { error = it.message }
    }
    UrSheet(close) {
        SheetHeader(stringResource(R.string.loan_pay_title, card.text("name")), close, null, null, cancelFirst = true)
        Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = Gutter).padding(bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            ListCard {
                @Composable
                fun Option(id: String, title: String, detail: String, figure: Long?) {
                    Row(Modifier.fillMaxWidth().clickable(role = Role.RadioButton) { choice = id; keypad = id == "other" }.padding(horizontal = 12.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(choice == id, { choice = id; keypad = id == "other" }, colors = RadioButtonDefaults.colors(selectedColor = ur.accent))
                        Column(Modifier.weight(1f).padding(start = 4.dp)) {
                            Text(title, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text)
                            Text(detail, style = MaterialTheme.typography.bodySmall, color = ur.muted)
                        }
                        figure?.let { Text(money(it, currency), style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = ur.text) }
                    }
                }
                if (category != null) { Option("aside", stringResource(R.string.card_set_aside_option), stringResource(R.string.card_set_aside_detail, category.text("name")), aside); Hairline(start = 52.dp) }
                Option("balance", stringResource(R.string.card_whole_balance), stringResource(R.string.card_whole_balance_detail), owed)
                Hairline(start = 52.dp)
                Option("other", stringResource(R.string.card_another_amount), if (choice == "other") (if (pad.isSum) Money.ltr(pad.display()) else money(pad.cents ?: 0, currency)) else stringResource(R.string.card_type_on_keypad), null)
            }
            ListCard {
                ListRow(stringResource(R.string.date_from), subtitle = sources.firstOrNull { it.text("id") == from }?.text("name") ?: stringResource(R.string.account_choose), onClick = { picking = true; keypad = false }, divider = true)
                ListRow(stringResource(R.string.common_date), subtitle = Money.date(date), onClick = { dating = true; keypad = false })
            }
            Field(memo, { memo = it }, stringResource(R.string.memo_optional))
            Text(stringResource(R.string.card_payment_help),
                color = ur.muted, style = MaterialTheme.typography.bodySmall)
            FormError(error)
            if (!keypad) PrimaryButton(stringResource(R.string.loan_pay_amount, money(amount, currency)), { pay() }, Modifier.fillMaxWidth(), enabled = amount > 0, icon = Icons.Outlined.CreditCard)
        }
        if (keypad) AmountKeypad(pad, { keypad = false })
    }
    if (dating) DatePickerSheet(date, { date = it; dating = false }) { dating = false }
    if (picking) UrSheet({ picking = false }) {
        SheetHeader(stringResource(R.string.card_pay_from), { picking = false }, null, null)
        Column(Modifier.padding(horizontal = Gutter).padding(bottom = 24.dp)) {
            ListCard {
                sources.forEachIndexed { index, account ->
                    val id = account.text("id")
                    ListRow(account.text("name"), value = money(balances[id] ?: 0, currency), icon = accountIcon(account), iconTint = ur.positive, iconFill = ur.positiveSoft,
                        onClick = { from = id; picking = false }, chevron = false, divider = index < sources.lastIndex)
                }
            }
        }
    }
}
