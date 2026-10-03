package art.capycanvas

import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.InputDevice
import android.view.MotionEvent
import android.view.Window
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sin

/** Opt-in measurement on the real Android frame clock, without Compose test
 * clock advancement or wait-for-idle between pointer samples. */
class AndroidWorkspacePerformanceTest {
    @get:Rule val device = CapyDeviceRule()

    @Test fun continuousDragFrameTiming() = frameTiming(false)

    @Test fun continuousResizeFrameTiming() = frameTiming(true)

    @Test fun colorPanelOverlapFrameTiming() = frameTiming(false, colorOverlap = true)

    @Test fun workspaceSwitcherScrollFrameTiming() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("switcherBenchmark") == "true")
        launchCapy(120_000).use { scenario ->
            device.landscape(scenario)
            val host = scenario.activity().host
            fun idle() = host.awaitMain("workspace preferences saved", 60_000, { "${host.workspaceManager}" }) {
                host.workspaceManager?.let { !it.optBoolean("busy") && !it.optBoolean("switcher_busy") && !it.optBoolean("dirty") } == true
            }
            fun workspace(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; host.drain(); idle() }
            workspace(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
            host.newDocument(4248, 2832)
            host.importImage(java.io.File(checkNotNull(args.getString("photo"))))
            host.awaitMain("photo placement") {
                host.snapshot?.getJSONObject("state")?.objectOrNull("canvas_bar")?.objectOrNull("context")?.optString("kind") == "placement"
            }
            host.drain(obj("type" to "invoke", "command" to "apply_transform"))
            host.awaitMain("photo placement applied") {
                host.snapshot?.getJSONObject("state")?.objectOrNull("canvas_bar")?.objectOrNull("context")?.optString("kind") != "placement"
            }
            for (id in listOf(1, 2)) host.drain(obj("type" to "layer", "action" to obj("op" to "delete", "id" to id)))
            host.drain(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
            host.drain(obj("type" to "invoke", "command" to "fit_canvas"))
            val fixture = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString())
            repeat(30) { index ->
                workspace(obj("type" to "form", "action" to obj("type" to "new")))
                workspace(obj("type" to "submit", "name" to "Workspace ${index.toString().padStart(2, '0')} long name"))
            }
            workspace(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
            fixture.getJSONObject("layout").put("header", obj("size" to "small", "next_id" to 901,
                "zones" to JSONArray(listOf(JSONArray(listOf(obj("id" to 900, "item" to obj("kind" to "workspaces")))), JSONArray(), JSONArray()))))
            host.drain(obj("type" to "restore_workspace", "workspace" to fixture))
            host.awaitMain("scrolling switcher and settled renderer", 120_000) {
                findTag("workspace-switcher-options") != null && host.snapshot?.optBoolean("shaders_ready") == true
            }
            val output = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "workspace-switcher-benchmark").apply { mkdirs() }
            output.listFiles()?.forEach { it.delete() }
            for (mode in listOf("choices", "options")) {
                if (mode == "options") instrumentation.runOnMainSync {
                    val button = findTag("workspace-switcher-options")!!.second.find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                    button.config[SemanticsActions.OnClick].action!!.invoke()
                }
                host.awaitMain("$mode scroll surface") { findTag(if (mode == "choices") "workspace-switcher-choices" else "workspace-menu") != null }
                lateinit var owner: ViewRootForTest
                lateinit var scroll: SemanticsNode
                lateinit var dots: androidx.compose.ui.geometry.Rect
                instrumentation.runOnMainSync {
                    val surface = findTag(if (mode == "choices") "workspace-switcher-choices" else "workspace-menu")!!
                    owner = surface.first
                    scroll = surface.second.find { it.config.getOrNull(if (mode == "choices") SemanticsProperties.HorizontalScrollAxisRange else SemanticsProperties.VerticalScrollAxisRange) != null }!!
                    dots = findTag("workspace-switcher-options")!!.second.boundsInRoot
                }
                val range = scroll.config[if (mode == "choices") SemanticsProperties.HorizontalScrollAxisRange else SemanticsProperties.VerticalScrollAxisRange]
                assertTrue("$mode has overflow", range.maxValue() > 0f)
                val bounds = scroll.boundsInRoot
                val amplitude = minOf((if (mode == "choices") bounds.width else bounds.height) * .2f, range.maxValue() * .2f)
                val rows = java.util.Collections.synchronizedList(mutableListOf<LongArray>())
                val measuring = AtomicBoolean(false)
                var previous = Float.NaN
                val draw = android.view.ViewTreeObserver.OnDrawListener {
                    val value = range.value()
                    if (measuring.get() && value != previous) rows.add(longArrayOf(SystemClock.elapsedRealtimeNanos(), (value * 1000).toLong()))
                    previous = value
                }
                instrumentation.runOnMainSync { owner.view.viewTreeObserver.addOnDrawListener(draw) }
                try {
                    for (run in 0..3) {
                        instrumentation.runOnMainSync {
                            val delta = range.maxValue() * .5f - range.value()
                            scroll.config[SemanticsActions.ScrollBy].action!!.invoke(if (mode == "choices") delta else 0f, if (mode == "options") delta else 0f)
                        }
                        host.awaitMain("$mode centered scroll") { kotlin.math.abs(range.value() - range.maxValue() * .5f) < 2f }
                        val duration = if (run == 0) 1000L else 5000L
                        val downAt = SystemClock.uptimeMillis()
                        fun event(action: Int, displacement: Float) {
                            val point = bounds.center + if (mode == "choices") Offset(displacement, 0f) else Offset(0f, displacement)
                            val contact = motion(MotionEvent.TOOL_TYPE_FINGER, action, point, downAt)
                            try { owner.view.dispatchTouchEvent(contact) } finally { contact.recycle() }
                        }
                        instrumentation.runOnMainSync { event(MotionEvent.ACTION_DOWN, -amplitude) }
                        val complete = CountDownLatch(1)
                        var inputs = 0
                        lateinit var callback: Choreographer.FrameCallback
                        rows.clear(); previous = Float.NaN
                        val begin = SystemClock.elapsedRealtimeNanos()
                        measuring.set(true)
                        android.os.Trace.beginAsyncSection("workspace-switcher-scroll-$mode-$run", run)
                        instrumentation.runOnMainSync {
                            val clock = Choreographer.getInstance()
                            callback = Choreographer.FrameCallback {
                                val elapsed = SystemClock.uptimeMillis() - downAt
                                if (elapsed >= duration) complete.countDown()
                                else {
                                    val cycle = (elapsed % 2000) / 1000f
                                    event(MotionEvent.ACTION_MOVE, amplitude * (2 * (if (cycle <= 1) cycle else 2 - cycle) - 1))
                                    inputs++
                                    clock.postFrameCallback(callback)
                                }
                            }
                            clock.postFrameCallback(callback)
                        }
                        try { assertTrue("Native scroll input completed", complete.await(15, TimeUnit.SECONDS)) }
                        finally { instrumentation.runOnMainSync { Choreographer.getInstance().removeFrameCallback(callback); event(MotionEvent.ACTION_CANCEL, 0f) } }
                        android.os.Trace.endAsyncSection("workspace-switcher-scroll-$mode-$run", run)
                        measuring.set(false)
                        val end = SystemClock.elapsedRealtimeNanos()
                        if (run > 0) {
                            val moving = synchronized(rows) { rows.toList() }
                            assertTrue("$mode scroll visibly moves", moving.size > 100)
                            val result = obj("mode" to mode, "run" to run, "begin_ns" to begin, "end_ns" to end,
                                "duration_ms" to duration, "inputs" to inputs, "display_hz" to owner.view.display.refreshRate,
                                "debuggable" to BuildConfig.DEBUG, "camera" to host.snapshot!!.getJSONObject("state").getJSONObject("camera"),
                                "tabs" to host.snapshot!!.getJSONObject("state").array("tabs"), "draws" to JSONArray(moving.map { JSONArray(it.toList()) }))
                            java.io.File(output, "$mode-$run.json").writeText(result.toString())
                        }
                        instrumentation.runOnMainSync { assertEquals("Options stay fixed while $mode scrolls", dots, findTag("workspace-switcher-options")!!.second.boundsInRoot) }
                    }
                } finally { instrumentation.runOnMainSync { owner.view.viewTreeObserver.removeOnDrawListener(draw) } }
                if (mode == "options") pressKey(android.view.KeyEvent.KEYCODE_ESCAPE)
            }
        }
    }

    private fun frameTiming(resize: Boolean, colorOverlap: Boolean = false) {
        assumeTrue(InstrumentationRegistry.getArguments().getString("workspaceBenchmark") == "true")
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var host: CanvasHost
            lateinit var owner: ViewRootForTest
            lateinit var window: Window
            scenario.onActivity { host = it.host; owner = it.window.decorView.descendant<ViewRootForTest>()!!; window = it.window }
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
            fun bounds(tag: String): androidx.compose.ui.geometry.Rect {
                var result = androidx.compose.ui.geometry.Rect.Zero
                scenario.onActivity { result = owner.find(hasTag(tag))!!.boundsInRoot }
                return result
            }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true }
            waitFor { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
            var saved = JSONObject()
            scenario.onActivity { saved = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString()) }
            var savedTransparency = 0
            scenario.onActivity { savedTransparency = listOf("off", "low", "medium", "high").indexOf(
                host.snapshot!!.getJSONObject("state").getJSONObject("settings").getString("transparency")) }
            val transparency = InstrumentationRegistry.getArguments().getString("workspaceTransparency")?.toInt() ?: savedTransparency
            val fixture = JSONObject(saved.toString())
            fixture.getJSONObject("layout").apply {
                put("bands", JSONArray(listOf(
                    obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                    obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties")))))
                put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
                put("next_id", maxOf(49, getInt("next_id")))
                if (colorOverlap) put("floating", JSONArray(listOf(obj("root" to tabs(48, "color"),
                    "position" to JSONArray(listOf(360, 120)), "width" to 360, "default_width" to 360, "height" to 400, "toolbar_layout" to "compact"))))
            }
            fixture.put("zen_mode", false)
            val measuring = AtomicBoolean(false)
            data class Frame(val duration: Long, val deadline: Long, val vsync: Long, val layout: Long, val draw: Long)
            val durations = mutableListOf<Frame>()
            val drawnRevisions = mutableSetOf<Long>()
            var resizeNode: SemanticsNode? = null
            var lastDrawnBounds: androidx.compose.ui.geometry.Rect? = null
            var changedBounds = 0
            var lostMetrics = 0
            val frames = HandlerThread("workspace-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
                if (measuring.get()) synchronized(durations) {
                    durations.add(Frame(metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                        metrics.getMetric(FrameMetrics.DEADLINE), metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP),
                        metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION), metrics.getMetric(FrameMetrics.DRAW_DURATION)))
                    lostMetrics += dropped
                }
            }
            val drawListener = android.view.ViewTreeObserver.OnDrawListener {
                if (measuring.get()) {
                    host.workspaceGeometry?.revision?.let { drawnRevisions.add(it) }
                    // Read the retained LayoutNode's actual allocation. A newly
                    // received revision may still be awaiting recomposition.
                    resizeNode?.boundsInRoot?.let { bounds ->
                        if (lastDrawnBounds != bounds) changedBounds++
                        lastDrawnBounds = bounds
                    }
                }
            }
            scenario.onActivity { owner.view.viewTreeObserver.addOnDrawListener(drawListener) }
            window.addOnFrameMetricsAvailableListener(listener, Handler(frames.looper))
            try {
                action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to transparency)))
                for (mouse in listOf(true, false)) for (mode in if (colorOverlap) listOf("color-overlap") else if (resize) listOf("brushes", "properties", "navigator", "toolbar") else listOf("attached", "floating", "destination")) {
                    if (resize) fixture.getJSONObject("layout").getJSONArray("bands").apply {
                        getJSONObject(0).put("root", tabs(41, mode))
                        getJSONObject(1).put("root", tabs(43, "layers"))
                    }
                    action(obj("type" to "restore_workspace", "workspace" to fixture))
                    waitFor { !resize || host.snapshot!!.getJSONObject("layout").array("groups").objects().any { it.getInt("id") == 41 && it.getString("active") == mode } }
                    SystemClock.sleep(300)
                    val workspace = bounds("workspace")
                    if (colorOverlap) {
                        action(obj("type" to "move_group", "group" to 43, "target" to obj("kind" to "float", "position" to JSONArray(listOf(500, 180))),
                            "viewport" to JSONArray(listOf(workspace.width / owner.view.resources.displayMetrics.density, workspace.height / owner.view.resources.displayMetrics.density))))
                        SystemClock.sleep(300)
                    }
                    val source = bounds(if (colorOverlap) "group-grip-43" else if (resize) "divider-40" else when (mode) {
                        "attached" -> "tab-tool_settings"
                        "floating" -> "group-grip-41"
                        else -> "tab-layers"
                    }).center
                    val first = if (colorOverlap) bounds("color-wheel").let { Offset(it.center.x - it.width * .15f, it.top + it.height * .25f) } else if (resize) source + Offset(60f, 0f) else bounds("tab-brushes").center
                    val last = if (colorOverlap) bounds("color-wheel").let { Offset(it.center.x + it.width * .15f, it.top + it.height * .25f) } else if (resize) source + Offset(220f, 0f) else bounds("tab-sizes").center
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
                        event(MotionEvent.ACTION_MOVE, if (resize || colorOverlap || mode == "attached") first else workspace.center)
                        SystemClock.sleep(750)
                        host.measurementReport(true)
                        synchronized(durations) { durations.clear(); lostMetrics = 0 }
                        scenario.onActivity {
                            drawnRevisions.clear(); changedBounds = 0
                            resizeNode = if (resize) owner.find(hasTag("group-41")) else null
                            lastDrawnBounds = resizeNode?.boundsInRoot
                        }
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
                                    // Constant-speed resize sweeps avoid repeated pixels from
                                    // easing near the ends, making actual allocation changes
                                    // a useful measure of fresh resize geometry.
                                    val cycle = (elapsed % 1000) / 500f
                                    val progress = if (resize) (if (cycle <= 1f) cycle else 2f - cycle)
                                        else (sin(elapsed * Math.PI / 500) * .5 + .5).toFloat()
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
                        var changes = 0
                        scenario.onActivity { drawn = drawnRevisions.size; changes = changedBounds }
                        assertTrue("Android must render while dragging", timings.isNotEmpty())
                        fun percentile(values: List<Long>, fraction: Double) = values[((values.size - 1) * fraction).toInt()] / 1_000_000.0
                        val metrics = host.measurementReport()
                        val expectRetained = !resize || InstrumentationRegistry.getArguments().getString("expectRetainedResize") != "false"
                        if (expectRetained) assertEquals("Steady motion retains the full UI models", 0L, metrics.getLong("snapshots_published"))
                        scenario.onActivity {
                            if (expectRetained) assertEquals("Steady motion retains panel content", 0L, metrics.getLong("panel_content_changes"))
                            val geometry = host.workspaceGeometry!!
                            if (geometry.group != null) {
                                val shown = owner.find(hasTag("group-${geometry.group}"))!!.boundsInRoot
                                val density = owner.view.resources.displayMetrics.density
                                assertEquals("Native placement follows Rust geometry", workspace.left + geometry.bounds!!.left * density, shown.left, 1.1f)
                                assertEquals(workspace.top + geometry.bounds.top * density, shown.top, 1.1f)
                                if (colorOverlap) {
                                    val wheel = owner.find(hasTag("color-wheel"))!!.boundsInRoot
                                    assertTrue("Motion overlaps the visible Color wheel", shown.overlaps(wheel))
                                }
                            }
                        }
                        val result = obj("mouse" to mouse, "mode" to mode, "elapsed_ms" to elapsed, "inputs" to samples,
                            "transparency" to transparency,
                            "display_hz" to refreshRate, "debuggable" to BuildConfig.DEBUG,
                            "trajectory" to if (resize) "triangle" else "sine",
                            "frames" to rows.size, "distinct_vsyncs" to vsyncs.size, "drawn_revisions" to drawn,
                            "frame_rate" to (vsyncs.size * 1000.0 / elapsed), "drawn_update_rate" to (drawn * 1000.0 / elapsed),
                            "changed_bounds" to changes, "changed_bounds_rate" to (changes * 1000.0 / elapsed),
                            "frame_p50_ms" to percentile(timings, .5), "frame_p95_ms" to percentile(timings, .95),
                            "layout_p50_ms" to percentile(rows.map { it.layout }.sorted(), .5),
                            "layout_p95_ms" to percentile(rows.map { it.layout }.sorted(), .95),
                            "draw_p50_ms" to percentile(rows.map { it.draw }.sorted(), .5),
                            "draw_p95_ms" to percentile(rows.map { it.draw }.sorted(), .95),
                            "vsync_interval_p50_ms" to percentile(intervals, .5), "vsync_interval_p95_ms" to percentile(intervals, .95),
                            "deadline_misses" to rows.count { it.deadline > 0 && it.duration > it.deadline }, "lost_metrics" to lostMetrics,
                            "snapshot_attempts" to metrics.getLong("snapshot_attempts"), "snapshots" to metrics.getLong("snapshots_published"),
                            "workspace_updates" to metrics.getLong("workspace_updates_published"))
                        val publicationRows = metrics.getJSONArray("publications").let { a -> (0 until a.length()).map { a.getJSONArray(it) } }
                        for ((column, name) in listOf("native", "parse", "prepare").withIndex()) {
                            val values = publicationRows.map { it.getLong(column) }.sorted()
                            if (values.isNotEmpty()) { result.put("${name}_p50_ms", percentile(values, .5)); result.put("${name}_p95_ms", percentile(values, .95)) }
                        }
                        result.put("publication_utf16_units", publicationRows.sumOf { it.getLong(3) })
                        result.put("panel_content_changes", metrics.getLong("panel_content_changes"))
                        Log.i(if (resize) "CapyResizePerf" else "CapyDragPerf", result.toString())
                    } finally {
                        measuring.set(false)
                        event(MotionEvent.ACTION_CANCEL, source)
                    }
                    action(obj("type" to "close_settings"))
                    if (colorOverlap) action(obj("type" to "restore_workspace", "workspace" to fixture))
                    waitFor { host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").array("floating").length() == (if (colorOverlap) 1 else 0) }
                }
            } finally {
                measuring.set(false)
                window.removeOnFrameMetricsAvailableListener(listener)
                scenario.onActivity { owner.view.viewTreeObserver.removeOnDrawListener(drawListener) }
                frames.quitSafely()
            }
        }
    }
}
