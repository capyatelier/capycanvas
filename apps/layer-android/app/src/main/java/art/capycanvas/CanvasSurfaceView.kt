package art.capycanvas

import android.content.Context
import android.os.Build
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import org.json.JSONArray
import kotlin.math.cos
import kotlin.math.sin

/** Separate compositor layer, not a texture embedded in Compose's renderer. */
class CanvasSurfaceView(context: Context, private val host: CanvasHost) : SurfaceView(context), SurfaceHolder.Callback {
    private var attached = false
    init {
        holder.addCallback(this)
        isFocusable = true
        isFocusableInTouchMode = true
        isLongClickable = false
        pointerIcon = PointerIcon.getSystemIcon(context, PointerIcon.TYPE_NULL)
        contentDescription = "Drawing canvas"
    }
    override fun surfaceCreated(holder: SurfaceHolder) = Unit
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (width <= 0 || height <= 0) return
        val density = resources.displayMetrics.density
        if (!attached) {
            val requested = display?.supportedModes?.maxOfOrNull { it.refreshRate }?.coerceAtMost(120f) ?: 60f
            if (Build.VERSION.SDK_INT >= 30) holder.surface.setFrameRate(requested, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT)
            host.attach(holder.surface, width, height, density, display?.refreshRate ?: requested)
            attached = true
        } else host.resize(width, height, density)
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (attached) { host.detach(); attached = false }
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            requestUnbufferedDispatch(event)
            parent.requestDisallowInterceptTouchEvent(true)
            requestFocus()
        }
        val action = event.actionMasked
        val indices = if (action == MotionEvent.ACTION_MOVE || action == MotionEvent.ACTION_CANCEL) {
            (0 until event.pointerCount).toList()
        } else listOf(event.actionIndex)
        indices.forEach { i ->
            val phase = when (action) {
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> 1
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> if (Build.VERSION.SDK_INT >= 33 && event.flags and MotionEvent.FLAG_CANCELED != 0) 4 else 3
                MotionEvent.ACTION_CANCEL -> 4
                else -> 2
            }
            send(event, i, phase, action == MotionEvent.ACTION_MOVE)
        }
        return true
    }
    override fun onHoverEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_HOVER_EXIT) {
            host.chrome(obj("kind" to "leave", "touch" to false))
            send(event, 0, 4, false)
        } else {
            host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(event.x / resources.displayMetrics.density, event.y / resources.displayMetrics.density))))
            send(event, 0, 0, true)
        }
        return true
    }
    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_SCROLL) {
            // Use the same key-modified scroll semantics through a native query
            // boundary, rather than implementing camera math in Kotlin.
            host.scroll(event.x, event.y, event.getAxisValue(MotionEvent.AXIS_HSCROLL),
                -event.getAxisValue(MotionEvent.AXIS_VSCROLL), event.metaState and KeyEvent.META_CTRL_ON != 0, event.metaState and KeyEvent.META_SHIFT_ON != 0)
            return true
        }
        return super.onGenericMotionEvent(event)
    }
    private fun send(event: MotionEvent, index: Int, phase: Int, history: Boolean) {
        val tool = when (event.getToolType(index)) {
            MotionEvent.TOOL_TYPE_MOUSE -> 1
            MotionEvent.TOOL_TYPE_ERASER -> 2
            MotionEvent.TOOL_TYPE_FINGER -> 3
            else -> 0
        }
        val button = if (event.buttonState and (MotionEvent.BUTTON_TERTIARY or MotionEvent.BUTTON_SECONDARY) != 0 && tool == 1) 1 else 0
        val count = if (history) event.historySize else 0
        val samples = DoubleArray((count + 1) * 9)
        for (h in 0..count) {
            val historical = h < count
            fun axis(axis: Int): Float = if (historical) event.getHistoricalAxisValue(axis, index, h) else event.getAxisValue(axis, index)
            val tilt = axis(MotionEvent.AXIS_TILT)
            val orientation = axis(MotionEvent.AXIS_ORIENTATION)
            val time = if (historical) event.getHistoricalEventTime(h) * 1_000_000L else event.eventTime * 1_000_000L
            val offset = h * 9
            samples[offset] = (if (historical) event.getHistoricalX(index, h) else event.getX(index)).toDouble()
            samples[offset + 1] = (if (historical) event.getHistoricalY(index, h) else event.getY(index)).toDouble()
            samples[offset + 2] = if (tool == 1) 1.0 else (if (historical) event.getHistoricalPressure(index, h) else event.getPressure(index)).toDouble()
            samples[offset + 3] = (sin(orientation) * tilt).toDouble()
            samples[offset + 4] = (-cos(orientation) * tilt).toDouble()
            samples[offset + 5] = 0.0 // Android stylus orientation is tilt azimuth, not barrel twist.
            samples[offset + 6] = axis(MotionEvent.AXIS_DISTANCE).toDouble()
            samples[offset + 7] = time.toDouble()
            samples[offset + 8] = (if (historical) { if (phase == 0) 0 else 2 } else phase).toDouble()
        }
        val id = (event.deviceId.toLong().and(0xffffffffL) shl 16) or event.getPointerId(index).toLong()
        host.pointer(id, tool, button, samples)
    }
}
