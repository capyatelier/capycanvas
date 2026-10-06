package art.capycanvas

import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.core.app.ActivityScenario
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*

/** Real Compose/native-view contacts on isolated workspaces and preferences. */
class AndroidTitleBarTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private var pressed: ViewRootForTest? = null
    private var point = Offset.Zero
    private var downAt = 0L
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var button = MotionEvent.BUTTON_PRIMARY
    private var density = 1f
    private fun view() = host.workspaceManager!!
    private fun layout() = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").toString()
    private fun node(tag: String) = findTag(tag)
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
    private fun waitFor(label: String, timeout: Long = 15000, condition: () -> Boolean) =
        host.awaitMain(label, timeout, {
            shot("failure-${label.replace(Regex("[^A-Za-z0-9-]"), "-")}")
            "${view()}; customization=${state().getJSONObject("customization")}" }) {
            host.snapshot?.objectOrNull("state")?.let { assertTrue(it.optString("host_error"), it.isNull("host_error")) }
            condition()
        }
    private fun idle() {
        // A settled workspace-manager view alone does not mean that a queued
        // header edit and its snapshot have reached the native owner and UI.
        host.drain()
        SystemClock.sleep(220)
        waitFor("workspace idle") { !view().optBoolean("busy") && !view().optBoolean("switcher_busy") && !view().optBoolean("dirty") }
        assertTrue(view().toString(), view().isNull("error")); assertTrue(view().toString(), view().isNull("switcher_error"))
    }
    private fun send(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; idle() }
    private fun capture() = host.workspaceCapture()
    private fun event(action: Int, next: Offset = point, meta: Int = 0) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) downAt = SystemClock.uptimeMillis()
        val event = motion(tool, action, next, downAt, button, meta)
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
    private fun key(code: Int, meta: Int = 0) { pressKey(code, meta); SystemClock.sleep(220) }

    private lateinit var fixture: JSONObject
    private fun snapshot() = host.snapshot!!
    private fun state() = snapshot().getJSONObject("state")
    private fun model() = snapshot().getJSONObject("header").getJSONObject("model")
    private fun entries() = model().array("zones").values().flatMap { (it as JSONArray).objects() }
    private fun editing() = snapshot().getJSONObject("header").optBoolean("editing")
    private fun action(value: JSONObject) { host.drain(value); SystemClock.sleep(250) }
    private fun edit(value: JSONObject) = action(obj("type" to "customize", "action" to obj("type" to "header", "action" to value)))
    private fun restore(size: String = "small") {
        val value = JSONObject(fixture.toString())
        value.getJSONObject("layout").getJSONObject("header").put("size", size)
        action(obj("type" to "restore_workspace", "workspace" to value))
        waitFor("bar settled") { node("header-item-1") != null && !editing() }
        idle()
    }
    private fun launch() {
        scenario = launchCapy(90_000)
        scenario.onActivity { host = it.host; density = it.resources.displayMetrics.density }
        idle()
    }
    @Before fun ready() {
        launch()
        fixture = JSONObject(state().getJSONObject("workspace").toString())
        fixture.put("zen_mode", false)
        fixture.getJSONObject("layout").apply {
            for (name in listOf("bands", "floating", "collapsed", "column_stacks", "column_scroll", "fit_tab_groups", "fit_height_groups")) put(name, JSONArray())
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
        if (::scenario.isInitialized) scenario.close()
    }
    private fun shot(name: String) = screenshot("validation/title-bar/$name.png")
    private fun tilePixel(tag: String): Int {
        var position = Offset.Zero
        instrumentation.runOnMainSync {
            val (root, node) = checkNotNull(node(tag))
            val screen = IntArray(2); root.view.getLocationOnScreen(screen)
            position = Offset(screen[0] + node.boundsInRoot.left + 8 * density, screen[1] + node.boundsInRoot.center.y)
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
    private fun tapMenuRow(label: String) {
        var menuRoot: ViewRootForTest? = null
        var target: SemanticsNode? = null
        waitFor("$label row") {
            fun text(node: SemanticsNode): Boolean = node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == label } == true ||
                node.children.filter { it.config.getOrNull(SemanticsActions.OnClick) == null }.any(::text)
            fun row(node: SemanticsNode): SemanticsNode? = if (node.config.getOrNull(SemanticsActions.OnClick) != null && text(node)) node
                else node.children.firstNotNullOfOrNull(::row)
            semanticsRoots().any { root -> root.find(hasTag("workspace-menu"))?.let(::row)?.let { menuRoot = root; target = it; true } == true }
        }
        pressed = menuRoot
        event(MotionEvent.ACTION_DOWN, target!!.boundsInRoot.center); event(MotionEvent.ACTION_UP)
    }
    private fun variantMenu(anchor: JSONObject): JSONObject {
        val done = java.util.concurrent.CountDownLatch(1)
        var result: JSONObject? = null
        instrumentation.runOnMainSync {
            host.query(obj("type" to "context", "target" to obj("kind" to "tool_variants", "anchor" to anchor))) {
                result = it as? JSONObject; done.countDown()
            }
        }
        assertTrue(done.await(10, java.util.concurrent.TimeUnit.SECONDS))
        return checkNotNull(result)
    }
    private fun variantRows(anchor: JSONObject) = variantMenu(anchor).array("sections").values().flatMap { (it as JSONArray).objects() }
    private fun openToolContext(tag: String) {
        button = if (tool == MotionEvent.TOOL_TYPE_MOUSE) MotionEvent.BUTTON_SECONDARY else MotionEvent.BUTTON_PRIMARY
        down(tag)
        if (tool != MotionEvent.TOOL_TYPE_MOUSE) SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        event(MotionEvent.ACTION_UP)
        button = MotionEvent.BUTTON_PRIMARY
        waitFor("tool context opens") { node("workspace-menu") != null }
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
        val theme = "dark"
        for ((show, edges) in listOf(true to false, true to true, false to false, false to true)) {
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
            val devices = if (show && !edges) listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS) else listOf(MotionEvent.TOOL_TYPE_FINGER)
            for (device in devices) {
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
        android.util.Log.i("ZenAcceptance", "PASS: defaults, switches, all combinations, touch/mouse/stylus, Capy exit, keyboard exit, unchanged layout/camera, restart persistence")
    }

    @Test fun bankBodiesGripsCancellationAndHistoryEveryDeviceAndSize() {
        val theme = "light"
        for (size in listOf("small", "medium", "large"))
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
                    val frame = android.graphics.Rect()
                    instrumentation.runOnMainSync { checkNotNull(node("title-bar")).first.view.getWindowVisibleDisplayFrame(frame) }
                    if (popup.height <= frame.bottom - anchor.bottom - 48 * density)
                        assertTrue("${menu.getString("id")} is below its label: $popup / $anchor", popup.top >= anchor.bottom - density && popup.top <= anchor.bottom + 12 * density)
                    else
                        assertTrue("Tall menus fit the viewport: $popup / $frame", popup.top >= frame.top - density && popup.bottom <= frame.bottom + density)
                    if (menu.getString("id") == "select") instrumentation.runOnMainSync {
                        fun row(label: String): SemanticsNode? {
                            fun find(n: SemanticsNode): SemanticsNode? =
                                if (n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == label } == true) n
                                else n.children.firstNotNullOfOrNull(::find)
                            return find(checkNotNull(node("workspace-menu")).first.semanticsOwner.rootSemanticsNode)
                        }
                        for (label in listOf("Load Selection", "Replace Selection Layer from Current Selection"))
                            assertTrue("$label is unavailable without saved layers", checkNotNull(row(label)).config.contains(SemanticsProperties.Disabled))
                        for (label in listOf("Grow Selection…", "Shrink Selection…", "Feather Selection…", "Border Selection…", "Smooth Selection…", "Transform Selection Outline")) assertNotNull(label, row(label))
                        assertNull(row("Modify"))
                    }
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
                assertEquals("Only the pill's choices, in configured order", listOf(view().getJSONObject("switcher_menu").getString("title")) + view().getJSONObject("switcher_menu").array("sections").values().flatMap { (it as JSONArray).objects().map { row -> row.getString("label") } }, texts(popup))
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
        tapMenuRow("Customize Title Bar…")
        waitFor("menu enters inline editor") { editing() && node("title-bar")?.first?.view?.hasWindowFocus() == true }
        SystemClock.sleep(250)
        drag("header-component-tools", center())
        waitFor("picker") { node("tool-picker-search") != null }
        action(obj("type" to "customize", "action" to obj("type" to "picker_search", "query" to "Brush color")))
        waitFor("Color choice") { node("tool-picker-choice-Brush color") != null }
        tap("tool-picker-choice-Brush color"); tap("tool-picker-confirm")
        val color = entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("kind") == "color" }.getInt("id")
        drag("header-component-workspaces", Offset(bounds("title-bar").right - 150 * density, center().y))
        tap("header-size-large")
        assertEquals("Pill remains compact", 36f, bounds("workspace-switcher").height / density, .5f)
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
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("latest default preview") { node("panel-body-color") != null && node("panel-body-layers") != null }
        assertEquals("Preview is not saved", before, capture())
        send(obj("type" to "cancel"))
        assertEquals("Cancel retains the current layout", before, capture())
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
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
            tap("tab-palettes"); waitFor("Palettes tab") { node("panel-body-palettes") != null }
            tap("tab-color")
            tap("tab-adjustments"); waitFor("Filters tab") { node("panel-body-adjustments") != null }
            tap("tab-properties")
            checkColumns(); shot("photo-default-$theme")
        }
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            for (panel in listOf("brushes", "tool_settings", "sizes", "navigator")) {
                tap("column-icon-$panel")
                waitFor("secondary $panel drawer opens") {
                    node("column-drawer-4") != null && state().getJSONObject("customization").array("column_drawers").objects()
                        .any { it.getJSONObject("anchor").optString("origin") == panel }
                }
                assertTrue(bounds("column-drawer-4").right < bounds("collapsed-column-4").left)
                assertNotNull(bounds("panel-body-color"))
                tap("column-icon-$panel")
                waitFor("secondary $panel drawer closes") { node("column-drawer-4") == null }
                checkColumns()
            }
        }
        val committed = layout()
        scenario.close(); launch()
        checkColumns()
        assertEquals("Default arrangement survives restart", committed, layout())
        shot("photo-default-restart")
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        tap("workspace-submit")
        waitFor("original Paint panels") { node("panel-body-brushes") != null && node("panel-body-color") != null && node("panel-body-navigator") != null }
        assertEquals("Paint restores its left panel column", bounds("group-6").left, bounds("group-10").left, 1f)
        assertTrue(bounds("group-10").right < bounds("group-14").left)
        assertTrue(bounds("group-14").right < bounds("collapsed-column-12").left)
        shot("paint-original-restored")
    }

    @Test fun paintColorAndNavigatorFitTheirContent() {
        waitFor("filter library", 60_000) { !state().getJSONObject("filter_load").optBoolean("pending") }
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        waitFor("restored fitted groups") { state().getJSONObject("workspace").getJSONObject("layout").getJSONArray("fit_height_groups").toString() == "[10,14]" }
        fun measured(panel: String) = snapshot().array("panel_measurements").objects().first { it.getString("panel") == panel }.number("content_height")
        fun reserved(group: String) = snapshot().getJSONObject("layout").array("groups").objects().first { it.getInt("id") == group.removePrefix("group-").toInt() }
            .array("panels").values().mapNotNull { panel -> snapshot().array("panel_measurements").objects().firstOrNull { it.getString("panel") == panel } }
            .maxOfOrNull { m -> m.optJSONObject("scroll")?.let { s -> minOf(s.number("fixed_height") + 4 * s.number("unit_height").takeIf { it > 0f }.let { it ?: 36f }, m.number("content_height")) } ?: m.number("content_height") }
        fun height(tag: String) = node(tag)!!.second.boundsInRoot.height / density
        fun fitted(label: String): Float {
            var settled = 0L
            var previous = ""
            waitFor("fitted Paint columns $label", 30_000) {
                if (listOf("group-6", "group-7", "group-10", "group-14", "navigator-overview").any { node(it) == null }) return@waitFor false
                val colors = reserved("group-10") ?: return@waitFor false
                val navigation = reserved("group-14") ?: return@waitFor false
                val tab = state().getJSONArray("tabs").getJSONObject(0)
                val aspect = (tab.getInt("height").toFloat() / tab.getInt("width")).coerceIn(.25f, 1f)
                val overview = node("navigator-overview")!!.second.boundsInRoot
                val sample = listOf(height("group-6"), height("group-7"), height("group-10"), height("group-14"), overview.height / density, colors, navigation).joinToString()
                val fits = kotlin.math.abs(height("group-6") - height("group-7")) <= 1f &&
                    kotlin.math.abs(height("group-10") - colors - 36f) <= 1f &&
                    kotlin.math.abs(height("group-14") - navigation - 36f) <= 1f &&
                    kotlin.math.abs(overview.height - overview.width * aspect) <= 2f * density
                if (!fits || sample != previous) { previous = sample; settled = SystemClock.uptimeMillis() }
                fits && SystemClock.uptimeMillis() - settled > 1000
            }
            shot("paint-fitted-$label")
            return measured("color")
        }
        val sdr = fitted("sdr")
        val defaults = state().getJSONObject("settings").getJSONObject("new_document").getJSONObject("defaults")
        defaults.put("extent", JSONArray(listOf(900, 1200))).getJSONObject("color").put("depth", "F16")
        action(obj("type" to "new_document_preferences", "action" to obj("type" to "remember", "options" to defaults, "name" to "", "defaults" to true)))
        action(obj("type" to "invoke", "command" to "new_document"))
        waitFor("new document dialog") { node("new-document-create") != null }
        tap("new-document-create")
        waitFor("HDR portrait document", 60_000) {
            val tab = state().getJSONArray("tabs").getJSONObject(0)
            tab.getInt("width") == 900 && tab.getInt("height") == 1200 && !state().getJSONObject("document_file").optBoolean("busy")
        }
        assertTrue("HDR Color panel is taller", fitted("hdr") > sdr + 10f)
        action(obj("type" to "invoke", "command" to "close_document"))
    }

    @Test fun colorExpansionClosesPromptlyWithoutRemeasuringTheWheel() {
        waitFor("filter library", 60_000) { !state().getJSONObject("filter_load").optBoolean("pending") }
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        waitFor("docked Color panel") { node("panel-body-color") != null && node("color-wheel") != null }
        SystemClock.sleep(1000)
        fun measured() = snapshot().array("panel_measurements").objects().first { it.getString("panel") == "color" }.number("content_height")
        val docked = bounds("group-10")
        val natural = measured()
        repeat(2) { round ->
            action(obj("type" to "select_panel_tab", "group" to 10, "panel" to "color"))
            waitFor("expanded Color $round") {
                state().getJSONObject("customization").optString("expanded") == "color" && (node("group-10")?.second?.boundsInRoot?.width ?: 0f) > docked.width + 100f
            }
            SystemClock.sleep(400)
            val start = SystemClock.uptimeMillis()
            instrumentation.runOnMainSync { host.customize(obj("type" to "close_expanded")) }
            waitFor("collapsed Color $round", 3_000) {
                node("group-10")?.second?.boundsInRoot?.let { r ->
                    kotlin.math.abs(r.top - docked.top) < 1f && kotlin.math.abs(r.width - docked.width) < 1f && kotlin.math.abs(r.height - docked.height) < 1f
                } == true
            }
            val elapsed = SystemClock.uptimeMillis() - start
            assertTrue("Collapse follows its own motion, not a remeasurement loop: $elapsed ms", elapsed < 800)
            SystemClock.sleep(500)
            assertEquals("Fitting the wheel in motion keeps its reported natural height", natural, measured(), 0f)
            assertEquals(docked, bounds("group-10"))
        }
    }

    @Test fun sketchDefaultsDrawersFeedbackStatusAndWorkspaceSwitch() {
        tool = MotionEvent.TOOL_TYPE_MOUSE
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        waitFor("Sketch") { view().optString("id") == "builtin:workspace:painter" && node("workspace-switcher") != null }
        val workspace = state().getJSONObject("workspace")
        val bands = workspace.getJSONObject("layout").array("bands").objects()
        assertEquals("Sketch docks only its compact brush toolbar", listOf(listOf("left", "center", 1)),
            bands.map { listOf(it.getString("edge"), it.optString("alignment"), it.getJSONObject("root").array("panels").length()) })
        val sliders = workspace.getJSONObject("layout").array("panels").objects()
            .first { it.getString("id") == bands[0].getJSONObject("root").array("panels").getString(0) }
            .getJSONObject("content").array("tiles").objects().map { it.getJSONObject("control").getString("kind") }
        assertTrue("Compact brush sliders: $sliders", "brush_size_slider" in sliders && "brush_opacity_slider" in sliders)
        assertFalse(workspace.getJSONObject("layout").getJSONObject("canvas_info").getBoolean("visible"))
        val tools = entries().filter { it.getJSONObject("item").getString("kind") == "tool" }
        assertEquals(8, tools.size)
        val gap = snapshot().getJSONObject("header").array("sizes").objects().first { it.getString("id") == model().getString("size") }.number("gap")
        val (first, second) = model().array("zones").values().flatMap { (it as JSONArray).objects().zipWithNext() }
            .first { pair -> pair.toList().all { it.getJSONObject("item").getString("kind") == "tool" } }.toList()
            .map { bounds("header-control-${it.getInt("id")}") }
        assertEquals("Joined tiles use the toolbar tile gap", gap, (second.left - first.right) / density, .5f)
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
            val explicit = state().getJSONObject("customization").getJSONObject("drawer").getString("dismissal") == "explicit"
            instrumentation.runOnMainSync { pressed = node("title-bar")!!.first }
            val last = model().array("zones").getJSONArray(0).objects().last().getInt("id")
            val gap = Offset(bounds("header-item-$last").right + 12 * density, center().y)
            event(MotionEvent.ACTION_DOWN, gap); event(MotionEvent.ACTION_UP)
            if (explicit) {
                idle(); assertNotNull("Explicit drawer $id ignores the bar gap", node("tool-drawer"))
                tap("header-control-$id")
            }
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
    @Test fun selectionDrawerToolsModesAndRememberedIcons() {
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        fun header(command: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("command") == command
        }.getInt("id")
        fun command(id: String) = state().array("commands").objects().first { it.getString("id") == id }
        fun invoke(id: String) = action(obj("type" to "invoke", "command" to id))
        val select = header("select")
        tap(select)
        if (state().getJSONObject("customization").isNull("drawer")) tap(select)
        assertEquals("[[\"tools\"],[\"tool_settings\"]]", state().getJSONObject("customization").getJSONObject("drawer").getJSONArray("columns").toString())
        val choices = state().getJSONObject("tool_set").array("subtools").objects()
        assertEquals(8, choices.size)
        val modes = listOf("selection_new", "selection_add", "selection_subtract", "selection_intersect")
        for ((i, choice) in choices.withIndex()) {
            tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[i % 3]
            val tag = "subtool-${choice.getString("label")}"
            assertTrue("Touch-friendly tool row", bounds(tag).height / density >= 48f)
            tap(tag)
            assertEquals(choice.getString("icon"), command("select").getString("icon"))
            assertFalse(state().getJSONObject("customization").isNull("drawer"))
            val brush = choice.getString("icon") == "selection-brush"
            assertNotNull(node("tool-setting-" + if(brush)"selection_brush_size" else "selection_feather"))
            if(!brush && choice.getString("icon")!="tonal-select") assertNotNull(node("tool-action-selection_antialias"))
            val availableModes = if(brush) listOf("selection_add", "selection_subtract") else modes
            val row = bounds("selection-mode-row")
            for (id in availableModes) {
                val button = bounds("tool-action-$id")
                assertEquals(row.top, button.top, 1f)
                assertTrue(button.right <= row.right + 1f)
                tap("tool-action-$id")
                assertTrue(command(id).getBoolean("selected"))
                assertEquals(1, availableModes.count { command(it).getBoolean("selected") })
                assertNull("Modes have no caption", node("tool-action-$id")!!.second.config.getOrNull(SemanticsProperties.Text))
            }
        }
        invoke("rectangle_select"); tap("tool-action-selection_new"); tap("tool-action-selection_fixed_size")
        assertNotNull(node("tool-setting-selection_width")); assertNotNull(node("tool-setting-selection_height"))
        tap("tool-action-selection_fixed_size")
        invoke("color_select")
        for (id in listOf("tolerance", "expansion", "smoothing")) assertNotNull(node("tool-setting-$id"))
        for (id in listOf("selection_visible", "selection_editing", "selection_reference")) assertNotNull(node("tool-action-$id"))
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); shot("selection-drawer-$theme")
        }
        tap(header("drawing_brush"))
        assertEquals("color-select", command("select").getString("icon"))
        tap(select); assertTrue(command("color_select").getBoolean("selected"))
        tap(select)
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        for (slot in listOf("marquee", "lasso", "automatic_selection"))
            assertTrue("Photo toolbar $slot", layout().contains("\"slot\":\"$slot\""))
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        idle(); tap(select)
        assertTrue(command("color_select").getBoolean("selected"))
    }

    @Test fun commandGroupsProjectTheirOwnChoicesIconsAndSelectionScope() {
        device.landscape(scenario); idle()
        fun resetPreset(id: String) {
            send(obj("type" to "switch", "id" to id))
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to id))) }
            host.drain()
            waitFor("starting group layout confirmation") { node("workspace-submit") != null }
            tap("workspace-submit")
            waitFor("starting group layout confirmation closes") { node("workspace-submit") == null }
            idle()
        }
        fun tiles() = snapshot().array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects()
        fun command(id: String) = state().array("commands").objects().first { it.getString("id") == id }
        fun header(id: Int) = snapshot().getJSONObject("header").array("items").objects().first { it.getInt("id") == id }
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun rowCommand(row: JSONObject) = row.getJSONObject("action").let {
            if (it.optString("type") == "choose_tool_variant") it.getJSONObject("variant").optString("command") else it.optString("command")
        }
        fun choose(anchor: JSONObject, tag: String, row: JSONObject, capture: String? = null) {
            assertTrue(row.optBoolean("enabled", true))
            openToolContext(tag)
            capture?.let { idle(); shot(it) }
            tapMenuRow(row.getString("label"))
            waitFor("group menu closes") { node("workspace-menu") == null }
            idle()
            assertEquals(row.getString("label"), variantRows(anchor).single { it.optBoolean("selected") }.getString("label"))
        }
        fun brushGroup(row: JSONObject) = row.getJSONObject("action").getJSONObject("variant").optString("group")
        val mediaIcons = mapOf("marker" to "marker", "pastel" to "pastel", "watercolor" to "watercolor", "oil" to "oil-paint", "spray" to "spray")
        val media = mapOf("pen" to listOf("marker"), "pencil" to listOf("pastel"),
            "brush" to listOf("watercolor", "oil"), "airbrush" to listOf("spray"))
        val mediaGroups = mapOf("pen" to setOf("pen", "marker"), "pencil" to setOf("pencil", "pastel"),
            "brush" to setOf("paint", "watercolor", "oil"), "airbrush" to setOf("airbrush", "spray"))
        resetPreset("builtin:workspace:illustrator")
        waitFor("Paint command groups") { snapshot().array("panels").objects().firstOrNull { it.getString("id") == "toolbar" }
            ?.array("tiles")?.objects()?.any { it.getJSONObject("control").optString("command") == "pen" } == true }
        val paintCommands = tiles().map { it.getJSONObject("control").optString("command") }
            .filter { it in listOf("pen", "pencil", "brush", "airbrush", "decoration", "eraser", "blend", "liquify", "clone", "heal", "spot_heal") }
        assertTrue(paintCommands.containsAll(listOf("pen", "pencil", "brush", "airbrush", "eraser")))
        edit(obj("type" to "edit", "editing" to true))
        for (entry in entries().filter { it.getJSONObject("item").getString("kind") !in listOf("capy", "settings") })
            edit(obj("type" to "remove", "id" to entry.getInt("id")))
        val headers = paintCommands.associateWith { id ->
            val headerId = model().getInt("next_id")
            edit(obj("type" to "add", "zone" to "left", "before" to null,
                "item" to obj("kind" to "tool", "control" to obj("kind" to "command", "command" to id))))
            headerId
        }
        val leafId = model().getInt("next_id")
        edit(obj("type" to "add", "zone" to "left", "before" to null,
            "item" to obj("kind" to "tool", "control" to obj("kind" to "brush", "id" to state().getJSONObject("brush").getInt("preset")))))
        edit(obj("type" to "set_size", "size" to "medium")); edit(obj("type" to "edit", "editing" to false)); idle()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            assertFalse(header(leafId).getBoolean("has_variants"))
            assertNull(node("header-variants-$leafId"))
            for ((index, id) in paintCommands.withIndex()) {
                tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index % 3]
                val tileId = tiles().first { it.getJSONObject("control").optString("command") == id }.getInt("id")
                fun tile() = tiles().first { it.getInt("id") == tileId }
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                val headerAnchor = obj("kind" to "header", "id" to headers.getValue(id))
                action(obj("type" to "invoke", "command" to "hand"))
                assertFalse(tile().getBoolean("selected"))
                assertTrue(tile().getBoolean("has_variants")); assertTrue(header(headers.getValue(id)).getBoolean("has_variants"))
                assertTrue(bounds("tile-variants-toolbar-$tileId").width >= 14 * density)
                val rows = variantRows(anchor)
                assertTrue(rows.isNotEmpty())
                mediaGroups[id]?.let { assertEquals(it, rows.map(::brushGroup).toSet()) }
                    ?: assertTrue(rows.all { it.getJSONObject("action").getJSONObject("variant").getString("type") == "brush_preset" })
                assertEquals(rows.map { it.getString("label") }, variantRows(headerAnchor).map { it.getString("label") })
                val choices = media[id]?.map { group -> rows.firstOrNull { brushGroup(it) == group } ?: error("$id lacks $group: $rows") }
                    ?: listOf(rows.first { it.optBoolean("enabled", true) })
                for (choice in choices) {
                    mediaIcons[brushGroup(choice)]?.let { assertEquals(it, choice.getString("icon")) }
                    choose(anchor, "tile-variants-toolbar-$tileId", choice, "paint-menu-$id-${choice.getString("icon")}-$theme")
                    waitFor("$id chosen medium published") { command(id).getBoolean("selected") && tile().getString("icon") == choice.getString("icon") &&
                        header(headers.getValue(id)).getString("icon") == choice.getString("icon") }
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    assertEquals(tile().getString("icon"), header(headers.getValue(id)).getString("icon"))
                    assertEquals(tile().getString("tooltip"), header(headers.getValue(id)).getString("label"))
                    val preset = state().getJSONObject("brush").getInt("preset")
                    action(obj("type" to "invoke", "command" to "hand"))
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    assertEquals(choice.getString("icon"), header(headers.getValue(id)).getString("icon"))
                    choose(headerAnchor, "header-variants-${headers.getValue(id)}", choice)
                    assertEquals(preset, state().getJSONObject("brush").getInt("preset"))
                    action(obj("type" to "invoke", "command" to "hand")); tap("tile-toolbar-$tileId")
                    waitFor("$id body activates remembered medium") { command(id).getBoolean("selected") && tile().getBoolean("selected") && header(headers.getValue(id)).getBoolean("selected") }
                    assertEquals(preset, state().getJSONObject("brush").getInt("preset"))
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    shot("paint-$id-${choice.getString("icon")}-$theme")
                }
            }
            for ((slot, expected) in listOf("manual_selection" to setOf("lasso", "rectangle_select", "ellipse_select", "polygon_select", "selection_brush"),
                "automatic_selection" to setOf("auto_select", "color_select"))) {
                val tileId = tiles().first { it.getJSONObject("control").optString("slot") == slot }.getInt("id")
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                action(obj("type" to "invoke", "command" to "hand"))
                val rows = variantRows(anchor)
                assertEquals(expected, rows.map(::rowCommand).toSet())
                choose(anchor, "tile-variants-toolbar-$tileId", rows.first(), "paint-menu-$slot-$theme")
                waitFor("$slot choice published") { command(rowCommand(rows.first())).getBoolean("selected") &&
                    state().getJSONObject("tool_set").array("subtools").objects().map(::rowCommand).toSet() == expected }
                assertEquals(expected, state().getJSONObject("tool_set").array("subtools").objects().map(::rowCommand).toSet())
                tap("tile-toolbar-$tileId")
                waitFor("$slot drawer") { drawer()?.objectOrNull("tool_set") != null && node("tool-drawer") != null }
                assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                assertEquals(expected, drawer()!!.getJSONObject("tool_set").array("groups").objects().map(::rowCommand).toSet())
                val sibling = drawer()!!.getJSONObject("tool_set").array("groups").objects().first { !it.optBoolean("selected") }
                waitFor("selection sibling laid out") { (node("tool-drawer")?.second?.find(hasTag("tool-group-${sibling.getString("label")}"))?.boundsInRoot?.height ?: 0f) >= 42 * density }
                instrumentation.runOnMainSync {
                    val surface = node("tool-drawer")!!
                    val row = surface.second.find(hasTag("tool-group-${sibling.getString("label")}"))!!.find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                    pressed = surface.first; point = row.boundsInRoot.center
                }
                event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP); idle()
                waitFor("$slot sibling published") { command(rowCommand(sibling)).getBoolean("selected") && drawer()?.getJSONObject("anchor")?.toString() == anchor.toString() }
                assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                shot("paint-$slot-$theme"); tap("tile-toolbar-$tileId")
                waitFor("selection drawer closes") { drawer() == null }
            }
        }
        for ((workspace, slots) in listOf("illustrator" to listOf("fill", "blend"), "photographer" to listOf("drawing", "healing", "photo_fill"))) {
            resetPreset("builtin:workspace:$workspace")
            for (theme in listOf("light", "dark")) for (slot in slots) {
                action(obj("type" to "set_theme", "theme" to theme))
                val tileId = tiles().first { it.getJSONObject("control").optString("slot") == slot }.getInt("id")
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                val rows = variantRows(anchor)
                choose(anchor, "tile-variants-toolbar-$tileId", rows.first())
                val docked = state().getJSONObject("tool_set").array("groups").objects()
                assertEquals(rows.map { it.getString("label") }, docked.map { it.getString("label") })
                assertEquals(rows.map { it.optBoolean("enabled", true) }, docked.map { it.optBoolean("enabled", true) })
                val sibling = rows[1].getString("label")
                if (workspace == "photographer" && node("tool-group-$sibling") == null) tap("column-icon-brushes")
                waitFor("$slot panel sibling laid out") { (node("tool-group-$sibling")?.second?.boundsInRoot?.height ?: 0f) >= 42 * density }
                tap("tool-group-$sibling")
                waitFor("$slot panel sibling published") { state().getJSONObject("tool_set").array("groups").objects().any { it.getString("label") == sibling && it.optBoolean("selected") } }
                assertEquals(rows.map { it.getString("label") }, state().getJSONObject("tool_set").array("groups").objects().map { it.getString("label") })
                assertEquals(sibling, variantRows(anchor).single { it.optBoolean("selected") }.getString("label"))
                shot("$workspace-$slot-panel-$theme")
            }
        }
        resetPreset("builtin:workspace:painter")
        waitFor("Sketch groups") { entries().firstOrNull { it.getJSONObject("item").objectOrNull("control")?.optString("command") == "drawing_brush" }
            ?.let { node("header-variants-${it.getInt("id")}") != null } == true }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((index, id) in listOf("drawing_brush", "sculpt", "select").withIndex()) {
                tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index]
                val headerId = entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("command") == id }.getInt("id")
                val anchor = obj("kind" to "header", "id" to headerId)
                action(obj("type" to "invoke", "command" to "hand"))
                assertTrue(header(headerId).getBoolean("has_variants")); assertFalse(header(headerId).getBoolean("selected"))
                val rows = variantRows(anchor)
                if (id == "drawing_brush") assertEquals(setOf("pen", "marker", "pencil", "pastel", "paint", "watercolor", "oil", "airbrush", "spray", "decoration"), rows.map(::brushGroup).toSet())
                if (id == "sculpt") assertEquals(setOf("blend", "liquify", "clone", "heal", "spot_heal"), rows.map(::brushGroup).toSet())
                val choices = if (id == "drawing_brush") mediaIcons.keys.map { group -> rows.firstOrNull { brushGroup(it) == group } ?: error("Sketch lacks $group: $rows") }
                    else listOf(rows.first { it.optBoolean("enabled", true) })
                if (id == "select") assertEquals(setOf("rectangle_select", "ellipse_select", "lasso", "polygon_select", "auto_select", "color_select", "selection_brush", "tonal_select"), rows.map(::rowCommand).toSet())
                for (choice in choices) {
                    mediaIcons[brushGroup(choice)]?.let { assertEquals(it, choice.getString("icon")) }
                    choose(anchor, "header-variants-$headerId", choice, "sketch-menu-$id-${choice.getString("icon")}-$theme")
                    waitFor("Sketch $id chosen group published") { command(id).getBoolean("selected") && header(headerId).getString("icon") == choice.getString("icon") }
                    assertEquals(choice.getString("icon"), header(headerId).getString("icon"))
                    tap("header-control-$headerId")
                    waitFor("Sketch $id grouped drawer") { drawer() != null && node("tool-drawer") != null }
                    assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                    if (id != "select") assertEquals("[[\"${if (id == "drawing_brush") "brush_sets" else "sculpt_sets"}\"],[\"tools\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
                    shot("sketch-$id-${choice.getString("icon")}-$theme")
                    tap("header-control-$headerId"); waitFor("Sketch group drawer closes") { drawer() == null }
                }
            }
        }
    }

    @Test fun toolVariantCornersAndContextMenusShareRememberedChoices() {
        device.landscape(scenario); idle()
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        waitFor("Photo grouped tools published") {
            view().optString("id") == "builtin:workspace:photographer" && snapshot().array("panels").objects()
                .firstOrNull { it.getString("id") == "toolbar" }?.array("tiles")?.objects()
                ?.any { it.getJSONObject("control").optString("slot") == "lasso" } == true
        }
        edit(obj("type" to "edit", "editing" to true))
        for (entry in entries().filter { it.getJSONObject("item").getString("kind") !in listOf("capy", "settings") }) {
            edit(obj("type" to "remove", "id" to entry.getInt("id")))
        }
        val headerId = model().getInt("next_id")
        edit(obj("type" to "add", "zone" to "left", "before" to null,
            "item" to obj("kind" to "tool", "control" to obj("kind" to "tool_slot", "slot" to "lasso"))))
        edit(obj("type" to "set_size", "size" to "medium"))
        edit(obj("type" to "edit", "editing" to false))
        idle()
        fun tile() = snapshot().array("panels").objects().first { it.getString("id") == "toolbar" }
            .array("tiles").objects().first { it.getJSONObject("control").optString("slot") == "lasso" }
        fun header() = snapshot().getJSONObject("header").array("items").objects().first { it.getInt("id") == headerId }
        val tileId = tile().getInt("id")
        val ribbonAnchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
        val headerAnchor = obj("kind" to "header", "id" to headerId)
        val stable = tile().getJSONObject("control").toString()
        fun rows(anchor: JSONObject) = variantRows(anchor)
        fun sameChoice() {
            assertEquals(stable, tile().getJSONObject("control").toString())
            assertEquals(tile().getString("tooltip"), header().getString("label"))
            instrumentation.runOnMainSync {
                for (tag in listOf("header-variants-$headerId", "tile-variants-toolbar-$tileId")) {
                    val marker = node(tag)!!.second.config
                    assertNull(marker.getOrNull(SemanticsActions.OnClick))
                    assertNull(marker.getOrNull(SemanticsActions.OnLongClick))
                    assertNull(marker.getOrNull(SemanticsProperties.ContentDescription))
                }
            }
            assertEquals(tile().getString("icon"), header().getString("icon"))
            assertEquals(tile().getJSONObject("resolved_control").toString(), header().getJSONObject("resolved_control").toString())
            assertEquals(1, rows(ribbonAnchor).count { it.optBoolean("selected") })
            assertEquals(1, rows(headerAnchor).count { it.optBoolean("selected") })
        }
        val raster = android.graphics.Bitmap.createBitmap(256, 256, android.graphics.Bitmap.Config.ARGB_8888)
        val pixels = try {
            val source = instrumentation.targetContext.assets.open("layer-tool-group-symbolic.svg").bufferedReader().use { it.readText() }
            val picture = com.caverock.androidsvg.SVG.getFromString(source.replace("currentColor", "#000000")).renderToPicture()
            android.graphics.Canvas(raster).drawPicture(picture, android.graphics.RectF(0f, 0f, 256f, 256f))
            (0 until raster.height).flatMap { y -> (0 until raster.width).filter { x -> android.graphics.Color.alpha(raster.getPixel(x, y)) > 0 }
                .map { x -> Offset((x + .5f) / raster.width, (y + .5f) / raster.height) } }.also { painted ->
                assertTrue("Group marker paints pixels", painted.isNotEmpty())
                assertTrue("Painted right inset is at least six SVG pixels", (1f - painted.maxOf { it.x } - .5f / raster.width) * 16 >= 6)
                assertTrue("Painted bottom inset is at least six SVG pixels", (1f - painted.maxOf { it.y } - .5f / raster.height) * 16 >= 6)
            }
        } finally { raster.recycle() }
        for (theme in listOf("light", "dark")) for (size in listOf("small", "medium", "large")) {
            action(obj("type" to "set_theme", "theme" to theme))
            edit(obj("type" to "set_size", "size" to size)); idle()
            waitFor("$size marker header published") { model().getString("size") == size && node("header-variants-$headerId") != null }
            val target = bounds("header-variants-$headerId")
            val body = bounds("header-control-$headerId")
            val outline = TileShape.createOutline(androidx.compose.ui.geometry.Size(body.width, body.height),
                androidx.compose.ui.unit.LayoutDirection.Ltr, androidx.compose.ui.unit.Density(density)) as androidx.compose.ui.graphics.Outline.Generic
            val region = android.graphics.Region().apply {
                setPath(outline.path.asAndroidPath(), android.graphics.Region(0, 0, kotlin.math.ceil(body.width).toInt(), kotlin.math.ceil(body.height).toInt()))
            }
            val painted = pixels.map { target.topLeft + Offset(it.x * target.width, it.y * target.height) }
            assertTrue("$size marker stays within its click target", painted.all(target::contains))
            assertTrue("$size marker stays within the header squircle", painted.all {
                body.contains(it) && region.contains((it.x - body.left).toInt(), (it.y - body.top).toInt()) })
            tool = MotionEvent.TOOL_TYPE_FINGER
            fun tapPaintedMarker() {
                instrumentation.runOnMainSync { pressed = node("header-variants-$headerId")!!.first }
                event(MotionEvent.ACTION_DOWN, Offset(painted.map { it.x }.average().toFloat(), painted.map { it.y }.average().toFloat()))
                event(MotionEvent.ACTION_UP); idle()
                assertNull("Marker tap has no context menu", node("workspace-menu"))
            }
            action(obj("type" to "invoke", "command" to "eraser"))
            tapPaintedMarker()
            waitFor("$size marker selects its owner") { header().getBoolean("selected") && state().getJSONObject("customization").isNull("drawer") }
            shot("tool-marker-$size-$theme")
            tapPaintedMarker()
            waitFor("$size marker opens its owner drawer") { node("tool-drawer") != null &&
                state().getJSONObject("customization").objectOrNull("drawer")?.getJSONObject("anchor")?.toString() == headerAnchor.toString() }
            tapPaintedMarker()
            waitFor("$size marker closes its owner drawer") { node("tool-drawer") == null }
        }
        edit(obj("type" to "set_size", "size" to "medium")); idle()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((index, nativeTool) in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS).withIndex()) {
                tool = nativeTool
                for ((anchor, tag) in listOf(ribbonAnchor to "tile-variants-toolbar-$tileId", headerAnchor to "header-variants-$headerId")) {
                    assertTrue("Marker remains inside the owner", bounds(tag).width >= 14 * density && bounds(tag).height >= 14 * density)
                    val choices = rows(anchor)
                    assertTrue(choices.size > 1)
                    val alternatives = choices.filter { it.optBoolean("enabled", true) && !it.optBoolean("selected") }
                    val choice = alternatives[index % alternatives.size]
                    openToolContext(tag)
                    tapMenuRow(choice.getString("label"))
                    waitFor("variant popup closes") { node("workspace-menu") == null }
                    idle(); sameChoice()
                    assertEquals(choice.getString("label"), rows(anchor).first { it.optBoolean("selected") }.getString("label"))
                    action(obj("type" to "invoke", "command" to "eraser"))
                    tap(tag)
                    assertNull("Corner activates the owner without a menu", node("workspace-menu"))
                    host.awaitMain("remembered body choice activates", 15_000, {
                        shot("failure-remembered-body-choice")
                        "tile=${tile()}; header=${header()}; brush=${state().getJSONObject("brush")}; layer_tools=${state().objectOrNull("layer_tools")}"
                    }) { tile().getBoolean("selected") && header().getBoolean("selected") }
                    sameChoice()
                    tap(tag)
                    assertNull("Corner reclick opens the drawer without a menu", node("workspace-menu"))
                    fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
                    waitFor("full grouped drawer") { node("tool-drawer") != null && drawer()?.objectOrNull("tool_set") != null }
                    assertEquals("[[\"brushes\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
                    assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                    val sibling = drawer()!!.getJSONObject("tool_set").array("groups").objects().first { it.optBoolean("enabled", true) && !it.optBoolean("selected") }
                    val siblingTag = "tool-group-${sibling.getString("label")}"
                    waitFor("drawer sibling laid out") {
                        val row = node("tool-drawer")?.second?.find(hasTag(siblingTag))
                        (row?.boundsInRoot?.height ?: 0f) >= 42 * density
                    }
                    instrumentation.runOnMainSync {
                        val drawerNode = checkNotNull(node("tool-drawer"))
                        val row = checkNotNull(drawerNode.second.find(hasTag(siblingTag)))
                            .find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                        pressed = drawerNode.first; point = row.boundsInRoot.center
                    }
                    event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP)
                    waitFor("drawer sibling keeps its opener") {
                        drawer()?.getJSONObject("anchor")?.toString() == anchor.toString() &&
                            drawer()?.getJSONObject("tool_set")?.array("groups")?.objects()?.any { it.optBoolean("selected") && it.getString("label") == sibling.getString("label") } == true
                    }
                    idle(); sameChoice()
                    tap(tag)
                    waitFor("grouped drawer closes") { node("tool-drawer") == null }
                }
                openToolContext("header-control-$headerId")
                val selected = rows(headerAnchor).first { it.optBoolean("selected") }
                tapMenuRow(selected.getString("label"))
                waitFor("full context closes") { node("workspace-menu") == null }
                button = MotionEvent.BUTTON_PRIMARY
                idle(); sameChoice()
            }
            shot("tool-variants-$theme")
        }
        val crowded = mutableListOf<Int>()
        while (node("header-item-$headerId") != null) {
            edit(obj("type" to "edit", "editing" to true))
            repeat(4) {
                assertTrue("Grouped item can overflow", crowded.size < 30)
                crowded.add(model().getInt("next_id"))
                edit(obj("type" to "add", "zone" to "left", "before" to headerId,
                    "item" to obj("kind" to "tool", "control" to obj("kind" to "command", "command" to "pen"))))
            }
            edit(obj("type" to "edit", "editing" to false)); idle()
        }
        waitFor("runtime grouped item overflow") { node("header-overflow-0") != null && node("header-item-$headerId") == null }
        for ((theme, nativeTool) in listOf("light" to MotionEvent.TOOL_TYPE_MOUSE, "dark" to MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme", "theme" to theme)); action(obj("type" to "invoke", "command" to "eraser"))
            tool = nativeTool
            tap("header-overflow-0")
            waitFor("overflow group marker") { node("header-overflow-variants-$headerId") != null }
            tap("header-overflow-item-$headerId")
            waitFor("overflow body activates its row") { header().getBoolean("selected") && node("header-overflow-list") == null }
            action(obj("type" to "invoke", "command" to "eraser"))
            tap("header-overflow-0")
            waitFor("overflow group marker returns") { node("header-overflow-variants-$headerId") != null }
            val marker = node("header-overflow-variants-$headerId")!!.second.config
            assertNull(marker.getOrNull(SemanticsActions.OnClick)); assertNull(marker.getOrNull(SemanticsActions.OnLongClick))
            tap("header-overflow-variants-$headerId")
            waitFor("overflow corner activates its row") { header().getBoolean("selected") && node("header-overflow-list") == null }
            assertNull("Overflow corner has no context menu", node("workspace-menu"))
            shot("tool-marker-overflow-$theme")
        }
        edit(obj("type" to "edit", "editing" to true))
        crowded.forEach { edit(obj("type" to "remove", "id" to it)) }
        edit(obj("type" to "edit", "editing" to false)); idle()
        val remembered = tile().getString("label")
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        waitFor("remembered Photo tools published") {
            view().optString("id") == "builtin:workspace:photographer" && snapshot().array("panels").objects()
                .firstOrNull { it.getString("id") == "toolbar" }?.array("tiles")?.objects()
                ?.any { it.getJSONObject("control").optString("slot") == "lasso" && it.getString("label") == remembered } == true
        }
        assertEquals(remembered, tile().getString("label")); sameChoice()
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

    private fun showSwipeLayers() {
        val defaults=createEnglishHostForTest()
        val workspace=try { JSONObject(Native.snapshot(defaults)!!).getJSONObject("state").getJSONObject("workspace") }
            finally { Native.destroy(defaults) }
        action(obj("type" to "restore_workspace","workspace" to workspace))
        idle()
    }

    @Test fun layerSwipeFrameTiming() {
        val args=androidx.test.platform.app.InstrumentationRegistry.getArguments()
        val reorder=args.getString("layerReorderBenchmark")=="true"
        val selection=args.getString("layerSelectionBenchmark")=="true"
        org.junit.Assume.assumeTrue(selection || reorder || args.getString("layerSwipeBenchmark")=="true")
        val width=args.getString("width")?.toInt() ?: 6000
        val height=args.getString("height")?.toInt() ?: 4000
        val label=if(selection) "layer-selection" else if(reorder) "layer-reorder" else "layer-swipe"
        val selectionInterval=args.getString("layerSelectionIntervalMs")?.toLong() ?: 150L
        require(selectionInterval>0)
        val theme=args.getString("layerBenchmarkTheme") ?: "dark"
        host.openDocument(java.io.File(checkNotNull(args.getString("photo"))))
        action(obj("type" to "set_theme","theme" to theme))
        assertTrue(state().array("tabs").objects().any { it.optBoolean("active") && it.optInt("width")==width && it.optInt("height")==height })
        action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
        if(args.getString("layerRelationshipBenchmark")=="true") {
            val owner=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
            action(obj("type" to "layer","action" to obj("op" to "clip","id" to owner,"value" to true)))
            for(kind in listOf("gaussian_blur","curves")) {
                action(obj("type" to "effect","action" to obj("op" to "insert","effect" to kind)))
                val effect=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
                action(obj("type" to "layer","action" to obj("op" to "attach_effect","id" to effect,"owner" to owner)))
            }
            if(!reorder)action(obj("type" to "layer","action" to obj("op" to "select","id" to owner,"mask" to false)))
        }
        showSwipeLayers()
        action(obj("type" to "invoke","command" to "fit_canvas"))
        val id=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        if(selection)action(obj("type" to "layer","action" to obj("op" to "add_mask","id" to id,"replace" to false)))
        val frames=java.util.Collections.synchronizedList(mutableListOf<LongArray>())
        val thread=android.os.HandlerThread("$label-frames").apply { start() }
        lateinit var window: android.view.Window
        scenario.onActivity { window=it.window; window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        val listener=android.view.Window.OnFrameMetricsAvailableListener { _,metrics,dropped ->
            frames.add(longArrayOf(metrics.getMetric(android.view.FrameMetrics.VSYNC_TIMESTAMP),
                metrics.getMetric(android.view.FrameMetrics.TOTAL_DURATION),dropped.toLong(),
                metrics.getMetric(android.view.FrameMetrics.INTENDED_VSYNC_TIMESTAMP),
                metrics.getMetric(android.view.FrameMetrics.INPUT_HANDLING_DURATION),
                metrics.getMetric(android.view.FrameMetrics.ANIMATION_DURATION),
                metrics.getMetric(android.view.FrameMetrics.LAYOUT_MEASURE_DURATION),
                metrics.getMetric(android.view.FrameMetrics.DRAW_DURATION),
                metrics.getMetric(android.view.FrameMetrics.SYNC_DURATION),
                metrics.getMetric(android.view.FrameMetrics.COMMAND_ISSUE_DURATION),
                metrics.getMetric(android.view.FrameMetrics.SWAP_BUFFERS_DURATION)))
        }
        instrumentation.runOnMainSync { window.addOnFrameMetricsAvailableListener(listener,android.os.Handler(thread.looper)) }
        tool=if(reorder) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
        try {
            for(run in 0..3) {
                if(!selection)down("layer-row-$id")
                val start=point
                if(!selection)event(MotionEvent.ACTION_MOVE,start+Offset((if(reorder) -24 else 24)*density,0f))
                if(reorder)waitFor("moving layer preview") { node("layer-drag-preview")!=null }
                val done=java.util.concurrent.CountDownLatch(1)
                val duration=if(run==0)1000L else 5000L
                val begin=SystemClock.uptimeMillis()
                val beginNs=System.nanoTime()
                frames.clear()
                lateinit var callback: android.view.Choreographer.FrameCallback
                var selectedInterval=-1L
                var observedMask=state().array("layers").objects().first { it.getLong("id")==id }.getBoolean("mask_selected")
                val transitions=mutableListOf<Long>()
                instrumentation.runOnMainSync {
                    val clock=android.view.Choreographer.getInstance()
                    callback=android.view.Choreographer.FrameCallback {
                        val elapsed=SystemClock.uptimeMillis()-begin
                        if(elapsed>=duration)done.countDown()
                        else {
                            if(selection) {
                                val mask=state().array("layers").objects().first { it.getLong("id")==id }.getBoolean("mask_selected")
                                if(mask!=observedMask) { observedMask=mask;transitions.add(System.nanoTime()) }
                                val interval=elapsed/selectionInterval
                                if(interval!=selectedInterval) {
                                    selectedInterval=interval
                                    host.dispatch(obj("type" to "layer","action" to obj("op" to "select","id" to id,"mask" to (interval%2==0L))))
                                }
                            } else {
                                val cycle=(elapsed%1000)/500f
                                val fraction=if(cycle<=1)cycle else 2-cycle
                                val delta=if(reorder) Offset(-24*density,40*density*fraction) else Offset((24+40*fraction)*density,0f)
                                val move=motion(tool,MotionEvent.ACTION_MOVE,start+delta,downAt,button)
                                try { checkNotNull(pressed).view.dispatchTouchEvent(move) } finally { move.recycle() }
                            }
                            clock.postFrameCallback(callback)
                        }
                    }
                    clock.postFrameCallback(callback)
                }
                try { assertTrue(done.await(15,java.util.concurrent.TimeUnit.SECONDS)) }
                finally { instrumentation.runOnMainSync { android.view.Choreographer.getInstance().removeFrameCallback(callback) } }
                val endNs=System.nanoTime()
                SystemClock.sleep(200)
                val received=synchronized(frames) { frames.toList() }
                val rows=received.filter { it[0] in beginNs..endNs && (!selection || transitions.any { start -> it[0] in start..(start+200_000_000L) }) }
                if(!selection)event(MotionEvent.ACTION_CANCEL)
                idle()
                if(selection)shot("$label-$run")
                if(run>0) {
                    assertTrue("Moving frames were measured",rows.size>1)
                    val result=obj("run" to run,"duration_ms" to duration,"begin_ns" to beginNs,"end_ns" to endNs,
                        "width" to width,"height" to height,"photo" to args.getString("photo"),"motion" to label,"theme" to theme,
                        "transitions_ns" to JSONArray(transitions),"selection_interval_ms" to (if(selection)selectionInterval else JSONObject.NULL),
                        "animation_window_ms" to (if(selection)200 else JSONObject.NULL),
                        "frame_columns" to JSONArray(listOf("vsync_ns","total_ns","dropped_reports","intended_vsync_ns",
                            "input_ns","animation_ns","layout_ns","draw_ns","sync_ns","command_ns","swap_ns")),
                        "dropped_reports" to received.sumOf { it[2] },
                        "received_frames" to JSONArray(received.map { JSONArray(it.toList()) }),
                        "camera" to state().getJSONObject("camera"),
                        "layers" to state().array("layers"),"frames" to JSONArray(rows.map { JSONArray(it.toList()) }))
                    java.io.File(instrumentation.targetContext.getExternalFilesDir(null),"$label-$run.json").writeText(result.toString())
                    android.util.Log.i("LayerSwipePerf","$label run $run: ${rows.size} moving frames in $duration ms")
                }
            }
        } finally {
            instrumentation.runOnMainSync { window.removeOnFrameMetricsAvailableListener(listener) }
            thread.quitSafely()
        }
    }

    @Test fun layerRelationships() {
        host.newDocument(2048,1536)
        val source=java.io.File(device.root,"relationship-source.png")
        android.graphics.Bitmap.createBitmap(2048,1536,android.graphics.Bitmap.Config.ARGB_8888).let { bitmap ->
            android.graphics.Canvas(bitmap).drawOval(350f,230f,1700f,1300f,android.graphics.Paint().apply { color=android.graphics.Color.rgb(75,174,158) })
            source.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it) };bitmap.recycle()
        }
        host.importImage(source)
        waitFor("imported relationship source",60_000) { state().array("layers").length()==3 }
        action(obj("type" to "invoke","command" to "apply_transform"))
        fun layer(value:JSONObject)=action(obj("type" to "layer","action" to value))
        fun current()=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        fun row(id:Long)=state().array("layers").objects().first { it.getLong("id")==id }
        fun order()=state().array("layers").objects().map { it.getLong("id") }
        fun rename(id:Long,name:String)=layer(obj("op" to "rename","id" to id,"name" to name))
        fun select(id:Long)=layer(obj("op" to "select","id" to id,"mask" to false))
        fun undo()=action(obj("type" to "invoke","command" to "undo"))
        val base=current();rename(base,"Base colors")
        layer(obj("op" to "new","group" to false,"clipped" to true))
        val owner=current();rename(owner,"Shadows")
        fun effect(name:String,kind:String):Long {
            select(owner)
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to kind)))
            return current().also { rename(it,name);layer(obj("op" to "attach_effect","id" to it,"owner" to owner)) }
        }
        val blur=effect("Blur","gaussian_blur");val curves=effect("Curves","curves")
        select(owner);layer(obj("op" to "new","group" to false,"clipped" to true))
        val highlights=current();rename(highlights,"Highlights")
        layer(obj("op" to "new","group" to true,"clipped" to false))
        val isolated=current();rename(isolated,"Isolated group");layer(obj("op" to "blend","id" to isolated,"value" to 0))
        select(highlights);layer(obj("op" to "new","group" to true,"clipped" to false))
        val through=current();rename(through,"Pass Through group")
        if(!row(through).getBoolean("pass_through"))layer(obj("op" to "toggle_pass_through","id" to through))
        action(obj("type" to "invoke","command" to "select_all"))
        action(obj("type" to "invoke","command" to "save_selection_layer"))
        val saved=state().array("layers").objects().first { it.getBoolean("selection_layer") }.getLong("id")
        rename(saved,"Saved selection")
        layer(obj("op" to "drop","id" to saved,"target" to owner,"fraction" to 0,"surface" to "row"))
        val savedAt=order().indexOf(saved)
        assertEquals(listOf(saved,curves,blur,owner),order().subList(savedAt,savedAt+4))
        val relationshipWorkspace=JSONObject(fixture.toString())
        relationshipWorkspace.getJSONObject("layout").apply {
            put("bands",JSONArray(listOf(obj("id" to 40,"edge" to "right","extent" to 226,"root" to tabs(41,"layers")))))
            put("next_id",maxOf(42,getInt("next_id")))
        }
        action(obj("type" to "restore_workspace","workspace" to relationshipWorkspace));select(curves)
        waitFor("all relationship rows") { listOf(base,owner,blur,curves,isolated,through,saved).all { node("layer-row-$it")!=null } }
        waitFor("relationship controls") { node("layer-attachment")!=null && node("layer-content-$curves")!=null }
        assertFalse(row(isolated).getBoolean("pass_through"))
        assertNotNull(node("layer-group-pass-through-$through"));assertNull(node("layer-group-pass-through-$isolated"))
        tap("layer-attachment");assertNull(row(curves).objectOrNull("relationship"))
        tap("layer-attachment");assertEquals(highlights,row(curves).getJSONObject("relationship").getLong("target"))
        undo();undo();assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
        for(width in listOf(226,300)) for(theme in listOf("light","dark")) {
            val workspace=JSONObject(state().getJSONObject("workspace").toString())
            workspace.getJSONObject("layout").array("bands").objects().first { it.getJSONObject("root").toString().contains("layers") }.put("extent",width)
            action(obj("type" to "restore_workspace","workspace" to workspace))
            action(obj("type" to "set_theme","theme" to theme))
            shot("layer-relationships-$theme-$width")
            val ordinary=bounds("layer-row-$base")
            for(id in listOf(owner,blur,curves,isolated,through,saved)) {
                assertEquals(ordinary.width,bounds("layer-row-$id").width,1f)
                assertEquals("Uniform row height: ${row(id).getString("label")}, $width/$theme",ordinary.height,bounds("layer-row-$id").height,1f)
                assertEquals(30*density,bounds("layer-content-$id").width,1f)
            }
            assertEquals(30*density,bounds("selection-load-$saved").width,1f)
        }
        val first=order().first()
        val firstThumb=bounds("layer-content-$first")
        val rail=screenBounds("layer-content-$owner").let { Offset(it.left-3.5f*density,it.center.y) }
        fun railPixel():Int {
            val bitmap=checkNotNull(instrumentation.uiAutomation.takeScreenshot())
            return try { bitmap.getPixel(rail.x.toInt(),rail.y.toInt()) } finally { bitmap.recycle() }
        }
        val railColor=railPixel()
        assertTrue("Clipping rail pixel",android.graphics.Color.blue(railColor)>android.graphics.Color.red(railColor)+20)
        down("layer-row-$first");val swipeStart=point
        for(i in 1..4)event(MotionEvent.ACTION_MOVE,swipeStart+Offset(-36*density*i/4,0f))
        assertTrue("Row face moves",bounds("layer-content-$first").left<firstThumb.left-20*density)
        assertEquals("Connector column stays fixed during first-row swipe",railColor,railPixel())
        shot("layer-relationships-swiping")
        event(MotionEvent.ACTION_CANCEL);idle()
        tap("layer-eye-$owner")
        for(id in listOf(blur,curves)) {
            assertTrue(row(id).getBoolean("visible"));assertTrue(row(id).getBoolean("visibility_blocked"))
            assertEquals(owner,row(id).getJSONObject("relationship").getLong("target"))
        }
        shot("layer-relationships-hidden-owner");undo()
        fun swipe(id:Long,dx:Float,cancel:Boolean=false) {
            down("layer-row-$id");val start=point
            for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/4,0f))
            event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP);idle()
        }
        layer(obj("op" to "blend","id" to isolated,"value" to 1))
        for((theme,pointer) in listOf("light" to MotionEvent.TOOL_TYPE_FINGER,"dark" to MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme","theme" to theme));tool=pointer
            swipe(isolated,12f);assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,60f,true);assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,-60f);assertNotNull(node("layer-delete-$isolated"))
            swipe(isolated,90f);assertNull(node("layer-delete-$isolated"));assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,60f);assertTrue(row(isolated).getBoolean("pass_through"));assertFalse(row(isolated).getBoolean("alpha_locked"))
            swipe(isolated,60f);assertFalse(row(isolated).getBoolean("pass_through"));assertEquals("Normal",row(isolated).getString("blend_label"))
            undo();assertTrue(row(isolated).getBoolean("pass_through"));undo();assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(base,60f);assertTrue(row(base).getBoolean("alpha_locked"));undo();assertFalse(row(base).getBoolean("alpha_locked"))
        }
        for((pointer,thumbnail) in listOf(MotionEvent.TOOL_TYPE_MOUSE to true,MotionEvent.TOOL_TYPE_FINGER to false,MotionEvent.TOOL_TYPE_STYLUS to true)) {
            action(obj("type" to "set_theme","theme" to if(pointer==MotionEvent.TOOL_TYPE_MOUSE) "light" else "dark"))
            tool=pointer;select(curves);val before=order()
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE) {
                down("layer-row-$curves");event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density));event(MotionEvent.ACTION_UP);idle();assertEquals(before,order())
            }
            down("layer-row-$curves")
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE)SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong()+150)
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("layer pickup") { node("layer-drag-preview")!=null }
            val target=if(thumbnail)bounds("layer-content-$isolated").center else bounds("layer-row-$through").let { Offset(it.center.x,it.bottom-2*density) }
            event(MotionEvent.ACTION_MOVE,target)
            event(MotionEvent.ACTION_UP);idle()
            if(thumbnail)assertEquals(isolated,row(curves).getJSONObject("relationship").getLong("target")) else assertNotEquals(before,order())
            undo();assertEquals(before,order());assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
        }
        for(theme in listOf("light","dark")) for(pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme","theme" to theme));tool=pointer;select(curves)
            val before=order()
            down("layer-row-$curves")
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE)SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong()+150)
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("filter gap pickup") { node("layer-drag-preview")!=null }
            val target=bounds("layer-row-$base").let { Offset(it.right-64*density,it.top+2*density) }
            event(MotionEvent.ACTION_MOVE,target)
            shot("layer-filter-gap-$theme-$pointer")
            event(MotionEvent.ACTION_UP);idle()
            assertEquals(base,row(curves).getJSONObject("relationship").getLong("target"))
            assertEquals(base,row(owner).getJSONObject("relationship").getLong("target"))
            undo();assertEquals(before,order());assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
            action(obj("type" to "invoke","command" to "redo"));assertEquals(base,row(curves).getJSONObject("relationship").getLong("target"));undo()
        }
        for(theme in listOf("light","dark")) {
            host.newDocument(2048,1536)
            repeat(3) { layer(obj("op" to "new","group" to false,"clipped" to false)) }
            action(obj("type" to "restore_workspace","workspace" to relationshipWorkspace))
            action(obj("type" to "set_theme","theme" to theme))
            val before=order()
            val checked=before.take(3)
            fun selection()=state().array("layers").objects().filter { it.getBoolean("selected") }.map { it.getLong("id") }
            fun clickRow(id:Long,meta:Int=0) {
                val b=bounds("layer-row-$id")
                instrumentation.runOnMainSync { pressed=checkNotNull(node("layer-row-$id")).first }
                val p=Offset(b.right-64*density,b.center.y)
                event(MotionEvent.ACTION_DOWN,p,meta);event(MotionEvent.ACTION_UP,p,meta);idle()
            }
            tool=MotionEvent.TOOL_TYPE_MOUSE
            layer(obj("op" to "add_mask","id" to checked[0],"replace" to false))
            fun clickLink() {
                val b=bounds("layer-content-${checked[0]}")
                instrumentation.runOnMainSync { pressed=checkNotNull(node("layer-row-${checked[0]}")).first }
                val p=Offset(b.right+8*density,b.center.y)
                event(MotionEvent.ACTION_DOWN,p);event(MotionEvent.ACTION_UP,p);idle()
            }
            assertTrue(row(checked[0]).getBoolean("mask_linked"))
            shot("layer-mask-linked-$theme")
            clickLink();assertFalse(row(checked[0]).getBoolean("mask_linked"))
            shot("layer-mask-unlinked-$theme")
            layer(obj("op" to "lock","id" to checked[0],"value" to true))
            clickLink();assertFalse("Locked layer disables mask linking",row(checked[0]).getBoolean("mask_linked"))
            layer(obj("op" to "lock","id" to checked[0],"value" to false))
            clickLink();assertTrue(row(checked[0]).getBoolean("mask_linked"))
            layer(obj("op" to "select","id" to checked[0],"mask" to true))
            waitFor("mask preview") { node("layer-thumbnail-${checked[0]}-true") != null }
            fun thumbnailPixels(mask:Boolean=true): List<Int> {
                val exit=motion(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.ACTION_HOVER_EXIT,point,SystemClock.uptimeMillis(),0)
                try { instrumentation.runOnMainSync { checkNotNull(node("layer-row-${checked[0]}")).first.view.dispatchGenericMotionEvent(exit) } }
                finally { exit.recycle() }
                SystemClock.sleep(400)
                val b=screenBounds("layer-thumbnail-${checked[0]}-$mask")
                val image=checkNotNull(instrumentation.uiAutomation.takeScreenshot())
                try {
                    fun pixel(x:Float,y:Float)=image.getPixel((b.left+x*density).toInt(),(b.top+y*density).toInt())
                    return listOf(pixel(14f,-1f),pixel(0f,0f),pixel(-5f,0f),pixel(3f,3f),
                        pixel(14f,2f),pixel(2f,14f),pixel(25f,14f),pixel(14f,25f))
                } finally { image.recycle() }
            }
            val activeMask=thumbnailPixels()
            val idleContent=thumbnailPixels(false)
            shot("layer-thumbnail-squircles-$theme")
            assertEquals("$theme editing border uses the accent outside the preview",android.graphics.Color.parseColor(state().getJSONObject("palette").getString("accent")),activeMask[0])
            layer(obj("op" to "select","id" to checked[0],"mask" to false))
            val idleMask=thumbnailPixels()
            assertEquals("$theme full squircle clears the preview corner",idleContent[2],idleContent[1])
            assertNotEquals("$theme full squircle retains more than a circular thumbnail",idleContent[2],idleContent[3])
            assertEquals("$theme preview pixels inside the editing border stay unchanged",activeMask.drop(4),idleMask.drop(4))
            assertNotEquals("$theme outer editing edge changes with the target",activeMask[0],idleMask[0])
            layer(obj("op" to "select","id" to checked[0],"mask" to true))
            clickRow(checked[0])
            assertTrue("Active row retains its mask target",row(checked[0]).getBoolean("mask_selected"))
            assertFalse(row(checked[0]).getBoolean("content_selected"))
            clickRow(checked[2],KeyEvent.META_SHIFT_ON)
            assertEquals("Shift extends the visible checked range",checked,selection())
            assertTrue("Range selection preserves the editing target",row(checked[0]).getBoolean("mask_selected"))
            shot("layer-range-mask-$theme")
            clickRow(checked[1])
            assertEquals("A checked row retains the other checks",checked,selection())
            assertTrue(row(checked[1]).getBoolean("content_selected"))
            assertFalse(row(checked[0]).getBoolean("mask_selected"))
            val target=bounds("layer-row-${before[3]}")
            down("layer-row-${checked[1]}")
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("checked block pickup") { node("layer-drag-preview")!=null }
            event(MotionEvent.ACTION_MOVE,Offset(target.right-64*density,target.bottom-2*density))
            event(MotionEvent.ACTION_UP);idle()
            assertEquals("Dragging a checked row moves the ordered block",listOf(before[3])+checked+before.drop(4),order())
            undo();assertEquals(before,order());assertEquals(checked,selection())
            layer(obj("op" to "new","group" to true,"clipped" to false))
            val folder=current()
            assertTrue(row(folder).getBoolean("group"))
            for(id in checked)assertEquals("New group contains every checked layer",1,row(id).getInt("depth"))
            shot("layer-checked-group-$theme")
            layer(obj("op" to "select_all_layers","selected" to false))
            layer(obj("op" to "select_row","id" to folder,"extend" to false,"toggle" to false))
            layer(obj("op" to "delete_selected"))
            assertFalse(order().contains(folder))
            for(id in checked)assertEquals("Deleting an expanded group promotes unchecked children",0,row(id).getInt("depth"))
            undo();assertTrue(order().contains(folder))
            for(id in checked)assertEquals(1,row(id).getInt("depth"))
            undo();assertEquals(before,order());assertEquals(checked,selection())
        }
        assertNull(host.actionError)
    }

    @Test fun layerSwipeAlphaLock() {
        showSwipeLayers()
        fun row()=state().array("layers").objects().first { it.getLong("id")==1L }
        fun swipe(dx: Float, cancel: Boolean=false, reverse: Boolean=false) {
            down("layer-row-1"); val start=point
            for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/4,0f))
            if(reverse)event(MotionEvent.ACTION_MOVE,start)
            event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP);idle()
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            for(device in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS,MotionEvent.TOOL_TYPE_MOUSE)) {
                tool=device
                swipe(18f);assertFalse("Short swipe",row().getBoolean("alpha_locked"))
                swipe(60f,cancel=true);assertFalse("Cancelled swipe",row().getBoolean("alpha_locked"))
                swipe(60f,reverse=true);assertFalse("Returned to start",row().getBoolean("alpha_locked"))
                swipe(60f)
                assertEquals("Swipe right $theme/$device",device!=MotionEvent.TOOL_TYPE_MOUSE,row().getBoolean("alpha_locked"))
                if(device==MotionEvent.TOOL_TYPE_MOUSE)continue
                shot("layer-alpha-lock-$theme-$device")
                action(obj("type" to "invoke","command" to "undo"));assertFalse(row().getBoolean("alpha_locked"))
                action(obj("type" to "invoke","command" to "redo"));assertTrue(row().getBoolean("alpha_locked"))
                swipe(60f);assertFalse("Second swipe unlocks",row().getBoolean("alpha_locked"))
                swipe(-60f);assertNotNull("Left reveals Delete",node("layer-delete-1"))
                swipe(90f);assertNull("Reverse closes Delete",node("layer-delete-1"))
                assertFalse("Closing Delete does not toggle alpha lock",row().getBoolean("alpha_locked"))
                action(obj("type" to "layer","action" to obj("op" to "lock","id" to 1,"value" to true)))
                swipe(60f);assertFalse("Locked layer",row().getBoolean("alpha_locked"))
                action(obj("type" to "layer","action" to obj("op" to "lock","id" to 1,"value" to false)))
            }
        }
    }

    @Test fun filterDrawerLayersAndPenScrolling() {
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to "builtin:workspace:painter"))) }
        waitFor("restore prompt") { !view().isNull("prompt") }
        send(obj("type" to "submit"))
        fun header(panel: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("panel") == panel
        }.getInt("id")
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun layer(op: String, id: Long) = action(obj("type" to "layer", "action" to obj("op" to op, "id" to id, "mask" to false)))
        val filters=header("adjustments")
        tap(filters)
        waitFor("filter drawer") { drawer() != null }
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
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(1.0,0.0,0.0,1.0))))
        tap("color-bucket")
        val paper=state().array("layers").objects().first { it.getLong("id")==2L }
        assertTrue(paper.getBoolean("has_thumbnail"))
        assertEquals("layer-fill-symbolic",paper.getString("content_icon"))
        assertFalse(paper.getBoolean("has_mask"))
        assertFalse(paper.getString("description").contains("Protected"))
        assertTrue(state().getJSONObject("layer_tools").getJSONObject("controls").getBoolean("opacity"))
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for(name in listOf("Paper", "Solid Color", "Paper")) {
                action(obj("type" to "layer", "action" to obj("op" to "rename", "id" to 2, "name" to name)))
                fun hasTitle(n: SemanticsNode): Boolean =
                    n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == name } == true || n.children.any(::hasTitle)
                waitFor("Properties heading $name") { node("layer-properties")?.second?.let(::hasTitle) == true }
            }
            shot("paper-properties-$theme")
        }
        fun waitThumbnail(label: String, matches: (List<Int>) -> Boolean) {
            waitFor(label) { node("layer-thumbnail-2-false") != null }
            val deadline=SystemClock.uptimeMillis()+15000
            while(true) {
                val bounds=screenBounds("layer-thumbnail-2-false")
                val image=instrumentation.uiAutomation.takeScreenshot()
                val colors=listOf(.2f,.5f,.8f).map { image.getPixel((bounds.left+bounds.width*it).toInt(),(bounds.top+bounds.height*.35f).toInt()) }
                image.recycle()
                if(matches(colors)) return
                assertTrue("$label: $colors", SystemClock.uptimeMillis()<deadline)
                SystemClock.sleep(100)
            }
        }
        tap(header("layers"))
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            waitThumbnail("$theme red fill thumbnail") { colors ->
                val color=colors[1]
                android.graphics.Color.red(color)>240 && android.graphics.Color.green(color)<10 && android.graphics.Color.blue(color)<10
            }
            waitFor("Fill type symbol") { node("layer-type-symbol-2") != null }
            assertNull(node("layer-type-symbol-1"))
            shot("fill-thumbnail-$theme")
        }
        tap(filters)
        tap("filter-type-fill")
        tap("adjustment-gradient_fill")
        action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to 2, "key" to "angle", "value" to obj("kind" to "number", "value" to 0))))
        assertEquals("layer-gradient-symbolic",state().array("layers").objects().first { it.getLong("id")==2L }.getString("content_icon"))
        tap(header("layers"))
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            waitFor("Gradient type symbol") { node("layer-type-symbol-2") != null }
            for((step, reverse) in listOf(false,true,false).withIndex()) {
                if(reverse) action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to 2, "key" to "reverse", "value" to obj("kind" to "toggle", "value" to true))))
                else if(step==2) action(obj("type" to "invoke", "command" to "undo"))
                waitThumbnail("$theme gradient thumbnail reverse=$reverse") { colors ->
                    val left=android.graphics.Color.red(colors[0]);val right=android.graphics.Color.red(colors[2])
                    if(reverse) left>180 && right<80 else left<80 && right>180
                }
                shot("gradient-thumbnail-$theme-$reverse")
            }
        }
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

    @Test fun touchAndPenHoldsOpenItemMenusOutsideEditing() {
        restore()
        fun preferencesOpen() = snapshot().objectOrNull("preferences") != null
        fun hold(device: Int) {
            tool = device
            down("header-item-3")
            SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        }
        hold(MotionEvent.TOOL_TYPE_MOUSE)
        instrumentation.runOnMainSync { assertNull("Mouse hold never opens a context menu", node("workspace-menu")) }
        event(MotionEvent.ACTION_UP)
        waitFor("mouse release clicks Settings") { preferencesOpen() }
        action(obj("type" to "close_settings"))
        for (device in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            hold(device)
            waitFor("held item menu") { node("workspace-menu") != null }
            event(MotionEvent.ACTION_CANCEL)
            waitFor("cancellation closes the held menu") { node("workspace-menu") == null }
            hold(device)
            event(MotionEvent.ACTION_UP)
            waitFor("release keeps the held menu") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
            idle()
            assertFalse("A hold suppresses the click", preferencesOpen())
            tapMenuRow("Customize Title Bar…")
            waitFor("held menu enters inline editor") { editing() && node("title-bar")?.first?.view?.hasWindowFocus() == true }
            tap("header-edit-cancel")
            waitFor("editor closed") { !editing() }
        }
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
