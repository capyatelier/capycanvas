package art.capycanvas

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.layer.setOutline
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp

/** Native elevation shadows extend inside their outline and show through glass.
 * Keep the shadow separate from content, with cached corner clips and rectangular
 * exterior clips. No bitmap blur or offscreen compositing buffer is needed. */
@Composable internal fun Modifier.panelShadow(elevation: Dp, shape: Shape): Modifier {
    if (elevation <= 0.dp) return this
    val shadow = rememberGraphicsLayer()
    val windowSize = LocalWindowInfo.current.containerSize
    val cornerCache = remember { arrayOfNulls<ShadowCorner>(4) }
    // Both shapes inherit CornerBasedShape's radius normalization. Use its rounded
    // outline to obtain the exact physical corner radii, including RTL and small sizes.
    val rounded = remember(shape) { (shape as? SquircleShape)?.let {
        RoundedCornerShape(it.topStart, it.topEnd, it.bottomEnd, it.bottomStart)
    } }
    return drawWithCache {
        val outline = shape.createOutline(size, layoutDirection, this)
        // An empty display list needs recording only once; the outline carries its geometry.
        if (shadow.size == IntSize.Zero) shadow.record(size = IntSize(1, 1)) { }
        shadow.setOutline(outline)
        shadow.shadowElevation = elevation.toPx()
        val roundRect = (outline as? Outline.Rounded)?.roundRect
            ?: (rounded?.createOutline(size, layoutDirection, this) as? Outline.Rounded)?.roundRect
        val corners = roundRect?.let { rect ->
            val radii = arrayOf(rect.topLeftCornerRadius, rect.topRightCornerRadius,
                rect.bottomRightCornerRadius, rect.bottomLeftCornerRadius)
            Array(4) { index ->
                val radius = radii[index]
                if (radius.x <= 0f || radius.y <= 0f) null else {
                    val squircle = shape is SquircleShape
                    val corner = cornerCache.firstOrNull { it?.radius == radius && it.squircle == squircle }
                        ?: ShadowCorner(radius, squircle)
                    cornerCache[index] = corner
                    corner
                }
            }
        }
        // RecordingCanvas's reported clip omits overflowing shadows. A window-sized
        // outset preserves native shadow reach while using ordinary scissoring.
        val outset = maxOf(windowSize.width.toFloat(), windowSize.height.toFloat(), size.width, size.height)
        onDrawWithContent {
            clipRect(-outset, -outset, 0f, size.height + outset) { drawLayer(shadow) }
            clipRect(size.width, -outset, size.width + outset, size.height + outset) { drawLayer(shadow) }
            clipRect(0f, -outset, size.width, 0f) { drawLayer(shadow) }
            clipRect(0f, size.height, size.width, size.height + outset) { drawLayer(shadow) }
            if (corners != null) corners.forEachIndexed { index, corner ->
                if (corner != null) drawShadowCorner(shadow, corner, index)
            } else if (outline is Outline.Generic) {
                clipRect(0f, 0f, size.width, size.height) {
                    clipPath(outline.path, ClipOp.Difference) { drawLayer(shadow) }
                }
            }
            drawContent()
        }
    }
}

private class ShadowCorner(val radius: CornerRadius, val squircle: Boolean) {
    val path = Path().apply {
        val rect = Rect(0f, 0f, radius.x, radius.y)
        if (squircle) addSquircle(rect, radius.x, 0f, 0f, 0f)
        else addRoundRect(RoundRect(rect, topLeft = radius))
    }
}

private fun DrawScope.drawShadowCorner(shadow: GraphicsLayer, corner: ShadowCorner, index: Int) {
    val x = if (index == 1 || index == 2) size.width else 0f
    val y = if (index >= 2) size.height else 0f
    val sx = if (index == 1 || index == 2) -1f else 1f
    val sy = if (index >= 2) -1f else 1f
    val canvas = drawContext.canvas
    canvas.save()
    try {
        canvas.translate(x, y)
        canvas.scale(sx, sy)
        canvas.clipRect(0f, 0f, corner.radius.x, corner.radius.y)
        canvas.clipPath(corner.path, ClipOp.Difference)
        // Restore the panel's coordinates while retaining the positioned corner clip.
        canvas.scale(sx, sy)
        canvas.translate(-x, -y)
        drawLayer(shadow)
    } finally { canvas.restore() }
}
