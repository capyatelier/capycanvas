package art.capycanvas

import android.app.Instrumentation
import android.graphics.Bitmap
import android.os.Bundle
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.LockSupport
import kotlin.math.*

// OS two-finger replay for displayed-frame measurements; never captures during motion.
internal fun runPinchBenchmark(instrumentation: Instrumentation, args: Bundle, host: CanvasHost,
    output: File, label: String, cx: Double, cy: Double) {
    fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    fun state(): JSONObject {
        native { Unit }
        var value = JSONObject()
        instrumentation.runOnMainSync { value = JSONObject(host.snapshot!!.toString()).getJSONObject("state") }
        return value
    }
    fun measurements(reset: Boolean): JSONObject {
        val ready = CountDownLatch(1)
        var result = JSONObject()
        host.measurements(reset) { result = it; ready.countDown() }
        check(ready.await(30, TimeUnit.SECONDS))
        return result
    }
    fun screenshot(name: String) {
        instrumentation.uiAutomation.takeScreenshot()?.let { b ->
            File(output, "$label-$name.png").outputStream().use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }
            b.recycle()
        }
    }
    val before = state()
    val revision = before.getJSONObject("document_file").getLong("revision")
    File(output, "$label-info.json").writeText(obj("state" to before,
        "renderer" to native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) },
        "display" to native { JSONObject(Native.displayStatus(it)) }).toString(2))
    screenshot("before")
    File(output, "$label-ready").writeText("ready")
    instrumentation.sendStatus(0, Bundle().apply { putString("stream", "PINCH_READY $label\n") })
    if (args.getString("waitForTrace") == "true") {
        val deadline = SystemClock.uptimeMillis() + 180000
        while (!File(output, "$label-go").isFile) {
            check(SystemClock.uptimeMillis() < deadline)
            SystemClock.sleep(50)
        }
    }
    val duration = args.getString("durationMs", "10000")!!.toInt()
    val speed = args.getString("speed", "1")!!.toDouble()
    val properties = Array(2) { index -> MotionEvent.PointerProperties().apply {
        id = index + 11; toolType = MotionEvent.TOOL_TYPE_FINGER
    } }
    val coordinates = Array(2) { MotionEvent.PointerCoords().apply { pressure = 1f; size = .1f } }
    val down = SystemClock.uptimeMillis()
    fun event(action: Int, count: Int, radius: Double) {
        for (i in 0..1) {
            coordinates[i].x = (cx + (if (i == 0) -radius else radius)).toFloat() + host.surfaceOrigin.x
            coordinates[i].y = cy.toFloat() + host.surfaceOrigin.y
        }
        val e = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, count,
            properties, coordinates, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
        try { check(instrumentation.uiAutomation.injectInputEvent(e, false)) } finally { e.recycle() }
    }
    val samples = JSONArray()
    measurements(true)
    val started = System.nanoTime()
    event(MotionEvent.ACTION_DOWN, 1, 220.0)
    event(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, 220.0)
    for (i in 1..duration / 5) {
        val delay = started + i * 5_000_000L - System.nanoTime()
        if (delay > 0) LockSupport.parkNanos(delay)
        // exp(+/- 0.4) around fit crosses the 12.5% and 25% LOD neighborhoods.
        val radius = 220.0 * exp(.4 * sin(i * .005 * speed * 2 * PI))
        event(MotionEvent.ACTION_MOVE, 2, radius)
        if (i % 20 == 0) samples.put(state().getJSONObject("camera"))
    }
    event(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, 220.0)
    event(MotionEvent.ACTION_UP, 1, 220.0)
    File(output, "$label-measurements.json").writeText(measurements(false).toString())
    SystemClock.sleep(1000)
    screenshot("after")
    val after = state()
    check(after.getJSONObject("document_file").getLong("revision") == revision) { "Pinch modified drawing" }
    check(host.failure == null && host.actionError == null)
    File(output, "$label-complete.json").writeText(obj("state_after" to after, "cameras" to samples,
        "renderer" to native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) },
        "display" to native { JSONObject(Native.displayStatus(it)) }).toString(2))
}
