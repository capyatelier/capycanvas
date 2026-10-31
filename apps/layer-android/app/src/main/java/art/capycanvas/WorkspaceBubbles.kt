package art.capycanvas

import android.os.SystemClock
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.*
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.*
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.*
import androidx.compose.ui.zIndex
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import kotlin.math.*

internal fun bubbleUi(request: JSONObject): Any? = JSONTokener(Native.bubbleUi(request.toString())).nextValue().takeUnless { it == JSONObject.NULL }
internal fun JSONArray.bubblePoint() = Offset(getDouble(0).toFloat(), getDouble(1).toFloat())
internal fun CanvasHost.bubble(action: JSONObject) = dispatch(obj("type" to "bubble", "action" to action), action.optString("phase") == "move")
private val LocalBubbleOrigin = staticCompositionLocalOf { Offset.Zero }
internal val LocalBubbleOwner = staticCompositionLocalOf<Int?> { null }
private fun smooth(t: Float) = t * t * (3f - 2f * t)
private fun ease(t: Float) = 1f - (1f - t).let { it * it * it }

internal class BubbleTween(initial: Float = 0f, private val easing: (Float) -> Float = ::ease) {
    var value by mutableFloatStateOf(initial)
    var target by mutableFloatStateOf(initial)
    private var from = initial
    private var start = -1L
    private var duration = 0L
    private var delay = 0L
    fun snap(next: Float) { value = next; target = next; from = next; start = -1L }
    fun to(next: Float, ms: Long, wait: Long = 0) {
        if (target == next) return
        from = value; target = next; duration = ms; delay = wait; start = -1L
    }
    fun step(now: Long, reduced: Boolean): Boolean {
        if (value == target) return false
        if (start < 0) start = now
        val t = if (reduced || duration == 0L) 1f else ((now - start - delay).toFloat() / duration).coerceIn(0f, 1f)
        value = from + (target - from) * easing(t)
        return t < 1f
    }
}

internal class BubblePart(val key: String, data: JSONObject, val fromScale: Float, initial: Float = 0f) {
    var data by mutableStateOf(data)
    val appearance = BubbleTween(initial)
    var center by mutableStateOf(Offset.Zero)
    var size by mutableStateOf(Size.Zero)
    var radius = 0f
    var squircle = false
    var glass = true
    var strength by mutableFloatStateOf(1f)
    private val layout = BubbleTween(1f)
    private var fromCenter = Offset.Zero
    private var toCenter = Offset.Zero
    private var fromSize = Size.Zero
    private var toSize = Size.Zero
    private var fromStrength = 1f
    private var toStrength = 1f
    fun place(at: Offset, extent: Size, alpha: Float, animate: Boolean) {
        if (at == toCenter && extent == toSize && alpha == toStrength) return
        fromCenter = center; toCenter = at; fromSize = size; toSize = extent; fromStrength = strength; toStrength = alpha
        layout.snap(if (animate && size != Size.Zero) 0f else 1f)
        layout.to(1f, 220)
        if (layout.value == 1f) { center = at; size = extent; strength = alpha }
    }
    fun step(now: Long, reduced: Boolean): Boolean {
        val changing = layout.step(now, reduced)
        center = fromCenter + (toCenter - fromCenter) * layout.value
        size = Size(fromSize.width + (toSize.width - fromSize.width) * layout.value, fromSize.height + (toSize.height - fromSize.height) * layout.value)
        strength = fromStrength + (toStrength - fromStrength) * layout.value
        return appearance.step(now, reduced) || changing
    }
    var order by mutableIntStateOf(0)
    var hot by mutableStateOf(false)
    val scale get() = (fromScale + (1f - fromScale) * appearance.value) * if (hot) { if (key.startsWith("brush")) 1.06f else 1.12f } else 1f
}

internal class BubbleRecord(packet: JSONObject) {
    var packet by mutableStateOf(packet)
    val id = packet.getJSONObject("view").getInt("id")
    val colorSurface = colorTouchSurfaces.incrementAndGet()
    val view get() = packet.getJSONObject("view")
    var order by mutableIntStateOf(0)
    var center by mutableStateOf(Offset.Zero)
    var lifted by mutableStateOf(false)
    var homing by mutableStateOf(false)
    var pinch by mutableFloatStateOf(1f)
    var faded = false
    val fade = BubbleTween(1f, ::smooth)
    val parts = mutableStateMapOf<String, BubblePart>()
    var hovered by mutableStateOf<Any?>(null)
    var pressed by mutableStateOf(false)
    val scale get() = pinch * if (homing) .82f else if (lifted) 1.04f else 1f
    val alpha get() = fade.value * if (homing) .55f else 1f
    fun hit(point: Offset) = bubbleUi(obj("type" to "hit", "view" to view, "point" to (point - center).headerPoint()))
    fun update(next: JSONObject, tweenBeads: Boolean) {
        val sameDiameter = view.number("diameter") == next.getJSONObject("view").number("diameter")
        packet = next
        val v = view
        val live = mutableSetOf<String>()
        fun part(key: String, data: JSONObject, center: Offset, size: Size, radius: Float = size.height / 2,
            squircle: Boolean = false, glass: Boolean = true, strength: Float = 1f, from: Float = .6f, duration: Long = 180, delay: Long = 0): BubblePart {
            live += key
            val p = parts.getOrPut(key) { BubblePart(key, data, from, if (key in listOf("face", "badge") && parts["card"] == null) 1f else 0f) }
            p.order = live.size
            p.data = data; p.radius = radius; p.squircle = squircle; p.glass = glass
            p.place(center, size, strength, tweenBeads && sameDiameter && key.startsWith("bead:"))
            p.appearance.to(1f, duration, delay)
            return p
        }
        val face = v.array("face").bubblePoint()
        val colorBounds = next.objectOrNull("color")?.getJSONObject("bounds")?.rect()
        if (v.objectOrNull("card") == null) part("face", v, colorBounds?.center ?: Offset.Zero, colorBounds?.size ?: Size(face.x, face.y), radius = if (v.getString("kind") == "layers") 20f else face.y / 2, squircle = v.getString("kind") == "layers", strength = if (v.getBoolean("enabled")) 1f else .45f, delay = if (parts["card"] != null) 150 else 0)
        if (v.getString("kind") == "layers" && v.objectOrNull("card") == null) {
            val badge = bubbleUi(obj("type" to "count_badge", "text" to JSONArray(listOf(0, 22)))) as JSONArray
            val size = badge.getJSONArray(1).bubblePoint()
            part("badge", obj("text" to v.getInt("layers").toString()), badge.getJSONArray(0).bubblePoint(), parts["badge"]?.size ?: Size(size.x, size.y), from = .6f, duration = 180, delay = if (parts["card"] != null) 150 else 0)
        }
        if (v.array("rings").length() > 0) part("halo", v.copy().put("_geometry", next.array("rings")), Offset.Zero, Size.Zero, glass = false, from = .85f, duration = 160)
        if (!v.isNull("readout")) part("readout", obj("text" to v.getString("readout")), next.array("readout").bubblePoint(), parts["readout"]?.size ?: Size.Zero, from = 1f, duration = 0)
        var fresh = 0L
        v.array("chooser").objects().forEachIndexed { i, item ->
            val brush = item.getString("kind") == "brush"
            val key = "${if (brush) "brush" else "item"}:$i"
            val previous = parts[key]
            val changed = previous?.data?.let { it.optString("label") != item.optString("label") || it.opt("preview") != item.opt("preview") } == true
            if (changed) {
                parts.remove(key)?.let { old -> old.appearance.to(0f, 120); parts["retired:$key:${SystemClock.uptimeMillis()}"] = old }
            }
            val delay = if (parts[key] == null) fresh++ * if (brush) 16L else 14L else 0L
            part(key, item, item.array("center").bubblePoint(), if (brush) Size(152f, 48f) else Size(44f, 44f), strength = if (item.getBoolean("enabled")) 1f else .45f, delay = delay)
            if (!brush) {
                val tag = item.getJSONObject("tag")
                part("tag:$i", item, tag.rect().center, tag.rect().size, radius = 9f, from = 1f, duration = 0)
            }
        }
        v.array("beads").objects().forEachIndexed { i, bead ->
            val placement = bead.getJSONObject("placement")
            part("bead:${placement.getJSONObject("bead")}", obj("bead" to bead, "rgba" to next.array("bead_colors").get(i)), placement.array("center").bubblePoint(),
                Size(placement.number("size"), placement.number("size")), glass = next.array("bead_colors").isNull(i), strength = placement.number("alpha"), from = 1f, duration = 220)
        }
        if (!v.isNull("pill")) part("pill", obj("text" to v.getString("pill")), next.getJSONObject("pill").rect().center, Size(184f, 30f), from = 1f, duration = 0)
        v.objectOrNull("card")?.let { card ->
            val size = card.array("size").bubblePoint()
            part("card", card, Offset.Zero, Size(size.x, size.y), radius = 18f, squircle = true, duration = 200)
        }
        for ((key, p) in parts) if (key !in live) p.appearance.to(0f, if (key == "card") 150 else 120)
    }
    fun step(now: Long, reduced: Boolean): Boolean {
        var active = fade.step(now, reduced)
        for (part in parts.values) active = part.step(now, reduced) || active
        parts.entries.removeAll { it.value.appearance.value == 0f && it.value.appearance.target == 0f }
        return active
    }
}

@Composable internal fun ToolBubbles(input: BubbleInteraction) {
    val host = input.host
    PopupOwner(input.menuModel != null)
    val packets = host.snapshot?.optJSONArray("bubbles")
    val state = host.snapshot?.objectOrNull("state") ?: return
    val density = LocalDensity.current.density
    LaunchedEffect(packets, host.menuEpoch(), state.getJSONObject("camera").array("viewport").toString()) { input.refresh(); input.wake() }
    LaunchedEffect(input) {
        val motionScale = currentCoroutineContext()[androidx.compose.ui.MotionDurationScale]
        while (isActive) {
            input.awaitWake()
            while (input.active) {
                val now = withFrameNanos { it }
                val owners = input.ownersVersion
                val wake = input.wakeVersion
                val reduced = motionScale?.scaleFactor == 0f
                val motion = host.awaitQuery(obj("type" to "bubble_motion", "now_ns" to now, "reduced" to reduced)) ?: break
                input.refresh()
                if (owners == input.ownersVersion) input.step(motion, now / 1_000_000, reduced, wake)
            }
        }
    }
    DisposableEffect(input) { onDispose { input.cancel(); host.bubbleGlass(emptyList(), emptyList()) } }
    val colors = LocalPalette.current.onGlass
    CompositionLocalProvider(LocalPalette provides colors) {
        for (record in input.records.values.sortedBy { it.order }) key(record.id) {
            BubbleStamp(host, record, density)
            val reach = record.packet.number("reach") + 16f
            CompositionLocalProvider(LocalBubbleOrigin provides Offset(reach, reach), LocalBubbleOwner provides record.id) {
            Box(Modifier.offset { IntOffset(((record.center.x - reach) * density).roundToInt(), ((record.center.y - reach) * density).roundToInt()) }
                .wrapContentSize(Alignment.TopStart, unbounded = true).requiredSize((reach * 2).dp).zIndex(if (record.lifted) 701f else 700f).graphicsLayer {
                    scaleX = record.scale; scaleY = record.scale; alpha = record.alpha
                }) {
                for ((partKey, part) in record.parts.entries.sortedWith(compareBy({ it.value.order }, { !it.key.startsWith("retired:") }))) key(partKey) {
                    when (part.key) {
                        "halo" -> BubbleHalo(record, part)
                        "card" -> BubbleSurface(record, part, if (part.appearance.target == 0f) Modifier else Modifier.testTag("bubble-card-${record.id}").bubbleCardSurface(record.id)) {
                            RetainedBubbleContent(part.appearance.target != 0f) {
                            val card = part.data
                            val position by androidx.compose.animation.core.animateFloatAsState(if (card.getBoolean("properties")) 1f else 0f,
                                androidx.compose.animation.core.tween(200), label = "bubble-properties")
                            LayerInspector(host, state, card.getBoolean("properties"), position, part.size.height, 49f,
                                "bubble:${record.id}:${card.getBoolean("full")}", full = card.getBoolean("full"), labelOverride = card.getString("disclosure").takeUnless { card.getBoolean("full") }, bubbleId = record.id,
                                toggle = { input.send(obj("type" to "properties", "id" to record.id)) }) { size ->
                                if (record.view.objectOrNull("card") != null) input.send(obj("type" to "measure_card", "id" to record.id, "heights" to size.array("heights")))
                            }
                            }
                        }
                        "face" -> BubbleSurface(record, part, Modifier.testTag("tool-bubble-${record.id}").semantics {
                            contentDescription = record.view.getString("label"); role = Role.Button
                            onClick { input.send(obj("type" to "tap", "id" to record.id, "position" to JSONArray(listOf(0, 0)))); true }
                        }) { BubbleFace(host, record) }
                        "badge", "readout" -> {
                            val measurer = androidx.compose.ui.text.rememberTextMeasurer()
                            val text = part.data.getString("text")
                            val style = androidx.compose.material3.LocalTextStyle.current.let {
                                if (part.key == "badge") it.copy(fontWeight = FontWeight.SemiBold) else it.copy(fontFeatureSettings = "tnum")
                            }
                            val measured = measurer.measure(text, style).size
                            SideEffect {
                                val size = if (part.key == "badge") {
                                    val badge = bubbleUi(obj("type" to "count_badge", "text" to JSONArray(listOf(measured.width / density, max(22f, measured.height / density))))) as JSONArray
                                    val extent = badge.getJSONArray(1).bubblePoint(); Size(extent.x, extent.y)
                                } else Size(measured.width / density + 14f, measured.height / density + 4f)
                                part.radius = size.height / 2
                                part.place(part.center, size, 1f, false)
                            }
                            BubbleSurface(record, part) { Text(text, Modifier.padding(horizontal = if (part.key == "badge") 5.dp else 7.dp), style = style, maxLines = 1) }
                        }
                        "pill" -> BubbleSurface(record, part) {
                            Row(Modifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                                Box(Modifier.width(24.dp).fillMaxHeight().clickable { input.send(obj("type" to "palette", "id" to record.id, "step" to -1)) }, contentAlignment = Alignment.Center) { SharedIcon("go-previous", null) }
                                Text(part.data.getString("text"), Modifier.weight(1f).clickable { input.menu(record, record.center + part.center, true) }.padding(horizontal = 4.dp), fontWeight = FontWeight.Bold, textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Box(Modifier.width(24.dp).fillMaxHeight().clickable { input.send(obj("type" to "palette", "id" to record.id, "step" to 1)) }, contentAlignment = Alignment.Center) { SharedIcon("go-next", null) }
                            }
                        }
                        else -> when {
                            part.key.startsWith("tag:") -> if (record.parts["item:${part.key.substringAfter(':')}"]?.appearance?.value == 1f && (part.data.optBoolean("shown") || (record.hovered as? JSONObject)?.optInt("item", -1) == part.key.substringAfter(':').toInt()))
                                {
                                    val text = part.data.getString("label")
                                    val measurer = androidx.compose.ui.text.rememberTextMeasurer()
                                    val measured = measurer.measure(text, androidx.compose.material3.LocalTextStyle.current).size
                                    val width = min(96f, measured.width / density + 18f)
                                    SideEffect {
                                        val bounds = (bubbleUi(obj("type" to "chooser_tag", "center" to part.data.array("center"), "width" to width)) as JSONObject).rect()
                                        part.place(bounds.center, Size(width, measured.height / density + 6f), 1f, false)
                                    }
                                    BubbleSurface(record, part) { Text(text, Modifier.padding(horizontal = 9.dp), maxLines = 1, overflow = TextOverflow.Ellipsis) }
                                }
                            part.key.startsWith("bead:") -> BubbleSurface(record, part, fill = part.glass, bead = true) {
                                val bead = part.data.getJSONObject("bead")
                                if (part.glass) SharedIcon("palette", null, Modifier.size(13.dp))
                                else ColorSwatchFace(part.data.array("rgba").color(), bead.getJSONObject("placement").getJSONObject("bead").getString("kind"), bead.getBoolean("selected"), false, Modifier.fillMaxSize())
                            }
                            else -> BubbleSurface(record, part) {
                                val item = part.data
                                if (!item.isNull("preview")) {
                                    BrushPreview(host, item.getInt("preview"), Modifier.align(Alignment.TopCenter).padding(top = 6.dp).size(103.36.dp, 14.dp))
                                    Text(item.getString("label"), Modifier.align(Alignment.BottomCenter).padding(bottom = 5.dp, start = 8.dp, end = 8.dp).graphicsLayer { alpha = .8f }, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                } else SharedIcon(item.getString("icon"), item.getString("label"), Modifier.size(18.dp))
                            }
                        }
                    }
                }
                Canvas(Modifier.fillMaxSize()) {
                    record.view.array("hints").values().forEach { raw ->
                        val hint = raw as JSONArray
                        drawCircle(colors.text.copy(alpha = hint.getDouble(1).toFloat() * .35f), 2.5.dp.toPx(), center + hint.getJSONArray(0).bubblePoint() * density)
                    }
                }
            }
            }
        }
    }
    input.menuModel?.let { model ->
        Box(Modifier.offset { IntOffset((input.menuPoint.x * density).roundToInt(), (input.menuPoint.y * density).roundToInt()) }.zIndex(900f)) {
            WorkspaceMenu(host, model, preserveContact = true, copy = { input.menuRequest?.let { host.menuCopy(it) } }) { input.dismiss() }
        }
    }
}

private fun Modifier.bubbleCardSurface(id: Int) = pointerInput(id) {
    awaitPointerEventScope { while (true) awaitPointerEvent() }
}

@Composable private fun RetainedBubbleContent(active: Boolean, content: @Composable () -> Unit) {
    val layer = rememberGraphicsLayer()
    if (active) Box(Modifier.fillMaxSize().drawWithContent {
        layer.record { this@drawWithContent.drawContent() }
        drawLayer(layer)
    }) { content() }
    else Canvas(Modifier.fillMaxSize()) { drawLayer(layer) }
}

@Composable private fun BubbleSurface(record: BubbleRecord, part: BubblePart, modifier: Modifier = Modifier, fill: Boolean = true,
    bead: Boolean = false, content: @Composable BoxScope.() -> Unit) {
    val density = LocalDensity.current.density
    val origin = LocalBubbleOrigin.current
    val index = part.key.substringAfter(':', "").toIntOrNull()
    val item = part.key.startsWith("brush:") || part.key.startsWith("item:")
    val hovered = if (item) (record.hovered as? JSONObject)?.optInt("item", -1) == index
        else part.key == "face" && record.hovered == "face" && record.view.getString("kind") !in listOf("color", "history")
    SideEffect { part.hot = item && hovered && record.pressed }
    val color = record.packet.objectOrNull("color").takeIf { part.key == "face" }
    val shape = if (color != null) remember(record.view.number("diameter")) {
        BubbleColorShape(record.view.number("diameter"), color.getJSONObject("bounds").rect(), color.array("outline"))
    } else if (part.squircle) SquircleShape(part.radius.dp) else RoundedCornerShape(part.radius.dp)
    val colors = LocalPalette.current
    Box(Modifier.offset { IntOffset(((origin.x + part.center.x - part.size.width / 2) * density).roundToInt(), ((origin.y + part.center.y - part.size.height / 2) * density).roundToInt()) }
        .requiredSize(part.size.width.dp, part.size.height.dp)
        .then(if (part.key in listOf("face", "card")) Modifier else Modifier.testTag("bubble-part-${record.id}-${part.key}")).then(modifier)
        .surfaceLayer(18.dp) {
            alpha = part.appearance.value * part.strength
            scaleX = part.scale; scaleY = part.scale
        }.then(if (part.key == "face" && record.view.getString("kind") == "layers") Modifier else Modifier.bubbleShadow(shape, part.radius, part.squircle, record.lifted, bead && !part.glass,
            if ((item || part.key == "face") && !part.data.optBoolean("enabled", true)) .45f else 1f)).clip(shape).then(if (fill) Modifier.background(colors.panelFill) else Modifier), contentAlignment = Alignment.Center) {
        if (part.data.optBoolean("selected") && (item || part.key == "face" && record.view.getString("kind") in listOf("dial", "brush", "medium", "controls")) || bead && part.glass && part.data.getJSONObject("bead").getBoolean("selected")) Box(Modifier.matchParentSize().background(colors.active))
        if (hovered) Box(Modifier.matchParentSize().background(colors.text.copy(alpha = if (record.pressed) .14f else .07f)))
        content()
    }
}

private class BubbleColorShape(private val diameter: Float, private val bounds: Rect, private val arcs: JSONArray, private val outset: Float = 0f) : Shape {
    private val shadows = mutableMapOf<Float, BubbleColorShape>()
    fun expanded(distance: Float) = shadows.getOrPut(distance) {
        BubbleColorShape(diameter, bounds, bubbleUi(obj("type" to "color_outline", "size" to diameter, "outset" to distance)) as JSONArray, distance)
    }
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        val path = Path()
        arcs.objects().forEachIndexed { index, arc ->
            val center = (arc.array("center").bubblePoint() - bounds.topLeft + Offset(outset, outset)) * density.density
            val radius = arc.number("radius") * density.density
            path.arcTo(Rect(center - Offset(radius, radius), Size(radius * 2, radius * 2)),
                arc.number("start") * 180f / PI.toFloat(), arc.number("sweep") * 180f / PI.toFloat(), index == 0)
        }
        path.close()
        return Outline.Generic(path)
    }
}

@Composable private fun Modifier.bubbleShadow(shape: Shape, radius: Float, squircle: Boolean, lifted: Boolean, bead: Boolean, strength: Float): Modifier {
    val texture = rememberGraphicsLayer()
    return drawWithCache {
        val reach = (if (bead) 3f else if (lifted) 12f else 8f) * density
        val drop = (if (bead) 1f else if (lifted) 4.5f else 2f) * density
        val opacity = (if (bead) .13f else if (lifted) .16f else .08f) / strength
        val inner = Path().apply { addOutline(shape.createOutline(size, layoutDirection, this@drawWithCache)) }
        val steps = ceil(reach * 2).toInt().coerceAtLeast(1)
        val outlines = (steps downTo 1).map { index ->
            val distance = index * reach / steps
            val expanded = Size(size.width + distance * 2, size.height + distance * 2)
            val expandedShape = if (shape is BubbleColorShape) shape.expanded(distance / density) else if (squircle) SquircleShape((radius + distance / density).dp) else RoundedCornerShape((radius + distance / density).dp)
            Triple(distance, Path().apply { addOutline(expandedShape.createOutline(expanded, layoutDirection, this@drawWithCache)) }, opacity * (1f - distance / reach).pow(2))
        }
        val margin = ceil(reach + drop)
        texture.compositingStrategy = androidx.compose.ui.graphics.layer.CompositingStrategy.Offscreen
        texture.record(size = IntSize(ceil(size.width + margin * 2).toInt(), ceil(size.height + margin * 2).toInt())) {
            translate(margin, margin) {
                clipPath(inner, ClipOp.Difference) {
                    var previous = 0f
                    for ((distance, path, strength) in outlines) {
                        translate(-distance, drop - distance) { drawPath(path, Color.Black.copy(alpha = (strength - previous) / (1f - previous))) }
                        previous = strength
                    }
                }
            }
        }
        onDrawBehind { translate(-margin, -margin) { drawLayer(texture) } }
    }
}

@Composable private fun BubbleFace(host: CanvasHost, record: BubbleRecord) {
    val v = record.view
    val colors = LocalPalette.current
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        when (v.getString("kind")) {
            "color" -> {
                val view = host.colorPreview?.objectOrNull("view") ?: host.snapshot!!.getJSONObject("color_panel")
                val color = record.packet.objectOrNull("color")
                val bounds = color?.getJSONObject("bounds")?.rect()
                val side = v.number("diameter") - 14f
                val wheel = if (bounds == null) Modifier.size(side.dp) else Modifier.align(Alignment.TopStart)
                    .offset((-bounds.left - side / 2).dp, (-bounds.top - side / 2).dp).size(side.dp)
                ColorWheel(host, view, (host.colorPreview?.objectOrNull("colors") ?: host.snapshot!!.getJSONObject("state").displayColors()).toString(), host.colorPreview != null,
                    wheel, interactive = false) {}
                if (color != null) ColorIntensityArc(view, JSONObject(), Modifier.matchParentSize(),
                    { action -> host.bubble(obj("type" to "set_intensity", "id" to record.id, "stops" to action.number("stops"))) },
                    caption = false, presentation = color.getJSONObject("intensity"), origin = -bounds!!.topLeft)
            }
            "history" -> v.array("buttons").objects().forEachIndexed { i, button ->
                val x = record.packet.array("history").getJSONArray(i).bubblePoint().x
                Box(Modifier.offset(x = x.dp).size(56.dp), contentAlignment = Alignment.Center) {
                    if ((record.hovered as? JSONObject)?.optInt("half", -1) == i) Box(Modifier.fillMaxSize().background(colors.text.copy(alpha = if (record.pressed) .14f else .07f), CircleShape))
                    SharedIcon(button.getString("icon"), button.getString("label"), Modifier.size(20.dp).graphicsLayer { alpha = if (button.getBoolean("enabled")) 1f else .35f })
                }
            }
            "layers" -> {
                val state = host.snapshot!!.getJSONObject("state")
                val layer = state.array("layers").objects().firstOrNull { it.optLong("id") == v.optLong("layer", -1) }
                val images = rememberLayerThumbnails(host, state.getJSONObject("document_file").optLong("epoch")) {
                    if (layer?.optBoolean("has_thumbnail") == true) listOf(LayerThumbnailTarget("${layer.getLong("id")}:false", layer.getLong("id"), layer.getLong("paint_revision"))) else emptyList()
                }
                images["${v.optLong("layer")}:false"]?.let { Image(it, null, Modifier.size(46.dp).drawWithCache {
                    val outline = SquircleShape(13.dp).createOutline(size, layoutDirection, this)
                    onDrawBehind { drawOutline(outline, Color.Gray.copy(alpha = .28f), style = Stroke(2.dp.toPx())) }
                }.clip(SquircleShape(13.dp)).background(Color.White), contentScale = androidx.compose.ui.layout.ContentScale.Crop) }
                    ?: SharedIcon(v.getString("icon"), null, Modifier.size(20.dp))
            }
            else -> {
                if (!v.isNull("preview")) BrushPreview(host, v.getInt("preview"), Modifier.offset(y = 11.dp).size(32.dp, 8.dp).graphicsLayer { alpha = .78f })
                SharedIcon(v.getString("icon"), null, Modifier.offset(y = if (!v.isNull("preview")) (-6).dp else if (v.getString("kind") == "medium") (-4).dp else 0.dp).size(20.dp))
                if (v.optBoolean("variants")) ToolGroupIndicator(Modifier.align(Alignment.BottomEnd).padding(9.dp).size(16.dp))
                Canvas(Modifier.fillMaxSize()) {
                    record.packet.array("dots").values().forEachIndexed { i, value ->
                        drawCircle(colors.text.copy(alpha = if (v.array("dots").optBoolean(i)) .9f else .3f), 1.75.dp.toPx(), center + (value as JSONArray).bubblePoint() * density)
                    }
                }
            }
        }
    }
}

@Composable private fun BubbleHalo(record: BubbleRecord, part: BubblePart) {
    val colors = LocalPalette.current
    val v = part.data
    val geometry = part.data.array("_geometry")
    val origin = LocalBubbleOrigin.current
    val disabled = remember { Paint().apply { alpha = .4f } }
    Box(Modifier.fillMaxSize().testTag("bubble-halo-${record.id}").graphicsLayer { alpha = part.appearance.value; scaleX = part.scale; scaleY = part.scale }) {
        Canvas(Modifier.fillMaxSize()) {
            translate(origin.x * density, origin.y * density) {
            v.array("rings").objects().forEachIndexed { i, ring ->
                val arc = ring.getJSONObject("arc"); val radius = arc.number("radius") * density
                val iconSpan = 21f / arc.number("radius") * 180f / PI.toFloat() * sign(arc.number("sweep"))
                val enabled = ring.getBoolean("enabled")
                val origin = Offset(-radius, -radius); val extent = Size(radius * 2, radius * 2)
                if (!enabled) drawContext.canvas.saveLayer(Rect(origin, extent).inflate(arc.number("band") * density), disabled)
                drawArc(colors.panelFill, arc.number("start") - iconSpan, arc.number("sweep") + iconSpan, false, origin, extent, style = Stroke(arc.number("band") * density, cap = StrokeCap.Round))
                drawArc(colors.text.copy(alpha = .18f), arc.number("start"), arc.number("sweep"), false, origin, extent, style = Stroke(4.dp.toPx(), cap = StrokeCap.Round))
                if (ring.number("fill") > 0f) drawArc(colors.text.copy(alpha = .5f), arc.number("start"), arc.number("sweep") * ring.number("fill"), false, origin, extent, style = Stroke(4.dp.toPx(), cap = StrokeCap.Round))
                val thumb = geometry.optJSONObject(i)?.array("thumb")?.bubblePoint() ?: Offset.Zero
                drawCircle(colors.thumb, (if (ring.getBoolean("active")) 7.5f else 7f) * density, thumb * density)
                drawCircle(colors.text.copy(alpha = .6f), (if (ring.getBoolean("active")) 7.5f else 7f) * density, thumb * density, style = Stroke(1.dp.toPx()))
                if (!enabled) drawContext.canvas.restore()
            }
            }
        }
        v.array("rings").objects().forEachIndexed { i, ring ->
            geometry.optJSONObject(i)?.array("icon")?.bubblePoint()?.let { center ->
                SharedIcon(ring.getString("icon"), ring.getString("label"), Modifier.offset((origin.x + center.x - 8f).dp, (origin.y + center.y - 8f).dp).size(16.dp).testTag("bubble-ring-icon-${record.id}-$i").graphicsLayer { alpha = if (ring.getBoolean("enabled")) 1f else .4f })
            }
        }
    }
}

@Composable private fun BubbleStamp(host: CanvasHost, record: BubbleRecord, density: Float) {
    val placement = record.view.objectOrNull("stamp") ?: return
    val stamp = rememberToolbarStamp(host, obj("type" to "bubble_stamp"), true)
    stamp?.let { (data, image) ->
        val diameter = placement.number("diameter") * data.number("extent")
        val at = record.center + placement.array("center").bubblePoint()
        Image(image, null, Modifier.offset { IntOffset(((at.x - diameter / 2) * density).roundToInt(), ((at.y - diameter / 2) * density).roundToInt()) }
            .size(diameter.dp).zIndex(699f), colorFilter = ColorFilter.tint(record.packet.array("stamp_color").color()), contentScale = androidx.compose.ui.layout.ContentScale.FillBounds)
    }
}
