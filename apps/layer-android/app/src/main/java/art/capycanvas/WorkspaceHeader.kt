package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

private fun JSONObject.headerEntries() = array("zones").values().flatMap { (it as JSONArray).objects() }

@Composable internal fun WorkspaceHeader(host: CanvasHost, snapshot: JSONObject, input: HeaderInteraction) {
    val view = snapshot.objectOrNull("header") ?: return
    val state = snapshot.getJSONObject("state")
    val model = view.getJSONObject("model")
    val entries = model.headerEntries()
    val specs = view.array("items").objects().associateBy { it.getInt("id") }
    val size = view.array("sizes").objects().first { it.getString("id") == model.getString("size") }
    val tile = size.number("tile")
    val height = size.number("height")
    val editing = view.optBoolean("editing")
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    // Menus alone occupy the default eight entries. Retain workspace labels,
    // document title and clock too, so every publication does not evict them.
    val text = rememberTextMeasurer(cacheSize = 64)
    val textStyle = LocalTextStyle.current
    fun measure(label: String): Float = text.measure(label,
        style = textStyle.copy(fontWeight = FontWeight.Normal), maxLines = 1).size.width / density
    val menus = snapshot.array("application_menus").objects()
    // Compose rounds each side's padding separately. Summing logical widths
    // first loses pixels at fractional scale and clips the final menu label.
    val menuPadding = (8f * density).roundToInt()
    val menuWidth = menus.sumOf { (measure(it.getString("label")) * density).roundToInt() + 2 * menuPadding } / density + 8f + 2f * (menus.size - 1)
    val tab = state.array("tabs").optJSONObject(0)
    val title = tab?.let { "${it.optString("title")}${if (state.getJSONObject("document_file").optBoolean("modified")) " •" else ""} · ${it.optInt("width")} × ${it.optInt("height")}" } ?: ""
    val choices = host.workspaceManager?.array("switcher_display")?.objects() ?: emptyList()
    val workspaceWidth = (6f + choices.sumOf { (measure(it.getString("title")) + 22f).coerceAtMost(130f).toDouble() }).toFloat().coerceAtMost(480f)
    val metrics = JSONArray(entries.map { entry ->
        val kind = entry.getJSONObject("item").getString("kind")
        val natural = when (kind) {
            "menu_labels" -> menuWidth
            "workspaces" -> workspaceWidth.coerceAtLeast(tile)
            "document_title" -> measure(title).coerceIn(tile, 350f) + 12f
            "clock" -> measure("00:00 PM") + 12f
            "space" -> tile * .5f
            else -> tile
        }
        val grip = if (editing) (20f * density).roundToInt() / density else 0f
        obj("id" to entry.getInt("id"), "width" to natural + grip,
            "compact" to (if (kind in listOf("menu_labels", "workspaces", "document_title")) tile else natural) + grip)
    })
    var bankHeight by remember { mutableFloatStateOf(0f) }
    var overflowMenu by remember { mutableStateOf<JSONObject?>(null) }
    val modelKey = model.toString()
    LaunchedEffect(modelKey, editing) {
        input.finish(true); input.overflow = null; input.context = null
        if (!editing || entries.none { it.getInt("id") == input.selected }) input.selected = null
    }
    BackHandler(editing && snapshot.objectOrNull("preferences") == null && snapshot.objectOrNull("picker") == null) {
        if (input.held != null) input.finish(true)
        else if (input.overflow != null) input.overflow = null
        else host.headerEdit(obj("type" to "cancel"))
    }
    BoxWithConstraints(Modifier.fillMaxSize().zIndex(300f)) {
        // The canvas extends behind the title bar. Empty chrome owns input,
        // but must not paint an opaque strip over the drawing.
        Box(Modifier.fillMaxWidth().height(height.dp).testTag("title-bar").chromeRegion(input.dock)
            .headerSource(input, obj("kind" to "background"), "Title Bar", -1).headerChrome())
        val width = maxWidth.value
        val geometryKey = "$width:$modelKey:$editing:$metrics:${host.drawingTabs.rows.size}"
        SideEffect { input.width = width; input.metrics = metrics }
        LaunchedEffect(geometryKey) {
            input.finish(true)
            input.geometry = host.awaitQuery(obj("type" to "header", "request" to obj("op" to "geometry",
                "width" to width, "insets" to JSONArray(listOf(0, 0)), "metrics" to metrics)))
            input.geometryKey = geometryKey
        }
        val resolved = input.geometry?.takeIf { input.geometryKey == geometryKey }
        LaunchedEffect(resolved?.toString(), height, bankHeight, editing) {
            resolved?.let { host.dispatch(obj("type" to "measure_header", "height" to (height + if (editing) bankHeight + 12f else 0f), "items" to it.array("items"))) }
        }
        // Keep existing item nodes at their prior positions while the native
        // owner resolves new metrics. Compaction changes a child's presentation,
        // never the identity or capture lifetime of its enclosing editable item.
        val geometry = input.preview?.objectOrNull("geometry") ?: resolved ?: input.geometry
        val heldId = input.held?.source?.takeIf { it.optString("kind") == "item" }?.getInt("value")
        val placements = geometry?.array("items")?.objects()?.associateBy { it.getInt("id") } ?: emptyMap()
        val bars = geometry?.optJSONArray("bars")?.objects() ?: emptyList()
        val joined = bars.flatMap { bar -> bar.array("items").values().map { (it as Number).toInt() } }.toSet()
        bars.forEach { bar -> Box(Modifier.placed(bar.getJSONObject("bounds"), density).background(colors.headerSurface, TileShape)) }
        if (editing) geometry?.array("zones")?.objects()?.forEachIndexed { index, zone ->
            val active = input.preview?.optJSONArray("target")?.optString(0) == listOf("left", "center", "right")[index]
            Box(Modifier.placed(zone, density).border(1.dp, colors.divider, ControlShape)
                .background(if (active) colors.accent.copy(alpha = .08f) else Color.Transparent, ControlShape))
        }
        entries.forEach { entry ->
            val id = entry.getInt("id")
            key(id) {
                val heldBounds = if (id == heldId) input.preview?.objectOrNull("held") else null
                val bounds = heldBounds ?: placements[id]?.getJSONObject("bounds")
                if (bounds != null) {
                    val x by animateFloatAsState(bounds.number("x"), tween(if (heldBounds != null) 0 else 120), label = "header-x")
                    val natural = metrics.objects().first { it.getInt("id") == id }.number("width")
                    HeaderItem(host, snapshot, input, entry, specs.getValue(id), size, title,
                        compact = bounds.number("width") < natural - .5f, inBar = id in joined,
                        modifier = Modifier.offset { IntOffset((x * density).roundToInt(), (bounds.number("y") * density).roundToInt()) }
                            .size(bounds.number("width").dp, bounds.number("height").dp).zIndex(if (heldBounds == null) 0f else 10f),
                        editing = editing)
                }
            }
        }
        geometry?.array("overflow")?.values()?.forEachIndexed { zone, value ->
            val bounds = value as? JSONObject ?: return@forEachIndexed
            val hidden = geometry.array("hidden").getJSONArray(zone).values().map { (it as Number).toInt() }.filter { specs.containsKey(it) }
            if (hidden.isEmpty()) return@forEachIndexed
            val single = hidden.singleOrNull()
            Box(Modifier.placed(bounds, density).testTag("header-overflow-$zone")
                .then(if (editing && single != null) Modifier.headerSource(input, obj("kind" to "item", "value" to single, "overflow_zone" to zone), specs.getValue(single).getString("label"), 1) else Modifier)) {
                HeaderButton("More title bar items", false, true, false, Modifier.fillMaxSize(),
                    inBar = bars.any { it.optInt("overflow", -1) == zone }, onClick = { input.overflow = if (input.overflow == zone) null else zone }) { SharedIcon("menu", "More title bar items", Modifier.size(size.number("icon").dp)) }
            }
        }
        // Overflow rows remain inside the stable capture owner. A native popup
        // would cancel the contact as soon as a row was dragged back to the bar.
        input.overflow?.let { zone ->
            val ids = resolved?.array("hidden")?.optJSONArray(zone)?.values()?.map { (it as Number).toInt() } ?: emptyList()
            if (ids.isNotEmpty()) {
                val anchor = resolved?.array("overflow")?.optJSONObject(zone)
                Column(Modifier.offset((anchor?.number("x") ?: 6f).coerceIn(0f, (width - 280f).coerceAtLeast(0f)).dp, height.dp)
                    .width(minOf(280f, width).dp).heightIn(max = 320.dp).zIndex(30f).shadow(8.dp, RoundedCornerShape(8.dp))
                    .background(colors.panel, RoundedCornerShape(8.dp)).chromeRegion(input.dock).verticalScroll(rememberScrollState()).testTag("header-overflow-list")) {
                    ids.forEach { id ->
                        val spec = specs.getValue(id)
                        Row(Modifier.fillMaxWidth().height(44.dp).testTag("header-overflow-item-$id")
                            .headerSource(input, obj("kind" to "item", "value" to id), spec.getString("label"), 2)
                            .then(if(entries.first { it.getInt("id")==id }.getJSONObject("item").getString("kind")=="document_title")Modifier.drawingDropTarget(host,!editing)else Modifier)
                            .clickable(enabled = !editing) {
                                val entry = entries.first { it.getInt("id") == id }
                                input.overflow = null
                                when (entry.getJSONObject("item").getString("kind")) {
                                    "menu", "menu_labels" -> overflowMenu = view.getJSONObject("primary_menu")
                                    "workspaces" -> overflowMenu = workspaceSwitcherMenu(host.workspaceManager)
                                    else -> activateHeader(host, entry)
                                }
                            }.padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                            if (editing) Box(Modifier.width(20.dp).fillMaxHeight()) { PanelGrip("Move ${spec.getString("label")}") }
                            Text(spec.getString("label"), maxLines = 1)
                        }
                    }
                }
            }
        }
        if (editing) HeaderBank(host, view, state, input, Modifier.offset(6.dp, (height + 6f).dp).width((width - 12f).coerceAtLeast(1f).dp)
            .onSizeChanged { bankHeight = it.height / density })
        if (heldId == null) input.held?.let { source -> input.preview?.objectOrNull("held")?.let { bounds ->
            Row(Modifier.placed(bounds, density).zIndex(40f).alpha(.9f).background(colors.panel, TileShape)
                .border(1.dp, colors.accent, TileShape).testTag("header-drag-ghost"), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.width(20.dp)) { PanelGrip("Move ${source.label}") }
                Text(source.label, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        } }
        input.context?.let { menu ->
            Box(Modifier.placed(input.contextBounds.headerBounds(), density)) {
                WorkspaceMenu(host, menu, preserveContact = input.contact) { input.context = null }
            }
        }
        overflowMenu?.let { menu ->
            Box(Modifier.offset(6.dp, height.dp)) { WorkspaceMenu(host, menu) { overflowMenu = null } }
        }
    }
}

private fun activateHeader(host: CanvasHost, entry: JSONObject) {
    when (entry.getJSONObject("item").getString("kind")) {
        "capy" -> host.invoke("zen_mode")
        "settings" -> host.invoke("settings")
        "document_title" -> host.drawingTabs.selector = true
        "tool" -> host.dispatch(obj("type" to "activate_header_item", "id" to entry.getInt("id")))
    }
}

@Composable private fun HeaderItem(host: CanvasHost, snapshot: JSONObject, input: HeaderInteraction, entry: JSONObject,
    spec: JSONObject, size: JSONObject, title: String, compact: Boolean, inBar: Boolean, modifier: Modifier, editing: Boolean) {
    val id = entry.getInt("id")
    val item = entry.getJSONObject("item")
    val kind = item.getString("kind")
    val activate=pickerClick(host,item.objectOrNull("control"),obj("kind" to "header","id" to id)) { activateHeader(host,entry) }
    val colors = LocalPalette.current
    val label = spec.getString("label")
    val focus = remember { FocusRequester() }
    LaunchedEffect(editing, input.selected) {
        // Selection owns native keyboard focus too. Otherwise Android consumes
        // the first navigation key leaving touch mode before Activity sees it.
        if (editing && input.selected == id) focus.requestFocus()
    }
    var menu by remember { mutableStateOf<JSONObject?>(null) }
    var menuLabel by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(compact, editing) { menu = null }
    DisposableEffect(menu != null) {
        val ownsPopup = menu != null
        if (ownsPopup) { input.dock.popupOpen = true; input.dock.refresh() }
        onDispose { if (ownsPopup) { input.dock.popupOpen = false; input.dock.refresh() } }
    }
    val open = snapshot.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer")?.getJSONObject("anchor")?.let {
        it.optString("kind") == "header" && it.optInt("id") == id
    } == true
    Row(modifier.alpha(if (!editing && !spec.optBoolean("enabled")) .4f else 1f).testTag("header-item-$id").headerSource(input, obj("kind" to "item", "value" to id), label, 1)
        .then(if (editing) Modifier.border(1.dp, if (input.selected == id) colors.accent else colors.divider, TileShape)
            .focusRequester(focus).onFocusChanged { if (it.isFocused) input.selected = id }.focusable().semantics { contentDescription = label; selected = input.selected == id } else Modifier),
        verticalAlignment = Alignment.CenterVertically) {
        if (editing) Box(Modifier.width(20.dp).fillMaxHeight().testTag("header-grip-$id"), contentAlignment = Alignment.Center) { PanelGrip("Move $label") }
        // SurfaceView artwork is outside Compose's render tree, so these use
        // the translucent fallback rather than a blur of the foreground text.
        Box(Modifier.weight(1f).fillMaxHeight().clipToBounds()
            .then(if (kind in listOf("document_title", "clock", "battery"))
                Modifier.background(colors.headerSurface, TileShape) else Modifier).then(if(kind=="document_title")Modifier.drawingDropTarget(host,!editing)else Modifier), contentAlignment = Alignment.Center) {
            val icon = when (kind) {
                "capy" -> snapshot.getJSONObject("state").array("commands").objects().first { it.getString("id") == "zen_mode" }.getString("icon")
                "settings" -> "settings"
                "tool" -> spec.getString("icon")
                else -> "menu"
            }
            when {
                kind == "menu_labels" && !compact -> Row(Modifier.height(34.dp).background(colors.headerSurface, SquircleShape(50)).padding(4.dp),
                    horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
                    snapshot.array("application_menus").objects().forEach { application ->
                        Box {
                            val menuId = application.getString("id")
                            HeaderButton(application.getString("label"), false, !editing, menu != null && menuLabel == menuId,
                                Modifier.height(26.dp).testTag("application-menu-${application.getString("id")}"),
                                fillWidth = false, surface = false, shape = SquircleShape(50),
                                onClick = { menuLabel = menuId; menu = application.getJSONObject("model") }) {
                                Text(application.getString("label"), Modifier.padding(horizontal = 8.dp), maxLines = 1)
                            }
                            if (menuLabel == menuId) menu?.let { WorkspaceMenu(host, it) { menu = null } }
                        }
                    }
                }
                kind == "workspaces" && !compact -> WorkspaceSwitcher(host, Modifier.fillMaxWidth(), interactive = !editing)
                kind == "document_title" -> DrawingHeader(host, title, editing)
                kind == "clock" -> SystemStatus(showBattery = false)
                kind == "battery" -> SystemStatus(clock = false)
                kind == "space" -> if (editing) Text("·", color = colors.secondary)
                else -> HeaderButton(label, spec.optBoolean("selected") && kind != "capy", !editing && spec.optBoolean("enabled"), open,
                    Modifier.fillMaxSize().testTag(if (kind == "menu_labels") "header-menu-labels-compact" else "header-control-$id"),
                    inBar = inBar, onClick = {
                        when (kind) {
                            "menu", "menu_labels" -> menu = snapshot.getJSONObject("header").getJSONObject("primary_menu")
                            "workspaces" -> menu = workspaceSwitcherMenu(host.workspaceManager)
                            else -> activate()
                        }
                    }) {
                    val fill = if (item.objectOrNull("control")?.optString("kind") == "color") snapshot.getJSONObject("state").getJSONObject("brush").array("color").let { Color(it.getDouble(0).toFloat(), it.getDouble(1).toFloat(), it.getDouble(2).toFloat()) } else null
                    val iconSize = if (kind == "capy") size.number("tile") * 440f / 512f else size.number("icon")
                    SharedIcon(icon, label, Modifier.size(iconSize.dp), fill = fill)
                }
            }
            if (kind != "menu_labels" || compact) menu?.let { WorkspaceMenu(host, it) { menu = null } }
        }
    }
}

/** Active tools stay blue through hover/press; actions use neutral feedback. */
@Composable private fun HeaderButton(label: String, selected: Boolean, enabled: Boolean, open: Boolean,
    modifier: Modifier, fillWidth: Boolean = true, surface: Boolean = true, shape: Shape = drawerButtonShape(if (open) "bottom" else null),
    inBar: Boolean = false, onClick: () -> Unit, content: @Composable () -> Unit) {
    val colors = LocalPalette.current
    val interaction = remember { MutableInteractionSource() }
    val hovered by interaction.collectIsHoveredAsState()
    val pressed by interaction.collectIsPressedAsState()
    val click = Modifier.hoverable(interaction).clickable(interactionSource = interaction, indication = rememberChromeFocusIndication(),
        enabled = enabled, role = Role.Button, onClickLabel = label, onClick = onClick)
    val inset = inBar && !open
    HoverTip(label, modifier) {
    Box((if (fillWidth) Modifier.fillMaxSize() else Modifier.fillMaxHeight())
        .then(if (inset) click.padding(vertical = 1.dp).clip(TileShape) else Modifier.clip(shape))
        .background(if (surface && !inBar) colors.headerSurface else Color.Transparent).background(when {
        selected && inBar -> colors.activeSolid
        selected -> colors.active
        open -> colors.panel
        enabled && pressed -> colors.text.copy(alpha = .16f)
        enabled && hovered -> colors.text.copy(alpha = .10f)
        else -> Color.Transparent
    }).then(if (inset) Modifier else click), contentAlignment = Alignment.Center) { content() }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable private fun HeaderBank(host: CanvasHost, view: JSONObject, state: JSONObject, input: HeaderInteraction, modifier: Modifier) {
    val colors = LocalPalette.current
    val entries = view.getJSONObject("model").headerEntries()
    val components = listOf(obj("item" to obj("kind" to "tools"), "label" to "Add Tools…")) + view.array("components").objects()
    Layout(modifier = modifier.heightIn(max = 230.dp).zIndex(20f).shadow(6.dp, SurfaceShape)
        .background(colors.panel, SurfaceShape).chromeRegion(input.dock).verticalScroll(rememberScrollState())
        .padding(6.dp).testTag("header-editor"), content = {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            components.filter { component -> !component.optBoolean("singleton") || entries.none { it.getJSONObject("item").toString() == component.getJSONObject("item").toString() } }.forEach { component ->
                val kind = component.getJSONObject("item").getString("kind")
                val label = component.getString("label")
                val source = if (kind == "tools") obj("kind" to "tools") else obj("kind" to "component", "value" to component.getJSONObject("item"))
                key(kind) {
                    Row(Modifier.height(36.dp).background(colors.button, ControlShape)
                        .headerSource(input, source, label, 1).testTag("header-component-$kind").semantics { contentDescription = label }
                        .padding(end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.width(20.dp), contentAlignment = Alignment.Center) { PanelGrip("Move $label") }
                        Text(label, maxLines = 1)
                    }
                }
            }
        }
        FlowRow(Modifier.testTag("header-editor-actions"), horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.End), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.height(36.dp).background(colors.button, ControlShape)) {
                view.array("sizes").objects().forEach { size ->
                    val selected = size.getString("id") == view.getJSONObject("model").getString("size")
                    TextButton({ host.headerEdit(obj("type" to "set_size", "size" to size.getString("id"))) },
                        Modifier.height(36.dp).testTag("header-size-${size.getString("id")}").background(if (selected) colors.active else Color.Transparent, ControlShape), contentPadding = PaddingValues(horizontal = 8.dp)) { Text(size.getString("label")) }
                }
            }
            val footer = state.getJSONObject("workspace").getJSONObject("layout").getJSONObject("canvas_info").optBoolean("visible")
            Row(Modifier.height(36.dp).testTag("header-show-footer").clickable { host.headerEdit(obj("type" to "canvas_info", "visible" to !footer)) }, verticalAlignment = Alignment.CenterVertically) {
                Checkbox(footer, null, Modifier.size(32.dp)); Text("Show footer")
            }
            TextButton({ host.headerEdit(obj("type" to "cancel")) }, Modifier.height(36.dp).testTag("header-edit-cancel")) { Text("Cancel") }
            Button({ host.headerEdit(obj("type" to "edit", "editing" to false)) }, Modifier.height(36.dp).testTag("header-edit-done")) { Text("Done") }
        }
    }) { children, constraints ->
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val actions = children[1].measure(loose)
        val gap = 6.dp.roundToPx()
        val remaining = (constraints.maxWidth - actions.width - gap).coerceAtLeast(0)
        // Keep controls at the trailing edge. If the bank cannot fit even its
        // widest chip beside them, wrap the controls below the full-width bank.
        val inline = remaining >= children[0].minIntrinsicWidth(Constraints.Infinity)
        val bank = children[0].measure(loose.copy(maxWidth = if (inline) remaining else constraints.maxWidth))
        val actionsY = if (inline) 0 else bank.height + gap
        layout(constraints.maxWidth, maxOf(bank.height, actionsY + actions.height)) {
            bank.placeRelative(0, 0)
            actions.placeRelative(constraints.maxWidth - actions.width, actionsY)
        }
    }
}
