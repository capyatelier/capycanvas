package art.capycanvas

import android.app.Application
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.provider.DocumentsContract
import android.system.Os
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.Image
import androidx.compose.ui.graphics.asImageBitmap
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
import org.json.JSONTokener
import java.io.File

private fun exportFileType(format: String?) = when (format) {
    "Exr" -> "image/x-exr" to listOf("exr")
    "Tiff" -> "image/tiff" to listOf("tif", "tiff")
    "Jpeg", "JpegHdr", "JpegHdrMapped" -> "image/jpeg" to listOf("jpg", "jpeg")
    "AvifHdr", "AvifHdrMapped" -> "image/avif" to listOf("avif")
    "Webp" -> "image/webp" to listOf("webp")
    else -> "image/png" to listOf("png")
}

internal data class DocumentPicker(val request: JSONObject, val epoch: Long, val revision: Long, var launched: Boolean = false)

/** SAF owns locations; the shared session owns dirty checkpoints and close policy. */
internal class DocumentController(private val host: CanvasHost, private val application: Application) {
    private val actionFailed get() = host.bootstrap!!.getString("action_failed")
    private suspend fun deliveryMessage(type: String, vararg arguments: Pair<String, Any?>): String =
        JSONTokener(host.withNative { Native.query(it, obj("type" to "document_delivery_message", "message" to obj("type" to type, *arguments)).toString()) }).nextValue() as String
    val images = ImageImportController(host, application)
    val clipboard = ClipboardController(host, application)
    companion object {
        // JNI integration tests own private file descriptors. SAF UI has a
        // separate end-to-end test and must not race those native job owners.
        @Volatile internal var nativeFileJobsForTest = false
    }
    var picker by mutableStateOf<DocumentPicker?>(null)
        private set
    var lookupLocation:Uri?=null
        private set
    var working by mutableStateOf(false)
        private set
    var exportRequest by mutableStateOf<JSONObject?>(null)
        private set
    private var exportRecipe: JSONObject? = null
    private var exportDestination = 0
    var profilePrompt by mutableStateOf<JSONObject?>(null)
        private set
    var packagePrompt by mutableStateOf<JSONObject?>(null)
        private set
    var packagePreview by mutableStateOf<android.graphics.Bitmap?>(null)
        private set
    var packageCopying by mutableStateOf(false)
        private set
    private var packageDecision: CompletableDeferred<Pair<Uri,Boolean>?>? = null
    fun choosePackageCopy(uri: Uri?, preview: Boolean = false) { packageDecision?.complete(uri?.let {it to preview}) }
    suspend fun showPackage(task: Long, original: Uri?, beforeShow: () -> Unit = {}): Boolean {
        val summary = withContext(Dispatchers.IO) { Native.projectPackagePrompt(task) }
        if (summary == "null") return false
        val preview = withContext(Dispatchers.IO) {
            val bytes = Native.projectPackagePreview(task)
            if (bytes.isEmpty()) null else android.graphics.BitmapFactory.decodeByteArray(bytes,0,bytes.size)
        }
        val decision = CompletableDeferred<Pair<Uri,Boolean>?>()
        packageDecision = decision
        exportCancelled = false
        beforeShow()
        packagePreview = preview
        packagePrompt = JSONObject(summary)
        var spool: File? = null
        try {
            val (uri, previewOnly) = decision.await() ?: return true
            packageCopying = true
            if (previewOnly && original != null && withContext(Dispatchers.IO) { sameDestination(original, uri) }) {
                error(JSONObject(summary).getString("destination_error"))
            }
            spool = withContext(Dispatchers.IO) {host.storage.temporaryFile("capy-package-",if(previewOnly)".png" else ".capy")}
            withContext(Dispatchers.IO) {
                Native.projectPackageWrite(task,ParcelFileDescriptor.open(spool,ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),previewOnly)
                if (exportCancelled) throw CancellationException("Drawing copy cancelled")
                application.contentResolver.openOutputStream(uri,"wt")?.use {out ->
                    spool!!.inputStream().use {it.copyTo(out)};out.flush()
                } ?: error("The selected file cannot be written")
            }
        } finally {
            packagePrompt = null; packagePreview = null; packageDecision = null; packageCopying = false
            withContext(NonCancellable + Dispatchers.IO) {spool?.delete()}
        }
        return true
    }
    fun observeDestination(uri: String?, task: Long = 0): String = uri?.let {
        runCatching {application.contentResolver.openFileDescriptor(Uri.parse(it),"r")?.use {fd -> if(task == 0L)Native.sessionFingerprint(fd.detachFd()) else Native.sessionObserve(task,fd.detachFd())}}.getOrNull()
    } ?: "null"
    private fun sameDestination(original: Uri, destination: Uri): Boolean {
        if (original.normalizeScheme() == destination.normalizeScheme()) return true
        if (original.scheme == "file" && destination.scheme == "file") {
            return runCatching { java.nio.file.Files.isSameFile(File(original.path!!).toPath(), File(destination.path!!).toPath()) }.getOrDefault(false)
        }
        if (original.authority == destination.authority &&
            DocumentsContract.isDocumentUri(application, original) && DocumentsContract.isDocumentUri(application, destination) &&
            DocumentsContract.getDocumentId(original) == DocumentsContract.getDocumentId(destination)) return true
        return runCatching {
            application.contentResolver.openFileDescriptor(original, "r")?.use { source ->
                application.contentResolver.openFileDescriptor(destination, "r")?.use { target ->
                    val a = Os.fstat(source.fileDescriptor); val b = Os.fstat(target.fileDescriptor)
                    a.st_dev == b.st_dev && a.st_ino == b.st_ino
                } ?: false
            } ?: false
        }.getOrDefault(false)
    }
    private var profileDecision: CompletableDeferred<JSONObject?>? = null
    fun chooseProfile(value: JSONObject?) { profileDecision?.complete(value); profilePrompt = null }
    var opening by mutableStateOf(false)
        private set
    var exporting by mutableStateOf(false)
        private set
    var publishing by mutableStateOf(false)
        private set
    var exportCancelled by mutableStateOf(false)
        private set
    private var exportControl = 0L
    fun cancelExport() {
        if ((exporting || opening) && !publishing) { exportCancelled = true; profileDecision?.complete(null); packageDecision?.complete(null); if (exportControl != 0L) Native.captureCancel(exportControl) }
    }
    private var activeId: Pair<Long, Int>? = null
    private val queuedOpen = java.util.ArrayDeque<Uri>()
    private var batchRelease:(()->Unit)?=null
    fun openUris(uris:List<Uri>,flags:Int=0,release:(()->Unit)?=null):Boolean {
        if(uris.isEmpty()||picker!=null){release?.invoke();return false}
        val pending=working||!queuedOpen.isEmpty()
        queuedOpen.addAll(uris)
        val previous=batchRelease
        batchRelease=when { previous==null->release;release==null->previous;else->{ {previous();release()} } }
        for(uri in uris)if(flags and Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION!=0)try{application.contentResolver.takePersistableUriPermission(uri,flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION))}catch(_:SecurityException){}
        if(!pending)host.viewModelScope.launch { requestQueuedOpen() }
        return true
    }
    private suspend fun requestQueuedOpen() {
        try {
            withTimeout(120_000) {
                while (host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") && !it.optBoolean("owner_lost") } != true ||
                    host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.any { it.getString("id") == "open_document" && it.getBoolean("enabled") } != true) delay(50)
            }
            host.withNative { Native.dispatch(it, obj("type" to "invoke", "command" to "open_document").toString()) }
            host.documentChanged()
        } catch (e: Exception) {
            queuedOpen.clear(); batchRelease?.invoke(); batchRelease = null
            host.reportActionError(e.message ?: actionFailed)
        }
    }
    fun acceptsDrop(event:android.view.DragEvent)=event.localState==null&&!working&&picker==null&&!host.drawingTabs.switching&&event.clipDescription?.let{it.hasMimeType("image/*")||it.hasMimeType("application/octet-stream")||it.hasMimeType("application/x-capy")||it.hasMimeType("text/uri-list")}==true
    fun drop(activity:Activity,event:android.view.DragEvent):Boolean {
        if(!acceptsDrop(event))return false
        val permission=activity.requestDragAndDropPermissions(event)
        val uris=event.clipData?.let{clip->(0 until clip.itemCount).mapNotNull{clip.getItemAt(it).uri}}.orEmpty()
        return openUris(uris,release={permission?.release()})
    }
    private var approval = 0L to 0L
    fun observe(request: JSONObject?, file: JSONObject) {
        if ((BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) && nativeFileJobsForTest) return
        val id = request?.getInt("id")
        val key = id?.let { file.optLong("epoch") to it }
        if (key == activeId) return
        activeId = key
        approval = file.optLong("epoch") to file.optLong("revision")
        if (request == null) return
        val document = request.getJSONObject("kind").getJSONObject("request")
        when (document.getString("type")) {
            "open" -> if (queuedOpen.isEmpty()) picker = DocumentPicker(request, approval.first, approval.second) else transfer(request, queuedOpen.removeFirst(), approval)
            "import_lookup" -> picker = DocumentPicker(request, approval.first, approval.second)
            "place" -> images.start(request, false)
            "paste" -> host.viewModelScope.launch { if (!clipboard.paste(request)) images.start(request, true) }
            "copy" -> clipboard.copy(request)
            "export" -> { exportRecipe = null;host.viewModelScope.launch {
                try {
                    if(host.proof.hasPending()) {
                        // No export capture has begun. Commit the panel draft
                        // while idle, then request options at its new revision.
                        val epoch=file.optLong("epoch")
                        finish(id!!,false)
                        try { host.proof.finishPending() } catch(e:Exception) {host.reportActionError(e.message?:actionFailed);return@launch}
                        if(host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch")==epoch)host.invoke(if(document.objectOrNull("repeat")!=null)"export_again" else "export_document")
                    } else {
                        val repeat=document.objectOrNull("repeat")
                        if(repeat==null)exportRequest=request else {
                            exportRecipe=repeat.getJSONObject("recipe")
                            val approved=approval
                            val uri=Uri.parse(repeat.getJSONObject("location").getString("uri"))
                            val writable=withContext(Dispatchers.IO) {runCatching {application.contentResolver.openFileDescriptor(uri,"rw")?.use {true} ?: false}.getOrDefault(false)}
                            if(writable)transfer(request,uri,approved) else picker=DocumentPicker(request,approved.first,approved.second)
                        }
                    }
                }
                catch(e:Exception){complete(id!!,false,e.message?:actionFailed)}
            } }
            "save" -> document.objectOrNull("location")?.let { transfer(request, Uri.parse(it.getString("uri")), approval) }
                ?: run { picker = DocumentPicker(request, approval.first, approval.second) }
        }
    }
    fun picked(uris: List<Uri>?, flags: Int = 0) {
        val pending = picker ?: return
        val isOpen = pending.request.getJSONObject("kind").getJSONObject("request").getString("type") == "open"
        if (isOpen && uris != null) queuedOpen.addAll(uris.drop(1))
        for (uri in uris.orEmpty()) {
            val grant=flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            if(grant!=0)try{application.contentResolver.takePersistableUriPermission(uri,grant)}catch(_:SecurityException){}
        }
        picked(uris?.firstOrNull(), flags)
    }
    fun picked(uri: Uri?, flags: Int = 0) {
        val pending = picker ?: return
        picker = null
        if (uri == null) { complete(pending.request.getInt("id"), false); return }
        if(pending.request.getJSONObject("kind").getJSONObject("request").getString("type")=="import_lookup")lookupLocation=uri
        val grant = flags and (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        if (grant != 0) try { application.contentResolver.takePersistableUriPermission(uri, grant) } catch (_: SecurityException) { /* Some providers grant access only for this session. */ }
        transfer(pending.request, uri, pending.epoch to pending.revision)
    }
    fun pickerFailed(error: Exception) {
        val id = picker?.request?.getInt("id") ?: return
        picker = null; complete(id, false, error.message ?: actionFailed)
    }
    fun create(request: JSONObject, options: JSONObject) = transfer(request, null, approval, options.getJSONArray("extent").getInt(0), options.getJSONArray("extent").getInt(1), options)
    fun chooseExport(recipe: JSONObject, destination: Int) {
        val request = exportRequest ?: return
        exportRecipe = recipe; exportDestination = destination; exportRequest = null
        val document = request.getJSONObject("kind").getJSONObject("request")
        val extension = exportFileType(recipe.getString("format")).second.first()
        document.put("name", document.getString("name").substringBeforeLast('.') + "." + extension)
        picker = DocumentPicker(request, approval.first, approval.second)
    }
    fun exportMime() = exportFileType(exportRecipe?.getString("format")).first
    fun cancel(id: Int) { exportRequest = null; complete(id, false) }
    fun close(id: Int, decision: String) {
        if(decision=="cancel")host.drawingTabs.cancelClose()
        host.viewModelScope.launch {
            try { host.withNative { Native.documentClose(it, id, JSONObject.quote(decision)) }; host.documentChanged() }
            catch (e: Exception) { host.reportActionError(e.message ?: actionFailed) }
        }
    }
    private fun complete(id: Int, success: Boolean, message: String? = null) {
        host.viewModelScope.launch { finish(id, success, message) }
    }
    private suspend fun finish(id: Int, success: Boolean, message: String? = null) {
        if(!success)host.drawingTabs.cancelClose()
        try { host.withNative { Native.documentComplete(it, id, success, message?.let(JSONObject::quote) ?: "null") }; host.documentChanged() }
        catch (e: Exception) { host.reportActionError(message ?: e.message ?: actionFailed) }
    }
    private fun transfer(request: JSONObject, uri: Uri?, approved: Pair<Long, Long>, width: Int = 0, height: Int = 0, options: JSONObject? = null) {
        if (working) return
        working = true
        host.viewModelScope.launch {
            val id = request.getInt("id")
            val document = request.getJSONObject("kind").getJSONObject("request")
            val kind = document.getString("type")
            var transitioning=false
            var task = 0L
            var control = 0L
            var temporary: File? = null
            try {
                val location = if (uri == null) null else withContext(Dispatchers.IO) {
                    val name = application.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    } ?: uri.lastPathSegment ?: document.optString("name", host.catalog.getJSONObject("document_delivery_copy").getString("untitled") + ".capy")
                    obj("uri" to uri.toString(), "name" to name)
                }
                if (kind == "export") {
                    val master = host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.objectOrNull("location")?.optString("uri")
                    check(master==null||!withContext(Dispatchers.IO) {sameDestination(Uri.parse(master),uri!!)}) { host.catalog.getJSONObject("document_delivery_copy").getString("separate_copy") }
                    val extensions = exportFileType(exportRecipe?.getString("format")).second
                    if (location!!.getString("name").substringAfterLast('.').lowercase() !in extensions) error(deliveryMessage("export_extension", "extension" to extensions.first()))
                }
                if(kind=="export")host.withNative {Native.dispatch(it,obj("type" to "prepare_export","id" to id,"owner" to document.getLong("owner"),"recipe" to exportRecipe,"location" to location).toString())}
                if(kind=="open"||kind=="new")host.drawingTabs.trim()
                if (kind in listOf("export", "open", "new")) { control = Native.captureControl(); exportControl = control; exportCancelled = false; publishing = false; exporting = kind == "export"; opening = !exporting }
                if (kind == "export") withTimeout(30_000) {
                    while (task == 0L) {
                        task = host.withNative { Native.projectExportTask(it, id, System.nanoTime(), control) }
                        if (task == 0L) { host.documentChanged(); delay(16) }
                    }
                } else task = host.withNative { Native.projectTask(it, id, location?.toString() ?: "null", approved.first, approved.second) }
                if (opening) Native.projectOpenControl(task, control)
                if (kind == "save" || kind == "export") {
                    val expectation = if(kind == "save") host.withNative { Native.sessionDestination(it) }.takeUnless { it == "null" }?.let(::JSONObject) else null
                    // Finish encoding before opening/truncating the destination.
                    temporary = withContext(Dispatchers.IO) { host.storage.temporaryFile("capy-save-", if (kind == "export") ".png" else ".capy") }
                    withContext(Dispatchers.IO) {
                        val fd = ParcelFileDescriptor.open(temporary, ParcelFileDescriptor.MODE_READ_WRITE).detachFd()
                        if (kind == "export") exportRecipe?.let { Native.projectExportOptions(task, it.toString()) }
                        Native.projectWork(task, fd, 0, 0)
                    }
                    if (kind == "export" && exportCancelled) { finish(id, false); return@launch }
                    if (kind == "export") publishing = true
                    val fingerprint = if(kind == "save")withContext(Dispatchers.IO) {
                        Native.sessionFingerprint(ParcelFileDescriptor.open(temporary, ParcelFileDescriptor.MODE_READ_ONLY).detachFd())
                    } else null
                    if(kind == "save")check(host.recovery.checkpointForSave()) {actionFailed}
                    withContext(Dispatchers.IO) {
                        if(expectation?.getJSONObject("location")?.getString("uri") == uri.toString()) {
                            val observed = observeDestination(uri.toString())
                            check(Native.sessionDestinationMatches(expectation.toString(), observed)) { host.catalog.getJSONObject("document_delivery_copy").getString("destination_changed") }
                        }
                        application.contentResolver.openOutputStream(uri!!, "wt")?.use { output ->
                            temporary!!.inputStream().use { it.copyTo(output) }; output.flush()
                        } ?: error(actionFailed)
                    }
                    if(kind == "save") {
                        host.withNative { Native.sessionCompleteSave(it, id, location!!.toString(), fingerprint!!) }
                        host.documentChanged()
                    } else finish(id, true)
                    if(kind=="export"&&document.objectOrNull("repeat")==null)try {
                        val color=JSONObject(host.withNative{Native.query(it,obj("type" to "document_color").toString())})
                        ColorPreferencesStore.presets(application,color,obj("type" to "remember","index" to if(exportDestination<4)exportDestination else 3,"recipe" to exportRecipe))
                    }catch(e:Exception){host.reportActionError(deliveryMessage("export_preferences", "detail" to (e.message ?: actionFailed)))}
                    if (kind == "save") host.recovery.capture()
                } else {
                    val sourceFingerprint = if(kind == "open" && uri != null)withContext(Dispatchers.IO) {
                        runCatching { application.contentResolver.openFileDescriptor(uri, "r")?.let { Native.sessionFingerprint(it.detachFd()) } }.getOrNull()
                    } else null
                    withContext(Dispatchers.IO) {
                        val fd = if (uri == null) -1 else application.contentResolver.openFileDescriptor(uri, "r")?.detachFd() ?: error(actionFailed)
                        if (options != null) Native.projectOptions(task, options.toString())
                        Native.projectWork(task, fd, width, height)
                    }
                    if (exportCancelled) { finish(id, false); return@launch }
                    if (showPackage(task, uri)) { finish(id, false); return@launch }
                    val prompt = withContext(Dispatchers.IO) { Native.projectProfilePrompt(task) }
                    if (prompt != "null") {
                        val decision = CompletableDeferred<JSONObject?>(); profileDecision = decision; profilePrompt = JSONObject(prompt)
                        val profile = try { decision.await() } finally { profileDecision = null; profilePrompt = null }
                        if (profile == null) { queuedOpen.clear(); finish(id, false); return@launch }
                        withContext(Dispatchers.IO) { Native.projectAssumeProfile(task, profile.toString()); Native.projectWork(task, -1, 0, 0) }
                    }
                    if (exportCancelled) { finish(id, false); return@launch }
                    if(kind=="new"||kind=="open") { host.drawingTabs.beforeAdopt(task); transitioning=true }
                    host.withNative { Native.projectAdopt(it, task, location?.toString() ?: "null") }
                    val adoptedDestination = host.withNative { Native.sessionDestination(it) }.takeUnless { it == "null" }?.let(::JSONObject)
                    if(sourceFingerprint != null && location != null && adoptedDestination?.getJSONObject("location")?.getString("uri") == uri.toString()) {
                        host.withNative { Native.sessionRecordDestination(it, location.toString(), sourceFingerprint) }
                    }
                    host.documentChanged()
                }
            } catch (e: CancellationException) {
                withContext(NonCancellable) { finish(id, false) }
                throw e
            } catch (e: Exception) {
                finish(id, false, if (exportCancelled) null else e.message ?: actionFailed)
            } finally {
                exportControl = 0; exporting = false; opening = false; publishing = false
                withContext(NonCancellable + Dispatchers.IO) { if (task != 0L) Native.projectFree(task); if (control != 0L) Native.captureFree(control); temporary?.delete() }
                if(transitioning)host.drawingTabs.afterAdopt()
                working = false
                if(exportCancelled)queuedOpen.clear()
                if(!queuedOpen.isEmpty())requestQueuedOpen() else {batchRelease?.invoke();batchRelease=null}
            }
        }
    }
}

@Composable internal fun DocumentRequests(host: CanvasHost) {
    val state = host.snapshot?.objectOrNull("state") ?: return
    ProofRequests(host,state)
    val file = state.getJSONObject("document_file")
    val request = state.array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type") == "document" }
    val options = host.snapshot!!.getJSONObject("document_options")
    LaunchedEffect(host.snapshot?.optBoolean("brush_ready")) {
        if (host.snapshot?.optBoolean("brush_ready") == true) host.recovery.start()
    }
    val recovery = host.recovery
    val recoveryCopy = host.bootstrap!!.getJSONObject("recovery")
    val common = host.bootstrap!!.getJSONObject("common")
    if (recovery.candidate != null) AlertDialog(
        onDismissRequest = { recovery.dismiss() },
        title = { Text(recoveryCopy.getString("title")) },
        text = { Text(recoveryCopy.getString(if (recovery.working) "restoring" else "explanation")) },
        confirmButton = { TextButton({ recovery.recover() }, enabled = !recovery.working, modifier = Modifier.testTag("recover-drawing")) { Text(recoveryCopy.getString("restore")) } },
        dismissButton = {
            TextButton({ recovery.dismiss() }, enabled = !recovery.working) { Text(recoveryCopy.getString("later")) }
            TextButton({ recovery.discard() }, enabled = !recovery.working, modifier = Modifier.testTag("discard-recovery"),
                colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text(recoveryCopy.getString("discard")) }
        }
    )
    val controller = host.documents
    if ((controller.exporting || controller.opening) && controller.packagePrompt == null) androidx.compose.ui.window.Popup(alignment = androidx.compose.ui.Alignment.BottomCenter,
        properties = androidx.compose.ui.window.PopupProperties(focusable = false)) {
        Surface(shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium, modifier = Modifier.padding(16.dp)) {
            Row(Modifier.padding(12.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Text(if (controller.publishing) host.catalog.getJSONObject("export_copy").getString("writing_image") else if (controller.exportCancelled) host.catalog.getJSONObject("document_delivery_copy").getString("cancelling") else if (controller.opening) host.bootstrap!!.getString("preparing_document") else host.catalog.getJSONObject("export_copy").getString("preparing"))
                TextButton(controller::cancelExport, enabled = !controller.publishing && !controller.exportCancelled) { Text(common.getString("cancel")) }
            }
        }
    }
    val packageCopy = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/x-capycanvas")) {uri -> controller.choosePackageCopy(uri)}
    val packagePreviewExport = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("image/png")) {uri -> controller.choosePackageCopy(uri,true)}
    controller.packagePrompt?.let {summary ->
        AlertDialog(onDismissRequest = {if(!controller.packageCopying)controller.choosePackageCopy(null)},
            title = {Text(summary.getString("status"))},
            text = {Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                controller.packagePreview?.let {Image(it.asImageBitmap(),null,Modifier.fillMaxWidth().heightIn(max=320.dp))}
                Text(summary.getString("reason"))
                if(controller.packageCopying)CircularProgressIndicator(Modifier.size(20.dp),strokeWidth=2.dp)
            }},
            confirmButton = {Column {
                if(summary.getJSONObject("capabilities").optBoolean("export"))TextButton({packagePreviewExport.launch("Preview.png")},enabled=!controller.packageCopying,modifier=Modifier.testTag("export-package-preview")) {Text(summary.getString("export_preview"))}
                TextButton({packageCopy.launch("Copy.capy")},enabled=!controller.packageCopying,modifier=Modifier.testTag("copy-original-package")) {Text(summary.getString("copy_original"))}
            }},
            dismissButton = {TextButton({controller.choosePackageCopy(null)},enabled=!controller.packageCopying) {Text(summary.getString("close"))}},
            modifier = Modifier.testTag("preserved-package-preview"))
    }
    controller.profilePrompt?.let { SourceProfileDialog(host,it, controller::chooseProfile) }
    controller.exportRequest?.let { pending ->
        key(pending.getInt("id")) { ExportDialog(host, { controller.cancel(pending.getInt("id")) }, controller::chooseExport) }
    }
    val activity = LocalActivity.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (controller.images.choosing) {
            try {
                val uris = if (result.resultCode == Activity.RESULT_OK) result.data?.clipData?.imageUris(host.bootstrap!!.getString("action_failed")) ?: result.data?.data?.let(::listOf) else null
                controller.images.picked(uris, result.data?.flags ?: 0)
            } catch (e: Exception) { controller.images.cancel(); host.reportActionError(e.message ?: host.bootstrap!!.getString("action_failed")) }
        } else controller.picked(if (result.resultCode == Activity.RESULT_OK) result.data?.clipData?.let { clip -> (0 until clip.itemCount).map { clip.getItemAt(it).uri } } ?: result.data?.data?.let(::listOf) else null as List<Uri>?, result.data?.flags ?: 0)
    }
    LaunchedEffect(controller.images.choosing) {
        if (controller.images.choosing && !controller.images.pickerLaunched) {
            controller.images.pickerLaunched = true
            try { launcher.launch(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE); type = "*/*"
                putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                putExtra(Intent.EXTRA_MIME_TYPES, controller.images.mimeTypes)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
            }) } catch (e: Exception) { controller.images.cancel(); host.reportActionError(e.message ?: host.bootstrap!!.getString("action_failed")) }
        }
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
            val lookup=document.getString("type")=="import_lookup"
            val opening = lookup || document.getString("type") in listOf("open", "place")
            val intent = Intent(if (opening) Intent.ACTION_OPEN_DOCUMENT else Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = if (opening) "*/*" else if (document.getString("type") == "export") controller.exportMime() else "application/octet-stream"
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                if (opening && !lookup) putExtra(Intent.EXTRA_MIME_TYPES, controller.images.mimeTypes + "application/octet-stream")
                if(lookup) putExtra(android.provider.DocumentsContract.EXTRA_INITIAL_URI,controller.lookupLocation ?: android.provider.DocumentsContract.buildRootUri("com.android.providers.downloads.documents","downloads"))
                if (document.getString("type") == "open") putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                if (!opening) putExtra(Intent.EXTRA_TITLE, document.getString("name"))
            }
            try { launcher.launch(intent) } catch (e: Exception) { controller.pickerFailed(e) }
        }
    }
    LaunchedEffect(file.optLong("epoch"), file.optBoolean("close_ready"), host.drawingTabs.switching, controller.working) { if (file.optBoolean("close_ready") && !host.drawingTabs.switching && !controller.working && !DocumentController.nativeFileJobsForTest) host.drawingTabs.acceptClose() }
    DrawingSelector(host)
    val drawingsRequest=state.array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type")=="drawings" }
    LaunchedEffect(drawingsRequest?.getInt("id")) { if(drawingsRequest!=null) { host.drawingTabs.selector=true; host.dispatch(obj("type" to "complete_request","id" to drawingsRequest.getInt("id"))) } }
    // Registered before workspace/popup handlers, which get first refusal.
    BackHandler { host.drawingTabs.closeSelected() }
    ImportAndTransformControls(host)
    ClipboardProgress(host.documents.clipboard)
    host.hostError?.let { message ->
        AlertDialog(onDismissRequest = host::dismissHostError, title = { Text(host.bootstrap!!.getString("action_failed")) }, text = { Text(message) },
            confirmButton = { TextButton(host::dismissHostError) { Text(common.getString("ok")) } })
    }
    if (request != null && !controller.working) {
        val id = request.getInt("id")
        val document = request.getJSONObject("kind").getJSONObject("request")
        when (document.getString("type")) {
            "repair_source_profile", "rasterize_source" -> if (!DocumentController.nativeFileJobsForTest) key(id) { SourceEditDialog(host, request) }
            "properties" -> key(id) { DocumentPropertiesDialog(host) { controller.cancel(id) } }
            "change_color", "color_history" -> if (!DocumentController.nativeFileJobsForTest) key(id) { DocumentColorDialog(host, request) }
            "new" -> key(id) {
                NewDrawingDialog(host, options, { controller.cancel(id) }) { choices -> controller.create(request, choices) }
            }
            "confirm_close" -> AlertDialog(onDismissRequest = { controller.close(id, "cancel") }, title = { Text(document.getString("title")) },
                text = { Text(options.getString("unsaved_description")) }, confirmButton = {
                    TextButton({ controller.close(id, "save") }, Modifier.testTag("document-close-save")) { Text(common.getString("save")) }
                }, dismissButton = {
                    Row {
                        TextButton({ controller.close(id, "cancel") }, Modifier.testTag("document-close-cancel")) { Text(common.getString("cancel")) }
                        TextButton({ controller.close(id, "discard") }, Modifier.testTag("document-close-discard")) { Text(options.getString("discard_label")) }
                    }
                })
        }
    }
}
