package art.capycanvas

import android.os.Handler
import android.os.HandlerThread
import android.os.PowerManager
import android.os.SystemClock
import android.os.Trace
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.MotionEvent
import android.view.ViewTreeObserver
import android.view.Window
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class AndroidBubbleBenchmarkTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)

    @Test fun fiveBubbleMotion() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("bubbleBenchmark") == "true")
        assertFalse("Bubble qualification uses the optimized benchmark build", BuildConfig.DEBUG)
        val duration = args.getString("durationMs", "6000")!!.toLong()
        val repeats = args.getString("repeats", "3")!!.toInt()
        val expectedRefresh = args.getString("expectedRefreshHz", "0")!!.toFloat()
        val workload = args.getString("workload", "drag")!!
        val theme = args.getString("theme", "light")!!.also { require(it in listOf("light", "dark")) }
        val hdr = workload.startsWith("hdr-")
        val intensity = workload == "hdr-intensity"
        require(duration in 5000L..10000L && repeats >= 3)
        val width = args.getString("width", "6000")!!.toInt()
        val height = args.getString("height", "4000")!!.toInt()
        val photo = File(requireNotNull(args.getString("photo")))
        val size = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
        android.graphics.BitmapFactory.decodeFile(photo.path, size)
        assertEquals(width, size.outWidth); assertEquals(height, size.outHeight)
        launchCapy(180_000).use { scenario ->
            device.landscape(scenario)
            val activity = scenario.activity()
            val host = activity.host
            fun action(value: JSONObject) = host.drain(value)
            action(obj("type" to "set_theme", "theme" to theme))
            fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
            fun state() = host.snapshot!!.getJSONObject("state")
            fun query(value: JSONObject): Any? {
                val ready = CountDownLatch(1)
                var answer: Any? = null
                instrumentation.runOnMainSync { host.query(value) { answer = it; ready.countDown() } }
                assertTrue("$value completes", ready.await(20, TimeUnit.SECONDS))
                return answer
            }
            fun packets() = (query(obj("type" to "bubbles")) as JSONArray).objects()
            if (hdr) {
                host.createDrawing(width, height, depth = "F16")
                host.importImage(photo)
                host.awaitMain("Reference photo placement opens", 180_000) { state().objectOrNull("canvas_bar")?.objectOrNull("context")?.optString("kind") == "placement" }
                invoke("apply_transform")
                host.awaitMain("Reference photo placement commits", 180_000) { state().objectOrNull("canvas_bar")?.objectOrNull("context")?.optString("kind") != "placement" }
            } else host.openDocument(photo)
            host.awaitMain("Tier photo opens", 180_000) {
                state().array("tabs").objects().any { it.optBoolean("active") && it.optInt("width") == width && it.optInt("height") == height }
            }
            val photoLayer = state().array("layers").objects().single { it.optBoolean("editing") }.getLong("id")
            state().array("layers").objects().filter { it.getLong("id") != photoLayer }.forEach {
                action(obj("type" to "set_layer_visibility", "id" to it.getLong("id"), "visible" to false))
            }
            invoke("add_layer"); invoke("fit_canvas")
            host.awaitMain("All photo pipelines warm", 180_000) { host.snapshot?.optBoolean("shaders_ready") == true }
            val saved = JSONObject(state().getJSONObject("workspace").toString())
            val fixture = JSONObject(saved.toString()).apply {
                getJSONObject("layout").put("bubbles", obj("bubbles" to JSONArray(), "rings" to JSONArray(), "kept" to JSONArray()))
            }
            val controls = listOf(obj("kind" to "command", "command" to "drawing_brush"), obj("kind" to "color"),
                obj("kind" to "color_picker"), obj("kind" to "command", "command" to "undo"), obj("kind" to "panel", "panel" to "layers"))
                .let { if (hdr) listOf(it[1], it[0]) + it.drop(2) else it }
            fixture.getJSONObject("layout").put("header", obj("size" to "medium", "next_id" to 7,
                "zones" to JSONArray(listOf(JSONArray(listOf(obj("id" to 1, "item" to obj("kind" to "capy")))),
                    JSONArray(controls.mapIndexed { i, control -> obj("id" to i + 2, "item" to obj("kind" to "tool", "control" to control)) }), JSONArray()))))
            val output = File(instrumentation.targetContext.getExternalFilesDir(null), "bubble-benchmark").apply { mkdirs() }
            val contacts = JSONArray()
            val frameThread = HandlerThread("bubble-frame-metrics").apply { start() }
            val measuring = AtomicBoolean(false)
            val drawn = Collections.synchronizedList(mutableListOf<LongArray>())
            val frameRows = Collections.synchronizedList(mutableListOf<LongArray>())
            val frameListener = Window.OnFrameMetricsAvailableListener { _, metric, lost ->
                if (measuring.get()) frameRows.add(longArrayOf(metric.getMetric(FrameMetrics.VSYNC_TIMESTAMP),
                    metric.getMetric(FrameMetrics.TOTAL_DURATION), metric.getMetric(FrameMetrics.DEADLINE), lost.toLong(),
                    metric.getMetric(FrameMetrics.INPUT_HANDLING_DURATION), metric.getMetric(FrameMetrics.ANIMATION_DURATION),
                    metric.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION), metric.getMetric(FrameMetrics.DRAW_DURATION),
                    metric.getMetric(FrameMetrics.SYNC_DURATION), metric.getMetric(FrameMetrics.COMMAND_ISSUE_DURATION),
                    metric.getMetric(FrameMetrics.SWAP_BUFFERS_DURATION), metric.getMetric(FrameMetrics.GPU_DURATION)))
            }
            var owner: ViewRootForTest? = null
            var nodes = emptyList<SemanticsNode>()
            val drawListener = ViewTreeObserver.OnDrawListener {
                if (measuring.get()) drawn.add(longArrayOf(System.nanoTime(), *nodes.flatMap {
                    val center = it.boundsInRoot.center
                    listOf((center.x * 1000).toLong(), (center.y * 1000).toLong())
                }.toLongArray(), ((host.snapshot?.objectOrNull("color_panel")?.optDouble("intensity") ?: 0.0) * 1000).toLong()))
            }
            try {
                action(obj("type" to "restore_workspace", "workspace" to fixture))
                if (packets().isNotEmpty()) action(obj("type" to "bubble", "action" to obj("type" to "put_back_all")))
                lateinit var workspace: androidx.compose.ui.geometry.Rect
                var density = 1f
                instrumentation.runOnMainSync {
                    val root = checkNotNull(findTag("workspace")); owner = root.first; workspace = root.second.boundsInRoot
                    density = root.first.view.resources.displayMetrics.density
                    root.first.view.viewTreeObserver.addOnDrawListener(drawListener)
                    activity.window.addOnFrameMetricsAvailableListener(frameListener, Handler(frameThread.looper))
                }
                val ids = mutableListOf<Int>()
                fun place(grab: JSONObject, index: Int) {
                    val position = JSONArray(listOf(workspace.width / density * (.22f + .56f * index / 4), workspace.height / density * .55f))
                    val press = if (grab.getString("kind") == "bubble") {
                        (query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())) as JSONObject)
                            .array("placements").objects().single { it.getInt("id") == grab.getInt("id") }.getJSONArray("center")
                    } else position
                    for ((phase, point) in listOf("down" to press, "move" to position, "up" to position)) action(obj("type" to "bubble", "action" to obj("type" to "drag",
                        "grab" to grab, "phase" to phase, "position" to point,
                        "viewport" to JSONArray(listOf(workspace.width / density, workspace.height / density)))))
                }
                for (i in controls.indices) {
                    val before = packets().map { it.getJSONObject("view").getInt("id") }.toSet()
                    place(obj("kind" to "header", "id" to i + 2), i)
                    ids.add(packets().single { it.getJSONObject("view").getInt("id") !in before }.getJSONObject("view").getInt("id"))
                }
                if (hdr) {
                    check(host.snapshot!!.getJSONObject("color_panel").getBoolean("hdr"))
                    action(obj("type" to "bubble", "action" to obj("type" to "set_options", "id" to ids.first(), "options" to obj(
                        "size" to 216, "recent" to true, "palette" to true, "pair" to true, "full" to false, "fold" to true, "swap" to false))))
                }
                SystemClock.sleep(1000)
                output.resolve("fixture.json").writeText(obj("theme" to theme, "width" to width, "height" to height, "photo" to photo.path,
                    "bubbles" to JSONArray(packets()), "workspace" to state().getJSONObject("workspace"), "transparency" to state().getJSONObject("settings").getString("transparency")).toString())
                awaitBenchmarkRefreshPolicy(args, output, instrumentation, host, activity.window.decorView)
                scenario.onActivity { assertSame("Policy setup retains the private Activity", activity, it) }
                if (workload == "palette-opening") {
                    val handler = checkNotNull(host.headerKeyHandler)
                    val inputField = checkNotNull(handler.javaClass.declaredFields.singleOrNull { it.type == BubbleInteraction::class.java }) { "Bubble frame-loop owner missing: ${handler.javaClass.declaredFields.map { it.type.name }}" }.apply { isAccessible = true }
                    val interaction = inputField.get(handler) as BubbleInteraction
                    val colorId = packets().single { it.getJSONObject("view").getString("kind") == "color" }.getJSONObject("view").getInt("id")
                    action(obj("type" to "bubble", "action" to obj("type" to "option", "id" to colorId, "option" to obj("kind" to "palette"))))
                    val power = instrumentation.targetContext.getSystemService(PowerManager::class.java)
                    val diagnosticTransitions = args.getString("paletteTransitions", "3")!!.toInt()
                    require(diagnosticTransitions in 1..10)
                    val windows = JSONArray()
                    val thermal = power.currentThermalStatus
                    drawn.clear(); frameRows.clear(); host.measurementReport(true)
                    var refresh = Float.MAX_VALUE
                    while (windows.length() < diagnosticTransitions) {
                        ids.forEachIndexed { index, id -> place(obj("kind" to "bubble", "id" to id), index) }
                        val current = (query(obj("type" to "bubble_motion", "now_ns" to System.nanoTime())) as JSONObject).array("placements").objects().single { it.getInt("id") == colorId }.getJSONArray("center")
                        val edge = JSONArray(listOf(workspace.width / density - 4, workspace.height / density - 4))
                        for ((phase, position) in listOf("down" to current, "move" to edge, "up" to edge)) action(obj("type" to "bubble", "action" to obj("type" to "drag", "grab" to obj("kind" to "bubble", "id" to colorId), "phase" to phase, "position" to position, "viewport" to JSONArray(listOf(workspace.width / density, workspace.height / density)))))
                        SystemClock.sleep(150)
                        host.awaitMain("Reset settles before palette opening") { !interaction.active }
                        val view = packets().single { it.getJSONObject("view").getInt("id") == colorId }.getJSONObject("view")
                        check(view.isNull("pill"))
                        val bead = checkNotNull(view.array("beads").objects().singleOrNull { it.getJSONObject("placement").getJSONObject("bead").getString("kind") == "palettes" }) { "Palette bead absent: $view" }.getJSONObject("placement").getJSONArray("center")
                        var begin = 0L
                        instrumentation.runOnMainSync {
                            nodes = ids.map { id -> checkNotNull(findTag("tool-bubble-$id") ?: findTag("bubble-card-$id")).second }
                            val center = nodes[ids.indexOf(colorId)].boundsInRoot.center
                            val point = center + Offset(bead.getDouble(0).toFloat(), bead.getDouble(1).toFloat()) * density
                            val down = SystemClock.uptimeMillis()
                            begin = System.nanoTime(); measuring.set(true)
                            for (phase in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                                val event = motion(MotionEvent.TOOL_TYPE_FINGER, phase, point, down)
                                try { owner!!.view.dispatchTouchEvent(event) } finally { event.recycle() }
                            }
                            refresh = minOf(refresh, owner!!.view.display.refreshRate)
                        }
                        val settled = CountDownLatch(1)
                        var end = 0L
                        instrumentation.runOnMainSync {
                            val clock = Choreographer.getInstance()
                            var observed = interaction.active
                            lateinit var boundary: Choreographer.FrameCallback
                            boundary = Choreographer.FrameCallback {
                                observed = observed || interaction.active
                                if (observed && !interaction.active) { end = System.nanoTime(); settled.countDown() }
                                else clock.postFrameCallback(boundary)
                            }
                            clock.postFrameCallback(boundary)
                        }
                        check(settled.await(10, TimeUnit.SECONDS)) { "Palette opening never settled" }
                        instrumentation.runOnMainSync { measuring.set(false); refresh = minOf(refresh, owner!!.view.display.refreshRate) }
                        check(!packets().single { it.getJSONObject("view").getInt("id") == colorId }.getJSONObject("view").isNull("pill"))
                        windows.put(JSONArray(listOf(begin, end)))
                    }
                    val positions = synchronized(drawn) { drawn.toList() }
                    val distinct = ids.indices.map { index -> positions.map { it[index * 2 + 1] to it[index * 2 + 2] }.toSet().size }
                    contacts.put(obj("run" to 0, "warmup" to false, "workload" to "palette-opening", "diagnostic" to true, "moving_windows_ns" to windows,
                        "begin_ns" to windows.getJSONArray(0).getLong(0), "end_ns" to windows.getJSONArray(windows.length() - 1).getLong(1),
                        "clock" to "MONOTONIC", "display_hz" to refresh, "thermal_before" to thermal, "thermal_after" to power.currentThermalStatus,
                        "distinct_centers" to JSONArray(distinct), "drawn_centers" to JSONArray(positions.map { JSONArray(it.toList()) }), "frame_metrics_proxy" to JSONArray(synchronized(frameRows) { frameRows.map { JSONArray(it.toList()) } }),
                        "publication_metrics" to host.measurementReport()))
                    output.resolve("result.json").writeText(obj("theme" to theme, "debuggable" to BuildConfig.DEBUG, "duration_ms" to duration,
                        "width" to width, "height" to height, "contacts" to contacts).toString())
                    check(distinct[ids.indexOf(colorId)] > 1) { "Palette opening visibly moves its wheel" }
                    return
                }
                for (run in 0..repeats) {
                    val resetBegin = System.nanoTime()
                    ids.forEachIndexed { index, id -> place(obj("kind" to "bubble", "id" to id), index) }
                    val resetEnd = System.nanoTime()
                    SystemClock.sleep(600)
                    lateinit var points: List<Offset>
                    var downAt = 0L
                    var refresh = 0f
                    instrumentation.runOnMainSync {
                        nodes = ids.map { id -> checkNotNull(findTag("tool-bubble-$id") ?: findTag("bubble-card-$id")).second }
                        points = nodes.map { it.boundsInRoot.center }.let { it + it.first() }
                        refresh = owner!!.view.display.refreshRate
                    }
                    var lastCenters = points.dropLast(1)
                    var stableAt = SystemClock.uptimeMillis()
                    host.awaitMain("Five bubble placements settle") {
                        val next = nodes.map { it.boundsInRoot.center }
                        if (next.zip(lastCenters).any { (a, b) -> (a - b).getDistance() > .1f }) stableAt = SystemClock.uptimeMillis()
                        lastCenters = next
                        SystemClock.uptimeMillis() - stableAt >= 200
                    }
                    points = lastCenters + lastCenters.first()
                    val colorPacket = if (hdr) packets().single { it.getJSONObject("view").getInt("id") == ids.first() }.getJSONObject("color") else null
                    val plate = colorPacket?.getJSONObject("bounds")
                    val coreCenter = lastCenters.first() - if (plate == null) Offset.Zero else Offset(
                        plate.number("x") + plate.number("width") / 2, plate.number("y") + plate.number("height") / 2) * density
                    if (hdr && !intensity) {
                        val rim = Offset(0f, (-108f + 3f) * density)
                        points = points.map { it - (lastCenters.first() - coreCenter) + rim }
                    }
                    val geometry = colorPacket?.getJSONObject("intensity")?.getJSONObject("geometry")
                    fun arcPoint(fraction: Float): Offset {
                        val arc = checkNotNull(geometry)
                        val end = arc.number("end_angle")
                        val angle = Math.PI.toFloat() - end - fraction * (Math.PI.toFloat() - 2 * end)
                        return coreCenter + (arc.array("center").bubblePoint() + Offset(kotlin.math.cos(angle), kotlin.math.sin(angle)) * arc.number("radius")) * density
                    }
                    val power = instrumentation.targetContext.getSystemService(PowerManager::class.java)
                    val thermalBefore = power.currentThermalStatus
                    val inputRows = mutableListOf<LongArray>()
                    fun event(action: Int, point: Offset) {
                        val event = motion(MotionEvent.TOOL_TYPE_FINGER, action, point, downAt)
                        try { owner!!.view.dispatchTouchEvent(event) } finally { event.recycle() }
                    }
                    instrumentation.runOnMainSync {
                        downAt = SystemClock.uptimeMillis()
                        val start = if (intensity) arcPoint(.125f) else points.first()
                        event(MotionEvent.ACTION_DOWN, start)
                        event(MotionEvent.ACTION_MOVE, if (intensity) arcPoint(.13f) else start + Offset(12 * density, 0f))
                    }
                    if (expectedRefresh > 0) host.awaitMain("Reference display reaches $expectedRefresh Hz during native drag", 10_000) {
                        owner!!.view.display.refreshRate >= expectedRefresh * .95f
                    }
                    instrumentation.runOnMainSync { refresh = owner!!.view.display.refreshRate }
                    drawn.clear(); frameRows.clear(); host.measurementReport(true)
                    val done = CountDownLatch(1)
                    var begin = 0L
                    var end = 0L
                    lateinit var callback: Choreographer.FrameCallback
                    val name = "bubble-motion-$run"
                    instrumentation.runOnMainSync {
                        val clock = Choreographer.getInstance()
                        begin = System.nanoTime(); measuring.set(true); Trace.beginAsyncSection(name, run)
                        callback = Choreographer.FrameCallback {
                            val now = System.nanoTime()
                            val elapsed = (now - begin) / 1_000_000.0
                            if (elapsed >= duration) { end = now; measuring.set(false); done.countDown() }
                            else {
                                val progress = elapsed / duration * (points.size - 1)
                                val segment = progress.toInt().coerceAtMost(points.size - 2)
                                val point = if (intensity) arcPoint((.125 + .75 * (1 - kotlin.math.abs(elapsed / duration * 2 - 1))).toFloat())
                                    else points[segment] + (points[segment + 1] - points[segment]) * (progress - segment).toFloat()
                                event(MotionEvent.ACTION_MOVE, point)
                                inputRows.add(longArrayOf(now, (point.x * 1000).toLong(), (point.y * 1000).toLong()))
                                clock.postFrameCallback(callback)
                            }
                        }
                        clock.postFrameCallback(callback)
                    }
                    try { assertTrue("Native motion producer completes", done.await(duration / 1000 + 20, TimeUnit.SECONDS)) }
                    finally {
                        instrumentation.runOnMainSync {
                            Choreographer.getInstance().removeFrameCallback(callback); measuring.set(false)
                            Trace.endAsyncSection(name, run); event(MotionEvent.ACTION_UP, if (intensity) arcPoint(.125f) else points.first())
                        }
                    }
                    val positions = synchronized(drawn) { drawn.toList() }
                    val distinct = ids.indices.map { index -> positions.map { it[index * 2 + 1] to it[index * 2 + 2] }.toSet().size }
                    val distinctIntensities = positions.map { it.last() }.toSet().size
                    contacts.put(obj("run" to run, "warmup" to (run == 0), "workload" to workload, "distinct_intensities" to distinctIntensities, "reset_begin_ns" to resetBegin, "reset_end_ns" to resetEnd,
                        "path_px" to JSONArray(points.map { JSONArray(listOf(it.x, it.y)) }), "begin_ns" to begin, "end_ns" to end,
                        "clock" to "MONOTONIC", "display_hz" to refresh, "thermal_before" to thermalBefore,
                        "thermal_after" to power.currentThermalStatus, "inputs" to JSONArray(inputRows.map { JSONArray(it.toList()) }),
                        "drawn_centers" to JSONArray(positions.map { JSONArray(it.toList()) }), "distinct_centers" to JSONArray(distinct),
                        "frame_metrics_proxy" to JSONArray(synchronized(frameRows) { frameRows.map { JSONArray(it.toList()) } }),
                        "publication_metrics" to host.measurementReport()))
                    output.resolve("result.json").writeText(obj("theme" to theme, "debuggable" to BuildConfig.DEBUG, "duration_ms" to duration,
                        "width" to width, "height" to height,
                        "frame_metrics_fields" to JSONArray(listOf("vsync_ns", "total_ns", "deadline_ns", "dropped_reports",
                            "input_ns", "animation_ns", "layout_measure_ns", "draw_ns", "sync_ns", "command_issue_ns", "swap_buffers_ns", "gpu_ns")),
                        "contacts" to contacts).toString())
                    if (intensity) assertTrue("HDR intensity visibly changes: $distinctIntensities", distinctIntensities > 10)
                    else {
                        assertTrue("Dragged bubble visibly moves: $distinct", distinct.first() > 10)
                        assertTrue("All four neighbors visibly move: $distinct", distinct.drop(1).all { it > 1 })
                    }
                }
            } finally {
                instrumentation.runOnMainSync {
                    measuring.set(false)
                    owner?.view?.viewTreeObserver?.removeOnDrawListener(drawListener)
                    activity.window.removeOnFrameMetricsAvailableListener(frameListener)
                }
                frameThread.quitSafely()
                action(obj("type" to "restore_workspace", "workspace" to saved))
            }
        }
    }
}
