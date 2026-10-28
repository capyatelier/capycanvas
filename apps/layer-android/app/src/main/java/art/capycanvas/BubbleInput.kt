package art.capycanvas

import android.view.KeyEvent
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.*
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalWindowInfo
import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

internal class BubbleInteraction(val host: CanvasHost, val dock: DockInteraction, val header: HeaderInteraction, private val scope: CoroutineScope) {
    val records = mutableStateMapOf<Int, BubbleRecord>()
    var active by mutableStateOf(false)
    var wakeVersion = 0L
        private set
    var ownersVersion = 0L
        private set
    private val wakes = Channel<Unit>(Channel.CONFLATED)
    fun wake() { wakeVersion++; active = true; wakes.trySend(Unit) }
    suspend fun awaitWake() { wakes.receive() }
    var menuModel by mutableStateOf<JSONObject?>(null)
    var menuRequest: JSONObject? = null
    var menuPoint = Offset.Zero
    private var menuGeneration = 0
    private var menuOwner: Pair<Int, String>? = null
    private var epoch: Long? = null
    private var presented: JSONArray? = null
    private var hovering = false
    private var stroke = false
    private var restoreAt: Long? = null
    var holdMillis = 500L
    var doubleTapMillis = 300L
    var doubleTapDistance = 8f
    private var lastRing: Triple<Int, Long, Offset>? = null
    private var lastRingKind: Any? = null
    data class Source(val grab: JSONObject, val bounds: Rect)
    val choices = mutableMapOf<Any, Source>()
    val ownsAllTouches get() = contact?.paired == true
    private class Candidate(val pointer: PointerId, val bounds: Rect, val start: Offset, val valid: () -> Boolean, val grab: JSONObject) {
        var moving = false
        var last = start
    }
    private class Contact(val record: BubbleRecord, val hit: Any, val id: PointerId, val device: PointerType, val start: Offset) {
        var mode = "pending"
        var last = start
        var held = false
        var hold: Job? = null
        var colorStarted = false
        var wheel: String? = null
        var grab: JSONObject? = null
        val fingers = linkedMapOf(id to start)
        var paired = false
        var pairLifted = false
        var pairEnded = false
        var pairOrigin = start
        var spread = 1f
        var scale = 1f
        var midpoint = start
    }
    private var candidate: Candidate? = null
    private var contact: Contact? = null
    private val colorTouches = mutableMapOf<PointerId, Pair<Long, Offset>>()
    fun send(action: JSONObject) { host.bubble(action); wake() }
    private fun drag(grab: JSONObject, phase: String, point: Offset) {
        send(obj("type" to "drag", "grab" to grab, "phase" to phase, "position" to point.headerPoint(), "viewport" to dock.viewport))
    }
    fun refresh() {
        val packets = host.snapshot?.optJSONArray("bubbles")
        val nextEpoch = host.menuEpoch()
        if (packets === presented && epoch == nextEpoch) return
        presented = packets
        val alive = packets?.objects().orEmpty().associateBy { it.getJSONObject("view").getInt("id") }
        if (epoch != nextEpoch || records.size != alive.size || records.any { (id, record) ->
            alive[id]?.getJSONObject("view")?.getJSONObject("source")?.toString() != record.view.getJSONObject("source").toString()
        }) { ownersVersion++; lastRing = null }
        if (menuOwner?.let { (id, source) -> alive[id]?.getJSONObject("view")?.getJSONObject("source")?.toString() != source } == true) dismiss()
        if (epoch != null && epoch != nextEpoch || contact?.let { c ->
            alive[c.record.id]?.let { it.getJSONObject("view").getJSONObject("source").toString() != c.record.view.getJSONObject("source").toString() }
                ?: (c.mode != "moving" && !c.pairLifted)
        } == true) abandon()
        epoch = nextEpoch
        records.keys.removeAll { it !in alive }
        for ((order, entry) in alive.entries.withIndex()) {
            val (id, packet) = entry
            val record = records.getOrPut(id) { BubbleRecord(packet) }
            record.order = order
            if (record.parts.isEmpty() || record.packet.toString() != packet.toString()) record.update(packet, contact?.let { it.record.id != id || it.mode != "beads" || (it.start - it.last).getDistance() <= 8f } != false)
        }
        wake()
    }
    fun step(motion: JSONObject, now: Long, reduced: Boolean, requestedWake: Long) {
        active = requestedWake != wakeVersion || motion.getBoolean("moving") || contact != null || candidate?.moving == true
        val fade = motion.array("fade")
        val down = fade.getBoolean(0)
        active = active || down
        val faded = fade.getJSONArray(1).values().map { (it as Number).toInt() }.toSet()
        if (down) {
            restoreAt = null
            records.values.forEach { record -> record.faded = record.id in faded; record.fade.to(if (record.faded) .2f else 1f, 140) }
        } else if (stroke) restoreAt = now + 180
        stroke = down
        restoreAt?.let { if (now >= it) { restoreAt = null; records.values.forEach { record -> record.faded = false; record.fade.to(1f, 140) } } else active = true }
        motion.array("placements").objects().forEach { placement ->
            records[placement.getInt("id")]?.let { record ->
                record.center = placement.array("center").bubblePoint(); record.lifted = placement.getBoolean("lifted"); record.homing = placement.getBoolean("homing")
            }
        }
        records.values.forEach { active = it.step(now, reduced) || active }
        glass()
    }
    private fun glass() {
        val boxes = mutableListOf<FloatArray>(); val arcs = mutableListOf<FloatArray>(); val colors = mutableListOf<FloatArray>(); val density = dock.density
        for (r in records.values) {
            val center = r.center * density + dock.origin
            val scale = r.scale * density
            for (p in r.parts.values) {
                if (p.key.startsWith("tag:") && (r.parts["item:${p.key.substringAfter(':')}"]?.appearance?.value != 1f || !p.data.optBoolean("shown") && (r.hovered as? JSONObject)?.optInt("item", -1) != p.key.substringAfter(':').toInt())) continue
                val strength = r.alpha * p.appearance.value * p.strength
                if (p.key == "halo") {
                    p.data.array("rings").objects().forEach { ring ->
                        val arc = ring.getJSONObject("arc"); val radius = arc.number("radius")
                        val sign = sign(arc.number("sweep")); val icon = 21f / radius * sign
                        arcs.add(floatArrayOf(center.x, center.y, radius * scale * p.scale, arc.number("band") * scale * p.scale,
                            arc.number("start") * PI.toFloat() / 180f - icon, arc.number("sweep") * PI.toFloat() / 180f + icon, strength * if (ring.getBoolean("enabled")) 1f else .4f))
                    }
                } else if (p.glass && strength > 0f) {
                    val color = r.packet.objectOrNull("color").takeIf { p.key == "face" }
                    if (color != null) {
                        val g = color.array("glass"); val s = scale * p.scale
                        colors.add(floatArrayOf(center.x, center.y, g.getDouble(0).toFloat() * s, g.getDouble(1).toFloat() * s, g.getDouble(2).toFloat() * s, g.getDouble(3).toFloat(), strength))
                        continue
                    }
                    val side = p.size * (scale * p.scale)
                    val at = center + p.center * scale - Offset(side.width / 2, side.height / 2)
                    val radius = p.radius * scale * p.scale
                    boxes.add(floatArrayOf(at.x, at.y, side.width, side.height, radius, radius, radius, radius, if (p.squircle) 1f else 0f, strength))
                }
            }
        }
        host.bubbleGlass(boxes, arcs, colors)
    }
    fun dismiss() { menuGeneration++; menuModel = null; menuRequest = null; menuOwner = null }
    fun menu(record: BubbleRecord, point: Offset, palettes: Boolean = false) {
        val ticket = ++menuGeneration
        menuOwner = record.id to record.view.getJSONObject("source").toString()
        menuPoint = point
        val request = obj("type" to "context", "target" to obj("kind" to if (palettes) "bubble_palettes" else "bubble", "id" to record.id))
        menuRequest = request
        host.query(request) { if (ticket == menuGeneration) menuModel = it as? JSONObject }
    }
    private fun target(point: Offset) = records.values.sortedWith(compareBy<BubbleRecord>({ it.lifted }, { it.order })).asReversed().firstOrNull {
        val relative = point - it.center
        val reach = it.packet.number("reach")
        !it.faded && abs(relative.x) <= reach && abs(relative.y) <= reach && it.hit(point) != null
    }
    private fun sourceView(grab: JSONObject): JSONObject? = when (grab.optString("kind")) {
        "header" -> host.snapshot?.objectOrNull("header")?.array("items")?.objects()?.firstOrNull { it.getInt("id") == grab.getInt("id") }
        "tile" -> host.snapshot?.array("panels")?.objects()?.firstOrNull { it.getString("id") == grab.getString("panel") }
            ?.array("tiles")?.objects()?.firstOrNull { it.getInt("id") == grab.getInt("tile") }
        else -> null
    }
    private fun sourceControl(grab: JSONObject) = sourceView(grab)?.objectOrNull("resolved_control")?.toString()
    private fun local(c: Contact, point: Offset) = (point - c.record.center).headerPoint()
    private fun restore(c: Contact) {
        finishColor(c, true)
        if (c.mode in listOf("ring", "beads", "choose")) send(obj("type" to c.mode, "id" to c.record.id, "phase" to "cancel", "position" to JSONArray(listOf(0, 0))))
        if (c.mode == "moving") c.grab?.let { drag(it, "cancel", c.last) }
        c.mode = "pending"
    }
    private fun pick(c: Contact, point: Offset) {
        if (c.mode != "intensity" && c.wheel == null) return
        send(obj("type" to if (c.mode == "intensity") "intensity" else "wheel", "id" to c.record.id, "phase" to if (c.colorStarted) "move" else "down", "part" to c.wheel, "position" to local(c, point)))
        c.colorStarted = true
    }
    private fun finishColor(c: Contact, cancelled: Boolean) {
        if (!c.colorStarted) return
        c.colorStarted = false
        send(obj("type" to if (c.mode == "intensity") "intensity" else "wheel", "id" to c.record.id, "phase" to if (cancelled) "cancel" else "up", "part" to c.wheel, "position" to local(c, c.last)))
    }
    private fun begin(c: Contact, point: Offset, now: Long) {
        when {
            c.hit == "wheel" -> {
                c.mode = "wheel"
                val size = c.record.view.number("diameter") - 14f
                c.wheel = org.json.JSONTokener(Native.colorWheelHit(obj("size" to size,
                    "point" to (point - c.record.center + Offset(size / 2, size / 2)).headerPoint(),
                    "shape" to host.snapshot!!.getJSONObject("color_panel").getString("shape")).toString())).nextValue() as? String
                pick(c, point)
            }
            c.hit == "ring" || c.hit == "intensity" -> {
                val repeated = lastRingKind == c.hit && lastRing?.let { it.first == c.record.id && now - it.second <= doubleTapMillis && (point - it.third).getDistance() <= doubleTapDistance } == true
                lastRing = if (repeated) null else Triple(c.record.id, now, point); lastRingKind = c.hit
                c.mode = if (repeated) "reset" else c.hit as String
                if (c.hit == "intensity") {
                    if (repeated) send(obj("type" to "set_intensity", "id" to c.record.id, "stops" to 0f)) else pick(c, point)
                } else send(obj("type" to if (repeated) "reset_ring" else "ring", "id" to c.record.id, "position" to local(c, point)).apply { if (!repeated) put("phase", "down") })
            }
            c.hit == "bead" || c.hit is JSONObject && c.hit.has("item") -> {
                c.mode = if (c.hit == "bead") "beads" else "choose"
                send(obj("type" to c.mode, "id" to c.record.id, "phase" to "down", "position" to local(c, point)))
            }
        }
    }
    private fun hold(c: Contact) {
        c.hold = scope.launch {
            delay(holdMillis * if (c.wheel != null || c.hit == "intensity") 2 else 1)
            refresh()
            if (contact === c && !c.paired) { lastRing = null; restore(c); c.held = true; menu(c.record, c.start) }
        }
    }
    fun down(change: PointerInputChange, secondary: Boolean): Boolean {
        val point = change.position / dock.density
        val current = contact
        if (current != null) {
            if (current.mode == "native" && current.record.hit(point) == "card") { current.hold?.cancel(); return false }
            if (current.device != PointerType.Touch || change.type != PointerType.Touch || (point - current.record.center).getDistance() > current.record.packet.number("reach") + 90f) return false
            current.fingers[change.id] = point
            if (current.fingers.size == 2 && !current.paired) {
                current.hold?.cancel(); lastRing = null; restore(current); dismiss()
                current.paired = true; current.mode = "pair"; current.grab = obj("kind" to "bubble", "id" to current.record.id)
                val pair = current.fingers.values.toList(); current.midpoint = (pair[0] + pair[1]) / 2f; current.spread = (pair[0] - pair[1]).getDistance().coerceAtLeast(1f)
                current.pairOrigin = current.midpoint
            }
            return true
        }
        if (menuModel != null || !dock.enabled || header.editing) return false
        val record = target(point)
        if (record != null) {
            if (secondary) { menu(record, point); return true }
            val hit = record.hit(point) ?: return false
            if (hit == "pill") return false
            val c = Contact(record, hit, change.id, change.type, point); contact = c
            if (hit == "card") { c.mode = "native"; return false }
            begin(c, point, change.uptimeMillis); hold(c)
            record.hovered = hit; record.pressed = true; wake()
            return true
        }
        if (secondary || dock.popupOpen) return false
        val choiceEntry = choices.entries.lastOrNull { it.value.bounds.contains(change.position) }
        val choice = choiceEntry?.value
        val headerSource = header.sources.values.filter { it.bounds.contains(point) && it.source.optString("kind") == "item" }.maxByOrNull { it.priority }
        val tile = dock.hit(change.position)?.takeIf { it.action?.objectOrNull("item")?.optString("kind") == "tile" }
        val grab = choice?.grab ?: headerSource?.let { obj("kind" to "header", "id" to it.source.getInt("value")) } ?: tile?.action?.getJSONObject("item") ?: return false
        if (choice == null && sourceView(grab)?.optBoolean("tears_bubble") != true) return false
        val sourceBounds = dock.drawerSources.values.lastOrNull { it.bounds.contains(point) }?.bounds
            ?: dock.chromeRegions.values.filter { it.bounds.contains(change.position) }.maxByOrNull { it.z }?.bounds?.let { Rect(it.topLeft / dock.density, it.bottomRight / dock.density) }
            ?: headerSource?.bounds ?: choice?.bounds?.let { Rect(it.topLeft / dock.density, it.bottomRight / dock.density) } ?: Rect.Zero
        val identity = sourceControl(grab)
        val valid = if (choiceEntry != null) { { choices[choiceEntry.key]?.grab?.toString() == grab.toString() } }
            else { { identity != null && sourceControl(grab) == identity } }
        candidate = Candidate(change.id, sourceBounds, point, valid, grab)
        return false
    }
    fun move(change: PointerInputChange): Boolean {
        val point = change.position / dock.density
        candidate?.takeIf { it.pointer == change.id }?.let { source ->
            if (!source.moving && !source.valid()) { candidate = null; return false }
            source.last = point
            if (source.moving) { drag(source.grab, "move", point); return true }
            if (header.context != null || dock.contextMenu != null || dock.dragging || source.bounds.contains(point)) return false
            dock.bubbleCaptured = true; dock.finish(true); header.finish(true); dock.closeContext(); header.closeMenu()
            source.moving = true; drag(source.grab, "down", point); drag(source.grab, "move", point)
            return true
        }
        val c = contact ?: return false
        if (change.id !in c.fingers) return false
        c.fingers[change.id] = point; c.last = point
        if (c.paired) {
            if (!c.pairEnded) {
                val pair = c.fingers.values.toList(); c.midpoint = (pair[0] + pair[1]) / 2f
                if (!c.pairLifted) {
                    if ((c.midpoint - c.pairOrigin).getDistance() <= 8f && abs((pair[0] - pair[1]).getDistance() - c.spread) <= 8f) return true
                    c.pairLifted = true; drag(c.grab!!, "down", c.pairOrigin)
                }
                if (c.record.view.getString("kind") == "color") {
                    val size = c.record.view.number("diameter")
                    c.scale = (bubbleUi(obj("type" to "pinched_size", "size" to size, "scale" to (pair[0] - pair[1]).getDistance() / c.spread)) as Number).toFloat() / size
                    c.record.pinch = c.scale
                }
                drag(c.grab!!, "move", c.midpoint)
            }
            return true
        }
        if (c.mode == "native") return false
        val far = (point - c.start).getDistance() > 8f
        if (far) { c.hold?.cancel(); lastRing = null }
        if (c.held) {
            if (!far) return true
            c.held = false; dismiss()
            if (c.hit == "wheel") c.mode = "wheel" else begin(c, point, change.uptimeMillis)
        }
        if (c.mode == "intensity") pick(c, point)
        else if (c.mode == "wheel") {
            if (bubbleUi(obj("type" to "color_drag_out", "size" to c.record.view.number("diameter"), "point" to local(c, point))) == true) {
                restore(c); c.grab = obj("kind" to "rim", "id" to c.record.id); drag(c.grab!!, "down", point); c.mode = "moving"
            } else pick(c, point)
        } else if (c.mode == "pending" && far) {
            c.grab = obj("kind" to "bubble", "id" to c.record.id); drag(c.grab!!, "down", c.start); c.mode = "moving"
        }
        if (c.mode == "moving") drag(c.grab!!, "move", point)
        else if (c.mode in listOf("ring", "beads", "choose")) send(obj("type" to c.mode, "id" to c.record.id, "phase" to "move", "position" to local(c, point)))
        c.record.hovered = c.record.hit(point)
        return true
    }
    fun nativeContact(change: PointerInputChange): Boolean {
        val c = contact?.takeIf { it.id == change.id && it.mode == "native" && !it.paired } ?: return false
        if (change.changedToDownIgnoreConsumed()) {
            if (!change.isConsumed) hold(c)
        } else if (change.pressed && (change.position / dock.density - c.start).getDistance() > 8f) {
            c.hold?.cancel()
            if (!change.isConsumed) {
                c.grab = obj("kind" to "bubble", "id" to c.record.id)
                drag(c.grab!!, "down", c.start); c.mode = "moving"
                drag(c.grab!!, "move", change.position / dock.density)
                return true
            }
        }
        return false
    }
    fun cardBackground(change: PointerInputChange) {
        val c = contact?.takeIf { it.id == change.id && it.mode == "native" } ?: return
        c.mode = "pending"; hold(c); change.consume()
    }
    fun up(change: PointerInputChange?, cancelled: Boolean = false): Boolean {
        var captured = false
        candidate?.takeIf { change == null || it.pointer == change.id }?.let { source ->
            if (source.moving) { drag(source.grab, if (cancelled) "cancel" else "up", change?.let { it.position / dock.density } ?: source.last); captured = true }
            candidate = null; dock.bubbleCaptured = false
        }
        val c = contact ?: return captured
        if (change != null && change.id !in c.fingers) return captured
        if (change != null && !cancelled) c.last = change.position / dock.density
        c.hold?.cancel()
        if (cancelled) lastRing = null
        if (c.mode == "native") { contact = null; return captured }
        if (c.paired) {
            if (!c.pairEnded && c.pairLifted) {
                drag(c.grab!!, if (cancelled) "cancel" else "up", c.midpoint)
                if (!cancelled && c.record.view.getString("kind") == "color" && abs(c.scale - 1f) > .01f) send(obj("type" to "resize", "id" to c.record.id, "scale" to c.scale))
            }
            c.pairEnded = true; c.record.pinch = 1f
        } else if (cancelled) { restore(c); if (c.held) dismiss() }
        else if (!c.held) {
            if (c.mode == "moving") drag(c.grab!!, "up", c.last)
            else if (c.mode in listOf("ring", "beads", "choose")) send(obj("type" to c.mode, "id" to c.record.id, "phase" to "up", "position" to local(c, c.last)))
            else if (c.mode == "pending") send(obj("type" to "tap", "id" to c.record.id, "position" to local(c, c.last)))
        }
        finishColor(c, false)
        if (change == null) c.fingers.clear() else c.fingers.remove(change.id)
        if (c.fingers.isEmpty()) { contact = null; c.record.hovered = null; c.record.pressed = false }
        wake()
        return true
    }
    fun colorTouch(change: PointerInputChange) {
        if (change.type != PointerType.Touch) return
        val phase = when {
            change.changedToDownIgnoreConsumed() -> "down"
            change.id !in colorTouches -> return
            change.pressed -> "move"
            change.isConsumed -> "cancel"
            else -> "up"
        }
        val surface = if (phase == "down") target(change.position / dock.density)?.takeIf { it.view.getString("kind") == "color" }?.colorSurface ?: return
            else colorTouches.getValue(change.id).first
        host.colorTouch(surface, change.id, phase, change.position, change.uptimeMillis)
        if (change.pressed) colorTouches[change.id] = surface to change.position else colorTouches.remove(change.id)
    }
    private fun cancelColorTouches() {
        val now = android.os.SystemClock.uptimeMillis()
        colorTouches.forEach { (id, touch) -> host.colorTouch(touch.first, id, "cancel", touch.second, now) }
        colorTouches.clear()
    }
    private fun abandon() {
        contact?.let { it.hold?.cancel(); it.record.pinch = 1f }
        contact = null; candidate = null; dock.bubbleCaptured = false; dismiss()
    }
    fun cancel() { refresh(); up(null, true); cancelColorTouches(); clearHover() }
    fun key(event: KeyEvent): Boolean {
        if (event.keyCode != KeyEvent.KEYCODE_ESCAPE || contact == null && candidate?.moving != true) return false
        if (event.action == KeyEvent.ACTION_DOWN) cancel()
        return true
    }
    fun hover(point: Offset): Boolean {
        val at = point / dock.density
        val target = target(at)
        if (target != null && !hovering) host.input(obj("type" to "cursor_leave"))
        hovering = target != null
        records.values.forEach {
            val next = if (it === target) it.hit(at) else null
            if (it.hovered?.toString() != next?.toString()) { it.hovered = next; wake() }
        }
        return target != null && target.hit(at) !in listOf("card", "pill")
    }
    fun clearHover(): Boolean {
        hovering = false
        records.values.forEach { if (it.hovered != null) { it.hovered = null; wake() } }
        return false
    }
    fun takeTile(item: JSONObject, position: Offset, valid: () -> Boolean, done: () -> Unit) {
        val source = sourceControl(item)
        val sourceEpoch = host.menuEpoch()
        fun current() = valid() && source != null && sourceEpoch == host.menuEpoch() && sourceControl(item) == source
        host.query(obj("type" to "bubble_grab", "grab" to item, "held" to true)) { raw ->
            if (!current()) { done(); return@query }
            val grab = raw as? JSONObject
            if (grab == null) done() else host.query(obj("type" to "can_take_bubble", "grab" to grab, "position" to position.headerPoint(), "viewport" to dock.viewport)) { can ->
                if (current() && can == true) { drag(grab, "down", position); drag(grab, "up", position) }
                done()
            }
        }
    }
}

@Composable internal fun Modifier.bubbleGestures(input: BubbleInteraction): Modifier {
    val focused = LocalWindowInfo.current.isWindowFocused
    val configuration = androidx.compose.ui.platform.LocalViewConfiguration.current
    val nativeConfiguration = android.view.ViewConfiguration.get(androidx.compose.ui.platform.LocalContext.current)
    SideEffect {
        input.holdMillis = configuration.longPressTimeoutMillis
        input.doubleTapMillis = configuration.doubleTapTimeoutMillis
        input.doubleTapDistance = max(8f, nativeConfiguration.scaledDoubleTapSlop / input.dock.density)
    }
    return pointerInput(input, focused) {
        if (!focused) { input.cancel(); return@pointerInput }
        try {
            awaitPointerEventScope {
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    input.refresh()
                    if (input.records.isNotEmpty() && event.changes.any { it.pressed || it.previousPressed }) input.wake()
                    event.changes.forEach { change ->
                        input.colorTouch(change)
                        val captured = when {
                            change.changedToDownIgnoreConsumed() -> input.down(change, event.buttons.isSecondaryPressed)
                            change.changedToUpIgnoreConsumed() -> input.up(change, change.isConsumed)
                            change.pressed -> input.move(change)
                            event.type == PointerEventType.Exit || change.type == PointerType.Touch -> input.clearHover()
                            else -> input.hover(change.position)
                        }
                        if (captured) change.consume()
                    }
                    if (input.ownsAllTouches) event.changes.forEach { it.consume() }
                    val final = awaitPointerEvent(PointerEventPass.Final)
                    final.changes.forEach { if (input.nativeContact(it)) it.consume() }
                }
            }
        } finally { input.cancel() }
    }
}

@Composable internal fun Modifier.bubbleChoice(action: JSONObject): Modifier {
    val dock = LocalDock.current ?: return this
    val input = dock.bubbles ?: return this
    val key = remember { Any() }
    val grab = remember(action.toString()) { bubbleUi(obj("type" to "choice", "action" to action)) as? JSONObject } ?: return this
    DisposableEffect(input, key) { onDispose { input.choices.remove(key) } }
    return onGloballyPositioned { input.choices[key] = BubbleInteraction.Source(grab, it.boundsInRoot().translate(-dock.origin)) }
}
