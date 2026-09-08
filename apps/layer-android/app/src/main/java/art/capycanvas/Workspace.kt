package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.GenericShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

internal class DockInteraction(val host: CanvasHost) {
    var hint by mutableStateOf<JSONObject?>(null)
    var dragging by mutableStateOf(false)
    var expansion by mutableStateOf<JSONObject?>(null)
    var contextMenu by mutableStateOf<JSONObject?>(null)
    var popupOpen = false
    var configurationHeight by mutableStateOf(0f)
    val tabs = mutableMapOf<String, JSONObject>()
    var origin = Offset.Zero
    var density = 1f
    var viewport = JSONArray(listOf(1, 1))
    private var generation = 0
    private var activeItem: JSONObject? = null
    private var position = Offset.Zero
    fun facts() = obj("held" to false, "dragging" to dragging, "popup_open" to (popupOpen || contextMenu != null), "expanded_panel" to expansion)
    fun refresh() = host.chrome(obj("kind" to "refresh"), facts())
    fun context(target: JSONObject) = host.query(obj("type" to "context", "target" to target)) {
        contextMenu = it as? JSONObject; refresh()
    }
    fun closeContext() { contextMenu = null; refresh() }
    private fun query() = obj("type" to "drop", "item" to activeItem, "position" to JSONArray(listOf(position.x, position.y)),
        "tabs" to JSONArray(tabs.values.toList()), "expansion" to expansion)
    fun start(item: JSONObject, point: Offset) {
        generation++; activeItem = item; dragging = true
        host.chrome(obj("kind" to "refresh"), facts())
        move(point)
    }
    fun move(point: Offset) {
        position = point
        val request = ++generation
        host.query(query()) { if (request == generation) hint = it as? JSONObject }
    }
    fun finish(cancel: Boolean) {
        if (!dragging) return
        val item = activeItem!!
        val request = ++generation
        fun end() { hint = null; dragging = false; activeItem = null; host.chrome(obj("kind" to "refresh"), facts()) }
        if (cancel) { end(); return }
        host.query(query()) { value ->
            if (request == generation) {
                val target = (value as? JSONObject)?.getJSONObject("target")
                if (target != null) {
                    val action = JSONObject(item.toString())
                    action.put("type", "move_${item.getString("kind")}"); action.remove("kind")
                    action.put("target", target); action.put("viewport", viewport)
                    host.dispatch(action)
                }
                end()
            }
        }
    }
}

private suspend fun CanvasHost.awaitQuery(query: JSONObject): JSONObject? = suspendCancellableCoroutine { continuation ->
    query(query) { if (continuation.isActive) continuation.resume(it as? JSONObject) }
}

@Composable internal fun dragSource(modifier: Modifier, dock: DockInteraction, item: JSONObject): Modifier {
    var origin by remember { mutableStateOf(Offset.Zero) }
    var point by remember { mutableStateOf(Offset.Zero) }
    return modifier.onGloballyPositioned { origin = (it.boundsInRoot().topLeft - dock.origin) / dock.density }
        .pointerInput(item.toString()) {
            detectDragGestures(onDragStart = { local -> point = origin + local / dock.density; dock.start(item, point) },
                onDragCancel = { dock.finish(true) }, onDragEnd = { dock.finish(false) }) { change, amount ->
                change.consume(); point += amount / dock.density; dock.move(point)
            }
        }
}
internal fun Modifier.placed(rect: JSONObject, density: Float): Modifier = offset {
    IntOffset((rect.number("x") * density).roundToInt(), (rect.number("y") * density).roundToInt())
}.size(rect.number("width").coerceAtLeast(0f).dp, rect.number("height").coerceAtLeast(0f).dp)

@Composable fun CapyApp(host: CanvasHost) {
    val snapshot = host.snapshot
    val state = snapshot?.getJSONObject("state")
    val colors = Palette(state?.optString("theme") != "light")
    val scheme = if (colors.dark) darkColorScheme() else lightColorScheme()
    val activity = LocalContext.current as? android.app.Activity
    SideEffect {
        activity?.window?.let { window ->
            androidx.core.view.WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = !colors.dark
                isAppearanceLightNavigationBars = !colors.dark
            }
        }
    }
    MaterialTheme(colorScheme = scheme.copy(surface = colors.panel, background = colors.surround,
        onSurface = colors.text, onBackground = colors.text, primary = colors.accent,
        onPrimary = if (colors.dark) colors.surround else Color.White,
        surfaceContainer = colors.panel, surfaceContainerHigh = colors.panel,
        surfaceContainerHighest = colors.tabs, surfaceContainerLow = colors.input,
        surfaceContainerLowest = colors.surround, surfaceTint = Color.Transparent,
        secondaryContainer = colors.active, onSecondaryContainer = colors.text,
        surfaceVariant = colors.tabs, onSurfaceVariant = colors.secondary, outline = colors.secondary,
        primaryContainer = colors.active, onPrimaryContainer = colors.text)) {
        CompositionLocalProvider(LocalPalette provides colors, LocalCanvasHost provides host) {
            ProvideTextStyle(MaterialTheme.typography.bodyMedium.copy(fontSize = (host.catalog.optDouble("text_size_pt", 11.0) * 4 / 3).sp, color = colors.text)) {
                Box(Modifier.fillMaxSize().background(colors.surround).windowInsetsPadding(WindowInsets.safeDrawing.exclude(WindowInsets.ime))) {
                    Workspace(host, snapshot)
                    host.actionError?.let { message ->
                        AlertDialog(onDismissRequest = host::clearActionError, text = { Text(message) },
                            confirmButton = { TextButton(host::clearActionError) { Text("OK") } })
                    }
                    if (snapshot?.objectOrNull("preferences") != null) PreferencesScreen(host, snapshot.getJSONObject("preferences"))
                    if (snapshot?.objectOrNull("picker") != null) ToolPicker(host, snapshot.getJSONObject("picker"))
                    state?.getJSONObject("customization")?.optString("control")?.takeIf { it.isNotEmpty() && it != "null" }?.let { control ->
                        AlertDialog(onDismissRequest = { host.customize(obj("type" to "close_control")) },
                            title = { Text(if (control == "brush_color") "Color" else "Opacity") },
                            text = { Column {
                                if (control == "brush_color") ColorControls(host, state.getJSONObject("brush").array("color"))
                                else NumericSetting("Opacity", state.getJSONObject("brush").number("opacity"), host.catalog.getJSONObject("opacity")) {
                                    host.dispatch(obj("type" to "set_brush_opacity", "value" to it))
                                }
                            } }, confirmButton = { TextButton({ host.customize(obj("type" to "close_control")) }) { Text("Done") } })
                    }
                }
            }
        }
    }
}

@Composable private fun Workspace(host: CanvasHost, snapshot: JSONObject?) {
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val dock = remember(host) { DockInteraction(host) }
    dock.density = density
    val panels = snapshot?.array("panels")?.objects()?.associateBy { it.getString("id") } ?: emptyMap()
    val state = snapshot?.getJSONObject("state")
    val expanded = state?.getJSONObject("customization")?.opt("expanded")?.takeIf { it != JSONObject.NULL } as? String
    var shownPanel by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(expanded, snapshot?.getJSONObject("layout")?.toString(), dock.configurationHeight) {
        val panel = expanded ?: shownPanel
        if (panel != null) {
            shownPanel = panel
            val from = dock.expansion
            val start = withFrameNanos { it }
            val duration = host.catalog.optLong("panel_expansion_ms", 200) * 1_000_000f
            do {
                val progress = ((withFrameNanos { it } - start) / duration).coerceIn(0f, 1f)
                dock.expansion = host.awaitQuery(obj("type" to "expansion", "panel" to panel,
                    "heights" to JSONArray(listOf(0f, dock.configurationHeight)), "progress" to progress,
                    "from" to from, "closing" to (expanded == null)))
                dock.refresh()
            } while (progress < 1f)
            if (expanded == null) { dock.expansion = null; shownPanel = null; dock.refresh() }
        }
    }
    BackHandler(expanded != null) { host.customize(obj("type" to "close_expanded")) }
    BoxWithConstraints(Modifier.fillMaxSize().onGloballyPositioned { dock.origin = it.boundsInRoot().topLeft }) {
        dock.viewport = JSONArray(listOf(maxWidth.value, maxHeight.value))
        AndroidView(factory = { CanvasSurfaceView(it, host) }, modifier = Modifier.fillMaxSize())
        host.failure?.let { message ->
            Surface(Modifier.align(Alignment.Center).widthIn(max = 440.dp).padding(24.dp), shape = RoundedCornerShape(16.dp), shadowElevation = 8.dp) {
                Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Could not initialize canvas", style = MaterialTheme.typography.titleLarge)
                    Text(message)
                }
            }
        }
        if (snapshot != null && !snapshot.optBoolean("chrome_hidden")) {
            Header(host, state!!, dock)
            val layout = snapshot.getJSONObject("layout")
            layout.array("groups").objects().sortedBy { it.getInt("id") == dock.expansion?.getInt("group") }.forEach { group ->
                key(group.getInt("id")) {
                    val expansion = dock.expansion?.takeIf { it.getInt("group") == group.getInt("id") }
                    val bounds = expansion?.getJSONObject("bounds") ?: group.getJSONObject("bounds")
                    val shape = expansion?.let { expandedShape(it, density) } ?: RoundedCornerShape(10.dp)
                    Box(Modifier.placed(bounds, density).shadow(if (expansion != null) 16.dp else 6.dp, shape).clip(shape)) {
                        val preview = expansion?.getJSONObject("preview")
                        val mod = if (preview == null) Modifier.fillMaxSize() else Modifier.placed(preview, density)
                        PanelGroup(host, state, group, panels, dock, mod)
                        expansion?.getJSONObject("configuration")?.let { rect ->
                            Box(Modifier.placed(rect, density).background(colors.panel)) {
                                panels[group.getString("active")]?.let { ConfigurePanel(host, it) { height -> dock.configurationHeight = height } }
                            }
                        }
                    }
                }
            }
            layout.array("dividers").objects().forEach { divider ->
                val rect = divider.getJSONObject("bounds")
                var origin by remember(divider.getInt("id")) { mutableStateOf(Offset.Zero) }
                var point by remember { mutableStateOf(Offset.Zero) }
                fun resize(phase: String) = host.dispatch(obj("type" to "drag_divider", "id" to divider.getInt("id"),
                    "phase" to phase, "position" to JSONArray(listOf(point.x, point.y)), "viewport" to dock.viewport))
                Box(Modifier.placed(rect, density).testTag("divider-${divider.getInt("id")}").onGloballyPositioned { origin = (it.boundsInRoot().topLeft - dock.origin) / density }
                    .pointerInput(divider.getInt("id")) {
                        detectDragGestures(onDragStart = { point = origin + it / density; resize("down") },
                            onDragEnd = { resize("up") }, onDragCancel = { resize("cancel") }) { change, amount ->
                            change.consume(); point += amount / density; resize("move")
                        }
                    })
            }
            val camera = state.getJSONObject("camera")
            Row(Modifier.placed(layout.getJSONObject("status"), density), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                Surface(color = colors.surround, shape = RoundedCornerShape(6.dp)) {
                    Text("${(camera.number("zoom", 1.0) * 100).roundToInt()}%  ·  ${(camera.number("rotation") * 180 / Math.PI).roundToInt()}°",
                        Modifier.clickable { host.invoke("fit_canvas") }.padding(horizontal = 10.dp, vertical = 2.dp))
                }
            }
        }
        dock.hint?.getJSONObject("bounds")?.let { Box(Modifier.placed(it, density).background(colors.accent)) }
    }
    dock.contextMenu?.let { menu ->
        AlertDialog(onDismissRequest = dock::closeContext, title = { Text(menu.getString("title")) }, text = {
            Column {
                menu.array("sections").values().forEachIndexed { index, section ->
                    if (index > 0) HorizontalDivider()
                    (section as JSONArray).objects().forEach { item ->
                        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clickable {
                            host.customize(item.getJSONObject("action")); dock.closeContext()
                        }, verticalAlignment = Alignment.CenterVertically) {
                            if (!item.isNull("selected")) RadioButton(item.getBoolean("selected"), onClick = null)
                            Text(item.getString("label"), Modifier.padding(8.dp))
                        }
                    }
                }
            }
        }, confirmButton = { TextButton(dock::closeContext) { Text("Done") } })
    }
}

/** One outline/shadow for both columns, with the drawer below the tab strip. */
private fun expandedShape(expansion: JSONObject, density: Float) = GenericShape { size, _ ->
    val preview = expansion.getJSONObject("preview")
    val configuration = expansion.getJSONObject("configuration")
    val left = preview.number("x") * density
    val right = left + preview.number("width") * density
    val top = configuration.number("y") * density
    val radius = minOf(10 * density, size.height / 2, size.width / 2)
    moveTo(left + radius, 0f)
    lineTo(right - radius, 0f); quadraticBezierTo(right, 0f, right, radius)
    if (right < size.width) { lineTo(right, top); lineTo(size.width - radius, top); quadraticBezierTo(size.width, top, size.width, top + radius) }
    lineTo(size.width, size.height - radius); quadraticBezierTo(size.width, size.height, size.width - radius, size.height)
    lineTo(radius, size.height); quadraticBezierTo(0f, size.height, 0f, size.height - radius)
    if (left > 0f) {
        lineTo(0f, top + radius); quadraticBezierTo(0f, top, radius, top)
        if (expansion.optBoolean("concave_join")) { lineTo(left - radius, top); quadraticBezierTo(left, top, left, top - radius) }
        else lineTo(left, top)
    }
    lineTo(left, radius); quadraticBezierTo(left, 0f, left + radius, 0f); close()
}

@Composable private fun Header(host: CanvasHost, state: JSONObject, dock: DockInteraction) {
    val colors = LocalPalette.current
    Row(Modifier.fillMaxWidth().height(48.dp).padding(6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        IconTile("zen", "Zen mode", state.getJSONObject("workspace").optBoolean("zen_mode")) { host.invoke("zen_mode") }
        host.catalog.array("menus").objects().forEach { menu ->
            var open by remember { mutableStateOf(false) }
            DisposableEffect(open) {
                dock.popupOpen = open; dock.refresh()
                onDispose { if (open) { dock.popupOpen = false; dock.refresh() } }
            }
            Box {
                Text(menu.getString("label"), Modifier.clickable { open = true }.padding(8.dp))
                DropdownMenu(open, { open = false }) {
                    menu.array("sections").values().forEachIndexed { index, section ->
                        if (index > 0) HorizontalDivider()
                        (section as JSONArray).values().forEach { id ->
                            state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                                DropdownMenuItem(text = { Text(command.getString("label")) }, enabled = command.getBoolean("enabled"),
                                    trailingIcon = { Text(if (command.optBoolean("selected")) "✓" else command.optString("shortcut"), color = colors.secondary) },
                                    onClick = { open = false; host.invoke(id.toString()) })
                            }
                        }
                    }
                }
            }
        }
        Spacer(Modifier.weight(1f))
        Text(state.array("tabs").optJSONObject(0)?.optString("title") ?: "Capy Canvas", color = colors.secondary)
        Spacer(Modifier.weight(1f))
        IconTile("settings", "Preferences") { host.invoke("settings") }
    }
}

@Composable private fun PanelGroup(host: CanvasHost, state: JSONObject, group: JSONObject,
    panels: Map<String, JSONObject>, dock: DockInteraction, modifier: Modifier) {
    val colors = LocalPalette.current
    val active = group.getString("active")
    val panel = panels[active] ?: return
    val tabsVisible = group.getBoolean("tabs_visible")
    DisposableEffect(group.getInt("id"), group.array("panels").toString()) {
        val prefix = "${group.getInt("id")}:"
        onDispose { dock.tabs.keys.removeAll { it.startsWith(prefix) } }
    }
    Surface(modifier, color = colors.panel) {
        Column {
            if (tabsVisible) Row(Modifier.fillMaxWidth().height(36.dp).background(colors.tabs), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()).clickable(enabled = panel.optBoolean("expanded")) { host.customize(obj("type" to "close_expanded")) }) {
                    group.array("panels").values().forEachIndexed { index, id ->
                        val p = panels[id.toString()] ?: return@forEachIndexed
                        val selected = id == active
                        val tab = dragSource(Modifier, dock, obj("kind" to "panel", "panel" to id))
                            .onGloballyPositioned { coords ->
                                val r = coords.boundsInRoot(); val pos = (r.topLeft - dock.origin) / dock.density
                                dock.tabs["${group.getInt("id")}:$index"] = obj("group" to group.getInt("id"), "index" to index,
                                    "bounds" to obj("x" to pos.x, "y" to pos.y, "width" to r.width / dock.density, "height" to r.height / dock.density))
                            }.height(36.dp).background(if (selected) colors.panel else Color.Transparent, RoundedCornerShape(topStart = 8.dp, topEnd = 8.dp))
                            .combinedClickable(onClick = { host.dispatch(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to id)) },
                                onLongClick = { dock.context(obj("kind" to "panel", "panel" to id)) }).padding(horizontal = 10.dp)
                        Box(tab, contentAlignment = Alignment.Center) {
                            if (p.getString("tab_style") == "icon") SharedIcon(p.getString("icon"), p.getString("title")) else Text(p.getString("title"))
                        }
                    }
                }
                Box(dragSource(Modifier.width(24.dp).fillMaxHeight(), dock, obj("kind" to "group", "group" to group.getInt("id")))
                    .combinedClickable(onClick = { if (panel.optBoolean("expanded")) host.customize(obj("type" to "close_expanded")) },
                        onLongClick = { dock.context(obj("kind" to "group", "group" to group.getInt("id"))) }), contentAlignment = Alignment.Center) { SharedIcon("grip", "Move panel group") }
            }
            if (group.objectOrNull("tiles") != null) ToolRibbon(host, panel, group.getJSONObject("tiles"), dock, Modifier.fillMaxSize())
            else PanelControls(host, state, panel, false, Modifier.fillMaxSize())
        }
    }
}
