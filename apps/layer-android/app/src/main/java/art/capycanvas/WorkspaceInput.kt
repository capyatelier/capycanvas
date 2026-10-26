package art.capycanvas

import android.view.PointerIcon as AndroidPointerIcon
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.AwaitPointerEventScope
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.isPrimaryPressed
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

internal val LocalWorkspaceOpacity = staticCompositionLocalOf<State<Float>?> { null }
internal val LocalWorkspaceVisible = staticCompositionLocalOf { true }
internal val LocalWorkspaceZ = staticCompositionLocalOf { 0 }
internal val LocalDock = staticCompositionLocalOf<DockInteraction?> { null }

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

internal fun pointerKind(type: PointerType) = when (type) {
    PointerType.Mouse -> "mouse"
    PointerType.Stylus, PointerType.Eraser -> "pen"
    else -> "touch"
}

private fun edgePointerIcon(cursor: String) = when (cursor) {
    "col-resize", "ew-resize" -> AndroidPointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW
    "row-resize", "ns-resize" -> AndroidPointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
    "nwse-resize" -> AndroidPointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
    else -> AndroidPointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
}

internal fun resizePointerIcon(edge: String) = when (edge) {
    "left", "right" -> AndroidPointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW
    "top", "bottom" -> AndroidPointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
    "top_left", "bottom_right" -> AndroidPointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
    "top_right", "bottom_left" -> AndroidPointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
    else -> AndroidPointerIcon.TYPE_ARROW
}

/** Native content facts in logical pixels; Rust chooses the dropped height. */
internal data class PanelContentSize(val height: Float, val fixedHeight: Float? = null, val unitHeight: Float = 0f)
private data class PanelMeasurement(val tabWidth: Float = 0f, val content: PanelContentSize = PanelContentSize(0f)) {
    fun json(panel: String) = obj("panel" to panel, "tab_width" to tabWidth, "content_height" to content.height.coerceAtMost(999_999f)).apply {
        content.fixedHeight?.let { put("scroll", obj("fixed_height" to it, "unit_height" to content.unitHeight)) }
    }
}

/** Native hit geometry and gesture capture only. Rust owns movement, tear-off,
 * docking, sizing, undo transactions and Zen visibility on every platform. */
internal class DockInteraction(val host: CanvasHost) {
    data class DrawerSource(val direction: String, val bounds: Rect, val anchor: JSONObject? = null)
    data class Region(val token: Any, val action: JSONObject?, val bounds: Rect, val z: Int, val priority: Int,
        val context: JSONObject?, val cursor: Int, val holdToDrag: Boolean)
    val regions = mutableMapOf<Any, Region>()
    var bubbles: BubbleInteraction? = null
    var bubbleCaptured = false
    data class ChromeRegion(val bounds: Rect, val z: Int, val header: Boolean)
    val chromeRegions = mutableMapOf<Any, ChromeRegion>()
    var drawer: JSONObject? = null
    val drawerSources = mutableStateMapOf<String, DrawerSource>()
    var drawerTileRevision by mutableIntStateOf(0)
        private set
    fun tileDrawerSource(panel: String, tile: Int) =
        drawerSources["tool"]?.takeIf { it.anchor?.optString("panel") == panel && it.anchor.optInt("tile") == tile }
    private val drawerTiles = mutableMapOf<String, JSONObject>()
    private val columnDrawers = mutableMapOf<Int, JSONObject>()
    data class Anchor(val target: String, val bounds: Rect)
    val anchors = mutableMapOf<Any, Anchor>()
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
        if (!dragging && edge?.claimed != true) dragCursor = if (armed) AndroidPointerIcon.TYPE_GRAB else null
    }
    class EdgeContact(val id: Long, val canvas: Boolean, val kind: String, val origin: Offset, var position: Offset = origin) {
        var pending = true
        var hold = false
        var selectOnRelease = false
        var claimed = false
    }
    var edge: EdgeContact? = null
        private set
    private var edgeContacts = 0L
    var edgeSurfaceHit = false
    var edgeControlHit: String? = null
    fun edgeContactClass(point: Offset, source: Region?): String? {
        if (!enabled || popupOpen || contextMenu != null) return null
        val action = source?.action
        if (!edgeSurfaceHit) return "free".takeIf { source == null && chromeRegions.values.none { it.bounds.contains(point) } }
        if (action != null && action.getString("type") !in listOf("drag_workspace", "tile_drag")) return null
        return edgeControlHit ?: when {
            action == null -> "free"
            source?.holdToDrag == true -> if (action.optJSONObject("item")?.optString("kind") == "tile") "input" else "free"
            else -> "pickup"
        }
    }
    private fun edgeInput(contact: EdgeContact, phase: String, time: Long, kind: String = "free") = obj("type" to "edge_contact",
        "id" to contact.id, "phase" to phase, "kind" to contact.kind, "contact" to kind,
        "position" to JSONArray(listOf(contact.position.x / density, contact.position.y / density)),
        "viewport" to viewport, "time_ns" to time * 1_000_000)
    fun beginEdge(down: PointerInputChange, contact: String, tab: JSONObject?) {
        val next = EdgeContact(++edgeContacts, !edgeSurfaceHit, pointerKind(down.type), down.position)
        edge = next
        host.edgeContact(edgeInput(next, "down", down.uptimeMillis, contact), tab) { reply ->
            next.selectOnRelease = reply.optBoolean("edge_select_on_release")
            edgeReply(next, reply)
        }
    }
    /** The contact when the shared session owns this change, or withholds motion
     * beyond [slop], where scrollers start and taps end. */
    fun edgeMotion(change: PointerInputChange, slop: Float): EdgeContact? {
        val current = edge ?: return null
        current.position = change.position
        if (!change.pressed) edge = null
        host.edgeContact(edgeInput(current, if (change.pressed) "move" else "up", change.uptimeMillis)) { edgeReply(current, it) }
        if (!change.pressed && current.claimed) dragCursor = null
        val withheld = change.pressed && current.hold && (change.position - current.origin).getDistance() > slop
        return current.takeIf { it.claimed || withheld }?.also { change.consume() }
    }
    private fun edgeReply(current: EdgeContact, reply: JSONObject) {
        if (edge !== current) return
        current.pending = reply.optBoolean("edge_pending")
        current.hold = reply.optBoolean("edge_hold") || reply.optBoolean("edge_select_on_release")
        val cursor = reply.nullableString("edge_cursor")
        if (cursor != null) {
            if (!current.claimed) closeContext()
            current.claimed = true
            dragCursor = edgePointerIcon(cursor)
        } else if (!current.pending && !current.claimed && !current.selectOnRelease) edge = null
    }
    fun cancelEdge() {
        val current = edge ?: return
        edge = null
        host.edgeContact(edgeInput(current, "cancel", android.os.SystemClock.uptimeMillis()))
        if (current.claimed) dragCursor = null
    }
    var expansion by mutableStateOf<JSONObject?>(null)
    var configurationHeight by mutableFloatStateOf(0f)
    var contextMenu by mutableStateOf<JSONObject?>(null)
    var contextRequest: JSONObject? = null
    var headerHeld = false
    var contactHeld by mutableStateOf(false)
    var contactType: PointerType? = null
    var contactPosition: Offset? = null
    var contactSource: Any? = null
    var retiredContext: String? = null
    var contextAnchor = Rect.Zero
    private var popupOwners = 0
    val popupOpen get() = popupOwners > 0
    fun popup(open: Boolean) {
        popupOwners = (popupOwners + if (open) 1 else -1).coerceAtLeast(0)
        refresh()
    }
    /** The canvas bar's place, kept while a canvas contact hides the bar. */
    var canvasBarSlot by mutableStateOf<Rect?>(null)
    private var active: JSONObject? = null
    private var position = Offset.Zero
    private var generation = 0
    private var contextTarget: String? = null
    private var contextOwner: Any? = null
    private val measurements = mutableMapOf<String, PanelMeasurement>()

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
    fun measure(panel: String, tabWidth: Float? = null, content: PanelContentSize? = null) {
        val old = measurements[panel] ?: PanelMeasurement()
        val next = PanelMeasurement(tabWidth ?: old.tabWidth, content ?: old.content)
        val accepted = host.snapshot?.array("panel_measurements")?.objects()?.find { it.getString("panel") == panel }
        if (next == old && accepted?.number("tab_width") == next.tabWidth && accepted.number("content_height") == next.content.height.coerceAtMost(999_999f) &&
            accepted.objectOrNull("scroll")?.number("fixed_height") == next.content.fixedHeight &&
            (next.content.fixedHeight == null || accepted.objectOrNull("scroll")?.number("unit_height") == next.content.unitHeight)) return
        measurements[panel] = next
        publishMeasurements()
    }
    private fun publishMeasurements() {
        host.dispatch(obj("type" to "measure_panels", "measurements" to JSONArray(measurements.map { (id, size) -> size.json(id) })))
    }

    // Shared workspace gestures have their own Zen state in Rust. Only tile
    // reordering uses the host's generic dragging fact.
    fun facts() = obj("held" to headerHeld, "dragging" to (active?.optString("type") == "tile_drag"),
        "popup_open" to (popupOpen || contextMenu != null), "expanded_panel" to expansion, "content_drawer" to drawer?.optJSONObject("placement")?.optJSONObject("bounds"),
        "drawer_connection" to drawer?.optJSONObject("connection")?.optJSONObject("bounds"))
    fun refresh() = host.chrome(obj("kind" to "refresh"), facts())
    fun anchorKey(item: JSONObject) = "${item.optString("kind")}:${item.optString("column")}:${item.optString("group")}:${item.optString("panel")}:${item.optString("tile")}"
    fun removeAnchor(owner: Any) {
        anchors.remove(owner)
        if (contextOwner == owner) closeContext()
    }
    fun beginContact(point: Offset, type: PointerType) {
        cancelPendingContext()
        contactPosition = point
        contactType = type
        edgeSurfaceHit = false
        edgeControlHit = null
    }
    fun cancelPendingContext() { if (contextMenu == null && contextTarget != null) closeContext() }
    fun windowFocus(value: Boolean) {
        if (focused == value) return
        focused = value
        if (!value && contextMenu == null) closeContext()
    }
    fun context(target: JSONObject) {
        if (dragging || !focused || !enabled) return
        val key = anchorKey(target)
        if (contactHeld && retiredContext == key) return
        val owner = contactSource?.takeIf { anchors[it]?.target == key }
            ?: anchors.entries.filter { it.value.target == key }.minByOrNull { it.value.bounds.width * it.value.bounds.height }?.key ?: return
        val anchor = contactPosition?.let { Rect(it, it) } ?: anchors.getValue(owner).bounds
        if (contextTarget == key) return
        contextTarget = key
        contextOwner = owner
        val request = ++generation
        val query = obj("type" to "context", "target" to target)
        host.query(query) {
            if (request != generation) return@query
            if (dragging || !focused || !enabled || anchors[owner]?.target != key) { closeContext(); return@query }
            contextRequest = query; contextAnchor = anchor; contextMenu = it as? JSONObject; refresh()
        }
    }
    fun holdContext(target: JSONObject) {
        if (contactType != PointerType.Mouse) context(target)
    }
    fun closeContext() { generation++; contextTarget = null; contextOwner = null; contextMenu = null; contextRequest = null; refresh() }
    fun doubleClickHandle(item: JSONObject) {
        host.query(obj("type" to "panel_handle_target", "item" to item)) { group ->
            if (group is Number) host.dispatch(obj("type" to "double_click_panel_handle", "group" to group, "viewport" to viewport))
        }
    }
    fun hit(point: Offset): Region? {
        if (!enabled || popupOpen || contextMenu != null) return null
        val z = chromeRegions.values.filter { it.bounds.contains(point) }.maxOfOrNull { it.z } ?: 0
        return regions.values.filter { it.z >= z && it.bounds.contains(point) }
            .maxWithOrNull(compareBy<Region> { it.z }.thenBy { it.priority })
    }
    private fun send(phase: String, preview: Boolean = false) {
        val action = active ?: return
        if (action.getString("type") == "tile_drag") return
        // Opening another tab can invalidate Rust's transient measurement even
        // when the displayed rectangle is unchanged.
        val actions = mutableListOf<JSONObject>()
        if (phase == "up") actions.add(obj("type" to "measure_panels", "measurements" to JSONArray(measurements.map { (id, size) -> size.json(id) })))
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
        cancelEdge()
        host.beginWorkspaceGesture()
        active = action; position = point; dragging = true
        val panel = action.optJSONObject("item")?.takeIf { it.optString("kind") == "panel" }?.optString("panel")
        // An open drawer also publishes tabs for its collapsed icons. Freeze
        // slots only when the actual press came from the tab, not the icon/grip.
        val tab = tabs.values.firstOrNull { it.optString("panel") == panel && it.getJSONObject("bounds").rect().contains(point) }
        frozenPanel = tab?.optString("panel")
        frozenTabGroup = tab?.getInt("group")
        frozenTabs = tabSlots.values.filter { it.optInt("group") == frozenTabGroup }.map { JSONObject(it.toString()) }
        frozenTabClip = frozenTabGroup?.let { tabClips[it] }?.let { Rect(it.left / density, it.top / density, it.right / density, it.bottom / density) }
        dragCursor = if (action.optJSONObject("item") != null) AndroidPointerIcon.TYPE_GRABBING else cursor
        closeContext(); send("down")
    }
    fun move(point: Offset) {
        position = point; send("move", preview = true)
        if (active?.optString("type") == "tile_drag") {
            host.chrome(obj("kind" to "motion"), facts())
            val request = generation
            host.query(query()) { if (request == generation) hint = it as? JSONObject }
        }
    }
    fun finish(cancel: Boolean) {
        if (!dragging) return
        val action = active!!
        val request = ++generation
        fun end() {
            if (request != generation) return
            active = null; hint = null; dragging = false; dragCursor = null; frozenTabs = emptyList(); frozenTabGroup = null; frozenTabClip = null; frozenPanel = null; refresh()
        }
        if (action.getString("type") == "tile_drag" && !cancel) {
            host.query(query()) { result ->
                if (request == generation) {
                    val target = (result as? JSONObject)?.optJSONObject("target")
                    val item = action.getJSONObject("item")
                    if (target != null) {
                        host.dispatch(obj("type" to "move_tile", "panel" to item.getString("panel"),
                            "tile" to item.getInt("tile"), "target" to target, "viewport" to viewport))
                        end()
                    } else bubbles?.takeTile(item, position, { request == generation }, ::end) ?: end()
                }
            }
        } else { send(if (cancel) "cancel" else "up"); end() }
    }
}

@Composable internal fun Modifier.workspaceSource(dock: DockInteraction, action: JSONObject?,
    priority: Int = 0, anchor: JSONObject? = null, cursor: Int = AndroidPointerIcon.TYPE_GRAB,
    holdToDrag: Boolean = false): Modifier {
    if(!LocalWorkspaceVisible.current) return this
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    val key = anchor?.let(dock::anchorKey)
    DisposableEffect(dock, token, key) {
        onDispose {
            dock.regions.remove(token)
            dock.removeAnchor(token)
            if (dock.contactSource == token && !dock.dragging) {
                dock.retiredContext = key
                dock.pickupCursor(false)
                dock.closeContext()
            }
        }
    }
    SideEffect { dock.regions[token]?.let {
        if (dock.contactSource == token && it.action?.toString() != action?.toString()) dock.pickupCursor(false)
        dock.regions[token] = it.copy(action = action, z = z, priority = priority, context = anchor, cursor = cursor, holdToDrag = holdToDrag)
    } }
    // Held tiles use the normal mouse pointer until pickup is armed. Keep
    // Android's existing default pen hover; grips retain their immediate hand.
    val feedback = if (holdToDrag || cursor == AndroidPointerIcon.TYPE_ARROW) pointerHoverIcon(PointerIcon(AndroidPointerIcon.TYPE_ARROW))
        else workspacePointerIcon(if (dock.enabled) cursor else null)
    return feedback.onGloballyPositioned {
        val bounds = it.boundsInRoot().translate(-dock.origin)
        dock.regions[token] = DockInteraction.Region(token, action, bounds, z, priority, anchor, cursor, holdToDrag)
        if (key != null) dock.anchors[token] = DockInteraction.Anchor(key, bounds)
    }
}

@Composable internal fun Modifier.contextSource(dock: DockInteraction, target: JSONObject): Modifier =
    workspaceSource(dock, null, anchor = target, cursor = AndroidPointerIcon.TYPE_ARROW)

@Composable internal fun Modifier.controlSource(dock: DockInteraction? = LocalDock.current): Modifier =
    if (dock == null) this else workspaceSource(dock, null, priority = 1, cursor = AndroidPointerIcon.TYPE_ARROW)

@Composable internal fun Modifier.dragSource(dock: DockInteraction, item: JSONObject, context: JSONObject = item,
    holdToDrag: Boolean = false, cursor: Int = AndroidPointerIcon.TYPE_GRAB): Modifier {
    val kind = item.getString("kind")
    return workspaceSource(dock, obj("type" to if (kind == "tile") "tile_drag" else "drag_workspace", "item" to item),
        priority = when (kind) { "tile" -> 3; "panel" -> 2; else -> 1 }, anchor = context, holdToDrag = holdToDrag, cursor = cursor)
}

/** Capture belongs to the stable workspace, not a tab or ribbon that Rust may
 * reparent during tear-off. Long presses retain this original source and press
 * position, including grips without a child click handler. */
@Composable internal fun Modifier.workspaceGestures(dock: DockInteraction): Modifier {
    val focused = LocalWindowInfo.current.isWindowFocused
    SideEffect { dock.windowFocus(focused) }
    return workspaceGestureCapture(dock, focused)
}

private fun Modifier.workspaceGestureCapture(dock: DockInteraction, focused: Boolean): Modifier = pointerInput(dock, focused) {
    if (!focused) return@pointerInput
    data class Tap(val target: String, val time: Long, val position: Offset, val device: PointerType)
    var chromeTap: Tap? = null
    var edgeTap: Tap? = null
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        dock.cancelPendingContext()
        if (dock.bubbleCaptured || down.isConsumed) return@awaitEachGesture
        dock.beginContact(down.position, down.type)
        try {
            val previousTap = chromeTap
            chromeTap = null
            val chrome = dock.chromeRegions.values.filter { it.bounds.contains(down.position) }.maxByOrNull { it.z }
            val source = dock.hit(down.position)
            dock.contactSource = source?.token
            val tab = dock.tabs.values.firstOrNull { it.optString("panel") == source?.action?.objectOrNull("item")?.optString("panel") && it.getJSONObject("bounds").let { b ->
                Rect(b.number("x"), b.number("y"), b.number("x") + b.number("width"), b.number("y") + b.number("height")).contains(down.position / dock.density)
            } }
            dock.headerHeld = chrome?.header == true
            if (chrome != null) {
                dock.host.chrome(obj("kind" to "contact", "canvas" to false,
                    "position" to JSONArray(listOf(down.position.x / dock.density, down.position.y / dock.density))), dock.facts().put("contact_tab", tab?.optString("panel")))
            }
            // Invisible resize strips have no child button to consume their
            // press. Do not let the underlying SurfaceView begin a stroke/pan.
            if (source != null && (source.action != null || source.context != null) &&
                source.action?.getString("type") !in listOf("drag_workspace", "tile_drag")) down.consume()
            val buttons = awaitPointerEvent(PointerEventPass.Final).buttons
            val primary = (down.type != PointerType.Mouse || buttons.isPrimaryPressed) && !buttons.isSecondaryPressed
            val select = tab?.takeIf { primary }?.let { obj("type" to "select_panel_tab", "group" to it.getInt("group"), "panel" to it.getString("panel")) }
            val contact = dock.edgeContactClass(down.position, source)?.takeIf { primary }
            val resettable = contact == "free" && dock.edgeSurfaceHit && source?.action == null && source?.context?.optString("kind") != "column"
            var tapUp: PointerInputChange? = null
            if (contact != null) dock.beginEdge(down, contact, select) else select?.let { dock.host.dispatch(it) }
            if (source != null && (source.action != null || source.context != null)) {
                if (currentEvent.buttons.isSecondaryPressed) {
                    source.context?.let(dock::context)
                    down.consume()
                    while (currentEvent.changes.any { it.pressed }) awaitPointerEvent(PointerEventPass.Final)
                    return@awaitEachGesture
                }
                var started = false
                var released = false
                var held = false
                var retired = false
                var remaining = viewConfiguration.longPressTimeoutMillis
                var eventTime = down.uptimeMillis
                val divider = source.action?.takeIf { it.optString("type") == "drag_divider" }?.getInt("id")
                val group = source.action?.takeIf { it.optString("type") == "drag_workspace" }?.objectOrNull("item")
                    ?.takeIf { it.optString("kind") == "group" }
                val column = source.context?.takeIf { source.action == null && it.optString("kind") == "column" }?.getInt("column")
                val band = divider != null && dock.host.snapshot?.getJSONObject("layout")?.array("dividers")?.objects()
                    ?.any { it.getInt("id") == divider && it.optBoolean("band") && it.optString("axis") == "horizontal" } == true
                // The registered source distinguishes empty header/grip contacts
                // from tabs, even inside a scrollable tab strip.
                val tapTarget = if (band) "divider:$divider" else group?.let { "group:${it.getInt("group")}" } ?: column?.let { "column:$it" }
                dock.contactHeld = true
                dock.contactSource = source.token
                dock.retiredContext = null
                fun sourceExists() = dock.regions[source.token]?.let { it.action?.toString() == source.action?.toString() } == true
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
                        } else { awaitPointerEvent(PointerEventPass.Initial) }
                        if (event == null) {
                            if (!sourceExists() || dock.popupOpen || !dock.enabled) {
                                retire()
                                continue
                            }
                            held = true
                            dock.cancelEdge()
                            if (source.holdToDrag && down.type == PointerType.Mouse) dock.pickupCursor(true)
                            source.context?.let(dock::holdContext)
                            continue
                        }
                        if (event.changes.any { it.id != down.id && it.pressed }) {
                            if (dock.edge?.claimed != true) { event.changes.forEach { it.consume() }; break }
                            event.changes.forEach { if (it.id != down.id) it.consume() }
                        }
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        remaining = (remaining - (change.uptimeMillis - eventTime)).coerceAtLeast(1)
                        eventTime = change.uptimeMillis
                        // Compose represents ACTION_CANCEL as an already-consumed
                        // release. Preserve the shared transaction before consuming it.
                        if (dock.bubbleCaptured || !change.pressed && change.isConsumed) break
                        val edge = if (started) null else dock.edgeMotion(change, viewConfiguration.touchSlop)
                        if (edge?.claimed == true) {
                            if (!retired) retire()
                            if (!change.pressed) { released = true; break }
                            continue
                        }
                        if (edge != null && change.pressed) continue
                        if (held) change.consume()
                        if (dock.popupOpen || !dock.enabled) break
                        if (!started && !retired && !sourceExists()) retire()
                        val moved = (change.position - down.position).getDistance() > viewConfiguration.touchSlop
                        // A tile hold must be stationary. Retiring it leaves motion
                        // unconsumed for its scroll container, even after a pause.
                        if (!started && !retired && (source.holdToDrag || source.action == null) && !held && moved) retire()
                        if (source.action != null && !started && !retired && change.pressed && moved && (!source.holdToDrag || held)) {
                            dock.start(source.action, down.position / dock.density, source.cursor); started = true
                        }
                        if (started) {
                            change.consume()
                            dock.move(change.position / dock.density)
                        }
                        if (!change.pressed) {
                            tapUp = change.takeUnless { it.isConsumed || retired || held }
                            if (retired) change.consume()
                            if (started) dock.finish(false)
                            else if (tapTarget != null && !held && !retired && change.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis) {
                                val gap = down.uptimeMillis - (previousTap?.time ?: 0)
                                if (previousTap?.target == tapTarget && previousTap.device == down.type &&
                                    gap in viewConfiguration.doubleTapMinTimeMillis..viewConfiguration.doubleTapTimeoutMillis &&
                                    (down.position - previousTap.position).getDistance() <= viewConfiguration.touchSlop * 2) {
                                    change.consume()
                                    if (band) dock.host.dispatch(obj("type" to "reset_column_width", "id" to divider, "viewport" to dock.viewport))
                                    else if (column != null) dock.host.customize(obj("type" to "set_column_collapsed", "group" to column, "collapsed" to false))
                                    else group?.let(dock::doubleClickHandle)
                                } else chromeTap = Tap(tapTarget, change.uptimeMillis, change.position, down.type)
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
            followEdge(dock, down)?.let { tapUp = it }
            // Non-draggable chrome still needs the original device when a
            // child delivers its native long-click callback.
            while (currentEvent.changes.any { it.pressed }) awaitPointerEvent(PointerEventPass.Final)
            val up = tapUp?.takeIf { resettable }?.let { awaitPointerEvent(PointerEventPass.Final).changes.firstOrNull { it.id == down.id } }
            val previousEdgeTap = edgeTap
            edgeTap = up?.takeIf { !it.isConsumed && it.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis &&
                (it.position - down.position).getDistance() <= viewConfiguration.touchSlop }?.let { Tap("edge", it.uptimeMillis, it.position, down.type) }
            if (edgeTap != null && previousEdgeTap?.device == down.type &&
                down.uptimeMillis - previousEdgeTap.time in viewConfiguration.doubleTapMinTimeMillis..viewConfiguration.doubleTapTimeoutMillis &&
                (down.position - previousEdgeTap.position).getDistance() <= viewConfiguration.touchSlop * 2) {
                edgeTap = null
                dock.host.input(obj("type" to "edge_double_press", "kind" to pointerKind(down.type), "contact" to "free",
                    "position" to JSONArray(listOf(down.position.x / dock.density, down.position.y / dock.density)), "viewport" to dock.viewport))
            }
        } finally { dock.cancelEdge(); dock.headerHeld = false; dock.refresh(); dock.contactType = null; dock.contactPosition = null; dock.contactSource = null }
    }
}

/** The unconsumed release, seen in the initial pass, when it ends the contact. */
private suspend fun AwaitPointerEventScope.followEdge(dock: DockInteraction, down: PointerInputChange): PointerInputChange? {
    while (dock.edge != null) {
        val event = awaitPointerEvent(PointerEventPass.Initial)
        val others = event.changes.filter { it.id != down.id && it.pressed }
        if (others.isNotEmpty() && dock.edge?.claimed != true) { dock.cancelEdge(); return null }
        others.forEach { it.consume() }
        val change = event.changes.firstOrNull { it.id == down.id } ?: run { dock.cancelEdge(); return null }
        if (!change.pressed && change.isConsumed) { dock.cancelEdge(); return null }
        val owner = dock.edgeMotion(change, viewConfiguration.touchSlop)
        if (!change.pressed) return change.takeIf { owner == null }
        if (owner == null && dock.edge?.canvas == false &&
            awaitPointerEvent(PointerEventPass.Final).changes.firstOrNull { it.id == down.id }?.isConsumed == true) dock.cancelEdge()
    }
    return null
}

@Composable internal fun Modifier.edgeSurface(dock: DockInteraction): Modifier = pointerInput(dock) {
    awaitEachGesture { awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial); dock.edgeSurfaceHit = true }
}

/** Marks a control that owns swipes ("input") or sideways swipes ("pickup"). */
@Composable internal fun Modifier.edgeControl(contact: String = "input"): Modifier {
    val dock = LocalDock.current ?: return this
    return pointerInput(dock, contact) {
        awaitEachGesture { awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial); dock.edgeControlHit = contact }
    }
}


/** Only UI-covered contacts are sent here; CanvasHost gates bare canvas contacts. */
@Composable internal fun Modifier.chromeRegion(dock: DockInteraction, header: Boolean = false, active: Boolean = true): Modifier {
    if(!LocalWorkspaceVisible.current) return this
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    DisposableEffect(dock, token) { onDispose { dock.chromeRegions.remove(token) } }
    SideEffect {
        if (!active) dock.chromeRegions.remove(token)
        else dock.chromeRegions[token]?.takeIf { it.z != z || it.header != header }?.let { dock.chromeRegions[token] = it.copy(z = z, header = header) }
    }
    return onGloballyPositioned { if (active) dock.chromeRegions[token] = DockInteraction.ChromeRegion(it.boundsInRoot().translate(-dock.origin), z, header) }
}
@Composable internal fun Modifier.contextAnchor(dock: DockInteraction, target: JSONObject): Modifier {
    if(!LocalWorkspaceVisible.current) return this
    val key = dock.anchorKey(target)
    val token = remember { Any() }
    DisposableEffect(dock, token, key) { onDispose { dock.removeAnchor(token) } }
    return onGloballyPositioned { dock.anchors[token] = DockInteraction.Anchor(key, it.boundsInRoot().translate(-dock.origin)) }
}
internal val LocalDrawerColumn = staticCompositionLocalOf<Int?> { null }
internal val LocalDrawerClip = staticCompositionLocalOf { Rect.Zero }

@Composable internal fun Modifier.columnDrawerBounds(dock: DockInteraction, column: Int, group: Int?): Modifier {
    if(!LocalWorkspaceVisible.current) return this
    var bounds by remember { mutableStateOf(Rect.Zero) }
    SideEffect { dock.measureColumnDrawer(column, group, bounds) }
    DisposableEffect(dock, column) { onDispose { dock.measureColumnDrawer(column, null, null) } }
    return onGloballyPositioned { bounds = it.boundsInRoot().translate(-dock.origin) }
}

@Composable internal fun Modifier.drawerTabHit(dock: DockInteraction, column: Int, group: Int, index: Int,
    panel: String, clip: Rect, active: Boolean): Modifier {
    if(!LocalWorkspaceVisible.current) return this
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
