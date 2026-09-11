package art.capycanvas

import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.test.core.app.ActivityScenario
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Run alone in a fresh instrumentation process to exercise the cold renderer. */
class AndroidStartupTest {
    @Test fun drawingAndNavigationWorkBeforeSpeculativeShadersFinish() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var host: CanvasHost
            scenario.onActivity { host = it.host }
            fun snapshot(): JSONObject? {
                var value: JSONObject? = null
                scenario.onActivity {
                    assertNull(host.failure)
                    value = host.snapshot?.let { s -> JSONObject(s.toString()) }
                }
                return value
            }
            fun waitFor(condition: (JSONObject) -> Boolean): JSONObject {
                val deadline = SystemClock.uptimeMillis() + 30_000
                while (SystemClock.uptimeMillis() < deadline) {
                    snapshot()?.let { if (condition(it)) return it }
                    SystemClock.sleep(10)
                }
                throw AssertionError("Startup did not become ready")
            }
            val ready = waitFor { it.optBoolean("brush_ready") }
            assertTrue(ready.getBoolean("canvas_ready"))
            // Timing thresholds stay in the device benchmark. This assertion
            // verifies that ready drawing is not gated on all shaders.
            org.junit.Assume.assumeFalse("Device completed speculative work before the test could observe it", ready.getBoolean("shaders_ready"))
            fun canvas(view: View): CanvasSurfaceView? {
                if (view is CanvasSurfaceView) return view
                if (view is ViewGroup) for (i in 0 until view.childCount) canvas(view.getChildAt(i))?.let { return it }
                return null
            }
            val down = SystemClock.uptimeMillis()
            fun event(action: Int, positions: List<Pair<Float, Float>>, tool: Int) {
                scenario.onActivity { activity ->
                    val view = canvas(activity.window.decorView)!!
                    val properties = positions.indices.map { MotionEvent.PointerProperties().apply { id = it; toolType = tool } }.toTypedArray()
                    val coordinates = positions.map { (x, y) -> MotionEvent.PointerCoords().apply {
                        this.x = x * view.width; this.y = y * view.height; pressure = 1f; size = 1f
                    } }.toTypedArray()
                    val motion = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, positions.size, properties, coordinates,
                        0, 0, 1f, 1f, 0, 0, if (tool == MotionEvent.TOOL_TYPE_STYLUS) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
                    view.dispatchTouchEvent(motion)
                    motion.recycle()
                }
            }
            event(MotionEvent.ACTION_DOWN, listOf(.45f to .5f), MotionEvent.TOOL_TYPE_STYLUS)
            for (i in 1..12) {
                event(MotionEvent.ACTION_MOVE, listOf((.45f + i * .005f) to .5f), MotionEvent.TOOL_TYPE_STYLUS)
                SystemClock.sleep(8)
            }
            event(MotionEvent.ACTION_UP, listOf(.51f to .5f), MotionEvent.TOOL_TYPE_STYLUS)
            val painted = waitFor { it.getJSONObject("state").array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
            android.util.Log.i("CapyStartupTest", "painted_while_compiling=${!painted.getBoolean("shaders_ready")}")
            val zoom = painted.getJSONObject("state").getJSONObject("camera").getDouble("zoom")
            event(MotionEvent.ACTION_DOWN, listOf(.45f to .5f), MotionEvent.TOOL_TYPE_FINGER)
            event(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(.45f to .5f, .55f to .5f), MotionEvent.TOOL_TYPE_FINGER)
            for (i in 1..20) {
                event(MotionEvent.ACTION_MOVE, listOf((.45f - i * .001f) to .5f, (.55f + i * .001f) to .5f), MotionEvent.TOOL_TYPE_FINGER)
                SystemClock.sleep(8)
            }
            event(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(.43f to .5f, .57f to .5f), MotionEvent.TOOL_TYPE_FINGER)
            event(MotionEvent.ACTION_UP, listOf(.43f to .5f), MotionEvent.TOOL_TYPE_FINGER)
            waitFor { it.getJSONObject("state").getJSONObject("camera").getDouble("zoom") > zoom * 1.1 }
            waitFor { it.getBoolean("shaders_ready") }
            val done = CountDownLatch(1)
            var report: JSONObject? = null
            host.measurements { data -> report = data; done.countDown() }
            assertTrue(done.await(10, TimeUnit.SECONDS))
            val data = report!!
            android.util.Log.i("CapyStartupTest", obj("startup_boot_ns" to data.getJSONArray("startup_boot_ns"),
                "frames" to data.array("frames").length(), "inputs" to data.array("inputs").length()).toString())
            val times = data.getJSONArray("startup_boot_ns")
            assertTrue(times.getLong(0) > 0)
            assertTrue(times.getLong(0) <= times.getLong(1))
            assertTrue(times.getLong(1) <= times.getLong(2))
            assertTrue(times.getLong(2) < times.getLong(3))
        }
    }
}
