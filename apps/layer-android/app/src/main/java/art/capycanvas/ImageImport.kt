package art.capycanvas

import android.app.Activity
import android.app.Application
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.view.DragEvent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

internal fun ClipData.imageUris(): List<Uri> = (0 until itemCount).map { index ->
    getItemAt(index).uri ?: error("Every item in the image batch must be a file")
}
internal data class IncomingImages(val uris: List<Uri>, val context: String, val release: () -> Unit, val finished: ((Boolean) -> Unit)? = null)

/** One coroutine owns the picker, all provider grants and one private native batch. */
internal class ImageImportController(private val host: CanvasHost, private val application: Application) {
    val formats = JSONArray(Native.photoFormats()).objects()
    val mimeTypes = formats.flatMap { f -> f.getJSONArray("mime_types").let { a -> (0 until a.length()).map(a::getString) } }.toTypedArray()
    var choosing by mutableStateOf(false); private set
    var pickerLaunched = false
    var working by mutableStateOf(false); private set
    var cancelled by mutableStateOf(false); private set
    var profilePrompt by mutableStateOf<JSONObject?>(null); private set
    private var selection: CompletableDeferred<List<Uri>?>? = null
    private var interpretation: CompletableDeferred<JSONObject?>? = null
    private var incoming: IncomingImages? = null
    private var receiving = false
    private var control = 0L
    private var providerSignal: android.os.CancellationSignal? = null
    fun chooseProfile(profile: JSONObject?) { interpretation?.complete(profile) }
    fun cancel() {
        cancelled = true
        if (control != 0L) Native.captureCancel(control)
        providerSignal?.cancel()
        selection?.complete(null); interpretation?.complete(null)
    }
    fun picked(uris: List<Uri>?, flags: Int) {
        if (flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION != 0) for (uri in uris.orEmpty()) {
            try { application.contentResolver.takePersistableUriPermission(uri, flags and Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            catch (_: SecurityException) { /* Session grants remain sufficient. */ }
        }
        selection?.complete(uris)
    }
    fun accepts(event: DragEvent): Boolean = event.localState == null && event.clipDescription?.let { description ->
        mimeTypes.any(description::hasMimeType) || description.hasMimeType("text/uri-list") || description.hasMimeType("application/octet-stream") || description.hasMimeType("application/x-capy")
    } == true && !working && !receiving && incoming == null && host.snapshot?.getJSONObject("state")?.array("commands")?.objects()
        ?.any { it.getString("id") == "import_image" && it.getBoolean("enabled") } == true

    fun drop(activity: Activity, event: DragEvent, screen: JSONObject?, destination: JSONObject?): Boolean {
        if (!accepts(event)) return false
        val permission = activity.requestDragAndDropPermissions(event)
        val uris = try { event.clipData?.imageUris().orEmpty().also { check(it.isNotEmpty()) { "No images to import" } } }
        catch (e: Exception) { permission?.release(); host.reportActionError(e.message ?: "Cannot read dropped images"); return false }
        receiving = true
        // Queue camera/identity capture immediately, before provider access.
        host.viewModelScope.launch {
            var requestId=0
            try {
                val (context, request) = host.withNative { handle ->
                    val resolved = destination?.let { d ->
                        val hint = JSONObject(Native.query(handle, obj("type" to "image_layer_drop", "target" to d.getLong("target"), "fraction" to d.getDouble("fraction")).toString()))
                        check(!hint.isNull("position")) { "Images cannot be placed at this layer boundary" }
                        obj("target" to d.getLong("target"), "position" to hint.getString("position"))
                    }
                    val captured = Native.imageImportContext(handle, screen?.toString() ?: "null", resolved?.toString() ?: "null")
                    Native.dispatch(handle, obj("type" to "invoke", "command" to "import_image").toString())
                    val request = JSONArray(Native.query(handle, obj("type" to "requests").toString())).objects().first { it.getJSONObject("kind").getString("type") == "document" }
                    captured to request
                }
                requestId=request.getInt("id");cancelled=false;providerSignal=android.os.CancellationSignal()
                // Classify encoded prefixes through shared Rust, never MIME or
                // provider filename guesses. Keep the original placement target.
                val masters=withContext(Dispatchers.IO) {uris.filter {uri->
                    val prefix=ByteArray(4);var size=0
                    val descriptor=application.contentResolver.openFileDescriptor(uri,"r",providerSignal)?:error("Dropped file cannot be read")
                    ParcelFileDescriptor.AutoCloseInputStream(descriptor).use {input->
                        val poll=android.system.StructPollfd().apply{fd=descriptor.fileDescriptor;events=android.system.OsConstants.POLLIN.toShort()}
                        while(size<4){ensureActive();check(!cancelled){"Drop cancelled"};if(android.system.Os.poll(arrayOf(poll),100)==0)continue;val read=input.read(prefix,size,4-size);if(read<0)break;size+=read}
                    }
                    Native.importSource(prefix.copyOf(size))=="\"Master\""
                }}
                val photos=uris.filterNot{it in masters}
                if(photos.isEmpty()) {
                    host.withNative{Native.documentComplete(it,request.getInt("id"),false,"null")}
                    host.documents.openUris(masters,release={permission?.release()});host.documentChanged()
                } else {
                    incoming = IncomingImages(photos,context,release={if(masters.isEmpty())permission?.release()},finished={success->
                        if(masters.isNotEmpty()){if(success)host.documents.openUris(masters,release={permission?.release()})else permission?.release()}
                    })
                    start(request, false, fromDrop=true); host.documentChanged()
                }
            } catch (e: Exception) { permission?.release();if(requestId!=0)withContext(NonCancellable){runCatching{host.withNative{Native.documentComplete(it,requestId,false,JSONObject.quote(e.message?:"Cannot drop images"))}}};host.reportActionError(e.message ?: "Cannot drop images") }
            finally { receiving = false }
        }
        return true
    }
    fun start(request: JSONObject, paste: Boolean, fromDrop:Boolean=false) {
        if (working || (receiving&&!fromDrop)) return
        working = true; cancelled = false
        val drop = incoming; incoming = null
        host.viewModelScope.launch {
            val id = request.getInt("id")
            var task = 0L
            var adopted=false
            try {
                control = Native.captureControl()
                providerSignal = android.os.CancellationSignal()
                val context = drop?.context ?: host.withNative { Native.imageImportContext(it, "null", "null") }
                task = host.withNative { Native.imageImportTask(it, id, context, control) }
                val uris = drop?.uris ?: if (paste) {
                    application.getSystemService(android.content.ClipboardManager::class.java).primaryClip?.imageUris()
                        ?: error("Copy a supported image (${formats.joinToString { it.getString("name") }}) to paste.")
                } else {
                    val decision = CompletableDeferred<List<Uri>?>(); selection = decision; pickerLaunched = false; choosing = true
                    try { decision.await() } finally { selection = null; choosing = false; pickerLaunched = false }
                }
                if (uris == null || cancelled) { finish(id, false); return@launch }
                check(uris.isNotEmpty()) { "Choose at least one image" }
                for (uri in uris) {
                    if (cancelled) { finish(id, false); return@launch }
                    withContext(Dispatchers.IO) {
                        val name = application.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null, providerSignal)?.use {
                            if (it.moveToFirst()) it.getString(0) else null
                        } ?: uri.lastPathSegment ?: "Image"
                        val descriptor = application.contentResolver.openFileDescriptor(uri, "r", providerSignal) ?: error("The selected image cannot be read")
                        descriptor.use {
                            check(it.statSize <= 512L * 1024 * 1024) { "Image file exceeds 512 MiB" }
                            val seekable = try { android.system.Os.lseek(it.fileDescriptor,0,android.system.OsConstants.SEEK_CUR); true }
                                catch (e: android.system.ErrnoException) { if (e.errno != android.system.OsConstants.ESPIPE) throw e; false }
                            if (seekable) Native.imageImportRead(task, it.detachFd(), name)
                            else {
                                // Cloud/clipboard providers may return a pipe. Retain
                                // its encoded bytes in a bounded private spool for codecs.
                                val temporary = File.createTempFile("capy-image-", ".source", application.cacheDir)
                                try {
                                    ParcelFileDescriptor.AutoCloseInputStream(it).use { input -> temporary.outputStream().use { output ->
                                        val buffer = ByteArray(64 * 1024); var total = 0L
                                        val poll = android.system.StructPollfd().apply { fd = it.fileDescriptor; events = android.system.OsConstants.POLLIN.toShort() }
                                        while (true) {
                                            check(!cancelled) { "Image import cancelled" }
                                            if (android.system.Os.poll(arrayOf(poll), 100) == 0) continue
                                            val size = input.read(buffer); if (size < 0) break
                                            total += size; check(total <= 512L * 1024 * 1024) { "Image file exceeds 512 MiB" }
                                            output.write(buffer,0,size)
                                        }
                                    } }
                                    Native.imageImportRead(task,ParcelFileDescriptor.open(temporary,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),name)
                                } finally { temporary.delete() }
                            }
                        }
                    }
                    val prompt = Native.imageImportProfilePrompt(task)
                    if (prompt != "null" && !cancelled) {
                        val decision = CompletableDeferred<JSONObject?>(); interpretation = decision; profilePrompt = JSONObject(prompt)
                        val profile = try { decision.await() } finally { interpretation = null; profilePrompt = null }
                        if (profile == null) { cancel(); finish(id, false); return@launch }
                        withContext(Dispatchers.IO) { Native.imageImportAssumeProfile(task, profile.toString()) }
                    }
                }
                if (cancelled) { finish(id, false); return@launch }
                host.withNative { Native.imageImportAdopt(it, task) }; adopted=true; host.documentChanged()
            } catch (e: CancellationException) {
                withContext(NonCancellable) { finish(id, false) }; throw e
            } catch (e: Exception) { finish(id, false, if (cancelled) null else e.message ?: "Could not import images") }
            finally {
                withContext(NonCancellable + Dispatchers.IO) { if (task != 0L) Native.imageImportFree(task) }
                if (control != 0L) Native.captureFree(control)
                control = 0; providerSignal = null; working = false; drop?.release?.invoke();drop?.finished?.invoke(adopted)
            }
        }
    }
    private suspend fun finish(id: Int, success: Boolean, message: String? = null) {
        try { host.withNative { Native.documentComplete(it, id, success, message?.let(JSONObject::quote) ?: "null") }; host.documentChanged() }
        catch (e: Exception) { host.reportActionError(e.message ?: "Could not finish image import") }
    }
}

@Composable internal fun ImagePlacementControls(host: CanvasHost, state: JSONObject) {
    val commands = state.array("commands").objects().associateBy { it.getString("id") }
    val images = host.documents.images
    if (images.working && !images.choosing) androidx.compose.ui.window.Popup(alignment = Alignment.BottomCenter) {
        Surface(Modifier.padding(12.dp), shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium) {
            Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(if (images.cancelled) "Cancelling…" else "Preparing images…")
                TextButton(images::cancel, enabled = !images.cancelled) { Text("Cancel") }
            }
        }
    }
    images.profilePrompt?.let { SourceProfileDialog(it, images::chooseProfile) }
    if (commands["placement_original_size"]?.optBoolean("enabled") != true) return
    BackHandler { host.invoke("cancel_transform") }
    androidx.compose.ui.window.Popup(alignment = Alignment.BottomCenter) {
        Surface(Modifier.padding(horizontal = 8.dp, vertical = 48.dp).testTag("image-placement-controls"), shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium) {
            FlowRow(Modifier.padding(4.dp).widthIn(max = 360.dp), horizontalArrangement = Arrangement.Center) {
                for ((id, label) in listOf("placement_original_size" to "Original Size (100%)", "cancel_transform" to "Cancel", "apply_transform" to "Apply")) {
                    TextButton({ host.invoke(id) }, enabled = commands[id]?.optBoolean("enabled") == true, modifier = Modifier.testTag(id)) { Text(label) }
                }
            }
        }
    }
}
