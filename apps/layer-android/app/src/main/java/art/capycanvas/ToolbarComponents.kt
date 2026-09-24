package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.*
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs

private fun toolbarUi(request: JSONObject) = JSONObject(Native.toolbarUi(request.toString()))
private fun formatted(control: JSONObject, value: Float, units: Boolean = true) = toolbarUi(obj("type" to "number",
    "request" to obj("control" to control, "value" to value, "operation" to obj("type" to "format")), "compact" to true, "units" to units))

/** Native contacts use the same normalized scale as the Rust numeric control. */
@Composable internal fun Modifier.toolbarNumberScrub(control: JSONObject, fill: Float, enabled: Boolean,
    position: (Float) -> Unit, step: (Int) -> Unit): Modifier {
    val current by rememberUpdatedState(fill)
    val onPosition by rememberUpdatedState(position)
    val onStep by rememberUpdatedState(step)
    return pointerInput(control.toString(), enabled) {
        if (!enabled) return@pointerInput
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false)
            if (down.type == PointerType.Mouse) return@awaitEachGesture
            val start = current
            var moved = false
            while (true) {
                val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                if (change.isConsumed) break
                if (!change.pressed) { if (moved) change.consume(); break }
                val delta = (down.position.y - change.position.y) / density
                if (!moved && abs(delta) <= viewConfiguration.touchSlop / density) continue
                moved = true; change.consume(); onPosition(start + delta / 200f)
            }
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

/** One host view for docked/floating toolbars, Zen strips and retained drawers. */
@Composable internal fun ToolbarComponent(host: CanvasHost, panel: JSONObject, tile: JSONObject,
    bounds: JSONObject, dock: DockInteraction, vertical: Boolean, modifier: Modifier) {
    val model = tile.getJSONObject("component")
    val context = model.getJSONObject("context")
    val control = tile.getJSONObject("control")
    val id = tile.getInt("id")
    val item = obj("kind" to "tile", "panel" to panel.getString("id"), "tile" to id)
    val style = panel.getString("tile_style")
    val dimensions = remember(style) { toolbarUi(obj("type" to "style", "style" to style)) }
    val tileWidth = dimensions.array("size").getDouble(0).toFloat()
    val tileHeight = dimensions.array("size").getDouble(1).toFloat()
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    val width = bounds.number("width"); val height = bounds.number("height")
    fun edit(action: JSONObject) { host.dispatch(obj("type" to "toolbar_edit", "context" to context, "action" to action)) }
    key(context.toString()) {
        if (control.getString("kind") != "tool_options") {
            val field = model.objectOrNull("numeric")
            val opacity = control.getString("kind") == "brush_opacity_slider"
            val spec = field?.getJSONObject("numeric") ?: remember(control.toString()) { toolbarUi(obj("type" to "slider_spec", "control" to control)) }
            val value = field?.number("value") ?: spec.number("min")
            val label = field?.getString("label") ?: tile.getString("label")
            val setting = field?.getString("id") ?: if (opacity) "opacity" else "size"
            val shown = formatted(spec, value)
            val marks = model.array("bookmarks").objects()
            val geometry = remember(width, height, vertical) { JSONArray(Native.toolbarUi(obj("type" to "slider_layout", "width" to width,
                "height" to height, "axis" to if (vertical) "vertical" else "horizontal").toString())) }
            var preview by remember { mutableStateOf(false) }
            var stamp by remember { mutableStateOf<JSONObject?>(null) }
            DisposableEffect(preview) {
                var active = true
                if (preview) host.query(obj("type" to "toolbar_stamp", "context" to context)) { if (active) stamp = it as? JSONObject }
                else stamp = null
                onDispose { active = false }
            }
            Box(modifier) {
                Box(Modifier.placed(geometry.getJSONObject(0), density).dragSource(dock, item, holdToDrag = true)
                    .clickable(enabled = field != null) { preview = true }.testTag("slider-cap-$id"))
                ToolbarSlider(shown.number("fill"), marks, opacity, vertical, field != null, label,
                    Modifier.placed(geometry.getJSONObject(1), density).testTag("component-slider-$id"),
                    contact = { down, moved -> preview = down || !moved }) { fill, snap, travel ->
                    val next = if (snap) org.json.JSONTokener(Native.toolbarUi(obj("type" to "slider_bookmark_value", "control" to control,
                        "values" to JSONArray(marks.map { it.number("value") }), "position" to fill, "travel" to travel).toString())).nextValue() as Number
                    else JSONObject(Native.number(obj("control" to spec, "value" to value, "operation" to obj("type" to "position", "position" to fill)).toString())).number("value")
                    edit(obj("type" to "set_tool_setting", "id" to setting, "value" to next.toFloat()))
                }
                if (preview && field != null) stamp?.let { brush ->
                    BrushSliderPreview(brush, control, value, maxOf(width, height), vertical, marks.any { it.getBoolean("selected") },
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
                option.has("Numeric") -> option.getJSONObject("Numeric").let { listOf(it.getString("id"), it.getString("label"), it.getJSONObject("numeric").toString()) }
                option.has("Choice") -> option.getJSONObject("Choice").let { listOf(it.getBoolean("segmented"), it.array("items").length()) }
                else -> "action"
            } }.toString()
            val sizes = remember(measureKey, preferences.toString(), style, vertical, width, textStyle, density) {
                options.map { option ->
                    when {
                        option.has("Choice") && option.getJSONObject("Choice").getBoolean("segmented") -> {
                            val count = option.getJSONObject("Choice").array("items").length()
                            if (vertical) listOf(width, tileHeight * if (width < tileWidth * count) count else 1)
                            else listOf(tileWidth * count, tileHeight)
                        }
                        vertical || option.has("Action") -> listOf(tileWidth, tileHeight)
                        option.has("Choice") -> listOf(168f, 24f)
                        else -> {
                            val field = option.getJSONObject("Numeric")
                            val samples = toolbarUi(obj("type" to "numeric_info", "id" to field.getString("id"), "control" to field.getJSONObject("numeric"), "compact" to true, "units" to true)).array("samples")
                            val valueWidth = (0 until samples.length()).maxOf { textWidth(samples.getString(it).replace(Regex("[0-9]"), "8")) } + 14f
                            listOf((if (preferences.getBoolean("text")) textWidth(field.getString("label")) else 16f) + 4f + valueWidth + if (preferences.getBoolean("sliders")) 60f else 0f, 24f)
                        }
                    }
                }
            }
            val layoutKey = sizes.toString()
            val layout = remember(width, height, vertical, layoutKey, style) { toolbarUi(obj("type" to "options_layout", "width" to width, "height" to height,
                "axis" to if (vertical) "vertical" else "horizontal", "sizes" to JSONArray(sizes.map(::JSONArray)),
                "button" to dimensions.array("size"), "gap" to if (vertical) 2f else 10f)) }
            Box(modifier.pointerInput(item.toString()) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = true)
                    if (currentEvent.buttons.isSecondaryPressed) { down.consume(); dock.context(item) }
                }
            }.combinedClickable(onClick = {}, onLongClick = { dock.holdContext(item) })) {
                options.forEachIndexed { index, option ->
                    layout.array("fields").optJSONObject(index)?.let { rect ->
                        Box(Modifier.placed(rect, density), contentAlignment = Alignment.Center) {
                            when {
                                option.has("Numeric") -> ToolbarNumber(option.getJSONObject("Numeric"), vertical, style, labeled, preferences, ::edit)
                                option.has("Choice") -> ToolbarChoice(option.getJSONObject("Choice"), vertical, labeled, rect.number("width") < tileWidth * option.getJSONObject("Choice").array("items").length(), panel.getInt("tile_icon_size"), ::edit)
                                else -> {
                                    val action = option.getJSONObject("Action"); val command = action.getJSONObject("state")
                                    Box(Modifier.fillMaxSize().testTag("toolbar-action-${command.getString("id")}").clip(TileShape)
                                        .background(if (command.getBoolean("selected")) colors.active else Color.Transparent)
                                        .clickable(enabled = command.getBoolean("enabled")) { edit(obj("type" to "invoke", "command" to command.getString("id"))) }, contentAlignment = Alignment.Center) {
                                        SharedIcon(command.getString("icon"), command.getString("label"), Modifier.size(panel.getInt("tile_icon_size").dp))
                                    }
                                }
                            }
                        }
                    }
                }
                val drawerAnchor = host.snapshot?.getJSONObject("state")?.getJSONObject("customization")?.objectOrNull("drawer")?.getJSONObject("anchor")
                val opensDrawer = drawerAnchor?.optString("panel") == panel.getString("id") && drawerAnchor.optInt("tile") == id
                Box(Modifier.placed(layout.getJSONObject("more"), density).contextAnchor(dock, item).dragSource(dock, item, holdToDrag = true)
                    .clip(drawerButtonShape(if (opensDrawer) dock.drawerSources["tool"]?.direction else null))
                    .background(if (opensDrawer) colors.panel else Color.Transparent)
                    .testTag("toolbar-more-$id").clickable { host.dispatch(obj("type" to "activate_tile", "panel" to panel.getString("id"), "tile" to id)) }, contentAlignment = Alignment.Center) {
                    SharedIcon("more", "More tool options", Modifier.size(panel.getInt("tile_icon_size").dp))
                }
            }
        }
    }
}

@Composable private fun ToolbarNumber(field: JSONObject, vertical: Boolean, style: String,
    labeled: Boolean, preferences: JSONObject, edit: (JSONObject) -> Unit) {
    val id = field.getString("id"); val label = field.getString("label"); val control = field.getJSONObject("numeric"); val value = field.number("value")
    val info = remember(id, control.toString()) { toolbarUi(obj("type" to "numeric_info", "id" to id, "control" to control, "compact" to true, "units" to true)) }
    val change: (Float) -> Unit = { edit(obj("type" to "set_tool_setting", "id" to id, "value" to it)) }
    val reset = { edit(obj("type" to "reset_tool_setting", "id" to id)) }
    val shown = formatted(control, value, style != "small")
    var open by remember { mutableStateOf(false) }
    if (vertical) Box(Modifier.fillMaxSize().testTag("toolbar-setting-$id")) {
        val face = Modifier.fillMaxSize().clip(ControlShape).toolbarNumberScrub(control, shown.number("fill"), true,
            { change(JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "position", "position" to it)).toString())).number("value")) },
            { change(JSONObject(Native.number(obj("control" to control, "value" to value, "operation" to obj("type" to "step", "steps" to it)).toString())).number("value")) })
            .clickable { open = true }.padding(horizontal = if (labeled) 8.dp else 2.dp, vertical = if (style == "small") 1.dp else 3.dp)
        if (labeled) Row(face, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.Center) {
                Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis)
                ToolbarFaceValue(shown.getString("text"), formatted(control, value, false).getString("text"), false)
            }
        } else Column(face, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
            ToolbarFaceValue(shown.getString("text"), formatted(control, value, false).getString("text"), style == "small")
        }
        DropdownMenu(open, { open = false }) {
            Box(Modifier.width(240.dp).padding(10.dp)) { NumericSetting(label, value, control, onChange = change) }
        }
    } else Row(Modifier.fillMaxSize().testTag("toolbar-setting-$id"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(Modifier.combinedClickable(onClick = {}, onDoubleClick = reset)) {
            if (preferences.getBoolean("text")) Text(label, maxLines = 1)
            else SharedIcon(info.getString("icon"), label, Modifier.size(16.dp))
        }
        NumericSetting(label, value, control, Modifier.weight(1f), id = "toolbar-$id", inline = true, toolbar = true,
            showSlider = preferences.getBoolean("sliders"), onChange = change)
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

@Composable private fun ToolbarChoice(choice: JSONObject, vertical: Boolean, labeled: Boolean, stacked: Boolean,
    iconSize: Int, edit: (JSONObject) -> Unit) {
    val colors = LocalPalette.current
    val items = choice.array("items").objects()
    val id = choice.getString("id")
    if (choice.getBoolean("segmented")) {
        val segment: @Composable (JSONObject, Int, Modifier) -> Unit = { item, index, modifier ->
            Box(modifier.testTag("toolbar-segment-$id-$index").background(if (item.getBoolean("selected")) colors.active else colors.input)
                .selectable(item.getBoolean("selected"), role = Role.RadioButton) { edit(item.getJSONObject("action")) }, contentAlignment = Alignment.Center) {
                SharedIcon(item.getString("icon"), item.getString("label"), Modifier.size(iconSize.dp))
            }
        }
        val modifier = Modifier.fillMaxSize().clip(TileShape).selectableGroup().testTag("toolbar-segments-$id")
        if (vertical && stacked) Column(modifier) { items.forEachIndexed { i, item -> segment(item, i, Modifier.fillMaxWidth().weight(1f)) } }
        else Row(modifier) { items.forEachIndexed { i, item -> segment(item, i, Modifier.fillMaxHeight().weight(1f)) } }
        return
    }
    var open by remember { mutableStateOf(false) }
    val selected = items.firstOrNull { it.getBoolean("selected") } ?: items.first()
    Box(Modifier.fillMaxSize().testTag("toolbar-choice-$id"), contentAlignment = Alignment.Center) {
        Row(Modifier.fillMaxWidth().then(if (vertical) Modifier.fillMaxHeight() else Modifier.height(24.dp)).clip(ControlShape)
            .background(if (vertical) Color.Transparent else colors.input).clickable { open = true }
            .padding(horizontal = if (vertical && !labeled) 2.dp else 8.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = if (vertical && !labeled) Arrangement.Center else Arrangement.spacedBy(6.dp)) {
            SharedIcon(selected.getString("icon"), choice.getString("label"), Modifier.size(16.dp))
            if (!vertical || labeled) Text(selected.getString("label"), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (!vertical) SharedIcon("chevron-down", null, Modifier.size(12.dp))
        }
        DropdownMenu(open, { open = false }) { items.forEach { item ->
            DropdownMenuItem(text = { Text(item.getString("label")) }, leadingIcon = { SharedIcon(item.getString("icon"), null, Modifier.size(16.dp)) },
                onClick = { open = false; edit(item.getJSONObject("action")) })
        } }
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
        }.pointerInput(vertical, enabled) {
            if (!enabled) return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(); down.consume()
                var moved = false; var finished = false
                fun pick(p: Offset, snap: Boolean) {
                    val half = 5.dp.toPx(); val length = (if (vertical) size.height else size.width) - 2 * half
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
        val thick = 20.dp.toPx().coerceAtMost(if (vertical) size.width else size.height)
        val x = (size.width - thick) / 2; val y = (size.height - thick) / 2
        val track = Path().apply {
            if (vertical) { moveTo(x, 0f); lineTo(x + thick, 0f); lineTo(size.width / 2 + if (opacity) thick / 2 else 1f, size.height); lineTo(size.width / 2 - if (opacity) thick / 2 else 1f, size.height) }
            else { moveTo(0f, size.height / 2 - if (opacity) thick / 2 else 1f); lineTo(size.width, y); lineTo(size.width, y + thick); lineTo(0f, size.height / 2 + if (opacity) thick / 2 else 1f) }; close()
        }
        clipPath(track) {
            if (opacity) {
                val cell = 4.dp.toPx()
                for (row in 0..(size.height / cell).toInt()) for (col in 0..(size.width / cell).toInt())
                    drawRect(if ((row + col) % 2 == 0) Color(0xffdddddd) else Color(0xff888888), Offset(col * cell, row * cell), Size(cell, cell))
                drawRect(if (vertical) Brush.verticalGradient(listOf(Color(0xff222222), Color.Transparent)) else Brush.horizontalGradient(listOf(Color.Transparent, Color(0xff222222))))
            } else drawRect(colors.text.copy(alpha = .4f))
        }
        val marker = 10.dp.toPx()
        if (vertical) drawRoundRect(colors.thumb, Offset(x - 4.dp.toPx(), (size.height - marker) * (1f - fill)), Size(thick + 8.dp.toPx(), marker), CornerRadius(marker / 2))
        else drawRoundRect(colors.thumb, Offset((size.width - marker) * fill, y - 4.dp.toPx()), Size(marker, thick + 8.dp.toPx()), CornerRadius(marker / 2))
        for (mark in marks) {
            val position = if (mark.getBoolean("selected")) fill else mark.number("fill")
            val center = if (vertical) Offset(size.width / 2, marker / 2 + (size.height-marker)*(1f-position))
                else Offset(marker / 2 + (size.width-marker)*position, size.height / 2)
            val delta = if (vertical) Offset(7.dp.toPx(), 0f) else Offset(0f, 7.dp.toPx())
            drawLine(if (mark.getBoolean("selected")) colors.panel else colors.text, center-delta, center+delta, 2.dp.toPx())
        }

    }
}


@Composable private fun BrushSliderPreview(stamp: JSONObject, control: JSONObject, value: Float, length: Float,
    vertical: Boolean, selected: Boolean, dismiss: () -> Unit, bookmark: () -> Unit) {
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    val layout = toolbarUi(obj("type" to "slider_preview", "control" to control, "value" to value, "length" to length, "extent" to stamp.number("extent")))
    val bitmap = remember(stamp) {
        val alpha = stamp.array("alpha"); val size = stamp.getInt("size")
        android.graphics.Bitmap.createBitmap(IntArray(alpha.length()) { (alpha.getInt(it) shl 24) or 0x00ffffff }, size, size, android.graphics.Bitmap.Config.ARGB_8888).asImageBitmap()
    }
    val provider = remember(vertical, density) { object : PopupPositionProvider {
        override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
            val gap = (8*density).toInt(); val p = popupContentSize; val a = anchorBounds
            val x = if (vertical) { if (a.right+p.width+gap <= windowSize.width) a.right+gap else a.left-p.width-gap } else a.left
            val y = if (vertical) a.top+(a.height-p.height)/2 else { if (a.bottom+p.height+gap <= windowSize.height) a.bottom+gap else a.top-p.height-gap }
            return IntOffset(x.coerceIn(gap, maxOf(gap,windowSize.width-p.width-gap)), y.coerceIn(gap,maxOf(gap,windowSize.height-p.height-gap)))
        }
    } }
    Popup(provider, onDismissRequest = dismiss, properties = PopupProperties(focusable = false)) {
        Surface(Modifier.size(layout.number("side").dp).testTag("brush-slider-preview"), shape = SurfaceShape,
            color = colors.panel, shadowElevation = 6.dp, border = androidx.compose.foundation.BorderStroke(1.dp, colors.divider)) {
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
                    if (fade > 0f) drawRect(Brush.verticalGradient(0f to colors.panel.copy(alpha=.65f),
                        1f to colors.panel.copy(alpha=0f), endY=fade), size=Size(size.width,fade))
                }
                Row(Modifier.fillMaxWidth().padding(start=12.dp,end=5.dp,top=4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(layout.getString("text"), Modifier.weight(1f), maxLines=1)
                    Box(Modifier.size(28.dp).clip(ControlShape).clickable(onClick=bookmark).testTag("slider-bookmark"), contentAlignment=Alignment.Center) {
                        SharedIcon(if(selected) "minus" else "plus", if(selected) "Remove bookmark" else "Bookmark this value", Modifier.size(16.dp))
                    }
                }
            }
        }
    }
}
