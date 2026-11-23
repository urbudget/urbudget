package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.NativeBridge
import app.urbudget.nativeapp.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** The template a new budget starts on, the engine's first (budget_templates.rs). */
internal const val DEFAULT_TEMPLATE = "everyday"

/**
 * The category templates a new budget can start with, as the engine writes them in the language in use, or none
 * if it can't say. A pure engine request: it needs no budget, so it's asked directly rather than through the
 * view model, whose queries are dropped when the open budget changes under them.
 */
internal fun budgetTemplates(): List<JSONObject> = runCatching {
    val request = JSONObject().put("version", 1).put("method", "budgetTemplates").put("params", JSONObject())
    val reply = JSONObject(NativeBridge.invoke(request.toString()))
    val rows = reply.optJSONObject("value")?.optJSONArray("templates")
    List(rows?.length() ?: 0) { rows!!.getJSONObject(it) }
}.getOrDefault(emptyList())

/**
 * Which template a new budget starts with: a choice, with what the chosen one holds under it. Until they've
 * loaded nothing shows, and the default is what's made.
 */
@Composable
internal fun TemplateChoice(selected: String, onChange: (String) -> Unit) {
    // The engine is told the app's language with the configuration, so a change asks again.
    val language = LocalConfiguration.current.locales[0].toLanguageTag()
    var templates by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    LaunchedEffect(language) { templates = withContext(Dispatchers.Default) { budgetTemplates() } }
    if (templates.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Choice(stringResource(R.string.budget_template), selected, templates.map { it.text("id") to it.text("name") }, onChange)
        templates.firstOrNull { it.text("id") == selected }?.let { chosen ->
            Column(Modifier.padding(horizontal = 4.dp).testTag("template-preview")) {
                Text(chosen.text("description"), color = ur.text2, style = MaterialTheme.typography.bodySmall)
                if ((chosen.optJSONArray("groups")?.length() ?: 0) > 0)
                    Text(chosen.text("summary"), color = ur.muted, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}
