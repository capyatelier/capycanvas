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
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.stylusHoverIcon
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalWindowInfo
import org.json.JSONArray
import org.json.JSONObject

internal val LocalWorkspaceZ = staticCompositionLocalOf { 0 }

/** Mouse handles use system hands; pen handles keep Android's default hover.
 * Directional resize cursors remain available to either pointing device. */
@Composable internal fun Modifier.workspacePointerIcon(type: Int?): Modifier {
    if (type == null) return this
    val icon = remember(type) { PointerIcon(type) }
    val mouse = pointerHoverIcon(icon)
    return if (type == AndroidPointerIcon.TYPE_GRAB || type == AndroidPointerIcon.TYPE_GRABBING) mouse else mouse.stylusHoverIcon(icon)
}

@Composable internal fun Modifier.workspaceDragCursor(type: Int?): Modifier {
    val icon = remember(type) { PointerIcon(type ?: AndroidPointerIcon.TYPE_ARROW) }
    // Keep the mouse hover node installed before contact, so it owns the
    // pointer when a resize/drag changes the icon without a new hover-enter.
    val mouse = pointerHoverIcon(icon, overrideDescendants = type != null)
    return if (type == null || type == AndroidPointerIcon.TYPE_GRABBING || type == AndroidPointerIcon.TYPE_GRAB) mouse
        else mouse.stylusHoverIcon(icon, overrideDescendants = true)
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
    data class DrawerSource(val direction: String, val bounds: Rect)
    data class Region(val token: Any, val action: JSONObject, val bounds: Rect, val z: Int, val priority: Int,
        val context: JSONObject?, val cursor: Int, val holdToDrag: Boolean)
    val regions = mutableMapOf<Any, Region>()
    val chromeRegions = mutableMapOf<Any, Rect>()
    var drawer: JSONObject? = null
    val drawerSources = mutableStateMapOf<String, DrawerSource>()
    var drawerTileRevision by mutableIntStateOf(0)
        private set
    private val drawerTiles = mutableMapOf<String, JSONObject>()
    private val columnDrawers = mutableMapOf<Int, JSONObject>()
    val anchors = mutableMapOf<String, Rect>()
    val tabs = mutableMapOf<String, JSONObject>()
    val tabSlots = mutableMapOf<String, JSONObject>()
    val tabClips = mutableMapOf<Int, Rect>()
    private var frozenTabs = emptyList<JSONObject>()
    private var frozenTabGroup: Int? = null
    private var frozenTabClip: Rect? = null
    private var frozenPanel: String? = null
    var origin = Offset.Zero
    var density = 1f
    var viewport = JSONArray(listOf(1, 1))
    var enabled = true
    var focused = true
    var hint by mutableStateOf<JSONObject?>(null)
    var dragging by mutableStateOf(false)
        private set
    var dragCursor by mutableStateOf<Int?>(null)
        private set
    fun pickupCursor(armed: Boolean) {
        if (!dragging) dragCursor = if (armed) AndroidPointerIcon.TYPE_GRAB else null
    }
    var expansion by mutableStateOf<JSONObject?>(null)
    var configurationHeight by mutableFloatStateOf(0f)
    var contextMenu by mutableStateOf<JSONObject?>(null)
    var contactHeld by mutableStateOf(false)
    var contactType: PointerType? = null
    var contactSource: Any? = null
    var retiredContext: String? = null
    var contextAnchor = Rect.Zero
    var popupOpen = false
    private var active: JSONObject? = null
    private var position = Offset.Zero
    private var generation = 0
    private var contextTarget: String? = null
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
        if (dragging || !focused) return
        val key = anchorKey(target)
        if (contactHeld && retiredContext == key) return
        val anchor = anchors[key] ?: return
        if (contextTarget == key) return
        contextTarget = key
        val request = generation
        host.query(obj("type" to "context", "target" to target)) {
            if (!dragging && request == generation) { contextAnchor = anchor; contextMenu = it as? JSONObject; refresh() }
        }
    }
    fun holdContext(target: JSONObject) {
        if (contactType != PointerType.Mouse) context(target)
    }
    fun closeContext() { generation++; contextTarget = null; contextMenu = null; refresh() }
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
                if (action.getString("type") == "drag_workspace") put("tabs", JSONArray(tabHits()))
            })
        if (phase == "down") frozenTabClip?.let { clip ->
            actions.add(obj("type" to "begin_tab_drag", "tabs" to JSONArray(frozenTabs), "clip" to
                obj("x" to clip.left, "y" to clip.top, "width" to clip.width, "height" to clip.height)))
        }
        val request = generation
        host.workspaceGesture(actions, if (preview && action.optJSONObject("item") != null) query() else null, moving = phase == "move") {
            // A newer pointer position must not starve completed feedback.
            // Only ending/replacing the gesture invalidates its replies.
            if (request == generation) hint = it as? JSONObject
        }
    }
    private fun tabHits(): List<JSONObject> {
        // Frozen slots belong to the attached preview. After tear-off the source
        // group has different members/widths and must use its displayed slots.
        if (host.workspaceGeometry?.group != null) return tabs.values.toList()
        return tabs.values.filter { it.optInt("group") != frozenTabGroup } + frozenTabs
    }
    fun isDraggedTab(panel: String) = dragging && frozenPanel == panel
    private fun query() = obj("type" to "drop", "item" to active?.optJSONObject("item"),
        "position" to JSONArray(listOf(position.x, position.y)), "tabs" to JSONArray(tabHits()), "expansion" to expansion)
    fun start(action: JSONObject, point: Offset, cursor: Int) {
        host.beginWorkspaceGesture()
        active = action; position = point; dragging = true; generation++
        contextMenu = null; contextTarget = null
        val panel = action.optJSONObject("item")?.takeIf { it.optString("kind") == "panel" }?.optString("panel")
        // An open drawer also publishes tabs for its collapsed icons. Freeze
        // slots only when the actual press came from the tab, not the icon/grip.
        val tab = tabs.values.firstOrNull { it.optString("panel") == panel && it.getJSONObject("bounds").rect().contains(point) }
        frozenPanel = tab?.optString("panel")
        frozenTabGroup = tab?.getInt("group")
        frozenTabs = tabSlots.values.filter { it.optInt("group") == frozenTabGroup }.map { JSONObject(it.toString()) }
        frozenTabClip = frozenTabGroup?.let { tabClips[it] }?.let { Rect(it.left / density, it.top / density, it.right / density, it.bottom / density) }
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
        fun end() { active = null; hint = null; dragging = false; dragCursor = null; frozenTabs = emptyList(); frozenTabGroup = null; frozenTabClip = null; frozenPanel = null; refresh() }
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
    priority: Int = 0, anchor: JSONObject? = null, cursor: Int = AndroidPointerIcon.TYPE_GRAB,
    holdToDrag: Boolean = false): Modifier {
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    val key = anchor?.let(dock::anchorKey)
    DisposableEffect(dock, token, key) {
        onDispose {
            dock.regions.remove(token)
            if (key != null) dock.anchors.remove(key)
            if (dock.contactSource == token && !dock.dragging) {
                dock.retiredContext = key
                dock.pickupCursor(false)
                dock.closeContext()
            }
        }
    }
    SideEffect { dock.regions[token]?.let {
        if (dock.contactSource == token && it.action.toString() != action.toString()) dock.pickupCursor(false)
        dock.regions[token] = it.copy(action = action, z = z, priority = priority, context = anchor, cursor = cursor, holdToDrag = holdToDrag)
    } }
    // Held tiles use the normal mouse pointer until pickup is armed. Keep
    // Android's existing default pen hover; grips retain their immediate hand.
    val feedback = if (holdToDrag) pointerHoverIcon(PointerIcon(AndroidPointerIcon.TYPE_ARROW))
        else workspacePointerIcon(if (dock.enabled) cursor else null)
    return feedback.onGloballyPositioned {
        val bounds = it.boundsInRoot().translate(-dock.origin)
        dock.regions[token] = DockInteraction.Region(token, action, bounds, z, priority, anchor, cursor, holdToDrag)
        if (key != null) dock.anchors[key] = bounds
    }
}

@Composable internal fun Modifier.dragSource(dock: DockInteraction, item: JSONObject, context: JSONObject = item,
    holdToDrag: Boolean = false): Modifier {
    val kind = item.getString("kind")
    return workspaceSource(dock, obj("type" to if (kind == "tile") "tile_drag" else "drag_workspace", "item" to item),
        priority = when (kind) { "tile" -> 3; "panel" -> 2; else -> 1 }, anchor = context, holdToDrag = holdToDrag)
}

/** Capture belongs to the stable workspace, not a tab or ribbon that Rust may
 * reparent during tear-off. Long presses retain this original source and press
 * position, including grips without a child click handler. */
@Composable internal fun Modifier.workspaceGestures(dock: DockInteraction): Modifier {
    val focused = LocalWindowInfo.current.isWindowFocused
    SideEffect { dock.focused = focused }
    return workspaceGestureCapture(dock, focused)
}

private fun Modifier.workspaceGestureCapture(dock: DockInteraction, focused: Boolean): Modifier = pointerInput(dock, focused) {
    if (!focused) return@pointerInput
    var chromeTap: Triple<String, Long, Offset>? = null
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        dock.contactType = down.type
        try {
            val previousTap = chromeTap
            chromeTap = null
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
                var held = false
                var retired = false
                var remaining = viewConfiguration.longPressTimeoutMillis
                var eventTime = down.uptimeMillis
                val divider = source.action.takeIf { it.optString("type") == "drag_divider" }?.getInt("id")
                val group = source.action.takeIf { it.optString("type") == "drag_workspace" }?.objectOrNull("item")
                    ?.takeIf { it.optString("kind") == "group" }
                val band = divider != null && dock.host.snapshot?.getJSONObject("layout")?.array("dividers")?.objects()
                    ?.any { it.getInt("id") == divider && it.optBoolean("band") && it.optString("axis") == "horizontal" } == true
                // The registered source distinguishes empty header/grip contacts
                // from tabs, even inside a scrollable tab strip.
                val tapTarget = if (band) "divider:$divider" else group?.let { "group:${it.getInt("group")}" }
                dock.contactHeld = true
                dock.contactSource = source.token
                dock.retiredContext = null
                fun sourceExists() = dock.regions[source.token]?.action?.toString() == source.action.toString()
                fun retire() {
                    retired = true
                    dock.pickupCursor(false)
                    dock.retiredContext = source.context?.let(dock::anchorKey)
                    dock.closeContext()
                }
                try {
                    do {
                        val event = if (!retired && !held && !started && (source.holdToDrag || source.context != null)) {
                            withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                        } else awaitPointerEvent(PointerEventPass.Initial)
                        if (event == null) {
                            if (!sourceExists() || dock.popupOpen || !dock.enabled) { retire(); continue }
                            held = true
                            if (source.holdToDrag && down.type == PointerType.Mouse) dock.pickupCursor(true)
                            source.context?.let(dock::holdContext)
                            continue
                        }
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        remaining = (remaining - (change.uptimeMillis - eventTime)).coerceAtLeast(1)
                        eventTime = change.uptimeMillis
                        // Compose represents ACTION_CANCEL as an already-consumed
                        // release. Preserve the shared transaction before consuming it.
                        if (!change.pressed && change.isConsumed) break
                        if (held) change.consume()
                        if (dock.popupOpen || !dock.enabled) break
                        if (!started && !retired && !sourceExists()) retire()
                        val moved = (change.position - down.position).getDistance() > viewConfiguration.touchSlop
                        // A tile hold must be stationary. Retiring it leaves motion
                        // unconsumed for its scroll container, even after a pause.
                        if (!started && !retired && source.holdToDrag && !held && moved) retire()
                        if (!started && !retired && change.pressed && moved && (!source.holdToDrag || held)) {
                            dock.start(source.action, down.position / dock.density, source.cursor); started = true
                        }
                        if (started) {
                            change.consume()
                            dock.move(change.position / dock.density)
                        }
                        if (!change.pressed) {
                            if (retired) change.consume()
                            if (started) dock.finish(false)
                            else if (tapTarget != null && !held && !retired && change.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis) {
                                val gap = down.uptimeMillis - (previousTap?.second ?: 0)
                                if (previousTap?.first == tapTarget &&
                                    gap in viewConfiguration.doubleTapMinTimeMillis..viewConfiguration.doubleTapTimeoutMillis &&
                                    (down.position - previousTap.third).getDistance() <= viewConfiguration.touchSlop * 2) {
                                    change.consume()
                                    if (band) dock.host.dispatch(obj("type" to "reset_column_width", "id" to divider, "viewport" to dock.viewport))
                                    else group?.let(dock::doubleClickHandle)
                                } else chromeTap = Triple(tapTarget, change.uptimeMillis, change.position)
                            }
                            released = true; break
                        }
                    } while (true)
                } finally {
                    if (started && !released) dock.finish(true)
                    if (!released) dock.closeContext()
                    dock.pickupCursor(false)
                    dock.contactHeld = false
                    dock.contactSource = null
                    dock.retiredContext = null
                }
            }
            // Non-draggable chrome (including Zen projections) still needs the
            // original device when a child delivers its native long-click callback.
            while (currentEvent.changes.any { it.pressed }) awaitPointerEvent(PointerEventPass.Final)
        } finally { dock.contactType = null }
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
    var natural by remember { mutableStateOf(Rect.Zero) }
    SideEffect {
        val r = bounds.intersect(clip)
        if (active && natural.width > 0f) {
            dock.tabSlots[key] = obj("group" to group, "index" to index, "panel" to panel,
                "bounds" to obj("x" to natural.left / dock.density, "y" to natural.top / dock.density,
                    "width" to natural.width / dock.density, "height" to natural.height / dock.density))
        } else dock.tabSlots.remove(key)
        if (active && r.width > 0f && r.height > 0f) {
            dock.tabs[key] = obj("group" to group, "index" to index, "panel" to panel,
                "bounds" to obj("x" to r.left / dock.density, "y" to r.top / dock.density,
                    "width" to r.width / dock.density, "height" to r.height / dock.density))
        } else dock.tabs.remove(key)
    }
    DisposableEffect(dock, key) { onDispose { dock.tabs.remove(key); dock.tabSlots.remove(key) } }
    return onGloballyPositioned {
        bounds = it.boundsInRoot().translate(-dock.origin)
        val origin = it.positionInRoot() - dock.origin
        natural = Rect(origin.x, origin.y, origin.x + it.size.width, origin.y + it.size.height)
    }
}

@Composable internal fun Modifier.drawerTile(dock: DockInteraction, panel: String, tile: Int): Modifier {
    val column = LocalDrawerColumn.current ?: return this
    val clip = LocalDrawerClip.current
    var bounds by remember { mutableStateOf(Rect.Zero) }
    SideEffect { dock.measureDrawerTile(column, panel, tile, bounds.intersect(clip)) }
    DisposableEffect(column, panel, tile) { onDispose { dock.measureDrawerTile(column, panel, tile, null) } }
    return onGloballyPositioned { bounds = it.boundsInRoot().translate(-dock.origin) }
}
