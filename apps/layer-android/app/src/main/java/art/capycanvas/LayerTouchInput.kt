package art.capycanvas

import android.os.SystemClock
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONArray

internal val layerTouchSurfaces = AtomicLong()

internal fun Modifier.layerTouchInput(host: CanvasHost, surface: Long, epoch: Long,
    origin: () -> Offset, rows: Map<Long, Rect>, claim: () -> Unit): Modifier = pointerInput(host, surface, epoch) {
    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        if (first.type != PointerType.Touch) return@awaitEachGesture
        val contacts = mutableMapOf<PointerId, Pair<Long, Offset>>()
        fun send(id: PointerId, phase: String, row: Long, point: Offset, time: Long) {
            host.input(obj("type" to "layer_touch", "surface" to surface, "id" to id.value, "phase" to phase,
                "position" to JSONArray().put(point.x).put(point.y), "row" to row, "time_ns" to time * 1_000_000L))
        }
        val firstPoint = first.position + origin()
        val firstRow = rows.entries.firstOrNull { it.value.contains(firstPoint) }?.key ?: return@awaitEachGesture
        contacts[first.id] = firstRow to firstPoint
        send(first.id, "down", firstRow, firstPoint, first.uptimeMillis)
        var claimed = false
        try {
            while (contacts.isNotEmpty()) {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                for (change in event.changes) {
                    if (change.type != PointerType.Touch) continue
                    val point = change.position + origin()
                    if (change.pressed && !change.previousPressed) {
                        val row = rows.entries.firstOrNull { it.value.contains(point) }?.key ?: continue
                        contacts[change.id] = row to point
                        send(change.id, "down", row, point, change.uptimeMillis)
                    } else {
                        val row = contacts[change.id]?.first ?: continue
                        send(change.id, if (change.pressed) "move" else "up", row, point, change.uptimeMillis)
                        if (change.pressed) contacts[change.id] = row to point else contacts.remove(change.id)
                    }
                }
                if (!claimed && contacts.size >= 2) { claimed = true; claim() }
                if (claimed) event.changes.forEach { it.consume() }
            }
        } finally {
            val now = SystemClock.uptimeMillis()
            contacts.forEach { (id, contact) -> send(id, "cancel", contact.first, contact.second, now) }
        }
    }
}
