package art.capycanvas

import android.app.Application
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject
import java.io.File

internal data class DocumentPicker(val request: JSONObject, val epoch: Long, val revision: Long, var launched: Boolean = false)

/** SAF owns locations; the shared session owns dirty checkpoints and close policy. */
internal class DocumentController(private val host: CanvasHost, private val application: Application) {
    companion object {
        // JNI integration tests own private file descriptors. SAF UI has a
        // separate end-to-end test and must not race those native job owners.
        @Volatile internal var nativeFileJobsForTest = false
    }
    var picker by mutableStateOf<DocumentPicker?>(null)
        private set
    var working by mutableStateOf(false)
        private set
    var exportRequest by mutableStateOf<JSONObject?>(null)
        private set
    private var exportRecipe: JSONObject? = null
    var profilePrompt by mutableStateOf<JSONObject?>(null)
        private set
    private var profileDecision: CompletableDeferred<JSONObject?>? = null
    fun chooseProfile(value: JSONObject?) { profileDecision?.complete(value); profilePrompt = null }
    var exporting by mutableStateOf(false)
        private set
    var publishing by mutableStateOf(false)
        private set
    var exportCancelled by mutableStateOf(false)
        private set
    private var exportControl = 0L
    fun cancelExport() {
        if (exporting && !publishing) { exportCancelled = true; if (exportControl != 0L) Native.captureCancel(exportControl) }
    }
    private var activeId: Int? = null
    private var approval = 0L to 0L
    fun observe(request: JSONObject?, file: JSONObject) {
        if (BuildConfig.DEBUG && nativeFileJobsForTest) return
        val id = request?.getInt("id")
        if (id == activeId) return
        activeId = id
        approval = file.optLong("epoch") to file.optLong("revision")
        if (request == null) return
        val document = request.getJSONObject("kind").getJSONObject("request")
        when (document.getString("type")) {
            "open" -> picker = DocumentPicker(request, approval.first, approval.second)
            "export" -> { exportRecipe = null; exportRequest = request }
            "save" -> document.objectOrNull("location")?.let { transfer(request, Uri.parse(it.getString("uri")), approval) }
                ?: run { picker = DocumentPicker(request, approval.first, approval.second) }
        }
    }
    fun picked(uri: Uri?, flags: Int = 0) {
        val pending = picker ?: return
        picker = null
        if (uri == null) { complete(pending.request.getInt("id"), false); return }
        val grant = flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        if (grant != 0) try { application.contentResolver.takePersistableUriPermission(uri, grant) } catch (_: SecurityException) { /* Some providers grant access only for this session. */ }
        transfer(pending.request, uri, pending.epoch to pending.revision)
    }
    fun pickerFailed(error: Exception) {
        val id = picker?.request?.getInt("id") ?: return
        picker = null; complete(id, false, error.message ?: "Could not open the file picker")
    }
    fun create(request: JSONObject, options: JSONObject) = transfer(request, null, approval, options.getJSONArray("extent").getInt(0), options.getJSONArray("extent").getInt(1), options)
    fun chooseExport(recipe: JSONObject) {
        val request = exportRequest ?: return
        exportRecipe = recipe; exportRequest = null
        val document = request.getJSONObject("kind").getJSONObject("request")
        val extension = when (recipe.getString("format")) { "Jpeg" -> "jpg"; "Tiff" -> "tif"; else -> "png" }
        document.put("name", document.getString("name").substringBeforeLast('.') + "." + extension)
        picker = DocumentPicker(request, approval.first, approval.second)
    }
    fun exportMime() = when (exportRecipe?.getString("format")) { "Jpeg" -> "image/jpeg"; "Tiff" -> "image/tiff"; else -> "image/png" }
    fun cancel(id: Int) { exportRequest = null; complete(id, false) }
    fun close(id: Int, decision: String) {
        host.viewModelScope.launch {
            try { host.withNative { Native.documentClose(it, id, JSONObject.quote(decision)) }; host.documentChanged() }
            catch (e: Exception) { host.reportActionError(e.message ?: "Could not close the drawing") }
        }
    }
    private fun complete(id: Int, success: Boolean, message: String? = null) {
        host.viewModelScope.launch { finish(id, success, message) }
    }
    private suspend fun finish(id: Int, success: Boolean, message: String? = null) {
        try { host.withNative { Native.documentComplete(it, id, success, message?.let(JSONObject::quote) ?: "null") }; host.documentChanged() }
        catch (e: Exception) { host.reportActionError(e.message ?: "Could not complete the file operation") }
    }
    private fun transfer(request: JSONObject, uri: Uri?, approved: Pair<Long, Long>, width: Int = 0, height: Int = 0, options: JSONObject? = null) {
        if (working) return
        working = true
        host.viewModelScope.launch {
            val id = request.getInt("id")
            val document = request.getJSONObject("kind").getJSONObject("request")
            val kind = document.getString("type")
            var task = 0L
            var control = 0L
            var temporary: File? = null
            try {
                val location = if (uri == null) null else withContext(Dispatchers.IO) {
                    val name = application.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    } ?: uri.lastPathSegment ?: document.optString("name", "Drawing.capy")
                    obj("uri" to uri.toString(), "name" to name)
                }
                if (kind == "export") {
                    val master = host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.objectOrNull("location")?.optString("uri")
                    check(uri.toString() != master) { "Choose a different file to keep the editable drawing." }
                    val extensions = when (exportRecipe?.getString("format")) { "Tiff" -> listOf("tif", "tiff"); "Jpeg" -> listOf("jpg", "jpeg"); else -> listOf("png") }
                    check(location!!.getString("name").substringAfterLast('.').lowercase() in extensions) { "Use a .${extensions.first()} filename for this image format." }
                }
                if (kind == "export") { control = Native.captureControl(); exportControl = control; exportCancelled = false; publishing = false; exporting = true }
                if (kind == "export") withTimeout(30_000) {
                    while (task == 0L) {
                        task = host.withNative { Native.projectExportTask(it, id, System.nanoTime(), control) }
                        if (task == 0L) { host.documentChanged(); delay(16) }
                    }
                } else task = host.withNative { Native.projectTask(it, id, location?.toString() ?: "null", approved.first, approved.second) }
                if (kind == "save" || kind == "export") {
                    // Finish encoding before opening/truncating the destination.
                    temporary = withContext(Dispatchers.IO) { File.createTempFile("capy-save-", if (kind == "export") ".png" else ".capy", application.cacheDir) }
                    withContext(Dispatchers.IO) {
                        val fd = ParcelFileDescriptor.open(temporary, ParcelFileDescriptor.MODE_READ_WRITE).detachFd()
                        if (kind == "export") exportRecipe?.let { Native.projectExportOptions(task, it.toString()) }
                        Native.projectWork(task, fd, 0, 0)
                    }
                    if (kind == "export" && exportCancelled) { finish(id, false); return@launch }
                    if (kind == "export") publishing = true
                    withContext(Dispatchers.IO) {
                        application.contentResolver.openOutputStream(uri!!, "wt")?.use { output ->
                            temporary!!.inputStream().use { it.copyTo(output) }; output.flush()
                        } ?: error("The selected file cannot be written")
                    }
                    finish(id, true)
                    if (kind == "save") host.documentChanged {
                        if (host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optBoolean("modified") == false) host.recovery.retire()
                    }
                } else {
                    withContext(Dispatchers.IO) {
                        val fd = if (uri == null) -1 else application.contentResolver.openFileDescriptor(uri, "r")?.detachFd() ?: error("The selected file cannot be read")
                        if (options != null) Native.projectOptions(task, options.toString())
                        Native.projectWork(task, fd, width, height)
                    }
                    val prompt = withContext(Dispatchers.IO) { Native.projectProfilePrompt(task) }
                    if (prompt != "null") {
                        val decision = CompletableDeferred<JSONObject?>(); profileDecision = decision; profilePrompt = JSONObject(prompt)
                        val profile = try { decision.await() } finally { profileDecision = null; profilePrompt = null }
                        if (profile == null) { finish(id, false); return@launch }
                        withContext(Dispatchers.IO) { Native.projectAssumeProfile(task, profile.toString()); Native.projectWork(task, -1, 0, 0) }
                    }
                    host.withNative { Native.projectAdopt(it, task, location?.toString() ?: "null") }
                    host.documentChanged()
                    host.recovery.retire()
                }
            } catch (e: CancellationException) {
                withContext(NonCancellable) { finish(id, false) }
                throw e
            } catch (e: Exception) {
                finish(id, false, if (kind == "export" && exportCancelled) null else e.message ?: "Could not complete the file operation")
            } finally {
                exportControl = 0; exporting = false; publishing = false
                withContext(NonCancellable + Dispatchers.IO) { if (task != 0L) Native.projectFree(task); if (control != 0L) Native.captureFree(control); temporary?.delete() }
                working = false
            }
        }
    }
}

@Composable internal fun DocumentRequests(host: CanvasHost) {
    val state = host.snapshot?.objectOrNull("state") ?: return
    val file = state.getJSONObject("document_file")
    val request = state.array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type") == "document" }
    val options = host.snapshot!!.getJSONObject("document_options")
    LaunchedEffect(host.snapshot?.optBoolean("brush_ready")) {
        if (host.snapshot?.optBoolean("brush_ready") == true) host.recovery.start()
    }
    val recovery = host.recovery
    if (recovery.candidate != null) AlertDialog(
        onDismissRequest = { recovery.dismiss(false) },
        title = { Text("Recover drawing?") },
        text = { Text(if (recovery.working) "Preparing drawing…" else "An unsaved drawing from a closed window is available.") },
        confirmButton = { TextButton({ recovery.recover() }, enabled = !recovery.working, modifier = Modifier.testTag("recover-drawing")) { Text("Recover") } },
        dismissButton = { Row {
            TextButton({ recovery.dismiss(false) }, enabled = !recovery.working) { Text("Keep for Later") }
            TextButton({ recovery.dismiss(true) }, enabled = !recovery.working) { Text("Discard") }
        } }
    )
    var histogramOpen by remember { mutableStateOf(false) }
    val histogramRequest = state.array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type") == "histogram" }
    LaunchedEffect(histogramRequest?.getInt("id")) {
        if (histogramRequest != null) { histogramOpen = true; host.dispatch(obj("type" to "complete_request", "id" to histogramRequest.getInt("id"))) }
    }
    if (histogramOpen) HistogramWindow(host) { histogramOpen = false }
    val controller = host.documents
    if (controller.exporting) androidx.compose.ui.window.Popup(alignment = androidx.compose.ui.Alignment.BottomCenter,
        properties = androidx.compose.ui.window.PopupProperties(focusable = false)) {
        Surface(shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium, modifier = Modifier.padding(16.dp)) {
            Row(Modifier.padding(12.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Text(if (controller.publishing) "Writing image…" else if (controller.exportCancelled) "Cancelling…" else "Preparing image…")
                TextButton(controller::cancelExport, enabled = !controller.publishing && !controller.exportCancelled) { Text("Cancel") }
            }
        }
    }
    controller.profilePrompt?.let { SourceProfileDialog(it, controller::chooseProfile) }
    controller.exportRequest?.let { pending ->
        key(pending.getInt("id")) { ExportDialog(host, { controller.cancel(pending.getInt("id")) }, controller::chooseExport) }
    }
    val activity = LocalActivity.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        controller.picked(if (result.resultCode == Activity.RESULT_OK) result.data?.data else null, result.data?.flags ?: 0)
    }
    LaunchedEffect(request?.getInt("id"), file.optLong("epoch")) { controller.observe(request, file) }
    val link = state.array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type") == "open_link" }
    LaunchedEffect(link?.getInt("id")) {
        if (link != null) {
            val error = runCatching {
                val url = host.withNative { Native.query(it, obj("type" to "application_link", "link" to link.getJSONObject("kind").getString("link")).toString()) }
                activity?.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(org.json.JSONTokener(url).nextValue() as String)))
            }.exceptionOrNull()?.message
            host.dispatch(obj("type" to "complete_request", "id" to link.getInt("id"), "error" to error))
        }
    }
    val picker = controller.picker
    LaunchedEffect(picker) {
        if (picker != null && !picker.launched) {
            picker.launched = true
            val document = picker.request.getJSONObject("kind").getJSONObject("request")
            val opening = document.getString("type") == "open"
            val intent = Intent(if (opening) Intent.ACTION_OPEN_DOCUMENT else Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = if (opening) "*/*" else if (document.getString("type") == "export") controller.exportMime() else "application/octet-stream"
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                if (!opening) putExtra(Intent.EXTRA_TITLE, document.getString("name"))
            }
            try { launcher.launch(intent) } catch (e: Exception) { controller.pickerFailed(e) }
        }
    }
    LaunchedEffect(file.optBoolean("close_ready")) { if (file.optBoolean("close_ready")) { host.recovery.retire(); activity?.finish() } }
    // Registered before workspace/popup handlers, which get first refusal.
    BackHandler { host.invoke("close_document") }
    state.optString("host_error").takeIf { it.isNotEmpty() && it != "null" }?.let { message ->
        var dismissed by remember(message) { mutableStateOf(false) }
        if (!dismissed) AlertDialog(onDismissRequest = { dismissed = true }, title = { Text("Could not complete action") }, text = { Text(message) },
            confirmButton = { TextButton({ dismissed = true }) { Text("OK") } })
    }
    if (request != null && !controller.working) {
        val id = request.getInt("id")
        val document = request.getJSONObject("kind").getJSONObject("request")
        when (document.getString("type")) {
            "change_color", "color_history" -> if (!DocumentController.nativeFileJobsForTest) key(id) { DocumentColorDialog(host, request) }
            "new" -> key(id) {
                NewDrawingDialog(host, options, { controller.cancel(id) }) { choices -> controller.create(request, choices) }
            }
            "confirm_close" -> AlertDialog(onDismissRequest = { controller.close(id, "cancel") }, title = { Text(document.getString("title")) },
                text = { Text(options.getString("unsaved_description")) }, confirmButton = {
                    TextButton({ controller.close(id, "save") }, Modifier.testTag("document-close-save")) { Text("Save") }
                }, dismissButton = {
                    Row {
                        TextButton({ controller.close(id, "cancel") }, Modifier.testTag("document-close-cancel")) { Text("Cancel") }
                        TextButton({ controller.close(id, "discard") }, Modifier.testTag("document-close-discard")) { Text(options.getString("discard_label")) }
                    }
                })
        }
    }
}
