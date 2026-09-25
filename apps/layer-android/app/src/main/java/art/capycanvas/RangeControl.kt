package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import kotlin.math.abs

private data class RangeContact(val index: Int, val before: Float, val domain: Pair<Float, Float>, val offset: Float)

/** One retained interval for panels and toolbar options. Rust resolves values;
 * Compose owns native mouse/touch/pen capture, focus and accessibility. */
@Composable internal fun RangeControl(bounds: List<JSONObject>, label: String, modifier: Modifier = Modifier,
    prefix: String = "tool", showSlider: Boolean = true, onChange: (Int, Float) -> Unit) {
    val colors = LocalPalette.current
    val host = LocalCanvasHost.current
    val focusToken = remember { Any() }
    val density = LocalDensity.current.density
    val spec = bounds[0].getJSONObject("numeric")
    val inputValues = bounds.map { it.number("value") }
    var values by remember { mutableStateOf(inputValues) }
    LaunchedEffect(inputValues) { values = inputValues }
    val currentChange by rememberUpdatedState(onChange)
    var contact by remember { mutableStateOf<RangeContact?>(null) }
    var live by remember { mutableStateOf(true) }
    DisposableEffect(Unit) { onDispose {
        live = false; contact = null
        if (host.rangeControlFocus === focusToken) host.rangeControlFocus = null
    } }
    val domain = contact?.domain ?: (minOf(spec.number("soft_min"), values[0]) to maxOf(spec.number("soft_max"), values[1]))
    val focus = remember { List(2) { FocusRequester() } }
    var focused by remember { mutableIntStateOf(-1) }
    fun resolve(index: Int, op: JSONObject, control: JSONObject = spec) = JSONObject(Native.number(
        obj("control" to control, "value" to values[index], "operation" to op).toString())).number("value")
    fun change(index: Int, value: Float) {
        if (!live) return
        val next = if (index == 0) minOf(value, values[1]) else maxOf(value, values[0])
        if (next != values[index]) { values = values.mapIndexed { i, v -> if (i == index) next else v }; currentChange(index, next) }
    }
    fun end(cancel: Boolean) {
        val c = contact ?: return
        contact = null
        if (cancel) change(c.index, c.before)
    }
    val move by rememberUpdatedState<(Float, Int) -> Unit>({ x, width ->
        contact?.let { c ->
            val control = JSONObject(spec.toString()).put("soft_min", c.domain.first).put("soft_max", c.domain.second)
            change(c.index, resolve(c.index, obj("type" to "position", "position" to
                ((x-c.offset-8*density)/maxOf(1f,width-16*density))), control))
        }
    })
    HoverTip(label, modifier) {
        Row(Modifier.fillMaxWidth().height(28.dp).testTag("$prefix-range-tonal"), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            val number: @Composable (Int) -> Unit = { index ->
                val field = bounds[index]; val id = field.getString("id")
                NumericSetting("${field.getString("label")} — $label", values[index], field.getJSONObject("numeric"),
                    Modifier.testTag("$prefix-setting-$id"), id = "$prefix-$id", inline = true, toolbar = true, valueOnly = true,
                    showUnits = false, showSlider = false,
                    limits = (if(index==0) spec.number("min") else values[0])..(if(index==0) values[1] else spec.number("max"))) { value -> if (live) currentChange(index, value) }
            }
            number(0)
            if (showSlider) BoxWithConstraints(Modifier.weight(1f).height(28.dp).testTag("$prefix-range-track")
                .onPreviewKeyEvent {
                    if (it.type == KeyEventType.KeyDown && it.key == Key.Escape && contact != null) { end(true); true } else false
                }.pointerInput(spec.toString()) {
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        if (currentEvent.buttons.isSecondaryPressed) return@awaitEachGesture
                        down.consume()
                        val d = minOf(spec.number("soft_min"), values[0]) to maxOf(spec.number("soft_max"), values[1])
                        val p = values.map { 8*density + (it-d.first)/(d.second-d.first)*(size.width-16*density) }
                        val x = down.position.x
                        val index = if (abs(p[1]-p[0]) < density) { if (x >= p[0]) 1 else 0 }
                            else if (abs(x-p[1]) < abs(x-p[0])) 1 else 0
                        contact = RangeContact(index, values[index], d, if (abs(x-p[index])<=12*density) x-p[index] else 0f)
                        focus[index].requestFocus(); move(x,size.width)
                        var finished = false
                        try {
                            while (true) {
                                val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                                // Compose delivers ACTION_CANCEL as a consumed release.
                                if (!c.pressed && c.isConsumed) break
                                c.consume()
                                if (!c.pressed) { finished = true; break }
                                move(c.position.x,size.width)
                            }
                        } finally { end(!finished) }
                    }
                }) {
                val trackWidth = constraints.maxWidth.toFloat()
                val positions = values.map { 8*density + ((it-domain.first)/(domain.second-domain.first)).coerceIn(0f,1f)*(trackWidth-16*density) }
                Canvas(Modifier.fillMaxSize()) {
                    val y = size.height/2
                    drawRect(colors.text.copy(alpha=.2f), Offset(8*density,y-2*density), Size(maxOf(0f,size.width-16*density),4*density))
                    drawRect(colors.text.copy(alpha=.65f), Offset(positions[0],y-2*density), Size(maxOf(0f,positions[1]-positions[0]),4*density))
                    for (i in 0..1) {
                        val x = positions[i]+if(i==0) -7*density else density
                        drawRoundRect(colors.text, Offset(x,y-7*density), Size(6*density,14*density), CornerRadius(2*density))
                        if (focused==i) drawRoundRect(colors.text, Offset(x-3*density,y-10*density), Size(12*density,20*density), CornerRadius(4*density), style=Stroke(density))
                    }
                }
                for (i in 0..1) Box(Modifier.offset(x=(positions[i]/density-8).dp).width(16.dp).fillMaxHeight()
                    .testTag("$prefix-range-handle-${bounds[i].getString("id")}").focusRequester(focus[i])
                    .onFocusChanged {
                        if(it.isFocused) { focused=i; host.rangeControlFocus=focusToken }
                        else if(focused==i) { focused=-1; if(host.rangeControlFocus===focusToken) host.rangeControlFocus=null }
                    }
                    .onKeyEvent { e ->
                        if(e.type!=KeyEventType.KeyDown) false else {
                            val steps = when(e.key) { Key.DirectionLeft,Key.DirectionDown -> -1; Key.DirectionRight,Key.DirectionUp -> 1; Key.PageDown -> -10; Key.PageUp -> 10; else -> 0 }
                            when {
                                steps!=0 -> { change(i,resolve(i,obj("type" to "step","steps" to steps)));true }
                                e.key==Key.MoveHome -> { change(i,if(i==0)spec.number("min") else values[0]);true }
                                e.key==Key.MoveEnd -> { change(i,if(i==0)values[1] else spec.number("max"));true }
                                else -> false
                            }
                        }
                    }.semantics {
                        contentDescription="${bounds[i].getString("label")} — $label"
                        progressBarRangeInfo=ProgressBarRangeInfo(values[i],(if(i==0) spec.number("min") else values[0])..(if(i==0) values[1] else spec.number("max")))
                        stateDescription=JSONObject(Native.number(obj("control" to spec,"value" to values[i],"operation" to obj("type" to "format")).toString())).getString("text")
                        setProgress { change(i,resolve(i,obj("type" to "value","value" to it)));true }
                    }.focusable())
            }
            number(1)
        }
    }
}
