package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.json.JSONObject

/** Rust supplies menu policy and the independent mask color state. */
internal fun JSONObject.displayColors(): JSONObject = objectOrNull("layer_tools")
    ?.objectOrNull("mask_editing")?.objectOrNull("colors") ?: getJSONObject("colors")

@Composable internal fun SelectionMenuButton(host: CanvasHost, label: String, kind: String, modifier: Modifier = Modifier, compact: Boolean = false) {
    var menu by remember { mutableStateOf<JSONObject?>(null) }
    Box(modifier) {
        val open = { host.query(obj("type" to "selection_menu", "kind" to kind)) { menu = it as? JSONObject } }
        if (compact) IconButton(open, Modifier.size(48.dp).testTag("selection-menu-$kind")) { SharedIcon("more", label) }
        else TextButton(open,
            Modifier.heightIn(min = 48.dp).testTag("selection-menu-$kind")) {
            Text(label); Spacer(Modifier.width(6.dp)); SharedIcon("chevron-down", null, Modifier.size(16.dp))
        }
        menu?.let { WorkspaceMenu(host, it) { menu = null } }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun SelectionMaskActions(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier) {
    val view = state.getJSONObject("layer_tools").objectOrNull("mask_editing") ?: return
    val enabled = state.array("commands").objects().find { it.getString("id") == "return_to_artwork" }?.optBoolean("enabled") == true
    Surface(modifier.testTag("selection-mask-actions"), color = LocalPalette.current.panel,
        shape = RoundedCornerShape(12.dp), shadowElevation = 4.dp) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(view.getString("label"), fontWeight = FontWeight.Bold)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                NumericSetting("Foreground mask gray", view.number("gray"), host.catalog.getJSONObject("opacity"),
                    Modifier.width(150.dp).heightIn(min = 48.dp).testTag("mask-gray"), settings = true, inline = true) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to "mask_gray", "value" to it))
                }
                TextButton({ host.invoke("swap_mask_colors") }, Modifier.heightIn(min = 48.dp).testTag("mask-swap")) { Text("Swap") }
                SelectionMenuButton(host, "Overlay", "overlay")
                TextButton({ host.invoke("return_to_artwork") }, Modifier.heightIn(min = 48.dp).testTag("mask-done"), enabled = enabled) { Text("Done") }
            }
            Text(view.optString("reason").takeIf { it.isNotEmpty() && it != "null" } ?: "Black protects · White selects", color = LocalPalette.current.secondary)
        }
    }
}

@Composable internal fun QuickMaskRow(host: CanvasHost, view: JSONObject) {
    if (!view.optBoolean("quick_mask")) return
    Surface(Modifier.fillMaxWidth().padding(6.dp).testTag("quick-mask-row"), color = LocalPalette.current.active, shape = RoundedCornerShape(6.dp)) {
        Row(Modifier.padding(6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            IconButton({ host.invoke("mask_overlay") }, Modifier.size(48.dp)) {
                SharedIcon(if(view.objectOrNull("mask_editing")?.optBoolean("overlay") == true) "eye" else "eye-hidden", "Show or hide mask overlay")
            }
            Column(Modifier.weight(1f)) { Text("Quick Mask", fontWeight = FontWeight.Bold); Text("Temporary", color = LocalPalette.current.secondary) }
            SelectionMenuButton(host, "Quick Mask actions", "quick_mask", compact = true)
        }
    }
}
