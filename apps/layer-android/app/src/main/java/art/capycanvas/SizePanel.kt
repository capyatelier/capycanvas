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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject
import kotlin.math.roundToInt

private val SizePanelGap = 12.dp
private val SizePanelMaxWidth = 400.dp

/** The value typed so far, or null while the text is not yet a number. */
private fun typedValue(control: JSONObject, value: Float, text: String, language: String): Float? = runCatching {
    JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "expression", "text" to text)).toString(), language)).number("value")
}.getOrNull()

internal class SizePanelActions(private val dispatch: (JSONObject) -> Unit, private val clearFocus: () -> Unit) {
    private val commits = linkedMapOf<Any, (Boolean) -> Boolean>()
    fun register(owner: Any, commit: ((Boolean) -> Boolean)?) {
        if (commit == null) commits.remove(owner) else commits[owner] = commit
    }
    fun send(action: JSONObject) = dispatch(action)
    fun endTyping(cancel: Boolean = false): Boolean {
        val accepted = commits.values.toList().map { it(cancel) }.all { it }
        if (accepted) clearFocus()
        return accepted
    }
    fun choose(action: JSONObject) {
        if (endTyping(action.getString("op") == "cancel")) send(action)
    }
}

/** The Canvas Size and Image Size panels: in the main window at the top of the
 * work area, over the undimmed canvas, above the keyboard. They take window
 * focus only while a number field is edited, and Back cancels them. */
@Composable internal fun SizePanel(host: CanvasHost, dock: DockInteraction, workArea: JSONObject, type: String, tag: String,
    view: JSONObject, content: @Composable ColumnScope.(SizePanelActions) -> Unit) {
    val focus = LocalFocusManager.current
    val actions = remember(host, type, focus) { SizePanelActions({ host.dispatch(obj("type" to type, "action" to it)) }, { focus.clearFocus() }) }
    BackHandler { actions.choose(obj("op" to "cancel")) }
    val colors = LocalPalette.current
    val ime = WindowInsets.ime
    Surface(Modifier.zIndex(240f).layout { measurable, constraints ->
        val gap = SizePanelGap.toPx()
        val left = workArea.number("x") * density
        val width = workArea.number("width") * density
        val top = workArea.number("y") * density
        val panel = measurable.measure(Constraints(maxWidth = minOf(SizePanelMaxWidth.toPx(), width - 2 * gap).roundToInt().coerceAtLeast(0)))
        val floor = constraints.maxHeight - ime.getBottom(this) - gap
        layout(constraints.maxWidth, constraints.maxHeight) {
            panel.place((left + (width - panel.width) / 2).roundToInt(), minOf(top + gap, floor - panel.height).coerceAtLeast(0f).roundToInt())
        }
    }.chromeRegion(dock).testTag("$tag-panel"), shape = SurfaceShape, color = colors.panel, contentColor = colors.text,
        shadowElevation = 8.dp) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(view.getString("title"), fontWeight = FontWeight.Bold)
            content(actions)
            Text(view.getString("message"), Modifier.testTag("$tag-message"), color = colors.secondary)
            Row(Modifier.align(Alignment.End), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SizePanelButton(view.getString("cancel_label"), "$tag-cancel", null, true) { actions.choose(obj("op" to "cancel")) }
                SizePanelButton(view.getString("apply_label"), "$tag-apply", colors.accent, view.getBoolean("can_apply")) { actions.choose(obj("op" to "apply")) }
            }
        }
    }
}

/** The Width and Height fields of a size dialog's view. */
@Composable internal fun SizeAxes(actions: SizePanelActions, view: JSONObject, tag: String) {
    val labels = view.getJSONArray("labels")
    val values = view.getJSONArray("values")
    val numeric = view.getJSONArray("numeric")
    listOf("width", "height").forEachIndexed { axis, op ->
        SizeNumber(actions, labels.getString(axis), values.getDouble(axis).toFloat(), numeric.getJSONObject(axis), "$tag-$op", op)
    }
}

/** A number field whose typed values reach the draft as soon as they read as numbers. */
@Composable internal fun SizeNumber(actions: SizePanelActions, label: String, value: Float, control: JSONObject, id: String, op: String) {
    val host = LocalCanvasHost.current
    val register = remember(actions) { { owner: Any, commit: ((Boolean) -> Boolean)? -> actions.register(owner, commit) } }
    key(control.toString()) {
        NumericSetting(label, value, control, id = id,
            registerCommit = register,
            onText = { text -> if (text.composition == null) typedValue(control, value, text.text, host.languageTag)?.takeIf { it != value }?.let { actions.send(obj("op" to op, "value" to it)) } }) {
            actions.send(obj("op" to op, "value" to it))
        }
    }
}

/** Pixels or Percent, as segments that never take focus. */
@Composable internal fun SizeUnits(actions: SizePanelActions, view: JSONObject, tag: String, modifier: Modifier) {
    val colors = LocalPalette.current
    Row(modifier.height(IntrinsicSize.Min).clip(ControlShape).background(colors.input)) {
        view.getJSONArray("units").objects().forEach { choice ->
            val unit = choice.getString("unit")
            val selected = unit == view.getString("unit")
            Box(Modifier.weight(1f).fillMaxHeight().heightIn(min = 40.dp).clip(ControlShape).background(if (selected) colors.active else Color.Transparent)
                .focusProperties { canFocus = false }.testTag("$tag-unit-$unit")
                .selectable(selected, role = Role.RadioButton) { actions.choose(obj("op" to "unit", "unit" to unit)) },
                contentAlignment = Alignment.Center) { Text(choice.getString("label"), Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp), textAlign = TextAlign.Center) }
        }
    }
}

/** A checkbox row that sends its state as the `key` field of an action. */
@Composable internal fun SizeCheck(actions: SizePanelActions, label: String, checked: Boolean, key: String, tag: String) {
    Row(Modifier.heightIn(min = 40.dp).clip(ControlShape).focusProperties { canFocus = false }.testTag(tag)
        .toggleable(checked, role = Role.Checkbox) { actions.choose(obj("op" to key, key to it)) }.padding(end = 8.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Checkbox(checked, onCheckedChange = null, Modifier.padding(horizontal = 8.dp))
        Text(label)
    }
}

@Composable private fun SizePanelButton(label: String, tag: String, fill: Color?, enabled: Boolean, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(Modifier.heightIn(min = 40.dp).alpha(if (enabled) 1f else .36f).clip(ControlShape).background(fill ?: Color.Transparent)
        .focusProperties { canFocus = false }.testTag(tag).clickable(enabled = enabled, role = Role.Button, onClick = onClick)
        .padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        Text(label, color = if (fill == null) colors.text else colors.accentForeground, fontWeight = FontWeight.Bold, maxLines = 1)
    }
}
