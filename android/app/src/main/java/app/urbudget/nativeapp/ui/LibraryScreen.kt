package app.urbudget.nativeapp.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.time.LocalDate

@Composable
internal fun LibraryScreen(budget: JSONObject?, vm: BudgetViewModel, importFile: () -> Unit, exportFile: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val syncStatus by NativeServices.syncStatus.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var drafts by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    var entries by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    var backups by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    var keep by remember { mutableStateOf("0") }
    var token by remember { mutableStateOf("") }
    var restoreToken by remember { mutableStateOf("") }
    var dialog by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf<JSONObject?>(null) }
    var localOnly by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var currency by remember { mutableStateOf(budget?.text("currency") ?: "SAR") }
    val currencies = remember(languageKey()) { currencyChoices(budget?.text("currency") ?: "SAR") }
    val defaultName = stringResource(R.string.welcome_default_name)
    var asOf by remember { mutableStateOf(LocalDate.now().toString()) }
    var exportPath by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableIntStateOf(0) }
    val exportBackup = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val path = exportPath
        if (uri != null && path != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) {
                val source = File(path).canonicalFile
                require(source.toPath().startsWith(context.filesDir.canonicalFile.toPath()) && source.isFile)
                context.contentResolver.openOutputStream(uri)?.use { output -> source.inputStream().use { it.copyTo(output) } } ?: error(context.getString(R.string.library_save_backup_failed))
            } }.onSuccess { notice = context.getString(R.string.library_backup_exported) }.onFailure { vm.notifyError(it.message ?: context.getString(R.string.library_export_backup_failed)) }
            exportPath = null
        }
    }
    LaunchedEffect(budget, revision, syncStatus) {
        runCatching {
            withContext(Dispatchers.IO) {
                val list = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries")
                val backupList = if (budget != null) NativeServices.call("list_backups") as JSONArray else JSONArray()
                val snapshot = NativeServices.call("load_budget") as JSONObject
                val retention = if (budget != null) NativeServices.call("backup_retention") else 0
                listOf(list, backupList, snapshot, retention)
            }
        }.onSuccess { values ->
            entries = (values[0] as JSONArray).objects(); backups = (values[1] as JSONArray).objects()
            token = (values[2] as JSONObject).getString("token"); keep = values[3].toString()
        }.onFailure { vm.notifyError(it.message ?: context.getString(R.string.library_load_failed)) }
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (drafts.isNotEmpty()) item {
            ListCard {
                drafts.forEach { draft ->
                    Text(draft.optJSONObject("draft")?.toString(2).orEmpty(), Modifier.padding(12.dp), style = MaterialTheme.typography.bodySmall)
                    TextButton({ vm.service("editor_discard", JSONObject().put("budgetId", draft.getString("budgetId")).put("id", draft.getString("editorId")).put("epoch", draft.getString("epoch"))) { drafts = drafts.filter { it !== draft } } }) {
                        Text(stringResource(R.string.library_discard_edits))
                    }
                }
            }
        }
        notice?.let { item { Notice(it, tone = "positive", icon = Icons.Outlined.CheckCircle) } }
        item { SectionTitle(stringResource(R.string.library_budgets), stringResource(R.string.library_budgets_detail)) }
        item {
            ListCard {
                entries.forEach { entry -> key(entry.text("id")) {
                    val current = entry.optBoolean("current")
                    val trashed = entry.text("lifecycle") in setOf("trashed", "purged")
                    val recovery = entry.text("lifecycle") == "recovery"
                    val deletionConflict = entry.text("status") == "deletion_conflict"
                    val remote = entry.text("availability") in setOf("remote", "evicted")
                    val missing = entry.optBoolean("missing") && !remote
                    val account = entry.text("scope") == "account"
                    var menu by remember { mutableStateOf(false) }
                    Box {
                        ListRow(entry.text("name"), subtitle = "${entry.text("currency")} · ${stringResource(if (deletionConflict) R.string.library_deletion_conflict else if (trashed) R.string.library_trash else if (entry.optBoolean("pending")) R.string.sync_state_waiting else if (remote) R.string.library_available_online else if (current) R.string.library_open_now else if (missing) R.string.library_file_missing else R.string.library_on_device)}",
                            icon = Icons.Outlined.AccountBalanceWallet, iconTint = if (current) ur.accent else ur.text2, iconFill = if (current) ur.accentSoft else ur.zeroSoft,
                            onClick = { if (current) dialog = "settings" else if (!state.busy && !missing && !trashed && !recovery) vm.service("library_open", JSONObject().put("id", entry.text("id"))) },
                            onLongClick = { menu = true }, chevron = !current,
                            trailing = if (current) ({ TagChip(stringResource(R.string.library_open_chip), fill = ur.accentSoft, color = ur.accent) }) else null, divider = true)
                        DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.common_open)) }, leadingIcon = { Icon(Icons.Outlined.FolderOpen, null) }, enabled = !state.busy && !missing && !trashed && !recovery,
                                onClick = { menu = false; vm.service("library_open", JSONObject().put("id", entry.text("id"))) })
                            if (!missing && !remote) DropdownMenuItem(text = { Text(stringResource(R.string.library_unfinished_edits)) }, onClick = {
                                menu = false; vm.service("editor_recovery", JSONObject().put("budgetId", entry.text("id"))) { drafts = (it as? JSONArray)?.objects().orEmpty() }
                            })
                            if (deletionConflict) DropdownMenuItem(text = { Text(stringResource(R.string.library_review_newer)) }, enabled = !state.busy, onClick = {
                                menu = false; vm.service("library_review_deletion", JSONObject().put("id", entry.text("id"))) { revision++ }
                            })
                            if (entry.text("lifecycle") == "trashed" && !deletionConflict) DropdownMenuItem(text = { Text(stringResource(R.string.common_restore)) }, onClick = {
                                menu = false; vm.service("library_restore", JSONObject().put("id", entry.text("id"))) { revision++ }
                            })
                            if (account && !trashed && !remote && !entry.optBoolean("pending")) DropdownMenuItem(text = { Text(stringResource(R.string.library_remove_download)) }, onClick = {
                                menu = false; vm.service("library_evict", JSONObject().put("id", entry.text("id"))) { revision++ }
                            })
                            if (entry.text("lifecycle") == "recovery" || (trashed && !missing && !remote)) DropdownMenuItem(text = { Text(stringResource(R.string.library_recover_copy)) }, onClick = {
                                menu = false; vm.service("library_recover", JSONObject().put("id", entry.text("id"))) { revision++ }
                            })
                            if (syncStatus?.optJSONObject("sync")?.optInt("protocol") == 2 && !account && !trashed) DropdownMenuItem(text = { Text(stringResource(R.string.library_add_account)) }, onClick = {
                                menu = false; vm.service("sync_enable", JSONObject().put("enabled", true).put("ids", JSONArray().put(entry.text("id")))) { revision++ }
                            })
                            if (!trashed && !recovery) DropdownMenuItem(text = { Text(stringResource(R.string.library_move_trash), color = ur.negative) }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = ur.negative) },
                                enabled = !state.busy, onClick = { menu = false; selected = entry; dialog = "delete" })
                        }
                    }
                } }
                ListRow(stringResource(R.string.library_new_budget), icon = Icons.Outlined.Add, titleColor = ur.accent, onClick = { if (!state.busy) { name = defaultName; currency = budget?.text("currency") ?: "SAR"; dialog = "new" } }, chevron = false)
            }
        }
        item { Text(stringResource(R.string.library_help), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall) }
        item { SectionTitle(stringResource(R.string.library_start_new)) }
        item {
            val freshName = stringResource(R.string.library_fresh_name, budget?.text("name").orEmpty())
            ListCard {
                ListRow(stringResource(R.string.library_fresh_start), subtitle = stringResource(R.string.library_fresh_start_detail),
                    icon = Icons.Outlined.AutoAwesome, onClick = { if (!state.busy && budget != null) { name = freshName; asOf = LocalDate.now().toString(); dialog = "fresh" } })
            }
        }
        item { SectionTitle(stringResource(R.string.library_backups), stringResource(R.string.library_backups_detail)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.library_create_backup), subtitle = stringResource(R.string.library_create_backup_detail), icon = Icons.Outlined.Backup,
                    onClick = { if (!state.busy && budget != null) vm.service("backup_database") { path -> exportPath = path as? String; revision++; if (exportPath != null) exportBackup.launch("UrBudget-${LocalDate.now()}.json") } }, divider = true)
                ListRow(stringResource(R.string.library_export_json), icon = Icons.Outlined.FileUpload, onClick = exportFile, divider = true)
                ListRow(stringResource(R.string.library_import_json), icon = Icons.Outlined.FileDownload, onClick = importFile)
            }
        }
        if (budget != null) item { Choice(stringResource(R.string.library_keep_backups), keep, listOf("0" to stringResource(R.string.library_keep_all)) + listOf("5", "10", "30").map { it to stringResource(R.string.library_keep_newest, it) }, { value -> keep = value; vm.service("set_backup_retention", JSONObject().put("keep", value.toInt())) }) }
        if (backups.isNotEmpty()) item {
            ListCard {
                backups.forEachIndexed { index, backup ->
                    ListRow(backup.text("name"), subtitle = stringResource(R.string.library_size_kib, "${backup.optLong("bytes") / 1024}"), icon = Icons.Outlined.History, iconTint = ur.text2, iconFill = ur.zeroSoft, chevron = false,
                        trailing = { TextButton({ selected = backup; restoreToken = token; dialog = "restore" }, enabled = !state.busy && token.isNotBlank()) { Text(stringResource(R.string.common_restore), fontWeight = FontWeight.SemiBold) } },
                        divider = index < backups.lastIndex)
                }
            }
        }
        item { Text(stringResource(R.string.library_settings_hint), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall) }
    }
    if (dialog == "settings" && budget != null) BudgetSettingsDialog(budget, vm) { dialog = "" }
    if (dialog == "new") FormDialog(stringResource(R.string.create_budget_action), { dialog = "" }, enabled = !state.busy && name.isNotBlank() && currency.isNotBlank(), onConfirm = {
        vm.createBudget(name, currency, localOnly = localOnly); dialog = ""
    }) {
        CheckRow(stringResource(R.string.library_keep_local), localOnly, { localOnly = it })
        Field(name, { name = it }, stringResource(R.string.budget_name))
        Choice(stringResource(R.string.budget_currency), currency, currencies, { currency = it })
    }
    if (dialog == "fresh") FormDialog(stringResource(R.string.library_fresh_start), { dialog = "" }, stringResource(R.string.library_fresh_create), enabled = !state.busy && name.isNotBlank(), onConfirm = {
        vm.service("fresh_start", JSONObject().put("name", name).put("asOf", asOf)) { id -> dialog = ""; revision++; vm.service("library_open", JSONObject().put("id", id)) }
    }) { Field(name, { name = it }, stringResource(R.string.library_fresh_field)); DateField(asOf, { asOf = it }, stringResource(R.string.library_start_date)); Text(stringResource(R.string.library_fresh_help)) }
    if (dialog == "delete" || dialog == "restore") AlertDialog(onDismissRequest = { dialog = "" },
        title = { Text(if (dialog == "delete") stringResource(R.string.library_delete_title, selected?.text("name").orEmpty()) else stringResource(R.string.library_restore_title)) },
        text = { Text(if (dialog == "delete") stringResource(R.string.library_trash_message) else stringResource(R.string.library_restore_message, selected?.text("name").orEmpty())) }, confirmButton = { TextButton({
        if (dialog == "delete") vm.service("library_trash", JSONObject().put("id", selected!!.text("id"))) { dialog = ""; revision++ }
        else vm.service("restore_backup_file", JSONObject().put("path", selected!!.text("path")).put("expectedToken", restoreToken)) { dialog = ""; revision++ }
    }, enabled = !state.busy) { Text(if (dialog == "delete") stringResource(R.string.library_move_trash) else stringResource(R.string.common_restore)) } }, dismissButton = { TextButton({ dialog = "" }) { Text(stringResource(R.string.common_cancel)) } })
}

private fun JSONArray.objects() = (0 until length()).mapNotNull(::optJSONObject)
