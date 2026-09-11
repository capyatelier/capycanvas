package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.material3.IconButton
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

/** GPU overview and shared camera geometry; Compose only displays and routes input. */
@Composable internal fun NavigatorPanel(host: CanvasHost) {
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    var geometry by remember { mutableStateOf<JSONObject?>(null) }
    val camera = host.cameraState.toString()
    DisposableEffect(host) { host.navigatorVisible(true); onDispose { host.navigatorVisible(false) } }
    LaunchedEffect(viewport, camera) {
        if (viewport.width > 0 && viewport.height > 0) geometry = host.awaitQuery(obj("type" to "navigator",
            "viewport" to JSONArray(listOf(viewport.width / density, viewport.height / density))))
    }
    Column {
        val image = host.navigatorImage
        Canvas(Modifier.fillMaxWidth().height(220.dp).testTag("navigator-overview").background(colors.surround)
            .onSizeChanged { viewport = it }.pointerInput(host, density) {
                awaitEachGesture {
                    val down = awaitFirstDown(); down.consume()
                    fun send(phase: String, point: Offset) = host.dispatch(obj("type" to "navigator", "phase" to phase,
                        "position" to JSONArray(listOf(point.x / density, point.y / density)),
                        "viewport" to JSONArray(listOf(size.width / density, size.height / density))))
                    send("down", down.position)
                    var released = false
                    try {
                        do {
                            val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                            change.consume(); send(if (change.pressed) "move" else "up", change.position)
                            if (!change.pressed) { released = true; break }
                        } while (true)
                    } finally { if (!released) send("cancel", Offset.Zero) }
                }
            }) {
            geometry?.let { g ->
                val rect = g.getJSONObject("image")
                if (image != null) drawImage(image, dstOffset = IntOffset((rect.number("x") * density).roundToInt(), (rect.number("y") * density).roundToInt()),
                    dstSize = IntSize((rect.number("width") * density).roundToInt(), (rect.number("height") * density).roundToInt()))
                val path = Path()
                g.array("work_area").values().forEachIndexed { index, point ->
                    point as JSONArray
                    val x = point.getDouble(0).toFloat() * density; val y = point.getDouble(1).toFloat() * density
                    if (index == 0) path.moveTo(x,y) else path.lineTo(x,y)
                }
                path.close(); drawPath(path, Color.Black, style = Stroke(3.dp.toPx())); drawPath(path, Color.White, style = Stroke(1.dp.toPx()))
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            val commands = host.snapshot?.getJSONObject("state")?.array("commands")?.objects() ?: emptyList()
            for (id in listOf("zoom_out", "zoom_in", "rotate_left", "rotate_right", "flip_horizontal", "flip_vertical")) {
                commands.find { it.getString("id") == id }?.let { command ->
                    IconButton({ host.invoke(id) }, modifier = Modifier.size(32.dp).testTag("navigator-$id"), enabled = command.getBoolean("enabled")) {
                        SharedIcon(command.getString("icon"), command.getString("label"))
                    }
                }
            }
        }
    }
}
