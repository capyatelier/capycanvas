package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject

/** The Image Size panel. Rust owns sizes, units, resampling, limits and history. */
@Composable internal fun ImageSizePanel(host: CanvasHost, dock: DockInteraction, workArea: JSONObject, state: JSONObject) {
    val view = state.getJSONObject("layer_tools").objectOrNull("image_size") ?: return
    val colors = LocalPalette.current
    val resampleMenu = remember { WindowlessMenuButton() }
    SizePanel(host, dock, workArea, "image_size", "image-size", view) { actions ->
        SizeAxes(actions, view, "image-size")
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            SizeUnits(actions, view, "image-size", Modifier.weight(1f))
            SizeCheck(actions, view.getString("constrain_label"), view.getBoolean("constrain"), "constrain", "image-size-constrain")
        }
        SizeNumber(actions, view.getString("resolution_label"), view.number("resolution"), view.getJSONObject("resolution_numeric"),
            "image-size-resolution", "resolution")
        val resamples = view.getJSONArray("resamples").objects()
        val chosen = view.getString("resample")
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(view.getString("resample_label"))
            Box(Modifier.weight(1f)) {
                Row(Modifier.fillMaxWidth().heightIn(min = 40.dp).clip(ControlShape).background(colors.input)
                    .focusProperties { canFocus = false }.testTag("image-size-resample")
                    .opensWindowlessMenu(resampleMenu, view.getString("resample_label")) { open ->
                        if (actions.endTyping()) open(obj("sections" to JSONArray().put(JSONArray(resamples.map { choice ->
                            val resample = choice.getString("resample")
                            obj("label" to choice.getString("label"), "selected" to (resample == chosen),
                                "command" to obj("op" to "resample", "resample" to resample))
                        }))))
                    }.padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(resamples.first { it.getString("resample") == chosen }.getString("label"), Modifier.weight(1f),
                        maxLines = 1, overflow = TextOverflow.Ellipsis)
                    SharedIcon("chevron-down", null, Modifier.size(12.dp))
                }
                WindowlessMenuHost(host, resampleMenu, actions::choose)
            }
        }
    }
}
