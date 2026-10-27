package app.urbudget.nativeapp

import android.os.Bundle
import android.content.Intent
import android.content.res.Configuration
import android.net.Uri
import androidx.activity.viewModels
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.FileProvider
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.debounce
import org.json.JSONObject
import java.io.File
import java.time.LocalDate
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import app.urbudget.nativeapp.ui.ProvideLanguage
import app.urbudget.nativeapp.ui.UrBudgetApp
import app.urbudget.nativeapp.ui.UrBudgetTheme

/**
 * AppCompat's activity, so the app's own language (AppLanguage) applies on every Android version.
 * It handles locale changes itself: a new language redraws the screens in place, keeping the page,
 * open editors and unsaved edits.
 */
class MainActivity : AppCompatActivity() {
    private val model: BudgetViewModel by viewModels()
    /** The configuration the screens are drawn in; a language change replaces it. */
    private var interfaceConfiguration by mutableStateOf<Configuration?>(null)

    @OptIn(kotlinx.coroutines.FlowPreview::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        applyLanguage()
        enableEdgeToEdge()
        setContent {
            ProvideLanguage(interfaceConfiguration ?: resources.configuration) { UrBudgetTheme { UrBudgetApp(model) } }
        }
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                launch {
                    NativeServices.events.collect { event ->
                        val payload = event.optJSONObject("payload") ?: return@collect
                        runCatching {
                            when (event.optString("event")) {
                                "host://open-url" -> {
                                    val uri = Uri.parse(payload.getString("url"))
                                    require(uri.scheme in setOf("https", "http"))
                                    startActivity(Intent(Intent.ACTION_VIEW, uri))
                                }
                                "host://open-file" -> shareFile(payload.getString("path"))
                            }
                        }.onFailure { model.notifyError(it.message ?: getString(R.string.error_no_app_to_open)) }
                    }
                }
                launch(Dispatchers.IO) {
                    runCatching { NativeServices.initialize(applicationContext) }
                    while (isActive) { runCatching { NativeServices.pollEvents() }; delay(300) }
                }
                launch(Dispatchers.IO) {
                    runCatching {
                        NativeServices.initialize(applicationContext)
                        if (NativePreferences(applicationContext).automaticSchedules) NativeServices.call("apply_due_schedules", JSONObject().put("through", LocalDate.now().toString()))
                    }
                    model.refresh()
                    while (isActive) { syncIfReady(); delay(60_000) }
                }
                launch(Dispatchers.IO) { NativeServices.saved.debounce(2_000).collect { syncIfReady() } }
            }
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        applyLanguage()
    }

    /** Points the words, month names and engine at the activity's language, and redraws the screens in it. */
    private fun applyLanguage() {
        val configuration = Configuration(resources.configuration)
        val language = AppLanguage.apply(applicationContext, configuration)
        NativeServices.setLanguage(language)
        interfaceConfiguration = configuration
        // Below Android 13 AppCompat sets the default locale after this callback; keep digits Western after it too.
        window.decorView.post { AppLanguage.keepWesternDigits() }
    }

    private fun syncIfReady() {
        if (!NativePreferences(applicationContext).automaticSync) return
        runCatching {
            if (NativeServices.syncAutomatically()?.optBoolean("openChanged") == true) model.refresh()
        }
    }

    private suspend fun shareFile(path: String) {
        val exported = withContext(Dispatchers.IO) {
            val source = File(path).canonicalFile
            require(source.toPath().startsWith(filesDir.canonicalFile.toPath()) && source.isFile) { getString(R.string.error_generated_file_unavailable) }
            require(source.length() <= 64L * 1024 * 1024) { getString(R.string.error_file_too_large_to_share) }
            val directory = File(cacheDir, "exports").apply { mkdirs() }
            source.copyTo(File(directory, source.name), overwrite = true)
        }
        val uri = FileProvider.getUriForFile(this, "$packageName.files", exported)
        val type = android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(exported.extension.lowercase()) ?: "application/octet-stream"
        try {
            startActivity(Intent(Intent.ACTION_VIEW).setDataAndType(uri, type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        } catch (_: android.content.ActivityNotFoundException) {
            startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType(type).putExtra(Intent.EXTRA_STREAM, uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
                getString(R.string.share_file_title, exported.name)))
        }
    }
}
