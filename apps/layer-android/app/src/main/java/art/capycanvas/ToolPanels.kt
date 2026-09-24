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
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
import org.json.JSONObject

/** The selected tool determines groups, subtools and settings in Rust. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun ToolSetControls(host: CanvasHost, state: JSONObject, panel: String = "brushes") {
    val view = state.getJSONObject("tool_panels").optJSONObject(panel) ?: state.getJSONObject("tool_set")
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (panel == "brush_sets" || panel == "sculpt_sets") Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("groups").objects().forEach { item ->
                ToolChoice(host, item, "set", Modifier.fillMaxWidth().testTag("$panel-${item.getString("label")}"))
            }
        } else FlowRow(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("groups").objects().forEach { item ->
                ToolChoice(host, item, "group", (if (view.array("groups").length() == 1) Modifier.fillMaxWidth() else Modifier.width(108.dp)).testTag("tool-group-${item.getString("label")}"))
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("subtools").objects().forEach { item ->
                ToolChoice(host, item, "subtool", Modifier.fillMaxWidth().testTag("subtool-${item.getString("label")}"))
            }
        }
    }
}

@Composable private fun ToolChoice(host: CanvasHost, item: JSONObject, kind: String, modifier: Modifier) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    val label = item.getString("label")
    val action = item.getJSONObject("action")
    val preview = item.opt("preview").takeIf { it is Number } as? Number
    ActionTip(host, label, action, modifier) {
        Column(Modifier.fillMaxWidth().clip(ControlShape)
            .background(if (item.optBoolean("selected")) colors.active else colors.panel)
            .clickable { host.dispatch(action) }.padding(horizontal = 6.dp, vertical = 3.dp)) {
            if (preview != null) {
                val id = preview.toInt()
                val swatch = remember(id, colors.dark) {
                    context.assets.open("$id-${if (colors.dark) "dark" else "light"}.png").use { BitmapFactory.decodeStream(it).asImageBitmap() }
                }
                Image(swatch, null, Modifier.fillMaxWidth().height(40.dp).testTag("brush-preview-$id"), contentScale = ContentScale.FillBounds)
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    SharedIcon(item.getString("icon"), null, Modifier.testTag("tool-$kind-icon-$label"))
                    Text(label, Modifier.weight(1f), textAlign = TextAlign.End, fontWeight = FontWeight.Bold)
                }
            } else Row(Modifier.heightIn(min = 42.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                SharedIcon(item.getString("icon"), null, Modifier.testTag("tool-$kind-icon-$label"))
                Text(label, fontWeight = FontWeight.Bold, maxLines = if (kind == "set") 1 else 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable internal fun ToolSettingsControls(host: CanvasHost, state: JSONObject) {
    val modes = setOf("selection_new", "selection_add", "selection_subtract", "selection_intersect")
    val actions = state.array("tool_actions").objects()
    val commands = state.array("commands").objects().associateBy { it.getString("id") }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (actions.any { it.getString("command") in modes }) Row(Modifier.fillMaxWidth().clip(ControlShape).border(1.dp, LocalPalette.current.divider, ControlShape).selectableGroup().testTag("selection-mode-row")) {
            actions.filter { it.getString("command") in modes }.forEach { action ->
                val id = action.getString("command")
                val command = commands.getValue(id)
                val selected = command.getBoolean("selected")
                HoverTip(command.getString("tooltip"), Modifier.weight(1f)) {
                    Box(Modifier.fillMaxWidth().height(48.dp).testTag("tool-action-$id")
                        .background(if (selected) LocalPalette.current.active else LocalPalette.current.panel)
                        .selectable(selected = selected, enabled = command.getBoolean("enabled"), role = Role.RadioButton) { host.invoke(id) },
                        contentAlignment = Alignment.Center) {
                        SharedIcon(command.getString("icon"), command.getString("label"), Modifier.size(20.dp))
                    }
                }
            }
        }
        if (actions.any { it.getString("command") in modes }) SelectionMenuButton(host, "Selection Actions…", "selection")
        var group = ""
        state.array("tool_settings").objects().forEach { field ->
            val next = field.getString("group")
            if (next != group && next.isNotEmpty()) Text(next, fontWeight = FontWeight.Bold, color = LocalPalette.current.secondary)
            group = next
            val id = field.getString("id")
            Box(Modifier.testTag("tool-setting-$id")) {
                NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric")) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
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
