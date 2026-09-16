package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** A single worker owns the candidate until explicit Apply, Cancel, or disposal. */
internal class DocumentColorJob(val host: CanvasHost, val id: Int, private val source: Boolean = false) {
    var busy by mutableStateOf(false)
    var ready by mutableStateOf(false)
    var error by mutableStateOf<String?>(null)
    var clipped by mutableStateOf(0L)
    var addsLayer by mutableStateOf(false)
    var sourceProfile by mutableStateOf("")
    var previews by mutableStateOf<List<ImageBitmap>>(emptyList())
    private var task = 0L
    private var control = 0L
    private var closing = false
    private var finished = false
    private suspend fun release() {
        val old = task; task = 0
        val flag = control; control = 0
        withContext(NonCancellable + Dispatchers.IO) { if (old != 0L) { if (source) Native.sourceFree(old) else Native.colorFree(old) }; if (flag != 0L) Native.captureFree(flag) }
        ready = false
    }
    fun invalidate() { ready = false; previews = emptyList() }
    fun close() {
        if (finished) return
        closing = true
        if (control != 0L) Native.captureCancel(control)
        if (!busy) host.viewModelScope.launch { finishCancel() }
    }
    private suspend fun finishCancel() {
        if (finished) return
        finished = true; release()
        runCatching { host.withNative { Native.documentComplete(it, id, false, "null") }; host.documentChanged() }
    }
    fun prepare(choice: JSONObject?, history: Boolean = false) {
        if (busy || closing || finished) return
        busy = true; ready = false; error = null; previews = emptyList()
        host.viewModelScope.launch {
            try {
                release(); control = Native.captureControl()
                task = host.withNative { if (source) Native.sourceTask(it, id, control) else Native.colorTask(it, id, control) }
                val result = if (source) {
                    withContext(Dispatchers.IO) { Native.sourceWork(task, choice?.toString() ?: "null") }
                    host.withNative { Native.sourcePrepareComparison(it, task) }
                    withContext(Dispatchers.IO) { JSONObject(Native.sourceCompare(task)) }
                } else withContext(Dispatchers.IO) { JSONObject(Native.colorWork(task, choice?.toString() ?: "null")) }
                if (!closing) {
                    clipped = result.optLong("clipped_channels"); addsLayer = result.optBoolean("adds_layer"); sourceProfile = result.optString("source_profile")
                    if (!history) previews = withContext(Dispatchers.IO) { listOf(false, true).map { comparisonBitmap(if (source) Native.sourcePreview(task, it) else Native.colorPreview(task, it)) } }
                    ready = true
                }
            } catch (e: Exception) { if (!closing) error = e.message ?: "Could not prepare color change"; release() }
            finally { busy = false; if (closing) finishCancel() }
            if (history && ready && !closing) apply()
        }
    }
    fun apply() {
        if (busy || !ready || closing || finished) return
        busy = true
        host.viewModelScope.launch {
            try {
                host.withNative { if (source) Native.sourceAdopt(it, task) else Native.colorAdopt(it, task) }
                finished = true; host.documentChanged(); release()
            } catch (e: Exception) { error = e.message ?: "Could not apply color change"; release() }
            finally { busy = false; if (closing) finishCancel() }
        }
    }

}

@Composable internal fun DocumentColorDialog(host: CanvasHost, request: JSONObject) {
    val id = request.getInt("id")
    val spec = request.getJSONObject("kind").getJSONObject("request")
    val history = spec.getString("type") == "color_history"
    val operation = spec.optString("operation")
    val job = remember(id) { DocumentColorJob(host, id) }
    var space by remember { mutableStateOf("Srgb") }
    var depth by remember { mutableStateOf("U16") }
    var intent by remember { mutableStateOf("RelativeColorimetric") }
    var dither by remember { mutableStateOf("None") }
    var loaded by remember { mutableStateOf(false) }
    LaunchedEffect(id) {
        try {
            val current = JSONObject(host.withNative { Native.query(it, obj("type" to "document_color").toString()) })
            space = current.getString("space"); depth = current.getString("depth"); loaded = true
            if (history) job.prepare(null, true)
        } catch (e: Exception) { job.error = e.message }
    }
    DisposableEffect(job) { onDispose { job.close() } }
    val title = if (history) (if (spec.getBoolean("redo")) "Redo color change" else "Undo color change") else when (operation) { "assign" -> "Assign Profile"; "depth" -> "Change Bit Depth"; else -> "Convert Color Space" }
    AlertDialog(onDismissRequest = job::close, title = { Text(title) },
        dismissButton = { TextButton(job::close) { Text(if (job.busy) "Cancel operation" else "Cancel") } },
        confirmButton = { if (!history) TextButton(job::apply, enabled = job.ready && !job.busy) { Text("Apply") } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 600.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!history) {
                    Text(when (operation) {
                        "assign" -> "Keep document RGB numbers and reinterpret their color. Retained original photos keep their source profile."
                        "depth" -> "Change stored precision. Effects and the blending domain remain the same."
                        else -> "Convert editable layers. Compare the complete composition before applying; original photo samples stay retained."
                    })
                    if (loaded && !job.busy) {
                        if (operation != "depth") ColorChoice("Color space", listOf("Srgb" to "sRGB", "DisplayP3" to "Display P3", "AdobeRgb" to "Adobe RGB", "ProPhoto" to "ProPhoto RGB"), space) { space = it; job.invalidate() }
                        if (operation == "depth") {
                            ColorChoice("Bit depth", listOf("U8" to "8-bit SDR", "U16" to "16-bit SDR"), depth) { depth = it; job.invalidate() }
                            if (depth == "U8") ColorChoice("Dither", listOf("None" to "None", "Stochastic8" to "Stochastic"), dither) { dither = it; job.invalidate() }
                        }
                        if (operation == "convert") ColorChoice("Rendering intent", listOf("RelativeColorimetric" to "Relative colorimetric", "Perceptual" to "Perceptual", "Saturation" to "Saturation", "AbsoluteColorimetric" to "Absolute colorimetric"), intent) { intent = it; job.invalidate() }
                        TextButton({ job.prepare(when (operation) {
                            "assign" -> obj("Assign" to space)
                            "depth" -> obj("Depth" to obj("depth" to depth, "dither" to if (depth == "U8") dither else "None"))
                            else -> obj("Convert" to obj("space" to space, "options" to obj("intent" to intent, "black_point_compensation" to false)))
                        }) }) { Text("Preview Complete Result") }
                    }
                }
                if (job.busy) { CircularProgressIndicator(); Text("Preparing complete color result…") }
                if (job.clipped > 0) Text("Some colors exceed the destination gamut. Compare the result before applying.")
                job.previews.forEachIndexed { index, bitmap -> Text(if (index == 0) "Before" else "After"); Image(bitmap, if (index == 0) "Original composition" else "Prepared composition", Modifier.fillMaxWidth().heightIn(max = 180.dp)) }
                job.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}

internal fun comparisonBitmap(bytes: ByteArray): ImageBitmap {
        val data = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        val width = data.int; val height = data.int
        val colors = IntArray(width * height) {
            val r = data.get().toInt() and 255; val g = data.get().toInt() and 255
            val b = data.get().toInt() and 255; val a = data.get().toInt() and 255
            (a shl 24) or (r shl 16) or (g shl 8) or b
        }
        return Bitmap.createBitmap(colors, width, height, Bitmap.Config.ARGB_8888).asImageBitmap()
    }
