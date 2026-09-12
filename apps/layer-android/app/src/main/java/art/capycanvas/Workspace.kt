package art.capycanvas

import android.view.PointerIcon as AndroidPointerIcon
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.zIndex
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.colorResource
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.viewinterop.AndroidView
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

internal suspend fun CanvasHost.awaitQuery(query: JSONObject): JSONObject? = suspendCancellableCoroutine { continuation ->
    query(query) { if (continuation.isActive) continuation.resume(it as? JSONObject) }
}
internal fun Modifier.placed(rect: JSONObject, density: Float): Modifier = offset {
    IntOffset((rect.number("x") * density).roundToInt(), (rect.number("y") * density).roundToInt())
}.size(rect.number("width").coerceAtLeast(0f).dp, rect.number("height").coerceAtLeast(0f).dp)

@Composable private fun floatingBounds(bounds: JSONObject, dragging: Boolean, host: CanvasHost, group: Int): JSONObject {
    val target = Rect(bounds.number("x"), bounds.number("y"),
        bounds.number("x") + bounds.number("width"), bounds.number("y") + bounds.number("height"))
    val animated = remember { Animatable(target, Rect.VectorConverter) }
    val wasDragging = remember { mutableStateOf(false) }
    LaunchedEffect(target, dragging) {
        val finishedDrag = wasDragging.value && !dragging
        wasDragging.value = dragging
        if (dragging) animated.snapTo(target) else {
            if (finishedDrag) host.lastWorkspaceGroup?.takeIf { it.first == group }?.let { animated.snapTo(it.second) }
            animated.animateTo(target, tween(200))
        }
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
    val scheme = remember(colors) {
        (if (colors.dark) darkColorScheme() else lightColorScheme()).copy(surface = colors.panel, background = colors.surround,
            onSurface = colors.text, onBackground = colors.text, primary = colors.accent,
            onPrimary = Color.White,
            surfaceContainer = colors.panel, surfaceContainerHigh = colors.panel,
            surfaceContainerHighest = colors.tabs, surfaceContainerLow = colors.input,
            surfaceContainerLowest = colors.surround, surfaceTint = Color.Transparent,
            secondaryContainer = colors.active, onSecondaryContainer = colors.text,
            surfaceVariant = colors.tabs, onSurfaceVariant = colors.settingsSecondary, outline = colors.secondary,
            primaryContainer = colors.active, onPrimaryContainer = colors.text)
    }
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
    val textStyle = remember(textSize) { TextStyle(fontSize = textSize, lineHeight = 18.sp, letterSpacing = 0.sp) }
    val typography = remember(textStyle) { Typography().copy(bodyLarge = textStyle, bodyMedium = textStyle,
        bodySmall = textStyle, labelLarge = textStyle.copy(fontWeight = FontWeight.Bold),
        labelMedium = textStyle, labelSmall = textStyle,
        titleMedium = textStyle.copy(fontWeight = FontWeight.Bold)) }
    MaterialTheme(typography = typography, colorScheme = scheme) {
        CompositionLocalProvider(LocalPalette provides colors, LocalCanvasHost provides host, LocalContentColor provides colors.text) {
            ProvideTextStyle(textStyle) {
                DocumentRequests(host)
                WorkspaceManager(host)
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
    val activity = LocalActivity.current
    val requestingDragFrames = dock.dragging
    DisposableEffect(requestingDragFrames, activity) {
        val window = activity?.window
        val previous = window?.attributes?.preferredRefreshRate ?: 0f
        if (requestingDragFrames && window != null) {
            val rate = window.decorView.display?.supportedModes?.maxOfOrNull { it.refreshRate }?.coerceAtMost(120f) ?: 60f
            window.attributes = window.attributes.apply { preferredRefreshRate = rate }
        }
        onDispose { if (requestingDragFrames && window != null) window.attributes = window.attributes.apply { preferredRefreshRate = previous } }
    }
    dock.density = density
    dock.enabled = snapshot?.optBoolean("partial_zen") != true && snapshot?.objectOrNull("preferences") == null && snapshot?.objectOrNull("picker") == null && snapshot?.objectOrNull("toolbar_prompt") == null && snapshot?.objectOrNull("toolbar_manager") == null
    val panels = host.panelContent?.array("panels")?.objects()?.associateBy { it.getString("id") } ?: emptyMap()
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
        .workspaceDragCursor(dock.dragCursor)
        .drawWithContent { drawContent(); host.recordUiDraw() }
        .onGloballyPositioned { dock.origin = it.boundsInRoot().topLeft; host.surfaceOrigin = dock.origin }) {
        dock.viewport = JSONArray(listOf(maxWidth.value, maxHeight.value))
        AndroidView(factory = { CanvasSurfaceView(it, host) { x, y ->
            val point = androidx.compose.ui.geometry.Offset(x, y)
            dock.chromeRegions.values.any { bounds -> bounds.contains(point) } || dock.regions.values.any { region -> region.bounds.contains(point) }
        } }, modifier = Modifier.fillMaxSize(), update = { view ->
            // AndroidView resolves its own icon outside Compose's descendant
            // override. Keep the active workspace cursor over bare canvas too.
            view.pointerIcon = AndroidPointerIcon.getSystemIcon(view.context, dock.dragCursor ?: AndroidPointerIcon.TYPE_NULL)
        })
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
            if (!hidden) CollapsedColumns(host, snapshot, panels, dock)
            ContentDrawers(host, snapshot, panels, dock)
            layout.array("groups").objects().filter { !hidden || (it.optBoolean("floating") && !snapshot.optBoolean("hide_floating_panels")) }
                .sortedBy { it.getInt("id") == dock.expansion?.getInt("group") }.forEachIndexed { index, group ->
                key(group.getInt("id")) {
                  val source = dock.drawerSources.containsKey("tool") && group.getString("active") ==
                      state.getJSONObject("customization").objectOrNull("drawer")?.getJSONObject("anchor")?.optString("panel")
                  val z = if (source) 199 else (if (group.optBoolean("floating")) 180 else 100) + index
                  CompositionLocalProvider(LocalWorkspaceZ provides z) {
                    val expansion = dock.expansion?.takeIf { it.getInt("group") == group.getInt("id") }
                    val base = group.getJSONObject("bounds")
                    val shown = if (group.optBoolean("floating")) floatingBounds(base, dock.dragging, host, group.getInt("id")) else base
                    val bounds = expansion?.getJSONObject("bounds") ?: shown
                    val shape = expansion?.takeIf { it.getJSONObject("configuration").number("y") > 0f }
                        ?.let { expandedShape(it, density) } ?: dock.drawerContainerShape(bounds)
                    val placement = if (expansion == null) Modifier.workspacePlaced(host, group.getInt("id"), bounds, shown, density) else Modifier.placed(bounds, density)
                    Box(placement.zIndex(z.toFloat()).testTag("group-${group.getInt("id")}").chromeRegion(dock)
                        .shadow(if (expansion != null) 16.dp else 6.dp, shape).clip(shape)) {
                        val preview = expansion?.getJSONObject("preview")
                        val mod = if (preview == null) Modifier.fillMaxSize() else Modifier.placed(preview, density)
                        val projected = expansion?.objectOrNull("tiles")?.let { JSONObject(group.toString()).put("tiles", it) } ?: group
                        PanelGroup(host, host.panelContent?.getJSONObject("state") ?: state, projected, panels, dock, mod)
                        expansion?.getJSONObject("configuration")?.let { rect ->
                            Box(Modifier.placed(rect, density).background(colors.panel)) {
                                panels[group.getString("active")]?.let { ConfigurePanel(host, it) { height -> dock.configurationHeight = height } }
                            }
                        }
                    }
                    if (expansion == null) group.array("resize_handles").objects().forEach { handle ->
                        val edge = handle.getString("edge")
                        val hit = handle.getJSONObject("bounds").rect().let { r ->
                            // Extend external handles outward, keeping panel
                            // controls and tabs fully reachable.
                            obj("x" to (r.left - if ("left" in edge) 6f else 0f),
                                "y" to (r.top - if ("top" in edge) 6f else 0f),
                                "width" to (r.width + if ("left" in edge || "right" in edge) 6f else 0f),
                                "height" to (r.height + if ("top" in edge || "bottom" in edge) 6f else 0f))
                        }
                        Box(Modifier.workspacePlaced(host, group.getInt("id"), hit, base, density).zIndex(z.toFloat())
                            .testTag("resize-${group.getInt("id")}-${handle.getString("edge")}").workspaceSource(dock,
                            obj("type" to "resize_floating", "group" to group.getInt("id"), "edge" to handle.getString("edge")),
                            priority = 4, cursor = resizePointerIcon(handle.getString("edge"))))
                    }
                  }
                }
            }
            if (!hidden) layout.array("dividers").objects().forEach { divider ->
                val rect = divider.getJSONObject("bounds")
                val horizontal = divider.getString("axis") == "horizontal"
                val hit = JSONObject(rect.toString()).apply {
                    val extent = if (horizontal) "width" else "height"
                    val start = if (horizontal) "x" else "y"
                    val extra = (16f - rect.number(extent)).coerceAtLeast(0f)
                    put(start, rect.number(start) - extra / 2); put(extent, rect.number(extent) + extra)
                }
                CompositionLocalProvider(LocalWorkspaceZ provides 170) {
                    Box(Modifier.placed(hit, density).zIndex(170f).testTag("divider-${divider.getInt("id")}").workspaceSource(dock,
                        obj("type" to "drag_divider", "id" to divider.getInt("id")), priority = 4,
                        cursor = if (horizontal) AndroidPointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW else AndroidPointerIcon.TYPE_VERTICAL_DOUBLE_ARROW))
                }
            }
            if (!hidden) Row(Modifier.placed(layout.getJSONObject("status"), density).padding(horizontal = 4.dp), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.Bottom) {
                Surface(color = colors.surround, shape = RoundedCornerShape(20.dp)) {
                    CameraStatus(host)
                }
            }
        }
        WorkspaceDropHint(dock)
        dock.contextMenu?.let { menu ->
            val anchor = dock.contextAnchor
            Box(Modifier.offset { IntOffset(anchor.left.roundToInt(), anchor.top.roundToInt()) }
                .size((anchor.width / density).dp, (anchor.height / density).dp)) {
                WorkspaceMenu(host, menu, preserveContact = dock.contactHeld, dismiss = dock::closeContext)
            }
        }
    }
}

/** Transient feedback must not invalidate the workspace and every panel. */
@Composable private fun WorkspaceDropHint(dock: DockInteraction) {
    val visible by remember(dock) { derivedStateOf { dock.hint != null } }
    if (!visible) return
    val density = LocalDensity.current.density
    Layout(content = {}, modifier = Modifier.offset {
        val b = dock.hint?.objectOrNull("bounds")
        IntOffset(((b?.number("x") ?: 0f) * density).roundToInt(), ((b?.number("y") ?: 0f) * density).roundToInt())
    }.zIndex(Float.MAX_VALUE).background(LocalPalette.current.accent).testTag("workspace-drop-hint")) { _, _ ->
        val b = dock.hint?.objectOrNull("bounds")
        layout(((b?.number("width") ?: 0f) * density).roundToInt().coerceAtLeast(0),
            ((b?.number("height") ?: 0f) * density).roundToInt().coerceAtLeast(0)) {}
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
        onLongClick = { dock.holdContext(target) }, iconSize = host.catalog.getInt("zen_icon_size").dp,
        selectedColor = colors.text.copy(alpha = .08f)) { host.invoke("zen_mode") }
}

@Composable private fun Header(host: CanvasHost, state: JSONObject, dock: DockInteraction) {
    val colors = LocalPalette.current
    BoxWithConstraints(Modifier.fillMaxWidth().height(48.dp).chromeRegion(dock).background(colors.surround).padding(6.dp)) {
        val showTitle = maxWidth >= 1100.dp
        val switcherWidth = (maxWidth * .45f).coerceAtMost(480.dp)
        Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Spacer(Modifier.size(36.dp))
            Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                host.snapshot?.array("application_menus")?.objects()?.forEach { menu ->
                    val id = menu.getString("id")
                    key(id) {
                        var open by remember { mutableStateOf(false) }
                        // Menus are part of the GPU-free snapshot, so they open
                        // immediately even while the render owner initializes.
                        val model = menu.objectOrNull("model")
                        DisposableEffect(open) {
                            val ownsPopup = open
                            if (ownsPopup) { dock.popupOpen = true; dock.refresh() }
                            onDispose { if (ownsPopup) { dock.popupOpen = false; dock.refresh() } }
                        }
                        Box {
                            Box(Modifier.height(36.dp).testTag("application-menu-$id").clip(RoundedCornerShape(6.dp)).background(colors.surround)
                                .clickable { open = true }.padding(horizontal = HeaderTextPadding), contentAlignment = Alignment.Center) {
                                Text(menu.getString("label"), fontWeight = FontWeight.Bold)
                            }
                            if (open) model?.let { WorkspaceMenu(host, it) { open = false } }
                        }
                    }
                }
            }
            if (showTitle) state.array("tabs").optJSONObject(0)?.let { tab ->
                Text("${tab.optString("title")}${if (state.getJSONObject("document_file").optBoolean("modified")) " •" else ""} · ${tab.optInt("width")} × ${tab.optInt("height")}",
                    Modifier.widthIn(max = 350.dp).testTag("document-title").padding(horizontal = HeaderTextPadding),
                    fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            // Android always uses an immersive fullscreen workspace.
            WorkspaceSwitcher(host, Modifier.widthIn(max = switcherWidth))
            if (state.getJSONObject("settings").optString("show_clock") != "never") SystemStatus()
            IconTile("settings", state.array("commands").objects().first { it.getString("id") == "settings" }.getString("tooltip"), modifier = Modifier.testTag("header-settings")) { host.invoke("settings") }
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
    DisposableEffect(group.getInt("id"), group.array("panels").toString()) {
        val prefix = "${group.getInt("id")}:"
        onDispose { dock.tabs.keys.removeAll { it.startsWith(prefix) }; dock.tabSlots.keys.removeAll { it.startsWith(prefix) }; dock.tabClips.remove(group.getInt("id")) }
    }
    Surface(modifier, color = colors.panel) {
        Column {
            if (tabsVisible) PanelHeaderFeedback {
                Row(Modifier.fillMaxWidth().height(36.dp).testTag("group-header-${group.getInt("id")}").background(colors.tabs).dragSource(dock, groupItem)
                    .combinedClickable(onClick = { if (panel.optBoolean("expanded")) host.customize(obj("type" to "close_expanded")) },
                        onLongClick = { dock.holdContext(groupItem) }), verticalAlignment = Alignment.CenterVertically) {
                    Row(Modifier.weight(1f).onGloballyPositioned { dock.tabClips[group.getInt("id")] = it.boundsInRoot().translate(-dock.origin) }
                        .horizontalScroll(rememberScrollState()).clickable(enabled = panel.optBoolean("expanded")) { host.customize(obj("type" to "close_expanded")) }) {
                        group.array("panels").values().forEachIndexed { index, id ->
                            val p = panels[id.toString()] ?: return@forEachIndexed
                            val tab = Modifier.testTag("tab-$id").dragSource(dock, obj("kind" to "panel", "panel" to id))
                                .onGloballyPositioned { coords ->
                                    val r = coords.boundsInRoot(); val pos = (r.topLeft - dock.origin) / dock.density
                                    // Contact handling needs the panel ID as well as the drop-target geometry.
                                    dock.tabs["${group.getInt("id")}:$index"] = obj("group" to group.getInt("id"), "index" to index, "panel" to id,
                                        "bounds" to obj("x" to pos.x, "y" to pos.y, "width" to r.width / dock.density, "height" to r.height / dock.density))
                                    val natural = (coords.positionInRoot() - dock.origin) / dock.density
                                    dock.tabSlots["${group.getInt("id")}:$index"] = obj("group" to group.getInt("id"), "index" to index, "panel" to id,
                                        "bounds" to obj("x" to natural.x, "y" to natural.y, "width" to coords.size.width / dock.density, "height" to coords.size.height / dock.density))
                                }
                            WorkspaceTab(host, dock, p, group.getInt("id"), index, id == active, tab)
                        }
                    }
                    Box(Modifier.width(20.dp).height(36.dp).testTag("group-grip-${group.getInt("id")}")
                        .combinedClickable(onClick = { if (panel.optBoolean("expanded")) host.customize(obj("type" to "close_expanded")) },
                            onLongClick = { dock.holdContext(obj("kind" to "group", "group" to group.getInt("id"))) }), contentAlignment = Alignment.Center) { PanelGrip("Move panel group") }
                }
            }
            Box(Modifier.weight(1f).testTag("panel-body-$active")) {
                if (group.objectOrNull("tiles") != null) ToolRibbon(host, panel, group.getJSONObject("tiles"), dock, Modifier.fillMaxSize(), group.optString("axis") == "vertical")
                else PanelControls(host, state, panel, Modifier.fillMaxSize()) { dock.measure(active, contentHeight = it) }
            }
            group.objectOrNull("footer_grip")?.let { grip ->
                Box(Modifier.fillMaxWidth().height(grip.number("height").dp).testTag("group-grip-${group.getInt("id")}").dragSource(dock, groupItem)
                    .combinedClickable(onClick = {},
                        onLongClick = { dock.holdContext(groupItem) }), contentAlignment = Alignment.Center) { PanelGrip("Move panel group", vertical = true) }
            }
        }
    }
}
