package art.capycanvas

import android.os.SystemClock
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.*
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONArray

internal val colorTouchSurfaces = AtomicLong()

internal class ColorTouchClaim { var claimed = false }
private val colorWheelTarget = obj("kind" to "color_wheel")

internal fun CanvasHost.colorTouch(surface: Long, id: PointerId, phase: String, point: Offset, time: Long) =
    input(obj("type" to "color_touch", "surface" to surface, "id" to id.value, "phase" to phase,
        "position" to JSONArray().put(point.x).put(point.y), "time_ns" to time * 1_000_000L))

@Composable internal fun Modifier.colorHistoryInput(host: CanvasHost, claim: ColorTouchClaim, owns: (Offset) -> Boolean): Modifier {
    val dock = LocalDock.current
    val surface = remember { colorTouchSurfaces.incrementAndGet() }
    val inside by rememberUpdatedState(owns)
    return (if (dock != null) contextAnchor(dock, colorWheelTarget) else this).edgeControl().pointerInput(host, surface, dock) {
        awaitEachGesture {
            val first = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
            if (first.isConsumed || !inside(first.position)) return@awaitEachGesture
            if (currentEvent.buttons.isSecondaryPressed) {
                if (dock == null) return@awaitEachGesture
                first.consume()
                dock.context(colorWheelTarget)
                return@awaitEachGesture
            }
            if (first.type == PointerType.Mouse && !currentEvent.buttons.isPrimaryPressed) return@awaitEachGesture
            fun contact(phase: String) = host.dispatch(obj("type" to "color_contact", "phase" to phase))
            val touches = mutableMapOf<PointerId, Offset>()
            var open = true
            claim.claimed = false
            contact("down")
            try {
                if (first.type == PointerType.Touch) {
                    touches[first.id] = first.position
                    host.colorTouch(surface, first.id, "down", first.position, first.uptimeMillis)
                }
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Initial)
                    for (change in event.changes) {
                        if (change.type != PointerType.Touch) continue
                        val phase = when {
                            change.changedToDownIgnoreConsumed() -> if (inside(change.position)) "down" else continue
                            change.id !in touches -> continue
                            change.pressed -> "move"
                            change.isConsumed -> "cancel"
                            else -> "up"
                        }
                        host.colorTouch(surface, change.id, phase, change.position, change.uptimeMillis)
                        if (change.pressed) touches[change.id] = change.position else touches.remove(change.id)
                    }
                    claim.claimed = claim.claimed || touches.size >= 2
                    if (claim.claimed) event.changes.forEach { it.consume() }
                    if (event.changes.none { it.pressed }) {
                        awaitPointerEvent(PointerEventPass.Final)
                        open = false
                        contact("up")
                        break
                    }
                }
            } finally {
                val now = SystemClock.uptimeMillis()
                touches.forEach { (id, point) -> host.colorTouch(surface, id, "cancel", point, now) }
                if (open) contact("up")
            }
        }
    }
}
