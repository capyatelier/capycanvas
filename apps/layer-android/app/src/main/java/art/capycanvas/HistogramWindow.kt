package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.geometry.Offset
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
    var failure by remember { mutableStateOf<String?>(null) }
    var captions by remember { mutableStateOf(JSONObject()) }
    var attempted by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var automatic by remember { mutableStateOf(true) }
    var logarithmic by remember { mutableStateOf(false) }
    var channel by remember { mutableStateOf("0") }
    var cancel by remember { mutableStateOf(0L) }
    val file = host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")
    val key = "${file?.optLong("epoch")}:${file?.optLong("revision")}"
    val copy=host.catalog.getJSONObject("native_copy").getJSONObject("color")
    val depthCopy=host.catalog.getJSONObject("document_color_copy")
    LaunchedEffect(result,host.languageTag,busy,failure) {
        val current=result
        val tag=host.languageTag
        val currentBusy=busy
        val currentFailure=failure
        val localized=host.withNative { owner ->
            fun caption(value:JSONObject)=org.json.JSONTokener(Native.query(owner,obj("type" to "native_caption","caption" to value).toString())).nextValue() as String
            val status=currentFailure ?: if(currentBusy)copy.getString("inspection_updating") else if(current==null)copy.getString("inspection_preparing") else if(current.isNull("sampled_time"))copy.getString("inspection_current") else caption(obj("type" to "inspection_sample","seconds" to current.getDouble("sampled_time")))
            val localized=obj("status" to status,"stale" to caption(obj("type" to "inspection_changed","status" to status)))
            current?.getJSONObject("histogram")?.let { histogram ->
                localized.put("pixels",caption(obj("type" to "inspection_pixels","sampled" to histogram.getLong("pixels"),"transparent" to histogram.getLong("transparent"))))
                histogram.getJSONArray("channels").objects().forEachIndexed { i,c -> localized.put("channel-$i",caption(obj("type" to "inspection_channel","below" to c.getLong("below"),"above" to c.getLong("above"),"black" to c.getLong("black"),"white" to c.getLong("white")))) }
                current.getJSONObject("axis").optJSONArray("stops")?.let { stops -> localized.put("range",caption(obj("type" to "inspection_range","start" to stops.getDouble(0),"end" to stops.getDouble(1)))) }
            }
            localized
        }
        ensureActive()
        if(tag==host.languageTag&&current===result&&currentBusy==busy&&currentFailure==failure)captions=localized
    }
    fun refresh() {
        if (busy || host.drawingTabs.switching) return
        attempted = key; busy = true; failure = null
        scope.launch {
            val control = Native.captureControl(); cancel = control
            host.drawingTabs.registerInspection(control, currentCoroutineContext().job)
            try {
                // Always consume an allocated job, including cancellation while
                // waiting for its owner to return the newly allocated handle.
                val next = withContext(NonCancellable) {
                    val task = host.withNative { Native.inspectionTask(it, control) }
                    withContext(Dispatchers.IO) { JSONObject(Native.inspectionHistogram(task)) }
                }
                ensureActive(); result = next
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) { failure = e.message ?: host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("inspection_failed") }
            finally { host.drawingTabs.releaseInspection(control); cancel = 0; Native.captureFree(control); busy = false }
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
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) { Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("histogram"), style = MaterialTheme.typography.titleMedium); TextButton(onClose) { Text(host.bootstrap!!.getJSONObject("common").getString("close")) } }
                ColorChoice(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("channel"), listOf("0" to "RGB", "1" to host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("red"), "2" to host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("green"), "3" to host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("blue"), "4" to host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("luminance")), channel) { channel = it }
                Row(verticalAlignment = Alignment.CenterVertically) { Checkbox(logarithmic, { logarithmic = it }); Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("log_scale")); Checkbox(automatic, { automatic = it }); Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("auto_update")) }
                val histogram = result?.getJSONObject("histogram")
                val axis=result?.getJSONObject("axis")
                val bins=axis?.getJSONArray("bins")?.let{it.getInt(0) until it.getInt(1)} ?: (0..255)
                val indices = if (channel == "0") listOf(0, 1, 2) else listOf(channel.toInt()-1)
                val colors = listOf(Color(0x88ed7474), Color(0x8869cf92), Color(0x8873a7f5), Color(0xccaaaaaa))
                Canvas(Modifier.fillMaxWidth().height(140.dp)) {
                    if (histogram != null) {
                        val channels = histogram.getJSONArray("channels")
                        fun value(channel: Int, x: Int): Double { val n = channels.getJSONObject(channel).getJSONArray("bins").getDouble(x); return if (logarithmic) ln(1+n) else n }
                        val maximum = indices.maxOf { i -> bins.maxOf { value(i, it) } }.coerceAtLeast(1.0)
                        for (i in indices) {
                            val path = Path().apply { moveTo(0f, size.height); for (x in bins) lineTo((x-bins.first).toFloat()/(bins.last-bins.first)*size.width, size.height-(value(i,x)/maximum*size.height).toFloat()); lineTo(size.width, size.height); close() }
                            drawPath(path, colors[i])
                        }
                        if(axis!=null&&!axis.isNull("white")){val x=axis.number("white")*size.width;drawLine(Color.Gray,Offset(x,0f),Offset(x,size.height),1.dp.toPx(),pathEffect=PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(),4.dp.toPx())))}
                    }
                }
                if (histogram != null) {
                    val color = histogram.getJSONObject("color")
                    Text("${color.getString("space")} · ${depthCopy.getString(when(color.getString("depth")){"F32"->"depth_float32";"F16"->"depth_float16";"U16"->"depth_16";else->"depth_8"})}")
                    Text(captions.optString("pixels"))
                    for (i in indices) {
                        Text("${listOf("R","G","B","Y")[i]}: ${captions.optString("channel-$i")}", style = MaterialTheme.typography.bodySmall)
                    }
                }
                Text(if (result != null && "${result!!.getLong("epoch")}:${result!!.getLong("revision")}" != key && !busy) captions.optString("stale") else captions.optString("status",copy.getString("inspection_preparing")))
                if(captions.has("range"))Text(captions.getString("range"),style=MaterialTheme.typography.bodySmall)
                Text(copy.getString(if(histogram?.getJSONObject("color")?.getString("depth") in listOf("F16","F32"))"inspection_hdr_help" else "inspection_help"),style=MaterialTheme.typography.bodySmall)
                TextButton({ refresh() }, enabled = !busy) { Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("refresh")) }
            }
        }
    }
}
