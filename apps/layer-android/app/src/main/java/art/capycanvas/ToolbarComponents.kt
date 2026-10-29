package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.graphics.ImageBitmap
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.suspendCancellableCoroutine

internal fun toolbarUi(language: String, request: JSONObject) = JSONObject(Native.toolbarUi(request.toString(), language))
private fun formatted(language: String, control: JSONObject, value: Float, units: Boolean = true) = toolbarUi(language, obj("type" to "number",
    "request" to obj("control" to control, "value" to value, "operation" to obj("type" to "format")), "compact" to true, "units" to units))

@Composable internal fun rememberToolbarStamp(host: CanvasHost, query: JSONObject, active: Boolean): Pair<JSONObject, ImageBitmap>? {
    var result by remember { mutableStateOf<Pair<JSONObject, ImageBitmap>?>(null) }
    LaunchedEffect(host, query.toString(), active) {
        result = null
        if (!active) return@LaunchedEffect
        while (isActive) {
            val next = host.awaitQuery(query.copy().put("known_request", result?.first?.optLong("request_id")))
            if (next?.optJSONArray("alpha")?.length()?.let { it > 0 } == true) {
                val image = withContext(Dispatchers.Default) {
                    val alpha = next.getJSONArray("alpha"); val size = next.getInt("size")
                    android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ALPHA_8).apply {
                        copyPixelsFromBuffer(java.nio.ByteBuffer.wrap(ByteArray(alpha.length()) { alpha.getInt(it).toByte() }))
                    }.asImageBitmap()
                }
                result = next to image
            }
            delay(16)
        }
    }
    return result.takeIf { active }
}

@Composable internal fun Modifier.toolbarNumberScrub(control: JSONObject, fill: Float, enabled: Boolean,
    position: (Float) -> Unit, step: (Int) -> Unit, phase: ((String) -> Unit)? = null): Modifier {
    val current by rememberUpdatedState(fill)
    val onPosition by rememberUpdatedState(position)
    val onStep by rememberUpdatedState(step)
    val onPhase by rememberUpdatedState(phase)
    return edgeControl().pointerInput(control.toString(), enabled) {
        if (!enabled) return@pointerInput
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false)
            if (down.type == PointerType.Mouse) return@awaitEachGesture
            val start = current
            var moved = false
            var released = false
            try { while (true) {
                val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                if (change.isConsumed) break
                if (!change.pressed) { released = true; if (moved) change.consume(); break }
                val delta = (down.position.y - change.position.y) / density
                if (!moved && abs(delta) <= viewConfiguration.touchSlop / density) continue
                if (!moved) onPhase?.invoke("down")
                moved = true; change.consume(); onPosition(start + delta / 200f)
            } } finally { if (moved) onPhase?.invoke(if (released) "up" else "cancel") }
        }
    }.pointerInput(enabled) {
        if (!enabled) return@pointerInput
        awaitPointerEventScope { while (true) {
            val event = awaitPointerEvent()
            if (event.type == PointerEventType.Scroll) event.changes.firstOrNull()?.let {
                if (it.scrollDelta.y != 0f) { onStep(if (it.scrollDelta.y < 0f) 1 else -1); it.consume() }
            }
        } }
    }
}

/** One host view for docked/floating toolbars and retained drawers. */
@Composable internal fun ToolbarComponent(host: CanvasHost, panel: JSONObject, tile: JSONObject,
    bounds: JSONObject, presentation: JSONObject, dock: DockInteraction, vertical: Boolean, modifier: Modifier) {
    val model = tile.getJSONObject("component")
    val context = model.getJSONObject("context")
    val control = tile.getJSONObject("control")
    val id = tile.getInt("id")
    val item = obj("kind" to "tile", "panel" to panel.getString("id"), "tile" to id)
    val style = presentation.getString("tile_style")
    val iconSize = presentation.getInt("tile_icon_size")
    val dimensions = remember(style) { toolbarUi(host.languageTag, obj("type" to "style", "style" to style)) }
    val tileWidth = dimensions.array("size").getDouble(0).toFloat()
    val tileHeight = dimensions.array("size").getDouble(1).toFloat()
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    val width = bounds.number("width"); val height = bounds.number("height")
    fun edit(action: JSONObject) { host.dispatch(obj("type" to "toolbar_edit", "context" to context, "action" to action)) }
    key(context.toString()) {
        if (control.getString("kind") != "tool_options") {
            val field = model.objectOrNull("numeric")
            val opacity = control.getString("kind") != "brush_size_slider"
            val spec = field?.getJSONObject("numeric") ?: remember(control.toString()) { toolbarUi(host.languageTag, obj("type" to "slider_spec", "control" to control)) }
            val value = field?.number("value") ?: spec.number("min")
            val label = field?.getString("label") ?: tile.getString("label")
            val setting = field?.getString("id") ?: if (opacity) "strength" else "size"
            val shown = formatted(host.languageTag, spec, value)
            val marks = model.array("bookmarks").objects()
            val sliderStyle = tile.getJSONObject("slider_style")
            val geometry = remember(width, height, vertical, style, sliderStyle.toString()) { JSONObject(Native.toolbarUi(obj("type" to "slider_layout", "width" to width,
                "height" to height, "axis" to if (vertical) "vertical" else "horizontal", "style" to style, "slider_style" to sliderStyle).toString(), host.languageTag)) }
            var preview by remember { mutableStateOf(false) }
            val sample = rememberToolbarStamp(host, obj("type" to "toolbar_stamp", "context" to context), preview)
            val stamp = sample?.first
            val stampImage = sample?.second
            Box(modifier.pointerInput(item.toString()) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    if (currentEvent.buttons.isSecondaryPressed) { down.consume(); dock.context(item) }
                }
            }) {
                Box(Modifier.placed(geometry.getJSONObject("cap"), density).dragSource(dock, item, holdToDrag = true)
                    .clickable(enabled = field != null) { preview = true }.testTag("slider-cap-$id"), contentAlignment = Alignment.Center) {
                    if (geometry.getBoolean("show_value")) {
                        val readout = toolbarUi(host.languageTag, obj("type" to "slider_readout", "style" to style,
                            "axis" to if (vertical) "vertical" else "horizontal", "control" to spec, "value" to value))
                        SliderReadout(readout, if (dimensions.getBoolean("labeled")) model.getString("value_label") else null, field != null, id)
                    }
                }
                ToolbarSlider(shown.number("fill"), marks, opacity, vertical, field != null, label,
                    Modifier.placed(geometry.getJSONObject("track"), density).testTag("component-slider-$id"),
                    contact = { down, moved -> preview = down || !moved }) { fill, snap, travel ->
                    val next = if (snap) org.json.JSONTokener(Native.toolbarUi(obj("type" to "slider_bookmark_value", "control" to control,
                        "values" to JSONArray(marks.map { it.number("value") }), "position" to fill, "travel" to travel).toString(), host.languageTag)).nextValue() as Number
                    else JSONObject(Native.number(obj("control" to spec, "value" to value, "operation" to obj("type" to "position", "position" to fill)).toString(), host.languageTag)).number("value")
                    edit(obj("type" to "set_tool_setting", "id" to setting, "value" to next.toFloat()))
                }
                if (preview && field != null) stamp?.let { brush ->
                    BrushSliderPreview(brush, stampImage ?: return@let, control, style, value, maxOf(width, height), vertical, marks.any { it.getBoolean("selected") },
                        dismiss = { preview = false }, bookmark = { edit(obj("type" to "toggle_slider_bookmark", "control" to control)) })
                }
            }
        } else {
            val options = model.array("options").objects()
            val preferences = control.getJSONObject("style")
            val labeled = dimensions.getBoolean("labeled")
            val measurer = rememberTextMeasurer()
            val textStyle = LocalTextStyle.current
            fun textWidth(text: String) = measurer.measure(text, textStyle).size.width / density
            val measureKey = options.map { option -> when {
                option.has("Gradient") -> option.getJSONObject("Gradient").getJSONObject("gradient").getJSONObject("destination").toString()
                option.has("Range") -> option.getJSONObject("Range").let { listOf(it.getString("id"), it.array("bounds").objects().map { f -> f.getJSONObject("numeric").toString() }) }
                option.has("Numeric") -> option.getJSONObject("Numeric").let { listOf(it.getString("id"), it.getString("label"), it.getJSONObject("numeric").toString()) }
                option.has("Choice") -> option.getJSONObject("Choice").let { listOf(it.getBoolean("segmented"), it.optBoolean("labeled"), it.array("items").length(), it.optInt("columns"), it.optString("beside")) }
                else -> option.getJSONObject("Action").getJSONObject("state").let { listOf(it.getString("label"), it.opt("icon")) }
            } }.toString()
            val control = panelMetrics().height.value
            val sizes = remember(measureKey, preferences.toString(), style, vertical, width, textStyle, density, host.languageTag, control) {
                options.map { toolOptionSize(it, vertical, width, tileWidth, tileHeight, preferences, ::textWidth, control, language = host.languageTag, style = style, density = density) }
            }
            val layoutKey = sizes.toString()
            val layout = remember(width, height, vertical, layoutKey, style) { toolbarUi(host.languageTag, obj("type" to "options_layout", "width" to width, "height" to height,
                "axis" to if (vertical) "vertical" else "horizontal", "sizes" to JSONArray(sizes.map(::JSONArray)),
                "button" to dimensions.array("size"), "gap" to dimensions.number("gap"))) }
            Box(modifier.pointerInput(item.toString()) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = true)
                    if (currentEvent.buttons.isSecondaryPressed) { down.consume(); dock.context(item) }
                }
            }.combinedClickable(onClick = {}, onLongClick = { dock.holdContext(item) })) {
                options.forEachIndexed { index, option ->
                    layout.array("fields").optJSONObject(index)?.let { rect ->
                        Box(Modifier.placed(rect, density), contentAlignment = Alignment.Center) {
                            ToolOptionField(option, rect.number("width"), vertical, style, labeled, preferences, tileWidth,
                                iconSize, ::edit)
                        }
                    }
                }
                val source = dock.tileDrawerSource(panel.getString("id"), id)
                Box(Modifier.placed(layout.getJSONObject("more"), density).dragSource(dock, item, holdToDrag = true)
                    .clip(drawerButtonShape(source?.direction))
                    .testTag("toolbar-more-$id").clickable { host.dispatch(obj("type" to "activate_tile", "panel" to panel.getString("id"), "tile" to id)) }, contentAlignment = Alignment.Center) {
                    SharedIcon("more", "More tool options", Modifier.size(iconSize.dp))
                }
            }
        }
    }
}

internal fun toolOptionSize(option: JSONObject, vertical: Boolean, width: Float, tileWidth: Float, tileHeight: Float,
    preferences: JSONObject, textWidth: (String) -> Float, control: Float, caption: String? = null, language: String = "", style: String = "small", density: Float = 1f): List<Float> {
    val clear = if (vertical) 0f else 2 * pixelAligned(FieldClear, density)
    return when {
    option.has("Gradient") -> listOf(if (vertical) width else 120f + clear, control)
    option.has("Range") -> listOf((if (preferences.getBoolean("sliders")) 280f else 100f) + clear, control)
    option.has("Choice") && option.getJSONObject("Choice").optBoolean("labeled") -> option.getJSONObject("Choice").let {
        val geometry = toolbarUi(language, obj("type" to "choice_style", "labeled" to true, "style" to style))
        listOf(textWidth(it.getString("label")) + 8f + geometry.array("size").getDouble(0).toFloat() * it.array("items").length() + clear, geometry.array("size").getDouble(1).toFloat())
    }
    option.has("Choice") && option.getJSONObject("Choice").optInt("columns") > 0 -> option.getJSONObject("Choice").let {
        val columns = it.getInt("columns")
        listOf(columns * 18f + clear, ((it.array("items").length() + columns - 1) / columns) * 18f)
    }
    option.has("Choice") && option.getJSONObject("Choice").getBoolean("segmented") -> {
        val items = option.getJSONObject("Choice").array("items").objects()
        if (caption != null) listOf(items.sumOf { segmentWidth(it.getString("label"), textWidth, density).toDouble() }.toFloat() + clear, tileHeight)
        else if (vertical) listOf(width, tileHeight * if (width < tileWidth * items.size) items.size else 1)
        else listOf(tileWidth * items.size, control)
    }
    option.has("Action") && option.getJSONObject("Action").optBoolean("checkable") && option.getJSONObject("Action").getJSONObject("state").isNull("icon") ->
        listOf(textWidth(option.getJSONObject("Action").getJSONObject("state").getString("label")) + 24f + clear, tileHeight)
    option.has("Action") && caption != null -> listOf(if (caption.isEmpty()) tileHeight else captionedWidth(caption, textWidth, density) + clear, tileHeight)
    vertical || option.has("Action") -> listOf(tileWidth, tileHeight)
    option.has("Choice") && caption != null -> listOf(menuButtonWidth(option.getJSONObject("Choice").array("items").objects().maxBy { textWidth(it.getString("label")) }.getString("label"),
        textWidth, density, ChoicePadding) + clear, tileHeight)
    option.has("Choice") -> listOf(168f + clear, control)
    else -> {
        val field = option.getJSONObject("Numeric")
        val samples = toolbarUi(language, obj("type" to "numeric_info", "id" to field.getString("id"), "control" to field.getJSONObject("numeric"), "compact" to true, "units" to true)).array("samples")
        val valueWidth = (0 until samples.length()).maxOf { textWidth(samples.getString(it).replace(Regex("[0-9]"), "8")) } + 14f
        listOf((if (preferences.getBoolean("text")) textWidth(field.getString("label")) else 16f) + 4f + valueWidth + (if (preferences.getBoolean("sliders")) 60f else 0f) + clear, control)
    }
    }
}

internal const val FieldClear = 5f
private const val CaptionPadding = 10f
private const val CaptionGap = 6f
private const val ChoicePadding = 8f
internal fun pixelAligned(value: Float, density: Float) = (value * density).roundToInt() / density
private fun captionedWidth(text: String, textWidth: (String) -> Float, density: Float, padding: Float = CaptionPadding) =
    2 * pixelAligned(padding, density) + pixelAligned(16f, density) + pixelAligned(CaptionGap, density) + kotlin.math.ceil(textWidth(text) * density) / density
private fun segmentWidth(text: String, textWidth: (String) -> Float, density: Float) = captionedWidth(text, textWidth, density, ChoicePadding)
internal fun menuButtonWidth(caption: String, textWidth: (String) -> Float, density: Float, padding: Float = CaptionPadding) =
    captionedWidth(caption, textWidth, density, padding) + pixelAligned(CaptionGap, density) + pixelAligned(12f, density)

@Composable internal fun ToolOptionField(option: JSONObject, width: Float, vertical: Boolean, style: String, labeled: Boolean,
    preferences: JSONObject, tileWidth: Float, iconSize: Int, edit: (JSONObject) -> Unit,
    caption: String? = null, prefix: String = "toolbar", accent: Boolean = false,
    choiceMenu: ((String, (JSONObject?) -> Unit) -> Unit)? = null,
    choiceCopy: (suspend (String) -> JSONObject?)? = null) {
    val action = option.optJSONObject("Action")
    val check = action != null && action.optBoolean("checkable") && action.getJSONObject("state").isNull("icon")
    val grid = option.optJSONObject("Choice")?.optInt("columns")?.let { it > 0 } == true
    val control = panelMetrics().height
    val field = when {
        vertical || action != null && !check && caption.isNullOrEmpty() -> Modifier.fillMaxSize()
        grid -> Modifier.fillMaxSize().padding(horizontal = FieldClear.dp)
        else -> Modifier.padding(horizontal = FieldClear.dp).fillMaxWidth().height(control)
    }
    Box(field, contentAlignment = Alignment.Center) { when {
        option.has("Gradient") -> GradientButton(LocalCanvasHost.current,option.getJSONObject("Gradient"),edit)
        option.has("Range") -> option.getJSONObject("Range").let { range ->
            val fields = range.array("bounds").objects()
            RangeControl(fields, range.getString("label"), Modifier.fillMaxWidth(), prefix=prefix, showSlider=preferences.getBoolean("sliders")) { index, value ->
                edit(obj("type" to "set_tool_setting", "id" to fields[index].getString("id"), "value" to value))
            }
        }
        option.has("Numeric") -> ToolbarNumber(option.getJSONObject("Numeric"), vertical, style, labeled, preferences, edit)
        option.has("Choice") -> option.getJSONObject("Choice").let { choice ->
            ToolbarChoice(choice, vertical, labeled, caption == null && width < tileWidth * choice.array("items").length(),
                iconSize, edit, prefix = prefix, height = control.value, captions = caption != null, menu = choiceMenu, menuCopy = choiceCopy, style = style)
        }
        else -> ToolOptionAction(action!!.getJSONObject("state"), action.optBoolean("checkable"), iconSize, caption, accent, prefix) {
            edit(obj("type" to "invoke", "command" to it))
        }
    } }
}

/** A disabled captioned action shows its published reason on tap, hold or hover. */
@Composable internal fun ToolOptionAction(command: JSONObject, checkable: Boolean, iconSize: Int, caption: String?, accent: Boolean,
    prefix: String, focusable: Boolean = caption == null, invoke: (String) -> Unit) {
    val colors = LocalPalette.current
    val id = command.getString("id")
    val enabled = command.getBoolean("enabled")
    if (checkable && command.isNull("icon")) {
        HoverTip(command.getString("tooltip"), Modifier.fillMaxSize()) {
            Row(Modifier.fillMaxSize().testTag("$prefix-action-$id")
                .toggleable(command.getBoolean("selected"), enabled = enabled, role = Role.Checkbox) { invoke(id) },
                verticalAlignment = Alignment.CenterVertically) {
                EditorCheck(command.getBoolean("selected"), command.getString("label"), Modifier.clearAndSetSemantics {}, enabled = enabled) { invoke(id) }
                Text(command.getString("label"), maxLines = 1, softWrap = false)
            }
        }
        return
    }
    var reveal by remember { mutableIntStateOf(0) }
    val reason = command.optString("disabled_reason").takeIf { caption != null && !enabled && !command.isNull("disabled_reason") }
    val explained = reason != null
    val face = @Composable {
        Row(Modifier.fillMaxSize().testTag("$prefix-action-$id").clip(if (caption.isNullOrEmpty()) TileShape else ControlShape)
            .background(when { accent && enabled -> colors.accent; checkable && command.getBoolean("selected") -> colors.active; else -> Color.Transparent })
            .alpha(if (enabled || caption == null) 1f else .4f)
            .then(if (focusable) Modifier else Modifier.focusProperties { canFocus = false })
            .combinedClickable(enabled = enabled || explained, role = Role.Button, onClickLabel = command.getString("label"),
                onLongClick = if (explained) {{ reveal++ }} else null) { if (enabled) invoke(id) else reveal++ }
            .semantics {
                if (!enabled) disabled()
                if (checkable) toggleableState = ToggleableState(command.getBoolean("selected"))
            }
            .padding(horizontal = if (caption.isNullOrEmpty()) 0.dp else CaptionPadding.dp),
            horizontalArrangement = Arrangement.spacedBy(CaptionGap.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
            val tint = if (accent && enabled) colors.accentForeground else colors.text
            command.optString("icon").takeIf { it.isNotEmpty() && it != "null" }?.let {
                SharedIcon(it, command.getString("label"), Modifier.size(if (caption == null) iconSize.dp else 16.dp), tint = tint)
            }
            if (!caption.isNullOrEmpty()) Text(caption, color = tint, maxLines = 1, softWrap = false)
        }
    }
    if (caption == null) face()
    else HoverTip(reason ?: command.getString("tooltip"), Modifier.fillMaxSize(), reveal = reveal, content = face)
}

/** An item that opens its core menu without taking window focus: a tile with
 * the tool-group corner when [iconOnly], else its caption and an arrow. A
 * disabled primary command disables it and shows its reason, like an action. */
@Composable internal fun ToolOptionMenu(id: String, icon: String, label: String, command: JSONObject?, prefix: String,
    iconOnly: Boolean = false, copy: (suspend () -> JSONObject?)? = null,
    load: ((JSONObject?) -> Unit) -> Unit) {
    val button = remember { WindowlessMenuButton() }
    SideEffect { button.copy = copy }
    val enabled = command?.getBoolean("enabled") ?: true
    val reason = command?.takeIf { !enabled && !it.isNull("disabled_reason") }?.getString("disabled_reason")
    var reveal by remember { mutableIntStateOf(0) }
    HoverTip(reason ?: label, Modifier.fillMaxSize(), reveal = reveal) {
        val face = Modifier.fillMaxSize().testTag("$prefix-menu-$id").clip(if (iconOnly) TileShape else ControlShape).alpha(if (enabled) 1f else .4f)
            .focusProperties { canFocus = false }
            .then(if (enabled) Modifier.opensWindowlessMenu(button, label, load)
                else Modifier.combinedClickable(enabled = reason != null, role = Role.Button, onClickLabel = label,
                    onLongClick = { reveal++ }) { reveal++ }.semantics { disabled() })
        if (iconOnly) Box(face, contentAlignment = Alignment.Center) {
            SharedIcon(icon, label, Modifier.size(16.dp))
            ToolGroupIndicator(Modifier.align(Alignment.BottomEnd))
        } else Row(face.padding(horizontal = CaptionPadding.dp),
            horizontalArrangement = Arrangement.spacedBy(CaptionGap.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
            SharedIcon(icon, null, Modifier.size(16.dp))
            Text(label, maxLines = 1, softWrap = false)
            SharedIcon("chevron-down", null, Modifier.size(12.dp))
        }
        WindowlessMenuHost(LocalCanvasHost.current, button)
    }
}

@Composable private fun ToolbarNumber(field: JSONObject, vertical: Boolean, style: String,
    labeled: Boolean, preferences: JSONObject, edit: (JSONObject) -> Unit) {
    val host = LocalCanvasHost.current
    val id = field.getString("id"); val label = field.getString("label"); val control = field.getJSONObject("numeric"); val value = field.number("value")
    val info = remember(id, control.toString(), host.languageTag) { toolbarUi(host.languageTag, obj("type" to "numeric_info", "id" to id, "control" to control, "compact" to true, "units" to true)) }
    val change: (Float) -> Unit = { edit(obj("type" to "set_tool_setting", "id" to id, "value" to it)) }
    val reset = { edit(obj("type" to "reset_tool_setting", "id" to id)) }
    val phase: ((String) -> Unit)? = if (!field.optBoolean("preview")) null else {
        { phase -> edit(obj("type" to "tool_setting_contact", "id" to id, "phase" to phase)) }
    }
    val shown = formatted(host.languageTag, control, value, style != "small")
    var open by remember { mutableStateOf(false) }
    if (vertical) Box(Modifier.fillMaxSize().testTag("toolbar-setting-$id")) {
        val face = Modifier.fillMaxSize().clip(ControlShape).toolbarNumberScrub(control, shown.number("fill"), true,
            { change(JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "position", "position" to it)).toString(), host.languageTag)).number("value")) },
            { change(JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "step", "steps" to it)).toString(), host.languageTag)).number("value")) }, phase)
            .clickable { open = true }.padding(horizontal = if (labeled) 8.dp else 2.dp, vertical = if (style == "small") 1.dp else 3.dp)
        if (labeled) Row(face, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.Center) {
                Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
                ToolbarFaceValue(shown.getString("text"), formatted(host.languageTag, control, value, false).getString("text"), false)
            }
        } else Column(face, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
            ToolbarFaceValue(shown.getString("text"), formatted(host.languageTag, control, value, false).getString("text"), style == "small")
        }
        var typing by remember { mutableStateOf(false) }
        LaunchedEffect(open) { if (!open) typing = false }
        BackHandler(open && !typing) { open = false }
        PopupOwner(open)
        DropdownMenu(open, { open = false }, Modifier.testTag("toolbar-number-menu"),
            properties = if (typing) PopupProperties(focusable = true) else WindowlessMenu) {
            Box(Modifier.width(240.dp).padding(10.dp)) {
                NumericSetting(label, value, control, onTyping = { if (it) typing = true }, onReset = reset, onEditPhase = phase, onChange = change)
            }
        }
    } else Row(Modifier.fillMaxSize().testTag("toolbar-setting-$id"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(Modifier.combinedClickable(onClick = {}, onDoubleClick = reset)) {
            if (preferences.getBoolean("text")) Text(label, maxLines = 1)
            else SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
        }
        NumericSetting(label, value, control, Modifier.weight(1f), id = "toolbar-$id", inline = true, toolbar = true,
            showSlider = preferences.getBoolean("sliders"), onEditPhase = phase, onChange = change)
    }
}

/** Keep app typography; omit units before shrinking a four-digit small value. */
@Composable private fun ToolbarFaceValue(text: String, bare: String, small: Boolean) {
    val measurer = rememberTextMeasurer()
    val style = LocalTextStyle.current
    val density = LocalDensity.current
    BoxWithConstraints {
        val available = with(density) { maxWidth.toPx() }
        val shown = if (measurer.measure(text, style).size.width <= available) text else bare
        Text(shown, maxLines = 1, softWrap = false,
            fontSize = style.fontSize * if (small && shown.length >= 4) .9f else 1f)
    }
}

@Composable internal fun ToolbarChoice(choice: JSONObject, vertical: Boolean, labeled: Boolean, stacked: Boolean,
    iconSize: Int, edit: (JSONObject) -> Unit, prefix: String = "toolbar", height: Float = panelMetrics().height.value, captions: Boolean = false,
    menu: ((String, (JSONObject?) -> Unit) -> Unit)? = null,
    menuCopy: (suspend (String) -> JSONObject?)? = null, style: String = "small") {
    if (choice.optBoolean("labeled")) {
        val host = LocalCanvasHost.current
        val geometry = remember(style, host.languageTag) { toolbarUi(host.languageTag, obj("type" to "choice_style", "labeled" to true, "style" to style)) }
        val size = geometry.array("size")
        val metrics=panelMetrics()
        val choiceHeight=if(prefix=="tool")metrics.height else size.getDouble(1).toFloat().dp
        val choiceWidth=if(prefix=="tool")metrics.height else size.getDouble(0).toFloat().dp
        Row(Modifier.fillMaxWidth().height(choiceHeight), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(if(prefix=="tool")metrics.spacing else 8.dp)) {
            Text(choice.getString("label"), Modifier.weight(1f), maxLines = 2, softWrap = true)
            Box(Modifier.width(choiceWidth * choice.array("items").length())) {
                ToolbarChoice(JSONObject(choice.toString()).put("labeled", false), false, false, false, if(prefix=="tool")metrics.icon.value.toInt() else geometry.getInt("icon"), edit,
                    prefix, choiceHeight.value, menu = menu, menuCopy = menuCopy, style = style)
            }
        }
        return
    }
    val colors = LocalPalette.current
    val items = choice.array("items").objects()
    val id = choice.getString("id")
    val columns = choice.optInt("columns")
    if (columns > 0) {
        Column(Modifier.width((columns * 18).dp).selectableGroup().testTag("$prefix-segments-$id")) {
            items.chunked(columns).forEachIndexed { row, cells ->
                Row {
                    cells.forEachIndexed { column, item ->
                        val selected = item.getBoolean("selected")
                        HoverTip(item.nullableString("disabled_reason") ?: item.getString("label"), Modifier.size(18.dp)) {
                            Box(Modifier.fillMaxSize().testTag("$prefix-segment-$id-${row * columns + column}")
                                .background(if (selected) colors.active else Color.Transparent)
                                .selectable(selected, role = Role.RadioButton) { edit(item.getJSONObject("action")) }, contentAlignment = Alignment.Center) {
                                SharedIcon(item.getString("icon"), item.getString("label"), Modifier.size(6.dp).alpha(if (selected) 1f else .45f))
                            }
                        }
                    }
                }
            }
        }
        return
    }
    if (choice.getBoolean("segmented")) {
        val measurer = rememberTextMeasurer()
        val textStyle = LocalTextStyle.current
        val density = LocalDensity.current.density
        fun weight(item: JSONObject) = if (captions) segmentWidth(item.getString("label"), { measurer.measure(it, textStyle).size.width / density }, density) else 1f
        val segment: @Composable (JSONObject, Int, Modifier) -> Unit = { item, index, modifier ->
            HoverTip(item.nullableString("disabled_reason") ?: item.getString("label"), modifier) {
            Row(Modifier.fillMaxSize().testTag(if(prefix=="tool" && id=="selection-mode") "tool-action-${item.getJSONObject("action").getString("command")}" else "$prefix-segment-$id-$index").background(if (item.getBoolean("selected")) colors.active else colors.input)
                .then(if (captions) Modifier.focusProperties { canFocus = false } else Modifier)
                .selectable(item.getBoolean("selected"), role = Role.RadioButton) { edit(item.getJSONObject("action")) },
                horizontalArrangement = Arrangement.spacedBy(CaptionGap.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
                SharedIcon(item.getString("icon"), item.getString("label"), Modifier.size(if(prefix=="tool")panelMetrics().icon else if(vertical)iconSize.dp else 16.dp))
                if (captions) Text(item.getString("label"), maxLines = 1, softWrap = false)
            }
            }
        }
        val modifier = (if (vertical) Modifier.fillMaxSize().clip(TileShape) else Modifier.fillMaxWidth().height(height.dp).clip(ControlShape))
            .selectableGroup().testTag(if(prefix=="tool" && id=="selection-mode") "selection-mode-row" else "$prefix-segments-$id")
        if (vertical && stacked) Column(modifier) { items.forEachIndexed { i, item -> segment(item, i, Modifier.fillMaxWidth().weight(1f)) } }
        else Row(modifier) { items.forEachIndexed { i, item -> segment(item, i, Modifier.fillMaxHeight().weight(weight(item))) } }
        return
    }
    var open by remember { mutableStateOf(false) }
    val button = remember { WindowlessMenuButton() }
    SideEffect { button.copy = if (menuCopy == null) null else suspend { menuCopy(id) } }
    val selected = items.firstOrNull { it.getBoolean("selected") } ?: items.first()
    Box(Modifier.fillMaxSize().testTag("$prefix-choice-$id"), contentAlignment = Alignment.Center) {
        Row(Modifier.fillMaxWidth().then(if (vertical) Modifier.fillMaxHeight() else Modifier.height(height.dp)).clip(ControlShape)
            .background(if (vertical) Color.Transparent else colors.input)
            .then(if (menu == null) Modifier.clickable { open = true }
                else Modifier.focusProperties { canFocus = false }.opensWindowlessMenu(button, choice.getString("label")) { menu(id, it) })
            .padding(horizontal = if (vertical && !labeled) 2.dp else ChoicePadding.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = if (vertical && !labeled) Arrangement.Center else Arrangement.spacedBy(CaptionGap.dp)) {
            SharedIcon(selected.getString("icon"), choice.getString("label"), Modifier.size(16.dp))
            if (!vertical || labeled) Text(selected.getString("label"), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!vertical) SharedIcon("chevron-down", null, Modifier.size(12.dp))
        }
        if (menu != null) WindowlessMenuHost(LocalCanvasHost.current, button)
        else {
            WindowlessPopup(open) { open = false }
            DropdownMenu(open, { open = false }, properties = WindowlessMenu) { items.forEach { item ->
                DropdownMenuItem(text = { Text(item.getString("label")) }, leadingIcon = { SharedIcon(item.getString("icon"), null, Modifier.size(16.dp)) },
                    onClick = { open = false; edit(item.getJSONObject("action")) })
            } }
        }
    }
}

@Composable private fun SliderReadout(readout: JSONObject, title: String?, enabled: Boolean, id: Int) {
    val metrics = readout.getJSONObject("metrics")
    val gap = metrics.number("gap").dp
    val stacked = metrics.getBoolean("units_below")
    val number = if (enabled) readout.getString("value") else ""
    val unit = if (enabled) readout.getString("unit") else ""
    val base = LocalTextStyle.current.copy(fontWeight = FontWeight.Normal, fontFeatureSettings = "tnum")
    val titleStyle = base.copy(fontSize = base.fontSize * .75f, lineHeight = base.lineHeight * .75f)
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    BoxWithConstraints(Modifier.fillMaxSize().padding(metrics.number("inset").dp).alpha(if (enabled) 1f else .4f), contentAlignment = Alignment.Center) {
        val availableWidth = with(density) { maxWidth.toPx() }
        val titleHeight = title?.let { measurer.measure(it, titleStyle).size.height + with(density) { gap.toPx() } } ?: 0f
        val availableHeight = with(density) { maxHeight.toPx() } - titleHeight
        val space = with(density) { gap.toPx() }
        fun measure(scale: Float): Size {
            val font = base.copy(fontSize = base.fontSize * scale, lineHeight = base.lineHeight * scale)
            val sizes = listOf(number, unit).map { measurer.measure(it, font, softWrap = false, maxLines = 1).size }
            return if (stacked) Size(sizes.maxOf { it.width }.toFloat(), sizes.sumOf { it.height }.toFloat() + space)
                else Size(sizes.sumOf { it.width }.toFloat() + space, sizes.maxOf { it.height }.toFloat())
        }
        val normal = measure(1f)
        var scale = minOf(1f, availableWidth.coerceAtLeast(1f) / normal.width.coerceAtLeast(1f), availableHeight.coerceAtLeast(1f) / normal.height.coerceAtLeast(1f))
        for (pass in 0..3) {
            val size = measure(scale)
            val fit = minOf(1f, availableWidth.coerceAtLeast(1f) / size.width.coerceAtLeast(1f), availableHeight.coerceAtLeast(1f) / size.height.coerceAtLeast(1f))
            if (fit >= 1f) break
            scale *= fit
        }
        val font = base.copy(fontSize = base.fontSize * scale, lineHeight = base.lineHeight * scale)
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(gap)) {
            if (title != null) Text(title, style = titleStyle, color = LocalPalette.current.text.copy(alpha = .65f), maxLines = 2, overflow = TextOverflow.Ellipsis)
            @Composable fun Values() {
                Text(number, Modifier.testTag("slider-value-$id"), style = font, maxLines = 1, softWrap = false)
                Text(unit, Modifier.testTag("slider-unit-$id"), style = font, color = LocalPalette.current.text.copy(alpha = .65f), maxLines = 1, softWrap = false)
            }
            if (stacked) Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(gap)) { Values() }
            else Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(gap)) { Values() }
        }
    }
}

@Composable private fun ToolbarSlider(fill: Float, marks: List<JSONObject>, opacity: Boolean, vertical: Boolean, enabled: Boolean,
    label: String, modifier: Modifier, contact: (Boolean, Boolean) -> Unit, change: (Float, Boolean, Float) -> Unit) {
    val onChange by rememberUpdatedState(change)
    val onContact by rememberUpdatedState(contact)
    val colors = LocalPalette.current
    Canvas(modifier.padding(if (vertical) PaddingValues(horizontal = 5.dp, vertical = 2.dp) else PaddingValues(horizontal = 2.dp, vertical = 5.dp))
        .alpha(if (enabled) 1f else .4f).semantics {
            contentDescription = label; progressBarRangeInfo = ProgressBarRangeInfo(fill, 0f..1f)
            if (enabled) setProgress { onChange(it, false, 0f); onContact(false, false); true } else disabled()
        }.edgeControl().pointerInput(vertical, enabled) {
            if (!enabled) return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(); down.consume()
                var moved = false; var finished = false
                fun pick(p: Offset, snap: Boolean) {
                    val half = 6.dp.toPx(); val length = (if (vertical) size.height else size.width) - 2 * half
                    if (length > 0) onChange((if (vertical) 1f - (p.y-half)/length else (p.x-half)/length).coerceIn(0f, 1f), snap, length / density)
                }
                onContact(true, false); pick(down.position, true)
                try {
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        if (!c.pressed) { finished = true; c.consume(); break }
                        if (!moved && (c.position - down.position).getDistance() < 3.dp.toPx()) continue
                        moved = true; c.consume(); pick(c.position, false)
                    }
                } finally { onContact(false, moved || !finished) }

            }
        }) {
        val unit = 1.dp.toPx()
        val extent = if (vertical) size.height else size.width
        val length = extent - 12 * unit
        if (length >= 4 * unit) withTransform({
            if (vertical) { translate(size.width / 2, size.height - 6 * unit); rotate(-90f, Offset.Zero) }
            else translate(6 * unit, size.height / 2)
        }) {
            val wide = 8 * unit; val narrow = if (opacity) wide else 2.5f * unit
            val track = Path().apply {
                moveTo(0f, -narrow); lineTo(length - 3 * unit, -wide)
                cubicTo(length + unit, -wide, length + unit, wide, length - 3 * unit, wide)
                lineTo(0f, narrow); cubicTo(-3 * unit, narrow, -3 * unit, -narrow, 0f, -narrow); close()
            }
            clipPath(track) {
                val bounds = Rect(-3 * unit, -wide, length + unit, wide)
                if (opacity) {
                    drawRect(Brush.horizontalGradient(listOf(colors.text.copy(alpha = .06f), colors.text.copy(alpha = .28f)), 0f, length),
                        bounds.topLeft, bounds.size)
                } else drawRect(colors.text.copy(alpha = .22f), bounds.topLeft, bounds.size)
            }
        }
        val along = 12 * unit; val across = 28 * unit
        val thumb = if (vertical) Rect(Offset((size.width - across) / 2, (size.height - along) * (1f - fill)), Size(across, along))
            else Rect(Offset((size.width - along) * fill, (size.height - across) / 2), Size(along, across))
        drawPath(Path().apply { addSquircle(thumb, 6 * unit, 6 * unit, 6 * unit, 6 * unit) }, colors.thumb)
        val edge = thumb.deflate(unit / 2); val corner = 5.5f * unit
        drawPath(Path().apply { addSquircle(edge, corner, corner, corner, corner) }, colors.text.copy(alpha = .6f), style = Stroke(unit))
        for (mark in marks) {
            val position = if (mark.getBoolean("selected")) fill else mark.number("fill")
            val center = if (vertical) Offset(size.width / 2, along / 2 + (size.height - along) * (1f - position))
                else Offset(along / 2 + (size.width - along) * position, size.height / 2)
            val delta = if (vertical) Offset(7 * unit, 0f) else Offset(0f, 7 * unit)
            drawLine(if (mark.getBoolean("selected")) colors.panel else colors.text, center - delta, center + delta, 2 * unit)
        }
    }
}


@Composable private fun BrushSliderPreview(stamp: JSONObject, bitmap: ImageBitmap, control: JSONObject, style: String, value: Float, length: Float,
    vertical: Boolean, selected: Boolean, dismiss: () -> Unit, bookmark: () -> Unit) {
    val host = LocalCanvasHost.current
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    val layout = toolbarUi(host.languageTag, obj("type" to "slider_preview", "control" to control, "style" to style, "value" to value, "length" to length, "extent" to stamp.number("extent")))
    val provider = remember(vertical, density) { object : PopupPositionProvider {
        override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
            val gap = (8*density).toInt(); val p = popupContentSize; val a = anchorBounds
            val x = if (vertical) { if (a.right+p.width+gap <= windowSize.width) a.right+gap else a.left-p.width-gap } else a.left
            val y = if (vertical) a.top+(a.height-p.height)/2 else { if (a.bottom+p.height+gap <= windowSize.height) a.bottom+gap else a.top-p.height-gap }
            return IntOffset(x.coerceIn(gap, maxOf(gap,windowSize.width-p.width-gap)), y.coerceIn(gap,maxOf(gap,windowSize.height-p.height-gap)))
        }
    } }
    Popup(provider, onDismissRequest = dismiss, properties = PopupProperties(focusable = false)) {
        Surface(Modifier.size(layout.number("side").dp).testTag("brush-slider-preview"), shape = SquircleShape(layout.number("radius").dp),
            color = colors.panel, shadowElevation = 6.dp) {
            Box {
                Canvas(Modifier.fillMaxSize()) {
                    val b = layout.getJSONObject("stamp")
                    val viewport = layout.getJSONObject("viewport")
                    clipRect(viewport.number("x")*density, viewport.number("y")*density,
                        (viewport.number("x")+viewport.number("width"))*density, (viewport.number("y")+viewport.number("height"))*density) {
                        drawImage(bitmap, dstOffset = IntOffset((b.number("x")*density).toInt(),(b.number("y")*density).toInt()),
                            dstSize = IntSize(maxOf(1,(b.number("width")*density).toInt()),maxOf(1,(b.number("height")*density).toInt())),
                            alpha = layout.number("opacity"), colorFilter = ColorFilter.tint(colors.text))
                    }
                    val fade = layout.number("header_fade")*density
                    if (fade > 0f) drawRect(Brush.verticalGradient(0f to colors.panel.copy(alpha=layout.number("header_fade_opacity")),
                        1f to colors.panel.copy(alpha=0f), endY=fade), size=Size(size.width,fade))
                }
                Box(Modifier.placed(layout.getJSONObject("caption"), density), contentAlignment = Alignment.CenterStart) {
                    Text(layout.getString("text"), maxLines = 1, softWrap = false)
                }
                Box(Modifier.placed(layout.getJSONObject("bookmark"), density).clip(TileShape).clickable(onClick=bookmark).testTag("slider-bookmark"), contentAlignment=Alignment.Center) {
                    SharedIcon(if(selected) "minus" else "plus", if(selected) "Remove bookmark" else "Bookmark this value", Modifier.size(layout.number("icon").dp))
                }
            }
        }
    }
}
