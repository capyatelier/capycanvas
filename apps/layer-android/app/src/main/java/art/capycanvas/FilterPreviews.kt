package art.capycanvas

import android.graphics.Bitmap
import android.view.Choreographer
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume

internal data class FilterPreviewReply(val status: JSONObject, val header: JSONArray?, val bytes: ByteArray?)
internal data class FilterPreviewTile(val key: String, val image: ImageBitmap)

/** One native image cache/clock per editor, shared by docked and drawer views.
 * Rust owns admission, revisions, batching, cancellation and cache retention. */
internal class FilterPreviewCache(private val host: CanvasHost) {
    private val views = mutableMapOf<Any, Pair<List<String>, List<Int>>>()
    private val wake = Channel<Unit>(Channel.CONFLATED)
    private var job: Job? = null
    private var enabled = false
    private var delivery = 0L
    private var key = ""
    var request = 0L
        private set
    @Volatile var pending = false
        private set
    val images = mutableStateMapOf<String, FilterPreviewTile>()

    fun update(view: Any, ids: List<String>, size: List<Int>) {
        views[view] = ids to size
        schedule()
    }
    fun remove(view: Any) { views.remove(view); schedule() }
    fun resume() { enabled = true; schedule() }
    fun pause() { enabled = false; delivery++; schedule() }
    fun reset() { delivery++; images.clear(); key = ""; schedule() }
    private fun schedule() {
        wake.trySend(Unit)
        if (job != null) return
        job = host.viewModelScope.launch {
            try {
                do {
                    val visible = if (enabled) views.values.toList() else emptyList()
                    val ids = visible.flatMap { it.first }.distinct()
                    val size = listOf(visible.maxOfOrNull { it.second[0] } ?: 80,
                        visible.maxOfOrNull { it.second[1] } ?: 40)
                    val generation = delivery
                    val response = suspendCancellableCoroutine<FilterPreviewReply?> { continuation ->
                        host.filterPreviews(obj("type" to "filter_previews", "filters" to JSONArray(ids),
                            "cache" to obj("key" to key, "rows" to JSONArray(images.keys.toList())),
                            "size" to JSONArray(size))) { if (continuation.isActive) continuation.resume(it) }
                    }
                    var waitMs = 1000L
                    if (response != null && generation == delivery && current(response.status)) {
                        val status = response.status
                        val next = status.getString("key")
                        if (key != next) { images.clear(); key = next }
                        val retained = status.getJSONArray("retained").values().map { it.toString() }.toSet()
                        images.keys.toList().filter { it !in retained }.forEach(images::remove)
                        request = status.getLong("requests")
                        pending = status.getBoolean("pending")
                        waitMs = status.getLong("wait_ms")
                        val header = response.header; val bytes = response.bytes
                        if (header != null && bytes != null) {
                            val rows = withContext(Dispatchers.Default) { decode(next, header, bytes) }
                            // Bitmap conversion crosses a dispatcher; a replaced
                            // document or stopped surface cannot accept its reply.
                            if (generation == delivery && current(status)) images.putAll(rows)
                        }
                    } else pending = false
                    if (!enabled || views.isEmpty()) {
                        if (ids.isEmpty()) break
                        continue // Publish empty geometry before stopping.
                    }
                    if (waitMs == 0L) frame() else withTimeoutOrNull(waitMs) { wake.receive() }
                } while (isActive)
            } finally { pending = false; job = null }
        }
    }
    private fun current(status: JSONObject): Boolean = status.getLong("epoch") ==
        host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch")

    private suspend fun frame() = suspendCancellableCoroutine<Unit> { continuation ->
        val clock = Choreographer.getInstance()
        val callback = Choreographer.FrameCallback { if (continuation.isActive) continuation.resume(Unit) }
        clock.postFrameCallback(callback)
        continuation.invokeOnCancellation { clock.removeFrameCallback(callback) }
    }
    private fun decode(key: String, header: JSONArray, bytes: ByteArray): List<Pair<String, FilterPreviewTile>> {
        val ids = header.getJSONArray(3).values().map { it.toString() }
        val width = header.getInt(1); val height = header.getInt(2) / ids.size
        val pixels = IntArray(width * height)
        return ids.mapIndexed { i, id ->
            // The shared atlas is straight RGBA; Android's color-int API
            // performs premultiplication, using one scratch row per batch.
            for (p in pixels.indices) {
                val b = (i * pixels.size + p) * 4
                pixels[p] = ((bytes[b+3].toInt() and 255) shl 24) or ((bytes[b].toInt() and 255) shl 16) or
                    ((bytes[b+1].toInt() and 255) shl 8) or (bytes[b+2].toInt() and 255)
            }
            id to FilterPreviewTile(key, Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888).asImageBitmap())
        }
    }
}
