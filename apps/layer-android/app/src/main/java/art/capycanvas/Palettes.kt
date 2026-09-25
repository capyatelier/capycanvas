package art.capycanvas

import android.view.KeyEvent as AndroidKeyEvent
import android.view.PointerIcon as AndroidPointerIcon
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.EaseOutCubic
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.ceil
import kotlin.math.roundToInt

private const val Tile = 40f
private const val Gap = 4f
private const val HistoryRows = 4
private const val GridRows = 4

private fun JSONArray.paint() = Color(getDouble(0).toFloat().coerceIn(0f, 1f), getDouble(1).toFloat().coerceIn(0f, 1f),
    getDouble(2).toFloat().coerceIn(0f, 1f), optDouble(3, 1.0).toFloat().coerceIn(0f, 1f))

internal class PaletteCells(val width: Float, val density: Float) {
    val columns = (((width + Gap) / (Tile + Gap)).toInt()).coerceAtLeast(1)
    private val cell = (width + Gap) / columns
    fun x(index: Int) = ((index % columns) * cell).roundToInt()
    fun width(index: Int) = (((index % columns) + 1) * cell).roundToInt() - x(index) - Gap.roundToInt()
    fun y(index: Int) = (index / columns) * (Tile + Gap).roundToInt()
    fun offset(index: Int) = IntOffset((x(index) * density).roundToInt(), (y(index) * density).roundToInt())
    fun rows(count: Int) = ((count + columns - 1) / columns).coerceAtLeast(1)
    fun height(rows: Int) = rows * (Tile + Gap) - Gap
    fun slot(local: Offset, count: Int): Int? {
        val x = local.x / density; val y = local.y / density
        if (x < 0f || x >= width || y < 0f) return null
        val index = (y / (Tile + Gap)).toInt() * columns + (x * columns / (width + Gap)).toInt()
        return index.takeIf { it < count }
    }
}

internal class PaletteDrag(val id: Long, val palette: Long, val grab: Offset, val size: IntSize, val color: Color,
    val original: List<Long>, val geometry: PaletteGeometry, val selected: Boolean) {
    var point by mutableStateOf(Offset.Zero)
    var started by mutableStateOf(false)
    var order by mutableStateOf<List<Long>?>(null)
    var slot: Int? = null
    var requested = false
    var answered: Int? = null
    var action: JSONObject? = null
}

internal class PaletteGeometry {
    var cells: PaletteCells? = null
    var origin = Offset.Zero
    var viewport = Rect.Zero
    var count = 0
    var scroll: ScrollState? = null
    fun slot(point: Offset): Int? = if (!viewport.contains(point)) null else cells?.slot(point - origin, count)
}

internal data class PaletteMenuState(val sections: JSONArray, val point: Offset, val owner: Any)
internal data class PaletteDialog(val kind: String, val id: Long? = null, val initial: String = "")

internal class PaletteController(val host: CanvasHost) {
    var selected by mutableStateOf<Long?>(null)
    var chooser by mutableStateOf(false)
    var expanded by mutableStateOf(false)
    var editing by mutableStateOf(false)
    var message by mutableStateOf<Pair<String, Boolean>?>(null)
    var dialog by mutableStateOf<PaletteDialog?>(null)
    var menu by mutableStateOf<PaletteMenuState?>(null)
    var contactHeld by mutableStateOf(false)
    var drag by mutableStateOf<PaletteDrag?>(null)
    var settle by mutableStateOf<List<Long>?>(null)
    var importRequest by mutableIntStateOf(0)
    var exportRequest by mutableStateOf<JSONObject?>(null)
    var editText by mutableStateOf("")
    private var editColor: String? = null
    var focus: Any? = null
    var keyboard by mutableStateOf(false)
    private var menuGeneration = 0

    fun view() = host.panelContent?.objectOrNull("palette_panel")
    fun state() = host.panelContent?.objectOrNull("state")
    fun current(): JSONObject? = state()?.displayColors()?.let { colors ->
        colors.optJSONObject(when (colors.optString("paint_slot")) { "background" -> "background"; "temporary" -> "temporary"; else -> "foreground" })
    }
    fun selection(view: JSONObject): JSONObject? {
        val swatches = view.array("swatches").objects()
        return swatches.firstOrNull { it.getLong("id") == selected && it.getBoolean("current") } ?: swatches.firstOrNull { it.getBoolean("current") }
    }
    fun apply(action: JSONObject, done: (String?) -> Unit = {}) = host.paletteAction(action) { error ->
        message = error?.let { it to true }
        done(error)
    }
    fun store() {
        val view = view() ?: return
        val color = current() ?: return
        apply(obj("op" to "store", "palette" to view.getLong("palette"), "name" to "", "color" to color)) { error ->
            if (error == null) selected = view()?.array("swatches")?.objects()?.lastOrNull()?.getLong("id")
        }
    }
    fun use(id: Long) { selected = id; apply(obj("op" to "use", "id" to id)) }
    fun useDefinition(color: JSONObject) { selected = null; host.dispatch(obj("type" to "color", "action" to obj("op" to "definition", "color" to color))) }
    fun beginEditing() { chooser = false; expanded = false; editColor = current()?.toString(); editing = true }
    fun retireStaleEdit() { if (editing && current()?.toString() != editColor) { editing = false; message = null } }
    fun commitName(text: String, done: () -> Unit = {}) {
        if (!editing) return
        val view = view() ?: return
        val id = selection(view)?.getLong("id")
        val action = if (id != null) obj("op" to "rename", "id" to id, "name" to text)
            else obj("op" to "name_current", "name" to text, "color" to (current() ?: return))
        editing = false
        apply(action) { error -> if (error != null) beginEditing() else done() }
    }
    fun openMenu(target: JSONObject, point: Offset, owner: Any, held: Boolean) {
        if (drag?.started == true) return
        val request = ++menuGeneration
        contactHeld = held
        host.query(obj("type" to "palette_menu", "target" to target)) { sections ->
            if (request == menuGeneration && drag?.started != true && sections is JSONArray) menu = PaletteMenuState(sections, point, owner)
        }
    }
    fun closeMenu() { menuGeneration++; menu = null }
    fun command(command: JSONObject) {
        when (command.getString("command")) {
            "new_palette" -> dialog = PaletteDialog("new")
            "import_palette" -> importRequest++
            "rename_palette" -> view()?.array("palettes")?.objects()?.firstOrNull { it.getLong("id") == command.getLong("id") }
                ?.let { dialog = PaletteDialog("rename", it.getLong("id"), it.getString("name")) }
            "remove_palette" -> view()?.array("palettes")?.objects()?.firstOrNull { it.getLong("id") == command.getLong("id") }
                ?.let { dialog = PaletteDialog("remove", it.getLong("id"), it.getString("name")) }
            "export_palette" -> exportRequest = command
            "rename_color" -> view()?.array("swatches")?.objects()?.firstOrNull { it.getLong("id") == command.getLong("id") }?.let {
                selected = it.getLong("id")
                apply(obj("op" to "use", "id" to it.getLong("id"))) { error -> if (error == null) beginEditing() }
            }
            "library" -> apply(command.getJSONObject("action"))
        }
    }
    fun begin(drag: PaletteDrag) { cancelDrag(); this.drag = drag }
    fun start(drag: PaletteDrag) {
        if (this.drag !== drag) return
        drag.started = true
        closeMenu()
        retarget(drag)
    }
    fun move(drag: PaletteDrag, point: Offset) {
        if (this.drag !== drag) return
        drag.point = point
        if (drag.started) retarget(drag)
    }
    fun retarget(drag: PaletteDrag) {
        val slot = if (chooser || expanded) null else drag.geometry.slot(drag.point)
        if (slot == drag.slot && drag.requested) return
        drag.slot = slot; drag.requested = true
        val target = slot ?: drag.original.indexOf(drag.id)
        host.query(obj("type" to "palette_reorder_preview", "palette" to drag.palette, "id" to drag.id, "slot" to target)) { preview ->
            if (this.drag !== drag || drag.slot != slot || preview !is JSONObject) return@query
            drag.answered = slot
            drag.order = preview.array("order").values().map { (it as Number).toLong() }
            drag.action = if (slot == null) null else preview.objectOrNull("action")
        }
    }
    fun drop(drag: PaletteDrag) {
        if (this.drag !== drag) return
        this.drag = null
        val slot = drag.slot ?: return
        settle = drag.order
        fun commit(action: JSONObject?, order: List<Long>?) {
            if (action == null) { settle = null; return }
            settle = order
            apply(action) { settle = null }
        }
        if (drag.answered == slot && drag.slot == slot) commit(drag.action, drag.order)
        else host.query(obj("type" to "palette_reorder_preview", "palette" to drag.palette, "id" to drag.id, "slot" to slot)) { preview ->
            (preview as? JSONObject)?.let { commit(it.objectOrNull("action"), it.array("order").values().map { v -> (v as Number).toLong() }) }
        }
    }
    fun end(drag: PaletteDrag) { if (this.drag === drag) this.drag = null }
    fun cancelDrag() { drag = null }
    fun escape(): Boolean = when {
        drag != null -> { cancelDrag(); true }
        menu != null -> { closeMenu(); true }
        editing -> { editing = false; message = null; true }
        chooser -> { chooser = false; true }
        expanded -> { expanded = false; true }
        else -> false
    }
    fun key(event: AndroidKeyEvent): Boolean {
        if (event.keyCode == AndroidKeyEvent.KEYCODE_ESCAPE && (focus != null || drag != null))
            return event.action != AndroidKeyEvent.ACTION_DOWN || escape()
        if (focus == null || host.editingText) return false
        if (event.action == AndroidKeyEvent.ACTION_DOWN) keyboard = true
        val undo = event.keyCode == AndroidKeyEvent.KEYCODE_Z || event.keyCode == AndroidKeyEvent.KEYCODE_Y
        if (!undo || !(event.isCtrlPressed || event.isMetaPressed)) return false
        if (event.action != AndroidKeyEvent.ACTION_DOWN) return true
        val view = view() ?: return true
        val redo = event.keyCode == AndroidKeyEvent.KEYCODE_Y || event.isShiftPressed
        if (view.getBoolean(if (redo) "can_redo" else "can_undo"))
            apply(obj("op" to if (redo) "redo_reorder" else "undo_reorder", "palette" to view.getLong("palette")))
        return true
    }
}

@Composable private fun Modifier.paletteFocus(controller: PaletteController): Modifier {
    val token = remember { Any() }
    val host = LocalCanvasHost.current
    DisposableEffect(token) { onDispose { if (controller.focus === token) controller.focus = null; if (host.colorControlFocus === token) host.colorControlFocus = null } }
    return onFocusChanged {
        if (it.hasFocus) { controller.focus = token; host.colorControlFocus = token }
        else { if (controller.focus === token) controller.focus = null; if (host.colorControlFocus === token) host.colorControlFocus = null }
    }
}

private fun Modifier.paletteKeys(activate: () -> Unit, menu: (() -> Unit)?): Modifier = onPreviewKeyEvent { event ->
    val menuKey = event.key == Key.Menu || (event.key == Key.F10 && event.isShiftPressed)
    val activation = event.key == Key.Enter || event.key == Key.NumPadEnter || event.key == Key.Spacebar
    when {
        menuKey && menu != null -> { if (event.type == KeyEventType.KeyDown) menu(); true }
        activation -> { if (event.type == KeyEventType.KeyUp) activate(); true }
        else -> false
    }
}

@Composable internal fun PalettePanel(host: CanvasHost, modifier: Modifier = Modifier, onContent: (PanelContentSize) -> Unit = {}) {
    val controller = host.palettes
    val view = host.panelContent?.objectOrNull("palette_panel") ?: return
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val owner = remember { Any() }
    val geometry = remember { PaletteGeometry() }
    val scroll = rememberScrollState()
    geometry.scroll = scroll
    var panelOrigin by remember { mutableStateOf(Offset.Zero) }
    var top by remember { mutableFloatStateOf(0f) }
    var bottom by remember { mutableFloatStateOf(0f) }
    var bodyHeight by remember { mutableFloatStateOf(0f) }
    val focused = LocalWindowInfo.current.isWindowFocused
    val swatches = view.array("swatches").objects()
    val ids = swatches.map { it.getLong("id") }
    val current = controller.current()?.toString()
    LaunchedEffect(focused) { if (!focused) controller.cancelDrag() }
    LaunchedEffect(view.getLong("palette"), ids) {
        controller.drag?.let { drag -> if (drag.palette != view.getLong("palette") || drag.original != ids) controller.cancelDrag() }
    }
    LaunchedEffect(current, view.array("palettes").toString()) { controller.retireStaleEdit() }
    DisposableEffect(owner) {
        onDispose { if (controller.drag?.geometry === geometry) controller.cancelDrag(); if (controller.menu?.owner === owner) controller.closeMenu() }
    }
    BackHandler(controller.drag != null || controller.editing || controller.chooser || controller.expanded) { controller.escape() }
    val selected = controller.selection(view)
    BoxWithConstraints(modifier.fillMaxWidth().testTag("palette-panel").onGloballyPositioned { panelOrigin = it.positionInRoot() }) {
        val bounded = constraints.hasBoundedHeight
        val cells = remember(maxWidth.value, density) { PaletteCells((maxWidth.value - 16f).coerceAtLeast(Tile), density) }
        val count = swatches.size + 1
        val naturalGrid = cells.grid(count)
        SideEffect {
            geometry.cells = cells; geometry.count = count
            paletteContent(cells, count, top, bottom)?.let(onContent)
        }
        val covered = controller.chooser || controller.expanded
        Column(Modifier.fillMaxWidth().then(if (bounded) Modifier.fillMaxHeight() else Modifier).padding(horizontal = 8.dp, vertical = 6.dp)) {
            Box((if (bounded) Modifier.weight(1f) else Modifier).fillMaxWidth().onSizeChanged { bodyHeight = it.height / density }) {
                Column(Modifier.fillMaxWidth().alpha(if (covered) 0f else 1f)) {
                    Column(Modifier.onSizeChanged { top = it.height / density }) {
                        HistoryRow(controller, view, cells, covered, expanded = false, rows = 1)
                        HorizontalDivider(Modifier.padding(vertical = 6.dp), color = colors.divider)
                    }
                    Box(Modifier.fillMaxWidth().heightIn(min = cells.height(2).dp, max = naturalGrid.dp)
                        .onGloballyPositioned { geometry.viewport = it.boundsInRoot() }
                        .verticalScroll(scroll).testTag("palette-swatches")) {
                        SwatchGrid(controller, view, swatches, cells, geometry, owner, covered)
                    }
                }
                if (controller.expanded) Box(Modifier.matchParentSize().blockInput().testTag("palette-history-expanded")) {
                    HistoryRow(controller, view, cells, false, expanded = true,
                        rows = (((bodyHeight + Gap) / (Tile + Gap)).toInt()).coerceIn(1, HistoryRows))
                }
                if (controller.chooser) PaletteChooser(controller, view, owner, Modifier.matchParentSize())
            }
            Column(Modifier.onSizeChanged { bottom = it.height / density }) {
                HorizontalDivider(Modifier.padding(vertical = 6.dp).testTag("palette-footer-divider"), color = colors.divider)
                PaletteFooter(controller, view, selected)
                controller.message?.let { (text, error) ->
                    Text(text, Modifier.fillMaxWidth().padding(top = 4.dp).testTag("palette-message"),
                        color = if (error) MaterialTheme.colorScheme.error else colors.secondary)
                }
            }
        }
        controller.drag?.takeIf { it.started && it.geometry === geometry }?.let { drag ->
            LaunchedEffect(drag) {
                var last = 0L
                while (controller.drag === drag) {
                    val now = withFrameNanos { it }
                    val elapsed = if (last == 0L) 0f else ((now - last).coerceIn(0, 50_000_000) / 1e9f)
                    last = now
                    val viewport = geometry.viewport
                    val edge = 20f * density; val outside = 12f * density
                    val y = drag.point.y
                    if (drag.point.x < viewport.left || drag.point.x > viewport.right) continue
                    val delta = when {
                        y < viewport.top + edge && y >= viewport.top - outside -> -240f * density * elapsed
                        y > viewport.bottom - edge && y <= viewport.bottom + outside -> 240f * density * elapsed
                        else -> 0f
                    }
                    if (delta != 0f && scroll.dispatchRawDelta(delta) != 0f) controller.retarget(drag)
                }
            }
        }
        controller.menu?.takeIf { it.owner === owner }?.let { menu ->
            Box(Modifier.offset { (menu.point - panelOrigin).round() }.size(1.dp)) {
                WorkspaceMenu(host, obj("sections" to menu.sections), preserveContact = controller.contactHeld,
                    command = controller::command, dismiss = controller::closeMenu)
            }
        }
    }
}

private fun PaletteCells.grid(count: Int) = height(rows(count).coerceIn(2, GridRows))
private fun paletteContent(cells: PaletteCells, count: Int, top: Float, bottom: Float): PanelContentSize? =
    if (top <= 0f || bottom <= 0f) null else PanelContentSize(top + bottom + 12f + cells.grid(count))

@Composable internal fun PaletteMeasurement(host: CanvasHost, onContent: (PanelContentSize) -> Unit) {
    val controller = host.palettes
    val view = host.panelContent?.objectOrNull("palette_panel") ?: return
    val density = LocalDensity.current.density
    val count = view.array("swatches").length() + 1
    val report by rememberUpdatedState(onContent)
    Layout({
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val cells = remember(maxWidth.value, density) { PaletteCells((maxWidth.value - 16f).coerceAtLeast(Tile), density) }
            Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 6.dp)) {
                HistoryRow(controller, view, cells, covered = true, expanded = false, rows = 1)
                HorizontalDivider(Modifier.padding(vertical = 6.dp))
                HorizontalDivider(Modifier.padding(vertical = 6.dp))
                PaletteFooter(controller, view, controller.selection(view), static = true)
                controller.message?.let { Text(it.first, Modifier.padding(top = 4.dp)) }
            }
        }
    }) { measurables, constraints ->
        val fixed = measurables.maxOf { it.measure(Constraints(maxWidth = constraints.maxWidth)).height } / density
        val cells = PaletteCells((constraints.maxWidth / density - 16f).coerceAtLeast(Tile), density)
        report(PanelContentSize(fixed + cells.grid(count)))
        layout(0, 0) {}
    }
}

private fun Offset.round() = IntOffset(x.roundToInt(), y.roundToInt())

private fun Modifier.blockInput() = pointerInput(Unit) { awaitPointerEventScope { while (true) awaitPointerEvent() } }

private fun DrawScope.checker() {
    val tile = 5.dp.toPx()
    for (y in 0..ceil(size.height / tile).toInt()) for (x in 0..ceil(size.width / tile).toInt())
        drawRect(if ((x + y) % 2 == 0) Color(0xffcccccc) else Color(0xff8c8c8c), Offset(x * tile, y * tile), Size(tile, tile))
}

@Composable internal fun PaletteFace(color: Color, modifier: Modifier = Modifier, shape: Shape = ControlShape) {
    Canvas(modifier.clip(shape)) { if (color.alpha < 1f) checker(); drawRect(color) }
}

@Composable private fun CellLayout(cells: PaletteCells, count: Int, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Layout(content, modifier) { measurables, constraints ->
        val placeables = measurables.mapIndexed { index, it ->
            val width = (cells.width(index) * cells.density).roundToInt().coerceAtLeast(1)
            val height = (Tile * cells.density).roundToInt()
            it.measure(Constraints.fixed(width, height))
        }
        val height = (cells.height(cells.rows(count)) * cells.density).roundToInt()
        layout(constraints.maxWidth, height) { placeables.forEachIndexed { index, p -> p.place(cells.offset(index)) } }
    }
}

@Composable private fun HistoryRow(controller: PaletteController, view: JSONObject, cells: PaletteCells, covered: Boolean, expanded: Boolean, rows: Int) {
    val history = view.array("history").objects()
    val capacity = cells.columns * rows
    CellLayout(cells, capacity, Modifier.fillMaxWidth().height(cells.height(rows).dp).testTag(if (expanded) "palette-history-grid" else "palette-history")) {
        repeat(capacity - 1) { index ->
            val tile = history.getOrNull(index)
            if (tile != null) key(tile.getJSONObject("color").toString()) { HistoryTile(controller, tile, covered) }
            else if (history.isEmpty() && index < 5) Box(Modifier.padding(3.dp).clip(ControlShape).background(LocalPalette.current.text.copy(alpha = .05f))
                .testTag("palette-empty-$index").semantics { contentDescription = "Colors appear here after painting" })
            else Spacer(Modifier)
        }
        PaletteButton("chevron-down", if (expanded) "Collapse color history" else "Expand color history",
            Modifier.testTag(if (expanded) "palette-history-collapse" else "palette-history-expand"), enabled = !covered, flipped = expanded) {
            controller.chooser = false; controller.expanded = !expanded
        }
    }
}

@Composable private fun PaletteButton(icon: String, label: String, modifier: Modifier = Modifier, enabled: Boolean = true,
    background: Color = Color.Transparent, flipped: Boolean = false, onClick: () -> Unit) {
    val controller = LocalCanvasHost.current.palettes
    HoverTip(label, modifier) {
        Box(Modifier.fillMaxSize().paletteFocus(controller).clip(ControlShape).background(background).alpha(if (enabled) 1f else .4f)
            .clickable(enabled = enabled, role = Role.Button, onClickLabel = label, onClick = onClick)
            .semantics { contentDescription = label }, contentAlignment = Alignment.Center) { SharedIcon(icon, null, Modifier.size(16.dp).rotate(if (flipped) 180f else 0f)) }
    }
}

@Composable private fun HistoryTile(controller: PaletteController, tile: JSONObject, covered: Boolean) {
    val detail = tile.getString("detail")
    val hover = remember { MutableInteractionSource() }
    val hovered by hover.collectIsHoveredAsState()
    HoverTip(detail, enabled = controller.drag?.started != true) {
        Box(Modifier.fillMaxSize().testTag("palette-recent-color").paletteFocus(controller).clip(ControlShape)
            .background(if (hovered && controller.drag?.started != true) LocalPalette.current.text.copy(alpha = .10f) else Color.Transparent)
            .hoverable(hover).clickable(enabled = !covered, role = Role.Button, onClickLabel = detail) { controller.useDefinition(tile.getJSONObject("color")) }
            .semantics { contentDescription = detail }.padding(3.dp)) {
            PaletteFace(tile.getJSONArray("rgba").paint(), Modifier.fillMaxSize())
        }
    }
}

@Composable private fun SwatchGrid(controller: PaletteController, view: JSONObject, swatches: List<JSONObject>, cells: PaletteCells,
    geometry: PaletteGeometry, owner: Any, covered: Boolean) {
    val drag = controller.drag?.takeIf { it.geometry === geometry }
    val viewOrder = swatches.map { it.getLong("id") }
    val order = drag?.order ?: controller.settle?.takeIf { it != viewOrder }
    val selected = controller.selection(view)?.getLong("id")
    CellLayout(cells, swatches.size + 1, Modifier.fillMaxWidth().onGloballyPositioned { geometry.origin = it.positionInRoot() }) {
        swatches.forEachIndexed { index, swatch ->
            val id = swatch.getLong("id")
            key(id) {
                val target = order?.indexOf(id)?.takeIf { it >= 0 }?.let { cells.offset(it) - cells.offset(index) } ?: IntOffset.Zero
                SwatchTile(controller, view, swatch, index, target, selected == id, drag?.id == id && drag.started, geometry, owner, covered, viewOrder)
            }
        }
        key("add") {
            PaletteButton("plus", "Add current color to this palette", Modifier.testTag("palette-add-color"),
                enabled = view.getBoolean("can_name") && !covered, background = LocalPalette.current.input) {
                if (controller.editing) controller.commitName(controller.editText) { controller.store() } else controller.store()
            }
        }
    }
}

@Composable private fun SwatchTile(controller: PaletteController, view: JSONObject, swatch: JSONObject, index: Int, target: IntOffset,
    selected: Boolean, lifted: Boolean, geometry: PaletteGeometry, owner: Any, covered: Boolean, order: List<Long>) {
    val colors = LocalPalette.current
    val id = swatch.getLong("id")
    val detail = swatch.getString("detail")
    val color = swatch.getJSONArray("rgba").paint()
    val offset = remember { Animatable(IntOffset.Zero, IntOffset.VectorConverter) }
    var lastIndex by remember { mutableIntStateOf(index) }
    LaunchedEffect(target, index) {
        if (index != lastIndex) { lastIndex = index; offset.snapTo(target) }
        else offset.animateTo(target, tween(140, easing = EaseOutCubic))
    }
    var bounds by remember { mutableStateOf(Rect.Zero) }
    var origin by remember { mutableStateOf(Offset.Zero) }
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    val focused by interactions.collectIsFocusedAsState()
    val latestView by rememberUpdatedState(view)
    val latestOrder by rememberUpdatedState(order)
    val latestColor by rememberUpdatedState(color)
    val latestSelected by rememberUpdatedState(selected)
    val windowFocused = LocalWindowInfo.current.isWindowFocused
    val requester = remember { FocusRequester() }
    fun menu(point: Offset, held: Boolean) = controller.openMenu(obj("kind" to "color", "id" to id), point, owner, held)
    HoverTip(detail, Modifier.offset { offset.value }, enabled = controller.drag?.started != true) {
        Box(Modifier.fillMaxSize().testTag("palette-swatch-$id").alpha(if (lifted) 0f else 1f)
            .onGloballyPositioned { bounds = it.boundsInRoot(); origin = it.positionInRoot() }
            .semantics { contentDescription = detail; this.selected = selected; role = Role.Button; onClick { controller.use(id); true } }
            .paletteFocus(controller).focusRequester(requester).hoverable(interactions).focusable(!covered, interactions)
            .paletteKeys({ controller.use(id) }, { menu(bounds.center, false) })
            .pointerHoverIcon(PointerIcon(if (lifted) AndroidPointerIcon.TYPE_GRABBING else AndroidPointerIcon.TYPE_GRAB))
            .pointerInput(id, windowFocused, covered) {
                if (!windowFocused || covered) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    controller.keyboard = false
                    if (currentEvent.buttons.isSecondaryPressed) { down.consume(); menu(origin + down.position, false); return@awaitEachGesture }
                    if (controller.drag != null) return@awaitEachGesture
                    down.consume()
                    requester.requestFocus()
                    val drag = PaletteDrag(id, latestView.getLong("palette"), down.position, size, latestColor, latestOrder, geometry, latestSelected)
                    drag.point = origin + down.position
                    controller.begin(drag)
                    var held = false; var released = false
                    var remaining = viewConfiguration.longPressTimeoutMillis
                    var time = down.uptimeMillis
                    try {
                        while (true) {
                            val event = if (!held && !drag.started) withTimeoutOrNull(remaining) { awaitPointerEvent() } else awaitPointerEvent()
                            if (controller.drag !== drag) break
                            if (event == null) {
                                held = true
                                if (down.type != PointerType.Mouse) menu(drag.point, true)
                                continue
                            }
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            remaining = (remaining - (change.uptimeMillis - time)).coerceAtLeast(1); time = change.uptimeMillis
                            if (!change.pressed && change.isConsumed) break
                            val point = origin + change.position
                            if (!drag.started && (change.position - down.position).getDistance() > viewConfiguration.touchSlop) controller.start(drag)
                            controller.move(drag, point)
                            change.consume()
                            if (!change.pressed) {
                                released = true
                                if (drag.started) controller.drop(drag)
                                else { controller.end(drag); if (!held && bounds.contains(point)) controller.use(id) }
                                break
                            }
                        }
                    } finally {
                        if (!released) { controller.end(drag); controller.closeMenu() }
                        controller.contactHeld = false
                    }
                }
            }.clip(ControlShape).background(if (hovered && controller.drag?.started != true) colors.text.copy(alpha = .10f) else Color.Transparent)
            .then(if (selected || (focused && controller.keyboard)) Modifier.border(2.dp, if (selected) colors.accent else colors.accent.copy(alpha = .6f), ControlShape) else Modifier)
            .padding(3.dp)) {
            PaletteFace(color, Modifier.fillMaxSize())
        }
    }
}

@Composable private fun PaletteFooter(controller: PaletteController, view: JSONObject, selected: JSONObject?, static: Boolean = false) {
    val colors = LocalPalette.current
    val name = selected?.getString("name") ?: view.getString("color_name")
    Row(Modifier.fillMaxWidth().testTag("palette-footer"), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        HoverTip("Choose a palette · ${view.getString("name")}") {
            Row(Modifier.heightIn(min = 24.dp).widthIn(max = 150.dp).clip(ControlShape).paletteFocus(controller)
                .clickable(role = Role.Button, onClickLabel = "Choose a palette") {
                    if (controller.editing) controller.commitName(controller.editText)
                    controller.expanded = false; controller.chooser = !controller.chooser
                }.testTag("palette-chooser").padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(view.getString("name"), Modifier.weight(1f, fill = false).testTag("palette-selector-name"), maxLines = 1, overflow = TextOverflow.Ellipsis)
                SharedIcon("chevron-down", null, Modifier.size(12.dp).rotate(180f))
            }
        }
        Column(Modifier.weight(1f), horizontalAlignment = Alignment.End) {
            if (controller.editing && !static) NameEditor(controller, name)
            else HoverTip("$name · Click to rename") {
                Text(name, Modifier.heightIn(min = 24.dp).clip(ControlShape).alpha(if (view.getBoolean("can_name")) 1f else .4f).paletteFocus(controller)
                    .clickable(enabled = view.getBoolean("can_name"), role = Role.Button, onClickLabel = "Name this color") {
                        controller.beginEditing()
                    }.testTag("palette-color-name").padding(horizontal = 2.dp, vertical = 3.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            HoverTip("sRGB hex preview; saved colors retain their original color space, alpha and HDR intensity") {
                Text(view.getString("color_detail"), Modifier.padding(end = 2.dp).testTag("palette-color-detail"),
                    color = colors.secondary, fontSize = LocalTextStyle.current.fontSize * .9f, maxLines = 1)
            }
        }
    }
}

@Composable private fun NameEditor(controller: PaletteController, initial: String) {
    val colors = LocalPalette.current
    val host = LocalCanvasHost.current
    var value by remember { mutableStateOf(TextFieldValue(initial, TextRange(0, initial.length))) }
    val focus = remember { FocusRequester() }
    var hadFocus by remember { mutableStateOf(false) }
    SideEffect { controller.editText = value.text }
    LaunchedEffect(Unit) { focus.requestFocus() }
    DisposableEffect(Unit) { onDispose { host.editingText = false } }
    BasicTextField(value, { if (it.text.length <= 64) value = it },
        Modifier.widthIn(min = 64.dp, max = 180.dp).height(24.dp).paletteFocus(controller).focusRequester(focus).testTag("palette-name-editor")
            .background(colors.input, ControlShape)
            .then(if (controller.message?.second == true) Modifier.border(1.dp, Color(0xffee5555), ControlShape) else Modifier)
            .onPreviewKeyEvent { event ->
                if (event.key == Key.Escape && event.type == KeyEventType.KeyDown) { controller.editing = false; controller.message = null; true } else false
            }
            .onFocusChanged {
                if (hadFocus && !it.isFocused) controller.commitName(value.text)
                hadFocus = it.isFocused; host.editingText = it.isFocused
            }.padding(horizontal = 6.dp, vertical = 3.dp),
        singleLine = true, textStyle = LocalTextStyle.current.copy(color = colors.text), cursorBrush = SolidColor(colors.accent),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done), keyboardActions = KeyboardActions(onDone = { controller.commitName(value.text) }))
}

@Composable private fun PaletteChooser(controller: PaletteController, view: JSONObject, owner: Any, modifier: Modifier) {
    val colors = LocalPalette.current
    var query by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }
    var addBounds by remember { mutableStateOf(Rect.Zero) }
    LaunchedEffect(Unit) { if (controller.keyboard) focus.requestFocus() }
    val palettes = view.array("palettes").objects()
    val matches = palettes.filter { it.getString("name").lowercase().contains(query.trim().lowercase()) }
    Column(modifier.blockInput().testTag("palette-browser"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            CoreTextField(query, { query = it }, Modifier.weight(1f).paletteFocus(controller).focusRequester(focus).testTag("palette-search"), height = 24.dp,
                shape = ControlShape, placeholder = { Text("Find a palette") }, leadingIcon = { SharedIcon("search", null, Modifier.size(14.dp)) })
            PaletteButton("plus", "New or import palette", Modifier.size(24.dp).onGloballyPositioned { addBounds = it.boundsInRoot() }
                .testTag("palette-library-add")) {
                controller.openMenu(obj("kind" to "library"), addBounds.bottomLeft, owner, false)
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
            LazyColumn(Modifier.fillMaxSize().testTag("palette-list")) {
                items(matches, key = { it.getLong("id") }) { palette -> PaletteRow(controller, palette, owner) }
            }
            if (matches.isEmpty()) Text("No matching palettes", Modifier.align(Alignment.Center).testTag("palette-empty-search"), color = colors.secondary)
        }
    }
}

@Composable private fun PaletteRow(controller: PaletteController, palette: JSONObject, owner: Any) {
    val colors = LocalPalette.current
    val id = palette.getLong("id")
    val active = palette.getBoolean("active")
    var bounds by remember { mutableStateOf(Rect.Zero) }
    var origin by remember { mutableStateOf(Offset.Zero) }
    val windowFocused = LocalWindowInfo.current.isWindowFocused
    fun choose() { controller.apply(obj("op" to "select_palette", "id" to id)) { error -> if (error == null) controller.chooser = false } }
    fun menu(point: Offset, held: Boolean) = controller.openMenu(obj("kind" to "palette", "id" to id), point, owner, held)
    Row(Modifier.fillMaxWidth().heightIn(min = 32.dp).testTag("palette-choice-$id").onGloballyPositioned { bounds = it.boundsInRoot(); origin = it.positionInRoot() }
        .semantics { selected = active; role = Role.Button; contentDescription = palette.getString("name"); onClick { choose(); true } }
        .paletteFocus(controller).focusable().paletteKeys(::choose) { menu(bounds.center, false) }
        .pointerInput(id, windowFocused) {
            if (!windowFocused) return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                controller.keyboard = false
                if (currentEvent.buttons.isSecondaryPressed) { down.consume(); menu(origin + down.position, false); return@awaitEachGesture }
                val direct = down.type != PointerType.Mouse
                var held = false
                var remaining = viewConfiguration.longPressTimeoutMillis
                var time = down.uptimeMillis
                try {
                    while (true) {
                        val event = if (direct && !held) withTimeoutOrNull(remaining) { awaitPointerEvent() } else awaitPointerEvent()
                        if (event == null) { held = true; menu(origin + down.position, true); continue }
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        remaining = (remaining - (change.uptimeMillis - time)).coerceAtLeast(1); time = change.uptimeMillis
                        if (change.isConsumed && !held) break
                        val moved = (change.position - down.position).getDistance() > viewConfiguration.touchSlop
                        if (held) { change.consume(); if (moved) controller.closeMenu() }
                        else if (moved) break
                        if (!change.pressed) { if (!held && bounds.contains(origin + change.position)) { change.consume(); choose() }; break }
                    }
                } finally { controller.contactHeld = false }
            }
        }.clip(ControlShape).background(if (active) colors.active else Color.Transparent).padding(horizontal = 4.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(palette.getString("name"), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(Modifier.clip(RoundedCornerShape(3.dp)).testTag("palette-preview-$id")) {
            palette.array("preview").values().forEach { rgba -> PaletteFace((rgba as JSONArray).paint(), Modifier.size(12.dp, 18.dp), RectangleShape) }
        }
    }
}

@Composable internal fun PaletteDragOverlay(host: CanvasHost, origin: Offset) {
    val drag = host.palettes.drag?.takeIf { it.started } ?: return
    val density = LocalDensity.current.density
    Box(Modifier.offset { (drag.point - drag.grab - origin).round() }.size((drag.size.width / density).dp, (drag.size.height / density).dp)
        .zIndex(Float.MAX_VALUE).testTag("palette-drag-preview").shadow(8.dp, ControlShape).background(LocalPalette.current.panel, ControlShape)
        .then(if (drag.selected) Modifier.border(2.dp, LocalPalette.current.accent, ControlShape) else Modifier).padding(3.dp)) {
        PaletteFace(drag.color, Modifier.fillMaxSize())
    }
}

@Composable internal fun PaletteFiles(host: CanvasHost) {
    val controller = host.palettes
    val context = LocalContext.current
    val resolver = context.contentResolver
    var pending by remember { mutableStateOf<Pair<JSONObject, ByteArray>?>(null) }
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) host.viewModelScope.launch {
            try {
                val action = withContext(Dispatchers.IO) {
                    val limit = JSONObject(Native.paletteFile(obj("type" to "limits").toString(), byteArrayOf())[0] as String).getInt("read_bytes")
                    val name = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                        if (it.moveToFirst()) it.getString(0) else null
                    } ?: "Imported palette"
                    val bytes = resolver.openInputStream(uri)?.use { input ->
                        val buffer = ByteArray(limit); var length = 0
                        while (length < buffer.size) { val n = input.read(buffer, length, buffer.size - length); if (n < 0) break; length += n }
                        buffer.copyOf(length)
                    } ?: error("Could not read the palette file")
                    JSONObject(Native.paletteFile(obj("type" to "import", "file_name" to name).toString(), bytes)[0] as String).getJSONObject("action")
                }
                controller.apply(action) { error -> if (error == null) controller.chooser = false }
            } catch (e: Exception) { controller.message = (e.message ?: "Could not import the palette") to true }
        }
    }
    val exporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val export = pending; pending = null
        if (uri != null && export != null) host.viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    (resolver.openOutputStream(uri, "wt") ?: error("Could not open the export destination")).use { it.write(export.second) }
                }
                controller.message = export.first.optString("notice").takeUnless { export.first.isNull("notice") }?.let { it to false }
            } catch (e: Exception) { controller.message = (e.message ?: "Could not export the palette") to true }
        }
    }
    LaunchedEffect(controller.importRequest) { if (controller.importRequest > 0) importer.launch(arrayOf("*/*")) }
    LaunchedEffect(controller.exportRequest) {
        val request = controller.exportRequest ?: return@LaunchedEffect
        controller.exportRequest = null
        val palette = controller.state()?.getJSONObject("colors")?.getJSONObject("library")?.array("palettes")?.objects()
            ?.firstOrNull { it.getLong("id") == request.getLong("id") } ?: return@LaunchedEffect
        try {
            val result = withContext(Dispatchers.Default) {
                Native.paletteFile(obj("type" to "export", "palette" to palette, "format" to request.getString("format")).toString(), byteArrayOf())
            }
            val metadata = JSONObject(result[0] as String)
            pending = metadata to (result[1] as ByteArray)
            exporter.launch(metadata.getString("file_name"))
        } catch (e: Exception) { controller.message = (e.message ?: "Could not export the palette") to true }
    }
    controller.dialog?.let { dialog -> PaletteDialogs(host, dialog) }
}

@Composable private fun PaletteDialogs(host: CanvasHost, dialog: PaletteDialog) {
    val controller = host.palettes
    val close = { controller.dialog = null }
    if (dialog.kind == "remove") {
        AlertDialog(onDismissRequest = close, modifier = Modifier.testTag("palette-remove-dialog"),
            title = { Text("Remove Palette?") }, text = { Text("Remove “${dialog.initial}” and its saved colors?") },
            dismissButton = { TextButton(close) { Text("Cancel") } },
            confirmButton = { TextButton({ close(); controller.apply(obj("op" to "remove_palette", "id" to dialog.id)) },
                colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) { Text("Remove") } })
        return
    }
    var name by remember(dialog) { mutableStateOf(dialog.initial) }
    var error by remember(dialog) { mutableStateOf<String?>(null) }
    fun action() = if (dialog.kind == "rename") obj("op" to "rename_palette", "id" to dialog.id, "name" to name) else obj("op" to "create_palette", "name" to name)
    LaunchedEffect(name) { host.paletteAction(action(), dryRun = true) { error = it } }
    AlertDialog(onDismissRequest = close, modifier = Modifier.testTag("palette-name-dialog"),
        title = { Text(if (dialog.kind == "rename") "Rename Palette" else "New Palette") },
        text = { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CoreTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("palette-library-name"), maxLength = 64, focusRequest = 1)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        } },
        dismissButton = { TextButton(close) { Text("Cancel") } },
        confirmButton = { TextButton({
            controller.apply(action()) { if (it == null) { close(); controller.chooser = false } else error = it }
        }, enabled = error == null, modifier = Modifier.testTag("palette-name-save")) { Text("Save") } })
}
