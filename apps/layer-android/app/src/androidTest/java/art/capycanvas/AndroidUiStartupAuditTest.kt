package art.capycanvas

import android.os.Debug
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.Window
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Opt-in physical-device audit. Run in a separate application ID to keep the
 * user's settings/documents intact. Frame callbacks use the real display clock. */
class AndroidUiStartupAuditTest {
    @Test fun measureUiStartupAndSettings() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("uiStartupAudit") == "true")
        val directory = File(instrumentation.targetContext.getExternalFilesDir(null), "ui-startup-audit").apply { mkdirs() }
        val label = args.getString("auditLabel") ?: "run"
        val sampling = args.getString("auditSampling") == "true"
        val previousWorkspaceDirectory = CanvasHost.workspaceDirectoryForTest
        CanvasHost.workspaceDirectoryForTest = File(instrumentation.targetContext.filesDir, "ui-audit-workspace").absolutePath
        val frames = mutableListOf<JSONObject>()
        val callbacks = mutableListOf<Long>()
        val running = AtomicBoolean(true)
        val metricsThread = HandlerThread("ui-audit-metrics").apply { start() }
        lateinit var choreographer: Choreographer
        val callback = object : Choreographer.FrameCallback {
            override fun doFrame(frameTimeNanos: Long) {
                synchronized(callbacks) { callbacks.add(frameTimeNanos) }
                if (running.get()) choreographer.postFrameCallback(this)
            }
        }
        instrumentation.runOnMainSync { choreographer = Choreographer.getInstance(); choreographer.postFrameCallback(callback) }
        val began = SystemClock.elapsedRealtimeNanos()
        var samplingStarted = false
        var scenario: ActivityScenario<MainActivity>? = null
        var observedWindow: Window? = null
        val listener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
            synchronized(frames) { frames.add(obj("vsync_ns" to metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP),
                "total_ns" to metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                "layout_ns" to metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION),
                "draw_ns" to metrics.getMetric(FrameMetrics.DRAW_DURATION),
                "sync_ns" to metrics.getMetric(FrameMetrics.SYNC_DURATION),
                "gpu_ns" to metrics.getMetric(FrameMetrics.GPU_DURATION),
                "deadline_ns" to metrics.getMetric(FrameMetrics.DEADLINE), "dropped_reports" to dropped)) }
        }
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            val active = scenario
            lateinit var host: CanvasHost
            active.onActivity { host = it.host; observedWindow = it.window; it.window.addOnFrameMetricsAvailableListener(listener, Handler(metricsThread.looper)) }
            fun waitFor(condition: () -> Boolean): Long {
                val deadline = SystemClock.uptimeMillis() + 60_000
                do {
                    var ready = false
                    instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
                    if (ready) return SystemClock.elapsedRealtimeNanos()
                    SystemClock.sleep(10)
                } while (SystemClock.uptimeMillis() < deadline)
                throw AssertionError("UI audit timed out")
            }
            val model = waitFor { host.snapshot != null }
            val workspace = waitFor { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
            val brush = waitFor { host.snapshot?.optBoolean("brush_ready") == true }
            val complete = waitFor { host.snapshot?.optBoolean("shaders_ready") == true }
            if (sampling) {
                Debug.startMethodTracingSampling(File(directory, "$label.trace").absolutePath, 64 * 1024 * 1024, 1000)
                samplingStarted = true
            }
            val operations = JSONArray()
            repeat(4) { index ->
                SystemClock.sleep(300)
                val draw = CountDownLatch(1)
                var start = 0L
                var drawnAt = 0L
                instrumentation.runOnMainSync {
                    val decor = observedWindow!!.decorView
                    val observer = decor.viewTreeObserver
                    val drawn = object : android.view.ViewTreeObserver.OnDrawListener {
                        override fun onDraw() {
                            if (host.snapshot?.objectOrNull("preferences") != null && drawnAt == 0L) {
                                drawnAt = SystemClock.elapsedRealtimeNanos()
                                draw.countDown()
                                decor.post { if (observer.isAlive) observer.removeOnDrawListener(this) }
                            }
                        }
                    }
                    observer.addOnDrawListener(drawn)
                    start = SystemClock.elapsedRealtimeNanos()
                    host.invoke("settings")
                }
                assertTrue("Settings draw", draw.await(20, TimeUnit.SECONDS))
                operations.put(obj("index" to index, "start_ns" to start, "draw_observed_ns" to drawnAt))
                // Allow the 240 ms entrance animation to finish before closing.
                SystemClock.sleep(400)
                instrumentation.runOnMainSync { host.dispatch(obj("type" to "close_settings")) }
                waitFor { host.snapshot?.objectOrNull("preferences") == null }
            }
            if (samplingStarted) { Debug.stopMethodTracing(); samplingStarted = false }
            val reportDone = CountDownLatch(1)
            var hostReport = JSONObject()
            host.measurements { hostReport = it; reportDone.countDown() }
            assertTrue(reportDone.await(10, TimeUnit.SECONDS))
            assertTrue("The workspace drew", hostReport.getLong("ui_first_draw_boot_ns") > began)
            val report = obj("label" to label, "debug" to BuildConfig.DEBUG, "sampling" to sampling,
                "started_boot_ns" to began, "model_observed_boot_ns" to model, "workspace_observed_boot_ns" to workspace,
                "brush_observed_boot_ns" to brush, "complete_observed_boot_ns" to complete,
                "settings" to operations, "host" to hostReport,
                "frames" to synchronized(frames) { JSONArray(frames.toList()) },
                "callbacks_monotonic_ns" to synchronized(callbacks) { JSONArray(callbacks.toList()) })
            File(directory, "$label.json").writeText(report.toString(2))
            android.util.Log.i("CapyUiAudit", "saved $label; UI draw ms=${(hostReport.getLong("ui_first_draw_boot_ns")-began)/1e6}")
        } finally {
            if (samplingStarted) Debug.stopMethodTracing()
            running.set(false)
            instrumentation.runOnMainSync { choreographer.removeFrameCallback(callback); observedWindow?.removeOnFrameMetricsAvailableListener(listener) }
            scenario?.close()
            metricsThread.quitSafely()
            CanvasHost.workspaceDirectoryForTest = previousWorkspaceDirectory
        }
    }
}
