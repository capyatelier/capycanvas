package art.capycanvas

import android.graphics.Bitmap
import android.content.ContentValues
import android.provider.MediaStore
import android.os.SystemClock
import android.os.ParcelFileDescriptor
import android.view.KeyEvent
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real native widgets, JNI and Vulkan in the tablet emulator. No fake renderer. */
class AndroidHostTest {
    companion object {
        private val runId = System.currentTimeMillis().toString()
        // Ask an isolated, GPU-less Rust session for its defaults, not a Kotlin
        // copy of the workspace schema. Repeated runs must not collect toolbars.
        private val defaultWorkspace by lazy {
            val handle = Native.create(false)
            try { JSONObject(Native.snapshot(handle)!!).getJSONObject("state").getJSONObject("workspace").toString() }
            finally { Native.destroy(handle) }
        }
    }
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private var originalWorkspace: JSONObject? = null
    @Before fun ready() {
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("gpu_ready") == true || host.failure != null }
        assertNull("GPU initialization", host.failure)
        originalWorkspace = JSONObject(state().getJSONObject("workspace").toString())
        compose.runOnIdle {
            host.dispatch(obj("type" to "close_settings"))
            host.dispatch(obj("type" to "set_theme", "theme" to "light"))
            host.dispatch(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
        }
        waitState { it.optString("theme") == "light" && it.getJSONObject("workspace").toString() == defaultWorkspace }
        compose.waitForIdle()
    }
    @After fun restoreWorkspace() {
        originalWorkspace?.let { workspace ->
            compose.runOnIdle {
                host.dispatch(obj("type" to "close_settings"))
                host.dispatch(obj("type" to "restore_workspace", "workspace" to workspace))
            }
            waitState { it.getJSONObject("workspace").toString() == workspace.toString() }
        }
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun preferences() = host.snapshot!!.getJSONObject("preferences")
    private fun shell(command: String) = ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command))
        .bufferedReader().use { it.readText() }
    private fun waitState(test: (JSONObject) -> Boolean) = compose.waitUntil(10_000) { test(state()) }
    private fun findCanvas(view: View): CanvasSurfaceView? = when (view) {
        is CanvasSurfaceView -> view
        is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { findCanvas(view.getChildAt(it)) }
        else -> null
    }
    private fun penStroke(steps: Int = 60, synchronous: Boolean = true) {
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        instrumentation.runOnMainSync {
            canvas = findCanvas(compose.activity.window.decorView)!!
            canvas.getLocationOnScreen(location)
        }
        val down = SystemClock.uptimeMillis()
        for (i in 0..steps) {
            val coords = MotionEvent.PointerCoords().apply {
                x = canvas.width * (0.35f + i.toFloat() / steps * 0.3f) + location[0]
                y = canvas.height * (0.45f + kotlin.math.sin(i / 12f) * 0.08f) + location[1]
                pressure = 0.3f + i.toFloat() / steps * 0.6f
                setAxisValue(MotionEvent.AXIS_TILT, 0.3f)
                setAxisValue(MotionEvent.AXIS_ORIENTATION, 0.2f)
            }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val action = when (i) { 0 -> MotionEvent.ACTION_DOWN; steps -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, arrayOf(props), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
            assertTrue("Stylus event accepted", instrumentation.uiAutomation.injectInputEvent(event, synchronous))
            event.recycle()
            if (i != steps) SystemClock.sleep(8)
        }
    }
    /** Native dispatch tests retain full MotionEvent history and pointer IDs. */
    private fun canvasEvent(action: Int, points: List<androidx.compose.ui.geometry.Offset>,
        tool: Int = MotionEvent.TOOL_TYPE_FINGER, history: Boolean = false,
        pointerTools: List<Int> = List(points.size) { tool }) {
        instrumentation.runOnMainSync {
            val canvas = findCanvas(compose.activity.window.decorView)!!
            val coords = points.map { point -> MotionEvent.PointerCoords().apply {
                x = canvas.width * point.x; y = canvas.height * point.y; pressure = 0.7f
                setAxisValue(MotionEvent.AXIS_TILT, 0.4f)
            } }.toTypedArray()
            val props = points.indices.map { i -> MotionEvent.PointerProperties().apply { id = i; toolType = pointerTools[i] } }.toTypedArray()
            val time = SystemClock.uptimeMillis()
            val source = when (tool) {
                MotionEvent.TOOL_TYPE_FINGER -> InputDevice.SOURCE_TOUCHSCREEN
                MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE
                else -> InputDevice.SOURCE_STYLUS
            }
            val event = MotionEvent.obtain(time - 30, time - if (history) 2 else 0, action, points.size, props, coords, 0, 0, 1f, 1f, 1, 0, source, 0)
            if (history) event.addBatch(time, coords.map { old -> MotionEvent.PointerCoords(old).apply { x += 4f; pressure = 0.9f } }.toTypedArray(), 0)
            assertTrue(if (action == MotionEvent.ACTION_HOVER_MOVE) canvas.dispatchGenericMotionEvent(event) else canvas.dispatchTouchEvent(event))
            event.recycle()
        }
    }
    private fun capture(name: String): Bitmap {
        compose.waitForIdle()
        val bitmap = instrumentation.uiAutomation.takeScreenshot()
        val directory = File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }
        File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        // Gradle's connected-test runner uninstalls the app, removing its private
        // output directory. MediaStore test captures survive for visual review.
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, "$name.png")
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/CapyCanvasValidation/$runId")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        })!!
        resolver.openOutputStream(uri)!!.use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
        return bitmap
    }
    private fun darkPixels(image: Bitmap): Int {
        var dark = 0
        for (y in image.height * 35 / 100 until image.height * 65 / 100 step 2) {
            for (x in image.width * 35 / 100 until image.width * 65 / 100 step 2) {
                val c = image.getPixel(x, y)
                if (android.graphics.Color.red(c) < 100 && android.graphics.Color.green(c) < 100) dark++
            }
        }
        return dark
    }
    @Test fun stylusDrawsAndUndoRedoChangePixels() {
        penStroke()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        assertNull(host.failure)
        val painted = capture("01-stylus-light")
        val dark = darkPixels(painted)
        assertTrue("Stroke deposits visible pixels in the canvas, not just cursor state ($dark)", dark > 100)
        compose.onNodeWithContentDescription("Undo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "redo" && c.getBoolean("enabled") } }
        val undone = darkPixels(capture("02-undo"))
        assertTrue("Undo removes deposited pixels ($undone vs $dark)", undone < dark / 10)
        compose.onNodeWithContentDescription("Redo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        assertTrue("Redo restores deposited pixels", darkPixels(capture("03-redo")) >= dark * 9 / 10)
    }
    @Test fun measureHighRateStylusIngressAndRenderScheduling() {
        val options = InstrumentationRegistry.getArguments()
        val brush = options.getString("capyBrush", "G-Pen")!!
        val diameter = options.getString("capyBrushSize", "18")!!.toFloat()
        val preset = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }
            .first { it.getString("label") == brush }.getInt("id")
        // Warm pipelines and provide existing pigment for destination-aware tools.
        penStroke(60)
        compose.runOnIdle {
            host.dispatch(obj("type" to "select_brush", "id" to preset))
            host.dispatch(obj("type" to "set_brush_size", "value" to diameter))
        }
        waitState { it.getJSONObject("brush").getInt("preset") == preset && it.getJSONObject("brush").number("diameter") == diameter }
        penStroke(60)
        val cleared = CountDownLatch(1)
        host.measurements(true) { cleared.countDown() }
        assertTrue(cleared.await(10, TimeUnit.SECONDS))
        shell("dumpsys SurfaceFlinger --latency-clear")
        penStroke(600, synchronous = false)
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        val collected = CountDownLatch(1)
        var report: JSONObject? = null
        host.measurements { report = it; collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        val data = report!!
        data.put("brush", brush).put("diameter", diameter)
        val layers = shell("dumpsys SurfaceFlinger --list").lineSequence().filter {
            it.contains("SurfaceView[art.capycanvas/art.capycanvas.MainActivity](BLAST)")
        }.map { it.substringAfter("RequestedLayerState{").substringBefore(" parentId=").removeSuffix("}") }.toList()
        // UiAutomation tokenizes arguments directly, not through a shell; these
        // app-owned names contain no whitespace and must not include quotes.
        val samples = layers.associateWith { shell("dumpsys SurfaceFlinger --latency $it") }
        data.put("surface_layers", JSONObject(samples))
        val compositor = samples.values.maxByOrNull { it.length } ?: ""
        data.put("surface_flinger", compositor)
        assertTrue("Input stream reached the native host", data.array("inputs").length() > 100)
        assertTrue("Renderer produced continuous frames", data.array("frames").length() > 100)
        assertTrue("Input arrays are reused, not allocated for every event",
            data.getLong("pointer_allocations") < data.array("inputs").length() / 2)
        assertTrue("Unchanged drawing state does not build and serialize UI snapshots",
            data.getLong("snapshots_published") < data.getLong("snapshot_attempts") / 2)
        val rows = data.array("frames").values().map { it as org.json.JSONArray }
        val input = data.array("inputs").values().map { it as org.json.JSONArray }
        fun summary(values: List<Double>): JSONObject {
            val sorted = values.sorted()
            fun p(q: Double) = sorted[((sorted.size - 1) * q).toInt()]
            return obj("count" to sorted.size, "p50_ms" to p(0.5), "p95_ms" to p(0.95), "p99_ms" to p(0.99), "max_ms" to sorted.last())
        }
        val summary = obj("cpu_render_present" to summary(rows.map { it.getDouble(2) / 1e6 }),
            "cpu_callback" to summary(rows.map { it.getDouble(10) / 1e6 }),
            "publish_schedule" to summary(rows.map { it.getDouble(9) / 1e6 }),
            "cpu_paint" to summary(rows.map { it.getDouble(4) / 1e6 }),
            "surface_acquire" to summary(rows.map { it.getDouble(5) / 1e6 }),
            "cpu_viewport" to summary(rows.map { it.getDouble(6) / 1e6 }),
            "queue_present" to summary(rows.map { it.getDouble(7) / 1e6 }),
            "cpu_poll" to summary(rows.map { it.getDouble(8) / 1e6 }),
            "frame_interval" to summary(rows.zipWithNext { a, b -> (b.getDouble(0) - a.getDouble(0)) / 1e6 }),
            "input_delivery" to summary(input.map { (it.getDouble(1) - it.getDouble(0)) / 1e6 }),
            "input_queue" to summary(input.map { (it.getDouble(2) - it.getDouble(1)) / 1e6 }),
            "cpu_input" to summary(input.map { it.getDouble(3) / 1e6 }))
        val presented = compositor.lineSequence().drop(1).mapNotNull { line ->
            line.trim().split(Regex("\\s+")).getOrNull(1)?.toLongOrNull()?.takeIf { it > 0 && it < Long.MAX_VALUE }
        }.toList().distinct().sorted()
        if (presented.size > 2) {
            summary.put("composited_interval", summary(presented.zipWithNext { a, b -> (b - a) / 1e6 }))
            summary.put("composited_fps", (presented.size - 1) * 1e9 / (presented.last() - presented.first()))
        }
        android.util.Log.i("CapyBenchmark", summary.toString())
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, "latency.json")
            put(MediaStore.Downloads.MIME_TYPE, "application/json")
            put(MediaStore.Downloads.RELATIVE_PATH, "Download/CapyCanvasValidation/$runId")
            put(MediaStore.Downloads.IS_PENDING, 1)
        })!!
        data.put("summary", summary)
        resolver.openOutputStream(uri)!!.bufferedWriter().use { it.write(data.toString()) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
        capture("11-high-rate-stylus")
    }
    @Test fun preferencesSearchAndThemeAreCoreDriven() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot?.objectOrNull("preferences") != null }
        capture("04-preferences-light")
        compose.onNodeWithContentDescription("Search settings").performClick()
        compose.onNodeWithText("Search settings").performTextInput("prediction")
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").array("search_results").length() > 0 }
        capture("05-settings-search")
        compose.onNodeWithText("×").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").getString("page") == "input" }
        capture("06-pen-input")
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("07-workspace-dark")
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        capture("08-preferences-dark")
    }
    @Test fun settingsAndDetailsSlideWithinOneSurface() {
        compose.mainClock.autoAdvance = false
        try {
            compose.onNodeWithContentDescription("Settings").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
            compose.mainClock.advanceTimeBy(80)
            val entering = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            compose.mainClock.advanceTimeBy(320)
            val settled = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            assertTrue("Settings slide down from above ($entering -> $settled)", entering < settled)

            compose.onNodeWithTag("preference-theme").performClick()
            compose.waitUntil(10_000) { preferences().objectOrNull("detail") != null }
            compose.mainClock.advanceTimeBy(80)
            val enteringDetail = compose.onNodeWithTag("settings-content-value:theme").fetchSemanticsNode().positionInRoot.x
            compose.mainClock.advanceTimeBy(300)
            val settledDetail = compose.onNodeWithTag("settings-content-value:theme").fetchSemanticsNode().positionInRoot.x
            assertTrue("Detail slides in from the right ($enteringDetail -> $settledDetail)", enteringDetail > settledDetail)
            compose.onAllNodes(isDialog()).assertCountEquals(0)
            compose.onAllNodes(isPopup()).assertCountEquals(0)

            compose.onNodeWithContentDescription("Back").performClick()
            compose.waitUntil(10_000) { preferences().objectOrNull("detail") == null }
            compose.mainClock.advanceTimeBy(400)
            compose.onNodeWithTag("preference-theme").assertIsDisplayed()
            compose.onNodeWithTag("settings-done").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
            compose.mainClock.advanceTimeBy(80)
            val leaving = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            assertTrue("Settings slide up on dismissal ($settled -> $leaving)", leaving < settled)
            compose.mainClock.advanceTimeBy(300)
            compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
        } finally { compose.mainClock.autoAdvance = true }
    }

    @Test fun inlineSettingsApplyValidateAndNeverPaintUnderneath() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.onNodeWithTag("preference-prediction_horizon").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("detail") != null }
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        capture("32-number-detail")
        val before = state().getJSONObject("settings").number("prediction_ms")
        compose.onNodeWithTag("setting-number").performTextReplacement("65")
        compose.onNodeWithTag("setting-number").performImeAction()
        compose.waitUntil(10_000) { !preferences().isNull("error") }
        assertEquals(before, state().getJSONObject("settings").number("prediction_ms"))
        capture("33-number-invalid")
        compose.onNodeWithTag("setting-number").performTextReplacement("64")
        compose.onNodeWithTag("setting-number").performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 64f }
        assertTrue(preferences().isNull("error"))
        compose.onNodeWithContentDescription("Back").performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("detail") == null }
        compose.onNodeWithTag("preference-prediction_horizon").assert(hasText("64"))
        compose.onNodeWithText("About").performClick()
        val collected = CountDownLatch(1)
        host.measurements(true) { collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        penStroke(15)
        val result = CountDownLatch(1)
        host.measurements {
            assertEquals("Settings surface intercepts stylus input instead of forwarding to canvas", 0, it.array("inputs").length())
            result.countDown()
        }
        assertTrue(result.await(10, TimeUnit.SECONDS))
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertEquals(64f, state().getJSONObject("settings").number("prediction_ms"))
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.onNodeWithTag("preference-prediction_horizon").assert(hasText("64"))
        // Return this shared preference to its original accepted value.
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "prediction_horizon", "value" to before)) }
        waitState { it.getJSONObject("settings").number("prediction_ms") == before }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun panelDrawerAndDividerUseSharedLayout() {
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "brushes" }
        capture("09-brush-drawer")
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").isNull("expanded") }
        val before = host.snapshot!!.getJSONObject("layout").toString()
        val divider = host.snapshot!!.getJSONObject("layout").array("dividers").objects().first { !it.getBoolean("band") }
        val density = compose.activity.resources.displayMetrics.density
        compose.onNodeWithTag("divider-${divider.getInt("id")}").performTouchInput {
            swipe(center, center - androidx.compose.ui.geometry.Offset(0f, 70 * density), 600)
        }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").toString() != before }
        capture("10-divider-resize")
    }

    @Test fun nativeContextMenuCreatesToolbarAndToolsCanBeReordered() {
        compose.onAllNodesWithContentDescription("Move panel group").onFirst().performTouchInput { longClick() }
        capture("27-panel-menu")
        compose.onNodeWithText("New Toolbar…").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") != null }
        val picker = host.snapshot!!.getJSONObject("picker")
        val name = "Quick tools $runId"
        picker.optString("name_label").takeIf { it.isNotEmpty() }?.let { label ->
            val field = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("toolbar-name")))
            field.performTextReplacement(name)
            field.performImeAction()
        }
        val choices = picker.array("choices").objects().take(3)
        choices.forEach { choice ->
            compose.onAllNodes(hasText(choice.getString("label")) and isToggleable()).onFirst().performScrollTo().performClick()
        }
        capture("28-tool-picker")
        compose.onNodeWithText(picker.getString("confirm_label")).performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") == null }
        val custom = host.snapshot!!.array("panels").objects().first { it.getString("title") == name }
        assertEquals(3, custom.array("tiles").length())
        val ids = custom.array("tiles").objects().map { it.getInt("id") }
        val source = compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[0]}")
        val target = compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[2]}").fetchSemanticsNode().boundsInRoot
        val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
        source.performTouchInput { swipe(center, target.centerRight - origin - androidx.compose.ui.geometry.Offset(2f, 0f), 700) }
        compose.waitUntil(10_000) {
            host.snapshot!!.array("panels").objects().first { it.getString("id") == custom.getString("id") }
                .array("tiles").objects().map { it.getInt("id") } != ids
        }
        capture("12-custom-toolbar")
        assertNull(host.actionError)
    }

    @Test fun editorGeometryStaysConsistent() {
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val firstTile = toolbar.array("tiles").objects().first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$firstTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithContentDescription("Zen mode").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag("number-Brush size").assertHeightIsEqualTo(31.dp)
        val brush = state().getJSONObject("brush").getInt("preset")
        compose.onNodeWithTag("brush-preview-$brush", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        compose.onAllNodesWithContentDescription("Move panel group").onFirst().assertWidthIsEqualTo(20.dp)
        compose.onAllNodesWithText("Brushes").onFirst().assertHeightIsEqualTo(36.dp)
        compose.runOnIdle { host.invoke("fit_canvas") }
        capture("25-editor-default-light")
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("26-editor-default-dark")
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("preferences-surface").assertWidthIsEqualTo(compose.activity.resources.configuration.screenWidthDp.dp)
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun compactNumberInputAndVerticalRibbonStayUsable() {
        val field = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("number-Brush size")))
        field.performTextReplacement("42.5")
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f }
        compose.onNodeWithContentDescription("Increase Brush size").performClick()
        val step = host.catalog.getJSONObject("brush_size").number("step")
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f + step }
        field.assertTextEquals("%.1f".format(java.util.Locale.ROOT, 42.5f + step))
        compose.onNodeWithContentDescription("Decrease Brush size").performClick()
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f }

        val grip = compose.onNodeWithContentDescription("Move toolbar")
        val origin = grip.fetchSemanticsNode().boundsInRoot.topLeft
        val density = compose.activity.resources.displayMetrics.density
        grip.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(density, 200 * density) - origin, 700) }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.getString("active") == "toolbar" && it.getString("axis") == "vertical"
        } }
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val lastTile = toolbar.array("tiles").objects().last().getInt("id")
        val tile = compose.onNodeWithTag("tile-toolbar-$lastTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        assertTrue("Vertical ribbon grip is below its tools", grip.fetchSemanticsNode().boundsInRoot.top >= tile.fetchSemanticsNode().boundsInRoot.bottom)
        capture("31-vertical-ribbon")
    }

    @Test fun tabDragAppendsAndWholeGroupDragPreservesTabs() {
        fun drag(source: SemanticsNodeInteraction, target: androidx.compose.ui.geometry.Offset) {
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(center, target - origin, 700) }
        }
        val brushes = compose.onAllNodesWithText("Brushes").onFirst()
        val layers = compose.onAllNodesWithText("Layers").onFirst()
        drag(brushes, layers.fetchSemanticsNode().boundsInRoot.centerRight - androidx.compose.ui.geometry.Offset(2f, 0f))
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers"))
        } }
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.array("panels").values().contains("brushes") }
        val grip = compose.onAllNodesWithContentDescription("Move panel group").filterToOne(
            SemanticsMatcher("group grip") { node ->
                val density = compose.activity.resources.displayMetrics.density
                node.boundsInRoot.center.x > group.getJSONObject("bounds").number("x") * density
            })
        // A whole-group drop onto Sizes must preserve both tab identities.
        val sizeTitle = host.snapshot!!.array("panels").objects().first { it.getString("id") == "sizes" }.getString("title")
        val sizes = compose.onAllNodesWithText(sizeTitle).onFirst()
        drag(grip, sizes.fetchSemanticsNode().boundsInRoot.center)
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers", "sizes"))
        } }
        assertNull(host.actionError)
        capture("13-tab-group-drag")
    }

    @Test fun shortcutPageRecordsMultipleBindingsAndPersists() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search keyboard shortcuts").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        // Instrumentation can run again against an already installed app.
        if (preferences().getJSONObject("shortcut_editor").getBoolean("modified")) {
            compose.onNodeWithText("Restore default").performClick()
            compose.waitUntil(10_000) { !preferences().getJSONObject("shortcut_editor").getBoolean("modified") }
        }
        val original = preferences().getJSONObject("shortcut_editor").array("bindings").length()
        compose.onNodeWithText("Add shortcut").performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        compose.waitForIdle()
        val conflictTime = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
            assertTrue(instrumentation.uiAutomation.injectInputEvent(
                KeyEvent(conflictTime, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_E, 0), true))
        }
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").optString("conflict") == "Eraser" }
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        capture("35-shortcut-conflict-inline")
        compose.onNodeWithText("Cancel recording").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") == null }
        assertEquals(original, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithText("Add shortcut").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        compose.waitForIdle()
        val now = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
            val event = KeyEvent(now, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_J, 0, KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON)
            assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
        }
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").objectOrNull("chord") != null }
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        capture("34-shortcut-recording-inline")
        compose.onNodeWithText("Use shortcut").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original + 1 }
        capture("14-shortcut-editor")
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.activityRule.scenario.recreate()
        compose.waitUntil(20_000) { host.snapshot!!.optBoolean("gpu_ready") }
        assertNull(host.failure)
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search keyboard shortcuts").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        assertEquals(original + 1, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithText("Restore default").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original }
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
    }

    @Test fun touchNavigationHistoryCancellationAndSurfaceRecovery() {
        val a = androidx.compose.ui.geometry.Offset(0.45f, 0.5f)
        val b = androidx.compose.ui.geometry.Offset(0.55f, 0.5f)
        val initial = state().getJSONObject("camera").number("zoom")
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(a))
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(a, b))
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(a - androidx.compose.ui.geometry.Offset(0.03f, 0.02f), b + androidx.compose.ui.geometry.Offset(0.05f, 0.04f)))
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(a, b))
        canvasEvent(MotionEvent.ACTION_UP, listOf(a))
        waitState { it.getJSONObject("camera").number("zoom") > initial }
        assertFalse("Finger navigation must not deposit paint", state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled"))
        compose.runOnIdle { host.invoke("fit_canvas") }
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(a), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(b), MotionEvent.TOOL_TYPE_STYLUS, history = true)
        canvasEvent(MotionEvent.ACTION_CANCEL, listOf(b), MotionEvent.TOOL_TYPE_STYLUS)
        val completed = CountDownLatch(1)
        var sampleCount = 0L
        host.measurements { data ->
            sampleCount = data.array("inputs").values().maxOf { (it as org.json.JSONArray).getLong(4) }; completed.countDown()
        }
        assertTrue(completed.await(10, TimeUnit.SECONDS))
        assertTrue("Coalesced historical samples cross JNI together", sampleCount >= 2)
        // Backgrounding destroys SurfaceView, not the Rust document/device.
        penStroke()
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        fun cameraGeometry() = JSONObject(state().getJSONObject("camera").toString()).apply { remove("revision") }.toString()
        val camera = cameraGeometry()
        compose.activityRule.scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
        compose.activityRule.scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        compose.waitForIdle()
        assertNull(host.failure)
        assertEquals(camera, cameraGeometry())
        penStroke(20)
        assertNull(host.failure)
        capture("15-surface-recovery")
        assertTrue(instrumentation.uiAutomation.setRotation(android.app.UiAutomation.ROTATION_FREEZE_90))
        compose.waitUntil(10_000) { compose.activity.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT }
        capture("16-workspace-portrait")
        compose.onNodeWithContentDescription("Settings").performClick()
        capture("17-settings-portrait")
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        compose.onNodeWithText("Prediction horizon (ms)").assertExists()
        capture("29-settings-portrait-detail")
        compose.onNodeWithContentDescription("Back").performClick()
        compose.onNodeWithText("Canvas").assertExists()
        capture("30-settings-portrait-back")
        assertTrue(instrumentation.uiAutomation.setRotation(android.app.UiAutomation.ROTATION_FREEZE_0))
    }

    @Test fun menusCursorChoicesAndAboutUseCoreMetadata() {
        compose.onNodeWithText("View").performClick()
        capture("18-view-menu")
        compose.onNodeWithText("Fit canvas").performClick()
        compose.onNodeWithText("Fit canvas").assertDoesNotExist()
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        capture("19-canvas-settings")
        val row = preferences().array("pages").objects().first { it.getString("id") == "canvas" }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getJSONObject("kind").getString("type") == "choice" }
        val kind = row.getJSONObject("kind")
        compose.onNodeWithTag("preference-${row.getString("id")}").performScrollTo().performClick()
        capture("20-cursor-choices")
        val selection = (kind.getInt("selected") + 1) % kind.array("options").length()
        compose.onNode(hasText(kind.array("options").getString(selection)) and hasClickAction() and hasAnyAncestor(hasTestTag("preferences-surface"))).performClick()
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onNodeWithText("About").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "about" }
        compose.onNodeWithText("capycanvas.art").assertExists()
        compose.onNodeWithText("github.com/capyatelier/capycanvas").assertExists()
        capture("21-about")
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertNull(host.actionError)
    }

    @Test fun palmDoesNotPanWhilePenDrawsAndEraserWorks() {
        val pen = androidx.compose.ui.geometry.Offset(0.45f, 0.5f)
        val palm = androidx.compose.ui.geometry.Offset(0.6f, 0.6f)
        val camera = state().getJSONObject("camera").toString()
        val tools = listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f), palm + androidx.compose.ui.geometry.Offset(0.05f, 0.05f)), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        assertEquals("Palm contact must not move the camera", camera, state().getJSONObject("camera").toString())
        val painted = darkPixels(capture("22-pen-with-palm"))
        assertTrue("Pen still deposits pigment during palm contact", painted > 100)
        compose.runOnIdle { host.dispatch(obj("type" to "set_brush_size", "value" to 64)) }
        waitState { it.getJSONObject("brush").number("diameter") == 64f }
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f)), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        assertTrue("The eraser removes deposited pigment", darkPixels(capture("23-eraser")) < painted / 5)
        assertNull(host.failure)
    }

    @Test fun zenKeepsChromeThroughDrawerDismissalAndPanelDrag() {
        compose.onNodeWithContentDescription("Zen mode").performClick()
        waitState { it.getJSONObject("workspace").getBoolean("zen_mode") }
        val edge = androidx.compose.ui.geometry.Offset(0.01f, 0.01f)
        val center = androidx.compose.ui.geometry.Offset(0.7f, 0.6f)
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        compose.onAllNodesWithText("Brushes").onFirst().performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "brushes" }
        compose.waitForIdle()
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.getJSONObject("customization").isNull("expanded") }
        assertFalse("First outside contact closes only the drawer", host.snapshot!!.getBoolean("chrome_hidden"))
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        val source = compose.onAllNodesWithText("Brushes").onFirst()
        val target = compose.onAllNodesWithText("Layers").onFirst().fetchSemanticsNode().boundsInRoot.center
        val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
        source.performTouchInput { swipe(this.center, target - origin, 700) }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers"))
        } }
        assertFalse("Dropping a panel keeps its workspace visible", host.snapshot!!.getBoolean("chrome_hidden"))
        capture("24-zen-after-drag")
        compose.runOnIdle { host.invoke("zen_mode") }
    }
}
