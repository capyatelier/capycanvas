package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import kotlinx.coroutines.*
import org.json.JSONObject
import kotlin.math.ln

@Composable internal fun HistogramWindow(host: CanvasHost, onClose: () -> Unit) {
    val scope = rememberCoroutineScope()
    var result by remember { mutableStateOf<JSONObject?>(null) }
    var status by remember { mutableStateOf("Preparing inspection…") }
    var attempted by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var automatic by remember { mutableStateOf(true) }
    var logarithmic by remember { mutableStateOf(false) }
    var channel by remember { mutableStateOf("0") }
    var cancel by remember { mutableStateOf(0L) }
    val file = host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")
    val key = "${file?.optLong("epoch")}:${file?.optLong("revision")}"
    fun refresh() {
        if (busy) return
        attempted = key; busy = true; status = "Updating · complete composite at full resolution"
        scope.launch {
            val control = Native.captureControl(); cancel = control
            try {
                // Always consume an allocated job, including cancellation while
                // waiting for its owner to return the newly allocated handle.
                val next = withContext(NonCancellable) {
                    val task = host.withNative { Native.inspectionTask(it, control) }
                    withContext(Dispatchers.IO) { JSONObject(Native.inspectionHistogram(task)) }
                }
                ensureActive(); result = next; status = if (next.isNull("sampled_time")) "Current committed drawing" else "Animated effects · snapshot at ${"%.2f".format(next.getDouble("sampled_time"))} s"
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { status = e.message ?: "Could not inspect the drawing" }
            finally { cancel = 0; Native.captureFree(control); busy = false }
        }
    }
    DisposableEffect(Unit) { onDispose { if (cancel != 0L) Native.captureCancel(cancel) } }
    LaunchedEffect(key) { if (cancel != 0L) Native.captureCancel(cancel) }
    LaunchedEffect(key, automatic, busy) {
        val current = result?.let { "${it.getLong("epoch")}:${it.getLong("revision")}" }
        if (automatic && !busy && current != key && attempted != key) { delay(300); refresh() }
    }
    Popup(alignment = Alignment.TopEnd, offset = IntOffset(-24, 120), properties = PopupProperties(focusable = false), onDismissRequest = onClose) {
        Surface(shadowElevation = 8.dp, tonalElevation = 4.dp, shape = MaterialTheme.shapes.medium) {
            Column(Modifier.width(380.dp).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { Text("Histogram", style = MaterialTheme.typography.titleMedium); TextButton(onClose) { Text("Close") } }
                ColorChoice("Channel", listOf("0" to "RGB", "1" to "Red", "2" to "Green", "3" to "Blue", "4" to "Luminance"), channel) { channel = it }
                Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(logarithmic, { logarithmic = it }); Text("Log scale"); Checkbox(automatic, { automatic = it }); Text("Auto update") }
                val histogram = result?.getJSONObject("histogram")
                val indices = if (channel == "0") listOf(0, 1, 2) else listOf(channel.toInt()-1)
                val colors = listOf(Color(0x88ed7474), Color(0x8869cf92), Color(0x8873a7f5), Color(0xccaaaaaa))
                Canvas(Modifier.fillMaxWidth().height(140.dp)) {
                    if (histogram != null) {
                        val channels = histogram.getJSONArray("channels")
                        fun value(channel: Int, x: Int): Double { val n = channels.getJSONObject(channel).getJSONArray("bins").getDouble(x); return if (logarithmic) ln(1+n) else n }
                        val maximum = indices.maxOf { i -> (0..255).maxOf { value(i, it) } }.coerceAtLeast(1.0)
                        for (i in indices) {
                            val path = Path().apply { moveTo(0f, size.height); for (x in 0..255) lineTo(x/255f*size.width, size.height-(value(i,x)/maximum*size.height).toFloat()); lineTo(size.width, size.height); close() }
                            drawPath(path, colors[i])
                        }
                    }
                }
                if (histogram != null) {
                    val color = histogram.getJSONObject("color")
                    Text("${color.getString("space")} · ${if(color.getString("depth")=="U16")16 else 8}-bit · ${histogram.getLong("pixels")} nontransparent pixels")
                    for (i in indices) {
                        val c = histogram.getJSONArray("channels").getJSONObject(i)
                        Text("${listOf("R","G","B","Y")[i]}: below 0 ${c.getLong("below")}, above 1 ${c.getLong("above")} · black ${c.getLong("black")}, white ${c.getLong("white")}", style = MaterialTheme.typography.bodySmall)
                    }
                }
                Text(if (result != null && "${result!!.getLong("epoch")}:${result!!.getLong("revision")}" != key && !busy) "Drawing changed · showing previous inspection" else status)
                Text("Encoded document RGB · linear luminance Y. Includes visible paper; excludes transparent pixels and display overlays.", style = MaterialTheme.typography.bodySmall)
                TextButton({ refresh() }, enabled = !busy) { Text("Refresh") }
            }
        }
    }
}
