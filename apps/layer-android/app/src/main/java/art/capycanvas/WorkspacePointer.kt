package art.capycanvas

import android.content.Context
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.View
import androidx.activity.compose.ReportDrawnWhen
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.AbstractComposeView
import androidx.compose.ui.platform.LocalView

internal data class TextCursorRegion(val bounds: Rect, val z: Int, val root: View, val bubble: Int?)

internal class CanvasContent(context: Context, private val host: CanvasHost) : AbstractComposeView(context) {
    @Composable override fun Content() {
        ReportDrawnWhen { host.snapshot?.optBoolean("brush_ready") == true }
        CapyApp(host)
    }

    override fun onResolvePointerIcon(event: MotionEvent, pointerIndex: Int): PointerIcon? {
        val point = Offset(event.getX(pointerIndex), event.getY(pointerIndex))
        val dock = host.workspaceInteraction
        val mouse = event.getToolType(pointerIndex) == MotionEvent.TOOL_TYPE_MOUSE
        fun icon(type: Int) = PointerIcon.getSystemIcon(context, type)
        fun hover(type: Int) = if (!mouse && (type == PointerIcon.TYPE_ARROW || type == PointerIcon.TYPE_GRAB || type == PointerIcon.TYPE_GRABBING)) null else icon(type)
        dock?.activePointerIcon()?.let { return if (it == PointerIcon.TYPE_GRAB) hover(it) else icon(it) }
        if (host.palettes.contactHeld && host.palettes.drag?.started == true) return icon(PointerIcon.TYPE_GRABBING)
        val local = point - (dock?.origin ?: Offset.Zero)
        val bubble = dock?.bubbles?.pointerTarget(local)
        val z = dock?.chromeRegions?.values?.filter { it.bounds.contains(local) }?.maxOfOrNull { it.z } ?: 0
        if (host.textCursorRegions.values.any { it.root === rootView && it.bubble == bubble?.id &&
            (bubble != null || it.z >= z) && it.bounds.contains(point) }) return super.onResolvePointerIcon(event, pointerIndex)
        if (bubble != null) return icon(if (!mouse || bubble.view.objectOrNull("stamp") != null) PointerIcon.TYPE_NULL else PointerIcon.TYPE_ARROW)
        if (dock != null && dock.bounds.contains(point)) {
            dock.hit(local)?.let { region ->
                val cursor = if (region.holdToDrag) PointerIcon.TYPE_ARROW else region.cursor
                return hover(cursor)
            }
            if (dock.chromeRegions.values.any { it.bounds.contains(local) }) return hover(PointerIcon.TYPE_ARROW)
            return dock.canvas?.pointerIconAt(local.x, local.y) ?: hover(PointerIcon.TYPE_ARROW)
        }
        return hover(PointerIcon.TYPE_ARROW)
    }
}

@Composable internal fun Modifier.textCursorRegion(enabled: Boolean = true): Modifier {
    if (!enabled || !LocalWorkspaceVisible.current) return this
    val host = LocalCanvasHost.current
    val view = LocalView.current
    val token = remember { Any() }
    val z = LocalWorkspaceZ.current
    val bubble = LocalBubbleOwner.current
    DisposableEffect(host, token, view, bubble) {
        onDispose { host.textCursorRegions.remove(token) }
    }
    SideEffect { host.textCursorRegions[token]?.let { if (it.z != z) host.textCursorRegions[token] = it.copy(z = z) } }
    return onGloballyPositioned { host.textCursorRegions[token] = TextCursorRegion(it.boundsInRoot(), z, view.rootView, bubble) }
}
