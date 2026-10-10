package art.capycanvas

import android.os.SystemClock
import android.view.WindowManager
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Rule
import org.junit.Test
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import java.io.File
import kotlin.math.*

/** Opt-in pen replay through either Android input dispatch or the canvas owner. */
class AndroidViewportBenchmarkTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)

    @Test fun retainedViewport() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("viewportBenchmark") == "true")
        val osInput = args.getString("osInput", "false") == "true"
        val prediction = args.getString("prediction", "true") == "true"
        val label = args.getString("label", "baseline")!!
        val interval = args.getString("intervalMs", "4.166667")!!.toDouble()
        val duration = args.getString("durationMs", "5000")!!.toInt()
        val contact = args.getString("contactMs", "0")!!.toInt()
        val pause = args.getString("pauseMs", "100")!!.toInt()
        val repeats = args.getString("repeats", "3")!!.toInt()
        val size = args.getString("canvasSize", "1024")!!.toInt()
        val pressure = args.getString("pressure")?.toDouble()
        val speed = args.getString("speed", "1")!!.toDouble()
        val brushSize = args.getString("brushSize", "18")!!.toDouble()
        val strokeOffset = args.getString("strokeOffset", "0")!!.toDouble()
        val zoomSteps = args.getString("zoomSteps", "0")!!.toInt()
        val transparency = args.getString("transparency")?.let { listOf("off", "low", "medium", "high").indexOf(it) }
        val languageSwitches = args.getString("languageSwitches")?.split(',').orEmpty()
        val motion = args.getString("motion", "stroke")!!
        val blending = args.getString("blending")
        check(motion in listOf("stroke", "hover", "pan", "pinch", "rotate", "smooth_zoom"))
        val smoothZoom = motion == "smooth_zoom"
        val zoomDirection = args.getString("zoomDirection", "horizontal")!!
        check(zoomDirection in listOf("horizontal", "vertical"))
        val cursor = args.getString("cursor")
        val retainedMotion = motion in listOf("stroke", "hover")
        val passThrough = args.getString("passThrough", "false") == "true"
        val navigator = args.getString("navigator", "true") == "true"
        val artworkQueries = args.getString("artworkQueries", "false") == "true"
        val statisticsPreview = args.getString("statisticsPreview", "false") == "true"
        val statisticsExact = args.getString("statisticsExact", "false") == "true"
        val statisticsWaveform = args.getString("statisticsWaveform", "false") == "true"
        val levelsStatistics = args.getString("levelsStatistics", "false") == "true"
        val clippingPreview = args.getString("clippingPreview", "false") == "true"
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(condition: () -> Boolean) {
                val start = SystemClock.uptimeMillis()
                while (!condition()) { assertNull(host.failure); check(SystemClock.uptimeMillis() - start < 120_000) { "G-pen did not settle: ${host.actionError}; readiness=${host.snapshot?.let { listOf(it.optBoolean("gpu_ready"), it.optBoolean("canvas_ready"), it.optBoolean("brush_ready"), it.optBoolean("shaders_ready")) }}" }; SystemClock.sleep(20) }
            }
            waitFor { host.snapshot?.optBoolean("brush_ready") == true && host.workspaceManager?.optBoolean("ready") == true && host.workspaceManager?.optBoolean("busy") == false }
            val openQueryPhoto = args.getString("openQueryPhoto", "false") == "true"
            if (openQueryPhoto) {
                host.openQueryPhoto(File(args.getString("photo", "/data/local/tmp/capy-brush-photo.jpg")!!))
                scenario.onActivity { host.invoke("add_layer") }
            } else host.newDocument(args.getString("width")?.toInt() ?: size, args.getString("height")?.toInt() ?: size)
            if (levelsStatistics) {
                host.drain()
                val photo = host.snapshot!!.getJSONObject("state").array("layers").objects().single { it.getLong("id") == 1L }.getLong("id")
                host.drain(obj("type" to "select_layer", "id" to photo))
                host.drain(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "levels")))
            }
            if (clippingPreview) {
                host.drain(obj("type" to "histogram", "action" to obj("type" to "shadows", "enabled" to true)))
                host.drain(obj("type" to "histogram", "action" to obj("type" to "highlights", "enabled" to true)))
                val histogram = JSONObject(native { Native.dispatch(it, obj("type" to "close_settings").toString()); Native.snapshot(it)!! }).getJSONObject("state").getJSONObject("histogram")
                check(histogram.getBoolean("shadows") && histogram.getBoolean("highlights"))
            }
            val querySource = if (levelsStatistics) obj("EffectChannels" to host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").getLong("layer")).toString() else "\"Visible\""
            args.getString("photo")?.takeUnless { openQueryPhoto }?.let { path ->
                host.importImage(File(path))
                waitFor { host.snapshot?.getJSONObject("state")?.optJSONObject("canvas_bar")?.optJSONObject("context")?.optString("kind") == "placement" }
                scenario.onActivity { host.invoke("apply_transform") }
                waitFor { host.snapshot?.getJSONObject("state")?.optJSONObject("canvas_bar")?.optJSONObject("context")?.optString("kind") != "placement" }
                val ink = host.snapshot!!.getJSONObject("state").array("layers").objects().single { it.getLong("id") == 1L }.getLong("id")
                scenario.onActivity { host.dispatch(obj("type" to "select_layer", "id" to ink)); host.invoke("raise_layer") }
            }
            scenario.onActivity { blending?.let { host.invoke("blend_$it") }; host.invoke("fit_canvas"); repeat(zoomSteps) { host.invoke("zoom_in") } }
            val preset = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }.first { it.getString("label") == "G-Pen" }.getInt("id")
            scenario.onActivity {
                host.dispatch(obj("type" to "select_brush", "id" to preset))
                host.dispatch(obj("type" to "set_brush_size", "value" to brushSize))
                host.preference(obj("type" to "edit", "id" to "feedback", "value" to prediction))
                host.preference(obj("type" to "edit", "id" to "platform_prediction", "value" to false))
                cursor?.let { mode -> host.dispatch(obj("type" to "restore_settings", "settings" to
                    JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("settings").toString()).put("cursor", mode))) }
                host.dispatch(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "navigator", "visible" to navigator)))
                transparency?.let { host.preference(obj("type" to "edit", "id" to "transparency", "value" to it)) }
                if (passThrough) {
                    host.preference(obj("type" to "edit", "id" to "pass_through_groups", "value" to true))
                    host.dispatch(obj("type" to "set_color", "rgba" to JSONArray(listOf(0.9, 0.3, 0.1, 1.0))))
                    host.dispatch(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "solid_color")))
                    host.dispatch(obj("type" to "layer", "action" to obj("op" to "new", "group" to true, "clipped" to false)))
                    host.dispatch(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
                    host.dispatch(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "black_white")))
                }
            }
            SystemClock.sleep(1500)
            assertNull(host.actionError)
            val state = host.snapshot!!.getJSONObject("state")
            val camera = state.getJSONObject("camera")
            val area = camera.getJSONArray("work_area")
            val cx = area.getDouble(0) + area.getDouble(2) * (.5 + strokeOffset)
            val cy = area.getDouble(1) + area.getDouble(3) / 2
            val radius = min(area.getDouble(2), area.getDouble(3)) * .32
            val radiusX = args.getString("radiusX")?.toDouble() ?: radius
            val radiusY = args.getString("radiusY")?.toDouble() ?: radius * .65
            val output = File(activity.getExternalFilesDir(null), "viewport-benchmark").apply { mkdirs() }
            if (clippingPreview) {
                File(output, "$label-clipping-state.json").writeText(native { Native.dispatch(it, obj("type" to "close_settings").toString()); Native.snapshot(it)!! })
                val image = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
                checkNotNull(image)
                File(output, "$label-clipping.png").outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                image.recycle()
            }
            var activePresent: JSONArray? = null
            var minimumZoomPercent = Int.MAX_VALUE
            var maximumZoomPercent = 0
            var switchLanguage: String? = null
            var languageRequested = 0L
            fun requestLanguage(tag: String) {
                val index = listOf("system", "en", "ja", "zh-Hans", "zh-Hant", "ko").indexOf(tag)
                check(index >= 0)
                languageRequested = System.nanoTime()
                scenario.onActivity { host.preference(obj("type" to "edit", "id" to "language", "value" to index)) }
            }
            fun stroke(run: Int, milliseconds: Int, hover: Boolean = false) {
                val count = (milliseconds / interval).toInt()
                val began = System.nanoTime()
                val down = SystemClock.uptimeMillis()
                val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply {
                    id = 7; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS
                })
                val coords = arrayOf(android.view.MotionEvent.PointerCoords())
                for (i in 0..count) {
                    val deadline = began + (i * interval * 1e6).toLong()
                    val left = deadline - System.nanoTime()
                    if (left > 0) java.util.concurrent.locks.LockSupport.parkNanos(left)
                    if (i == count / 2) switchLanguage?.let { tag -> requestLanguage(tag); switchLanguage = null }
                    val t = i * interval / 1000.0 * speed
                    val x = cx + if (smoothZoom) { if (zoomDirection == "horizontal") 100 * sin(t * PI) else 0.0 } else radiusX * sin(t * 3.2)
                    val y = cy + if (smoothZoom) { if (zoomDirection == "vertical") -100 * sin(t * PI) else 0.0 } else radiusY * sin(t * 4.7 + run * .31)
                    val p = pressure ?: (.65 + .3 * sin(t * 1.7))
                    if (osInput || smoothZoom) {
                        coords[0].x = x.toFloat() + host.surfaceOrigin.x
                        coords[0].y = y.toFloat() + host.surfaceOrigin.y
                        coords[0].pressure = if (i == count) 0f else p.toFloat()
                        val action = if (hover) android.view.MotionEvent.ACTION_HOVER_MOVE else if (i == 0) android.view.MotionEvent.ACTION_DOWN else if (i == count) android.view.MotionEvent.ACTION_UP else android.view.MotionEvent.ACTION_MOVE
                        val event = android.view.MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1,
                            properties, coords, 0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
                        try { check(instrumentation.uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
                    } else {
                        val records = host.pointerBuffer(9)
                        doubleArrayOf(x, y, p, 0.0, 0.0, 0.0, 0.0, System.nanoTime().toDouble(),
                            (if (hover) 0 else if (i == 0) 1 else if (i == count) 3 else 2).toDouble()).copyInto(records)
                        host.pointer((9300 + run).toLong(), 0, 0, records, 9)
                    }
                    if (i % 120 == 119 && activePresent != null) native {
                        activePresent!!.put(JSONArray(Native.presentationTimings(it, true)))
                    }
                    if (smoothZoom && i % 120 == 119) {
                        minimumZoomPercent = min(minimumZoomPercent, host.cameraReadout.zoomPercent)
                        maximumZoomPercent = max(maximumZoomPercent, host.cameraReadout.zoomPercent)
                    }
                }
            }
            fun gesture(milliseconds: Int) {
                val properties = Array(2) { index -> android.view.MotionEvent.PointerProperties().apply {
                    id = index + 11; toolType = android.view.MotionEvent.TOOL_TYPE_FINGER
                } }
                val coords = Array(2) { android.view.MotionEvent.PointerCoords().apply { this.pressure = 1f; this.size = .1f } }
                val down = SystemClock.uptimeMillis()
                fun send(action: Int, count: Int, t: Double) {
                    val phase = t / 2 * 2 * PI
                    val (x, y, spread) = if (motion == "pan") Triple(cx + radius * .8 * sin(phase), cy + radius * .5 * sin(2 * phase), 150.0)
                        else Triple(cx, cy, if (motion == "pinch") 150 * exp(.4 * sin(phase)) else 150.0)
                    val angle = if (motion == "rotate") .4 * sin(phase) else 0.0
                    for (i in 0..1) {
                        val direction = if (i == 0) -1 else 1
                        coords[i].x = (x + direction * spread * cos(angle)).toFloat() + host.surfaceOrigin.x
                        coords[i].y = (y + direction * spread * sin(angle)).toFloat() + host.surfaceOrigin.y
                    }
                    val event = android.view.MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, count, properties, coords,
                        0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
                    try { check(instrumentation.uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
                }
                val pointer = android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT
                val began = System.nanoTime()
                send(android.view.MotionEvent.ACTION_DOWN, 1, 0.0)
                send(android.view.MotionEvent.ACTION_POINTER_DOWN or (1 shl pointer), 2, 0.0)
                val count = (milliseconds / interval).toInt()
                for (i in 1..count) {
                    val left = began + (i * interval * 1e6).toLong() - System.nanoTime()
                    if (left > 0) java.util.concurrent.locks.LockSupport.parkNanos(left)
                    send(android.view.MotionEvent.ACTION_MOVE, 2, i * interval / 1000.0 * speed)
                    if (i % 120 == 119 && activePresent != null) native {
                        activePresent!!.put(JSONArray(Native.presentationTimings(it, true)))
                    }
                }
                val end = count * interval / 1000.0 * speed
                send(android.view.MotionEvent.ACTION_POINTER_UP or (1 shl pointer), 2, end)
                send(android.view.MotionEvent.ACTION_UP, 1, end)
            }
            fun restoreNavigationCamera() {
                scenario.onActivity {
                    if (motion == "rotate") host.invoke("reset_rotation")
                    host.invoke("fit_canvas")
                    repeat(zoomSteps) { host.invoke("zoom_in") }
                }
            }
            stroke(0, 1500)
            waitFor { !native { Native.renderingPending(it) } }
            host.drain(obj("type" to "invoke", "command" to "undo"))
            SystemClock.sleep(800)
            if (smoothZoom) {
                val settings = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("settings").toString())
                val zoom = settings.optJSONObject("zoom_tool")
                if (zoom != null) {
                    zoom.put("drag", "smooth").put("direction", zoomDirection).put("zoom_out", false)
                    host.drain(obj("type" to "restore_settings", "settings" to settings))
                } else check(zoomDirection == "horizontal")
                host.drain(obj("type" to "invoke", "command" to "zoom"))
            }
            if (motion == "hover") stroke(0, 1500, true)
            if (!retainedMotion) {
                if (smoothZoom) stroke(0, 1500) else gesture(1500)
                restoreNavigationCamera()
                SystemClock.sleep(800)
            }
            waitFor { host.snapshot?.optBoolean("brush_ready") == true }
            val readiness = host.snapshot!!
            val info = obj("radii" to JSONArray(listOf(radiusX, radiusY)), "navigator" to navigator, "label" to label, "photo" to (args.getString("photo") ?: "generated"), "motion" to motion, "zoom_direction" to zoomDirection, "repeats" to repeats, "os_input" to (osInput || smoothZoom), "prediction" to prediction, "interval_ms" to interval, "duration_ms" to duration, "pressure" to pressure, "speed" to speed, "state" to state,
                "startup" to obj("gpu_ready" to readiness.optBoolean("gpu_ready"), "canvas_ready" to readiness.optBoolean("canvas_ready"), "brush_ready" to readiness.optBoolean("brush_ready"), "shaders_ready" to readiness.optBoolean("shaders_ready")),
                "display" to native { JSONObject(Native.displayStatus(it)) })
            info.put("thermal_status",activity.getSystemService(android.os.PowerManager::class.java).currentThermalStatus)
            if (smoothZoom) info.put("state", host.snapshot!!.getJSONObject("state"))
            File(output, "$label-info.json").writeText(info.toString(2))
            assertEquals(if (retainedMotion) "SharedDemandRefresh" else "Fifo", info.getJSONObject("display").getString("present_mode"))
            assertEquals(retainedMotion, info.getJSONObject("display").getBoolean("retained_target"))
            assertEquals("Navigator visibility after adoption", navigator, info.getJSONObject("display").getInt("overview_count") > 0)
            native { Native.presentationTimings(it, true) }
            repeat(repeats) { run ->
                val captureBegin = System.nanoTime()
                val queryJobs = if (artworkQueries) List(args.getString("queryCount", "600")!!.toInt()) {
                    native { Native.inspectionTask(it, 0) }
                } else emptyList()
                val captureEnd = System.nanoTime()
                val captureMemory = if (artworkQueries) native { JSONObject(Native.rendererMemory(it)) } else null
                val queryPool = java.util.concurrent.Executors.newSingleThreadExecutor()
                val queryResults = JSONArray()
                val beforeRevision = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("revision")
                minimumZoomPercent = Int.MAX_VALUE
                maximumZoomPercent = 0
                host.measurementReport(true)
                val present = JSONArray()
                native { Native.presentationTimings(it, true); Native.completionTimings(it, true) }
                val beforeRenderer = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
                activePresent = present
                val sessionSamples = JSONArray()
                val sessionSampler = java.util.concurrent.Executors.newSingleThreadScheduledExecutor()
                fun sessionSample() {
                    val files = device.recovery.walkTopDown().filter {it.isFile}.toList()
                    val newest = files.filter {it.name == "head.json"}.maxOfOrNull {it.lastModified()}
                    val io = File("/proc/self/io").readLines().associate {line ->line.substringBefore(':') to line.substringAfter(':').trim().toLong()}
                    synchronized(sessionSamples) {sessionSamples.put(obj("time_ns" to System.nanoTime(),"disk_bytes" to files.sumOf {it.length()},
                        "durable_age_ms" to newest?.let {System.currentTimeMillis()-it},"process_write_bytes" to io["write_bytes"],"process_wchar" to io["wchar"]))}
                }
                sessionSample()
                sessionSampler.scheduleWithFixedDelay({sessionSample()},100,100,java.util.concurrent.TimeUnit.MILLISECONDS)
                val switchedTag = languageSwitches.takeIf { it.isNotEmpty() }?.let { it[run % it.size] }
                switchLanguage = switchedTag
                val began = System.nanoTime()
                val beganBoot = SystemClock.elapsedRealtimeNanos()
                val queryFuture = queryPool.submit {
                    queryJobs.forEach { job ->
                        val start = System.nanoTime()
                        val result = JSONObject(if (levelsStatistics) Native.inspectionLevelsStatistics(job, querySource)
                            else if (statisticsPreview || statisticsExact) Native.inspectionStatistics(job, querySource, !statisticsExact, false, statisticsWaveform)
                            else Native.inspectionSample(job, "\"Visible\"", 4752f, 3168f, 101))
                        queryResults.put(result.put("begin_ns", start).put("end_ns", System.nanoTime()))
                    }
                }
                if (motion == "stroke"&&contact > 0)repeat(duration/(contact+pause)) {index ->stroke((run+1)*1000+index,contact);SystemClock.sleep(pause.toLong())}
                else if (retainedMotion || smoothZoom) stroke(run + 1, duration, motion == "hover") else gesture(duration)
                val ended = System.nanoTime()
                val endedBoot = SystemClock.elapsedRealtimeNanos()
                sessionSampler.shutdown();assertTrue(sessionSampler.awaitTermination(10,java.util.concurrent.TimeUnit.SECONDS));sessionSample()
                try { queryFuture.get(180, java.util.concurrent.TimeUnit.SECONDS) } finally { queryPool.shutdownNow() }
                activePresent = null
                val languageVisible = switchedTag?.let { tag -> waitFor { host.languageTag == tag }; System.nanoTime() }
                val resumed = languageVisible?.let {
                    val beganResume = System.nanoTime()
                    stroke(run + 20, 100)
                    waitFor { !native { Native.renderingPending(it) } }
                    beganResume
                }
                if (switchedTag == null) SystemClock.sleep(500)
                native { present.put(JSONArray(Native.presentationTimings(it, true))) }
                val completions = native { JSONArray(Native.completionTimings(it, false)) }
                val afterRevision = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("revision")
                assertNull(host.failure)
                assertNull(host.actionError)
                if (motion == "stroke") assertTrue("Replay must commit actual paint", afterRevision > beforeRevision)
                if (motion == "hover") assertEquals("Hover never paints", beforeRevision, afterRevision)
                if (smoothZoom) {
                    assertEquals("Zoom never changes the document", beforeRevision, afterRevision)
                    assertTrue("Zoom moves the camera", maximumZoomPercent > minimumZoomPercent)
                }
                val data = host.measurementReport(false).put("presentation", present).put("completions", completions).put("begin_ns", began).put("end_ns", ended)
                    .put("minimum_zoom_percent", minimumZoomPercent).put("maximum_zoom_percent", maximumZoomPercent)
                    .put("session_samples",sessionSamples)
                    .put("contact_ms",contact).put("pause_ms",pause)
                    .put("artwork_queries", queryResults)
                    .put("statistics_preview", statisticsPreview)
                    .put("statistics_exact", statisticsExact)
                    .put("statistics_waveform", statisticsWaveform)
                    .put("levels_statistics", levelsStatistics)
                    .put("clipping_preview", clippingPreview)
                    .put("query_capture_ms", (captureEnd - captureBegin) / 1e6)
                    .put("query_capture_allocator", captureMemory)
                    .put("begin_boot_ns", beganBoot).put("end_boot_ns", endedBoot)
                    .put("revision_before", beforeRevision).put("revision_after", afterRevision)
                    .put("display", native { JSONObject(Native.displayStatus(it)) })
                    .put("renderer_before", beforeRenderer).put("renderer", native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) })
                File(output, "$label-$run.json").writeText(data.toString())
                val minimumSamples = if(contact > 0)maxOf(1,100*contact/(contact+pause)) else 100
                assertTrue("CPU frame instrumentation must be enabled", data.getJSONArray("frames").length() > minimumSamples)
                assertTrue("GPU timestamps must be collected", (0 until present.length()).sumOf { present.getJSONArray(it).length() } > minimumSamples)
                if (switchedTag != null) {
                    val visible = checkNotNull(languageVisible)
                    val resumed = checkNotNull(resumed)
                    val resumedCompletions = JSONArray(completions.values().filter { (it as JSONArray).getLong(4) >= resumed })
                    val first = resumedCompletions.values().map { it as JSONArray }.firstOrNull { it.getLong(4) >= resumed }
                    data.put("language", obj("tag" to switchedTag, "requested_ns" to languageRequested,
                        "model_published_observed_ns" to visible, "gesture_end_ns" to ended, "resumed_input_ns" to resumed,
                        "resume_gap_ms" to (resumed - visible) / 1e6,
                        "first_resumed_completion_ms" to first?.let { (it.getLong(2) - resumed) / 1e6 }, "resumed_completions" to resumedCompletions))
                }
                File(output, "$label-$run.json").writeText(data.toString())
                println("VIEWPORT $label run=$run frames=${data.getJSONArray("frames").length()} renderer=${data.getJSONObject("renderer").getJSONArray("rows")}")
                if (!retainedMotion) {
                    restoreNavigationCamera()
                    SystemClock.sleep(1500)
                }
            }
            if (clippingPreview) {
                host.drain(obj("type" to "set_color", "rgba" to JSONArray(listOf(1.0, 0.0, 0.0, 1.0))))
                host.drain(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "solid_color")))
                SystemClock.sleep(1500)
                waitFor { !native { Native.renderingPending(it) } }
                val image = checkNotNull(InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot())
                var black = 0
                var white = 0
                for (y in image.height / 2 - 32 until image.height / 2 + 32) for (x in image.width / 2 - 32 until image.width / 2 + 32) {
                    val rgb = image.getPixel(x, y) and 0xffffff
                    if (rgb == 0) black++
                    if (rgb == 0xffffff) white++
                }
                File(output, "$label-clipping-marker.png").outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                image.recycle()
                File(output, "$label-clipping-marker.json").writeText(obj("black_pixels" to black, "white_pixels" to white).toString())
                check(black > 100 && white > 100) { "Native clipping presentation must show both marker stripes: $black/$white" }
            }
            assertNull(host.failure)
            native { Native.presentationTimings(it, false) }
        }
    }
}
