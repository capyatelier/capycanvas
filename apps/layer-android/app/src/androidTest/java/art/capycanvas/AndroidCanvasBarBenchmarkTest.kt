package art.capycanvas

import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import android.view.FrameMetrics
import android.view.InputDevice
import android.view.MotionEvent
import android.view.Window
import android.view.WindowManager
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import kotlin.math.PI
import kotlin.math.sin

class AndroidCanvasBarBenchmarkTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)

    @OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)
    @After fun clearCompositionTracer() = androidx.compose.runtime.Composer.setTracer(null)

    @Test fun canvasBarFrameTiming() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("canvasBarBenchmark") == "true")
        val duration = args.getString("durationMs", "5000")!!.toInt()
        val interval = args.getString("intervalMs", "4.166667")!!.toDouble()
        val width = args.getString("width", "6000")!!.toInt()
        val height = args.getString("height", "4000")!!.toInt()
        val transparency = listOf("off", "low", "medium", "high").indexOf(args.getString("transparency", "low"))
        val only = args.getString("scenarios")?.split(',')
        if (args.getString("composeTrace") == "true") @OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)
            androidx.compose.runtime.Composer.setTracer(object : androidx.compose.runtime.CompositionTracer {
                override fun isTraceInProgress() = android.os.Trace.isEnabled()
                override fun traceEventStart(key: Int, dirty1: Int, dirty2: Int, info: String) = android.os.Trace.beginSection(info.take(120))
                override fun traceEventEnd() = android.os.Trace.endSection()
            })
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(label: String, condition: () -> Boolean) = host.awaitMain(label, 120_000, condition = condition)
            fun action(value: JSONObject) = host.drain(value, 30)
            fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
            fun state() = host.snapshot!!.getJSONObject("state")
            var documentExtent = "${width}x$height"
            fun newDocument(extent: Pair<Int, Int> = width to height) {
                documentExtent = "${extent.first}x${extent.second}"
                host.newDocument(extent.first, extent.second)
                invoke("fit_canvas"); invoke("zoom_out")
                SystemClock.sleep(800)
            }
            fun photo(): File {
                val file = File(instrumentation.targetContext.cacheDir, "canvas-bar-${width}x$height.jpg")
                if (file.exists()) return file
                val bitmap = android.graphics.Bitmap.createBitmap(width, height, android.graphics.Bitmap.Config.ARGB_8888)
                try {
                    val canvas = android.graphics.Canvas(bitmap)
                    canvas.drawPaint(android.graphics.Paint().apply { shader = android.graphics.LinearGradient(0f, 0f, width.toFloat(), height.toFloat(),
                        intArrayOf(0xff2b6cb0.toInt(), 0xffe8a33d.toInt(), 0xff2f855a.toInt()), null, android.graphics.Shader.TileMode.CLAMP) })
                    val stroke = android.graphics.Paint().apply { color = 0x60ffffff; strokeWidth = 18f; isAntiAlias = true }
                    for (i in 0 until 400) canvas.drawLine(i * 37f % width, 0f, (i * 53f + 900f) % width, height.toFloat(), stroke)
                    file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 90, it) }
                } finally { bitmap.recycle() }
                return file
            }
            fun place(file: File) {
                host.importImage(file)
                waitFor("placement bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "placement" }
                waitFor("canvas ready") { host.snapshot?.let { it.optBoolean("canvas_ready") && it.optBoolean("brush_ready") } == true }
            }
            fun corner(): Pair<Double, Double> {
                val anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor")
                val camera = state().getJSONObject("camera")
                val zoom = camera.getDouble("zoom"); val translation = camera.getJSONArray("translation")
                return anchor.getDouble(2) * zoom + translation.getDouble(0) to anchor.getDouble(3) * zoom + translation.getDouble(1)
            }
            fun inject(action: Int, down: Long, x: Double, y: Double, pressure: Float) {
                val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 5; toolType = MotionEvent.TOOL_TYPE_STYLUS })
                val coords = arrayOf(MotionEvent.PointerCoords().apply {
                    this.x = x.toFloat() + host.surfaceOrigin.x; this.y = y.toFloat() + host.surfaceOrigin.y; this.pressure = pressure
                })
                val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
                try { check(instrumentation.uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
            }
            fun drag(start: Pair<Double, Double>, milliseconds: Int, path: (Double) -> Pair<Double, Double>) {
                val count = (milliseconds / interval).toInt()
                val began = System.nanoTime(); val down = SystemClock.uptimeMillis()
                for (i in 0..count) {
                    val left = began + (i * interval * 1e6).toLong() - System.nanoTime()
                    if (left > 0) java.util.concurrent.locks.LockSupport.parkNanos(left)
                    val (dx, dy) = path(i * interval / 1000.0)
                    inject(if (i == 0) MotionEvent.ACTION_DOWN else if (i == count) MotionEvent.ACTION_UP else MotionEvent.ACTION_MOVE,
                        down, start.first + dx, start.second + dy, if (i == count) 0f else .7f)
                }
            }
            fun drags(milliseconds: Int) {
                val began = SystemClock.uptimeMillis()
                var index = 0
                while (SystemClock.uptimeMillis() - began < milliseconds) {
                    val start = corner()
                    android.os.Trace.beginAsyncSection("capy-drag", ++index)
                    drag(start, 400) { t -> (-60 - 50 * sin(2 * PI * t / .4)) to (-40 - 35 * sin(2 * PI * t / .4)) }
                    android.os.Trace.endAsyncSection("capy-drag", index)
                    SystemClock.sleep(900)
                }
            }
            fun hideAndShow(milliseconds: Int) {
                val began = SystemClock.uptimeMillis()
                while (SystemClock.uptimeMillis() - began < milliseconds) {
                    instrumentation.runOnMainSync { host.holdCanvasBar(1) }
                    SystemClock.sleep(250)
                    instrumentation.runOnMainSync { host.holdCanvasBar(0) }
                    SystemClock.sleep(450)
                }
            }
            fun taps(at: Pair<Double, Double>, milliseconds: Int) {
                val began = SystemClock.uptimeMillis()
                while (SystemClock.uptimeMillis() - began < milliseconds) {
                    val down = SystemClock.uptimeMillis()
                    inject(MotionEvent.ACTION_DOWN, down, at.first, at.second, .7f)
                    SystemClock.sleep(60)
                    inject(MotionEvent.ACTION_UP, down, at.first, at.second, 0f)
                    SystemClock.sleep(540)
                }
            }
            data class UiFrame(val total: Long, val layout: Long, val draw: Long, val animation: Long, val delay: Long,
                val sync: Long, val issue: Long, val swap: Long, val gpu: Long)
            val uiFrames = mutableListOf<UiFrame>()
            var measuring = false
            val metricsThread = HandlerThread("canvas-bar-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
                if (measuring) synchronized(uiFrames) { uiFrames.add(UiFrame(metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                    metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION), metrics.getMetric(FrameMetrics.DRAW_DURATION),
                    metrics.getMetric(FrameMetrics.ANIMATION_DURATION), metrics.getMetric(FrameMetrics.UNKNOWN_DELAY_DURATION),
                    metrics.getMetric(FrameMetrics.SYNC_DURATION), metrics.getMetric(FrameMetrics.COMMAND_ISSUE_DURATION),
                    metrics.getMetric(FrameMetrics.SWAP_BUFFERS_DURATION), metrics.getMetric(FrameMetrics.GPU_DURATION))) }
            }
            activity.window.addOnFrameMetricsAvailableListener(listener, Handler(metricsThread.looper))
            val output = File(activity.getExternalFilesDir(null), "canvas-bar-benchmark").apply { mkdirs() }
            fun quantiles(values: List<Double>): JSONObject {
                val sorted = values.sorted()
                fun at(fraction: Double) = if (sorted.isEmpty()) JSONObject.NULL else Math.round(sorted[((sorted.size - 1) * fraction).toInt()] * 1000) / 1000.0
                return obj("n" to sorted.size, "p50" to at(.5), "p95" to at(.95), "p99" to at(.99), "max" to at(1.0),
                    "over_8_33" to sorted.count { it > 8.333 })
            }
            var refreshRate = 0f
            fun measure(label: String, operation: () -> Unit) {
                SystemClock.sleep(600)
                host.measurementReport(true)
                native { Native.completionTimings(it, true) }
                synchronized(uiFrames) { uiFrames.clear() }
                val bars = mutableListOf<Boolean>()
                measuring = true
                val began = System.nanoTime()
                val sampler = Thread {
                    while (measuring) { instrumentation.runOnMainSync { bars.add(host.canvasBar != null && host.canvasBarVisible) }; SystemClock.sleep(8) }
                }.apply { start() }
                android.os.Trace.beginAsyncSection("canvas-bar-$label", 1)
                try { operation() } finally { android.os.Trace.endAsyncSection("canvas-bar-$label", 1) }
                SystemClock.sleep(400)
                measuring = false; sampler.join()
                val ended = System.nanoTime()
                instrumentation.runOnMainSync { refreshRate = activity.window.decorView.display.refreshRate }
                val completions = native { JSONArray(Native.completionTimings(it, false)) }
                val metrics = host.measurementReport(false)
                val frames = metrics.getJSONArray("frames").let { a -> (0 until a.length()).map { a.getJSONArray(it) } }
                    .filter { it.getLong(1) in began..ended }
                val submitted = frames.filter { it.getLong(6) > 0 }
                val rows = (0 until completions.length()).map { completions.getJSONArray(it) }.filter { it.getLong(1) in began..ended }
                val completedAt = rows.map { it.getLong(2) }.sorted()
                val ui = synchronized(uiFrames) { uiFrames.toList() }
                val seconds = (ended - began) / 1e9
                val result = obj("label" to label, "display_hz" to refreshRate, "seconds" to seconds, "transparency" to transparency,
                    "canvas" to documentExtent, "debuggable" to BuildConfig.DEBUG,
                    "bar_visible_fraction" to if (bars.isEmpty()) 0.0 else bars.count { it } / bars.size.toDouble(),
                    "bar_transitions" to bars.zipWithNext().count { (a, b) -> a != b },
                    "renderer_submitted_hz" to submitted.size / seconds,
                    "renderer_cpu_callback_ms" to quantiles(submitted.map { it.getLong(10) / 1e6 }),
                    "renderer_owner_cpu_ms" to quantiles(submitted.map { it.getLong(17) / 1e6 }),
                    "gpu_submit_to_complete_ms" to quantiles(rows.map { (it.getLong(2) - it.getLong(1)) / 1e6 }),
                    "gpu_completion_interval_ms" to quantiles(completedAt.zipWithNext { a, b -> (b - a) / 1e6 }),
                    "ui_frames" to ui.size,
                    "ui_frame_ms" to quantiles(ui.map { it.total / 1e6 }),
                    "ui_layout_ms" to quantiles(ui.map { it.layout / 1e6 }),
                    "ui_draw_ms" to quantiles(ui.map { it.draw / 1e6 }),
                    "ui_animation_ms" to quantiles(ui.map { it.animation / 1e6 }),
                    "ui_cpu_ms" to quantiles(ui.map { (it.animation + it.layout + it.draw + it.sync + it.issue) / 1e6 }),
                    "ui_delay_ms" to quantiles(ui.map { it.delay / 1e6 }),
                    "ui_command_issue_ms" to quantiles(ui.map { it.issue / 1e6 }),
                    "ui_swap_ms" to quantiles(ui.map { it.swap / 1e6 }),
                    "ui_gpu_ms" to quantiles(ui.map { it.gpu / 1e6 }),
                    "glass_regions" to native { JSONObject(Native.displayStatus(it)).optInt("glass_regions", -1) })
                File(output, "$label.json").writeText(JSONObject(result.toString()).put("frame_fields", metrics.getJSONArray("frame_fields")).put("frames", JSONArray(frames)).put("completions", JSONArray(rows)).toString(2))
                Log.i("CapyBarPerf", result.toString())
                println("CANVAS BAR $label $result")
            }
            fun wanted(name: String) = only == null || name in only
            waitFor("ready") { host.snapshot?.optBoolean("shaders_ready") == true && host.workspaceManager?.optBoolean("ready") == true && host.workspaceManager?.optBoolean("busy") == false }
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to transparency)))
            val wiggle = { t: Double -> (-60 - 50 * sin(2 * PI * t)) to (-40 - 35 * sin(2 * PI * t)) }

            if (wanted("ui")) {
                newDocument(2048 to 1536)
                invoke("zoom_out")
                invoke("select_all"); invoke("fill_selection"); SystemClock.sleep(800)
                invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                waitFor("transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                SystemClock.sleep(1500)
                measure("ui-bar-show-hide") { hideAndShow(duration) }
                measure("ui-bar-move") {
                    val began = SystemClock.uptimeMillis()
                    var step = 0
                    while (SystemClock.uptimeMillis() - began < duration) {
                        action(obj("type" to "set_tool_setting", "id" to "transform_x", "value" to if (step++ % 2 == 0) 240f else 0f))
                        SystemClock.sleep(400)
                    }
                }
                invoke("zen_mode")
                SystemClock.sleep(1500)
                measure("ui-bar-show-hide-zen") { hideAndShow(duration) }
                invoke("zen_mode")
                invoke("cancel_transform"); invoke("deselect"); invoke("brush")
                SystemClock.sleep(1500)
                measure("ui-panel-change") {
                    val began = SystemClock.uptimeMillis()
                    var step = 0
                    while (SystemClock.uptimeMillis() - began < duration) {
                        action(obj("type" to "set_brush_size", "value" to if (step++ % 2 == 0) 40f else 18f))
                        SystemClock.sleep(400)
                    }
                }
            }
            if (wanted("paint")) {
                newDocument()
                invoke("brush")
                val area = state().getJSONObject("camera").getJSONArray("work_area")
                val center = area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                val radius = minOf(area.getDouble(2), area.getDouble(3)) * .3
                measure("paint-strokes") { drag(center, duration) { t -> radius * sin(t * 3.2) to radius * .65 * sin(t * 4.7) } }
            }
            if (wanted("photo")) {
                newDocument()
                place(photo())
                SystemClock.sleep(3000)
                measure("photo-bar-show-hide") { hideAndShow(duration) }
                measure("photo-bar-contact-taps") { taps(corner().let { it.first - 200 to it.second - 200 }, duration) }
                measure("photo-handle-drag-bar-hidden") { drag(corner(), duration, wiggle) }
                invoke("show_canvas_action_bar")
                waitFor("completion-only bar") { host.canvasBar?.array("items")?.length() == 0 }
                measure("photo-handle-drag-bar-visible") { drag(corner(), duration, wiggle) }
                measure("photo-handle-drags") { drags(duration) }
                invoke("show_canvas_action_bar")
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                waitFor("photo transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                SystemClock.sleep(1500)
                measure("photo-pixels-handle-drag") { drag(corner(), duration, wiggle) }
                measure("photo-pixels-drags") { drags(duration) }
                invoke("transform_distort")
                SystemClock.sleep(1000)
                measure("photo-pixels-distort-drag") { drag(corner(), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("scaled")) {
                newDocument()
                place(photo())
                for (axis in listOf("transform_width", "transform_height")) action(obj("type" to "set_tool_setting", "id" to axis, "value" to .45f))
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                SystemClock.sleep(3000)
                invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                waitFor("photo transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                SystemClock.sleep(1500)
                measure("scaled-photo-pixels-handle-drag") { drag(corner(), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("selection")) {
                newDocument()
                invoke("select_all"); invoke("fill_selection"); SystemClock.sleep(1500)
                invoke("rectangle_select"); invoke("select_all")
                invoke("scale_rotate")
                waitFor("transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                SystemClock.sleep(1500)
                measure("selection-bar-show-hide") { hideAndShow(duration) }
                measure("selection-bar-contact-taps") { taps(corner().let { it.first - 200 to it.second - 200 }, duration) }
                measure("selection-handle-drag") { drag(corner(), duration, wiggle) }
                invoke("transform_distort")
                SystemClock.sleep(1000)
                measure("selection-distort-drag") { drag(corner(), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("menus")) {
                newDocument(2048 to 1536)
                invoke("select_all"); invoke("fill_selection"); invoke("deselect"); invoke("rectangle_select")
                val area = state().getJSONObject("camera").getJSONArray("work_area")
                val center = area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                val span = minOf(area.getDouble(2), area.getDouble(3)) * .15
                fun strokes(milliseconds: Int) {
                    val began = SystemClock.uptimeMillis()
                    var index = 0
                    do {
                        val shift = (index % 3 - 1) * span * .4
                        android.os.Trace.beginAsyncSection("capy-selection-stroke", ++index)
                        drag(center.first - span + shift to center.second - span, 300) { t -> 2 * span * t / .3 to 1.5 * span * t / .3 }
                        android.os.Trace.endAsyncSection("capy-selection-stroke", index)
                        SystemClock.sleep(900)
                    } while (SystemClock.uptimeMillis() - began < milliseconds)
                }
                fun menuButton(tag: String): Pair<Float, Float>? {
                    var point: Pair<Float, Float>? = null
                    instrumentation.runOnMainSync {
                        findTag(tag)?.let { (root, node) ->
                            val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                            point = node.boundsInRoot.center.let { it.x + origin[0] to it.y + origin[1] }
                        }
                    }
                    return point
                }
                fun menus(tag: String, milliseconds: Int) {
                    val began = SystemClock.uptimeMillis()
                    while (SystemClock.uptimeMillis() - began < milliseconds) {
                        val (x, y) = menuButton(tag) ?: error("The selection bar has no $tag")
                        val down = SystemClock.uptimeMillis()
                        inject(MotionEvent.ACTION_DOWN, down, x - host.surfaceOrigin.x.toDouble(), y - host.surfaceOrigin.y.toDouble(), .7f)
                        SystemClock.sleep(50)
                        inject(MotionEvent.ACTION_UP, down, x - host.surfaceOrigin.x.toDouble(), y - host.surfaceOrigin.y.toDouble(), 0f)
                        waitFor("$tag opens its menu") { semanticsRoots().any { it.find(hasTag("workspace-menu")) != null } }
                        SystemClock.sleep(600)
                        instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
                        waitFor("$tag closes its menu") { semanticsRoots().none { it.find(hasTag("workspace-menu")) != null } }
                        SystemClock.sleep(600)
                    }
                }
                strokes(0)
                waitFor("selection bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "selection" && host.canvasBarVisible }
                SystemClock.sleep(1500)
                measure("selection-bar-after-strokes") { strokes(duration) }
                invoke("show_canvas_action_bar")
                measure("selection-strokes-bar-off") { strokes(duration) }
                invoke("show_canvas_action_bar")
                waitFor("selection bar after strokes") { host.canvasBarVisible && findTag("canvas-bar-menu-adjust") != null }
                SystemClock.sleep(1500)
                measure("selection-bar-menu-open") { menus("canvas-bar-menu-adjust", duration) }
                measure("selection-bar-more-open") { menus("canvas-bar-more", duration) }
            }
            activity.window.removeOnFrameMetricsAvailableListener(listener)
            metricsThread.quitSafely()
            assertNull(host.failure)
        }
    }
}
