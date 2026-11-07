package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** A single worker owns the candidate until explicit Apply, Cancel, or disposal. */
internal class DocumentColorJob(val host: CanvasHost, val id: Int, private val source: Boolean = false) {
    var busy by mutableStateOf(false)
    var applying by mutableStateOf(false)
        private set
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
    fun invalidate() {
        ready = false; previews = emptyList(); pending = null; generation++
        if (control != 0L) Native.captureCancel(control)
    }
    fun close() {
        if (finished) return
        closing = true; pending = null
        if (control != 0L) Native.captureCancel(control)
        if (!busy) host.viewModelScope.launch { finishCancel() }
    }
    private suspend fun finishCancel() {
        if (finished) return
        finished = true; release()
        runCatching { host.withNative { Native.documentComplete(it, id, false, "null") }; host.documentChanged() }
    }
    private data class Draft(val choice: String?, val history: Boolean, val copy: Boolean, val generation: Long)
    private var generation = 0L
    private var pending: Draft? = null
    fun prepare(choice: JSONObject?, history: Boolean = false, copy: Boolean = false) {
        if (closing || finished || applying) return
        invalidate()
        pending = Draft(choice?.toString(), history, copy, generation)
        if (busy) return
        busy = true
        host.viewModelScope.launch {
            var historyReady = false
            try {
                while (!closing && !finished) {
                    val draft = pending ?: break
                    pending = null
                    release(); control = Native.captureControl()
                    this@DocumentColorJob.copy = draft.copy
                    error = null
                    try {
                        task = host.withNative { if (source) Native.sourceTask(it, id, control) else Native.colorTask(it, id, control) }
                        val result = if (source) {
                            withContext(Dispatchers.IO) { Native.sourceWork(task, draft.choice ?: "null") }
                            host.withNative { Native.sourcePrepareComparison(it, task) }
                            withContext(Dispatchers.IO) { JSONObject(Native.sourceCompare(task)) }
                        } else withContext(Dispatchers.IO) { JSONObject(Native.colorWork(task, draft.choice ?: "null", draft.copy)) }
                        if (!closing && draft.generation == generation) {
                            val images = if (draft.history) emptyList() else withContext(Dispatchers.IO) { listOf(false, true).map { comparisonBitmap(if (source) Native.sourcePreview(task, it) else Native.colorPreview(task, it)) } }
                            if (!closing && draft.generation == generation) {
                                clipped = result.optLong("clipped_channels"); addsLayer = result.optBoolean("adds_layer")
                                sourceProfileName = if (result.isNull("source_profile_name")) null else result.getString("source_profile_name")
                                previews = images; ready = true; historyReady = draft.history
                            }
                        }
                    } catch (e: Exception) {
                        if (!closing && draft.generation == generation) error = e
                        release()
                    }
                }
            } finally { busy = false; if (closing) finishCancel() else refreshCopy() }
            if (historyReady && ready && !closing) apply()
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
        busy = true; applying = true
        host.viewModelScope.launch {
            try {
                host.withNative { if (source) Native.sourceAdopt(it, task) else Native.colorAdopt(it, task) }
                finished = true; host.documentChanged(); release()
            } catch (e: Exception) { error = e; release() }
            finally { applying = false; busy = false; if (closing) finishCancel() }
        }
    }

    fun openCopy() {
        if (busy || !ready || closing || finished || !copy) return
        busy = true; applying = true; error = null
        host.viewModelScope.launch {
            var opening = 0L
            var transitioning = false
            try {
                opening = host.withNative { Native.colorCopyTask(it, task) }
                withContext(Dispatchers.IO) { Native.colorCopyPrepare(opening, task) }
                if (closing) return@launch
                host.drawingTabs.beforeAdopt { host.withNative { Native.colorCopyReady(it, opening, task) } }
                transitioning = true
                host.withNative { Native.colorCopyAdopt(it, opening, task) }
                finished = true; host.documentChanged(); release()
            } catch (e: Exception) { if (!closing) error = e }
            finally {
                withContext(NonCancellable + Dispatchers.IO) { if (opening != 0L) Native.colorCopyFree(opening) }
                if (transitioning) host.drawingTabs.afterAdopt()
                applying = false; busy = false; if (closing) finishCancel()
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
    var dither by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf("layers") }
    var spaces by remember { mutableStateOf<List<Pair<String,String>>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    LaunchedEffect(id) {
        try {
            val current = JSONObject(host.withNative { Native.query(it, obj("type" to "document_color").toString()) })
            val form=JSONObject(host.withNative { Native.query(it,obj("type" to "export_form").toString()) })
            spaces=form.array("profiles").objects().mapNotNull {profile -> profile.getJSONObject("profile").nullableString("Builtin")?.let{it to profile.getString("name")} }
            space = current.getString("space")
            depth = org.json.JSONTokener(Native.dialogUi(obj("kind" to "document_color_suggested_depth", "depth" to current.getString("depth")).toString())).nextValue() as String
            loaded = true
            if (history) job.prepare(null, true)
        } catch (e: Exception) { job.error = e }
    }
    val ditherChoice = JSONObject(Native.dialogUi(obj("kind" to "document_color_dither", "depth" to depth, "active" to dither).toString()))
    val choice = when (operation) {
        "assign" -> obj("Assign" to space)
        "depth" -> obj("Depth" to obj("depth" to depth, "dither" to ditherChoice.getString("value")))
        else -> obj("Convert" to obj("space" to space, "options" to obj("intent" to intent, "black_point_compensation" to false)))
    }
    LaunchedEffect(loaded, history, choice.toString(), result) { if (loaded && !history) job.prepare(choice, copy = result == "copy") }
    DisposableEffect(job) { onDispose { job.close() } }
    val copy = host.catalog.getJSONObject("document_color_copy")
    val common = copy.getJSONObject("common")
    val exportCopy = host.catalog.getJSONObject("export_copy")
    val title = if (history) copy.getString(if (spec.getBoolean("redo")) "redo_title" else "undo_title") else when (operation) { "assign" -> copy.getString("assign_title"); "depth" -> copy.getString("depth_title"); else -> copy.getString("convert_title") }
    val apply={if(job.copy)job.openCopy() else job.apply()}
    if (job.applying) {
        ProgressDialog(host, copy.getString("applying"), "document-color", onCancel = job::close)
        return
    }
    DialogShell(host,copy.getJSONObject(if(history)"progress_dialog" else "dialog"),title,
        onCancel=job::close,onPrimary=apply,primaryLabel=common.getString("apply"),primaryEnabled=job.ready&&!job.busy,tag="document-color",primaryTag="document-color-apply",
        responses=listOf(DialogResponse(common.getString("cancel"),"document-color-cancel",action=job::close))+
            if(history)emptyList() else listOf(DialogResponse(common.getString("apply"),"document-color-apply",accent=true,enabled=job.ready&&!job.busy,action=apply)),
        body={padding ->
            Column(Modifier.fillMaxWidth().padding(padding),verticalArrangement=Arrangement.spacedBy(8.dp)) {
                if (!history) {
                    Text(when (operation) {
                        "assign" -> copy.getString("assign_native_help")
                        "depth" -> copy.getString("depth_native_help")
                        else -> copy.getString("convert_native_help")
                    })
                    if (loaded) {
                        if (operation != "depth") ColorChoice(exportCopy.getString("profile"),spaces,space, enabled = !job.applying) { if (!job.applying) { space = it; job.invalidate() } }
                        if (operation == "depth") {
                            ColorChoice(exportCopy.getString("depth"), listOf("U8" to copy.getString("depth_8"), "U16" to copy.getString("depth_16"), "F16" to copy.getString("depth_float16"), "F32" to copy.getString("depth_float32")), depth, enabled = !job.applying) { if (!job.applying) { depth = it; job.invalidate() } }
                            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                                Column(Modifier.weight(1f)) {Text(exportCopy.getString("reduce_banding"));Text(exportCopy.getString("dither_gradients"),color=LocalPalette.current.settingsSecondary)}
                                Switch(dither,{dither=it;job.invalidate()},modifier=Modifier.testTag("document-color-dither"),enabled=ditherChoice.getBoolean("available")&&!job.applying)
                            }
                        }
                        if (operation == "convert") ColorChoice(copy.getString("result"), listOf("layers" to copy.getString("convert_layers"), "copy" to copy.getString("create_flattened")), result, enabled = !job.applying) { if (!job.applying) { result = it; job.invalidate() } }
                        if (operation == "convert") ColorChoice(exportCopy.getString("intent"), listOf("RelativeColorimetric" to exportCopy.getString("relative"), "Perceptual" to exportCopy.getString("perceptual"), "Saturation" to exportCopy.getString("saturation"), "AbsoluteColorimetric" to exportCopy.getString("absolute")), intent, enabled = !job.applying) { if (!job.applying) { intent = it; job.invalidate() } }

                    }
                }
                if (job.busy) { CircularProgressIndicator(); Text(copy.getString("preparing")) }
                if (job.clipped > 0) Text(copy.getString("outside_gamut"))
                if(!history)Text(copy.getString("compare_before_apply"))
                if(job.previews.isNotEmpty())BoxWithConstraints(Modifier.fillMaxWidth()) {
                    val split=Native.dialogUi(obj("kind" to "panes_fit","width" to maxWidth.value,"minimum" to JSONArray(listOf(160,160)),"gap" to 12).toString()).toBoolean()
                    @Composable fun picture(index:Int,modifier:Modifier) {
                        Column(modifier,horizontalAlignment=Alignment.CenterHorizontally) {
                            Text(copy.getString(if(index==0)"before" else "after"))
                            Image(job.previews[index],copy.getString(if(index==0)"original_composition" else "prepared_composition"),Modifier.fillMaxWidth().heightIn(max=180.dp))
                        }
                    }
                    if(split)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)){job.previews.indices.forEach{picture(it,Modifier.weight(1f))}}
                    else Column(verticalArrangement=Arrangement.spacedBy(12.dp)){job.previews.indices.forEach{picture(it,Modifier.fillMaxWidth())}}
                }
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
