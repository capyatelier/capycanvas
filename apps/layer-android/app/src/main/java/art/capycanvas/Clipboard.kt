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
import org.json.JSONObject
import java.io.File
import java.util.UUID

/**
 * Pixel copies. The window keeps the full-depth clip; other apps read its PNG
 * through a FileProvider URI whose clip description carries the nonce, so a
 * paste of this window's own copy reads the clip instead of the PNG.
 */
internal class ClipboardController(private val host: CanvasHost, private val application: Application) {
    companion object {
        const val NONCE = "art.capycanvas.clip.nonce"
    }
    private val clipboard = application.getSystemService(ClipboardManager::class.java)
    private val directory = File(application.cacheDir, "clipboard")
    var progress by mutableStateOf<Int?>(null); private set
    var cancelling by mutableStateOf(false); private set
    private var control = 0L

    /** The copy the system clipboard still names, when it is this window's. */
    fun nonce(): String? = clipboard.primaryClipDescription?.extras?.getString(NONCE)

    init {
        directory.listFiles()?.sortedByDescending { it.lastModified() }?.drop(1)?.forEach { it.delete() }
    }

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
            try {
                task = host.withNative { Native.clipTask(it, id) }
                val nonce = UUID.randomUUID().toString()
                control = Native.captureControl()
                cancelling = false
                if (Native.clipTaskLarge(task)) progress = id
                val running = task; task = 0L
                val file = File(directory, "$nonce.png")
                clip = withContext(Dispatchers.IO) {
                    val finished = Native.clipRun(running, control, nonce)
                    directory.mkdirs()
                    Native.clipWritePng(finished, file.path)
                    finished
                }
                val uri = FileProvider.getUriForFile(application, "${application.packageName}.clipboard", file)
                val data = ClipData.newUri(application.contentResolver, "Capy Canvas", uri)
                data.description.extras = PersistableBundle().apply { putString(NONCE, nonce) }
                clipboard.setPrimaryClip(data)
                prune(nonce)
                val adopted = clip; clip = 0L
                host.withNative { Native.clipAdopt(it, id, adopted) }
                host.documentChanged()
            } catch (e: CancellationException) {
                withContext(NonCancellable) { finish(id, false, null) }; throw e
            } catch (e: Exception) {
                finish(id, false, if (cancelling) null else e.message ?: host.bootstrap!!.getString("action_failed"))
            } finally {
                withContext(NonCancellable + Dispatchers.IO) {
                    if (task != 0L) Native.clipTaskFree(task)
                    if (clip != 0L) Native.clipFree(clip)
                }
                if (control != 0L) Native.captureFree(control)
                control = 0L; progress = null; cancelling = false
            }
        }
    }

    /** Paste this window's copy, or return false for another app's image. */
    suspend fun paste(request: JSONObject): Boolean {
        val nonce = nonce() ?: return false
        if (host.withNative { Native.clipNonce(it) } != nonce) return false
        try { host.withNative { Native.pasteClip(it, request.getInt("id")) }; host.documentChanged() }
        catch (e: Exception) { finish(request.getInt("id"), false, e.message ?: host.bootstrap!!.getString("action_failed")) }
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
