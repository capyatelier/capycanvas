package art.capycanvas

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
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

@androidx.annotation.Keep
internal class NumericFailure(encoded: String) : RuntimeException(JSONObject(encoded).getString("text")) {
    val reason: Any = JSONObject(encoded).get("reason")
}
internal fun numericFailureCopy(error: Exception, language: String, fallback: String): String =
    if (error is NumericFailure) JSONObject(Native.nativeCaption(obj("type" to "numeric_error", "reason" to error.reason).toString(), language)).getString("text")
    else error.message ?: fallback

/** A native text field and slider; Rust owns numeric semantics. Tapping a value
 * opens the keyboard. */
@Composable internal fun NumericSetting(label: String, value: Float, control: JSONObject,
    modifier: Modifier = Modifier, enabled: Boolean = true, description: String = "",
    settings: Boolean = false, id: String = label, inline: Boolean = false,
    toolbar: Boolean = false, showUnits: Boolean = true, showSlider: Boolean = true, valueOnly: Boolean = false,
    limits: ClosedFloatingPointRange<Float>? = null, onTyping: (Boolean) -> Unit = {}, onText: (TextFieldValue) -> Unit = {},
    registerCommit: (Any, ((Boolean) -> Boolean)?) -> Unit = { _, _ -> },
    onChange: (Float) -> Unit) {
    NumericSetting(label, value.toDouble(), control, modifier, enabled, description, settings, id, inline,
        toolbar, showUnits, showSlider, valueOnly, limits?.let { it.start.toDouble()..it.endInclusive.toDouble() },
        onTyping, onText, registerCommit, onChange = { onChange(it.toFloat()) })
}

@Composable internal fun NumericSetting(label: String, value: Double, control: JSONObject,
    modifier: Modifier = Modifier, enabled: Boolean = true, description: String = "",
    settings: Boolean = false, id: String = label, inline: Boolean = false,
    toolbar: Boolean = false, showUnits: Boolean = true, showSlider: Boolean = true, valueOnly: Boolean = false,
    limits: ClosedFloatingPointRange<Double>? = null, onTyping: (Boolean) -> Unit = {}, onText: (TextFieldValue) -> Unit = {},
    registerCommit: (Any, ((Boolean) -> Boolean)?) -> Unit = { _, _ -> },
    presentedText: String? = null, onEditPhase: ((String) -> Unit)? = null,
    onChange: (Double) -> Unit) {
    val host = LocalCanvasHost.current
    val captions = remember(label, host.languageTag) { JSONObject(Native.numericLabels(label, host.languageTag)) }
    val colors = LocalPalette.current
    val shape = if (settings) RoundedCornerShape(6.dp) else ControlShape
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    val ranged = control.getString("kind") == "slider"
    val displayKey = if (ranged) "edit" else "text"
    fun resolve(value: Double, op: JSONObject): JSONObject {
        val request = obj("control" to control, "value" to value, "operation" to op)
        return JSONObject(if (toolbar && !valueOnly) Native.toolbarUi(obj("type" to "number", "request" to request, "compact" to true, "units" to showUnits).toString(), host.languageTag) else Native.number(request.toString(), host.languageTag))
    }
    var shown by remember(value, control.toString(), showUnits, presentedText, host.languageTag) { mutableStateOf(resolve(value, obj("type" to "format")).also { shown -> presentedText?.let { shown.put("text", it).put("edit", it) } }) }
    var editing by rememberSaveable { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    var fieldBounds by remember { mutableStateOf(Rect.Zero) }
    var text by rememberSaveable(stateSaver = TextFieldValue.Saver) { mutableStateOf(TextFieldValue(shown.getString(displayKey))) }
    var fieldValue by remember { mutableDoubleStateOf(shown.getDouble("value")) }
    var error by remember { mutableStateOf<Exception?>(null) }
    val errorCaption = remember(error, host.languageTag) { error?.let { numericFailureCopy(it, host.languageTag, host.bootstrap!!.getString("action_failed")) } }
    val height = if (settings) 48.dp else if (ranged || toolbar) 24.dp else 32.dp
    val valuePadding = if (settings) 12.dp else if (toolbar && !showUnits && !valueOnly) 2.dp else 6.dp
    val measurer = rememberTextMeasurer()
    val widest = remember(inline, control.toString(), showUnits, host.languageTag) {
        if (inline) (if (toolbar) JSONObject(Native.toolbarUi(obj("type" to "numeric_info", "id" to id, "control" to control, "compact" to true, "units" to true).toString(), host.languageTag)).array("samples").let { samples -> (0 until samples.length()).map(samples::getString) }
            else listOf(control.getDouble("min"), control.getDouble("max")).map { resolve(it, obj("type" to "format")).getString("text") })
            .maxBy { it.length }.replace(Regex("[0-9]"), "8") else ""
    }
    val fixedWidth = if (inline) with(LocalDensity.current) { measurer.measure(if (valueOnly) shown.getString("text") else widest, LocalTextStyle.current).size.width.toDp() } + valuePadding * 2 + 2.dp else 0.dp
    var active by remember { mutableIntStateOf(0) }
    var heldKey by remember { mutableStateOf<Key?>(null) }
    val phase by rememberUpdatedState(onEditPhase)
    fun beginEdit() { if (active == 0 && phase != null) { active = 1; phase?.invoke("down") } }
    fun endEdit(cancel: Boolean = false) {
        val notify = active == 1; active = 0; heldKey = null
        if (notify) phase?.invoke(if (cancel) "cancel" else "up")
    }
    fun apply(op: JSONObject): Boolean {
        if (active == 2 || (op.optString("type") == "expression" && presentedText != null && op.optString("text") == presentedText)) return true
        return try {
        var next = resolve(shown.getDouble("value"), op)
        limits?.let { range ->
            val bounded = next.getDouble("value").coerceIn(range)
            if (bounded != next.getDouble("value")) next = resolve(bounded, obj("type" to "format"))
        }
        val changed = next.getDouble("value") != shown.getDouble("value")
        shown = next; error = null
        if (changed) onChange(next.getDouble("value"))
        true
        } catch (e: Exception) { error = e; false }
    }
    fun finish(cancel: Boolean = false, keepEditing: Boolean = false): Boolean {
        if (!cancel && text.composition != null) return false
        if (!dirty && !editing) return true
        if (!cancel && dirty && text.text != shown.getString(displayKey) && !apply(obj("type" to "expression", "text" to text.text))) return false
        editing = keepEditing; dirty = false
        error = null; text = TextFieldValue(shown.getString(displayKey))
        host.textComposition.clear(requester)
        return true
    }
    val commit by rememberUpdatedState<(Boolean) -> Boolean>({ finish(it) })
    DisposableEffect(registerCommit) {
        registerCommit(requester) { cancel -> commit(cancel) }
        onDispose { registerCommit(requester, null) }
    }
    LaunchedEffect(shown, dirty) {
        if (!dirty && (!focused || fieldValue != shown.getDouble("value"))) {
            text = TextFieldValue(shown.getString(displayKey)); fieldValue = shown.getDouble("value")
        }
    }
    if (editing && settings) {
        val settingsOpen = LocalPreferencesOpen.current
        LaunchedEffect(settingsOpen) { if (!settingsOpen) finish(cancel = true) }
    }
    LaunchedEffect(editing) { if (editing) requester.requestFocus() }
    DisposableEffect(Unit) { onDispose {
        endEdit(cancel = true)
        if (focused) host.editingText = false
        host.textComposition.clear(requester)
        if (toolbar && host.toolbarEditorBounds == fieldBounds) host.toolbarEditorBounds = null
    } }
    val contact = if (onEditPhase == null || !enabled) Modifier else Modifier.pointerInput(enabled) {
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            focus.clearFocus(); beginEdit(); var released = false
            try {
                while (true) {
                    val change = awaitPointerEvent(PointerEventPass.Final).changes.firstOrNull { it.id == down.id } ?: break
                    if (!change.pressed) { released = true; endEdit(); break }
                }
            } finally { if (!released) endEdit(cancel = true) }
        }
    }
    val step: @Composable (Int, String) -> Unit = { direction, name ->
        val available = enabled && if (direction < 0) shown.getDouble("value") > control.getDouble("min") else shown.getDouble("value") < control.getDouble("max")
        Box(Modifier.then(contact).size(height).clip(shape).alpha(if (available) 1f else .36f)
            .clickable(enabled = available) { if (finish()) { focus.clearFocus(); apply(obj("type" to "step", "steps" to direction)) } }, contentAlignment = Alignment.Center) {
            SharedIcon(name, captions.getString(if (direction < 0) "decrease" else "increase"), Modifier.size(16.dp))
        }
    }
    val field: @Composable () -> Unit = {
        BasicTextField(text, {
            if (editing && active != 2) {
                if (it.text != text.text || it.composition != null) dirty = true
                text = it
                host.textComposition.update(requester, text, focused)
                if (dirty) onText(it)
            }
        }, Modifier.then(if (inline && valueOnly && !toolbar) Modifier.fillMaxWidth() else if (inline) Modifier.width(fixedWidth) else if (ranged) Modifier.widthIn(min = 48.dp, max = 100.dp).width(IntrinsicSize.Min) else Modifier.width(if (presentedText != null || control.optString("unit").isNotEmpty()) 80.dp else 60.dp)).height(height)
            .onGloballyPositioned { fieldBounds = it.boundsInRoot(); if (toolbar && focused) host.toolbarEditorBounds = fieldBounds }
            .focusRequester(requester).onFocusChanged {
                if (focused && !it.isFocused) { finish(); if (heldKey != null) endEdit() }
                focused = it.isFocused; host.editingText = focused; onTyping(focused)
                host.textComposition.update(requester, text, focused)
                if (toolbar) {
                    if (focused) host.toolbarEditorBounds = fieldBounds
                    else if (host.toolbarEditorBounds == fieldBounds) host.toolbarEditorBounds = null
                }
                if (focused) editing = true
            }.onPreviewKeyEvent {
                if (it.type == KeyEventType.KeyUp && it.key == heldKey) { endEdit(); true }
                else if (host.textComposition.owns(it.nativeKeyEvent)) false
                else if (it.type == KeyEventType.KeyDown && it.key in listOf(Key.DirectionUp, Key.DirectionDown)) {
                    if (heldKey != null && heldKey != it.key) endEdit()
                    beginEdit(); heldKey = it.key
                    if (finish(keepEditing = true)) apply(obj("type" to "step", "steps" to if (it.key == Key.DirectionUp) 1 else -1))
                    true
                }
                else if (it.type == KeyEventType.KeyDown && it.key == Key.Escape) {
                    if (active == 1) { active = 2; phase?.invoke("cancel"); finish(true, keepEditing = true) }
                    else { finish(true); focus.clearFocus() }; true
                }
                else if (it.type == KeyEventType.KeyDown && it.key == Key.Enter) { if (finish()) focus.clearFocus(); true }
                else false
            }.semantics { contentDescription = captions.getString("edit") }.testTag(if (settings) "setting-number-$id" else if (presentedText != null) "number-$id" else "number-$label"), enabled = enabled, singleLine = true,
            textStyle = LocalTextStyle.current.copy(color = colors.text, textAlign = TextAlign.Start),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (text.composition == null && finish()) focus.clearFocus() }),
            decorationBox = { input -> Box(Modifier.fillMaxSize().background(colors.input, shape).padding(horizontal = valuePadding), contentAlignment = Alignment.CenterEnd) { input() } })
    }
    val valueControl: @Composable () -> Unit = {
        if (editing) field()
        else Box(Modifier.then(if (inline && valueOnly && !toolbar) Modifier.fillMaxWidth() else if (inline) Modifier.width(fixedWidth) else Modifier).height(height).clip(shape)
            .then(if (toolbar) Modifier.toolbarNumberScrub(control, shown.number("fill"), enabled,
                { if (finish()) apply(obj("type" to "position", "position" to it)) },
                { if (finish()) apply(obj("type" to "step", "steps" to it)) }) else Modifier)
            .clickable(enabled = enabled) {
            val edit = shown.getString("edit"); text = TextFieldValue(edit, TextRange(0, edit.length)); editing = true
        }.padding(horizontal = valuePadding).testTag("number-value-$id"), contentAlignment = if (toolbar && !showUnits) Alignment.Center else Alignment.CenterEnd) { Text(shown.getString("text"), maxLines = 1, softWrap = false) }
    }
    if (inline) {
        Column(modifier) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                if (showSlider) EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) }, Modifier.weight(1f).then(contact).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                    enabled = enabled, label = label, height = height, inactiveTrackColor = colors.input, showThumb = false, activeTrackColor = colors.sliderFill)
                valueControl()
            }
            errorCaption?.let { Text(it, Modifier.testTag("number-error-$id"), color = colors.accent, fontSize = 12.sp) }
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
                Modifier.weight(1f).then(contact).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                enabled = enabled, label = label, height = height,
                inactiveTrackColor = if (settings) colors.divider else colors.input, showThumb = settings,
                activeTrackColor = if (settings) colors.accent else colors.sliderFill)
            step(1, "plus")
        }
        errorCaption?.let { Text(it, Modifier.testTag("number-error-$id"), color = colors.accent, fontSize = 12.sp) }
    }
}
