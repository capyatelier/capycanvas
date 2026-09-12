package art.capycanvas

import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import android.view.FrameMetrics
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.Window
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sin

/** Opt-in measurement on the real Android frame clock, without Compose test
 * clock advancement or wait-for-idle between pointer samples. */
class AndroidWorkspacePerformanceTest {
    @Test fun continuousDragFrameTiming() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("workspaceBenchmark") == "true")
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var host: CanvasHost
            lateinit var owner: ViewRootForTest
            lateinit var window: Window
            fun root(view: View): ViewRootForTest? {
                if (view is ViewRootForTest) return view
                if (view is ViewGroup) for (index in 0 until view.childCount) root(view.getChildAt(index))?.let { return it }
                return null
            }
            scenario.onActivity { host = it.host; owner = root(it.window.decorView)!!; window = it.window }
            fun waitFor(condition: () -> Boolean) {
                val deadline = SystemClock.uptimeMillis() + 60_000
                do {
                    var ready = false
                    scenario.onActivity { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
                    if (ready) return
                    SystemClock.sleep(10)
                } while (SystemClock.uptimeMillis() < deadline)
                fail("Native workspace did not settle")
            }
            fun action(value: JSONObject) {
                val done = CountDownLatch(1)
                scenario.onActivity { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
                assertTrue(done.await(10, TimeUnit.SECONDS))
            }
            fun report(reset: Boolean = false): JSONObject {
                val done = CountDownLatch(1)
                var result = JSONObject()
                host.measurements(reset) { result = it; done.countDown() }
                assertTrue(done.await(10, TimeUnit.SECONDS))
                return result
            }
            fun find(node: SemanticsNode, tag: String): SemanticsNode? =
                if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node
                else node.children.firstNotNullOfOrNull { find(it, tag) }
            fun bounds(tag: String): androidx.compose.ui.geometry.Rect {
                var result = androidx.compose.ui.geometry.Rect.Zero
                scenario.onActivity { result = find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)!!.boundsInRoot }
                return result
            }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true }
            var saved = JSONObject()
            scenario.onActivity { saved = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString()) }
            val fixture = JSONObject(saved.toString())
            fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
                "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon")
            fixture.getJSONObject("layout").apply {
                put("bands", JSONArray(listOf(
                    obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                    obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties")))))
                put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray())
                put("next_id", maxOf(44, getInt("next_id")))
            }
            fixture.put("zen_mode", false)
            val measuring = AtomicBoolean(false)
            val durations = mutableListOf<Long>()
            val frames = HandlerThread("workspace-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
                if (measuring.get()) synchronized(durations) { durations.add(metrics.getMetric(FrameMetrics.TOTAL_DURATION)) }
            }
            window.addOnFrameMetricsAvailableListener(listener, Handler(frames.looper))
            try {
                for (mouse in listOf(true, false)) for (mode in listOf("attached", "floating", "destination")) {
                    action(obj("type" to "restore_workspace", "workspace" to fixture))
                    SystemClock.sleep(300)
                    val workspace = bounds("workspace")
                    val source = bounds(when (mode) {
                        "attached" -> "tab-tool_settings"
                        "floating" -> "group-grip-41"
                        else -> "tab-layers"
                    }).center
                    val first = bounds("tab-brushes").center
                    val last = bounds("tab-sizes").center
                    val down = SystemClock.uptimeMillis()
                    fun event(action: Int, point: Offset) {
                        scenario.onActivity {
                            val properties = arrayOf(MotionEvent.PointerProperties().apply {
                                id = 0; toolType = if (mouse) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
                            })
                            val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f })
                            val motion = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords,
                                0, if (mouse && action != MotionEvent.ACTION_CANCEL) MotionEvent.BUTTON_PRIMARY else 0,
                                1f, 1f, 0, 0, if (mouse) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_TOUCHSCREEN, 0)
                            owner.view.dispatchTouchEvent(motion); motion.recycle()
                        }
                    }
                    event(MotionEvent.ACTION_DOWN, source)
                    try {
                        event(MotionEvent.ACTION_MOVE, if (mode == "attached") first else workspace.center)
                        SystemClock.sleep(150)
                        report(reset = true)
                        synchronized(durations) { durations.clear() }
                        measuring.set(true)
                        val start = SystemClock.uptimeMillis()
                        var samples = 0
                        do {
                            val elapsed = SystemClock.uptimeMillis() - start
                            val progress = (sin(elapsed * Math.PI / 500) * .5 + .5).toFloat()
                            val point = if (mode == "floating") Offset(workspace.center.x + (progress - .5f) * workspace.width * .25f, workspace.center.y)
                                else Offset(first.x + (last.x - first.x) * progress, first.y)
                            event(MotionEvent.ACTION_MOVE, point)
                            samples++
                            SystemClock.sleep(8)
                        } while (SystemClock.uptimeMillis() - start < 2500)
                        measuring.set(false)
                        val elapsed = SystemClock.uptimeMillis() - start
                        val timings = synchronized(durations) { durations.sorted() }
                        assertTrue("Android must render while dragging", timings.isNotEmpty())
                        fun percentile(fraction: Double) = timings[((timings.size - 1) * fraction).toInt()] / 1_000_000.0
                        val metrics = report()
                        val result = obj("mouse" to mouse, "mode" to mode, "elapsed_ms" to elapsed, "inputs" to samples,
                            "frames" to timings.size, "frame_p50_ms" to percentile(.5), "frame_p95_ms" to percentile(.95),
                            "frames_over_16ms" to timings.count { it > 16_666_667 },
                            "snapshot_attempts" to metrics.getLong("snapshot_attempts"), "snapshots" to metrics.getLong("snapshots_published"))
                        Log.i("CapyDragPerf", result.toString())
                    } finally {
                        measuring.set(false)
                        event(MotionEvent.ACTION_CANCEL, source)
                    }
                    action(obj("type" to "close_settings"))
                    waitFor { host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").array("floating").length() == 0 }
                }
            } finally {
                measuring.set(false)
                window.removeOnFrameMetricsAvailableListener(listener)
                frames.quitSafely()
                action(obj("type" to "restore_workspace", "workspace" to saved))
                waitFor { host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString() == saved.toString() }
            }
        }
    }
}
