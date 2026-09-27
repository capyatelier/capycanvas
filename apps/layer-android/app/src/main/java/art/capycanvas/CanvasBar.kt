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
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.ceil
import kotlin.math.roundToInt

private const val BarGap = 4f
private const val BarPadding = 6f
private const val BarItemHeight = 32f
private const val BarLabelPadding = 6f
private val BarItemStyle = obj("sliders" to false, "text" to true)

private val BarElevation = 6.dp
private val BarShadowMargin = BarElevation * PanelShadowReach

private class BarFrame {
    var bounds by mutableStateOf<JSONObject?>(null)
}

@Composable internal fun CanvasBar(host: CanvasHost, dock: DockInteraction, layout: JSONObject) {
    val dragging = dock.dragging
    LaunchedEffect(dragging) { host.holdCanvasBar(CanvasHost.CanvasBarWorkspaceDrag, dragging) }
    val density = LocalDensity.current.density
    val frame = remember { BarFrame() }
    val placement = remember(frame, density) {
        val margin = (BarShadowMargin.value * density).roundToInt()
        fun px(bounds: JSONObject, key: String) = (bounds.number(key) * density).roundToInt()
        Modifier.zIndex(198f).offset {
            frame.bounds?.let { IntOffset(px(it, "x") - margin, px(it, "y") - margin) } ?: IntOffset.Zero
        }.layout { measurable, _ ->
            val bounds = frame.bounds
            val width = bounds?.let { px(it, "width") + 2 * margin } ?: 0
            val height = bounds?.let { px(it, "height") + 2 * margin } ?: 0
            val content = measurable.measure(Constraints.fixed(width, height))
            layout(width, height) { content.place(0, 0) }
        }.clipToBounds()
    }
    Box(placement) {
        val view = host.canvasBar ?: return@Box
        key(view.getJSONObject("context").toString()) { PlacedCanvasBar(host, dock, view, layout, frame, host.canvasBarVisible) }
    }
}

@Composable private fun PlacedCanvasBar(host: CanvasHost, dock: DockInteraction, view: JSONObject, layout: JSONObject, frame: BarFrame,
    visible: Boolean) {
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val context = view.getJSONObject("context")
    val items = view.array("items").objects()
    val completion = view.array("completion").objects()
    val label = if (view.isNull("label")) null else view.getString("label")
    val measurer = rememberTextMeasurer()
    val textStyle = LocalTextStyle.current
    fun textWidth(text: String) = measurer.measure(text, textStyle).size.width / density
    fun width(item: JSONObject) = toolOptionSize(item.getJSONObject("option"), false, 0f, BarItemHeight, BarItemHeight,
        BarItemStyle, ::textWidth, item.getString("label"))[0]
    val itemWidths = remember(view.opt("items"), textStyle, density) { items.map(::width) }
    val completionWidths = remember(view.opt("completion"), textStyle, density) { completion.map(::width) }
    val labelWidth = remember(label, textStyle, density) { label?.let { ceil(textWidth(it)) + 2 * BarLabelPadding } ?: 0f }
    val measure = obj("context" to context, "label" to labelWidth, "items" to JSONArray(itemWidths),
        "completion" to JSONArray(completionWidths), "more" to BarItemHeight, "height" to BarItemHeight + 2 * BarPadding,
        "gap" to BarGap, "padding" to BarPadding).toString()
    var placed by remember { mutableStateOf<JSONObject?>(null) }
    var revealed by remember { mutableStateOf(false) }
    LaunchedEffect(measure, view.opt("anchor")?.toString(), layout) {
        val next = host.awaitQuery(obj("type" to "canvas_bar_layout", "measure" to JSONObject(measure)))
        if (next?.toString() != placed?.toString()) {
            placed = next
            if (revealed) next?.getJSONObject("bounds")?.let { frame.bounds = it }
        }
    }
    val glass = remember { Any() }
    DisposableEffect(host, glass) { onDispose { host.glassBox(glass, null) } }
    val placement = placed ?: return
    val shape = ControlShape
    LaunchedEffect(visible) {
        if (!visible) {
            revealed = false
            host.glassBox(glass, null)
            dock.canvasBar = null; dock.refresh()
            return@LaunchedEffect
        }
        val bounds = placed?.getJSONObject("bounds") ?: placement.getJSONObject("bounds")
        fun px(key: String) = (bounds.number(key) * density).roundToInt().toFloat()
        val radius = ControlRadius.value * density
        host.glassBox(glass, floatArrayOf(px("x"), px("y"), px("width"), px("height"), radius, radius, radius, radius))
        host.glassPresented()
        frame.bounds = placed?.getJSONObject("bounds") ?: bounds
        revealed = true
    }
    DisposableEffect(dock) { onDispose { dock.canvasBar = null; dock.canvasBarSlot = null; dock.refresh() } }
    val shown = placement.optInt("items").coerceIn(0, items.size)
    fun edit(action: JSONObject) = host.dispatch(obj("type" to "canvas_bar_edit", "context" to context, "action" to action))
    fun choiceMenu(id: String, load: (JSONObject?) -> Unit) = host.query(obj("type" to "canvas_bar_choice_menu", "context" to context, "id" to id)) {
        load(it as? JSONObject)
    }
    Layout(content = {
        Box(Modifier.padding(BarShadowMargin).fillMaxSize().testTag("canvas-action-bar")
            .then(if (revealed) Modifier.chromeRegion(dock) else Modifier).onGloballyPositioned {
                val bounds = it.boundsInRoot().translate(-dock.origin)
                if (dock.canvasBar != bounds) { dock.canvasBar = bounds; dock.canvasBarSlot = bounds; dock.refresh() }
            }
            .panelSurface(BarElevation, shape).glass(shape, key = glass)
            .semantics { contentDescription = "Canvas actions" }) {
            CompositionLocalProvider(LocalPalette provides colors.onGlass) {
                Surface(Modifier.fillMaxSize(), color = colors.onGlass.panelFill, contentColor = colors.text) {
                    Row(Modifier.padding(BarPadding.dp).clipToBounds(), horizontalArrangement = Arrangement.spacedBy(BarGap.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        label?.let {
                            Text(it, Modifier.width(labelWidth.dp).padding(horizontal = BarLabelPadding.dp).testTag("canvas-bar-label"),
                                color = colors.secondary, maxLines = 1, softWrap = false)
                        }
                        items.take(shown).forEachIndexed { index, item -> BarField(item, itemWidths[index], false, ::edit, ::choiceMenu) }
                        CanvasBarMore(host, context, shown)
                        completion.forEachIndexed { index, item -> BarField(item, completionWidths[index], true, ::edit, ::choiceMenu) }
                    }
                }
            }
        }
    }, modifier = if (revealed) Modifier else Modifier.clearAndSetSemantics {}) { bar, constraints ->
        val placeable = bar.single().measure(constraints)
        layout(constraints.maxWidth, constraints.maxHeight) { if (revealed) placeable.place(0, 0) }
    }
}

@Composable private fun BarField(item: JSONObject, width: Float, completion: Boolean, edit: (JSONObject) -> Unit,
    choiceMenu: (String, (JSONObject?) -> Unit) -> Unit) {
    val option = item.getJSONObject("option")
    val command = option.optJSONObject("Action")?.getJSONObject("state")?.getString("id")
    Box(Modifier.width(width.dp).height(BarItemHeight.dp), contentAlignment = Alignment.Center) {
        ToolOptionField(option, width, false, "medium", false, BarItemStyle, BarItemHeight, 16, edit,
            caption = item.getString("label"), prefix = "canvas-bar",
            accent = completion && command in listOf("apply_transform", "complete_selection"), choiceMenu = choiceMenu)
    }
}

@Composable private fun CanvasBarMore(host: CanvasHost, context: JSONObject, shown: Int) {
    val button = remember { WindowlessMenuButton() }
    HoverTip("More") {
        Box(Modifier.size(BarItemHeight.dp).testTag("canvas-bar-more").clip(ControlShape).focusProperties { canFocus = false }
            .opensWindowlessMenu(button, "More") { load ->
                host.query(obj("type" to "canvas_bar_menu", "context" to context, "shown" to shown)) { load(it as? JSONObject) }
            }, contentAlignment = Alignment.Center) {
            SharedIcon("more", "More", Modifier.size(16.dp))
            WindowlessMenuHost(host, button)
        }
    }
}
