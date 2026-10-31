package art.capycanvas

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.getOrNull
import android.graphics.Bitmap
import android.view.WindowManager
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Device coverage for native projections of the evolving shared GTK/core models. */
class AndroidFeatureParityTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    private fun state() = host.snapshot!!.getJSONObject("state")
    private lateinit var savedWorkspace: JSONObject
    private lateinit var savedSettings: JSONObject
    private lateinit var defaultWorkspace: JSONObject
    @Before fun ready() {
        wakeDevice()
        compose.activity.runOnUiThread { compose.activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        host.awaitReady(compose = compose)
        savedWorkspace = JSONObject(state().getJSONObject("workspace").toString())
        savedSettings = JSONObject(state().getJSONObject("settings").toString())
        val native = createEnglishHostForTest()
        try { defaultWorkspace = JSONObject(Native.snapshot(native)!!).getJSONObject("state").getJSONObject("workspace") }
        finally { Native.destroy(native) }
        action(obj("type" to "close_settings"))
        action(obj("type" to "restore_workspace", "workspace" to defaultWorkspace))
        action(obj("type" to "restore_settings", "settings" to JSONObject(savedSettings.toString())))
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
    private fun capture(name: String) = screenshot("parity-$name.png")
    @Test fun startupCanvasUsesCurrentDisplayInBothThemes() {
        val screen = screenInfo(androidx.core.content.ContextCompat.getDisplayOrDefault(compose.activity))!!
        val height = screen.getJSONArray("extent").getInt(1)
        val tab = state().array("tabs").objects().first()
        assertEquals(height, tab.getInt("height"))
        assertEquals(kotlin.math.round(height * 4.0 / 3.0).toInt(), tab.getInt("width"))
        assertEquals(9.0, state().getJSONObject("brush").getDouble("diameter"), 0.0)
        val rgba = state().getJSONObject("colors").getJSONObject("foreground").getJSONArray("rgba")
        for (channel in 0..2) assertEquals(46.0 / 255.0, rgba.getDouble(channel), 0.000001)
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            capture("startup-$theme")
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
                val tag = if (!item.isNull("group") && !item.optBoolean("segmented")) "tool-action-group-${item.getString("group")}" else "tool-action-${item.getString("command")}"
                compose.onNodeWithTag(tag).assertExists()
            }
            capture("tool-$command")
            assertNull(host.actionError)
        }
    }
    private fun tap(tag: String, tool: Int) {
        compose.onNodeWithTag(tag).performScrollTo()
        val (root, node) = findTag(tag)!!
        val at = node.boundsInRoot.center
        val downAt = android.os.SystemClock.uptimeMillis()
        for (phase in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
            val event = motion(tool, phase, at, downAt)
            try { instrumentation.runOnMainSync { root.view.dispatchTouchEvent(event) } }
            finally { event.recycle() }
            android.os.SystemClock.sleep(40)
        }
        compose.waitForIdle()
        host.drain()
        compose.waitForIdle()
    }
    @Test fun colorMixingChoicesAcrossDevicesPaintInEachSpace() {
        val choices = listOf("color_mix_oklab", "color_mix_linear", "color_mix_classic")
        fun command(id: String) = state().array("commands").objects().first { it.getString("id") == id }
        fun revision() = state().getJSONObject("document_file").getLong("revision")
        fun paint(from: Offset, to: Offset) {
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true || host.failure != null }
            assertNull(host.failure)
            val before = revision()
            stroke(from, to)
            compose.waitUntil(15_000) { revision() > before || host.failure != null }
            assertNull(host.failure)
        }
        action(obj("type" to "select_brush", "id" to 1))
        action(obj("type" to "set_brush_size", "value" to 120))
        for ((y, rgba) in listOf(.45f to listOf(.9, .05, .05, 1), .55f to listOf(.05, .3, .9, 1))) {
            action(obj("type" to "set_color", "rgba" to JSONArray(rgba)))
            paint(Offset(.35f, y), Offset(.65f, y))
        }
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "tool_settings", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "tool_settings", "target" to obj("kind" to "edge", "edge" to "right", "outer" to false), "viewport" to viewport()))
        compose.onAllNodesWithTag("tool-action-group-color-mixing").assertCountEquals(0)
        action(obj("type" to "select_brush", "id" to 24))
        compose.onNodeWithTag("tool-action-group-color-mixing").assertExists()
        assertTrue("Mixing brushes default to Oklab", command("color_mix_oklab").getBoolean("selected"))
        for ((i, choice) in listOf(android.view.MotionEvent.TOOL_TYPE_MOUSE to "color_mix_classic",
            android.view.MotionEvent.TOOL_TYPE_FINGER to "color_mix_linear", android.view.MotionEvent.TOOL_TYPE_STYLUS to "color_mix_oklab").withIndex()) {
            val (tool, id) = choice
            tap("tool-action-group-color-mixing", tool)
            compose.waitUntil(5_000) { compose.onAllNodesWithTag("tool-action-$id").fetchSemanticsNodes().isNotEmpty() }
            tap("tool-action-$id", tool)
            compose.waitUntil(10_000) { command(id).getBoolean("selected") }
            for (other in choices) assertEquals("tool $tool chose $id", other == id, command(other).getBoolean("selected"))
            val x = .4f + i * .1f
            paint(Offset(x, .4f), Offset(x, .6f))
            assertTrue("$id holds while painting", command(id).getBoolean("selected"))
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            compose.onNodeWithTag("tool-action-group-color-mixing").performScrollTo()
            compose.waitForIdle()
            capture("color-mixing-$theme")
        }
    }
    @Test fun colorWheelPixelsMatchSharedPickedColors() {
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "color", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "color", "target" to obj("kind" to "float", "position" to JSONArray(listOf(440, 100))), "viewport" to viewport()))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(1, 0, 0, 1))))
        for (space in listOf("hsv", "hls")) {
            val shape = if (space == "hsv") "square" else "triangle"
            action(obj("type" to "color", "action" to obj("op" to "shape", "shape" to shape)))
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
            action(obj("type" to "color", "action" to obj("op" to "shape", "shape" to shape))) // Drain the input's shared action.
            val rgba = state().getJSONObject("colors").getJSONObject("foreground").array("rgba")
            for ((index, value) in listOf(shown.red, shown.green, shown.blue).withIndex()) {
                assertEquals("$space wheel pixel matches picked component $index", value.toDouble(), rgba.getDouble(index), .035)
            }
            capture("color-$space")
        }
    }
    private fun shown(tag: String) = shown(hasTestTag(tag))
    private fun shown(matcher: SemanticsMatcher) {
        compose.waitUntil(15_000) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
        compose.onNode(matcher).assertIsDisplayed()
    }
    private fun canvasEvent(phase: Int, point: Offset) {
        fun find(view: android.view.View): CanvasSurfaceView? {
            if (view is CanvasSurfaceView) return view
            if (view is android.view.ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
            return null
        }
        instrumentation.runOnMainSync {
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
        val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
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
        action(obj("type" to "move_panel", "panel" to "toolbar", "viewport" to viewport(),
            "target" to obj("kind" to "float", "position" to JSONArray(listOf(400, 200)))))
        action(obj("type" to "invoke", "command" to "zen_mode"))
        compose.waitUntil(10_000) { host.snapshot!!.optBoolean("chrome_hidden") }
        val pen = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects()
            .first().getInt("id")
        val undoBefore = state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled")
        shown("tile-toolbar-$pen")
        compose.onNodeWithTag("tile-toolbar-$pen").performTouchInput { click() }
        shown("tool-drawer")
        shown(hasTestTag("tool-group-${state().getJSONObject("tool_set").array("groups").getJSONObject(0).getString("label")}") and hasAnyAncestor(hasTestTag("tool-drawer")))
        assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
        capture("zen-tool-drawer")
        val drawer = compose.onNodeWithTag("tool-drawer").fetchSemanticsNode().boundsInRoot
        val workspace = compose.onNodeWithTag("workspace").fetchSemanticsNode().boundsInRoot
        val sample = listOf(Offset(.7f, .8f), Offset(.75f, .7f), Offset(.3f, .7f))
            .first { !drawer.contains(Offset(it.x * workspace.width, it.y * workspace.height) + workspace.topLeft) }
        val outside = Offset(sample.x * workspace.width, sample.y * workspace.height)
        val before = pixel(sample)
        compose.activity.getExternalFilesDir(null)!!.resolve("zen-tool-drawer-contact.json").writeText(obj(
            "workspace" to workspace.toString(), "drawer" to drawer.toString(), "contact" to outside.toString(), "snapshot" to host.snapshot).toString(2))
        val owner = checkNotNull(findTag("workspace")).first
        val downAt = android.os.SystemClock.uptimeMillis()
        for ((phase, point) in listOf(android.view.MotionEvent.ACTION_DOWN to outside, android.view.MotionEvent.ACTION_MOVE to outside + Offset(20f, 0f), android.view.MotionEvent.ACTION_UP to outside + Offset(20f, 0f))) {
            val event = motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, phase, point + workspace.topLeft, downAt)
            try { instrumentation.runOnMainSync { owner.view.dispatchTouchEvent(event) } }
            finally { event.recycle() }
            android.os.SystemClock.sleep(40)
        }
        compose.waitUntil(10_000) { state().getJSONObject("customization").objectOrNull("drawer") == null }
        assertEquals(undoBefore, state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled"))
        awaitPixel(sample) { it == before }
    }
    @Test fun dockedToolbarStaysInZenAndEdgeSwipePeeksUntilOutsideContact() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace.toString())))
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "edge", "edge" to "right", "outer" to true), "viewport" to viewport()))
            action(obj("type" to "invoke", "command" to "hand"))
            val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.getString("active") == "toolbar" }.getInt("id")
            fun kept() = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.getInt("id") == group }.getBoolean("zen_kept")
            fun save(step: String) {
                compose.activity.getExternalFilesDir(null)!!.resolve("zen-toolbar-$theme-$step.json").writeText(host.snapshot!!.toString(2))
                capture("zen-toolbar-$theme-$step")
            }
            fun outside() = compose.onNodeWithTag("workspace").performTouchInput { click(Offset(width * .5f, height * .75f)) }
            try {
                action(obj("type" to "customize", "action" to obj("type" to "set_show_in_zen", "panel" to "toolbar", "shown" to true)))
                action(obj("type" to "invoke", "command" to "zen_mode"))
                compose.waitUntil(10_000) { host.snapshot!!.optBoolean("chrome_hidden") && kept() }
                shown("group-$group")
                val tile = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects().first().getInt("id")
                compose.onNodeWithTag("tile-toolbar-$tile").performTouchInput { click() }
                compose.waitUntil(10_000) { state().array("commands").objects().first { it.getString("id") == "pen" }.getBoolean("selected") }
                save("kept-hit")
                outside()
                assertTrue(kept())
                shown("group-$group")
                action(obj("type" to "customize", "action" to obj("type" to "set_show_in_zen", "panel" to "toolbar", "shown" to false)))
                compose.waitUntil(10_000) { !kept() && compose.onAllNodesWithTag("group-$group").fetchSemanticsNodes().isEmpty() }
                val density = compose.activity.resources.displayMetrics.density
                compose.onNodeWithTag("workspace").performTouchInput {
                    swipe(Offset(width - 4 * density, height * .5f), Offset(width - 84 * density, height * .5f), 350)
                }
                compose.waitUntil(10_000) { kept() }
                shown("group-$group")
                assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
                save("peek")
                outside()
                compose.waitUntil(10_000) { !kept() && compose.onAllNodesWithTag("group-$group").fetchSemanticsNodes().isEmpty() }
                assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
                save("dismissed")
            } catch (failure: Throwable) {
                save("failure")
                throw failure
            }
        }
    }
    @Test fun collapsedColumnOpensNestedToolDrawerAndExpandsWithoutLosingPanels() {
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.getString("active") == "brushes" }.getInt("id")
        action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "tab", "group" to group), "viewport" to viewport()))
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to true)))
        val column = host.snapshot!!.getJSONObject("layout").array("collapsed").objects().first { c -> c.array("groups").objects().any { it.getInt("group") == group } }.getInt("id")
        action(obj("type" to "customize", "action" to obj("type" to "set_column_drawers", "column" to column, "drawers" to true)))
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
        compose.onNodeWithTag("expand-column-$column").assertDoesNotExist()
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to column, "collapsed" to false)))
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("collapsed").objects().none { it.getInt("id") == column } }
        compose.onNodeWithTag("tile-toolbar-$pen").assertIsDisplayed()
    }
    private fun awaitNavigatorPixel(present: Boolean = true, predicate: (Int) -> Boolean) {
        compose.waitUntil(15_000) {
            val bounds = compose.onNodeWithTag("navigator-overview").fetchSemanticsNode().boundsInRoot
            val screenshot = instrumentation.uiAutomation.takeScreenshot()!!
            val image = screenshot.copy(Bitmap.Config.ARGB_8888, false)
            try { (bounds.left.toInt() until bounds.right.toInt() step 2).any { x ->
                (bounds.top.toInt() until bounds.bottom.toInt() step 2).any { y -> predicate(image.getPixel(x,y)) }
            } == present } finally { image.recycle(); screenshot.recycle() }
        }
        assertNull(host.failure)
    }
    @Test fun fullEditorPresetShowsToolsCommandsAndContentPanels() {
        val commands = host.snapshot!!.array("panels").objects().first { it.getString("id") == "commands" }
        commands.array("tiles").objects().filter { it.getJSONObject("control").getString("kind") != "divider" }.forEach { compose.onNodeWithTag("tile-commands-${it.getInt("id")}").assertExists() }
        compose.onNodeWithTag("navigator-zoom_in").assertIsDisplayed()
        compose.onNodeWithTag("color-wheel").assertIsDisplayed()
        capture("full-editor-preset")
    }
    @Test fun restoreMissingCommandsToolbarPreservesSavedTools() {
        val workspace = JSONObject(savedWorkspace.toString())
        workspace.getJSONObject("layout").put("header", obj("size" to "medium", "next_id" to 3, "zones" to JSONArray(listOf(
            JSONArray(listOf(obj("id" to 1, "item" to obj("kind" to "capy")), obj("id" to 2, "item" to obj("kind" to "menu_labels")))), JSONArray(), JSONArray()))))
        action(obj("type" to "restore_workspace", "workspace" to workspace))
        val layout = state().getJSONObject("workspace").getJSONObject("layout")
        if (layout.array("panels").objects().any { it.getString("id") == "commands" }) {
            action(obj("type" to "customize", "action" to obj("type" to "delete_toolbar", "panel" to "commands")))
            action(obj("type" to "customize", "action" to obj("type" to "confirm_toolbar")))
        }
        val tools = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first { it.getString("id") == "toolbar" }.toString()
        compose.onNodeWithTag("application-menu-window").performClick()
        compose.onNodeWithText("Quick Access Toolbars", useUnmergedTree = true).performTouchInput { click() }
        compose.onNodeWithText("Restore Commands", useUnmergedTree = true).performTouchInput { click() }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any { it.getString("active") == "commands" } }
        val commands = host.snapshot!!.array("panels").objects().first { it.getString("id") == "commands" }
        commands.array("tiles").objects().filter { it.getJSONObject("control").getString("kind") != "divider" }.forEach { shown("tile-commands-${it.getInt("id")}") }
        assertEquals(tools, state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first { it.getString("id") == "toolbar" }.toString())
        assertNull(host.actionError)
        compose.activity.getExternalFilesDir(null)!!.resolve("commands-restored-workspace.json").writeText(state().getJSONObject("workspace").toString())
        capture("commands-restored")
    }
    @Test fun resetLayoutKeepsTabDraggingDockingAndSelectionUsable() {
        fun group(panel: String) = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { it.array("panels").values().contains(panel) }
        fun drag(tag: String, target: Offset) {
            val source = compose.onNodeWithTag(tag)
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(center, target - origin, 700) }
        }
        fun workspaceIdle() = compose.waitUntil(15_000) {
            host.workspaceManager?.let { !it.optBoolean("busy") && !it.optBoolean("switcher_busy") } == true
        }
        // Cover both an existing saved workspace and the new editor preset.
        for (workspace in listOf(savedWorkspace, defaultWorkspace)) {
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(workspace.toString()).put("zen_mode", false)))
            action(obj("type" to "move_panel", "panel" to "tool_settings", "target" to obj("kind" to "float", "position" to JSONArray(listOf(480, 130))), "viewport" to viewport()))
            compose.onNodeWithTag("application-menu-window").performClick()
            compose.onNodeWithText("Workspaces", useUnmergedTree = true).performTouchInput { click() }
            compose.onNodeWithText("Restore Starting Layout…", useUnmergedTree = true).performTouchInput { click() }
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-submit", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
            workspaceIdle()
            compose.onNodeWithTag("workspace-submit", useUnmergedTree = true).performTouchInput { click() }
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-submit", useUnmergedTree = true).fetchSemanticsNodes().isEmpty() }
            workspaceIdle()
            compose.waitUntil(10_000) { !group("tool_settings").getBoolean("floating") }
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
            workspaceIdle()
        }
        capture("reset-layout-tab-drag")
    }
    @Test fun navigatorAndEyedropperUseActualGpuPixels() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            host.newDocument(1024, 768)
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.85, .12, .24, 1))))
            action(obj("type" to "set_brush_size", "value" to 180))
            action(obj("type" to "invoke", "command" to "pen"))
            stroke(Offset(.5f,.55f), Offset(.65f,.55f))
            awaitPixel(Offset(.575f,.55f)) { android.graphics.Color.red(it) > 150 && android.graphics.Color.green(it) < 80 }
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 1, 1))))
            action(obj("type" to "invoke", "command" to "eyedropper"))
            stroke(Offset(.575f,.55f))
            compose.waitUntil(15_000) { state().getJSONObject("colors").getJSONObject("foreground").array("rgba").getDouble(0) > .7 || host.failure != null }
            val rgba = state().getJSONObject("colors").getJSONObject("foreground").array("rgba")
            assertEquals(.85, rgba.getDouble(0), .04); assertEquals(.12, rgba.getDouble(1), .04); assertEquals(.24, rgba.getDouble(2), .04)
            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "navigator", "visible" to true)))
            action(obj("type" to "move_panel", "panel" to "navigator", "target" to obj("kind" to "float", "position" to JSONArray(listOf(360, 100))), "viewport" to viewport()))
            shown("navigator-overview")
            awaitNavigatorPixel { android.graphics.Color.red(it) > 150 && android.graphics.Color.green(it) < 80 }
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
            capture("navigator-$theme")
            action(obj("type" to "invoke", "command" to "undo"))
            awaitNavigatorPixel(false) { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
            action(obj("type" to "invoke", "command" to "redo"))
            awaitNavigatorPixel { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
            action(obj("type" to "invoke", "command" to "undo"))
            awaitNavigatorPixel(false) { android.graphics.Color.blue(it) > 150 && android.graphics.Color.red(it) < 80 }
            capture("navigator-history-$theme")
        }
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
            val screen = instrumentation.uiAutomation.takeScreenshot()!!
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
        return find(instrumentation.uiAutomation.rootInActiveWindow)
    }
    private fun chooseSaveFile(name: String) {
        // Exercise Android's real DocumentsUI create picker and URI grant result.
        var field: android.view.accessibility.AccessibilityNodeInfo? = null
        compose.waitUntil(30_000) {
            field = systemNode { it.isEditable && it.packageName?.toString()?.contains("documentsui") == true }
            field != null
        }
        assertTrue(field!!.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT, android.os.Bundle().apply {
            putCharSequence(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, name)
        }))
        val save = systemNode { it.isClickable && it.text?.toString()?.equals("save", ignoreCase = true) == true }
        assertNotNull("DocumentsUI Save", save)
        assertTrue(save!!.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
    }
    @Test fun paperLibraryImportsRenamesAndDeletesWithoutChangingTheDrawing() {
        fun texture() = state().getJSONObject("layer_properties").array("controls").objects().firstOrNull { it.getString("key") == "texture" }
        fun picker() = texture()?.objectOrNull("paper")
        fun library() = picker()?.array("tiles")?.objects().orEmpty().flatMap { it.array("actions").objects() }.map { it.getJSONObject("action") }
            .filter { it.getString("op") == "rename_paper" }
        fun idle() = state().array("requests").objects().none { it.getJSONObject("kind").getString("type") == "paper" }
        fun tiles() = picker()!!.array("tiles").objects()
        fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        fun dialogOpen() = compose.onAllNodesWithTag("texture-dialog").fetchSemanticsNodes().isNotEmpty()
        fun tileMenu(index: Int, op: String) {
            compose.onNodeWithTag("texture-tile-$index").performScrollTo().performTouchInput { longClick() }
            shown("texture-action-$op")
        }
        val theme = androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("theme") ?: "light"
        action(obj("type" to "set_theme", "theme" to theme))
        val paper = state().array("layers").objects().map { it.getLong("id") }.first { id -> action(obj("type" to "select_layer", "id" to id)); texture() != null }
        compose.waitUntil(30_000) { picker()?.isNull("import") == false }
        for (entry in library()) action(obj("type" to "effect", "action" to obj("op" to "delete_paper", "id" to entry.getString("id"))))
        compose.waitUntil(30_000) { idle() && library().isEmpty() }
        val scan = java.io.File(compose.activity.cacheDir, "paper-scan.png")
        val bitmap = Bitmap.createBitmap(64, 48, Bitmap.Config.ARGB_8888)
        for (y in 0 until 48) for (x in 0 until 64) bitmap.setPixel(x, y, android.graphics.Color.rgb((x * 7 + y * 3) % 256, (x * 7 + y * 3) % 256, (x * 7 + y * 3) % 256))
        scan.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        val picked = java.util.concurrent.atomic.AtomicReference<android.content.Intent>()
        val monitor = object : android.app.Instrumentation.ActivityMonitor() {
            override fun onStartActivity(intent: android.content.Intent): android.app.Instrumentation.ActivityResult? {
                if (intent.action != android.content.Intent.ACTION_OPEN_DOCUMENT) return null
                picked.set(android.content.Intent(intent))
                return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK, android.content.Intent().setData(android.net.Uri.fromFile(scan)))
            }
        }
        val instrumentation = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation()
        instrumentation.addMonitor(monitor)
        try {
            compose.onNodeWithTag("property-texture").performClick()
            shown("texture-import")
            val dialog = bounds("texture-dialog")
            val import = bounds("texture-import")
            val density = compose.activity.resources.displayMetrics.density
            assertEquals("Import… starts the footer", dialog.left + 20 * density, import.left, 1.5f)
            assertEquals("Import… shares the footer row", bounds("texture-cancel").center.y, import.center.y, 1f)
            capture("texture-import-$theme")
            compose.onNodeWithTag("texture-import").performClick()
            compose.waitUntil(5_000) { !dialogOpen() }
            compose.waitUntil(60_000) { idle() && library().size == 1 }
            assertEquals(listOf("image/*"), picked.get().getStringArrayExtra(android.content.Intent.EXTRA_MIME_TYPES)?.toList() ?: listOf(picked.get().type))
            assertTrue(state().getJSONObject("document_file").getBoolean("modified"))
            val id = library().single().getString("id")
            assertEquals("Import applies the new paper", id, tiles()[texture()!!.getJSONObject("value").getInt("value")].optString("id"))
            compose.onNodeWithTag("layer-rows").performScrollToIndex(state().array("layers").objects().indexOfFirst { it.getLong("id") == paper })
            compose.onNodeWithTag("layer-texture-$paper").performClick()
            shown("texture-dialog")
            val cotton = tiles().indexOfFirst { it.optString("id") == id }
            compose.onNodeWithTag("texture-tile-$cotton").assertIsDisplayed().assertIsSelected()
            val imported = state().getJSONObject("document_file").getLong("revision")
            tileMenu(cotton, "rename_paper")
            compose.onNodeWithTag("texture-action-delete_paper").assertIsDisplayed()
            capture("texture-imported-menu-$theme")
            compose.onNodeWithTag("texture-action-rename_paper").performClick()
            shown("paper-rename-dialog")
            val field = hasSetTextAction() and hasAnyAncestor(hasTestTag("paper-rename-name"))
            compose.onNode(field).assertTextEquals(tiles()[cotton].getString("label"))
            compose.onNode(field).performTextReplacement(" ")
            compose.onNodeWithTag("paper-rename-save").assertIsNotEnabled()
            capture("texture-rename-$theme")
            compose.onNode(field).performTextReplacement("Cotton rag")
            compose.onNodeWithTag("paper-rename-save").assertIsEnabled().performClick()
            compose.waitUntil(30_000) { idle() && picker()!!.array("tiles").objects().any { it.getString("label") == "Cotton rag" } }
            compose.onNodeWithTag("paper-rename-dialog").assertDoesNotExist()
            compose.onNode(hasTestTag("texture-tile-$cotton") and hasText("Cotton rag")).assertIsDisplayed().assertIsSelected()
            capture("texture-imported-$theme")
            tileMenu(cotton, "delete_paper")
            compose.onNodeWithTag("texture-action-delete_paper").performClick()
            compose.waitUntil(30_000) { idle() && library().isEmpty() }
            assertEquals("Library edits leave the drawing unchanged", imported, state().getJSONObject("document_file").getLong("revision"))
            compose.waitUntil(10_000) { compose.onAllNodesWithText("Cotton rag").fetchSemanticsNodes().isEmpty() }
            assertTrue("The dialog stays open through library changes", dialogOpen())
            val own = tiles().indexOfFirst { it.isNull("id") }
            assertEquals("The drawing's own paper comes first", 0, own)
            compose.onNodeWithTag("texture-tile-$own").assertIsSelected()
            tileMenu(own, "add_paper")
            capture("texture-own-menu-$theme")
            compose.onNodeWithTag("texture-action-add_paper").performClick()
            compose.waitUntil(30_000) { idle() && library().size == 1 && tiles().none { it.isNull("id") } }
            val added = library().single().getString("id")
            val kept = tiles().indexOfFirst { it.optString("id") == added }
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("texture-tile-$kept").fetchSemanticsNodes().singleOrNull()?.config?.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.Selected) == true }
            capture("texture-added-$theme")
            tileMenu(kept, "delete_paper")
            compose.onNodeWithTag("texture-action-delete_paper").performClick()
            compose.waitUntil(30_000) { idle() && library().isEmpty() }
            compose.onNodeWithTag("texture-cancel").performClick()
            compose.waitUntil(5_000) { !dialogOpen() }
            assertEquals("Cancel keeps the drawing", imported, state().getJSONObject("document_file").getLong("revision"))
            assertNull(host.hostError)
        } finally {
            instrumentation.removeMonitor(monitor)
            scan.delete()
        }
    }
    @Test fun paperScaleShadesTheCanvasWhileDragging() {
        fun scale() = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "scale" }.getJSONObject("value").getDouble("value")
        val theme = androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("theme") ?: "light"
        action(obj("type" to "set_theme", "theme" to theme))
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "paper")))
        fun find(view: android.view.View): CanvasSurfaceView? = when (view) {
            is CanvasSurfaceView -> view
            is android.view.ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { find(view.getChildAt(it)) }
            else -> null
        }
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        compose.runOnIdle { canvas = find(compose.activity.window.decorView)!!; canvas.getLocationOnScreen(location) }
        fun spread(name: String): Double {
            android.os.SystemClock.sleep(300)
            val camera = state().getJSONObject("camera")
            val tab = state().array("tabs").objects().first { it.optBoolean("active") }
            val zoom = camera.getDouble("zoom")
            val left = location[0] + camera.getJSONArray("translation").getDouble(0)
            val top = location[1] + camera.getJSONArray("translation").getDouble(1)
            val (width, height) = tab.getInt("width") * zoom to tab.getInt("height") * zoom
            var spread = 0.0
            screenshot("paper-scale/$name-$theme.png") { bitmap ->
                val samples = ((top + height * .3).toInt() until (top + height * .7).toInt() step 2).flatMap { y ->
                    ((left + width * .3).toInt() until (left + width * .7).toInt() step 2).map { x ->
                        bitmap.getPixel(x, y).let { (android.graphics.Color.red(it) + android.graphics.Color.green(it) + android.graphics.Color.blue(it)) / 3.0 }
                    }
                }
                val mean = samples.average()
                spread = kotlin.math.sqrt(samples.sumOf { (it - mean) * (it - mean) } / samples.size)
            }
            return spread
        }
        val slider = compose.onNodeWithTag("number-slider-Scale").performScrollTo()
        val flat = spread("flat")
        val before = scale()
        slider.performTouchInput { down(center) }
        slider.performTouchInput { repeat(20) { moveBy(Offset(width * .02f, 0f), 16) } }
        val shown = android.os.SystemClock.uptimeMillis() + 10_000
        var dragging = flat
        while (android.os.SystemClock.uptimeMillis() < shown && spread("dragging").also { dragging = it } <= flat + 2) Unit
        val dragged = scale()
        slider.performTouchInput { up() }
        assertTrue("Dragging Scale enlarges the paper: $before -> $dragged", dragged > before * 1.5)
        assertTrue("The relief shades the drawing while Scale is held: $flat -> $dragging", dragging > flat + 2)
        val hidden = android.os.SystemClock.uptimeMillis() + 10_000
        var settled = dragging
        while (android.os.SystemClock.uptimeMillis() < hidden && spread("released").also { settled = it } > flat + 1) Unit
        assertTrue("The relief hides after release: $settled", settled <= flat + 1)
        action(obj("type" to "invoke", "command" to "undo"))
        assertEquals("A Scale drag is one Undo step", before, scale(), 1e-9)
    }
    private fun drawing(): JSONObject? = state().objectOrNull("new_drawing")
    private fun drawer(): JSONObject? = drawing()?.objectOrNull("drawer")
    private fun awaitDrawing(what: String, predicate: (JSONObject) -> Boolean) {
        try { compose.waitUntil(15_000) { drawing()?.let(predicate) == true } }
        catch (e: Throwable) { throw AssertionError("$what: ${drawing()}", e) }
        compose.waitForIdle()
    }
    private fun field(tag: String) = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag(tag)), useUnmergedTree = true)
    private fun typeNumber(tag: String, text: String) { field(tag).performTextReplacement(text); field(tag).performImeAction(); compose.waitForIdle() }
    private fun choose(tag: String, label: String) {
        compose.onNodeWithTag(tag).performClick()
        val item = hasText(label) and SemanticsMatcher("$tag choice") { it.config.getOrNull(androidx.compose.ui.semantics.SemanticsProperties.TestTag)?.startsWith("$tag-") == true }
        shown(item)
        compose.onNode(item).performClick()
        compose.waitForIdle()
    }
    private fun openNewDrawing() {
        action(obj("type" to "invoke", "command" to "new_document"))
        shown("new-drawing-create")
        awaitDrawing("the dialog opens") { it.isNull("drawer") }
    }
    @Test fun newDocumentCreatesTheChosenResolution() {
        compose.waitUntil(60_000) { state().array("commands").objects().first { it.getString("id") == "new_document" }.getBoolean("enabled") }
        compose.onNodeWithTag("application-menu-file").performClick()
        compose.onNodeWithText("New…").performClick()
        shown("new-drawing-resolution")
        choose("new-drawing-unit", "px")
        awaitDrawing("pixels") { it.getJSONObject("setup").getString("unit") == "px" }
        field("new-drawing-resolution").performTextReplacement("600")
        compose.onNodeWithTag("new-drawing-create").performClick()
        awaitDocument()
        action(obj("type" to "invoke", "command" to "image_size"))
        compose.waitUntil(60_000) { state().getJSONObject("layer_tools").optJSONObject("image_size")?.optDouble("resolution") == 600.0 }
        action(obj("type" to "image_size", "action" to obj("op" to "cancel")))
        compose.waitUntil(15_000) { state().getJSONObject("layer_tools").isNull("image_size") }
    }
    @Test fun documentSafSaveReopenAndExportPreservePaint() {
        compose.waitUntil(60_000) { state().array("commands").objects().first { it.getString("id") == "new_document" }.getBoolean("enabled") }
        compose.onNodeWithTag("application-menu-file").performClick()
        compose.onNodeWithText("New…").performClick()
        shown("new-drawing-width")
        choose("new-drawing-unit", "px")
        awaitDrawing("pixels") { it.getJSONObject("setup").getString("unit") == "px" }
        field("new-drawing-width").performTextReplacement("512")
        field("new-drawing-height").performTextReplacement("384")
        compose.onNodeWithTag("new-drawing-create").performClick()
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
        openNewDrawing()
        typeNumber("new-drawing-width", "640")
        compose.onNodeWithTag("new-drawing-create").performClick()
        compose.waitUntil(60_000) { state().array("tabs").getJSONObject(0).getInt("width") == 640 }
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
        shown("export-choose-file")
        compose.waitUntil(60_000) { compose.onAllNodes(hasTestTag("export-choose-file") and isEnabled()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("export-choose-file").performClick()
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
        val drawings = host.drawingTabs.rows.size
        openNewDrawing()
        compose.onNodeWithTag("new-drawing-cancel").performClick()
        awaitDocument()
        compose.waitUntil(15_000) { drawing() == null && request() == null }
        assertEquals(drawings, host.drawingTabs.rows.size)
        assertEquals(epoch, state().getJSONObject("document_file").getLong("epoch"))
        assertTrue(state().getJSONObject("document_file").getBoolean("modified"))
        awaitPixel(Offset(.55f,.5f)) { android.graphics.Color.green(it) < 50 }
    }
    @Test fun newDocumentBlendingAndPropertiesInBothThemes() {
        fun selected(id: String) = state().array("commands").objects().first { it.getString("id") == id }.getBoolean("selected")
        fun blending() = compose.onNodeWithTag("new-drawing-blending").performScrollTo()
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        try {
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                openNewDrawing()
                blending().assertIsEnabled().assert(hasText("Perceptual"))
                assertTrue(drawing()!!.isNull("blending_note"))
                capture("new-drawing-blending-$theme")
                choose("new-drawing-depth", "16-bit float HDR")
                awaitDrawing("float blending") { !it.isNull("blending_note") }
                blending().assertIsNotEnabled().assert(hasText("Linear light"))
                assertEquals("Float documents blend in linear light", drawing()!!.getString("blending_note"))
                capture("new-drawing-blending-float-$theme")
                choose("new-drawing-depth", "8-bit SDR")
                awaitDrawing("integer blending") { it.isNull("blending_note") }
                blending().assertIsEnabled().assert(hasText("Perceptual"))
                compose.onNodeWithTag("new-drawing-cancel").performClick()
                awaitDocument()
            }
            openNewDrawing()
            choose("new-drawing-blending", "Linear light")
            awaitDrawing("linear blending") { it.getJSONObject("setup").getString("blend_space") == "Linear" }
            compose.onNodeWithTag("new-drawing-create").performClick()
            awaitDocument()
            compose.waitUntil(60_000) { selected("blend_linear") && !selected("blend_perceptual") }
            action(obj("type" to "invoke", "command" to "document_properties"))
            shown(hasText("Blending") and !hasClickAction() and hasAnyAncestor(isDialog()))
            shown(hasText("Linear Light Blending") and hasAnyAncestor(isDialog()))
            capture("document-properties-blending")
            compose.onNodeWithText("Done").performClick()
            compose.waitUntil(15_000) { compose.onAllNodesWithText("Document Properties").fetchSemanticsNodes().isEmpty() }
        } finally {
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
    }

    @Test fun newDrawingDialogJourneyInBothThemes() {
        fun shot(theme: String, name: String) = screenshot("new-drawing/$theme-$name.png")
        fun tap(tag: String, scroll: Boolean = false) {
            if (scroll) {
                compose.waitUntil(15_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
                compose.onNodeWithTag(tag).performScrollTo()
            }
            shown(tag)
            compose.onNodeWithTag(tag).performClick()
            compose.waitForIdle()
        }
        fun hold(tag: String) { compose.onNodeWithTag(tag).performScrollTo().performTouchInput { longClick() }; compose.waitForIdle() }
        fun JSONObject.sheet() = getJSONObject("sheet").getJSONArray("extent").let { it.getInt(0) to it.getInt(1) }
        fun JSONObject.panel() = objectOrNull("drawer")
        fun JSONObject.tiles() = panel()!!.getJSONArray("tiles").objects()
        fun JSONObject.paper() = getJSONObject("setup").getJSONObject("paper")
        fun JSONObject.relief() = panel()!!.getJSONObject("relief")
        fun JSONObject.marked() = tiles().indexOfFirst { it.getBoolean("selected") }
        fun reliefField() = compose.onNode(SemanticsMatcher.keyIsDefined(androidx.compose.ui.semantics.SemanticsProperties.EditableText) and
            hasAnyAncestor(hasTestTag("new-drawing-paper-relief")), useUnmergedTree = true)
        val documentsUi = { node: android.view.accessibility.AccessibilityNodeInfo -> node.packageName?.toString()?.contains("documentsui") == true }
        fun importOpensPicker(open: () -> Unit) {
            val marked = drawing()!!.marked()
            open()
            val pickerDeadline = android.os.SystemClock.uptimeMillis() + 10_000
            while (systemNode(documentsUi) == null && android.os.SystemClock.uptimeMillis() < pickerDeadline) android.os.SystemClock.sleep(100)
            assertNotNull("Import… opens the system picker", systemNode(documentsUi))
            assertEquals("Import never becomes the marked paper", marked, drawing()!!.marked())
            instrumentation.uiAutomation.performGlobalAction(android.accessibilityservice.AccessibilityService.GLOBAL_ACTION_BACK)
            compose.waitUntil(10_000) { systemNode(documentsUi) == null }
            awaitDrawing("paper stays open with its marked paper") { it.panel()?.getString("kind") == "paper" && it.marked() == marked }
        }
        val paperImage = compose.activity.getExternalFilesDir(null)!!.resolve("new-drawing-paper.png")
        Bitmap.createBitmap(512, 512, Bitmap.Config.ARGB_8888).let { bitmap ->
            val random = java.util.Random(7)
            for (y in 0 until 512) for (x in 0 until 512) random.nextInt(256).let { v -> bitmap.setPixel(x, y, android.graphics.Color.rgb(v, v, v)) }
            paperImage.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        try {
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                val drawings = host.drawingTabs.rows.size
                openNewDrawing()
                assertTrue(drawing()!!.getString("untitled").isNotEmpty())
                shot(theme, "1-main")

                choose("new-drawing-unit", "px")
                awaitDrawing("pixels") { it.getJSONObject("setup").getString("unit") == "px" }
                typeNumber("new-drawing-width", "1200")
                typeNumber("new-drawing-height", "900")
                awaitDrawing("1200 × 900") { it.sheet() == 1200 to 900 }
                tap("new-drawing-link", true)
                awaitDrawing("linked") { it.getBoolean("linked") }
                typeNumber("new-drawing-width", "2400")
                awaitDrawing("the linked height follows") { it.sheet() == 2400 to 1800 }
                tap("new-drawing-link", true)
                awaitDrawing("unlinked") { !it.getBoolean("linked") }
                tap("new-drawing-portrait", true)
                awaitDrawing("portrait") { it.getJSONObject("page").getString("orientation") == "portrait" && it.sheet() == 1800 to 2400 }
                tap("new-drawing-resolution-choices", true)
                shown("new-drawing-resolution-1")
                shot(theme, "1b-resolution-menu")
                compose.onNodeWithTag("new-drawing-resolution-1").performClick()
                awaitDrawing("150 ppi") { it.getJSONObject("setup").getInt("resolution") == 150 }
                typeNumber("new-drawing-resolution", "200")
                awaitDrawing("200 ppi") { it.getJSONObject("setup").getInt("resolution") == 200 }
                choose("new-drawing-unit", "mm")
                awaitDrawing("millimetres keep the pixels") { it.getJSONObject("setup").getString("unit") == "mm" && it.sheet() == 1800 to 2400 }

                tap("new-drawing-presets-button")
                awaitDrawing("presets") { it.panel()?.getString("kind") == "presets" }
                shot(theme, "2-presets")
                field("new-drawing-search").performTextReplacement("chalk")
                awaitDrawing("search") { d -> d.panel()!!.let { !it.getBoolean("tabs_enabled") } && d.tiles().firstOrNull()?.getString("name") == "Chalkboard" }
                tap("new-drawing-preset-0", true)
                awaitDrawing("Chalkboard picked") { it.panel()!!.getJSONObject("summary").getString("name") == "Chalkboard" }
                tap("new-drawing-save-as")
                awaitDrawing("save row") { !it.panel()!!.isNull("save") }
                shot(theme, "4-save-as")
                field("new-drawing-save-name").performTextReplacement("Studio wall")
                awaitDrawing("preset name") { it.panel()!!.objectOrNull("save")?.getString("name") == "Studio wall" }
                tap("new-drawing-save-confirm")
                awaitDrawing("saved") { d -> d.panel()!!.isNull("save") && d.tiles().any { it.getBoolean("own") && it.getBoolean("selected") } }
                hold("new-drawing-preset-${drawing()!!.tiles().indexOfFirst { it.getBoolean("own") }}")
                tap("new-drawing-preset-rename")
                awaitDrawing("renaming") { it.panel()!!.objectOrNull("save")?.getString("confirm") == "Rename" }
                field("new-drawing-save-name").performTextReplacement("Studio wall 2")
                choose("new-drawing-save-group", "Pastel")
                awaitDrawing("Pastel") { it.panel()!!.objectOrNull("save")?.getString("group") == "pastel" }
                tap("new-drawing-save-confirm")
                awaitDrawing("renamed") { d -> d.panel()!!.isNull("save") && d.tiles().any { it.getBoolean("own") && it.getString("name") == "Studio wall 2" } }
                shot(theme, "3-presets-pastel")
                hold("new-drawing-preset-${drawing()!!.tiles().indexOfFirst { it.getBoolean("own") }}")
                tap("new-drawing-preset-delete")
                awaitDrawing("deleted") { d -> d.tiles().none { it.getBoolean("own") } && !d.panel()!!.getBoolean("can_select") }
                field("new-drawing-search").performTextReplacement("chalk")
                awaitDrawing("Chalkboard found") { it.tiles().firstOrNull()?.getString("name") == "Chalkboard" }
                tap("new-drawing-preset-0", true)
                awaitDrawing("Chalkboard picked again") { it.panel()!!.getBoolean("can_select") }
                tap("new-drawing-select")
                awaitDrawing("Chalkboard") { it.isNull("drawer") && it.getJSONObject("page").getString("preset") == "Chalkboard" }
                assertEquals("50 %", drawing()!!.getJSONObject("page").getString("relief"))
                compose.onNodeWithTag("new-drawing-texture-relief", useUnmergedTree = true).assertIsDisplayed().assert(hasText("50 %"))
                shot(theme, "5-chalkboard")

                tap("new-drawing-texture", true)
                awaitDrawing("paper") { it.panel()?.getString("kind") == "paper" }
                assertEquals("Chalkboard's texture visibility", 50.0, drawing()!!.relief().getDouble("value"), 0.0)
                reliefField().assertIsEnabled().assert(hasText("50"))
                assertEquals("the visibility field matches the footer buttons", compose.onNodeWithTag("new-drawing-paper-use").fetchSemanticsNode().size.height,
                    compose.onNodeWithTag("new-drawing-paper-relief").fetchSemanticsNode().size.height)
                shot(theme, "6-paper")
                val cold = drawing()!!.tiles().indexOfFirst { it.optString("id") == "cold_press" }
                tap("new-drawing-paper-tile-$cold", true)
                awaitDrawing("cold press picked") { it.tiles()[cold].getBoolean("selected") }
                compose.onNodeWithTag("new-drawing-paper-import").performScrollTo()
                compose.waitForIdle()
                val lastPaper = compose.onNodeWithTag("new-drawing-paper-tile-${drawing()!!.tiles().size - 1}").fetchSemanticsNode().boundsInRoot
                val importTile = compose.onNodeWithTag("new-drawing-paper-import").assertIsDisplayed().assert(hasText(drawing()!!.getJSONObject("copy").getString("import")))
                    .fetchSemanticsNode().boundsInRoot
                assertTrue("Import follows the last paper", importTile.top > lastPaper.bottom || (importTile.top == lastPaper.top && importTile.left > lastPaper.right))
                shot(theme, "6c-import")
                if (theme == "light") {
                    importOpensPicker { compose.onNodeWithTag("new-drawing-paper-import").performClick() }
                    val builtIn = drawing()!!.tiles().size
                    kotlinx.coroutines.runBlocking { importLibraryPaper(host, android.net.Uri.fromFile(paperImage)) }
                    awaitDrawing("imported paper picked") { d -> d.tiles().size == builtIn + 1 && d.tiles().last().getBoolean("selected") }
                    compose.onNodeWithTag("new-drawing-paper-tile-$builtIn").performScrollTo()
                    shot(theme, "6d-imported-paper")
                } else {
                    instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_TAB)
                    compose.onNodeWithTag("new-drawing-paper-import").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.RequestFocus)
                    compose.onNodeWithTag("new-drawing-paper-import").assertIsFocused()
                    host.drain()
                    compose.waitForIdle()
                    importOpensPicker { instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_ENTER) }
                }
                tap("new-drawing-paper-tile-$cold", true)
                awaitDrawing("cold press picked again") { it.tiles()[cold].getBoolean("selected") }
                reliefField().performTextReplacement("120")
                compose.waitForIdle()
                tap("new-drawing-paper-use")
                awaitDrawing("Use Texture applies the typed visibility") { it.isNull("drawer") && it.paper().optString("texture") == "cold_press" && it.paper().getInt("relief") == 120 }
                assertEquals("120 %", drawing()!!.getJSONObject("page").getString("relief"))

                tap("new-drawing-texture", true)
                awaitDrawing("paper again") { it.panel()?.getString("kind") == "paper" && it.relief().getDouble("value") == 120.0 }
                reliefField().assert(hasText("120")).performTextReplacement("200")
                tap("new-drawing-paper-tile-0", true)
                awaitDrawing("No paper") { it.tiles()[0].getBoolean("selected") && !it.relief().getBoolean("enabled") }
                reliefField().assertIsNotEnabled()
                shot(theme, "6b-no-paper")
                tap("new-drawing-paper-cancel")
                awaitDrawing("Cancel keeps the paper and its visibility") { it.isNull("drawer") && it.paper().optString("texture") == "cold_press" && it.paper().getInt("relief") == 120 }
                assertNull(host.actionError)

                tap("new-drawing-paper-color", true)
                awaitDrawing("paper color") { it.panel()?.getString("kind") == "paper_color" }
                shot(theme, "7-paper-color")
                tap("new-drawing-paper-color-5", true)
                awaitDrawing("Kraft") { it.panel()!!.getJSONArray("palette").getJSONObject(5).getBoolean("selected") }
                tap("color-value-hex", true)
                compose.onNodeWithTag("color-value-hex-input").performTextReplacement("#D96B7A")
                compose.onNodeWithTag("color-value-hex-input").performImeAction()
                awaitDrawing("a new color can be added") { it.panel()!!.getBoolean("can_add") }
                val colors = drawing()!!.panel()!!.getJSONArray("palette").length()
                tap("new-drawing-add-paper-color", true)
                awaitDrawing("added") { it.panel()!!.getString("count") == "${colors + 1}" }
                hold("new-drawing-paper-color-$colors")
                tap("new-drawing-paper-color-delete")
                awaitDrawing("removed") { it.panel()!!.getString("count") == "$colors" }
                tap("new-drawing-color-mode-transparent", true)
                awaitDrawing("transparent") { it.panel()!!.getBoolean("transparent") }
                shown("color-transparent")
                shot(theme, "8-transparent")
                tap("new-drawing-color-use")
                awaitDrawing("transparent paper") { it.isNull("drawer") && it.getJSONObject("setup").getJSONObject("paper").optBoolean("transparent") && !it.getJSONObject("sheet").getBoolean("shown") }
                shot(theme, "9-main-transparent")

                tap("new-drawing-cancel")
                compose.waitUntil(15_000) { drawing() == null && request() == null }
                awaitDocument()
                assertEquals(drawings, host.drawingTabs.rows.size)

                openNewDrawing()
                tap("new-drawing-presets-button")
                awaitDrawing("Digital presets") { it.panel()?.getString("kind") == "presets" }
                field("new-drawing-search").performTextReplacement("Full HD")
                awaitDrawing("Full HD found") { it.tiles().firstOrNull()?.getString("name") == "Full HD" }
                tap("new-drawing-preset-0", true)
                tap("new-drawing-select")
                awaitDrawing("Digital preset has no paper") { it.isNull("drawer") && it.paper().isNull("texture") }
                shot(theme, "digital-no-paper")
                field("new-drawing-name").performTextReplacement("Harbor study $theme")
                awaitDrawing("named") { it.getString("name") == "Harbor study $theme" }
                tap("new-drawing-create")
                compose.waitUntil(60_000) { drawing() == null && request() == null && !host.documents.working }
                awaitDocument()
                compose.waitUntil(30_000) { host.drawingTabs.rows.any { it.getString("title") == "Harbor study $theme" } }
                assertEquals(drawings + 1, host.drawingTabs.rows.size)
                assertEquals("Digital creates only its paint layer", 1, state().array("layers").length())
                assertEquals("Harbor study $theme", state().getJSONObject("document_file").getString("unsaved_name"))
            }
        } finally {
            action(obj("type" to "set_theme", "theme" to originalTheme))
            paperImage.delete()
        }
    }

}
