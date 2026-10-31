package art.capycanvas

import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.*
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.GlobalPositionAwareModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.node.requireLayoutCoordinates
import androidx.compose.ui.platform.InspectorInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject

private val TabFoot = 6.dp

private fun Path.tabFace(w: Float, h: Float, r: Float, left: Float, right: Float) {
    moveTo(r, 0f); lineTo(w-r, 0f); squircleTo(Offset(w-r, r), Offset(0f, -r), Offset(r, 0f))
    lineTo(w, h-kotlin.math.abs(right)); squircleTo(Offset(w-right, h-kotlin.math.abs(right)), Offset(right, 0f), Offset(0f, kotlin.math.abs(right)))
    lineTo(left, h); squircleTo(Offset(left, h-kotlin.math.abs(left)), Offset(0f, kotlin.math.abs(left)), Offset(-left, 0f))
    lineTo(0f, r); squircleTo(Offset(r, r), Offset(-r, 0f), Offset(0f, -r)); close()
}

internal class TabStrip {
    var area: StripNode? = null
    var tab: HoleNode? = null
    fun moved() { area?.invalidateDraw() }
}

@Composable internal fun Modifier.tabStrip(strip: TabStrip, color: Color, panel: Color) = this then StripElement(strip, color, panel, LocalVectorPanel.current)

private data class StripElement(val strip: TabStrip, val color: Color, val panel: Color, val replace: Boolean) : ModifierNodeElement<StripNode>() {
    override fun create() = StripNode(strip, color, panel, replace)
    override fun update(node: StripNode) = node.update(strip, color, panel, replace)
    override fun InspectorInfo.inspectableProperties() { name = "tabStrip" }
}

internal class StripNode(private var strip: TabStrip, private var color: Color, private var panel: Color, private var replace: Boolean) : Modifier.Node(), DrawModifierNode {
    private val face = Path(); private val hole = Path()
    private val paint = Paint()
    private var faceSize = IntSize.Zero
    override fun onAttach() { strip.area = this }
    override fun onDetach() { if (strip.area === this) strip.area = null }
    fun update(strip: TabStrip, color: Color, panel: Color, replace: Boolean) {
        if (strip !== this.strip) { onDetach(); this.strip = strip; onAttach() }
        this.color = color
        this.panel = panel
        this.replace = replace
        invalidateDraw()
    }
    override fun ContentDrawScope.draw() {
        if (!replace) drawContext.canvas.saveLayer(Rect(Offset.Zero, size), paint)
        drawRect(color, blendMode = if (replace) BlendMode.Src else BlendMode.SrcOver)
        val tab = strip.tab
        if (tab != null) {
            val placed = tab.requireLayoutCoordinates()
            if (placed.size != faceSize) {
                faceSize = placed.size
                face.reset(); face.tabFace(faceSize.width.toFloat(), faceSize.height.toFloat(), SurfaceRadius.toPx(), -TabFoot.toPx(), -TabFoot.toPx())
            }
            hole.reset(); hole.addPath(face, requireLayoutCoordinates().localPositionOf(placed, Offset.Zero) + Offset(tab.slide(), 0f))
            drawPath(hole, panel, blendMode = BlendMode.Src)
        }
        if (!replace) drawContext.canvas.restore()
        drawContent()
    }
}

private fun Modifier.tabHole(strip: TabStrip, slide: () -> Float) = this then HoleElement(strip, slide)

private data class HoleElement(val strip: TabStrip, val slide: () -> Float) : ModifierNodeElement<HoleNode>() {
    override fun create() = HoleNode(strip, slide)
    override fun update(node: HoleNode) = node.update(strip, slide)
    override fun InspectorInfo.inspectableProperties() { name = "tabHole" }
}

internal class HoleNode(private var strip: TabStrip, var slide: () -> Float) : Modifier.Node(), GlobalPositionAwareModifierNode {
    override fun onAttach() { strip.tab = this; strip.moved() }
    override fun onDetach() { if (strip.tab === this) { strip.tab = null; strip.moved() } }
    override fun onGloballyPositioned(coordinates: LayoutCoordinates) = strip.moved()
    fun update(strip: TabStrip, slide: () -> Float) {
        if (strip !== this.strip) { onDetach(); this.strip = strip; onAttach() }
        this.slide = slide
        strip.moved()
    }
}

/** Docked, floating and drawer tabs share geometry, colors and focus feedback. */
@Composable internal fun WorkspaceTab(host: CanvasHost, dock: DockInteraction, panel: JSONObject,
    group: Int, index: Int, selected: Boolean, strip: TabStrip, modifier: Modifier, presentation: JSONObject, enabled: Boolean = true) {
    val colors = LocalPalette.current
    val id = panel.getString("id")
    val showIcon = presentation.getBoolean("show_icon")
    val showName = presentation.getBoolean("show_name")
    val measurer = rememberTextMeasurer()
    val style = LocalTextStyle.current.copy(fontWeight = FontWeight.Bold)
    val widths = listOf(16f + 22f +
        measurer.measure(AnnotatedString(panel.getString("title")), style).size.width / dock.density, 36f)
    val origin = dock.measurementOrigin(id)
    SideEffect { if (enabled) dock.measure(id, origin, tabWidths = widths) }
    val dragged = dock.isDraggedTab(id)
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    val slide = tabSlide(host, group, id, index, dock.density)
    Row(modifier.height(36.dp).then(if (!showName) Modifier.width(36.dp) else Modifier)
        .zIndex(if (dragged) 2f else if (selected) 1f else 0f)
        .then(if (selected) Modifier.tabHole(strip, slide) else Modifier)
        .graphicsLayer { translationX = slide() }
        .drawWithCache {
            val hover = if (!selected && hovered) Path().apply {
                val bottom = presentation.getJSONArray("hover_bottom")
                tabFace(size.width, size.height, SurfaceRadius.toPx(), bottom.getInt(0).dp.toPx(), bottom.getInt(1).dp.toPx())
            } else null
            onDrawBehind {
                if (dragged && !selected) drawRect(colors.tabs)
                hover?.let { drawPath(it, colors.text.copy(alpha = .08f)) }
            }
        }
        .combinedClickable(enabled = enabled, interactionSource = interactions, indication = LocalIndication.current,
            onClick = { host.dispatch(obj("type" to "select_panel_tab", "group" to group, "panel" to id)) },
            onLongClick = { dock.holdContext(obj("kind" to "panel", "panel" to id)) })
        .padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally)) {
        if (showIcon) LiveIcon(host, panel.getString("icon"), if (showName) null else panel.getString("title"),
            Modifier.testTag("tab-icon-$id"))
        if (showName) Text(panel.getString("title"), Modifier.testTag("tab-name-$id"), color = colors.text, fontWeight = FontWeight.Bold)
    }
}
