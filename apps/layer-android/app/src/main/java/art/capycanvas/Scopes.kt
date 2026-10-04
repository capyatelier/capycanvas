package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import kotlin.math.roundToInt

internal data class ScopePlot(val paths: List<Pair<Color, Path>>, val image: ImageBitmap?)

internal fun scopePlots(packet: Array<Any>, previous: Map<String, ScopePlot>): Map<String, ScopePlot> {
    val header = JSONObject(packet[0] as String)
    val colors = header.array("colors").values().map { value ->
        val rgb = value as org.json.JSONArray
        Color(rgb.getInt(0), rgb.getInt(1), rgb.getInt(2))
    }
    return previous.toMutableMap().apply {
        header.getJSONObject("plots").let { plots -> plots.keys().forEach { kind ->
            val view = plots.getJSONObject(kind)
            val paths = view.array("plot").values().map { raw ->
                val channel = raw as org.json.JSONArray
                val bins = channel.getJSONArray(1)
                val path = Path()
                for (i in 0 until bins.length()) path.addRect(Rect(i.toFloat() / bins.length(), 1f - bins.getDouble(i).toFloat(), (i + 1f) / bins.length(), 1f))
                colors[channel.getInt(0)] to path
            }
            val image = if (kind == "waveform" && !view.isNull("extent")) {
                val extent = view.getJSONArray("extent")
                Bitmap.createBitmap(packet[1] as IntArray, extent.getInt(0), extent.getInt(1), Bitmap.Config.ARGB_8888).asImageBitmap()
            } else null
            put(kind, ScopePlot(paths, image))
        } }
    }
}

@Composable internal fun ScopeGraph(host: CanvasHost, state: JSONObject, kind: String, modifier: Modifier = Modifier) {
    val plot = host.scopes[kind]
    val view = state.getJSONObject(kind)
    Canvas(modifier.testTag("scope-$kind-chart").semantics { contentDescription = listOf(view.getString("description"), view.getString("range")).joinToString("\n") }) {
        if (kind == "waveform") plot?.image?.let { drawImage(it, dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()), filterQuality = FilterQuality.None) }
        else scale(size.width, size.height, pivot = androidx.compose.ui.geometry.Offset.Zero) {
            plot?.paths?.forEach { (color, path) -> drawPath(path, color, alpha = .55f) }
        }
    }
}

@Composable internal fun ScopeFooter(host: CanvasHost, state: JSONObject, kind: String, logarithmic: Boolean = false) {
    val view = state.getJSONObject(kind)
    val histogram = state.getJSONObject("histogram")
    val colors = LocalPalette.current
    fun send(type: String, enabled: Boolean) = host.dispatch(obj("type" to "histogram", "action" to obj("type" to type, "enabled" to enabled)))
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(view.getString("status"), Modifier.weight(1f).testTag("scope-$kind-status"), color = colors.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (logarithmic) Row(verticalAlignment = Alignment.CenterVertically) {
            val label = view.array("labels").getString(0)
            EditorCheck(view.getBoolean("logarithmic"), label, Modifier.testTag("scope-$kind-log")) { send(if (kind == "waveform") "waveform_logarithmic" else "logarithmic", it) }
            Text(label, maxLines = 1)
        }
        for ((index, name) in listOf("shadows", "highlights").withIndex()) {
            val label = histogram.array("labels").getString(index + 1)
            Box(Modifier.size(28.dp).testTag("scope-$kind-$name").toggleable(histogram.getBoolean(name), role = Role.Checkbox) { send(name, it) }, contentAlignment = Alignment.Center) {
                SharedIcon("tonal-$name", label, tint = if (histogram.getBoolean(name)) colors.accent else colors.text)
            }
        }
    }
}

@Composable internal fun ScopeControl(host: CanvasHost, state: JSONObject, kind: String, modifier: Modifier = Modifier, tonal: Boolean = false) {
    val view = state.getJSONObject(kind)
    val colors = LocalPalette.current
    val waveform = kind == "waveform"
    fun select(type: String, index: Int) = host.dispatch(obj("type" to "histogram", "action" to obj("type" to type, "index" to index)))
    Column(modifier.fillMaxWidth().testTag("scope-$kind"), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        if (!tonal) Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Box(Modifier.weight(1f).testTag("scope-$kind-source")) {
                PropertyChoice(host.catalog.getJSONObject("native_copy").getJSONObject("sampler").getString("source"), view.array("sources").values().map { it.toString() }, view.getInt("source")) { select("source", it) }
            }
            Box(Modifier.weight(1f).testTag("scope-$kind-channel")) {
                PropertyChoice(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("channel"), view.array("channels").values().map { it.toString() }, view.getInt("channel")) { select(if (waveform) "waveform_channel" else "channel", it) }
            }
        }
        Box(Modifier.fillMaxWidth()) {
            ScopeGraph(host, state, kind, Modifier.fillMaxWidth().height(if (tonal) 120.dp else 160.dp))
            if (waveform) Column(Modifier.matchParentSize().padding(start = 3.dp), verticalArrangement = Arrangement.SpaceBetween) {
                Text(view.array("axis").getString(1), color = colors.secondary)
                Text(view.array("axis").getString(0), color = colors.secondary)
            }
        }
        if (!tonal && !waveform) Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            view.array("axis").values().forEach { Text(it.toString(), color = colors.secondary) }
        }
        ScopeFooter(host, state, kind, logarithmic = !tonal)
    }
}
