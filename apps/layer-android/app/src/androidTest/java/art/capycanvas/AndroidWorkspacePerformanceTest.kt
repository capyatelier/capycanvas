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

    @Test fun columnBridgeGapFrameTiming() = frameTiming(true, bridge = true)

    @Test fun columnBridgePanelFrameTiming() = frameTiming(false, bridge = true)

    @Test fun colorPanelOverlapFrameTiming() = frameTiming(false, colorOverlap = true)

    @Test fun colorWheelFrameTiming() = frameTiming(false, colorWheel = true)

    @Test fun editColorWheelFrameTiming() = frameTiming(false, colorWheel = true, editColor = true)

    @Test fun workspaceSwitcherScrollFrameTiming() = scrollFrameTiming(false)

    @Test fun groupedDrawerScrollFrameTiming() = scrollFrameTiming(true)

    private fun scrollFrameTiming(grouped: Boolean) {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString(if (grouped) "groupedToolBenchmark" else "switcherBenchmark") == "true")
        launchCapy(120_000).use { scenario ->
            device.landscape(scenario)
            val host = scenario.activity().host
            fun idle() = host.awaitMain("workspace preferences saved", 60_000, { "${host.workspaceManager}" }) {
                host.workspaceManager?.let { !it.optBoolean("busy") && !it.optBoolean("switcher_busy") && !it.optBoolean("dirty") } == true
            }
            fun workspace(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; host.drain(); idle() }
            workspace(obj("type" to "switch", "id" to if (grouped) "builtin:workspace:photographer" else "builtin:workspace:illustrator"))
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
            if (grouped) {
                host.awaitMain("grouped Photo toolbar") {
                    host.snapshot?.array("panels")?.objects()?.firstOrNull { it.getString("id") == "toolbar" }
                        ?.array("tiles")?.objects()?.any { it.getJSONObject("control").optString("slot") == "drawing" } == true
                }
                val tile = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
                    .array("tiles").objects().first { it.getJSONObject("control").optString("slot") == "drawing" }
                host.drain(obj("type" to "invoke", "command" to "eraser"))
                repeat(2) { host.drain(obj("type" to "activate_tile", "panel" to "toolbar", "tile" to tile.getInt("id"))) }
                host.awaitMain("grouped Drawing drawer", 120_000) {
                    findTag("tool-drawer") != null && host.snapshot?.optBoolean("shaders_ready") == true
                }
            } else {
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
            }
            val output = java.io.File(instrumentation.targetContext.getExternalFilesDir(null),
                if (grouped) "grouped-drawer-benchmark" else "workspace-switcher-benchmark").apply { mkdirs() }
            output.listFiles()?.forEach { it.delete() }
            for (mode in if (grouped) listOf("grouped-drawer") else listOf("choices", "options")) {
                if (mode == "options") instrumentation.runOnMainSync {
                    val button = findTag("workspace-switcher-options")!!.second.find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                    button.config[SemanticsActions.OnClick].action!!.invoke()
                }
                val surfaceTag = if (grouped) "tool-drawer" else if (mode == "choices") "workspace-switcher-choices" else "workspace-menu"
                host.awaitMain("$mode scroll surface") { findTag(surfaceTag) != null }
                lateinit var owner: ViewRootForTest
                lateinit var scroll: SemanticsNode
                var dots: androidx.compose.ui.geometry.Rect? = null
                instrumentation.runOnMainSync {
                    val surface = findTag(surfaceTag)!!
                    owner = surface.first
                    scroll = surface.second.find {
                        (it.config.getOrNull(if (mode == "choices") SemanticsProperties.HorizontalScrollAxisRange else SemanticsProperties.VerticalScrollAxisRange)?.maxValue() ?: 0f) > 0f
                    }!!
                    if (!grouped) dots = findTag("workspace-switcher-options")!!.second.boundsInRoot
                }
                val range = scroll.config[if (mode == "choices") SemanticsProperties.HorizontalScrollAxisRange else SemanticsProperties.VerticalScrollAxisRange]
                assertTrue("$mode has overflow", range.maxValue() > 0f)
                var maximum = -1f
                var settledAt = SystemClock.uptimeMillis()
                host.awaitMain("$mode scroll geometry settles") {
                    val next = range.maxValue()
                    if (next != maximum) { maximum = next; settledAt = SystemClock.uptimeMillis() }
                    maximum in 1f..999_999f && scroll.boundsInRoot.height > 100f && SystemClock.uptimeMillis() - settledAt >= 300
                }
                val bounds = scroll.boundsInRoot
                val amplitude = minOf((if (mode == "choices") bounds.width else bounds.height) * .2f, range.maxValue() * .2f)
                val rows = java.util.Collections.synchronizedList(mutableListOf<LongArray>())
                val frameRows = java.util.Collections.synchronizedList(mutableListOf<LongArray>())
                val frameThread = HandlerThread("scroll-frame-metrics").apply { start() }
                val window = scenario.activity().window
                val measuring = AtomicBoolean(false)
                var previous = Float.NaN
                val draw = android.view.ViewTreeObserver.OnDrawListener {
                    val value = range.value()
                    if (measuring.get() && value != previous) rows.add(longArrayOf(System.nanoTime(), (value * 1000).toLong()))
                    previous = value
                }
                val frameListener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
                    if (measuring.get()) frameRows.add(longArrayOf(metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP),
                        metrics.getMetric(FrameMetrics.TOTAL_DURATION), metrics.getMetric(FrameMetrics.DEADLINE), dropped.toLong()))
                }
                instrumentation.runOnMainSync {
                    owner.view.viewTreeObserver.addOnDrawListener(draw)
                    window.addOnFrameMetricsAvailableListener(frameListener, Handler(frameThread.looper))
                }
                try {
                    for (run in 0..3) {
                        instrumentation.runOnMainSync {
                            val delta = range.maxValue() * .5f - range.value()
                            scroll.config[SemanticsActions.ScrollBy].action!!.invoke(if (mode == "choices") delta else 0f, if (mode != "choices") delta else 0f)
                        }
                        host.awaitMain("$mode centered scroll", diagnostics = {
                            screenshot("${output.name}/failure-center.png")
                            "value=${range.value()}; maximum=${range.maxValue()}; bounds=${scroll.boundsInRoot}"
                        }) { kotlin.math.abs(range.value() - range.maxValue() * .5f) < 2f }
                        val duration = if (run == 0) 1000L else 6000L
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
                        rows.clear(); frameRows.clear()
                        instrumentation.runOnMainSync { previous = range.value() }
                        val begin = System.nanoTime()
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
                        val end = System.nanoTime()
                        val moving = synchronized(rows) { rows.toList() }
                        SystemClock.sleep(200)
                        measuring.set(false)
                        if (run > 0) {
                            assertTrue("$mode scroll visibly moves", moving.size > 100)
                            val result = obj("mode" to mode, "run" to run, "begin_ns" to begin, "end_ns" to end,
                                "duration_ms" to duration, "frame_callback_drain_ms" to 200, "inputs" to inputs, "display_hz" to owner.view.display.refreshRate,
                                "debuggable" to BuildConfig.DEBUG, "camera" to host.snapshot!!.getJSONObject("state").getJSONObject("camera"),
                                "tabs" to host.snapshot!!.getJSONObject("state").array("tabs"),
                                "layers" to host.snapshot!!.getJSONObject("state").array("layers"),
                                "workspace" to host.snapshot!!.getJSONObject("state").getJSONObject("workspace"),
                                "settings" to host.snapshot!!.getJSONObject("state").getJSONObject("settings"),
                                "photo" to args.getString("photo"),
                                "draws" to JSONArray(moving.map { JSONArray(it.toList()) }),
                                "frames" to JSONArray(synchronized(frameRows) { frameRows.map { JSONArray(it.toList()) } }))
                            java.io.File(output, "$mode-$run.json").writeText(result.toString())
                        }
                        if (!grouped) instrumentation.runOnMainSync {
                            assertEquals("Options stay fixed while $mode scrolls", dots, findTag("workspace-switcher-options")!!.second.boundsInRoot)
                        }
                    }
                } finally {
                    measuring.set(false)
                    instrumentation.runOnMainSync {
                        owner.view.viewTreeObserver.removeOnDrawListener(draw)
                        window.removeOnFrameMetricsAvailableListener(frameListener)
                    }
                    frameThread.quitSafely()
                }
                if (mode == "options") pressKey(android.view.KeyEvent.KEYCODE_ESCAPE)
            }
        }
    }

    private fun frameTiming(resize: Boolean, colorOverlap: Boolean = false, colorWheel: Boolean = false, editColor: Boolean = false, bridge: Boolean = false) {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("workspaceBenchmark") == "true")
        (if (bridge) launchCapy(180_000) else ActivityScenario.launch(MainActivity::class.java)).use { scenario ->
            if (colorWheel || bridge) device.landscape(scenario)
            WorkspaceFrameTiming(instrumentation, args).run(scenario.activity(), resize, colorOverlap, colorWheel, editColor, bridge)
        }
    }
}
