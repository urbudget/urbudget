package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.LifecycleResumeEffect
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.AssistantState
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import org.json.JSONObject
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map

@Composable
internal fun AssistantSettingsDialog(controller: AssistantController, close: () -> Unit) {
    LifecycleResumeEffect(controller) {
        controller.refreshSignInStatus()
        onPauseOrDispose { }
    }
    val settingsState = remember(controller) { controller.state.map { it.copy(streaming = "", thinking = "", thread = null, entries = emptyList()) }.distinctUntilChanged() }
    val state by settingsState.collectAsStateWithLifecycle(AssistantState())
    val status = state.status
    var key by remember { mutableStateOf("") }
    var deleteHistory by remember { mutableStateOf(false) }
    val provider = status?.text("provider") ?: "chatgpt"
    val ready = status?.optBoolean("ready") == true
    val providerDefault = stringResource(R.string.assistant_model_default)
    val modelChoices = remember(state.models, providerDefault) { listOf("" to providerDefault) + state.models.map { it.text("id") to it.text("name", it.text("id")) } }
    val signingIn = "signIn" in state.busy || status?.optBoolean("signingIn") == true
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxWidth().fillMaxHeight(.94f).padding(12.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(stringResource(R.string.assistant_settings), style = MaterialTheme.typography.headlineSmall)
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(stringResource(R.string.assistant_settings_intro))
                    (status?.optString("signInError")?.takeUnless { it.isEmpty() || it == "null" } ?: state.error)
                        ?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    Choice(stringResource(R.string.assistant_provider), provider, listOf("chatgpt" to stringResource(R.string.assistant_provider_chatgpt), "openai" to stringResource(R.string.assistant_provider_openai), "anthropic" to stringResource(R.string.assistant_provider_anthropic))) {
                        if (!signingIn) { key = ""; controller.settings(JSONObject().put("provider", it)) }
                    }
                    if (provider == "chatgpt") {
                        val email = status?.optJSONObject("account")?.text("email").orEmpty()
                        Text(if (!ready) stringResource(R.string.assistant_sign_in_browser) else if (email.isEmpty()) stringResource(R.string.assistant_signed_in) else stringResource(R.string.assistant_signed_in_as, Money.own(email)))
                        if (signingIn) {
                            Text(stringResource(when (status?.optString("signInPhase")) {
                                "returning" -> R.string.assistant_sign_in_returning
                                "exchanging", "validating" -> R.string.assistant_sign_in_finishing
                                "saving" -> R.string.assistant_sign_in_saving
                                else -> R.string.assistant_sign_in_waiting
                            }))
                            LinearProgressIndicator(Modifier.fillMaxWidth())
                            OutlinedButton(controller::cancelSignIn) { Text(stringResource(R.string.assistant_cancel_sign_in)) }
                        } else if (ready) OutlinedButton(controller::signOut, enabled = "signOut" !in state.busy) { Text(stringResource(R.string.assistant_sign_out)) }
                        else Button(controller::signIn) { Text(stringResource(R.string.assistant_continue_chatgpt)) }
                        Row { TextButton({ controller.openLink("usage") }) { Text(stringResource(R.string.assistant_plan_usage)) }; TextButton({ controller.openLink("disconnect") }) { Text(stringResource(R.string.assistant_connected_apps)) } }
                    } else {
                        val saved = status?.optBoolean(if (provider == "openai") "hasOpenaiKey" else "hasAnthropicKey") == true
                        Text(stringResource(R.string.assistant_api_billing, if (provider == "openai") "OpenAI" else "Anthropic"))
                        if (saved) OutlinedButton({ controller.forgetKey(provider) }, enabled = "key" !in state.busy) { Text(stringResource(R.string.assistant_remove_key)) }
                        else {
                            FilledField(key, { key = it }, label = { Text(stringResource(R.string.assistant_api_key)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true, modifier = Modifier.fillMaxWidth())
                            Button({ controller.setKey(provider, key) { key = "" } }, enabled = key.isNotBlank() && "key" !in state.busy) { Text(stringResource(if ("key" in state.busy) R.string.assistant_checking_key else R.string.assistant_verify_key)) }
                        }
                        TextButton({ controller.openLink(if (provider == "openai") "openaiKeys" else "anthropicKeys") }) { Text(stringResource(R.string.assistant_get_key)) }
                    }
                    status?.text("storage")?.takeIf(String::isNotEmpty)?.let { Text(stringResource(R.string.assistant_credentials, it), style = MaterialTheme.typography.bodySmall) }
                    if (ready && status?.text("storage").isNullOrEmpty()) Text(stringResource(R.string.assistant_credentials_memory), style = MaterialTheme.typography.bodySmall)
                    if (ready) {
                        Choice(stringResource(R.string.assistant_model), status?.text("model").orEmpty(), modelChoices) {
                            controller.settings(JSONObject().put("model", it))
                        }
                        TextButton(controller::models, enabled = "models" !in state.busy) { Text(stringResource(R.string.assistant_refresh_models)) }
                    }
                    HorizontalDivider()
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) { Text(stringResource(R.string.assistant_share_memos)); Text(stringResource(R.string.assistant_share_memos_detail), style = MaterialTheme.typography.bodySmall) }
                        Switch(status?.optBoolean("shareMemos", true) ?: true, { controller.settings(JSONObject().put("shareMemos", it)) })
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) { Text(stringResource(R.string.assistant_keep_history)); Text(stringResource(R.string.assistant_keep_history_detail), style = MaterialTheme.typography.bodySmall) }
                        Switch(status?.optBoolean("keepHistory", true) ?: true, { controller.settings(JSONObject().put("keepHistory", it)) })
                    }
                    OutlinedButton({ deleteHistory = true }) { Text(stringResource(R.string.assistant_delete_everything)) }
                }
                TextButton(close, Modifier.align(Alignment.End)) { Text(stringResource(R.string.common_done)) }
            }
        }
    }
    if (deleteHistory) AlertDialog(onDismissRequest = { deleteHistory = false }, title = { Text(stringResource(R.string.assistant_delete_history_title)) },
        text = { Text(stringResource(R.string.assistant_delete_history_message)) },
        confirmButton = { TextButton({ controller.deleteAll(); deleteHistory = false }) { Text(stringResource(R.string.assistant_delete_all)) } },
        dismissButton = { TextButton({ deleteHistory = false }) { Text(stringResource(R.string.common_cancel)) } })
}
