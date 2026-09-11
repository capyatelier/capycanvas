package art.capycanvas

import android.app.Application
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.graphics.Bitmap
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
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

internal data class DocumentPicker(val request: JSONObject, val epoch: Long, val revision: Long, var launched: Boolean = false)

/** SAF owns locations; the shared session owns dirty checkpoints and close policy. */
internal class DocumentController(private val host: CanvasHost, private val application: Application) {
    var picker by mutableStateOf<DocumentPicker?>(null)
        private set
    var working by mutableStateOf(false)
        private set
    private var activeId: Int? = null
    private var approval = 0L to 0L
    fun observe(request: JSONObject?, file: JSONObject) {
        val id = request?.getInt("id")
        if (id == activeId) return
        activeId = id
        approval = file.optLong("epoch") to file.optLong("revision")
        if (request == null) return
        val document = request.getJSONObject("kind").getJSONObject("request")
        when (document.getString("type")) {
            "open", "export" -> picker = DocumentPicker(request, approval.first, approval.second)
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
    fun create(request: JSONObject, width: Int, height: Int) = transfer(request, null, approval, width, height)
    fun cancel(id: Int) = complete(id, false)
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
    private fun transfer(request: JSONObject, uri: Uri?, approved: Pair<Long, Long>, width: Int = 0, height: Int = 0) {
        if (working) return
        working = true
        host.viewModelScope.launch {
            val id = request.getInt("id")
            val document = request.getJSONObject("kind").getJSONObject("request")
            val kind = document.getString("type")
            var task = 0L
            var temporary: File? = null
            try {
                val location = if (uri == null) null else withContext(Dispatchers.IO) {
                    val name = application.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    } ?: uri.lastPathSegment ?: document.optString("name", "Drawing.capy")
                    obj("uri" to uri.toString(), "name" to name)
                }
                if (kind == "export") {
                    val data = withTimeout(30_000) {
                        var image: Array<Any>? = null
                        while (image == null) {
                            image = host.withNative { Native.documentPixels(it, id) }
                            if (image == null) { host.documentChanged(); delay(16) }
                        }
                        image
                    }
                    withContext(Dispatchers.IO) {
                        val size = JSONArray(data[0] as String)
                        val bitmap = Bitmap.createBitmap(size.getInt(0), size.getInt(1), Bitmap.Config.ARGB_8888)
                        try {
                            bitmap.setPremultiplied(false)
                            bitmap.copyPixelsFromBuffer(java.nio.ByteBuffer.wrap(data[1] as ByteArray))
                            application.contentResolver.openOutputStream(uri!!, "wt")?.use {
                                check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) { "PNG encoding failed" }; it.flush()
                            } ?: error("The selected file cannot be written")
                        } finally { bitmap.recycle() }
                    }
                    finish(id, true)
                } else {
                    task = host.withNative { Native.projectTask(it, id, location?.toString() ?: "null", approved.first, approved.second) }
                    if (kind == "save") {
                        // Finish encoding before opening/truncating the destination.
                        temporary = withContext(Dispatchers.IO) { File.createTempFile("capy-save-", ".capy", application.cacheDir) }
                        withContext(Dispatchers.IO) {
                            val fd = ParcelFileDescriptor.open(temporary, ParcelFileDescriptor.MODE_READ_WRITE).detachFd()
                            Native.projectWork(task, fd, 0, 0)
                            application.contentResolver.openOutputStream(uri!!, "wt")?.use { output ->
                                temporary!!.inputStream().use { it.copyTo(output) }; output.flush()
                            } ?: error("The selected file cannot be written")
                        }
                        finish(id, true)
                    } else {
                        withContext(Dispatchers.IO) {
                            val fd = if (uri == null) -1 else application.contentResolver.openFileDescriptor(uri, "r")?.detachFd() ?: error("The selected file cannot be read")
                            Native.projectWork(task, fd, width, height)
                        }
                        host.withNative { Native.projectAdopt(it, task, location?.toString() ?: "null") }
                        host.documentChanged()
                    }
                }
            } catch (e: CancellationException) {
                withContext(NonCancellable) { finish(id, false) }
                throw e
            } catch (e: Exception) {
                finish(id, false, e.message ?: "Could not complete the file operation")
            } finally {
                withContext(NonCancellable + Dispatchers.IO) { if (task != 0L) Native.projectFree(task); temporary?.delete() }
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
    val controller = host.documents
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
                type = if (opening) "*/*" else if (document.getString("type") == "export") "image/png" else "application/octet-stream"
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                if (!opening) putExtra(Intent.EXTRA_TITLE, document.getString("name"))
            }
            try { launcher.launch(intent) } catch (e: Exception) { controller.pickerFailed(e) }
        }
    }
    LaunchedEffect(file.optBoolean("close_ready")) { if (file.optBoolean("close_ready")) activity?.finish() }
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
            "new" -> key(id) {
                var width by remember { mutableStateOf(options.array("extent").getInt(0).toString()) }; var height by remember { mutableStateOf(options.array("extent").getInt(1).toString()) }
                val w = width.toIntOrNull(); val h = height.toIntOrNull()
                AlertDialog(onDismissRequest = { controller.cancel(id) }, title = { Text(options.getString("new_title")) }, text = {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        OutlinedTextField(width, { width = it }, label = { Text(options.getString("width_label")) }, modifier = Modifier.testTag("new-document-width"), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true)
                        OutlinedTextField(height, { height = it }, label = { Text(options.getString("height_label")) }, modifier = Modifier.testTag("new-document-height"), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true)
                    }
                }, confirmButton = { TextButton({ controller.create(request, w!!, h!!) }, enabled = w != null && h != null && w in 1..options.getInt("max_dimension") && h in 1..options.getInt("max_dimension"), modifier = Modifier.testTag("new-document-create")) { Text("Create") } },
                    dismissButton = { TextButton({ controller.cancel(id) }) { Text("Cancel") } })
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
