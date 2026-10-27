package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.SwapHoriz
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BankReviewRow
import app.urbudget.nativeapp.BankReviewSummary
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words

/** "12 rows · 3 already imported · 2 renamed by your payee rules" (BankImport.tsx's summary). */
internal fun reviewCounts(summary: BankReviewSummary) = listOfNotNull(
    Words.count(R.plurals.bank_rows, summary.rows.toLong()),
    if (summary.duplicates > 0) Words.count(R.plurals.bank_already_imported_count, summary.duplicates.toLong()) else null,
    if (summary.renamed > 0) Words.count(R.plurals.bank_renamed_count, summary.renamed.toLong()) else null,
    if (summary.refunds > 0) Words.count(R.plurals.bank_refunds_count, summary.refunds.toLong()) else null,
).joinToString(" · ")

/** What the review says before its rows: how matching works, files without bank IDs, and rows still to decide. */
internal fun LazyListScope.bankReviewNotes(summary: BankReviewSummary, undecided: Int) {
    item(key = "review:matching") {
        Text(stringResource(R.string.bank_matching_note),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
    if (summary.transfers) item(key = "review:transfers") {
        Text(stringResource(R.string.bank_transfers_note),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
    if (summary.idless) item(key = "review:idless") {
        Notice(stringResource(R.string.bank_idless_note),
            tone = "info", icon = Icons.Outlined.Info)
    }
    if (undecided > 0) item(key = "review:undecided") {
        Notice(countText(R.plurals.bank_rows_need_choice, undecided), icon = Icons.Outlined.WarningAmber)
    }
}

/** Transfers that are only suggested (the bank text doesn't back them up) and still to decide, with a way to pair them all. */
internal fun LazyListScope.suggestedTransfers(rows: List<BankReviewRow>, decisions: MutableMap<Int, String>, enabled: Boolean) {
    val suggested = rows.indices.filter { rows[it].transfer != null && rows[it].decision(decisions[it]) == "review" }
    if (suggested.isNotEmpty()) item(key = "review:suggested") {
        Notice(countText(R.plurals.bank_transfers_suggested, suggested.size),
            tone = "info", action = stringResource(if (suggested.size == 1) R.string.bank_pair_it else R.string.bank_pair_all),
            onAction = { if (enabled) suggested.forEach { decisions[it] = "match:${rows[it].transfer}" } }, icon = Icons.Outlined.SwapHoriz)
    }
}

/** Each row of a bank review, a card of its own: what the bank says, and what to do with it (add, match, skip). */
internal fun LazyListScope.bankReviewRows(rows: List<BankReviewRow>, decisions: Map<Int, String>, enabled: Boolean, choose: (Int, String) -> Unit) {
    itemsIndexed(rows, key = { _, row -> "review:${row.key}" }, contentType = { _, _ -> "review-row" }) { index, row ->
        BankReviewItem(row, row.decision(decisions[index]), enabled, Modifier.cardSegment(0, 1, cardFill()), false) { choose(index, it) }
    }
}

@Composable
private fun BankReviewItem(row: BankReviewRow, decision: String, enabled: Boolean, modifier: Modifier, divider: Boolean, choose: (String) -> Unit) {
    // Its choice stands out from the card it sits on, as a field in a ListCard does.
    CompositionLocalProvider(LocalFieldFill provides if (LocalSheet.current) ur.surface else ur.surface2) { Column(modifier) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(row.payee, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold,
                    color = if (row.locked) ur.muted else ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(row.amountText, Modifier.padding(start = 10.dp), style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold,
                    color = if (row.locked) ur.muted else if (row.amount > 0) ur.positive else ur.text, maxLines = 1)
            }
            row.bankText?.let { Text(stringResource(R.string.bank_row_bank, it), style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            // Imported, a refund takes its purchase's category when it has none.
            row.refund?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.accent, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis) }
            if (row.detail.isNotEmpty()) Text(row.detail, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (row.locked) Text(stringResource(R.string.bank_already_imported_skipped), style = MaterialTheme.typography.bodySmall, color = ur.muted, fontWeight = FontWeight.SemiBold)
            else {
                if (decision == "review") Text(stringResource(if (row.possibleDuplicate) R.string.bank_may_be_imported else R.string.bank_choose_matching),
                    style = MaterialTheme.typography.bodySmall, color = ur.warning, fontWeight = FontWeight.SemiBold)
                Box(Modifier.padding(top = 4.dp)) { Choice(stringResource(R.string.bank_what_to_do), decision, row.options) { if (enabled) choose(it) } }
            }
        }
        if (divider) Hairline(start = 16.dp)
    } }
}
