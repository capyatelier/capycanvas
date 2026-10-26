package art.capycanvas

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.view.Choreographer
import android.view.ViewTreeObserver
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import org.json.JSONObject

/** Runs inside the optimized benchmark APK. A separate JUnit APK cannot safely
 * call application/library methods that R8 has inlined or removed. This runner
 * and all its access paths are absent from the production release source set. */
class UiStartupInstrumentation : Instrumentation() {
    private lateinit var arguments: Bundle
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        this.arguments = arguments ?: Bundle()
        start()
    }
    override fun onStart() {
        if (arguments.getString("workspaceBenchmark") == "true" || arguments.getString("viewportBenchmark") == "true") {
            workspaceTiming()
            return
        }
        var activity: MainActivity? = null
        var frameObserver: Choreographer.FrameCallback? = null
        var observing = true
        val result = Bundle()
        try {
            check(arguments.getString("uiStartupAudit") == "true") { "Opt in with -e uiStartupAudit true" }
            check(!BuildConfig.DEBUG) { "Use a non-debug build" }
            val label = arguments.getString("auditLabel") ?: "run"
            AppStorage.directoryForTest = File(targetContext.filesDir, "ui-audit")
            val began = SystemClock.elapsedRealtimeNanos()
            activity = startActivitySync(Intent(targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
            val host = activity.host
            val frames = JSONArray()
            runOnMainSync {
                val choreographer = Choreographer.getInstance()
                var previous = SystemClock.elapsedRealtimeNanos()
                frameObserver = object : Choreographer.FrameCallback {
                    override fun doFrame(frameTimeNanos: Long) {
                        if (!observing) return
                        val now = SystemClock.elapsedRealtimeNanos()
                        frames.put(JSONArray().put(now).put(now - previous)
                            .put(host.snapshot?.optBoolean("shaders_ready") == true))
                        previous = now
                        choreographer.postFrameCallback(this)
                    }
                }
                choreographer.postFrameCallback(frameObserver!!)
            }
            fun waitFor(condition: () -> Boolean): Long {
                val deadline = SystemClock.uptimeMillis() + 600_000
                do {
                    var ready = false
                    runOnMainSync { check(host.failure == null) { host.failure!! }; check(host.actionError == null) { host.actionError!! }; ready = condition() }
                    if (ready) return SystemClock.elapsedRealtimeNanos()
                    SystemClock.sleep(10)
                } while (SystemClock.uptimeMillis() < deadline)
                error("UI startup audit timed out")
            }
            val workspace = waitFor { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
            fun settings(count: Int): JSONArray {
                val operations = JSONArray()
                repeat(count) {
                    SystemClock.sleep(300)
                    val draw = CountDownLatch(1)
                    var started = 0L
                    var drawn = 0L
                    runOnMainSync {
                        val decor = activity.window.decorView
                        val observer = decor.viewTreeObserver
                        val listener = object : ViewTreeObserver.OnDrawListener {
                            override fun onDraw() {
                                if (host.snapshot?.objectOrNull("preferences") != null && drawn == 0L) {
                                    drawn = SystemClock.elapsedRealtimeNanos()
                                    draw.countDown()
                                    decor.post { if (observer.isAlive) observer.removeOnDrawListener(this) }
                                }
                            }
                        }
                        observer.addOnDrawListener(listener)
                        started = SystemClock.elapsedRealtimeNanos()
                        host.invoke("settings")
                    }
                    check(draw.await(20, TimeUnit.SECONDS)) { "Settings did not draw" }
                    operations.put(obj("start_ns" to started, "draw_observed_ns" to drawn))
                    SystemClock.sleep(400)
                    runOnMainSync { host.dispatch(obj("type" to "close_settings")) }
                    waitFor { host.snapshot?.objectOrNull("preferences") == null }
                }
                return operations
            }
            val duringStartup = settings(2)
            val complete = waitFor { host.snapshot?.optBoolean("shaders_ready") == true }
            val steady = settings(4)
            runOnMainSync {
                observing = false
                frameObserver?.let { Choreographer.getInstance().removeFrameCallback(it) }
            }
            val reportDone = CountDownLatch(1)
            var measurements = JSONObject()
            host.measurements { measurements = it; reportDone.countDown() }
            check(reportDone.await(10, TimeUnit.SECONDS))
            val report = obj("label" to label, "debug" to BuildConfig.DEBUG, "started_boot_ns" to began,
                "workspace_observed_boot_ns" to workspace, "complete_observed_boot_ns" to complete,
                "settings_startup" to duringStartup, "settings" to steady, "host" to measurements,
                "ui_frames" to frames, "ui_frame_fields" to JSONArray(listOf("callback_boot_ns", "interval_ns", "shaders_ready")))
            val directory = File(targetContext.getExternalFilesDir(null), "ui-startup-audit").apply { mkdirs() }
            File(directory, "$label.json").writeText(report.toString(2))
            result.putString("stream", "\nUI startup audit saved $label.json\n")
            // Frame/input samples can exceed Binder's transaction budget.
            // The complete report is already durable; transport only its path.
            result.putString("report", File(directory, "$label.json").absolutePath)
        } catch (error: Throwable) {
            result.putString("stream", "\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
            return
        } finally {
            activity?.let { runOnMainSync {
                observing = false
                frameObserver?.let { Choreographer.getInstance().removeFrameCallback(it) }
                it.finish()
            } }
            AppStorage.directoryForTest = null
        }
        finish(Activity.RESULT_OK, result)
    }

    private fun workspaceTiming() {
        var activity: MainActivity? = null
        val result = Bundle()
        val root = File(targetContext.cacheDir, "workspace-benchmark-${System.nanoTime()}")
        val previousFileJobs = DocumentController.nativeFileJobsForTest
        AppStorage.directoryForTest = root
        DocumentController.nativeFileJobsForTest = true
        try {
            val label = checkNotNull(arguments.getString("label"))
            require(label.matches(Regex("[a-zA-Z0-9_-]+")))
            activity = startActivitySync(Intent(targetContext, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)) as MainActivity
            runOnMainSync { activity.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE }
            val host = activity.host
            val until = SystemClock.uptimeMillis() + 180_000
            do {
                var ready = false
                runOnMainSync {
                    check(host.failure == null) { host.failure!! }
                    ready = host.snapshot?.optBoolean("brush_ready") == true && host.recovery.ready &&
                        host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true &&
                        activity.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_LANDSCAPE
                }
                if (ready) break
                check(SystemClock.uptimeMillis() < until) { "Workspace benchmark startup timed out" }
                SystemClock.sleep(16)
            } while (true)
            if (arguments.getString("viewportBenchmark") == "true") {
                ViewportFrameTiming(this, arguments).run(activity)
                result.putString("stream", "\nVIEWPORT_COMPLETE $label\n")
            } else {
                val workload = arguments.getString("workspaceWorkload", "gap")
                require(workload == "gap" || workload == "panel")
                WorkspaceFrameTiming(this, arguments).run(activity, resize = workload == "gap", bridge = true)
                result.putString("stream", "\nWORKSPACE_COMPLETE $label\n")
            }
        } catch (error: Throwable) {
            result.putString("stream", "\n${error.stackTraceToString()}\n")
            finish(Activity.RESULT_CANCELED, result)
            return
        } finally {
            activity?.let { runOnMainSync { it.finish() } }
            targetContext.deleteSharedPreferences(CanvasHost.preferencesName)
            DocumentController.nativeFileJobsForTest = previousFileJobs
            AppStorage.directoryForTest = null
            root.deleteRecursively()
        }
        finish(Activity.RESULT_OK, result)
    }
}
