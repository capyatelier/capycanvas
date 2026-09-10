package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.*
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.*
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

/** A native text field and slider; Rust owns numeric semantics. Tapping a value
 * opens the keyboard. */
@Composable internal fun NumericSetting(label: String, value: Float, control: JSONObject,
    modifier: Modifier = Modifier, enabled: Boolean = true, description: String = "",
    settings: Boolean = false, id: String = label, inline: Boolean = false, onChange: (Float) -> Unit) {
    val host = LocalCanvasHost.current
    val colors = LocalPalette.current
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    val ranged = control.getString("kind") == "slider"
    val displayKey = if (ranged) "edit" else "text"
    fun resolve(value: Float, op: JSONObject) = JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to op).toString()))
    var shown by remember(value, control.toString()) { mutableStateOf(resolve(value, obj("type" to "format"))) }
    var editing by remember { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    var text by remember { mutableStateOf(TextFieldValue(shown.getString(displayKey))) }
    var error by remember { mutableStateOf<String?>(null) }
    val height = if (settings) 48.dp else if (ranged) 24.dp else 32.dp
    val valuePadding = if (settings) 12.dp else 6.dp
    val measurer = rememberTextMeasurer()
    val widest = remember(inline, control.toString()) {
        if (inline) listOf(control.number("min"), control.number("max")).map { resolve(it, obj("type" to "format")).getString("text") }
            .maxBy { it.length }.replace(Regex("[0-9]"), "8") else ""
    }
    val fixedWidth = if (inline) with(LocalDensity.current) { measurer.measure(widest, LocalTextStyle.current).size.width.toDp() } + valuePadding * 2 else 0.dp
    fun apply(op: JSONObject): Boolean = try {
        val next = resolve(shown.number("value"), op)
        val changed = next.number("value") != shown.number("value")
        shown = next; error = null
        if (changed) onChange(next.number("value"))
        true
    } catch (e: Exception) { error = e.message ?: "Enter a number"; false }
    fun finish(cancel: Boolean = false): Boolean {
        if (!editing) return true
        if (!cancel && !apply(obj("type" to "expression", "text" to text.text))) return false
        editing = false; error = null; text = TextFieldValue(shown.getString(displayKey))
        return true
    }
    LaunchedEffect(value, editing) { if (!editing) text = TextFieldValue(shown.getString(displayKey)) }
    LaunchedEffect(editing) { if (editing && ranged) requester.requestFocus() }
    DisposableEffect(Unit) { onDispose { if (focused) host.editingText = false } }
    val step: @Composable (Int, String) -> Unit = { direction, name ->
        val available = enabled && if (direction < 0) shown.number("value") > control.number("min") else shown.number("value") < control.number("max")
        Box(Modifier.size(height).clip(RoundedCornerShape(6.dp)).alpha(if (available) 1f else .36f)
            .clickable(enabled = available) { if (finish()) { focus.clearFocus(); apply(obj("type" to "step", "steps" to direction)) } }, contentAlignment = Alignment.Center) {
            SharedIcon(name, "${if (direction < 0) "Decrease" else "Increase"} $label", Modifier.size(16.dp))
        }
    }
    val field: @Composable () -> Unit = {
        BasicTextField(text, { text = it }, Modifier.then(if (inline) Modifier.width(fixedWidth) else if (ranged) Modifier.widthIn(min = 48.dp, max = 100.dp).width(IntrinsicSize.Min) else Modifier.width(if (control.optString("unit").isEmpty()) 60.dp else 80.dp)).height(height)
            .focusRequester(requester).onFocusChanged {
                if (focused && !it.isFocused) finish()
                focused = it.isFocused; host.editingText = focused
                if (focused) editing = true
            }.onPreviewKeyEvent {
                if (it.type == KeyEventType.KeyDown && it.key == Key.Escape) { finish(true); focus.clearFocus(); true }
                else if (it.type == KeyEventType.KeyDown && it.key == Key.Enter) { if (finish()) focus.clearFocus(); true }
                else false
            }.testTag(if (settings) "setting-number-$id" else "number-$label"), enabled = enabled, singleLine = true,
            textStyle = LocalTextStyle.current.copy(color = colors.text, textAlign = TextAlign.End),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (finish()) focus.clearFocus() }),
            decorationBox = { input -> Box(Modifier.fillMaxSize().background(colors.input, RoundedCornerShape(6.dp)).padding(horizontal = valuePadding), contentAlignment = Alignment.CenterEnd) { input() } })
    }
    val valueControl: @Composable () -> Unit = {
        if (editing) field()
        else Box(Modifier.then(if (inline) Modifier.width(fixedWidth) else Modifier).height(height).clip(RoundedCornerShape(6.dp)).clickable(enabled = enabled) {
            val edit = shown.getString("edit"); text = TextFieldValue(edit, TextRange(0, edit.length)); editing = true
        }.padding(horizontal = valuePadding).testTag("number-value-$id"), contentAlignment = Alignment.CenterEnd) { Text(shown.getString("text")) }
    }
    if (inline) {
        Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            EditorSlider(shown.number("fill"), { finish(true); apply(obj("type" to "position", "position" to it)) }, Modifier.weight(1f),
                enabled = enabled, label = label, height = height, inactiveTrackColor = colors.input, showThumb = false, activeTrackColor = colors.sliderFill)
            valueControl()
        }
        return
    }
    Column(modifier.widthIn(max = if (settings) 600.dp else 1000.dp).fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Column(Modifier.weight(1f).padding(start = if (settings) 0.dp else 6.dp).testTag("preference-label-$id"), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(label, color = if (enabled) colors.text else colors.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (description.isNotEmpty()) Text(description, color = colors.settingsSecondary, fontSize = 14.sp, lineHeight = 20.sp)
            }
            if (!ranged) Row(Modifier.clip(RoundedCornerShape(6.dp)).background(colors.input), verticalAlignment = Alignment.CenterVertically) { field(); step(-1, "minus"); step(1, "plus") }
            else valueControl()
        }
        if (ranged) Row(Modifier.fillMaxWidth().padding(top = if (settings) 3.dp else 0.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            step(-1, "minus")
            EditorSlider(shown.number("fill"), { finish(true); apply(obj("type" to "position", "position" to it)) },
                Modifier.weight(1f).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                enabled = enabled, label = label, height = height,
                inactiveTrackColor = if (settings) colors.divider else colors.input, showThumb = settings,
                activeTrackColor = if (settings) colors.accent else colors.sliderFill)
            step(1, "plus")
        }
        error?.let { Text(it, color = colors.accent, fontSize = 12.sp) }
    }
}
