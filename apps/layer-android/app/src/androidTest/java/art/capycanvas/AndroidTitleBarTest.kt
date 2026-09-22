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
import androidx.compose.ui.text.TextLayoutResult
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
    private fun screenBounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync {
            val (root, node) = checkNotNull(node(tag))
            val screen = IntArray(2); root.view.getLocationOnScreen(screen)
            result = node.boundsInRoot.translate(Offset(screen[0].toFloat(), screen[1].toFloat()))
        }
        return checkNotNull(result)
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
        // A settled workspace-manager view alone does not mean that a queued
        // header edit and its snapshot have reached the native owner and UI.
        val published = CountDownLatch(1)
        instrumentation.runOnMainSync { host.query(obj("type" to "catalog")) { published.countDown() } }
        assertTrue("Native UI publication", published.await(15, TimeUnit.SECONDS))
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

    @Test fun zenCapyAndEdgeRevealPreferences() {
        fun preference(id: String, value: Boolean) = action(obj("type" to "preferences", "action" to
            obj("type" to "edit", "id" to id, "value" to value)))
        fun hidden() = snapshot().optBoolean("chrome_hidden")
        fun contact(position: Offset) {
            instrumentation.runOnMainSync { pressed = checkNotNull(node("workspace")).first }
            event(MotionEvent.ACTION_DOWN, position); event(MotionEvent.ACTION_UP); idle()
        }
        action(obj("type" to "restore_settings", "settings" to JSONObject()))
        assertTrue(state().getJSONObject("settings").getBoolean("zen_show_capy"))
        assertFalse(state().getJSONObject("settings").getBoolean("zen_reveal_at_edges"))
        for (theme in listOf("dark", "light")) for (show in listOf(true, false)) for (edges in listOf(false, true)) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "open_settings", "page" to "appearance"))
            // Reveal scrolls the native preferences row into view, then use real contacts.
            for ((id, value) in listOf("zen_show_capy" to show, "zen_reveal_at_edges" to edges)) {
                action(obj("type" to "preferences", "action" to obj("type" to "reveal", "id" to id)))
                waitFor("preference row") { node("preference-$id") != null }
                if (state().getJSONObject("settings").getBoolean(id) != value) {
                    tool = MotionEvent.TOOL_TYPE_FINGER
                    tap("preference-$id")
                    waitFor("switch $id") { state().getJSONObject("settings").getBoolean(id) == value }
                }
            }
            shot("zen-settings-$theme-$show-$edges")
            action(obj("type" to "close_settings"))
            val baseline = layout()
            val camera = state().getJSONObject("camera").toString()
            for (device in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = device
                val workspace = bounds("workspace")
                instrumentation.runOnMainSync { host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(workspace.width / density / 2, workspace.height / density / 2)))) }
                action(obj("type" to "invoke", "command" to "zen_mode"))
                waitFor("hidden chrome $theme/$show/$edges/$device") { hidden() && (node("zen-button") != null) == show }
                shot("zen-$theme-$show-$edges-$device")
                contact(Offset(workspace.center.x, workspace.top + 6 * density))
                waitFor("edge policy $edges device $device") { hidden() == !edges }
                assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
                if (edges) {
                    // Move away from the revealed edge before using the standalone Capy.
                    instrumentation.runOnMainSync { host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(workspace.width / density / 2, workspace.height / density / 2)))) }
                    waitFor("rehide after edge reveal") { hidden() && (!show || node("zen-button") != null) }
                }
                if (show) {
                    tap("zen-button")
                } else {
                    key(KeyEvent.KEYCODE_TAB)
                }
                waitFor("Zen exit") { !state().getJSONObject("workspace").getBoolean("zen_mode") && !hidden() }
                assertEquals(baseline, layout())
                assertEquals(camera, state().getJSONObject("camera").toString())
            }
        }
        preference("zen_show_capy", false)
        preference("zen_reveal_at_edges", true)
        scenario.close(); launch()
        assertFalse(state().getJSONObject("settings").getBoolean("zen_show_capy"))
        assertTrue(state().getJSONObject("settings").getBoolean("zen_reveal_at_edges"))
        android.util.Log.i("ZenAcceptance", "PASS: defaults, switches, all combinations, both themes, touch/mouse/stylus, Capy exit, keyboard exit, unchanged layout/camera, restart persistence")
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
                waitFor("Cancel leaves editor") { !editing() }
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

    @Test fun fullLabelsFitAndMenusAnchorToEachLabelAndEditorActionsAlignRight() {
        tool = MotionEvent.TOOL_TYPE_FINGER
        for (size in listOf("small", "medium", "large")) {
            restore(size)
            for (menu in snapshot().array("application_menus").objects()) {
                val tag = "application-menu-${menu.getString("id")}"
                instrumentation.runOnMainSync {
                    fun textNode(node: SemanticsNode): SemanticsNode? =
                        if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == menu.getString("label") } == true) node
                        else node.children.firstNotNullOfOrNull(::textNode)
                    val label = checkNotNull(textNode(checkNotNull(node(tag)).second))
                    val layouts = mutableListOf<TextLayoutResult>()
                    assertTrue(label.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts))
                    val text = layouts.single()
                    val lastGlyph = text.getBoundingBox(text.layoutInput.text.lastIndex)
                    assertTrue("Final letter of ${menu.getString("label")} fits at $size: glyph=$lastGlyph, size=${text.size}", lastGlyph.right <= text.size.width + .5f)
                    assertEquals("Text is not clipped by its enclosing item", layouts.single().size.width.toFloat(), label.boundsInRoot.width, 1f)
                }
                if (size == "small") {
                    val anchor = screenBounds(tag)
                    tap(tag)
                    waitFor("${menu.getString("id")} popup focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
                    val popup = screenBounds("workspace-menu")
                    assertEquals("Menu starts under its own label", anchor.left, popup.left, 2 * density)
                    assertTrue("Menu is below its label", popup.top >= anchor.bottom - density && popup.top <= anchor.bottom + 12 * density)
                    shot("anchored-${menu.getString("id")}")
                    key(KeyEvent.KEYCODE_BACK)
                    waitFor("menu dismissed") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
                    idle()
                }
            }
            startEditor()
            val actions = bounds("header-editor-actions")
            assertEquals("Editor actions end at the right padding", bounds("title-bar").right - 12 * density, actions.right, density)
            assertEquals("Done is the trailing control", actions.right, bounds("header-edit-done").right, density)
            for (tag in listOf("header-size-small", "header-size-medium", "header-size-large", "header-show-footer", "header-edit-cancel", "header-edit-done")) {
                assertEquals("Controls share a row", bounds("header-edit-done").center.y, bounds(tag).center.y, density)
            }
            shot("aligned-editor-$size")
            tap("header-edit-cancel"); waitFor("Cancel") { !editing() }
        }
    }

    @Test fun compactWorkspaceChoicesAndOverflowIconsFollowTheTitleBar() {
        val initial = view().array("switcher_display").objects().map { it.getString("id") }
        send(obj("type" to "edit_switcher", "edit" to obj("type" to "move", "id" to initial[2], "before" to initial[0])))
        var next = 1
        fun entry(kind: String) = obj("id" to next++, "item" to obj("kind" to kind))
        val left = JSONArray(listOf(entry("capy"), entry("menu")) + List(40) { entry("space") })
        val workspace = entry("workspaces")
        val id = workspace.getInt("id")
        val center = JSONArray(listOf(workspace) + List(30) { entry("space") })
        fixture.getJSONObject("layout").put("header", obj("size" to "small", "next_id" to next,
            "zones" to JSONArray(listOf(left, center, JSONArray()))))
        fun texts(node: SemanticsNode): List<String> =
            (node.config.getOrNull(SemanticsProperties.Text)?.map { it.text } ?: emptyList()) + node.children.flatMap(::texts)
        fun label(node: SemanticsNode, title: String): SemanticsNode? =
            if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == title } == true) node
            else node.children.firstNotNullOfOrNull { label(it, title) }
        fun choose() {
            val choices = view().array("switcher_display").objects()
            val target = choices.first { it.getString("id") != view().getString("id") }
            waitFor("workspace choices") { node("workspace-menu") != null }
            instrumentation.runOnMainSync {
                val (root, popup) = checkNotNull(node("workspace-menu"))
                assertEquals("Only the pill's choices, in configured order", listOf("Workspaces") + choices.map { it.getString("title") }, texts(popup))
                pressed = root
                point = checkNotNull(label(popup, target.getString("title"))).boundsInRoot.center
            }
            event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP)
            idle()
            assertEquals(target.getString("id"), view().getString("id"))
            instrumentation.runOnMainSync { assertNull(node("workspace-menu")) }
        }
        for (theme in listOf("dark", "light")) for ((index, size) in listOf("small", "medium", "large").withIndex()) {
            tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index]
            restore(size)
            action(obj("type" to "set_theme", "theme" to theme))
            waitFor("compact workspace selector") { node("header-control-$id") != null && node("workspace-switcher") == null }
            instrumentation.runOnMainSync {
                val overflow = checkNotNull(node("header-overflow-0")).second
                fun image(node: SemanticsNode): SemanticsNode? =
                    if (node.config.getOrNull(SemanticsProperties.ContentDescription)?.contains("More title bar items") == true) node
                    else node.children.firstNotNullOfOrNull(::image)
                val icon = checkNotNull(image(overflow)).boundsInRoot
                val expected = listOf(20f, 28f, 36f)[index]
                assertEquals("Hamburger width at $size", expected, icon.width / density, .5f)
                assertEquals("Hamburger height at $size", expected, icon.height / density, .5f)
                assertEquals(overflow.boundsInRoot.center.x, icon.center.x, 1f)
                assertEquals(overflow.boundsInRoot.center.y, icon.center.y, 1f)
            }
            tap("header-control-$id")
            shot("workspace-choices-$theme-$size")
            choose()
        }
        // Moving the selector into a crowded region must retain its menu action.
        restore("large")
        var before = 0
        instrumentation.runOnMainSync {
            before = left.objects().first { node("header-item-${it.getInt("id")}") == null }.getInt("id")
        }
        edit(obj("type" to "move", "id" to id, "zone" to "left", "before" to before))
        waitFor("hidden workspace selector") { node("header-item-$id") == null }
        tap("header-overflow-0")
        tap("header-overflow-item-$id")
        choose()
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
    @Test fun photoDefaultColumnsAndPaintRestorationSurviveRestart() {
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        assertEquals("builtin:workspace:photographer", view().getString("id"))
        val before = capture()
        send(obj("type" to "form", "kind" to "reset"))
        waitFor("latest default preview") { node("panel-body-color") != null && node("panel-body-layers") != null }
        assertEquals("Preview is not saved", before, capture())
        send(obj("type" to "cancel"))
        assertEquals("Cancel retains the current layout", before, capture())
        send(obj("type" to "form", "kind" to "reset"))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        fun checkColumns() {
            waitFor("primary Photo panels") { node("panel-body-color") != null && node("panel-body-properties") != null && node("panel-body-layers") != null }
            val color = bounds("group-14")
            val properties = bounds("group-15")
            val layers = bounds("group-16")
            val strip = bounds("collapsed-column-4")
            assertEquals(color.left, properties.left, 1f)
            assertEquals(properties.left, layers.left, 1f)
            assertTrue(color.bottom < properties.top && properties.bottom < layers.top)
            assertEquals("Secondary strip is immediately inward from the outer column", color.left, strip.right + 6 * density, density)
            val icons = listOf("brushes", "tool_settings", "sizes", "navigator").map { bounds("column-icon-$it") }
            assertTrue(icons.zipWithNext().all { (a, b) -> a.bottom < b.top })
            instrumentation.runOnMainSync { assertNull("Secondary column starts closed", node("group-6")) }
        }
        checkColumns()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            tap("tab-stats"); waitFor("Diagnostics tab") { node("panel-body-stats") != null }
            tap("tab-color")
            tap("tab-adjustments"); waitFor("Filters tab") { node("panel-body-adjustments") != null }
            tap("tab-properties")
            checkColumns(); shot("photo-default-$theme")
        }
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            for (panel in listOf("brushes", "tool_settings", "sizes", "navigator")) {
                tap("column-icon-$panel")
                waitFor("secondary $panel opens") { node("panel-body-$panel") != null }
                assertTrue(bounds("group-6").right < bounds("collapsed-column-4").left)
                assertNotNull(bounds("panel-body-color"))
                tap("column-icon-$panel")
                checkColumns()
            }
        }
        val committed = layout()
        scenario.close(); launch()
        checkColumns()
        assertEquals("Default arrangement survives restart", committed, layout())
        shot("photo-default-restart")
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "kind" to "reset"))
        tap("workspace-submit")
        waitFor("original Paint panels") { node("panel-body-brushes") != null && node("panel-body-color") != null && node("panel-body-navigator") != null }
        assertEquals("Paint restores its left panel column", bounds("group-6").left, bounds("group-10").left, 1f)
        assertTrue(bounds("group-10").right < bounds("group-14").left)
        assertTrue(bounds("group-14").right < bounds("collapsed-column-12").left)
        shot("paint-original-restored")
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
        val brush = tools.first { it.getJSONObject("item").getJSONObject("control").optString("command") == "drawing_brush" }.getInt("id")
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
    @Test fun brushAndSculptDrawersKeepIndependentSelections() {
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        fun header(command: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("command") == command
        }.getInt("id")
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun checkColumns(first: String) {
            assertEquals("[[\"$first\"],[\"tools\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
            val set = state().getJSONObject("tool_panels").getJSONObject(first).array("groups").objects().first().getString("label")
            val choice = state().getJSONObject("tool_set").array("subtools").objects().first().getString("label")
            waitFor("$first drawer content laid out") {
                (node("$first-$set")?.second?.boundsInRoot?.height ?: 0f) >= 48 * density &&
                    (node("subtool-$choice")?.second?.boundsInRoot?.width ?: 0f) > 0f
            }
            val a = bounds("$first-$set"); val b = bounds("subtool-$choice")
            assertTrue("Sets $a should be narrower than tools $b", a.width < b.width)
            assertTrue(state().array("tool_settings").length() > 0)
            assertNull("Tools has no category headers", node("tool-group-" + if(first == "sculpt_sets") "Blend" else "Paint"))
        }
        val brush = header("drawing_brush")
        val sculpt = header("sculpt")
        tap(brush)
        if (drawer() == null) tap(brush)
        checkColumns("brush_sets")
        assertEquals(10, state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").length())
        assertFalse(state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").objects().any { it.getString("label") in listOf("Eraser", "Blend", "Liquify") })
        for ((device, label) in listOf(MotionEvent.TOOL_TYPE_MOUSE to "Pencil", MotionEvent.TOOL_TYPE_FINGER to "Pastel", MotionEvent.TOOL_TYPE_STYLUS to "Paint")) {
            tool = device
            assertTrue(bounds("brush_sets-$label").height / density >= 48f)
            tap("brush_sets-$label")
            checkColumns("brush_sets")
            val choice = state().getJSONObject("tool_set").array("subtools").objects().first()
            tap("subtool-${choice.getString("label")}")
            assertEquals(choice.getInt("preview"), state().getJSONObject("brush").getInt("preset"))
        }
        action(obj("type" to "set_brush_size", "value" to 37))
        val drawing = state().getJSONObject("brush").getInt("preset")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); shot("brush-drawer-$theme")
        }
        tap(sculpt)
        checkColumns("sculpt_sets")
        assertEquals(listOf("Blend", "Liquify"), state().getJSONObject("tool_panels").getJSONObject("sculpt_sets").array("groups").objects().map { it.getString("label") })
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            for (label in listOf("Liquify", "Blend")) {
                tap("sculpt_sets-$label"); checkColumns("sculpt_sets")
                assertEquals(label.lowercase(), state().getJSONObject("brush").getString("tool"))
            }
        }
        tap("sculpt_sets-Liquify")
        action(obj("type" to "set_brush_size", "value" to 79))
        val sculpting = state().getJSONObject("brush").getInt("preset")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); shot("sculpt-drawer-$theme")
        }
        tap(brush)
        assertEquals(drawing, state().getJSONObject("brush").getInt("preset"))
        assertEquals(37.0, state().getJSONObject("brush").getDouble("diameter"), .01)
        tap(header("eraser"))
        assertEquals("[[\"tools\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
        val eraserChoice = state().getJSONObject("tool_set").array("subtools").objects().first().getString("label")
        waitFor("Eraser tools laid out") { (node("subtool-$eraserChoice")?.second?.boundsInRoot?.width ?: 0f) > 0f }
        assertNull("Eraser has no category header", node("tool-group-Eraser"))
        shot("eraser-drawer")
        assertFalse(state().array("commands").objects().filter { it.getString("id") in listOf("drawing_brush", "sculpt") }.any { it.getBoolean("selected") })
        tap(sculpt)
        assertEquals(sculpting, state().getJSONObject("brush").getInt("preset"))
        assertEquals(79.0, state().getJSONObject("brush").getDouble("diameter"), .01)
        tap(sculpt); assertNull(drawer())
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        // The workspace is adopted after send's publication fence. Wait for its
        // native header measurements before delivering the next contact.
        idle()
        tap(brush)
        assertEquals(drawing, state().getJSONObject("brush").getInt("preset"))
        assertNull("First contact selects Brush", drawer())
        tap(brush); checkColumns("brush_sets")
    }

    @Test fun filterDrawerLayersAndPenScrolling() {
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        fun header(panel: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("panel") == panel
        }.getInt("id")
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun layer(op: String, id: Long) = action(obj("type" to "layer", "action" to obj("op" to op, "id" to id, "mask" to false)))
        val filters=header("adjustments")
        tap(filters)
        assertEquals("[[\"filter_types\"],[\"adjustments\"],[\"properties\"]]",drawer()!!.getJSONArray("columns").toString())
        val choices=state().array("adjustments").objects().take(2)
        val count=state().array("layers").length()
        var selected=0L
        for((i,device) in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS).withIndex()) {
            tool=device
            val id=choices[i%2].getString("id")
            waitFor("Visible filter $id") { (node("adjustment-$id")?.second?.boundsInRoot?.height ?: 0f)>0 }
            tap("adjustment-$id")
            waitFor("Selected filter $id") { state().getJSONObject("filter_picker").optString("selected")==id }
            val next=state().getJSONObject("layer_properties").getLong("layer")
            if(selected!=0L)assertEquals(selected,next) else selected=next
            assertEquals(count+1,state().array("layers").length())
            assertEquals(id,state().getJSONObject("filter_picker").getString("selected"))
            assertTrue(state().array("layers").objects().first { it.getLong("id")==1L }.getBoolean("drawing"))
        }
        tap(filters);assertNull(drawer());tap(filters)
        assertEquals(selected,state().getJSONObject("layer_properties").getLong("layer"))
        for(theme in listOf("light","dark")) { action(obj("type" to "set_theme","theme" to theme));shot("filter-drawer-$theme") }
        tap("cancel-filter");assertNull(drawer());assertEquals(count,state().array("layers").length())
        tap(filters)
        layer("select",2)
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(.06,.08,.12,1.0))))
        tap("paper-color-bucket")
        val paper=state().array("layers").objects().first { it.getLong("id")==2L }
        assertEquals("layer-paper-symbolic",paper.getString("content_icon"))
        assertFalse(paper.getString("description").contains("Protected"))
        assertFalse(state().getJSONObject("layer_tools").getJSONObject("controls").getBoolean("opacity"))
        shot("paper-properties")
        tap(header("layers"))
        fun swipe(id: Long, dx: Float) {
            down("layer-row-$id");val start=point
            for(i in 1..5)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/5,0f))
            event(MotionEvent.ACTION_UP);idle()
        }
        tool=MotionEvent.TOOL_TYPE_MOUSE
        swipe(1,-90f)
        assertNull("Mouse row drag does not reveal Delete",node("layer-delete-1"))
        for((id,device) in listOf(1L to MotionEvent.TOOL_TYPE_STYLUS,2L to MotionEvent.TOOL_TYPE_FINGER)) {
            tool=device
            swipe(id,-90f)
            waitFor("Delete revealed") { node("layer-delete-$id")!=null }
            assertTrue(bounds("layer-delete-$id").width>=70*density)
            swipe(id,90f)
            waitFor("Reverse closes Delete") { node("layer-delete-$id")==null }
            swipe(id,-90f)
            shot("layer-delete-$id")
            tap("layer-delete-$id")
            assertFalse(state().array("layers").objects().any { it.getLong("id")==id })
        }
        assertEquals(0,state().array("layers").length())
        action(obj("type" to "invoke","command" to "undo"))
        action(obj("type" to "invoke","command" to "undo"))
        assertEquals(2,state().array("layers").length())
        // Constrain the native viewport so this small catalog actually overflows.
        instrumentation.runOnMainSync { host.resize((1200*density).toInt(),(450*density).toInt(),density) }
        idle()
        val brush="header-control-"+entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("command")=="drawing_brush" }.getInt("id")
        tap(brush)
        if(drawer()==null)tap(brush)
        val sets=state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").objects()
        var longest=sets.first();var size=0
        for(set in sets) {
            action(set.getJSONObject("action"))
            val count=state().getJSONObject("tool_set").array("subtools").length()
            if(count>size){size=count;longest=set}
        }
        action(longest.getJSONObject("action"))
        val first="subtool-"+state().getJSONObject("tool_set").array("subtools").getJSONObject(0).getString("label")
        for(device in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)) {
            tool=device
            // Reopen to reset the native scroll position before each contact.
            tap(brush);tap(brush)
            waitFor("Tools shown") { (node(first)?.second?.boundsInRoot?.height ?: 0f)>0 }
            val before=bounds(first).top
            down(first);val start=point
            for(i in 1..6)event(MotionEvent.ACTION_MOVE,start+Offset(0f,-35*density*i))
            event(MotionEvent.ACTION_UP);idle()
            val after=node(first)?.second?.boundsInRoot
            if(device==MotionEvent.TOOL_TYPE_MOUSE)assertEquals(before,after!!.top,1f)
            else assertTrue("Touch and pen scroll tool choices ($device): $before -> $after",after==null || after.height==0f || after.top<before-10*density)
        }
        shot("pen-tools-scroll")
        android.util.Log.i("FilterAcceptance","PASS: three panels, replacement/reopen/cancel, paper color, mouse/touch/pen, reversible swipe, final layer deletion/undo, tool scrolling")
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
            waitFor("held contact releases native focus") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
            idle()
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
