package art.capycanvas

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.activity.compose.LocalActivity
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.animation.core.EaseOutCubic
import androidx.compose.animation.core.tween
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.roundToInt

internal fun displayColor(preview: JSONObject): Color {
    return displayColor(preview.getJSONArray("rgba"))
}
internal fun displayColor(v: JSONArray): Color {
    return Color(v.getDouble(0).toFloat(), v.getDouble(1).toFloat(), v.getDouble(2).toFloat(), v.getDouble(3).toFloat())
}
private fun colorEpoch(host: CanvasHost): Long =
    host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch") ?: 0L

internal class ColorEditorRequest(val slot: String?, val color: JSONObject?, val opaque: Boolean, val windowed: Boolean, val onUse: (JSONObject, Float?) -> Unit)

@Composable internal fun ManagedColorButton(host: CanvasHost, label: String, value: JSONObject, enabled: Boolean, renderedPreview: JSONObject? = null, compact: Boolean = false, opaque: Boolean = false, trailing: @Composable RowScope.() -> Unit = {}, onChange: (JSONObject) -> Unit) {
    val preview = renderedPreview ?: remember(value.toString()) {
        JSONArray(Native.colorUi(obj("type" to "preview", "colors" to JSONArray().put(value)).toString(), host.languageTag)).getJSONObject(0)
    }
    val live = remember { booleanArrayOf(true) }
    val change by rememberUpdatedState(onChange)
    val windowed = inSeparateWindow()
    DisposableEffect(Unit) { onDispose { live[0] = false } }
    Row(if(compact) Modifier else Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        if(!compact) Text(label, Modifier.weight(1f))
        Button(onClick = { host.colorEditor = ColorEditorRequest(null, JSONObject(value.toString()), opaque, windowed) { selected, _ -> if (live[0]) change(selected) } }, enabled = enabled,
            modifier = Modifier.size(if(compact) 36.dp else 56.dp, 32.dp).semantics {contentDescription=label}.testTag("property-color-$label"), contentPadding = PaddingValues(0.dp),
            colors = ButtonDefaults.buttonColors(containerColor = displayColor(preview))) { if(!compact) Text("…") }
        trailing()
    }
    if (!compact && !preview.getBoolean("in_gamut")) Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("outside_srgb"), style = MaterialTheme.typography.labelSmall)
}

@Composable internal fun inSeparateWindow(): Boolean = LocalActivity.current?.window?.decorView !== LocalView.current.rootView

@Composable internal fun ColorEditorLayer(host: CanvasHost) {
    val request = host.colorEditor ?: return
    key(request) { ColorEditorOverlay(host, request) }
}

private val Shapes = listOf("circle" to "OKLCH", "square" to "HSB", "triangle" to "HLS")

@Composable private fun ColorEditorOverlay(host: CanvasHost, request: ColorEditorRequest) {
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("color")
    val common = host.bootstrap!!.getJSONObject("common")
    val context = LocalContext.current
    val density = LocalDensity.current.density
    val epoch = remember { colorEpoch(host) }
    val rendition = remember { host.snapshot?.objectOrNull("color_panel")?.objectOrNull("rendition") }
    fun call(body: JSONObject) = JSONObject(Native.colorUi(body.toString(), host.languageTag))
    val opened = remember {
        runCatching {
            val colorsState = host.snapshot!!.getJSONObject("state").displayColors()
            call(obj("type" to "editor_open", "colors" to colorsState, "slot" to request.slot, "color" to request.color, "opaque" to request.opaque,
                "display_space" to "Srgb", "rendition" to rendition))
        }.getOrNull()
    }
    if (opened == null) { SideEffect { host.colorEditor = null }; return }
    var editor by remember { mutableStateOf(opened.getJSONObject("editor")) }
    var view by remember { mutableStateOf(opened.getJSONObject("view")) }
    val memory = remember { opened.getJSONObject("editor").getJSONObject("picker").getJSONObject("editor").toString() }
    var error by remember { mutableStateOf<String?>(null) }
    var errorTarget by remember { mutableStateOf<String?>(null) }
    var refused by remember { mutableStateOf<JSONObject?>(null) }
    var editing by remember { mutableStateOf<String?>(null) }
    var sheet by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    var seenPicking by remember { mutableStateOf(false) }
    fun send(action: JSONObject?): String? {
        val next = call(obj("type" to "editor", "editor" to editor, "action" to action, "display_space" to "Srgb", "rendition" to rendition))
        editor = next.getJSONObject("editor"); view = next.getJSONObject("view")
        return next.optString("error").takeIf { !next.isNull("error") && it.isNotBlank() }
    }
    fun act(action: JSONObject, target: String? = null): Boolean {
        val failure = runCatching { send(action) }.getOrElse { it.message ?: copy.getString("read_failed") }
        if (failure != null || target == errorTarget || errorTarget == null) { error = failure; errorTarget = if (failure != null) target else null; refused = if (failure != null) action else null }
        return failure == null
    }
    LaunchedEffect(host.languageTag) { runCatching { send(null); refused?.let { error = send(it) } } }
    fun close(result: Pair<JSONObject, Float?>?) {
        val remembered = editor.getJSONObject("picker").getJSONObject("editor")
        if (remembered.toString() != memory) host.dispatch(obj("type" to "color", "action" to obj("op" to "editor_memory", "memory" to remembered)))
        host.colorEditor = null
        if (result != null && colorEpoch(host) == epoch) request.onUse(result.first, result.second)
    }
    val pickerState = host.colorPreview?.objectOrNull("picker") ?: host.snapshot?.objectOrNull("state")?.objectOrNull("color_picker")
    val pickerActive = picking && pickerState?.optBoolean("editor") == true
    LaunchedEffect(pickerActive) { if (pickerActive) seenPicking = true }
    LaunchedEffect(picking, pickerState?.optBoolean("editor"), seenPicking) {
        if (!picking) return@LaunchedEffect
        if (!seenPicking) { delay(1500); if (!seenPicking) picking = false; return@LaunchedEffect }
        if (pickerState?.optBoolean("editor") == false) {
            picking = false; seenPicking = false
            pickerState.objectOrNull("picked")?.let { act(obj("op" to "color", "color" to it)) }
        }
    }
    if (picking) {
        if (pickerActive) PickStrip(host, editor, view, pickerState!!, rendition)
        return
    }
    val wide = LocalConfiguration.current.screenWidthDp >= 744
    val invalidOpen = editing != null && errorTarget == editing && error != null
    val card = @Composable {
        Surface(Modifier.padding(16.dp).widthIn(max = 720.dp).fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp - 32).dp)
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown && event.key == Key.Escape && editing == null) { if (sheet) sheet = false else close(null); true } else false
            }.testTag("color-editor"), shape = SquircleShape(20.dp), color = colors.settingsBackground) {
            Column {
                Box(Modifier.weight(1f, fill = false).clipToBounds()) {
                    val body = @Composable {
                        val title = @Composable { Text(copy.getString("edit"), Modifier.fillMaxWidth().testTag("color-editor-title"), fontWeight = FontWeight.Bold, fontSize = 16.sp, textAlign = TextAlign.Center) }
                        val wheel = @Composable { modifier: Modifier ->
                            Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                DialogWheel(host, editor, view, if (wide) 232f else 208f) { act(obj("op" to "wheel", "action" to it)) }
                                Row(Modifier.clip(TileShape).background(colors.text.copy(alpha = .06f)).padding(4.dp).testTag("color-shapes"), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                                    val choices = view.array("shapes").objects()
                                    Shapes.forEachIndexed { index, (shape, label) ->
                                        val selected = choices.getOrNull(index)?.optBoolean("selected") == true
                                        Row(Modifier.height(26.dp).clip(TileShape).background(if (selected) colors.headerActive else Color.Transparent)
                                            .clickable(role = Role.RadioButton) { act(obj("op" to "wheel", "action" to obj("op" to "shape", "shape" to shape))) }
                                            .padding(horizontal = 8.dp).semantics { this.selected = selected; contentDescription = copy.getString(shape) }.testTag("color-shape-choice-$shape"),
                                            verticalAlignment = Alignment.CenterVertically) {
                                            SharedIcon("color-$shape", null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp))
                                            Text(label, fontWeight = FontWeight.Medium)
                                        }
                                    }
                                }
                            }
                        }
                        val header = @Composable {
                            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Row(Modifier.clip(ControlShape).border(1.dp, colors.divider, ControlShape).testTag("color-pair")) {
                                        Box(Modifier.size(54.dp, 48.dp).background(displayColor(view.getJSONObject("current"))).clickable { act(obj("op" to "revert")) }
                                            .semantics { contentDescription = copy.getString("current") }.testTag("color-current"))
                                        Box(Modifier.size(54.dp, 48.dp).background(displayColor(view.getJSONObject("new"))).semantics { contentDescription = copy.getString("new") }.testTag("color-new"))
                                    }
                                    if (!request.windowed) IconButton({
                                        picking = true; seenPicking = false
                                        val metrics = context.resources.displayMetrics
                                        host.dispatch(obj("type" to "color_picker", "action" to obj("kind" to "editor", "original" to view.getJSONObject("value"),
                                            "touch_offset" to (metrics.ydpi * 10f / 25.4f).coerceIn(36f * metrics.density, 64f * metrics.density))))
                                    }, Modifier.size(48.dp).clip(ControlShape).background(colors.button).testTag("color-pick")) {
                                        SharedIcon("eyedropper", copy.getString("pick_canvas"), Modifier.size(24.dp))
                                    }
                                    Spacer(Modifier.weight(1f))
                                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                        view.objectOrNull("hex_note")?.let { note -> Badge(note.getString("text"), Modifier.testTag("color-hex-note")) }
                                        ValueCell(view.getString("hex"), view.getString("hex"), copy.getString("hex"), "hex", editing == "hex", errorTarget == "hex" && error != null, true,
                                            { editing = "hex" }, { text -> if (act(obj("op" to "text", "text" to text), "hex")) editing = null }, { if (errorTarget == "hex") { error = null; errorTarget = null; refused = null }; editing = null })
                                        CopyButton(copy, "color-copy-hex") { copyText(context, view.getString("hex")) }
                                    }
                                }
                                Row(Modifier.width(108.dp)) {
                                    Text(copy.getString("current"), Modifier.weight(1f), color = colors.secondary, fontSize = 11.sp, textAlign = TextAlign.Center)
                                    Text(copy.getString("new"), Modifier.weight(1f), color = colors.secondary, fontSize = 11.sp, textAlign = TextAlign.Center)
                                }
                            }
                        }
                        val rows = @Composable {
                            val formNames = view.array("rows").objects().flatMap { it.array("forms").objects().map { form -> form.getString("label") } }
                            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                view.array("rows").objects().forEachIndexed { row, shown ->
                                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                        Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                            var open by remember { mutableStateOf(false) }
                                            Box {
                                                Row(Modifier.clip(ControlShape).clickable { open = true }.padding(horizontal = 6.dp, vertical = 6.dp).testTag("color-format-$row")
                                                    .semantics { contentDescription = shown.getString("label") }, verticalAlignment = Alignment.CenterVertically) {
                                                    Box { formNames.forEach { name -> Text(name, Modifier.alpha(if (name == shown.getString("label")) 1f else 0f), maxLines = 1) } }
                                                    Spacer(Modifier.width(4.dp)); SharedIcon("chevron-down", null, Modifier.size(12.dp))
                                                }
                                                DropdownMenu(open, { open = false }) {
                                                    shown.array("forms").objects().forEach { form ->
                                                        DropdownMenuItem(text = { Text(form.getString("label"), fontWeight = if (form.getString("form") == shown.getString("form")) FontWeight.Bold else FontWeight.Normal) },
                                                            modifier = Modifier.testTag("color-form-${form.getString("form")}"), onClick = { open = false; act(obj("op" to "form", "row" to row, "form" to form.getString("form"))) })
                                                    }
                                                }
                                            }
                                            if (!shown.isNull("space")) Badge(shown.getString("space"))
                                        }
                                        shown.array("values").objects().forEachIndexed { index, value ->
                                            val name = "$row-$index"
                                            val target = obj("kind" to "value", "row" to row, "index" to index)
                                            ValueCell(value.getString("text"), value.getString("edit"), value.getString("name"), name, editing == name, errorTarget == name && error != null, false,
                                                { editing = name }, { text -> if (act(obj("op" to "value", "row" to row, "index" to index, "text" to text), name)) editing = null },
                                                { if (errorTarget == name) { error = null; errorTarget = null; refused = null }; editing = null },
                                                { pixels, speed -> act(obj("op" to "scrub", "target" to target, "pixels" to pixels, "speed" to speed), name) },
                                                { cancel -> act(obj("op" to "end_scrub", "cancel" to cancel), name) })
                                        }
                                        CopyButton(copy, "color-copy-$row") { copyText(context, shown.getString("copy")) }
                                    }
                                }
                                view.objectOrNull("intensity")?.let { value ->
                                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                                        Text(copy.getString("intensity_ev"), Modifier.weight(1f).padding(start = 6.dp))
                                        val target = obj("kind" to "intensity")
                                        ValueCell(value.getString("text"), value.getString("edit"), value.getString("name"), "ev", editing == "ev", errorTarget == "ev" && error != null, false,
                                            { editing = "ev" }, { text -> if (act(obj("op" to "intensity", "text" to text), "ev")) editing = null },
                                            { if (errorTarget == "ev") { error = null; errorTarget = null; refused = null }; editing = null },
                                            { pixels, speed -> act(obj("op" to "scrub", "target" to target, "pixels" to pixels, "speed" to speed), "ev") },
                                            { cancel -> act(obj("op" to "end_scrub", "cancel" to cancel), "ev") }, wide = true)
                                        Spacer(Modifier.width(28.dp))
                                    }
                                }
                                if (error != null) Text(error!!, Modifier.padding(top = 6.dp).testTag("color-editor-error"), color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
                            }
                        }
                        Column(Modifier.verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                            title()
                            if (wide) Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
                                wheel(Modifier)
                                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(14.dp)) { header(); rows() }
                            } else Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                                header(); wheel(Modifier.align(Alignment.CenterHorizontally)); rows()
                            }
                        }
                    }
                    body()
                    androidx.compose.animation.AnimatedVisibility(sheet, Modifier.matchParentSize(), enter = slideInVertically(tween(250, easing = EaseOutCubic)) { it }, exit = slideOutVertically(tween(250, easing = EaseOutCubic)) { it }) {
                        SwatchSheet(host, editor, view, { text -> runCatching { send(obj("op" to "search", "text" to text)) } }, { color -> act(obj("op" to "color", "color" to color)) }) { sheet = false }
                    }
                }
                HorizontalDivider(Modifier.testTag("color-editor-divider"), color = colors.divider)
                Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (sheet) Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(Modifier.clip(ControlShape)) {
                            Box(Modifier.size(18.dp, 22.dp).background(displayColor(view.getJSONObject("current"))))
                            Box(Modifier.size(18.dp, 22.dp).background(displayColor(view.getJSONObject("new"))))
                        }
                        Text(view.getString("hex"))
                    } else RecentRow(host, view, Modifier.weight(1f)) { color -> act(obj("op" to "color", "color" to color)) }
                    Box(Modifier.size(34.dp).clip(ControlShape).background(colors.button).clickable { sheet = !sheet }
                        .semantics { contentDescription = copy.getString(if (sheet) "close_swatches" else "all_swatches") }.testTag("color-swatches"), contentAlignment = Alignment.Center) {
                        SharedIcon("chevron-down", null, Modifier.size(16.dp).rotate(if (sheet) 0f else 180f))
                    }
                    val footerButton = Modifier.height(34.dp)
                    val footerPadding = PaddingValues(horizontal = 16.dp)
                    Button({ close(null) }, footerButton.testTag("color-cancel"), shape = ControlShape, contentPadding = footerPadding,
                        colors = ButtonDefaults.buttonColors(containerColor = colors.button, contentColor = colors.text)) { Text(common.getString("cancel"), fontWeight = FontWeight.Medium) }
                    Button({ close(view.getJSONObject("value") to (if (view.isNull("stops")) null else view.number("stops"))) }, footerButton.testTag("color-use"),
                        enabled = !invalidOpen, shape = ControlShape, contentPadding = footerPadding) { Text(copy.getString("use_color"), fontWeight = FontWeight.Medium) }
                }
            }
        }
    }
    if (request.windowed) Dialog({ close(null) }, DialogProperties(usePlatformDefaultWidth = false)) { Box(contentAlignment = Alignment.Center) { card() } }
    else Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Box(Modifier.matchParentSize().background(Color.Black.copy(alpha = .32f)).pointerInput(Unit) {
            awaitEachGesture { awaitFirstDown(requireUnconsumed = false).consume(); do { val event = awaitPointerEvent(); event.changes.forEach { it.consume() } } while (event.changes.any { it.pressed }) }
        }.testTag("color-editor-scrim"))
        card()
    }
}

@Composable private fun Badge(text: String, modifier: Modifier = Modifier) {
    val colors = LocalPalette.current
    Text(text, modifier.clip(ControlShape).background(colors.text.copy(alpha = .08f)).padding(horizontal = 6.dp, vertical = 1.dp), fontSize = 11.sp, maxLines = 1)
}

@Composable private fun CopyButton(copy: JSONObject, tag: String, onCopy: () -> Unit) {
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1200); copied = false } }
    IconButton({ onCopy(); copied = true }, Modifier.offset(x = 6.dp).size(28.dp).testTag(tag)) {
        SharedIcon(if (copied) "check" else "copy", copy.getString(if (copied) "copied" else "copy"), Modifier.size(16.dp))
    }
}

private fun copyText(context: android.content.Context, text: String) {
    context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Color", text))
}

@Composable private fun ValueCell(text: String, edit: String, name: String, tag: String, editing: Boolean, invalid: Boolean, hex: Boolean,
    onBegin: () -> Unit, onCommit: (String) -> Unit, onCancel: () -> Unit,
    onScrub: ((Float, String) -> Unit)? = null, onEndScrub: ((Boolean) -> Unit)? = null, wide: Boolean = false) {
    val colors = LocalPalette.current
    val host = LocalCanvasHost.current
    val width = if (hex) 112.dp else if (wide) 88.dp else 58.dp
    val style = if (hex) MaterialTheme.typography.bodyMedium.copy(fontSize = 22.sp, fontWeight = FontWeight.Medium, letterSpacing = .6.sp, color = colors.text, fontFeatureSettings = "tnum")
        else MaterialTheme.typography.bodyMedium.copy(color = colors.text, textAlign = TextAlign.End, fontFeatureSettings = "tnum")
    if (editing) {
        val keyboard = LocalSoftwareKeyboardController.current
        DisposableEffect(Unit) { onDispose { keyboard?.hide() } }
        val owner = remember { Any() }
        val focus = remember { FocusRequester() }
        var value by remember { mutableStateOf(TextFieldValue(edit, TextRange(0, edit.length))) }
        var focused by remember { mutableStateOf(false) }
        LaunchedEffect(Unit) { focus.requestFocus() }
        DisposableEffect(owner) { onDispose { host.textComposition.clear(owner); if (focused) host.editingText = false } }
        BasicTextField(value, { value = it; host.textComposition.update(owner, value, focused) }, singleLine = true, textStyle = style, cursorBrush = SolidColor(colors.accent),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done), keyboardActions = KeyboardActions(onDone = { onCommit(value.text) }),
            modifier = Modifier.width(width).height(if (hex) 40.dp else 28.dp).clip(ControlShape).background(colors.input)
                .border(2.dp, if (invalid) MaterialTheme.colorScheme.error else colors.accent, ControlShape).padding(horizontal = 4.dp, vertical = 4.dp)
                .focusRequester(focus).testTag("color-value-$tag-input")
                .onPreviewKeyEvent { event -> if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) { onCancel(); true } else false }
                .onFocusChanged { state ->
                    if (focused && !state.isFocused && value.text != edit) onCommit(value.text)
                    focused = state.isFocused; host.editingText = focused; host.textComposition.update(owner, value, focused)
                }.semantics { contentDescription = name })
        return
    }
    Box(Modifier.width(width).height(if (hex) 40.dp else 28.dp).clip(ControlShape).testTag("color-value-$tag")
        .semantics { contentDescription = "$name $text"; onClick { onBegin(); true } }
        .focusable()
        .onKeyEvent { event ->
            val direction = when (event.key) { Key.DirectionUp -> 2f; Key.DirectionDown -> -2f; else -> 0f }
            if (event.type != KeyEventType.KeyDown || direction == 0f || onScrub == null) false
            else { onScrub(direction, if (event.isShiftPressed) "fast" else "normal"); onEndScrub?.invoke(false); true }
        }
        .pointerInput(onScrub != null) {
            awaitEachGesture {
                val down = awaitFirstDown()
                var moved = false
                var released = false
                while (true) {
                    val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    val dy = (down.position.y - change.position.y) / density
                    if (!moved && onScrub != null && abs(dy) * density > viewConfiguration.touchSlop) moved = true
                    if (moved) { change.consume(); onScrub?.invoke(dy, "normal") }
                    if (!change.pressed) { released = true; break }
                }
                if (moved) onEndScrub?.invoke(!released) else if (released) onBegin()
            }
        }, contentAlignment = if (hex) Alignment.CenterStart else Alignment.CenterEnd) {
        Text(text, Modifier.padding(horizontal = 4.dp), style = style, maxLines = 1)
    }
}

@Composable private fun DialogWheel(host: CanvasHost, editor: JSONObject, view: JSONObject, width: Float, pick: (JSONObject) -> Unit) {
    val panel = view.getJSONObject("panel")
    val hdr = panel.optBoolean("hdr")
    val stage = width + 28f
    val (inset, height, layout) = remember(stage, hdr) {
        val layout = JSONObject(Native.colorUi(obj("type" to "layout", "size" to stage, "hdr" to hdr).toString(), host.languageTag))
        val inset = layout.array("wheel").getDouble(1).toFloat()
        val bottom = if (hdr) {
            val g = JSONObject(Native.colorUi(obj("type" to "arc", "size" to stage, "fraction" to 0).toString(), host.languageTag)).getJSONObject("geometry")
            g.array("center").getDouble(1).toFloat() + g.number("radius") + max(g.number("width") / 2, g.number("marker_radius")) + 2f
        } else inset + width
        Triple(inset, bottom - inset, layout)
    }
    Box(Modifier.size(width.dp, height.dp).testTag("color-editor-wheel")) {
        ColorWheel(host, panel, editor.getJSONObject("picker").toString(), false, Modifier.size(width.dp), pick)
        if (hdr) Box(Modifier.offset((-inset).dp, (-inset).dp).size(stage.dp, (height + inset).dp).hdrIntensityInput(panel, pick)) {
            ColorIntensityArc(panel, layout, Modifier.matchParentSize(), pick, caption = false)
        }
    }
}

@Composable private fun RecentRow(host: CanvasHost, view: JSONObject, modifier: Modifier, choose: (JSONObject) -> Unit) {
    var tiles by remember { mutableStateOf(listOf<JSONObject>()) }
    LaunchedEffect(Unit) {
        host.query(obj("type" to "swatch_sheet", "query" to "", "current" to view.getJSONObject("value"))) { value ->
            tiles = (value as? JSONObject)?.array("sections")?.objects()?.firstOrNull { it.isNull("palette") }?.array("tiles")?.objects().orEmpty()
        }
    }
    Row(modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        tiles.forEach { tile -> Tile(tile, "color-recent-tile", 34.dp, choose) }
    }
}

@Composable private fun Tile(tile: JSONObject, tag: String, size: Dp, choose: (JSONObject) -> Unit) {
    val colors = LocalPalette.current
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    Box(Modifier.size(size).clip(ControlShape).background(if (hovered) colors.text.copy(alpha = .10f) else Color.Transparent)
        .then(if (tile.optBoolean("current")) Modifier.border(2.dp, colors.accent, ControlShape) else Modifier)
        .hoverable(interactions).clickable { choose(tile.getJSONObject("color")) }.padding(3.dp)
        .semantics { contentDescription = tile.getString("detail") }.testTag(tag)) {
        Box(Modifier.fillMaxSize().clip(ControlShape).background(displayColor(tile.array("rgba"))))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun SwatchSheet(host: CanvasHost, editor: JSONObject, view: JSONObject, search: (String) -> Unit, choose: (JSONObject) -> Unit, close: () -> Unit) {
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy")
    var query by remember { mutableStateOf(TextFieldValue(editor.getJSONObject("picker").getJSONObject("editor").getString("search"))) }
    var shown by remember { mutableStateOf<JSONObject?>(null) }
    var revision by remember { mutableIntStateOf(0) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(query.text, revision) {
        host.query(obj("type" to "swatch_sheet", "query" to query.text, "current" to view.getJSONObject("value"))) { value -> shown = value as? JSONObject }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    Column(Modifier.fillMaxSize().background(colors.settingsBackground).padding(start = 20.dp, end = 20.dp, top = 20.dp).testTag("color-sheet")
        .onPreviewKeyEvent { event -> if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) { if (query.text.isNotEmpty()) { query = TextFieldValue(""); search("") } else close(); true } else false },
        verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            BasicTextField(query, { query = it; search(it.text) }, singleLine = true, textStyle = MaterialTheme.typography.bodyMedium.copy(color = colors.text), cursorBrush = SolidColor(colors.accent),
                modifier = Modifier.weight(1f).height(34.dp).clip(ControlShape).background(colors.input).padding(horizontal = 10.dp, vertical = 8.dp)
                    .focusRequester(focus).testTag("color-sheet-search").semantics { contentDescription = copy.getJSONObject("color").getString("swatch_search") },
                decorationBox = { inner -> Box { if (query.text.isEmpty()) Text(copy.getJSONObject("color").getString("swatch_search"), color = colors.secondary); inner() } })
            IconButton(close, Modifier.size(34.dp).testTag("color-sheet-close")) { SharedIcon("chevron-down", copy.getJSONObject("color").getString("close_swatches"), Modifier.size(16.dp)) }
        }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(bottom = 16.dp).testTag("color-sheet-list"), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            shown?.optString("empty")?.takeIf { !shown!!.isNull("empty") }?.let { Text(it, color = colors.secondary, modifier = Modifier.testTag("color-sheet-empty")) }
            shown?.array("sections")?.objects()?.forEach { section ->
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(section.getString("title")); Text(section.getString("count"), color = colors.secondary)
                    }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        section.array("tiles").objects().forEach { tile -> Tile(tile, "color-sheet-tile", 40.dp, choose) }
                        if (section.optBoolean("can_add")) {
                            Box(Modifier.size(40.dp).padding(3.dp).clip(ControlShape).background(colors.text.copy(alpha = .06f)).clickable {
                                host.dispatch(obj("type" to "color", "action" to obj("op" to "library", "action" to obj("op" to "store", "palette" to section.getLong("palette"), "name" to "", "color" to view.getJSONObject("value")))))
                                revision++
                            }.semantics { contentDescription = copy.getJSONObject("palettes").getString("add_current") }.testTag("color-add-${section.getLong("palette")}"), contentAlignment = Alignment.Center) {
                                SharedIcon("plus", null, Modifier.size(16.dp))
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable private fun PickStrip(host: CanvasHost, editor: JSONObject, view: JSONObject, picker: JSONObject, rendition: JSONObject?) {
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("color")
    val original = view.getJSONObject("value")
    val sample = picker.objectOrNull("preview") ?: original
    val strip = remember(editor.toString(), sample.toString()) {
        JSONObject(Native.colorUi(obj("type" to "editor_strip", "editor" to editor, "sample" to sample).toString(), host.languageTag))
    }
    val previews = remember(sample.toString()) {
        JSONArray(Native.colorUi(obj("type" to "preview", "colors" to JSONArray().put(original).put(sample), "document_space" to view.getJSONObject("panel").getString("rgb_space"),
            "display_space" to "Srgb", "rendition" to rendition).toString(), host.languageTag))
    }
    val density = LocalDensity.current.density
    val area = host.snapshot?.objectOrNull("state")?.objectOrNull("camera")?.array("work_area")
    val point: JSONArray? = picker.optJSONArray("sample_point")
    val corner = remember { arrayOf("top_right") }
    var height by remember { mutableIntStateOf(0) }
    var hover by remember { mutableStateOf<Offset?>(null) }
    val origin = remember(area.toString(), point.toString(), height, hover) {
        area?.let {
            val avoid = JSONArray(listOfNotNull(point, hover?.let { JSONArray(listOf(it.x, it.y)) }))
            JSONObject(Native.colorUi(obj("type" to "strip_placement", "area" to it, "size" to JSONArray(listOf(272f * density, height)), "scale" to density,
                "avoid" to avoid, "corner" to corner[0]).toString(), host.languageTag)).also { placed -> corner[0] = placed.getString("corner") }.array("origin")
        }
    }
    var stage by remember { mutableStateOf(Offset.Zero) }
    val surface = host.surfaceOrigin - stage
    val at = Offset(origin?.optDouble(0)?.toFloat() ?: 0f, origin?.optDouble(1)?.toFloat() ?: 0f) + surface
    Box(Modifier.fillMaxSize().onGloballyPositioned { stage = it.positionInRoot() }) {
        Row(Modifier.offset { IntOffset(at.x.roundToInt(), at.y.roundToInt()) }.width(272.dp).onSizeChanged { height = it.height }.clip(SurfaceShape).background(colors.settingsBackground)
            .pointerInput(at) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull() ?: continue
                        hover = when {
                            change.type == PointerType.Touch -> hover
                            event.type == PointerEventType.Exit -> null
                            event.type == PointerEventType.Move || event.type == PointerEventType.Enter -> at + change.position - surface
                            else -> hover
                        }
                    }
                }
            }
            .clickable { if (host.colorPreview?.objectOrNull("picker")?.optBoolean("editor") == true) host.dispatch(obj("type" to "color_picker", "action" to obj("kind" to "toggle"))) }
            .semantics { contentDescription = copy.getString("picking_strip") }.padding(horizontal = 12.dp, vertical = 8.dp).testTag("color-strip"),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.clip(ControlShape)) {
                Box(Modifier.size(20.dp, 40.dp).background(displayColor(previews.getJSONObject(0))))
                Box(Modifier.size(20.dp, 40.dp).background(displayColor(previews.getJSONObject(1))))
            }
            Column(Modifier.weight(1f)) {
                Row { Text(strip.getString("hex"), Modifier.weight(1f), fontWeight = FontWeight.Medium); if (!strip.isNull("intensity")) Text(strip.getString("intensity")) }
                Row { Text(strip.getString("label"), Modifier.weight(1f), color = colors.secondary); Text(strip.array("values").values().joinToString("  "), maxLines = 1) }
            }
        }
    }
}
