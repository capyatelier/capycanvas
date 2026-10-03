package art.capycanvas

import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

private fun CanvasHost.effect(action: JSONObject) = dispatch(obj("type" to "effect", "action" to action))

/** Category/search decisions and preview sampling are shared Rust policy. Only
 * visible row geometry and native bitmap presentation belong to this view. */
@Composable internal fun AdjustmentPanel(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier, splitPicker: Boolean = false, onContent: (PanelContentSize) -> Unit = {}) {
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val picker = state.getJSONObject("filter_picker")
    val choices = state.array("adjustments").objects()
    val categories = state.array("filter_categories").objects()
    var headerHeight by remember { mutableFloatStateOf(0f) }
    var rowHeight by remember { mutableFloatStateOf(64f) }
    var categoryHeight by remember { mutableFloatStateOf(36f) }
    var emptyHeight by remember { mutableFloatStateOf(36f) }
    val categoryCount = if(splitPicker) 0 else choices.filterIndexed { i, choice -> i == 0 || choices[i - 1].getString("category") != choice.getString("category") }.size
    val listHeight = if (choices.isEmpty()) emptyHeight else choices.size * rowHeight + categoryCount * categoryHeight + (choices.size + categoryCount - 1) * 2f
    val fixedHeight = if(splitPicker) 16f else headerHeight + 18f // Native outer padding and header/list gap.
    val measured = if (splitPicker || headerHeight > 0f) PanelContentSize(fixedHeight + listHeight, fixedHeight, rowHeight + 2f) else null
    SideEffect { measured?.let(onContent) }
    val currentChoices by rememberUpdatedState(choices)
    val list = rememberLazyListState()
    val focus = remember { FocusRequester() }
    val cache = host.filterPreviewCache
    var width by remember { mutableIntStateOf(0) }
    val currentSize by rememberUpdatedState(listOf((width - 12*density).roundToInt().coerceIn(80,512), (40*density).roundToInt().coerceIn(1,128)))
    val search = picker.takeUnless { it.isNull("search") }?.getString("search")
    fun send(action: JSONObject) = host.dispatch(obj("type" to "filter_picker", "action" to action))
    LaunchedEffect(search != null) { if (search != null) focus.requestFocus() }
    val previewView = remember { Any() }
    DisposableEffect(host, previewView) { onDispose { cache.remove(previewView) } }
    LaunchedEffect(host, previewView) {
        snapshotFlow {
            val visible = list.layoutInfo.visibleItemsInfo.map { it.key }.toSet()
            currentChoices.map { it.getString("id") }.filter { it in visible } to currentSize
        }.collect { (ids, size) -> cache.update(previewView, ids, size) }
    }
    Column(modifier.padding(if (splitPicker) 8.dp else 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if(!splitPicker) Row(Modifier.fillMaxWidth().wrapContentHeight(unbounded = true).onSizeChanged { headerHeight = it.height / density }, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            if (search == null) SharedIcon(categories.firstOrNull { it.optString("id") == picker.optString("category") }?.getString("icon") ?: "adjustments", null)
            Box(Modifier.weight(1f)) {
                if (search != null) CoreTextField(search, { send(obj("op" to "search", "query" to it)) },
                    Modifier.fillMaxWidth().focusRequester(focus).testTag("filter-search"), height = 34.dp, maxLength = 120, shape = ControlShape,
                    placeholder = { Text(picker.getString("search_label"), maxLines = 1) })
                else PropertyChoice(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("category"), categories.map { it.getString("label") },
                    categories.indexOfFirst { it.optString("id") == picker.optString("category") }.coerceAtLeast(0)) {
                    send(obj("op" to "category", "category" to categories[it].get("id")))
                }
            }
            Box(Modifier.size(48.dp,34.dp).clip(ControlShape).testTag("filter-search-toggle")
                .clickable { send(obj("op" to "toggle_search")) }, contentAlignment = Alignment.Center) {
                SharedIcon("search", picker.getString("search_label"))
            }
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth().onSizeChanged { width = it.width }.testTag("filter-list"),
            state = list, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            var category: String? = null
            choices.forEach { choice ->
                val id = choice.getString("id")
                if(!splitPicker && category != choice.getString("category")) {
                    category = choice.getString("category")
                    item("category-$category") {
                        Row(Modifier.onSizeChanged { categoryHeight = it.height / density }.padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            SharedIcon(choice.getString("category_icon"), null, tint = colors.secondary)
                            Text(choice.getString("category_label"), color = colors.secondary, fontWeight = FontWeight.Bold)
                        }
                    }
                }
                item(id) {
                    HoverTip(choice.getString("tooltip"), Modifier.fillMaxWidth().onSizeChanged { rowHeight = it.height / density }) {
                        Column(Modifier.fillMaxWidth().testTag("adjustment-$id").clip(ControlShape)
                            .background(if(picker.optString("selected")==id) colors.active else Color.Transparent)
                            .clickable { host.dispatch(choice.getJSONObject("action")) }.padding(horizontal = 6.dp, vertical = 3.dp)) {
                            val image = cache.images[id]?.image
                            if(image != null) Image(image, null, Modifier.fillMaxWidth().height(40.dp).testTag("filter-preview-$id"), contentScale = ContentScale.FillBounds)
                            else Spacer(Modifier.fillMaxWidth().height(40.dp))
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.End) {
                                if(choice.getBoolean("animated")) SharedIcon("animation", choice.getString("tooltip"), Modifier.padding(end = 4.dp).size(12.dp).alpha(.55f))
                                SharedIcon(choice.getString("icon"), null, Modifier.padding(end = 6.dp).size(16.dp).testTag("filter-icon-$id"))
                                Text(choice.getString("label"), maxLines = 1, overflow = TextOverflow.Ellipsis, color = colors.text)
                            }
                        }
                    }
                }
            }
            if(choices.isEmpty()) item { Text(picker.getString("empty_label"), Modifier.onSizeChanged { emptyHeight = it.height / density }.padding(8.dp), color = colors.secondary) }
        }
    }
}

@Composable internal fun FilterTypesPanel(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier) {
    val picker=state.getJSONObject("filter_picker")
    Column(modifier.padding(8.dp),verticalArrangement=Arrangement.spacedBy(6.dp)) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            state.array("filter_categories").objects().forEach { category ->
                item(category.optString("id")) {
                    Row(Modifier.fillMaxWidth().heightIn(min=44.dp).testTag("filter-type-${category.optString("id")}")
                        .background(if(category.optString("id")==picker.optString("category")) LocalPalette.current.active else Color.Transparent)
                        .clickable { host.dispatch(obj("type" to "filter_picker","action" to obj("op" to "category","category" to category.get("id")))) }.padding(horizontal=6.dp),
                        verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
                        SharedIcon(category.getString("icon"),null)
                        Text(category.getString("label"),maxLines=1,overflow=TextOverflow.Ellipsis)
                    }
                }
            }
        }
        TextButton(onClick={host.effect(obj("op" to "cancel_filter"))},modifier=Modifier.testTag("cancel-filter")) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) }
    }
}

@Composable internal fun PropertyChoice(label: String, options: List<String>, selected: Int, enabled: Boolean = true, onOpenChanged: (Boolean) -> Unit = {}, select: (Int) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val openChanged by rememberUpdatedState(onOpenChanged)
    fun close() { open=false;openChanged(false) }
    DisposableEffect(Unit) { onDispose { if(open)openChanged(false) } }
    Box {
        Row(Modifier.fillMaxWidth().heightIn(min = 32.dp).clip(ControlShape)
            .background(LocalPalette.current.input).clickable(enabled = enabled) { openChanged(true);open = true }.padding(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(options.getOrNull(selected) ?: label, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            SharedIcon("chevron-down", label)
        }
        DropdownMenu(open, ::close) {
            options.forEachIndexed { index, text -> DropdownMenuItem(text = { Text(text) }, onClick = { close(); select(index) }) }
        }
    }
}

internal fun propertySectionId(control: JSONObject): String = JSONArray().put(control.opt("section_id") ?: JSONObject.NULL).toString()

@Composable internal fun LayerPropertiesPanel(host: CanvasHost, state: JSONObject) {
    val view = state.getJSONObject("layer_properties")
    val controls = view.array("controls").objects()
    val layer = view.optLong("layer")
    val enabled = view.getBoolean("enabled")
    val pages = view.array("pages").objects()
    key(state.getJSONObject("document_file").optLong("epoch"), layer) {
    Column(Modifier.fillMaxWidth().testTag("layer-properties").alpha(if(enabled) 1f else .4f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(view.getString("title"), fontWeight = FontWeight.Bold)
        if (pages.size > 1) Box(Modifier.testTag("properties-page")) {
            PropertyChoice(view.getString("title"), pages.map { it.getString("label") }, pages.indexOfFirst { it.getString("id") == view.optString("page") }, enabled) {
                host.effect(obj("op" to "select_page", "layer" to layer, "page" to pages[it].getString("id")))
            }
        }
        controls.forEachIndexed { index, control ->
            val section = control.takeUnless { it.isNull("section") }?.getString("section")
            val sectionId = propertySectionId(control)
            val previousSectionId = controls.getOrNull(index - 1)?.let(::propertySectionId) ?: "[null]"
            if(sectionId != previousSectionId) {
                if(index > 0) HorizontalDivider(Modifier.padding(vertical = 3.dp), color = LocalPalette.current.text.copy(alpha = .15f))
                if(section != null) Text(section, Modifier.padding(start = 6.dp), fontWeight = FontWeight.Bold)
            }
            val key = control.getString("key")
            val label = control.getString("label")
            val kind = control.getJSONObject("kind")
            val value = control.getJSONObject("value").get("value")
            fun change(value: Any) = host.effect(obj("op" to "set", "layer" to layer, "key" to key,
                "value" to obj("kind" to kind.getString("kind"), "value" to value)))
            when(kind.getString("kind")) {
                "number" -> key(layer, key, kind.toString()) {
                    EffectNumber(host, label, (value as Number).toDouble(), kind.getJSONObject("numeric"), enabled, key) { obj("op" to "number", "layer" to layer, "key" to key) }
                }
                "curve" -> key(layer, key, control.getJSONObject("curve").getJSONObject("domain").toString()) { CurveControl(host, layer, control, enabled) }
                "toggle" -> Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(label, Modifier.weight(1f)); Switch(value as Boolean, { change(it) }, enabled = enabled)
                }
                "choice" -> Row(Modifier.fillMaxWidth().testTag("property-$key"),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
                    Text(label,Modifier.weight(1f))
                    Box(Modifier.weight(2f)){PropertyChoice(label,kind.array("options").values().map{it.toString()},(value as Number).toInt(),enabled){change(it)}}
                }
                "color" -> ManagedColorButton(host,label,value as JSONObject,enabled,trailing = {
                    if(!control.isNull("color_action")) Box(Modifier.size(40.dp,36.dp).testTag("${key.replace('_','-')}-bucket")
                        .clickable(enabled=enabled){host.dispatch(control.getJSONObject("color_action"))},contentAlignment=Alignment.Center) { SharedIcon("fill",host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("use_selected")) }
                }) { change(it) }
                "gradient" -> GradientControl(host,layer,control,enabled)
            }
        }
    }
    }
}

@Composable private fun GradientControl(host:CanvasHost,layer:Long,control:JSONObject,enabled:Boolean) {
    val colors=LocalPalette.current
    val key=control.getString("key")
    val stops=control.getJSONObject("value").getJSONArray("value").objects()
    var selected by remember(layer,key) {mutableIntStateOf(0)}
    val index=selected.coerceIn(stops.indices)
    val current by rememberUpdatedState(stops)
    fun change(i:Int?,position:Float,color:JSONObject?=null,remove:Boolean=false) = host.effect(obj("op" to "gradient_stop","layer" to layer,"key" to key,"index" to i,"position" to position,"color" to color,"remove" to remove))
    val samples = remember(control.getJSONObject("value").toString(), documentRgbSpace(host)) {
        JSONArray(Native.colorUi(obj("type" to "gradient", "stops" to control.getJSONObject("value").getJSONArray("value"),
            "document_space" to documentRgbSpace(host)).toString(), host.languageTag)).objects()
    }
    Canvas(Modifier.fillMaxWidth().height(44.dp).testTag("effect-gradient").pointerInput(layer,key,enabled) {
        if(!enabled)return@pointerInput
        awaitEachGesture {
            val down=awaitFirstDown();down.consume();val p=((down.position.x-6.dp.toPx())/(size.width-12.dp.toPx())).coerceIn(0f,1f)
            val found=current.indexOfFirst { kotlin.math.abs(it.number("position")-p)*(size.width-12.dp.toPx())<12.dp.toPx() }
            if(found>=0)selected=found else {selected=current.count {it.number("position")<p};change(null,p)}
        }
    }) {
        val margin=6.dp.toPx();val width=size.width-2*margin
        val ramp=samples.mapIndexed { i, sample -> i.toFloat() / (samples.size - 1) to displayColor(sample) }.toTypedArray()
        drawRect(Brush.horizontalGradient(*ramp,startX=margin,endX=size.width-margin),Offset(margin,0f),androidx.compose.ui.geometry.Size(width,32.dp.toPx()))
        stops.forEachIndexed { i,s ->drawCircle(colors.text,(if(index==i)4f else 2.5f).dp.toPx(),Offset(margin+s.number("position")*width,39.dp.toPx())) }
    }
    NumericSetting(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("position"),stops[index].number("position"),host.catalog.getJSONObject("opacity"),enabled=enabled && index>0 && index<stops.lastIndex) {change(index,it)}
    ManagedColorButton(host,host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("color"),stops[index].getJSONObject("color"),enabled) {change(index,stops[index].number("position"),it)}
    Row(horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        TextButton(enabled=enabled && index>0 && index<stops.lastIndex,onClick={selected=(index-1).coerceAtLeast(0);change(index,0f,remove=true)}) {Text(host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("remove_stop"))}
        TextButton(enabled=enabled,onClick={host.effect(obj("op" to "reset","layer" to layer,"key" to key))}) {Text(host.bootstrap!!.getJSONObject("common").getString("reset"))}
    }
}

@Composable private fun EffectNumber(host: CanvasHost, label: String, value: Double, numeric: JSONObject,
    enabled: Boolean, id: String, text: String? = null, request: () -> JSONObject) {
    val currentRequest by rememberUpdatedState(request)
    var owner by remember { mutableStateOf<JSONObject?>(null) }
    var latest by remember { mutableDoubleStateOf(value) }
    SideEffect { if (owner == null) latest = value }
    fun action(value: Double) = JSONObject((owner ?: currentRequest()).toString()).put("operation", obj("type" to "value", "value" to value))
    NumericSetting(label, value, numeric, enabled = enabled, id = id, presentedText = text, onEditPhase = { phase ->
        if (phase == "down") { owner = currentRequest(); latest = value }
        val action = action(latest)
        if (phase != "down") owner = null
        host.effect(obj("op" to "gesture", "phase" to phase, "action" to action))
    }) { next ->
        latest = next
        val action = action(next)
        host.effect(if (owner == null) action else obj("op" to "gesture", "phase" to "move", "action" to action))
    }
}

private data class CurveTap(val position: Offset, val time: Long, val epoch: Long, val points: Int)

@Composable private fun CurveControl(host: CanvasHost, layer: Long, control: JSONObject, enabled: Boolean) {
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val current by rememberUpdatedState(control)
    val key = control.getString("key")
    val curve = control.getJSONObject("curve")
    val axes = curve.array("axes").objects()
    val focus = remember { FocusRequester() }
    var origin by remember { mutableStateOf(Offset.Zero) }
    var contactOwner by remember { mutableStateOf<JSONObject?>(null) }
    var keyOwner by remember { mutableStateOf<Pair<Key, JSONObject>?>(null) }
    var lastTap by remember { mutableStateOf<CurveTap?>(null) }
    fun owner() = obj("layer" to layer, "key" to key, "epoch" to current.getJSONObject("curve").getLong("epoch"))
    fun send(owner: JSONObject, op: String, phase: String? = null, point: Offset = Offset.Zero, extent: Offset = Offset(1f, 1f)) {
        val action = JSONObject(owner.toString()).put("op", op).put("point", JSONArray(listOf(point.x, point.y))).put("extent", JSONArray(listOf(extent.x, extent.y)))
        phase?.let { action.put("phase", it) }; host.effect(action)
    }
    fun cancel() {
        val captured = contactOwner ?: keyOwner?.second
        contactOwner = null; keyOwner = null
        if (captured != null) send(captured, "curve_contact", "cancel")
    }
    DisposableEffect(Unit) { onDispose { cancel(); if (host.curveControlFocus === focus) host.curveControlFocus = null } }
    Column(Modifier.fillMaxWidth().testTag("curve-$key"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Column(Modifier.height(200.dp), verticalArrangement = Arrangement.SpaceBetween, horizontalAlignment = Alignment.CenterHorizontally) {
                Text(axes[1].getString("maximum"), color = colors.secondary)
                Text(axes[1].getString("label"), color = colors.secondary)
                Text(axes[1].getString("minimum"), color = colors.secondary)
            }
            Column(Modifier.weight(1f)) {
                Box(Modifier.fillMaxWidth().height(200.dp)) {
                    Canvas(Modifier.fillMaxSize().testTag("effect-curve").semantics { contentDescription = control.getString("label") }.background(colors.input)
                        .onGloballyPositioned { origin = it.positionInRoot() }.focusRequester(focus)
                        .onFocusChanged {
                            if (it.isFocused) host.curveControlFocus = focus
                            else if (host.curveControlFocus === focus) { host.curveControlFocus = null; cancel() }
                        }.onPreviewKeyEvent { event ->
                            val name = when (event.key) {
                                Key.DirectionLeft -> "ArrowLeft"; Key.DirectionRight -> "ArrowRight"
                                Key.DirectionUp -> "ArrowUp"; Key.DirectionDown -> "ArrowDown"
                                Key.Delete -> "Delete"; Key.Backspace -> "Backspace"; Key.Escape -> "Escape"
                                else -> null
                            }
                            val pressed = event.type == KeyEventType.KeyDown
                            if (name == null || host.textComposition.owns(event.nativeKeyEvent) || (pressed && (event.isCtrlPressed || event.isMetaPressed || event.isAltPressed))) false
                            else {
                                val target = keyOwner?.takeIf { it.first == event.key }?.second ?: owner()
                                if (pressed) keyOwner = event.key to target
                                host.effect(JSONObject(target.toString()).put("op", "curve_key").put("key_event", name).put("pressed", pressed)
                                    .put("repeat", event.nativeKeyEvent.repeatCount > 0).put("modifiers", obj("command" to (event.isCtrlPressed || event.isMetaPressed), "shift" to event.isShiftPressed, "alt" to event.isAltPressed)))
                                if ((!pressed && keyOwner?.first == event.key) || event.key == Key.Escape) keyOwner = null
                                if (event.key == Key.Escape) contactOwner = null
                                true
                            }
                        }.focusable(enabled)
                        .pointerInput(layer, key, enabled) {
                            if (!enabled) return@pointerInput
                            awaitEachGesture {
                                val down = awaitFirstDown(); down.consume(); focus.requestFocus(); cancel()
                                val captured = owner(); val start = origin
                                val count = current.getJSONObject("value").array("value").length()
                                val extent = Offset(size.width / density, size.height / density)
                                fun point(position: Offset) = (position + origin - start) / density
                                val double = lastTap?.let { tap -> tap.epoch == captured.getLong("epoch") && down.uptimeMillis - tap.time in viewConfiguration.doubleTapMinTimeMillis..viewConfiguration.doubleTapTimeoutMillis && (down.position + origin - tap.position).getDistance() <= viewConfiguration.touchSlop * 2 } == true
                                if (currentEvent.buttons.isSecondaryPressed || double) {
                                    if (!currentEvent.buttons.isSecondaryPressed) captured.put("point_count", lastTap!!.points)
                                    lastTap = null; send(captured, "curve_remove_at", point = point(down.position), extent = extent)
                                    return@awaitEachGesture
                                }
                                contactOwner = captured; send(captured, "curve_contact", "down", point(down.position), extent)
                                var released = false
                                try {
                                    while (true) {
                                        val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                                        if (!change.pressed) {
                                            released = true
                                            val tapped = (change.position + origin - down.position - start).getDistance() < viewConfiguration.touchSlop
                                            lastTap = if (tapped) CurveTap(change.position + origin, change.uptimeMillis, captured.getLong("epoch"), count) else null
                                            if (contactOwner === captured) { contactOwner = null; send(captured, "curve_contact", "up", point(change.position), extent) }
                                            break
                                        }
                                        if (change.position != change.previousPosition && contactOwner === captured) { change.consume(); send(captured, "curve_contact", "move", point(change.position), extent) }
                                    }
                                } finally { if (!released && contactOwner === captured) { contactOwner = null; send(captured, "curve_contact", "cancel") } }
                            }
                        }) {
                        for (i in 1..3) {
                            drawLine(colors.text.copy(alpha = .2f), Offset(size.width * i / 4, 0f), Offset(size.width * i / 4, size.height))
                            drawLine(colors.text.copy(alpha = .2f), Offset(0f, size.height * i / 4), Offset(size.width, size.height * i / 4))
                        }
                        if (!axes[0].isNull("white")) {
                            val x = axes[0].number("white") * size.width; val y = (1 - axes[1].number("white")) * size.height
                            val dash = PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 3.dp.toPx()))
                            drawLine(colors.text, Offset(x, 0f), Offset(x, size.height), pathEffect = dash)
                            drawLine(colors.text, Offset(0f, y), Offset(size.width, y), pathEffect = dash)
                        }
                        val path = Path()
                        control.array("plot").values().forEachIndexed { i, raw ->
                            val p = raw as JSONArray; val x = p.getDouble(0).toFloat() * size.width; val y = (1 - p.getDouble(1).toFloat()) * size.height
                            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
                        }
                        drawPath(path, colors.text, style = Stroke(1.5.dp.toPx()))
                        control.getJSONObject("value").array("value").values().forEachIndexed { index, raw ->
                            val p = raw as JSONArray; val selected = !curve.isNull("selected") && curve.getInt("selected") == index
                            drawCircle(colors.text, (if (selected) 5f else 3.5f).dp.toPx(), Offset(p.getDouble(0).toFloat() * size.width, (1 - p.getDouble(1).toFloat()) * size.height), style = if (selected) Stroke(1.5.dp.toPx()) else androidx.compose.ui.graphics.drawscope.Fill)
                        }
                    }
                    if (control.optBoolean("modified")) Box(Modifier.align(Alignment.BottomEnd).padding(2.dp).size(32.dp).testTag("curve-reset")
                        .clickable(enabled = enabled) { lastTap = null; host.effect(obj("op" to "reset", "layer" to layer, "key" to key)) }, contentAlignment = Alignment.Center) {
                        SharedIcon("reset", curve.getString("reset_label"), tint = colors.secondary)
                    }
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    for (name in listOf("minimum", "label", "maximum")) Text(axes[0].getString(name), color = colors.secondary)
                }
            }
        }
        listOf("input", "output").forEachIndexed { index, axis ->
            val coordinate = curve.optJSONObject(axis)
            EffectNumber(host, axes[index].getString("label"), coordinate?.getDouble("value") ?: 0.0, curve.getJSONObject("numeric"),
                enabled && coordinate != null && !coordinate.getBoolean("read_only"), "curve-$key-$axis", coordinate?.getString("text") ?: "") {
                obj("op" to "curve_number", "layer" to layer, "key" to key, "epoch" to current.getJSONObject("curve").getLong("epoch"), "axis" to axis)
            }
            if (curve.getJSONObject("domain").getString("kind") == "log_hdr") Text(coordinate?.optString("ev") ?: "", Modifier.fillMaxWidth().heightIn(min = 18.dp).testTag("curve-$key-$axis-ev"), color = colors.secondary, textAlign = androidx.compose.ui.text.style.TextAlign.End)
        }
    }
}

@Composable internal fun RendererStatsPanel(host: CanvasHost) {
    var stats by remember { mutableStateOf<JSONObject?>(null) }
    LaunchedEffect(host) { while(isActive) { host.query(obj("type" to "renderer_stats")) { stats = it as? JSONObject }; delay(200) } }
    val colors = LocalPalette.current
    Column(Modifier.fillMaxWidth().testTag("renderer-stats"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        stats?.let { view ->
            view.array("rows").objects().forEachIndexed { index, row ->
                HoverTip(row.getString("description")) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(row.getString("label")); Text(row.getString("value"))
                    }
                }
                if (index + 1 == view.getInt("chart_after_rows")) {
                    Canvas(Modifier.fillMaxWidth().height(46.dp).testTag("renderer-stats-chart")) {
                        val samples = view.array("samples").values().map { (it as Number).toFloat() }
                        val budget = view.number("budget_ms"); val max = maxOf(budget,samples.maxOrNull() ?: 0f)*1.1f
                        drawLine(colors.secondary,Offset(0f,size.height*(1-budget/max)),Offset(size.width,size.height*(1-budget/max)))
                        val path=Path();samples.forEachIndexed { i,v -> val x=i*size.width/119;val y=size.height*(1-v/max);if(i==0)path.moveTo(x,y) else path.lineTo(x,y) }
                        drawPath(path,colors.text,style=Stroke(1.dp.toPx()))
                    }
                }
            }
        }
        Button(onClick = host.strokeRecording::click, enabled = !host.strokeRecording.busy, modifier = Modifier.testTag("stroke-recording")) {
            Text(host.strokeRecording.status?.optString("label") ?: host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("record_tablet"))
        }
    }
}
