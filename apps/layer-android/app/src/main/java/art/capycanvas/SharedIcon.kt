package art.capycanvas

import android.graphics.RectF
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.caverock.androidsvg.SVG

/** Paint the canonical vectors at the actual device size. Tint only foreground
 * paint: fixed swatch colors and their drawing order are part of the artwork. */
@Composable internal fun SharedIcon(name: String, description: String?, modifier: Modifier = Modifier,
    tint: Color = LocalPalette.current.text, fill: Color? = null) {
    val context = LocalContext.current
    val painter = remember(context, name, tint, fill) {
        context.assets.open("layer-$name-symbolic.svg").bufferedReader().use { source ->
            fun paint(color: Color): String {
                val argb = color.toArgb()
                return "rgba(${argb shr 16 and 255},${argb shr 8 and 255},${argb and 255},${color.alpha})"
            }
            val svg = SVG.getFromString(source.readText()
                .replace("currentColor", paint(tint))
                .replace("#33d17a", fill?.let(::paint) ?: "none"))
            // SVG width/height attributes are CSS pixels; the host viewport is
            // device pixels. Keep the viewBox and fit it to the painter bounds.
            svg.setDocumentWidth("100%")
            svg.setDocumentHeight("100%")
            SharedIconPainter(svg)
        }
    }
    Image(painter, description, modifier.size(16.dp))
}

private class SharedIconPainter(private val svg: SVG) : Painter() {
    override val intrinsicSize = Size(16f, 16f)
    override fun DrawScope.onDraw() {
        drawIntoCanvas { canvas ->
            canvas.save()
            try {
                svg.renderToCanvas(canvas.nativeCanvas, RectF(0f, 0f, size.width, size.height))
            } finally {
                canvas.restore()
            }
        }
    }
}
