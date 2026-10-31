package art.capycanvas

import android.content.ClipData
import android.content.ClipboardManager
import android.view.KeyEvent as AndroidKeyEvent
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
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.activity.compose.LocalActivity
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.rememberTextMeasurer
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isShiftPressed
import androidx.compose.ui.input.pointer.isCtrlPressed
import androidx.compose.ui.input.pointer.isAltPressed
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
    Row(if(compact) Modifier else Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(if(LocalCompactPanel.current)panelMetrics().spacing else 6.dp), verticalAlignment = Alignment.CenterVertically) {
        if(!compact) Text(label, Modifier.weight(1f))
        Button(onClick = { host.colorEditor = ColorEditorRequest(null, JSONObject(value.toString()), opaque, windowed) { selected, _ -> if (live[0]) change(selected) } }, enabled = enabled,
            modifier = Modifier.size(if(compact) 36.dp else 56.dp, if(LocalCompactPanel.current)panelMetrics().height else 32.dp).semantics {contentDescription=label}.testTag("property-color-$label"), shape=ControlShape, contentPadding = PaddingValues(0.dp),
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

/** A color form's shared editor and view, with the value being typed and its refusal. */
internal class ColorForm(private val host: CanvasHost, opened: JSONObject, val rendition: JSONObject?) {
    var editor by mutableStateOf(opened.getJSONObject("editor")); private set
    var view by mutableStateOf(opened.getJSONObject("view")); private set
    var error by mutableStateOf<String?>(null); private set
    var errorTarget by mutableStateOf<String?>(null); private set
    var editing by mutableStateOf<String?>(null)
    private var refused: JSONObject? = null
    val invalidOpen get() = editing != null && errorTarget == editing && error != null
    fun send(action: JSONObject?): String? {
        val next = JSONObject(Native.colorUi(obj("type" to "editor", "editor" to editor, "action" to action, "display_space" to "Srgb", "rendition" to rendition).toString(), host.languageTag))
        editor = next.getJSONObject("editor"); view = next.getJSONObject("view")
        return next.optString("error").takeIf { !next.isNull("error") && it.isNotBlank() }
    }
    fun act(action: JSONObject, target: String? = null): Boolean {
        val failure = runCatching { send(action) }.getOrElse { it.message ?: host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("read_failed") }
        if (failure != null || target == errorTarget || errorTarget == null) { error = failure; errorTarget = if (failure != null) target else null; refused = if (failure != null) action else null }
        return failure == null
    }
    fun relabel() { runCatching { send(null); refused?.let { error = send(it) } } }
    fun endEditing(target: String) {
        if (errorTarget == target) { error = null; errorTarget = null; refused = null }
        editing = null
    }
}

@Composable internal fun rememberColorForm(host: CanvasHost, slot: String?, color: JSONObject?, opaque: Boolean): ColorForm? {
    val form = remember {
        val rendition = host.snapshot?.objectOrNull("color_panel")?.objectOrNull("rendition")
        runCatching {
            val colorsState = host.snapshot!!.getJSONObject("state").displayColors()
            ColorForm(host, JSONObject(Native.colorUi(obj("type" to "editor_open", "colors" to colorsState, "slot" to slot, "color" to color, "opaque" to opaque,
                "display_space" to "Srgb", "rendition" to rendition).toString(), host.languageTag)), rendition)
        }.getOrNull()
    }
    LaunchedEffect(host.languageTag) { form?.relabel() }
    return form
}

@Composable private fun ColorEditorOverlay(host: CanvasHost, request: ColorEditorRequest) {
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("color")
    val common = host.bootstrap!!.getJSONObject("common")
    val context = LocalContext.current
    val epoch = remember { colorEpoch(host) }
    val form = rememberColorForm(host, request.slot, request.color, request.opaque)
    if (form == null) { SideEffect { host.colorEditor = null }; return }
    val memory = remember { form.editor.getJSONObject("picker").getJSONObject("editor").toString() }
    var sheet by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    var seenPicking by remember { mutableStateOf(false) }
    fun close(result: Pair<JSONObject, Float?>?) {
        val remembered = form.editor.getJSONObject("picker").getJSONObject("editor")
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
            pickerState.objectOrNull("picked")?.let { form.act(obj("op" to "color", "color" to it)) }
        }
    }
    if (picking) {
        if (pickerActive) PickStrip(host, form.editor, form.view, pickerState!!, form.rendition)
        return
    }
    val keys = { event: AndroidKeyEvent ->
        if (host.editingText || host.textComposition.owns(event)) false
        else if (event.keyCode == AndroidKeyEvent.KEYCODE_ESCAPE) {
            if (event.action == AndroidKeyEvent.ACTION_DOWN && event.repeatCount == 0) {
                if (sheet) sheet = false else close(null)
            }
            true
        } else if ((event.isCtrlPressed || event.isMetaPressed) && !event.isAltPressed && !event.isShiftPressed &&
            event.keyCode in listOf(AndroidKeyEvent.KEYCODE_C, AndroidKeyEvent.KEYCODE_X, AndroidKeyEvent.KEYCODE_V)) {
            if (event.action == AndroidKeyEvent.ACTION_DOWN && event.repeatCount == 0) {
                if (event.keyCode == AndroidKeyEvent.KEYCODE_C) copyText(context, form.view.getString("hex"))
                if (event.keyCode == AndroidKeyEvent.KEYCODE_V) {
                    context.getSystemService(ClipboardManager::class.java).primaryClip?.takeIf { it.itemCount > 0 }
                        ?.getItemAt(0)?.text?.let { form.act(obj("op" to "text", "text" to it.toString()), "hex") }
                }
            }
            true
        } else false
    }
    PickerDialog(host, request.windowed, "color-editor", keys, { close(null) }, page = {
        ColorFormBody(host, form, copy.getString("edit"), "color-editor-title", pick = if (request.windowed) null else ({
            picking = true; seenPicking = false
            val metrics = context.resources.displayMetrics
            host.dispatch(obj("type" to "color_picker", "action" to obj("kind" to "editor", "original" to form.view.getJSONObject("value"),
                "touch_offset" to (metrics.ydpi * 10f / 25.4f).coerceIn(36f * metrics.density, 64f * metrics.density))))
        }))
        androidx.compose.animation.AnimatedVisibility(sheet, Modifier.matchParentSize(), enter = slideInVertically(tween(250, easing = EaseOutCubic)) { it }, exit = slideOutVertically(tween(250, easing = EaseOutCubic)) { it }) {
            SwatchSheet(host, form.editor, form.view, { text -> runCatching { form.send(obj("op" to "search", "text" to text)) } }, { color -> form.act(obj("op" to "color", "color" to color)) }) { sheet = false }
        }
    }) {
        androidx.compose.animation.Crossfade(sheet, modifier = Modifier.weight(1f), label = "color-footer") { expanded ->
            if (expanded) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ColorPair(form.view, 38.dp, 34.dp, Modifier.testTag("color-mini-pair"))
                Text(form.view.getString("hex"))
            } else RecentRow(host, form.view, Modifier.fillMaxWidth()) { color -> form.act(obj("op" to "color", "color" to color)) }
        }
        Box(Modifier.size(34.dp).clip(ControlShape).background(colors.button).clickable { sheet = !sheet }
            .semantics { contentDescription = copy.getString(if (sheet) "close_swatches" else "all_swatches") }.testTag("color-swatches"), contentAlignment = Alignment.Center) {
            SharedIcon("chevron-down", null, Modifier.size(16.dp).rotate(if (sheet) 0f else 180f))
        }
        DialogButton(common.getString("cancel"), "color-cancel") { close(null) }
        DialogButton(copy.getString("use_color"), "color-use", accent = true, enabled = !form.invalidOpen) {
            close(form.view.getJSONObject("value") to (if (form.view.isNull("stops")) null else form.view.number("stops")))
        }
    }
}

/** The wheel and shapes, the current and new colors, the hex value and the value rows. `pick`
 * shows the eyedropper, `single` shows only the new color, `mode` takes the eyedropper's place at
 * the end of the header, and `transparent` dims the form and names the transparent new color.
 * `extra` follows the value rows. */
@Composable internal fun ColorFormBody(host: CanvasHost, form: ColorForm, title: String, titleTag: String, pick: (() -> Unit)? = null,
    copies: Boolean = true, single: Boolean = false, mode: (@Composable () -> Unit)? = null, transparent: String? = null, extra: @Composable ColumnScope.() -> Unit = {}) {
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("color")
    val context = LocalContext.current
    val view = form.view
    val wide = colorEditorWide()
    val dimmed = Modifier.alpha(if (transparent == null) 1f else .4f)
    val wheel = @Composable { modifier: Modifier ->
        Column(modifier.then(dimmed), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(WheelGap)) {
            DialogWheel(host, form.editor, view, wheelSize(wide)) { form.act(obj("op" to "wheel", "action" to it)) }
            Row(Modifier.clip(TileShape).background(colors.text.copy(alpha = .06f)).padding(ShapeInset).testTag("color-shapes"), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                val choices = view.array("shapes").objects()
                Shapes.forEachIndexed { index, (shape, label) ->
                    val selected = choices.getOrNull(index)?.optBoolean("selected") == true
                    Row(Modifier.height(ShapeRow).clip(TileShape).background(if (selected) colors.headerActive else Color.Transparent)
                        .clickable(role = Role.RadioButton) { form.act(obj("op" to "wheel", "action" to obj("op" to "shape", "shape" to shape))) }
                        .padding(horizontal = 8.dp).semantics { this.selected = selected; contentDescription = copy.getString(shape) }.testTag("color-shape-choice-$shape"),
                        verticalAlignment = Alignment.CenterVertically) {
                        SharedIcon("color-$shape", null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp))
                        Text(label, fontWeight = FontWeight.Medium)
                    }
                }
            }
        }
    }
    val hex = @Composable {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            if (transparent != null) Text(transparent, Modifier.testTag("color-transparent"), color = colors.secondary, fontSize = 17.sp, maxLines = 1)
            else {
                view.objectOrNull("hex_note")?.let { note -> Badge(note.getString("text"), Modifier.testTag("color-hex-note")) }
                ValueCell(view.getString("hex"), view.getString("hex"), copy.getString("hex"), "hex", form.editing == "hex", form.errorTarget == "hex" && form.error != null, true,
                    { form.editing = "hex" }, { text -> if (form.act(obj("op" to "text", "text" to text), "hex")) form.editing = null }, { form.endEditing("hex") })
                if (copies) CopyButton(copy, "color-copy-hex") { copyText(context, view.getString("hex")) }
            }
        }
    }
    val header = @Composable {
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ColorPair(view, 54.dp, 48.dp, Modifier.testTag("color-pair"),
                    Modifier.clickable { form.act(obj("op" to "revert")) }.semantics { contentDescription = copy.getString("current") }.testTag("color-current"),
                    Modifier.semantics { contentDescription = copy.getString("new") }.testTag("color-new"), transparent != null, single)
                if (pick != null) IconButton(pick, Modifier.size(48.dp).clip(ControlShape).background(colors.button).testTag("color-pick")) {
                    SharedIcon("eyedropper", copy.getString("pick_canvas"), Modifier.size(24.dp))
                }
                if (mode != null && wide) { hex(); Spacer(Modifier.weight(1f)); mode() }
                else { Spacer(Modifier.weight(1f)); hex() }
            }
            if (!single) Row(Modifier.width(108.dp)) {
                Text(copy.getString("current"), Modifier.weight(1f), color = colors.secondary, fontSize = 11.sp, textAlign = TextAlign.Center)
                Text(copy.getString("new"), Modifier.weight(1f), color = colors.secondary, fontSize = 11.sp, textAlign = TextAlign.Center)
            }
            if (mode != null && !wide) Box(Modifier.padding(top = 6.dp)) { mode() }
        }
    }
    val rows = @Composable {
        val formNames = view.array("rows").objects().flatMap { it.array("forms").objects().map { form -> form.getString("label") } }
        Column(dimmed, verticalArrangement = Arrangement.spacedBy(2.dp)) {
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
                                shown.array("forms").objects().forEach { choice ->
                                    DropdownMenuItem(text = { Text(choice.getString("label"), fontWeight = if (choice.getString("form") == shown.getString("form")) FontWeight.Bold else FontWeight.Normal) },
                                        modifier = Modifier.testTag("color-form-${choice.getString("form")}"), onClick = { open = false; form.act(obj("op" to "form", "row" to row, "form" to choice.getString("form"))) })
                                }
                            }
                        }
                        if (!shown.isNull("space")) Badge(shown.getString("space"))
                    }
                    shown.array("values").objects().forEachIndexed { index, value ->
                        val name = "$row-$index"
                        val target = obj("kind" to "value", "row" to row, "index" to index)
                        ValueCell(value.getString("text"), value.getString("edit"), value.getString("name"), name, form.editing == name, form.errorTarget == name && form.error != null, false,
                            { form.editing = name }, { text -> if (form.act(obj("op" to "value", "row" to row, "index" to index, "text" to text), name)) form.editing = null },
                            { form.endEditing(name) },
                            { pixels, speed -> form.act(obj("op" to "scrub", "target" to target, "pixels" to pixels, "speed" to speed), name) },
                            { cancel -> form.act(obj("op" to "end_scrub", "cancel" to cancel), name) })
                    }
                    if (copies) CopyButton(copy, "color-copy-$row") { copyText(context, shown.getString("copy")) }
                }
            }
            view.objectOrNull("intensity")?.let { value ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(copy.getString("intensity_ev"), Modifier.weight(1f).padding(start = 6.dp))
                    val target = obj("kind" to "intensity")
                    ValueCell(value.getString("text"), value.getString("edit"), value.getString("name"), "ev", form.editing == "ev", form.errorTarget == "ev" && form.error != null, false,
                        { form.editing = "ev" }, { text -> if (form.act(obj("op" to "intensity", "text" to text), "ev")) form.editing = null },
                        { form.endEditing("ev") },
                        { pixels, speed -> form.act(obj("op" to "scrub", "target" to target, "pixels" to pixels, "speed" to speed), "ev") },
                        { cancel -> form.act(obj("op" to "end_scrub", "cancel" to cancel), "ev") }, wide = true)
                    Spacer(Modifier.width(28.dp))
                }
            }
            if (form.error != null) Text(form.error!!, Modifier.padding(top = 6.dp).testTag("color-editor-error"), color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
            extra()
        }
    }
    Column(Modifier.verticalScroll(rememberScrollState()).padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        DialogTitle(title, titleTag)
        if (wide) Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
            wheel(Modifier)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(14.dp)) { header(); rows() }
        } else Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            header(); wheel(Modifier.align(Alignment.CenterHorizontally)); rows()
        }
    }
}

private val WheelGap = 6.dp
private val ShapeRow = 26.dp
private val ShapeInset = 4.dp
private fun wheelSize(wide: Boolean) = if (wide) 232f else 208f
@Composable private fun colorEditorWide() = LocalConfiguration.current.screenWidthDp >= 744

@Composable internal fun colorEditorBodyHeight(): Dp {
    val wheel = wheelSize(colorEditorWide()).dp + WheelGap + ShapeRow + ShapeInset * 2
    if (colorEditorWide()) return wheel
    val measurer = rememberTextMeasurer()
    val style = LocalTextStyle.current
    with(LocalDensity.current) {
        val line = measurer.measure("", style).size.height.toDp()
        val caption = measurer.measure("", style.copy(fontSize = 11.sp)).size.height.toDp()
        val row = maxOf(line + 12.dp, 28.dp)
        return 48.dp + 2.dp + caption + 14.dp + wheel + 14.dp + row * 3 + 4.dp
    }
}

@Composable private fun ColorPair(view: JSONObject, width: Dp, height: Dp, modifier: Modifier, current: Modifier = Modifier, new: Modifier = Modifier,
    transparent: Boolean = false, single: Boolean = false) {
    val colors = LocalPalette.current
    Row(modifier.clip(ControlShape).border(1.dp, colors.divider, ControlShape)) {
        if (!single) Box(Modifier.size(width, height).background(displayColor(view.getJSONObject("current"))).then(current))
        if (transparent) PaletteFace(Color.Transparent, Modifier.size(width, height).then(new), RectangleShape)
        else Box(Modifier.size(width, height).background(displayColor(view.getJSONObject("new"))).then(new))
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
        var cancelled by remember { mutableStateOf(false) }
        LaunchedEffect(Unit) { focus.requestFocus() }
        DisposableEffect(owner) { onDispose { host.textComposition.clear(owner); if (focused) host.editingText = false } }
        BasicTextField(value, { value = it; host.textComposition.update(owner, value, focused) }, singleLine = true, textStyle = style, cursorBrush = SolidColor(colors.accent),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done), keyboardActions = KeyboardActions(onDone = { onCommit(value.text) }),
            modifier = Modifier.width(width).height(if (hex) 40.dp else 28.dp).textCursorRegion().clip(ControlShape).background(colors.dialogField)
                .border(2.dp, if (invalid) MaterialTheme.colorScheme.error else colors.accent, ControlShape).padding(horizontal = 4.dp, vertical = 4.dp)
                .focusRequester(focus).testTag("color-value-$tag-input")
                .onPreviewKeyEvent { event -> if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) { cancelled = true; onCancel(); true } else false }
                .onFocusChanged { state ->
                    if (!cancelled && focused && !state.isFocused && value.text != edit) onCommit(value.text)
                    focused = state.isFocused; host.editingText = focused; host.textComposition.update(owner, value, focused)
                }.semantics { contentDescription = name })
        return
    }
    Box(Modifier.width(width).height(if (hex) 40.dp else 28.dp).clip(ControlShape).testTag("color-value-$tag")
        .semantics { contentDescription = "$name $text"; onClick { onBegin(); true } }
        .onKeyEvent { event ->
            val direction = when (event.key) { Key.DirectionUp -> 2f; Key.DirectionDown -> -2f; else -> 0f }
            if (event.type != KeyEventType.KeyDown || direction == 0f || onScrub == null) false
            else { onScrub(direction, if (event.isShiftPressed) "fast" else if (event.isCtrlPressed || event.isAltPressed) "fine" else "normal"); onEndScrub?.invoke(false); true }
        }.focusable()
        .edgeControl().pointerInput(onScrub != null) {
            awaitEachGesture {
                val down = awaitFirstDown()
                var moved = false
                var released = false
                try { while (true) {
                    val event = awaitPointerEvent()
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (change.isConsumed) break
                    val dy = (down.position.y - change.position.y) / density
                    if (!moved && onScrub != null && abs(dy) * density > viewConfiguration.touchSlop) moved = true
                    if (moved) { change.consume(); onScrub?.invoke(dy, if (event.keyboardModifiers.isShiftPressed) "fast" else if (event.keyboardModifiers.isCtrlPressed || event.keyboardModifiers.isAltPressed) "fine" else "normal") }
                    if (!change.pressed) { released = true; break }
                }
                } finally { if (moved) onEndScrub?.invoke(!released) }
                if (!moved && released) onBegin()
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
        ColorWheel(host, panel, editor.getJSONObject("picker").toString(), false, Modifier.size(width.dp), color = pick)
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
    val owner = remember { Any() }
    var focused by remember { mutableStateOf(false) }
    DisposableEffect(owner) { onDispose { host.textComposition.clear(owner); if (focused) host.editingText = false } }
    LaunchedEffect(query.text, revision) {
        host.query(obj("type" to "swatch_sheet", "query" to query.text, "current" to view.getJSONObject("value"))) { value -> shown = value as? JSONObject }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    Column(Modifier.fillMaxSize().background(colors.settingsBackground).padding(start = 20.dp, end = 20.dp, top = 20.dp).testTag("color-sheet")
        .onPreviewKeyEvent { event -> if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) { if (query.text.isNotEmpty()) { query = TextFieldValue(""); host.textComposition.update(owner, query, focused); search("") } else close(); true } else false },
        verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            BasicTextField(query, { query = it; host.textComposition.update(owner, query, focused); search(it.text) }, singleLine = true, textStyle = MaterialTheme.typography.bodyMedium.copy(color = colors.text), cursorBrush = SolidColor(colors.accent),
                modifier = Modifier.weight(1f).height(34.dp).textCursorRegion().clip(ControlShape).background(colors.dialogField).padding(horizontal = 10.dp, vertical = 8.dp)
                    .focusRequester(focus).onFocusChanged { state -> focused = state.isFocused; host.editingText = focused; host.textComposition.update(owner, query, focused) }
                    .testTag("color-sheet-search").semantics { contentDescription = copy.getJSONObject("color").getString("swatch_search") },
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
    val points = listOfNotNull(picker.optJSONArray("sample_point"), picker.optJSONArray("loupe_point"))
    val corner = remember { arrayOf("top_right") }
    var height by remember { mutableIntStateOf(0) }
    var hover by remember { mutableStateOf<Offset?>(null) }
    val origin = remember(area.toString(), points.toString(), height, hover) {
        area?.let {
            val avoid = JSONArray(points + listOfNotNull(hover?.let { JSONArray(listOf(it.x, it.y)) }))
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
