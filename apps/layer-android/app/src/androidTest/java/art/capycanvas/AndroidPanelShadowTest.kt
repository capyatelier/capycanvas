package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.IntOffset
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.abs

/** PixelCopy captures real hardware shadows, including their overlap with translucent fills. */
class AndroidPanelShadowTest {
    @get:Rule val compose = createComposeRule()

    @Test fun shadowsStayOutsideTranslucentPanels() {
        var shape: Shape by mutableStateOf(SurfaceShape)
        var alpha by mutableStateOf(.35f)
        var elevation by mutableStateOf(6)
        var enabled by mutableStateOf(false)
        var width by mutableStateOf(160)
        var height by mutableStateOf(160)
        var left by mutableStateOf(64)
        var fill by mutableStateOf(Color(0xffededed))
        val directory = File(InstrumentationRegistry.getInstrumentation().targetContext.getExternalFilesDir(null),
            "validation/panel-shadow").apply { mkdirs() }
        compose.setContent {
            Box(Modifier.size(320.dp).background(Color.White).testTag("shadow-scene")) {
                Box(Modifier.offset { IntOffset(left.dp.roundToPx(), 64.dp.roundToPx()) }.size(width.dp, height.dp)
                    .then(if (enabled) Modifier.panelShadow(elevation.dp, shape) else Modifier)
                    .clip(shape).background(fill.copy(alpha = alpha))) {
                    // Content must survive exclusion of the shadow's interior.
                    Box(Modifier.offset { IntOffset(66.dp.roundToPx(), (height / 2 - 4).dp.roundToPx()) }
                        .size(8.dp).background(Color.Red))
                }
            }
        }
        for ((name, outline, panelHeight) in listOf(Triple("squircle", SurfaceShape, 160),
            Triple("joined", SquircleShape(topStart = 18.dp, bottomStart = 18.dp), 160),
            Triple("round", RoundedCornerShape(18.dp), 160), Triple("square", RectangleShape, 160),
            Triple("short", SurfaceShape, 24))) {
            for (dark in listOf(false, true)) for (opacity in listOf(0f, .35f, 1f)) {
                compose.runOnIdle {
                    shape = outline; fill = Color(if (dark) 0xff414141 else 0xffededed); alpha = opacity
                    // Exercise cache invalidation as both allocation and placement change.
                    width = if (dark) 192 else 160; left = if (dark) 48 else 64
                    height = panelHeight
                    elevation = if (opacity == 0f) 16 else if (dark) 12 else 6
                    enabled = true
                }
                val image = compose.onNodeWithTag("shadow-scene").captureToImage()
                val pixels = image.toPixelMap()
                compose.runOnIdle { enabled = false }
                val reference = compose.onNodeWithTag("shadow-scene").captureToImage().toPixelMap()
                compose.runOnIdle { enabled = true }
                compose.waitForIdle()
                val label = "$name-${if (dark) "dark" else "light"}-$opacity"
                File(directory, "$label.png").outputStream().use {
                    image.asAndroidBitmap().compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)
                }
                val scale = image.width / 320f
                fun equalAt(x: Float, y: Float) {
                    val px = (x * scale).toInt(); val py = (y * scale).toInt()
                    val a = reference[px, py]; val b = pixels[px, py]
                    assertTrue("$label: shadow darkens panel at $x,$y ($a -> $b)",
                        listOf(abs(a.red - b.red), abs(a.green - b.green), abs(a.blue - b.blue)).all { it <= 2f / 255 })
                }
                // Straight edges and rounded/square corners, clear of antialiasing coverage.
                for (inset in listOf(2f, 4f, 8f, 16f, 32f)) {
                    equalAt(left + inset, 64 + height / 2f); equalAt(left + width - inset, 64 + height / 2f)
                    if (inset < height / 2f) {
                        equalAt(left + width / 2f, 64 + inset); equalAt(left + width / 2f, 64 + height - inset)
                    }
                }
                for (x in listOf(left + 8f, left + width - 8f)) for (y in listOf(72f, 64 + height - 8f)) equalAt(x, y)
                equalAt(left + 70f, 64 + height / 2f)
                assertTrue("$label: exterior shadow remains visible", (1..24).any { distance ->
                    pixels[((left + width / 2f) * scale).toInt(), ((64 + height + distance) * scale).toInt()].red < .98f
                })
            }
        }
    }
}
