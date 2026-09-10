package art.capycanvas

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.dp
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

@Composable internal fun ToolRibbon(host: CanvasHost, panel: JSONObject, geometry: JSONObject, dock: DockInteraction, modifier: Modifier, vertical: Boolean = false) {
    val density = LocalDensity.current.density
    Box(modifier) {
        val style = panel.getString("tile_style")
        val tiles = panel.array("tiles").objects()
        geometry.array("tiles").objects().forEachIndexed { index, bounds ->
            tiles.getOrNull(index)?.let { tile ->
                val control = tile.getJSONObject("control")
                val kind = control.getString("kind")
                val icon = tile.optString("icon").takeIf { it != "null" && it.isNotEmpty() }
                    ?: when (kind) { "color" -> "color"; "opacity" -> "opacity"; "size" -> "size"; else -> "brush" }
                val modifier = Modifier.placed(bounds, density).testTag("tile-${panel.getString("id")}-${tile.getInt("id")}").dragSource(dock,
                    obj("kind" to "tile", "panel" to panel.getString("id"), "tile" to tile.getInt("id")))
                val fill = if (kind == "color") host.snapshot?.getJSONObject("state")?.getJSONObject("brush")?.array("color")?.let {
                        Color(it.getDouble(0).toFloat(), it.getDouble(1).toFloat(), it.getDouble(2).toFloat())
                    } else null
                val colors = LocalPalette.current
                HoverTip(tile.getString("tooltip"), modifier) {
                Row(Modifier.fillMaxSize().clip(RoundedCornerShape(6.dp)).alpha(if (tile.getBoolean("enabled")) 1f else .4f)
                    .background(if (tile.optBoolean("selected")) colors.active else Color.Transparent)
                    .combinedClickable(enabled = tile.getBoolean("enabled"),
                        onLongClick = { dock.context(obj("kind" to "tile", "panel" to panel.getString("id"), "tile" to tile.getInt("id"))) },
                        onClick = { host.dispatch(obj("type" to "activate_tile", "panel" to panel.getString("id"), "tile" to tile.getInt("id"))) }),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Center) {
                    Box(if (style == "labeled") Modifier.width(36.dp) else Modifier, contentAlignment = Alignment.Center) {
                        SharedIcon(icon, tile.getString("label"), Modifier.size(if (style == "large") 32.dp else 16.dp), fill = fill)
                    }
                    if (style == "labeled") Text(tile.getString("label"), Modifier.weight(1f).padding(end = 4.dp),
                        fontWeight = FontWeight.Bold, maxLines = 3, overflow = TextOverflow.Ellipsis)
                }
                }
            }
        }
        geometry.objectOrNull("grip")?.let { grip ->
            val item = obj("kind" to "panel", "panel" to panel.getString("id"))
            Box(Modifier.placed(grip, density).testTag("ribbon-grip-${panel.getString("id")}").dragSource(dock, item,
                context = obj("kind" to "ribbon", "panel" to panel.getString("id")))
                .combinedClickable(onClick = {}, onDoubleClick = { dock.doubleClickHandle(item) },
                    onLongClick = { dock.context(obj("kind" to "ribbon", "panel" to panel.getString("id"))) }), contentAlignment = Alignment.Center) {
                PanelGrip("Move toolbar", vertical)
            }
        }
    }
}

@Composable internal fun PanelControls(host: CanvasHost, state: JSONObject, panel: JSONObject, modifier: Modifier = Modifier, onHeight: (Float) -> Unit = {}) {
    val layers = panel.getString("id") == "layers"
    val density = LocalDensity.current.density
    if (layers) {
        LayerPanel(host, state, modifier)
        return
    }
    Box(modifier) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).onSizeChanged { onHeight(it.height / density) }.padding(if (layers) 12.dp else 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            panel.array("controls").objects().filter { it.getBoolean("visible_in_panel") }.forEach { item ->
                when (item.getString("control")) {
                    "brushes" -> BrushList(host, state.getJSONObject("brush"))
                    "brush_size" -> NumericSetting("Brush size", state.getJSONObject("brush").number("diameter"), host.catalog.getJSONObject("brush_size")) {
                        host.dispatch(obj("type" to "set_brush_size", "value" to it))
                    }
                    "size_presets" -> SizePresets(host, state.getJSONObject("brush").number("diameter"))
                    "brush_opacity" -> NumericSetting("Brush opacity", state.getJSONObject("brush").number("opacity"), host.catalog.getJSONObject("opacity")) {
                        host.dispatch(obj("type" to "set_brush_opacity", "value" to it))
                    }
                    "brush_color" -> ColorControls(host, state.getJSONObject("brush").array("color"))
                    "layers" -> LayerPanel(host, state, Modifier.heightIn(min = 240.dp, max = 480.dp))
                    "layer_actions" -> Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        host.catalog.array("layer_commands").values().forEach { id ->
                            state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                                Box(Modifier.size(40.dp, 28.dp).alpha(if (command.getBoolean("enabled")) 1f else .36f)
                                    .clip(RoundedCornerShape(6.dp)).clickable(enabled = command.getBoolean("enabled")) { host.invoke(id.toString()) }, contentAlignment = Alignment.Center) {
                                    SharedIcon(command.getString("icon"), command.getString("label"), Modifier.size(14.dp))
                                }
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
}
@Composable private fun BrushList(host: CanvasHost, brush: JSONObject) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
      host.catalog.array("brush_categories").objects().forEach { category ->
        Text(category.getString("label"), Modifier.padding(8.dp), color = colors.secondary, fontWeight = FontWeight.Bold)
        category.array("brushes").objects().forEach { choice ->
            val id = choice.getInt("id")
            val swatch = remember(id, colors.dark) {
                context.assets.open("$id-${if (colors.dark) "dark" else "light"}.png").use { BitmapFactory.decodeStream(it).asImageBitmap() }
            }
            ActionTip(host, choice.getString("label"), obj("type" to "select_brush", "id" to id), Modifier.fillMaxWidth()) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp)).background(if (brush.getInt("preset") == id) colors.active else Color.Transparent)
                .clickable { host.dispatch(obj("type" to "select_brush", "id" to id)) }.padding(horizontal = 6.dp, vertical = 3.dp)) {
                Image(swatch, null, Modifier.fillMaxWidth().height(40.dp).testTag("brush-preview-$id"), contentScale = ContentScale.FillBounds)
                Text(choice.getString("label"), Modifier.fillMaxWidth(), textAlign = TextAlign.End, fontWeight = FontWeight.Bold)
            }
            }
        }
      }
    }
}
@Composable private fun SizePresets(host: CanvasHost, current: Float) {
    val colors = LocalPalette.current
    BoxWithConstraints {
      val columns = if (maxWidth < 130.dp) 2 else if (maxWidth < 174.dp) 3 else 4
      Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        host.catalog.array("brush_sizes").values().chunked(columns).forEach { sizes ->
          Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
           sizes.forEach { size ->
            val value = (size as Number).toFloat()
            ActionTip(host, "${value.roundToInt()} px", obj("type" to "set_brush_size", "value" to value), Modifier.weight(1f).testTag("size-preset-${value.roundToInt()}")) {
            Column(Modifier.fillMaxWidth().padding(3.dp).clip(RoundedCornerShape(6.dp))
                .background(if (current == value) colors.active else Color.Transparent)
                .clickable { host.dispatch(obj("type" to "set_brush_size", "value" to value)) }.padding(2.dp),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Canvas(Modifier.fillMaxWidth().height(28.dp)) {
                    drawCircle(colors.text, minOf(27f, 2f + kotlin.math.sqrt(value) * 1.2f).dp.toPx() / 2)
                }
                Text(value.roundToInt().toString())
            }
            }
           }
           repeat(columns - sizes.size) { Spacer(Modifier.weight(1f)) }
          }
        }
      }
    }
}
@Composable internal fun ColorControls(host: CanvasHost, rgba: JSONArray) {
    val values = (0..3).map { rgba.optDouble(it, 1.0).toFloat() }
    Row(Modifier.fillMaxWidth().height(36.dp).background(Color(values[0], values[1], values[2], values[3]), RoundedCornerShape(6.dp))) {}
    listOf("Red", "Green", "Blue").forEachIndexed { index, label ->
        NumericSetting(label, values[index], host.catalog.getJSONObject("opacity")) { value ->
                val changed = values.toMutableList(); changed[index] = value
                host.dispatch(obj("type" to "set_color", "rgba" to JSONArray(changed)))
        }
    }
}
@Composable internal fun ConfigurePanel(host: CanvasHost, panel: JSONObject, onHeight: (Float) -> Unit = {}) {
    val density = LocalDensity.current.density
    Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).onSizeChanged { onHeight(it.height / density) }.padding(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(panel.getString("configuration_title"), fontWeight = FontWeight.Bold)
        Text(panel.getString("configuration_hint"), color = LocalPalette.current.secondary)
        panel.array("controls").objects().forEach { control ->
          Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                EditorCheck(control.getBoolean("visible_in_panel"), control.getString("label")) { visible -> host.customize(obj("type" to "set_control_visible", "panel" to panel.getString("id"), "control" to control.getString("control"), "visible" to visible)) }
                Text(control.getString("label"))
            }
            host.snapshot?.getJSONObject("state")?.let { state -> ConfigurationControl(host, state, control.getString("control"), control.getString("label")) }
          }
        }
        Column { WorkspaceMenuItems(host, panel.array("toolbar_options")) }
    }
}

/** Drawer controls edit the same values as the live panel alongside them. */
@OptIn(ExperimentalLayoutApi::class)
@Composable private fun ConfigurationControl(host: CanvasHost, state: JSONObject, control: String, label: String) {
    val brush = state.getJSONObject("brush")
    when (control) {
        "brush_size" -> NumericSetting(label, brush.number("diameter"), host.catalog.getJSONObject("brush_size")) {
            host.dispatch(obj("type" to "set_brush_size", "value" to it))
        }
        "brush_opacity" -> NumericSetting(label, brush.number("opacity"), host.catalog.getJSONObject("opacity")) { host.dispatch(obj("type" to "set_brush_opacity", "value" to it)) }
        "layer_opacity" -> state.array("layers").objects().find { it.getBoolean("selected") }?.let { layer ->
            NumericSetting(label, layer.number("opacity"), host.catalog.getJSONObject("opacity")) { host.dispatch(obj("type" to "set_layer_opacity", "opacity" to it)) }
        }
        "brush_color" -> {
            val rgba = brush.array("color")
            Box(Modifier.fillMaxWidth().height(34.dp).clip(RoundedCornerShape(6.dp)).background(LocalPalette.current.button)
                .clickable { host.customize(obj("type" to "open_control", "control" to "brush_color")) }.padding(6.dp)) {
                Box(Modifier.fillMaxSize().background(Color(rgba.getDouble(0).toFloat(), rgba.getDouble(1).toFloat(), rgba.getDouble(2).toFloat()), RoundedCornerShape(4.dp)))
            }
        }
        "brushes", "layers" -> {
            var open by remember { mutableStateOf(false) }
            val choices = if (control == "brushes") host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() } else state.array("layers").objects()
            val selected = if (control == "brushes") choices.find { it.getInt("id") == brush.getInt("preset") } else choices.find { it.getBoolean("selected") }
            Box {
                Row(Modifier.fillMaxWidth().heightIn(min = 34.dp).clip(RoundedCornerShape(6.dp)).background(LocalPalette.current.input)
                    .clickable { open = true }.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(selected?.getString("label") ?: "", Modifier.weight(1f)); SharedIcon("chevron-down", null)
                }
                DropdownMenu(open, { open = false }) {
                    choices.forEach { choice -> DropdownMenuItem(text = { Text(choice.getString("label")) }, onClick = {
                        open = false; host.dispatch(obj("type" to if (control == "brushes") "select_brush" else "select_layer", "id" to choice.getLong("id")))
                    }, enabled = choice.optBoolean("editable", true)) }
                }
            }
        }
        "size_presets" -> FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            host.catalog.array("brush_sizes").values().forEach { value ->
                Box(Modifier.widthIn(min = 52.dp).height(34.dp).clip(RoundedCornerShape(6.dp)).background(LocalPalette.current.button)
                    .clickable { host.dispatch(obj("type" to "set_brush_size", "value" to value)) }, contentAlignment = Alignment.Center) { Text(value.toString(), fontWeight = FontWeight.Bold) }
            }
        }
        "layer_actions" -> FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            host.catalog.array("layer_commands").values().forEach { id ->
                state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                    TextButton({ host.invoke(id.toString()) }, enabled = command.getBoolean("enabled")) { Text(command.getString("label")) }
                }
            }
        }
    }
}
