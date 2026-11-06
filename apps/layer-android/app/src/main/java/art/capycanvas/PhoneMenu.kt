package art.capycanvas

import android.view.KeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlinx.coroutines.Job
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.launch
import kotlinx.coroutines.isActive
import org.json.JSONObject
import kotlin.math.roundToInt

internal class PhoneMenuInteraction(val host: CanvasHost, val dock: DockInteraction) {
    data class Hit(val value: JSONObject, val bounds: Rect, val focus: FocusRequester, val submenu: Boolean, val activate: () -> Unit)
    val hits = mutableMapOf<String, Hit>()
    var focused: String? = null
    var rows = emptyList<String>()
    var origin = Offset.Zero
    var list = Rect.Zero
    var route = JSONObject()
    var canBack = false
    var contact: JSONObject? = null
    var frame by mutableStateOf<JSONObject?>(null)
    var pressed by mutableStateOf<String?>(null)
    var progress by mutableFloatStateOf(0f)
    var swiping by mutableStateOf(false)
    var generation = 0
    var settling: Job? = null
    fun input(input: JSONObject): JSONObject {
        val request = obj("kind" to "menu_input", "route" to route, "can_back" to canBack, "input" to input)
        contact?.let { request.put("contact", it) }
        val value = phoneUi(request) as JSONObject
        contact = value.getJSONObject("contact")
        return value.getJSONObject("reply")
    }
    fun cancel() { generation++; contact = null; pressed = null; progress = 0f; swiping = false; settling?.cancel(); settling = null }
    fun hit(point: Offset) = hits.entries.lastOrNull { it.value.bounds.contains(point) }
}

private fun Modifier.menuHit(input: PhoneMenuInteraction?, hit: JSONObject, enabled: Boolean, submenu: Boolean = false, activate: () -> Unit): Modifier {
    if (input == null || !enabled) return this
    val key = hit.toString()
    return composed {
        val current by rememberUpdatedState(activate)
        val focus = remember { FocusRequester() }
        val bring = remember { BringIntoViewRequester() }
        val scope = rememberCoroutineScope()
        DisposableEffect(input, key) { onDispose { input.hits.remove(key);if (input.focused == key) input.focused = null } }
        focusRequester(focus).bringIntoViewRequester(bring).onFocusChanged {
            if (it.isFocused) { input.focused = key;scope.launch { bring.bringIntoView() } }
            else if (input.focused == key) input.focused = null
        }.onGloballyPositioned { input.hits[key] = PhoneMenuInteraction.Hit(hit, it.boundsInRoot().translate(-input.dock.origin-input.origin),focus,submenu) { current() } }
    }
}

private fun Modifier.menuGestures(input: PhoneMenuInteraction, width: Float, rtl: Boolean, scope: CoroutineScope): Modifier = pointerInput(input, width, rtl, input.route.toString()) {
    val job = currentCoroutineContext()[Job]!!
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        if (input.settling != null) { down.consume(); return@awaitEachGesture }
        if (down.isConsumed || !input.dock.enabled || input.dock.popupOpen) return@awaitEachGesture
        val hit = input.hit(down.position)
        val inList = input.list.contains(down.position)
        if (hit == null && !inList) return@awaitEachGesture
        val owner = input.dock.phone
        owner?.jobs?.add(job)
        val generation = input.generation
        var released = false
        input.pressed = hit?.key
        hit?.let { input.input(obj("kind" to "press", "hit" to it.value.value)) }
        if (inList) input.input(obj("kind" to "begin_swipe", "time_ms" to (down.uptimeMillis and 0xffffffffL)))
        try {
            while (true) {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                if (generation != input.generation || !input.dock.enabled || input.dock.popupOpen || event.changes.count { it.pressed } > 1) {
                    input.cancel(); change.consume(); break
                }
                val delta = change.position-down.position
                val moved = delta.getDistance() > viewConfiguration.touchSlop
                if (inList && change.pressed) {
                    val reply = input.input(obj("kind" to "swipe", "delta" to org.json.JSONArray(listOf(delta.x,delta.y)), "width" to width,
                        "past_slop" to moved, "rtl" to rtl, "time_ms" to (change.uptimeMillis and 0xffffffffL))).getJSONObject("swipe")
                    if (reply.getString("kind") != "pending") input.pressed = null
                    if (reply.getString("kind") == "back") { input.swiping = true; input.progress = reply.number("progress"); change.consume() }
                } else if (moved) { input.input(obj("kind" to "cancel_press")); input.pressed = null }
                if (!change.pressed) {
                    if (change.isConsumed) { input.cancel(); break }
                    if (input.swiping) {
                        val settle = input.input(obj("kind" to "end_swipe", "width" to width, "rtl" to rtl,
                            "time_ms" to (change.uptimeMillis and 0xffffffffL))).getJSONObject("settle")
                        val from = input.progress
                        val target = if (settle.isNull("action")) 0f else 1f
                        input.settling = scope.launch {
                            var completed = false
                            try {
                                workspaceFrames(settle.getLong("duration_ms")) { p -> input.progress = from+(target-from)*(1f-(1f-p)*(1f-p)*(1f-p)) }
                                if (generation == input.generation) settle.objectOrNull("action")?.let { action -> completed = true;input.host.phone(action) }
                            } finally { if (generation == input.generation && !completed) { input.progress = 0f; input.swiping = false; input.settling = null } }
                        }.also { animation -> owner?.jobs?.add(animation); animation.invokeOnCompletion { owner?.jobs?.remove(animation) } }
                    } else if (hit != null) {
                        val release = input.hit(change.position)
                        val reply = input.input(obj("kind" to "release", "hit" to (release?.value?.value ?: hit.value.value),
                            "inside" to (release?.key == hit.key)))
                        if (reply.getBoolean("accepted")) release?.value?.activate?.invoke()
                    }
                    released = true; change.consume(); break
                }
            }
        } finally {
            if (!released && generation == input.generation) input.cancel()
            input.pressed = null; owner?.jobs?.remove(job)
        }
    }
}

@Composable internal fun PhoneMenu(host: CanvasHost, snapshot: JSONObject, dock: DockInteraction) {
    val phone = snapshot.getJSONObject("state").getJSONObject("phone")
    val open = phone.getBoolean("menu")
    var model by remember { mutableStateOf<JSONObject?>(null) }
    val route = phone.getJSONObject("menu_route")
    val input = remember(host, dock) { PhoneMenuInteraction(host, dock) }
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    LaunchedEffect(open, snapshot.getJSONObject("state").getLong("revision"), host.drawingTabs.view.toString(), host.languageTag) {
        if (open) {
            val value = org.json.JSONTokener(host.drawingTabs.query(obj("op" to "phone_menu"))).nextValue() as? JSONObject
            if (value != null && host.snapshot?.getJSONObject("state")?.getJSONObject("phone")?.optBoolean("menu") == true) model = value
        }
    }
    val view = model ?: return
    val pages = view.array("pages").objects()
    val page = pages.firstOrNull { it.getJSONObject("id").toString() == route.toString() } ?: return
    val parent = page.objectOrNull("parent")?.let { parent -> pages.firstOrNull { it.getJSONObject("id").toString() == parent.toString() } }
    val structure = pages.map { p -> listOf(p.get("id"), p.opt("parent"), p.get("title"), p.get("checks"),
        p.array("sections").values().map { section -> (section as org.json.JSONArray).objects().map { row ->
            listOf(if (row.getBoolean("is_heading")) row.getString("label") else false, row.opt("target"), !row.isNull("close"))
        } }) }.toString()
    DisposableEffect(input, route.toString(), structure) {
        input.cancel(); input.route = route; input.canBack = parent != null; dock.phone?.menu = input
        onDispose { input.cancel(); if (dock.phone?.menu === input) dock.phone?.menu = null }
    }
    DisposableEffect(host, route.toString(), rtl) {
        val keys: (KeyEvent) -> Boolean = { event ->
            val back = event.keyCode == KeyEvent.KEYCODE_ESCAPE || event.keyCode == if (rtl) KeyEvent.KEYCODE_DPAD_RIGHT else KeyEvent.KEYCODE_DPAD_LEFT
            val forward = event.keyCode == if (rtl) KeyEvent.KEYCODE_DPAD_LEFT else KeyEvent.KEYCODE_DPAD_RIGHT
            val navigation = event.keyCode in listOf(KeyEvent.KEYCODE_DPAD_UP,KeyEvent.KEYCODE_DPAD_DOWN,KeyEvent.KEYCODE_MOVE_HOME,KeyEvent.KEYCODE_MOVE_END)
            if (event.action == KeyEvent.ACTION_DOWN) {
                if (back) host.phone(obj("kind" to "menu_back", "route" to route))
                else if (forward) input.hits[input.focused]?.takeIf { it.submenu }?.activate?.invoke()
                else if (navigation) {
                    val rows = input.rows.filter { it in input.hits }
                    if (rows.isNotEmpty()) {
                        val current = rows.indexOf(input.focused)
                        val next = when (event.keyCode) {
                            KeyEvent.KEYCODE_MOVE_HOME -> 0
                            KeyEvent.KEYCODE_MOVE_END -> rows.lastIndex
                            KeyEvent.KEYCODE_DPAD_UP -> (current-1+rows.size)%rows.size
                            else -> (current+1+rows.size)%rows.size
                        }
                        input.hits[rows[next]]?.focus?.requestFocus()
                    }
                }
            }
            back || forward || navigation
        }
        host.menuKeyHandler = keys
        onDispose { if (host.menuKeyHandler === keys) host.menuKeyHandler = null }
    }
    val layout = snapshot.getJSONObject("layout").getJSONObject("phone")
    val density = LocalDensity.current.density
    var target by remember { mutableStateOf<JSONObject?>(null) }
    LaunchedEffect(target?.toString(), open) {
        val to = target ?: return@LaunchedEffect
        val from = input.frame?.getJSONObject("placement")
        workspaceFrames(200) { p ->
            val current = JSONObject(Native.drawerFrame(obj("from" to from, "to" to to,
                "progress" to (1f-(1f-p)*(1f-p)*(1f-p)), "closing" to !open, "clip_contents" to true).toString()))
            input.frame = current
            current.objectOrNull("connection")?.let {
                dock.drawerSources["menu"] = DockInteraction.DrawerSource(current.getJSONObject("placement").getString("direction"),
                    current.getJSONObject("placement").getJSONObject("anchor").rect(), obj("kind" to "phone", "anchor" to obj("kind" to "capy")))
            } ?: dock.drawerSources.remove("menu")
            dock.refresh()
        }
        if (!open) model = null
    }
    DisposableEffect(dock) { onDispose { dock.drawerSources.remove("menu"); input.frame = null; dock.refresh() } }
    SubcomposeLayout(Modifier.fillMaxSize().zIndex(220f)) { constraints ->
        val root = pages.first { it.isNull("parent") }
        val probe = subcompose("root-measure") { PhoneMenuPage(host, root, view.number("close_width"), null, Modifier.clearAndSetSemantics {}) }.single()
        val minimum = probe.minIntrinsicWidth(Int.MAX_VALUE)/density
        val natural = probe.maxIntrinsicWidth(Int.MAX_VALUE)/density
        val row = root.array("sections").values().flatMap { (it as org.json.JSONArray).objects() }.first()
        val rowProbe = subcompose("row-measure") { MenuRow(row, Modifier.clearAndSetSemantics {}, checks = root.getBoolean("checks"), maxLines = 1) {} }.single()
            .measure(Constraints(maxWidth = constraints.maxWidth))
        val rowHeight = rowProbe.height/density
        val width = (phoneUi(obj("kind" to "menu_width", "layout" to layout, "minimum" to minimum, "natural" to natural, "row_height" to rowHeight)) as Number).toFloat()
        val rootSize = probe.measure(Constraints.fixedWidth((width*density).roundToInt().coerceAtLeast(1)))
        val next = phoneUi(obj("kind" to "menu_placement", "layout" to layout, "viewport" to snapshot.getJSONObject("layout").getJSONArray("viewport"),
            "width" to width, "height" to rootSize.height/density, "row_height" to rowHeight)) as? JSONObject
        if (next != null && target?.toString() != next.toString()) target = next
        val destination = next ?: target
        val shown = input.frame
        val body = if (shown != null && destination != null) subcompose("body") {
            key(host.phoneContactEpoch) { PhoneMenuSurface(host, dock, view, page, parent, input, destination, shown, rowHeight, rtl) }
        }.single().measure(constraints) else null
        layout(constraints.maxWidth, constraints.maxHeight) { body?.place(0,0) }
    }
}

@Composable private fun rememberMenuActivation(host: CanvasHost, route: String? = null, enabled: Boolean = true): () -> Boolean = key(route,enabled) {
    val scope = rememberCoroutineScope()
    return@key { enabled && scope.isActive && host.snapshot?.getJSONObject("state")?.getJSONObject("phone")?.let {
        it.getBoolean("menu") && (route == null || it.getJSONObject("menu_route").toString() == route)
    } == true }
}

@Composable private fun PhoneMenuSurface(host: CanvasHost, dock: DockInteraction, model: JSONObject, page: JSONObject, parent: JSONObject?,
    input: PhoneMenuInteraction, target: JSONObject, frame: JSONObject, rowHeight: Float, rtl: Boolean) {
    val density = LocalDensity.current.density
    val scope = rememberCoroutineScope()
    val active = rememberMenuActivation(host)
    DisposableEffect(input,target.toString()) { onDispose { input.cancel() } }
    val placement = frame.getJSONObject("placement")
    val bounds = placement.getJSONObject("bounds")
    val targetBounds = target.getJSONObject("bounds")
    val geometry = phoneUi(obj("kind" to "menu_geometry", "layout" to host.snapshot!!.getJSONObject("layout").getJSONObject("phone"),
        "placement" to target, "row_height" to rowHeight)) as JSONObject
    val connection = frame.objectOrNull("connection")
    val panelShape = dock.drawerContainerShape(bounds, joined = connection?.array("square_corners"))
    val colors = LocalPalette.current
    SideEffect {
        input.origin = Offset(bounds.number("x")*density,bounds.number("y")*density)
        input.list = geometry.getJSONObject("list").let { Rect(it.number("x")*density,it.number("y")*density,(it.number("x")+it.number("width"))*density,(it.number("y")+it.number("height"))*density) }
    }
    Box(Modifier.fillMaxSize(), contentAlignment = AbsoluteAlignment.TopLeft) {
        connection?.let { DrawerBridge(it, dock, 219f) }
        Box(Modifier.placedAbsolute(bounds, density).testTag("phone-menu").chromeRegion(dock).panelSurface(6.dp, panelShape)
            .glass(panelShape, colors.onGlass.panelFill).semantics { paneTitle = page.getString("title") }
            .menuGestures(input, geometry.getJSONObject("list").number("width")*density, rtl, scope), contentAlignment = AbsoluteAlignment.TopLeft) {
            Box(Modifier.requiredSize(targetBounds.number("width").dp,targetBounds.number("height").dp), contentAlignment = AbsoluteAlignment.TopLeft) {
                val scroll = rememberScrollState()
                LaunchedEffect(page.getJSONObject("id").toString()) { scroll.scrollTo(0) }
                LaunchedEffect(scroll.value) { input.input(obj("kind" to "cancel_press")); input.pressed = null }
                val list = geometry.getJSONObject("list")
                Box(Modifier.placedAbsolute(list, density).clipToBounds().testTag("phone-menu-scroll"), contentAlignment = AbsoluteAlignment.TopLeft) {
                    if (input.swiping && parent != null) PhoneMenuPage(host, parent, model.number("close_width"), null,
                        Modifier.fillMaxWidth().graphicsLayer { clip = true; shape = androidx.compose.foundation.shape.GenericShape { size, _ ->
                            val distance = size.width*input.progress
                            addRect(if (rtl) Rect(size.width-distance,0f,size.width,size.height) else Rect(0f,0f,distance,size.height))
                        } }.clearAndSetSemantics {})
                    PhoneMenuPage(host, page, model.number("close_width"), input,
                        Modifier.fillMaxWidth().graphicsLayer { translationX = list.number("width")*density*input.progress*(if (rtl) -1 else 1) }
                            .verticalScroll(scroll))
                }
                Box(Modifier.placedAbsolute(geometry.getJSONObject("separator"), density).background(colors.menuSeparator))
                val names = listOf("preferences","search","finger","zen","swap")
                model.array("strip").objects().forEachIndexed { index, item ->
                    val hit = obj("kind" to "strip", "position" to index)
                    val action = { if (active()) item.objectOrNull("action")?.let(host::dispatch); Unit }
                    HeaderButton(item.getString("label"), item.optBoolean("selected"), item.getBoolean("enabled"), false,
                        Modifier.placedAbsolute(geometry.array("buttons").getJSONObject(index), density).testTag("phone-${names[index]}").then(if (item.isNull("selected")) Modifier else Modifier.semantics { selected = item.getBoolean("selected") })
                            .menuHit(input, hit, item.getBoolean("enabled"), activate = action), inBar = true, onClick = action) {
                        SharedIcon(item.getString("icon"), null, Modifier.size(geometry.number("icon").dp))
                    }
                }
            }
        }
    }
}

@Composable private fun PhoneMenuPage(host: CanvasHost, page: JSONObject, closeWidth: Float, input: PhoneMenuInteraction?, modifier: Modifier = Modifier) {
    val colors = LocalPalette.current
    val active = rememberMenuActivation(host,page.getJSONObject("id").toString(),input != null)
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    val order = buildList {
        if (!page.isNull("parent")) add(obj("kind" to "back").toString())
        page.array("sections").values().forEachIndexed { section, rows ->
            (rows as org.json.JSONArray).objects().forEachIndexed { index, item ->
                val position = org.json.JSONArray(listOf(section,index))
                add(obj("kind" to "row","position" to position).toString())
                if (!item.isNull("close")) add(obj("kind" to "close","position" to position).toString())
            }
        }
    }
    SideEffect { if (input != null) input.rows = order }
    val focus = remember(page.getJSONObject("id").toString()) { FocusRequester() }
    LaunchedEffect(page.getJSONObject("id").toString(), input) { if (input != null) focus.requestFocus() }
    Column(modifier.focusProperties { canFocus = input != null }.padding(vertical = 6.dp)) {
        if (!page.isNull("parent")) {
            val hit = obj("kind" to "back")
            val action = { if (active()) host.phone(obj("kind" to "menu_back", "route" to page.getJSONObject("id"))) }
            Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).then(if (input != null) Modifier.testTag("phone-menu-back").focusRequester(focus) else Modifier).menuHit(input, hit, true, activate = action).clickable(onClickLabel = host.bootstrap!!.getJSONObject("common").getString("back"), onClick = action)
                .padding(horizontal = 16.dp,vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) { SharedIcon("back", null, Modifier.graphicsLayer { scaleX = if (rtl) -1f else 1f }) }
                Text(page.getString("title"), Modifier.weight(1f), textAlign = androidx.compose.ui.text.style.TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Spacer(Modifier.width(24.dp))
            }
        }
        page.array("sections").values().forEachIndexed { sectionIndex, value ->
            if (sectionIndex > 0) HorizontalDivider(Modifier.padding(horizontal = 6.dp,vertical = 6.dp), color = colors.menuSeparator)
            (value as org.json.JSONArray).objects().forEachIndexed { index, item ->
                val hit = obj("kind" to "row", "position" to org.json.JSONArray(listOf(sectionIndex,index)))
                val action = {
                    if (active()) item.objectOrNull("target")?.let { host.phone(obj("kind" to "menu_select", "route" to it)) }
                        ?: item.objectOrNull("action")?.let(host::dispatch)
                    Unit
                }
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    MenuRow(item, Modifier.weight(1f).then(if (input != null && page.isNull("parent") && sectionIndex == 0 && index == 0) Modifier.focusRequester(focus) else Modifier).then(if (input != null) Modifier.testTag(if (item.objectOrNull("target")?.objectOrNull("page")?.optString("kind") == "drawings") "phone-drawing" else "phone-menu-row-$sectionIndex-$index") else Modifier)
                        .menuHit(input, hit, item.getBoolean("enabled") && !item.optBoolean("is_heading"),submenu = item.objectOrNull("target") != null,activate = action), checks = page.getBoolean("checks"),
                        submenu = item.objectOrNull("target") != null, detail = item.getString("count").ifEmpty { item.getString("hint") },
                        pressed = input?.pressed == hit.toString(), maxLines = 1,
                        overflow = if (item.optString("ellipsis") == "middle") TextOverflow.MiddleEllipsis else TextOverflow.Ellipsis, onClick = action)
                    item.objectOrNull("close")?.let { close ->
                        val closeHit = obj("kind" to "close", "position" to org.json.JSONArray(listOf(sectionIndex,index)))
                        val closeAction = { if (active()) host.dispatch(close.getJSONObject("action")) }
                        HeaderButton(close.getString("label"), false, item.getBoolean("enabled"), false,
                            Modifier.width(closeWidth.dp).height(36.dp).then(if (input != null) Modifier.testTag("phone-menu-close-$sectionIndex-$index") else Modifier).menuHit(input, closeHit, item.getBoolean("enabled"), activate = closeAction),
                            surface = false, onClick = closeAction) { SharedIcon("close", null, Modifier.size(16.dp)) }
                    }
                }
            }
        }
    }
}
