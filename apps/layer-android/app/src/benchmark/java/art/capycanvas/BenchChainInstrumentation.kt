package art.capycanvas

import android.accessibilityservice.AccessibilityService
import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.RectF
import android.net.Uri
import android.os.Bundle
import android.os.Debug
import android.os.ParcelFileDescriptor
import android.os.StatFs
import android.os.SystemClock
import android.view.Choreographer
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.inspector.WindowInspector
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.lifecycle.Lifecycle
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.locks.LockSupport
import kotlin.math.*

class BenchChainInstrumentation : Instrumentation() {
    private class Skipped(reason: String) : Exception(reason)
    private class Refused(reason: String) : Exception(reason)
    private class Timeout(reason: String) : Exception(reason)
    private class Opened(val tab: Long, val paint: Long, val photo: Long?)
    private class Area(val x: Double, val y: Double, val w: Double, val h: Double) {
        fun at(fx: Double, fy: Double) = doubleArrayOf(x + w * fx, y + h * fy)
    }

    private val aliases = mapOf("transform" to "scale_rotate", "apply" to "apply_transform", "magic_wand" to "auto_select")
    private val workspaceIds = mapOf("photo" to "builtin:workspace:photographer")
    private lateinit var args: Bundle
    private lateinit var chain: JSONObject
    private lateinit var activity: MainActivity
    private lateinit var host: CanvasHost
    private var record = JSONObject()
    private var alive = false
    private var shownNs: Long? = null
    private var refreshHz = 60.0
    private var canvas: JSONArray? = null
    private var startWorkspace: String? = null
    private var memoryEnd = JSONObject()
    private var active: String? = null
    private val documents = mutableMapOf<String, Opened>()
    private val staged = mutableMapOf<String, Result<File>>()
    private val saved = mutableMapOf<String, File>()

    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        args = arguments ?: Bundle()
        start()
    }

    override fun onStart() {
        val result = Bundle()
        val code = try { runChain(); Activity.RESULT_OK } catch (error: Throwable) {
            result.putString("stream", "\n${error.stackTraceToString()}\n"); Activity.RESULT_CANCELED
        } finally {
            if (::activity.isInitialized) runCatching { runOnMainSync { if (!activity.isFinishing) activity.finish() } }
            AppStorage.directoryForTest = null
        }
        finish(code, result)
    }

    private fun runChain() {
        check(!BuildConfig.DEBUG) { "Use the benchmark build" }
        val number = args.getString("pass", "1")!!.toInt()
        val cold = args.getString("cold") == "true"
        val selected = args.getString("steps")?.split(',')?.map(String::trim)?.toSet()
        val bytes = shellFile(requireNotNull(args.getString("chain")) { "Pass -e chain PATH" }).use { it.readBytes() }
        chain = JSONObject(String(bytes))
        check(chain.getInt("schema") == 1) { "Unknown chain schema" }
        val output = File(targetContext.getExternalFilesDir(null), "bench-chain/pass-$number").apply { deleteRecursively(); mkdirs() }
        val root = File(targetContext.cacheDir, "bench-chain")
        check(root.deleteRecursively()) { "Could not clear the benchmark data directory" }
        AppStorage.directoryForTest = root
        check(targetContext.deleteSharedPreferences(CanvasHost.preferencesName)) { "Could not clear benchmark preferences" }
        if (cold) File(targetContext.cacheDir, "shaders").deleteRecursively()
        val directory = File(targetContext.cacheDir, "bench-chain-documents").apply { deleteRecursively(); mkdirs() }
        for (name in chain.getJSONObject("documents").keys()) args.getString(name)?.let { remote ->
            staged[name] = runCatching { File(directory, File(remote).name).also { local ->
                shellFile(remote).use { input -> local.outputStream().use { input.copyTo(it) } }
                check(local.length() > 0) { "$remote is empty or unreadable" }
            } }
        }
        val steps = JSONArray()
        val pass = obj("schema" to 1, "host" to "android", "device" to args.getString("device"), "version" to args.getString("version"),
            "commit" to args.getString("commit"), "app_id" to targetContext.packageName, "pass" to number, "cold" to cold,
            "clock" to "boottime", "refresh_hz" to null, "canvas" to null,
            "chain_sha256" to MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) },
            "steps" to steps, "memory_end" to null)
        fun write() = File(output, "pass.json").writeText(pass.put("refresh_hz", refreshHz).put("canvas", canvas ?: JSONObject.NULL)
            .put("memory_end", memoryEnd).toString())
        var opened: String? = null
        var stuck: String? = null
        var drain = false
        for (step in chain.getJSONArray("steps").objects()) {
            if (step.getString("do") == "open") opened = step.getString("document")
            val id = step.getString("id")
            if (selected != null && id !in selected) continue
            stuck?.let { reason ->
                steps.put(obj("id" to id, "status" to "error", "reason" to "not run: $reason", "input" to "real", "begin_ns" to now(),
                    "end_ns" to now(), "surface" to step.optString("surface", "canvas"), "motion" to JSONArray(), "contacts" to JSONArray(),
                    "completions" to JSONArray(), "settled_ns" to null, "latency" to JSONArray(), "extra" to JSONObject()))
                continue
            }
            val kind = step.getString("do")
            val needs = listOfNotNull(opened.takeIf { kind !in setOf("launch", "resume", "close") }, step.optString("document").ifEmpty { null }) +
                step.array("tabs").values().map { it as String }
            record = obj("id" to id, "status" to "ok", "reason" to null, "input" to "real", "begin_ns" to now(), "end_ns" to null,
                "surface" to step.optString("surface", "canvas"), "motion" to JSONArray(), "contacts" to JSONArray(),
                "completions" to JSONArray(), "settled_ns" to null, "latency" to JSONArray(), "extra" to JSONObject())
            try {
                if (needs.any { it !in staged }) throw Skipped("fixture not provided")
                if (kind != "open") needs.firstOrNull { it !in documents }?.let { throw Skipped("$it did not open") }
                if (kind != "launch") {
                    check(alive) { "The app is not running" }
                    await("idle before $id", if (drain) 300_000 else 60_000) { quiet() }
                }
                record.put("begin_ns", now())
                perform(step)
            } catch (skip: Skipped) {
                record.put("status", "skipped").put("reason", skip.message)
            } catch (refusal: Refused) {
                record.put("status", "refused").put("reason", refusal.message)
                runCatching { main { host.clearActionError() } }
            } catch (timeout: Timeout) {
                record.put("status", "timeout").put("reason", timeout.message)
            } catch (error: Throwable) {
                record.put("status", "error").put("reason", error.message ?: error.toString())
                extra().put("error", error.stackTraceToString())
                if (alive) runCatching { main { host.clearActionError() } }
            }
            record.put("end_ns", now())
            drain = record.getString("status") == "timeout"
            if (record.optString("reason").startsWith("Timed out waiting for idle")) stuck = record.getString("reason")
            if (alive) sampleMemory()
            steps.put(record)
            write()
            progress("BENCH_STEP $id ${record.getString("status")} ${if (record.isNull("reason")) "" else record.getString("reason")}")
            if (alive && host.failure != null) break
        }
        write()
    }

    private fun perform(step: JSONObject) = when (step.getString("do")) {
        "launch" -> launch()
        "open" -> open(step)
        "stroke" -> stroke(step)
        "gesture" -> gesture(step)
        "drag" -> drag(step)
        "tap" -> tap(step)
        "repeat" -> repeatItems(step)
        "scrub" -> scrub(step)
        "save" -> save(step.getString("document"))
        "resume" -> resume()
        "close" -> close()
        else -> throw Skipped("unknown step kind ${step.getString("do")}")
    }

    private fun progress(line: String) = sendStatus(0, Bundle().apply { putString("stream", "$line\n") })
    private fun now() = SystemClock.elapsedRealtimeNanos()
    private fun extra(): JSONObject = record.getJSONObject("extra")
    private fun motion(window: LongArray) = record.getJSONArray("motion").put(obj("begin_ns" to window[0], "end_ns" to window[1]))
    private fun latency(name: String, input: Long, settled: Long, surface: String) {
        record.getJSONArray("latency").put(obj("name" to name, "input_ns" to input, "ready_ns" to (shownNs ?: settled),
            "settled_ns" to settled, "surface" to surface))
        shownNs = null
    }
    private fun motionMs(step: JSONObject) = if (step.has("duration_ms")) step.getDouble("duration_ms")
        else step.optInt("motion_frames", chain.optInt("motion_frames", 150)) * 1000.0 / refreshHz

    private fun shellFile(path: String): ParcelFileDescriptor.AutoCloseInputStream {
        check(path.matches(Regex("/[A-Za-z0-9_./-]+"))) { "Unsupported path $path" }
        return ParcelFileDescriptor.AutoCloseInputStream(uiAutomation.executeShellCommand("cat $path"))
    }
    private fun shell(command: String) =
        ParcelFileDescriptor.AutoCloseInputStream(uiAutomation.executeShellCommand(command)).use { String(it.readBytes()) }

    private fun <T> main(block: () -> T): T {
        var result: Result<T>? = null
        runOnMainSync { result = runCatching(block) }
        return result!!.getOrThrow()
    }
    private fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    private fun state(): JSONObject = host.snapshot!!.getJSONObject("state")
    private fun revision() = state().getJSONObject("document_file").getLong("revision")
    private fun quiet() = !host.drawingTabs.switching && !host.documents.working
    private fun phone() = host.snapshot!!.getJSONObject("layout").objectOrNull("phone") != null
    private fun failed() = host.failure ?: host.actionError

    private fun await(label: String, timeoutMs: Long = 120_000, ready: (() -> Boolean)? = null): Long {
        var beat = SystemClock.uptimeMillis()
        val deadline = beat + timeoutMs
        shownNs = null
        while (true) {
            failed()?.let { error(it) }
            val met = main { host.snapshot != null && ready?.invoke() != false }
            if (met && ready != null && shownNs == null) shownNs = now()
            if (met && !native { Native.renderingPending(it) }) return now()
            val at = SystemClock.uptimeMillis()
            if (at >= deadline) throw Timeout("Timed out waiting for $label after ${timeoutMs / 1000} s" + (shownNs?.let { " (shown, not settled)" } ?: ""))
            if (at - beat >= 30_000) { beat = at; progress("BENCH_WAIT $label") }
            LockSupport.parkNanos(2_000_000)
        }
    }
    private fun dispatch(action: JSONObject) {
        main { host.dispatch(action) }
        native { }
        failed()?.let { error(it) }
    }
    private fun commandEnabled(id: String) = main {
        state().array("commands").objects().firstOrNull { it.getString("id") == id }?.optBoolean("enabled", true)
    }
    private fun command(name: String) {
        val id = aliases[name] ?: name
        when (commandEnabled(id)) {
            null -> throw Skipped("command $id is not in this build")
            false -> error("command $id is disabled")
            else -> dispatch(obj("type" to "invoke", "command" to id))
        }
    }
    private fun report(reset: Boolean): JSONObject {
        val done = CountDownLatch(1)
        var value = JSONObject()
        host.measurements(reset) { value = it; done.countDown() }
        check(done.await(30, TimeUnit.SECONDS)) { "Measurement report timed out" }
        return value
    }
    private fun sampleMemory() {
        memoryEnd = obj("app_pss_kib" to Debug.getPss())
        runCatching { org.json.JSONTokener(native { Native.rendererMemory(it) }).nextValue() as? JSONObject }.getOrNull()
            ?.takeIf { it.has("allocated_bytes") }?.let { memoryEnd.put("gpu_allocated_bytes", it.getLong("allocated_bytes")) }
    }
    private fun freeBytes() = StatFs(targetContext.cacheDir.path).availableBytes
    private fun activeSize() = main { state().array("tabs").objects().first { it.optBoolean("active") }.let { it.getDouble("width") to it.getDouble("height") } }
    private fun place(step: JSONObject) {
        val anchor = step.optJSONArray("anchor")
        when (val zoom = step.opt("zoom")) {
            "fit" -> command("fit_canvas")
            is Number -> {
                if (anchor != null) command("fit_canvas")
                dispatch(obj("type" to "set_zoom", "zoom" to zoom.toDouble()))
            }
        }
        if (anchor != null) {
            val (width, height) = activeSize()
            val position = JSONArray(listOf(4 + anchor.getDouble(0) * width, 4 + anchor.getDouble(1) * height))
            for (phase in listOf("down", "up")) dispatch(obj("type" to "navigator", "phase" to phase, "position" to position, "viewport" to JSONArray(listOf(width + 8, height + 8))))
        }
        await("camera placement")
    }
    private fun composite(): JSONObject {
        command("fit_canvas")
        await("Fit before the capture")
        val a = area()
        val shot = uiAutomation.takeScreenshot() ?: error("The screenshot failed")
        val pixels = shot.copy(Bitmap.Config.ARGB_8888, false).also { shot.recycle() }
        try {
            val x = a.x.roundToInt().coerceIn(0, pixels.width - 1)
            val y = a.y.roundToInt().coerceIn(0, pixels.height - 1)
            val w = a.w.roundToInt().coerceIn(1, pixels.width - x)
            val h = a.h.roundToInt().coerceIn(1, pixels.height - y)
            val small = Bitmap.createScaledBitmap(Bitmap.createBitmap(pixels, x, y, w, h), 32, max(1, (32.0 * h / w).roundToInt()), true)
            val rgb = IntArray(small.width * small.height).also { small.getPixels(it, 0, small.width, 0, 0, small.width, small.height) }
            return obj("w" to small.width, "h" to small.height, "rgb" to rgb.joinToString("") { "%06x".format(it and 0xffffff) })
        } finally { pixels.recycle() }
    }
    private fun area(): Area = main {
        val a = state().getJSONObject("camera").getJSONArray("work_area")
        Area(a.getDouble(0) + host.surfaceOrigin.x, a.getDouble(1) + host.surfaceOrigin.y, a.getDouble(2), a.getDouble(3))
    }

    private fun inject(action: Int, down: Long, points: List<DoubleArray>, stylus: Boolean, pressure: Float) {
        val properties = Array(points.size) { i -> MotionEvent.PointerProperties().apply {
            id = i + if (stylus) 7 else 11; toolType = if (stylus) MotionEvent.TOOL_TYPE_STYLUS else MotionEvent.TOOL_TYPE_FINGER
        } }
        val coords = Array(points.size) { i -> MotionEvent.PointerCoords().apply {
            x = points[i][0].toFloat(); y = points[i][1].toFloat(); this.pressure = pressure; size = if (stylus) 0f else .1f
        } }
        val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, points.size, properties, coords, 0, 0, 1f, 1f, 0, 0,
            if (stylus) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
        try { check(uiAutomation.injectInputEvent(event, false)) { "Input injection was refused" } } finally { event.recycle() }
    }
    private fun contacts(stylus: Boolean, durationMs: Double, count: Int = 1, pressure: (Double) -> Float = { 1f },
        point: (Double) -> DoubleArray): List<LongArray> {
        val hz = if (stylus) 200 else 240
        val samples = max(1, (durationMs * hz / 1000 / count).roundToInt())
        val start = System.nanoTime()
        var index = 0
        return List(count) { contact ->
            var down = 0L
            var begin = 0L
            for (sample in 0..samples) {
                val delay = start + (index++ * 1e9 / hz).toLong() - System.nanoTime()
                if (delay > 0) LockSupport.parkNanos(delay)
                val progress = (contact + sample.toDouble() / samples) / count
                if (sample == 0) { down = SystemClock.uptimeMillis(); begin = now() }
                val action = if (sample == 0) MotionEvent.ACTION_DOWN else if (sample == samples) MotionEvent.ACTION_UP else MotionEvent.ACTION_MOVE
                inject(action, down, listOf(point(progress)), stylus, if (sample == samples) 0f else pressure(progress))
            }
            longArrayOf(begin, now())
        }
    }
    private fun twoFingers(durationMs: Double, flickMs: Double, points: (Double, Double) -> List<DoubleArray>): LongArray {
        val samples = max(1, (durationMs * .24).roundToInt())
        val flick = (flickMs * .24).roundToInt()
        fun at(i: Int) = points(min(1.0, i.toDouble() / samples), if (flick == 0) 0.0 else ((i - samples).toDouble() / flick).coerceIn(0.0, 1.0))
        val second = 1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT
        val down = SystemClock.uptimeMillis()
        val begin = now()
        val start = System.nanoTime()
        inject(MotionEvent.ACTION_DOWN, down, at(0).take(1), false, 1f)
        inject(MotionEvent.ACTION_POINTER_DOWN or second, down, at(0), false, 1f)
        for (i in 1..samples + flick) {
            val delay = start + (i * 1e9 / 240).toLong() - System.nanoTime()
            if (delay > 0) LockSupport.parkNanos(delay)
            inject(MotionEvent.ACTION_MOVE, down, at(i), false, 1f)
        }
        val last = at(samples + flick)
        inject(MotionEvent.ACTION_POINTER_UP or second, down, last, false, 1f)
        inject(MotionEvent.ACTION_UP, down, last.take(1), false, 1f)
        return longArrayOf(begin, now())
    }
    private fun perFrame(frames: Int, action: (Int) -> JSONObject): LongArray {
        val done = CountDownLatch(1)
        var begin = 0L
        var index = 0
        val callback = object : Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                if (index == 0) begin = now()
                host.dispatch(action(index++))
                if (index < frames) Choreographer.getInstance().postFrameCallback(this) else done.countDown()
            }
        }
        runOnMainSync { Choreographer.getInstance().postFrameCallback(callback) }
        check(done.await(120, TimeUnit.SECONDS)) { "Frame-paced changes did not finish" }
        val end = now()
        native { }
        failed()?.let { error(it) }
        return longArrayOf(begin, end)
    }

    private fun root(view: View): ViewRootForTest? {
        val pending = ArrayDeque(listOf(view))
        while (pending.isNotEmpty()) {
            val next = pending.removeFirst()
            if (next is ViewRootForTest) return next
            if (next is ViewGroup) for (i in 0 until next.childCount) pending.add(next.getChildAt(i))
        }
        return null
    }
    private fun find(node: SemanticsNode, tag: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node else node.children.firstNotNullOfOrNull { find(it, tag) }
    private fun box(tag: String): RectF? = WindowInspector.getGlobalWindowViews().firstNotNullOfOrNull { view ->
        root(view)?.let { owner -> find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)?.let { owner.view to it } }
    }?.let { (view, node) ->
        val origin = IntArray(2)
        view.getLocationOnScreen(origin)
        val r = node.boundsInRoot
        RectF(r.left + origin[0], r.top + origin[1], r.right + origin[0], r.bottom + origin[1]).takeIf { !it.isEmpty }
    }
    private fun visible(tag: String) = box(tag) != null
    private fun center(tag: String) = main { box(tag) ?: throw Skipped("nothing tagged $tag is visible") }
        .let { doubleArrayOf(it.centerX().toDouble(), it.centerY().toDouble()) }
    private fun tap(point: DoubleArray, stylus: Boolean = false) = contacts(stylus, 30.0) { point }.single()[0]
    private fun lerp(a: DoubleArray, b: DoubleArray, t: Double) = doubleArrayOf(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)

    private fun launch() {
        val input = now()
        activity = startActivitySync(Intent(targetContext, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)) as MainActivity
        host = activity.host
        alive = true
        runOnMainSync { activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        val ready = await("a ready brush", 120_000) {
            host.snapshot!!.optBoolean("brush_ready") && host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true
        }
        latency("launch", input, ready, "canvas")
        awaitBenchmarkRefreshPolicy(args, File(targetContext.getExternalFilesDir(null), "bench-chain"), this, host, activity.window.decorView)
        refreshHz = main { activity.window.decorView.display.refreshRate.toDouble() }
        startWorkspace = main { host.workspaceManager?.optString("id") }
        extra().put("workspace", startWorkspace).put("tabs", main { host.drawingTabs.rows.size }).put("phone", main { phone() })
    }

    private fun open(step: JSONObject) {
        val name = step.getString("document")
        record.put("input", "dispatch")
        val reopen = step.optBoolean("reopen")
        val file = if (reopen) saved[name] ?: throw Skipped("$name was not saved in this pass") else staged.getValue(name).getOrThrow()
        if (reopen) {
            showTab(name)
            if (main { host.drawingTabs.rows.size } < 2) throw Skipped("closing the only open drawing ends the app, so $name cannot reopen")
            extra().put("composite_before", composite())
            closeActive()
            documents.remove(name)
        }
        val free = freeBytes()
        val (tabs, epoch) = main { host.drawingTabs.rows.map { it.getLong("id") }.toSet() to state().getJSONObject("document_file").getLong("epoch") }
        val notice = main { state().objectOrNull("notice")?.getLong("id") ?: 0L }
        var hostError = main { state().nullableString("host_error") }
        val input = now()
        check(main { host.documents.openUris(listOf(Uri.fromFile(file))) }) { "The host did not accept $name" }
        var refusal: String? = null
        fun declined(): String? {
            val shown = state()
            val error = shown.nullableString("host_error")
            if (error == null) hostError = null
            return shown.objectOrNull("notice")?.takeIf { it.getLong("id") > notice }?.getString("text") ?: error?.takeIf { it != hostError }
        }
        val ready = try { await("$name to open", 180_000) {
            val controller = host.documents
            val packagePrompt = controller.packagePrompt
            val profilePrompt = controller.profilePrompt
            when {
                refusal != null -> quiet()
                packagePrompt != null -> {
                    extra().put("open_prompt", packagePrompt)
                    refusal = "Opening $name showed a package dialog: " + listOf(packagePrompt.optString("status"), packagePrompt.optString("reason")).filter(String::isNotEmpty).joinToString(": ")
                    controller.choosePackageCopy(null)
                    false
                }
                profilePrompt != null -> {
                    extra().put("open_prompt", profilePrompt)
                    refusal = "Opening $name requires a color profile"
                    controller.chooseProfile(null)
                    false
                }
                else -> {
                    val selected = host.drawingTabs.selected
                    val opened = selected !in tabs && host.drawingTabs.rows.any { it.getLong("id") == selected }
                    if (!opened) refusal = declined()
                    opened && quiet() && state().getJSONObject("document_file").getLong("epoch") != epoch &&
                        host.snapshot!!.optBoolean("canvas_ready") && host.snapshot!!.optBoolean("brush_ready")
                }
            }
        } } catch (failure: IllegalStateException) {
            main { host.actionError.takeIf { host.failure == null } }?.let { throw Refused(it) }
            throw failure
        }
        refusal?.let { throw Refused(it) }
        latency("open", input, ready, "canvas")
        extra().put("disk_free_bytes", JSONArray(listOf(free, freeBytes())))
        val photo = if (!reopen && chain.getJSONObject("documents").getJSONObject(name).getString("kind") == "photo") main { editingLayer() } else null
        if (photo != null) command("add_layer")
        val (tab, paint, size) = main {
            val shown = state().array("tabs").objects().first { it.optBoolean("active") }
            Triple(host.drawingTabs.selected, editingLayer(), JSONArray(listOf(shown.getInt("width"), shown.getInt("height"))))
        }
        documents[name] = Opened(tab, paint, photo)
        active = name
        if (canvas == null) canvas = size
        extra().put("canvas", size)
        if (reopen) extra().put("composite_after", composite())
    }

    private fun editingLayer() = state().array("layers").objects().firstOrNull { it.optBoolean("editing") }?.getLong("id")
        ?: state().getJSONObject("layer_properties").getLong("layer")

    private fun selectLayer(label: String?) {
        val opened = active?.let { documents[it] }
        val id = when (label) {
            null -> opened?.paint ?: return
            "Photo" -> opened?.photo
            else -> null
        } ?: main { state().array("layers").objects().firstOrNull { it.optString("label") == label }?.getLong("id") }
            ?: throw Skipped("no layer labelled $label")
        dispatch(obj("type" to "select_layer", "id" to id))
    }

    private fun stroke(step: JSONObject) {
        val preset = step.getInt("preset")
        if (main { host.catalog.array("brush_categories").objects().none { c -> c.array("brushes").objects().any { it.getInt("id") == preset } } })
            throw Skipped("brush preset $preset is not in this build")
        selectLayer(step.optString("layer").ifEmpty { null })
        dispatch(obj("type" to "select_brush", "id" to preset))
        await("brush $preset") { host.snapshot!!.optBoolean("brush_ready") }
        val tool = main { state().getJSONObject("brush").optString("tool") }
        if (tool in setOf("clone", "heal", "spot_heal")) command("use_reference_below")
        val pixels = activeSize().let { (width, height) -> width * height }
        val size = step.get("size").let { if (it == "guarantee") (if (pixels < 20_000_000) 1024.0 else if (pixels < 40_000_000) 1536.0 else 2048.0) else (it as Number).toDouble() }
        dispatch(obj("type" to "set_brush_size", "value" to size))
        place(step)
        await("brush ready") { host.snapshot!!.optBoolean("brush_ready") }
        extra().put("size", main { state().getJSONObject("brush").optDouble("diameter") })
            .put("zoom", main { state().getJSONObject("camera").getDouble("zoom") })
        val a = area()
        if (tool in setOf("clone", "heal")) {
            command("clone_source_arm")
            tap(a.at(.25, .5), stylus = true)
            await("clone source") {
                state().array("commands").objects().none { it.getString("id") == "clone_source_arm" && it.optBoolean("selected") }
            }
        }
        val primed = main { revision() }
        val primeTravel = if (tool == "liquify") .025 else 0.0
        contacts(true, 100.0) { a.at(.5 + primeTravel * it, .5) }
        await("priming dab") { revision() > primed }
        val duration = motionMs(step)
        val framesBefore = native {
            val stats = JSONObject(Native.query(it, obj("type" to "renderer_stats").toString()))
            if (stats.has("frames")) stats.getLong("frames")
            else stats.array("rows").objects().first { it.getString("label") == "Frames" }.getString("value").toLong()
        }
        report(true)
        native { Native.completionTimings(it, true) }
        val zigzag = step.getString("path") == "zigzag"
        val windows = contacts(true, duration, step.optInt("contacts", 1), { (.625 - .375 * cos(2 * PI * it * duration / 1000)).toFloat() }) { p ->
            val t = p * duration / 1000
            if (zigzag) a.at(.5 + .3 * (1 - 4 * abs(t % 1 - .5)), .5 + .2 * (2 * p - 1)) else a.at(.5 + .3 * cos(2 * PI * t), .5 + .2 * sin(2 * PI * t))
        }
        val cap = SystemClock.uptimeMillis() + step.optLong("catch_up_cap_ms", 3000)
        var settled: Long? = null
        while (settled == null && SystemClock.uptimeMillis() < cap) {
            failed()?.let { error(it) }
            if (!native { Native.renderingPending(it) }) settled = now() else LockSupport.parkNanos(2_000_000)
        }
        record.put("settled_ns", settled ?: JSONObject.NULL)
        if (settled == null) extra().put("catch_up_capped", true)
        await("stroke catch-up", 60_000)
        val measured = report(false)
        val rows = JSONArray(native { Native.completionTimings(it, false) })
        val offset = now() - System.nanoTime()
        val inputs = measured.getJSONArray("inputs")
        val processed = (0 until inputs.length()).map { inputs.getJSONArray(it) }.filter { it.getInt(5) in 1..2 }.map { it.getLong(0) }.sorted()
        var raster = framesBefore
        var consumed = -1
        for (row in (0 until rows.length()).map { rows.getJSONArray(it) }) {
            if (row.getLong(3) <= raster) continue
            raster = row.getLong(3)
            val index = processed.binarySearch(row.getLong(4)).let { if (it >= 0) it else -it - 2 }
            if (index <= consumed) continue
            consumed = index
            record.getJSONArray("completions").put(obj("complete_ns" to row.getLong(2) + offset, "input_ns" to processed[index] + offset))
        }
        for (window in windows) {
            motion(window)
            record.getJSONArray("contacts").put(obj("down_ns" to window[0], "up_ns" to window[1]))
        }
        extra().put("measurements", measured.put("completions", rows).put("renderer_frames_before", framesBefore).put("monotonic_to_boot_ns", offset))
    }

    private fun gesture(step: JSONObject) {
        val kind = step.getString("gesture")
        if (kind !in setOf("pan", "pinch", "rotate")) throw Skipped("unknown gesture $kind")
        val quick = step.optString("release") == "quick"
        val scale = step.optDouble("scale", 2.5)
        place(step)
        val a = area()
        val radius = .12 * min(a.w, a.h)
        val before = main { state().getJSONObject("camera").getLong("revision") }
        motion(twoFingers(motionMs(step), if (quick) 100.0 else 0.0) { s, flick ->
            val wave = sin(PI * s)
            val c = a.at(.5 + if (kind == "pan") .3 * wave else 0.0, .5)
            val spread = radius * (if (kind == "pinch") 1 + (scale - 1) * wave else 1.0) * (1 + .6 * flick)
            val angle = if (kind == "rotate") PI / 3 * wave else 0.0
            listOf(-1.0, 1.0).map { d -> doubleArrayOf(c[0] + d * spread * cos(angle), c[1] + d * spread * sin(angle)) }
        })
        if (quick) {
            val deadline = SystemClock.uptimeMillis() + 30_000
            var previous: String? = null
            while (true) {
                val camera = main { state().getJSONObject("camera").toString() }
                if (camera == previous && !native { Native.renderingPending(it) }) break
                check(SystemClock.uptimeMillis() < deadline) { "The camera did not stop" }
                previous = camera
                LockSupport.parkNanos(4_000_000)
            }
            record.put("settled_ns", now())
        }
        await("gesture to settle")
        check(main { state().getJSONObject("camera").getLong("revision") } != before) { "The $kind gesture did not move the camera" }
    }

    private fun fillPoint(point: DoubleArray, area: Area): DoubleArray = main {
        val camera = state().getJSONObject("camera")
        val drawing = state().array("tabs").objects().first { it.optBoolean("active") }
        val halfWidth = drawing.getInt("width") / 2.0
        val halfHeight = drawing.getInt("height") / 2.0
        val zoom = camera.getDouble("zoom")
        val flipped = camera.getJSONArray("flipped")
        val x = halfWidth * (if (flipped.getBoolean(0)) -zoom else zoom)
        val y = halfHeight * (if (flipped.getBoolean(1)) -zoom else zoom)
        val rotation = camera.getDouble("rotation")
        val cos = cos(rotation)
        val sin = sin(rotation)
        val translation = camera.getJSONArray("translation")
        val centerX = host.surfaceOrigin.x + translation.getDouble(0) + cos * x - sin * y
        val centerY = host.surfaceOrigin.y + translation.getDouble(1) + sin * x + cos * y
        val dx = point[0] - centerX
        val dy = point[1] - centerY
        val reach = max(abs((cos * dx + sin * dy) / x), abs((cos * dy - sin * dx) / y))
        if (reach < 1.0) return@main point
        val scale = .9 / reach
        val projected = doubleArrayOf(centerX + dx * scale, centerY + dy * scale)
        check(projected[0] in area.x..(area.x + area.w) && projected[1] in area.y..(area.y + area.h)) {
            "Fill point does not intersect the visible drawing"
        }
        projected
    }

    private fun tap(step: JSONObject) {
        val tool = step.getString("tool")
        val document = tool == "fill"
        if (document && commandEnabled("deselect") == true) command("deselect")
        place(step)
        command(tool)
        val surface = step.optString("surface", "canvas")
        val id = step.getString("id")
        for ((index, point) in step.getJSONArray("points").values().map { it as JSONArray }.withIndex()) {
            await("idle before tap")
            progress("BENCH_TAP $id ${index + 1}")
            val before = main { if (document) revision() else state().getLong("revision") }
            val a = area()
            val target = a.at(point.getDouble(0), point.getDouble(1))
            val input = tap(if (document) fillPoint(target, a) else target, true)
            val ready = await("$tool tap") { (if (document) revision() else state().getLong("revision")) > before }
            latency(tool, input, ready, surface)
        }
    }

    private fun drag(step: JSONObject) {
        val target = step.getString("target")
        val duration = motionMs(step)
        if (target == "canvas" || target.startsWith("object:")) canvasDrag(step, target, duration)
        else {
            record.put("surface", "window")
            val phone = main { phone() }
            when {
                phone && (target.startsWith("grip:") || target.startsWith("edge:")) -> drawerDrag(duration)
                target.startsWith("grip:") -> panelDrag(target.removePrefix("grip:"), duration)
                target.startsWith("edge:") -> edgeDrag(target.removePrefix("edge:").removeSuffix("-column"), duration)
                target == "slider:size" -> sliderDrag(duration)
                target.startsWith("list:") -> fling(target.removePrefix("list:"), duration)
                else -> throw Skipped("unknown drag target $target")
            }
        }
    }

    private fun canvasDrag(step: JSONObject, target: String, duration: Double) {
        if (step.optString("select") == "all") command("select_all")
        step.optString("command").ifEmpty { null }?.let { command(it) }
        step.optString("tool").ifEmpty { null }?.let { command(it) }
        if (target.startsWith("object:")) selectLayer(target.removePrefix("object:"))
        await("drag target")
        val a = area()
        val start = if (target == "canvas") a.at(.5, .5) else main {
            val camera = state().getJSONObject("camera")
            val shown = state().array("tabs").objects().first { it.optBoolean("active") }
            val t = camera.getJSONArray("translation")
            val zoom = camera.getDouble("zoom")
            doubleArrayOf(t.getDouble(0) + zoom * shown.getInt("width") / 2 + host.surfaceOrigin.x, t.getDouble(1) + zoom * shown.getInt("height") / 2 + host.surfaceOrigin.y)
        }
        val radius = .15 * min(a.w, a.h)
        val loop = step.getString("path") == "loop"
        motion(contacts(true, duration) { p ->
            if (loop) a.at(.5 + .25 * sin(2 * PI * p), .5 - .25 * cos(2 * PI * p))
            else doubleArrayOf(start[0] + radius * (1 - cos(PI * p)), start[1] - radius * sin(PI * p))
        }.single())
        if (step.optString("then") == "apply") {
            await("drag to settle")
            val input = now()
            command("apply")
            latency("apply", input, await("apply"), "canvas")
        }
    }

    private fun panelGripTags(panel: String): List<String> {
        val layout = host.snapshot!!.getJSONObject("layout")
        val group = layout.array("groups").objects().firstOrNull { panel in it.array("panels").values() }?.getInt("id")
        val column = layout.array("collapsed").objects().firstOrNull { column ->
            column.array("groups").objects().any { group ->
                group.array("icons").objects().any { it.getString("panel") == panel }
            }
        }?.getInt("id")
        return listOfNotNull(group?.let { "group-grip-$it" }, column?.let { "column-grip-$it" })
    }

    private fun showDockedPanel(panel: String, visible: Boolean) =
        dispatch(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to visible)))

    private fun workspaceLayout() = state().getJSONObject("workspace").getJSONObject("layout")

    private fun panelPlacement() = workspaceLayout().let { "${it.opt("bands")}${it.opt("floating")}${it.opt("collapsed")}" }

    private fun rightDockBands() = workspaceLayout().array("bands").objects().filter { it.getString("edge") == "right" }

    private fun rightDockDrop(group: Int, previousBand: Int?, start: DoubleArray, ready: (DoubleArray) -> Unit) {
        val workspace = box("workspace") ?: return
        val divider = previousBand?.let { box("divider-$it") }
        val density = activity.resources.displayMetrics.density.toDouble()
        val candidates = if (divider == null) {
            listOf(start[1], workspace.centerY().toDouble()).map { y ->
                doubleArrayOf(workspace.right - 4 * density, y)
            }
        } else {
            listOf(44, 56, 68).map { distance ->
                doubleArrayOf(divider.left - distance * density, divider.centerY().toDouble())
            }
        }
        val item = obj("kind" to "group", "group" to group)
        val points = candidates + listOf(start)
        fun find(index: Int) {
            val point = points.getOrNull(index) ?: return
            val position = JSONArray(listOf((point[0] - workspace.left) / density, (point[1] - workspace.top) / density))
            host.query(obj("type" to "drop", "item" to item, "position" to position, "tabs" to JSONArray())) { value ->
                val target = (value as? JSONObject)?.optJSONObject("target")
                val matches = if (previousBand == null) {
                    target?.optString("kind") == "edge" && target.optString("edge") == "right" && target.optBoolean("outer")
                } else {
                    target?.optString("kind") == "beside_band" && target.optInt("band") == previousBand
                }
                if (matches) ready(point) else find(index + 1)
            }
        }
        find(0)
    }

    private fun panelDrag(panel: String, duration: Double) {
        val temporary = main { panelGripTags(panel).isEmpty() }
        try {
            if (temporary) showDockedPanel(panel, true)
            await("visible $panel grip", 5_000) { panelGripTags(panel).any { visible(it) } }
            val (tag, before) = main { panelGripTags(panel).first { visible(it) } to panelPlacement() }
            val start = center(tag)
            val group = tag.removePrefix("group-grip-").toIntOrNull()
            val bands = main { rightDockBands() }
            val slot = bands.indexOfFirst { it.getJSONObject("root").getInt("id") == group }
            val previousBand = bands.getOrNull(slot - 1)?.getInt("id")
            val dockDrop = AtomicReference<DoubleArray>()
            var requestedDock = slot < 0
            var returning = false
            var drop: DoubleArray? = start.takeIf { slot < 0 }
            val a = area()
            val float = a.at(.5, .5)
            val radius = .1 * min(a.w, a.h)
            motion(contacts(false, duration) { p ->
                if (p >= .4 && !requestedDock) {
                    activity.runOnUiThread { rightDockDrop(requireNotNull(group), previousBand, start, dockDrop::set) }
                    requestedDock = true
                }
                when {
                    p < .25 -> lerp(start, float, p / .25)
                    p < .75 -> (2 * PI * (p - .25) / .5).let { q -> doubleArrayOf(float[0] + radius * sin(q), float[1] - radius * (1 - cos(q))) }
                    else -> {
                        if (!returning) {
                            if (slot >= 0) drop = dockDrop.get()
                            returning = true
                        }
                        lerp(float, drop ?: start, (p - .75) / .25)
                    }
                }
            }.single())
            await("panel drop")
            val restored = main {
                if (slot < 0) panelPlacement() == before
                else drop != null && rightDockBands().getOrNull(slot)?.getJSONObject("root")?.getInt("id") == group
            }
            extra().put("target", tag).put("returned_in_place", restored)
            if (!restored && !temporary) command("undo_workspace")
            check(drop != null) { "No live drop target for the starting $panel dock" }
            check(restored) { "$panel did not return to its starting dock" }
        } finally {
            if (temporary) showDockedPanel(panel, false)
        }
    }

    private fun edgeDrag(side: String, duration: Double) {
        val tag = main {
            val band = state().getJSONObject("workspace").getJSONObject("layout").array("bands").objects()
                .firstOrNull { it.optString("edge") == side }?.getInt("id") ?: throw Skipped("no $side column in this workspace")
            "divider-$band".takeIf { visible(it) } ?: throw Skipped("the $side column has no visible divider")
        }
        val start = center(tag)
        val direction = if (side == "right") -1 else 1
        motion(contacts(false, duration) { p -> doubleArrayOf(start[0] + direction * 120 * sin(PI * p), start[1]) }.single())
        extra().put("target", tag)
    }

    private fun drawerDrag(duration: Double) {
        main { host.revealPanel("layers") }
        await("layers drawer") { visible("drawer-panel-layers") }
        val start = center("drawer-panel-layers")
        motion(contacts(false, duration) { p -> doubleArrayOf(start[0], start[1] + 120 * sin(PI * p)) }.single())
        extra().put("variant", "drawer").put("target", "drawer-panel-layers")
        dispatch(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
    }

    private fun sliderDrag(duration: Double) {
        val (tag, bounds) = main {
            val tiles = mutableListOf<String>()
            fun collect(value: Any?) {
                when (value) {
                    is JSONObject -> {
                        if (value.optJSONObject("control")?.optString("kind") == "brush_size_slider" && value.has("id")) tiles += "component-slider-${value.get("id")}"
                        value.keys().forEach { collect(value.opt(it)) }
                    }
                    is JSONArray -> for (i in 0 until value.length()) collect(value.opt(i))
                }
            }
            collect(host.snapshot)
            (listOf("phone-slider-size") + tiles).firstNotNullOfOrNull { t -> box(t)?.let { t to it } }
                ?: throw Skipped("no brush size slider is visible in this workspace")
        }
        val horizontal = bounds.width() >= bounds.height()
        val x = bounds.centerX().toDouble()
        val y = bounds.centerY().toDouble()
        val before = main { state().getJSONObject("brush").optDouble("diameter") }
        motion(contacts(false, duration) { p ->
            val offset = 120 * sin(PI * p)
            if (horizontal) doubleArrayOf(x + offset, y) else doubleArrayOf(x, y - offset)
        }.single())
        await("slider release")
        extra().put("target", tag).put("size_before", before).put("size_after", main { state().getJSONObject("brush").optDouble("diameter") })
    }

    private fun fling(panel: String, duration: Double) {
        val tags = listOf("panel-body-$panel", "drawer-panel-$panel")
        if (main { tags.none { visible(it) } }) {
            main { host.revealPanel(panel) }
            await("$panel panel") { tags.any { visible(it) } }
        }
        val tag = main { tags.first { visible(it) } }
        val bounds = main { box(tag)!! }
        val x = bounds.centerX().toDouble()
        fun y(f: Double) = bounds.top + bounds.height() * f
        val begin = now()
        val start = SystemClock.uptimeMillis()
        contacts(false, 120.0) { p -> doubleArrayOf(x, y(.75 - .5 * p)) }
        SystemClock.sleep(max(0L, start + (duration / 2).toLong() - SystemClock.uptimeMillis()))
        contacts(false, 120.0) { p -> doubleArrayOf(x, y(.25 + .5 * p)) }
        SystemClock.sleep(max(0L, start + duration.toLong() - SystemClock.uptimeMillis()))
        motion(longArrayOf(begin, now()))
        extra().put("target", tag)
        if (main { phone() }) dispatch(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
    }

    private fun scrub(step: JSONObject) {
        record.put("input", "dispatch")
        val frames = step.optInt("frames", chain.optInt("motion_frames", 150))
        if (step.has("effect")) {
            val effect = step.getString("effect")
            if (effect != "curves") throw Skipped("no scrub value is defined for $effect")
            selectLayer("Photo")
            dispatch(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effect)))
            val layer = main { state().getJSONObject("layer_properties").getLong("layer") }
            extra().put("effect_layer", layer)
            try {
                motion(perFrame(frames) { i ->
                    val y = .5 + .25 * sin(2 * PI * i / frames)
                    val curve = obj("kind" to "curve", "value" to JSONArray(listOf(JSONArray(listOf(0, 0)), JSONArray(listOf(.5, y)), JSONArray(listOf(1, 1)))))
                    obj("type" to "effect", "action" to obj("op" to "gesture", "phase" to if (i == 0) "down" else if (i == frames - 1) "up" else "move",
                        "action" to obj("op" to "set", "layer" to layer, "key" to "rgb", "value" to curve)))
                })
                await("curves to settle", 60_000)
            } finally { dispatch(obj("type" to "layer", "action" to obj("op" to "delete", "id" to layer))) }
        } else {
            val ramp = step.getJSONArray("layer_opacity")
            selectLayer(step.optString("layer").ifEmpty { null })
            motion(perFrame(frames) { i ->
                obj("type" to "set_layer_opacity", "opacity" to ramp.getDouble(0) + (ramp.getDouble(1) - ramp.getDouble(0)) * i / max(1, frames - 1))
            })
            await("opacity to settle", 60_000)
        }
    }

    private fun repeatItems(step: JSONObject) {
        val surface = step.optString("surface", "canvas")
        val items: List<() -> Unit> = when {
            step.has("commands") -> {
                record.put("input", "dispatch")
                step.getJSONArray("commands").values().map { it as String }.map { name ->
                    val item: () -> Unit = {
                        val input = now()
                        command(name)
                        latency(name, input, await(name), surface)
                    }
                    item
                }
            }
            step.has("open") -> step.getJSONArray("open").values().map { openItem(it as String, surface) }
            step.has("workspaces") -> {
                record.put("input", "dispatch")
                step.getJSONArray("workspaces").values().map { it as String }.map { name ->
                    val id = if (name == "start") startWorkspace ?: throw Skipped("the start workspace is unknown") else workspaceIds[name] ?: "builtin:workspace:$name"
                    val item: () -> Unit = {
                        val input = now()
                        main { host.workspaceInput(obj("type" to "switch", "id" to id)) }
                        latency(name, input, await("workspace $name", 120_000) {
                            host.workspaceManager?.let { it.optString("id") == id && !it.optBoolean("busy") && !it.optBoolean("loading") } == true &&
                                host.snapshot!!.optBoolean("canvas_ready")
                        }, surface)
                    }
                    item
                }
            }
            step.has("tabs") -> step.getJSONArray("tabs").values().map { it as String }.map { name ->
                val opened = documents[name] ?: throw Skipped("$name is not open")
                val item: () -> Unit = {
                    val tag = "drawing-tab-${opened.tab}"
                    val input = if (main { visible(tag) }) tap(center(tag)) else {
                        record.put("input", "dispatch")
                        now().also { main { host.drawingTabs.select(opened.tab) } }
                    }
                    latency("tab:$name", input, await("tab $name", 120_000) {
                        host.drawingTabs.selected == opened.tab && quiet() && host.snapshot!!.optBoolean("canvas_ready")
                    }, surface)
                    active = name
                }
                item
            }
            else -> throw Skipped("nothing to repeat")
        }
        repeat(step.optInt("count", 1)) { items.forEach { it() } }
    }

    private fun openItem(spec: String, surface: String): () -> Unit {
        val phone = main { phone() }
        if (spec == "menu:main") {
            val (control, menu) = if (phone) "phone-capy" to "phone-menu" else main {
                headerItems().mapNotNull { entry ->
                    when (entry.optJSONObject("item")?.optString("kind")) {
                        "menu" -> "header-control-${entry.getInt("id")}"
                        "menu_labels" -> "header-menu-labels-compact"
                        else -> null
                    }
                }.firstOrNull(::visible) ?: "application-menu-window".takeIf(::visible)
                    ?: throw Skipped("no main menu control is visible")
            } to "workspace-menu"
            return {
                val input = tap(center(control))
                latency(spec, input, await("main menu") { visible(menu) }, surface)
                sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                await("main menu to close") { !visible(menu) }
            }
        }
        val panel = if (phone) "layers" else spec.removePrefix("panel:")
        val drawer = "drawer-panel-$panel"
        val shown = { visible(drawer) || visible("panel-body-$panel") }
        return {
            val docked = main { panelGripTags(panel).isNotEmpty() }
            fun close() {
                if (phone || main { visible(drawer) }) dispatch(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
                else showDockedPanel(panel, false)
                await("$panel to close", 30_000) { !shown() }
            }
            if (main(shown)) close()
            record.put("input", "dispatch")
            val input = now()
            main { host.revealPanel(panel) }
            latency("panel:$panel", input, await("$panel to open", 30_000, shown), surface)
            close()
            if (docked && !main { panelGripTags(panel).isNotEmpty() }) showDockedPanel(panel, true)
        }
    }

    private fun headerItems() = host.snapshot!!.optJSONObject("header")?.optJSONObject("model")?.array("zones")
        ?.values()?.flatMap { (it as JSONArray).objects() }.orEmpty()

    private fun showTab(name: String) {
        val opened = documents[name] ?: throw Skipped("$name is not open")
        if (main { host.drawingTabs.selected } != opened.tab) {
            main { host.drawingTabs.select(opened.tab) }
            await("tab $name", 120_000) { host.drawingTabs.selected == opened.tab && quiet() }
            active = name
        }
    }

    private fun save(name: String) {
        record.put("input", "dispatch").put("surface", "window")
        showTab(name)
        val source = staged.getValue(name).getOrThrow()
        val target = File(source.parentFile, "$name-saved.capy").apply { delete() }
        var picked = false
        val free = freeBytes()
        val input = now()
        command("save_document")
        latency("save", input, await("save", 180_000) {
            host.documents.picker?.takeIf { it.request.getJSONObject("kind").getJSONObject("request").getString("type") == "save" }?.let {
                picked = true
                host.documents.picked(Uri.fromFile(target))
            }
            quiet() && host.documents.picker == null && !state().getJSONObject("document_file").optBoolean("modified") &&
                state().array("requests").objects().none { it.getJSONObject("kind").optString("type") == "document" }
        }, "window")
        if (main { !activity.hasWindowFocus() }) {
            check(uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)) { "Back was refused" }
            await("the window to return from the save picker", 30_000) { activity.hasWindowFocus() }
        }
        val file = if (picked) target else source
        saved[name] = file
        extra().put("saved_bytes", file.length()).put("disk_free_bytes", JSONArray(listOf(free, freeBytes())))
    }

    private fun resume() {
        check(uiAutomation.performGlobalAction(AccessibilityService.GLOBAL_ACTION_HOME)) { "Home was refused" }
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (main { activity.lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED) }) {
            check(SystemClock.uptimeMillis() < deadline) { "The app did not go to the background" }
            LockSupport.parkNanos(5_000_000)
        }
        SystemClock.sleep(1000)
        val input = now()
        shell("am start -n ${targetContext.packageName}/${MainActivity::class.java.name} -f 0x10020000")
        latency("resume", input, await("resume", 60_000) {
            activity.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED) && activity.hasWindowFocus() && host.surfaceReady &&
                host.snapshot!!.optBoolean("canvas_ready")
        }, "canvas")
    }

    private fun discardOnClose() = main {
        state().array("requests").objects().firstOrNull {
            it.getJSONObject("kind").optJSONObject("request")?.optString("type") == "confirm_close"
        }?.getInt("id")?.let { host.documents.close(it, "discard") }
    }

    private fun closeActive() {
        val count = main { host.drawingTabs.rows.size }
        command("close_document")
        discardOnClose()
        await("the drawing to close", 120_000) { host.drawingTabs.rows.size < count && quiet() }
    }

    private fun close() {
        record.put("input", "dispatch").put("surface", "window")
        while (alive) {
            val count = main { host.drawingTabs.rows.size }
            if (count == 0) break
            val input = now()
            command("close_document")
            discardOnClose()
            val deadline = SystemClock.uptimeMillis() + 120_000
            while (main { !activity.isFinishing && host.drawingTabs.rows.size >= count }) {
                failed()?.let { error(it) }
                check(SystemClock.uptimeMillis() < deadline) { "The drawing did not close" }
                LockSupport.parkNanos(2_000_000)
            }
            if (main { activity.isFinishing || host.drawingTabs.rows.isEmpty() }) {
                latency("close", input, now(), "window")
                alive = false
                while (main { !activity.isDestroyed }) {
                    check(SystemClock.uptimeMillis() < deadline) { "The window did not close" }
                    LockSupport.parkNanos(10_000_000)
                }
            } else latency("close", input, await("close", 120_000) { quiet() }, "window")
        }
    }
}
