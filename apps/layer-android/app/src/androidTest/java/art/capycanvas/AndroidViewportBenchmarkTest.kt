package art.capycanvas

import android.os.SystemClock
import android.view.WindowManager
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Test
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.*

/** Opt-in pen replay through either Android input dispatch or the canvas owner. */
class AndroidViewportBenchmarkTest {
    @Test fun retainedViewport() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("viewportBenchmark") == "true")
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val root = File(instrumentation.targetContext.cacheDir, "gpen-${System.nanoTime()}")
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        RecoveryController.directoryForTest = File(root, "recovery")
        ColorPreferencesStore.directoryForTest = File(root, "color")
        DocumentController.nativeFileJobsForTest = true
        val osInput = args.getString("osInput", "false") == "true"
        val prediction = args.getString("prediction", "true") == "true"
        val label = args.getString("label", "baseline")!!
        val interval = args.getString("intervalMs", "4.166667")!!.toDouble()
        val duration = args.getString("durationMs", "5000")!!.toInt()
        val repeats = args.getString("repeats", "3")!!.toInt()
        val size = args.getString("canvasSize", "1024")!!.toInt()
        val pressure = args.getString("pressure")?.toDouble()
        val speed = args.getString("speed", "1")!!.toDouble()
        val brushSize = args.getString("brushSize", "18")!!.toDouble()
        val strokeOffset = args.getString("strokeOffset", "0")!!.toDouble()
        val zoomSteps = args.getString("zoomSteps", "0")!!.toInt()
        val transparency = args.getString("transparency")?.let { listOf("off", "low", "medium", "high").indexOf(it) }
        val motion = args.getString("motion", "stroke")!!
        check(motion in listOf("stroke", "pan", "pinch"))
        try { ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(condition: () -> Boolean) {
                val start = SystemClock.uptimeMillis()
                while (!condition()) { assertNull(host.failure); check(SystemClock.uptimeMillis() - start < 120_000) { "G-pen did not settle: ${host.actionError}" }; SystemClock.sleep(20) }
            }
            fun report(reset: Boolean): JSONObject { val done = CountDownLatch(1); var value = JSONObject(); host.measurements(reset) { value = it; done.countDown() }; check(done.await(30, TimeUnit.SECONDS)); return value }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true && host.workspaceManager?.optBoolean("ready") == true && host.workspaceManager?.optBoolean("busy") == false }
            val task = native { h ->
                Native.dispatch(h, obj("type" to "invoke", "command" to "new_document").toString())
                val s = JSONObject(Native.snapshot(h)!!).getJSONObject("state")
                val request = s.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
                val f = s.getJSONObject("document_file")
                Native.projectTask(h, request.getInt("id"), "null", f.getLong("epoch"), f.getLong("revision"))
            }
            try {
                Native.projectOptions(task, obj("extent" to JSONArray(listOf(size, size)), "color" to obj("space" to "Srgb", "depth" to "U8"), "background" to "White").toString())
                Native.projectWork(task, -1, size, size)
                native { Native.projectAdopt(it, task, "null") }
            } finally { Native.projectFree(task) }
            scenario.onActivity { host.documentChanged(); host.invoke("fit_canvas"); repeat(zoomSteps) { host.invoke("zoom_in") } }
            waitFor { host.snapshot?.getJSONObject("state")?.getJSONArray("tabs")?.getJSONObject(0)?.optInt("width") == size && host.snapshot?.optBoolean("brush_ready") == true }
            val preset = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }.first { it.getString("label") == "G-Pen" }.getInt("id")
            scenario.onActivity {
                host.dispatch(obj("type" to "select_brush", "id" to preset))
                host.dispatch(obj("type" to "set_brush_size", "value" to brushSize))
                host.preference(obj("type" to "edit", "id" to "feedback", "value" to prediction))
                host.preference(obj("type" to "edit", "id" to "platform_prediction", "value" to false))
                transparency?.let { host.preference(obj("type" to "edit", "id" to "transparency", "value" to it)) }
            }
            SystemClock.sleep(1500)
            assertNull(host.actionError)
            val state = host.snapshot!!.getJSONObject("state")
            val camera = state.getJSONObject("camera")
            val area = camera.getJSONArray("work_area")
            val cx = area.getDouble(0) + area.getDouble(2) * (.5 + strokeOffset)
            val cy = area.getDouble(1) + area.getDouble(3) / 2
            val radius = min(area.getDouble(2), area.getDouble(3)) * .32
            val output = File(activity.getExternalFilesDir(null), "viewport-benchmark").apply { mkdirs() }
            var activePresent: JSONArray? = null
            fun stroke(run: Int, milliseconds: Int) {
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
                    val t = i * interval / 1000.0 * speed
                    val x = cx + radius * sin(t * 3.2)
                    val y = cy + radius * .65 * sin(t * 4.7 + run * .31)
                    val p = pressure ?: (.65 + .3 * sin(t * 1.7))
                    if (osInput) {
                        coords[0].x = x.toFloat() + host.surfaceOrigin.x
                        coords[0].y = y.toFloat() + host.surfaceOrigin.y
                        coords[0].pressure = if (i == count) 0f else p.toFloat()
                        val action = if (i == 0) android.view.MotionEvent.ACTION_DOWN else if (i == count) android.view.MotionEvent.ACTION_UP else android.view.MotionEvent.ACTION_MOVE
                        val event = android.view.MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1,
                            properties, coords, 0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
                        try { check(instrumentation.uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
                    } else {
                        val records = host.pointerBuffer(9)
                        doubleArrayOf(x, y, p, 0.0, 0.0, 0.0, 0.0, System.nanoTime().toDouble(),
                            (if (i == 0) 1 else if (i == count) 3 else 2).toDouble()).copyInto(records)
                        host.pointer((9300 + run).toLong(), 0, 0, records, 9)
                    }
                    if (i % 120 == 119 && activePresent != null) native {
                        activePresent!!.put(JSONArray(Native.presentationTimings(it, true)))
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
                        else Triple(cx, cy, 150 * exp(.4 * sin(phase)))
                    for (i in 0..1) {
                        coords[i].x = (x + (if (i == 0) -spread else spread)).toFloat() + host.surfaceOrigin.x
                        coords[i].y = y.toFloat() + host.surfaceOrigin.y
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
                }
                val end = count * interval / 1000.0 * speed
                send(android.view.MotionEvent.ACTION_POINTER_UP or (1 shl pointer), 2, end)
                send(android.view.MotionEvent.ACTION_UP, 1, end)
            }
            stroke(0, 1500); SystemClock.sleep(800)
            val info = obj("label" to label, "os_input" to osInput, "prediction" to prediction, "interval_ms" to interval, "duration_ms" to duration, "pressure" to pressure, "speed" to speed, "state" to state,
                "display" to native { JSONObject(Native.displayStatus(it)) })
            File(output, "$label-info.json").writeText(info.toString(2))
            assertEquals("SharedDemandRefresh", info.getJSONObject("display").getString("present_mode"))
            assertTrue(info.getJSONObject("display").getBoolean("retained_target"))
            assertTrue("Navigator survives document adoption", info.getJSONObject("display").getInt("overview_count") > 0)
            native { Native.presentationTimings(it, true) }
            repeat(repeats) { run ->
                val beforeRevision = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("revision")
                report(true)
                val present = JSONArray()
                native { Native.presentationTimings(it, true); Native.completionTimings(it, true) }
                activePresent = present
                val began = System.nanoTime()
                if (motion == "stroke") stroke(run + 1, duration) else gesture(duration)
                activePresent = null
                SystemClock.sleep(500)
                val ended = System.nanoTime()
                native { present.put(JSONArray(Native.presentationTimings(it, true))) }
                val completions = native { JSONArray(Native.completionTimings(it, false)) }
                val afterRevision = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("revision")
                assertNull(host.failure)
                assertNull(host.actionError)
                if (motion == "stroke") assertTrue("Replay must commit actual paint", afterRevision > beforeRevision)
                val data = report(false).put("presentation", present).put("completions", completions).put("begin_ns", began).put("end_ns", ended)
                    .put("revision_before", beforeRevision).put("revision_after", afterRevision)
                    .put("display", native { JSONObject(Native.displayStatus(it)) })
                    .put("renderer", native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) })
                assertTrue("CPU frame instrumentation must be enabled", data.getJSONArray("frames").length() > 100)
                assertTrue("GPU timestamps must be collected", (0 until present.length()).sumOf { present.getJSONArray(it).length() } > 100)
                File(output, "$label-$run.json").writeText(data.toString())
                println("VIEWPORT $label run=$run frames=${data.getJSONArray("frames").length()} renderer=${data.getJSONObject("renderer").getJSONArray("rows")}")
                if (motion != "stroke") {
                    scenario.onActivity { host.invoke("fit_canvas"); repeat(zoomSteps) { host.invoke("zoom_in") } }
                    SystemClock.sleep(1500)
                }
            }
            assertNull(host.failure)
            native { Native.presentationTimings(it, false) }
        } } finally {
            CanvasHost.workspaceDirectoryForTest = null
            RecoveryController.directoryForTest = null
            ColorPreferencesStore.directoryForTest = null
            DocumentController.nativeFileJobsForTest = false
        }
    }
}
