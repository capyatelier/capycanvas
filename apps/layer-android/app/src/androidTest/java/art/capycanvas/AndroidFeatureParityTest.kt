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
        if (value.optString("command").contains("document")) compose.activity.getExternalFilesDir(null)!!.resolve("parity-document-debug.json").writeText(state().toString(2))
        if (value.optString("command").contains("document")) compose.waitUntil(60_000) {
            state().array("commands").objects().first { it.getString("id") == value.getString("command") }.getBoolean("enabled")
        }
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
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "commands", "visible" to false)))
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
        try { compose.waitUntil(20_000) { predicate(pixel(point)) || host.failure != null || host.actionError != null } }
        catch (e: Throwable) {
            capture("pixel-failure")
            compose.activity.getExternalFilesDir(null)!!.resolve("parity-pixel-debug.json").writeText(state().toString(2))
            throw e
        }
        assertNull(host.failure); assertNull(host.actionError); assertTrue(predicate(pixel(point)))
    }
    @Test fun toolDrawersOpenInZenAndOutsideContactDoesNotPaint() {
        action(obj("type" to "invoke", "command" to "pen"))
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
        action(obj("type" to "invoke", "command" to "pen"))
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
    private fun awaitNavigatorPixel(predicate: (Int) -> Boolean) {
        compose.waitUntil(15_000) {
            val bounds = compose.onNodeWithTag("navigator-overview").fetchSemanticsNode().boundsInRoot
            val screenshot = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()!!
            val image = screenshot.copy(Bitmap.Config.ARGB_8888, false)
            try { (bounds.left.toInt() until bounds.right.toInt() step 2).any { x ->
                (bounds.top.toInt() until bounds.bottom.toInt() step 2).any { y -> predicate(image.getPixel(x,y)) }
            } } finally { image.recycle(); screenshot.recycle() }
        }
        assertNull(host.failure)
    }
    @Test fun fullEditorPresetShowsToolsCommandsAndContentPanels() {
        for (id in listOf("toolbar", "commands", "brushes", "tool_settings", "sizes", "color", "navigator", "properties", "layers")) {
            assertTrue("Default preset includes $id", host.snapshot!!.array("panels").objects().any { it.getString("id") == id })
        }
        val commands = host.snapshot!!.array("panels").objects().first { it.getString("id") == "commands" }
        commands.array("tiles").objects().filter { it.getJSONObject("control").getString("kind") != "divider" }.forEach { compose.onNodeWithTag("tile-commands-${it.getInt("id")}").assertExists() }
        compose.onNodeWithTag("navigator-zoom_in").assertIsDisplayed()
        compose.onNodeWithTag("color-wheel").assertIsDisplayed()
        capture("full-editor-preset")
    }
    @Test fun resetLayoutKeepsTabDraggingDockingAndSelectionUsable() {
        fun group(panel: String) = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { it.array("panels").values().contains(panel) }
        fun drag(tag: String, target: Offset) {
            val source = compose.onNodeWithTag(tag)
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(center, target - origin, 700) }
        }
        // Cover both an existing saved workspace and the new editor preset.
        for (workspace in listOf(savedWorkspace, defaultWorkspace)) {
            val input = JSONObject(workspace.toString()).put("zen_mode", false)
            // Reset preserves tab appearance. Exercise a visible tab even if
            // the saved workspace previously floated or explicitly hid it.
            input.getJSONObject("layout").array("panels").objects().first { it.getString("id") == "tool_settings" }.put("hide_tab", false)
            action(obj("type" to "restore_workspace", "workspace" to input))
            val oldGroup = group("tool_settings").getInt("id")
            compose.onNodeWithTag("application-menu-view").performClick()
            compose.onNodeWithText("Reset layout").performClick()
            // Closing the native menu precedes the worker applying Reset.
            compose.waitUntil(10_000) { group("tool_settings").getInt("id") != oldGroup }
            shown("tab-tool_settings")
            assertFalse(group("tool_settings").getBoolean("floating"))
            val canvas = compose.onNodeWithTag("workspace").fetchSemanticsNode().boundsInRoot
            // Inject a real contact/drag on the tab, including native chrome
            // hit-testing; a semantics-only click bypasses the faulty path.
            drag("tab-tool_settings", canvas.center)
            compose.waitUntil(10_000) { host.actionError != null || group("tool_settings").getBoolean("floating") }
            assertNull(host.actionError)
            assertTrue(group("tool_settings").getBoolean("floating"))
            val grip = "group-grip-${group("tool_settings").getInt("id")}"
            drag(grip, compose.onNodeWithTag("tab-layers").fetchSemanticsNode().boundsInRoot.center)
            compose.waitUntil(10_000) { host.actionError != null || group("layers").array("panels").values().contains("tool_settings") }
            assertNull(host.actionError)
            assertTrue(group("layers").array("panels").values().contains("tool_settings"))
            for (panel in listOf("layers", "tool_settings")) {
                compose.onNodeWithTag("tab-$panel").performTouchInput { click() }
                compose.waitUntil(10_000) { host.actionError != null || group(panel).getString("active") == panel }
                assertNull(host.actionError)
                assertEquals(panel, group(panel).getString("active"))
            }
            assertNull(host.failure)
        }
        capture("reset-layout-tab-drag")
    }
    private fun newSmallDocument() {
        action(obj("type" to "invoke", "command" to "new_document"))
        shown("new-document-width")
        compose.onNodeWithTag("new-document-width").performTextReplacement("512")
        compose.onNodeWithTag("new-document-height").performTextReplacement("384")
        compose.onNodeWithTag("new-document-create").performClick()
        compose.waitUntil(60_000) { state().array("tabs").getJSONObject(0).getInt("width") == 512 }
        awaitDocument()
    }
    private fun documentPoint(x: Float, y: Float): Offset {
        val camera = state().getJSONObject("camera")
        val t = camera.array("translation"); val v = camera.array("viewport"); val z = camera.number("zoom")
        return Offset((t.getDouble(0).toFloat() + x*z)/v.getDouble(0).toFloat(), (t.getDouble(1).toFloat() + y*z)/v.getDouble(1).toFloat())
    }
    private fun editToolNumber(label: String, text: String) {
        if (compose.onAllNodesWithTag("number-value-$label").fetchSemanticsNodes().isNotEmpty()) compose.onNodeWithTag("number-value-$label").performScrollTo().performClick()
        val field = compose.onNodeWithTag("number-$label").performScrollTo()
        field.performTextReplacement(text)
        field.performImeAction()
        compose.waitForIdle()
    }
    @Test fun regionEdgeControlsCloseGapsForFillAndAutoSelect() {
        newSmallDocument()
        val rgba = ByteArray(512*384*4) { -1 }
        for (y in 80..300) for (x in 100..400) {
            val edge = x < 106 || x > 394 || y < 86 || y > 294
            val gap = y < 86 && x in 244..251
            if (edge && !gap) { val i=(y*512+x)*4; rgba[i]=0; rgba[i+1]=0; rgba[i+2]=0 }
        }
        compose.runOnIdle { host.importLayer("Gap fixture",512,384,rgba) }
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1,.2,.9,1))))
        val inside = documentPoint(260f,190f); val outside = documentPoint(440f,190f)
        awaitPixel(documentPoint(102f,190f)) { android.graphics.Color.red(it) < 80 }
        action(obj("type" to "move_panel", "panel" to "tool_settings", "target" to obj("kind" to "float", "position" to JSONArray(listOf(10,90))), "viewport" to viewport()))
        action(obj("type" to "invoke", "command" to "fill"))
        stroke(inside)
        awaitPixel(outside) { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
        action(obj("type" to "invoke", "command" to "undo"))
        awaitPixel(outside) { android.graphics.Color.red(it) > 245 }
        editToolNumber("Close gaps", "12")
        editToolNumber("Expansion", "2")
        editToolNumber("Edge smoothing", "100")
        stroke(inside)
        awaitPixel(documentPoint(270f,190f)) { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
        assertTrue(android.graphics.Color.red(pixel(outside)) > 245)
        capture("gap-closed-fill")
        action(obj("type" to "invoke", "command" to "undo"))
        action(obj("type" to "invoke", "command" to "auto_select"))
        for (id in listOf("gap_closing","expansion","smoothing")) compose.onNodeWithTag("tool-setting-$id").assertExists()
        stroke(inside)
        compose.waitUntil(10_000) { state().array("commands").objects().first { it.getString("id") == "fill_selection" }.getBoolean("enabled") }
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.9,.1,.15,1))))
        action(obj("type" to "invoke", "command" to "fill_selection"))
        awaitPixel(documentPoint(270f,190f)) { android.graphics.Color.red(it) > 150 && android.graphics.Color.blue(it) < 80 }
        assertTrue(android.graphics.Color.red(pixel(outside)) > 245)
        capture("gap-closed-auto-select")
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
        awaitNavigatorPixel { android.graphics.Color.red(it) > 150 && android.graphics.Color.green(it) < 80 }
        // The overview must update while contact remains down, with no idle
        // readback/polling window between the canvas and Navigator.
        action(obj("type" to "invoke", "command" to "pen"))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .2, .9, 1))))
        canvasEvent(android.view.MotionEvent.ACTION_DOWN, Offset(.5f,.7f))
        canvasEvent(android.view.MotionEvent.ACTION_MOVE, Offset(.65f,.7f))
        awaitNavigatorPixel { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
        canvasEvent(android.view.MotionEvent.ACTION_UP, Offset(.65f,.7f))
        compose.waitUntil(10_000) { state().array("commands").objects().first { it.getString("id") == "zoom_in" }.getBoolean("enabled") }
        compose.waitForIdle()
        val zoom = state().getJSONObject("camera").number("zoom")
        compose.onNodeWithTag("navigator-zoom_in").performClick()
        compose.waitUntil(10_000) { state().getJSONObject("camera").number("zoom") > zoom }
        val before = state().getJSONObject("camera").toString()
        compose.onNodeWithTag("navigator-overview").performTouchInput { swipe(center, center + Offset(40f,20f), 300) }
        compose.waitUntil(10_000) { state().getJSONObject("camera").toString() != before }
        capture("navigator")
    }

    @Test fun drawingToolsRenderMoveFillGradientAndRulers() {
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, .2, 1, 1))))
        action(obj("type" to "set_brush_opacity", "value" to 1))
        action(obj("type" to "invoke", "command" to "figure"))
        action(state().getJSONObject("tool_set").array("groups").objects().first { it.getString("label") == "Rectangle" }.getJSONObject("action"))
        action(state().getJSONObject("tool_set").array("subtools").objects().first { it.getString("label") == "Fill" }.getJSONObject("action"))
        stroke(Offset(.45f,.4f), Offset(.58f,.65f))
        awaitPixel(Offset(.5f,.5f)) { android.graphics.Color.blue(it) > 220 && android.graphics.Color.red(it) < 30 }
        capture("figure")
        action(obj("type" to "invoke", "command" to "move"))
        stroke(Offset(.5f,.5f), Offset(.66f,.5f))
        awaitPixel(Offset(.68f,.55f)) { android.graphics.Color.blue(it) > 220 && android.graphics.Color.red(it) < 30 }
        awaitPixel(Offset(.5f,.5f)) { android.graphics.Color.red(it) > 245 }
        action(obj("type" to "invoke", "command" to "undo"))
        awaitPixel(Offset(.5f,.5f)) { android.graphics.Color.red(it) < 30 }
        action(obj("type" to "invoke", "command" to "fill"))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(1, .1, .1, 1))))
        stroke(Offset(.5f,.5f))
        awaitPixel(Offset(.53f,.55f)) { android.graphics.Color.red(it) > 220 && android.graphics.Color.blue(it) < 60 }
        awaitPixel(Offset(.65f,.5f)) { android.graphics.Color.green(it) > 245 }
        capture("fill")
        action(obj("type" to "invoke", "command" to "gradient"))
        action(state().getJSONObject("tool_set").array("subtools").getJSONObject(0).getJSONObject("action"))
        stroke(Offset(.42f,.5f), Offset(.68f,.5f))
        awaitPixel(Offset(.45f,.75f)) { android.graphics.Color.red(it) > 220 && android.graphics.Color.green(it) < 100 }
        assertTrue(android.graphics.Color.green(pixel(Offset(.65f,.75f))) > 150)
        capture("gradient")
        action(obj("type" to "invoke", "command" to "undo"))
        action(obj("type" to "invoke", "command" to "ruler"))
        stroke(Offset(.42f,.7f), Offset(.65f,.7f))
        compose.waitUntil(10_000) { state().array("commands").objects().first { it.getString("id") == "delete_ruler" }.getBoolean("enabled") }
        capture("ruler")
        compose.waitUntil(10_000) {
            val screen = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()!!
            val pixels = screen.copy(Bitmap.Config.ARGB_8888, false)
            try {
                ((pixels.width * .52f).toInt()..(pixels.width * .58f).toInt()).any { x ->
                    ((pixels.height * .69f).toInt()..(pixels.height * .71f).toInt()).any { y ->
                        val p = pixels.getPixel(x,y)
                        android.graphics.Color.red(p) < 235 || android.graphics.Color.green(p) < 235
                    }
                }
            } finally { pixels.recycle(); screen.recycle() }
        }
        action(obj("type" to "invoke", "command" to "delete_ruler"))
        assertFalse(state().array("commands").objects().first { it.getString("id") == "delete_ruler" }.getBoolean("enabled"))
    }
    private fun request(): JSONObject? = state().array("requests").objects().firstOrNull { it.getJSONObject("kind").getString("type") == "document" }
    private fun awaitDocument() {
        compose.waitUntil(60_000) { !state().getJSONObject("document_file").optBoolean("busy") && !host.documents.working }
        assertNull(state().opt("host_error").takeIf { it != JSONObject.NULL })
        assertNull(host.failure); assertNull(host.actionError)
    }
    private fun systemNode(predicate: (android.view.accessibility.AccessibilityNodeInfo) -> Boolean): android.view.accessibility.AccessibilityNodeInfo? {
        fun find(node: android.view.accessibility.AccessibilityNodeInfo?): android.view.accessibility.AccessibilityNodeInfo? {
            node ?: return null
            if (predicate(node)) return node
            for (i in 0 until node.childCount) find(node.getChild(i))?.let { return it }
            return null
        }
        return find(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
    }
    private fun chooseSaveFile(name: String) {
        // Exercise Android's real DocumentsUI create picker and URI grant result.
        val until = android.os.SystemClock.uptimeMillis() + 15_000
        var field: android.view.accessibility.AccessibilityNodeInfo? = null
        while (field == null && android.os.SystemClock.uptimeMillis() < until) {
            field = systemNode { it.isEditable && it.packageName?.toString()?.contains("documentsui") == true }
            if (field == null) android.os.SystemClock.sleep(100)
        }
        assertNotNull("DocumentsUI filename field", field)
        assertTrue(field!!.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT, android.os.Bundle().apply {
            putCharSequence(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, name)
        }))
        val save = systemNode { it.isClickable && it.text?.toString()?.equals("save", ignoreCase = true) == true }
        assertNotNull("DocumentsUI Save", save)
        assertTrue(save!!.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
    }
    @Test fun documentSafSaveReopenAndExportPreservePaint() {
        compose.waitUntil(60_000) { state().array("commands").objects().first { it.getString("id") == "new_document" }.getBoolean("enabled") }
        compose.onNodeWithTag("application-menu-file").performClick()
        compose.onNodeWithText("New…").performClick()
        shown("new-document-width")
        compose.onNodeWithTag("new-document-width").performTextReplacement("512")
        compose.onNodeWithTag("new-document-height").performTextReplacement("384")
        compose.onNodeWithTag("new-document-create").performClick()
        compose.waitUntil(60_000) { state().array("tabs").getJSONObject(0).getInt("width") == 512 }
        awaitDocument()
        action(obj("type" to "invoke", "command" to "pen"))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .25, .9, 1))))
        action(obj("type" to "set_brush_size", "value" to 32))
        stroke(Offset(.5f,.55f), Offset(.62f,.55f))
        awaitPixel(Offset(.56f,.55f)) { android.graphics.Color.blue(it) > 160 && android.graphics.Color.red(it) < 80 }
        val name = "capy-android-parity-${System.currentTimeMillis()}.capy"
        action(obj("type" to "invoke", "command" to "save_document"))
        chooseSaveFile(name)
        awaitDocument()
        val location = state().getJSONObject("document_file").getJSONObject("location")
        assertEquals(name, location.getString("name"))
        assertFalse(state().getJSONObject("document_file").getBoolean("modified"))
        val uri = android.net.Uri.parse(location.getString("uri"))
        val resolver = compose.activity.contentResolver
        val copy = compose.activity.getExternalFilesDir(null)!!.resolve(name)
        resolver.openInputStream(uri)!!.use { input -> copy.outputStream().use { input.copyTo(it) } }
        assertTrue(copy.length() > 100)
        // Reopen through the production candidate preparation/adoption boundary.
        action(obj("type" to "invoke", "command" to "new_document"))
        shown("new-document-create")
        compose.onNodeWithTag("new-document-create").performClick()
        compose.waitUntil(60_000) { state().array("tabs").getJSONObject(0).getInt("width") == 2048 }
        awaitDocument()
        action(obj("type" to "invoke", "command" to "open_document"))
        val pickerDeadline = android.os.SystemClock.uptimeMillis() + 10_000
        while (systemNode { it.packageName?.toString()?.contains("documentsui") == true } == null && android.os.SystemClock.uptimeMillis() < pickerDeadline) android.os.SystemClock.sleep(100)
        var saved = systemNode { it.text?.toString() == name }
        if (saved == null) {
            val recent = systemNode { it.isClickable && it.text?.toString() == "Recent" }
            recent?.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK)
            android.os.SystemClock.sleep(300)
            saved = systemNode { it.text?.toString() == name }
        }
        assertNotNull("Saved project appears in Android picker", saved)
        var clickable = saved!!
        while (!clickable.isClickable && clickable.parent != null) clickable = clickable.parent
        assertTrue(clickable.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
        compose.waitUntil(60_000) { state().array("tabs").getJSONObject(0).getInt("width") == 512 }
        awaitDocument()
        awaitPixel(Offset(.56f,.55f)) { android.graphics.Color.blue(it) > 160 && android.graphics.Color.red(it) < 80 }
        capture("document-reopened")
        action(obj("type" to "invoke", "command" to "export_document"))
        chooseSaveFile(name.removeSuffix(".capy") + ".png")
        awaitDocument()
        // Verify the exported PNG from its real persisted provider grant and
        // remove only this test's uniquely named public files.
        val pngName = name.removeSuffix(".capy") + ".png"
        val exported = resolver.persistedUriPermissions.map { it.uri }.firstOrNull { candidate ->
            resolver.query(candidate, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                it.moveToFirst() && it.getString(0) == pngName
            } == true
        }
        assertNotNull("Export returned a persistent file grant", exported)
        val pngCopy = compose.activity.getExternalFilesDir(null)!!.resolve(pngName)
        resolver.openInputStream(exported!!)!!.use { input -> pngCopy.outputStream().use { input.copyTo(it) } }
        val png = android.graphics.BitmapFactory.decodeFile(pngCopy.absolutePath)
        assertEquals(512, png.width); assertEquals(384, png.height)
        assertTrue((0 until png.width step 4).any { x -> (0 until png.height step 4).any { y ->
            val c = png.getPixel(x,y); android.graphics.Color.blue(c) > 160 && android.graphics.Color.red(c) < 80
        } })
        png.recycle()
        android.provider.DocumentsContract.deleteDocument(resolver, exported)
        android.provider.DocumentsContract.deleteDocument(resolver, uri)
    }
    @Test fun newDocumentCancellationKeepsUnsavedCanvas() {
        action(obj("type" to "invoke", "command" to "pen"))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(1,0,0,1))))
        action(obj("type" to "set_brush_size", "value" to 160))
        stroke(Offset(.5f,.5f), Offset(.6f,.5f))
        awaitPixel(Offset(.55f,.5f)) { android.graphics.Color.green(it) < 50 }
        val epoch = state().getJSONObject("document_file").getLong("epoch")
        action(obj("type" to "invoke", "command" to "new_document"))
        shown("document-close-cancel")
        compose.onNodeWithTag("document-close-cancel").performClick()
        awaitDocument()
        assertEquals(epoch, state().getJSONObject("document_file").getLong("epoch"))
        assertTrue(state().getJSONObject("document_file").getBoolean("modified"))
        awaitPixel(Offset(.55f,.5f)) { android.graphics.Color.green(it) < 50 }
    }

}
