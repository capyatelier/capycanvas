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
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.text.TextLayoutResult
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
        compose.onNodeWithText("Search settings").performTextInput("prediction")
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").array("search_results").length() > 0 }
        capture("05-settings-search")
        compose.onNodeWithContentDescription("Clear search").performClick()
        compose.waitUntil(10_000) { preferences().getString("query").isEmpty() && !preferences().getBoolean("searching") }
        compose.onNodeWithText("Search settings").assertExists()
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

            compose.onNodeWithTag("setting-choice-theme").performClick()
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

    @Test fun settingsPanesShareTopEdgeAndUseAppScale() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        for (theme in listOf("light", "dark")) {
            compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
            waitState { it.getString("theme") == theme }
            fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val surface = bounds("preferences-surface")
            val sidebar = bounds("settings-sidebar")
            val main = bounds("settings-main-pane")
            assertEquals("Sidebar reaches the top; no global header", surface.top, sidebar.top, 1f)
            assertEquals("Sidebar reaches the bottom", surface.bottom, sidebar.bottom, 1f)
            assertEquals("Panes start at the same height", sidebar.top, main.top, 1f)
            assertEquals("Panes are adjacent", sidebar.right, main.left, 1f)
            val search = bounds("settings-search")
            compose.onNodeWithTag("settings-sidebar-title").assertDoesNotExist()
            assertEquals("Sidebar glyphs share a center line", bounds("settings-search-icon").center.x,
                bounds("settings-category-icon-appearance").center.x, 1f)
            assertTrue("Persistent search occupies the sidebar top", search.top < bounds("settings-category-appearance").top)
            compose.onNodeWithTag("settings-category-appearance").assertHeightIsEqualTo(48.dp)
            compose.onNodeWithTag("settings-category-icon-appearance", useUnmergedTree = true).assertWidthIsEqualTo(20.dp).assertHeightIsEqualTo(20.dp)
            compose.onNodeWithTag("settings-search").assertHeightIsEqualTo(48.dp)
            compose.onNodeWithTag("settings-done").assertHeightIsEqualTo(40.dp).assertTouchHeightIsEqualTo(48.dp)
            assertTrue("Done belongs to the main pane", bounds("settings-done").left >= main.left)
            val text = mutableListOf<TextLayoutResult>()
            compose.onNodeWithTag("settings-category-label-appearance", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Sidebar uses native settings body typography", 16f, text.single().layoutInput.style.fontSize.value, .01f)
            text.clear()
            compose.onNodeWithText("Done", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Done text is proportionate to its 40 dp surface", 16f, text.single().layoutInput.style.fontSize.value, .01f)
            text.clear()
            compose.onNodeWithTag("settings-group-title-Interface", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Group headings are larger than body copy", 18f, text.single().layoutInput.style.fontSize.value, .01f)
            val button = compose.onNodeWithTag("settings-done").captureToImage().toPixelMap()
            val fill = button[button.width / 2, button.height / 4]
            assertTrue("Done has a visible filled surface, not just colored text", fill.blue > fill.red + .2f)
            capture("36-settings-two-panes-$theme")
        }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun typingFromSettingsFocusesSearchWithoutLosingCharacters() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("settings-category-about").performClick()
        compose.waitForIdle()
        // Send a burst before Compose can transfer focus; Rust must retain it
        // and the field must put its caret after the complete query.
        instrumentation.runOnMainSync {
            for ((code, meta) in listOf(KeyEvent.KEYCODE_P to KeyEvent.META_SHIFT_ON, KeyEvent.KEYCODE_R to 0)) {
                for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
                    compose.activity.dispatchKeyEvent(KeyEvent(0, 0, action, code, 0, meta))
                }
            }
        }
        compose.waitUntil(10_000) { preferences().optString("query") == "Pr" }
        val search = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("settings-search")))
        search.assertTextEquals("Pr").assertIsFocused()
        search.performTextInput("essure")
        compose.waitUntil(10_000) { preferences().optString("query") == "Pressure" }
        compose.onNodeWithText("Pressure response", substring = true).assertExists()
        capture("38-type-to-search")
        compose.onNodeWithContentDescription("Clear search").performClick()
        compose.onNodeWithTag("settings-category-input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        compose.waitForIdle()
        val number = compose.onNodeWithTag("setting-number-prediction_horizon").performScrollTo()
        number.performTextReplacement("12")
        instrumentation.runOnMainSync {
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
                compose.activity.dispatchKeyEvent(KeyEvent(action, KeyEvent.KEYCODE_3))
        }
        compose.waitForIdle()
        assertEquals("Number editing stays local", "", preferences().optString("query"))
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun baseColorsAreValidatedTextAndDriveTheNativePalette() {
        compose.onNodeWithContentDescription("Settings").performClick()
        for ((theme, color) in listOf("dark" to "#1C2C3C", "light" to "#C0B49C")) {
            compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
            waitState { it.getString("theme") == theme }
            compose.onNodeWithTag("settings-category-appearance").performClick()
            val input = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-" + theme + "_base")))
            input.performScrollTo().performTextReplacement("invalid")
            input.performImeAction()
            compose.waitUntil(10_000) { !preferences().isNull("error") }
            assertNotEquals("invalid", state().getJSONObject("settings").getString(theme + "_base"))
            input.performTextReplacement(color)
            input.performImeAction()
            waitState { it.getJSONObject("settings").getString(theme + "_base") == color.lowercase() }
            assertTrue(preferences().isNull("error"))
            assertEquals(color.lowercase(), state().getJSONObject("palette").getString("bg"))
            val image = compose.onNodeWithTag("preferences-surface").captureToImage().toPixelMap()
            val expected = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("settings"))
            val pixel = image[image.width - 2, image.height - 2]
            assertEquals(android.graphics.Color.red(expected) / 255f, pixel.red, .01f)
            assertEquals(android.graphics.Color.green(expected) / 255f, pixel.green, .01f)
            assertEquals(android.graphics.Color.blue(expected) / 255f, pixel.blue, .01f)
            capture("40-custom-base-$theme")
        }
        compose.runOnIdle {
            host.preference(obj("type" to "edit", "id" to "dark_base", "value" to "#333333"))
            host.preference(obj("type" to "edit", "id" to "light_base", "value" to "#b8b8b8"))
        }
        waitState { it.getJSONObject("settings").getString("light_base") == "#b8b8b8" }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun inlineSettingsApplyValidateAndNeverPaintUnderneath() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.onNodeWithTag("preference-prediction_horizon").performScrollTo()
        assertNull("Numbers edit directly in their row", preferences().objectOrNull("detail"))
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        val slider = compose.onNodeWithTag("setting-slider-pressure").performScrollTo().assertTouchHeightIsEqualTo(48.dp)
        val track = slider.captureToImage().toPixelMap()
        val trackX = track.width * 9 / 10
        assertTrue("Inactive slider track remains visible on the light settings surface",
            track[trackX, track.height / 4].red - track[trackX, track.height / 2].red > .05f)
        capture("32-inline-numbers")
        val before = state().getJSONObject("settings").number("prediction_ms")
        compose.onNodeWithTag("setting-number-prediction_horizon").performTextReplacement("1/0")
        compose.onNodeWithTag("setting-number-prediction_horizon").performImeAction()
        compose.onNodeWithText("Enter a finite number", substring = true).assertExists()
        assertEquals(before, state().getJSONObject("settings").number("prediction_ms"))
        capture("33-number-invalid")
        slider.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(width * .75f, center.y), 300) }
        compose.waitForIdle()
        waitState { it.getJSONObject("settings").number("pressure_gamma") != 1f }
        val dragged = state().getJSONObject("settings").number("pressure_gamma")
        assertTrue("A slider drag changes the value inside its range ($dragged)", dragged in .25f..4f)
        compose.onNodeWithTag("setting-number-prediction_horizon").performTextReplacement("32*2")
        compose.onNodeWithTag("setting-number-prediction_horizon").performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 64f }
        assertTrue(preferences().isNull("error"))
        compose.onNodeWithTag("setting-number-prediction_horizon").assertTextEquals("64 ms")
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
        compose.onNodeWithTag("setting-number-prediction_horizon").assertTextEquals("64 ms")
        // Return this shared preference to its original accepted value.
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "prediction_horizon", "value" to before)) }
        waitState { it.getJSONObject("settings").number("prediction_ms") == before }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun settingDefaultsResetFromContextAndEmptyCommits() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("settings-category-appearance").performClick()
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "dark_base", "value" to "#224466")) }
        waitState { it.getJSONObject("settings").getString("dark_base") == "#224466" }
        val label = compose.onNodeWithTag("preference-label-dark_base", useUnmergedTree = true).performScrollTo()
        label.performTouchInput { longClick() }
        compose.onNodeWithTag("preference-reset").assertIsEnabled()
        compose.onNodeWithText("#333333").assertExists()
        capture("36-setting-reset")
        compose.onNodeWithTag("preference-reset").performClick()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#333333" }
        label.performTouchInput { longClick() }
        compose.onNodeWithTag("preference-reset").assertIsNotEnabled()
        instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
        val text = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-dark_base")))
        text.performTextReplacement("#335577")
        text.performImeAction()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#335577" }
        text.performTextReplacement("")
        assertEquals("#335577", state().getJSONObject("settings").getString("dark_base"))
        text.performImeAction()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#333333" }
        compose.onNodeWithText("Pen & Input").performClick()
        val number = compose.onNodeWithTag("setting-number-prediction_horizon").performScrollTo()
        number.performTextReplacement("32"); number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 32f }
        number.performTextReplacement("")
        assertEquals(32f, state().getJSONObject("settings").number("prediction_ms"))
        number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 8f }
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertNull(host.actionError)
    }

    @Test fun allPreferenceRowsRenderCoreMetadataAndTrailingControls() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        // Iterate the actual core catalog: new rows using existing kinds are
        // automatically covered, with no duplicated IDs, defaults or ranges.
        for (page in preferences().array("pages").objects()) {
            if (page.array("groups").length() == 0) continue
            compose.onNodeWithTag("settings-category-" + page.getString("id")).performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == page.getString("id") }
            for (row in page.array("groups").objects().flatMap { it.array("rows").objects() }.filter { it.getBoolean("visible") }) {
                val id = row.getString("id")
                val node = compose.onNodeWithTag("preference-$id").performScrollTo()
                node.assert(hasAnyDescendant(hasText(row.getString("title"))) or hasText(row.getString("title")))
                row.optString("description").takeIf { it.isNotEmpty() }?.let {
                    compose.onNode(hasText(it) and hasAnyAncestor(hasTestTag("preference-$id")), useUnmergedTree = true).assertExists()
                }
                val kind = row.getJSONObject("kind")
                if (kind.getString("type") == "number") {
                    val label = compose.onNodeWithTag("preference-label-$id", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
                    val control = kind.getJSONObject("control")
                    val ranged = control.getString("kind") == "slider"
                    val field = compose.onNodeWithTag(if (ranged) "number-value-$id" else "setting-number-$id").assertHeightIsEqualTo(48.dp)
                    assertTrue("$id input is to the right of its description", field.fetchSemanticsNode().boundsInRoot.left > label.right)
                    if (ranged) {
                        val slider = compose.onNodeWithTag("setting-slider-$id").assertTouchHeightIsEqualTo(48.dp)
                        val bounds = slider.fetchSemanticsNode().boundsInRoot
                        assertTrue("$id slider is below all labels", bounds.top >= label.bottom)
                        val progress = slider.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo]
                        val formatted = JSONObject(Native.number(obj("control" to control, "value" to kind.number("value"), "operation" to obj("type" to "format")).toString()))
                        assertEquals(0f, progress.range.start); assertEquals(1f, progress.range.endInclusive)
                        assertEquals(formatted.number("fill"), progress.current)
                        val widthDp = bounds.width / compose.activity.resources.displayMetrics.density
                        assertTrue("$id slider uses available width with a 600dp control cap", widthDp in 150f..600f)
                    } else compose.onNodeWithTag("setting-slider-$id").assertDoesNotExist()
                }
            }
        }
        compose.onNodeWithTag("settings-category-input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        compose.onNodeWithTag("preference-feedback").performClick()
        waitState { !it.getJSONObject("settings").getBoolean("feedback") }
        compose.onNodeWithTag("setting-number-prediction_horizon").assertIsNotEnabled()
        compose.onNodeWithTag("setting-slider-tip_lock").assertIsNotEnabled()
        compose.onNodeWithTag("preference-feedback").performClick()
        waitState { it.getJSONObject("settings").getBoolean("feedback") }
        compose.onNodeWithTag("setting-number-prediction_horizon").assertIsEnabled()
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("37-inline-controls-dark")
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
        compose.onNodeWithTag("number-value-Brush size").assertHeightIsEqualTo(24.dp)
        compose.onNodeWithTag("number-slider-Brush size").assertHeightIsEqualTo(24.dp)
        val numericSlider = compose.onNodeWithTag("number-slider-Brush size")
        // Visual spacing excludes Compose's expanded minimum touch targets.
        val rangeBounds = numericSlider.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val minusBounds = compose.onNodeWithContentDescription("Decrease Brush size").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val plusBounds = compose.onNodeWithContentDescription("Increase Brush size").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val gap = with(compose.density) { 6.dp.toPx() }
        assertEquals(gap, rangeBounds.left - minusBounds.right, 1f)
        assertEquals(gap, plusBounds.left - rangeBounds.right, 1f)
        numericSlider.performTouchInput { swipe(center, centerRight, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") > 1000.0 }
        numericSlider.performTouchInput { swipe(center, centerLeft, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") < 2.0 }
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
        compose.onNodeWithTag("number-value-Brush size").performClick()
        val field = compose.onNodeWithTag("number-Brush size")
        field.performTextReplacement("85/2")
        field.performImeAction()
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f }
        compose.onNodeWithContentDescription("Increase Brush size").performClick()
        val step = host.catalog.getJSONObject("brush_size").number("step")
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f + step }
        compose.onNodeWithTag("number-value-Brush size").assertTextEquals("%.1f px".format(java.util.Locale.ROOT, 42.5f + step))
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
        compose.onNodeWithText("Prediction time").assertExists()
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
        compose.onNodeWithTag("setting-choice-${row.getString("id")}").performScrollTo().performClick()
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
        val hover = androidx.compose.ui.geometry.Offset(0.7f, 0.3f)
        val camera = state().getJSONObject("camera").toString()
        val tools = listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f), palm + androidx.compose.ui.geometry.Offset(0.05f, 0.05f)), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        assertEquals("Palm contact must not move the camera", camera, state().getJSONObject("camera").toString())
        // The cursor is presentation, not pigment: move it out of the sampled
        // area before both captures (its size changes for the eraser).
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(hover), MotionEvent.TOOL_TYPE_STYLUS)
        val painted = darkPixels(capture("22-pen-with-palm"))
        assertTrue("Pen still deposits pigment during palm contact", painted > 100)
        compose.runOnIdle { host.dispatch(obj("type" to "set_brush_size", "value" to 64)) }
        waitState { it.getJSONObject("brush").number("diameter") == 64f }
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f)), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(hover), MotionEvent.TOOL_TYPE_ERASER)
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
