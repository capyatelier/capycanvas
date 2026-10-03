package art.capycanvas

import android.app.Application
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
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
import org.json.JSONTokener
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** A single worker owns the candidate until explicit Apply, Cancel, or disposal. */
internal class DocumentColorJob(val host: CanvasHost, val id: Int, private val source: Boolean = false) {
    var busy by mutableStateOf(false)
    var publishing by mutableStateOf(false)
    var copy by mutableStateOf(false)
        private set
    var ready by mutableStateOf(false)
    var error by mutableStateOf<Exception?>(null)
    var clipped by mutableStateOf(0L)
    var addsLayer by mutableStateOf(false)
    var sourceProfile by mutableStateOf("")
        private set
    private var sourceProfileName: String? = null
    private var copyGeneration = 0L
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
        if (finished || publishing) return
        closing = true
        if (control != 0L) Native.captureCancel(control)
        if (!busy) host.viewModelScope.launch { finishCancel() }
    }
    private suspend fun finishCancel() {
        if (finished) return
        finished = true; release()
        runCatching { host.withNative { Native.documentComplete(it, id, false, "null") }; host.documentChanged() }
    }
    fun prepare(choice: JSONObject?, history: Boolean = false, copy: Boolean = false) {
        if (busy || closing || finished) return
        this.copy = copy
        busy = true; ready = false; error = null; previews = emptyList()
        host.viewModelScope.launch {
            try {
                release(); control = Native.captureControl()
                task = host.withNative { if (source) Native.sourceTask(it, id, control) else Native.colorTask(it, id, control) }
                val result = if (source) {
                    withContext(Dispatchers.IO) { Native.sourceWork(task, choice?.toString() ?: "null") }
                    host.withNative { Native.sourcePrepareComparison(it, task) }
                    withContext(Dispatchers.IO) { JSONObject(Native.sourceCompare(task)) }
                } else withContext(Dispatchers.IO) { JSONObject(Native.colorWork(task, choice?.toString() ?: "null", copy)) }
                if (!closing) {
                    clipped = result.optLong("clipped_channels"); addsLayer = result.optBoolean("adds_layer"); sourceProfileName = if (result.isNull("source_profile_name")) null else result.getString("source_profile_name")
                    if (!history) previews = withContext(Dispatchers.IO) { listOf(false, true).map { comparisonBitmap(if (source) Native.sourcePreview(task, it) else Native.colorPreview(task, it)) } }
                    ready = true
                }
            } catch (e: Exception) { if (!closing) error = e; release() }
            finally { busy = false; if (closing) finishCancel() else refreshCopy() }
            if (history && ready && !closing) apply()
        }
    }
    fun refreshCopy() {
        if (!source || !ready || closing || finished) return
        val generation = ++copyGeneration
        val language = host.languageTag
        val retained = sourceProfileName
        host.viewModelScope.launch {
            val caption = profileNameCopy(host, retained)
            if (generation == copyGeneration && language == host.languageTag && ready && !closing && !finished) sourceProfile = caption
        }
    }
    fun apply() {
        if (busy || !ready || closing || finished || copy) return
        busy = true
        host.viewModelScope.launch {
            try {
                host.withNative { if (source) Native.sourceAdopt(it, task) else Native.colorAdopt(it, task) }
                finished = true; host.documentChanged(); release()
            } catch (e: Exception) { error = e; release() }
            finally { busy = false; if (closing) finishCancel() }
        }
    }

    fun saveCopy(uri: Uri) {
        if (busy || !ready || closing || finished || !copy) return
        busy = true; error = null
        host.viewModelScope.launch {
            var temporary: File? = null
            try {
                val application = host.getApplication<Application>()
                val master = host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.objectOrNull("location")?.optString("uri")
                check(uri.toString() != master) { host.catalog.getJSONObject("document_delivery_copy").getString("separate_copy") }
                withContext(Dispatchers.IO) {
                    val name = application.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { if (it.moveToFirst()) it.getString(0) else null }
                    check(name?.endsWith(".capy", ignoreCase = true) == true) { host.catalog.getJSONObject("document_delivery_copy").getString("converted_filename_invalid") }
                    temporary = File.createTempFile("capy-converted-", ".capy", application.cacheDir)
                    Native.colorWriteCopy(task, ParcelFileDescriptor.open(temporary, ParcelFileDescriptor.MODE_READ_WRITE).detachFd())
                }
                if (closing) return@launch
                publishing = true
                withContext(Dispatchers.IO) {
                    application.contentResolver.openOutputStream(uri, "wt")?.use { output ->
                        temporary!!.inputStream().use { it.copyTo(output) }; output.flush()
                    } ?: error(host.bootstrap!!.getString("action_failed"))
                }
                host.withNative { Native.documentComplete(it, id, true, "null") }
                finished = true; host.documentChanged(); release()
            } catch (e: Exception) { if (!closing) error = e }
            finally {
                withContext(NonCancellable + Dispatchers.IO) { temporary?.delete() }
                publishing = false; busy = false; if (closing) finishCancel()
            }
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
    var result by remember { mutableStateOf("layers") }
    val scope = rememberCoroutineScope()
    val saveCopy = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri -> if (uri != null) job.saveCopy(uri) }
    var loaded by remember { mutableStateOf(false) }
    LaunchedEffect(id) {
        try {
            val current = JSONObject(host.withNative { Native.query(it, obj("type" to "document_color").toString()) })
            space = current.getString("space"); depth = current.getString("depth"); loaded = true
            if (history) job.prepare(null, true)
        } catch (e: Exception) { job.error = e }
    }
    DisposableEffect(job) { onDispose { job.close() } }
    val copy = host.catalog.getJSONObject("document_color_copy")
    val common = copy.getJSONObject("common")
    val exportCopy = host.catalog.getJSONObject("export_copy")
    val delivery = host.catalog.getJSONObject("document_delivery_copy")
    val title = if (history) copy.getString(if (spec.getBoolean("redo")) "redo_title" else "undo_title") else when (operation) { "assign" -> copy.getString("assign_title"); "depth" -> copy.getString("depth_title"); else -> copy.getString("convert_title") }
    AlertDialog(onDismissRequest = job::close, title = { Text(title) },
        dismissButton = { TextButton(job::close, enabled = !job.publishing) { Text(common.getString("cancel")) } },
        confirmButton = { if (!history) TextButton({
            if (job.copy) scope.launch {
                try {
                    val name = host.drawingTabs.rows.first { it.getLong("id") == host.drawingTabs.selected }.getString("title").substringBeforeLast('.')
                    val filename = host.withNative { Native.query(it, obj("type" to "document_delivery_message", "message" to obj("type" to "converted_name", "name" to name)).toString()) }
                    saveCopy.launch(JSONTokener(filename).nextValue() as String)
                } catch (e: Exception) { job.error = e }
            } else job.apply()
        }, enabled = job.ready && !job.busy) { Text(if (job.copy) copy.getString("save_copy") else common.getString("apply")) } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 600.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (!history) {
                    Text(when (operation) {
                        "assign" -> copy.getString("assign_help")
                        "depth" -> copy.getString("depth_help")
                        else -> copy.getString("convert_help")
                    })
                    if (loaded && !job.busy) {
                        if (operation != "depth") ColorChoice(copy.getString("space"), listOf("Srgb" to "sRGB", "DisplayP3" to "Display P3", "AdobeRgb" to "Adobe RGB", "ProPhoto" to "ProPhoto RGB"), space) { space = it; job.invalidate() }
                        if (operation == "depth") {
                            ColorChoice(copy.getString("depth"), listOf("U8" to copy.getString("depth_8"), "U16" to copy.getString("depth_16"), "F16" to copy.getString("depth_float16"), "F32" to copy.getString("depth_float32")), depth) { depth = it; job.invalidate() }
                            if (depth == "U8") ColorChoice(copy.getString("dither"), listOf("None" to exportCopy.getString("dither_none"), "Stochastic8" to copy.getString("dither_stochastic")), dither) { dither = it; job.invalidate() }
                        }
                        if (operation == "convert") ColorChoice(copy.getString("result"), listOf("layers" to copy.getString("editable_layers"), "copy" to copy.getString("flattened_copy")), result) { result = it; job.invalidate() }
                        if (operation == "convert") ColorChoice(exportCopy.getString("intent"), listOf("RelativeColorimetric" to exportCopy.getString("relative"), "Perceptual" to exportCopy.getString("perceptual"), "Saturation" to exportCopy.getString("saturation"), "AbsoluteColorimetric" to exportCopy.getString("absolute")), intent) { intent = it; job.invalidate() }
                        TextButton({ job.prepare(when (operation) {
                            "assign" -> obj("Assign" to space)
                            "depth" -> obj("Depth" to obj("depth" to depth, "dither" to if (depth == "U8") dither else "None"))
                            else -> obj("Convert" to obj("space" to space, "options" to obj("intent" to intent, "black_point_compensation" to false)))
                        }, copy = result == "copy") }) { Text(copy.getString("preview")) }
                    }
                }
                if (job.busy) { CircularProgressIndicator(); Text(if (job.publishing) delivery.getString("writing_converted_copy") else copy.getString("preparing")) }
                if (job.clipped > 0) Text(copy.getString("outside_gamut"))
                job.previews.forEachIndexed { index, bitmap -> Text(copy.getString(if (index == 0) "before" else "after")); Image(bitmap, copy.getString(if (index == 0) "original_composition" else "prepared_composition"), Modifier.fillMaxWidth().heightIn(max = 180.dp)) }
                ColorFailureText(host, job.error)
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
