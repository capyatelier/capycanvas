package art.capycanvas

import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import org.json.JSONArray
import org.json.JSONObject

internal val LocalWorkspaceZ = staticCompositionLocalOf { 0 }

/** Native hit geometry and gesture capture only. Rust owns movement, tear-off,
 * docking, sizing, undo transactions and Zen visibility on every platform. */
internal class DockInteraction(val host: CanvasHost) {
    data class Region(val action: JSONObject, val bounds: Rect, val z: Int, val priority: Int, val context: JSONObject?)
    val regions = mutableMapOf<Any, Region>()
    val anchors = mutableMapOf<String, Rect>()
    val tabs = mutableMapOf<String, JSONObject>()
    var origin = Offset.Zero
    var density = 1f
    var viewport = JSONArray(listOf(1, 1))
    var enabled = true
    var hint by mutableStateOf<JSONObject?>(null)
    var dragging by mutableStateOf(false)
        private set
    var expansion by mutableStateOf<JSONObject?>(null)
    var configurationHeight by mutableFloatStateOf(0f)
    var contextMenu by mutableStateOf<JSONObject?>(null)
    var contextAnchor = Rect.Zero
    var popupOpen = false
    private var active: JSONObject? = null
    private var position = Offset.Zero
    private var generation = 0
    private val measurements = mutableMapOf<String, Pair<Float, Float>>()

    fun measure(panel: String, tabWidth: Float? = null, contentHeight: Float? = null) {
        val old = measurements[panel] ?: (0f to 0f)
        val next = (tabWidth ?: old.first) to (contentHeight ?: old.second)
        val accepted = host.snapshot?.array("panel_measurements")?.objects()?.find { it.getString("panel") == panel }
        if (next == old && accepted?.number("tab_width") == next.first && accepted.number("content_height") == next.second) return
        measurements[panel] = next
        host.dispatch(obj("type" to "measure_panels", "measurements" to JSONArray(measurements.map { (id, size) ->
            obj("panel" to id, "tab_width" to size.first, "content_height" to size.second)
        })))
    }

    // Shared workspace gestures have their own Zen state in Rust. Only tile
    // reordering uses the host's generic dragging fact.
    fun facts() = obj("held" to false, "dragging" to (active?.optString("type") == "tile_drag"),
        "popup_open" to (popupOpen || contextMenu != null), "expanded_panel" to expansion)
    fun refresh() = host.chrome(obj("kind" to "refresh"), facts())
    fun anchorKey(item: JSONObject) = "${item.optString("kind").replace("ribbon", "panel")}:${item.optString("group")}:${item.optString("panel")}:${item.optString("tile")}"
    fun context(target: JSONObject) {
        if (dragging) return
        val anchor = anchors[anchorKey(target)] ?: return
        val request = generation
        host.query(obj("type" to "context", "target" to target)) {
            if (!dragging && request == generation) { contextAnchor = anchor; contextMenu = it as? JSONObject; refresh() }
        }
    }
    fun closeContext() { contextMenu = null; refresh() }
    fun doubleClickHandle(item: JSONObject) {
        host.query(obj("type" to "panel_handle_target", "item" to item)) { group ->
            if (group is Number) host.dispatch(obj("type" to "double_click_panel_handle", "group" to group, "viewport" to viewport))
        }
    }
    fun hit(point: Offset): Region? = if (!enabled || popupOpen || contextMenu != null) null else regions.values
        .filter { it.bounds.contains(point) }.maxWithOrNull(compareBy<Region> { it.z }.thenBy { it.priority })
    private fun send(phase: String) {
        val action = active ?: return
        if (action.getString("type") == "tile_drag") return
        host.dispatch(JSONObject(action.toString()).put("phase", phase).put("viewport", viewport)
            .put("position", JSONArray(listOf(position.x, position.y))).apply {
                if (action.getString("type") == "drag_workspace") put("tabs", JSONArray(tabs.values.toList()))
            })
    }
    private fun query() = obj("type" to "drop", "item" to active?.optJSONObject("item"),
        "position" to JSONArray(listOf(position.x, position.y)), "tabs" to JSONArray(tabs.values.toList()), "expansion" to expansion)
    fun start(action: JSONObject, point: Offset) {
        active = action; position = point; dragging = true; generation++
        refresh(); send("down")
    }
    fun move(point: Offset) {
        position = point; send("move")
        if (active?.optString("type") == "tile_drag") host.chrome(obj("kind" to "motion",
            "position" to JSONArray(listOf(point.x, point.y))), facts())
        if (active?.optJSONObject("item") == null) return
        val request = ++generation
        host.query(query()) { if (request == generation) hint = it as? JSONObject }
    }
    fun finish(cancel: Boolean) {
        if (!dragging) return
        val action = active!!
        val request = ++generation
        fun end() { active = null; hint = null; dragging = false; refresh() }
        if (action.getString("type") == "tile_drag" && !cancel) {
            host.query(query()) { result ->
                if (request == generation) {
                    (result as? JSONObject)?.optJSONObject("target")?.let { target ->
                        val item = action.getJSONObject("item")
                        host.dispatch(obj("type" to "move_tile", "panel" to item.getString("panel"),
                            "tile" to item.getInt("tile"), "target" to target, "viewport" to viewport))
                    }
                    end()
                }
            }
        } else { send(if (cancel) "cancel" else "up"); end() }
    }
}

@Composable internal fun Modifier.workspaceSource(dock: DockInteraction, action: JSONObject,
    priority: Int = 0, anchor: JSONObject? = null): Modifier {
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    val key = anchor?.let(dock::anchorKey)
    DisposableEffect(dock, token, key) {
        onDispose { dock.regions.remove(token); if (key != null) dock.anchors.remove(key) }
    }
    SideEffect { dock.regions[token]?.let { dock.regions[token] = it.copy(action = action, z = z, priority = priority, context = anchor) } }
    return onGloballyPositioned {
        val bounds = it.boundsInRoot().translate(-dock.origin)
        dock.regions[token] = DockInteraction.Region(action, bounds, z, priority, anchor)
        if (key != null) dock.anchors[key] = bounds
    }
}

@Composable internal fun Modifier.dragSource(dock: DockInteraction, item: JSONObject, context: JSONObject = item): Modifier {
    val kind = item.getString("kind")
    return workspaceSource(dock, obj("type" to if (kind == "tile") "tile_drag" else "drag_workspace", "item" to item),
        priority = when (kind) { "tile" -> 3; "panel" -> 2; else -> 1 }, anchor = context)
}

/** Capture belongs to the stable workspace, not a tab or ribbon that Rust may
 * reparent during tear-off. Child clicks keep native timing and long-press. */
internal fun Modifier.workspaceGestures(dock: DockInteraction): Modifier = pointerInput(dock) {
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        val source = dock.hit(down.position)
        if (source != null) {
            // Invisible resize strips have no child button to consume their
            // press. Do not let the underlying SurfaceView begin a stroke/pan.
            if (source.action.getString("type") !in listOf("drag_workspace", "tile_drag")) down.consume()
            if (currentEvent.buttons.isSecondaryPressed) {
                source.context?.let(dock::context)
                down.consume()
                return@awaitEachGesture
            }
            var started = false
            var released = false
            try {
                do {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (!started && (dock.popupOpen || dock.contextMenu != null || !dock.enabled)) break
                    if (!started && change.pressed && (change.position - down.position).getDistance() > viewConfiguration.touchSlop) {
                        dock.start(source.action, down.position / dock.density); started = true
                    }
                    if (started) {
                        change.consume()
                        dock.move(change.position / dock.density)
                    }
                    if (!change.pressed) { if (started) dock.finish(false); released = true; break }
                } while (true)
            } finally { if (started && !released) dock.finish(true) }
        }
    }
}
