package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import org.json.JSONArray
import org.json.JSONTokener

private fun JSONArray.color() = Color(getDouble(0).toFloat(), getDouble(1).toFloat(), getDouble(2).toFloat(), optDouble(3, 1.0).toFloat())
private fun JSONArray.point(scale: Float) = Offset(getDouble(0).toFloat() * scale, getDouble(1).toFloat() * scale)

/** Normalized geometry, color conversion, selection and clamping belong to Rust. */
@Composable internal fun ColorPanelControls(host: CanvasHost, availableHeight: Dp = Dp.Infinity) {
    val view = host.panelContent?.objectOrNull("color_panel") ?: return
    val colors = LocalPalette.current
    val space = view.getString("space")
    fun color(action: org.json.JSONObject) = host.dispatch(obj("type" to "color", "action" to action))
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Canvas(Modifier.widthIn(max = (availableHeight - 288.dp).coerceIn(64.dp, 280.dp)).fillMaxWidth().aspectRatio(1f).testTag("color-wheel")
            .semantics { contentDescription = "Color wheel" }
            .pointerInput(host, space) {
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    val side = minOf(size.width, size.height).toFloat()
                    val part = JSONTokener(Native.colorWheelHit(obj("size" to side, "point" to JSONArray(listOf(down.position.x, down.position.y)), "space" to space).toString())).nextValue() as? String
                    if (part != null) {
                        fun pick(point: Offset) = color(obj("op" to "pick", "part" to part, "size" to side, "point" to JSONArray(listOf(point.x, point.y))))
                        down.consume(); pick(down.position)
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                            change.consume(); pick(change.position)
                        } while (change.pressed)
                    }
                }
            }) {
            val side = minOf(size.width, size.height)
            val geometry = view.getJSONObject("geometry")
            val center = geometry.array("center").point(side)
            val inner = geometry.number("inner") * side
            val outer = geometry.number("outer") * side
            val hue = view.array("hue_color").color()
            val stops = view.array("hue_stops").values().map { (it as JSONArray).color() }
            rotate(view.number("hue_start_degrees"), center) {
                drawCircle(Brush.sweepGradient(stops, center), (inner + outer) / 2, center, style = Stroke(outer - inner))
            }
            if (space == "hsv") {
                val square = geometry.array("square")
                val origin = square.point(side)
                val length = square.getDouble(2).toFloat() * side
                drawRect(Brush.horizontalGradient(listOf(Color.White, hue), origin.x, origin.x + length), origin, Size(length, length))
                drawRect(Brush.verticalGradient(listOf(Color.Transparent, Color.Black), origin.y, origin.y + length), origin, Size(length, length))
            } else {
                val points = geometry.array("triangle").values().map { (it as JSONArray).point(side) }
                val vertices = Vertices(VertexMode.Triangles, points, points, listOf(Color.White, Color.Black, hue), listOf(0, 1, 2))
                drawIntoCanvas { it.drawVertices(vertices, BlendMode.Dst, Paint().apply { color = Color.White }) }
            }
            for (key in listOf("hue_marker", "field_marker")) {
                val point = view.array(key).point(side)
                drawCircle(Color.Black, 3.5.dp.toPx(), point, style = Stroke(3.dp.toPx()))
                drawCircle(Color.White, 3.5.dp.toPx(), point, style = Stroke(1.5.dp.toPx()))
            }
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            view.array("swatches").objects().forEach { swatch ->
                val selected = swatch.getBoolean("selected")
                val label = swatch.getString("label")
                val slot = swatch.getString("slot")
                Canvas(Modifier.size(36.dp).testTag("color-swatch-$slot")
                    .semantics { contentDescription = label; this.selected = selected }
                    .clip(RoundedCornerShape(6.dp)).background(colors.input)
                    .border(if (selected) 2.dp else 1.dp, if (selected) colors.accent else colors.divider, RoundedCornerShape(6.dp))
                    .clickable { color(obj("op" to "select", "slot" to slot)) }) {
                    val tile = size.width / 4
                    for (y in 0..3) for (x in 0..3) drawRect(if ((x + y) % 2 == 0) Color.LightGray else Color.White, Offset(x * tile, y * tile), Size(tile, tile))
                    drawRect(swatch.array("rgba").color())
                }
            }
            TextButton({ color(obj("op" to "swap")) }, Modifier.testTag("color-swap")) { Text("Swap") }
        }
        TextButton({ color(obj("op" to "toggle_space")) }, Modifier.testTag("color-space")) {
            Text(if (space == "hsv") "HSV square" else "HLS triangle")
        }
        view.array("components").objects().forEachIndexed { index, component ->
            NumericSetting(component.getString("name"), component.number("value"), component.getJSONObject("numeric"),
                modifier = Modifier.testTag("color-component-$index"), id = "color-component-$index") {
                color(obj("op" to "component", "index" to index, "value" to it))
            }
        }
    }
}
