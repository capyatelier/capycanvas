package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Paint
import android.graphics.Typeface
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.ViewConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.*
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import kotlin.math.*

private fun JSONArray.color() = Color(getDouble(0).toFloat(), getDouble(1).toFloat(), getDouble(2).toFloat(), optDouble(3, 1.0).toFloat())
private fun JSONArray.point(scale: Float) = Offset(getDouble(0).toFloat() * scale, getDouble(1).toFloat() * scale)
private fun Modifier.place(rect: JSONArray) = offset(rect.getDouble(0).toFloat().dp, rect.getDouble(1).toFloat().dp)
    .size(rect.getDouble(2).toFloat().dp, rect.getDouble(3).toFloat().dp)

/** One square, including the corner controls. Rust owns all layout and color math. */
@Composable internal fun ColorPanelControls(host: CanvasHost, availableHeight: Dp = Dp.Infinity, onHeight: (natural: Float, displayed: Float) -> Unit = { _, _ -> }) {
    val view = host.panelContent?.objectOrNull("color_panel") ?: return
    val colors = LocalPalette.current
    val config = LocalViewConfiguration.current
    // Expanding the compact corner targets to 48dp would cover the hue ring and
    // the intentional paint overlap. Keep the shared, clipped hit regions exact.
    val compactConfig = remember(config) { object : ViewConfiguration by config {
        override val minimumTouchTargetSize = DpSize.Zero
    } }
    fun color(action: JSONObject) = host.dispatch(obj("type" to "color", "action" to action))
    BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
        val side = minOf(maxWidth, (availableHeight - 16.dp).coerceAtLeast(128.dp)).coerceAtLeast(128.dp)
        SideEffect { onHeight(maxWidth.coerceAtLeast(128.dp).value, side.value) }
        val layout = remember(side) { JSONObject(Native.colorPanelLayout(side.value)) }
        CompositionLocalProvider(LocalViewConfiguration provides compactConfig) {
            Box(Modifier.size(side).testTag("color-panel")) {
                ColorWheel(view, Modifier.place(layout.array("wheel")), ::color)
                // Foreground paints/hits above background in their shared overlap.
                for (slot in listOf("background", "foreground", "transparent")) {
                    val swatch = view.array("swatches").objects().first { it.getString("slot") == slot }
                    ColorSwatch(swatch, Modifier.place(layout.array(slot)), ::color)
                }
                view.array("other_shapes").values().forEachIndexed { index, value ->
                    val shape = value as String
                    val model = when (shape) { "circle" -> "Okhsv"; "square" -> "HSV"; else -> "HLS" }
                    ColorButton("Use $model $shape", Modifier.place(layout.array("shapes").getJSONArray(index))
                        .testTag("color-shape-$shape"), onClick = { color(obj("op" to "shape", "shape" to shape)) }) { focused, hovered ->
                        SharedIcon("color-$shape", null, Modifier.size(16.dp).rotate(layout.array("shape_rotations").getDouble(index).toFloat()),
                            tint = if (focused || hovered) colors.accent else colors.text)
                    }
                }
                ColorButton("Swap foreground and background", Modifier.place(layout.array("swap")).testTag("color-swap"),
                    onClick = { color(obj("op" to "swap")) }) { _, hovered ->
                    Canvas(Modifier.matchParentSize()) { if (hovered) drawCircle(colors.text.copy(alpha = .12f)) }
                    SharedIcon("color-swap", null, Modifier.size(16.dp))
                }
                val readoutClip = remember(layout) { ReadoutCorner(layout.array("wheel").getDouble(2).toFloat() * view.getJSONObject("geometry").number("outer") + 2f) }
                ColorButton(view.getString("readout_description"), Modifier.place(layout.array("readout")).testTag("color-readout"),
                    shape = readoutClip, showFocusRing = false, onClick = { color(obj("op" to "toggle_readout")) }) { focused, _ ->
                    ColorReadout(view, layout.number("readout_radius"), if (focused) colors.accent else colors.text, focused)
                }
            }
        }
    }
}

/** Clip drawing AND picking so the top-left readout cannot intercept the ring. */
private class ReadoutCorner(private val radius: Float) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val r = radius * density.density
        val square = Path().apply { addRect(Rect(Offset.Zero, size)) }
        val disc = Path().apply { addOval(Rect(size.width - r, size.height - r, size.width + r, size.height + r)) }
        return Outline.Generic(Path.combine(PathOperation.Difference, square, disc))
    }
}

@Composable private fun ColorButton(label: String, modifier: Modifier, shape: Shape = CircleShape,
    showFocusRing: Boolean = true, onClick: () -> Unit, content: @Composable BoxScope.(Boolean, Boolean) -> Unit) {
    val interactions = remember { MutableInteractionSource() }
    val focused by interactions.collectIsFocusedAsState()
    val hovered by interactions.collectIsHoveredAsState()
    val colors = LocalPalette.current
    val host = LocalCanvasHost.current
    val focusToken = remember { Any() }
    DisposableEffect(host, focusToken) { onDispose { if (host.colorControlFocus === focusToken) host.colorControlFocus = null } }
    Box(modifier.clip(shape).semantics { contentDescription = label }
        .onFocusChanged { if (it.isFocused) host.colorControlFocus = focusToken else if (host.colorControlFocus === focusToken) host.colorControlFocus = null }
        .clickable(interactionSource = interactions, indication = null, role = Role.Button, onClick = onClick), contentAlignment = Alignment.Center) {
        content(focused, hovered)
        if (focused && showFocusRing) Canvas(Modifier.matchParentSize()) {
            drawCircle(colors.accent, size.minDimension / 2 - 2.dp.toPx(), style = Stroke(1.5.dp.toPx()))
        }
    }
}

@Composable private fun ColorSwatch(swatch: JSONObject, modifier: Modifier, color: (JSONObject) -> Unit) {
    val colors = LocalPalette.current
    val slot = swatch.getString("slot")
    val selected = swatch.getBoolean("selected")
    var menu by remember { mutableStateOf(false) }
    val focusedWindow = LocalWindowInfo.current.isWindowFocused
    ColorButton(swatch.getString("label"), modifier.testTag("color-swatch-$slot").semantics { this.selected = selected }.clip(CircleShape)
        .then(if (slot == "transparent") Modifier else Modifier
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown && (event.key == Key.Menu || event.key == Key.F10 && event.isShiftPressed)) {
                    menu = true; true
                } else false
            }
            .pointerInput(slot, focusedWindow) {
                if (!focusedWindow) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    if (currentEvent.buttons.isSecondaryPressed) {
                        down.consume(); menu = true
                        return@awaitEachGesture
                    }
                    if (down.type == PointerType.Mouse) return@awaitEachGesture
                    // Own this contact before opening a native popup can cancel
                    // its coroutine. Otherwise clickable may turn the held
                    // release into an ordinary paint selection after focus moves.
                    down.consume()
                    val released = withTimeoutOrNull(viewConfiguration.longPressTimeoutMillis) {
                        while (true) {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (change.isConsumed || (change.position - down.position).getDistance() > viewConfiguration.touchSlop) break
                            if (!change.pressed) {
                                change.consume()
                                color(obj("op" to "select", "slot" to slot))
                                break
                            }
                        }
                        true
                    }
                    if (released == null) {
                        menu = true
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            val cancelled = !change.pressed && change.isConsumed
                            change.consume()
                            if (cancelled || (change.position - down.position).getDistance() > viewConfiguration.touchSlop) menu = false
                        } while (change.pressed)
                    }
                }
            }), onClick = { color(obj("op" to "select", "slot" to slot)) }) { _, hovered ->
        Canvas(Modifier.fillMaxSize()) {
            val radius = size.minDimension / 2
            val padding = (if (slot == "foreground") 3.dp else 1.dp).toPx()
            drawCircle(colors.panel)
            val field = Path().apply { addOval(Rect(center - Offset(radius - padding, radius - padding), Size((radius - padding) * 2, (radius - padding) * 2))) }
            clipPath(field) {
                val tile = 5.dp.toPx()
                for (y in 0..ceil(size.height / tile).toInt()) for (x in 0..ceil(size.width / tile).toInt()) {
                    drawRect(if ((x + y) % 2 == 0) Color(0xffcccccc) else Color(0xff8c8c8c), Offset(padding + x * tile, padding + y * tile), Size(tile, tile))
                }
                drawRect(swatch.array("rgba").color())
            }
            val stroke = (if (selected || hovered) 2.dp else 1.dp).toPx()
            drawCircle(colors.text.copy(alpha = if (selected || hovered) 1f else .25f), radius - stroke / 2, style = Stroke(stroke))
        }
        DropdownMenu(menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(text = { Text("Swap foreground and background") }, leadingIcon = { SharedIcon("color-swap", null) },
                modifier = Modifier.testTag("color-swap-menu"), onClick = { menu = false; color(obj("op" to "swap")) })
        }
    }
}

@Composable private fun ColorWheel(view: JSONObject, modifier: Modifier, color: (JSONObject) -> Unit) {
    val shape = view.getString("shape")
    val hue = view.array("wheel_components").getDouble(0).toFloat()
    val stops = remember(shape) { JSONArray(Native.colorHueStops(shape)).objects().map { it.number("offset") to it.array("color").color() }.toTypedArray() }
    val focused = LocalWindowInfo.current.isWindowFocused
    val pick by rememberUpdatedState(color)
    BoxWithConstraints(modifier) {
        val density = LocalDensity.current.density
        val pixels = ceil(maxWidth.value * if (shape == "circle") 1f else density).toInt().coerceIn(1, 2048)
        // Like GTK/Web, sample the smooth disc once per logical pixel. Keep the
        // ring, clip, triangle and marker outlines at the tablet's physical DPI.
        val field = remember(shape, hue, pixels) {
            if (shape == "square") null else Bitmap.createBitmap(Native.colorFieldPixels(pixels, hue, shape), pixels, pixels, Bitmap.Config.ARGB_8888).asImageBitmap()
        }
        Canvas(Modifier.fillMaxSize().testTag("color-wheel").semantics { contentDescription = "Color wheel" }
            .pointerInput(shape, focused) {
                if (!focused) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    if (down.isConsumed || currentEvent.buttons.isSecondaryPressed ||
                        down.type == PointerType.Mouse && !currentEvent.buttons.isPrimaryPressed) return@awaitEachGesture
                    val side = minOf(size.width, size.height).toFloat()
                    val part = JSONTokener(Native.colorWheelHit(obj("size" to side, "point" to JSONArray(listOf(down.position.x, down.position.y)), "shape" to shape).toString())).nextValue() as? String
                    if (part != null) {
                        fun update(point: Offset) = pick(obj("op" to "pick_wheel", "part" to part, "size" to side, "point" to JSONArray(listOf(point.x, point.y))))
                        down.consume(); update(down.position)
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (!change.pressed && change.isConsumed) break // ACTION_CANCEL is a consumed release.
                            change.consume()
                            if (change.pressed && change.position != change.previousPosition) update(change.position)
                        } while (change.pressed)
                    }
                }
            }) {
            val side = size.minDimension
            val geometry = view.getJSONObject("geometry")
            val center = geometry.array("center").point(side)
            val inner = geometry.number("inner") * side
            val outer = geometry.number("outer") * side
            if (shape == "square") {
                val square = geometry.array("square")
                val origin = square.point(side)
                val length = square.getDouble(2).toFloat() * side
                val outline = Path().apply { addRoundRect(RoundRect(Rect(origin, Size(length, length)), CornerRadius(min(6.dp.toPx(), side * .02f)))) }
                clipPath(outline) {
                    drawRect(Brush.horizontalGradient(listOf(Color.White, view.array("wheel_hue_color").color()), origin.x, origin.x + length), origin, Size(length, length))
                    drawRect(Brush.verticalGradient(listOf(Color.Transparent, Color.Black), origin.y, origin.y + length), origin, Size(length, length))
                }
            } else if (field != null) {
                val outline = Path().apply {
                    if (shape == "circle") {
                        val radius = geometry.number("disc_radius") * side
                        addOval(Rect(center - Offset(radius, radius), Size(radius * 2, radius * 2)))
                    } else addRect(Rect(Offset.Zero, size))
                }
                clipPath(outline) { drawImage(field, dstSize = IntSize(size.width.roundToInt(), size.height.roundToInt()), filterQuality = FilterQuality.Low) }
            }
            rotate(view.number("wheel_hue_start_degrees"), center) {
                drawCircle(Brush.sweepGradient(*stops, center = center), (inner + outer) / 2, center, style = Stroke(outer - inner))
            }
            val radius = (maxWidth.value * .04f).coerceIn(6f, 10f).dp.toPx()
            for ((key, fill) in listOf("wheel_hue_marker" to "wheel_hue_color", "wheel_marker" to "marker_color")) {
                val point = view.array(key).point(side)
                drawCircle(view.array(fill).color(), radius, point)
                drawCircle(Color.Black.copy(alpha = .5f), radius, point, style = Stroke(4.dp.toPx()))
                drawCircle(Color.White, radius, point, style = Stroke(2.dp.toPx()))
            }
        }
    }
}

/** Native glyph advances, with the same fixed digit cells and arc as GTK/Web. */
@Composable private fun ColorReadout(view: JSONObject, radius: Float, ink: Color, focused: Boolean) {
    val paint = remember { Paint(Paint.ANTI_ALIAS_FLAG or Paint.SUBPIXEL_TEXT_FLAG) }
    Canvas(Modifier.fillMaxSize()) {
        drawIntoCanvas { composeCanvas ->
            val canvas = composeCanvas.nativeCanvas
            canvas.save()
            canvas.scale(density, density)
            val half = size.width / density
            var font = (half * 2 * .044f).coerceIn(9f, 12f)
            paint.typeface = Typeface.create("sans-serif", Typeface.BOLD)
            paint.textSize = font
            paint.color = ink.copy(alpha = .9f).toArgb()
            canvas.drawText(view.getString("readout_label"), 2f, font + 1f, paint)
            if (focused) {
                paint.style = Paint.Style.STROKE; paint.strokeWidth = 1.5f
                canvas.drawRoundRect(1f, 1f, paint.measureText(view.getString("readout_label")) + 6f, font + 5f, 5f, 5f, paint)
                paint.style = Paint.Style.FILL
            }
            paint.typeface = Typeface.create("sans-serif", Typeface.NORMAL)
            val rgb = view.getString("readout") == "rgb"
            val texts = view.array("readout_layout_text").values().map { it as String }
            val available = radius * PI.toFloat() / 2 - 4
            var digitAdvance = 0f
            var widths: List<Float>
            var total: Float
            fun advance(c: Char) = if (c in '0'..'9' || c == ' ') digitAdvance else paint.measureText(c.toString())
            while (true) {
                paint.textSize = font
                digitAdvance = ('0'..'9').maxOf { paint.measureText(it.toString()) }
                widths = texts.map { text -> text.sumOf { advance(it).toDouble() }.toFloat() + if (rgb) font * .8f + 2 else 0f }
                total = widths.sum()
                if (total + 6 <= available || font <= 8) break
                font -= .25f
            }
            val chip = font * .8f
            val gap = min(radius * .24f, max(3f, (available - total) * .5f))
            var cursor = -(total + gap * 2) * .5f
            fun at(angle: Float, draw: () -> Unit) {
                canvas.save()
                canvas.translate(half + radius * cos(angle), half + radius * sin(angle))
                canvas.rotate(angle * 180 / PI.toFloat() + 90)
                draw(); canvas.restore()
            }
            texts.forEachIndexed { index, text ->
                val width = widths[index]
                val mid = -135 * PI.toFloat() / 180 + (cursor + width * .5f) / radius
                cursor += width + gap
                var along = -width * .5f
                if (rgb) {
                    at(mid + (along + chip * .5f) / radius) {
                        paint.color = listOf(Color(.93f, .31f, .36f), Color(.25f, .73f, .43f), Color(.29f, .56f, .98f))[index].toArgb()
                        canvas.drawRoundRect(-chip * .5f, -font * .76f, chip * .5f, -font * .76f + chip, 2f, 2f, paint)
                    }
                    along += chip + 2
                }
                paint.color = ink.copy(alpha = .8f).toArgb()
                for (glyph in text) {
                    val cell = advance(glyph)
                    at(mid + (along + cell * .5f) / radius) { canvas.drawText(glyph.toString(), -paint.measureText(glyph.toString()) * .5f, 0f, paint) }
                    along += cell
                }
            }
            canvas.restore()
        }
    }
}
