package art.capycanvas

import android.view.KeyEvent
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalWindowInfo
import org.json.JSONArray
import org.json.JSONObject

internal fun CanvasHost.headerEdit(action: JSONObject) = customize(obj("type" to "header", "action" to action))
internal fun CanvasHost.headerQuery(request: JSONObject, reply: (Any?) -> Unit = {}) = query(obj("type" to "header", "request" to request), reply)
internal fun Offset.headerPoint() = JSONArray(listOf(x, y))
internal fun Rect.headerBounds() = obj("x" to left, "y" to top, "width" to width, "height" to height)

/** The stable workspace owns capture, so compacting/reparenting a child cannot
 * lose a contact. Only Rust resolves destinations, live slides and final edits. */
internal class HeaderInteraction(val host: CanvasHost, val dock: DockInteraction) {
    data class Source(val token: Any, val source: JSONObject, val label: String, val bounds: Rect, val priority: Int)
    val sources = mutableMapOf<Any, Source>()
    var editing = false
    var enabled = false
    var width = 0f
    var metrics = JSONArray()
    var geometry by mutableStateOf<JSONObject?>(null)
    var geometryKey by mutableStateOf("")
    var preview by mutableStateOf<JSONObject?>(null)
    var selected by mutableStateOf<Int?>(null)
    var held by mutableStateOf<Source?>(null)
    var overflow by mutableStateOf<Int?>(null)
    var context by mutableStateOf<JSONObject?>(null)
    var contextBounds = Rect.Zero
    var contact by mutableStateOf(false)
    private var generation = 0
    private var last = Offset.Zero
    fun begin(source: Source, press: Offset) {
        generation++
        held = source; last = press; context = null; overflow = null
        host.headerQuery(obj("op" to "begin", "source" to source.source, "width" to width,
            "insets" to JSONArray(listOf(0, 0)), "metrics" to metrics,
            "press" to press.headerPoint(), "grab" to source.bounds.headerBounds()))
    }
    fun move(point: Offset) {
        last = point
        val token = generation
        host.headerQuery(obj("op" to "preview", "position" to point.headerPoint())) {
            if (generation == token && held != null) preview = it as? JSONObject
        }
    }
    fun finish(cancel: Boolean) {
        if (held == null) return
        generation++
        held = null; preview = null
        host.headerAction(obj("op" to "finish", "position" to last.headerPoint(), "cancel" to cancel))
    }
    fun menu(id: Int?, bounds: Rect) {
        contextBounds = bounds
        host.query(obj("type" to "context", "target" to obj("kind" to "header", "id" to id))) {
            context = it as? JSONObject
        }
    }
    fun key(event: KeyEvent): Boolean {
        if (!editing || !enabled || host.editingText || event.isCtrlPressed || event.isMetaPressed || event.isAltPressed) return false
        val keys = listOf(KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_DEL, KeyEvent.KEYCODE_FORWARD_DEL,
            KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT, KeyEvent.KEYCODE_MENU, KeyEvent.KEYCODE_F10)
        if (event.keyCode !in keys) return false
        if (event.action != KeyEvent.ACTION_DOWN) return true
        if (event.keyCode == KeyEvent.KEYCODE_ESCAPE) {
            if (held != null) finish(true) else if (context != null || overflow != null) { context = null; overflow = null }
            else host.headerEdit(obj("type" to "cancel"))
            return true
        }
        val id = selected ?: return false
        when (event.keyCode) {
            KeyEvent.KEYCODE_DEL, KeyEvent.KEYCODE_FORWARD_DEL -> host.headerEdit(obj("type" to "remove", "id" to id))
            KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT -> host.headerAction(obj("op" to "step", "id" to id,
                "forward" to (event.keyCode == KeyEvent.KEYCODE_DPAD_RIGHT)))
            else -> menu(id, sources.values.firstOrNull { it.source.optInt("value", -1) == id }?.bounds ?: Rect.Zero)
        }
        return true
    }
}

@Composable internal fun Modifier.headerSource(input: HeaderInteraction, source: JSONObject, label: String, priority: Int = 0): Modifier {
    val token = remember { Any() }
    DisposableEffect(input, token) { onDispose { input.sources.remove(token) } }
    return onGloballyPositioned {
        val bounds = it.boundsInRoot().translate(-input.dock.origin)
        input.sources[token] = HeaderInteraction.Source(token, source, label,
            Rect(bounds.topLeft / input.dock.density, bounds.bottomRight / input.dock.density), priority)
    }
}

@Composable internal fun Modifier.headerGestures(input: HeaderInteraction): Modifier {
    val focused = LocalWindowInfo.current.isWindowFocused
    return pointerInput(input, focused) {
        if (!focused) { input.finish(true); return@pointerInput }
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            val press = down.position / input.dock.density
            val source = input.sources.values.filter { it.bounds.contains(press) }.maxByOrNull { it.priority }
            if (source == null || !input.enabled) return@awaitEachGesture
            val id = source.source.takeIf { it.optString("kind") == "item" }?.optInt("value")
            if (currentEvent.buttons.isSecondaryPressed) {
                down.consume(); input.selected = id; input.menu(id, source.bounds)
                return@awaitEachGesture
            }
            if (!input.editing || source.source.optString("kind") == "background") return@awaitEachGesture
            down.consume()
            input.selected = id; input.contact = true
            var started = false
            var released = false
            var held = false
            var remaining = viewConfiguration.longPressTimeoutMillis
            var eventTime = down.uptimeMillis
            try {
                while (true) {
                    val event = if (!held && !started && id != null && down.type != PointerType.Mouse)
                        withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                        else awaitPointerEvent(PointerEventPass.Initial)
                    if (event == null) { held = true; input.menu(id, source.bounds); continue }
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    remaining = (remaining - (change.uptimeMillis - eventTime)).coerceAtLeast(1)
                    eventTime = change.uptimeMillis
                    if ((!change.pressed && change.isConsumed) || !input.editing || !input.enabled) break
                    if (!started && input.sources[source.token]?.source?.toString() != source.source.toString()) break
                    if (!started && change.pressed && (change.position - down.position).getDistance() > viewConfiguration.touchSlop) {
                        input.begin(source, press); started = true
                    }
                    change.consume()
                    if (started) input.move(change.position / input.dock.density)
                    if (!change.pressed) {
                        if (started) input.finish(false)
                        else if (source.source.has("overflow_zone")) input.overflow = source.source.getInt("overflow_zone")
                        released = true; break
                    }
                }
            } finally {
                if (started && !released) input.finish(true)
                if (!released) input.context = null
                input.contact = false
            }
        }
    }
}
