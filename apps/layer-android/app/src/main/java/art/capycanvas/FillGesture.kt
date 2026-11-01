package art.capycanvas

import android.view.HapticFeedbackConstants
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject

internal fun CanvasHost.fillGesture(): JSONObject? {
    colorPreview?.let { if (it.has("fill_gesture")) return it.objectOrNull("fill_gesture") }
    return snapshot?.objectOrNull("state")?.objectOrNull("fill_gesture")
}

@Composable internal fun FillGestureOverlay(host: CanvasHost) {
    val gesture = host.fillGesture() ?: return
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val nativeView = LocalView.current
    val tick = gesture.get("tick").toString()
    LaunchedEffect(gesture.get("transaction").toString(), tick) {
        if (tick != "0") nativeView.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
    }
    Box(Modifier.fillMaxSize().zIndex(1100f)) {
        Canvas(Modifier.matchParentSize().testTag("fill-gesture-overlay")) {
            val dashed = PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 3.dp.toPx()))
            for (segment in gesture.array("seals").values()) {
                val points = segment as org.json.JSONArray
                fun point(index: Int) = points.getJSONArray(index).let { Offset(it.getDouble(0).toFloat() * density, it.getDouble(1).toFloat() * density) }
                drawLine(colors.accent, point(0), point(1), strokeWidth = 1.5f.dp.toPx(), pathEffect = dashed)
            }
            if (!gesture.isNull("ring")) {
                val tip = gesture.getJSONArray("tip")
                val radius = 12.dp.toPx()
                val at = Offset(tip.getDouble(0).toFloat() * density - radius, tip.getDouble(1).toFloat() * density - radius)
                drawArc(colors.accent, -90f, gesture.number("ring") * 360f, false, at, Size(radius * 2, radius * 2), style = Stroke(2.dp.toPx()))
            }
        }
        gesture.objectOrNull("pill")?.let { pill ->
            val bounds = pill.getJSONObject("bounds")
            Box(Modifier.placed(bounds, density).testTag("fill-gesture-pill")
                .panelSurface(BarElevation, SurfaceShape).glass(SurfaceShape)) {
                Surface(Modifier.fillMaxSize(), shape = SurfaceShape, color = colors.onGlass.panelFill, contentColor = colors.text) {
                    Box {
                        Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 5.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(pill.getString("label"), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(pill.getString("value"), maxLines = 1)
                        }
                        val track = pill.getJSONArray("track")
                        val left = track.getDouble(0).toFloat()
                        Canvas(Modifier.offset((left - bounds.number("x")).dp, 30.dp)
                            .width((track.getDouble(1).toFloat() - left).dp).height(4.dp)) {
                            editorSliderTrack(colors.input, colors.sliderFill, (pill.number("fill_end") - left) * density)
                        }
                    }
                }
            }
        }
    }
}
