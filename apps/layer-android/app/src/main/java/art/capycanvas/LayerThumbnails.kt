package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.runtime.*
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import kotlinx.coroutines.*
import org.json.JSONArray

internal data class LayerThumbnailTarget(val key: String, val target: Long, val revision: Long)

internal class LayerThumbnails(private val host: CanvasHost) {
    private val scope = MainScope()
    private val owners = mutableMapOf<Any, List<LayerThumbnailTarget>>()
    private val pending = mutableMapOf<Long, LayerThumbnailTarget>()
    private val revisions = mutableMapOf<String, Long>()
    val images = mutableStateMapOf<String, ImageBitmap>()
    private var next = 0L
    private var loop: Job? = null
    fun update(owner: Any, targets: List<LayerThumbnailTarget>) {
        owners[owner] = targets
        if (loop?.isActive != true) loop = scope.launch {
            while (owners.isNotEmpty()) {
                val wanted = owners.values.flatten().associateBy { it.key }
                images.keys.removeAll { it !in wanted }
                revisions.keys.removeAll { it !in wanted }
                pending.entries.removeAll { it.value.key !in wanted }
                val requests = wanted.values.filter { revisions[it.key] != it.revision && pending.values.none { request -> request.key == it.key } }
                    .take((8 - pending.size).coerceAtLeast(0)).associateBy { ++next }
                val response = host.awaitQuery(obj("type" to "layer_thumbnails", "requests" to JSONArray(requests.map { (id, target) -> JSONArray(listOf(id, target.target)) })))
                if (response != null) {
                    val accepted = response.array("accepted").values().map { (it as Number).toLong() }.toSet()
                    requests.filterKeys { it in accepted }.forEach { (id, target) -> pending[id] = target }
                    for (raw in response.array("images").values()) {
                        val image = raw as JSONArray
                        val request = pending.remove(image.getLong(0)) ?: continue
                        val bitmap = withContext(Dispatchers.Default) {
                            val width = image.getInt(1); val height = image.getInt(2); val bytes = image.getJSONArray(3)
                            val pixels = IntArray(width * height) { p ->
                                (bytes.getInt(p * 4 + 3) shl 24) or (bytes.getInt(p * 4) shl 16) or (bytes.getInt(p * 4 + 1) shl 8) or bytes.getInt(p * 4 + 2)
                            }
                            Bitmap.createBitmap(pixels, width, height, Bitmap.Config.ARGB_8888).asImageBitmap()
                        }
                        if (owners.values.any { targets -> request in targets }) { images[request.key] = bitmap; revisions[request.key] = request.revision }
                    }
                }
                delay(120)
            }
            images.clear(); revisions.clear(); pending.clear()
        }
    }
    fun remove(owner: Any) { owners.remove(owner) }
    fun close() { scope.cancel(); owners.clear(); pending.clear(); images.clear(); revisions.clear() }
}

@Composable internal fun rememberLayerThumbnails(host: CanvasHost, epoch: Long, targets: () -> List<LayerThumbnailTarget>): Map<String, ImageBitmap> {
    val owner = remember { Any() }
    val current by rememberUpdatedState(targets)
    val cache = host.layerThumbnails
    LaunchedEffect(cache, epoch) {
        while (isActive) {
            cache.update(owner, current().map { it.copy(key = "$epoch:${it.key}") })
            delay(120)
        }
    }
    DisposableEffect(cache, owner) { onDispose { cache.remove(owner) } }
    return remember(cache, epoch) {
        derivedStateOf(structuralEqualityPolicy()) {
            cache.images.filterKeys { it.startsWith("$epoch:") }.mapKeys { it.key.substringAfter(':') }
        }
    }.value
}
