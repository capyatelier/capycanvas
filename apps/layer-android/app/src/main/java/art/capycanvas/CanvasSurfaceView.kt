package art.capycanvas

import android.content.Context
import android.hardware.input.InputManager
import android.os.Build
import android.os.SystemClock
import android.util.LongSparseArray
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.MotionPredictor
import android.view.PointerIcon
import android.view.Surface
import android.view.SurfaceHolder
import android.view.ViewConfiguration
import android.view.SurfaceView
import androidx.core.util.size
import org.json.JSONArray
import kotlin.math.cos
import kotlin.math.sin

/** Separate compositor layer, not a texture embedded in Compose's renderer. */
class CanvasSurfaceView(context: Context, private val host: CanvasHost,
    private val chromeHitTest: (Float, Float) -> Boolean = { _, _ -> false }) : SurfaceView(context), SurfaceHolder.Callback {
    private class Contact(val tool: Int, val button: Int, var x: Double, var y: Double)
    private val contacts = LongSparseArray<Contact>()
    private fun cancelContacts(time: Long = SystemClock.uptimeMillis() * 1_000_000L) {
        for (i in 0 until contacts.size) {
            val contact = contacts.valueAt(i)
            val samples = host.pointerBuffer(9)
            samples.fill(0.0, 0, 9)
            samples[0] = contact.x; samples[1] = contact.y
            samples[7] = time.toDouble(); samples[8] = 4.0
            host.pointer(contacts.keyAt(i), contact.tool, contact.button, samples, 9)
        }
        contacts.clear()
        predictor = null; predictionDevice = null
    }
    private var pickerHold: Runnable? = null
    private var pickerContact = -1
    private var pickerX=0f
    private var pickerY=0f
    private fun cancelPickerHold() { pickerHold?.let(::removeCallbacks);pickerHold=null;pickerContact=-1 }
    private fun pickerTouch(event: MotionEvent) {
        if(event.actionMasked==MotionEvent.ACTION_DOWN && event.getToolType(0)==MotionEvent.TOOL_TYPE_FINGER && !event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            cancelPickerHold();pickerContact=event.getPointerId(0);pickerX=event.x;pickerY=event.y
            val id=(event.deviceId.toLong().and(0xffffffffL) shl 16) or pickerContact.toLong()
            val metrics=resources.displayMetrics
            val offset=(metrics.ydpi*10f/25.4f).coerceIn(36f*metrics.density,64f*metrics.density)
            pickerHold=Runnable {
                pickerHold=null
                host.input(obj("type" to "color_picker_hold","id" to id,"position" to JSONArray(listOf(pickerX,pickerY)),"offset" to offset))
            }.also { postDelayed(it,ViewConfiguration.getLongPressTimeout().toLong()) }
        } else if(event.actionMasked==MotionEvent.ACTION_MOVE) {
            val index=event.findPointerIndex(pickerContact)
            val slop=ViewConfiguration.get(context).scaledTouchSlop
            if(index<0 || kotlin.math.hypot(event.getX(index)-pickerX,event.getY(index)-pickerY)>slop)cancelPickerHold()
        } else if(event.actionMasked in listOf(MotionEvent.ACTION_POINTER_DOWN,MotionEvent.ACTION_UP,MotionEvent.ACTION_POINTER_UP,MotionEvent.ACTION_CANCEL))cancelPickerHold()
    }
    private var attached = false
    private var predictor: MotionPredictor? = null
    private var predictionDevice: Int? = null
    private var predictionProbe: MotionPredictor? = null
    private val inputManager = context.getSystemService(InputManager::class.java)
    private val inputDevices = object : InputManager.InputDeviceListener {
        override fun onInputDeviceAdded(deviceId: Int) = refreshPredictionAvailability()
        override fun onInputDeviceChanged(deviceId: Int) = refreshPredictionAvailability()
        override fun onInputDeviceRemoved(deviceId: Int) {
            if (predictionDevice == deviceId) { predictor = null; predictionDevice = null }
            refreshPredictionAvailability()
        }
    }
    init {
        holder.addCallback(this)
        isFocusable = true
        isFocusableInTouchMode = true
        defaultFocusHighlightEnabled = false
        isLongClickable = false
        pointerIcon = PointerIcon.getSystemIcon(context, PointerIcon.TYPE_NULL)
        contentDescription = "Drawing canvas"
    }
    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        inputManager.registerInputDeviceListener(inputDevices, handler)
        refreshPredictionAvailability()
    }
    override fun onDetachedFromWindow() {
        if (Build.VERSION.SDK_INT >= 30) requestUnbufferedDispatch(InputDevice.SOURCE_CLASS_NONE)
        inputManager.unregisterInputDeviceListener(inputDevices)
        predictionProbe = null
        cancelPickerHold()
        cancelContacts()
        super.onDetachedFromWindow()
    }
    override fun onWindowFocusChanged(hasWindowFocus: Boolean) {
        super.onWindowFocusChanged(hasWindowFocus)
        if (!hasWindowFocus) { cancelPickerHold(); cancelContacts() }
        if (!hasWindowFocus && Build.VERSION.SDK_INT >= 30) requestUnbufferedDispatch(InputDevice.SOURCE_CLASS_NONE)
        if (hasWindowFocus) refreshPredictionAvailability()
    }
    internal fun refreshPredictionAvailability() {
        val available = if (Build.VERSION.SDK_INT >= 34) {
            val probe = predictionProbe ?: MotionPredictor(context).also { predictionProbe = it }
            inputManager.inputDeviceIds.any { id ->
                val device = inputManager.getInputDevice(id)
                device != null && device.supportsSource(InputDevice.SOURCE_STYLUS) &&
                    probe.isPredictionAvailable(id, InputDevice.SOURCE_STYLUS)
            }
        } else false
        host.updatePredictionAvailability(available)
    }
    override fun onResolvePointerIcon(event: MotionEvent, pointerIndex: Int): PointerIcon? {
        // Compose controls are virtual siblings above this full-window native
        // view. Let their owner resolve the icon instead of hiding it underneath.
        if (chromeHitTest(event.getX(pointerIndex), event.getY(pointerIndex))) return null
        return super.onResolvePointerIcon(event, pointerIndex)
    }
    override fun surfaceCreated(holder: SurfaceHolder) = Unit
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (width <= 0 || height <= 0) return
        val density = resources.displayMetrics.density
        if (!attached) {
            host.hdr.bindSurface(this)
            val requested = display?.supportedModes?.maxOfOrNull { it.refreshRate }?.coerceAtMost(120f) ?: 60f
            if (Build.VERSION.SDK_INT >= 30) holder.surface.setFrameRate(requested, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT)
            host.attach(holder.surface, width, height, density, display?.refreshRate ?: requested)
            attached = true
        } else host.resize(width, height, density)
    }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        cancelPickerHold()
        cancelContacts()
        host.hdr.unbindSurface(this)
        if (attached) { host.detach(); attached = false }
    }
    override fun onTouchEvent(event: MotionEvent): Boolean {
        pickerTouch(event)
        if (event.actionMasked == MotionEvent.ACTION_CANCEL) {
            // Compose can synthesize cancellation with device/id/source zero
            // and TOOL_TYPE_UNKNOWN. End the captured native contacts, never
            // reinterpret that empty record as a new pen's cancellation.
            val time = if (Build.VERSION.SDK_INT >= 34) event.eventTimeNanos else event.eventTime * 1_000_000L
            cancelContacts(time)
            return true
        }
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            requestUnbufferedDispatch(event)
            parent.requestDisallowInterceptTouchEvent(true)
            requestFocus()
        }
        val action = event.actionMasked
        val all = action == MotionEvent.ACTION_MOVE
        val first = if (all) 0 else event.actionIndex
        val last = if (all) event.pointerCount - 1 else first
        for (i in first..last) {
            val phase = when (action) {
                MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> 1
                MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> if (Build.VERSION.SDK_INT >= 33 && event.flags and MotionEvent.FLAG_CANCELED != 0) 4 else 3
                else -> 2
            }
            send(event, i, phase, action == MotionEvent.ACTION_MOVE)
        }
        if (!host.nativePredictionEnabled) {
            predictor = null; predictionDevice = null
        } else if (Build.VERSION.SDK_INT >= 34 && event.isFromSource(InputDevice.SOURCE_STYLUS)) {
            if (action == MotionEvent.ACTION_DOWN && (predictor == null || predictionDevice != event.deviceId)) {
                val native = MotionPredictor(context)
                predictor = native.takeIf { it.isPredictionAvailable(event.deviceId, event.source) }
                predictionDevice = event.deviceId
                refreshPredictionAvailability()
            }
            predictor?.let { native ->
                try {
                    native.record(event)
                    if (action == MotionEvent.ACTION_MOVE) {
                        val target = System.nanoTime() + (1_000_000_000 / (display?.refreshRate ?: 60f)).toLong()
                        native.predict(target)?.let { predicted ->
                            try { send(predicted, 0, 2, true, predicted = true) } finally { predicted.recycle() }
                        }
                    }
                } catch (_: IllegalArgumentException) {
                    // Device switches/cancelled system gestures can invalidate
                    // a predictor; raw input and core feedback still work.
                    predictor = null
                }
            }
        }
        return true
    }
    override fun onHoverEvent(event: MotionEvent): Boolean {
        // Hover otherwise waits for the UI vsync before reaching our separate
        // canvas Looper. The touch-only request on pen-down cannot cover it.
        if (Build.VERSION.SDK_INT >= 30) requestUnbufferedDispatch(
            if (event.actionMasked == MotionEvent.ACTION_HOVER_EXIT) InputDevice.SOURCE_CLASS_NONE
            else InputDevice.SOURCE_CLASS_POINTER
        )
        if (event.actionMasked == MotionEvent.ACTION_HOVER_EXIT) {
            host.chrome(obj("kind" to "leave", "touch" to false))
            host.input(obj("type" to "cursor_leave"))
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
    private fun send(event: MotionEvent, index: Int, phase: Int, history: Boolean, predicted: Boolean = false) {
        val indirect = event.isFromSource(InputDevice.SOURCE_MOUSE) || event.isFromSource(InputDevice.SOURCE_TOUCHPAD)
        val tool = when (event.getToolType(index)) {
            MotionEvent.TOOL_TYPE_STYLUS -> 0
            MotionEvent.TOOL_TYPE_MOUSE -> 1
            MotionEvent.TOOL_TYPE_ERASER -> 2
            MotionEvent.TOOL_TYPE_FINGER -> if (indirect) 1 else 3
            else -> if (indirect) 1 else 0
        }
        val button = if (event.buttonState and (MotionEvent.BUTTON_TERTIARY or MotionEvent.BUTTON_SECONDARY) != 0 && tool == 1) 1 else 0
        val count = if (history) event.historySize else 0
        val used = (count + 1) * 9
        val samples = host.pointerBuffer(used)
        packPointerSamples(event, index, phase, count, tool == 1, samples)
        val id = (event.deviceId.toLong().and(0xffffffffL) shl 16) or event.getPointerId(index).toLong()
        if (!predicted) {
            val x = samples[count * 9]; val y = samples[count * 9 + 1]
            when (phase) {
                1 -> contacts.put(id, Contact(tool, button, x, y))
                2 -> contacts[id]?.let { it.x = x; it.y = y }
                3, 4 -> contacts.remove(id)
            }
        }
        host.pointer(id, tool, button, samples, used, predicted)
    }
}

/** Shared packing for real, historical and Android-predicted samples. */
internal fun packPointerSamples(event: MotionEvent, index: Int, phase: Int, historySize: Int,
    mouse: Boolean, samples: DoubleArray) {
    for (h in 0..historySize) {
        val historical = h < historySize
        fun axis(axis: Int): Float = if (historical) event.getHistoricalAxisValue(axis, index, h) else event.getAxisValue(axis, index)
        val tilt = axis(MotionEvent.AXIS_TILT)
        val orientation = axis(MotionEvent.AXIS_ORIENTATION)
        val time = if (Build.VERSION.SDK_INT >= 34) {
            if (historical) event.getHistoricalEventTimeNanos(h) else event.eventTimeNanos
        } else if (historical) event.getHistoricalEventTime(h) * 1_000_000L else event.eventTime * 1_000_000L
        val offset = h * 9
        samples[offset] = (if (historical) event.getHistoricalX(index, h) else event.getX(index)).toDouble()
        samples[offset + 1] = (if (historical) event.getHistoricalY(index, h) else event.getY(index)).toDouble()
        samples[offset + 2] = if (mouse) 1.0 else (if (historical) event.getHistoricalPressure(index, h) else event.getPressure(index)).toDouble()
        // Android azimuth points toward the tip; shared tilt points toward the barrel.
        samples[offset + 3] = (-sin(orientation) * tilt).toDouble()
        samples[offset + 4] = (cos(orientation) * tilt).toDouble()
        samples[offset + 5] = 0.0 // Android stylus orientation is tilt azimuth, not barrel twist.
        samples[offset + 6] = axis(MotionEvent.AXIS_DISTANCE).toDouble()
        samples[offset + 7] = time.toDouble()
        samples[offset + 8] = (if (historical) { if (phase == 0) 0 else 2 } else phase).toDouble()
    }
}
