package art.capycanvas

import android.graphics.RectF
import android.graphics.Picture
import android.content.res.AssetManager
import android.util.LruCache
import java.util.WeakHashMap
import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.caverock.androidsvg.RenderOptions
import com.caverock.androidsvg.SVG
import org.json.JSONObject
import kotlin.math.ceil

private data class IconPaint(val name: String, val tint: Color, val fill: Color?)

/** Main-thread vector recordings shared across component lifetimes. Each Image
 * still owns its Painter; no Activity, bitmap size or mutable painter is cached. */
private object IconPictures {
    private val assets = WeakHashMap<AssetManager, LruCache<IconPaint, Picture>>()
    fun get(manager: AssetManager, name: String, tint: Color, fill: Color?): Picture {
        val cache = assets.getOrPut(manager) { LruCache(192) }
        val key = IconPaint(name, tint, fill)
        cache.get(key)?.let { return it }
        fun paint(color: Color): String {
            val argb = color.toArgb()
            return "rgba(${argb shr 16 and 255},${argb shr 8 and 255},${argb and 255},${color.alpha})"
        }
        val picture = manager.open("layer-$name-symbolic.svg").bufferedReader().use { source ->
            val svg = SVG.getFromString(source.readText()
                .replace("currentColor", paint(tint))
                .replace("#33d17a", fill?.let(::paint) ?: "none"))
            if (fill == null) svg.renderToPicture() else svg.renderToPicture(RenderOptions().css(".success{fill:${paint(fill)}}"))
        }
        cache.put(key, picture)
        return picture
    }
}

/** Paint the canonical vectors at the actual device size. Tint only foreground
 * paint: fixed swatch colors and their drawing order are part of the artwork. */
@Composable internal fun SharedIcon(name: String, description: String?, modifier: Modifier = Modifier,
    tint: Color = LocalPalette.current.text, fill: Color? = null) {
    val context = LocalContext.current
    val painter = remember(context, name, tint, fill) {
        SharedIconPainter(IconPictures.get(context.assets, name, tint, fill))
    }
    Image(painter, description, modifier.size(16.dp))
}

@Composable internal fun PaintPairIcon(view: JSONObject, description: String?, modifier: Modifier = Modifier) {
    val ink = LocalPalette.current.text
    val fields = remember {
        listOf(Offset(6.75f, 6.75f) to 6f, Offset(11f, 11f) to 4.25f).map { (center, radius) ->
            Path().apply { addOval(Rect(center - Offset(radius, radius), Size(radius * 2, radius * 2))) }
        }
    }
    val front = view.getString("front_swatch")
    val swatches = view.array("swatches").objects().sortedBy { it.getString("slot") == front }.map { swatch ->
        (if (swatch.getString("slot") == "foreground") 0 else 1) to swatch.array("checker").values().map { displayColor(it as org.json.JSONArray) }
    }
    val cell = view.number("checker_cell")
    Canvas(modifier.size(16.dp).semantics { description?.let { contentDescription = it } }) {
        scale(size.width / 16f, size.height / 16f, Offset.Zero) {
            for ((index, checker) in swatches) {
                val foreground = index == 0
                val center = if (foreground) Offset(6.75f, 6.75f) else Offset(11f, 11f)
                val radius = if (foreground) 6f else 4.25f
                val origin = center - Offset(radius, radius)
                clipPath(fields[index]) {
                    drawRect(checker[0], origin, Size(radius * 2, radius * 2))
                    for (y in 0 until ceil(radius * 2 / cell).toInt()) for (x in 0 until ceil(radius * 2 / cell).toInt())
                        if ((x + y) % 2 == 1) drawRect(checker[1], origin + Offset(x * cell, y * cell), Size(cell, cell))
                }
                drawCircle(ink, radius, center, style = Stroke(1f))
            }
        }
    }
}

private class SharedIconPainter(private val picture: Picture) : Painter() {
    override val intrinsicSize = Size(16f, 16f)
    override fun DrawScope.onDraw() {
        drawIntoCanvas { canvas ->
            canvas.save()
            try {
                canvas.nativeCanvas.drawPicture(picture, RectF(0f, 0f, size.width, size.height))
            } finally {
                canvas.restore()
            }
        }
    }
}
