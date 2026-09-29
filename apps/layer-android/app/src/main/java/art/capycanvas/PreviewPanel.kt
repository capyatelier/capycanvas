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

private val PanelGap = 12.dp
private val PanelMaxWidth = 440.dp

/** The ops a one-value dialog sends in the actions of `type`. */
internal class PreviewOps(val type: String, val value: String, val apply: String, val cancel: String)

/** A shared one-value dialog the session previews on the canvas: Refine (Grow,
 * Shrink, Feather, Border or Smooth) or Frequency Separation. The canvas stays
 * visible and undimmed above the panel. It sits at the bottom of the canvas,
 * above a canvas action bar along that edge. It is not a popup, so it never
 * takes window focus or cancels a selection or transform. Back cancels it. */
@Composable internal fun PreviewPanel(host: CanvasHost, dock: DockInteraction, status: JSONObject, view: JSONObject?, name: String, ops: PreviewOps) {
    view ?: return
    fun send(action: JSONObject) = host.dispatch(obj("type" to ops.type, "action" to action))
    BackHandler { send(obj("op" to ops.cancel)) }
    val colors = LocalPalette.current
    Surface(Modifier.zIndex(240f).layout { measurable, constraints ->
        val gap = PanelGap.toPx()
        val left = status.number("x") * density
        val width = status.number("width") * density
        val top = status.number("y") * density
        val bar = dock.canvasBarSlot?.takeIf { it.bottom > top - 2 * gap }
        val bottom = minOf(top, bar?.top ?: top)
        val panel = measurable.measure(Constraints(maxWidth = minOf(PanelMaxWidth.toPx(), width - 2 * gap).roundToInt().coerceAtLeast(0)))
        layout(constraints.maxWidth, constraints.maxHeight) {
            panel.place((left + (width - panel.width) / 2).roundToInt(), (bottom - gap - panel.height).roundToInt())
        }
    }.chromeRegion(dock).testTag("$name-panel"), shape = SurfaceShape, color = colors.panel, contentColor = colors.text,
        shadowElevation = 8.dp) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(view.getString("title"), Modifier.padding(end = 8.dp), fontWeight = FontWeight.Bold)
            key(view.optString("kind", view.getString("label"))) {
                NumericSetting(view.getString("label"), view.number("radius"), view.getJSONObject("numeric"),
                    Modifier.padding(end = 8.dp).testTag("$name-value"), settings = true, id = name) {
                    send(obj("op" to ops.value, "radius" to it))
                }
            }
            Row(Modifier.align(Alignment.End).padding(bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PanelButton("Cancel", "$name-cancel", null) { send(obj("op" to ops.cancel)) }
                PanelButton("Apply", "$name-apply", colors.accent) { send(obj("op" to ops.apply)) }
            }
        }
    }
}

@Composable private fun PanelButton(label: String, tag: String, fill: Color?, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(Modifier.heightIn(min = 40.dp).clip(ControlShape).background(fill ?: Color.Transparent).focusProperties { canFocus = false }
        .testTag(tag).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        Text(label, color = if (fill == null) colors.text else colors.accentForeground, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}
