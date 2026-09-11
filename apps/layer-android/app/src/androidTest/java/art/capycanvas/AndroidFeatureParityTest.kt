package art.capycanvas

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.toPixelMap
import androidx.test.platform.app.InstrumentationRegistry
import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.view.WindowManager
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Device coverage for native projections of the evolving shared GTK/core models. */
class AndroidFeatureParityTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    private fun state() = host.snapshot!!.getJSONObject("state")
    private lateinit var savedWorkspace: JSONObject
    private lateinit var savedSettings: JSONObject
    private lateinit var defaultWorkspace: JSONObject
    @Before fun ready() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("input keyevent KEYCODE_WAKEUP", "wm dismiss-keyguard")) {
            ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use { it.readBytes() }
        }
        compose.activity.runOnUiThread { compose.activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true || host.failure != null }
        assertNull(host.failure)
        savedWorkspace = JSONObject(state().getJSONObject("workspace").toString())
        savedSettings = JSONObject(state().getJSONObject("settings").toString())
        val native = Native.create(false)
        try { defaultWorkspace = JSONObject(Native.snapshot(native)!!).getJSONObject("state").getJSONObject("workspace") }
        finally { Native.destroy(native) }
        action(obj("type" to "close_settings"))
        action(obj("type" to "restore_workspace", "workspace" to defaultWorkspace))
        action(obj("type" to "restore_settings", "settings" to JSONObject(savedSettings.toString()).put("total_zen", false)))
    }
    @After fun restore() {
        if (::savedWorkspace.isInitialized) {
            action(obj("type" to "close_settings"))
            action(obj("type" to "restore_workspace", "workspace" to savedWorkspace))
            action(obj("type" to "restore_settings", "settings" to savedSettings))
        }
    }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        compose.runOnIdle { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(15, TimeUnit.SECONDS))
        compose.waitForIdle()
        assertNull(host.actionError)
        assertNull(host.failure)
    }
    private fun viewport(): JSONArray {
        val r = compose.onNodeWithTag("workspace").fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        return JSONArray(listOf(r.width / density, r.height / density))
    }
    private fun capture(name: String) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
        try {
            instrumentation.targetContext.getExternalFilesDir(null)!!.resolve("parity-$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        } finally { bitmap.recycle() }
    }
    @Test fun partialZenProjectsEdgeToolbarsAndPreservesLayout() {
        for (edge in listOf("left", "top", "right", "bottom")) {
            action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "edge", "edge" to edge, "outer" to true), "viewport" to viewport()))
            val before = state().getJSONObject("workspace").getJSONObject("layout").toString()
            action(obj("type" to "invoke", "command" to "zen_mode"))
            assertTrue(host.snapshot!!.getBoolean("partial_zen"))
            val sections = host.snapshot!!.getJSONObject("zen_toolbars").array("sections").objects()
            assertTrue(sections.isNotEmpty())
            sections.forEachIndexed { index, section ->
                compose.onNodeWithTag("zen-section-$index").assertIsDisplayed()
                assertEquals(edge, section.getString("edge"))
                section.array("tiles").values().forEach { pair ->
                    pair as JSONArray
                    compose.onNodeWithTag("tile-toolbar-${pair.getInt(0)}").assertIsDisplayed()
                }
            }
            assertEquals(before, state().getJSONObject("workspace").getJSONObject("layout").toString())
            capture("partial-zen-$edge")
            compose.onNodeWithTag("zen-button").performClick()
            compose.waitUntil(10_000) { !state().getJSONObject("workspace").getBoolean("zen_mode") }
            assertEquals(before, state().getJSONObject("workspace").getJSONObject("layout").toString())
        }
    }
    @Test fun toolSetAndSettingsFollowSelectedTool() {
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "tool_settings", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "tool_settings", "target" to obj("kind" to "float", "position" to JSONArray(listOf(480, 130))), "viewport" to viewport()))
        for (command in listOf("pen", "fill", "gradient", "figure", "ruler", "move")) {
            action(obj("type" to "invoke", "command" to command))
            val tools = state().getJSONObject("tool_set")
            for (item in tools.array("groups").objects()) compose.onNodeWithTag("tool-group-${item.getString("label")}").assertExists()
            val subtools = tools.array("subtools").objects()
            subtools.forEach { compose.onNodeWithTag("subtool-${it.getString("label")}").assertExists() }
            subtools.firstOrNull()?.let { item ->
                compose.onNodeWithTag("subtool-${item.getString("label")}").performScrollTo().performClick()
                compose.waitForIdle()
            }
            state().array("tool_settings").objects().forEach { field ->
                compose.onNodeWithTag("tool-setting-${field.getString("id")}").assertExists()
            }
            state().array("tool_actions").objects().forEach { item ->
                compose.onNodeWithTag("tool-action-${item.getString("command")}").assertExists()
            }
            capture("tool-$command")
            assertNull(host.actionError)
        }
    }
    @Test fun colorWheelPixelsMatchSharedPickedColorsAndSlots() {
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "color", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "color", "target" to obj("kind" to "float", "position" to JSONArray(listOf(440, 100))), "viewport" to viewport()))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(1, 0, 0, 1))))
        for (space in listOf("hsv", "hls")) {
            action(obj("type" to "color", "action" to obj("op" to "space", "space" to space)))
            val wheel = compose.onNodeWithTag("color-wheel").performScrollTo()
            val pixels = wheel.captureToImage().toPixelMap()
            val geometry = host.snapshot!!.getJSONObject("color_panel").getJSONObject("geometry")
            val position = if (space == "hsv") {
                val s = geometry.array("square")
                Offset((s.getDouble(0) + s.getDouble(2) * .3).toFloat(), (s.getDouble(1) + s.getDouble(2) * .3).toFloat())
            } else {
                val p = geometry.array("triangle").values().map { it as JSONArray }
                Offset(p.sumOf { it.getDouble(0) }.toFloat() / 3, p.sumOf { it.getDouble(1) }.toFloat() / 3)
            } * pixels.width.toFloat()
            val shown = pixels[position.x.toInt(), position.y.toInt()]
            wheel.performTouchInput { click(position) }
            action(obj("type" to "color", "action" to obj("op" to "space", "space" to space))) // Drain the input's shared action.
            val rgba = state().getJSONObject("colors").array("foreground")
            for ((index, value) in listOf(shown.red, shown.green, shown.blue).withIndex()) {
                assertEquals("$space wheel pixel matches picked component $index", value.toDouble(), rgba.getDouble(index), .035)
            }
            capture("color-$space")
        }
        compose.onNodeWithTag("color-swatch-background").performClick()
        compose.waitUntil(10_000) { state().getJSONObject("colors").getString("slot") == "background" }
        val before = state().getJSONObject("colors").array("foreground").toString()
        compose.onNodeWithTag("color-swap").performClick()
        compose.waitUntil(10_000) { state().getJSONObject("colors").array("background").toString() == before }
        compose.onNodeWithTag("color-swatch-transparent").performClick()
        compose.waitUntil(10_000) { state().getJSONObject("colors").getString("slot") == "transparent" }
        compose.onNodeWithTag("color-swatch-transparent").assertIsSelected()
        capture("color-transparent")
    }
    private fun shown(tag: String) {
        compose.waitUntil(15_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag(tag).assertIsDisplayed()
    }
    private fun canvasEvent(phase: Int, point: Offset) {
        fun find(view: android.view.View): CanvasSurfaceView? {
            if (view is CanvasSurfaceView) return view
            if (view is android.view.ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
            return null
        }
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val canvas = find(compose.activity.window.decorView)!!
            val properties = android.view.MotionEvent.PointerProperties().apply { id = 0; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS }
            val coords = android.view.MotionEvent.PointerCoords().apply { x = point.x * canvas.width; y = point.y * canvas.height; pressure = .8f }
            val now = android.os.SystemClock.uptimeMillis()
            val event = android.view.MotionEvent.obtain(now - 30, now, phase, 1, arrayOf(properties), arrayOf(coords), 0, 0, 1f, 1f, 1, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
            assertTrue(canvas.dispatchTouchEvent(event)); event.recycle()
        }
    }
    private fun stroke(from: Offset, to: Offset = from) {
        canvasEvent(android.view.MotionEvent.ACTION_DOWN, from)
        canvasEvent(android.view.MotionEvent.ACTION_MOVE, to)
        canvasEvent(android.view.MotionEvent.ACTION_UP, to)
    }
    private fun pixel(point: Offset): Int {
        val bitmap = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()!!
        val readable = bitmap.copy(Bitmap.Config.ARGB_8888, false)
        val color = readable.getPixel((point.x * readable.width).toInt(), (point.y * readable.height).toInt())
        readable.recycle(); bitmap.recycle(); return color
    }
    private fun awaitPixel(point: Offset, predicate: (Int) -> Boolean) {
        compose.waitUntil(20_000) { predicate(pixel(point)) || host.failure != null || host.actionError != null }
        assertNull(host.failure); assertNull(host.actionError); assertTrue(predicate(pixel(point)))
    }
    @Test fun toolDrawersOpenInZenAndOutsideContactDoesNotPaint() {
        action(obj("type" to "invoke", "command" to "brush"))
        action(obj("type" to "invoke", "command" to "zen_mode"))
        val pen = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects()
            .first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$pen").performTouchInput { click() }
        shown("tool-drawer")
        shown("tool-group-${state().getJSONObject("tool_set").array("groups").getJSONObject(0).getString("label")}")
        capture("zen-tool-drawer")
        val undoBefore = state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled")
        stroke(Offset(.6f, .8f), Offset(.7f, .8f))
        compose.waitUntil(10_000) { state().getJSONObject("customization").objectOrNull("drawer") == null }
        assertEquals(undoBefore, state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled"))
        awaitPixel(Offset(.65f,.8f)) { android.graphics.Color.red(it) > 245 }
    }
    @Test fun collapsedColumnOpensNestedToolDrawerAndExpandsWithoutLosingPanels() {
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.getString("active") == "brushes" }.getInt("id")
        action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "tab", "group" to group), "viewport" to viewport()))
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to true)))
        val column = host.snapshot!!.getJSONObject("layout").array("collapsed").objects().first { c -> c.array("groups").objects().any { it.getInt("group") == group } }.getInt("id")
        shown("column-icon-toolbar")
        compose.onNodeWithTag("column-icon-toolbar").performTouchInput { click() }
        shown("column-drawer-$column")
        val pen = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects()
            .first().getInt("id")
        action(obj("type" to "invoke", "command" to "brush"))
        shown("tile-toolbar-$pen")
        compose.onNodeWithTag("tile-toolbar-$pen").performTouchInput { click() }
        shown("tool-drawer")
        capture("nested-column-drawer")
        stroke(Offset(.75f,.8f))
        compose.waitUntil(10_000) { state().getJSONObject("customization").objectOrNull("drawer") == null }
        assertEquals(1, state().getJSONObject("customization").array("column_drawers").length())
        shown("column-drawer-$column")
        compose.onNodeWithTag("expand-column-$column").performTouchInput { click() }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("collapsed").objects().none { it.getInt("id") == column } }
        compose.onNodeWithTag("tile-toolbar-$pen").assertIsDisplayed()
    }
    @Test fun navigatorAndEyedropperUseActualGpuPixels() {
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.85, .12, .24, 1))))
        action(obj("type" to "set_brush_size", "value" to 180))
        action(obj("type" to "invoke", "command" to "pen"))
        stroke(Offset(.5f,.55f), Offset(.65f,.55f))
        awaitPixel(Offset(.575f,.55f)) { android.graphics.Color.red(it) > 150 && android.graphics.Color.green(it) < 80 }
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 1, 1))))
        action(obj("type" to "invoke", "command" to "eyedropper"))
        stroke(Offset(.575f,.55f))
        compose.waitUntil(15_000) { state().getJSONObject("colors").array("foreground").getDouble(0) > .7 || host.failure != null }
        val rgba = state().getJSONObject("colors").array("foreground")
        assertEquals(.85, rgba.getDouble(0), .04); assertEquals(.12, rgba.getDouble(1), .04); assertEquals(.24, rgba.getDouble(2), .04)
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "navigator", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "navigator", "target" to obj("kind" to "float", "position" to JSONArray(listOf(360, 100))), "viewport" to viewport()))
        shown("navigator-overview")
        compose.waitUntil(15_000) { host.navigatorImage != null }
        val pixels = host.navigatorImage!!.toPixelMap()
        assertTrue((0 until pixels.width).any { x -> (0 until pixels.height).any { y -> pixels[x,y].red > .6 && pixels[x,y].green < .3 } })
        val zoom = state().getJSONObject("camera").number("zoom")
        compose.onNodeWithTag("navigator-zoom_in").performClick()
        compose.waitUntil(10_000) { state().getJSONObject("camera").number("zoom") > zoom }
        val before = state().getJSONObject("camera").toString()
        compose.onNodeWithTag("navigator-overview").performTouchInput { swipe(center, center + Offset(40f,20f), 300) }
        compose.waitUntil(10_000) { state().getJSONObject("camera").toString() != before }
        capture("navigator")
    }

}
