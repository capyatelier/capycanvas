package art.capycanvas

import android.view.PointerIcon as AndroidPointerIcon
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.stylusHoverIcon
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import org.json.JSONArray
import org.json.JSONObject

internal val LocalWorkspaceZ = staticCompositionLocalOf { 0 }

/** System icons render at the device's cursor size for both mouse and pen. */
@Composable internal fun Modifier.workspacePointerIcon(type: Int?): Modifier {
    if (type == null) return this
    val icon = remember(type) { PointerIcon(type) }
    return pointerHoverIcon(icon).stylusHoverIcon(icon)
}

@Composable internal fun Modifier.workspaceDragCursor(type: Int?): Modifier {
    val icon = remember(type) { PointerIcon(type ?: AndroidPointerIcon.TYPE_ARROW) }
    // Keep the mouse hover node installed before contact, so it owns the
    // pointer when a resize/drag changes the icon without a new hover-enter.
    val mouse = pointerHoverIcon(icon, overrideDescendants = type != null)
    return if (type == null) mouse else mouse.stylusHoverIcon(icon, overrideDescendants = true)
}

internal fun resizePointerIcon(edge: String) = when (edge) {
    "left", "right" -> AndroidPointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW
    "top", "bottom" -> AndroidPointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
    "top_left", "bottom_right" -> AndroidPointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
    "top_right", "bottom_left" -> AndroidPointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
    else -> AndroidPointerIcon.TYPE_ARROW
}

/** Native hit geometry and gesture capture only. Rust owns movement, tear-off,
 * docking, sizing, undo transactions and Zen visibility on every platform. */
internal class DockInteraction(val host: CanvasHost) {
    data class Region(val action: JSONObject, val bounds: Rect, val z: Int, val priority: Int, val context: JSONObject?, val cursor: Int)
    val regions = mutableMapOf<Any, Region>()
    val chromeRegions = mutableMapOf<Any, Rect>()
    var drawer: JSONObject? = null
    var drawerTileRevision by mutableIntStateOf(0)
        private set
    private val drawerTiles = mutableMapOf<String, JSONObject>()
    private val columnDrawers = mutableMapOf<Int, JSONObject>()
    val anchors = mutableMapOf<String, Rect>()
    val tabs = mutableMapOf<String, JSONObject>()
    var origin = Offset.Zero
    var density = 1f
    var viewport = JSONArray(listOf(1, 1))
    var enabled = true
    var hint by mutableStateOf<JSONObject?>(null)
    var dragging by mutableStateOf(false)
        private set
    var dragCursor by mutableStateOf<Int?>(null)
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

    fun measureColumnDrawer(column: Int, group: Int?, bounds: Rect?) {
        val next = bounds?.takeIf { group != null && it.width > 0f && it.height > 0f }?.let {
            obj("group" to group, "bounds" to obj("x" to it.left / density, "y" to it.top / density,
                "width" to it.width / density, "height" to it.height / density))
        }
        if (columnDrawers[column]?.toString() == next?.toString()) return
        if (next == null) columnDrawers.remove(column) else columnDrawers[column] = next
        publishColumnDrawers()
    }
    private fun publishColumnDrawers() {
        host.dispatch(obj("type" to "measure_column_drawers", "measurements" to JSONArray(columnDrawers.values.toList())))
    }

    fun clearDrawerTiles(column: Int) {
        if (drawerTiles.entries.removeAll { it.value.getInt("column") == column }) publishDrawerTiles()
    }
    fun measureDrawerTile(column: Int, panel: String, tile: Int, bounds: Rect?) {
        val key = "$column:$panel:$tile"
        val next = bounds?.takeIf { it.width > 0f && it.height > 0f }?.let {
            obj("column" to column, "anchor" to obj("panel" to panel, "tile" to tile), "bounds" to
                obj("x" to it.left / density, "y" to it.top / density, "width" to it.width / density, "height" to it.height / density))
        }
        if (drawerTiles[key]?.toString() == next?.toString()) return
        if (next == null) drawerTiles.remove(key) else drawerTiles[key] = next
        publishDrawerTiles()
    }
    private fun publishDrawerTiles() {
        host.dispatch(obj("type" to "measure_drawer_tiles", "measurements" to JSONArray(drawerTiles.values.toList())))
        drawerTileRevision++
    }
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
        "popup_open" to (popupOpen || contextMenu != null), "expanded_panel" to expansion, "content_drawer" to drawer?.optJSONObject("placement")?.optJSONObject("bounds"),
        "drawer_connection" to drawer?.optJSONObject("connection")?.optJSONObject("bounds"))
    fun refresh() = host.chrome(obj("kind" to "refresh"), facts())
    fun anchorKey(item: JSONObject) = "${item.optString("kind").replace("ribbon", "panel")}:${item.optString("column")}:${item.optString("group")}:${item.optString("panel")}:${item.optString("tile")}"
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
    private fun send(phase: String, preview: Boolean = false) {
        val action = active ?: return
        if (action.getString("type") == "tile_drag") return
        // Opening another tab can invalidate Rust's transient measurement even
        // when the displayed rectangle is unchanged.
        val actions = mutableListOf<JSONObject>()
        if (action.getString("type") == "drag_workspace") actions.add(obj("type" to "measure_column_drawers",
            "measurements" to JSONArray(columnDrawers.values.toList())))
        actions.add(JSONObject(action.toString()).put("phase", phase).put("viewport", viewport)
            .put("position", JSONArray(listOf(position.x, position.y))).apply {
                if (action.getString("type") == "drag_workspace") put("tabs", JSONArray(tabs.values.toList()))
            })
        val request = generation
        host.workspaceGesture(actions, if (preview && action.optJSONObject("item") != null) query() else null) {
            // A newer pointer position must not starve completed feedback.
            // Only ending/replacing the gesture invalidates its replies.
            if (request == generation) hint = it as? JSONObject
        }
    }
    private fun query() = obj("type" to "drop", "item" to active?.optJSONObject("item"),
        "position" to JSONArray(listOf(position.x, position.y)), "tabs" to JSONArray(tabs.values.toList()), "expansion" to expansion)
    fun start(action: JSONObject, point: Offset, cursor: Int) {
        active = action; position = point; dragging = true; generation++
        dragCursor = if (action.optJSONObject("item") != null) AndroidPointerIcon.TYPE_GRABBING else cursor
        refresh(); send("down")
    }
    fun move(point: Offset) {
        position = point; send("move", preview = true)
        if (active?.optString("type") == "tile_drag") {
            host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(point.x, point.y))), facts())
            val request = generation
            host.query(query()) { if (request == generation) hint = it as? JSONObject }
        }
    }
    fun finish(cancel: Boolean) {
        if (!dragging) return
        val action = active!!
        val request = ++generation
        fun end() { active = null; hint = null; dragging = false; dragCursor = null; refresh() }
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
    priority: Int = 0, anchor: JSONObject? = null, cursor: Int = AndroidPointerIcon.TYPE_GRAB): Modifier {
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    val key = anchor?.let(dock::anchorKey)
    DisposableEffect(dock, token, key) {
        onDispose { dock.regions.remove(token); if (key != null) dock.anchors.remove(key) }
    }
    SideEffect { dock.regions[token]?.let { dock.regions[token] = it.copy(action = action, z = z, priority = priority, context = anchor, cursor = cursor) } }
    return workspacePointerIcon(if (dock.enabled) cursor else null).onGloballyPositioned {
        val bounds = it.boundsInRoot().translate(-dock.origin)
        dock.regions[token] = DockInteraction.Region(action, bounds, z, priority, anchor, cursor)
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
        if (dock.chromeRegions.values.any { it.contains(down.position) }) {
            val tab = dock.tabs.values.firstOrNull { it.getJSONObject("bounds").let { b ->
                Rect(b.number("x"), b.number("y"), b.number("x") + b.number("width"), b.number("y") + b.number("height")).contains(down.position / dock.density)
            } }?.optString("panel")
            dock.host.chrome(obj("kind" to "contact", "canvas" to false,
                "position" to JSONArray(listOf(down.position.x / dock.density, down.position.y / dock.density))), dock.facts().put("contact_tab", tab))
        }
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
                    // Compose represents ACTION_CANCEL as an already-consumed
                    // release. Preserve the shared transaction before consuming it.
                    if (!change.pressed && change.isConsumed) break
                    if (!started && (dock.popupOpen || dock.contextMenu != null || !dock.enabled)) break
                    if (!started && change.pressed && (change.position - down.position).getDistance() > viewConfiguration.touchSlop) {
                        dock.start(source.action, down.position / dock.density, source.cursor); started = true
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


/** Only UI-covered contacts are sent here; CanvasHost gates bare canvas contacts. */
@Composable internal fun Modifier.chromeRegion(dock: DockInteraction): Modifier {
    val token = remember { Any() }
    DisposableEffect(dock, token) { onDispose { dock.chromeRegions.remove(token) } }
    return onGloballyPositioned { dock.chromeRegions[token] = it.boundsInRoot().translate(-dock.origin) }
}
@Composable internal fun Modifier.contextAnchor(dock: DockInteraction, target: JSONObject): Modifier {
    val key = dock.anchorKey(target)
    DisposableEffect(dock, key) { onDispose { dock.anchors.remove(key) } }
    return onGloballyPositioned { dock.anchors[key] = it.boundsInRoot().translate(-dock.origin) }
}
internal val LocalDrawerColumn = staticCompositionLocalOf<Int?> { null }
internal val LocalDrawerClip = staticCompositionLocalOf { Rect.Zero }

@Composable internal fun Modifier.columnDrawerBounds(dock: DockInteraction, column: Int, group: Int?): Modifier {
    var bounds by remember { mutableStateOf(Rect.Zero) }
    SideEffect { dock.measureColumnDrawer(column, group, bounds) }
    DisposableEffect(dock, column) { onDispose { dock.measureColumnDrawer(column, null, null) } }
    return onGloballyPositioned { bounds = it.boundsInRoot().translate(-dock.origin) }
}

@Composable internal fun Modifier.drawerTabHit(dock: DockInteraction, column: Int, group: Int, index: Int,
    panel: String, clip: Rect, active: Boolean): Modifier {
    val key = "drawer:$column:$index"
    var bounds by remember { mutableStateOf(Rect.Zero) }
    SideEffect {
        val r = bounds.intersect(clip)
        if (active && r.width > 0f && r.height > 0f) {
            dock.tabs[key] = obj("group" to group, "index" to index, "panel" to panel,
                "bounds" to obj("x" to r.left / dock.density, "y" to r.top / dock.density,
                    "width" to r.width / dock.density, "height" to r.height / dock.density))
        } else dock.tabs.remove(key)
    }
    DisposableEffect(dock, key) { onDispose { dock.tabs.remove(key) } }
    return onGloballyPositioned { bounds = it.boundsInRoot().translate(-dock.origin) }
}

@Composable internal fun Modifier.drawerTile(dock: DockInteraction, panel: String, tile: Int): Modifier {
    val column = LocalDrawerColumn.current ?: return this
    val clip = LocalDrawerClip.current
    var bounds by remember { mutableStateOf(Rect.Zero) }
    SideEffect { dock.measureDrawerTile(column, panel, tile, bounds.intersect(clip)) }
    DisposableEffect(column, panel, tile) { onDispose { dock.measureDrawerTile(column, panel, tile, null) } }
    return onGloballyPositioned { bounds = it.boundsInRoot().translate(-dock.origin) }
}
