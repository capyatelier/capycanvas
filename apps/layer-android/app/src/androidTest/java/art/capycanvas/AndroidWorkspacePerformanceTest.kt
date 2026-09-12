package art.capycanvas

import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
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
            data class Frame(val duration: Long, val deadline: Long, val vsync: Long)
            val durations = mutableListOf<Frame>()
            val drawnRevisions = mutableSetOf<Long>()
            var lostMetrics = 0
            val frames = HandlerThread("workspace-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
                if (measuring.get()) synchronized(durations) {
                    durations.add(Frame(metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                        metrics.getMetric(FrameMetrics.DEADLINE), metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP)))
                    lostMetrics += dropped
                }
            }
            val drawListener = android.view.ViewTreeObserver.OnDrawListener {
                if (measuring.get()) host.workspaceGeometry?.revision?.let { drawnRevisions.add(it) }
            }
            scenario.onActivity { owner.view.viewTreeObserver.addOnDrawListener(drawListener) }
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
                    fun eventOnMain(action: Int, point: Offset) {
                        val properties = arrayOf(MotionEvent.PointerProperties().apply {
                            id = 0; toolType = if (mouse) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
                        })
                        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f })
                        val motion = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords,
                            0, if (mouse && action != MotionEvent.ACTION_CANCEL) MotionEvent.BUTTON_PRIMARY else 0,
                            1f, 1f, 0, 0, if (mouse) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_TOUCHSCREEN, 0)
                        owner.view.dispatchTouchEvent(motion); motion.recycle()
                    }
                    fun event(action: Int, point: Offset) = scenario.onActivity { eventOnMain(action, point) }
                    event(MotionEvent.ACTION_DOWN, source)
                    try {
                        event(MotionEvent.ACTION_MOVE, if (mode == "attached") first else workspace.center)
                        SystemClock.sleep(750)
                        var retainedSnapshot: JSONObject? = null
                        var retainedPanels: JSONObject? = null
                        scenario.onActivity { retainedSnapshot = host.snapshot; retainedPanels = host.panelContent }
                        report(reset = true)
                        synchronized(durations) { durations.clear(); lostMetrics = 0 }
                        scenario.onActivity { drawnRevisions.clear() }
                        measuring.set(true)
                        val traceName = "workspace-benchmark-${if (mouse) "mouse" else "touch"}-$mode"
                        android.os.Trace.beginAsyncSection(traceName, 1)
                        val start = SystemClock.uptimeMillis()
                        val finished = CountDownLatch(1)
                        var samples = 0
                        var refreshRate = 0f
                        // Generate native motion on the real display clock. Sleeping
                        // 8 ms AFTER a blocking main-thread call caps input below 120 Hz.
                        lateinit var motion: Choreographer.FrameCallback
                        scenario.onActivity {
                            refreshRate = owner.view.display.refreshRate
                            val choreographer = Choreographer.getInstance()
                            motion = Choreographer.FrameCallback {
                                val elapsed = SystemClock.uptimeMillis() - start
                                if (elapsed >= 5000) { finished.countDown() }
                                else {
                                    val progress = (sin(elapsed * Math.PI / 500) * .5 + .5).toFloat()
                                    val point = if (mode == "floating") Offset(workspace.center.x + (progress - .5f) * workspace.width * .25f, workspace.center.y)
                                        else Offset(first.x + (last.x - first.x) * progress, first.y)
                                    eventOnMain(MotionEvent.ACTION_MOVE, point)
                                    samples++
                                    choreographer.postFrameCallback(motion)
                                }
                            }
                            choreographer.postFrameCallback(motion)
                        }
                        try { assertTrue("Real display input producer completed", finished.await(15, TimeUnit.SECONDS)) }
                        finally {
                            scenario.onActivity { Choreographer.getInstance().removeFrameCallback(motion) }
                            android.os.Trace.endAsyncSection(traceName, 1)
                        }
                        measuring.set(false)
                        val elapsed = SystemClock.uptimeMillis() - start
                        val rows = synchronized(durations) { durations.toList() }
                        val timings = rows.map { it.duration }.sorted()
                        val vsyncs = rows.map { it.vsync }.distinct().sorted()
                        val intervals = vsyncs.zipWithNext { a, b -> b - a }.sorted()
                        var drawn = 0
                        scenario.onActivity { drawn = drawnRevisions.size }
                        assertTrue("Android must render while dragging", timings.isNotEmpty())
                        fun percentile(values: List<Long>, fraction: Double) = values[((values.size - 1) * fraction).toInt()] / 1_000_000.0
                        val metrics = report()
                        assertEquals("Steady motion retains the full UI models", 0L, metrics.getLong("snapshots_published"))
                        scenario.onActivity {
                            assertSame(retainedSnapshot, host.snapshot)
                            assertSame(retainedPanels, host.panelContent)
                            val geometry = host.workspaceGeometry!!
                            if (geometry.group != null) {
                                val shown = find(owner.semanticsOwner.unmergedRootSemanticsNode, "group-${geometry.group}")!!.boundsInRoot
                                val density = owner.view.resources.displayMetrics.density
                                assertEquals("Native placement follows Rust geometry", workspace.left + geometry.bounds!!.left * density, shown.left, 1.1f)
                                assertEquals(workspace.top + geometry.bounds.top * density, shown.top, 1.1f)
                            }
                        }
                        val result = obj("mouse" to mouse, "mode" to mode, "elapsed_ms" to elapsed, "inputs" to samples,
                            "display_hz" to refreshRate, "debuggable" to BuildConfig.DEBUG,
                            "frames" to rows.size, "distinct_vsyncs" to vsyncs.size, "drawn_revisions" to drawn,
                            "frame_rate" to (vsyncs.size * 1000.0 / elapsed), "drawn_update_rate" to (drawn * 1000.0 / elapsed),
                            "frame_p50_ms" to percentile(timings, .5), "frame_p95_ms" to percentile(timings, .95),
                            "vsync_interval_p50_ms" to percentile(intervals, .5), "vsync_interval_p95_ms" to percentile(intervals, .95),
                            "deadline_misses" to rows.count { it.deadline > 0 && it.duration > it.deadline }, "lost_metrics" to lostMetrics,
                            "snapshot_attempts" to metrics.getLong("snapshot_attempts"), "snapshots" to metrics.getLong("snapshots_published"),
                            "workspace_updates" to metrics.getLong("workspace_updates_published"))
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
                scenario.onActivity { owner.view.viewTreeObserver.removeOnDrawListener(drawListener) }
                frames.quitSafely()
                action(obj("type" to "restore_workspace", "workspace" to saved))
                waitFor { host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString() == saved.toString() }
            }
        }
    }
}
