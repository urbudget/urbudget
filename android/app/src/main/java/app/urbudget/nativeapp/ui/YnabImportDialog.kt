package app.urbudget.nativeapp.ui

import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.encodeFileBytes
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

@Composable
internal fun YnabImportDialog(budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    var token by remember { mutableStateOf("") }
    var saved by remember { mutableStateOf("") }
    var tokens by remember { mutableStateOf(emptyList<JSONObject>()) }
    var plans by remember { mutableStateOf(emptyList<JSONObject>()) }
    var plan by remember { mutableStateOf("") }
    var rememberToken by remember { mutableStateOf(false) }
    var replace by remember { mutableStateOf(false) }
    var mode by remember { mutableStateOf("api") }
    var result by remember { mutableStateOf<JSONObject?>(null) }
    val choices = remember { mutableStateMapOf<String, String>() }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var filePreview by remember { mutableStateOf<ScopedResult<JSONObject>?>(null) }
    var dateOrder by remember { mutableStateOf("") }
    var selectedFiles by remember { mutableStateOf<JSONArray?>(null) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val requests = rememberPreviewJobs()
    val apiContext = remember(mode, token, saved) { Any() }
    val currentApiContext by rememberUpdatedState(apiContext)
    val apiRequests = rememberPreviewJobs(apiContext)
    val fileContext = remember(budget, selectedFiles, dateOrder, mode) { Any() }
    val currentFileContext by rememberUpdatedState(fileContext)
    val fileRequests = rememberPreviewJobs(fileContext)
    LaunchedEffect(apiContext, fileContext) { loading = false }
    fun loadTokens() { requests.track(vm.service("ynab_tokens") { value -> val rows = value as? JSONArray; tokens = if (rows == null) emptyList() else (0 until rows.length()).mapNotNull(rows::optJSONObject) }) }
    fun authentication() = JSONObject().put("token", token).also { if (saved.isNotBlank()) it.put("savedToken", saved) }
    fun call(method: String, params: JSONObject) {
        loading = true; error = null
        apiRequests.track(vm.service(method, params, onError = { if (currentApiContext === apiContext) { error = it; loading = false } }) { value ->
            if (currentApiContext !== apiContext) return@service
            loading = false; result = value as? JSONObject; choices.clear()
            if (result?.text("reviewId").isNullOrBlank()) { token = ""; loadTokens() }
        })
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) scope.launch {
            loading = true; error = null
            runCatching {
                withContext(Dispatchers.IO) {
                    require(uris.size <= 8) { context.getString(R.string.ynab_too_many_files) }
                    var total = 0
                    JSONArray(uris.map { uri ->
                        val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else "export.zip" } ?: "export.zip"
                        val bytes = context.contentResolver.openInputStream(uri)?.use { stream -> val out = java.io.ByteArrayOutputStream(); val block = ByteArray(8192); while (out.size() <= 8 * 1024 * 1024) { val count = stream.read(block); if (count < 0) break; out.write(block, 0, count) }; out.toByteArray() } ?: error(context.getString(R.string.ynab_file_unreadable))
                        require(bytes.size <= 8 * 1024 * 1024) { context.getString(R.string.ynab_file_too_large) }; total += bytes.size; require(total <= 16 * 1024 * 1024) { context.getString(R.string.ynab_files_too_large) }
                        JSONObject().put("name", name).put("bytes", encodeFileBytes(bytes))
                    })
                }
            }.onSuccess { selectedFiles = it; filePreview = null }.onFailure { if (it is CancellationException) throw it; error = it.message }
            loading = false
        }
    }
    LaunchedEffect(Unit) { loadTokens() }
    val conflicts = remember(result) { result?.rows("conflicts").orEmpty() }
    val differences = remember(result) { result?.rows("differences").orEmpty() }
    val pasteToken = stringResource(R.string.ynab_paste_token)
    val tokenChoices = remember(tokens, pasteToken) { listOf("" to pasteToken) + tokens.map { it.text("id") to "${Money.own(it.text("label"))} · ${Money.own(it.text("hint"))}" } }
    val planChoices = remember(plans) { plans.map { it.text("id") to it.text("name") } }
    val fileNames = remember(selectedFiles) { selectedFiles?.let { files -> (0 until files.length()).joinToString("\n") { files.getJSONObject(it).text("name") } } }
    val candidate = filePreview?.forScope(fileContext)
    val fileDifferences = remember(candidate) { candidate?.optJSONObject("importAudit")?.rows("categoryDifferences").orEmpty() }
    val allConflictsChosen by remember(conflicts) { derivedStateOf { conflicts.all { choices[it.text("path")] in listOf("local", "remote") } } }
    val reviewId = result?.text("reviewId").orEmpty()
    LazyFormDialog(stringResource(R.string.import_ynab), close, onConfirm = null) {
        item { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Choice(stringResource(R.string.ynab_method), mode, listOf("api" to stringResource(R.string.ynab_method_account), "files" to stringResource(R.string.ynab_method_files)), { mode = it; error = null })
        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
        FormError(error)
        } }
        if (mode == "api") {
            item { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Choice(stringResource(R.string.ynab_credential), saved, tokenChoices, { saved = it; plans = emptyList(); plan = "" })
            if (saved.isBlank()) {
                FilledField(token, { token = it }, Modifier.fillMaxWidth(), label = { Text(stringResource(R.string.ynab_access_token)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
                Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(rememberToken, { rememberToken = it }); Text(stringResource(R.string.ynab_remember_token)) }
            } else TextButton({ requests.track(vm.service("forget_ynab_token", JSONObject().put("id", saved)) { saved = ""; loadTokens() }) }) { Text(stringResource(R.string.ynab_forget_token)) }
            OutlinedButton({ loading = true; error = null; apiRequests.track(vm.service("ynab_plans", authentication(), onError = { if (currentApiContext === apiContext) { loading = false; error = it } }) { value -> if (currentApiContext === apiContext) { val list = value as? JSONArray; plans = if (list == null) emptyList() else (0 until list.length()).mapNotNull(list::optJSONObject); plan = plans.firstOrNull()?.text("id").orEmpty(); loading = false } }) }, enabled = !loading && (token.isNotBlank() || saved.isNotBlank())) { Text(stringResource(R.string.ynab_load_plans)) }
            if (plans.isNotEmpty()) {
                Choice(stringResource(R.string.ynab_plan), plan, planChoices, { plan = it })
                Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(replace, { replace = it }); Text(stringResource(R.string.ynab_replace)) }
                Text(stringResource(R.string.ynab_backup_note), style = MaterialTheme.typography.bodySmall)
                Button({ call("sync_ynab", authentication().put("planId", plan).put("replace", replace).put("rememberToken", rememberToken).put("tokenLabel", plans.firstOrNull { it.text("id") == plan }?.text("name"))) }, enabled = !loading && plan.isNotBlank()) { Text(stringResource(if (replace) R.string.ynab_import_replace else R.string.ynab_import_changes)) }
            }
            } }
            if (reviewId.isNotBlank() && differences.isNotEmpty()) {
                item { Text(stringResource(R.string.ynab_review_differences), style = MaterialTheme.typography.titleMedium) }
                items(differences) { DifferenceRow(it, budget.text("currency")) }
                item { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) { Text(stringResource(R.string.ynab_differences_note))
                Button({ call("accept_ynab_import", JSONObject().put("reviewId", reviewId)) }, enabled = !loading) { Text(stringResource(R.string.ynab_accept_differences)) }
                } }
            }
            if (reviewId.isNotBlank() && conflicts.isNotEmpty()) {
                item { Text(stringResource(R.string.ynab_choose_each), style = MaterialTheme.typography.titleMedium) }
                items(conflicts) { conflict ->
                    val deleted = stringResource(R.string.conflict_deleted)
                    OutlinedCard { Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(conflict.text("about").ifBlank { conflict.text("path") }); Text(conflict.text("field"), style = MaterialTheme.typography.labelMedium)
                        Text(stringResource(R.string.ynab_local_value, conflict.text("localText").ifBlank { conflict.opt("local")?.toString() ?: deleted }))
                        Text(stringResource(R.string.ynab_remote_value, conflict.text("remoteText").ifBlank { conflict.opt("remote")?.toString() ?: deleted }))
                        Choice(stringResource(R.string.conflict_keep), choices[conflict.text("path")] ?: "", listOf("" to stringResource(R.string.ynab_choose_version), "local" to stringResource(R.string.ynab_local), "remote" to "YNAB"), { choices[conflict.text("path")] = it })
                    } }
                }
                item { Button({ call("resolve_ynab", JSONObject().put("reviewId", reviewId).put("choices", JSONObject(choices.toMap()))) }, enabled = !loading && allConflictsChosen) { Text(stringResource(R.string.ynab_apply_versions)) } }
            }
            if (result != null && reviewId.isBlank()) item { Text(stringResource(if (result?.optBoolean("unchanged") == true) R.string.ynab_up_to_date else R.string.ynab_saved), color = MaterialTheme.colorScheme.primary) }
        } else {
            item { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Text(stringResource(R.string.ynab_files_help))
            OutlinedButton({ picker.launch(arrayOf("*/*")) }, enabled = !loading) { Text(stringResource(R.string.ynab_choose_files)) }
            fileNames?.let { Text(it) }
            Choice(stringResource(R.string.ynab_date_order), dateOrder, listOf("" to stringResource(R.string.ynab_detect), "mdy" to stringResource(R.string.ynab_mdy), "dmy" to stringResource(R.string.ynab_dmy), "ymd" to stringResource(R.string.ynab_ymd)), { dateOrder = it; filePreview = null })
            Button({ loading = true; error = null; fileRequests.track(vm.query("importYnabFiles", JSONObject().put("files", selectedFiles).put("dateOrder", dateOrder.ifBlank { null } ?: JSONObject.NULL).put("currency", budget.text("currency")), onError = { if (currentFileContext === fileContext) { loading = false; error = it } }) { if (currentFileContext === fileContext) { filePreview = ScopedResult(fileContext, it); loading = false } }) }, enabled = !loading && selectedFiles != null) { Text(stringResource(R.string.ynab_preview)) }
            } }
            candidate?.let { candidate ->
                item {
                    val accounts = countText(R.plurals.filter_accounts, candidate.optJSONArray("accounts")?.length() ?: 0)
                    val transactions = countText(R.plurals.report_transactions_count, candidate.optJSONArray("transactions")?.length() ?: 0)
                    Text("${Money.own(candidate.text("name"))} · ${candidate.text("currency")}\n$accounts · $transactions", style = MaterialTheme.typography.titleMedium)
                }
                items(fileDifferences) { DifferenceRow(it, candidate.text("currency")) }
                item { Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                    Text(stringResource(R.string.ynab_replace_note))
                    Button({
                        loading = true; error = null
                        fileRequests.track(scope.launch {
                            try {
                                val encoded = withContext(Dispatchers.Default) { candidate.toString() }
                                require(currentFileContext === fileContext && vm.state.value.budget === budget) { Words[R.string.ynab_budget_changed] }
                                vm.importBudget(encoded); close()
                            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message; loading = false }
                        })
                    }, enabled = !loading) { Text(stringResource(R.string.ynab_accept_replace)) }
                } }
            }
        }
    }
}

@Composable
private fun DifferenceRow(difference: JSONObject, currency: String) {
    OutlinedCard { Column(Modifier.fillMaxWidth().padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(listOf(difference.text("month"), difference.text("category"), difference.text("account"), difference.text("field")).filter { it.isNotBlank() }.joinToString(" · ") { Money.own(it) })
        if (difference.has("expected") && difference.has("actual")) Text(stringResource(R.string.ynab_difference_values, money(difference.optLong("expected"), currency), money(difference.optLong("actual"), currency)))
        else Text(remember(difference) { difference.toString(2) })
    } }
}
