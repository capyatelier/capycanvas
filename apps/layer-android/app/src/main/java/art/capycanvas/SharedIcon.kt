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
import com.caverock.androidsvg.SVG

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
            SVG.getFromString(source.readText()
                .replace("currentColor", paint(tint))
                .replace("#33d17a", fill?.let(::paint) ?: "none")).renderToPicture()
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
