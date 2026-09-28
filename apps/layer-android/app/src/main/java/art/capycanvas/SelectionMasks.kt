package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject
import kotlin.math.roundToInt

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

private val RefineGap = 12.dp
private val RefineMaxWidth = 440.dp

/** The Refine panel: one value for Grow, Shrink, Feather, Border or Smooth.
 * The session previews every value on the canvas, which stays visible and
 * undimmed above the panel. It sits at the bottom of the canvas, above a canvas
 * action bar along that edge. It is not a popup, so it never takes window focus
 * or cancels a selection or transform. Back cancels it. */
@Composable internal fun SelectionRefinePanel(host: CanvasHost, dock: DockInteraction, status: JSONObject, state: JSONObject) {
    val view = state.getJSONObject("layer_tools").objectOrNull("selection_resize") ?: return
    fun send(action: JSONObject) = host.dispatch(obj("type" to "selection", "action" to action))
    BackHandler { send(obj("op" to "cancel_resize")) }
    val colors = LocalPalette.current
    Surface(Modifier.zIndex(240f).layout { measurable, constraints ->
        val gap = RefineGap.toPx()
        val left = status.number("x") * density
        val width = status.number("width") * density
        val top = status.number("y") * density
        val bar = dock.canvasBarSlot?.takeIf { it.bottom > top - 2 * gap }
        val bottom = minOf(top, bar?.top ?: top)
        val panel = measurable.measure(Constraints(maxWidth = minOf(RefineMaxWidth.toPx(), width - 2 * gap).roundToInt().coerceAtLeast(0)))
        layout(constraints.maxWidth, constraints.maxHeight) {
            panel.place((left + (width - panel.width) / 2).roundToInt(), (bottom - gap - panel.height).roundToInt())
        }
    }.chromeRegion(dock).testTag("selection-refine-panel"), shape = SurfaceShape, color = colors.panel, contentColor = colors.text,
        shadowElevation = 8.dp) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(view.getString("title"), Modifier.padding(end = 8.dp), fontWeight = FontWeight.Bold)
            key(view.getString("kind")) {
                NumericSetting(view.getString("label"), view.number("radius"), view.getJSONObject("numeric"),
                    Modifier.padding(end = 8.dp).testTag("selection-refine-value"), settings = true, id = "selection-refine") {
                    send(obj("op" to "resize_radius", "radius" to it))
                }
            }
            Row(Modifier.align(Alignment.End).padding(bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                RefineButton("Cancel", "selection-refine-cancel", null) { send(obj("op" to "cancel_resize")) }
                RefineButton("Apply", "selection-refine-apply", colors.accent) { send(obj("op" to "apply_resize")) }
            }
        }
    }
}

@Composable private fun RefineButton(label: String, tag: String, fill: Color?, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(Modifier.heightIn(min = 40.dp).clip(ControlShape).background(fill ?: Color.Transparent).focusProperties { canFocus = false }
        .testTag(tag).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        Text(label, color = if (fill == null) colors.text else colors.accentForeground, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}
