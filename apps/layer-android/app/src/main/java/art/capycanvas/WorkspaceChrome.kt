package art.capycanvas

import android.view.PointerIcon as AndroidPointerIcon
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.Layout
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.graphics.graphicsLayer
import kotlin.math.roundToInt
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject

private fun JSONObject.relativeTo(parent: JSONObject) = JSONObject(toString())
    .put("x", number("x") - parent.number("x")).put("y", number("y") - parent.number("y"))

internal fun drawerButtonShape(direction: String?): SquircleShape {
    fun corner(vararg joined: String) = CornerSize(if (direction in joined) 0 else 50)
    return SquircleShape(corner("top", "left"), corner("top", "right"), corner("bottom", "right"), corner("bottom", "left"))
}

/** Ancestor clipping must preserve the connected source corners, including those its rounding reaches. */
internal fun DockInteraction.drawerContainerShape(bounds: JSONObject, radius: Float = SurfaceRadius.value, joined: JSONArray? = null,
    sources: Collection<DockInteraction.DrawerSource> = drawerSources.values): SquircleShape {
    val b = bounds.rect()
    val corners = listOf(b.topLeft, b.topRight, b.bottomRight, b.bottomLeft)
    val square = corners.mapIndexed { index, point ->
        joined?.optBoolean(index) == true || sources.any { source ->
            val facing = when (source.direction) {
                "top" -> listOf(0, 1); "right" -> listOf(1, 2); "bottom" -> listOf(2, 3); else -> listOf(0, 3)
            }
            val (reachX, reachY) = if (source.direction == "top" || source.direction == "bottom") radius to .5f else .5f to radius
            index in facing && point.x >= source.bounds.left - reachX && point.x <= source.bounds.right + reachX &&
                point.y >= source.bounds.top - reachY && point.y <= source.bounds.bottom + reachY
        }
    }
    return SquircleShape(topStart = if (square[0]) 0.dp else radius.dp,
        topEnd = if (square[1]) 0.dp else radius.dp, bottomEnd = if (square[2]) 0.dp else radius.dp,
        bottomStart = if (square[3]) 0.dp else radius.dp)
}

@Composable internal fun CollapsedColumns(host: CanvasHost, columns: JSONArray, drawerModels: JSONArray, vectorGroups: Set<Int>, panels: Map<String, JSONObject>, dock: DockInteraction) {
    val drawers = remember(drawerModels) { drawerModels.objects() }
    columns.objects().forEach { column ->
        val id = column.getInt("id")
        key(id) {
            val bounds = column.getJSONObject("bounds")
            val opened = column.objectOrNull("open")
            val members = remember(column) { column.array("groups").objects().map { it.getInt("group") } }
            val vector = members.any(vectorGroups::contains)
            val sources = if (opened == null) emptyList() else column.array("groups").objects().flatMap { group ->
                group.array("icons").objects().filter { it.getString("panel") == group.getString("active") }.map {
                    DockInteraction.DrawerSource(opened.getString("direction"), it.getJSONObject("bounds").rect())
                }
            }
            val shape = if (vector) column.panelCornerShape() else dock.drawerContainerShape(bounds, sources = dock.drawerSources.values + sources)
            opened?.array("connections")?.values()?.forEach { pair ->
                pair as JSONArray
                key(pair.getString(0)) {
                    if (!vector) DrawerBridge(pair.getJSONObject(1), dock, 159f, Modifier.testTag("column-connection-$id-${pair.getString(0)}"))
                }
            }
            val content = column.getJSONObject("content")
            val current by rememberUpdatedState(column)
            val scroll = rememberScrollableState { delta ->
                val c = current
                val old = host.snapshot?.getJSONObject("state")?.getJSONObject("workspace")?.getJSONObject("layout")
                    ?.array("column_scroll")?.values()?.map { it as JSONArray }?.find { it.getInt(0) == id }?.getDouble(1)?.toFloat() ?: 0f
                val bottom = c.array("groups").objects().maxOfOrNull { it.getJSONObject("bounds").let { b -> b.number("y") + b.number("height") } } ?: 0f
                val currentContent = c.getJSONObject("content")
                val max = (old + bottom - currentContent.number("y") - currentContent.number("height")).coerceAtLeast(0f)
                val next = (old - delta / dock.density).coerceIn(0f, max)
                if (next != old) host.dispatch(obj("type" to "measure_column_scroll", "column" to id, "offset" to next))
                (old - next) * dock.density
            }
            CompositionLocalProvider(LocalWorkspaceZ provides 160, LocalPalette provides LocalPalette.current.onGlass) {
                Box(Modifier.placed(bounds, dock.density).zIndex(160f).testTag("collapsed-column-$id")
                    .chromeRegion(dock).edgeSurface(dock).contextSource(dock, obj("kind" to "column", "column" to id))
                    .panelSurface(if (vector) 0.dp else 6.dp, shape).glass(shape, if (vector) Color.Transparent else LocalPalette.current.panelFill)) {
                    Box(Modifier.placed(content.relativeTo(bounds), dock.density).clipToBounds().scrollable(scroll, Orientation.Vertical)) {
                        val groups = column.array("groups").objects()
                        groups.forEachIndexed { index, group ->
                            if (index > 0) {
                                val top = group.getJSONObject("bounds").number("y") - 2f - DividerSlot.value
                                ToolbarDivider(Modifier.placed(obj("x" to 0f, "y" to (top - content.number("y")),
                                    "width" to content.number("width"), "height" to DividerSlot.value), dock.density)
                                    .testTag("column-divider-$id-$index"), horizontal = true)
                            }
                            group.array("icons").objects().forEach { icon ->
                                val panel = icon.getString("panel")
                                val view = panels[panel] ?: return@forEach
                                val target = obj("kind" to "panel", "panel" to panel)
                                val selected = if (opened != null) group.getString("active") == panel else drawers.any { it.getJSONObject("anchor").let { anchor ->
                                    anchor.getInt("column") == id && anchor.getString("origin") == panel
                                } }
                                val source = if (vector) null else dock.drawerSources[id.toString()]?.takeIf { it.anchor?.optString("origin") == panel }
                                val shape = if (vector) icon.panelCornerShape() else drawerButtonShape(if (opened != null) opened.getString("direction").takeIf { selected } else source?.direction)
                                HoverTip(view.getString("title"), Modifier.placed(icon.getJSONObject("bounds").relativeTo(content), dock.density)
                                    .dragSource(dock, target, holdToDrag = true)) {
                                    Box(Modifier.fillMaxSize().testTag("column-icon-$panel").semantics { this.selected = selected }
                                        .clip(shape).background(if (selected) LocalPalette.current.active else Color.Transparent)
                                        .combinedClickable(onLongClick = { dock.holdContext(target) }, onClick = {
                                            host.customize(obj("type" to "toggle_column_drawer", "group" to group.getInt("group"), "panel" to panel))
                                        }), contentAlignment = Alignment.Center) {
                                        LiveIcon(host, view.getString("icon"), view.getString("title"))
                                    }
                                }
                            }
                        }
                    }
                    val item = obj("kind" to "column", "column" to id)
                    val context = item
                    Box(Modifier.placed(column.getJSONObject("grip").relativeTo(bounds), dock.density)
                        .testTag("column-grip-$id").dragSource(dock, item, context), contentAlignment = Alignment.Center) { PanelGrip("Move column", vertical = true) }
                }
            }
        }
    }
}

@Composable internal fun ContentDrawers(host: CanvasHost, snapshot: JSONObject, panels: Map<String, JSONObject>, dock: DockInteraction) {
    val customization = snapshot.getJSONObject("state").getJSONObject("customization")
    val models = customization.array("column_drawers").objects().associateBy { it.getJSONObject("anchor").getInt("column").toString() }.toMutableMap()
    customization.objectOrNull("drawer")?.let { models["tool"] = it }
    val retained = remember { mutableStateMapOf<String, JSONObject>() }
    LaunchedEffect(models.mapValues { it.value.toString() }) { models.forEach { (id, model) -> retained[id] = model } }
    val activeTool = models["tool"] != null
    val openColumns = snapshot.getJSONObject("layout").array("collapsed").objects().filter { it.objectOrNull("open") != null }
    BackHandler(models.isNotEmpty() || openColumns.isNotEmpty()) {
        if (models.isEmpty()) host.customize(obj("type" to "close_column", "column" to openColumns.last().getInt("id")))
        else if (activeTool) host.customize(obj("type" to if(models["tool"]?.optBoolean("layer_properties")==true) "toggle_layer_drawer_properties" else "close_expanded")) else models.values.lastOrNull()?.getJSONObject("anchor")?.let {
            host.customize(obj("type" to "toggle_column_drawer", "group" to it.getInt("group"), "panel" to it.getString("origin")))
        }
    }
    retained.keys.toList().sortedBy { it == "tool" }.forEach { id ->
        key(id) { ContentDrawer(host, snapshot, models[id], retained.getValue(id), panels, dock, id) { retained.remove(id) } }
    }
}

@Composable private fun ContentDrawer(host: CanvasHost, snapshot: JSONObject, current: JSONObject?, retained: JSONObject,
    panels: Map<String, JSONObject>, dock: DockInteraction, id: String, closed: () -> Unit) {
    val model = current ?: retained
    val bodies = remember { mutableMapOf<String, JSONObject>() }.apply { putAll(panels) }
    val columns = model.array("columns").values().map { it as JSONArray }
    val tabs = model.objectOrNull("tabs")
    val heights = remember(model.array("columns").toString(), tabs?.toString()) { mutableStateListOf<Pair<Int, Float>?>().apply { repeat(columns.size) { add(null) } } }
    var plan by remember { mutableStateOf<JSONObject?>(null) }
    var planContext by remember { mutableStateOf<String?>(null) }
    var toolbars by remember { mutableStateOf<Map<String, JSONObject>>(emptyMap()) }
    var geometry by remember { mutableStateOf<JSONObject?>(null) }
    val placementKey = listOf("anchor", "columns", "dismissal", "tabs", "compact").map { model.opt(it) }.toString().takeIf { current != null }
    val tabHeight = if (tabs == null) 0f else 36f
    val inspector = model.has("layer_properties")
    val propertiesOpen = model.optBoolean("layer_properties")
    var pagePosition by remember { mutableFloatStateOf(0f) }
    var layerSizes by remember { mutableStateOf<JSONObject?>(null) }
    var layerSizesContext by remember { mutableStateOf<String?>(null) }
    val columnId = id.toIntOrNull()
    val tileOrigins = if (id == "tool") dock.drawerTileRevision else 0
    val toolbarInputs = columns.flatMap { it.values() }.mapNotNull { bodies[it.toString()] }
        .filter { it.array("tiles").length() > 0 }.map { panel ->
            listOf(panel.getString("id"), panel.getString("tile_style"),
                panel.array("tiles").objects().map { listOf(it.getInt("id"), it.getJSONObject("control")) })
        }
    val layout = snapshot.getJSONObject("layout")
    val anchor = model.getJSONObject("anchor")
    val source = when (anchor.getString("kind")) {
        "header" -> snapshot.objectOrNull("header_presentation")?.array("items")?.objects()
            ?.firstOrNull { it.getInt("id") == anchor.getInt("id") }?.getJSONObject("bounds")
        "column" -> layout.array("collapsed").objects().firstOrNull { it.getInt("id") == anchor.getInt("column") }?.let { column ->
            listOf(column.getJSONObject("content").rect(), column.array("groups").objects()
                .firstOrNull { it.getInt("group") == anchor.getInt("group") }?.array("icons")?.objects()
                ?.firstOrNull { it.getString("panel") == anchor.getString("origin") }?.getJSONObject("bounds")?.rect())
        }
        else -> layout.array("groups").objects().firstOrNull { it.getString("active") == anchor.optString("panel") }
            ?.let { listOf(it.getJSONObject("bounds"), it.opt("tiles")) }
    }
    val sizingContext = listOf(anchor, source, model.array("columns"), tabs, layout.getJSONArray("viewport"),
        snapshot.opt("header_presentation_height"), snapshot.opt("workspace_bottom_inset"), tileOrigins, toolbarInputs).toString()
    var lastAnchor by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(placementKey, sizingContext) {
        if (current == null) return@LaunchedEffect
        val placement = host.awaitQuery(obj("type" to "drawer", "column" to columnId,
            "heights" to JSONArray(List(columns.size) { layout.getJSONArray("viewport").getDouble(1) }), "progress" to 1f, "from" to null, "closing" to false))
            ?.objectOrNull("placement") ?: return@LaunchedEffect
        val measured = mutableMapOf<String, JSONObject>()
        columns.forEachIndexed { index, panels -> panels.values().forEach { panelId ->
            val panel = bodies[panelId.toString()]
            if (panel != null && panel.array("tiles").length() > 0) {
                host.awaitQuery(obj("type" to "drawer_toolbar", "panel" to panel.getString("id"),
                    "width" to placement.array("columns").getJSONObject(index).number("width"), "height" to 800f))
                    ?.let { measured[panel.getString("id")] = it }
            }
        } }
        toolbars = measured
        plan = placement
        planContext = sizingContext
    }
    val layerPlacement = layerSizes?.array("placements")?.optJSONObject(if (propertiesOpen) 1 else 0)
    LaunchedEffect(placementKey, sizingContext, planContext, plan, heights.toList(), propertiesOpen, layerPlacement?.toString(), layerSizesContext) {
        val from = geometry?.getJSONObject("placement")
        val anchor = current?.getJSONObject("anchor")?.toString()
        val switching = anchor != null && lastAnchor != null && lastAnchor != anchor
        val pageTarget = if(propertiesOpen) 1f else 0f
        val pageFrom = pagePosition
        val pageChange = inspector && current != null && pageFrom != pageTarget
        val target = when {
            current == null -> from
            inspector -> {
                if (layerSizesContext != sizingContext) return@LaunchedEffect
                layerPlacement
            }
            planContext != sizingContext || plan == null || heights.indices.any { index ->
                heights[index]?.first != (plan!!.array("columns").getJSONObject(index).number("width") * dock.density).roundToInt()
            } -> return@LaunchedEffect
            else -> host.awaitQuery(obj("type" to "drawer", "column" to columnId, "heights" to JSONArray(heights.map { it!!.second }),
                "progress" to 1f, "from" to null, "closing" to false))?.objectOrNull("placement")
        }
        if(target == null) { if(current == null) closed(); return@LaunchedEffect }
        val animate = current == null || pageChange || !switching && from?.toString() != target.toString()
        if (anchor != null) lastAnchor = anchor
        workspaceFrames(200,animate) { progress ->
            val eased=1f-(1f-progress)*(1f-progress)*(1f-progress)
            val position = if(pageChange) pageFrom+(pageTarget-pageFrom)*eased else pageTarget
            val reply=JSONObject(Native.drawerFrame(obj("from" to from, "to" to target, "progress" to eased,
                "closing" to (current == null), "clip_contents" to true).toString()))
            geometry=reply; pagePosition=position
            if (reply.objectOrNull("connection") != null) reply.getJSONObject("placement").let { placement ->
                val source=DockInteraction.DrawerSource(placement.getString("direction"),placement.getJSONObject("anchor").rect(),model.getJSONObject("anchor"))
                if(dock.drawerSources[id]!=source)dock.drawerSources[id]=source
            } else dock.drawerSources.remove(id)
            if(id=="tool"){dock.drawer=reply;dock.refresh()}
        }
        if (current == null) closed()
    }
    DisposableEffect(dock, id) {
        onDispose {
            dock.drawerSources.remove(id)
            if (id == "tool") { dock.drawer = null; dock.refresh() }
            if (columnId != null) dock.clearDrawerTiles(columnId)
        }
    }
    val visible = geometry != null
    val placement = geometry?.objectOrNull("placement") ?: plan ?: return
    val connection = geometry?.objectOrNull("connection")
    val corners = connection?.array("square_corners")
    val shape = dock.drawerContainerShape(placement.getJSONObject("bounds"), joined = corners)
    val z = if (id == "tool") 220 else 200
    val tabbed = tabs != null && columnId != null
    CompositionLocalProvider(LocalWorkspaceZ provides z, LocalPalette provides LocalPalette.current.onGlass) {
        // Shadow below source chrome; a nested toolbar's source lives in a column drawer.
        val nested = id == "tool" && model.getJSONObject("anchor").optString("panel").let { panel ->
            snapshot.getJSONObject("state").getJSONObject("customization").array("column_drawers").objects()
                .any { drawer -> drawer.array("columns").values().any { panel in (it as JSONArray).values() } }
        }
        val cut = connection?.getJSONObject("bounds")?.rect()?.translate(-placement.getJSONObject("bounds").rect().topLeft)
        if (visible) {
        Box(Modifier.placed(placement.getJSONObject("bounds"), dock.density).zIndex(if (nested) 199f else 99f)
            .panelSurface(12.dp, shape, cut = cut?.let { Rect(it.topLeft * dock.density, it.bottomRight * dock.density) }))
        connection?.let { DrawerBridge(it, dock, z.toFloat()) }
        }
        val opening = remember { PanelOpening() }
        Layout(modifier = Modifier.fillMaxSize().zIndex(z.toFloat()), content = {
        Box(Modifier.placed(placement.getJSONObject("bounds"), dock.density).zIndex(z.toFloat())
            .testTag(if (id == "tool") "tool-drawer" else "column-drawer-$id").chromeRegion(dock)
            .then(if (columnId != null) Modifier.columnDrawerBounds(dock, columnId, if (current != null) tabs?.getInt("group") else null) else Modifier)
            .panelSurface(0.dp, shape, opening = opening)
            .glass(shape, if (tabbed) Color.Transparent else LocalPalette.current.panelFill)) {
            // Placement can lag the model by a frame when switching to a drawer with fewer columns.
            placement.array("columns").objects().take(columns.size).forEachIndexed { index, bounds ->
                val contentBounds = JSONObject(bounds.toString()).apply {
                    plan?.array("columns")?.optJSONObject(index)?.let { put("x", it.get("x")); put("width", it.get("width")) }
                }
                Column(Modifier.wrapContentSize(Alignment.TopStart, unbounded = true).placed(contentBounds, dock.density)) {
                    if (tabs != null && columnId != null) {
                        val group = tabs.getInt("group")
                        val item = obj("kind" to "group", "group" to group)
                        var tabClip by remember { mutableStateOf(Rect.Zero) }
                        PanelHeaderFeedback {
                            Row(Modifier.fillMaxWidth().height(tabHeight.dp).testTag("column-drawer-header-$id")
                                .then(if (current != null) Modifier.dragSource(dock, item, cursor = AndroidPointerIcon.TYPE_ARROW) else Modifier),
                                verticalAlignment = Alignment.CenterVertically) {
                                val cutout = remember { TabStrip() }
                                Box(Modifier.weight(1f).clipToBounds().onGloballyPositioned {
                                    tabClip = it.boundsInRoot().translate(-dock.origin); dock.tabClips[group] = tabClip
                                }.tabStrip(cutout, LocalPalette.current.strip, LocalPalette.current.panelFill)) {
                                    Row(Modifier.horizontalScroll(rememberScrollState()).testTag("column-drawer-tabs-$id")) {
                                        tabs.array("panels").values().forEachIndexed { tabIndex, panelId ->
                                            val panel = bodies[panelId.toString()] ?: return@forEachIndexed
                                            key(panelId.toString()) {
                                                WorkspaceTab(host, dock, panel, group, tabIndex,
                                                    panelId == tabs.getString("active"), cutout,
                                                    Modifier.testTag("drawer-tab-$panelId")
                                                        .then(if (current != null) Modifier.dragSource(dock, obj("kind" to "panel", "panel" to panelId), cursor = AndroidPointerIcon.TYPE_ARROW) else Modifier)
                                                        .drawerTabHit(dock, columnId, group, tabIndex, panelId.toString(), tabClip, current != null),
                                                    presentation = placement.array("tab_presentations").getJSONObject(tabIndex), enabled = current != null)
                                            }
                                        }
                                    }
                                }
                                Box(Modifier.width(20.dp).fillMaxHeight().background(LocalPalette.current.strip).testTag("column-drawer-grip-$id")
                                    .then(if (current != null) Modifier.dragSource(dock, item) else Modifier), contentAlignment = Alignment.Center) {
                                    PanelGrip("Move panel group")
                                }
                            }
                        }
                    }
                    var clip by remember { mutableStateOf(Rect.Zero) }
                    Box(Modifier.fillMaxWidth().weight(1f).then(if (tabbed) Modifier.background(LocalPalette.current.panelFill) else Modifier)
                        .clipToBounds().onGloballyPositioned { clip = it.boundsInRoot().translate(-dock.origin) }) {
                        CompositionLocalProvider(LocalDrawerColumn provides columnId, LocalDrawerClip provides clip,
                            LocalWorkspaceZ provides (200 + (columnId ?: 100)), LocalPanelOpening provides opening) {
                            if(inspector) LayerInspector(host, host.snapshot?.getJSONObject("state") ?: snapshot.getJSONObject("state"), propertiesOpen,
                                pagePosition, bounds.number("height"), layerSizes?.number("footer_height",49.0) ?: 49f,
                                sizingContext) { layerSizesContext=sizingContext; layerSizes = it }
                            else Column(Modifier.fillMaxWidth().panelScroll(rememberScrollState())
                                .onSizeChanged { heights[index] = it.width to (it.height / dock.density + tabHeight) }, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                columns[index].values().forEach { panelId ->
                                    bodies[panelId.toString()]?.let { panel ->
                                        if (panel.array("tiles").length() > 0) toolbars[panel.getString("id")]?.let { DrawerToolbar(host, panel, dock, it) }
                                        else PanelControls(host, host.snapshot?.getJSONObject("state") ?: snapshot.getJSONObject("state"), panel,
                                            if (panelId in listOf("layers", "adjustments", "filter_types")) Modifier.height(480.dp).then(if(panelId=="layers") Modifier.padding(vertical=panelMetrics().inset) else Modifier) else Modifier.fillMaxWidth().then(if(model.optBoolean("compact"))Modifier.padding(12.dp) else Modifier), scrollable = false,
                                            splitFilters = columns.any { it.values().contains("filter_types") },
                                            toolSet = model.objectOrNull("tool_set").takeIf { panelId == "brushes" })
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        }) { measurables, constraints ->
            val content = measurables.single().measure(constraints.copy(minWidth = 0, minHeight = 0))
            layout(constraints.maxWidth, constraints.maxHeight) { if (visible) content.place(0, 0) }
        }
    }
}

@Composable internal fun LayerInspector(host: CanvasHost, state: JSONObject, open: Boolean, position: Float,
    height: Float, footerHeight: Float, sizingContext: String, full: Boolean = true, labelOverride: String? = null,
    bubbleId: Int? = null, toggle: () -> Unit = { host.customize(obj("type" to "toggle_layer_drawer_properties")) }, onSizes: (JSONObject) -> Unit) {
    val density=androidx.compose.ui.platform.LocalDensity.current.density
    val inset=if(full) panelMetrics().compound else panelMetrics().inset
    val view=state.getJSONObject("layer_properties")
    val disclosure=view.getJSONObject("disclosure")
    val list=rememberLazyListState()
    var measured by remember { mutableStateOf<PanelContentSize?>(null) }
    var propertiesHeight by remember { mutableStateOf<Float?>(null) }
    var anchor by remember { mutableStateOf<Pair<Int,Int>?>(null) }
    var anchorTarget by remember { mutableStateOf<String?>(null) }
    val target="${state.getJSONObject("document_file").optLong("epoch")}:${view.optLong("layer")}"
    LaunchedEffect(open) {
        if(open) { anchor=list.firstVisibleItemIndex to list.firstVisibleItemScrollOffset; anchorTarget=target }
    }
    LaunchedEffect(position,target) {
        if(position==0f && anchorTarget==target) anchor?.let { list.scrollToItem(it.first.coerceAtMost((state.array("layers").length()-1).coerceAtLeast(0)),it.second); anchor=null }
        else if(anchorTarget!=target) anchor=null
    }
    LaunchedEffect(measured,propertiesHeight,state.array("layers").length(),state.getJSONObject("camera").array("viewport").toString(),sizingContext) {
        val measurement = measured ?: return@LaunchedEffect
        val properties = propertiesHeight ?: return@LaunchedEffect
        host.awaitQuery(obj("type" to "layer_drawer_sizing","measurement" to obj("fixed_height" to ((measurement.fixedHeight ?: 0f)+inset.value*2),"unit_height" to measurement.unitHeight),
            "properties_height" to properties))?.let { onSizes(it) }
    }
    Column(Modifier.fillMaxSize().testTag("layer-inspector")) {
        val pageHeight=(height-footerHeight).coerceAtLeast(0f)
        Box(Modifier.weight(1f).fillMaxWidth().clipToBounds().testTag("layer-properties-pages")) {
            LayerPanel(host,state,Modifier.fillMaxWidth().requiredHeight(pageHeight.coerceAtLeast((measured?.fixedHeight ?: 0f)+(measured?.unitHeight ?: 0f)+inset.value*2).dp)
                .offset { IntOffset(0,(-position*pageHeight*density).roundToInt()) }.padding(vertical=inset),listState=list, full=full) { measured=it }
            Column(Modifier.fillMaxWidth().height(pageHeight.dp).offset { IntOffset(0,((1f-position)*pageHeight*density).roundToInt()) }
                .panelScroll(rememberScrollState())) {
                Box(Modifier.fillMaxWidth().wrapContentHeight(Alignment.Top, unbounded = true).padding(8.dp)
                    .onSizeChanged { propertiesHeight=it.height/density+16f }) { LayerPropertiesPanel(host,state) }
            }
        }
        HorizontalDivider(thickness=1.dp,color=LocalPalette.current.text.copy(alpha=.16f))
        val label=labelOverride ?: disclosure.getString(if(open)"return_label" else "label")
        val summary=if(open || !full)"" else disclosure.getString("summary")
        Row(Modifier.fillMaxWidth().height((footerHeight-1f).dp), verticalAlignment=Alignment.CenterVertically, horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        val slot=panelMetrics().height
        if (!full) Box(Modifier.fillMaxHeight().testTag("bubble-new-layer-$bubbleId")
            .graphicsLayer { alpha=if(open)0f else 1f }.clickable(enabled=!open) { host.layer(obj("op" to "new", "group" to false, "clipped" to false)) }
            .padding(horizontal=LayerEdge), contentAlignment=Alignment.Center) {
            Box(Modifier.size(slot).testTag("bubble-new-layer-slot-$bubbleId"),contentAlignment=Alignment.Center) {
                SharedIcon("add-layer", host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("new_layer"))
            }
        }
        Row(Modifier.weight(1f).fillMaxHeight().testTag("layer-properties-disclosure")
            .clickable(enabled=open || !view.isNull("layer")) { toggle() }
            .padding(horizontal=LayerEdge),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
            if (full) Box(Modifier.size(slot).testTag("layer-properties-icon-slot"),contentAlignment=Alignment.Center) { SharedIcon("properties",null,Modifier.size(16.dp)) }
            Column(Modifier.weight(1f),verticalArrangement=Arrangement.spacedBy(1.dp)) {
                Text(label,Modifier.fillMaxWidth().testTag("layer-properties-disclosure-label"),textAlign=if(full)androidx.compose.ui.text.style.TextAlign.Start else androidx.compose.ui.text.style.TextAlign.End,maxLines=1,overflow=TextOverflow.Ellipsis)
                if(summary.isNotEmpty())Text(summary,Modifier.testTag("layer-properties-disclosure-summary"),color=LocalPalette.current.secondary,fontSize=LocalTextStyle.current.fontSize*.9f,maxLines=1,overflow=TextOverflow.Ellipsis)
            }
            Box(Modifier.size(slot).testTag("layer-properties-chevron-slot"),contentAlignment=Alignment.Center) { SharedIcon("chevron-down",null,Modifier.size(16.dp).graphicsLayer { rotationZ=if(open)0f else 180f }) }
        }
        }
    }
}

@Composable private fun DrawerToolbar(host: CanvasHost, panel: JSONObject, dock: DockInteraction, geometry: JSONObject) {
    ToolRibbon(host, panel, geometry, dock, Modifier.fillMaxWidth().height(geometry.number("content_height").dp), true)
}

@Composable private fun BridgeGlass(connection: JSONObject, dock: DockInteraction) {
    val host = LocalCanvasHost.current
    val key = remember { Any() }
    DisposableEffect(host, key) { onDispose { host.glassConnection(key, null, 0f) } }
    val current by rememberUpdatedState(connection)
    val opacity=LocalWorkspaceOpacity.current
    LaunchedEffect(host,key,opacity,dock.density) {
        snapshotFlow { current.toString() to (opacity?.value ?: 1f) }.collect { (value,strength) ->
            host.glassConnection(key,JSONObject(value),dock.density,strength)
        }
    }
}

@Composable private fun DrawerBridge(connection: JSONObject, dock: DockInteraction, z: Float, modifier: Modifier = Modifier) {
    val color = LocalPalette.current.onGlass.panelFill
    BridgeGlass(connection, dock)
    Canvas(Modifier.placed(connection.getJSONObject("bounds"), dock.density).then(modifier).zIndex(z).chromeRegion(dock)) {
        val t = connection.array("transform")
        fun point(x: Float, y: Float) = Offset((t.getDouble(0).toFloat() * x + t.getDouble(2).toFloat() * y + t.getDouble(4).toFloat()) * dock.density,
            (t.getDouble(1).toFloat() * x + t.getDouble(3).toFloat() * y + t.getDouble(5).toFloat()) * dock.density)
        val length = connection.number("length"); val depth = connection.number("depth")
        val r0 = connection.array("radii").getDouble(0).toFloat(); val r1 = connection.array("radii").getDouble(1).toFloat()
        val path = Path()
        fun move(x: Float, y: Float) { val p = point(x,y); path.moveTo(p.x,p.y) }
        fun line(x: Float, y: Float) { val p = point(x,y); path.lineTo(p.x,p.y) }
        fun corner(center: Offset, start: Offset, end: Offset) = squircleCorner(center, start, end).forEach { line(it.x, it.y) }
        move(0f,0f); line(length,0f); line(length,depth-r1)
        corner(Offset(length+r1,depth-r1), Offset(-r1,0f), Offset(0f,r1))
        line(-r0,depth); corner(Offset(-r0,depth-r0), Offset(0f,r0), Offset(r0,0f))
        path.close(); drawPath(path,color)
    }
}
