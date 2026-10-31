package art.capycanvas

import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown

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
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
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
        if (view.array("groups").length() > 0 && view.array("subtools").length() > 0)
            ToolbarDivider(Modifier.fillMaxWidth().height(1.dp).padding(horizontal = 4.dp).testTag("tool-set-divider"), horizontal = true, length = null)
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("subtools").objects().forEach { item ->
                ToolChoice(host, item, "subtool", Modifier.fillMaxWidth().testTag("subtool-${item.getString("label")}"), compact)
            }
        }
    }
}

@Composable internal fun BrushPreview(host: CanvasHost, id: Int, modifier: Modifier) =
    BundledPreview(host, "$id-${if (LocalPalette.current.dark) "dark" else "light"}.webp", "brush-preview-$id", ContentScale.FillBounds, modifier)

@Composable internal fun BundledPreview(host: CanvasHost, name: String, tag: String, scale: ContentScale, modifier: Modifier) {
    val context = LocalContext.current
    var image by remember(host, name) { mutableStateOf(host.bundledPreviews.get(name)) }
    LaunchedEffect(host, name) {
        if (image == null) image = withContext(Dispatchers.IO) {
            host.bundledPreviews.get(name) ?: context.assets.open(name).use { BitmapFactory.decodeStream(it).asImageBitmap() }
                .also { host.bundledPreviews.put(name, it) }
        }
    }
    Box(modifier) { image?.let { Image(it, null, Modifier.fillMaxSize().testTag(tag), contentScale = scale) } }
}

@Composable private fun ToolChoice(host: CanvasHost, item: JSONObject, kind: String, modifier: Modifier, compact: Boolean = false) {
    val colors = LocalPalette.current
    val label = item.getString("label")
    val action = item.getJSONObject("action")
    val enabled = item.optBoolean("enabled", true)
    val reason = item.nullableString("disabled_reason")
    val preview = item.opt("preview").takeIf { it is Number } as? Number
    var reveal by remember { mutableIntStateOf(0) }
    val row = @Composable {
        Column(Modifier.fillMaxWidth().clip(ControlShape).alpha(if (enabled) 1f else .4f)
            .background(if (item.optBoolean("selected")) colors.active else Color.Transparent)
            .clickable(enabled = enabled || reason != null) { if (enabled) host.dispatch(action) else reveal++ }
            .semantics { if (!enabled) disabled() }
            .alpha(if (enabled && reason != null) .4f else 1f).padding(horizontal = 6.dp, vertical = 3.dp)) {
            if (preview != null) {
                BrushPreview(host, preview.toInt(), Modifier.fillMaxWidth().height(40.dp))
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
    if (reason == null) ActionTip(host, label, action, modifier.bubbleChoice(action), row)
    else HoverTip(reason, modifier.bubbleChoice(action), reveal = reveal, content = row)
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
        Column(verticalArrangement=Arrangement.spacedBy(panelMetrics().spacing)) {
            val labelWidth=panelLabelWidth(listOf(copy.getString("source"),copy.getString("sample_size")))
            Row(Modifier.fillMaxWidth().testTag("picker-setting-source"),horizontalArrangement=Arrangement.spacedBy(panelMetrics().spacing),verticalAlignment=Alignment.CenterVertically) {
                Text(copy.getString("source"),Modifier.width(labelWidth),maxLines=1)
                PropertyChoice(copy.getString("source"),if(picker.optBoolean("can_sample_layer"))listOf(copy.getString("visible_color"),copy.getString("selected_layer")) else listOf(copy.getString("visible_color")),if(picker.getBoolean("layer"))1 else 0,onOpenChanged={host.pickerPopupOpen=it}) {
                    host.dispatch(obj("type" to "color_picker","action" to obj("kind" to "source","layer" to (it==1))))
                }
            }
            val sizes=picker.array("sample_sizes").values().map{(it as Number).toInt()}
            Row(Modifier.fillMaxWidth().testTag("picker-setting-size"),horizontalArrangement=Arrangement.spacedBy(panelMetrics().spacing),verticalAlignment=Alignment.CenterVertically) {
                Text(copy.getString("sample_size"),Modifier.width(labelWidth),maxLines=1)
                PropertyChoice(copy.getString("sample_size"),sizes.map{sizeLabels.getValue(it)},sizes.indexOf(picker.getInt("sample_width")),onOpenChanged={host.pickerPopupOpen=it}) {
                    host.dispatch(obj("type" to "set_color_sample_size","width" to sizes[it]))
                }
            }
        }
        return
    }
    val metrics = panelMetrics()
    val controlCopy = remember(host.catalog) { host.catalog.getJSONObject("native_copy").getJSONObject("tool_controls") }
    val actions = state.array("tool_actions").objects()
    val commands = state.array("commands").let { list -> remember(list) { list.objects().associateBy { it.getString("id") } } }
    Column(verticalArrangement = Arrangement.spacedBy(metrics.group)) {
        if (actions.any { it.optBoolean("segmented") }) Column(verticalArrangement=Arrangement.spacedBy(metrics.spacing)) {
        actions.filter {it.optBoolean("segmented")}.groupBy {it.getString("group")}.forEach { (group, grouped) -> Row(Modifier.fillMaxWidth().clip(ControlShape).border(1.dp, LocalPalette.current.divider, ControlShape).selectableGroup().testTag(if(group=="selection-mode") "selection-mode-row" else "tool-action-group-$group")) {
            grouped.forEach { action ->
                val id = action.getString("command")
                val command = commands.getValue(id)
                val selected = command.getBoolean("selected")
                HoverTip(command.getString("tooltip"), Modifier.weight(1f)) {
                    Box(Modifier.fillMaxWidth().height(metrics.height).testTag("tool-action-$id")
                        .background(if (selected) LocalPalette.current.active else Color.Transparent)
                        .selectable(selected = selected, enabled = command.getBoolean("enabled"), role = Role.RadioButton) { host.invoke(id) },
                        contentAlignment = Alignment.Center) {
                        SharedIcon(command.getString("icon"), command.getString("label"), Modifier.size(metrics.icon))
                    }
                }
            }
        }
        }
        if (actions.any { it.optString("group")=="selection-mode" }) SelectionMenuButton(host, controlCopy.getString("selection_menu"), "selection", panel = true)
        }
        Column(verticalArrangement=Arrangement.spacedBy(metrics.spacing)) {
        val choices = state.array("tool_extra").objects().mapNotNull { it.optJSONObject("Choice") }
        val beside = choices.filter { it.has("beside") }.associateBy { it.getString("beside") }
        val choiceLabelWidth = panelLabelWidth(choices.filter { !it.getBoolean("segmented") }.map { it.getString("label") })
        choices.filterNot { it.has("beside") }.forEach { choice ->
            if (choice.getBoolean("segmented") || choice.optBoolean("labeled")) {
                ToolbarChoice(choice, false, false, false, 20, host::dispatch, prefix = "tool", height = metrics.height.value)
            } else Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.spacing)) {
                Text(choice.getString("label"), Modifier.width(choiceLabelWidth), maxLines = 1, overflow = TextOverflow.Ellipsis)
                Box(Modifier.weight(1f)) { ToolbarChoice(choice, false, false, false, 20, host::dispatch, prefix = "tool", height = metrics.height.value) }
            }
        }
        state.array("tool_extra").objects().mapNotNull {it.optJSONObject("Gradient")}.forEach {GradientControl(host,it)}
        state.array("tool_extra").objects().mapNotNull {it.optJSONObject("Checklist")}.forEach {ToolChecklist(it, host::dispatch, "tool")}
        val generation = state.optLong("toolbar_context_generation")
        val fieldControl: @Composable (JSONObject, Boolean) -> Unit = { field, compact ->
            val id = field.getString("id")
            val phase: ((String) -> Unit)? = if (!field.optBoolean("preview")) null else {
                { phase ->
                if (host.snapshot?.getJSONObject("state")?.optLong("toolbar_context_generation") == generation)
                    host.dispatch(obj("type" to "tool_setting_contact", "id" to id, "phase" to phase))
                }
            }
            Box(Modifier.testTag("tool-setting-$id")) {
                if (compact) Row(Modifier.fillMaxWidth().height(metrics.height), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.spacing)) {
                    Text(field.getString("label"), Modifier.weight(1f), maxLines = 1)
                    NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), id = id, inline = true, showSlider = false, onEditPhase = phase) {
                        host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                    }
                } else NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), id = id, onEditPhase = phase,
                    onReset = { if (host.snapshot?.getJSONObject("state")?.optLong("toolbar_context_generation") == generation) host.dispatch(obj("type" to "reset_tool_setting", "id" to id)) }) {
                    if (host.snapshot?.getJSONObject("state")?.optLong("toolbar_context_generation") == generation) host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
            }
        }
        state.array("tool_settings").objects().fold(mutableListOf<MutableList<JSONObject>>()) { groups, field ->
            if (groups.lastOrNull()?.last()?.getString("group_id") == field.getString("group_id")) groups.last().add(field)
            else groups.add(mutableListOf(field))
            groups
        }.forEachIndexed { index, fields ->
            val choice = beside[fields.first().getString("id")]
            val title = choice?.getString("label") ?: fields.first().getString("group")
            if (title.isNotEmpty()) Text(title, Modifier.padding(top=if(index>0)2.dp else 0.dp), fontWeight = FontWeight.Bold, color = LocalPalette.current.secondary)
            if (choice == null) fields.forEach { fieldControl(it, false) }
            else Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.compound)) {
                ToolbarChoice(choice, false, false, false, 20, host::dispatch, prefix = "tool")
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) { fields.forEach { fieldControl(it, true) } }
            }
        }
        ToolActionRows(host,state,actions.filterNot {it.optBoolean("segmented")},commands)
        }
    }
}

@Composable private fun ToolActionRows(host:CanvasHost,state:JSONObject,actions:List<JSONObject>,commands:Map<String,JSONObject>) {
    val metrics=panelMetrics()
    val groups=host.catalog.array("tool_action_groups").objects().associateBy {it.getString("id")}
    val labelWidth=panelLabelWidth(actions.mapNotNull {it.optString("group").takeIf(String::isNotEmpty)}.distinct().map {groups.getValue(it).getString("label")})
    val completed=mutableSetOf<String>()
    actions.forEach {action ->
        val group=action.optString("group")
        val row=action.optString("row")
        if(group.isNotEmpty()) {
            if(completed.add(group)) {
                val choices=actions.filter {it.optString("group")==group}.map {commands.getValue(it.getString("command"))}
                val label=groups.getValue(group).getString("label")
                Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(metrics.spacing)) {
                    Text(label,Modifier.width(labelWidth),maxLines=1,overflow=TextOverflow.Ellipsis)
                    Box(Modifier.weight(1f)) {PropertyChoice(label,choices.map {it.getString("label")},choices.indexOfFirst {it.getBoolean("selected")},choices.any {it.getBoolean("enabled")},
                        modifier=Modifier.testTag("tool-action-group-$group"),optionEnabled={choices[it].getBoolean("enabled")},optionTag={"tool-action-${choices[it].getString("id")}"}) {host.invoke(choices[it].getString("id"))}}
                }
            }
        } else if(row.isNotEmpty()) {
            if(completed.add(row)) Row(Modifier.fillMaxWidth().testTag("tool-action-row-$row"),horizontalArrangement=Arrangement.spacedBy(metrics.spacing)) {
                actions.filter {it.optString("row")==row}.forEach {spec -> ToolActionButton(host,state,spec,commands.getValue(spec.getString("command")),if(spec.optBoolean("icon_only"))Modifier else Modifier.weight(1f))}
            }
        } else ToolActionButton(host,state,action,commands.getValue(action.getString("command")),Modifier.fillMaxWidth())
    }
}

@Composable private fun ToolActionButton(host:CanvasHost,state:JSONObject,action:JSONObject,command:JSONObject,modifier:Modifier) {
    val metrics=panelMetrics()
    val id=command.getString("id")
    val enabled=command.getBoolean("enabled")
    val selected=command.getBoolean("selected")
    val label=command.getString("label")
    val iconOnly=action.optBoolean("icon_only")
    val caption=state.objectOrNull("canvas_bar")?.array("completion")?.objects()?.firstOrNull {it.objectOrNull("option")?.objectOrNull("Action")?.getJSONObject("state")?.getString("id")==id}?.getString("label")?:label
    HoverTip(command.getString("tooltip"),modifier) {
        if(action.optBoolean("checkable")) Row(Modifier.fillMaxWidth().heightIn(min=metrics.height).testTag("tool-action-$id")
            .toggleable(selected,enabled=enabled,role=Role.Checkbox){host.invoke(id)},verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(metrics.spacing)) {
            EditorCheck(selected,label,Modifier.clearAndSetSemantics{},enabled){host.invoke(id)}
            Text(label,maxLines=1,overflow=TextOverflow.Ellipsis)
        } else Row((if(iconOnly)Modifier.size(metrics.height)else Modifier.fillMaxWidth().heightIn(min=metrics.height)).clip(ControlShape)
            .background(if(selected)LocalPalette.current.active else LocalPalette.current.button).alpha(if(enabled)1f else .4f)
            .clickable(enabled=enabled){host.invoke(id)}.padding(horizontal=if(iconOnly)0.dp else metrics.padding).testTag("tool-action-$id"),
            verticalAlignment=Alignment.CenterVertically,horizontalArrangement=if(iconOnly)Arrangement.Center else Arrangement.spacedBy(metrics.spacing)) {
            if(!command.isNull("icon")) SharedIcon(command.getString("icon"),if(iconOnly)label else null,Modifier.size(metrics.icon))
            if(!iconOnly)Text(caption,maxLines=1,overflow=TextOverflow.Ellipsis)
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
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(panelMetrics().spacing)) {
        ToolbarChoice(modes, false, false, false, 20, host::dispatch, prefix="tool", height=panelMetrics().height.value)
        state.array("tool_extra").objects().forEach { option ->
            ToolbarChoice(option.getJSONObject("Choice"), false, false, false, 20, host::dispatch, prefix="tool", height=panelMetrics().height.value)
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
            Row(Modifier.fillMaxWidth().height(panelMetrics().height).testTag("tool-setting-$id"), verticalAlignment=Alignment.CenterVertically, horizontalArrangement=Arrangement.spacedBy(panelMetrics().spacing)) {
                Text(field.getString("label"), Modifier.width(62.dp), maxLines=1)
                NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric"), Modifier.weight(1f), id=id, inline=true) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
            }
        }
    }
}

internal class PickerActivation(val modifier: Modifier, val click: () -> Unit)

@Composable internal fun pickerClick(host: CanvasHost, control: JSONObject?, anchor: JSONObject, activate: () -> Unit): PickerActivation {
    val current by rememberUpdatedState(activate)
    val picker=control?.optString("kind")=="color_picker" || control?.optString("kind")=="command" && control.optString("command")=="eyedropper"
    val configuration=androidx.compose.ui.platform.LocalViewConfiguration.current
    data class Press(val device: androidx.compose.ui.input.pointer.PointerType, val time: Long)
    var pending by remember(anchor.toString(), picker) { mutableStateOf<Press?>(null) }
    var previous by remember(anchor.toString(), picker) { mutableStateOf<Press?>(null) }
    val modifier=if(!picker) Modifier else Modifier.onPreviewKeyEvent { pending=null; previous=null; false }.pointerInput(anchor.toString()) {
        awaitEachGesture {
            val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial)
            pending=Press(down.type,down.uptimeMillis)
            try {
                do { val event=awaitPointerEvent(PointerEventPass.Final) } while(event.changes.any { it.pressed })
            } finally { pending=null }
        }
    }
    return PickerActivation(modifier) {
        val press=pending; pending=null
        val last=previous; previous=null
        if(picker && press!=null && last!=null && press.device==last.device &&
            press.time-last.time in 0..configuration.doubleTapTimeoutMillis) {
            host.dispatch(obj("type" to "color_picker","action" to obj("kind" to "settings","anchor" to anchor)))
        } else { if(picker)previous=press; current() }
    }
}
