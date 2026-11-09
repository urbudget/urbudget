package app.urbudget.nativeapp.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import org.json.JSONObject

/**
 * The cards and accounts a review's texts name that the budget doesn't have, set up in one place
 * above the rows: each made as an account, with the name, type and balance before the texts that
 * they suggest; put in another (a debit card in its current account); or not tracked. [apply]
 * makes them in one change and remembers each card, and their rows move to Ready to add; it's
 * null when the page's own button does it.
 */
internal fun LazyListScope.smsSetupSection(candidates: List<SmsSetupCandidate>, plans: Map<String, SmsSetupPlan>, budget: JSONObject, accounts: List<Pair<String, String>>,
                                           typeNames: Map<String, String>, enabled: Boolean, adds: (SmsItem) -> Boolean, answer: (String, SmsSetupPlan) -> Unit,
                                           edit: (String) -> Unit, apply: (() -> Unit)?) {
    item(key = "setup") {
        if (accounts.isEmpty()) SectionTitle(stringResource(R.string.sms_setup_title), stringResource(R.string.sms_setup_detail))
        else SectionTitle(stringResource(R.string.sms_setup_title_more), stringResource(R.string.sms_setup_more_detail))
    }
    candidates.forEach { c ->
        item(key = "setup:${c.key}") {
            val plan = plans.getValue(c.key)
            val opening = smsOpening(smsSetupMembers(c.key, candidates, plans), adds, smsPlanLimit(plan))
            SmsSetupRow(c, plan, opening, candidates, plans, budget, accounts, typeNames[plan.type].orEmpty(), enabled, { answer(c.key, it) }) { edit(c.key) }
        }
    }
    if (apply != null) item(key = "setup-apply") {
        PrimaryButton(smsSetupAction(plans), apply, Modifier.fillMaxWidth().testTag("sms-setup-apply"), enabled = enabled)
    }
}

/** "Set up 3 accounts", or, with none to make, applying the choices. */
@Composable
internal fun smsSetupAction(plans: Map<String, SmsSetupPlan>): String {
    val making = plans.values.count { it.action == SmsSetupPlan.Create }
    return if (making > 0) countText(R.plurals.sms_setup_apply, making) else stringResource(R.string.sms_setup_apply_none)
}

/** The first day of a candidate's texts: its account's balance is set as it was before them. */
internal fun smsSetupStart(members: List<SmsSetupCandidate>): String =
    members.flatMap { it.items + it.via }.mapNotNull { it.transaction?.text("date")?.ifBlank { null } }.minOrNull().orEmpty()

/** One card or account: what its texts are, and what to do with it. */
@Composable
private fun SmsSetupRow(c: SmsSetupCandidate, plan: SmsSetupPlan, opening: SmsOpening?, candidates: List<SmsSetupCandidate>, plans: Map<String, SmsSetupPlan>, budget: JSONObject,
                        accounts: List<Pair<String, String>>, typeName: String, enabled: Boolean, choose: (SmsSetupPlan) -> Unit, edit: () -> Unit) {
    val currency = budget.text("currency")
    val tag = c.ref?.let(::smsRefKey) ?: SmsAnyCard
    ListCard(Modifier.testTag("sms-setup:$tag")) { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(iconForType(plan.type, AccountTypes.kindFor(plan.type)), null, Modifier.size(20.dp), tint = ur.accent)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(if (c.refs.size > 1) countText(R.plurals.sms_setup_numbers, c.refs.size) else c.ref?.let { smsRefLabel(it) } ?: stringResource(R.string.sms_setup_no_card),
                    style = MaterialTheme.typography.titleMedium, color = ur.text)
                Text(Money.own(c.sender) + " · " + countText(R.plurals.sms_transaction_count, c.texts), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        val payees = c.items.mapNotNull { it.transaction?.text("payee")?.ifBlank { null } }.distinct().take(4)
        if (payees.isNotEmpty()) Text(payees.joinToString(", ") { Money.own(it) }, style = MaterialTheme.typography.bodyMedium, color = ur.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
        // Made here, part of an open account or of another being made, or not tracked.
        val made = candidates.filter { it.key != c.key && plans[it.key]?.action == SmsSetupPlan.Create }
        val choices = listOf(SmsSetupPlan.Create to stringResource(R.string.sms_setup_create)) +
            accounts.map { (id, name) -> "join:$id" to stringResource(R.string.sms_setup_part_of, Money.own(name)) } +
            made.map { "join:${it.key}" to stringResource(R.string.sms_setup_part_of, stringResource(R.string.sms_setup_new, Money.own(plans.getValue(it.key).name))) } +
            (SmsSetupPlan.Skip to stringResource(R.string.sms_setup_skip))
        val selected = if (plan.action == SmsSetupPlan.Join) "join:${plan.joins}" else plan.action
        Choice(stringResource(R.string.sms_setup_action), selected, choices) { id ->
            choose(when {
                id.startsWith("join:") -> plan.copy(action = SmsSetupPlan.Join, joins = id.removePrefix("join:"))
                else -> plan.copy(action = id, joins = null)
            })
        }
        when (plan.action) {
            SmsSetupPlan.Create -> Row(Modifier.fillMaxWidth().clickable(enabled, role = Role.Button, onClick = edit).testTag("sms-setup-edit:$tag"), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(Money.own(plan.name) + " · " + typeName, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold, color = ur.text,
                        maxLines = 2, overflow = TextOverflow.Ellipsis)
                    val start = smsSetupStart(smsSetupMembers(c.key, candidates, plans))
                    Text(stringResource(R.string.sms_setup_opening, Money.date(start), money(smsSetupBalance(plan, opening), currency)),
                        Modifier.testTag("sms-setup-opening:$tag"), style = MaterialTheme.typography.bodySmall.merge(Tabular), color = ur.text2)
                    val how = when {
                        plan.opening != null -> null
                        AccountTypes.isLiability(plan.type) -> stringResource(R.string.sms_setup_owed_help)
                        opening != null -> stringResource(R.string.sms_setup_opening_from, Money.date(opening.date), money(opening.given, currency))
                        else -> stringResource(R.string.sms_setup_opening_none)
                    }
                    how?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
                }
                Text(stringResource(R.string.sms_setup_change), Modifier.padding(start = 10.dp), color = ur.accent, fontWeight = FontWeight.SemiBold)
            }
            SmsSetupPlan.Join -> {
                val into = accounts.firstOrNull { it.first == plan.joins }?.second ?: plans[plan.joins]?.name.orEmpty()
                Text(stringResource(R.string.sms_setup_joins, Money.own(into)), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
            else -> Text(stringResource(if (c.items.isEmpty()) R.string.sms_setup_untracked else R.string.sms_setup_skips), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    } }
}

/** An account to make, as the user wants it: its name, type, and balance before the texts. */
@Composable
internal fun SmsSetupDialog(plan: SmsSetupPlan, shown: Long, currency: String, close: () -> Unit, save: (SmsSetupPlan) -> Unit) {
    var name by remember { mutableStateOf(plan.name) }
    var type by remember { mutableStateOf(plan.type) }
    val initial = remember { amountText(shown) }
    var balance by remember { mutableStateOf(initial) }
    var limit by remember { mutableStateOf(plan.limit?.let(::amountText) ?: "") }
    var error by remember { mutableStateOf<String?>(null) }
    val types = remember(languageKey()) { accountTypes() }
    val credit = AccountTypes.kindFor(type) == "credit"
    FormDialog(stringResource(R.string.sms_setup_edit_title), close, enabled = name.isNotBlank(), onConfirm = {
        runCatching {
            val creditLimit = if (credit && limit.isNotBlank()) amountValue(limit).also { require(it > 0) { Words[R.string.account_credit_limit_positive] } } else null
            // A balance left as shown keeps following the texts.
            save(plan.copy(action = SmsSetupPlan.Create, joins = null, name = name.trim(), type = type, opening = if (balance == initial) plan.opening else amountValue(balance), limit = creditLimit))
            close()
        }.onFailure { error = it.message }
    }) {
        Field(name, { name = it }, stringResource(R.string.account_name))
        Choice(stringResource(R.string.account_type), type, types) { type = it }
        Field(balance, { balance = it }, stringResource(R.string.sms_new_account_balance, currency), numeric = true)
        Text(stringResource(if (AccountTypes.isLiability(type)) R.string.account_owed_negative else R.string.sms_new_account_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (credit) {
            Field(limit, { limit = it }, stringResource(R.string.account_credit_limit, currency), numeric = true)
            Text(stringResource(R.string.sms_setup_limit_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
        FormError(error)
    }
}
