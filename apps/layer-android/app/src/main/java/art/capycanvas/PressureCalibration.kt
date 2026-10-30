package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

@Composable internal fun ResponseCalibration(host: CanvasHost, state: JSONObject, pressure: JSONObject?, tilt: JSONObject?) {
    val view = tilt ?: pressure ?: return
    val kind = if (tilt != null) "tilt" else "pressure"
    key(kind) {
        val colors = LocalPalette.current
        val density = LocalDensity.current.density
        val metrics = panelMetrics()
        val current by rememberUpdatedState(view)
        val prefix = "pen-$kind"
        fun send(action: JSONObject, published: (() -> Unit)? = null) = host.dispatch(if (tilt != null)
            obj("type" to "tilt_calibration", "action" to obj("kind" to "panel", "action" to action))
            else obj("type" to "pressure_calibration", "action" to action), published = published)
        BackHandler { send(obj("kind" to "cancel")) }
        DisposableEffect(host) { onDispose { host.calibrationPenBounds = null; host.endCalibrationMotion() } }
        BoxWithConstraints(Modifier.fillMaxSize().zIndex(300f)) {
            val viewport = JSONArray(listOf(maxWidth.value, maxHeight.value))
            val bounds = view.getJSONObject("bounds")
            var extent by remember { mutableStateOf(Offset.Zero) }
            var acceptedMeasurement by remember { mutableStateOf<Pair<Offset, String>?>(null) }
            var exposed by remember { mutableStateOf(false) }
            LaunchedEffect(extent, maxWidth, maxHeight) {
                if (extent.x > 0 && extent.y > 0) {
                    val measured = extent to viewport.toString()
                    send(obj("kind" to "measure", "extent" to JSONArray(listOf(extent.x, extent.y)), "viewport" to viewport)) { acceptedMeasurement = measured }
                }
            }
            val fitted = extent.x > 0 && acceptedMeasurement == (extent to viewport.toString()) &&
                kotlin.math.abs(bounds.number("width") - extent.x) < .5f && kotlin.math.abs(bounds.number("height") - extent.y) < .5f
            LaunchedEffect(fitted) { if (fitted) exposed = true }
            Surface(Modifier.offset { IntOffset((bounds.number("x") * density).roundToInt(), (bounds.number("y") * density).roundToInt()) }
                .width(maxOf(384.dp, bounds.number("width").dp).coerceAtMost(maxWidth)).heightIn(max = maxHeight)
                .onSizeChanged { extent = Offset(it.width / density, it.height / density) }
                .onGloballyPositioned { host.calibrationPenBounds = it.boundsInWindow() }
                .pointerInput(Unit) { awaitPointerEventScope { while (true) awaitPointerEvent() } }
                .alpha(if (exposed) 1f else 0f)
                .testTag("$prefix-dialog").semantics { paneTitle = current.getString("title") },
                shape = SurfaceShape, color = colors.panel, contentColor = colors.text, shadowElevation = 8.dp) {
                Column {
                    var origin by remember { mutableStateOf(Offset.Zero) }
                    Row(Modifier.fillMaxWidth()
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
                        }.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.Top) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(view.getString("title"), Modifier.heightIn(min = 24.dp).testTag("$prefix-title"), fontWeight = FontWeight.Bold)
                            if (tilt != null) Text(view.getString("device"), Modifier.testTag("pen-tilt-device"), color = colors.secondary, style = MaterialTheme.typography.bodySmall)
                        }
                        Spacer(Modifier.width(8.dp))
                        Box(Modifier.size(24.dp).clip(ControlShape).testTag("$prefix-close")
                            .semantics { contentDescription = current.getString("close") }
                            .clickable { send(obj("kind" to "cancel")) }, contentAlignment = Alignment.Center) { SharedIcon("window-close", null, Modifier.size(16.dp)) }
                    }
                    Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        SharedCurveControl(host, state, view.getJSONObject("editor"), view.getString("title"), obj("kind" to kind), gutterLabel = view.getString("axis_gutter")) {
                            if (tilt != null) {
                                Canvas(Modifier.align(Alignment.BottomEnd).padding(8.dp).size(72.dp)
                                    .pointerInput(Unit) { awaitPointerEventScope { while (true) awaitPointerEvent().changes.forEach { it.consume() } } }.testTag("pen-tilt-direction").semantics { contentDescription = current.getString("direction") }) {
                                    val radius = size.minDimension / 2f - 5.dp.toPx()
                                    drawCircle(colors.text.copy(alpha = .18f), radius, style = Stroke(1.dp.toPx()))
                                    for ((key, adjusted) in listOf("input_direction" to false, "adjusted_direction" to true)) {
                                        view.optJSONArray(key)?.let { vector ->
                                            val delta = Offset(vector.getDouble(0).toFloat(), vector.getDouble(1).toFloat()) * radius
                                            val color = if (adjusted) colors.accent else colors.text.copy(alpha = .55f)
                                            val width = (if (adjusted) 2.5f else 1.5f).dp.toPx()
                                            val tip = center + delta
                                            drawLine(color, center, tip, width)
                                            if (delta.getDistance() > 3.dp.toPx()) {
                                                val unit = delta / delta.getDistance()
                                                val back = tip - unit * 6.dp.toPx()
                                                val side = Offset(-unit.y, unit.x) * 3.dp.toPx()
                                                drawLine(color, back + side, tip, width); drawLine(color, tip, back - side, width)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.inset)) {
                            if (tilt == null) {
                                OutlinedButton({ send(obj("kind" to "sensitivity", "lighter" to false)) }, Modifier.weight(1f).height(36.dp).testTag("$prefix-firmer"),
                                    enabled = view.getBoolean("firmer_enabled"), shape = ControlShape, contentPadding = PaddingValues(horizontal = 12.dp)) { Text(view.getString("firmer")) }
                                OutlinedButton({ send(obj("kind" to "sensitivity", "lighter" to true)) }, Modifier.weight(1f).height(36.dp).testTag("$prefix-lighter"),
                                    enabled = view.getBoolean("lighter_enabled"), shape = ControlShape, contentPadding = PaddingValues(horizontal = 12.dp)) { Text(view.getString("lighter")) }
                            } else {
                                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.spacing)) {
                                    Text(view.getString("sensitivity"), Modifier.weight(1f, fill = false).testTag("pen-tilt-sensitivity-title"))
                                    Row(Modifier.clip(ControlShape).border(1.dp, colors.divider, ControlShape)) {
                                        for ((key, lighter, icon) in listOf(Triple("firmer", false, "minus"), Triple("lighter", true, "plus"))) {
                                            HoverTip(view.getString(key)) {
                                                Box(Modifier.size(36.dp).background(colors.input).testTag("$prefix-$key")
                                                    .clickable(enabled = view.getBoolean("${key}_enabled")) { send(obj("kind" to "sensitivity", "lighter" to lighter)) }
                                                    .semantics { contentDescription = view.getString(key) }, contentAlignment = Alignment.Center) { SharedIcon(icon, null, Modifier.size(16.dp)) }
                                            }
                                        }
                                    }
                                }
                                Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(metrics.spacing, Alignment.End)) {
                                    Text(view.getString("direction"), Modifier.weight(1f, fill = false).testTag("pen-tilt-direction-title"))
                                    Row(Modifier.height(36.dp).clip(ControlShape).border(1.dp, colors.divider, ControlShape).testTag("pen-tilt-axes")) {
                                        for ((field, icon, tag) in listOf(Triple("reverse_horizontal", "flip-horizontal", "reverse-horizontal"),
                                            Triple("reverse_vertical", "flip-vertical", "reverse-vertical"), Triple("swap", "swap-orientation", "swap-axes"))) {
                                            val checked = view.getJSONObject("axes").getBoolean(field)
                                            val label = view.getString(if (field == "swap") "swap_axes" else field)
                                            HoverTip(label) {
                                                Box(Modifier.size(36.dp).background(if (checked) colors.active else colors.input).testTag("pen-tilt-$tag")
                                                    .toggleable(checked, role = Role.Checkbox) { value ->
                                                        val axes = current.getJSONObject("axes").copy().put(field, value)
                                                        host.dispatch(obj("type" to "tilt_calibration", "action" to obj("kind" to "axes", "axes" to axes)))
                                                    }.semantics { contentDescription = label }, contentAlignment = Alignment.Center) { SharedIcon(icon, null, Modifier.size(16.dp)) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton({ host.dispatch(obj("type" to "curve_editor", "target" to obj("kind" to kind), "action" to obj("kind" to "reset"))) },
                            Modifier.height(36.dp).testTag("$prefix-reset"), shape = ControlShape, contentPadding = PaddingValues(horizontal = 12.dp)) { Text(view.getString("reset")) }
                        Spacer(Modifier.weight(1f))
                        OutlinedButton({ send(obj("kind" to "cancel")) }, Modifier.height(36.dp).testTag("$prefix-cancel"), shape = ControlShape,
                            contentPadding = PaddingValues(horizontal = 12.dp)) { Text(view.getString("cancel")) }
                        Button({ send(obj("kind" to "apply")) }, Modifier.height(36.dp).testTag("$prefix-apply"), enabled = tilt == null || view.getBoolean("ready"),
                            shape = ControlShape, contentPadding = PaddingValues(horizontal = 12.dp)) { Text(view.getString("apply")) }
                    }
                }
            }
        }
    }
}
