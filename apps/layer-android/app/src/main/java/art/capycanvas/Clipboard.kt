package art.capycanvas

import android.app.Application
import android.content.ClipData
import android.content.ClipboardManager
import android.os.PersistableBundle
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import org.json.JSONObject
import java.io.File
import java.util.UUID

/**
 * Pixel copies. The application keeps the full-depth clip; other apps read its PNG
 * through a FileProvider URI whose clip description carries the nonce, so a
 * paste of the application's own copy reads the clip instead of the PNG.
 */
internal class ClipboardController(private val host: CanvasHost, private val application: Application) {
    companion object {
        const val NONCE = "art.capycanvas.clip.nonce"
        private val publication = Mutex()
    }
    private val clipboard = application.getSystemService(ClipboardManager::class.java)
    private val directory get() = host.storage.clipboard
    var progress by mutableStateOf<Int?>(null); private set
    var cancelling by mutableStateOf(false); private set
    private var control = 0L

    /** The copy the system clipboard still names. */
    fun nonce(): String? = clipboard.primaryClipDescription?.extras?.getString(NONCE)

    /** Keep only the latest copy's file, which the system clipboard names. */
    private fun prune(keep: String) {
        directory.listFiles()?.filter { it.name != "$keep.png" }?.forEach { it.delete() }
    }

    fun cancel() {
        cancelling = true
        if (control != 0L) Native.captureCancel(control)
    }

    fun copy(request: JSONObject) {
        val id = request.getInt("id")
        host.viewModelScope.launch {
            var task = 0L
            var clip = 0L
            var capture = 0L
            var completed = false
            var published = false
            var file: File? = null
            var locked = false
            try {
                task = host.withNative { Native.clipTask(it, id) }
                val nonce = UUID.randomUUID().toString()
                capture = Native.captureControl(); control = capture
                cancelling = false
                progress = if (Native.clipTaskLarge(task)) id else null
                publication.lock(); locked = true
                val output = File(directory, "$nonce.png"); file = output
                withContext(Dispatchers.IO) {
                    val running = task; task = 0L
                    clip = Native.clipRun(running, capture, nonce)
                    directory.mkdirs()
                    Native.clipWritePng(clip, output.path)
                }
                ensureActive()
                if (cancelling) throw CancellationException("Copy cancelled")
                val uri = FileProvider.getUriForFile(application, "${application.packageName}.clipboard", output)
                val data = ClipData.newUri(application.contentResolver, host.catalog.getString("app_name"), uri)
                data.description.extras = PersistableBundle().apply { putString(NONCE, nonce) }
                clipboard.setPrimaryClip(data); published = true
                withContext(NonCancellable + Dispatchers.IO) { runCatching { prune(nonce) } }
                host.withNative {
                    val adopted = clip; clip = 0L
                    Native.clipAdopt(it, id, adopted, capture); completed = true
                }
                publication.unlock(); locked = false
                host.documentChanged()
            } catch (e: CancellationException) {
                if (!completed) withContext(NonCancellable) { finish(id, false, null) }; throw e
            } catch (e: Exception) {
                if (!completed) finish(id, false, if (cancelling) null else e.message ?: host.bootstrap!!.getString("action_failed"))
            } finally {
                if (task != 0L || clip != 0L || !published) withContext(NonCancellable + Dispatchers.IO) {
                    if (task != 0L) Native.clipTaskFree(task)
                    if (clip != 0L) Native.clipFree(clip)
                    if (!published) file?.delete()
                }
                if (capture != 0L) Native.captureFree(capture)
                if (control == capture) { control = 0L; progress = null; cancelling = false }
                if (locked) publication.unlock()
            }
        }
    }

    /** Paste the retained copy, or return false for another app's image. */
    suspend fun paste(request: JSONObject): Boolean {
        val nonce = nonce() ?: return false
        if (host.withNative { Native.clipNonce(it) } != nonce) return false
        if (nonce() != nonce) return false
        host.documents.images.start(request, true, clipNonce=nonce)
        return true
    }

    private suspend fun finish(id: Int, success: Boolean, message: String?) {
        try { host.withNative { Native.documentComplete(it, id, success, message?.let(JSONObject::quote) ?: "null") }; host.documentChanged() }
        catch (e: Exception) { host.reportActionError(e.message ?: host.bootstrap!!.getString("action_failed")) }
    }
}

@Composable internal fun ClipboardProgress(clipboard: ClipboardController) {
    val host = LocalCanvasHost.current
    val id = clipboard.progress ?: return
    var label by remember(id) { mutableStateOf("") }
    LaunchedEffect(id, host.languageTag) {
        val language = host.languageTag
        val next = host.withNative { Native.documentRequestTitle(it, id) }.orEmpty()
        if (language == host.languageTag && id == clipboard.progress) label = next
    }
    androidx.compose.ui.window.Popup(alignment = Alignment.BottomCenter) {
        Surface(Modifier.padding(12.dp).testTag("clipboard-progress"), shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium) {
            Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(if (clipboard.cancelling) host.catalog.getJSONObject("document_delivery_copy").getString("cancelling") else label)
                TextButton(clipboard::cancel, enabled = !clipboard.cancelling) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) }
            }
        }
    }
}
