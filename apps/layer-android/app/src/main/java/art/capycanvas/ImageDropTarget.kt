package art.capycanvas

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.draganddrop.dragAndDropTarget
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draganddrop.DragAndDropEvent
import androidx.compose.ui.draganddrop.DragAndDropTarget
import androidx.compose.ui.draganddrop.toAndroidDragEvent
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import org.json.JSONObject

/** Root fallback participates in Compose's hit testing alongside layer rows.
 * An embedded SurfaceView would otherwise intercept drops meant for Compose. */
@Composable internal fun Modifier.imageCanvasDropTarget(host: CanvasHost, chromeHit: (Offset) -> Boolean): Modifier {
    val activity = LocalActivity.current
    var origin by remember { mutableStateOf(Offset.Zero) }
    val hit by rememberUpdatedState(chromeHit)
    val target = remember(host, activity) { object : DragAndDropTarget {
        override fun onDrop(event: DragAndDropEvent): Boolean {
            val native = event.toAndroidDragEvent()
            val point = Offset(native.x, native.y) - origin
            if (hit(point)) return false
            return activity?.let { host.documents.images.drop(it, native, obj("x" to point.x, "y" to point.y), null) } ?: false
        }
    } }
    return onGloballyPositioned { origin = it.boundsInRoot().topLeft }
        .dragAndDropTarget({ host.documents.images.accepts(it.toAndroidDragEvent()) }, target)
}

/** Retained rows and drawer copies use the same shared external-drop resolver. */
@Composable internal fun Modifier.imageDropTarget(host: CanvasHost, id: Long): Modifier {
    val activity = LocalActivity.current
    val colors = LocalPalette.current
    var bounds by remember { mutableStateOf(Rect.Zero) }
    var position by remember { mutableStateOf<String?>(null) }
    var serial by remember { mutableIntStateOf(0) }
    val target = remember(host, id, activity) { object : DragAndDropTarget {
        fun fraction(event: DragAndDropEvent) = ((event.toAndroidDragEvent().y - bounds.top) / bounds.height.coerceAtLeast(1f)).coerceIn(0f, 1f)
        fun update(event: DragAndDropEvent) {
            val request = ++serial
            host.query(obj("type" to "image_layer_drop", "target" to id, "fraction" to fraction(event))) {
                if (serial == request) position = (it as? JSONObject)?.let { hint -> if (hint.isNull("position")) null else hint.getString("position") }
            }
        }
        override fun onEntered(event: DragAndDropEvent) = update(event)
        override fun onMoved(event: DragAndDropEvent) = update(event)
        override fun onExited(event: DragAndDropEvent) { serial++; position = null }
        override fun onEnded(event: DragAndDropEvent) { serial++; position = null }
        override fun onDrop(event: DragAndDropEvent): Boolean {
            serial++; position = null
            return activity?.let { host.documents.images.drop(it, event.toAndroidDragEvent(), null, obj("target" to id, "fraction" to fraction(event))) } ?: false
        }
    } }
    DisposableEffect(id) { onDispose { serial++; position = null } }
    return onGloballyPositioned { bounds = it.boundsInRoot() }
        .dragAndDropTarget({ host.documents.images.accepts(it.toAndroidDragEvent()) }, target)
        .drawWithContent {
            drawContent()
            when (position) {
                "above" -> drawLine(colors.accent, Offset.Zero, Offset(size.width, 0f), 2 * density)
                "below" -> drawLine(colors.accent, Offset(0f, size.height), Offset(size.width, size.height), 2 * density)
                "into" -> drawRect(colors.accent, style = Stroke(2 * density))
            }
        }
}
