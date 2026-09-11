package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject

/** Partial Zen uses the core's edge clusters, preserving saved dock topology. */
@Composable internal fun ZenToolbars(host: CanvasHost, snapshot: JSONObject, panels: Map<String, JSONObject>, dock: DockInteraction) {
    snapshot.objectOrNull("zen_toolbars")?.array("sections")?.objects()?.forEachIndexed { index, section ->
        val id = section.getString("panel")
        val panel = panels[id] ?: return@forEachIndexed
        val tiles = panel.array("tiles").objects().associateBy { it.getInt("id") }
        val projectedTiles = JSONArray()
        val bounds = JSONArray()
        section.array("tiles").values().forEach { pair ->
            pair as JSONArray
            tiles[pair.getInt(0)]?.let { projectedTiles.put(it); bounds.put(pair.getJSONObject(1)) }
        }
        val projected = JSONObject(panel.toString()).put("tiles", projectedTiles).put("tile_style", section.getString("style"))
        key(id, index) {
            ToolRibbon(host, projected, obj("tiles" to bounds), dock,
                Modifier.placed(section.getJSONObject("bounds"), dock.density).zIndex(150f)
                    .testTag("zen-section-$index").shadow(6.dp, RoundedCornerShape(6.dp))
                    .clip(RoundedCornerShape(6.dp)).background(LocalPalette.current.panel),
                section.getString("edge") in listOf("left", "right"))
        }
    }
}

private fun JSONObject.relativeTo(parent: JSONObject) = JSONObject(toString())
    .put("x", number("x") - parent.number("x")).put("y", number("y") - parent.number("y"))

@Composable internal fun CollapsedColumns(host: CanvasHost, snapshot: JSONObject, panels: Map<String, JSONObject>, dock: DockInteraction) {
    snapshot.getJSONObject("layout").array("collapsed").objects().forEach { column ->
        val id = column.getInt("id")
        key(id) {
            val bounds = column.getJSONObject("bounds")
            val content = column.getJSONObject("content")
            val current by rememberUpdatedState(column)
            val scroll = rememberScrollableState { delta ->
                val c = current
                val old = host.snapshot?.getJSONObject("state")?.getJSONObject("workspace")?.getJSONObject("layout")
                    ?.array("column_scroll")?.values()?.map { it as JSONArray }?.find { it.getInt(0) == id }?.getDouble(1)?.toFloat() ?: 0f
                val bottom = c.array("groups").objects().maxOfOrNull { it.getJSONObject("bounds").let { b -> b.number("y") + b.number("height") } } ?: 0f
                val max = (old + bottom - content.number("y") - content.number("height")).coerceAtLeast(0f)
                val next = (old - delta / dock.density).coerceIn(0f, max)
                if (next != old) host.dispatch(obj("type" to "measure_column_scroll", "column" to id, "offset" to next))
                (old - next) * dock.density
            }
            CompositionLocalProvider(LocalWorkspaceZ provides 160) {
                Box(Modifier.placed(bounds, dock.density).zIndex(160f).testTag("collapsed-column-$id")
                    .chromeRegion(dock).shadow(6.dp, RoundedCornerShape(8.dp)).clip(RoundedCornerShape(8.dp)).background(LocalPalette.current.panel)) {
                    Box(Modifier.placed(column.getJSONObject("expand").relativeTo(bounds), dock.density)
                        .testTag("expand-column-$id").clickable { host.customize(obj("type" to "set_column_collapsed", "group" to id, "collapsed" to false)) }, contentAlignment = Alignment.Center) {
                        SharedIcon("column-expand", "Expand column")
                    }
                    Box(Modifier.placed(content.relativeTo(bounds), dock.density).clipToBounds().scrollable(scroll, Orientation.Vertical)) {
                        column.array("groups").objects().forEach { group ->
                            group.array("icons").objects().forEach { icon ->
                                val panel = icon.getString("panel")
                                val view = panels[panel] ?: return@forEach
                                val target = obj("kind" to "panel", "panel" to panel)
                                Box(Modifier.placed(icon.getJSONObject("bounds").relativeTo(content), dock.density)
                                    .testTag("column-icon-$panel").contextAnchor(dock, target)
                                    .background(if (group.getString("active") == panel) LocalPalette.current.active else Color.Transparent, RoundedCornerShape(6.dp))
                                    .combinedClickable(onLongClick = { dock.context(target) }, onClick = {
                                        host.customize(obj("type" to "toggle_column_drawer", "group" to group.getInt("group"), "panel" to panel))
                                    }), contentAlignment = Alignment.Center) {
                                    SharedIcon(view.getString("icon"), view.getString("title"))
                                }
                            }
                        }
                    }
                    val item = obj("kind" to "column", "column" to id)
                    Box(Modifier.placed(column.getJSONObject("grip").relativeTo(bounds), dock.density)
                        .testTag("column-grip-$id").dragSource(dock, item), contentAlignment = Alignment.Center) { PanelGrip("Move column", false) }
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
    BackHandler(models.isNotEmpty()) {
        if (activeTool) host.customize(obj("type" to "close_expanded")) else models.values.lastOrNull()?.getJSONObject("anchor")?.let {
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
    val heights = remember(model.toString()) { mutableStateListOf<Float>().apply { repeat(columns.size) { add(0f) } } }
    var geometry by remember { mutableStateOf<JSONObject?>(null) }
    val tabs = model.objectOrNull("tabs")
    val tabHeight = if (tabs == null) 0f else 36f
    val columnId = id.toIntOrNull()
    val tileOrigins = if (id == "tool") dock.drawerTileRevision else 0
    var lastModel by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(current?.toString(), snapshot.getJSONObject("layout").toString(), heights.toList(), tileOrigins) {
        val from = geometry?.getJSONObject("placement")
        val animate = lastModel != current?.toString()
        lastModel = current?.toString()
        val start = withFrameNanos { it }
        do {
            val progress = if (!animate) 1f else ((withFrameNanos { it } - start) / 200_000_000f).coerceIn(0f, 1f)
            geometry = host.awaitQuery(obj("type" to "drawer", "column" to columnId, "heights" to JSONArray(heights.toList()),
                "progress" to progress, "from" to from, "closing" to (current == null)))
            if (id == "tool") { dock.drawer = geometry; dock.refresh() }
        } while (progress < 1f)
        if (current == null) closed()
    }
    DisposableEffect(dock, id) {
        onDispose {
            if (id == "tool") { dock.drawer = null; dock.refresh() }
            if (columnId != null) dock.clearDrawerTiles(columnId)
        }
    }
    val placement = geometry?.objectOrNull("placement") ?: return
    val connection = geometry?.objectOrNull("connection")
    val corners = connection?.array("square_corners")
    val shape = RoundedCornerShape(
        topStart = if (corners?.optBoolean(0) == true) 0.dp else 8.dp,
        topEnd = if (corners?.optBoolean(1) == true) 0.dp else 8.dp,
        bottomEnd = if (corners?.optBoolean(2) == true) 0.dp else 8.dp,
        bottomStart = if (corners?.optBoolean(3) == true) 0.dp else 8.dp)
    val z = if (id == "tool") 220 else 200
    CompositionLocalProvider(LocalWorkspaceZ provides z) {
        connection?.let { DrawerBridge(it, dock, z.toFloat()) }
        Box(Modifier.placed(placement.getJSONObject("bounds"), dock.density).zIndex(z.toFloat())
            .testTag(if (id == "tool") "tool-drawer" else "column-drawer-$id").chromeRegion(dock)
            .shadow(12.dp, shape).clip(shape).background(LocalPalette.current.panel)) {
            placement.array("columns").objects().forEachIndexed { index, bounds ->
                Column(Modifier.placed(bounds, dock.density)) {
                    if (tabs != null) Row(Modifier.fillMaxWidth().height(tabHeight.dp).background(LocalPalette.current.tabs).horizontalScroll(rememberScrollState())) {
                        tabs.array("panels").values().forEach { panelId ->
                            val panel = bodies[panelId.toString()] ?: return@forEach
                            TextButton({ host.dispatch(obj("type" to "select_tab", "group" to tabs.getInt("group"), "panel" to panelId)) },
                                modifier = Modifier.testTag("drawer-tab-$panelId")) {
                                SharedIcon(panel.getString("icon"), null)
                                if (panelId == tabs.getString("active")) Text(panel.getString("title"))
                            }
                        }
                    }
                    var clip by remember { mutableStateOf(Rect.Zero) }
                    Box(Modifier.fillMaxWidth().weight(1f).clipToBounds().onGloballyPositioned { clip = it.boundsInRoot().translate(-dock.origin) }) {
                        CompositionLocalProvider(LocalDrawerColumn provides columnId, LocalDrawerClip provides clip) {
                            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState())
                                .onSizeChanged { heights[index] = it.height / dock.density + tabHeight }, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                columns[index].values().forEach { panelId ->
                                    bodies[panelId.toString()]?.let { panel ->
                                        if (panel.array("tiles").length() > 0) DrawerToolbar(host, panel, dock, bounds.number("width"))
                                        else PanelControls(host, snapshot.getJSONObject("state"), panel,
                                            if (panelId in listOf("layers", "adjustments")) Modifier.height(480.dp) else Modifier.fillMaxWidth(), scrollable = false)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable private fun DrawerToolbar(host: CanvasHost, panel: JSONObject, dock: DockInteraction, width: Float) {
    var geometry by remember { mutableStateOf<JSONObject?>(null) }
    LaunchedEffect(panel.getString("id"), panel.array("tiles").toString(), panel.getString("tile_style"), width) {
        geometry = host.awaitQuery(obj("type" to "drawer_toolbar", "panel" to panel.getString("id"), "width" to width, "height" to 800f))
    }
    geometry?.let { g ->
        val height = (g.array("tiles").objects().maxOfOrNull { it.number("y") + it.number("height") } ?: 32f) + 4f
        ToolRibbon(host, panel, g, dock, Modifier.fillMaxWidth().height(height.dp), true)
    }
}

@Composable private fun DrawerBridge(connection: JSONObject, dock: DockInteraction, z: Float) {
    val color = LocalPalette.current.panel
    Canvas(Modifier.placed(connection.getJSONObject("bounds"), dock.density).zIndex(z).chromeRegion(dock)) {
        val t = connection.array("transform")
        fun point(x: Float, y: Float) = Offset((t.getDouble(0).toFloat() * x + t.getDouble(2).toFloat() * y + t.getDouble(4).toFloat()) * dock.density,
            (t.getDouble(1).toFloat() * x + t.getDouble(3).toFloat() * y + t.getDouble(5).toFloat()) * dock.density)
        val length = connection.number("length"); val depth = connection.number("depth")
        val r0 = connection.array("radii").getDouble(0).toFloat(); val r1 = connection.array("radii").getDouble(1).toFloat()
        val path = Path()
        fun move(x: Float, y: Float) { val p = point(x,y); path.moveTo(p.x,p.y) }
        fun line(x: Float, y: Float) { val p = point(x,y); path.lineTo(p.x,p.y) }
        fun curve(x1: Float,y1: Float,x2: Float,y2: Float,x3: Float,y3: Float) { val a=point(x1,y1); val b=point(x2,y2); val c=point(x3,y3); path.cubicTo(a.x,a.y,b.x,b.y,c.x,c.y) }
        val k = .5522848f
        move(0f,0f); line(length,0f); line(length,depth-r1)
        curve(length,depth-r1+r1*k,length+r1-r1*k,depth,length+r1,depth)
        line(-r0,depth); curve(-r0+r0*k,depth,0f,depth-r0+r0*k,0f,depth-r0)
        path.close(); drawPath(path,color)
    }
}
