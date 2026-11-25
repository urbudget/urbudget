package app.urbudget.nativeapp.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.decodeSyncCode
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The camera, full screen, until it reads one of UrBudget's QR codes ([found], with the code's
 * text), the person closes it ([close]), or it can't be used ([unavailable]: no camera, or the
 * camera permission refused), after which the code can be typed.
 *
 * What the camera sees stays on the phone: each frame's brightness is read in memory for a code
 * (`decodeSyncCode`) and dropped. Nothing is saved, and no service of anyone else's is involved.
 * The permission is asked for here, the first time someone scans.
 */
@Composable
internal fun SyncCodeScanner(found: (String) -> Unit, unavailable: () -> Unit, close: () -> Unit) {
    val context = LocalContext.current
    var allowed by remember { mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) }
    val latestUnavailable by rememberUpdatedState(unavailable)
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted -> if (granted) allowed = true else latestUnavailable() }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    if (!allowed) return
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            CameraFrames(found, unavailable)
            Row(Modifier.fillMaxWidth().background(Color.Black.copy(alpha = 0.55f)).statusBarsPadding().padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.scan_hint), Modifier.weight(1f), color = Color.White, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Start)
                IconButton(close) { Icon(Icons.Outlined.Close, stringResource(R.string.common_close), tint = Color.White) }
            }
        }
    }
}

/** The back camera's picture, with each frame looked at for a code until one is found. */
@Composable
private fun CameraFrames(found: (String) -> Unit, unavailable: () -> Unit) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val latestFound by rememberUpdatedState(found)
    val latestUnavailable by rememberUpdatedState(unavailable)
    val view = remember { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FILL_CENTER } }
    DisposableEffect(owner) {
        val frames = Executors.newSingleThreadExecutor()
        val main = ContextCompat.getMainExecutor(context)
        val cameras = ProcessCameraProvider.getInstance(context)
        // One answer per opening: the first code read, or that the camera couldn't start.
        val answered = AtomicBoolean(false)
        var bound: ProcessCameraProvider? = null
        cameras.addListener({
            try {
                val provider = cameras.get()
                val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                // Only the newest frame is read, so a slow phone falls behind on nothing.
                val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                analysis.setAnalyzer(frames) { frame ->
                    val code = frame.use(::codeIn)
                    if (code != null && answered.compareAndSet(false, true)) main.execute { latestFound(code) }
                }
                provider.unbindAll()
                // A Chromebook or tablet may have only the camera facing the person: the code is
                // held up to that one instead.
                val camera = if (provider.hasCamera(CameraSelector.DEFAULT_BACK_CAMERA)) CameraSelector.DEFAULT_BACK_CAMERA else CameraSelector.DEFAULT_FRONT_CAMERA
                provider.bindToLifecycle(owner, camera, preview, analysis)
                bound = provider
            } catch (_: Exception) {
                if (answered.compareAndSet(false, true)) latestUnavailable()
            }
        }, main)
        onDispose {
            answered.set(true)
            bound?.unbindAll()
            frames.shutdown()
        }
    }
    AndroidView({ view }, Modifier.fillMaxSize())
}

/** The UrBudget code a frame shows, if any. Reads the brightness plane, row by row past its padding. */
private fun codeIn(frame: ImageProxy): String? {
    val plane = frame.planes.firstOrNull() ?: return null
    val width = frame.width
    val height = frame.height
    val brightness = ByteArray(width * height)
    val pixels = plane.buffer
    for (row in 0 until height) {
        val start = row * plane.rowStride
        if (start + width > pixels.limit()) return null
        pixels.position(start)
        pixels.get(brightness, row * width, width)
    }
    return decodeSyncCode(brightness, width, height)
}
