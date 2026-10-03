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
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
    settings: Boolean = false, id: String = label, inline: Boolean = false,
    toolbar: Boolean = false, showUnits: Boolean = true, showSlider: Boolean = true, valueOnly: Boolean = false,
    limits: ClosedFloatingPointRange<Float>? = null, onTyping: (Boolean) -> Unit = {}, onText: (TextFieldValue) -> Unit = {},
    registerCommit: (Any, ((Boolean) -> Boolean)?) -> Unit = { _, _ -> },
    onChange: (Float) -> Unit) {
    val host = LocalCanvasHost.current
    val captions = remember(label, host.languageTag) { JSONObject(Native.numericLabels(label, host.languageTag)) }
    val colors = LocalPalette.current
    val shape = if (settings) RoundedCornerShape(6.dp) else ControlShape
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    val ranged = control.getString("kind") == "slider"
    val displayKey = if (ranged) "edit" else "text"
    fun resolve(value: Float, op: JSONObject): JSONObject {
        val request = obj("control" to control, "value" to value, "operation" to op)
        return JSONObject(if (toolbar && !valueOnly) Native.toolbarUi(obj("type" to "number", "request" to request, "compact" to true, "units" to showUnits).toString(), host.languageTag) else Native.number(request.toString(), host.languageTag))
    }
    var shown by remember(value, control.toString(), showUnits, host.languageTag) { mutableStateOf(resolve(value, obj("type" to "format"))) }
    var editing by remember { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    var dirty by remember { mutableStateOf(false) }
    var fieldBounds by remember { mutableStateOf(Rect.Zero) }
    var text by remember { mutableStateOf(TextFieldValue(shown.getString(displayKey))) }
    var fieldValue by remember { mutableFloatStateOf(shown.number("value")) }
    var error by remember { mutableStateOf<String?>(null) }
    val height = if (settings) 48.dp else if (ranged || toolbar) 24.dp else 32.dp
    val valuePadding = if (settings) 12.dp else if (toolbar && !showUnits && !valueOnly) 2.dp else 6.dp
    val measurer = rememberTextMeasurer()
    val widest = remember(inline, control.toString(), showUnits, host.languageTag) {
        if (inline) (if (toolbar) JSONObject(Native.toolbarUi(obj("type" to "numeric_info", "id" to id, "control" to control, "compact" to true, "units" to showUnits).toString(), host.languageTag)).array("samples").let { samples -> (0 until samples.length()).map(samples::getString) }
            else listOf(control.number("min"), control.number("max")).map { resolve(it, obj("type" to "format")).getString("text") })
            .maxBy { it.length }.replace(Regex("[0-9]"), "8") else ""
    }
    val fixedWidth = if (inline) with(LocalDensity.current) { measurer.measure(if (valueOnly) shown.getString("text") else widest, LocalTextStyle.current).size.width.toDp() } + valuePadding * 2 + 2.dp else 0.dp
    fun apply(op: JSONObject): Boolean = try {
        var next = resolve(shown.number("value"), op)
        limits?.let { range ->
            val bounded = next.number("value").coerceIn(range)
            if (bounded != next.number("value")) next = resolve(bounded, obj("type" to "format"))
        }
        val changed = next.number("value") != shown.number("value")
        shown = next; error = null
        if (changed) onChange(next.number("value"))
        true
    } catch (e: Exception) { error = e.message ?: host.bootstrap!!.getString("action_failed"); false }
    fun finish(cancel: Boolean = false): Boolean {
        if (!cancel && text.composition != null) return false
        if (!dirty && !editing) return true
        if (!cancel && dirty && !apply(obj("type" to "expression", "text" to text.text))) return false
        editing = false; dirty = false
        error = null; text = TextFieldValue(shown.getString(displayKey))
        host.textComposition.clear(requester)
        return true
    }
    val commit by rememberUpdatedState<(Boolean) -> Boolean>({ finish(it) })
    DisposableEffect(registerCommit) {
        registerCommit(requester) { cancel -> commit(cancel) }
        onDispose { registerCommit(requester, null) }
    }
    LaunchedEffect(host.languageTag) {
        if (error != null) error = runCatching { resolve(shown.number("value"), obj("type" to "expression", "text" to text.text)) }.exceptionOrNull()?.message
    }
    LaunchedEffect(shown, dirty) {
        if (!dirty && (!focused || fieldValue != shown.number("value"))) {
            text = TextFieldValue(shown.getString(displayKey)); fieldValue = shown.number("value")
        }
    }
    if (editing && settings) {
        val settingsOpen = LocalPreferencesOpen.current
        LaunchedEffect(settingsOpen) { if (!settingsOpen) finish(cancel = true) }
    }
    LaunchedEffect(editing) { if (editing && (ranged || inline)) requester.requestFocus() }
    DisposableEffect(Unit) { onDispose {
        if (focused) host.editingText = false
        host.textComposition.clear(requester)
        if (toolbar && host.toolbarEditorBounds == fieldBounds) host.toolbarEditorBounds = null
    } }
    val step: @Composable (Int, String) -> Unit = { direction, name ->
        val available = enabled && if (direction < 0) shown.number("value") > control.number("min") else shown.number("value") < control.number("max")
        Box(Modifier.size(height).clip(shape).alpha(if (available) 1f else .36f)
            .clickable(enabled = available) { if (finish()) { focus.clearFocus(); apply(obj("type" to "step", "steps" to direction)) } }, contentAlignment = Alignment.Center) {
            SharedIcon(name, captions.getString(if (direction < 0) "decrease" else "increase"), Modifier.size(16.dp))
        }
    }
    val field: @Composable () -> Unit = {
        BasicTextField(text, {
            if (it.text != text.text || it.composition != null) dirty = true
            text = it
            host.textComposition.update(requester, text, focused)
            if (dirty) onText(it)
        }, Modifier.then(if (inline) Modifier.width(fixedWidth) else if (ranged) Modifier.widthIn(min = 48.dp, max = 100.dp).width(IntrinsicSize.Min) else Modifier.width(if (control.optString("unit").isEmpty()) 60.dp else 80.dp)).height(height)
            .onGloballyPositioned { fieldBounds = it.boundsInRoot(); if (toolbar && focused) host.toolbarEditorBounds = fieldBounds }
            .focusRequester(requester).onFocusChanged {
                if (focused && !it.isFocused) finish()
                focused = it.isFocused; host.editingText = focused; onTyping(focused)
                host.textComposition.update(requester, text, focused)
                if (toolbar) {
                    if (focused) host.toolbarEditorBounds = fieldBounds
                    else if (host.toolbarEditorBounds == fieldBounds) host.toolbarEditorBounds = null
                }
                if (focused) editing = true
            }.onPreviewKeyEvent {
                if (host.textComposition.owns(it.nativeKeyEvent)) false
                else if (it.type == KeyEventType.KeyDown && it.key == Key.Escape) { finish(true); focus.clearFocus(); true }
                else if (it.type == KeyEventType.KeyDown && it.key == Key.Enter) { if (finish()) focus.clearFocus(); true }
                else false
            }.semantics { contentDescription = captions.getString("edit") }.testTag(if (settings) "setting-number-$id" else "number-$label"), enabled = enabled, singleLine = true,
            textStyle = LocalTextStyle.current.copy(color = colors.text, textAlign = TextAlign.End),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (text.composition == null && finish()) focus.clearFocus() }),
            decorationBox = { input -> Box(Modifier.fillMaxSize().background(colors.input, shape).padding(horizontal = valuePadding), contentAlignment = Alignment.CenterEnd) { input() } })
    }
    val valueControl: @Composable () -> Unit = {
        if (editing) field()
        else Box(Modifier.then(if (inline) Modifier.width(fixedWidth) else Modifier).height(height).clip(shape)
            .then(if (toolbar) Modifier.toolbarNumberScrub(control, shown.number("fill"), enabled,
                { if (finish()) apply(obj("type" to "position", "position" to it)) },
                { if (finish()) apply(obj("type" to "step", "steps" to it)) }) else Modifier)
            .clickable(enabled = enabled) {
            val edit = shown.getString("edit"); text = TextFieldValue(edit, TextRange(0, edit.length)); editing = true
        }.padding(horizontal = valuePadding).testTag("number-value-$id"), contentAlignment = if (toolbar && !showUnits) Alignment.Center else Alignment.CenterEnd) { Text(shown.getString("text"), maxLines = 1, softWrap = false) }
    }
    if (inline) {
        Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            if (showSlider) EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) }, Modifier.weight(1f),
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
            if (!ranged) Row(Modifier.clip(shape).background(colors.input), verticalAlignment = Alignment.CenterVertically) { field(); step(-1, "minus"); step(1, "plus") }
            else valueControl()
        }
        if (ranged) Row(Modifier.fillMaxWidth().padding(top = if (settings) 3.dp else 0.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            step(-1, "minus")
            EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) },
                Modifier.weight(1f).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                enabled = enabled, label = label, height = height,
                inactiveTrackColor = if (settings) colors.divider else colors.input, showThumb = settings,
                activeTrackColor = if (settings) colors.accent else colors.sliderFill)
            step(1, "plus")
        }
        error?.let { Text(it, color = colors.accent, fontSize = 12.sp) }
    }
}
