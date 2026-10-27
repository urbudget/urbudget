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
internal fun LibraryScreen(budget: JSONObject, vm: BudgetViewModel, importFile: () -> Unit, exportFile: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var entries by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    var backups by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    var keep by remember { mutableStateOf("0") }
    var token by remember { mutableStateOf("") }
    var restoreToken by remember { mutableStateOf("") }
    var dialog by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf<JSONObject?>(null) }
    var name by remember { mutableStateOf("") }
    var currency by remember { mutableStateOf(budget.text("currency")) }
    val currencies = remember(languageKey()) { currencyChoices(budget.text("currency")) }
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
    LaunchedEffect(budget, revision) {
        runCatching {
            withContext(Dispatchers.IO) {
                val list = NativeServices.call("list_budgets") as JSONArray
                val backupList = NativeServices.call("list_backups") as JSONArray
                val snapshot = NativeServices.call("load_budget") as JSONObject
                val retention = NativeServices.call("backup_retention")
                listOf(list, backupList, snapshot, retention)
            }
        }.onSuccess { values ->
            entries = (values[0] as JSONArray).objects(); backups = (values[1] as JSONArray).objects()
            token = (values[2] as JSONObject).getString("token"); keep = values[3].toString()
        }.onFailure { vm.notifyError(it.message ?: context.getString(R.string.library_load_failed)) }
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        notice?.let { item { Notice(it, tone = "positive", icon = Icons.Outlined.CheckCircle) } }
        item { SectionTitle(stringResource(R.string.library_budgets), stringResource(R.string.library_budgets_detail)) }
        item {
            ListCard {
                entries.forEach { entry -> key(entry.text("id")) {
                    val current = entry.optBoolean("current")
                    val missing = entry.optBoolean("missing")
                    var menu by remember { mutableStateOf(false) }
                    Box {
                        ListRow(entry.text("name"), subtitle = "${entry.text("currency")} · ${stringResource(if (current) R.string.library_open_now else if (missing) R.string.library_file_missing else R.string.library_on_device)}",
                            icon = Icons.Outlined.AccountBalanceWallet, iconTint = if (current) ur.accent else ur.text2, iconFill = if (current) ur.accentSoft else ur.zeroSoft,
                            onClick = { if (current) dialog = "settings" else if (!state.busy && !missing) vm.service("open_budget", JSONObject().put("id", entry.text("id"))) },
                            onLongClick = if (current) null else ({ menu = true }), chevron = !current,
                            trailing = if (current) ({ TagChip(stringResource(R.string.library_open_chip), fill = ur.accentSoft, color = ur.accent) }) else null, divider = true)
                        DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.common_open)) }, leadingIcon = { Icon(Icons.Outlined.FolderOpen, null) }, enabled = !state.busy && !missing,
                                onClick = { menu = false; vm.service("open_budget", JSONObject().put("id", entry.text("id"))) })
                            DropdownMenuItem(text = { Text(stringResource(R.string.library_delete_local), color = ur.negative) }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = ur.negative) },
                                enabled = !state.busy, onClick = { menu = false; selected = entry; dialog = "delete" })
                        }
                    }
                } }
                ListRow(stringResource(R.string.library_new_budget), icon = Icons.Outlined.Add, titleColor = ur.accent, onClick = { if (!state.busy) { name = defaultName; currency = budget.text("currency"); dialog = "new" } }, chevron = false)
            }
        }
        item { Text(stringResource(R.string.library_help), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall) }
        item { SectionTitle(stringResource(R.string.library_start_new)) }
        item {
            val freshName = stringResource(R.string.library_fresh_name, budget.text("name"))
            ListCard {
                ListRow(stringResource(R.string.library_fresh_start), subtitle = stringResource(R.string.library_fresh_start_detail),
                    icon = Icons.Outlined.AutoAwesome, onClick = { if (!state.busy) { name = freshName; asOf = LocalDate.now().toString(); dialog = "fresh" } })
            }
        }
        item { SectionTitle(stringResource(R.string.library_backups), stringResource(R.string.library_backups_detail)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.library_create_backup), subtitle = stringResource(R.string.library_create_backup_detail), icon = Icons.Outlined.Backup,
                    onClick = { if (!state.busy) vm.service("backup_database") { path -> exportPath = path as? String; revision++; if (exportPath != null) exportBackup.launch("UrBudget-${LocalDate.now()}.json") } }, divider = true)
                ListRow(stringResource(R.string.library_export_json), icon = Icons.Outlined.FileUpload, onClick = exportFile, divider = true)
                ListRow(stringResource(R.string.library_import_json), icon = Icons.Outlined.FileDownload, onClick = importFile)
            }
        }
        item { Choice(stringResource(R.string.library_keep_backups), keep, listOf("0" to stringResource(R.string.library_keep_all)) + listOf("5", "10", "30").map { it to stringResource(R.string.library_keep_newest, it) }, { value -> keep = value; vm.service("set_backup_retention", JSONObject().put("keep", value.toInt())) }) }
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
    if (dialog == "settings") BudgetSettingsDialog(budget, vm) { dialog = "" }
    if (dialog == "new") FormDialog(stringResource(R.string.create_budget_action), { dialog = "" }, enabled = !state.busy && name.isNotBlank() && currency.isNotBlank(), onConfirm = {
        vm.createBudget(name, currency); dialog = ""
    }) {
        Field(name, { name = it }, stringResource(R.string.budget_name))
        Choice(stringResource(R.string.budget_currency), currency, currencies, { currency = it })
    }
    if (dialog == "fresh") FormDialog(stringResource(R.string.library_fresh_start), { dialog = "" }, stringResource(R.string.library_fresh_create), enabled = !state.busy && name.isNotBlank(), onConfirm = {
        vm.service("fresh_start", JSONObject().put("name", name).put("asOf", asOf)) { id -> dialog = ""; revision++; vm.service("open_budget", JSONObject().put("id", id)) }
    }) { Field(name, { name = it }, stringResource(R.string.library_fresh_field)); DateField(asOf, { asOf = it }, stringResource(R.string.library_start_date)); Text(stringResource(R.string.library_fresh_help)) }
    if (dialog == "delete" || dialog == "restore") AlertDialog(onDismissRequest = { dialog = "" },
        title = { Text(if (dialog == "delete") stringResource(R.string.library_delete_title, selected?.text("name").orEmpty()) else stringResource(R.string.library_restore_title)) },
        text = { Text(if (dialog == "delete") stringResource(R.string.library_delete_message) else stringResource(R.string.library_restore_message, selected?.text("name").orEmpty())) }, confirmButton = { TextButton({
        if (dialog == "delete") vm.service("delete_budget", JSONObject().put("id", selected!!.text("id"))) { dialog = ""; revision++ }
        else vm.service("restore_backup_file", JSONObject().put("path", selected!!.text("path")).put("expectedToken", restoreToken)) { dialog = ""; revision++ }
    }, enabled = !state.busy) { Text(if (dialog == "delete") stringResource(R.string.library_delete_local) else stringResource(R.string.common_restore)) } }, dismissButton = { TextButton({ dialog = "" }) { Text(stringResource(R.string.common_cancel)) } })
}

private fun JSONArray.objects() = (0 until length()).mapNotNull(::optJSONObject)
