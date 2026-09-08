package art.capycanvas

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

@Composable internal fun ToolRibbon(host: CanvasHost, panel: JSONObject, geometry: JSONObject, dock: DockInteraction, modifier: Modifier) {
    val density = LocalDensity.current.density
    Box(modifier) {
        val tiles = panel.array("tiles").objects()
        geometry.array("tiles").objects().forEachIndexed { index, bounds ->
            tiles.getOrNull(index)?.let { tile ->
                val control = tile.getJSONObject("control")
                val kind = control.getString("kind")
                val icon = tile.optString("icon").takeIf { it != "null" && it.isNotEmpty() }
                    ?: when (kind) { "color" -> "color"; "opacity" -> "opacity"; "size" -> "size"; else -> "brush" }
                val modifier = dragSource(Modifier.placed(bounds, density), dock,
                    obj("kind" to "tile", "panel" to panel.getString("id"), "tile" to tile.getInt("id")))
                IconTile(icon, tile.getString("label"), tile.optBoolean("selected"), tile.getBoolean("enabled"), modifier) {
                    host.dispatch(obj("type" to "activate_tile", "panel" to panel.getString("id"), "tile" to tile.getInt("id")))
                }
            }
        }
        geometry.objectOrNull("grip")?.let { grip ->
            Box(dragSource(Modifier.placed(grip, density), dock, obj("kind" to "panel", "panel" to panel.getString("id"))), contentAlignment = Alignment.Center) {
                SharedIcon("grip", "Move toolbar")
            }
        }
    }
}

@Composable internal fun PanelControls(host: CanvasHost, state: JSONObject, panel: JSONObject, all: Boolean, modifier: Modifier = Modifier) {
    Column(modifier.verticalScroll(rememberScrollState()).padding(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        panel.array("controls").objects().filter { all || it.getBoolean("visible_in_panel") }.forEach { item ->
            when (item.getString("control")) {
                "brushes" -> BrushList(host, state.getJSONObject("brush"))
                "brush_size" -> NumericSetting("Brush size", state.getJSONObject("brush").number("diameter"), host.catalog.getJSONObject("brush_size")) {
                    host.dispatch(obj("type" to "set_brush_size", "value" to it))
                }
                "size_presets" -> SizePresets(host, state.getJSONObject("brush").number("diameter"))
                "brush_opacity" -> NumericSetting("Opacity", state.getJSONObject("brush").number("opacity"), host.catalog.getJSONObject("opacity")) {
                    host.dispatch(obj("type" to "set_brush_opacity", "value" to it))
                }
                "brush_color" -> ColorControls(host, state.getJSONObject("brush").array("color"))
                "layers" -> LayerList(host, state)
                "layer_actions" -> Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                    host.catalog.array("layer_commands").values().forEach { id ->
                        state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                            IconTile(command.getString("icon"), command.getString("label"), enabled = command.getBoolean("enabled")) { host.invoke(id.toString()) }
                        }
                    }
                }
                "layer_opacity" -> state.array("layers").objects().find { it.getBoolean("selected") }?.let { layer ->
                    NumericSetting("Layer opacity", layer.number("opacity"), host.catalog.getJSONObject("opacity")) {
                        host.dispatch(obj("type" to "set_layer_opacity", "opacity" to it))
                    }
                }
            }
        }
    }
}
@Composable internal fun NumericSetting(label: String, value: Float, control: JSONObject, onChange: (Float) -> Unit) {
    val low = control.number("min"); val high = control.number("max"); val step = control.number("step", 1.0)
    val text = if (value == value.toInt().toFloat()) value.toInt().toString() else "%.2f".format(java.util.Locale.ROOT, value)
    Column {
        Text(label, color = LocalPalette.current.secondary)
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconTile("minus", "Decrease $label", enabled = value > low) { onChange((value - step).coerceIn(low, high)) }
            CoreTextField(text, { next -> next.toFloatOrNull()?.takeIf { it.isFinite() && it in low..high }?.let(onChange) },
                modifier = Modifier.weight(1f),
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal))
            IconTile("plus", "Increase $label", enabled = value < high) { onChange((value + step).coerceIn(low, high)) }
        }
        Slider(value.coerceIn(low, high), onChange, valueRange = low..high, modifier = Modifier.fillMaxWidth())
    }
}
@Composable private fun BrushList(host: CanvasHost, brush: JSONObject) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    host.catalog.array("brush_categories").objects().forEach { category ->
        Text(category.getString("label"), Modifier.padding(top = 4.dp, bottom = 2.dp), color = colors.secondary)
        category.array("brushes").objects().forEach { choice ->
            val id = choice.getInt("id")
            val swatch = remember(id, colors.dark) {
                context.assets.open("$id-${if (colors.dark) "dark" else "light"}.png").use { BitmapFactory.decodeStream(it).asImageBitmap() }
            }
            Row(Modifier.fillMaxWidth().height(44.dp).background(if (brush.getInt("preset") == id) colors.active else Color.Transparent, RoundedCornerShape(6.dp))
                .clickable { host.dispatch(obj("type" to "select_brush", "id" to id)) }.padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(choice.getString("label"), Modifier.weight(1f))
                Image(swatch, null, Modifier.width(84.dp).height(26.dp))
            }
        }
    }
}
@OptIn(ExperimentalLayoutApi::class)
@Composable private fun SizePresets(host: CanvasHost, current: Float) {
    val colors = LocalPalette.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        host.catalog.array("brush_sizes").values().forEach { size ->
            val value = (size as Number).toFloat()
            Box(Modifier.size(48.dp).background(if (current == value) colors.active else colors.input, RoundedCornerShape(6.dp))
                .clickable { host.dispatch(obj("type" to "set_brush_size", "value" to value)) }, contentAlignment = Alignment.Center) {
                Text(value.roundToInt().toString())
            }
        }
    }
}
@Composable private fun LayerList(host: CanvasHost, state: JSONObject) {
    val colors = LocalPalette.current
    state.array("layers").objects().forEach { layer ->
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).background(if (layer.getBoolean("selected")) colors.active else Color.Transparent, RoundedCornerShape(6.dp))
            .clickable(enabled = layer.getBoolean("editable")) { host.dispatch(obj("type" to "select_layer", "id" to layer.getLong("id"))) }, verticalAlignment = Alignment.CenterVertically) {
            Checkbox(layer.getBoolean("visible"), { host.dispatch(obj("type" to "set_layer_visibility", "id" to layer.getLong("id"), "visible" to it)) })
            Text(layer.getString("label"), Modifier.weight(1f))
        }
    }
}
@Composable internal fun ColorControls(host: CanvasHost, rgba: JSONArray) {
    val values = (0..3).map { rgba.optDouble(it, 1.0).toFloat() }
    Row(Modifier.fillMaxWidth().height(36.dp).background(Color(values[0], values[1], values[2], values[3]), RoundedCornerShape(6.dp))) {}
    listOf("Red", "Green", "Blue").forEachIndexed { index, label ->
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, Modifier.width(48.dp))
            Slider(values[index], { value ->
                val changed = values.toMutableList(); changed[index] = value
                host.dispatch(obj("type" to "set_color", "rgba" to JSONArray(changed)))
            }, modifier = Modifier.weight(1f))
        }
    }
}
@Composable internal fun ConfigurePanel(host: CanvasHost, panel: JSONObject) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(panel.getString("configuration_title"), style = MaterialTheme.typography.titleMedium)
        Text(panel.getString("configuration_hint"), color = LocalPalette.current.secondary)
        panel.array("controls").objects().forEach { control ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(control.getBoolean("visible_in_panel"), { visible -> host.customize(obj("type" to "set_control_visible", "panel" to panel.getString("id"), "control" to control.getString("control"), "visible" to visible)) })
                Text(control.getString("label"))
            }
        }
        host.snapshot?.getJSONObject("state")?.let { state ->
            if (panel.array("controls").length() > 0) PanelControls(host, state, panel, true, Modifier.heightIn(max = 500.dp))
        }
        if (panel.getString("id").startsWith("toolbar")) {
            panel.array("tiles").objects().forEach { tile ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(tile.getString("label"), Modifier.weight(1f))
                    IconTile("minus", "Remove ${tile.getString("label")}") { host.customize(obj("type" to "remove_tool", "panel" to panel.getString("id"), "tile" to tile.getInt("id"))) }
                }
            }
            Button({ host.customize(obj("type" to "insert_tools", "panel" to panel.getString("id"), "before" to null)) }) { Text("Add tools") }
        }
    }
}
