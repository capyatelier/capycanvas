package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.GenericShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.zIndex
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.viewinterop.AndroidView
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

private suspend fun CanvasHost.awaitQuery(query: JSONObject): JSONObject? = suspendCancellableCoroutine { continuation ->
    query(query) { if (continuation.isActive) continuation.resume(it as? JSONObject) }
}
internal fun Modifier.placed(rect: JSONObject, density: Float): Modifier = offset {
    IntOffset((rect.number("x") * density).roundToInt(), (rect.number("y") * density).roundToInt())
}.size(rect.number("width").coerceAtLeast(0f).dp, rect.number("height").coerceAtLeast(0f).dp)

@Composable private fun floatingBounds(bounds: JSONObject, dragging: Boolean): JSONObject {
    val target = Rect(bounds.number("x"), bounds.number("y"),
        bounds.number("x") + bounds.number("width"), bounds.number("y") + bounds.number("height"))
    val animated = remember { Animatable(target, Rect.VectorConverter) }
    LaunchedEffect(target, dragging) {
        if (dragging) animated.snapTo(target) else animated.animateTo(target, tween(200))
    }
    val r = if (dragging) target else animated.value
    return obj("x" to r.left, "y" to r.top, "width" to r.width, "height" to r.height)
}

@Composable fun CapyApp(host: CanvasHost) {
    val snapshot = host.snapshot
    val state = snapshot?.getJSONObject("state")
    if (state == null) {
        // The session publishes its palette before the native surface attaches.
        // Only the launch/error screen uses the system-themed launch background.
        Box(Modifier.fillMaxSize().background(colorResource(R.color.canvas_launch_background)), contentAlignment = Alignment.Center) {
            host.failure?.let { Text(it, Modifier.padding(24.dp), color = Color.White) }
        }
        return
    }
    val colors = remember(state.getString("theme"), state.getJSONObject("palette").toString()) {
        Palette(state.getString("theme") != "light", state.getJSONObject("palette"))
    }
    val scheme = if (colors.dark) darkColorScheme() else lightColorScheme()
    val activity = LocalActivity.current
    SideEffect {
        activity?.window?.let { window ->
            androidx.core.view.WindowCompat.getInsetsController(window, window.decorView).apply {
                isAppearanceLightStatusBars = !colors.dark
                isAppearanceLightNavigationBars = !colors.dark
            }
        }
    }
    val textSize = (host.catalog.optDouble("text_size_pt", 11.0) * 4 / 3).sp
    val textStyle = TextStyle(fontSize = textSize, lineHeight = 18.sp, letterSpacing = 0.sp)
    val typography = Typography().let { it.copy(bodyLarge = textStyle, bodyMedium = textStyle,
        bodySmall = textStyle, labelLarge = textStyle.copy(fontWeight = FontWeight.Bold),
        labelMedium = textStyle, labelSmall = textStyle,
        titleMedium = textStyle.copy(fontWeight = FontWeight.Bold)) }
    MaterialTheme(typography = typography, colorScheme = scheme.copy(surface = colors.panel, background = colors.surround,
        onSurface = colors.text, onBackground = colors.text, primary = colors.accent,
        onPrimary = Color.White,
        surfaceContainer = colors.panel, surfaceContainerHigh = colors.panel,
        surfaceContainerHighest = colors.tabs, surfaceContainerLow = colors.input,
        surfaceContainerLowest = colors.surround, surfaceTint = Color.Transparent,
        secondaryContainer = colors.active, onSecondaryContainer = colors.text,
        surfaceVariant = colors.tabs, onSurfaceVariant = colors.settingsSecondary, outline = colors.secondary,
        primaryContainer = colors.active, onPrimaryContainer = colors.text)) {
        CompositionLocalProvider(LocalPalette provides colors, LocalCanvasHost provides host, LocalContentColor provides colors.text) {
            ProvideTextStyle(textStyle) {
                // Status/navigation bars overlay this immersive workspace. Their
                // visibility (including startup animations) must never resize the
                // SurfaceView or the shared canvas layout. Protect physical screen
                // obstructions and desktop window captions independently of bars.
                val workspaceInsets = WindowInsets.displayCutout
                    .union(WindowInsets.waterfall).union(WindowInsets.captionBar)
                Box(Modifier.fillMaxSize().background(colors.surround).windowInsetsPadding(workspaceInsets)) {
                    Box(if (snapshot?.objectOrNull("preferences") != null) Modifier.clearAndSetSemantics {} else Modifier) {
                        Workspace(host, snapshot)
                    }
                    host.actionError?.takeIf { snapshot?.objectOrNull("preferences") == null }?.let { message ->
                        AlertDialog(onDismissRequest = host::clearActionError, text = { Text(message) },
                            confirmButton = { TextButton(host::clearActionError) { Text("OK") } })
                    }
                    PreferencesOverlay(host, snapshot?.objectOrNull("preferences"))
                    if (snapshot?.objectOrNull("preferences") == null && snapshot?.objectOrNull("picker") != null)
                        ToolPicker(host, snapshot.getJSONObject("picker"))
                    if (snapshot?.objectOrNull("preferences") == null)
                        snapshot?.objectOrNull("toolbar_manager")?.let { ToolbarManager(host, it) }
                    if (snapshot?.objectOrNull("preferences") == null)
                        snapshot?.objectOrNull("toolbar_prompt")?.let { ToolbarPrompt(host, it) }
                    state?.getJSONObject("customization")?.optString("control")?.takeIf {
                        snapshot?.objectOrNull("preferences") == null && it.isNotEmpty() && it != "null"
                    }?.let { control ->
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
    dock.enabled = snapshot?.optBoolean("partial_zen") != true && snapshot?.objectOrNull("preferences") == null && snapshot?.objectOrNull("picker") == null && snapshot?.objectOrNull("toolbar_prompt") == null && snapshot?.objectOrNull("toolbar_manager") == null
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
    BoxWithConstraints(Modifier.fillMaxSize().testTag("workspace").workspaceGestures(dock)
        .drawWithContent { drawContent(); host.recordUiDraw() }
        .onGloballyPositioned { dock.origin = it.boundsInRoot().topLeft }) {
        dock.viewport = JSONArray(listOf(maxWidth.value, maxHeight.value))
        AndroidView(factory = { CanvasSurfaceView(it, host) }, modifier = Modifier.fillMaxSize())
        // SurfaceView punches through the window background. Cover its empty
        // layer with normal Android UI until this surface has a finished buffer.
        // This requires none of the application's Vulkan shaders.
        if (!host.surfaceReady) {
            Box(Modifier.fillMaxSize().background(colors.surround).testTag("canvas-placeholder"))
        }
        if (snapshot != null && !snapshot.optBoolean("brush_ready") && host.failure == null) {
            Surface(Modifier.align(Alignment.BottomCenter).padding(bottom = 48.dp), shape = RoundedCornerShape(12.dp), tonalElevation = 3.dp) {
                Text(if (snapshot.optBoolean("canvas_ready")) "Preparing brush…" else "Preparing canvas…", Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
            }
        }
        host.failure?.let { message ->
            Surface(Modifier.align(Alignment.Center).widthIn(max = 440.dp).padding(24.dp), shape = RoundedCornerShape(16.dp), shadowElevation = 8.dp) {
                Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Could not initialize canvas", style = MaterialTheme.typography.titleLarge)
                    Text(message)
                }
            }
        }
        if (snapshot != null && state != null) {
            val hidden = snapshot.optBoolean("chrome_hidden")
            if (!hidden && snapshot.objectOrNull("preferences") == null) Header(host, state, dock)
            if (snapshot.objectOrNull("preferences") == null && (!hidden || snapshot.optBoolean("keep_zen_button"))) {
                ZenButton(host, state, dock, hidden)
            }
            val layout = snapshot.getJSONObject("layout")
            if (snapshot.optBoolean("partial_zen")) ZenToolbars(host, snapshot, panels, dock)
            layout.array("groups").objects().filter { !hidden || (it.optBoolean("floating") && !snapshot.optBoolean("hide_floating_panels")) }
                .sortedBy { it.getInt("id") == dock.expansion?.getInt("group") }.forEachIndexed { index, group ->
                key(group.getInt("id")) {
                  CompositionLocalProvider(LocalWorkspaceZ provides index) {
                    val expansion = dock.expansion?.takeIf { it.getInt("group") == group.getInt("id") }
                    val base = group.getJSONObject("bounds")
                    val shown = if (group.optBoolean("floating")) floatingBounds(base, dock.dragging) else base
                    val bounds = expansion?.getJSONObject("bounds") ?: shown
                    val shape = expansion?.takeIf { it.getJSONObject("configuration").number("y") > 0f }
                        ?.let { expandedShape(it, density) } ?: RoundedCornerShape(8.dp)
                    Box(Modifier.placed(bounds, density).zIndex(100f + index).testTag("group-${group.getInt("id")}")
                        .shadow(if (expansion != null) 16.dp else 6.dp, shape).clip(shape)) {
                        val preview = expansion?.getJSONObject("preview")
                        val mod = if (preview == null) Modifier.fillMaxSize() else Modifier.placed(preview, density)
                        PanelGroup(host, state, group, panels, dock, mod)
                        expansion?.getJSONObject("configuration")?.let { rect ->
                            Box(Modifier.placed(rect, density).background(colors.panel)) {
                                panels[group.getString("active")]?.let { ConfigurePanel(host, it) { height -> dock.configurationHeight = height } }
                            }
                        }
                    }
                    if (expansion == null) group.array("resize_handles").objects().forEach { handle ->
                        Box(Modifier.placed(handle.getJSONObject("bounds"), density).zIndex(100f + index)
                            .testTag("resize-${group.getInt("id")}-${handle.getString("edge")}").workspaceSource(dock,
                            obj("type" to "resize_floating", "group" to group.getInt("id"), "edge" to handle.getString("edge")), priority = 4))
                    }
                  }
                }
            }
            if (!hidden) layout.array("dividers").objects().forEach { divider ->
                val rect = divider.getJSONObject("bounds")
                Box(Modifier.placed(rect, density).testTag("divider-${divider.getInt("id")}").workspaceSource(dock,
                    obj("type" to "drag_divider", "id" to divider.getInt("id")), priority = 4))
            }
            if (!hidden) Row(Modifier.placed(layout.getJSONObject("status"), density).padding(horizontal = 4.dp), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.Bottom) {
                Surface(color = colors.surround, shape = RoundedCornerShape(20.dp)) {
                    CameraStatus(host)
                }
            }
        }
        dock.hint?.getJSONObject("bounds")?.let { Box(Modifier.placed(it, density).zIndex(Float.MAX_VALUE)
            .background(colors.accent).testTag("workspace-drop-hint")) }
        dock.contextMenu?.let { menu ->
            val anchor = dock.contextAnchor
            Box(Modifier.offset { IntOffset(anchor.left.roundToInt(), anchor.top.roundToInt()) }
                .size((anchor.width / density).dp, (anchor.height / density).dp)) {
                WorkspaceMenu(host, menu, dock::closeContext)
            }
        }
    }
}

/** Read camera state here so navigation never invalidates the workspace tree. */
@Composable private fun CameraStatus(host: CanvasHost) {
    val camera = host.cameraReadout
    Text("${camera.zoomPercent}% · ${camera.rotationDegrees}°",
        Modifier.testTag("camera-readout").clickable { host.invoke("fit_canvas") }
            .padding(horizontal = 10.dp, vertical = 3.dp))
}

/** One outline/shadow for both columns, with the drawer below the tab strip. */
private fun expandedShape(expansion: JSONObject, density: Float) = GenericShape { size, _ ->
    val preview = expansion.getJSONObject("preview")
    val configuration = expansion.getJSONObject("configuration")
    val left = preview.number("x") * density
    val right = left + preview.number("width") * density
    val top = configuration.number("y") * density
    val radius = minOf(8 * density, size.height / 2, size.width / 2)
    moveTo(left + radius, 0f)
    lineTo(right - radius, 0f); quadraticTo(right, 0f, right, radius)
    if (right < size.width) { lineTo(right, top); lineTo(size.width - radius, top); quadraticTo(size.width, top, size.width, top + radius) }
    lineTo(size.width, size.height - radius); quadraticTo(size.width, size.height, size.width - radius, size.height)
    lineTo(radius, size.height); quadraticTo(0f, size.height, 0f, size.height - radius)
    if (left > 0f) {
        lineTo(0f, top + radius); quadraticTo(0f, top, radius, top)
        if (expansion.optBoolean("concave_join")) { lineTo(left - radius, top); quadraticTo(left, top, left, top - radius) }
        else lineTo(left, top)
    }
    lineTo(left, radius); quadraticTo(left, 0f, left + radius, 0f); close()
}

@Composable private fun ZenButton(host: CanvasHost, state: JSONObject, dock: DockInteraction, hidden: Boolean) {
    val colors = LocalPalette.current
    val command = state.array("commands").objects().first { it.getString("id") == "zen_mode" }
    val target = remember { obj("kind" to "zen_mode") }
    val anchor = dock.anchorKey(target)
    DisposableEffect(dock) { onDispose { dock.anchors.remove(anchor) } }
    IconTile(command.getString("icon"), command.getString("tooltip"), command.getBoolean("selected") && !hidden,
        modifier = Modifier.offset(6.dp, 6.dp).zIndex(1000f).testTag("zen-button")
            .background(colors.surround, RoundedCornerShape(6.dp))
            .onGloballyPositioned { dock.anchors[anchor] = it.boundsInRoot().translate(-dock.origin) }
            .pointerInput(dock) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    if (currentEvent.buttons.isSecondaryPressed) { down.consume(); dock.context(target) }
                }
            },
        onLongClick = { dock.context(target) }, iconSize = host.catalog.getInt("zen_icon_size").dp,
        selectedColor = colors.text.copy(alpha = .08f)) { host.invoke("zen_mode") }
}

@Composable private fun Header(host: CanvasHost, state: JSONObject, dock: DockInteraction) {
    val colors = LocalPalette.current
    BoxWithConstraints(Modifier.fillMaxWidth().height(48.dp).padding(6.dp)) {
      if (maxWidth >= 600.dp) state.array("tabs").optJSONObject(0)?.let { tab ->
        Text("${tab.optString("title")} · ${tab.optInt("width")} × ${tab.optInt("height")}",
            Modifier.align(Alignment.Center).background(colors.surround, RoundedCornerShape(6.dp)).padding(horizontal = 8.dp, vertical = 8.dp),
            fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
      }
      Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        Spacer(Modifier.size(36.dp))
        host.catalog.array("menus").objects().forEach { menu ->
            var open by remember { mutableStateOf(false) }
            DisposableEffect(open) {
                dock.popupOpen = open; dock.refresh()
                onDispose { if (open) { dock.popupOpen = false; dock.refresh() } }
            }
            Box {
                Box(Modifier.height(36.dp).clip(RoundedCornerShape(6.dp)).background(colors.surround)
                    .clickable { open = true }.padding(horizontal = 17.dp), contentAlignment = Alignment.Center) {
                    Text(menu.getString("label"), fontWeight = FontWeight.Bold)
                }
                if (open && menu.array("sections").length() == 0) {
                    host.snapshot?.objectOrNull("workspace_menu")?.let { WorkspaceMenu(host, it) { open = false } }
                } else DropdownMenu(open, { open = false }, shape = RoundedCornerShape(10.dp), containerColor = colors.panel) {
                    menu.array("sections").values().forEachIndexed { index, section ->
                        if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = colors.divider)
                        (section as JSONArray).values().forEach { id ->
                            state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                                Row(Modifier.widthIn(min = 200.dp).fillMaxWidth().heightIn(min = 36.dp)
                                    .padding(horizontal = 6.dp).clip(RoundedCornerShape(6.dp))
                                    .clickable(enabled = command.getBoolean("enabled")) { open = false; host.invoke(id.toString()) }
                                    .padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                                    Text(command.getString("label"), Modifier.weight(1f), fontWeight = FontWeight.Bold,
                                        color = if (command.getBoolean("enabled")) colors.text else colors.secondary)
                                    if (command.optBoolean("selected")) SharedIcon("check", null)
                                    command.optString("shortcut").takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.secondary) }
                                }
                            }
                        }
                    }
                }
            }
        }
        Spacer(Modifier.weight(1f))
        IconTile("settings", state.array("commands").objects().first { it.getString("id") == "settings" }.getString("tooltip")) { host.invoke("settings") }
      }
    }
}

@Composable private fun PanelGroup(host: CanvasHost, state: JSONObject, group: JSONObject,
    panels: Map<String, JSONObject>, dock: DockInteraction, modifier: Modifier) {
    val colors = LocalPalette.current
    val active = group.getString("active")
    val panel = panels[active] ?: return
    val tabsVisible = group.getBoolean("tabs_visible")
    val groupItem = obj("kind" to "group", "group" to group.getInt("id"))
    val measurer = rememberTextMeasurer()
    val textStyle = LocalTextStyle.current.copy(fontWeight = FontWeight.Bold)
    group.array("panels").values().forEach { id ->
        panels[id.toString()]?.let { view ->
            val tab = view.getJSONObject("tab")
            val showIcon = tab.getBoolean("show_icon")
            val showName = tab.getBoolean("show_name")
            val width = 16f + (if (showIcon) 16f else 0f) +
                (if (showName) measurer.measure(AnnotatedString(view.getString("title")), textStyle).size.width / dock.density else 0f) +
                (if (showIcon && showName) 6f else 0f)
            SideEffect { dock.measure(id.toString(), tabWidth = width) }
        }
    }
    DisposableEffect(group.getInt("id"), group.array("panels").toString()) {
        val prefix = "${group.getInt("id")}:"
        onDispose { dock.tabs.keys.removeAll { it.startsWith(prefix) } }
    }
    Surface(modifier, color = colors.panel) {
        Column {
            if (tabsVisible) Row(Modifier.fillMaxWidth().height(36.dp).testTag("group-header-${group.getInt("id")}").background(colors.tabs).dragSource(dock, groupItem)
                .combinedClickable(onClick = { if (panel.optBoolean("expanded")) host.customize(obj("type" to "close_expanded")) },
                    onDoubleClick = { dock.doubleClickHandle(groupItem) }, onLongClick = { dock.context(groupItem) }), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()).clickable(enabled = panel.optBoolean("expanded")) { host.customize(obj("type" to "close_expanded")) }) {
                    group.array("panels").values().forEachIndexed { index, id ->
                        val p = panels[id.toString()] ?: return@forEachIndexed
                        val selected = id == active
                        val content = p.getJSONObject("tab")
                        val tab = Modifier.testTag("tab-$id").dragSource(dock, obj("kind" to "panel", "panel" to id))
                            .onGloballyPositioned { coords ->
                                val r = coords.boundsInRoot(); val pos = (r.topLeft - dock.origin) / dock.density
                                dock.tabs["${group.getInt("id")}:$index"] = obj("group" to group.getInt("id"), "index" to index,
                                    "bounds" to obj("x" to pos.x, "y" to pos.y, "width" to r.width / dock.density, "height" to r.height / dock.density))
                            }.height(36.dp).then(if (!content.getBoolean("show_name")) Modifier.width(36.dp) else Modifier).zIndex(if (selected) 1f else 0f)
                            .drawBehind {
                                if (selected) {
                                    val r = 6.dp.toPx(); val w = size.width; val h = size.height
                                    val path = Path().apply {
                                        moveTo(r, 0f); lineTo(w-r, 0f); quadraticTo(w, 0f, w, r)
                                        lineTo(w, h-r); quadraticTo(w, h, w+r, h)
                                        lineTo(-r, h); quadraticTo(0f, h, 0f, h-r)
                                        lineTo(0f, r); quadraticTo(0f, 0f, r, 0f); close()
                                    }
                                    drawPath(path, colors.panel)
                                }
                            }
                            .combinedClickable(onClick = { host.dispatch(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to id)) },
                                onLongClick = { dock.context(obj("kind" to "panel", "panel" to id)) }).padding(horizontal = 8.dp)
                        Row(tab, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally)) {
                            if (content.getBoolean("show_icon")) SharedIcon(p.getString("icon"), if (content.getBoolean("show_name")) null else p.getString("title"), Modifier.testTag("tab-icon-$id"))
                            if (content.getBoolean("show_name")) Text(p.getString("title"), Modifier.testTag("tab-name-$id"), fontWeight = FontWeight.Bold)
                        }
                    }
                }
                Box(Modifier.width(20.dp).height(36.dp).testTag("group-grip-${group.getInt("id")}")
                    .combinedClickable(onClick = { if (panel.optBoolean("expanded")) host.customize(obj("type" to "close_expanded")) },
                        onDoubleClick = { dock.doubleClickHandle(groupItem) },
                        onLongClick = { dock.context(obj("kind" to "group", "group" to group.getInt("id"))) }), contentAlignment = Alignment.Center) { PanelGrip("Move panel group") }
            }
            Box(Modifier.weight(1f)) {
                if (group.objectOrNull("tiles") != null) ToolRibbon(host, panel, group.getJSONObject("tiles"), dock, Modifier.fillMaxSize(), group.optString("axis") == "vertical")
                else PanelControls(host, state, panel, Modifier.fillMaxSize()) { dock.measure(active, contentHeight = it) }
            }
            group.objectOrNull("footer_grip")?.let { grip ->
                Box(Modifier.fillMaxWidth().height(grip.number("height").dp).testTag("group-grip-${group.getInt("id")}").dragSource(dock, groupItem)
                    .combinedClickable(onClick = {}, onDoubleClick = { dock.doubleClickHandle(groupItem) },
                        onLongClick = { dock.context(groupItem) }), contentAlignment = Alignment.Center) { PanelGrip("Move panel group", vertical = true) }
            }
        }
    }
}
