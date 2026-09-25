package art.capycanvas

import android.graphics.RectF
import android.graphics.Picture
import android.content.res.AssetManager
import android.util.LruCache
import java.util.WeakHashMap
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
import com.caverock.androidsvg.RenderOptions
import com.caverock.androidsvg.SVG
import org.json.JSONArray
import org.json.JSONObject

private data class IconPaint(val name: String, val tint: Color, val fill: Color?, val secondary: Color?)

/** Main-thread vector recordings shared across component lifetimes. Each Image
 * still owns its Painter; no Activity, bitmap size or mutable painter is cached. */
private object IconPictures {
    private val assets = WeakHashMap<AssetManager, LruCache<IconPaint, Picture>>()
    fun get(manager: AssetManager, name: String, tint: Color, fill: Color?, secondary: Color?): Picture {
        val cache = assets.getOrPut(manager) { LruCache(192) }
        val key = IconPaint(name, tint, fill, secondary)
        cache.get(key)?.let { return it }
        fun paint(color: Color): String {
            val argb = color.toArgb()
            return "rgba(${argb shr 16 and 255},${argb shr 8 and 255},${argb and 255},${color.alpha})"
        }
        val picture = manager.open("layer-$name-symbolic.svg").bufferedReader().use { source ->
            val svg = SVG.getFromString(source.readText()
                .replace("currentColor", paint(tint))
                .replace("#33d17a", fill?.let(::paint) ?: "none"))
            val paints = listOfNotNull(fill?.let { ".success{fill:${paint(it)}}" }, secondary?.let { ".warning{fill:${paint(it)}}" })
            if (paints.isEmpty()) svg.renderToPicture() else svg.renderToPicture(RenderOptions().css(paints.joinToString("")))
        }
        cache.put(key, picture)
        return picture
    }
}

/** Paint the canonical vectors at the actual device size. Tint only foreground
 * paint: fixed swatch colors and their drawing order are part of the artwork. */
@Composable internal fun SharedIcon(name: String, description: String?, modifier: Modifier = Modifier,
    tint: Color = LocalPalette.current.text, fill: Color? = null, secondary: Color? = null) {
    val context = LocalContext.current
    val painter = remember(context, name, tint, fill, secondary) {
        SharedIconPainter(IconPictures.get(context.assets, name, tint, fill, secondary))
    }
    Image(painter, description, modifier.size(16.dp))
}

@Composable internal fun PaintPairIcon(state: JSONObject?, description: String?, modifier: Modifier = Modifier) {
    val paints = state?.displayColors()?.let { JSONArray().put(it.get("foreground")).put(it.get("background")) }
    val previews = remember(paints?.toString()) {
        paints?.let { JSONArray(Native.colorUi(obj("type" to "preview", "colors" to it).toString())) }
    }
    fun paint(slot: Int) = previews?.getJSONObject(slot)?.getJSONArray("rgba")?.let {
        Color(it.getDouble(0).toFloat(), it.getDouble(1).toFloat(), it.getDouble(2).toFloat(), it.optDouble(3, 1.0).toFloat())
    }
    SharedIcon("colors", description, modifier, fill = paint(0), secondary = paint(1))
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
