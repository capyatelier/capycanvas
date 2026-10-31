package art.capycanvas

import androidx.compose.foundation.focusable
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.isShiftPressed
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.foundation.Canvas
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.stateDescription
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
    onEditPhase: ((String) -> Unit)? = null,
    onReset: (() -> Unit)? = null,
    onPositionChange: ((Float) -> Unit)? = null,
    fieldFill: Color = LocalPalette.current.input,
    onChange: (Float) -> Unit) {
    NumericSetting(label, value.toDouble(), control, modifier, enabled, description, settings, id, inline,
        toolbar, showUnits, showSlider, valueOnly, limits?.let { it.start.toDouble()..it.endInclusive.toDouble() },
        onTyping, onText, registerCommit, onEditPhase = onEditPhase, onReset = onReset,
        onPositionChange = onPositionChange?.let { change -> { change(it.toFloat()) } }, fieldFill = fieldFill, onChange = { onChange(it.toFloat()) })
}

@Composable internal fun NumericSetting(label: String, value: Double, control: JSONObject,
    modifier: Modifier = Modifier, enabled: Boolean = true, description: String = "",
    settings: Boolean = false, id: String = label, inline: Boolean = false,
    toolbar: Boolean = false, showUnits: Boolean = true, showSlider: Boolean = true, valueOnly: Boolean = false,
    limits: ClosedFloatingPointRange<Double>? = null, onTyping: (Boolean) -> Unit = {}, onText: (TextFieldValue) -> Unit = {},
    registerCommit: (Any, ((Boolean) -> Boolean)?) -> Unit = { _, _ -> },
    presentedText: String? = null, onEditPhase: ((String) -> Unit)? = null,
    onReset: (() -> Unit)? = null,
    onPositionChange: ((Double) -> Unit)? = null,
    fieldFill: Color = LocalPalette.current.input,
    onChange: (Double) -> Unit) {
    val host = LocalCanvasHost.current
    val captions = remember(label, host.languageTag) { JSONObject(Native.numericLabels(label, host.languageTag)) }
    val colors = LocalPalette.current
    val shape = if (settings) RoundedCornerShape(6.dp) else ControlShape
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    val ranged = control.getString("kind") == "slider"
    val dial = control.getString("kind") == "dial"
    val panel = (ranged || dial) && !settings && !inline && !toolbar && !valueOnly
    var positionKeys by remember { mutableStateOf(false) }
    val positionInput = if (onPositionChange == null) Modifier else Modifier.onPreviewKeyEvent { positionKeys = it.type == KeyEventType.KeyDown; false }
        .onFocusChanged { if (!it.isFocused) positionKeys = false }
        .pointerInput(Unit) { awaitEachGesture { awaitFirstDown(requireUnconsumed = false); positionKeys = false } }
    val displayKey = if (ranged) "edit" else "text"
    fun resolve(value: Double, op: JSONObject): JSONObject {
        val request = obj("control" to control, "value" to value, "operation" to op)
        return JSONObject(if (toolbar && !valueOnly) Native.toolbarUi(obj("type" to "number", "request" to request, "compact" to true, "units" to showUnits).toString(), host.languageTag) else Native.number(request.toString(), host.languageTag))
    }
    var shown by remember(value, control.toString(), showUnits, presentedText, host.languageTag) { mutableStateOf(resolve(value, obj("type" to "format")).also { shown -> presentedText?.let { shown.put("text", it).put("edit", it) } }) }
    var editing by rememberSaveable { mutableStateOf(false) }
    var focused by remember { mutableStateOf(false) }
    var scrubbing by remember { mutableStateOf(false) }
    var scrubCancelled by remember { mutableStateOf(false) }
    var scrubOrigin by remember { mutableDoubleStateOf(value) }
    var valueBounds by remember { mutableStateOf(Rect.Zero) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    var fieldBounds by remember { mutableStateOf(Rect.Zero) }
    var text by rememberSaveable(stateSaver = TextFieldValue.Saver) { mutableStateOf(TextFieldValue(shown.getString(displayKey))) }
    var fieldValue by remember { mutableDoubleStateOf(shown.getDouble("value")) }
    var error by remember { mutableStateOf<Exception?>(null) }
    val errorCaption = remember(error, host.languageTag) { error?.let { numericFailureCopy(it, host.languageTag, host.bootstrap!!.getString("action_failed")) } }
    val compact = LocalCompactPanel.current
    val metrics = panelMetrics()
    val height = if (settings) 48.dp else if (compact || ranged || toolbar) metrics.height else 32.dp
    val valuePadding = if (settings) 12.dp else if (toolbar && !showUnits && !valueOnly) 2.dp else 6.dp
    val measurer = rememberTextMeasurer()
    val widest = remember(inline, control.toString(), showUnits, host.languageTag) {
        if (inline || (compact && !ranged)) (if (toolbar) JSONObject(Native.toolbarUi(obj("type" to "numeric_info", "id" to id, "control" to control, "compact" to true, "units" to true).toString(), host.languageTag)).array("samples").let { samples -> (0 until samples.length()).map(samples::getString) }
            else listOf(control.getDouble("min"), control.getDouble("max")).map { resolve(it, obj("type" to "format")).getString("text") })
            .maxBy { it.length }.replace(Regex("[0-9]"), "8") else ""
    }
    val fixedWidth = if (inline || (compact && !ranged)) with(LocalDensity.current) { measurer.measure(if (valueOnly) shown.getString("text") else widest, LocalTextStyle.current).size.width.toDp() } + valuePadding * 2 + 2.dp else 0.dp
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
        error = null
        if (op.optString("type") == "position" && onPositionChange != null && !positionKeys) {
            if (changed) onPositionChange(next.getDouble("value"))
        } else {
            shown = next
            if (changed) onChange(next.getDouble("value"))
        }
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
    val numericOperation by rememberUpdatedState<(JSONObject) -> Boolean>({ apply(it) })
    val completeText by rememberUpdatedState<() -> Boolean>({ finish() })
    val currentFill by rememberUpdatedState(shown.number("fill"))
    val currentValue by rememberUpdatedState(shown.getDouble("value"))
    val beginScrub by rememberUpdatedState<(Double) -> Unit>({ origin -> scrubOrigin = origin; beginEdit(); scrubbing = true })
    val moveScrub by rememberUpdatedState<(Double, Double, Boolean) -> Unit>({ origin, pixels, fine -> apply(obj("type" to "scrub", "origin" to origin, "pixels" to pixels, "fine" to fine)) })
    val settleScrub by rememberUpdatedState<(Double, Boolean) -> Unit>({ origin, cancel ->
        scrubbing = false
        if (cancel) {
            endEdit(cancel = true)
            shown = resolve(origin, obj("type" to "format"))
            if (phase == null) onChange(origin)
        } else endEdit()
    })
    val cancelScrub by rememberUpdatedState({ scrubCancelled = true; settleScrub(scrubOrigin, true) })
    DisposableEffect(scrubbing) {
        val handler: ((android.view.KeyEvent) -> Boolean)? = if (!scrubbing) null else { event ->
            if (event.keyCode != android.view.KeyEvent.KEYCODE_ESCAPE) false
            else { if (event.action == android.view.KeyEvent.ACTION_DOWN) cancelScrub(); true }
        }
        if (handler != null) host.numberKeyHandler = handler
        onDispose { if (handler != null && host.numberKeyHandler === handler) host.numberKeyHandler = null }
    }
    val fineDrag = Modifier.edgeControl().pointerInput(control.toString(), enabled, panel, toolbar) {
        if (!enabled) return@pointerInput
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false)
            if (!panel && down.type == PointerType.Mouse || toolbar) return@awaitEachGesture
            val start = currentValue
            val startFill = currentFill
            var origin = start
            var fine = currentEvent.keyboardModifiers.isShiftPressed
            var base = 0f
            var last = 0f
            scrubCancelled = false
            val y = valueBounds.top + down.position.y
            var moved = false; var released = false
            try {
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    if (scrubCancelled) break
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (change.isConsumed) break
                    val pixels = (y - valueBounds.top - change.position.y) / density
                    if (event.keyboardModifiers.isShiftPressed != fine) {
                        origin = shown.getDouble("value"); base = last; fine = !fine
                    }
                    last = pixels
                    if (!change.pressed) {
                        if (moved) { if (panel) moveScrub(origin, (pixels - base).toDouble(), fine) else numericOperation(obj("type" to "position", "position" to startFill + pixels / 200f)); change.consume() }
                        released = true; break
                    }
                    if (!moved && kotlin.math.abs(pixels) <= viewConfiguration.touchSlop / density) continue
                    if (!moved) { moved = true; beginScrub(start) }
                    change.consume(); if (panel) moveScrub(origin, (pixels - base).toDouble(), fine) else numericOperation(obj("type" to "position", "position" to startFill + pixels / 200f))
                }
            } finally { if (moved && !scrubCancelled) settleScrub(start, !released) }
        }
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
    val contact = if (onEditPhase == null || !enabled) Modifier else Modifier.edgeControl().pointerInput(enabled) {
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            if (down.isConsumed) return@awaitEachGesture
            focus.clearFocus(); beginEdit(); var released = false
            try {
                while (true) {
                    val change = awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull { it.id == down.id } ?: break
                    if (change.isConsumed) break
                    if (!change.pressed) {
                        awaitPointerEvent(PointerEventPass.Final)
                        released = true; endEdit(); break
                    }
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
    val wheelInput = Modifier.pointerInput(enabled, toolbar) {
                if (enabled && !toolbar) awaitPointerEventScope { while (true) {
                    val event = awaitPointerEvent()
                    if (event.type == PointerEventType.Scroll && completeText()) {
                        val delta = event.changes.sumOf { it.scrollDelta.y.toDouble() }
                        if (delta != 0.0) { numericOperation(obj("type" to "step", "steps" to -delta)); event.changes.forEach { it.consume() } }
                    }
                } }
            }
    val field: @Composable () -> Unit = {
        BasicTextField(text, {
            if (editing && active != 2) {
                if (it.text != text.text || it.composition != null) dirty = true
                text = it
                host.textComposition.update(requester, text, focused)
                if (dirty) onText(it)
            }
        }, Modifier.edgeControl().then(if (panel) Modifier.width(68.dp) else if (inline && valueOnly && !toolbar) Modifier.fillMaxWidth() else if (inline) Modifier.width(fixedWidth) else if (ranged) Modifier.widthIn(min = 48.dp, max = 100.dp).width(IntrinsicSize.Min) else if(compact) Modifier.width(fixedWidth) else Modifier.width(if (presentedText != null || control.optString("unit").isNotEmpty()) 80.dp else 60.dp)).height(height).textCursorRegion(enabled)
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
            textStyle = LocalTextStyle.current.copy(color = colors.text, textAlign = if (panel) TextAlign.End else TextAlign.Start),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (text.composition == null && finish()) focus.clearFocus() }),
            decorationBox = { input -> Box(Modifier.fillMaxSize().background(fieldFill, shape).padding(horizontal = valuePadding), contentAlignment = Alignment.CenterEnd) { input() } })
    }
    val valueControl: @Composable () -> Unit = {
        if (editing) field()
        else Box(Modifier.then(if (inline && valueOnly && !toolbar) Modifier.fillMaxWidth() else if (inline) Modifier.width(fixedWidth) else Modifier).height(height).clip(shape)
            .onGloballyPositioned { valueBounds = it.boundsInRoot() }.then(fineDrag)
            .then(wheelInput)
            .then(if (toolbar) Modifier.toolbarNumberScrub(control, shown.number("fill"), enabled,
                { if (finish()) apply(obj("type" to "position", "position" to it)) },
                { if (finish()) apply(obj("type" to "step", "steps" to it)) }) else Modifier)
            .clickable(enabled = enabled) {
            val edit = shown.getString("edit"); text = TextFieldValue(edit, TextRange(0, edit.length)); editing = true
        }.padding(horizontal = valuePadding).testTag("number-value-$id"), contentAlignment = if (toolbar && !showUnits) Alignment.Center else Alignment.CenterEnd) { Text(shown.getString("text"), maxLines = 1, softWrap = false, overflow = TextOverflow.Ellipsis) }
    }
    if (panel) {
        val reset = {
            endEdit(cancel = true)
            finish(cancel = true)
            if (onReset != null) onReset() else if (!control.isNull("default_value")) apply(obj("type" to "expression", "text" to ""))
        }
        Column(modifier.fillMaxWidth()) {
            Box(Modifier.fillMaxWidth().height(36.dp)) {
                Text(label, Modifier.fillMaxWidth().padding(start = if(compact)0.dp else 6.dp, end = 76.dp).align(Alignment.TopStart)
                    .pointerInput(enabled, onReset, control.toString()) { if (enabled) detectTapGestures(onDoubleTap = { reset() }) },
                    color = if (enabled) colors.text else colors.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Box(Modifier.align(Alignment.BottomEnd)) { valueControl() }
                if (dial) AngleDial(shown.getDouble("value") * control.getDouble("scale"), enabled, fieldFill, colors.sliderFill,
                    Modifier.align(Alignment.CenterEnd).padding(end = 76.dp).size(metrics.sliderHeight - metrics.spacing * 2).then(contact)
                        .semantics { contentDescription = label; stateDescription = shown.getString("text") }
                        .onPreviewKeyEvent { event ->
                            if (!enabled || event.type != KeyEventType.KeyDown) false else {
                                val steps = when (event.key) {
                                    Key.DirectionLeft, Key.DirectionDown -> if (event.isShiftPressed) -15 else -1
                                    Key.DirectionRight, Key.DirectionUp -> if (event.isShiftPressed) 15 else 1
                                    Key.PageUp -> 45
                                    Key.PageDown -> -45
                                    else -> 0
                                }
                                if (event.key == Key.MoveHome) { reset(); true }
                                else if (steps != 0 && finish()) { apply(obj("type" to "step", "steps" to steps)); true } else false
                            }
                        }.focusable(enabled)
                        .edgeControl().pointerInput(enabled) { if (enabled) awaitPointerEventScope { while (true) {
                            val event = awaitPointerEvent()
                            if (event.type == PointerEventType.Scroll && completeText()) {
                                val delta = event.changes.sumOf { it.scrollDelta.y.toDouble() }
                                if (delta != 0.0) { numericOperation(obj("type" to "step", "steps" to -delta)); event.changes.forEach { it.consume() } }
                            }
                        } } }.testTag("number-dial-$id"),
                    onDoubleTap = reset) { position, radius -> if (finish()) apply(obj("type" to "dial", "x" to position.x.toDouble(), "y" to position.y.toDouble(), "radius" to radius.toDouble())) }
                else EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) },
                    Modifier.fillMaxWidth().padding(start = 36.dp, end = 76.dp).align(Alignment.BottomStart).then(contact).testTag("number-slider-$label"),
                    enabled = enabled, label = label, height = 16.dp, inactiveTrackColor = fieldFill, showThumb = false, activeTrackColor = colors.sliderFill)
            }
            errorCaption?.let { Text(it, Modifier.testTag("number-error-$id"), color = colors.accent, fontSize = 12.sp) }
        }
        return
    }
    if (inline) {
        Column(modifier) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                if (showSlider) EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) }, Modifier.weight(1f).then(contact).then(positionInput).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                    enabled = enabled, label = label, height = height, inactiveTrackColor = fieldFill, showThumb = false, activeTrackColor = colors.sliderFill)
                valueControl()
            }
            errorCaption?.let { Text(it, Modifier.testTag("number-error-$id"), color = colors.accent, fontSize = 12.sp) }
        }
        return
    }
    Column(modifier.widthIn(max = if (settings) 600.dp else 1000.dp).fillMaxWidth()) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(if(compact)metrics.spacing else 6.dp)) {
            Column(Modifier.weight(1f).padding(start = if (settings || compact) 0.dp else 6.dp).testTag("preference-label-$id"), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(label, color = if (enabled) colors.text else colors.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (description.isNotEmpty()) Text(description, color = colors.settingsSecondary, fontSize = 14.sp, lineHeight = 20.sp)
            }
            if (!ranged) Row(Modifier.clip(shape).background(fieldFill), verticalAlignment = Alignment.CenterVertically) { field(); step(-1, "minus"); step(1, "plus") }
            else valueControl()
        }
        if (ranged) Row(Modifier.fillMaxWidth().padding(top = if (settings) 3.dp else 0.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            step(-1, "minus")
            EditorSlider(shown.number("fill"), { if (finish()) apply(obj("type" to "position", "position" to it)) },
                Modifier.weight(1f).then(contact).testTag(if (settings) "setting-slider-$id" else "number-slider-$label"),
                enabled = enabled, label = label, height = height,
                inactiveTrackColor = if (settings) colors.divider else fieldFill, showThumb = settings,
                activeTrackColor = if (settings) colors.accent else colors.sliderFill)
            step(1, "plus")
        }
        errorCaption?.let { Text(it, Modifier.testTag("number-error-$id"), color = colors.accent, fontSize = 12.sp) }
    }
}

@Composable private fun AngleDial(degrees: Double, enabled: Boolean, face: Color, ink: Color, modifier: Modifier,
    onDoubleTap: () -> Unit, onPoint: (Offset, Float) -> Unit) {
    val point by rememberUpdatedState(onPoint)
    val reset by rememberUpdatedState(onDoubleTap)
    Canvas(modifier.alpha(if (enabled) 1f else .4f).edgeControl()
        .pointerInput(enabled) {
            if (!enabled) return@pointerInput
            detectTapGestures(onDoubleTap = { reset() })
        }
        .pointerInput(enabled) {
            if (!enabled) return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                if (down.isConsumed) return@awaitEachGesture
                val center = Offset(size.width / 2f, size.height / 2f)
                val radius = minOf(size.width, size.height) / 2f
                point(down.position - center, radius)
                awaitPointerEvent(PointerEventPass.Main)
                while (true) {
                    val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (change.isConsumed || !change.pressed) break
                    change.consume()
                    point(change.position - center, radius)
                }
            }
        }) {
        val radius = size.minDimension / 2f
        val angle = Math.toRadians(degrees)
        val direction = Offset(kotlin.math.cos(angle).toFloat(), kotlin.math.sin(angle).toFloat())
        drawCircle(face, radius)
        drawCircle(ink, radius * .22f, center + direction * (radius * .55f))
    }
}
