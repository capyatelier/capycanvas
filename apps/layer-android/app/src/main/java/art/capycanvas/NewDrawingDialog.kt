package art.capycanvas

import android.view.KeyEvent as AndroidKeyEvent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.EaseOutCubic
import androidx.compose.animation.core.tween
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageShader
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

private val FieldHeight = 36.dp
private val FooterHeight = 34.dp
private val ControlGap = 6.dp
private val RowGap = 10.dp
private val SectionGap = 14.dp
private val Preview = DpSize(232.dp, 250.dp)
private val NarrowPreview = DpSize(348.dp, 140.dp)
private val PreviewInset = 36.dp
private val WideDialog = 600.dp
private val NumberWidth = 96.dp
private val LinkWidth = 26.dp
private val UnitWidth = 76.dp
private val ReliefWidth = 76.dp
private val OrientationWidth = 70.dp
private val PresetTile = 120.dp
private val PresetFrame = DpSize(116.dp, 86.dp)
private val PresetFit = 74.dp
private val SummaryArt = 36.dp
private val Thumbnail = 36.dp
private val Swatch = 44.dp

/** The display's size in device pixels and its physical pixels per inch. */
internal fun screenInfo(view: android.view.View): JSONObject? = screenInfo(view.display)
internal fun screenInfo(display: android.view.Display?): JSONObject? {
    display ?: return null
    val metrics = android.util.DisplayMetrics()
    @Suppress("DEPRECATION") display.getRealMetrics(metrics)
    if (metrics.widthPixels <= 0 || metrics.heightPixels <= 0) return null
    val ppi = ((metrics.xdpi + metrics.ydpi) / 2).takeIf { it.isFinite() && it > 0 }?.roundToInt()
    return obj("extent" to JSONArray(listOf(metrics.widthPixels, metrics.heightPixels)), "ppi" to ppi)
}

private class NumberDraft(val typing: () -> Boolean, val commit: () -> Boolean)

private class NewDrawingActions(private val host: CanvasHost) {
    private val drafts = linkedMapOf<Any, NumberDraft>()
    var importFocused = false
    fun register(owner: Any, draft: NumberDraft?) { if (draft == null) drafts.remove(owner) else drafts[owner] = draft }
    fun typing() = drafts.values.any { it.typing() }
    fun commit() = drafts.values.toList().map { it.commit() }.all { it }
    fun send(op: String, vararg fields: Pair<String, Any?>) { host.documents.newDrawing(obj("op" to op, *fields)) }
    fun page(op: String, vararg fields: Pair<String, Any?>) { commit(); send(op, *fields) }
    fun create() { if (commit()) host.documents.createNewDrawing() }
    fun usePaper() { if (commit()) send("use_paper") }
}

private class DrawerMemory { var shown: JSONObject? = null; var open = false; var opening = 0 }

@Composable private fun fieldColor() = LocalPalette.current.text.copy(alpha = .07f)
@Composable private fun smallSize() = LocalTextStyle.current.fontSize * .9f
@Composable private fun captionSize() = LocalTextStyle.current.fontSize * .82f

@Composable private fun rememberDisplayColors(host: CanvasHost, values: List<JSONObject>): List<Color> {
    val key = values.joinToString(",")
    return remember(key) {
        if (values.isEmpty()) emptyList()
        else JSONArray(Native.colorUi(obj("type" to "preview", "colors" to JSONArray(values)).toString(), host.languageTag)).objects().map(::displayColor)
    }
}

@Composable internal fun NewDrawingLayer(host: CanvasHost) {
    val view = host.snapshot?.objectOrNull("state")?.objectOrNull("new_drawing") ?: return
    val actions = remember(host) { NewDrawingActions(host) }
    Box(Modifier.fillMaxSize().imePadding()) { NewDrawingDialog(host, view, actions) }
}

@Composable private fun NewDrawingDialog(host: CanvasHost, view: JSONObject, actions: NewDrawingActions) {
    val copy = view.getJSONObject("copy")
    val drawer = view.objectOrNull("drawer")
    val memory = remember { DrawerMemory() }
    if (drawer != null) { if (!memory.open) memory.opening++; memory.shown = drawer }
    memory.open = drawer != null
    val wide = minOf(720, LocalConfiguration.current.screenWidthDp - 32).dp >= WideDialog
    fun accept() {
        when (drawer?.getString("kind")) {
            null -> if (view.getBoolean("can_create")) actions.create()
            "presets" -> drawer.objectOrNull("save")?.let { if (it.getBoolean("can_save")) actions.send("save_preset") }
                ?: run { if (drawer.getBoolean("can_select")) actions.send("use_preset") }
            "paper" -> actions.usePaper()
        }
    }
    val keys = { event: AndroidKeyEvent ->
        val pressed = event.action == AndroidKeyEvent.ACTION_DOWN && event.repeatCount == 0
        val kind = drawer?.getString("kind")
        if (host.textComposition.owns(event)) false
        else when (event.keyCode) {
            AndroidKeyEvent.KEYCODE_ESCAPE -> if (actions.typing() || (kind == "paper_color" && host.editingText)) false else { if (pressed) actions.send("back"); true }
            AndroidKeyEvent.KEYCODE_ENTER, AndroidKeyEvent.KEYCODE_NUMPAD_ENTER ->
                if (host.editingText || kind == "paper_color" || (kind == "paper" && actions.importFocused)) false else { if (pressed) accept(); true }
            else -> false
        }
    }
    PickerDialog(host, false, "new-drawing-dialog", keys, { actions.send("back") }, page = {
        Box(if (drawer == null) Modifier else Modifier.clearAndSetSemantics {}) { MainPage(host, view, actions, wide) }
    }, sheet = {
        AnimatedVisibility(drawer != null, Modifier.matchParentSize(), enter = slideInVertically(tween(250, easing = EaseOutCubic)) { it },
            exit = slideOutVertically(tween(250, easing = EaseOutCubic)) { it }) {
            val shown = memory.shown ?: return@AnimatedVisibility
            key(memory.opening) {
                when (shown.getString("kind")) {
                    "presets" -> PresetsDrawer(host, copy, shown, actions, wide)
                    "paper" -> PaperDrawer(host, copy, shown, actions)
                    else -> PaperColorDrawer(host, copy, shown, actions)
                }
            }
        }
    }) {
        val colors = LocalPalette.current
        Button({ actions.page("presets") }, Modifier.height(34.dp).testTag("new-drawing-presets-button"), shape = ControlShape, contentPadding = PaddingValues(horizontal = 16.dp),
            colors = ButtonDefaults.buttonColors(containerColor = colors.button, contentColor = colors.text)) {
            Text(copy.getString("presets"), fontWeight = FontWeight.Medium)
            Spacer(Modifier.width(6.dp))
            SharedIcon("chevron-down", null, Modifier.size(12.dp).rotate(180f))
        }
        Spacer(Modifier.weight(1f))
        DialogButton(copy.getString("cancel"), "new-drawing-cancel") { actions.send("cancel") }
        DialogButton(copy.getString("create"), "new-drawing-create", accent = true, enabled = view.getBoolean("can_create")) { actions.create() }
    }
}

@Composable private fun MainPage(host: CanvasHost, view: JSONObject, actions: NewDrawingActions, wide: Boolean) {
    Column(Modifier.verticalScroll(rememberScrollState()).padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(SectionGap)) {
        DialogTitle(view.getJSONObject("copy").getString("title"), "new-drawing-title")
        if (wide) Row(horizontalArrangement = Arrangement.spacedBy(24.dp)) {
            SheetPreview(host, view, actions, Modifier.width(Preview.width), Modifier.size(Preview))
            DrawingForm(host, view, actions, Modifier.weight(1f))
        } else Column(verticalArrangement = Arrangement.spacedBy(SectionGap)) {
            SheetPreview(host, view, actions, Modifier.fillMaxWidth(), Modifier.widthIn(max = NarrowPreview.width).fillMaxWidth().height(NarrowPreview.height))
            DrawingForm(host, view, actions, Modifier.fillMaxWidth())
        }
    }
}

@Composable private fun SheetPreview(host: CanvasHost, view: JSONObject, actions: NewDrawingActions, modifier: Modifier, well: Modifier) {
    val colors = LocalPalette.current
    val copy = view.getJSONObject("copy")
    val page = view.getJSONObject("page")
    val sheet = view.getJSONObject("sheet")
    val paper = rememberDisplayColors(host, listOf(sheet.getJSONObject("color"))).first()
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        BoxWithConstraints(well.clip(SquircleShape(22.dp)).background(colors.view).clickable(role = Role.Button) { actions.page("presets") }
            .semantics { contentDescription = copy.getString("presets") }.testTag("new-drawing-preview"), contentAlignment = Alignment.Center) {
            SheetArt(host, sheet, paper, DpSize(maxWidth - PreviewInset, maxHeight - PreviewInset), 40.dp, false)
        }
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(buildAnnotatedString {
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(page.getString("preset")) }
                withStyle(SpanStyle(color = colors.text.copy(alpha = .6f))) { append(" · " + page.getString("orientation_label")) }
            }, Modifier.testTag("new-drawing-caption"), textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
            HoverTip(page.getString("memory")) {
                Text(page.getString("capacity"), Modifier.semantics { stateDescription = page.getString("memory") }.testTag("new-drawing-capacity"),
                    fontSize = smallSize(), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** A sheet at its proportions within `fit`, in its paper color with the paper's grain, or transparent. */
@Composable private fun SheetArt(host: CanvasHost, sheet: JSONObject, color: Color, fit: DpSize, grain: Dp, mini: Boolean) {
    val colors = LocalPalette.current
    val extent = sheet.getJSONArray("extent")
    val width = extent.getDouble(0).coerceAtLeast(1.0)
    val height = extent.getDouble(1).coerceAtLeast(1.0)
    val side = minOf(fit.width, fit.height).value.toDouble()
    val scale = minOf(side / maxOf(width, height), fit.width.value / width, fit.height.value / height)
    val texture = if (sheet.isNull("texture")) null else sheet.getString("texture")
    if (texture != null) LaunchedEffect(host, texture) { host.paperPreviews.load(texture) }
    val image = texture?.let { host.paperPreviews.grain(it) }
    val tile = with(LocalDensity.current) { grain.toPx() }
    val brush = remember(image, tile) {
        image?.let { grainImage ->
            ShaderBrush(ImageShader(grainImage, TileMode.Repeated, TileMode.Repeated).apply {
                setLocalMatrix(android.graphics.Matrix().apply { setScale(tile / grainImage.width, tile / grainImage.height) })
            })
        }
    }
    val shown = sheet.getBoolean("shown")
    Canvas(Modifier.size((width * scale).coerceAtLeast(4.0).dp, (height * scale).coerceAtLeast(4.0).dp)
        .shadow(if (mini) 1.dp else 3.dp, RectangleShape)) {
        if (shown) drawRect(color)
        else {
            val cell = 5.dp.toPx()
            for (y in 0..(size.height / cell).toInt()) for (x in 0..(size.width / cell).toInt())
                drawRect(if ((x + y) % 2 == 0) colors.checkerLight else colors.checkerDark, androidx.compose.ui.geometry.Offset(x * cell, y * cell), androidx.compose.ui.geometry.Size(cell, cell))
        }
        brush?.let { drawRect(it, alpha = if (shown) 1f else .35f) }
    }
}

@Composable private fun Section(heading: String, content: @Composable ColumnScope.() -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(heading, fontWeight = FontWeight.Bold)
        content()
    }
}

@Composable private fun DrawingForm(host: CanvasHost, view: JSONObject, actions: NewDrawingActions, modifier: Modifier) {
    val colors = LocalPalette.current
    val copy = view.getJSONObject("copy")
    val setup = view.getJSONObject("setup")
    val page = view.getJSONObject("page")
    val sheet = view.getJSONObject("sheet")
    Column(modifier, verticalArrangement = Arrangement.spacedBy(SectionGap)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(RowGap)) {
            Text(copy.getString("name"))
            CoreTextField(view.getString("name"), { actions.send("name", "text" to it) }, Modifier.weight(1f).semantics { contentDescription = copy.getString("name") }.testTag("new-drawing-name"),
                placeholder = { Text(view.getString("untitled")) }, height = FieldHeight, shape = ControlShape, background = fieldColor())
        }
        Section(copy.getString("size")) {
            Column(verticalArrangement = Arrangement.spacedBy(RowGap)) {
                Dimensions(view, actions)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(ControlGap)) {
                    Text(copy.getString("resolution"), Modifier.padding(end = 4.dp))
                    ResolutionField(view, actions)
                    Text(page.getString("size"), Modifier.weight(1f).padding(start = 4.dp).testTag("new-drawing-size"), fontSize = smallSize(),
                        color = if (page.getBoolean("problem")) MaterialTheme.colorScheme.error else colors.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        Section(copy.getString("color")) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(ControlGap)) {
                val color = setup.getJSONObject("color")
                Captioned(copy.getString("space"), Modifier.weight(4f)) {
                    val spaces = view.getJSONArray("spaces").objects()
                    Choice(spaces.map { it.getString("label") }, spaces.indexOfFirst { it.getString("value") == color.getString("space") }, "new-drawing-space", copy.getString("space")) {
                        actions.page("space", "space" to spaces[it].getString("value"))
                    }
                }
                Captioned(copy.getString("depth"), Modifier.weight(5f)) {
                    val depths = view.getJSONArray("depths").objects()
                    Choice(depths.map { it.getString("label") }, depths.indexOfFirst { it.getString("value") == color.getString("depth") }, "new-drawing-depth", copy.getString("depth")) {
                        actions.page("depth", "depth" to depths[it].getString("value"))
                    }
                }
                Captioned(copy.getString("blending"), Modifier.weight(4f)) {
                    val blendings = view.getJSONArray("blendings").objects()
                    val note = if (view.isNull("blending_note")) null else view.getString("blending_note")
                    Choice(blendings.map { it.getString("label") }, blendings.indexOfFirst { it.getString("value") == setup.getString("blend_space") }, "new-drawing-blending",
                        copy.getString("blending"), enabled = note == null, note = note) {
                        actions.page("blending", "blending" to blendings[it].getString("value"))
                    }
                }
            }
        }
        Section(copy.getString("paper")) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(ControlGap)) {
                val texture = if (sheet.isNull("texture")) null else sheet.getString("texture")
                val relief = if (page.isNull("relief")) null else page.getString("relief")
                PaperTile(page.getString("texture"), relief, copy.getString("paper"), "new-drawing-texture", Modifier.weight(1f), true, { actions.page("paper") }) {
                    PaperPreview(host, texture, Thumbnail, colors.text.copy(alpha = .06f), "none")
                }
                val paper = rememberDisplayColors(host, listOf(sheet.getJSONObject("color"))).first()
                PaperTile(page.getString("paper_color"), null, copy.getString("paper_color"), "new-drawing-paper-color", Modifier.weight(1f), view.getBoolean("paper_color_enabled"), { actions.page("paper_color") }) {
                    PaletteFace(if (sheet.getBoolean("shown")) paper else Color.Transparent, Modifier.size(Thumbnail), TileShape)
                }
            }
        }
    }
}

@Composable private fun Dimensions(view: JSONObject, actions: NewDrawingActions) {
    val colors = LocalPalette.current
    val copy = view.getJSONObject("copy")
    val setup = view.getJSONObject("setup")
    val values = view.getJSONArray("values")
    val numeric = view.getJSONObject("numeric")
    val linked = view.getBoolean("linked")
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val number = ((maxWidth - LinkWidth - UnitWidth - OrientationWidth - ControlGap * 2) / 2).coerceIn(56.dp, NumberWidth)
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            key(numeric.toString()) {
                DialogNumber(actions, values.getDouble(0), numeric, copy.getString("width_name"), copy.getString("width"), null, "new-drawing-width", Modifier.width(number)) {
                    actions.send("width", "value" to it)
                }
            }
            Box(Modifier.size(LinkWidth, FieldHeight).clip(ControlShape).toggleable(linked, role = Role.Checkbox) { actions.page("linked", "linked" to it) }
                .semantics { contentDescription = copy.getString("link") }.testTag("new-drawing-link"), contentAlignment = Alignment.Center) {
                SharedIcon(if (linked) "link" else "unlink", null, Modifier.size(12.dp), tint = if (linked) colors.accent else colors.text.copy(alpha = .62f))
            }
            key(numeric.toString()) {
                DialogNumber(actions, values.getDouble(1), numeric, copy.getString("height_name"), copy.getString("height"), null, "new-drawing-height", Modifier.width(number)) {
                    actions.send("height", "value" to it)
                }
            }
            Spacer(Modifier.width(ControlGap))
            val units = view.getJSONArray("units").objects()
            Choice(units.map { it.getString("label") }, units.indexOfFirst { it.getString("value") == setup.getString("unit") }, "new-drawing-unit", copy.getString("unit"),
                Modifier.width(UnitWidth)) { actions.page("unit", "unit" to units[it].getString("value")) }
            Spacer(Modifier.weight(1f).widthIn(min = ControlGap))
            val orientation = view.getJSONObject("page").getString("orientation")
            Row(Modifier.width(OrientationWidth).clip(SquircleShape(ControlRadius + 3.dp)).background(fieldColor()).padding(3.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                for (choice in listOf("portrait", "landscape")) {
                    Box(Modifier.weight(1f).height(FieldHeight - 6.dp).clip(ControlShape).background(if (orientation == choice) colors.headerActive else Color.Transparent)
                        .selectable(orientation == choice, role = Role.RadioButton) { actions.page("orientation", "orientation" to choice) }
                        .semantics { contentDescription = copy.getString(choice) }.testTag("new-drawing-$choice"), contentAlignment = Alignment.Center) {
                        SharedIcon(choice, null, Modifier.size(16.dp))
                    }
                }
            }
        }
    }
}

@Composable private fun ResolutionField(view: JSONObject, actions: NewDrawingActions) {
    val colors = LocalPalette.current
    val copy = view.getJSONObject("copy")
    val focus = LocalFocusManager.current
    var open by remember { mutableStateOf(false) }
    Box {
        DialogNumber(actions, view.getJSONObject("setup").getDouble("resolution"), view.getJSONObject("resolution_numeric"), copy.getString("resolution"), null,
            copy.getString("resolution_unit"), "new-drawing-resolution", Modifier.width(124.dp), trailing = {
                Box(Modifier.padding(end = 3.dp).size(30.dp).clip(ControlShape).clickable(role = Role.Button) { focus.clearFocus(); open = true }
                    .semantics { contentDescription = copy.getString("common_resolutions") }.testTag("new-drawing-resolution-choices"), contentAlignment = Alignment.Center) {
                    SharedIcon("chevron-down", null, Modifier.size(12.dp))
                }
            }) { actions.send("resolution", "value" to it) }
        DropdownMenu(open, { open = false }, Modifier.testTag("new-drawing-resolution-menu")) {
            view.getJSONArray("resolutions").objects().forEachIndexed { index, choice ->
                DropdownMenuItem(text = {
                    Row(Modifier.widthIn(min = 180.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        Text(choice.getString("label"), Modifier.weight(1f))
                        Text(choice.getString("purpose"), color = colors.secondary)
                    }
                }, enabled = choice.getBoolean("enabled"), onClick = { open = false; actions.page("resolution", "value" to choice.getInt("value")) },
                    modifier = Modifier.background(if (choice.getBoolean("selected")) colors.text.copy(alpha = .10f) else Color.Transparent).testTag("new-drawing-resolution-$index"))
            }
        }
    }
}

/** A number typed in place. Rust reads the text; Done, Enter or leaving the field commits it, Escape restores the value. */
@Composable private fun DialogNumber(actions: NewDrawingActions, value: Double, control: JSONObject, name: String, prefix: String?, suffix: String?, tag: String,
    modifier: Modifier, height: Dp = FieldHeight, enabled: Boolean = true, trailing: @Composable RowScope.() -> Unit = {}, onValue: (Double) -> Unit) {
    val host = LocalCanvasHost.current
    val colors = LocalPalette.current
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    fun resolve(base: Double, operation: JSONObject) =
        JSONObject(Native.number(obj("control" to control, "value" to base, "operation" to operation).toString(), host.languageTag))
    val shown = remember(value, control.toString(), host.languageTag) { resolve(value, obj("type" to "format")) }
    var focused by remember { mutableStateOf(false) }
    var dirty by remember { mutableStateOf(false) }
    var invalid by remember { mutableStateOf(false) }
    var awaiting by remember { mutableStateOf<String?>(null) }
    var text by remember { mutableStateOf(TextFieldValue(shown.getString("text"))) }
    LaunchedEffect(shown.toString(), focused) {
        if (dirty || awaiting == shown.toString()) return@LaunchedEffect
        awaiting = null
        val edit = shown.getString("edit")
        text = if (focused) TextFieldValue(edit, TextRange(0, edit.length)) else TextFieldValue(shown.getString("text"))
    }
    fun commit(): Boolean {
        if (text.composition != null) return false
        if (!dirty) return true
        return try {
            val next = resolve(shown.getDouble("value"), obj("type" to "expression", "text" to text.text))
            dirty = false; invalid = false
            text = TextFieldValue(next.getString(if (focused) "edit" else "text"))
            if (next.getDouble("value") != value) { awaiting = shown.toString(); onValue(next.getDouble("value")) }
            true
        } catch (e: Exception) { invalid = true; false }
    }
    val committing by rememberUpdatedState(::commit)
    val typing by rememberUpdatedState { dirty }
    DisposableEffect(requester) {
        actions.register(requester, NumberDraft({ typing() }, { committing() }))
        onDispose {
            actions.register(requester, null)
            if (focused) host.editingText = false
            host.textComposition.clear(requester)
        }
    }
    Row(modifier.height(height).clip(ControlShape).background(fieldColor())
        .then(if (invalid) Modifier.border(1.5.dp, MaterialTheme.colorScheme.error, ControlShape) else Modifier).testTag(tag),
        verticalAlignment = Alignment.CenterVertically) {
        prefix?.let { Text(it, Modifier.padding(start = 10.dp), color = colors.text.copy(alpha = .62f), fontSize = smallSize(), maxLines = 1) }
        BasicTextField(text, { next ->
            if (next.text != text.text || next.composition != null) { dirty = true; invalid = false }
            text = next
            host.textComposition.update(requester, next, focused)
        }, Modifier.weight(1f).textCursorRegion(enabled).padding(start = 4.dp, end = if (suffix == null) 10.dp else 2.dp).focusRequester(requester)
            .onFocusChanged { state ->
                val was = focused
                focused = state.isFocused
                if (!was && focused) awaiting = null
                if (was && !focused) commit()
                host.editingText = focused
                host.textComposition.update(requester, text, focused)
            }.onPreviewKeyEvent { event ->
                if (host.textComposition.owns(event.nativeKeyEvent) || event.type != KeyEventType.KeyDown) false
                else when (event.key) {
                    Key.Escape -> { dirty = false; invalid = false; awaiting = null; text = TextFieldValue(shown.getString("text")); focus.clearFocus(); true }
                    Key.Enter, Key.NumPadEnter -> { if (commit()) focus.clearFocus(); true }
                    else -> false
                }
            }.semantics { contentDescription = name },
            enabled = enabled, singleLine = true, textStyle = LocalTextStyle.current.copy(color = colors.text, textAlign = TextAlign.End, fontFeatureSettings = "tnum"),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (text.composition == null && commit()) focus.clearFocus() }))
        suffix?.let { Text(it, Modifier.padding(horizontal = 3.dp), color = colors.text.copy(alpha = .62f), fontSize = smallSize(), maxLines = 1) }
        trailing()
    }
}

@Composable private fun Captioned(caption: String, modifier: Modifier, content: @Composable () -> Unit) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(caption, color = LocalPalette.current.secondary, fontSize = captionSize(), maxLines = 1, overflow = TextOverflow.Ellipsis)
        content()
    }
}

@Composable private fun Choice(labels: List<String>, selected: Int, tag: String, name: String, modifier: Modifier = Modifier, enabled: Boolean = true, note: String? = null, choose: (Int) -> Unit) {
    val colors = LocalPalette.current
    var open by remember { mutableStateOf(false) }
    HoverTip(note.orEmpty(), modifier, enabled = note != null) {
        Box {
            Row(Modifier.fillMaxWidth().height(FieldHeight).clip(ControlShape).background(fieldColor()).alpha(if (enabled) 1f else .5f)
                .clickable(enabled, role = Role.Button) { open = true }
                .semantics { contentDescription = name; stateDescription = listOfNotNull(labels.getOrNull(selected), note).joinToString(". ") }.testTag(tag)
                .padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(labels.getOrElse(selected) { "" }, Modifier.weight(1f), fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                SharedIcon("chevron-down", null, Modifier.size(12.dp))
            }
            DropdownMenu(open, { open = false }) {
                labels.forEachIndexed { index, label ->
                    DropdownMenuItem(text = { Text(label, fontWeight = if (index == selected) FontWeight.Bold else FontWeight.Normal) },
                        modifier = Modifier.testTag("$tag-$index"), onClick = { open = false; choose(index) })
                }
            }
        }
    }
}

@Composable private fun PaperTile(name: String, detail: String?, description: String, tag: String, modifier: Modifier, enabled: Boolean, open: () -> Unit,
    thumbnail: @Composable () -> Unit) {
    Row(modifier.height(Thumbnail + 8.dp).clip(ControlShape).background(fieldColor()).alpha(if (enabled) 1f else .5f)
        .clickable(enabled, role = Role.Button, onClick = open).semantics { contentDescription = description; stateDescription = listOfNotNull(name, detail).joinToString(", ") }
        .testTag(tag).padding(start = 4.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(RowGap)) {
        thumbnail()
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(name, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            detail?.let { Text(it, Modifier.testTag("$tag-relief"), color = LocalPalette.current.secondary, fontSize = captionSize(), maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
    }
}

@Composable private fun DrawerSheet(tag: String, head: @Composable () -> Unit, footer: @Composable RowScope.() -> Unit, body: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxSize().background(LocalPalette.current.settingsBackground).pointerInput(Unit) {
        awaitEachGesture { awaitFirstDown(requireUnconsumed = false); do { val event = awaitPointerEvent() } while (event.changes.any { it.pressed }) }
    }.testTag(tag)) {
        Box(Modifier.padding(start = 20.dp, end = 20.dp, top = 18.dp)) { head() }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 20.dp, end = 20.dp, top = 10.dp, bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(SectionGap), content = body)
        PickerFooter(tag, footer)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun PresetsDrawer(host: CanvasHost, copy: JSONObject, drawer: JSONObject, actions: NewDrawingActions, wide: Boolean) {
    val colors = LocalPalette.current
    val tiles = drawer.getJSONArray("tiles").objects()
    val sheets = rememberDisplayColors(host, tiles.map { it.getJSONObject("sheet").getJSONObject("color") })
    val save = drawer.objectOrNull("save")
    val focus = LocalFocusManager.current
    DrawerSheet("new-drawing-browser", head = {
        CoreTextField(drawer.getString("query"), { actions.send("search", "query" to it) }, Modifier.fillMaxWidth().semantics { contentDescription = copy.getString("search") }.testTag("new-drawing-search"),
            placeholder = { Text(copy.getString("search")) }, leadingIcon = { SharedIcon("search", null, Modifier.size(16.dp)) },
            height = FieldHeight, shape = ControlShape, background = fieldColor(), focusRequest = 1)
    }, footer = {
        var retainedSave by remember { mutableStateOf(save) }
        if (save != null) retainedSave = save
        val controls = @Composable {
            AnimatedContent(save != null, label = "preset-save-footer", transitionSpec = { fadeIn() togetherWith fadeOut() }) { saving -> FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!saving) {
                DialogButton(copy.getString("cancel"), "new-drawing-browser-cancel") { actions.send("close_drawer") }
                DialogButton(drawer.getString("save_label"), "new-drawing-save-as") { focus.clearFocus(); actions.send("save_as") }
                DialogButton(copy.getString("select"), "new-drawing-select", accent = true, enabled = drawer.getBoolean("can_select")) { actions.send("use_preset") }
            } else retainedSave?.let { save ->
                val canSave = save.getBoolean("can_save")
                CoreTextField(save.getString("name"), { actions.send("preset_name", "text" to it) },
                    Modifier.width(150.dp).then(if (canSave) Modifier else Modifier.border(1.5.dp, MaterialTheme.colorScheme.error, ControlShape))
                        .semantics { contentDescription = copy.getString("preset_name") }.testTag("new-drawing-save-name"),
                    placeholder = { Text(copy.getString("preset_name")) }, height = 34.dp, shape = ControlShape, background = fieldColor())
                val groups = save.getJSONArray("groups").objects()
                Choice(groups.map { it.getString("label") }, groups.indexOfFirst { it.getString("value") == save.getString("group") }, "new-drawing-save-group",
                    copy.getString("preset_category"), Modifier.width(140.dp)) { actions.send("preset_group", "group" to groups[it].getString("value")) }
                DialogButton(copy.getString("cancel"), "new-drawing-save-cancel") { actions.send("cancel_save") }
                DialogButton(save.getString("confirm"), "new-drawing-save-confirm", accent = true, enabled = canSave) { actions.send("save_preset") }
            }
            } }
        }
        if (wide) {
            PresetSummary(host, drawer, Modifier.weight(1f))
            controls()
        } else Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(RowGap)) {
            PresetSummary(host, drawer, Modifier.fillMaxWidth())
            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalArrangement = Arrangement.spacedBy(8.dp)) { controls() }
        }
    }) {
        FlowRow(Modifier.testTag("new-drawing-tabs"), horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            val enabled = drawer.getBoolean("tabs_enabled")
            drawer.getJSONArray("tabs").objects().forEachIndexed { index, tab ->
                val selected = tab.getBoolean("selected")
                Box(Modifier.height(32.dp).clip(SquircleShape(50)).background(if (selected) colors.headerActive else fieldColor()).alpha(if (enabled) 1f else .4f)
                    .selectable(selected, enabled, Role.Tab) { actions.send("tab", "index" to index) }.testTag("new-drawing-tab-$index")
                    .padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                    Text(tab.getString("label"), maxLines = 1)
                }
            }
        }
        BoxWithConstraints(Modifier.fillMaxWidth().testTag("new-drawing-presets")) {
            val columns = ((maxWidth + 10.dp) / (PresetTile + 10.dp)).toInt().coerceAtLeast(2)
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                tiles.withIndex().chunked(columns).forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        row.forEach { (index, tile) -> PresetTileView(host, copy, tile, sheets[index], index, actions, Modifier.weight(1f)) }
                        repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
        if (!drawer.isNull("empty")) Text(drawer.getString("empty"), Modifier.testTag("new-drawing-empty"), color = colors.secondary, fontSize = smallSize())
    }
}

@Composable private fun PresetTileView(host: CanvasHost, copy: JSONObject, tile: JSONObject, color: Color, index: Int, actions: NewDrawingActions, modifier: Modifier) {
    val colors = LocalPalette.current
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    var menu by remember { mutableStateOf(false) }
    val own = tile.getBoolean("own")
    val selected = tile.getBoolean("selected")
    val name = tile.getString("name")
    val focus = LocalFocusManager.current
    val activate = rememberActivation({ actions.send("pick_preset", "index" to index) }) { actions.send("use_preset") }
    Box(modifier) {
        HoverTip("$name\n${tile.getString("dimensions")} · ${tile.getString("details")}", Modifier.fillMaxWidth()) {
            Column(Modifier.fillMaxWidth().alpha(if (tile.getBoolean("enabled")) 1f else .4f)
                .semantics(mergeDescendants = true) { this.selected = selected }.testTag("new-drawing-preset-$index")
                .pointerInput(own) {
                    if (!own) return@pointerInput
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        if (currentEvent.buttons.isSecondaryPressed) { down.consume(); menu = true }
                    }
                }
                .hoverable(interactions)
                .combinedClickable(interactions, null, role = Role.RadioButton, onLongClick = if (own) ({ menu = true }) else null, onClick = activate),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Box(Modifier.size(PresetFrame).clip(SquircleShape(26.dp)).background(if (hovered) colors.text.copy(alpha = .10f) else Color.Transparent)
                    .then(if (selected) Modifier.border(2.dp, colors.accent, SquircleShape(26.dp)) else Modifier), contentAlignment = Alignment.Center) {
                    SheetArt(host, tile.getJSONObject("sheet"), color, DpSize(PresetFit, PresetFit), 14.dp, true)
                }
                Text(name, Modifier.fillMaxWidth(), fontSize = LocalTextStyle.current.fontSize * .93f, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Column {
                    for (line in listOf(tile.getString("dimensions"), tile.getString("details")))
                        Text(line, Modifier.fillMaxWidth(), color = colors.secondary, fontSize = captionSize(), textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        DropdownMenu(menu, { menu = false }) {
            DropdownMenuItem(text = { Text(copy.getString("rename")) }, modifier = Modifier.testTag("new-drawing-preset-rename"),
                onClick = { menu = false; focus.clearFocus(); actions.send("rename_preset", "index" to index) })
            DropdownMenuItem(text = { Text(copy.getString("delete")) }, modifier = Modifier.testTag("new-drawing-preset-delete"),
                onClick = { menu = false; actions.send("delete_preset", "index" to index) })
        }
    }
}

@Composable private fun PresetSummary(host: CanvasHost, drawer: JSONObject, modifier: Modifier) {
    val colors = LocalPalette.current
    val summary = drawer.getJSONObject("summary")
    val sheet = drawer.getJSONObject("summary_sheet")
    val paper = rememberDisplayColors(host, listOf(sheet.getJSONObject("color"))).first()
    Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(RowGap)) {
        Box(Modifier.size(SummaryArt), contentAlignment = Alignment.Center) { SheetArt(host, sheet, paper, DpSize(SummaryArt - 2.dp, SummaryArt - 2.dp), 14.dp, true) }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(summary.getString("name"), Modifier.weight(1f, fill = false).testTag("new-drawing-summary-name"), fontWeight = FontWeight.SemiBold,
                    fontSize = LocalTextStyle.current.fontSize * .93f, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(summary.getString("size"), Modifier.weight(1f, fill = false).testTag("new-drawing-summary-size"), color = colors.secondary, fontSize = smallSize(),
                    maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(summary.getString("details"), Modifier.testTag("new-drawing-summary-details"), color = colors.secondary, fontSize = smallSize(), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable private fun PaperDrawer(host: CanvasHost, copy: JSONObject, drawer: JSONObject, actions: NewDrawingActions) {
    val failed = host.bootstrap!!.getString("action_failed")
    val chooser = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) host.viewModelScope.launch { importLibraryPaper(host, uri) }
    }
    val tiles = drawer.getJSONArray("tiles").objects()
    val relief = drawer.getJSONObject("relief")
    DrawerSheet("new-drawing-paper-drawer", head = { DialogTitle(copy.getString("paper"), "new-drawing-paper-title") }, footer = {
        val enabled = relief.getBoolean("enabled")
        Row(Modifier.weight(1f).alpha(if (enabled) 1f else .5f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(RowGap)) {
            Text(copy.getString("relief"), Modifier.weight(1f, fill = false).testTag("new-drawing-paper-relief-label"), maxLines = 1, overflow = TextOverflow.Ellipsis)
            DialogNumber(actions, relief.getDouble("value"), relief.getJSONObject("numeric"), copy.getString("relief"), null, copy.getString("relief_unit"),
                "new-drawing-paper-relief", Modifier.width(ReliefWidth), FooterHeight, enabled, trailing = { Spacer(Modifier.width(7.dp)) }) {
                actions.send("paper_relief", "value" to it)
            }
        }
        DialogButton(copy.getString("cancel"), "new-drawing-paper-cancel") { actions.send("close_drawer") }
        DialogButton(copy.getString("use_texture"), "new-drawing-paper-use", accent = true) { actions.usePaper() }
    }) {
        BoxWithConstraints(Modifier.fillMaxWidth().testTag("new-drawing-papers")) {
            val columns = ((maxWidth + TextureGap) / (TextureFrame + TextureGap)).toInt().coerceIn(1, 6)
            Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                (tiles.withIndex() + null).chunked(columns).forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(TextureGap)) {
                        row.forEach { item ->
                            if (item == null) ImportTile(host, copy.getString("import"), Modifier.weight(1f), actions) {
                                try { chooser.launch(arrayOf("image/*")) } catch (e: Exception) { host.reportActionError(e.message ?: failed) }
                            } else TextureTile(host, item.value, item.index, item.value.getBoolean("selected"), Modifier.weight(1f), {}, "none", "new-drawing-paper-tile",
                                use = { actions.usePaper() }) { actions.send("pick_paper", "id" to item.value.paperId()) }
                        }
                        repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
    }
}

@Composable private fun ImportTile(host: CanvasHost, label: String, modifier: Modifier, actions: NewDrawingActions, import: () -> Unit) {
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    DisposableEffect(actions) { onDispose { actions.importFocused = false } }
    HoverTip(label, modifier) {
        Column(Modifier.fillMaxWidth().onFocusChanged { actions.importFocused = it.isFocused }.hoverable(interactions)
            .clickable(interactions, null, role = Role.Button, onClick = import).testTag("new-drawing-paper-import"),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(TextureCaptionGap)) {
            TextureFace(host, null, label, "plus", hovered, false)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun PaperColorDrawer(host: CanvasHost, copy: JSONObject, drawer: JSONObject, actions: NewDrawingActions) {
    val colors = LocalPalette.current
    val form = rememberColorForm(host, null, drawer.getJSONObject("color"), true)
    if (form == null) { LaunchedEffect(Unit) { actions.send("close_drawer") }; return }
    val value = form.view.getJSONObject("value").toString()
    val reported = remember { arrayOf(value) }
    fun report() { if (value != reported[0]) { reported[0] = value; actions.send("paper_color_value", "color" to JSONObject(value)) } }
    LaunchedEffect(value) { report() }
    val transparent = drawer.getBoolean("transparent")
    val palette = drawer.getJSONArray("palette").objects()
    val swatches = rememberDisplayColors(host, palette.map { it.getJSONObject("color") })
    Column(Modifier.fillMaxSize().background(colors.settingsBackground).pointerInput(Unit) {
        awaitEachGesture { awaitFirstDown(requireUnconsumed = false); do { val event = awaitPointerEvent() } while (event.changes.any { it.pressed }) }
    }.testTag("new-drawing-color-drawer")) {
        Box(Modifier.weight(1f)) {
            ColorFormBody(host, form, copy.getString("paper_color"), "new-drawing-color-title", copies = false, single = true,
                transparent = if (transparent) copy.getString("transparent") else null, mode = {
                Row(Modifier.clip(SquircleShape(ControlRadius + 3.dp)).background(fieldColor()).padding(3.dp).semantics { contentDescription = copy.getString("sheet_mode") }
                    .testTag("new-drawing-color-mode"), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                    for ((icon, chosen) in listOf("solid" to false, "transparent" to true)) {
                        Row(Modifier.height(30.dp).clip(ControlShape).background(if (transparent == chosen) colors.headerActive else Color.Transparent)
                            .selectable(transparent == chosen, role = Role.RadioButton) { actions.send("transparent", "transparent" to chosen) }
                            .testTag("new-drawing-color-mode-${if (chosen) "transparent" else "color"}").padding(horizontal = 9.dp),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                            SharedIcon(icon, null, Modifier.size(14.dp))
                            Text(copy.getString(if (chosen) "transparent" else "color"), maxLines = 1)
                        }
                    }
                }
            }) {
                Column(Modifier.padding(top = 8.dp).testTag("new-drawing-paper-colors"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(copy.getString("paper_colors"))
                        Text(drawer.getString("count"), color = colors.secondary)
                    }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        palette.forEachIndexed { index, choice ->
                            PaperSwatch(swatches[index], choice.getString("name"), choice.getBoolean("selected"), index, copy.getString("delete"),
                                { form.act(obj("op" to "color", "color" to choice.getJSONObject("color"))) }) { actions.send("remove_paper_color", "index" to index) }
                        }
                        val canAdd = drawer.getBoolean("can_add")
                        Box(Modifier.size(Swatch).padding(3.dp).clip(ControlShape).background(colors.text.copy(alpha = .06f)).alpha(if (canAdd) 1f else .4f)
                            .clickable(canAdd, role = Role.Button) { report(); actions.send("add_paper_color") }
                            .semantics { contentDescription = copy.getString("add_paper_color") }.testTag("new-drawing-add-paper-color"), contentAlignment = Alignment.Center) {
                            SharedIcon("plus", null, Modifier.size(16.dp))
                        }
                    }
                }
            }
        }
        PickerFooter("new-drawing-color-drawer") {
            Spacer(Modifier.weight(1f))
            DialogButton(copy.getString("cancel"), "new-drawing-color-cancel") { actions.send("close_drawer") }
            DialogButton(copy.getString("use_color"), "new-drawing-color-use", accent = true, enabled = !form.invalidOpen) { report(); actions.send("use_paper_color") }
        }
    }
}

@Composable private fun PaperSwatch(color: Color, name: String, selected: Boolean, index: Int, delete: String, choose: () -> Unit, remove: () -> Unit) {
    val colors = LocalPalette.current
    var menu by remember { mutableStateOf(false) }
    Box {
        HoverTip(name) {
            Box(Modifier.size(Swatch).clip(ControlShape).then(if (selected) Modifier.border(2.dp, colors.accent, ControlShape) else Modifier)
                .pointerInput(Unit) {
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        if (currentEvent.buttons.isSecondaryPressed) { down.consume(); menu = true }
                    }
                }
                .combinedClickable(role = Role.RadioButton, onLongClick = { menu = true }, onClick = choose)
                .semantics { contentDescription = name; this.selected = selected }.testTag("new-drawing-paper-color-$index").padding(3.dp)) {
                PaletteFace(color, Modifier.fillMaxSize(), ControlShape)
            }
        }
        DropdownMenu(menu, { menu = false }) {
            DropdownMenuItem(text = { Text(delete) }, modifier = Modifier.testTag("new-drawing-paper-color-delete"), onClick = { menu = false; remove() })
        }
    }
}
