package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
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
        menu?.let { WorkspaceMenu(host, it, copy = { host.menuCopy(obj("type" to "selection_menu", "kind" to kind)) }) { menu = null } }
    }
}
