package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.abs
import kotlin.math.roundToInt

/** Production SVG painter in an isolated activity: never opens or edits a user document. */
class AndroidIconTest {
    @get:Rule val compose = createComposeRule()

    @Test fun translucentForegroundPreservesFixedSwatchPaints() {
        compose.setContent {
            Row(Modifier.background(Color(0xffcccccc))) {
                SharedIcon("plus", null, Modifier.size(32.dp).testTag("tint-alpha"), tint = Color.Black.copy(alpha = .5f))
                SharedIcon("colors", null, Modifier.size(32.dp).testTag("fixed-paints"), tint = Color.Black.copy(alpha = .5f))
            }
        }
        val plus = compose.onNodeWithTag("tint-alpha", useUnmergedTree = true).captureToImage()
        val pixel = plus.toPixelMap()[plus.width / 2, plus.height / 2]
        assertEquals("Secondary foreground respects its alpha", .4f, pixel.red, .01f)
        val fixed = compose.onNodeWithTag("fixed-paints", useUnmergedTree = true).captureToImage()
        val pixels = fixed.toPixelMap()
        assertEquals("Fixed black remains opaque", 0f, pixels[fixed.width * 5 / 16, fixed.height * 5 / 16].red, .01f)
        assertEquals("Fixed white remains opaque", 1f, pixels[fixed.width * 13 / 16, fixed.height * 13 / 16].red, .01f)
    }

    @Test fun allIconsRenderAtToolbarSizesWithThemeAndFixedPaints() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val icons = context.assets.list("")!!.filter { it.startsWith("layer-") && it.endsWith("-symbolic.svg") }.sorted()
        assertTrue("Complete packaged icon bank", icons.size >= 157)
        val directory = File(context.getExternalFilesDir(null), "validation/icons").apply { mkdirs() }
        val fixtures = JSONArray()
        var theme by mutableStateOf("light")
        var size by mutableStateOf(16)
        var opacity by mutableStateOf(1f)
        var foreground by mutableStateOf(Color(0xff292a2d))
        var background by mutableStateOf(Color(0xfffafafa))
        compose.setContent {
            Column(Modifier.width(576.dp).background(background).testTag("icon-grid")) {
                icons.chunked(12).forEach { row ->
                    Row(Modifier.height(48.dp)) {
                        row.forEach { file ->
                            Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                                val name = file.removePrefix("layer-").removeSuffix("-symbolic.svg")
                                SharedIcon(name, name, Modifier.size(size.dp).alpha(opacity), tint = foreground,
                                    fill = if (name == "color") Color(0xff33d17a) else null)
                            }
                        }
                    }
                }
            }
        }
        for (mode in listOf("light", "dark")) for (points in listOf(16, 24, 32)) {
            for (state in listOf("normal", "accent", "disabled")) {
                compose.runOnIdle {
                    theme = mode; size = points
                    background = if (mode == "light") Color(0xfffafafa) else Color(0xff242629)
                    foreground = if (state == "accent") Color(0xff3584e4)
                        else if (mode == "light") Color(0xff292a2d) else Color(0xfff0f0f1)
                    opacity = if (state == "disabled") .35f else 1f
                }
                compose.waitForIdle()
                val image = compose.onNodeWithTag("icon-grid").captureToImage()
                val pixels = image.toPixelMap()
                val scale = image.width / 576f
                fun pixel(index: Int, x: Float, y: Float): Color {
                    val left = index % 12 * 48 + 24 - size / 2f
                    val top = index / 12 * 48 + 24 - size / 2f
                    return pixels[((left + x * size / 16) * scale).toInt(), ((top + y * size / 16) * scale).toInt()]
                }
                fun differs(a: Color, b: Color) = maxOf(abs(a.red-b.red), abs(a.green-b.green), abs(a.blue-b.blue)) > .025f
                for ((index, name) in icons.withIndex()) {
                    val left = ((index % 12 * 48 + 24 - size / 2f) * scale).roundToInt()
                    val top = ((index / 12 * 48 + 24 - size / 2f) * scale).roundToInt()
                    val extent = (size * scale).toInt()
                    var ink = 0
                    for (y in top until top + extent) for (x in left until left + extent) {
                        if (differs(pixels[x, y], background)) ink++
                    }
                    assertTrue("$name $mode $size $state is visible", ink > 1)
                }
                val clear = icons.indexOf("layer-clear-symbolic.svg")
                assertFalse("Clear has an empty center", differs(pixel(clear, 8f, 8f), background))
                val swatches = icons.indexOf("layer-colors-symbolic.svg")
                fun composite(value: Float, channel: Float) = value * opacity + channel * (1-opacity)
                val black = pixel(swatches, 4f, 4f)
                val white = pixel(swatches, 12f, 12f)
                assertEquals("Fixed black is not foreground tinted", composite(0f, background.red), black.red, .025f)
                assertEquals("Fixed white is not foreground tinted", composite(1f, background.red), white.red, .025f)
                val name = "$mode-$size-$state"
                File(directory, "native-$name.png").outputStream().use {
                    image.asAndroidBitmap().compress(Bitmap.CompressFormat.PNG, 100, it)
                }
                fun rgb(color: Color) = "#%02x%02x%02x".format((color.red*255).roundToInt(), (color.green*255).roundToInt(), (color.blue*255).roundToInt())
                fixtures.put(JSONObject().put("name", name).put("theme", theme).put("size", size)
                    .put("width", 576).put("height", (icons.size+11)/12*48).put("scale", scale)
                    .put("foreground", rgb(foreground)).put("background", rgb(background)).put("opacity", opacity)
                    .put("icons", JSONArray(icons.map { it.removeSuffix(".svg") })))
            }
        }
        File(directory, "fixtures.json").writeText(JSONObject().put("schema", 1).put("fixtures", fixtures).toString(2))
    }
}
