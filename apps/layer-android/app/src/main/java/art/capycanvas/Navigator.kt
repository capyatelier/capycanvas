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
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject

/** GPU overview and shared camera geometry; Compose only displays and routes input. */
@Composable internal fun NavigatorPanel(host: CanvasHost, availableHeight: Dp = 268.dp) {
    val density = LocalDensity.current.density
    val colors = LocalPalette.current
    var viewport by remember { mutableStateOf(IntSize.Zero) }
    var geometry by remember { mutableStateOf<JSONObject?>(null) }
    val key = remember { Any() }
    val order = LocalWorkspaceZ.current
    val document = host.panelContent?.getJSONObject("state")?.array("tabs")?.optJSONObject(0)
    val documentSize = document?.let { it.optInt("width") to it.optInt("height") }
    DisposableEffect(host) { onDispose { host.navigatorPlacement(key, null) } }
    LaunchedEffect(viewport, documentSize) {
        if (viewport.width > 0 && viewport.height > 0) geometry = host.awaitQuery(obj("type" to "navigator",
            "viewport" to JSONArray(listOf(viewport.width / density, viewport.height / density))))
    }
    Column {
        Canvas(Modifier.fillMaxWidth().height((availableHeight - 48.dp).coerceIn(64.dp, 220.dp)).testTag("navigator-overview").background(colors.surround)
            .onSizeChanged { viewport = it }
            .onGloballyPositioned { coords ->
                val origin = coords.positionInRoot() - host.surfaceOrigin
                val clip = coords.boundsInRoot().translate(-host.surfaceOrigin)
                fun rect(r: Rect) = JSONArray(listOf(r.left, r.top, r.width, r.height))
                host.navigatorPlacement(key, if (clip.isEmpty) null else obj(
                    "bounds" to JSONArray(listOf(origin.x, origin.y, coords.size.width, coords.size.height)),
                    "clip" to rect(clip), "order" to order))
            }.pointerInput(host, density) {
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
                // Clear this part of the native window to reveal the live
                // overview in the existing SurfaceView below Compose.
                drawRect(Color.Transparent, Offset(rect.number("x") * density, rect.number("y") * density),
                    Size(rect.number("width") * density, rect.number("height") * density), blendMode = BlendMode.Clear)
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            val commands = host.panelContent?.getJSONObject("state")?.array("commands")?.objects() ?: emptyList()
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
