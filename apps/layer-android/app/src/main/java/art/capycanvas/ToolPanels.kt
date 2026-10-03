package art.capycanvas

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.border
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject
import org.json.JSONArray
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** The selected tool determines groups, subtools and settings in Rust. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun ToolSetControls(host: CanvasHost, state: JSONObject, panel: String = "brushes", projection: JSONObject? = null) {
    val view = projection ?: state.getJSONObject("tool_panels").optJSONObject(panel) ?: state.getJSONObject("tool_set")
    val compact = state.array("tool_extra").objects().any { it.optJSONObject("Choice")?.optString("id") == "tonal-tones" }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (panel == "brush_sets" || panel == "sculpt_sets") Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("groups").objects().forEach { item ->
                ToolChoice(host, item, "set", Modifier.fillMaxWidth().testTag("$panel-${item.getString("label")}"), compact)
            }
        } else FlowRow(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("groups").objects().forEach { item ->
                ToolChoice(host, item, "group", (if (view.array("groups").length() == 1) Modifier.fillMaxWidth() else Modifier.width(108.dp)).testTag("tool-group-${item.getString("label")}"), compact)
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("subtools").objects().forEach { item ->
                ToolChoice(host, item, "subtool", Modifier.fillMaxWidth().testTag("subtool-${item.getString("label")}"), compact)
            }
        }
    }
}

@Composable private fun ToolChoice(host: CanvasHost, item: JSONObject, kind: String, modifier: Modifier, compact: Boolean = false) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    val label = item.getString("label")
    val action = item.getJSONObject("action")
    val enabled = item.optBoolean("enabled", true)
    val preview = item.opt("preview").takeIf { it is Number } as? Number
    ActionTip(host, label, action, modifier) {
        Column(Modifier.fillMaxWidth().clip(ControlShape).alpha(if (enabled) 1f else .4f)
            .background(if (item.optBoolean("selected")) colors.active else Color.Transparent)
            .clickable(enabled = enabled) { host.dispatch(action) }.padding(horizontal = 6.dp, vertical = 3.dp)) {
            if (preview != null) {
                val id = preview.toInt()
                val name = "$id-${if (colors.dark) "dark" else "light"}.png"
                var swatch by remember(host, name) { mutableStateOf(host.brushPreviews.get(name)) }
                LaunchedEffect(host, name) {
                    if (swatch == null) swatch = withContext(Dispatchers.IO) {
                        host.brushPreviews.get(name) ?: context.assets.open(name).use { BitmapFactory.decodeStream(it).asImageBitmap() }
                            .also { host.brushPreviews.put(name, it) }
                    }
                }
                Box(Modifier.fillMaxWidth().height(40.dp)) {
                    swatch?.let { Image(it, null, Modifier.fillMaxSize().testTag("brush-preview-$id"), contentScale = ContentScale.FillBounds) }
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    SharedIcon(item.getString("icon"), null, Modifier.testTag("tool-$kind-icon-$label"))
                    Text(label, Modifier.weight(1f), textAlign = TextAlign.End, fontWeight = FontWeight.Bold)
                }
            } else Row(Modifier.heightIn(min = if (compact) 30.dp else 42.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                SharedIcon(item.getString("icon"), null, Modifier.testTag("tool-$kind-icon-$label"))
                Text(label, fontWeight = FontWeight.Bold, maxLines = if (kind == "set") 1 else 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable internal fun ToolSettingsControls(host: CanvasHost, state: JSONObject) {
    if (state.array("tool_extra").objects().any { it.optJSONObject("Choice")?.optString("id") == "tonal-tones" }) {
        key(state.optLong("toolbar_context_generation"), state.getJSONObject("document_file").optLong("epoch"), state.getJSONObject("layer_tools").optJSONObject("editing_layer")?.toString()) {
            TonalSettingsControls(host, state)
        }
        return
    }
    if (state.getJSONObject("layer_tools").optString("tool") in listOf("pick_visible", "pick_layer")) {
        val picker=state.getJSONObject("color_picker")
        val copy=remember(host.catalog) { host.catalog.getJSONObject("native_copy").getJSONObject("sampler") }
        val sizeLabels=remember(copy) { copy.array("sizes").values().associate { value -> (value as JSONArray).getInt(0) to value.getString(1) } }
        ProvideTextStyle(LocalTextStyle.current.copy(fontSize=13.sp)) {
        Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth().testTag("picker-setting-source"),verticalAlignment=Alignment.CenterVertically) {
                Text(copy.getString("source"),Modifier.width(76.dp),maxLines=1)
                PropertyChoice(copy.getString("source"),if(picker.optBoolean("can_sample_layer"))listOf(copy.getString("visible_color"),copy.getString("selected_layer")) else listOf(copy.getString("visible_color")),if(picker.getBoolean("layer"))1 else 0,onOpenChanged={host.pickerPopupOpen=it}) {
                    host.dispatch(obj("type" to "color_picker","action" to obj("kind" to "source","layer" to (it==1))))
                }
            }
            val sizes=picker.array("sample_sizes").values().map{(it as Number).toInt()}
            Row(Modifier.fillMaxWidth().testTag("picker-setting-size"),verticalAlignment=Alignment.CenterVertically) {
                Text(copy.getString("sample_size"),Modifier.width(76.dp),maxLines=1)
                PropertyChoice(copy.getString("sample_size"),sizes.map{sizeLabels.getValue(it)},sizes.indexOf(picker.getInt("sample_width")),onOpenChanged={host.pickerPopupOpen=it}) {
                    host.dispatch(obj("type" to "set_color_sample_size","width" to sizes[it]))
                }
            }
        }
        }
        return
    }
    val controlCopy = remember(host.catalog) { host.catalog.getJSONObject("native_copy").getJSONObject("tool_controls") }
    val modes = setOf("selection_new", "selection_add", "selection_subtract", "selection_intersect")
    val actions = state.array("tool_actions").objects()
    val commands = state.array("commands").let { list -> remember(list) { list.objects().associateBy { it.getString("id") } } }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (actions.any { it.getString("command") in modes }) Row(Modifier.fillMaxWidth().clip(ControlShape).border(1.dp, LocalPalette.current.divider, ControlShape).selectableGroup().testTag("selection-mode-row")) {
            actions.filter { it.getString("command") in modes }.forEach { action ->
                val id = action.getString("command")
                val command = commands.getValue(id)
                val selected = command.getBoolean("selected")
                HoverTip(command.getString("tooltip"), Modifier.weight(1f)) {
                    Box(Modifier.fillMaxWidth().height(48.dp).testTag("tool-action-$id")
                        .background(if (selected) LocalPalette.current.active else Color.Transparent)
                        .selectable(selected = selected, enabled = command.getBoolean("enabled"), role = Role.RadioButton) { host.invoke(id) },
                        contentAlignment = Alignment.Center) {
                        SharedIcon(command.getString("icon"), command.getString("label"), Modifier.size(20.dp))
                    }
                }
            }
        }
        if (actions.any { it.getString("command") in modes }) SelectionMenuButton(host, controlCopy.getString("selection_menu"), "selection")
        val choices = state.array("tool_extra").objects().mapNotNull { it.optJSONObject("Choice") }
        val beside = choices.filter { it.has("beside") }.associateBy { it.getString("beside") }
        choices.filterNot { it.has("beside") }.forEach { choice ->
            ToolbarChoice(choice, false, false, false, 20, host::dispatch, prefix = "tool", height = 32f)
        }
        val fieldControl: @Composable (JSONObject, Boolean) -> Unit = { field, compact ->
            val id = field.getString("id")
            Box(Modifier.testTag("tool-setting-$id")) {
                if (compact) Row(Modifier.fillMaxWidth().height(26.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(field.getString("label"), Modifier.weight(1f), maxLines = 1)
                    NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), id = id, inline = true, showSlider = false) {
                        host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                    }
                } else NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), id = id) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
            }
        }
        state.array("tool_settings").objects().groupBy { it.getString("group") }.forEach { (group, fields) ->
            val choice = beside[fields.first().getString("id")]
            val title = choice?.getString("label") ?: group
            if (title.isNotEmpty()) Text(title, fontWeight = FontWeight.Bold, color = LocalPalette.current.secondary)
            if (choice == null) fields.forEach { fieldControl(it, false) }
            else Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ToolbarChoice(choice, false, false, false, 20, host::dispatch, prefix = "tool")
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) { fields.forEach { fieldControl(it, true) } }
            }
        }
        actions.filter { it.getString("command") !in modes }.forEach { action ->
            val id = action.getString("command")
            commands[id]?.let { command ->
                if (action.optBoolean("checkable")) Row(Modifier.fillMaxWidth().testTag("tool-action-$id")
                    .toggleable(command.getBoolean("selected"), enabled = command.getBoolean("enabled"), role = Role.Checkbox) { host.invoke(id) },
                    verticalAlignment = Alignment.CenterVertically) {
                    EditorCheck(command.optBoolean("selected"), command.getString("label"), Modifier.clearAndSetSemantics {}, enabled = command.getBoolean("enabled")) { host.invoke(id) }
                    Text(command.getString("label"))
                } else TextButton({ host.invoke(id) }, Modifier.testTag("tool-action-$id"), enabled = command.getBoolean("enabled")) {
                    SharedIcon(command.getString("icon"), null)
                    Spacer(Modifier.width(6.dp))
                    Text(command.getString("label"))
                }
            }
        }
    }
}

@Composable private fun TonalSettingsControls(host: CanvasHost, state: JSONObject) {
    val copy = remember(host.catalog) { host.catalog.getJSONObject("native_copy").getJSONObject("tool_controls") }
    val commands = state.array("commands").let { list -> remember(list) { list.objects().associateBy { it.getString("id") } } }
    val actions = state.array("tool_actions").objects()
    val modes = obj("id" to "selection-mode", "label" to copy.getString("selection_mode"), "segmented" to true, "items" to JSONArray(actions.map { action ->
        val command = commands.getValue(action.getString("command"))
        obj("icon" to command.getString("icon"), "label" to command.getString("label"), "selected" to command.getBoolean("selected"),
            "action" to obj("type" to "invoke", "command" to action.getString("command")))
    }))
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        ToolbarChoice(modes, false, false, false, 20, host::dispatch, prefix="tool", height=36f)
        state.array("tool_extra").objects().forEach { option ->
            ToolbarChoice(option.getJSONObject("Choice"), false, false, false, 20, host::dispatch, prefix="tool", height=36f)
        }
        val fields = state.array("tool_settings").objects()
        if (fields.any { it.getString("id") == "tonal_lower" }) {
            val bounds = listOf("tonal_lower", "tonal_upper").map { id -> fields.first { it.getString("id") == id } }
            RangeControl(bounds, copy.getString("range_hint"), Modifier.fillMaxWidth()) { index, value ->
                host.dispatch(obj("type" to "set_tool_setting", "id" to bounds[index].getString("id"), "value" to value))
            }
        }
        fields.filter { it.getString("id") !in listOf("tonal_lower", "tonal_upper") }.forEach { field ->
            val id = field.getString("id")
            Row(Modifier.fillMaxWidth().height(28.dp).testTag("tool-setting-$id"), verticalAlignment=Alignment.CenterVertically, horizontalArrangement=Arrangement.spacedBy(6.dp)) {
                Text(field.getString("label"), Modifier.width(62.dp), maxLines=1)
                NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), Modifier.weight(1f), id=id, inline=true) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
            }
        }
    }
}

/** Native double-press timing; shared Rust owns activation and drawer state. */
@Composable internal fun pickerClick(host: CanvasHost, control: JSONObject?, anchor: JSONObject, activate: () -> Unit): () -> Unit {
    val current by rememberUpdatedState(activate)
    val picker=control?.optString("kind")=="color_picker" || control?.optString("kind")=="command" && control.optString("command")=="eyedropper"
    val timeout=androidx.compose.ui.platform.LocalViewConfiguration.current.doubleTapTimeoutMillis
    var last by remember(anchor.toString()) { mutableLongStateOf(0L) }
    return {
        val now=android.os.SystemClock.uptimeMillis()
        if(picker && last!=0L && now-last<=timeout) {
            last=0L
            host.dispatch(obj("type" to "color_picker","action" to obj("kind" to "settings","anchor" to anchor)))
        } else { last=now;current() }
    }
}
