package art.capycanvas

import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.layer.CompositingStrategy as LayerCompositingStrategy
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.layer.setOutline
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.layout
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.offset
import kotlin.math.ceil

internal const val PanelShadowReach = 3f

internal object PanelLayers {
    var cached by mutableStateOf(true)
}

internal class PanelOpening {
    var coordinates: LayoutCoordinates? = null
    var bounds by mutableStateOf<Rect?>(null)
}

internal val LocalPanelOpening = staticCompositionLocalOf<PanelOpening?> { null }

@Composable internal fun Modifier.panelShadow(elevation: Dp, shape: Shape, cut: Rect? = null, isolated: Boolean = false): Modifier {
    if (elevation <= 0.dp) return this
    val caster = rememberGraphicsLayer()
    val texture = rememberGraphicsLayer()
    return drawWithCache {
        val outline = shape.createOutline(size, layoutDirection, this)
        if (caster.size == IntSize.Zero) caster.record(size = IntSize(1, 1)) { }
        caster.setOutline(outline)
        caster.shadowElevation = elevation.toPx()
        val interior = Path().apply { addOutline(outline) }
        val reach = PanelShadowReach * elevation.toPx()
        val width = size.width
        val height = size.height
        val cast: DrawScope.() -> Unit = {
            clipRect(-reach, -reach, width + reach, height + reach) {
                if (cut == null) drawLayer(caster)
                else clipRect(cut.left, cut.top, cut.right, cut.bottom, ClipOp.Difference) { drawLayer(caster) }
            }
            drawPath(interior, Color.Transparent, blendMode = BlendMode.Clear)
        }
        val margin = ceil(reach)
        if (!isolated) {
            texture.compositingStrategy = LayerCompositingStrategy.Offscreen
            texture.record(size = IntSize(ceil(width + 2 * margin).toInt(), ceil(height + 2 * margin).toInt())) {
                translate(margin, margin, cast)
            }
        }
        onDrawWithContent {
            if (isolated) cast() else translate(-margin, -margin) { drawLayer(texture) }
            drawContent()
        }
    }
}

@Composable internal fun Modifier.panelSurface(elevation: Dp, shape: Shape, cut: Rect? = null, opening: PanelOpening? = null): Modifier {
    val reach = elevation * PanelShadowReach
    val margin = with(LocalDensity.current) { reach.roundToPx() }
    val cached = PanelLayers.cached
    return (if (opening == null) this else onPlaced { opening.coordinates = it }.drawWithContent {
        drawContent()
        opening.bounds?.let { drawRect(Color.Transparent, it.topLeft, it.size, blendMode = BlendMode.Clear) }
    }).layout { measurable, constraints ->
        val placeable = measurable.measure(constraints.offset(2 * margin, 2 * margin))
        layout(placeable.width - 2 * margin, placeable.height - 2 * margin) { placeable.place(-margin, -margin) }
    }.graphicsLayer { compositingStrategy = if (cached) CompositingStrategy.Offscreen else CompositingStrategy.Auto }
        .padding(reach).panelShadow(elevation, shape, cut, isolated = cached).clip(shape)
}
