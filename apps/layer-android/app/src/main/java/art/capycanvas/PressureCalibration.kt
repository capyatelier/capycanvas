package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

@Composable internal fun PressureCalibration(host: CanvasHost, state: JSONObject, view: JSONObject?) {
    view ?: return
    val colors = LocalPalette.current
    val density = LocalDensity.current.density
    val current by rememberUpdatedState(view)
    fun send(action: JSONObject) = host.dispatch(obj("type" to "pressure_calibration", "action" to action))
    BackHandler { send(obj("kind" to "cancel")) }
    BoxWithConstraints(Modifier.fillMaxSize().zIndex(300f)) {
        val viewport = JSONArray(listOf(maxWidth.value, maxHeight.value))
        val bounds = view.getJSONObject("bounds")
        var extent by remember { mutableStateOf(Offset.Zero) }
        LaunchedEffect(extent, maxWidth, maxHeight) {
            if (extent.x > 0 && extent.y > 0) send(obj("kind" to "measure", "extent" to JSONArray(listOf(extent.x, extent.y)), "viewport" to viewport))
        }
        Surface(Modifier.offset { IntOffset((bounds.number("x") * density).roundToInt(), (bounds.number("y") * density).roundToInt()) }
            .width(bounds.number("width").dp.coerceAtMost(maxWidth)).heightIn(max = maxHeight)
            .onSizeChanged { extent = Offset(it.width / density, it.height / density) }.testTag("pen-pressure-dialog"),
            shape = SurfaceShape, color = colors.panel, contentColor = colors.text, shadowElevation = 8.dp) {
            Column {
                var origin by remember { mutableStateOf(Offset.Zero) }
                Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).background(colors.tabs)
                    .onGloballyPositioned { origin = it.positionInRoot() }
                    .pointerInput(viewport.toString()) {
                        awaitEachGesture {
                            val down = awaitFirstDown(); down.consume()
                            val start = origin
                            fun contact(phase: String, at: Offset) = send(obj("kind" to "drag", "phase" to phase,
                                "position" to JSONArray(listOf(at.x / density, at.y / density)), "viewport" to viewport))
                            contact("down", down.position + start)
                            var released = false
                            try {
                                while (true) {
                                    val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                                    if (!change.pressed) { contact("up", change.position + origin); released = true; break }
                                    change.consume(); contact("move", change.position + origin)
                                }
                            } finally { if (!released) contact("cancel", Offset.Zero) }
                        }
                    }.padding(start = 12.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(view.getString("title"), Modifier.weight(1f), fontWeight = FontWeight.Bold)
                    Box(Modifier.size(28.dp).clip(ControlShape).testTag("pen-pressure-close")
                        .semantics { contentDescription = current.getString("close") }
                        .clickable { send(obj("kind" to "cancel")) }, contentAlignment = Alignment.Center) { SharedIcon("window-close", null, Modifier.size(16.dp)) }
                }
                Column(Modifier.verticalScroll(rememberScrollState()).padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    SharedCurveControl(host, state, view.getJSONObject("editor"), view.getString("title"), obj("kind" to "pressure"))
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton({ send(obj("kind" to "sensitivity", "lighter" to false)) }, Modifier.weight(1f).testTag("pen-pressure-firmer"), enabled = view.getBoolean("firmer_enabled"), shape = ControlShape) { Text(view.getString("firmer")) }
                        OutlinedButton({ send(obj("kind" to "sensitivity", "lighter" to true)) }, Modifier.weight(1f).testTag("pen-pressure-lighter"), enabled = view.getBoolean("lighter_enabled"), shape = ControlShape) { Text(view.getString("lighter")) }
                    }
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        TextButton({ host.dispatch(obj("type" to "curve_editor", "target" to obj("kind" to "pressure"), "action" to obj("kind" to "reset"))) }, Modifier.testTag("pen-pressure-reset"), shape = ControlShape) { Text(view.getString("reset")) }
                        Spacer(Modifier.weight(1f))
                        TextButton({ send(obj("kind" to "cancel")) }, Modifier.testTag("pen-pressure-cancel"), shape = ControlShape) { Text(view.getString("cancel")) }
                        Button({ send(obj("kind" to "apply")) }, Modifier.testTag("pen-pressure-apply"), shape = ControlShape) { Text(view.getString("apply")) }
                    }
                }
            }
        }
    }
}
