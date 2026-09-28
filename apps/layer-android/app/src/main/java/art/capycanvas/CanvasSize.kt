package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import org.json.JSONObject

private val AnchorCellSize = 40.dp

/** The Canvas Size panel. Rust owns sizes, units, limits and history. */
@Composable internal fun CanvasSizePanel(host: CanvasHost, dock: DockInteraction, workArea: JSONObject, state: JSONObject) {
    val view = state.getJSONObject("layer_tools").objectOrNull("canvas_size") ?: return
    val colors = LocalPalette.current
    SizePanel(host, dock, workArea, "canvas_size", "canvas-size", view) { actions ->
        SizeAxes(actions, view, "canvas-size")
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            SizeUnits(actions, view, "canvas-size", Modifier.weight(1f))
            SizeCheck(actions, view.getString("relative_label"), view.getBoolean("relative"), "relative", "canvas-size-relative")
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(view.getString("anchor_label"), Modifier.weight(1f))
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                view.getJSONArray("anchors").objects().chunked(3).forEach { row ->
                    Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                        row.forEach { choice ->
                            val anchor = choice.getString("anchor")
                            val selected = anchor == view.getString("anchor")
                            Box(Modifier.size(AnchorCellSize).clip(ControlShape).background(if (selected) colors.active else colors.input)
                                .focusProperties { canFocus = false }.testTag("canvas-size-anchor-$anchor")
                                .semantics { contentDescription = choice.getString("label") }
                                .selectable(selected, role = Role.RadioButton) { actions.choose(obj("op" to "anchor", "anchor" to anchor)) },
                                contentAlignment = Alignment.Center) { if (selected) SharedIcon("rectangle-fill", null) }
                        }
                    }
                }
            }
        }
    }
}
