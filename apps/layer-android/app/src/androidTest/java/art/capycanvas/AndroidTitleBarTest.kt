package art.capycanvas

import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.inspector.WindowInspector
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real Compose/native-view contacts; user workspaces and preferences are restored. */
class AndroidTitleBarTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var legacy: Map<String, *>
    private var pressed: ViewRootForTest? = null
    private var point = Offset.Zero
    private var downAt = 0L
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var button = MotionEvent.BUTTON_PRIMARY
    private var density = 1f
    private fun view() = host.workspaceManager!!
    private fun layout() = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").toString()
    private fun find(node: SemanticsNode, tag: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node else node.children.firstNotNullOfOrNull { find(it, tag) }
    private fun roots(view: View): List<ViewRootForTest> = when (view) {
        is ViewRootForTest -> listOf(view)
        is ViewGroup -> (0 until view.childCount).flatMap { roots(view.getChildAt(it)) }
        else -> emptyList()
    }
    private fun node(tag: String): Pair<ViewRootForTest, SemanticsNode>? = WindowInspector.getGlobalWindowViews().flatMap(::roots)
        .firstNotNullOfOrNull { root -> find(root.semanticsOwner.unmergedRootSemanticsNode, tag)?.let { root to it } }
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync { result = node(tag)?.second?.boundsInRoot }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun waitFor(label: String, timeout: Long = 15000, condition: () -> Boolean) {
        val until = SystemClock.uptimeMillis() + timeout
        do {
            var ready = false
            instrumentation.runOnMainSync {
                assertNull(host.failure); assertNull(host.actionError)
                host.snapshot?.objectOrNull("state")?.let { assertTrue(it.optString("host_error"), it.isNull("host_error")) }
                ready = condition()
            }
            if (ready) return
            SystemClock.sleep(20)
        } while (SystemClock.uptimeMillis() < until)
        shot("failure-${label.replace(Regex("[^A-Za-z0-9-]"), "-")}")
        fail("Timed out: $label; ${view()}")
    }
    private fun idle() {
        SystemClock.sleep(220)
        waitFor("workspace idle") { !view().optBoolean("busy") && !view().optBoolean("switcher_busy") && !view().optBoolean("dirty") }
        assertTrue(view().toString(), view().isNull("error")); assertTrue(view().toString(), view().isNull("switcher_error"))
    }
    private fun send(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; idle() }
    private fun capture(): String {
        var result = ""
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync {
            CoroutineScope(Dispatchers.Main).launch {
                result = host.withNative { Native.workspace(it, obj("type" to "capture").toString()) }; done.countDown()
            }
        }
        assertTrue(done.await(10, TimeUnit.SECONDS)); return result
    }
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) downAt = SystemClock.uptimeMillis()
        val source = when (tool) { MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE; MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS; else -> InputDevice.SOURCE_TOUCHSCREEN }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) button else 0
        val event = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1,
            arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
            arrayOf(MotionEvent.PointerCoords().apply { x = next.x; y = next.y; pressure = .7f }),
            0, buttons, 1f, 1f, 0, 0, source, 0)
        try { instrumentation.runOnMainSync { checkNotNull(pressed).view.dispatchTouchEvent(event) } }
        finally { event.recycle() }
        if (action in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) pressed = null
        SystemClock.sleep(40)
    }
    private fun down(tag: String) {
        var start = Offset.Zero
        instrumentation.runOnMainSync { checkNotNull(node(tag)) { "Missing $tag; ${view()}" }.let { pressed = it.first; start = it.second.boundsInRoot.center } }
        event(MotionEvent.ACTION_DOWN, start)
    }
    private fun tap(tag: String) {
        android.util.Log.i("TitleBarAcceptance", "Tap $tag")
        down(tag); event(MotionEvent.ACTION_UP); idle()
    }
    private fun key(code: Int, meta: Int = 0) {
        // Keyboard input must pass ViewRootImpl so Android leaves touch mode.
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
            instrumentation.sendKeySync(KeyEvent(0, SystemClock.uptimeMillis(), action, code, 0, meta))
        SystemClock.sleep(220)
    }

    private lateinit var fixture: JSONObject
    private var originalSettings = ""
    private fun snapshot() = host.snapshot!!
    private fun state() = snapshot().getJSONObject("state")
    private fun model() = snapshot().getJSONObject("header").getJSONObject("model")
    private fun entries() = model().array("zones").values().flatMap { (it as JSONArray).objects() }
    private fun editing() = snapshot().getJSONObject("header").optBoolean("editing")
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(15, TimeUnit.SECONDS)); SystemClock.sleep(250)
    }
    private fun edit(value: JSONObject) = action(obj("type" to "customize", "action" to obj("type" to "header", "action" to value)))
    private fun restore(size: String = "small") {
        val value = JSONObject(fixture.toString())
        value.getJSONObject("layout").getJSONObject("header").put("size", size)
        action(obj("type" to "restore_workspace", "workspace" to value))
        waitFor("bar settled") { node("header-item-1") != null && !editing() }
        idle()
    }
    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { host = it.host; density = it.resources.displayMetrics.density }
        waitFor("startup", 90000) { host.workspaceManager?.optBoolean("ready") == true && host.snapshot?.optBoolean("brush_ready") == true }
        idle()
    }
    @Before fun ready() {
        legacy = instrumentation.targetContext.getSharedPreferences("capy-canvas", 0).all
        val directory = File(instrumentation.targetContext.filesDir, "title-bar-tests/${UUID.randomUUID()}")
        CanvasHost.workspaceDirectoryForTest = directory.absolutePath
        RecoveryController.directoryForTest = File(directory, "recovery")
        launch()
        originalSettings = state().getJSONObject("settings").toString()
        fixture = JSONObject(state().getJSONObject("workspace").toString())
        fixture.put("zen_mode", false)
        fixture.getJSONObject("layout").apply {
            for (name in listOf("bands", "floating", "collapsed", "column_stacks", "column_scroll", "fit_tab_groups")) put(name, JSONArray())
            val left = JSONArray(listOf(
                obj("id" to 1, "item" to obj("kind" to "capy")),
                obj("id" to 2, "item" to obj("kind" to "menu_labels"))))
            val right = JSONArray(listOf(obj("id" to 3, "item" to obj("kind" to "settings"))))
            put("header", obj("size" to "small", "next_id" to 10,
                "zones" to JSONArray(listOf(left, JSONArray(), right))))
            put("canvas_info", obj("visible" to true))
        }
        restore()
    }
    @After fun cleanup() {
        if (pressed != null) event(MotionEvent.ACTION_CANCEL)
        if (::host.isInitialized && originalSettings.isNotEmpty()) action(obj("type" to "restore_settings", "settings" to JSONObject(originalSettings)))
        if (::scenario.isInitialized) scenario.close()
        CanvasHost.workspaceDirectoryForTest = null
        RecoveryController.directoryForTest = null
        val preferences = instrumentation.targetContext.getSharedPreferences("capy-canvas", 0)
        preferences.edit().apply {
            val settings = legacy["settings"] as? String
            if (settings == null) remove("settings") else putString("settings", settings)
        }.commit()
        assertEquals("User preferences preserved", legacy, preferences.all)
    }
    private fun shot(name: String) {
        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/title-bar/$name.png")
        file.parentFile!!.mkdirs()
        instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
            file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }; bitmap.recycle()
        }
    }
    private fun tilePixel(tag: String): Int {
        var position = Offset.Zero
        instrumentation.runOnMainSync {
            val (root, node) = checkNotNull(node(tag))
            val screen = IntArray(2); root.view.getLocationOnScreen(screen)
            position = Offset(screen[0] + node.boundsInRoot.left + 3 * density, screen[1] + node.boundsInRoot.center.y)
        }
        val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
        return try { bitmap.getPixel(position.x.toInt(), position.y.toInt()) } finally { bitmap.recycle() }
    }
    private fun center() = bounds("title-bar").let { Offset(it.center.x, it.center.y) }
    private fun outside() = bounds("workspace").let { Offset(it.center.x, it.bottom - 80 * density) }
    private fun drag(tag: String, destination: Offset, cancel: Boolean = false, inspect: (() -> Unit)? = null) {
        down(tag)
        event(MotionEvent.ACTION_MOVE, destination)
        inspect?.invoke()
        event(if (cancel) MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP)
        idle()
    }
    private fun startEditor() {
        action(obj("type" to "invoke", "command" to "customize_workspace_ui"))
        waitFor("inline editor") { editing() && node("header-editor") != null }
        SystemClock.sleep(200)
    }

    @Test fun bankBodiesGripsCancellationAndHistoryEveryDeviceSizeAndTheme() {
        for (theme in listOf("light", "dark")) for (size in listOf("small", "medium", "large"))
            for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = device
                android.util.Log.i("TitleBarAcceptance", "Case $theme $size $device")
                action(obj("type" to "set_theme", "theme" to theme)); restore(size)
                val baseline = model().toString()
                val durable = capture()
                startEditor()
                val opened = model().toString()
                tap("header-component-space"); assertEquals("Bank taps are inert", opened, model().toString())
                tap("header-component-tools"); assertNull(snapshot().objectOrNull("picker"))
                drag("header-component-space", outside()); assertEquals("Outside bank drop cancels", opened, model().toString())
                drag("header-component-space", center(), inspect = {
                    waitFor("immediate bank preview") { node("header-drag-ghost") != null }
                    assertEquals("Motion does not publish layout", opened, model().toString())
                })
                val added = entries().first { it.getJSONObject("item").getString("kind") == "space" }.getInt("id")
                assertEquals(added, model().array("zones").getJSONArray(1).getJSONObject(0).getInt("id"))
                val preview = model().toString()
                drag("header-component-tools", center())
                waitFor("drop opens existing tool picker") { snapshot().objectOrNull("picker") != null && node("tool-picker-cancel") != null }
                tap("tool-picker-cancel")
                waitFor("picker Cancel keeps editor") { editing() && snapshot().objectOrNull("picker") == null }
                assertEquals(preview, model().toString())
                drag("header-item-1", outside(), cancel = true)
                assertEquals("ACTION_CANCEL preserves layout", preview, model().toString())
                down("header-grip-1")
                val grab = point - bounds("header-item-1").topLeft
                event(MotionEvent.ACTION_MOVE, outside())
                waitFor("detached original grab") { node("header-item-1")!!.second.boundsInRoot.top > 200 * density }
                assertEquals(grab.x, point.x - bounds("header-item-1").left, 2f)
                event(MotionEvent.ACTION_MOVE, center())
                waitFor("reentry") { node("header-item-1")!!.second.boundsInRoot.top < node("title-bar")!!.second.boundsInRoot.bottom }
                event(MotionEvent.ACTION_UP); idle()
                assertTrue(model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == 1 })
                drag("header-item-1", outside())
                assertFalse(entries().any { it.getInt("id") == 1 })
                waitFor("singleton returns to bank") { node("header-component-capy") != null }
                assertEquals("Preview never enters durable capture", durable, capture())
                shot("$theme-$size-$device")
                tap("header-edit-done"); waitFor("Done") { !editing() }
                val committed = model().toString()
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(baseline, model().toString())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(committed, model().toString())
                startEditor(); tap("header-size-large"); tap("header-show-footer"); tap("header-edit-cancel")
                assertEquals(committed, model().toString())
            }
    }

    @Test fun compactMenuKeepsItsGripAndNeighborsAndOverflowRemainsMovable() {
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device; restore(); startEditor()
            var stableId = 0
            instrumentation.runOnMainSync { stableId = node("header-item-2")!!.second.id }
            var added = 0
            while (true) {
                var compact = false
                instrumentation.runOnMainSync { compact = node("header-menu-labels-compact") != null }
                if (compact) break
                assertTrue("Menu compacts before capacity", added++ < 30)
                val menu = bounds("header-item-2")
                drag("header-component-space", Offset(menu.right - 2 * density, menu.center.y))
            }
            assertEquals(20f, bounds("header-grip-2").width / density, .5f)
            instrumentation.runOnMainSync { assertEquals("Same item survives compaction", stableId, node("header-item-2")!!.second.id) }
            val neighbor = model().array("zones").getJSONArray(0).objects().last().getInt("id")
            assertNotEquals(2, neighbor)
            assertTrue(bounds("header-item-$neighbor").width > 0)
            shot("compact-$device")
            drag("header-item-$neighbor", center())
            drag("header-item-2", center())
            assertTrue("Menu body moves with Capy retained", model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == 2 })
            assertTrue(entries().any { it.getInt("id") == 1 })
            tap("header-edit-cancel")
            restore("large"); startEditor()
            repeat(22) { edit(obj("type" to "add", "zone" to "left", "before" to null, "item" to obj("kind" to "space"))) }
            waitFor("real whole-item overflow") { node("header-overflow-0") != null }
            tap("header-overflow-0")
            waitFor("overflow chooser") { node("header-overflow-list") != null }
            val hidden = entries().map { it.getInt("id") }.first { id ->
                var exists = false; instrumentation.runOnMainSync { exists = node("header-overflow-item-$id") != null }; exists
            }
            drag("header-overflow-item-$hidden", center())
            assertTrue(model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == hidden })
            tap("header-edit-done")
        }
    }

    @Test fun nativeMenusToolPickerDrawersFooterZenAndRestart() {
        tool = MotionEvent.TOOL_TYPE_FINGER
        restore()
        tap("application-menu-window")
        // Select the shared entry via a real native popup row.
        var menuRoot: ViewRootForTest? = null
        var target: SemanticsNode? = null
        waitFor("Window menu") {
            fun label(node: SemanticsNode): SemanticsNode? = if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == "Customize Title Bar…" } == true) node
                else node.children.firstNotNullOfOrNull(::label)
            WindowInspector.getGlobalWindowViews().flatMap(::roots).any { root -> label(root.semanticsOwner.unmergedRootSemanticsNode)?.let { menuRoot = root; target = it; true } == true }
        }
        pressed = menuRoot
        event(MotionEvent.ACTION_DOWN, target!!.boundsInRoot.center); event(MotionEvent.ACTION_UP)
        waitFor("menu enters inline editor") { editing() && node("title-bar")?.first?.view?.hasWindowFocus() == true }
        SystemClock.sleep(250)
        drag("header-component-tools", center())
        waitFor("picker") { node("tool-picker-search") != null }
        action(obj("type" to "customize", "action" to obj("type" to "picker_search", "query" to "Color")))
        waitFor("Color choice") { node("tool-picker-choice-Brush color") != null }
        tap("tool-picker-choice-Brush color"); tap("tool-picker-confirm")
        val color = entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("kind") == "color" }.getInt("id")
        drag("header-component-workspaces", Offset(bounds("title-bar").right - 150 * density, center().y))
        tap("header-size-large")
        assertEquals("Pill remains compact", 34f, bounds("workspace-switcher").height / density, .5f)
        assertEquals("Pill remains centered", bounds("title-bar").center.y, bounds("workspace-switcher").center.y, density)
        tap("header-show-footer"); tap("header-edit-done")
        var readout = true; instrumentation.runOnMainSync { readout = node("camera-readout") != null }; assertFalse(readout)
        tap("header-control-$color")
        waitFor("shared header Color drawer") { node("tool-drawer") != null }
        tap("header-control-1")
        waitFor("full Zen") { snapshot().optBoolean("chrome_hidden") }
        action(obj("type" to "invoke", "command" to "zen_mode")); waitFor("leave Zen") { !snapshot().optBoolean("chrome_hidden") }
        val committed = model().toString()
        val persisted = capture()
        startEditor(); tap("header-size-small"); tap("header-show-footer")
        assertEquals(persisted, capture())
        scenario.close(); launch()
        assertFalse(editing()); assertEquals("Restart uses committed header", committed, model().toString())
        shot("restart")
    }
    @Test fun sketchDefaultsDrawersFeedbackStatusAndWorkspaceSwitch() {
        tool = MotionEvent.TOOL_TYPE_MOUSE
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        waitFor("Sketch") { view().optString("id") == "builtin:workspace:painter" && node("workspace-switcher") != null }
        val workspace = state().getJSONObject("workspace")
        assertEquals("Sketch has no painter toolbars", 0, workspace.getJSONObject("layout").array("bands").length())
        assertFalse(workspace.getJSONObject("layout").getJSONObject("canvas_info").getBoolean("visible"))
        val tools = entries().filter { it.getJSONObject("item").getString("kind") == "tool" }
        assertEquals(8, tools.size)
        // Transform is an action; the other seven default tools have drawers.
        for (entry in tools.filter { it.getJSONObject("item").getJSONObject("control").optString("command") != "scale_rotate" }) {
            val id = entry.getInt("id")
            tap("header-control-$id")
            // Selecting an inactive tool takes one click; its next click opens
            // the settings drawer, matching shared toolbar activation.
            if (state().getJSONObject("customization").objectOrNull("drawer") == null) tap("header-control-$id")
            waitFor("Sketch drawer $id") { node("tool-drawer") != null && state().getJSONObject("customization").objectOrNull("drawer")
                ?.getJSONObject("anchor")?.optInt("id") == id }
            shot("sketch-drawer-$id")
            // Unused bar space dismisses the drawer without activating a tool.
            // Center is the switcher, so use the free gap beside the first region.
            instrumentation.runOnMainSync { pressed = node("title-bar")!!.first }
            val last = model().array("zones").getJSONArray(0).objects().last().getInt("id")
            val gap = Offset(bounds("header-item-$last").right + 12 * density, center().y)
            event(MotionEvent.ACTION_DOWN, gap); event(MotionEvent.ACTION_UP)
            waitFor("bar gap dismisses drawer") { node("tool-drawer") == null }
        }
        val brush = tools.first { it.getJSONObject("item").getJSONObject("control").optString("command") == "brush" }.getInt("id")
        val filters = tools.first { it.getJSONObject("item").getJSONObject("control").optString("panel") == "adjustments" }.getInt("id")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            tap("header-control-$brush")
            val before = tilePixel("header-control-$brush")
            assertTrue("Selected tool is blue", android.graphics.Color.blue(before) > android.graphics.Color.red(before) + 10)
            down("header-control-$brush")
            assertEquals("Press retains selected blue", before, tilePixel("header-control-$brush"))
            event(MotionEvent.ACTION_UP); idle()
            down("header-control-$filters")
            val action = tilePixel("header-control-$filters")
            assertTrue("Action press stays neutral", kotlin.math.abs(android.graphics.Color.blue(action) - android.graphics.Color.red(action)) < 15)
            shot("$theme-action-feedback")
            event(MotionEvent.ACTION_UP); idle()
        }
        val committed = model().toString()
        val durable = capture()
        startEditor()
        drag("header-component-clock", Offset(bounds("header-item-5").right + 12 * density, center().y))
        drag("header-component-battery", Offset(bounds("title-bar").right - 12 * density, center().y))
        waitFor("native tablet status") { node("system-clock") != null && node("system-battery") != null }
        assertEquals(durable, capture())
        shot("sketch-status-preview")
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        assertFalse(editing()); assertEquals("Switch discards the temporary header", committed, model().toString())
        shot("sketch-default")
    }
    @Test fun keyboardContextHoldAndFocusLossKeepTheirOwnership() {
        restore(); startEditor(); tool = MotionEvent.TOOL_TYPE_MOUSE
        tap("header-item-1")
        instrumentation.runOnMainSync { assertTrue("Pointer selects Capy", node("header-item-1")!!.second.config.getOrNull(SemanticsProperties.Selected) == true) }
        key(KeyEvent.KEYCODE_DPAD_RIGHT)
        assertEquals(1, model().array("zones").getJSONArray(0).getJSONObject(1).getInt("id"))
        key(KeyEvent.KEYCODE_DPAD_RIGHT)
        assertEquals(1, model().array("zones").getJSONArray(1).getJSONObject(0).getInt("id"))
        key(KeyEvent.KEYCODE_FORWARD_DEL)
        assertFalse(entries().any { it.getInt("id") == 1 })
        key(KeyEvent.KEYCODE_ESCAPE)
        assertFalse(editing()); assertTrue(entries().any { it.getInt("id") == 1 })
        startEditor()
        val baseline = model().toString()
        down("header-item-1")
        SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        instrumentation.runOnMainSync { assertNull("Mouse hold never opens a context menu", node("workspace-menu")) }
        event(MotionEvent.ACTION_UP)
        assertEquals(baseline, model().toString())
        button = MotionEvent.BUTTON_SECONDARY
        down("header-item-1"); event(MotionEvent.ACTION_UP)
        waitFor("secondary context menu focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
        shot("secondary-context")
        key(KeyEvent.KEYCODE_BACK)
        waitFor("context closed") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
        idle()
        button = MotionEvent.BUTTON_PRIMARY
        for (device in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            android.util.Log.i("TitleBarAcceptance", "Hold context $device")
            down("header-item-1")
            SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
            waitFor("touch or pen hold context") { node("workspace-menu") != null }
            event(MotionEvent.ACTION_MOVE, center())
            waitFor("drag dismisses held context") { node("workspace-menu") == null }
            event(MotionEvent.ACTION_CANCEL)
            assertEquals(baseline, model().toString())
        }
        tool = MotionEvent.TOOL_TYPE_MOUSE
        down("header-item-1"); event(MotionEvent.ACTION_MOVE, outside())
        var dialog: android.app.Dialog? = null
        scenario.onActivity { activity ->
            dialog = android.app.Dialog(activity).apply {
                setContentView(android.widget.TextView(activity).apply { text = "Capture-loss test" })
                show()
            }
        }
        waitFor("native dialog takes focus") { dialog?.window?.decorView?.hasWindowFocus() == true }
        event(MotionEvent.ACTION_CANCEL)
        instrumentation.runOnMainSync { dialog!!.dismiss() }
        waitFor("activity focus restored") { node("title-bar")?.first?.view?.hasWindowFocus() == true }
        assertEquals("Focus loss cancels the drag", baseline, model().toString())
        tap("header-edit-cancel")
    }

}
