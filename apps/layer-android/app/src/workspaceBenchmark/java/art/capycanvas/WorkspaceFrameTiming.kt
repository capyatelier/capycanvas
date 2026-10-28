package art.capycanvas

import android.app.Instrumentation
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.Window
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sin
import kotlin.math.roundToInt

internal class WorkspaceFrameTiming(private val instrumentation: Instrumentation, private val args: Bundle) {
    fun run(activity: MainActivity, resize: Boolean, colorOverlap: Boolean = false, colorWheel: Boolean = false, editColor: Boolean = false, bridge: Boolean = false) {
        require(args.getString("workspaceBenchmark") == "true")
        val scenario = WorkspaceBenchmarkActivity(instrumentation, activity)
        val tierPhoto = args.getString("tierPhoto") == "true"
        val repeats = if (bridge || tierPhoto) args.getString("repeats", "3")!!.toInt() else 1
        val measuredDuration = if (bridge || tierPhoto) args.getString("durationMs", "6000")!!.toLong() else 5000L
        val label = args.getString("label", "column-bridge")!!
        val bridgeContent = args.getString("bridgeContent", "default")!!
        require(bridgeContent in listOf("default", "empty-navigator", "empty-both"))
        val modelIdentityTrace = bridge && args.getString("modelIdentityTrace") == "true"
        val dirtyTrace = bridge && args.getString("dirtyTrace") == "true"
        val requireSharedPresentation = args.getString("requireSharedPresentation") == "true"
        require(!requireSharedPresentation || bridge)
        val measuring = AtomicBoolean(false)
        data class CompositionDirty(val time: Long, val key: Int, val first: Int, val second: Int, val name: String)
        val compositionDirty = mutableListOf<CompositionDirty>()
        if (bridge) { assertFalse(BuildConfig.DEBUG); require((repeats >= 3 || (modelIdentityTrace || args.getString("composeTrace") == "true" || bridgeContent != "default") && repeats == 1) && measuredDuration in 5000L..10000L) }
        if (args.getString("composeTrace") == "true") @OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)
            androidx.compose.runtime.Composer.setTracer(object : androidx.compose.runtime.CompositionTracer {
                override fun isTraceInProgress() = android.os.Trace.isEnabled()
                override fun traceEventStart(key: Int, dirty1: Int, dirty2: Int, info: String) {
                    if (dirtyTrace && measuring.get() && (info.startsWith("art.capycanvas.CollapsedColumns (") || info.startsWith("art.capycanvas.ZenChrome (")))
                        compositionDirty.add(CompositionDirty(System.nanoTime(), key, dirty1, dirty2, info))
                    android.os.Trace.beginSection(info.take(120))
                }
                override fun traceEventEnd() = android.os.Trace.endSection()
            })
        run {
            lateinit var host: CanvasHost
            lateinit var owner: ViewRootForTest
            lateinit var window: Window
            scenario.onActivity { host = it.host; owner = it.window.decorView.descendant<ViewRootForTest>()!!; window = it.window }
            fun waitFor(condition: () -> Boolean) {
                val deadline = SystemClock.uptimeMillis() + 60_000
                do {
                    var ready = false
                    scenario.onActivity { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
                    if (ready) return
                    SystemClock.sleep(10)
                } while (SystemClock.uptimeMillis() < deadline)
                fail("Native workspace did not settle")
            }
            fun action(value: JSONObject) {
                val done = CountDownLatch(1)
                scenario.onActivity { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
                assertTrue(done.await(10, TimeUnit.SECONDS))
            }
            fun click(tag: String) = scenario.onActivity { assertTrue(owner.find(hasTag(tag))!!.config[SemanticsActions.OnClick].action!!.invoke()) }
            val wheel = if (editColor) "color-editor-wheel" else "color-wheel"
            fun bounds(tag: String): androidx.compose.ui.geometry.Rect {
                var result = androidx.compose.ui.geometry.Rect.Zero
                scenario.onActivity { result = owner.find(hasTag(tag))!!.boundsInRoot }
                return result
            }
            waitFor { host.snapshot?.optBoolean("shaders_ready") == true }
            waitFor { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
            if (bridge || tierPhoto) {
                action(obj("type" to "set_theme", "theme" to args.getString("theme", "light")))
                host.openDocument(java.io.File(requireNotNull(args.getString("photo"))))
                val state = host.snapshot!!.getJSONObject("state")
                assertTrue("The tier photo is active", state.array("tabs").objects().any { it.optBoolean("active") &&
                    it.optInt("width") == args.getString("width")!!.toInt() && it.optInt("height") == args.getString("height")!!.toInt() })
                val active = state.array("layers").objects().single { it.optBoolean("editing") }.getLong("id")
                state.array("layers").objects().filter { it.getLong("id") != active }.forEach {
                    action(obj("type" to "set_layer_visibility", "id" to it.getLong("id"), "visible" to false))
                }
                host.drain(obj("type" to "invoke", "command" to "add_layer"))
                host.drain(obj("type" to "invoke", "command" to "fit_canvas"))
                host.awaitMain("Tier photo and shaders ready", 180_000) { host.snapshot?.optBoolean("shaders_ready") == true }
            }
            var saved = JSONObject()
            scenario.onActivity { saved = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString()) }
            var savedTransparency = 0
            scenario.onActivity { savedTransparency = listOf("off", "low", "medium", "high").indexOf(
                host.snapshot!!.getJSONObject("state").getJSONObject("settings").getString("transparency")) }
            val transparency = args.getString("workspaceTransparency")?.toInt() ?: savedTransparency
            val fixture = JSONObject(saved.toString())
            if (bridge && bridgeContent != "default") {
                val hidden = if (bridgeContent == "empty-both") setOf("brushes", "navigator") else setOf("navigator")
                fixture.getJSONObject("layout").getJSONArray("panels").objects().filter { it.getString("id") in hidden }
                    .forEach { it.getJSONObject("content").put("visible", JSONArray()) }
            }
            fixture.getJSONObject("layout").apply {
                put("bands", JSONArray(listOf(
                    obj("id" to 40, "edge" to "left", "extent" to 252, "root" to if (colorWheel) tabs(41, "toolbar") else tabs(41, "brushes", "sizes", "tool_settings")),
                    obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties")))))
                put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
                put("next_id", maxOf(49, getInt("next_id")))
                if (colorOverlap || colorWheel) put("floating", JSONArray(listOf(obj("root" to tabs(48, "color"),
                    "position" to JSONArray(listOf(360, 120)), "width" to 360, "default_width" to 360, "height" to 400, "toolbar_layout" to "compact"))))
            }
            fixture.put("zen_mode", false)
            if (bridge) fixture.getJSONObject("layout").apply {
                fun split(id: Int, first: JSONObject, second: JSONObject) = obj("kind" to "split", "id" to id,
                    "axis" to "vertical", "fraction" to .5, "first" to first, "second" to second)
                put("bands", JSONArray(listOf(
                    obj("id" to 40, "edge" to "left", "extent" to 42, "root" to split(51, tabs(42, "color"),
                        split(41, split(45, tabs(43, "brushes", "sizes"), tabs(44, "navigator")), tabs(50, "tool_settings")))),
                    obj("id" to 60, "edge" to "right", "extent" to 252, "root" to tabs(61, "layers", "properties")))))
                put("collapsed", JSONArray(listOf(51, 42, 45, 50).map { obj("root" to it, "expanded_width" to 246) }))
                put("column_stacks", JSONArray(listOf(obj("column" to 51, "members" to JSONArray(listOf(42, 45, 50)), "drawers" to false, "auto_hide" to false))))
                put("next_id", maxOf(62, getInt("next_id")))
            }
            val output = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), if (tierPhoto && !bridge) "workspace-tab-benchmark" else "workspace-bridge-benchmark").apply { if (bridge || tierPhoto) mkdirs() }
            val presentationSamples = JSONArray()
            fun presentationStatus(phase: String, run: Int): JSONObject {
                val status = runBlocking { host.withNative { JSONObject(Native.displayStatus(it)) } }
                val sample = obj("phase" to phase, "run" to run, "boot_ns" to SystemClock.elapsedRealtimeNanos(), "status" to status)
                presentationSamples.put(sample)
                java.io.File(output, "$label-display-status.json").writeText(presentationSamples.toString())
                if (requireSharedPresentation) {
                    assertEquals("Required shared presentation capability: $sample", "SharedDemandRefresh", status.getString("ink_present_mode"))
                    if (phase == "keyframe" || phase == "during") assertEquals("Workspace uses shared presentation: $sample",
                        "SharedDemandRefresh", status.getString("present_mode"))
                }
                return sample
            }
            data class SnapshotChange(val time: Long, val identity: Int, val type: String, val valueType: String?, val elementType: String?)
            val snapshotChanges = mutableListOf<SnapshotChange>()
            val stateLabels = JSONObject()
            if (dirtyTrace) scenario.onActivity {
                host.javaClass.declaredFields.filter { it.name.contains("panelVector") }.forEach { field ->
                    field.isAccessible = true
                    field.get(host)?.let { stateLabels.put(System.identityHashCode(it).toString(), field.name) }
                }
            }
            val applyObserver = if (dirtyTrace) androidx.compose.runtime.snapshots.Snapshot.registerApplyObserver { changed, _ ->
                if (measuring.get()) {
                    val now = System.nanoTime()
                    val rows = changed.map { state ->
                        val value = (state as? androidx.compose.runtime.State<*>)?.value
                        val element = when (val collection = value ?: state) {
                            is Map<*, *> -> collection.values.firstOrNull()
                            is List<*> -> collection.firstOrNull()
                            else -> null
                        }
                        SnapshotChange(now, System.identityHashCode(state), state.javaClass.name, value?.javaClass?.name, element?.javaClass?.name)
                    }
                    synchronized(snapshotChanges) { snapshotChanges.addAll(rows) }
                }
            } else null
            data class Frame(val duration: Long, val deadline: Long, val vsync: Long, val layout: Long, val draw: Long)
            val durations = mutableListOf<Frame>()
            val drawnRevisions = mutableSetOf<Long>()
            val drawnPaints = mutableSetOf<String>()
            data class ModelDraw(val time: Long, val revision: Long?, val values: Map<String, Any?>)
            val modelDraws = mutableListOf<ModelDraw>()
            fun modelValues(): Map<String, Any?> {
                val snapshot = host.snapshot!!
                val layout = snapshot.getJSONObject("layout")
                val values = linkedMapOf<String, Any?>("layout" to layout, "panelModels" to snapshot.optJSONArray("panels"),
                    "state.customization.column_drawers" to snapshot.getJSONObject("state").getJSONObject("customization").optJSONArray("column_drawers"))
                for (name in listOf("collapsed", "groups")) {
                    val array = layout.optJSONArray(name)
                    values["layout.$name"] = array
                    array?.objects()?.forEachIndexed { index, value ->
                        val path = "layout.$name[$index](id=${value.optInt("id", -1)})"
                        values[path] = value
                        if (name == "groups" && value.optInt("id") == 43) values["$path.panels"] = value.optJSONArray("panels")
                    }
                    for (id in listOf(41, 43)) values["layout.$name(id=$id)"] = array?.objects()?.firstOrNull { it.optInt("id") == id }
                }
                return values
            }
            fun changedPaths(previous: Any?, next: Any?, path: String, output: MutableList<String>) {
                if (previous === next) return
                when {
                    previous is JSONObject && next is JSONObject ->
                        (previous.keys().asSequence().toSet() + next.keys().asSequence().toSet()).sorted().forEach {
                            changedPaths(previous.opt(it), next.opt(it), "$path.$it", output)
                        }
                    previous is JSONArray && next is JSONArray -> {
                        if (previous.length() != next.length()) output.add("$path.length")
                        for (i in 0 until maxOf(previous.length(), next.length())) changedPaths(previous.opt(i), next.opt(i), "$path[$i]", output)
                    }
                    previous != next -> output.add(path)
                }
            }
            var resizeNode: SemanticsNode? = null
            var lastDrawnBounds: androidx.compose.ui.geometry.Rect? = null
            var changedBounds = 0
            var lostMetrics = 0
            val frames = HandlerThread("workspace-frame-metrics").apply { start() }
            val listener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
                if (measuring.get()) synchronized(durations) {
                    durations.add(Frame(metrics.getMetric(FrameMetrics.TOTAL_DURATION),
                        metrics.getMetric(FrameMetrics.DEADLINE), metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP),
                        metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION), metrics.getMetric(FrameMetrics.DRAW_DURATION)))
                    lostMetrics += dropped
                }
            }
            val drawListener = android.view.ViewTreeObserver.OnDrawListener {
                if (measuring.get()) {
                    host.workspaceGeometry?.revision?.let { drawnRevisions.add(it) }
                    if (modelIdentityTrace) modelDraws.add(ModelDraw(System.nanoTime(), host.workspaceGeometry?.revision, modelValues()))
                    if (colorWheel) host.snapshot?.objectOrNull("paint_pair")?.array("rgba")?.toString()?.let { drawnPaints.add(it) }
                    // Read the retained LayoutNode's actual allocation. A newly
                    // received revision may still be awaiting recomposition.
                    resizeNode?.boundsInRoot?.let { bounds ->
                        if (lastDrawnBounds != bounds) changedBounds++
                        lastDrawnBounds = bounds
                    }
                }
            }
            scenario.onActivity { owner.view.viewTreeObserver.addOnDrawListener(drawListener) }
            window.addOnFrameMetricsAvailableListener(listener, Handler(frames.looper))
            try {
                action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to transparency)))
                val selectedMode = args.getString("benchmarkMode")
                for (mouse in if (bridge) listOf(true) else listOf(true, false))
                for (mode in (if (bridge) listOf(if (resize) "bridge-gap" else "bridge-panel") else if (colorWheel) listOf(wheel) else if (colorOverlap) listOf("color-overlap") else if (resize) listOf("brushes", "properties", "navigator", "toolbar") else listOf("attached", "floating", "destination")).filter { selectedMode == null || it == selectedMode })
                for (run in if (bridge || tierPhoto) -1 until repeats else 0 until repeats) {
                    if (resize && !bridge) fixture.getJSONObject("layout").getJSONArray("bands").apply {
                        getJSONObject(0).put("root", tabs(41, mode))
                        getJSONObject(1).put("root", tabs(43, "layers"))
                    }
                    action(obj("type" to "restore_workspace", "workspace" to fixture))
                    waitFor { bridge || !resize || host.snapshot!!.getJSONObject("layout").array("groups").objects().any { it.getInt("id") == 41 && it.getString("active") == mode } }
                    if (bridge) {
                        click("column-icon-brushes")
                        waitFor { owner.find(hasTag("group-43")) != null && owner.find(hasTag("divider-45")) != null }
                    }
                    SystemClock.sleep(300)
                    if (bridge) {
                        host.drain(obj("type" to "invoke", "command" to "fit_canvas"))
                        host.awaitMain("Tier photo fits the opened bridge workspace", 120_000) {
                            val camera = host.snapshot!!.getJSONObject("state").getJSONObject("camera")
                            val area = camera.getJSONArray("work_area")
                            val zoom = camera.getDouble("zoom")
                            zoom * args.getString("width")!!.toInt() <= area.getDouble(2) + 1 &&
                                zoom * args.getString("height")!!.toInt() <= area.getDouble(3) + 1
                        }
                    }
                    if (tierPhoto && !bridge) host.drain(obj("type" to "invoke", "command" to "fit_canvas"))
                    if (colorWheel) {
                        action(obj("type" to "color", "action" to obj("op" to "shape", "shape" to "circle")))
                        action(obj("type" to "color", "action" to obj("op" to "select", "slot" to "foreground")))
                    }
                    if (editColor) {
                        click("color-edit-button")
                        waitFor { owner.find(hasTag(wheel)) != null }
                        SystemClock.sleep(300)
                    }
                    val workspace = bounds("workspace")
                    if (colorOverlap) {
                        action(obj("type" to "move_group", "group" to 43, "target" to obj("kind" to "float", "position" to JSONArray(listOf(500, 180))),
                            "viewport" to JSONArray(listOf(workspace.width / owner.view.resources.displayMetrics.density, workspace.height / owner.view.resources.displayMetrics.density))))
                        SystemClock.sleep(300)
                    }
                    if (bridge) presentationStatus("before-gesture", run)
                    val source = bounds(if (bridge) { if (resize) "divider-45" else "group-grip-43" } else if (colorWheel) wheel else if (colorOverlap) "group-grip-43" else if (resize) "divider-40" else when (mode) {
                        "attached" -> "tab-tool_settings"
                        "floating" -> "group-grip-41"
                        else -> "tab-layers"
                    }).center
                    val density = owner.view.resources.displayMetrics.density
                    val anchor = if (bridge) bounds("column-icon-navigator") else workspace
                    val first = if (bridge) { if (resize) Offset(source.x, anchor.top - 3 * density) else workspace.center - Offset(80 * density, 0f) }
                        else if (colorWheel) source - Offset(bounds(wheel).width * .15f, 0f) else if (colorOverlap) bounds("color-wheel").let { Offset(it.center.x - it.width * .15f, it.top + it.height * .25f) } else if (resize) source + Offset(60f, 0f) else bounds("tab-brushes").center
                    val last = if (bridge) { if (resize) Offset(source.x, anchor.bottom + 21 * density) else workspace.center + Offset(80 * density, 0f) }
                        else if (colorWheel) source + Offset(bounds(wheel).width * .15f, 0f) else if (colorOverlap) bounds("color-wheel").let { Offset(it.center.x + it.width * .15f, it.top + it.height * .25f) } else if (resize) source + Offset(220f, 0f) else bounds("tab-sizes").center
                    val duration = if ((bridge || tierPhoto) && run < 0) 1000L else measuredDuration
                    val down = SystemClock.uptimeMillis()
                    fun eventOnMain(action: Int, point: Offset) {
                        val properties = arrayOf(MotionEvent.PointerProperties().apply {
                            id = 0; toolType = if (mouse) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
                        })
                        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f })
                        val motion = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords,
                            0, if (mouse && action != MotionEvent.ACTION_CANCEL) MotionEvent.BUTTON_PRIMARY else 0,
                            1f, 1f, 0, 0, if (mouse) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_TOUCHSCREEN, 0)
                        owner.view.dispatchTouchEvent(motion); motion.recycle()
                    }
                    fun event(action: Int, point: Offset) = scenario.onActivity { eventOnMain(action, point) }
                    event(MotionEvent.ACTION_DOWN, source)
                    try {
                        event(MotionEvent.ACTION_MOVE, if (resize || bridge || colorOverlap || colorWheel || mode == "attached") first else workspace.center)
                        SystemClock.sleep(750)
                        if (bridge && resize && run < 0) {
                            val keys = JSONArray()
                            for (gap in listOf(anchor.top - 6 * density, anchor.top - 3 * density, anchor.top,
                                anchor.bottom - 6 * density, anchor.bottom - 3 * density, anchor.bottom,
                                anchor.bottom + .25f * density, anchor.bottom + density, anchor.bottom + 6 * density, anchor.bottom + 18 * density)) {
                                event(MotionEvent.ACTION_MOVE, Offset(source.x, gap + 3 * density))
                                SystemClock.sleep(250)
                                val name = "$label-key-${keys.length()}"
                                val origin = IntArray(2)
                                scenario.onActivity { owner.view.getLocationOnScreen(origin) }
                                val x = (anchor.right + origin[0] + 1).toInt()
                                val top = kotlin.math.ceil(anchor.top + origin[1]).toInt()
                                val bottom = kotlin.math.ceil(anchor.bottom + origin[1]).toInt()
                                val palette = host.snapshot!!.getJSONObject("state").getJSONObject("palette")
                                val expected = android.graphics.Color.parseColor(palette.getString("panel"))
                                val sourceX = (anchor.left + anchor.width * .1f + origin[0]).toInt()
                                val sourceY = (anchor.center.y + origin[1]).toInt()
                                val selection = android.graphics.Color.parseColor(palette.getString("selection"))
                                keys.put(obj("target_gap_px" to gap, "actual_gap_px" to bounds("group-43").bottom,
                                    "anchor_top_px" to anchor.top, "anchor_bottom_px" to anchor.bottom, "face_x_px" to x,
                                    "expected_face_rgba" to expected, "source_sample_px" to JSONArray(listOf(sourceX, sourceY)),
                                    "expected_source_rgba" to selection, "layout" to JSONObject(host.snapshot!!.getJSONObject("layout").toString()),
                                    "capture" to "$name.png"))
                                if (keys.length() == 1) scenario.onActivity {
                                    val layout = host.snapshot!!.getJSONObject("layout")
                                    keys.getJSONObject(0).put("prepared_columns", JSONArray(host.panelVectorColumns.map { column ->
                                        val authored = layout.array("collapsed").objects().first { it.getInt("id") == column.id }
                                        obj("column" to column.id, "groups" to JSONArray(column.groups.sorted()),
                                            "native_bounds" to JSONObject(column.bounds.toString()),
                                            "source_bounds" to JSONObject(authored.getJSONObject("bounds").toString()),
                                            "open_bounds" to JSONObject(authored.getJSONObject("open").getJSONObject("bounds").toString()),
                                            "rounded_origin_px" to JSONArray(listOf((column.bounds.number("x") * density).roundToInt(),
                                                (column.bounds.number("y") * density).roundToInt())))
                                    }))
                                }
                                keys.getJSONObject(keys.length() - 1).put("presentation_status", presentationStatus("keyframe", run))
                                java.io.File(output, "$label-keyframes.json").writeText(keys.toString())
                                screenshot("workspace-bridge-benchmark/$name.png") { pixels ->
                                    for (y in top until bottom) assertEquals("$name: filled source bridge face at $x,$y; gap=$gap anchor=$anchor",
                                        expected, pixels.getPixel(x, y))
                                    val actual = pixels.getPixel(sourceX, sourceY)
                                    assertEquals("$name: selected source remains opaque", 255, actual ushr 24)
                                    assertTrue("$name: selected source remains visible at $sourceX,$sourceY; expected=$selection actual=$actual anchor=$anchor",
                                        listOf(0, 8, 16).all { kotlin.math.abs((selection shr it and 255) - (actual shr it and 255)) <= 1 })
                                }
                            }
                        }
                        if (bridge && !resize && run < 0) screenshot("workspace-bridge-benchmark/$label-panel.png")
                        host.measurementReport(true)
                        synchronized(durations) { durations.clear(); lostMetrics = 0 }
                        scenario.onActivity {
                            drawnRevisions.clear(); drawnPaints.clear(); changedBounds = 0
                            modelDraws.clear()
                            compositionDirty.clear()
                            synchronized(snapshotChanges) { snapshotChanges.clear() }
                            resizeNode = if (resize) owner.find(hasTag(if (bridge) "group-43" else "group-41")) else null
                            lastDrawnBounds = resizeNode?.boundsInRoot
                        }
                        if (bridge && run == 0 && args.getString("profileBarrier") == "true") {
                            java.io.File(output, "$label-profile-ready").writeText("ready")
                            val go = java.io.File(output, "$label-profile-go")
                            val until = SystemClock.uptimeMillis() + 60_000
                            while (!go.exists()) { check(SystemClock.uptimeMillis() < until); SystemClock.sleep(25) }
                        }
                        val presentationBefore = if (bridge) presentationStatus("before-contact", run) else null
                        var presentationDuring: JSONObject? = null
                        measuring.set(true)
                        val traceName = "workspace-benchmark-${if (mouse) "mouse" else "touch"}-$mode" + if (bridge) "-$label-$run" else ""
                        android.os.Trace.beginAsyncSection(traceName, 1)
                        val beginNs = System.nanoTime()
                        val beginBootNs = SystemClock.elapsedRealtimeNanos()
                        val beginMonoAfterBootNs = System.nanoTime()
                        val thermal = (scenario.activity().getSystemService(android.content.Context.POWER_SERVICE) as android.os.PowerManager)
                        val thermalBefore = thermal.currentThermalStatus
                        val start = SystemClock.uptimeMillis()
                        val finished = CountDownLatch(1)
                        var samples = 0
                        var refreshRate = 0f
                        // Generate native motion on the real display clock. Sleeping
                        // 8 ms AFTER a blocking main-thread call caps input below 120 Hz.
                        lateinit var motion: Choreographer.FrameCallback
                        scenario.onActivity {
                            refreshRate = owner.view.display.refreshRate
                            val choreographer = Choreographer.getInstance()
                            motion = Choreographer.FrameCallback {
                                val elapsed = SystemClock.uptimeMillis() - start
                                if (elapsed >= duration) { finished.countDown() }
                                else {
                                    // Constant-speed resize sweeps avoid repeated pixels from
                                    // easing near the ends, making actual allocation changes
                                    // a useful measure of fresh resize geometry.
                                    val cycle = (elapsed % 1000) / 500f
                                    val progress = if (resize || bridge) (if (cycle <= 1f) cycle else 2f - cycle)
                                        else (sin(elapsed * Math.PI / 500) * .5 + .5).toFloat()
                                    val point = if (mode == "floating") Offset(workspace.center.x + (progress - .5f) * workspace.width * .25f, workspace.center.y)
                                        else Offset(first.x + (last.x - first.x) * progress, first.y + (last.y - first.y) * progress)
                                    eventOnMain(MotionEvent.ACTION_MOVE, point)
                                    samples++
                                    choreographer.postFrameCallback(motion)
                                }
                            }
                            choreographer.postFrameCallback(motion)
                        }
                        try {
                            if (requireSharedPresentation) {
                                val firstWait = duration / 2
                                finished.await(firstWait, TimeUnit.MILLISECONDS)
                                presentationDuring = presentationStatus("during", run)
                                assertTrue("Real display input producer completed", finished.await(15_000 - firstWait, TimeUnit.MILLISECONDS))
                            } else assertTrue("Real display input producer completed", finished.await(15, TimeUnit.SECONDS))
                        }
                        finally {
                            scenario.onActivity { Choreographer.getInstance().removeFrameCallback(motion) }
                            android.os.Trace.endAsyncSection(traceName, 1)
                        }
                        val endNs = System.nanoTime()
                        val endBootNs = SystemClock.elapsedRealtimeNanos()
                        val endMonoAfterBootNs = System.nanoTime()
                        if (bridge) SystemClock.sleep(200)
                        measuring.set(false)
                        val elapsed = if (bridge) (endNs - beginNs) / 1_000_000 else SystemClock.uptimeMillis() - start
                        val rows = synchronized(durations) { durations.filter { !bridge || it.vsync in beginNs..endNs } }
                        val timings = rows.map { it.duration }.sorted()
                        val vsyncs = rows.map { it.vsync }.distinct().sorted()
                        val intervals = vsyncs.zipWithNext { a, b -> b - a }.sorted()
                        var drawn = 0
                        var changes = 0
                        scenario.onActivity { drawn = drawnRevisions.size; changes = changedBounds }
                        assertTrue("Android must render while dragging", timings.isNotEmpty())
                        fun percentile(values: List<Long>, fraction: Double) = values[((values.size - 1) * fraction).toInt()] / 1_000_000.0
                        val metrics = host.measurementReport()
                        val presentationAfter = if (bridge) presentationStatus("after-contact", run) else null
                        val expectRetained = !bridge && !colorWheel && (!resize || args.getString("expectRetainedResize") != "false")
                        if (expectRetained) assertEquals("Steady motion retains the full UI models", 0L, metrics.getLong("snapshots_published"))
                        scenario.onActivity {
                            if (colorWheel && !editColor) assertTrue("Committed wheel colors change during motion", drawnPaints.size > 30)
                            if (expectRetained) assertEquals("Steady motion retains panel content", 0L, metrics.getLong("panel_content_changes"))
                            val geometry = host.workspaceGeometry!!
                            if (geometry.group != null) {
                                val shown = owner.find(hasTag("group-${geometry.group}"))!!.boundsInRoot
                                val density = owner.view.resources.displayMetrics.density
                                assertEquals("Native placement follows Rust geometry", workspace.left + geometry.bounds!!.left * density, shown.left, 1.1f)
                                assertEquals(workspace.top + geometry.bounds.top * density, shown.top, 1.1f)
                                if (colorOverlap) {
                                    val wheel = owner.find(hasTag("color-wheel"))!!.boundsInRoot
                                    assertTrue("Motion overlaps the visible Color wheel", shown.overlaps(wheel))
                                }
                            }
                        }
                        val result = obj("mouse" to mouse, "mode" to mode, "elapsed_ms" to elapsed, "inputs" to samples,
                            "bridge_content" to bridgeContent,
                            "transparency" to transparency,
                            "display_hz" to refreshRate, "debuggable" to BuildConfig.DEBUG,
                            "trajectory" to if (resize) "triangle" else "sine",
                            "frames" to rows.size, "distinct_vsyncs" to vsyncs.size, "drawn_revisions" to drawn,
                            "frame_rate" to (vsyncs.size * 1000.0 / elapsed), "drawn_update_rate" to (drawn * 1000.0 / elapsed),
                            "changed_bounds" to changes, "changed_bounds_rate" to (changes * 1000.0 / elapsed),
                            "frame_p50_ms" to percentile(timings, .5), "frame_p95_ms" to percentile(timings, .95),
                            "layout_p50_ms" to percentile(rows.map { it.layout }.sorted(), .5),
                            "layout_p95_ms" to percentile(rows.map { it.layout }.sorted(), .95),
                            "draw_p50_ms" to percentile(rows.map { it.draw }.sorted(), .5),
                            "draw_p95_ms" to percentile(rows.map { it.draw }.sorted(), .95),
                            "drawn_paints" to drawnPaints.size,
                            "vsync_interval_p50_ms" to percentile(intervals, .5), "vsync_interval_p95_ms" to percentile(intervals, .95),
                            "vsync_interval_p99_ms" to percentile(intervals, .99),
                            "deadline_misses" to rows.count { it.deadline > 0 && it.duration > it.deadline }, "lost_metrics" to lostMetrics,
                            "snapshot_attempts" to metrics.getLong("snapshot_attempts"), "snapshots" to metrics.getLong("snapshots_published"),
                            "workspace_updates" to metrics.getLong("workspace_updates_published"))
                        val publicationRows = metrics.getJSONArray("publications").let { a -> (0 until a.length()).map { a.getJSONArray(it) } }
                        for ((column, name) in listOf("native", "parse", "prepare").withIndex()) {
                            val values = publicationRows.map { it.getLong(column) }.sorted()
                            if (values.isNotEmpty()) { result.put("${name}_p50_ms", percentile(values, .5)); result.put("${name}_p95_ms", percentile(values, .95)) }
                        }
                        result.put("publication_utf16_units", publicationRows.sumOf { it.getLong(3) })
                        result.put("panel_content_changes", metrics.getLong("panel_content_changes"))
                        if ((bridge || tierPhoto) && run >= 0) {
                            if (dirtyTrace) {
                                result.put("composition_dirty_records", JSONArray(compositionDirty.filter { it.time in beginNs..endNs }.map {
                                    obj("time_ns" to it.time, "key" to it.key, "dirty1" to it.first, "dirty2" to it.second, "name" to it.name)
                                }))
                                result.put("snapshot_state_labels", stateLabels)
                                result.put("snapshot_change_records", JSONArray(synchronized(snapshotChanges) { snapshotChanges.filter { it.time in beginNs..endNs } }.map {
                                    obj("time_ns" to it.time, "identity" to it.identity, "type" to it.type,
                                        "value_type" to it.valueType, "element_type" to it.elementType)
                                }))
                            }
                            if (modelIdentityTrace) {
                                val records = modelDraws.filter { it.time in beginNs..endNs }
                                val versions = JSONObject()
                                records.forEach { draw -> draw.values.values.filterNotNull().forEach {
                                    versions.put(System.identityHashCode(it).toString(), it)
                                } }
                                result.put("draw_model_values", versions)
                                result.put("draw_model_records", JSONArray(records.mapIndexed { index, draw ->
                                    val previous = records.getOrNull(index - 1)
                                    val identities = JSONObject()
                                    draw.values.forEach { (path, value) ->
                                        val old = previous?.values?.get(path)
                                        val changes = mutableListOf<String>()
                                        if (previous != null) changedPaths(old, value, path, changes)
                                        identities.put(path, obj("identity" to if (value == null) 0 else System.identityHashCode(value),
                                            "present" to (value != null), "same_reference" to (previous != null && old === value),
                                            "content_equal" to (previous != null && changes.isEmpty()), "changed_paths" to JSONArray(changes)))
                                    }
                                    val changes = mutableListOf<String>()
                                    if (previous != null) changedPaths(previous.values["layout"], draw.values["layout"], "layout", changes)
                                    obj("draw_ns" to draw.time, "revision" to draw.revision, "objects" to identities,
                                        "layout_changed_paths" to JSONArray(changes))
                                }))
                            }
                            result.put("begin_ns", beginNs); result.put("end_ns", endNs); result.put("trace_name", traceName)
                            result.put("begin_boot_ns", beginBootNs); result.put("end_boot_ns", endBootNs)
                            result.put("begin_mono_after_boot_ns", beginMonoAfterBootNs); result.put("end_mono_after_boot_ns", endMonoAfterBootNs)
                            result.put("host_metrics", metrics)
                            if (bridge) result.put("presentation_status", JSONArray(listOfNotNull(presentationBefore, presentationDuring, presentationAfter)))
                            result.put("run", run); result.put("theme", args.getString("theme", "light"))
                            result.put("thermal_before", thermalBefore); result.put("thermal_after", thermal.currentThermalStatus)
                            result.put("raw_frames", JSONArray(rows.map { JSONArray(listOf(it.vsync, it.duration, it.deadline, it.layout, it.draw)) }))
                            result.put("frame_columns", JSONArray(listOf("vsync_ns", "total_ns", "deadline_ns", "layout_ns", "draw_ns")))
                            result.put("state", host.snapshot!!.getJSONObject("state")); result.put("workspace", fixture)
                            result.put("photo", args.getString("photo")); result.put("width", args.getString("width")); result.put("height", args.getString("height"))
                            val filename = if (bridge) "$label-$run.json" else "$label-$mode-${if (mouse) "mouse" else "touch"}-$run.json"
                            java.io.File(output, filename).writeText(result.toString())
                        }
                        Log.i(if (resize) "CapyResizePerf" else "CapyDragPerf", result.toString())
                    } finally {
                        measuring.set(false)
                        event(MotionEvent.ACTION_CANCEL, source)
                    }
                    if (editColor) { click("color-cancel"); waitFor { owner.find(hasTag(wheel)) == null } }
                    action(obj("type" to "close_settings"))
                    if (colorOverlap || colorWheel) action(obj("type" to "restore_workspace", "workspace" to fixture))
                    waitFor { host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").array("floating").length() == (if (colorOverlap || colorWheel) 1 else 0) }
                }
            } finally {
                measuring.set(false)
                applyObserver?.dispose()
                window.removeOnFrameMetricsAvailableListener(listener)
                scenario.onActivity { owner.view.viewTreeObserver.removeOnDrawListener(drawListener) }
                frames.quitSafely()
            }
        }
    }

    private fun CanvasHost.awaitMain(label: String, timeout: Long = 10_000, condition: () -> Boolean) {
        val until = SystemClock.uptimeMillis() + timeout
        do {
            var ready = false
            instrumentation.runOnMainSync { assertNull(failure); assertNull(actionError); ready = condition() }
            if (ready) return
            SystemClock.sleep(16)
        } while (SystemClock.uptimeMillis() < until)
        throw AssertionError("Timed out: $label")
    }

    private fun CanvasHost.drain(action: JSONObject? = null, seconds: Long = 15) {
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync { action?.let { dispatch(it) }; query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue("Native UI publication", done.await(seconds, TimeUnit.SECONDS))
    }

    private fun CanvasHost.measurementReport(reset: Boolean = false): JSONObject {
        val done = CountDownLatch(1)
        var report = JSONObject()
        measurements(reset) { report = it; done.countDown() }
        assertTrue("Measurement report", done.await(30, TimeUnit.SECONDS))
        return report
    }

    private fun CanvasHost.openDocument(file: File) {
        val hostFileJobs = DocumentController.nativeFileJobsForTest
        DocumentController.nativeFileJobsForTest = true
        try {
            val task = runBlocking { withNative { handle ->
                Native.dispatch(handle, obj("type" to "invoke", "command" to "open_document").toString())
                fun state() = JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
                fun request(value: JSONObject) = value.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
                var state = state()
                if (request(state).getJSONObject("kind").getJSONObject("request").getString("type") == "confirm_close") {
                    Native.documentClose(handle, request(state).getInt("id"), "\"discard\"")
                    state = state()
                }
                val document = state.getJSONObject("document_file")
                Native.projectTask(handle, request(state).getInt("id"), "null", document.getLong("epoch"), document.getLong("revision"))
            } }
            val epoch = try {
                Native.projectWork(task, ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), "")
                runBlocking { withNative { handle ->
                    Native.projectAdopt(handle, task, "null")
                    val now = System.nanoTime(); Native.frame(handle, now, now + 16_666_667)
                    Native.dispatch(handle, obj("type" to "close_settings").toString())
                    JSONObject(Native.snapshot(handle)!!).getJSONObject("state").getJSONObject("document_file").getLong("epoch")
                } }
            } finally { Native.projectFree(task) }
            instrumentation.runOnMainSync { documentChanged() }
            awaitMain("the opened drawing", 120_000) {
                snapshot?.optBoolean("shaders_ready") == true && snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch") == epoch
            }
        } finally { DocumentController.nativeFileJobsForTest = hostFileJobs }
    }

    private fun screenshot(path: String, inspect: ((android.graphics.Bitmap) -> Unit)? = null) {
        val file = File(instrumentation.targetContext.getExternalFilesDir(null), path).apply { parentFile!!.mkdirs() }
        val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot()) { "Native screenshot unavailable" }
        try {
            file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
            inspect?.let { verify -> bitmap.copy(android.graphics.Bitmap.Config.ARGB_8888, false).let { pixels -> try { verify(pixels) } finally { pixels.recycle() } } }
        } finally { bitmap.recycle() }
    }

    private fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
        "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon")

    private inline fun <reified T> View.descendant(): T? {
        val pending = ArrayDeque(listOf(this))
        while (pending.isNotEmpty()) {
            val next = pending.removeFirst()
            if (next is T) return next
            if (next is ViewGroup) for (i in 0 until next.childCount) pending.add(next.getChildAt(i))
        }
        return null
    }
    private fun SemanticsNode.find(match: (SemanticsNode) -> Boolean): SemanticsNode? =
        if (match(this)) this else children.firstNotNullOfOrNull { it.find(match) }
    private fun ViewRootForTest.find(match: (SemanticsNode) -> Boolean) = semanticsOwner.unmergedRootSemanticsNode.find(match)
    private fun hasTag(tag: String): (SemanticsNode) -> Boolean = { it.config.getOrNull(SemanticsProperties.TestTag) == tag }

    private fun assertTrue(condition: Boolean) = assertTrue("Expected true", condition)
    private fun assertTrue(message: String, condition: Boolean) { if (!condition) throw AssertionError(message) }
    private fun assertFalse(condition: Boolean) = assertTrue("Expected false", !condition)
    private fun assertNull(value: Any?) { if (value != null) throw AssertionError("Expected null, was $value") }
    private fun assertEquals(message: String, expected: Any?, actual: Any?) {
        if (expected != actual) throw AssertionError("$message expected:<$expected> but was:<$actual>")
    }
    private fun assertEquals(message: String, expected: Float, actual: Float, delta: Float) {
        if (!(kotlin.math.abs(expected - actual) <= delta)) throw AssertionError("$message expected:<$expected> but was:<$actual>")
    }
    private fun assertEquals(expected: Float, actual: Float, delta: Float) = assertEquals("Expected equal coordinates", expected, actual, delta)
    private fun fail(message: String): Nothing = throw AssertionError(message)
}

private class WorkspaceBenchmarkActivity(private val instrumentation: Instrumentation, private val value: MainActivity) {
    fun onActivity(action: (MainActivity) -> Unit) = instrumentation.runOnMainSync { action(value) }
    fun activity() = value
}
