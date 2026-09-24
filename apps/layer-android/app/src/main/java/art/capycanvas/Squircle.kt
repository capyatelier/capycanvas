package art.capycanvas

import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.toRect
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

private const val SquircleSegments = 24

internal val SurfaceRadius = 18.dp
internal val ControlRadius = 12.dp
internal val SurfaceShape = SquircleShape(SurfaceRadius)
internal val ControlShape = SquircleShape(ControlRadius)
internal val TileShape = SquircleShape(50)

/** Superellipse corners matching CSS `corner-shape: squircle`; radii clamp like rounded corners. */
internal class SquircleShape(topStart: CornerSize, topEnd: CornerSize, bottomEnd: CornerSize, bottomStart: CornerSize) :
    CornerBasedShape(topStart, topEnd, bottomEnd, bottomStart) {
    override fun copy(topStart: CornerSize, topEnd: CornerSize, bottomEnd: CornerSize, bottomStart: CornerSize) =
        SquircleShape(topStart, topEnd, bottomEnd, bottomStart)

    override fun createOutline(size: Size, topStart: Float, topEnd: Float, bottomEnd: Float, bottomStart: Float,
        layoutDirection: LayoutDirection): Outline {
        if (topStart + topEnd + bottomEnd + bottomStart == 0f) return Outline.Rectangle(size.toRect())
        val ltr = layoutDirection == LayoutDirection.Ltr
        return Outline.Generic(Path().apply {
            addSquircle(size.toRect(), if (ltr) topStart else topEnd, if (ltr) topEnd else topStart,
                if (ltr) bottomEnd else bottomStart, if (ltr) bottomStart else bottomEnd)
        })
    }

    override fun equals(other: Any?) = other is SquircleShape && topStart == other.topStart &&
        topEnd == other.topEnd && bottomEnd == other.bottomEnd && bottomStart == other.bottomStart

    override fun hashCode() = listOf(topStart, topEnd, bottomEnd, bottomStart).hashCode()
}

internal fun SquircleShape(radius: Dp) = SquircleShape(radius, radius, radius, radius)

internal fun SquircleShape(topStart: Dp = 0.dp, topEnd: Dp = 0.dp, bottomEnd: Dp = 0.dp, bottomStart: Dp = 0.dp) =
    SquircleShape(CornerSize(topStart), CornerSize(topEnd), CornerSize(bottomEnd), CornerSize(bottomStart))

internal fun SquircleShape(percent: Int) = CornerSize(percent).let { SquircleShape(it, it, it, it) }

internal fun squircleCorner(center: Offset, start: Offset, end: Offset) = (1..SquircleSegments).map { step ->
    val angle = step * PI / 2 / SquircleSegments
    center + start * sqrt(cos(angle)).toFloat() + end * sqrt(sin(angle)).toFloat()
}

internal fun Path.squircleTo(center: Offset, start: Offset, end: Offset) =
    squircleCorner(center, start, end).forEach { lineTo(it.x, it.y) }

internal fun Path.addSquircle(rect: Rect, topLeft: Float, topRight: Float, bottomRight: Float, bottomLeft: Float) {
    moveTo(rect.left + topLeft, rect.top)
    lineTo(rect.right - topRight, rect.top)
    squircleTo(Offset(rect.right - topRight, rect.top + topRight), Offset(0f, -topRight), Offset(topRight, 0f))
    lineTo(rect.right, rect.bottom - bottomRight)
    squircleTo(Offset(rect.right - bottomRight, rect.bottom - bottomRight), Offset(bottomRight, 0f), Offset(0f, bottomRight))
    lineTo(rect.left + bottomLeft, rect.bottom)
    squircleTo(Offset(rect.left + bottomLeft, rect.bottom - bottomLeft), Offset(0f, bottomLeft), Offset(-bottomLeft, 0f))
    lineTo(rect.left, rect.top + topLeft)
    squircleTo(Offset(rect.left + topLeft, rect.top + topLeft), Offset(-topLeft, 0f), Offset(0f, -topLeft))
    close()
}
