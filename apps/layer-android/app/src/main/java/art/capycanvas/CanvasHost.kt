package art.capycanvas

import android.app.Application
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

internal fun obj(vararg pairs: Pair<String, Any?>) = JSONObject().apply {
    pairs.forEach { (key, value) -> put(key, value ?: JSONObject.NULL) }
}
internal fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
internal fun JSONArray.values(): List<Any> = (0 until length()).map { get(it) }
internal fun JSONObject.array(key: String) = optJSONArray(key) ?: JSONArray()
internal fun JSONObject.objectOrNull(key: String) = optJSONObject(key)
internal fun JSONObject.number(key: String, default: Double = 0.0) = optDouble(key, default).toFloat()

/** Platform ownership and transport, not application policy. The UI never waits
 * for a GPU submission. One dedicated Looper owns both Rust and the swapchain. */
class CanvasHost(application: Application) : AndroidViewModel(application) {
    var snapshot by mutableStateOf<JSONObject?>(null)
        private set
    var catalog by mutableStateOf(JSONObject())
        private set
    var failure by mutableStateOf<String?>(null)
        private set
    var actionError by mutableStateOf<String?>(null)
        private set
    // Native focus, not application state; prevents typing from invoking tools.
    var editingText = false
    private val main = Handler(Looper.getMainLooper())
    private val thread = HandlerThread("capy-canvas", Process.THREAD_PRIORITY_DISPLAY).apply { start() }
    private val worker = Handler(thread.looper)
    private val saved = application.getSharedPreferences("capy-canvas", 0)
    private var handle = 0L
    private var choreographer: Choreographer? = null
    private var attached = false
    private var scheduled = false
    private var disposed = false
    private var frameInterval = 8_333_333L
    private var lastSnapshot = ""
    private var snapshotAt = 0L
    private var savedWorkspace = ""
    private val measuredFrames = if (BuildConfig.DEBUG) ArrayList<LongArray>() else null
    private val measuredInputs = if (BuildConfig.DEBUG) ArrayList<LongArray>() else null
    private val suppressedContacts = mutableSetOf<Long>()

    init {
        worker.post {
            attempt {
                handle = Native.create(BuildConfig.DEBUG)
                choreographer = Choreographer.getInstance()
                attempt(canvas = false) {
                    saved.getString("settings", null)?.let { Native.dispatch(handle, obj("type" to "restore_settings", "settings" to JSONObject(it)).toString()) }
                }
                attempt(canvas = false) {
                    saved.getString("workspace", null)?.let { Native.dispatch(handle, obj("type" to "restore_workspace", "workspace" to JSONObject(it)).toString()) }
                }
                val value = JSONObject(Native.query(handle, obj("type" to "catalog").toString()))
                main.post { catalog = value }
                publish(true)
            }
        }
    }
    private fun attempt(canvas: Boolean = true, block: () -> Unit) {
        try { block() } catch (e: Exception) {
            Log.e("CapyCanvas", "Native canvas operation failed", e)
            main.post {
                if (canvas) failure = e.message ?: "Could not initialize canvas"
                else actionError = e.message ?: "Could not complete this action"
            }
        }
    }
    private fun post(canvas: Boolean = false, block: () -> Unit) {
        worker.post { if (!disposed && handle != 0L) attempt(canvas, block) }
    }
    fun clearActionError() { actionError = null }
    fun dispatch(action: JSONObject) = post {
        Native.dispatch(handle, action.toString())
        refreshChrome()
        publish(true)
        wake()
    }
    fun invoke(command: String) = dispatch(obj("type" to "invoke", "command" to command))
    fun customize(action: JSONObject) = dispatch(obj("type" to "customize", "action" to action))
    fun preference(action: JSONObject) = dispatch(obj("type" to "preferences", "action" to action))
    fun query(query: JSONObject, reply: (Any?) -> Unit) = post {
        val value = org.json.JSONTokener(Native.query(handle, query.toString())).nextValue()
        main.post { reply(if (value == JSONObject.NULL) null else value) }
    }
    fun input(input: JSONObject, reply: ((JSONObject) -> Unit)? = null) = post {
        val value = JSONObject(Native.input(handle, input.toString()))
        if (reply != null) main.post { reply(value) }
        publish(false)
        wake()
    }
    // These are presentation facts, supplied by native widgets. Rust owns Zen
    // visibility and the rules for dismissing/pinning expanded panels.
    private var chromeFacts = obj("held" to false, "dragging" to false, "popup_open" to false)
    private var logicalWidth = 1f
    private var logicalHeight = 1f
    private var surfaceDensity = 1f
    fun chrome(event: JSONObject, facts: JSONObject? = null, reply: ((JSONObject) -> Unit)? = null) = post {
        if (facts != null) chromeFacts = facts
        val result = JSONObject(Native.input(handle, chromeInput(event).toString()))
        if (reply != null) main.post { reply(result) }
        publish(true)
    }
    private fun chromeInput(event: JSONObject) = obj("type" to "chrome", "event" to event,
        "facts" to chromeFacts, "viewport" to JSONArray(listOf(logicalWidth, logicalHeight)))
    private fun refreshChrome() { Native.input(handle, chromeInput(obj("kind" to "refresh")).toString()) }

    fun attach(surface: Surface, width: Int, height: Int, density: Float, refreshRate: Float) = post(canvas = true) {
        logicalWidth = width / density; logicalHeight = height / density; surfaceDensity = density
        frameInterval = (1_000_000_000.0 / refreshRate.coerceAtLeast(30f)).toLong()
        Native.resize(handle, width, height, density)
        Native.attach(handle, surface)
        attached = true
        main.post { failure = null }
        publish(true)
        wake()
    }
    fun resize(width: Int, height: Int, density: Float) = post {
        logicalWidth = width / density; logicalHeight = height / density; surfaceDensity = density
        Native.resize(handle, width, height, density)
        publish(true)
        wake()
    }
    /** SurfaceHolder requires rendering to have stopped before this callback
     * returns. This wait is only at surface teardown, never in an input/frame. */
    fun detach() {
        val stopped = CountDownLatch(1)
        if (!worker.post {
            try { if (handle != 0L) { attached = false; Native.detach(handle) } }
            finally { stopped.countDown() }
        }) return // The owning thread has already destroyed the native session.
        check(stopped.await(10, TimeUnit.SECONDS)) { "Canvas surface did not detach" }
    }
    fun pointer(id: Long, tool: Int, button: Int, samples: DoubleArray, predicted: Boolean = false) {
        val arrival = System.nanoTime()
        post(canvas = true) {
            val started = System.nanoTime()
            val phase = samples[samples.size - 1].toInt()
            if (phase == 1 && !predicted) {
                val event = obj("kind" to "contact", "canvas" to true,
                    "position" to JSONArray(listOf(samples[0] / surfaceDensity, samples[1] / surfaceDensity)))
                val reply = JSONObject(Native.input(handle, chromeInput(event).toString()))
                if (reply.optBoolean("handled")) suppressedContacts.add(id)
            }
            if (id !in suppressedContacts) Native.pointer(handle, id, tool, button, samples, predicted)
            if (phase == 3 || phase == 4) suppressedContacts.remove(id)
            if (!predicted && measuredInputs != null && measuredInputs.size < 8192) measuredInputs.add(longArrayOf(
                samples[samples.size - 2].toLong(), arrival, started, System.nanoTime() - started, (samples.size / 9).toLong()))
            wake()
        }
    }
    fun scroll(x: Float, y: Float, dx: Float, dy: Float, zoom: Boolean, horizontal: Boolean) = post {
        Native.scroll(handle, x, y, dx * 40, dy * 40, zoom, horizontal)
        wake()
    }
    private fun wake() {
        if (!attached || scheduled) return
        scheduled = true
        if (Build.VERSION.SDK_INT >= 33) choreographer!!.postVsyncCallback { data ->
            draw(data.frameTimeNanos, data.preferredFrameTimeline.expectedPresentationTimeNanos)
        } else choreographer!!.postFrameCallback { time -> draw(time, time + frameInterval) }
    }
    private fun draw(frameTime: Long, expectedPresentation: Long) {
        scheduled = false
        if (!attached || disposed) return
        attempt {
            val start = System.nanoTime()
            val again = Native.frame(handle, start, expectedPresentation.coerceAtLeast(start))
            val elapsed = System.nanoTime() - start
            if (measuredFrames != null && measuredFrames.size < 8192) measuredFrames.add(longArrayOf(frameTime, start, elapsed, expectedPresentation) + Native.frameCost(handle))
            publish(!again)
            if (again) wake()
        }
    }
    /** Debug-build measurement only. CPU submission is deliberately not labelled
     * GPU completion or on-screen presentation; collect compositor data separately. */
    fun measurements(reset: Boolean = false, reply: (JSONObject) -> Unit) = post {
        val report = obj("frames" to JSONArray(measuredFrames?.map { JSONArray(it.toList()) } ?: emptyList<Any>()),
            "inputs" to JSONArray(measuredInputs?.map { JSONArray(it.toList()) } ?: emptyList<Any>()),
            "frame_fields" to JSONArray(listOf("vsync_ns", "start_ns", "cpu_render_present_ns", "expected_presentation_ns", "paint_ns", "acquire_ns", "viewport_ns", "queue_present_ns", "poll_ns")),
            "input_fields" to JSONArray(listOf("event_ns", "arrival_ns", "worker_start_ns", "cpu_input_ns", "sample_count")))
        if (reset) { measuredFrames?.clear(); measuredInputs?.clear() }
        main.post { reply(report) }
    }
    private fun publish(force: Boolean) {
        val now = SystemClock.uptimeMillis()
        if (!force && now - snapshotAt < 33) return
        snapshotAt = now
        val serialized = Native.snapshot(handle)
        if (serialized == lastSnapshot) return
        lastSnapshot = serialized
        val next = JSONObject(serialized)
        val state = next.getJSONObject("state")
        val workspace = state.getJSONObject("workspace").toString()
        if (workspace != savedWorkspace) {
            savedWorkspace = workspace
            saved.edit().putString("workspace", workspace).apply()
        }
        state.array("requests").objects().forEach { request ->
            val kind = request.getJSONObject("kind")
            when (kind.getString("type")) {
                "save_settings" -> {
                    saved.edit().putString("settings", kind.getJSONObject("settings").toString()).apply()
                    Native.dispatch(handle, obj("type" to "complete_request", "id" to request.getLong("id"), "error" to null).toString())
                }
            }
        }
        main.post { snapshot = next }
    }
    override fun onCleared() {
        worker.post {
            disposed = true; attached = false
            if (handle != 0L) { Native.destroy(handle); handle = 0 }
            thread.quitSafely()
        }
    }
}
