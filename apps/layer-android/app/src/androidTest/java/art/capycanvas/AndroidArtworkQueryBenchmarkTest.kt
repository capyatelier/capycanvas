package art.capycanvas

import android.os.SystemClock
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

internal fun CanvasHost.openQueryPhoto(file: File) {
    val source = if (file.path.startsWith("/data/local/tmp/")) {
        check(file.path.matches(Regex("/data/local/tmp/[a-zA-Z0-9_.-]+[.]jpg")))
        File(instrumentation.targetContext.cacheDir, "query-${file.name}").also { local ->
            android.os.ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand("cat ${file.path}")).use { input ->
                local.outputStream().use { input.copyTo(it) }
            }
        }
    } else file
    val task = runBlocking { withNative { h ->
        val (id, state) = documentRequest(h, "open_document")
        Native.projectTask(h, id, "null", state.getLong("epoch"), state.getLong("revision"))
    } }
    try {
        Native.projectWork(task, android.os.ParcelFileDescriptor.open(source, android.os.ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
        runBlocking { withNative { Native.projectAdopt(it, task, "null") } }
    } finally { Native.projectFree(task) }
    instrumentation.runOnMainSync { documentChanged() }
}

class AndroidArtworkQueryBenchmarkTest {
    @get:Rule val device = CapyDeviceRule(nativeFileJobs = true)
    @Test fun artworkStatistics() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("artworkStatisticsBenchmark") == "true")
        val mode = args.getString("statisticsMode", "preview")!!
        val waveform = args.getString("statisticsWaveform", "false") == "true"
        val uniform = args.getString("statisticsUniform", "false") == "true"
        check(mode in listOf("preview", "exact", "auto"))
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(test: () -> Boolean) {
                val deadline = SystemClock.uptimeMillis() + 180_000
                while (!test()) { assertNull(host.failure); check(SystemClock.uptimeMillis() < deadline) { "Statistics fixture timeout: ${host.actionError}" }; SystemClock.sleep(20) }
            }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true && host.workspaceManager?.optBoolean("ready") == true }
            host.openQueryPhoto(File(args.getString("photo", if (uniform) "/data/local/tmp/capycanvas4-waveform-white-61mp.jpg" else "/data/local/tmp/capy-brush-photo.jpg")!!))
            scenario.onActivity { host.invoke("fit_canvas") }
            waitFor { host.snapshot?.getJSONObject("state")?.array("tabs")?.objects()?.any { it.optInt("width") == 9504 } == true && host.snapshot?.optBoolean("shaders_ready") == true }
            waitFor { !native { Native.renderingPending(it) } }
            if (mode == "auto") {
                host.drain(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "levels")))
                waitFor { !native { Native.renderingPending(it) } }
            }
            val source = if (mode == "auto") obj("EffectChannels" to host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").getLong("layer")).toString() else "\"Visible\""
            fun query(job: Long) = if (mode == "auto") Native.inspectionLevelsStatistics(job, source) else Native.inspectionStatistics(job, source, mode == "preview", false, waveform)
            val output = File(activity.getExternalFilesDir(null), "artwork-statistics-benchmark").apply { mkdirs() }
            File(output, "info.json").writeText(obj("state" to host.snapshot!!.getJSONObject("state"),
                "display" to native { JSONObject(Native.displayStatus(it)) }, "waveform" to waveform, "fixture" to if (uniform) "uniform-white" else "reference-photo").toString(2))
            val rows = JSONArray()
            val memory = JSONArray()
            fun observe(phase: String) {
                val available = File("/proc/meminfo").useLines { lines -> lines.first { it.startsWith("MemAvailable:") }.split(Regex("\\s+")).get(1).toLong() * 1024 }
                memory.put(obj("phase" to phase, "allocator" to native { JSONObject(Native.rendererMemory(it)) },
                    "pss_bytes" to android.os.Debug.getPss().toLong() * 1024, "system_mem_available_bytes" to available,
                    "observed_boot_ns" to SystemClock.elapsedRealtimeNanos()))
            }
            observe("before")
            val preview = mode == "preview"
            repeat(args.getString("statisticsRepeats", "2")!!.toInt()) { run ->
                val captureBegin = System.nanoTime()
                val job = native { Native.inspectionTask(it, 0) }
                val captureEnd = System.nanoTime()
                val begin = System.nanoTime()
                val result = JSONObject(query(job))
                val duration = (System.nanoTime() - begin) / 1e6
                val histogram = result.getJSONObject(if (mode == "auto") "statistics" else "histogram")
                val pixels = histogram.getLong("pixels")
                assertEquals(if (preview) 65536L else 9504L * 6336L, pixels)
                if (mode == "auto") {
                    val channels = histogram.getJSONArray("bins")
                    for (channel in 0 until channels.length()) {
                        val bins = channels.getJSONArray(channel)
                        assertEquals(4096, bins.length())
                        assertEquals(pixels, (0 until bins.length()).sumOf { bins.getLong(it) })
                        assertTrue(histogram.getJSONArray("maximum").getDouble(channel) >= histogram.getJSONArray("minimum").getDouble(channel))
                    }
                } else {
                    assertEquals(0L, histogram.getLong("transparent"))
                    histogram.getJSONArray("channels").objects().forEach { channel ->
                        val bins = channel.getJSONArray("bins")
                        assertEquals(pixels, (0 until bins.length()).sumOf { bins.getLong(it) })
                    }
                }
                rows.put(result.put("mode", mode).put("waveform", waveform).put("uniform", uniform).put("run", run).put("duration_ms", duration)
                    .put("capture_ms", (captureEnd - captureBegin) / 1e6))
                File(output, "$mode-samples.json").writeText(rows.toString(2))
                observe("after-$mode-$run")
                InstrumentationRegistry.getInstrumentation().sendStatus(0, android.os.Bundle().apply {
                    putString("artwork_statistics", obj("mode" to mode, "run" to run, "duration_ms" to duration,
                        "capture_ms" to (captureEnd - captureBegin) / 1e6, "pixels" to pixels).toString())
                })
            }
            if (args.getString("statisticsAllocation") == "true") {
                val tracked = Native.captureControl()
                try {
                    val job = native { Native.inspectionTask(it, tracked) }
                    val begin = System.nanoTime()
                    val result = JSONObject(query(job))
                    check(!result.isNull("allocation_peaks")) { "Allocation observations require an instrumented capture control" }
                    result.put("duration_ms", (System.nanoTime() - begin) / 1e6)
                    File(output, "$mode-allocation-observations.json").writeText(result.toString(2))
                    assertEquals(if (preview) 65536L else 9504L * 6336L, result.getJSONObject(if (mode == "auto") "statistics" else "histogram").getLong("pixels"))
                } finally { Native.captureFree(tracked) }
            }
            repeat(args.getString("statisticsLifecycleRepeats", "0")!!.toInt()) { cycle ->
                val job = native { Native.inspectionTask(it, 0) }
                query(job)
                observe("after-reopened-query-$cycle")
                SystemClock.sleep(250)
                observe("after-result-released-$cycle")
            }
            val pool = Executors.newSingleThreadExecutor()
            val control = Native.captureControl()
            try {
                val job = native { Native.inspectionTask(it, control) }
                val started = java.util.concurrent.CountDownLatch(1)
                val future = pool.submit<String> {
                    started.countDown()
                    try { query(job); "completed" }
                    catch (failure: Throwable) { failure.message ?: failure.javaClass.name }
                }
                assertTrue(started.await(10, TimeUnit.SECONDS))
                SystemClock.sleep(args.getString("statisticsCancelMs", "100")!!.toLong())
                val cancelAt = System.nanoTime()
                Native.captureCancel(control)
                val outcome = future.get(120, TimeUnit.SECONDS)
                val returned = System.nanoTime()
                File(output, "$mode-cancel.json").writeText(obj("cancel_to_return_ms" to (returned - cancelAt) / 1e6,
                    "outcome" to outcome).toString(2))
                assertTrue(outcome.contains("cancelled", ignoreCase = true))
            } finally { Native.captureFree(control); pool.shutdownNow() }
            observe("after-cancel-$mode")
            SystemClock.sleep(250)
            observe("settled-after-cancel-$mode")
            File(output, "$mode-allocator-boundaries.json").writeText(memory.toString(2))
            assertNull(host.failure)
            assertNull(host.actionError)
        }
    }
    @Test fun exactSamples() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue(args.getString("artworkQueryBenchmark") == "true")
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; it.window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun waitFor(test: () -> Boolean) {
                val deadline = SystemClock.uptimeMillis() + 180_000
                while (!test()) { assertNull(host.failure); check(SystemClock.uptimeMillis() < deadline) { "Query fixture timeout: ${host.actionError}" }; SystemClock.sleep(20) }
            }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true && host.workspaceManager?.optBoolean("ready") == true }
            host.openQueryPhoto(File(args.getString("photo", "/data/local/tmp/capy-brush-photo.jpg")!!))
            scenario.onActivity { host.invoke("fit_canvas") }
            waitFor { host.snapshot?.getJSONObject("state")?.array("tabs")?.objects()?.any { it.optInt("width") == 9504 } == true && host.snapshot?.optBoolean("shaders_ready") == true }
            waitFor { !native { Native.renderingPending(it) } }
            val output = File(activity.getExternalFilesDir(null), "artwork-query-benchmark").apply { mkdirs() }
            File(output, "info.json").writeText(obj("state" to host.snapshot!!.getJSONObject("state"),
                "display" to native { JSONObject(Native.displayStatus(it)) },
                "renderer" to native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }).toString(2))
            val rows = JSONArray()
            val memory = JSONArray()
            fun observeMemory(phase: String) { memory.put(obj("phase" to phase, "allocator" to native { JSONObject(Native.rendererMemory(it)) })) }
            observeMemory("before")
            fun task(control: Long = 0) = native { Native.inspectionTask(it, control) }
            fun sample(job: Long, width: Int, source: String = "\"Visible\""): JSONObject {
                val begin = System.nanoTime()
                val result = JSONObject(Native.inspectionSample(job, source, 4752f, 3168f, width))
                result.put("duration_ms", (System.nanoTime() - begin) / 1e6).put("width", width).put("source", source)
                assertTrue(result.getJSONObject("sample").has("Color"))
                return result
            }
            rows.put(sample(task(), 101).put("phase", "cold"))
            for (width in listOf(1, 5, 15, 51, 101)) repeat(10) { rows.put(sample(task(), width).put("phase", "warm")) }
            observeMemory("after_warm")
            val photo = host.snapshot!!.getJSONObject("state").getJSONArray("layers").objects().first { it.getBoolean("selected") }.getLong("id")
            repeat(10) { rows.put(sample(task(), 101, obj("LayerContent" to photo).toString()).put("phase", "warm_layer")) }
            val original = sample(task(), 101)
            val retained = task()
            val control = Native.captureControl()
            val cancelledTask = task(control)
            host.newDocument(64, 64)
            waitFor { !native { Native.renderingPending(it) } }
            val old = sample(retained, 101).put("phase", "retained_after_replace")
            assertEquals(original.getJSONObject("sample").toString(), old.getJSONObject("sample").toString())
            rows.put(old)
            observeMemory("after_replace_and_retained_query")
            val pool = Executors.newSingleThreadExecutor()
            try {
                val began = System.nanoTime()
                val started = java.util.concurrent.CountDownLatch(1)
                val cancelled = pool.submit<String> {
                    started.countDown()
                    try { Native.inspectionSample(cancelledTask, "\"Visible\"", 4752f, 3168f, 101); "completed" }
                    catch (failure: Throwable) { failure.message ?: failure.javaClass.name }
                }
                assertTrue(started.await(10, TimeUnit.SECONDS))
                SystemClock.sleep(2)
                val cancelAt = System.nanoTime()
                Native.captureCancel(control)
                val outcome = cancelled.get(120, TimeUnit.SECONDS)
                rows.put(obj("phase" to "cancel", "launch_to_cancel_ms" to (cancelAt - began) / 1e6,
                    "cancel_to_return_ms" to (System.nanoTime() - cancelAt) / 1e6, "outcome" to outcome))
            } finally { Native.captureFree(control); pool.shutdownNow() }
            File(output, "samples.json").writeText(rows.toString(2))
            observeMemory("after_cancel")
            File(output, "allocator-boundaries.json").writeText(memory.toString(2))
            println("ARTWORK_QUERY samples=${rows.length()}")
            assertNull(host.failure)
            assertNull(host.actionError)
        }
    }
}
