package art.capycanvas

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.os.Bundle
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.WindowManager
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.LockSupport
import kotlin.math.*

/** Release-optimized, opt-in photo/brush benchmark. All document state belongs
 * to a fresh workspace. Input traverses Android's normal stylus dispatcher.
 * Submitted/completed updates are deliberately not called displayed frames. */
class BrushBenchmarkInstrumentation : Instrumentation() {
    private lateinit var arguments: Bundle
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        this.arguments = arguments ?: Bundle()
        start()
    }

    override fun onStart() {
        val result = Bundle()
        var activity: MainActivity? = null
        var output: File? = null
        var label = "unknown"
        try {
            fun stage(name: String) = sendStatus(0, Bundle().apply { putString("stream", "BRUSH_STAGE $name\n") })
            stage("starting")
            check(arguments.getString("brushBenchmark") == "true") { "Opt in with -e brushBenchmark true" }
            check(!BuildConfig.DEBUG) { "Use the benchmark build" }
            label = arguments.getString("label") ?: error("A report label is required")
            check(label.matches(Regex("[a-zA-Z0-9_-]+")))
            val preset = arguments.getString("preset", "1")!!.toInt()
            val size = arguments.getString("brushSize", "1000")!!.toDouble()
            val duration = arguments.getString("durationMs", "10000")!!.toInt()
            val repeats = arguments.getString("repeats", "3")!!.toInt()
            val mode = arguments.getString("mode", "constant")!!
            val prediction = arguments.getString("prediction", "true") == "true"
            val speed = arguments.getString("speed", "1")!!.toDouble()
            check(duration in 1000..60000 && repeats in 1..10)
            check(mode in listOf("constant", "pressure", "tilt", "stationary", "lifts", "visual", "pinch"))
            output = File(targetContext.getExternalFilesDir(null), "brush-benchmark").apply { mkdirs() }
            val root = File(targetContext.cacheDir, "brush-benchmark-$label-${System.nanoTime()}")
            CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
            RecoveryController.directoryForTest = File(root, "recovery")
            ColorPreferencesStore.directoryForTest = File(root, "color")
            DocumentController.nativeFileJobsForTest = true
            val photo = (if (mode == "pinch") File(targetContext.getExternalFilesDir(null), "photo.capy").takeIf { it.isFile } else null)
                ?: File(targetContext.filesDir, "brush-benchmark.jpg")
            if (!photo.isFile) ParcelFileDescriptor.AutoCloseInputStream(
                uiAutomation.executeShellCommand("cat /data/local/tmp/capy-brush-photo.jpg")
            ).use { input -> photo.outputStream().use { input.copyTo(it) } }
            check(photo.length() > 1_000_000)
            stage("photo-staged")
            activity = startActivitySync(Intent(targetContext, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)) as MainActivity
            stage("activity-started")
            val active = activity
            runOnMainSync { active.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = active.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(condition: () -> Boolean) {
                val deadline = SystemClock.uptimeMillis() + 180_000
                while (!condition()) {
                    check(host.failure == null) { host.failure!! }
                    check(host.actionError == null) { host.actionError!! }
                    check(SystemClock.uptimeMillis() < deadline) { "Timed out waiting for brush benchmark" }
                    SystemClock.sleep(20)
                }
            }
            fun snapshot(): JSONObject {
                native { Unit } // Drain prior owner work and its main-thread publication.
                var value = JSONObject()
                runOnMainSync { value = JSONObject(host.snapshot!!.toString()) }
                return value
            }
            fun state(): JSONObject = snapshot().getJSONObject("state")
            fun display(): JSONObject = native { JSONObject(Native.displayStatus(it)) }
            fun stats(): JSONObject = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
            fun action(value: JSONObject) {
                runOnMainSync { host.dispatch(value) }
                native { Unit }
                check(host.actionError == null) { host.actionError!! }
            }
            fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
            fun report(reset: Boolean): JSONObject {
                val done = CountDownLatch(1)
                var value = JSONObject()
                host.measurements(reset) { value = it; done.countDown() }
                check(done.await(30, TimeUnit.SECONDS))
                return value
            }
            fun resources(): JSONObject = obj("boot_ns" to SystemClock.elapsedRealtimeNanos(),
                "meminfo" to File("/proc/meminfo").readText(),
                "process_status" to File("/proc/self/status").readText(),
                "process_mappings" to File("/proc/self/maps").useLines { it.count() })
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true &&
                host.workspaceManager?.optBoolean("ready") == true && host.workspaceManager?.optBoolean("busy") == false }
            stage("workspace-ready")
            val task = native { h ->
                Native.dispatch(h, obj("type" to "invoke", "command" to "open_document").toString())
                val s = JSONObject(Native.snapshot(h)!!).getJSONObject("state")
                val request = s.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
                val f = s.getJSONObject("document_file")
                Native.projectTask(h, request.getInt("id"), "null", f.getLong("epoch"), f.getLong("revision"))
            }
            try {
                Native.projectWork(task, ParcelFileDescriptor.open(photo, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                native { Native.projectAdopt(it, task, "null") }
            } finally { Native.projectFree(task) }
            stage("photo-adopted")
            runOnMainSync { host.documentChanged() }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true &&
                host.snapshot?.getJSONObject("state")?.array("tabs")?.objects()?.any { it.optInt("width") == 9504 } == true }
            stage("photo-ready")
            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true)))
            val group = snapshot().getJSONObject("layout")
                .array("groups").objects().first { "stats" in it.array("panels").values() }.getInt("id")
            action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to false)))
            action(obj("type" to "select_panel_tab", "group" to group, "panel" to "stats"))
            for (layer in state().array("layers").objects().filter { it.optString("label") == "Paper" })
                action(obj("type" to "set_layer_visibility", "id" to layer.getLong("id"), "visible" to (mode == "visual")))
            if (mode == "visual") for (layer in state().array("layers").objects().filter { it.optString("label") == "Photo" })
                action(obj("type" to "set_layer_visibility", "id" to layer.getLong("id"), "visible" to false))
            invoke("add_layer")
            invoke("fit_canvas")
            action(obj("type" to "select_brush", "id" to preset))
            action(obj("type" to "set_brush_size", "value" to size))
            for ((id, value) in listOf("feedback" to prediction, "platform_prediction" to false, "prediction_horizon" to 16))
                action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to id, "value" to value)))
            SystemClock.sleep(1500)
            waitFor { host.snapshot?.optBoolean("brush_ready") == true }
            val initial = state()
            check(abs(initial.getJSONObject("brush").getDouble("diameter") - size) < .01)
            val camera = initial.getJSONObject("camera")
            val area = camera.getJSONArray("work_area")
            val cx = area.getDouble(0) + area.getDouble(2) / 2
            val cy = area.getDouble(1) + area.getDouble(3) / 2
            val rx = 520.0
            val ry = 299.0
            val sampleInterval = 5_000_000L
            if (mode == "pinch") {
                runPinchBenchmark(this, arguments, host, output, label, cx, cy)
                result.putString("stream", "PINCH_COMPLETE $label\n")
                finish(Activity.RESULT_OK, result)
                return
            }
            fun stroke(milliseconds: Int, kind: String = mode): JSONObject {
                val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS })
                val coords = arrayOf(MotionEvent.PointerCoords())
                val begun = System.nanoTime()
                val boot = SystemClock.elapsedRealtimeNanos()
                var down = SystemClock.uptimeMillis()
                val delivered = JSONArray()
                val count = milliseconds / 5
                for (i in 0..count) {
                    val delay = begun + i * sampleInterval - System.nanoTime()
                    if (delay > 0) LockSupport.parkNanos(delay)
                    val t = i * .005
                    val progress = i.toDouble() / count
                    val angle = t * speed * 2 * PI
                    val lift = kind == "lifts" && i > 0 && i % 40 == 0
                    val restart = kind == "lifts" && i > 1 && i % 40 == 1
                    val phase = if (i == count || lift) MotionEvent.ACTION_UP else if (i == 0 || restart) MotionEvent.ACTION_DOWN else MotionEvent.ACTION_MOVE
                    if (phase == MotionEvent.ACTION_DOWN) down = SystemClock.uptimeMillis()
                    coords[0].x = (cx + (if (kind == "stationary") 0.0 else rx * cos(angle))).toFloat() + host.surfaceOrigin.x
                    coords[0].y = (cy + (if (kind == "stationary") 0.0 else ry * sin(angle))).toFloat() + host.surfaceOrigin.y
                    coords[0].pressure = if (phase == MotionEvent.ACTION_UP) 0f else if (kind == "pressure") (.55 + .45 * sin(t * 2 * PI)).toFloat() else 1f
                    coords[0].setAxisValue(MotionEvent.AXIS_TILT, if (kind == "tilt") .9f else 0f)
                    coords[0].orientation = if (kind == "tilt") .5f else 0f
                    if (kind == "visual") {
                        coords[0].x = (cx - 560 + 1120 * progress).toFloat() + host.surfaceOrigin.x
                        coords[0].y = (cy + 140 * sin(progress * 3 * PI)).toFloat() + host.surfaceOrigin.y
                        coords[0].pressure = if (phase == MotionEvent.ACTION_UP) 0f else (.15 + .85 * sin(progress * PI)).toFloat()
                        coords[0].setAxisValue(MotionEvent.AXIS_TILT, (.8 * sin(progress * PI).pow(2)).toFloat())
                        coords[0].orientation = .5f
                    }
                    val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, 1, properties, coords,
                        0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
                    try { check(uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
                    delivered.put(JSONArray(listOf(System.nanoTime(), phase, coords[0].pressure)))
                }
                return obj("begin_ns" to begun, "begin_boot_ns" to boot, "end_ns" to System.nanoTime(),
                    "end_boot_ns" to SystemClock.elapsedRealtimeNanos(), "injected" to delivered)
            }
            // Erase actual paint along the replay path. Preparing a wider ink
            // stroke keeps the photo visible and avoids timing empty erasure.
            if (preset == 3) {
                action(obj("type" to "select_brush", "id" to 1))
                action(obj("type" to "set_brush_size", "value" to 2000))
                SystemClock.sleep(1000)
                waitFor { host.snapshot?.optBoolean("brush_ready") == true }
                stroke(2000, if (mode == "visual") "visual" else "constant")
                SystemClock.sleep(1500)
                action(obj("type" to "select_brush", "id" to 3))
                action(obj("type" to "set_brush_size", "value" to size))
                SystemClock.sleep(1000)
                waitFor { host.snapshot?.optBoolean("brush_ready") == true }
            }
            val displayInfo = display()
            check(displayInfo.getString("present_mode") in listOf("SharedDemandRefresh", "Fifo"))
            File(output, "$label-info.json").writeText(obj("label" to label, "preset" to preset,
                "brush_size" to size, "mode" to mode, "prediction" to prediction, "speed" to speed,
                "duration_ms" to duration, "repeats" to repeats, "interval_ns" to sampleInterval,
                "state" to state(), "display" to displayInfo, "resources" to resources(),
                "center" to JSONArray(listOf(cx, cy)), "radii" to JSONArray(listOf(rx, ry))).toString(2))
            stroke(1500, "constant")
            SystemClock.sleep(1500)
            invoke("undo")
            SystemClock.sleep(1500)
            File(output, "$label-ready").writeText("ready")
            sendStatus(0, Bundle().apply { putString("stream", "BRUSH_READY $label\n") })
            if (arguments.getString("waitForTrace") == "true") waitFor { File(output, "$label-go").isFile }
            repeat(repeats) { run ->
                if (arguments.getString("navigationBetweenStrokes") == "true") {
                    invoke("zoom_in")
                    SystemClock.sleep(250)
                    invoke("fit_canvas")
                    SystemClock.sleep(arguments.getString("navigationSettleMs", "750")!!.toLong().coerceIn(0, 5000))
                    check(display().getString("present_mode") == "Fifo")
                    check(!display().getBoolean("retained_target"))
                }
                val beforeRevision = state().getJSONObject("document_file").getLong("revision")
                val before = stats()
                val displayBefore = display()
                report(true)
                native { Native.presentationTimings(it, true) }
                native { Native.completionTimings(it, true) }
                val motion = stroke(duration)
                val displayAfterInput = display()
                check(displayAfterInput.getString("present_mode") == "SharedDemandRefresh")
                check(displayAfterInput.getBoolean("retained_target"))
                SystemClock.sleep(1000)
                val data = report(false)
                val present = native { JSONArray(Native.presentationTimings(it, false)) }
                val completions = native { JSONArray(Native.completionTimings(it, false)) }
                check(completions.length() < 32768) { "Completion observation capacity exceeded" }
                val after = state()
                check(host.failure == null) { host.failure!! }
                check(host.actionError == null) { host.actionError!! }
                check(after.getJSONObject("document_file").getLong("revision") > beforeRevision) { "No committed paint" }
                check(data.getJSONArray("frames").length() > 0)
                data.put("motion", motion).put("presentation", present).put("renderer_before", before)
                    .put("completions", completions)
                    .put("renderer_after", stats()).put("display_before", displayBefore)
                    .put("display_after_input", displayAfterInput).put("display_after_drain", display())
                    .put("state_after", after).put("resources_after", resources())
                File(output, "$label-$run.json").writeText(data.toString())
                if (run == 0) uiAutomation.takeScreenshot()?.let { bitmap ->
                    File(output, "$label.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                    bitmap.recycle()
                }
                if (run == 0 && mode == "visual") {
                    val zoom = state().getJSONObject("camera").getDouble("zoom")
                    runOnMainSync { host.scroll(cx.toFloat(), (cy - 140).toFloat(), 0f,
                        (ln(zoom) / .0015 / 40).toFloat(), true, false) }
                    native { Unit }
                    SystemClock.sleep(1500)
                    check(abs(state().getJSONObject("camera").getDouble("zoom") - 1.0) < .001)
                    uiAutomation.takeScreenshot()?.let { bitmap ->
                        File(output, "$label-detail.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                        bitmap.recycle()
                    }
                    invoke("fit_canvas")
                }
                sendStatus(0, Bundle().apply { putString("stream", "BRUSH_RUN $label $run\n") })
                if (mode == "lifts") repeat(duration / 200) { invoke("undo") } else invoke("undo")
                SystemClock.sleep(2000)
            }
            result.putString("stream", "\nBRUSH_COMPLETE $label\n")
        } catch (error: Throwable) {
            output?.let { File(it, "$label-error.txt").writeText(error.stackTraceToString()) }
            result.putString("stream", "\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
            return
        } finally {
            activity?.let { runOnMainSync { it.finish() } }
            CanvasHost.workspaceDirectoryForTest = null
            RecoveryController.directoryForTest = null
            ColorPreferencesStore.directoryForTest = null
            DocumentController.nativeFileJobsForTest = false
        }
        finish(Activity.RESULT_OK, result)
    }
}
