package art.capycanvas

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject
import java.util.zip.GZIPOutputStream

/** Window-owned state survives panel hiding, document switches and rotation. */
internal class StrokeRecording(private val host: CanvasHost) {
    var status by mutableStateOf<JSONObject?>(null)
        private set
    var busy by mutableStateOf(false)
    var saveRequested by mutableStateOf(false)

    suspend fun update(action: Int = 0) {
        val next = host.withNative { JSONObject(Native.strokeRecording(it, action)) }
        if (status?.optBoolean("recording") == true && next.optBoolean("ready")) saveRequested = true
        status = next
    }
    fun click() {
        if (busy) return
        busy = true
        host.viewModelScope.launch {
            try {
                when {
                    status?.optBoolean("recording") == true -> update(2)
                    status?.optBoolean("ready") == true -> saveRequested = true
                    else -> update(1)
                }
            } catch (e: Exception) { host.reportActionError(e.message ?: "Could not record strokes") }
            finally { busy = false }
        }
    }
}

@Composable internal fun StrokeRecordingSave(host: CanvasHost) {
    val recording = host.strokeRecording
    val resolver = LocalContext.current.contentResolver
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri == null) recording.busy = false
        else host.viewModelScope.launch {
            try {
                // Only take the snapshot on the engine owner; compression and
                // provider I/O run away from the render Looper and main thread.
                val raw = host.withNative { Native.strokeRecordingData(it) }
                withContext(Dispatchers.IO) {
                    val stream = resolver.openOutputStream(uri, "wt") ?: error("Could not open recording destination")
                    stream.use { output ->
                        output.write("CAPYPEN2".toByteArray(Charsets.US_ASCII))
                        GZIPOutputStream(output).use { it.write(raw) }
                    }
                }
                recording.update(3)
            } catch (e: Exception) { host.reportActionError(e.message ?: "Could not save stroke recording") }
            finally { recording.busy = false }
        }
    }
    LaunchedEffect(host) {
        while (isActive) {
            // Native initialization may still be pending on first composition.
            if (host.snapshot != null && (recording.status == null || recording.status?.optBoolean("recording") == true)) {
                try { recording.update() }
                catch (e: CancellationException) { throw e }
                catch (e: Exception) { host.reportActionError(e.message ?: "Could not read stroke recording status"); break }
            }
            delay(200)
        }
    }
    LaunchedEffect(recording.saveRequested) {
        if (recording.saveRequested) {
            recording.saveRequested = false
            recording.busy = true
            try { launcher.launch("stroke-recording.capystrokes") }
            catch (e: Exception) { recording.busy = false; host.reportActionError(e.message ?: "Could not open recording destination") }
        }
    }
}
