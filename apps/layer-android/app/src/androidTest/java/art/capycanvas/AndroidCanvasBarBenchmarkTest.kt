package art.capycanvas

import android.os.Handler
import android.os.ParcelFileDescriptor
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import android.view.FrameMetrics
import android.view.InputDevice
import android.view.MotionEvent
import android.view.Window
import android.view.WindowManager
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
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
import kotlin.math.cos
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
        val photoPath = args.getString("photo")
        val zoomOut = args.getString("zoomOut", "true") == "true"
        val effectZoom = args.getString("effectZoom")?.toDouble()
        val transparency = listOf("off", "low", "medium", "high").indexOf(args.getString("transparency", "low"))
        val only = args.getString("scenarios")?.split(',')
        val refine = args.getString("refine", "feather")!!
        val refineSpan = args.getString("refineSpan", ".5")!!.toDouble()
        val refineBar = args.getString("refineBar", "on") == "on"
        val blending = args.getString("blending")
        val memory = args.getString("memory") == "true"
        val materialWatercolor = args.getString("materialWatercolor") == "true"
        if (args.getString("composeTrace") == "true") @OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)
            androidx.compose.runtime.Composer.setTracer(object : androidx.compose.runtime.CompositionTracer {
                override fun isTraceInProgress() = android.os.Trace.isEnabled()
                override fun traceEventStart(key: Int, dirty1: Int, dirty2: Int, info: String) = android.os.Trace.beginSection(info.take(120))
                override fun traceEventEnd() = android.os.Trace.endSection()
            })
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            device.landscape(scenario)
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            var transformEntry: JSONObject? = null
            var snapNeighbor: Long? = null
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(label: String, condition: () -> Boolean) = host.awaitMain(label, 120_000, condition = condition)
            fun action(value: JSONObject) = host.drain(value, 30)
            fun state() = host.snapshot!!.getJSONObject("state")
            fun waitGuide(label: String) {
                waitFor(label) { state().getJSONObject("layer_properties").optString("description") != "Updating…" }
                check(state().getJSONObject("layer_properties").optString("description") != "Could not update this adjustment.")
                val deadline = SystemClock.uptimeMillis() + 120_000
                while (native { Native.renderingPending(it) }) {
                    check(SystemClock.uptimeMillis() < deadline) { "$label raster work did not finish" }
                    SystemClock.sleep(16)
                }
            }
            fun invoke(command: String) {
                val deadline = SystemClock.uptimeMillis() + 120_000
                while (native { Native.query(it, obj("type" to "command_reason", "command" to command).toString()) } != "null") {
                    check(SystemClock.uptimeMillis() < deadline) { "$command stayed unavailable" }
                    SystemClock.sleep(16)
                }
                action(obj("type" to "invoke", "command" to command))
            }
            var documentExtent = "${width}x$height"
            fun openProject(file: File) {
                val task = native { handle ->
                    val (id, request) = documentRequest(handle, "open_document")
                    Native.projectTask(handle, id, "null", request.getLong("epoch"), request.getLong("revision"))
                }
                try {
                    Native.projectWork(task, android.os.ParcelFileDescriptor.open(file, android.os.ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                    val deadline = SystemClock.uptimeMillis() + 120_000
                    while (!native { Native.projectParkReady(it, task) }) {
                        check(SystemClock.uptimeMillis() < deadline) { "Project did not become ready" }
                        host.documentChanged(); SystemClock.sleep(16)
                    }
                    native { Native.projectAdopt(it, task, "null") }
                } finally { Native.projectFree(task) }
                host.documentChanged()
                waitFor("project canvas ready") { host.snapshot?.optBoolean("brush_ready") == true }
            }
            fun newDocument(extent: Pair<Int, Int> = width to height) {
                documentExtent = "${extent.first}x${extent.second}"
                val inputProject = args.getString("project")
                if (inputProject == null) host.newDocument(extent.first, extent.second) else openProject(File(inputProject))
                blending?.let { invoke("blend_$it") }
                invoke("fit_canvas")
                if (zoomOut) invoke("zoom_out")
                SystemClock.sleep(800)
            }
            fun photo(): File {
                if (photoPath != null) return File(photoPath).also { file ->
                    check(file.isFile)
                    val size = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    android.graphics.BitmapFactory.decodeFile(file.path, size)
                    check(size.outWidth == width && size.outHeight == height)
                }
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
            fun photoDocument(): Long {
                val file = photo()
                host.openDocument(file)
                waitFor("tier photo document") { state().array("tabs").objects().any {
                    it.optBoolean("active") && it.optInt("width") == width && it.optInt("height") == height } }
                documentExtent = "${width}x$height"
                blending?.let { invoke("blend_$it") }
                val photoLayer = state().array("layers").objects().single { it.optBoolean("editing") }.getLong("id")
                for (layer in state().array("layers").objects().filter { it.getLong("id") != photoLayer })
                    action(obj("type" to "set_layer_visibility", "id" to layer.getLong("id"), "visible" to false))
                invoke("add_layer")
                invoke("fit_canvas")
                return photoLayer
            }
            fun lookupDocument(): Pair<Long, Long> {
                openProject(File(requireNotNull(args.getString("lookupProject"))))
                waitFor("lookup photo document") { host.snapshot?.optBoolean("shaders_ready") == true &&
                    state().array("tabs").objects().any { it.optBoolean("active") && it.optInt("width") == width && it.optInt("height") == height } }
                documentExtent = "${width}x$height"
                val layers = state().array("layers").objects()
                val photoLayer = layers.single { it.getString("label") == "Photo" }.getLong("id")
                val lookupLayer = layers.single { it.getString("label") == "Owned N65 lookup" }.getLong("id")
                action(obj("type" to "select_layer", "id" to photoLayer))
                invoke("add_layer")
                invoke("fit_canvas")
                return photoLayer to lookupLayer
            }
            fun anchorPoint(fraction: Double): Pair<Double, Double> {
                val anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor")
                val camera = state().getJSONObject("camera")
                val zoom = camera.getDouble("zoom"); val translation = camera.getJSONArray("translation")
                return (anchor.getDouble(0) + fraction * (anchor.getDouble(2) - anchor.getDouble(0))) * zoom + translation.getDouble(0) to
                    (anchor.getDouble(1) + fraction * (anchor.getDouble(3) - anchor.getDouble(1))) * zoom + translation.getDouble(1)
            }
            fun corner() = anchorPoint(1.0)
            var measuring = false
            var lastDownInjectionNs = 0L
            var lastUpInjectionBootNs = 0L
            var firstDownInjectionBootNs = 0L
            fun inject(action: Int, down: Long, x: Double, y: Double, pressure: Float) {
                if (action == MotionEvent.ACTION_DOWN && measuring && firstDownInjectionBootNs == 0L) firstDownInjectionBootNs = SystemClock.elapsedRealtimeNanos()
                if (action == MotionEvent.ACTION_UP) lastUpInjectionBootNs = SystemClock.elapsedRealtimeNanos()
                val injectionBegan = System.nanoTime()
                val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 5; toolType = MotionEvent.TOOL_TYPE_STYLUS })
                val coords = arrayOf(MotionEvent.PointerCoords().apply {
                    this.x = x.toFloat() + host.surfaceOrigin.x; this.y = y.toFloat() + host.surfaceOrigin.y; this.pressure = pressure
                })
                val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
                try { check(instrumentation.uiAutomation.injectInputEvent(event, action != MotionEvent.ACTION_MOVE) || action == MotionEvent.ACTION_CANCEL) {
                    "Stylus injection failed: $event"
                } } finally { event.recycle(); if (action == MotionEvent.ACTION_DOWN) lastDownInjectionNs = System.nanoTime() - injectionBegan }
            }
            var mark = 0L
            var dispatched = 0L
            val gestureDown = java.util.concurrent.atomic.AtomicLong()
            fun drag(start: Pair<Double, Double>, milliseconds: Int, path: (Double) -> Pair<Double, Double>) {
                val initial = path(0.0)
                check(initial.first == 0.0 && initial.second == 0.0) { "The gesture must press its requested handle" }
                val count = (milliseconds / interval).toInt()
                val began = System.nanoTime(); val down = SystemClock.uptimeMillis()
                for (i in 0..count) {
                    val left = began + (i * interval * 1e6).toLong() - System.nanoTime()
                    if (left > 0) java.util.concurrent.locks.LockSupport.parkNanos(left)
                    val (dx, dy) = path(i * interval / 1000.0)
                    if (i == 0 && measuring) { mark = System.nanoTime(); gestureDown.set(mark) }
                    inject(if (i == 0) MotionEvent.ACTION_DOWN else if (i == count) MotionEvent.ACTION_UP else MotionEvent.ACTION_MOVE,
                        down, start.first + dx, start.second + dy, if (i == count) 0f else .7f)
                    if (i == 0 && measuring) dispatched = System.nanoTime()
                }
            }
            fun drags(milliseconds: Int) {
                val began = SystemClock.uptimeMillis()
                var index = 0
                while (SystemClock.uptimeMillis() - began < milliseconds) {
                    val start = corner()
                    android.os.Trace.beginAsyncSection("capy-drag", ++index)
                    drag(start, 400) { t -> (60 * (cos(2 * PI * t / .4) - 1)) to (40 * (cos(2 * PI * t / .4) - 1)) }
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
                val sync: Long, val issue: Long, val swap: Long, val gpu: Long, val vsync: Long)
            val uiFrames = mutableListOf<UiFrame>()
            val metricsThread = HandlerThread("canvas-bar-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
                if (measuring) synchronized(uiFrames) { uiFrames.add(UiFrame(metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                    metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION), metrics.getMetric(FrameMetrics.DRAW_DURATION),
                    metrics.getMetric(FrameMetrics.ANIMATION_DURATION), metrics.getMetric(FrameMetrics.UNKNOWN_DELAY_DURATION),
                    metrics.getMetric(FrameMetrics.SYNC_DURATION), metrics.getMetric(FrameMetrics.COMMAND_ISSUE_DURATION),
                    metrics.getMetric(FrameMetrics.SWAP_BUFFERS_DURATION), metrics.getMetric(FrameMetrics.GPU_DURATION),
                    metrics.getMetric(FrameMetrics.INTENDED_VSYNC_TIMESTAMP))) }
            }
            activity.window.addOnFrameMetricsAvailableListener(listener, Handler(metricsThread.looper))
            val output = File(activity.getExternalFilesDir(null), "canvas-bar-benchmark").apply {
                check(!exists() || deleteRecursively())
                check(mkdirs())
            }
            fun saveProject(name: String): JSONObject {
                val project = File(output, name)
                val job = native { handle ->
                    Native.dispatch(handle, obj("type" to "invoke", "command" to "save_document_as").toString())
                    Native.dispatch(handle, obj("type" to "close_settings").toString())
                    val current = JSONObject(Native.snapshot(handle)).getJSONObject("state")
                    val request = current.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }; val document = current.getJSONObject("document_file")
                    Native.projectTask(handle, request.getInt("id"), obj("uri" to "test:$name", "name" to name).toString(),
                        document.getLong("epoch"), document.getLong("revision")) to request.getInt("id")
                }
                try {
                    Native.projectWork(job.first, android.os.ParcelFileDescriptor.open(project,
                        android.os.ParcelFileDescriptor.MODE_CREATE or android.os.ParcelFileDescriptor.MODE_TRUNCATE or android.os.ParcelFileDescriptor.MODE_READ_WRITE).detachFd(), 0, 0)
                    native { Native.documentComplete(it, job.second, true, "null") }
                } finally { Native.projectFree(job.first) }
                return packageManifest(project.readBytes())
            }
            fun recordProcessMemory(label: String) {
                if (!memory) return
                val maps = File("/proc/self/maps").readLines()
                File(output, "$label-maps.txt").writeText(maps.joinToString("\n"))
                File(output, "$label-memory.json").writeText(obj("boot_ns" to android.os.SystemClock.elapsedRealtimeNanos(),
                    "pss_bytes" to android.os.Debug.getPss().toLong() * 1024,
                    "mappings" to maps.size, "renderer" to native { JSONObject(Native.rendererMemory(it)) }).toString(2))
            }
            fun finalBake() {
                check(materialWatercolor) { "Final bake requires the native watercolor workload" }
                action(obj("type" to "set_tool_setting", "id" to "transform_x", "value" to 16.0))
                invoke("apply_transform")
                val retained = saveProject("bake-retained.capy")
                val owner = retained.occurrenceRecords().objects().first {
                    it.getJSONObject("data").getJSONObject("content").has("paint") && it.getJSONObject("data").authoredAffine().let { pose ->
                        (0 until 6).map { pose.getDouble(it) } != listOf(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                    }
                }.getString("id")
                fun raster(project: JSONObject) = project.paintData(owner)
                val style = raster(retained).getJSONObject("material").getJSONObject("watercolor").toString()
                check(state().array("commands").objects().any { it.getString("id") == "apply_transform_pixels" && it.getBoolean("enabled") })
                val samples = JSONArray()
                fun sample() {
                    val info = android.app.ActivityManager.MemoryInfo()
                    activity.getSystemService(android.app.ActivityManager::class.java).getMemoryInfo(info)
                    val tracked = native { JSONObject(Native.rendererMemory(it)) }
                    samples.put(obj("boot_ns" to android.os.SystemClock.elapsedRealtimeNanos(),
                        "pss_bytes" to android.os.Debug.getPss().toLong() * 1024,
                        "allocated_bytes" to tracked.getLong("allocated_bytes"), "reserved_bytes" to tracked.getLong("reserved_bytes"),
                        "system_available_bytes" to info.availMem, "system_low_memory" to info.lowMemory))
                }
                sample()
                val began = android.os.SystemClock.elapsedRealtimeNanos()
                instrumentation.runOnMainSync { host.dispatch(obj("type" to "invoke", "command" to "apply_transform_pixels")) }
                var pendingPublished: Long? = null
                val deadline = SystemClock.uptimeMillis() + 300_000
                while (true) {
                    check(host.failure == null) { host.failure ?: "Renderer failed" }
                    val current = state()
                    val disabled = current.array("commands").objects().any { it.getString("id") == "apply_transform_pixels" && !it.getBoolean("enabled") }
                    val pending = !current.isNull("canvas_bar")
                    if (disabled && pending && pendingPublished == null) pendingPublished = android.os.SystemClock.elapsedRealtimeNanos()
                    if (disabled && !pending) break
                    check(SystemClock.uptimeMillis() < deadline) { "Final bake did not complete: $current" }
                    sample(); SystemClock.sleep(100)
                }
                val completed = android.os.SystemClock.elapsedRealtimeNanos()
                sample()
                val baked = saveProject("bake-completed.capy")
                check(baked.originalImages().length() == 0) { "The retained source remains after bake" }
                val properties = baked.packageData(owner)
                check((0 until 6).map { properties.authoredAffine().getDouble(it) } == listOf(1.0, 0.0, 0.0, 1.0, 0.0, 0.0))
                check(raster(baked).getJSONObject("material").getJSONObject("watercolor").toString() == style)
                val planes = raster(baked).array("tiles").objects().map { it.getString("plane") }
                check("color" in planes && "watercolor_wetness" in planes) { "Baked native material is missing" }
                openProject(File(output, "bake-completed.capy"))
                val reopened = saveProject("bake-reopened.capy")
                fun digests(project: JSONObject) = project.rasterResources().objects().map {
                    JSONObject(it.toString()).toString()
                }.sorted()
                check(digests(baked) == digests(reopened)) { "Baked native backing changed on reopen" }
                check(raster(baked).toString() == raster(reopened).toString()) { "Baked material changed on reopen" }
                File(output, "final-bake.json").writeText(obj("canvas" to documentExtent,
                    "dispatch_boot_ns" to began, "pending_published_boot_ns" to pendingPublished,
                    "completed_boot_ns" to completed, "elapsed_ms" to (completed - began) / 1e6,
                    "samples" to samples, "native_plane_counts" to obj("Color" to planes.count { it == "Color" },
                        "WatercolorWetness" to planes.count { it == "WatercolorWetness" }), "reopen_exact" to true).toString(2))
            }
            fun quantiles(values: List<Double>): JSONObject {
                val sorted = values.sorted()
                fun at(fraction: Double) = if (sorted.isEmpty()) JSONObject.NULL else Math.round(sorted[((sorted.size - 1) * fraction).toInt()] * 1000) / 1000.0
                return obj("n" to sorted.size, "p50" to at(.5), "p95" to at(.95), "p99" to at(.99), "max" to at(1.0),
                    "over_8_33" to sorted.count { it > 8.333 })
            }
            var refreshRate = 0f
            fun measure(label: String, operation: () -> Unit) {
                if (args.getString("labels")?.split(',')?.let { label !in it } == true) return
                mark = 0L
                gestureDown.set(0)
                firstDownInjectionBootNs = 0L; lastUpInjectionBootNs = 0L
                SystemClock.sleep(600)
                host.measurementReport(true)
                native { Native.completionTimings(it, true) }
                val rendererBefore = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
                val memoryBefore = if (memory) native { JSONObject(Native.rendererMemory(it)) } else null
                val displayBefore = native { JSONObject(Native.displayStatus(it)) }
                check(displayBefore.getInt("overview_count") == if (args.getString("panel") == "navigator") 1 else 0) { "Navigator visibility differs from the requested workload" }
                val cameraBefore = JSONObject(state().getJSONObject("camera").toString())
                val visibleLayerIdsBefore = state().array("layers").objects().filter { it.optBoolean("visible") }.map { it.getLong("id") }
                synchronized(uiFrames) { uiFrames.clear() }
                val bars = mutableListOf<Boolean>()
                val anchorBefore = state().optJSONObject("canvas_bar")?.optJSONArray("anchor")
                measuring = true
                val began = System.nanoTime()
                val beganBoot = SystemClock.elapsedRealtimeNanos()
                val sampler = Thread {
                    while (measuring) { instrumentation.runOnMainSync { bars.add(host.canvasBar != null && host.canvasBarVisible) }; SystemClock.sleep(8) }
                }.apply { start() }
                android.os.Trace.beginAsyncSection("canvas-bar-$label", 1)
                try { operation() } finally { android.os.Trace.endAsyncSection("canvas-bar-$label", 1) }
                val operated = System.nanoTime()
                val operatedBoot = SystemClock.elapsedRealtimeNanos()
                val displayAfterInput = native { JSONObject(Native.displayStatus(it)) }
                val deadline = SystemClock.uptimeMillis() + 120_000
                do {
                    check(SystemClock.uptimeMillis() < deadline) { "$label did not finish its raster work" }
                    SystemClock.sleep(10)
                } while (native { Native.renderingPending(it) })
                measuring = false; sampler.join()
                val ended = System.nanoTime()
                val drained = native { JSONObject(Native.displayStatus(it)) }
                instrumentation.runOnMainSync { refreshRate = activity.window.decorView.display.refreshRate }
                val completions = native { JSONArray(Native.completionTimings(it, false)) }
                val metrics = host.measurementReport(false)
                val frames = metrics.getJSONArray("frames").let { a -> (0 until a.length()).map { a.getJSONArray(it) } }
                    .filter { it.getLong(1) in began..ended }
                val submitted = frames.filter { it.getLong(1) < operated && it.getLong(6) > 0 }
                val rows = (0 until completions.length()).map { completions.getJSONArray(it) }.filter { it.getLong(1) in began..ended }
                val completedAt = rows.map { it.getLong(2) }.filter { it < operated }.sorted()
                val ui = synchronized(uiFrames) { uiFrames.filter { it.vsync < operated } }
                val vsyncs = ui.map { it.vsync }.filter { it <= operated }.distinct().sorted()
                val seconds = (operated - began) / 1e9
                val result = obj("label" to label, "display_hz" to refreshRate, "seconds" to seconds, "transparency" to transparency,
                    "canvas" to documentExtent, "debuggable" to BuildConfig.DEBUG, "material_watercolor" to materialWatercolor,
                    "transform_entry" to transformEntry,
                    "snap_neighbor_layer" to snapNeighbor,
                    "visible_layer_ids_before" to JSONArray(visibleLayerIdsBefore), "visible_layer_count_before" to visibleLayerIdsBefore.size,
                    "photo" to (photoPath ?: "synthetic"), "renderer_profile" to (args.getString("rendererProfile") == "true"), "camera_before" to cameraBefore,
                    "motion" to obj("begin_ns" to began, "end_ns" to operated,
                        "begin_boot_ns" to beganBoot, "end_boot_ns" to operatedBoot,
                        "contact_begin_boot_ns" to (firstDownInjectionBootNs.takeIf { it > 0 } ?: JSONObject.NULL),
                        "contact_end_boot_ns" to (lastUpInjectionBootNs.takeIf { it > 0 } ?: JSONObject.NULL)),
                    "drained_ns" to ended, "display_after_drain" to drained, "renderer_before" to rendererBefore,
                    "measurements" to metrics, "completions" to completions,
                    "memory_before" to memoryBefore,
                    "process_pss_after_bytes" to if (memory) android.os.Debug.getPss().toLong() * 1024 else null,
                    "memory_after" to if (memory) native { JSONObject(Native.rendererMemory(it)) } else null,
                    "renderer_after" to native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) },
                    "display_before" to displayBefore, "display_after_input" to displayAfterInput,
                    "photo_source" to (photoPath ?: "generated"),
                    "bar_visible_fraction" to if (bars.isEmpty()) 0.0 else bars.count { it } / bars.size.toDouble(),
                    "bar_transitions" to bars.zipWithNext().count { (a, b) -> a != b },
                    "anchor_before" to anchorBefore,
                    "anchor_after" to state().optJSONObject("canvas_bar")?.optJSONArray("anchor"),
                    "renderer_submitted_hz" to submitted.size / seconds,
                    "gpu_completed_hz" to completedAt.size / seconds,
                    "renderer_cpu_callback_ms" to quantiles(submitted.map { it.getLong(10) / 1e6 }),
                    "renderer_owner_cpu_ms" to quantiles(submitted.map { it.getLong(17) / 1e6 }),
                    "gpu_submit_to_complete_ms" to quantiles(rows.map { (it.getLong(2) - it.getLong(1)) / 1e6 }),
                    "gpu_completion_interval_ms" to quantiles(completedAt.zipWithNext { a, b -> (b - a) / 1e6 }),
                    "ui_frames" to ui.size,
                    "ui_hz" to if (vsyncs.size < 2) 0.0 else (vsyncs.size - 1) * 1e9 / (vsyncs.last() - vsyncs.first()),
                    "ui_interval_ms" to quantiles(vsyncs.zipWithNext { a, b -> (b - a) / 1e6 }),
                    "ui_frame_ms" to quantiles(ui.map { it.total / 1e6 }),
                    "ui_layout_ms" to quantiles(ui.map { it.layout / 1e6 }),
                    "ui_draw_ms" to quantiles(ui.map { it.draw / 1e6 }),
                    "ui_animation_ms" to quantiles(ui.map { it.animation / 1e6 }),
                    "ui_cpu_ms" to quantiles(ui.map { (it.animation + it.layout + it.draw + it.sync + it.issue) / 1e6 }),
                    "ui_delay_ms" to quantiles(ui.map { it.delay / 1e6 }),
                    "ui_command_issue_ms" to quantiles(ui.map { it.issue / 1e6 }),
                    "ui_swap_ms" to quantiles(ui.map { it.swap / 1e6 }),
                    "ui_gpu_ms" to quantiles(ui.map { it.gpu / 1e6 }),
                    "glass_regions" to native { JSONObject(Native.displayStatus(it)).optInt("glass_regions", -1) },
                    "after_mark" to if (mark == 0L) JSONObject.NULL else obj(
                        "dispatch_ms" to (dispatched - mark) / 1e6,
                        "first_frames" to JSONArray(frames.filter { it.getLong(1) >= mark }.take(4).map {
                            obj("start_ms" to (it.getLong(1) - mark) / 1e6, "cpu_callback_ms" to it.getLong(10) / 1e6,
                                "owner_cpu_ms" to it.getLong(17) / 1e6, "submitted" to (it.getLong(6) > 0))
                        }),
                        "first_completions_ms" to JSONArray(completedAt.filter { it >= mark }.take(3).map { (it - mark) / 1e6 }),
                        "later_completion_interval_ms" to quantiles(completedAt.filter { it >= mark }.drop(3).zipWithNext { a, b -> (b - a) / 1e6 })))
                File(output, "$label.json").writeText(JSONObject(result.toString()).put("frame_fields", metrics.getJSONArray("frame_fields")).put("frames", JSONArray(frames)).put("completions", JSONArray(rows)).toString(2))
                Log.i("CapyBarPerf", result.toString())
                println("CANVAS BAR $label $result")
            }
            fun wanted(name: String) = only == null || name in only
            fun controlState(tag: String): JSONObject {
                val current = state()
                val properties = current.getJSONObject("layer_properties")
                val tonal = current.getJSONObject("tonal_histogram")
                return obj("boot_ns" to SystemClock.elapsedRealtimeNanos(), "tag" to tag,
                    "document" to current.getJSONObject("document_file"), "layer" to properties.optLong("layer"),
                    "epoch" to properties.optLong("epoch"), "description" to properties.optString("description"),
                    "actions" to properties.array("actions"),
                    "controls" to JSONArray(properties.array("controls").objects().map { obj("key" to it.getString("key"), "value" to it.getJSONObject("value")) }),
                    "sampler" to current.getJSONObject("color_picker"), "notice" to current.opt("notice"),
                    "tonal_status" to tonal.optString("status"), "tonal_source" to tonal.opt("captured_source"))
            }
            fun controlGeometry(pair: Pair<ViewRootForTest, SemanticsNode>?): JSONObject {
                if (pair == null) return obj("found" to false)
                val node = pair.second
                val origin = IntArray(2); pair.first.view.getLocationOnScreen(origin)
                val r = node.boundsInRoot
                val enabled = generateSequence(node) { it.parent }.none { it.config.getOrNull(SemanticsProperties.Disabled) != null }
                return obj("found" to true, "enabled" to enabled,
                    "visible" to (node.size.width > 0 && node.size.height > 0 && r.width >= node.size.width * .9f && r.height >= node.size.height * .9f),
                    "screen_bounds" to JSONArray(listOf(r.left + origin[0], r.top + origin[1], r.right + origin[0], r.bottom + origin[1])),
                    "node_size" to JSONArray(listOf(node.size.width, node.size.height)), "root_origin" to JSONArray(origin.toList()),
                    "surface_origin" to JSONArray(listOf(host.surfaceOrigin.x, host.surfaceOrigin.y)),
                    "text" to node.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text },
                    "focused" to node.config.getOrNull(SemanticsProperties.Focused),
                    "native_click" to (node.config.getOrNull(SemanticsActions.OnClick)?.action != null))
            }
            fun interactionSnapshot(label: String, tag: String, control: () -> Pair<ViewRootForTest, SemanticsNode>? = { findTag(tag) }) {
                runCatching {
                    var window = JSONObject()
                    var snapshot = JSONObject()
                    instrumentation.runOnMainSync {
                        val decor = activity.window.decorView
                        val power = activity.getSystemService(android.content.Context.POWER_SERVICE) as android.os.PowerManager
                        val keyguard = activity.getSystemService(android.content.Context.KEYGUARD_SERVICE) as android.app.KeyguardManager
                        window = obj("package" to activity.packageName, "window_focus" to activity.hasWindowFocus(),
                            "visible" to decor.isShown, "attached" to decor.isAttachedToWindow, "view_focus" to decor.hasFocus(),
                            "interactive" to power.isInteractive, "keyguard_locked" to keyguard.isKeyguardLocked,
                            "device_locked" to keyguard.isDeviceLocked, "input_restricted" to keyguard.inKeyguardRestrictedInputMode())
                        snapshot = controlState(tag).put("geometry", controlGeometry(control()))
                    }
                    val record = obj("window" to window, "state" to snapshot)
                    File(output, "$label.json").writeText(record.toString(2))
                    val bitmap = instrumentation.uiAutomation.takeScreenshot()
                    try {
                        check(bitmap != null) { "Private display screenshot unavailable" }
                        File(output, "$label.png").outputStream().use { check(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)) }
                    } finally { bitmap?.recycle() }
                    record.put("display", native { JSONObject(Native.displayStatus(it)) })
                    File(output, "$label.json").writeText(record.toString(2))
                }.onFailure { File(output, "$label-diagnostic-error.json").writeText(obj("error" to it.toString()).toString(2)) }
            }
            fun settledControl(label: String, control: () -> Pair<ViewRootForTest, SemanticsNode>?,
                observe: (JSONObject) -> Unit = {}): android.graphics.RectF {
                host.awaitMain("$label ready", 10_000, condition = { control() != null })
                runBlocking { kotlinx.coroutines.withContext(androidx.compose.ui.platform.AndroidUiDispatcher.Main) {
                    val node = control()!!.second
                    if (node.boundsInRoot.height < node.size.height * .9f) {
                        val scroll = generateSequence(node.parent) { it.parent }.first { it.config.getOrNull(SemanticsActions.ScrollByOffset) != null }
                        scroll.config[SemanticsActions.ScrollByOffset].invoke(androidx.compose.ui.geometry.Offset(0f, node.positionInRoot.y + node.size.height * .5f - scroll.boundsInRoot.center.y))
                    }
                } }
                var previous: android.graphics.RectF? = null
                var stable = 0
                var bounds = android.graphics.RectF()
                host.awaitMain("$label enabled and settled", 10_000, condition = {
                    val geometry = controlGeometry(control()).put("boot_ns", SystemClock.elapsedRealtimeNanos())
                    if (!geometry.getBoolean("found")) { stable = 0; observe(geometry); false }
                    else {
                        val r = geometry.getJSONArray("screen_bounds")
                        val actual = android.graphics.RectF(r.getDouble(0).toFloat(), r.getDouble(1).toFloat(), r.getDouble(2).toFloat(), r.getDouble(3).toFloat())
                        val enabled = geometry.getBoolean("enabled"); val visible = geometry.getBoolean("visible")
                        stable = if (enabled && visible && actual == previous) stable + 1 else 0
                        previous = actual; bounds = actual
                        observe(geometry.put("stable_polls", stable))
                        enabled && visible && stable >= 3
                    }
                })
                return bounds
            }
            var clickIndex = 0
            fun clickControl(tag: String) {
                val geometry = JSONArray()
                val index = clickIndex++
                val before = controlState(tag)
                var bounds: android.graphics.RectF? = null
                var clicked = false
                var failure: String? = null
                try {
                    fun sampleGeometry(value: JSONObject) { if (geometry.length() < 128) geometry.put(value) }
                    settledControl(tag, { findTag(tag) }, ::sampleGeometry)
                    interactionSnapshot("control-click-$index-$tag-before", tag)
                    val actual = settledControl(tag, { findTag(tag) }, ::sampleGeometry)
                    bounds = actual
                    val point = actual.centerX() - host.surfaceOrigin.x.toDouble() to actual.centerY() - host.surfaceOrigin.y.toDouble()
                    val down = SystemClock.uptimeMillis()
                    inject(MotionEvent.ACTION_DOWN, down, point.first, point.second, .7f)
                    inject(MotionEvent.ACTION_UP, down, point.first, point.second, 0f)
                    clicked = true
                } catch (error: Throwable) { failure = error.toString(); throw error }
                finally {
                    interactionSnapshot("control-click-$index-$tag-after", tag)
                    File(output, "control-click-$index-$tag.json").writeText(obj("before" to before, "after" to controlState(tag),
                        "geometry" to geometry, "clicked" to clicked, "failure" to failure,
                        "click_screen_bounds" to bounds?.let { JSONArray(listOf(it.left, it.top, it.right, it.bottom)) }).toString(2))
                }
            }
            fun precisionSettled(label: String) {
                waitGuide(label)
                waitFor("$label input statistics") {
                    val view = state().getJSONObject("tonal_histogram")
                    val layer = state().getJSONObject("layer_properties").getLong("layer")
                    val source = view.optJSONObject("captured_source")
                    view.optString("status") == "Exact" && !view.isNull("data") &&
                        (source?.optLong("EffectChannels") == layer || source?.optLong("EffectInput") == layer)
                }
            }
            inject(MotionEvent.ACTION_CANCEL, SystemClock.uptimeMillis(), 0.0, 0.0, 0f)
            waitFor("ready") { host.snapshot?.optBoolean("shaders_ready") == true && host.workspaceManager?.optBoolean("ready") == true && host.workspaceManager?.optBoolean("busy") == false }
            if (args.getString("defaultPhoto") == "true") {
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:photographer")) }
                waitFor("Photo workspace") { host.workspaceManager?.optString("id") == "builtin:workspace:photographer" && host.workspaceManager?.optBoolean("busy") == false }
            }
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to transparency)))
            val panels = listOfNotNull(args.getString("panel"), if (args.getString("rendererProfile") == "true") "stats" else null).distinct()
            for (name in listOf("stats", "navigator")) {
                if (args.getString("defaultPhoto") == "true" && panels.isEmpty()) continue
                action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to name, "visible" to (name in panels))))
            }
            for (panel in panels) {
                if (panel !in listOf("stats", "navigator")) action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to true)))
                val group = host.panelGroup(panel)
                action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                if (group.getString("active") != panel) action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to panel))
            }
            val wiggle = { t: Double -> (60 * (cos(2 * PI * t) - 1)) to (40 * (cos(2 * PI * t) - 1)) }
            fun primeTransform(fraction: Double = 1.0, mode: String? = null) {
                val before = state().getJSONObject("canvas_bar").getJSONArray("anchor")
                drag(anchorPoint(fraction), 600, wiggle)
                if (materialWatercolor) File(output, "prime-$fraction.json").writeText(obj(
                    "before" to before, "after" to state().optJSONObject("canvas_bar"),
                    "notice" to state().optJSONObject("notice"), "camera" to state().getJSONObject("camera")).toString(2))
                if (mode == null && (fraction == 1.0 || fraction == .4)) waitFor("priming gesture changes its geometry") {
                    val after = state().getJSONObject("canvas_bar").getJSONArray("anchor")
                    if (fraction == .4) kotlin.math.abs(after.getDouble(0) - before.getDouble(0)) > 100
                    else kotlin.math.abs((after.getDouble(2) - after.getDouble(0)) - (before.getDouble(2) - before.getDouble(0))) > 100
                }
                invoke("reset_transform")
                if (mode != null) {
                    invoke(mode)
                    waitFor("$mode is selected") { state().getJSONArray("commands").objects().any { it.getString("id") == mode && it.getBoolean("selected") } }
                }
                SystemClock.sleep(800)
            }

            if (wanted("ui")) {
                val photoLayer = photoDocument()
                action(obj("type" to "select_layer", "id" to photoLayer))
                invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                waitFor("transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                SystemClock.sleep(1500)
                primeTransform()
                hideAndShow(1200)
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
            if (wanted("local-analysis-export")) {
                val photoLayer = photoDocument()
                action(obj("type" to "select_layer", "id" to photoLayer))
                fun edit(value: JSONObject) = action(obj("type" to "effect", "action" to value))
                edit(obj("op" to "insert", "effect" to "shadows_highlights"))
                val lower = state().getJSONObject("layer_properties").getLong("layer")
                edit(obj("op" to "set", "layer" to lower, "key" to "shadows", "value" to obj("kind" to "number", "value" to 63)))
                edit(obj("op" to "insert", "effect" to "clarity"))
                val upper = state().getJSONObject("layer_properties").getLong("layer")
                edit(obj("op" to "set", "layer" to upper, "key" to "amount", "value" to obj("kind" to "number", "value" to 28)))
                val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
                val recipe = runBlocking { ColorPreferencesStore.presets(activity, color, obj("type" to "get", "index" to 0)) }.getJSONObject("recipe")
                    .put("format", "Png").put("size", "Original").put("resolution", "Master")
                fun export(label: String): JSONObject {
                    val began = SystemClock.elapsedRealtimeNanos()
                    val statusBefore = state().getJSONObject("layer_properties").optString("description")
                    val memoryBefore = native { JSONObject(Native.rendererMemory(it)) }
                    val id = native { documentRequest(it, "export_document").first }
                    var task = 0L
                    val deadline = SystemClock.uptimeMillis() + 120_000
                    while (task == 0L) {
                        check(SystemClock.uptimeMillis() < deadline) { "Exact export capture did not become ready" }
                        task = native { Native.projectExportTask(it, id, System.nanoTime()) }
                        if (task == 0L) { host.documentChanged(); SystemClock.sleep(16) }
                    }
                    val captured = SystemClock.elapsedRealtimeNanos()
                    val statusCaptured = native { Native.dispatch(it, obj("type" to "close_settings").toString()); JSONObject(Native.snapshot(it)!!).getJSONObject("state").getJSONObject("layer_properties").optString("description") }
                    val file = File(output, "$label.png")
                    try {
                        Native.projectExportOptions(task, recipe.toString())
                        Native.projectWork(task, ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(), 0, 0)
                        native { Native.documentComplete(it, id, true, "null") }
                    } catch (error: Exception) {
                        native { Native.documentComplete(it, id, false, JSONObject.quote(error.message ?: "Export failed")) }
                        throw error
                    } finally { Native.projectFree(task) }
                    val completed = SystemClock.elapsedRealtimeNanos()
                    val dimensions = android.graphics.BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    android.graphics.BitmapFactory.decodeFile(file.path, dimensions)
                    check(dimensions.outWidth == width && dimensions.outHeight == height)
                    val digest = java.security.MessageDigest.getInstance("SHA-256")
                    file.inputStream().use { stream ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) { val count = stream.read(buffer); if (count < 0) break; digest.update(buffer, 0, count) }
                    }
                    val result = obj("label" to label, "canvas" to documentExtent, "begin_boot_ns" to began,
                        "captured_boot_ns" to captured, "completed_boot_ns" to completed, "capture_ms" to (captured - began) / 1e6,
                        "worker_ms" to (completed - captured) / 1e6, "guide_status_before" to statusBefore,
                        "guide_status_at_capture" to statusCaptured, "guide_pending_at_capture" to (statusCaptured == "Updating…"),
                        "recipe" to recipe, "file_bytes" to file.length(), "sha256" to digest.digest().joinToString("") { "%02x".format(it) },
                        "memory_before" to memoryBefore, "memory_after" to native { JSONObject(Native.rendererMemory(it)) },
                        "pss_after_bytes" to android.os.Debug.getPss().toLong() * 1024)
                    File(output, "$label.json").writeText(result.toString(2))
                    file.delete()
                    return result
                }
                val cold = export("local-guide-first-export")
                waitGuide("stacked guides published")
                val warm = export("local-guide-warm-export")
                check(cold.getString("sha256") == warm.getString("sha256")) { "Exact stacked export changed without a source edit" }
                action(obj("type" to "select_layer", "id" to photoLayer))
                val sourceEditBegan = SystemClock.elapsedRealtimeNanos()
                action(obj("type" to "set_layer_opacity", "opacity" to .75))
                action(obj("type" to "select_layer", "id" to upper))
                waitGuide("source edit guides published")
                File(output, "local-guide-source-rebuild.json").writeText(obj("begin_boot_ns" to sourceEditBegan,
                    "ready_boot_ns" to SystemClock.elapsedRealtimeNanos(), "lower" to lower, "upper" to upper,
                    "status" to state().getJSONObject("layer_properties").optString("description"),
                    "memory_after" to native { JSONObject(Native.rendererMemory(it)) }).toString(2))
            }
            if (wanted("scopes")) {
                photoDocument(); invoke("hand"); invoke("fit_canvas")
                fun visibility(kind: String, visible: Boolean) {
                    action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to kind, "visible" to visible)))
                    if (visible) {
                        val g = host.panelGroup(kind)
                        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to g.getInt("id"), "collapsed" to false)))
                        if (g.optString("active") != kind) action(obj("type" to "select_panel_tab", "group" to g.getInt("id"), "panel" to kind))
                        host.awaitMain("$kind active", 10_000, condition = { host.panelGroup(kind).optString("active") == kind })
                    }
                }
                fun hide() {
                    visibility("histogram", false); visibility("waveform", false)
                    host.awaitMain("scopes retired", 10_000, condition = {
                        state().getJSONObject("histogram").isNull("data") && state().getJSONObject("waveform").isNull("data")
                    })
                }
                hide()
                fun center(): Pair<Double, Double> {
                    val area = state().getJSONObject("camera").getJSONArray("work_area")
                    return area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                }
                drag(center(), 600) { t -> 120 * sin(t * 2) to 80 * sin(t * 3) }; invoke("fit_canvas"); waitGuide("scope pan primed")
                for (kind in listOf("histogram", "waveform")) {
                    val shown = SystemClock.elapsedRealtimeNanos(); visibility(kind, true)
                    host.awaitMain("$kind demand published", 10_000, condition = { state().getJSONObject(kind).optString("status").isNotEmpty() })
                    waitFor("$kind first data") { !state().getJSONObject(kind).isNull("data") }
                    val preview = SystemClock.elapsedRealtimeNanos()
                    waitFor("$kind exact data") { state().getJSONObject(kind).optString("status") == "Exact" }
                    check(host.scopes[kind]?.let { it.paths.isNotEmpty() || it.image != null } == true)
                    File(output, "scope-$kind-readiness.json").writeText(obj("show_boot_ns" to shown, "first_data_boot_ns" to preview,
                        "exact_boot_ns" to SystemClock.elapsedRealtimeNanos(), "state" to state().getJSONObject(kind),
                        "memory" to native { JSONObject(Native.rendererMemory(it)) }).toString(2))
                    for (phase in listOf("pending", "settled")) repeat(args.getString("effectRepeats", "3")!!.toInt()) { index ->
                        if (phase == "pending") {
                            hide(); visibility(kind, true)
                            host.awaitMain("$kind pending demand published", 10_000, condition = { state().getJSONObject(kind).optString("status").isNotEmpty() })
                        }
                        val revision = state().getJSONObject("document_file").getLong("revision")
                        val phases = java.util.Collections.synchronizedList(mutableListOf<JSONObject>())
                        measure("scope-$kind-$phase-pan") {
                            check((state().getJSONObject(kind).optString("status") == "Exact") == (phase == "settled")) { "$kind $phase admission was lost before contact" }
                            val sampler = Thread {
                                val until = SystemClock.uptimeMillis() + duration
                                while (SystemClock.uptimeMillis() < until) {
                                    phases += obj("boot_ns" to SystemClock.elapsedRealtimeNanos(), "status" to state().getJSONObject(kind).optString("status"))
                                    SystemClock.sleep(8)
                                }
                            }.apply { start() }
                            drag(center(), duration) { t -> 120 * sin(t * 2) to 80 * sin(t * 3) }; sampler.join()
                        }
                        val released = SystemClock.elapsedRealtimeNanos()
                        waitFor("$kind contact exact") { state().getJSONObject(kind).optString("status") == "Exact" }
                        check(state().getJSONObject("document_file").getLong("revision") == revision)
                        val label = "scope-$kind-$phase-pan"
                        val result = File(output, "$label.json")
                        result.writeText(JSONObject(result.readText()).put("initial_scope_phase", phase).put("scope_status_samples", JSONArray(phases)).put("scope", kind)
                            .put("exact_wait_after_raster_drain_ms", (SystemClock.elapsedRealtimeNanos() - released) / 1e6)
                            .put("scope_state", state().getJSONObject(kind)).toString(2))
                        result.copyTo(File(output, "$label-$index.json"), overwrite = true)
                    }
                    hide()
                }
            }
            if (wanted("effects") || wanted("spatial-effects")) {
                data class Scrub(val id: String, val key: String, val title: String, val label: String, val start: Double, val chain: Boolean = false, val page: String? = null, val colorize: Boolean = false, val span: Double = .1, val radius: Double? = null, val primeSpan: Double = .1)
                val scrubs = if (wanted("spatial-effects")) listOf(
                    Scrub("gaussian_blur", "sigma", "Radius", "effect-gaussian-small-drag", .2),
                    Scrub("gaussian_blur", "sigma", "Radius", "effect-gaussian-large-drag", .7),
                    Scrub("unsharp_mask", "amount", "Amount", "effect-unsharp-amount-drag", .3, span = .4, radius = args.getString("effectRadius")?.toDouble() ?: 85.0),
                ) else listOf(
                    Scrub("solid_color", "opacity", "Opacity", "fill-opacity-drag", .35, span = .4),
                    Scrub("levels", "black", "Black", "effect-levels-black-drag", .15, span = .4, primeSpan = .4),
                    Scrub("exposure", "exposure", "Exposure", "effect-exposure-drag", .45),
                    Scrub("hue_saturation", "hue", "Hue", "effect-master-hue-drag", .55),
                    Scrub("hue_saturation", "greens_hue", "Hue", "effect-range-hue-drag", .55, page = "greens"),
                    Scrub("hue_saturation", "colorize_hue", "Hue", "effect-colorize-hue-drag", .15, colorize = true),
                    Scrub("hue_saturation", "colorize_saturation", "Saturation", "effect-colorize-saturation-drag", .35, colorize = true, span = .4),
                    Scrub("threshold", "threshold", "Threshold", "effect-threshold-drag", .35, span = .4),
                    Scrub("photo_filter", "density", "Density", "effect-photo-filter-drag", .35, span = .4),
                    Scrub("selective_color", "neutrals_cyan", "Cyan", "effect-selective-neutrals-cyan-drag", .3, page = "neutrals", span = .4),
                    Scrub("selective_color", "reds_cyan", "Cyan", "effect-selective-reds-cyan-drag", .3, page = "reds", span = .4),
                    Scrub("channel_mixer", "red_green", "Green", "effect-channel-mixer-coefficient-drag", .3, page = "red", span = .4),
                    Scrub("channel_mixer", "red_constant", "Constant", "effect-channel-mixer-constant-drag", .3, page = "red", span = .4),
                    Scrub("color_lookup", "intensity", "Intensity", "effect-color-lookup-intensity-drag", .3, span = .4),
                    Scrub("shadows_highlights", "shadows", "Shadows", "effect-shadows-drag", .3, span = .4),
                    Scrub("shadows_highlights", "highlights", "Highlights", "effect-highlights-drag", .3, span = .4),
                    Scrub("clarity", "amount", "Amount", "effect-clarity-drag", .3, span = .4),
                    Scrub("dehaze", "amount", "Amount", "effect-dehaze-amount-drag", .3, span = .4),
                    Scrub("exposure", "exposure", "Exposure", "effect-chain-exposure-drag", .45, true),
                )
                val selectedLabels = args.getString("labels")?.split(',')
                for (scrub in scrubs.filter { selectedLabels == null || it.label in selectedLabels }) {
                    val preparedAt = System.nanoTime()
                    val lookup = if (scrub.id == "color_lookup") lookupDocument() else null
                    val photoLayer = lookup?.first ?: photoDocument()
                    val sourceMemoryBeforeEffect = if (scrub.id in listOf("shadows_highlights", "clarity", "dehaze")) native { JSONObject(Native.rendererMemory(it)) } else null
                    val sourceDisplayBeforeEffect = if (scrub.id in listOf("shadows_highlights", "clarity", "dehaze")) native { JSONObject(Native.displayStatus(it)) } else null
                    val fixtureVisibleLayerIds = state().array("layers").objects().filter { it.optBoolean("visible") }.map { it.getLong("id") }
                    check(fixtureVisibleLayerIds.size == (if (lookup == null) 2 else 3) && photoLayer in fixtureVisibleLayerIds) { "Effect fixture has unexpected visible layers" }
                    action(obj("type" to "select_layer", "id" to photoLayer))
                    fun effect(op: JSONObject) = action(obj("type" to "effect", "action" to op))
                    if (scrub.chain) {
                        effect(obj("op" to "insert", "effect" to "levels"))
                        effect(obj("op" to "set", "layer" to state().getJSONObject("layer_properties").getLong("layer"),
                            "key" to "gamma", "value" to obj("kind" to "number", "value" to 1.25)))
                        effect(obj("op" to "insert", "effect" to "vibrance"))
                    }
                    val guideBegan = SystemClock.elapsedRealtimeNanos()
                    if (lookup == null) effect(obj("op" to "insert", "effect" to scrub.id))
                    else action(obj("type" to "select_layer", "id" to lookup.second))
                    val effectLayer = state().getJSONObject("layer_properties").getLong("layer")
                    scrub.radius?.let { radius -> effect(obj("op" to "set", "layer" to effectLayer, "key" to "sigma", "value" to obj("kind" to "number", "value" to radius))) }
                    if (scrub.id == "solid_color") action(obj("type" to "layer", "action" to obj("op" to "delete_mask", "id" to effectLayer)))
                    val hasGuide = scrub.id in listOf("shadows_highlights", "clarity", "dehaze")
                    if (hasGuide) {
                        waitGuide("local guide published")
                    }
                    val guideReady = SystemClock.elapsedRealtimeNanos()
                    scrub.page?.let { effect(obj("op" to "select_page", "layer" to effectLayer, "page" to it)) }
                    if (scrub.colorize) effect(obj("op" to "set", "layer" to effectLayer, "key" to "colorize", "value" to obj("kind" to "toggle", "value" to true)))
                    val panel = if (scrub.id == "solid_color") "layers" else "properties"
                    action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to true)))
                    val group = host.panelGroup(panel)
                    action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                    if (group.optString("active") != panel) action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to panel))
                    action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
                    waitFor("panel configuration closed") { state().getJSONObject("customization").isNull("expanded") }
                    fun slider() = if (scrub.id == "solid_color") findTag("layer-opacity")?.let { (root, panel) ->
                        panel.find { it.config.getOrNull(SemanticsProperties.ProgressBarRangeInfo) != null }?.let { root to it }
                    } else findTag("number-value-${scrub.key}")?.let { (root, value) ->
                        generateSequence(value.parent) { it.parent }.firstNotNullOfOrNull { row ->
                            row.find(hasTag("number-slider-${scrub.title}"))?.let { root to it }
                        }
                    }
                    waitFor("${scrub.title} control") { slider() != null }
                    invoke("fit_canvas")
                    effectZoom?.let { action(obj("type" to "set_zoom", "zoom" to it)) }
                    waitFor("filter shaders ready") { host.snapshot?.optBoolean("shaders_ready") == true }
                    var track = android.graphics.RectF()
                    fun locateSlider() {
                        track = settledControl("${scrub.title} slider", ::slider)
                        check(track.width() > 40 && track.height() > 0)
                    }
                    locateSlider()
                    fun value() = if (scrub.id == "solid_color") state().array("layers").objects().first { it.getLong("id") == effectLayer }.getDouble("opacity") * 100
                        else state().getJSONObject("layer_properties").array("controls").objects()
                            .first { it.getString("key") == scrub.key }.getJSONObject("value").getDouble("value")
                    if (scrub.id == "levels" && args.getString("precisionAuto") == "true") {
                        precisionSettled("Levels input ready")
                        effect(obj("op" to "set", "layer" to effectLayer, "key" to "gamma", "value" to obj("kind" to "number", "value" to 2)))
                        check(state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "gamma" }.getJSONObject("value").getDouble("value") == 2.0)
                        val began = SystemClock.elapsedRealtimeNanos()
                        val timeline = JSONArray()
                        var sampledAt = 0L
                        var lastState = ""
                        var adopted: Long? = null
                        var failure: String? = null
                        fun sampleAuto(phase: String, force: Boolean = false) {
                            val sample = controlState("property-action-auto_levels")
                            val content = JSONObject(sample.toString()).apply { remove("boot_ns") }.toString()
                            val now = sample.getLong("boot_ns")
                            if (force || content != lastState || now - sampledAt >= 250_000_000) {
                                if (timeline.length() < 512) timeline.put(sample.put("phase", phase))
                                sampledAt = now; lastState = content
                            }
                        }
                        try {
                            sampleAuto("before_click", true)
                            clickControl("property-action-auto_levels")
                            sampleAuto("after_click", true)
                            waitFor("Auto adopted") {
                                sampleAuto("adoption")
                                val p = state().getJSONObject("layer_properties")
                                p.array("controls").objects().first { it.getString("key") == "gamma" }.getJSONObject("value").getDouble("value") == 1.0 &&
                                    p.array("actions").objects().first { it.getJSONObject("action").getString("op") == "auto_levels" }.getString("label") != "Cancel"
                            }
                            adopted = SystemClock.elapsedRealtimeNanos()
                            invoke("undo")
                            check(state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "gamma" }.getJSONObject("value").getDouble("value") == 2.0)
                            File(output, "levels-auto-adoption.json").writeText(obj("begin_boot_ns" to began, "adopted_boot_ns" to adopted,
                                "elapsed_ms" to (adopted!! - began) / 1e6, "undo_gamma" to 2, "memory" to native { JSONObject(Native.rendererMemory(it)) }).toString(2))
                        } catch (error: Throwable) { failure = error.toString(); throw error }
                        finally {
                            interactionSnapshot("levels-auto-terminal", "property-action-auto_levels")
                            File(output, "levels-auto-timeline.json").writeText(obj("begin_boot_ns" to began, "adopted_boot_ns" to adopted,
                                "failure" to failure, "timeline" to timeline, "final" to controlState("property-action-auto_levels")).toString(2))
                        }
                        effect(obj("op" to "set", "layer" to effectLayer, "key" to "gamma", "value" to obj("kind" to "number", "value" to 1)))
                        precisionSettled("Levels after Auto settled")
                        locateSlider()
                    }
                    val start = track.left + track.width() * scrub.start - host.surfaceOrigin.x to track.centerY() - host.surfaceOrigin.y.toDouble()
                    val before = value()
                    val touchSlop = android.view.ViewConfiguration.get(activity).scaledTouchSlop
                    val primeSpan = minOf(scrub.primeSpan, .95 - scrub.start)
                    if (scrub.id == "levels") check(track.width() * primeSpan > touchSlop * 2)
                    if (scrub.id == "levels") File(output, "levels-preprime-geometry.json").writeText(obj(
                        "boot_ns" to SystemClock.elapsedRealtimeNanos(), "slider_bounds" to JSONArray(listOf(track.left, track.top, track.right, track.bottom)),
                        "surface_origin" to JSONArray(listOf(host.surfaceOrigin.x, host.surfaceOrigin.y)), "down_surface_point" to JSONArray(listOf(start.first, start.second)),
                        "scaled_touch_slop_px" to touchSlop, "density" to activity.resources.displayMetrics.density,
                        "prime_travel_px" to track.width() * primeSpan, "motion_travel_px" to track.width() * scrub.span,
                        "initial_value" to before, "layer_properties" to state().getJSONObject("layer_properties"), "camera" to state().getJSONObject("camera")).toString(2))
                    val sliderTag = if (scrub.id == "solid_color") "layer-opacity" else "number-slider-${scrub.title}"
                    interactionSnapshot("${scrub.label}-prime-before", sliderTag, ::slider)
                    try {
                        drag(start, 250) { t -> track.width() * primeSpan * t / .25 to 0.0 }
                        host.awaitMain("${scrub.title} gesture changes its value", 10_000, condition = { value() != before })
                    } finally { interactionSnapshot("${scrub.label}-prime-after", sliderTag, ::slider) }
                    val warmupDownInjectionMs = lastDownInjectionNs / 1e6
                    invoke("undo")
                    waitFor("filter shaders ready") { host.snapshot?.optBoolean("shaders_ready") == true }
                    if (wanted("spatial-effects")) waitGuide("spatial filter warmup settled")
                    if (scrub.id == "levels") precisionSettled("Levels warmup settled")
                    if (scrub.id == "dehaze") waitGuide("Dehaze warmup settled")
                    val preparationMs = (System.nanoTime() - preparedAt) / 1e6
                    val effectRepeats = args.getString("effectRepeats", "1")!!.toInt().also { require(it > 0) }
                    repeat(effectRepeats) { index ->
                        val settleBegan = SystemClock.elapsedRealtimeNanos()
                        if (wanted("spatial-effects")) waitGuide("spatial filter contact settled")
                        if (scrub.id == "levels") precisionSettled("Levels contact settled")
                        if (scrub.id == "dehaze") waitGuide("Dehaze contact settled")
                        val settleMs = (SystemClock.elapsedRealtimeNanos() - settleBegan) / 1e6
                        val values = java.util.Collections.synchronizedSet(mutableSetOf<Double>())
                        val firstChanged = java.util.concurrent.atomic.AtomicLong()
                        val label = scrub.label
                        measure(label) {
                            val initialValue = value()
                            val sampler = Thread {
                                val until = SystemClock.uptimeMillis() + duration
                                while (SystemClock.uptimeMillis() < until) {
                                    val sampled = value()
                                    values += sampled
                                    if (sampled != initialValue && gestureDown.get() != 0L) firstChanged.compareAndSet(0, System.nanoTime())
                                    SystemClock.sleep(8)
                                }
                            }.apply { start() }
                            drag(start, duration) { t -> track.width() * scrub.span * (1 - kotlin.math.abs(1 - (t % .5) * 4)) to 0.0 }
                            sampler.join()
                        }
                        val result = File(output, "$label.json")
                        result.writeText(JSONObject(result.readText()).put("effect_values", JSONArray(values.toList().sorted()))
                            .put("source_memory_before_effect", sourceMemoryBeforeEffect).put("source_display_before_effect", sourceDisplayBeforeEffect)
                            .put("guide_wait_ms", if (hasGuide) (guideReady - guideBegan) / 1e6 else JSONObject.NULL).put("guide_wait_begin_boot_ns", if (hasGuide) guideBegan else JSONObject.NULL).put("guide_ready_boot_ns", if (hasGuide) guideReady else JSONObject.NULL)
                            .put("settle_before_contact_ms", settleMs).put("warmup_down_injection_ms", warmupDownInjectionMs).put("down_injection_ms", lastDownInjectionNs / 1e6).put("slider_travel_fraction", scrub.span).put("triangle_period_seconds", .5).put("preparation_ms", preparationMs).put("effect_id", scrub.id).put("effect_key", scrub.key).put("effect_page", scrub.page ?: "rgb").put("colorize", scrub.colorize).put("effect_radius", scrub.radius).put("theme", state().getString("theme"))
                            .put("first_value_observed_ns", firstChanged.get().takeIf { it > 0 } ?: JSONObject.NULL)
                            .put("down_to_first_value_observed_ms", firstChanged.get().takeIf { it > 0 && gestureDown.get() > 0 }?.let { (it - gestureDown.get()) / 1e6 } ?: JSONObject.NULL)
                            .put("photo_fixture_visible_layer_ids", JSONArray(fixtureVisibleLayerIds)).put("photo_fixture_visible_layer_count", fixtureVisibleLayerIds.size).put("slider_bounds",
                            JSONArray(listOf(track.left, track.top, track.right, track.bottom))).toString(2))
                        if (effectRepeats > 1) result.copyTo(File(output, "$label-$index.json"), overwrite = true)
                        check(values.size > 1) { "$label did not change its value during motion" }
                        if (scrub.span == .4 && scrub.id == "color_lookup") check(values.max() - values.min() > 25) { "$label did not traverse enough coarse numeric steps" }
                        if (scrub.id == "gaussian_blur") check(values.all { it > 0 && it < 21 }) { "$label reached a stationary slider limit" }
                    }
                }
                if (args.getString("captureFilters") == "true") for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    waitFor("filter canvas ready") { host.snapshot?.optBoolean("brush_ready") == true }
                    SystemClock.sleep(300)
                    val shot = instrumentation.uiAutomation.takeScreenshot()
                    File(output, "filter-$theme.png").outputStream().use { shot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                    shot.recycle()
                }
            }
            if (wanted("pointwise-navigation")) {
                val selectedLabels = args.getString("labels")?.split(',')
                for (id in listOf("none", "invert", "desaturate", "threshold", "photo_filter", "color_lookup", "shadows_highlights", "clarity", "dehaze", "gaussian_blur", "unsharp_mask")) {
                    val label = "effect-$id-pan"
                    if (selectedLabels != null && label !in selectedLabels) continue
                    val preparedAt = System.nanoTime()
                    val lookup = if (id == "color_lookup") lookupDocument() else null
                    val photoLayer = lookup?.first ?: photoDocument()
                    action(obj("type" to "select_layer", "id" to photoLayer))
                    val sourceMemoryBeforeEffect = if (id in listOf("shadows_highlights", "clarity", "dehaze")) native { JSONObject(Native.rendererMemory(it)) } else null
                    val sourceDisplayBeforeEffect = if (sourceMemoryBeforeEffect != null) native { JSONObject(Native.displayStatus(it)) } else null
                    val guideBeganBoot = SystemClock.elapsedRealtimeNanos()
                    val appliedAt = System.nanoTime()
                    if (id != "none" && lookup == null) action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to id)))
                    val radius = args.getString("effectRadius")?.toDouble()
                    val radiusMemoryBefore = if (radius != null && id in listOf("gaussian_blur", "unsharp_mask")) native { JSONObject(Native.rendererMemory(it)) } else null
                    val radiusBegan = SystemClock.elapsedRealtimeNanos()
                    if (id in listOf("gaussian_blur", "unsharp_mask") && radius != null) {
                        if (radius == 85.0) {
                            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to true)))
                            val group = host.panelGroup("properties")
                            action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                            if (group.optString("active") != "properties") action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to "properties"))
                            waitFor("Radius value control") { findTag("number-value-sigma") != null }
                            instrumentation.runOnMainSync { check(findTag("number-value-sigma")!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
                            waitFor("Radius text field") { findTag("number-Radius") != null }
                            instrumentation.runOnMainSync {
                                val field = findTag("number-Radius")!!.second
                                check(field.config[SemanticsActions.SetText].action!!.invoke(androidx.compose.ui.text.AnnotatedString("85")))
                            }
                            host.awaitMain("Radius text entered", 10_000, condition = { findTag("number-Radius")?.second?.config?.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.EditableText)?.text == "85" })
                            instrumentation.runOnMainSync { check(findTag("number-Radius")!!.second.config[SemanticsActions.OnImeAction].action!!.invoke()) }
                            waitFor("Typed Radius applied") { state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "sigma" }.getJSONObject("value").getDouble("value") == radius }
                        } else action(obj("type" to "effect", "action" to obj("op" to "set",
                            "layer" to state().getJSONObject("layer_properties").getLong("layer"), "key" to "sigma", "value" to obj("kind" to "number", "value" to radius))))
                    }
                    if (radiusMemoryBefore != null) waitGuide("radius change raster complete")
                    val radiusCompleted = SystemClock.elapsedRealtimeNanos()
                    val radiusMemoryAfter = if (radiusMemoryBefore != null) native { JSONObject(Native.rendererMemory(it)) } else null
                    val hasGuide = id in listOf("shadows_highlights", "clarity", "dehaze")
                    if (hasGuide) {
                        val layer = state().getJSONObject("layer_properties").getLong("layer")
                        action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer,
                            "key" to if (id in listOf("clarity", "dehaze")) "amount" else "shadows", "value" to obj("kind" to "number", "value" to if (id in listOf("clarity", "dehaze")) 28 else 63))))
                        waitGuide("navigation guide published")
                    }
                    val guideReadyBoot = SystemClock.elapsedRealtimeNanos()
                    val applicationMs = (System.nanoTime() - appliedAt) / 1e6
                    invoke("hand"); invoke("fit_canvas")
                    waitFor("filter shaders ready") { host.snapshot?.optBoolean("shaders_ready") == true }
                    val preparationMs = (System.nanoTime() - preparedAt) / 1e6
                    val area = state().getJSONObject("camera").getJSONArray("work_area")
                    val center = area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                    if (id in listOf("gaussian_blur", "unsharp_mask", "dehaze")) {
                        drag(center,250) { t -> 120*sin(t*2) to 80*sin(t*3) }
                        invoke("fit_canvas")
                        if (id == "dehaze") waitGuide("Dehaze navigation warmup settled")
                    }
                    repeat(args.getString("effectRepeats", "1")!!.toInt()) { index ->
                        if (id == "dehaze") waitGuide("Dehaze navigation contact settled")
                        measure(label) { drag(center, duration) { t -> 120 * sin(t * 2) to 80 * sin(t * 3) } }
                        val result = File(output, "$label.json")
                        result.writeText(JSONObject(result.readText()).put("effect_id", id)
                            .put("theme", state().getString("theme")).put("effect_radius", radius).put("preparation_ms", preparationMs)
                            .put("radius_input", if (radius == 85.0) "native text field" else "shared action")
                            .put("radius_change_ms", if (radiusMemoryBefore != null) (radiusCompleted - radiusBegan) / 1e6 else JSONObject.NULL)
                            .put("radius_memory_before", radiusMemoryBefore).put("radius_memory_after", radiusMemoryAfter)
                            .put("source_memory_before_effect", sourceMemoryBeforeEffect).put("source_display_before_effect", sourceDisplayBeforeEffect)
                            .put("guide_wait_begin_boot_ns", if (hasGuide) guideBeganBoot else JSONObject.NULL)
                            .put("guide_ready_boot_ns", if (hasGuide) guideReadyBoot else JSONObject.NULL)
                            .put("guide_wait_ms", if (hasGuide) (guideReadyBoot - guideBeganBoot) / 1e6 else JSONObject.NULL)
                            .put("shared_application_and_drain_ms", applicationMs).toString(2))
                        result.copyTo(File(output, "$label-$index.json"), overwrite = true)
                    }
                }
            }
            if (wanted("targeted")) {
                val photoLayer = photoDocument(); action(obj("type" to "select_layer", "id" to photoLayer))
                action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "curves")))
                action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to true)))
                val group = host.panelGroup("properties")
                action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                if (group.optString("active") != "properties") action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to "properties"))
                invoke("fit_canvas"); precisionSettled("Targeted input ready")
                val camera = state().getJSONObject("camera"); val translation = camera.getJSONArray("translation")
                val center = translation.getDouble(0) + width * .5 * camera.getDouble("zoom") to translation.getDouble(1) + height * .5 * camera.getDouble("zoom")
                fun controls() = state().getJSONObject("layer_properties").array("controls").objects().map { it.getJSONObject("value").toString() }
                fun arm() {
                    clickControl("property-action-target_curve")
                    try { host.awaitMain("Targeted sampler armed", 10_000, condition = { state().getJSONObject("color_picker").optBoolean("calibrating") }) }
                    finally {
                        interactionSnapshot("targeted-arm-terminal", "property-action-target_curve")
                        File(output, "targeted-arm-state.json").writeText(controlState("property-action-target_curve").toString(2))
                    }
                }
                fun retire() {
                    clickControl("property-action-target_curve")
                    try { host.awaitMain("Targeted sampler retired", 10_000, condition = { !state().getJSONObject("color_picker").optBoolean("calibrating") }) }
                    finally {
                        interactionSnapshot("targeted-retire-terminal", "property-action-target_curve")
                        File(output, "targeted-retire-state.json").writeText(controlState("property-action-target_curve").toString(2))
                    }
                }
                val initial = controls(); arm()
                drag(center, 250) { t -> 0.0 to -24 * t / .25 }
                host.awaitMain("Targeted warmup adopted", 10_000, condition = { controls() != initial })
                retire(); invoke("undo"); check(controls() == initial); precisionSettled("Targeted warmup settled")
                repeat(args.getString("effectRepeats", "3")!!.toInt()) { index ->
                    precisionSettled("Targeted contact settled"); val before = controls(); arm()
                    val samples = java.util.Collections.synchronizedList(mutableListOf<JSONObject>())
                    val inputBegan = java.util.concurrent.atomic.AtomicLong()
                    val firstChanged = java.util.concurrent.atomic.AtomicLong()
                    measure("effect-targeted-curves-drag") {
                        val sampler = Thread {
                            val until = SystemClock.uptimeMillis() + duration
                            while (SystemClock.uptimeMillis() < until) {
                                val now = SystemClock.elapsedRealtimeNanos()
                                val picker = state().getJSONObject("color_picker")
                                if (!picker.isNull("preview")) samples += obj("boot_ns" to now, "preview" to JSONObject(picker.getJSONObject("preview").toString()))
                                if (controls() != before) firstChanged.compareAndSet(0, now)
                                SystemClock.sleep(8)
                            }
                        }.apply { start() }
                        inputBegan.set(SystemClock.elapsedRealtimeNanos())
                        drag(center, duration) { t -> 0.0 to -24 * (1 - cos(2 * PI * t)) - 6 * kotlin.math.min(t / .1, 1.0) }
                        sampler.join()
                    }
                    val afterMeasure = SystemClock.elapsedRealtimeNanos()
                    val up = lastUpInjectionBootNs
                    host.awaitMain("Targeted correction adopted", 10_000, condition = { controls() != before })
                    retire(); val retired = SystemClock.elapsedRealtimeNanos(); precisionSettled("Targeted Exact restored")
                    val result = File(output, "effect-targeted-curves-drag.json")
                    result.writeText(JSONObject(result.readText()).put("sample_previews", JSONArray(samples))
                        .put("first_parameter_change_boot_ns", firstChanged.get().takeIf { it > 0 } ?: JSONObject.NULL)
                        .put("contact_begin_boot_ns", inputBegan.get()).put("up_injection_boot_ns", up).put("after_measure_boot_ns", afterMeasure).put("retired_boot_ns", retired)
                        .put("restored_exact_boot_ns", SystemClock.elapsedRealtimeNanos()).toString(2))
                    result.copyTo(File(output, "effect-targeted-curves-drag-$index.json"), overwrite = true)
                    invoke("undo"); check(controls() == before); precisionSettled("Targeted Undo settled")
                }
            }
            if (wanted("gradients")) {
                val selectedLabels = args.getString("labels")?.split(',')
                fun enabled(label: String) = selectedLabels == null || label in selectedLabels
                fun reveal(panel: String) {
                    action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to true)))
                    val group = host.panelGroup(panel)
                    action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                    if (group.optString("active") != panel) action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to panel))
                    action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
                }
                for (effectId in listOf("gradient_map", "gradient_fill")) {
                    val label = "effect-$effectId-stop-drag"
                    if (!enabled(label)) continue
                    val photoLayer = photoDocument()
                    action(obj("type" to "select_layer", "id" to photoLayer))
                    waitFor("Gradient catalog ready") { !state().getJSONObject("filter_load").getBoolean("pending") }
                    action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effectId)))
                    reveal("properties")
                    fun definition() = state().getJSONObject("layer_properties").array("controls").objects()
                        .first { it.getString("key") == "gradient" }.getJSONObject("value").getJSONObject("value")
                    waitFor("Native gradient strip") { findTag("effect-gradient") != null }
                    clickControl("effect-gradient")
                    waitFor("Native interior gradient stop") { definition().getJSONArray("stops").length() == 3 }
                    invoke("fit_canvas"); waitGuide("Gradient source settled")
                    fun strip() = settledControl("Gradient stop strip", { findTag("effect-gradient") })
                    fun start(bounds: android.graphics.RectF): Pair<Double, Double> {
                        val inset = 6 * activity.resources.displayMetrics.density
                        val position = definition().getJSONArray("stops").getJSONObject(1).getDouble("position")
                        return bounds.left + inset + (bounds.width() - 2 * inset) * position - host.surfaceOrigin.x to bounds.centerY() - host.surfaceOrigin.y.toDouble()
                    }
                    val original = definition().toString()
                    var bounds = strip()
                    drag(start(bounds), 250) { t -> bounds.width() * .1 * t / .25 to 0.0 }
                    waitFor("Native gradient prime changes definition") { definition().toString() != original }
                    invoke("undo"); check(definition().toString() == original); waitGuide("Gradient warmup settled")
                    repeat(args.getString("effectRepeats", "1")!!.toInt()) { index ->
                        waitGuide("Gradient contact settled"); bounds = strip()
                        val before = definition().toString()
                        val variants = java.util.Collections.synchronizedSet(mutableSetOf<String>())
                        measure(label) {
                            val sampler = Thread {
                                val until = SystemClock.uptimeMillis() + duration
                                while (SystemClock.uptimeMillis() < until) { variants += definition().toString(); SystemClock.sleep(8) }
                            }.apply { start() }
                            drag(start(bounds), duration) { t -> bounds.width() * .2 * (1 - kotlin.math.abs(1 - (t % .5) * 4)) to 0.0 }
                            sampler.join()
                        }
                        check(variants.size > 1) { "Native gradient stop motion did not change its definition" }
                        val result = File(output, "$label.json")
                        result.writeText(JSONObject(result.readText()).put("gradient_definition_variants", variants.size)
                            .put("gradient_definition_before", JSONObject(before)).put("gradient_definition_after", definition())
                            .put("native_strip_bounds", JSONArray(listOf(bounds.left, bounds.top, bounds.right, bounds.bottom))).toString(2))
                        result.copyTo(File(output, "$label-$index.json"), overwrite = true)
                        if (definition().toString() != before) { invoke("undo"); check(definition().toString() == before) }
                    }
                }
                val label = "gradient-tool-geometry-drag"
                if (enabled(label)) {
                    photoDocument(); invoke("add_layer"); invoke("gradient"); reveal("tool_settings")
                    clickControl("tool-segment-gradient-shape-0"); invoke("fit_canvas"); waitGuide("Gradient tool source settled")
                    fun geometry() = state().getJSONObject("camera").getJSONArray("work_area")
                    fun start(area: JSONArray) = area.getDouble(0) + area.getDouble(2) * .35 to area.getDouble(1) + area.getDouble(3) * .5
                    var area = geometry()
                    val beforePrime = state().getJSONObject("document_file").getLong("revision")
                    drag(start(area), 250) { t -> area.getDouble(2) * .25 * t / .25 to 0.0 }
                    waitFor("Native gradient tool prime adopted") { state().getJSONObject("document_file").getLong("revision") > beforePrime }
                    waitGuide("Gradient tool prime settled"); invoke("undo"); waitGuide("Gradient tool warmup settled")
                    repeat(args.getString("effectRepeats", "1")!!.toInt()) { index ->
                        waitGuide("Gradient tool contact settled"); area = geometry()
                        val before = state().getJSONObject("document_file").getLong("revision")
                        val points = mutableSetOf<Pair<Double, Double>>()
                        measure(label) {
                            drag(start(area), duration) { t ->
                                val point = area.getDouble(2) * .25 * (1 - kotlin.math.abs(1 - (t % .5) * 4)) to 0.0
                                points += point; point
                            }
                        }
                        waitFor("Native gradient tool contact adopted") { state().getJSONObject("document_file").getLong("revision") > before }
                        waitGuide("Gradient tool commit settled")
                        val result = File(output, "$label.json")
                        result.writeText(JSONObject(result.readText()).put("injected_geometry_positions", points.size)
                            .put("geometry_work_area", area).put("document_revision_before", before)
                            .put("document_revision_after", state().getJSONObject("document_file").getLong("revision"))
                            .put("commit_settled_boot_ns", SystemClock.elapsedRealtimeNanos())
                            .put("motion_phase", "native geometry preview; raster commit and drain follow Up").toString(2))
                        result.copyTo(File(output, "$label-$index.json"), overwrite = true)
                        invoke("undo"); waitGuide("Gradient tool Undo settled")
                    }
                }
            }
            if (wanted("curves") && args.getString("labels")?.split(',')?.let { "effect-curves-drag" in it } != false) {
                val photoLayer = photoDocument()
                action(obj("type" to "select_layer", "id" to photoLayer))
                waitFor("Curves catalog ready") { !state().getJSONObject("filter_load").getBoolean("pending") }
                action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "curves")))
                waitFor("published Curves controls") { state().getJSONObject("layer_properties").array("controls").objects().any { !it.isNull("curve") } }
                action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to true)))
                val group = host.panelGroup("properties")
                action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
                if (group.getString("active") != "properties") action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to "properties"))
                val layer = state().getJSONObject("layer_properties").getLong("layer")
                val key = state().getJSONObject("layer_properties").array("controls").objects().first { !it.isNull("curve") }.getString("key")
                action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to key,
                    "value" to obj("kind" to "curve", "value" to JSONArray("[[0,0],[0.5,0.5],[1,1]]")))))
                waitFor("native curve graph") { findTag("effect-curve") != null }
                invoke("fit_canvas")
                var graph = android.graphics.RectF()
                var visibleGraph = android.graphics.RectF()
                instrumentation.runOnMainSync {
                    val node = findTag("effect-curve")!!.second
                    val middle = node.positionInRoot.y + node.size.height * .5f
                    val visible = node.boundsInRoot
                    if (middle <= visible.top + visible.height * .25f || middle >= visible.bottom - visible.height * .25f) {
                        val scroll = generateSequence(node.parent) { it.parent }.first { it.config.getOrNull(SemanticsActions.ScrollBy) != null }
                        check(scroll.config[SemanticsActions.ScrollBy].action!!.invoke(0f, middle - node.boundsInRoot.center.y))
                    }
                }
                waitFor("existing middle knot visible") {
                    val node = findTag("effect-curve")!!.second
                    val middle = node.positionInRoot.y + node.size.height * .5f
                    val visible = node.boundsInRoot
                    middle > visible.top + visible.height * .25f && middle < visible.bottom - visible.height * .25f
                }
                precisionSettled("Curves input ready")
                var prime = 0.0 to 0.0
                instrumentation.runOnMainSync {
                    val (root, node) = findTag("effect-curve")!!
                    val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                    prime = node.positionInRoot.x + node.size.width * .5 + origin[0] - host.surfaceOrigin.x to
                        node.positionInRoot.y + node.size.height * .5 + origin[1] - host.surfaceOrigin.y
                }
                drag(prime, 250) { t -> -20 * t / .25 to -20 * t / .25 }; invoke("undo"); precisionSettled("Curves warmup settled")
                val repeats = args.getString("translationRepeats", "1")!!.toInt().also { require(it > 0) }
                repeat(repeats) { index ->
                    precisionSettled("Curves contact settled")
                    instrumentation.runOnMainSync {
                        val (root, node) = findTag("effect-curve")!!
                        val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                        graph = android.graphics.RectF(node.positionInRoot.x + origin[0], node.positionInRoot.y + origin[1],
                            node.positionInRoot.x + node.size.width + origin[0], node.positionInRoot.y + node.size.height + origin[1])
                        node.boundsInRoot.let { visibleGraph = android.graphics.RectF(it.left + origin[0], it.top + origin[1], it.right + origin[0], it.bottom + origin[1]) }
                    }
                    check(graph.width() > 40 && graph.height() > 40)
                    val points = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == key }.getJSONObject("value").getJSONArray("value")
                    check(points.length() == 3)
                    val middle = points.getJSONArray(1)
                    val x = graph.left + middle.getDouble(0) * graph.width()
                    val y = graph.top + (1 - middle.getDouble(1)) * graph.height()
                    check(visibleGraph.contains(x.toFloat(), y.toFloat())) { "Existing middle knot is outside the visible graph: $points $graph $visibleGraph" }
                    val start = x - host.surfaceOrigin.x.toDouble() to y - host.surfaceOrigin.y.toDouble()
                    val values = java.util.Collections.synchronizedSet(mutableSetOf<String>())
                    measure("effect-curves-drag") {
                        val sampler = Thread {
                            val until = SystemClock.uptimeMillis() + duration
                            while (SystemClock.uptimeMillis() < until) {
                                values += state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == key }.getJSONObject("value").getJSONArray("value").toString()
                                SystemClock.sleep(8)
                            }
                        }.apply { start() }
                        drag(start, duration) { t -> visibleGraph.width() * .12 * (cos(2 * PI * t) - 1) to visibleGraph.height() * .12 * (cos(2 * PI * t) - 1) }
                        sampler.join()
                    }
                    check(values.size > 1) { "Native curve motion did not change its stored points" }
                    val after = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == key }.getJSONObject("value").getJSONArray("value")
                    check(after.length() == 3) { "Native curve contact must move the existing middle knot: before=$points after=$after graph=$graph visible=$visibleGraph start=$start" }
                    val result = File(output, "effect-curves-drag.json")
                    result.writeText(JSONObject(result.readText()).put("curve_variants", values.size)
                        .put("curve_graph_bounds", JSONArray(listOf(graph.left, graph.top, graph.right, graph.bottom)))
                        .put("curve_graph_visible_bounds", JSONArray(listOf(visibleGraph.left, visibleGraph.top, visibleGraph.right, visibleGraph.bottom))).toString(2))
                    result.copyTo(File(output, "effect-curves-drag-${index + 1}.json"), overwrite = true)
                }
            }
            if (wanted("photo")) {
                val tierPhoto = args.getString("tierPhoto") == "true"
                val openedPhoto = if (tierPhoto || args.getString("transformSnapping") == "true") photoDocument() else null
                if (openedPhoto == null) { newDocument(); place(photo()) }
                else action(obj("type" to "select_layer", "id" to openedPhoto))
                if (tierPhoto || materialWatercolor || args.getString("acceptedPhoto") == "true") {
                    Log.i("CapyBarPerf", "material setup: accept photo")
                    if (openedPhoto == null) invoke("apply_transform")
                    if (materialWatercolor) {
                        Log.i("CapyBarPerf", "material setup: select wet brush")
                        invoke("brush")
                        action(obj("type" to "select_brush", "id" to 21))
                        action(obj("type" to "set_brush_size", "value" to 400))
                        waitFor("watercolor brush ready") { host.snapshot?.optBoolean("brush_ready") == true }
                        val camera = state().getJSONObject("camera")
                        val translation = camera.getJSONArray("translation")
                        val zoom = camera.getDouble("zoom")
                        val center = width * .5 * zoom + translation.getDouble(0) to height * .5 * zoom + translation.getDouble(1)
                        Log.i("CapyBarPerf", "material setup: wet stroke")
                        drag(center, 800) { t -> 120 * t to 40 * t }
                        val paintDeadline = SystemClock.uptimeMillis() + 120_000
                        while (native { Native.renderingPending(it) }) {
                            check(SystemClock.uptimeMillis() < paintDeadline) { "Watercolor paint did not drain" }
                            SystemClock.sleep(16)
                        }
                        Log.i("CapyBarPerf", "material setup: save raw material")
                        val manifest = saveProject("material-input.capy")
                        File(output, "material-manifest.json").writeText(manifest.toString(2))
                        val rasters = manifest.paintRecords().objects().map {it.getJSONObject("data")}
                        check(rasters.any { it.optJSONObject("material")?.isNull("watercolor") == false && it.getJSONArray("tiles").objects().any { tile ->
                            tile.getString("plane") == "watercolor_wetness" } }) { "The workload has no stored watercolor material" }
                        check(manifest.originalImages().length() > 0) { "The photo source was lost" }
                    }
                    if (args.getString("transformSnapping") == "true") {
                        val photoLayer = state().array("layers").objects().single { it.optBoolean("editing") }.getLong("id")
                        invoke("add_layer")
                        snapNeighbor = state().array("layers").objects().single { it.optBoolean("editing") }.getLong("id")
                        invoke("brush")
                        action(obj("type" to "select_brush", "id" to 1))
                        action(obj("type" to "set_brush_size", "value" to 80))
                        waitFor("snap target brush ready") { host.snapshot?.optBoolean("brush_ready") == true }
                        val camera = state().getJSONObject("camera")
                        val shift = camera.getJSONArray("translation"); val zoom = camera.getDouble("zoom")
                        val center = width * .5 * zoom + shift.getDouble(0) to height * .5 * zoom + shift.getDouble(1)
                        drag(center, 300) { t -> 40 * t to 0.0 }
                        val target = saveProject("snap-input.capy")
                        val neighborIndex = state().array("layers").objects().indexOfFirst {it.getLong("id") == snapNeighbor}
                        check(neighborIndex >= 0)
                        val neighbor = target.occurrenceRecords().getJSONObject(neighborIndex).getString("id")
                        check(target.paintData(neighbor).array("tiles").objects().any {it.getString("plane") == "color"}) { "Snapping has no painted target" }
                        action(obj("type" to "select_layer", "id" to photoLayer))
                    }
                    val entryBegan = android.os.SystemClock.elapsedRealtimeNanos()
                    instrumentation.runOnMainSync { host.dispatch(obj("type" to "invoke", "command" to "scale_rotate")) }
                    waitFor("material placement bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "placement" }
                    val entryPublished = android.os.SystemClock.elapsedRealtimeNanos()
                    val entryDeadline = SystemClock.uptimeMillis() + 120_000
                    while (native { Native.renderingPending(it) }) {
                        check(SystemClock.uptimeMillis() < entryDeadline) { "Transform entry did not drain" }
                        SystemClock.sleep(16)
                    }
                    transformEntry = obj("dispatch_boot_ns" to entryBegan, "published_boot_ns" to entryPublished,
                        "pending_idle_boot_ns" to android.os.SystemClock.elapsedRealtimeNanos())
                }
                SystemClock.sleep(3000)
                measure("photo-bar-show-hide") { hideAndShow(duration) }
                measure("photo-bar-contact-taps") { taps(corner().let { it.first - 200 to it.second - 200 }, duration) }
                primeTransform(.4)
                if (args.getString("transformSnapping") == "true") {
                    val snapping = args.getString("snappingEnabled", "true") != "false"
                    if (snapping) invoke("transform_snapping")
                    waitFor("transform snapping matches the workload") { state().array("commands").objects().any { it.getString("id") == "transform_snapping" && it.getBoolean("selected") == snapping } }
                }
                val translationRepeats = args.getString("translationRepeats", "1")!!.toInt().also { require(it > 0) }
                recordProcessMemory("before-photo-motion")
                repeat(translationRepeats) { index ->
                    measure("photo-translate-drag") { drag(anchorPoint(.4), duration, wiggle) }
                    if (translationRepeats > 1) File(output, "photo-translate-drag.json").takeIf { it.exists() }
                        ?.copyTo(File(output, "photo-translate-drag-${index + 1}.json"), overwrite = true)
                    recordProcessMemory("after-photo-motion-${index + 1}")
                }
                if (args.getString("labels") in listOf("photo-retained-distort-drag", "photo-retained-warp-drag")) {
                    val label = args.getString("labels")!!
                    invoke("reset_transform")
                    val warp = label == "photo-retained-warp-drag"
                    invoke(if (warp) "transform_warp" else "transform_distort")
                    SystemClock.sleep(800)
                    repeat(translationRepeats) { index ->
                        measure(label) {
                            drag(if (warp) anchorPoint(1.0 / 3) else corner(), duration,
                                if (args.getString("saveRetainedDiagnostic") == "true") { t ->
                                    -120 * (t * 1000 / duration).coerceIn(0.0, 1.0) to -80 * (t * 1000 / duration).coerceIn(0.0, 1.0)
                                } else wiggle)
                        }
                        File(output, "$label.json").takeIf { it.exists() }
                            ?.copyTo(File(output, "$label-${index + 1}.json"), overwrite = true)
                        recordProcessMemory("after-$label-${index + 1}")
                    }
                    if (args.getString("saveRetainedDiagnostic") == "true") {
                        invoke("apply_transform")
                        val saved = saveProject("retained-diagnostic.capy")
                        File(output, "retained-diagnostic-manifest.json").writeText(saved.toString(2))
                    } else if (args.getString("finalBake") == "true") finalBake() else invoke("cancel_transform")
                } else if (args.getString("labels") == "photo-translate-drag") {
                    if (args.getString("finalBake") == "true") finalBake() else invoke("cancel_transform")
                    if (memory) {
                        SystemClock.sleep(2000)
                        recordProcessMemory("after-photo-idle")
                    }
                } else {
                    invoke("reset_transform")
                    primeTransform()
                    measure("photo-handle-drag-bar-hidden") { drag(corner(), duration, wiggle) }
                    invoke("reset_transform")
                    invoke("show_canvas_action_bar")
                    waitFor("completion-only bar") { host.canvasBar?.array("items")?.length() == 0 }
                    measure("photo-handle-drag-bar-visible") { drag(corner(), duration, wiggle) }
                    invoke("reset_transform")
                    measure("photo-handle-drags") { drags(duration) }
                    invoke("reset_transform")
                    invoke("show_canvas_action_bar")
                    invoke("apply_transform")
                    waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                    if (!materialWatercolor) {
                        invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                        waitFor("photo transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                        SystemClock.sleep(1500)
                        primeTransform(.4)
                        measure("photo-pixels-translate-drag") { drag(anchorPoint(.4), duration, wiggle) }
                        invoke("reset_transform")
                        primeTransform()
                        measure("photo-pixels-handle-drag") { drag(corner(), duration, wiggle) }
                        invoke("reset_transform")
                        measure("photo-pixels-drags") { drags(duration) }
                        invoke("reset_transform")
                        invoke("transform_distort")
                        SystemClock.sleep(1000)
                        primeTransform(mode = "transform_distort")
                        measure("photo-pixels-distort-drag") { drag(corner(), duration, wiggle) }
                        invoke("reset_transform")
                        invoke("transform_warp")
                        primeTransform(1.0 / 3, "transform_warp")
                        measure("photo-pixels-warp-drag") { drag(anchorPoint(1.0 / 3), duration, wiggle) }
                        invoke("cancel_transform")
                    }
                }
            }
            if (wanted("composed_transform")) {
                newDocument()
                place(photo())
                invoke("apply_transform")
                action(obj("type" to "layer", "action" to obj("op" to "duplicate_selected")))
                action(obj("type" to "set_layer_opacity", "opacity" to .35))
                invoke("rectangle_select"); invoke("select_all"); invoke("scale_rotate")
                waitFor("composed transform bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "transform" }
                primeTransform()
                measure("composed-photo-pixels-handle-drag") { drag(corner(), duration, wiggle) }
                invoke("reset_transform")
                primeTransform(mode = "transform_distort")
                measure("composed-photo-pixels-distort-drag") { drag(corner(), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("cropped_photo")) {
                newDocument(width / 4 to height / 4)
                place(photo())
                invoke("placement_original_size")
                val anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor")
                check(kotlin.math.abs(anchor.getDouble(2) - anchor.getDouble(0) - width) < 1) { "Photo width: $anchor, expected $width" }
                check(kotlin.math.abs(anchor.getDouble(3) - anchor.getDouble(1) - height) < 1) { "Photo height: $anchor, expected $height" }
                repeat(4) { if (state().getJSONObject("camera").getDouble("zoom") > .35) invoke("zoom_out") }
                check(state().getJSONObject("camera").getDouble("zoom") <= .35)
                SystemClock.sleep(1500)
                drag(anchorPoint(.4), 500, wiggle)
                SystemClock.sleep(1500)
                measure("cropped-photo-translate-drag") { drag(anchorPoint(.4), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("crop")) {
                newDocument()
                place(photo())
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                SystemClock.sleep(3000)
                invoke("crop")
                waitFor("crop bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "crop" }
                SystemClock.sleep(1500)
                val camera = state().getJSONObject("camera")
                val translation = camera.getJSONArray("translation")
                val handle = width * camera.getDouble("zoom") + translation.getDouble(0) to height * camera.getDouble("zoom") + translation.getDouble(1)
                val inward = { t: Double -> (-60 * (1 - cos(2 * PI * t))) to (-40 * (1 - cos(2 * PI * t))) }
                measure("crop-handle-drag") { drag(handle, duration, inward) }
                measure("crop-handle-drags") {
                    val began = SystemClock.uptimeMillis()
                    var index = 0
                    while (SystemClock.uptimeMillis() - began < duration) {
                        android.os.Trace.beginAsyncSection("capy-drag", ++index)
                        drag(handle, 400) { t -> inward(t / .4) }
                        android.os.Trace.endAsyncSection("capy-drag", index)
                        SystemClock.sleep(900)
                    }
                }
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
                primeTransform()
                measure("scaled-photo-pixels-handle-drag") { drag(corner(), duration, wiggle) }
                invoke("cancel_transform")
            }
            if (wanted("move")) {
                newDocument()
                place(photo())
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                SystemClock.sleep(3000)
                fun surface(x: Double, y: Double): Pair<Double, Double> {
                    val camera = state().getJSONObject("camera")
                    val zoom = camera.getDouble("zoom"); val translation = camera.getJSONArray("translation")
                    return x * zoom + translation.getDouble(0) to y * zoom + translation.getDouble(1)
                }
                val (w, h) = width.toDouble() to height.toDouble()
                val selections = listOf<Pair<String, () -> Unit>>(
                    "move-all" to { invoke("select_all") },
                    "move-part" to {
                        invoke("rectangle_select")
                        val (x0, y0) = surface(w * .25, h * .25); val (x1, y1) = surface(w * .75, h * .75)
                        drag(x0 to y0, 300) { t -> (x1 - x0) * t / .3 to (y1 - y0) * t / .3 }
                    },
                    "move-part-leave-copy" to { invoke("move_leave_copy") },
                )
                for ((label, select) in selections) {
                    select()
                    invoke("move")
                    waitFor("$label selection bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "selection" }
                    SystemClock.sleep(1500)
                    measure("$label-drag") { drag(corner(), duration, wiggle) }
                    measure("$label-drags") { drags(duration) }
                }
                invoke("move_leave_copy")
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
            if (wanted("canvas_size")) {
                newDocument()
                invoke("select_all"); invoke("fill_selection"); invoke("deselect"); invoke("hand")
                SystemClock.sleep(1500)
                val area = state().getJSONObject("camera").getJSONArray("work_area")
                val center = area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                val span = minOf(area.getDouble(2), area.getDouble(3)) * .25
                fun resize(extent: Pair<Int, Int>) {
                    fun send(value: JSONObject) = action(obj("type" to "canvas_size", "action" to value))
                    invoke("canvas_size")
                    send(obj("op" to "anchor", "anchor" to "bottom_right"))
                    send(obj("op" to "width", "value" to extent.first)); send(obj("op" to "height", "value" to extent.second))
                    mark = System.nanoTime()
                    send(obj("op" to "apply"))
                    dispatched = System.nanoTime()
                    check(state().array("tabs").objects().first { it.getBoolean("active") }.getInt("width") == extent.first)
                }
                val pan = { t: Double -> span * sin(2 * PI * t / 1.6) to span * .6 * sin(2 * PI * t / 2.3) }
                measure("pan") { drag(center, duration, pan) }
                measure("canvas-size-grow-then-pan") { resize(width + 512 to height + 512); drag(center, duration, pan) }
                measure("canvas-size-crop-then-pan") { resize(width to height); drag(center, duration, pan) }
            }
            if (wanted("merge")) {
                newDocument()
                place(photo())
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                invoke("pen"); action(obj("type" to "select_brush", "id" to 1)); action(obj("type" to "set_brush_size", "value" to 120))
                val area = state().getJSONObject("camera").getJSONArray("work_area")
                val left = area.getDouble(0) + area.getDouble(2) * .1
                val top = area.getDouble(1) + area.getDouble(3) * .1
                val span = area.getDouble(2) * .8
                repeat(8) { band ->
                    invoke("add_layer")
                    action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1 * band, .8 - .07 * band, .5, .8))))
                    drag(left to top + area.getDouble(3) * .1 * band, 500) { t -> span * t / .5 to 0.0 }
                }
                val layers = { state().array("layers").length() }
                waitFor("ten layers over the paper") { layers() == 11 }
                SystemClock.sleep(3000)
                for (command in listOf("merge_visible", "flatten_image")) {
                    measure("merge-${command.replace('_', '-')}") {
                        mark = System.nanoTime(); invoke(command); dispatched = System.nanoTime()
                        waitFor("$command replaces the layers") { layers() == 2 }
                        SystemClock.sleep(2500)
                    }
                    invoke("undo")
                    waitFor("undo restores the layers") { layers() == 11 }
                    SystemClock.sleep(3000)
                }
            }
            if (wanted("dodge_burn") || wanted("frequency_separation")) {
                newDocument()
                invoke("blend_perceptual")
                place(photo())
                invoke("apply_transform")
                waitFor("placed photo") { host.canvasBar == null || host.canvasBar?.getJSONObject("context")?.getString("kind") != "placement" }
                val layers = { state().array("layers").length() }
                val count = layers()
                SystemClock.sleep(3000)
                if (wanted("dodge_burn")) {
                    measure("dodge-burn-layer") {
                        mark = System.nanoTime(); invoke("new_dodge_burn_layer"); dispatched = System.nanoTime()
                        waitFor("New Dodge & Burn Layer adds a layer") { layers() == count + 1 }
                    }
                    invoke("undo")
                    waitFor("undo removes the layer") { layers() == count }
                    SystemClock.sleep(3000)
                }
                if (wanted("frequency_separation")) {
                    fun separate(value: JSONObject) = action(obj("type" to "frequency_separation", "action" to value))
                    invoke("frequency_separation")
                    separate(obj("op" to "radius", "radius" to 8))
                    SystemClock.sleep(3000)
                    measure("frequency-separation") {
                        mark = System.nanoTime(); separate(obj("op" to "apply")); dispatched = System.nanoTime()
                        waitFor("Frequency Separation adds its group") { layers() == count + 3 }
                    }
                    invoke("undo")
                    waitFor("undo removes the group") { layers() == count }
                }
            }
            if (wanted("refine")) for (extent in listOf(2048 to 1536, width to height).distinct()) {
                newDocument(extent)
                invoke("select_all"); invoke("fill_selection"); invoke("deselect"); invoke("rectangle_select")
                val area = state().getJSONObject("camera").getJSONArray("work_area")
                val center = area.getDouble(0) + area.getDouble(2) / 2 to area.getDouble(1) + area.getDouble(3) / 2
                val span = minOf(area.getDouble(2), area.getDouble(3)) * .25
                drag(center.first - span to center.second - span, 300) { t -> 2 * span * t / .3 to 1.5 * span * t / .3 }
                waitFor("selection bar") { state().optJSONObject("canvas_bar")?.getJSONObject("context")?.getString("kind") == "selection" }
                if (!refineBar) invoke("show_canvas_action_bar")
                invoke("${refine}_selection")
                waitFor("Refine panel") { findTag("setting-slider-selection-refine") != null }
                SystemClock.sleep(1500)
                var track = android.graphics.RectF()
                instrumentation.runOnMainSync {
                    val (root, node) = findTag("setting-slider-selection-refine")!!
                    val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                    node.boundsInRoot.let { track = android.graphics.RectF(it.left + origin[0], it.top + origin[1], it.right + origin[0], it.bottom + origin[1]) }
                }
                val radii = java.util.Collections.synchronizedSet(mutableSetOf<Float>())
                fun stats() = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
                var previewCount = 0L
                var valueCount = 0L
                val anchors = java.util.Collections.synchronizedSet(mutableSetOf<String>())
                measure("refine-$refine-drag-${extent.first}x${extent.second}") {
                    val before = stats()
                    val sampler = Thread {
                        val until = SystemClock.uptimeMillis() + duration
                        while (SystemClock.uptimeMillis() < until) {
                            val shown = host.snapshot?.getJSONObject("state")
                            shown?.getJSONObject("layer_tools")?.objectOrNull("selection_resize")?.let { radii += it.number("radius") }
                            shown?.optJSONObject("canvas_bar")?.optJSONArray("anchor")?.let { anchors += it.toString() }
                            SystemClock.sleep(8)
                        }
                    }.apply { start() }
                    val start = track.left + track.width() * .1 - host.surfaceOrigin.x to track.centerY() - host.surfaceOrigin.y.toDouble()
                    drag(start, duration) { t -> track.width() * refineSpan / 2 * (1 - kotlin.math.cos(2 * PI * t / 2)) to 0.0 }
                    sampler.join()
                    val after = stats()
                    previewCount = after.optLong("selection_previews") - before.optLong("selection_previews")
                    valueCount = after.optLong("selection_values") - before.optLong("selection_values")
                }
                println("CANVAS BAR refine values ${extent.first}x${extent.second}: $valueCount values sent, $previewCount previews drawn, ${radii.size} distinct published radii and ${anchors.size} bar positions, ${synchronized(radii) { radii.minOrNull() }}–${synchronized(radii) { radii.maxOrNull() }} px")
                action(obj("type" to "selection", "action" to obj("op" to "cancel_resize")))
                if (!refineBar) invoke("show_canvas_action_bar")
                invoke("deselect")
            }
            activity.window.removeOnFrameMetricsAvailableListener(listener)
            metricsThread.quitSafely()
            assertNull(host.failure)
        }
    }
}
