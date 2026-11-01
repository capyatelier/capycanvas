package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.ceil
import kotlin.math.roundToInt

internal const val LabelStart = 12f
internal const val LabelEnd = 8f
private val BarItemStyle = obj("sliders" to false, "text" to true)

internal val BarElevation = 6.dp
private val BarShadowMargin = BarElevation * PanelShadowReach

private class BarFrame {
    var bounds by mutableStateOf<Rect?>(null)
    var revealedContext by mutableStateOf<String?>(null)
}

@Composable internal fun CanvasBar(host: CanvasHost, dock: DockInteraction, viewportSize: IntSize) {
    val density = LocalDensity.current.density
    val frame = remember { BarFrame() }
    val placement = remember(frame, density) {
        val margin = (BarShadowMargin.value * density).roundToInt()
        fun px(value: Float) = (value * density).roundToInt()
        fun bounds() = host.canvasBarGeometry?.takeIf { host.canvasBarVisible && frame.revealedContext == it.context }?.bounds ?: frame.bounds
        Modifier.zIndex(198f).offset {
            bounds()?.let { IntOffset(px(it.left) - margin, px(it.top) - margin) } ?: IntOffset.Zero
        }.layout { measurable, _ ->
            val bounds = bounds()
            val width = bounds?.let { px(it.width) + 2 * margin } ?: 0
            val height = bounds?.let { px(it.height) + 2 * margin } ?: 0
            val content = measurable.measure(Constraints.fixed(width, height))
            layout(width, height) { content.place(0, 0) }
        }.clipToBounds()
    }
    Box(placement) {
        val view = host.canvasBar ?: return@Box
        key(view.getJSONObject("context").toString()) { PlacedCanvasBar(host, dock, view, frame, host.canvasBarVisible, viewportSize) }
    }
}

@Composable private fun PlacedCanvasBar(host: CanvasHost, dock: DockInteraction, view: JSONObject, frame: BarFrame,
    visible: Boolean, viewportSize: IntSize) {
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val context = view.getJSONObject("context")
    val items = view.array("items").objects()
    val completion = view.array("completion").objects()
    val label = if (view.isNull("label")) null else view.getString("label")
    val measurer = rememberTextMeasurer()
    val textStyle = LocalTextStyle.current
    fun textWidth(text: String) = measurer.measure(text, textStyle).size.width / density
    val tiles = remember(host.languageTag) { toolbarUi(host.languageTag, obj("type" to "style", "style" to "small")) }
    val tile = tiles.array("size").getDouble(1).toFloat()
    val gap = pixelAligned(tiles.number("gap"), density)
    val control = panelMetrics().height.value
    fun fieldWidth(item: JSONObject) = when {
        item.isNull("menu") -> toolOptionSize(item.getJSONObject("option"), false, 0f, tile, tile, BarItemStyle, ::textWidth, control,
            item.getString("label"), language = host.languageTag, density = density)[0]
        item.optBoolean("icon_only") -> tile
        else -> menuButtonWidth(item.getString("label"), ::textWidth, density) + 2 * pixelAligned(FieldClear, density)
    }
    fun width(field: Float, item: JSONObject) = field + if (item.optBoolean("separated")) DividerSlot.value + gap else 0f
    val itemFields = remember(view.opt("items"), textStyle, density, tile, control) { items.map(::fieldWidth) }
    val completionFields = remember(view.opt("completion"), textStyle, density, tile, control) { completion.map(::fieldWidth) }
    val labelWidth = remember(label, textStyle, density) { label?.let { ceil(textWidth(it)) + LabelStart + LabelEnd } ?: 0f }
    val measure = obj("context" to context, "label" to labelWidth, "items" to JSONArray(items.indices.map { width(itemFields[it], items[it]) }),
        "completion" to JSONArray(completion.indices.map { width(completionFields[it], completion[it]) }), "more" to tile, "height" to tile,
        "gap" to gap, "padding" to 0f).toString()
    val revealed = frame.revealedContext == context.toString()
    LaunchedEffect(measure) {
        host.dispatch(obj("type" to "measure_canvas_bar", "measure" to JSONObject(measure)))
    }
    val glass = remember { Any() }
    val placement = host.canvasBarGeometry?.takeIf { it.context == context.toString() }
    val shape = SurfaceShape
    LaunchedEffect(visible) {
        val bounds = placement?.bounds
        if (!visible || bounds == null) {
            frame.revealedContext = null
            host.glassBox(glass, null)
            return@LaunchedEffect
        }
        fun px(value: Float) = (value * density).roundToInt().toFloat()
        val radius = SurfaceRadius.value * density
        host.glassBox(glass, floatArrayOf(px(bounds.left), px(bounds.top), px(bounds.width), px(bounds.height), radius, radius, radius, radius))
        host.glassPresented()
        frame.bounds = bounds
        frame.revealedContext = context.toString()
    }
    val frameBounds = if (visible && revealed) placement?.bounds ?: frame.bounds else frame.bounds
    SideEffect {
        if (visible && revealed) placement?.bounds?.let { frame.bounds = it }
        frameBounds?.let { bounds ->
            fun px(value: Float) = (value * density).roundToInt().toFloat()
            val left = px(bounds.left); val top = px(bounds.top)
            val slot = Rect(left, top, left + px(bounds.width), top + px(bounds.height))
                .intersect(Rect(0f, 0f, viewportSize.width.toFloat(), viewportSize.height.toFloat())).let { if (it.isEmpty) Rect.Zero else it }
            if (dock.canvasBarSlot != slot) dock.canvasBarSlot = slot
        }
    }
    DisposableEffect(dock) { onDispose { dock.canvasBarSlot = null } }
    val shown = (placement?.items ?: 0).coerceIn(0, items.size)
    fun edit(action: JSONObject) = host.dispatch(obj("type" to "canvas_bar_edit", "context" to context, "action" to action))
    fun choiceMenu(id: String, load: (JSONObject?) -> Unit) = host.query(obj("type" to "canvas_bar_choice_menu", "context" to context, "id" to id)) {
        load(it as? JSONObject)
    }
    suspend fun choiceCopy(id: String) = host.menuCopy(obj("type" to "canvas_bar_choice_menu", "context" to context, "id" to id))
    Layout(content = {
        Box(Modifier.padding(BarShadowMargin).fillMaxSize().testTag("canvas-action-bar")
            .then(if (revealed) Modifier.chromeRegion(dock) else Modifier)
            .panelSurface(BarElevation, shape).glass(shape, key = glass)
            .semantics { contentDescription = host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("canvas_actions") }) {
            CompositionLocalProvider(LocalPalette provides colors.onGlass) {
                Surface(Modifier.fillMaxSize(), color = colors.onGlass.panelFill, contentColor = colors.text) {
                    Row(Modifier.clipToBounds(), horizontalArrangement = Arrangement.spacedBy(gap.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        label?.let {
                            Text(it, Modifier.width(labelWidth.dp).padding(start = LabelStart.dp, end = LabelEnd.dp).testTag("canvas-bar-label"),
                                color = colors.text.copy(alpha = .8f), maxLines = 1, softWrap = false)
                        }
                        var dividers = 0
                        @Composable fun field(item: JSONObject, width: Float) {
                            if (item.optBoolean("separated")) ToolbarDivider(Modifier.width(DividerSlot).height(tile.dp).testTag("canvas-bar-divider-${dividers++}"), horizontal = false)
                            BarField(item, width, tile, ::edit, ::choiceMenu, ::choiceCopy)
                        }
                        items.take(shown).forEachIndexed { index, item -> field(item, itemFields[index]) }
                        CanvasBarMore(host, context, shown, tile)
                        completion.forEachIndexed { index, item -> field(item, completionFields[index]) }
                    }
                }
            }
        }
    }, modifier = if (revealed) Modifier else Modifier.clearAndSetSemantics {}) { bar, constraints ->
        val placeable = bar.single().measure(constraints)
        layout(constraints.maxWidth, constraints.maxHeight) { if (revealed) placeable.place(0, 0) }
    }
}

@Composable private fun BarField(item: JSONObject, width: Float, tile: Float, edit: (JSONObject) -> Unit,
    choiceMenu: (String, (JSONObject?) -> Unit) -> Unit, choiceCopy: suspend (String) -> JSONObject?) {
    val option = item.getJSONObject("option")
    Box(Modifier.width(width.dp).height(tile.dp), contentAlignment = Alignment.Center) {
        if (!item.isNull("menu")) item.getString("menu").let { menu ->
            val iconOnly = item.optBoolean("icon_only")
            Box(if (iconOnly) Modifier.fillMaxSize() else Modifier.padding(horizontal = FieldClear.dp).fillMaxWidth().height(panelMetrics().height)) {
                ToolOptionMenu(menu, item.getString("icon"), item.getString("label"), option.optJSONObject("Action")?.getJSONObject("state"),
                    "canvas-bar", iconOnly, copy = { choiceCopy(menu) }) { choiceMenu(menu, it) }
            }
        } else ToolOptionField(option, width, false, "small", false, BarItemStyle, tile, 16, edit,
            caption = item.getString("label"), prefix = "canvas-bar", accent = item.optBoolean("accent"), choiceMenu = choiceMenu, choiceCopy = choiceCopy)
    }
}

@Composable private fun CanvasBarMore(host: CanvasHost, context: JSONObject, shown: Int, tile: Float) {
    val button = remember { WindowlessMenuButton() }
    SideEffect { button.copy = { host.menuCopy(obj("type" to "canvas_bar_menu", "context" to context, "shown" to shown)) } }
    HoverTip(host.bootstrap!!.getJSONObject("common").getString("more")) {
        Box(Modifier.size(tile.dp).testTag("canvas-bar-more").clip(TileShape).focusProperties { canFocus = false }
            .opensWindowlessMenu(button, host.bootstrap!!.getJSONObject("common").getString("more")) { load ->
                host.query(obj("type" to "canvas_bar_menu", "context" to context, "shown" to shown)) { load(it as? JSONObject) }
            }, contentAlignment = Alignment.Center) {
            SharedIcon("more", host.bootstrap!!.getJSONObject("common").getString("more"), Modifier.size(16.dp))
            WindowlessMenuHost(host, button)
        }
    }
}
