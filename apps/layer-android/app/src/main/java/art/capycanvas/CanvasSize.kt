package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject
import kotlin.math.roundToInt

private val CanvasSizeGap = 12.dp
private val CanvasSizeMaxWidth = 400.dp
private val AnchorCellSize = 40.dp

/** The value typed so far, or null while the text is not yet a number. */
private fun typedValue(control: JSONObject, value: Float, text: String): Float? = runCatching {
    JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "expression", "text" to text)).toString())).number("value")
}.getOrNull()

/** The Canvas Size panel. Rust owns sizes, units, limits and history. It sits
 * in the main window at the top of the work area, over the undimmed canvas,
 * and moves above the keyboard. It takes window focus only while a number
 * field is edited; typed values reach the draft as they become numbers, and
 * every other choice commits the field first. Back cancels it. */
@Composable internal fun CanvasSizePanel(host: CanvasHost, dock: DockInteraction, workArea: JSONObject, state: JSONObject) {
    val view = state.getJSONObject("layer_tools").objectOrNull("canvas_size") ?: return
    val focus = LocalFocusManager.current
    fun send(action: JSONObject) = host.dispatch(obj("type" to "canvas_size", "action" to action))
    fun choose(action: JSONObject) { focus.clearFocus(); send(action) }
    BackHandler { choose(obj("op" to "cancel")) }
    val colors = LocalPalette.current
    val ime = WindowInsets.ime
    Surface(Modifier.zIndex(240f).layout { measurable, constraints ->
        val gap = CanvasSizeGap.toPx()
        val left = workArea.number("x") * density
        val width = workArea.number("width") * density
        val top = workArea.number("y") * density
        val panel = measurable.measure(Constraints(maxWidth = minOf(CanvasSizeMaxWidth.toPx(), width - 2 * gap).roundToInt().coerceAtLeast(0)))
        val floor = constraints.maxHeight - ime.getBottom(this) - gap
        layout(constraints.maxWidth, constraints.maxHeight) {
            panel.place((left + (width - panel.width) / 2).roundToInt(), minOf(top + gap, floor - panel.height).coerceAtLeast(0f).roundToInt())
        }
    }.chromeRegion(dock).testTag("canvas-size-panel"), shape = SurfaceShape, color = colors.panel, contentColor = colors.text,
        shadowElevation = 8.dp) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(view.getString("title"), fontWeight = FontWeight.Bold)
            val labels = view.getJSONArray("labels")
            val values = view.getJSONArray("values")
            val numeric = view.getJSONArray("numeric")
            listOf("width", "height").forEachIndexed { axis, op ->
                val control = numeric.getJSONObject(axis)
                val value = values.getDouble(axis).toFloat()
                key(control.toString()) {
                    NumericSetting(labels.getString(axis), value, control, settings = true, id = "canvas-size-$op",
                        onText = { text -> typedValue(control, value, text)?.takeIf { it != value }?.let { send(obj("op" to op, "value" to it)) } }) {
                        send(obj("op" to op, "value" to it))
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(Modifier.weight(1f).clip(ControlShape).background(colors.input)) {
                    view.getJSONArray("units").objects().forEach { choice ->
                        val unit = choice.getString("unit")
                        val selected = unit == view.getString("unit")
                        Box(Modifier.weight(1f).heightIn(min = 40.dp).clip(ControlShape).background(if (selected) colors.active else Color.Transparent)
                            .focusProperties { canFocus = false }.testTag("canvas-size-unit-$unit")
                            .selectable(selected, role = Role.RadioButton) { choose(obj("op" to "unit", "unit" to unit)) },
                            contentAlignment = Alignment.Center) { Text(choice.getString("label"), maxLines = 1) }
                    }
                }
                val relative = view.getBoolean("relative")
                Row(Modifier.heightIn(min = 40.dp).clip(ControlShape).focusProperties { canFocus = false }.testTag("canvas-size-relative")
                    .toggleable(relative, role = Role.Checkbox) { choose(obj("op" to "relative", "relative" to it)) }.padding(end = 8.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(relative, onCheckedChange = null, Modifier.padding(horizontal = 8.dp))
                    Text(view.getString("relative_label"))
                }
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
                                    .selectable(selected, role = Role.RadioButton) { choose(obj("op" to "anchor", "anchor" to anchor)) },
                                    contentAlignment = Alignment.Center) { if (selected) SharedIcon("rectangle-fill", null) }
                            }
                        }
                    }
                }
            }
            Text(view.getString("message"), Modifier.testTag("canvas-size-message"), color = colors.secondary)
            Row(Modifier.align(Alignment.End), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                CanvasSizeButton("Cancel", "canvas-size-cancel", null, true) { choose(obj("op" to "cancel")) }
                CanvasSizeButton("Apply", "canvas-size-apply", colors.accent, view.getBoolean("can_apply")) { choose(obj("op" to "apply")) }
            }
        }
    }
}

@Composable private fun CanvasSizeButton(label: String, tag: String, fill: Color?, enabled: Boolean, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(Modifier.heightIn(min = 40.dp).alpha(if (enabled) 1f else .36f).clip(ControlShape).background(fill ?: Color.Transparent)
        .focusProperties { canFocus = false }.testTag(tag).clickable(enabled = enabled, role = Role.Button, onClick = onClick)
        .padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        Text(label, color = if (fill == null) colors.text else colors.accentForeground, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}
