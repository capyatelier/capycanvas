package art.capycanvas

import android.content.res.Configuration
import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ViewConfiguration
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.LayoutInfo
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.toSize
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.core.app.ActivityScenario
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*

/** Typed native contacts in real Compose dialog windows, with isolated SQLite. */
class AndroidWorkspaceSwitcherTest {
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
    private fun ids(field: String) = view().array(field).objects().map { it.getString("id") }
    private val defaults = listOf("builtin:workspace:painter", "builtin:workspace:illustrator", "builtin:workspace:photographer")
    private fun order() = view().array("order").values().map { it.toString() }
    private fun layout() = jsonValue(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout"))
    private fun node(tag: String) = findTag(tag)
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync { result = node(tag)?.second?.boundsInRoot }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun waitFor(label: String, timeout: Long = 15000, condition: () -> Boolean) = host.awaitMain(label, timeout, { "${view()}" }, condition)
    private fun rowsEnabled() = view().optString("page") != "workspaces" || !view().isNull("prompt") || view().array("rows").objects()
        .all { node("workspace-row-${it.getString("id")}")?.second?.config?.getOrNull(SemanticsProperties.Disabled) == null }
    private fun idle() {
        host.drain()
        SystemClock.sleep(220)
        waitFor("workspace idle") { !view().optBoolean("busy") && !view().optBoolean("switcher_busy") && !view().optBoolean("dirty") && rowsEnabled() }
        assertTrue(view().toString(), view().isNull("error")); assertTrue(view().toString(), view().isNull("switcher_error"))
    }
    private fun send(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; idle() }
    private fun capture() = jsonValue(JSONObject(host.workspaceCapture()))
    private fun event(action: Int, next: Offset = point, delay: Long = 40) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) downAt = SystemClock.uptimeMillis()
        val event = motion(tool, action, next, downAt, button)
        try { instrumentation.runOnMainSync { checkNotNull(pressed).view.dispatchTouchEvent(event) } }
        finally { event.recycle() }
        if (action in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) pressed = null
        SystemClock.sleep(delay)
    }
    private fun down(tag: String, delay: Long = 40) {
        var start = Offset.Zero
        instrumentation.runOnMainSync { checkNotNull(node(tag)) { "Missing $tag; ${view()}" }.let { pressed = it.first; start = it.second.boundsInRoot.center } }
        event(MotionEvent.ACTION_DOWN, start, delay)
    }
    private fun tap(tag: String) {
        android.util.Log.i("SwitcherAcceptance", "Tap $tag")
        down(tag); event(MotionEvent.ACTION_UP); idle()
    }
    private fun key(code: Int, meta: Int = 0) { pressKey(code, meta); SystemClock.sleep(220) }
    private fun open() { send(obj("type" to "open", "page" to "workspaces")); waitFor("dialog focus") { node("workspace-manager")?.first?.view?.hasWindowFocus() == true } }
    private fun newWorkspace(name: String): String {
        send(obj("type" to "form", "action" to obj("type" to "new"))); send(obj("type" to "submit", "name" to name))
        return view().getString("id")
    }
    private fun renameInline(id: String, name: String) {
        waitFor("inline workspace name") { node("workspace-name-$id")?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } != null }
        instrumentation.runOnMainSync {
            val field = node("workspace-name-$id")!!.second.find { it.config.getOrNull(SemanticsActions.SetText) != null }!!
            assertTrue(field.config[SemanticsActions.SetText].action!!.invoke(androidx.compose.ui.text.AnnotatedString(name)))
        }
        key(KeyEvent.KEYCODE_ENTER); idle()
        waitFor("renamed workspace row") { view().array("rows").objects().first { it.getString("id") == id }.getString("title") == name }
        assertNotNull(node("workspace-manager"))
    }
    private fun shot(name: String) = screenshot("validation/workspace-switcher/$name.png")
    private fun scrollTop() {
        instrumentation.runOnMainSync {
            fun findScroll(node: SemanticsNode): SemanticsNode? = if (node.config.getOrNull(SemanticsActions.ScrollBy) != null) node
                else node.children.firstNotNullOfOrNull(::findScroll)
            findScroll(node("workspace-rows")!!.second)!!.config[SemanticsActions.ScrollBy].action!!.invoke(0f, -100000f)
        }
        SystemClock.sleep(350)
    }
    private fun launch() {
        scenario = launchCapy()
        scenario.onActivity { host = it.host; density = it.resources.displayMetrics.density }
        idle()
        if (host.snapshot!!.getJSONObject("state").getJSONObject("workspace").optBoolean("zen_mode")) {
            instrumentation.runOnMainSync { host.dispatch(obj("type" to "invoke", "command" to "zen_mode")) }
            waitFor("header visible") { node("workspace-switcher") != null }; idle()
        }
    }
    @Before fun ready() = launch()
    @After fun cleanup() {
        if (pressed != null) event(MotionEvent.ACTION_CANCEL)
        if (::scenario.isInitialized) scenario.close()
    }

    @Test fun inlineNameGeometryAndFooterSpacingAcrossThemes() {
        var original: Configuration? = null
        scenario.onActivity { original = Configuration(it.resources.configuration) }
        val checks = mutableListOf<() -> Unit>()
        fun frame(tag: String): Rect {
            var result: Rect? = null
            instrumentation.runOnMainSync {
                val coordinates = node(tag)!!.second.layoutInfo.coordinates
                result = Rect(coordinates.positionInRoot(), coordinates.size.toSize())
            }
            return result!!
        }
        fun geometry(id: String): List<Rect> {
            val origin = frame("workspace-row-$id").topLeft
            return listOf("workspace-name-slot-$id", "workspace-row-$id", "workspace-pin-$id", "workspace-grip-$id").map { frame(it).translate(-origin) }
        }
        fun footer(id: String) {
            val rows = bounds("workspace-rows")
            val button = bounds("new-workspace")
            assertEquals("Footer leaves six dp above its targets", 6 * density, button.top - rows.bottom, 1f)
            for (target in listOf(button, bounds("workspace-remove"))) {
                assertEquals(34 * density, target.width, 1f)
                assertEquals(34 * density, target.height, 1f)
            }
            for (tag in listOf("workspace-cancel", "workspace-confirm")) {
                val action = bounds(tag)
                assertEquals("Manager action remains fully visible", 34 * density, action.height, 1f)
                assertTrue("Manager action stays inside the panel", action.bottom <= bounds("workspace-manager").bottom + 1)
            }
            val row = frame("workspace-row-$id")
            assertTrue("Selected workspace row stays fully inside the list: $row / $rows", row.top >= rows.top - 1 && row.bottom <= rows.bottom + 1)
            instrumentation.runOnMainSync {
                val manager = node("workspace-manager")!!
                val root = manager.first.view
                val insets = checkNotNull(ViewCompat.getRootWindowInsets(root))
                val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
                assertFalse("Workspace manager preserves immersive app chrome", insets.isVisible(WindowInsetsCompat.Type.statusBars()))
                assertTrue("Manager and footer stay above the software keyboard", manager.second.boundsInRoot.bottom <= root.height - ime + 1)
            }
        }
        fun textOrigins(id: String): List<Offset> {
            val result = mutableListOf<Offset>()
            instrumentation.runOnMainSync {
                val title = node("workspace-name-$id")!!.second.find { it.config.getOrNull(SemanticsActions.GetTextLayoutResult) != null }!!
                val layouts = mutableListOf<TextLayoutResult>()
                assertTrue(title.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts))
                val text = layouts.single()
                fun layouts(info: LayoutInfo): List<LayoutInfo> = listOf(info) + (info.javaClass.getMethod("getChildrenInfo").invoke(info) as List<*>).flatMap { layouts(it as LayoutInfo) }
                val content = layouts(title.layoutInfo).last { it.coordinates.size == text.size }
                result += content.coordinates.boundsInRoot().topLeft + text.getBoundingBox(0).topLeft
                val subtitle = view().array("rows").objects().first { it.getString("id") == id }.getString("subtitle")
                result += label(node("workspace-row-$id")!!.second, subtitle)!!.layoutInfo.coordinates.boundsInRoot().topLeft
            }
            return result.map { it - frame("workspace-row-$id").topLeft }
        }
        fun unchanged(before: List<Rect>, origins: List<Offset>, after: List<Rect>, afterOrigins: List<Offset>) {
            before.zip(after).forEachIndexed { index, (first, second) ->
                val message = "Rename preserves control $index; before=$before after=$after; text=$origins → $afterOrigins"
                assertEquals(message, first.left, second.left, 1f)
                assertEquals(message, first.top, second.top, 1f)
                assertEquals(message, first.right, second.right, 1f)
                assertEquals(message, first.bottom, second.bottom, 1f)
            }
            origins.zip(afterOrigins).forEach { (first, second) ->
                assertEquals("Rename preserves text origin", first.x, second.x, 1f)
                assertEquals("Rename preserves text origin", first.y, second.y, 1f)
            }
        }
        try {
            for (scale in listOf(1f, 1.3f)) {
                scenario.onActivity { activity ->
                    val configuration = Configuration(original!!).apply { fontScale = scale }
                    @Suppress("DEPRECATION")
                    activity.resources.updateConfiguration(configuration, activity.resources.displayMetrics)
                    activity.window.decorView.dispatchConfigurationChanged(configuration)
                    activity.onConfigurationChanged(configuration)
                }
                for (theme in listOf("light", "dark")) {
                    waitFor("document accepts theme") { !host.documentInputBlocked }
                    host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
                    waitFor("geometry theme") { host.snapshot!!.getJSONObject("state").getString("theme") == theme }
                    val custom = newWorkspace("Geometry $theme $scale")
                    open(); tap("workspace-row-$custom")
                    val before = geometry(custom); val origins = textOrigins(custom)
                    shot("name-normal-$theme-$scale")
                    val metrics = host.catalog.getJSONObject("panel_metrics")
                    assertTrue("Name slot reserves the shared compact height", before[0].height + 1 >= metrics.getDouble("control_height") * density)
                    footer(custom)
                    focus("workspace-row-$custom"); key(KeyEvent.KEYCODE_F2); idle()
                    waitFor("geometry inline editor") { node("workspace-name-$custom")?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } != null }
                    key(KeyEvent.KEYCODE_MOVE_HOME)
                    shot("rename-geometry-$theme-$scale")
                    val editing = geometry(custom); val editingOrigins = textOrigins(custom)
                    footer(custom)
                    key(KeyEvent.KEYCODE_ESCAPE); idle(); shot("name-canceled-$theme-$scale")
                    val canceled = geometry(custom); val canceledOrigins = textOrigins(custom)
                    footer(custom)
                    checks += { unchanged(before, origins, editing, editingOrigins); unchanged(before, origins, canceled, canceledOrigins) }
                    focus("workspace-row-$custom"); key(KeyEvent.KEYCODE_F2); idle()
                    renameInline(custom, "Renamed $theme $scale")
                    assertEquals("Renamed $theme $scale", view().getString("name"))
                    if (scale == 1f) {
                        val panel = bounds("workspace-manager")
                        instrumentation.runOnMainSync { pressed = node("workspace-manager")!!.first }
                        event(MotionEvent.ACTION_DOWN, Offset(panel.left + 2 * density, panel.center.y)); event(MotionEvent.ACTION_UP); idle()
                        assertNotNull("Empty panel padding keeps the manager open", node("workspace-manager"))
                        instrumentation.runOnMainSync { pressed = node("workspace-manager")!!.first }
                        event(MotionEvent.ACTION_DOWN, Offset(2 * density, panel.center.y)); event(MotionEvent.ACTION_UP); idle()
                        assertNull("Backdrop dismisses the manager", node("workspace-manager"))
                        open()
                    }
                    key(KeyEvent.KEYCODE_ESCAPE); idle()
                    assertNull(node("workspace-manager"))
                }
            }
            checks.forEach { it() }
        } finally {
            scenario.onActivity { activity ->
                @Suppress("DEPRECATION")
                activity.resources.updateConfiguration(original!!, activity.resources.displayMetrics)
                activity.window.decorView.dispatchConfigurationChanged(original!!)
                activity.onConfigurationChanged(original!!)
            }
        }
    }

    @Test fun bottomListActionsCreateDeleteAndKeepManagerAcrossThemes() {
        for (theme in listOf("light", "dark")) {
            waitFor("restored document accepts input") { !host.documentInputBlocked }
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            waitFor("requested button theme") { host.snapshot!!.getJSONObject("state").getString("theme") == theme }
            val before = order()
            val name = "New workspace $theme"
            open()
            val button = bounds("new-workspace")
            val rows = bounds("workspace-rows")
            val rowBounds = view().array("rows").objects().map { bounds("workspace-row-${it.getString("id")}") }
            rowBounds.zipWithNext().forEach { (first, second) -> assertEquals("Workspace rows keep their native pitch", 56 * density, second.top - first.top, 1f) }
            assertTrue("Last workspace row stays inside the list", rowBounds.last().bottom <= rows.bottom + 1)
            assertTrue("New Workspace appears below the list", button.top >= rows.bottom)
            assertEquals("New Workspace aligns with the list's left edge", rows.left, button.left, density)
            assertTrue("Delete appears below the list", bounds("workspace-remove").top >= rows.bottom)
            assertNull(node("workspace-search"))
            instrumentation.runOnMainSync {
                val caption = host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("new_workspace")
                assertTrue(caption in (node("new-workspace")!!.second.config.getOrNull(SemanticsProperties.ContentDescription) ?: emptyList()))
                assertFalse(texts(node("workspace-manager")!!.second).contains(view().getString("intro")))
                fun count(current: SemanticsNode): Int = (if (hasTag("new-workspace")(current)) 1 else 0) + current.children.sumOf(::count)
                assertEquals(1, semanticsRoots().sumOf { count(it.semanticsOwner.unmergedRootSemanticsNode) })
            }
            val current = view().getString("id")
            val pinned = current in ids("switcher")
            tap("workspace-pin-$current"); assertEquals(!pinned, current in ids("switcher"))
            tap("workspace-pin-$current"); assertEquals(pinned, current in ids("switcher"))
            shot("new-workspace-$theme")
            tap("new-workspace")
            waitFor("new workspace name form") { node("workspace-name") != null && view().getJSONObject("prompt_action").getString("type") == "new" }
            instrumentation.runOnMainSync {
                val input = node("workspace-name")!!.second.find { it.config.getOrNull(SemanticsActions.SetText) != null }!!
                assertTrue(input.config[SemanticsActions.SetText].action!!.invoke(androidx.compose.ui.text.AnnotatedString(name)))
            }
            tap("workspace-submit")
            waitFor("created workspace selected") { view().getString("name") == name && view().isNull("prompt") }
            val created = view().getString("id")
            assertFalse(created in before)
            assertEquals(before.toSet() + created, order().toSet())
            assertTrue(created in ids("switcher"))
            assertNotNull("Creating keeps the manager open", node("workspace-manager"))
            assertEquals("workspaces", view().getString("page"))
            key(KeyEvent.KEYCODE_ESCAPE); idle()
            assertNull(node("workspace-manager"))
            scenario.close(); SystemClock.sleep(300); launch()
            assertEquals(created, view().getString("id")); assertEquals(name, view().getString("name"))
            assertTrue(created in ids("switcher"))
            open(); tap("workspace-row-$created"); tap("workspace-remove")
            waitFor("delete confirmation") { !view().isNull("prompt") }
            tap("workspace-submit")
            waitFor("workspace removed") { created !in order() && view().isNull("prompt") }
            assertNotNull("Deleting keeps the manager open", node("workspace-manager"))
            assertEquals("workspaces", view().getString("page"))
            key(KeyEvent.KEYCODE_ESCAPE); idle(); assertNull(node("workspace-manager"))
        }
    }

    @Test fun pinsOrderingPreviewKeyboardAndRestart() {
        val custom = newWorkspace("Tablet Switcher")
        assertTrue("New workspaces start pinned", custom in ids("switcher"))
        val before = capture()
        val current = view().getString("id")
        open(); tap("workspace-row-${defaults.last()}")
        shot("manager")
        val preview = layout()
        for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            for (selected in listOf(false, true)) {
                tap("workspace-pin-$current")
                assertEquals(selected, current in ids("switcher"))
                assertEquals(defaults.last(), view().getString("selected"))
                assertEquals(preview, layout()); assertEquals(before, capture())
                assertNull(node("workspace-row-menu"))
            }
            for (count in listOf(2, 3, 3, 4)) {
                val selected = current in ids("switcher")
                val revision = view().getLong("switcher_revision")
                repeat(count) { down("workspace-pin-$current", 0); event(MotionEvent.ACTION_UP, delay = 0) }
                waitFor("every rapid pin activation saves") { view().getLong("switcher_revision") >= revision + count }
                idle()
                assertEquals(if (count % 2 == 0) selected else !selected, current in ids("switcher"))
                assertEquals(defaults.last(), view().getString("selected"))
                assertEquals(preview, layout()); assertEquals(before, capture())
                assertNull(node("workspace-row-menu"))
            }
        }
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap("workspace-pin-$current")
        assertFalse(current in ids("switcher")); assertEquals(current, ids("switcher_display").first())
        assertEquals(preview, layout()); assertEquals(before, capture())
        assertEquals(defaults.last(), view().getString("selected"))
        assertNotNull(node("workspace-pin-$current"))
        assertFalse(view().array("rows").objects().first { it.getString("id") == current }.getJSONObject("pin").getBoolean("selected"))
        assertTrue(view().array("rows").objects().first { it.getString("id") == current }.getJSONObject("pin").getString("label") in node("workspace-pin-$current")!!.second.config[SemanticsProperties.ContentDescription])
        val previous = order().indexOf(current)
        tap("workspace-grip-$current")
        shot("options")
        waitFor("menu keyboard focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
        key(KeyEvent.KEYCODE_TAB) // Leave native touch mode before requesting focus.
        instrumentation.runOnMainSync { assertTrue(node("workspace-up")!!.second.config[SemanticsActions.RequestFocus].action!!.invoke()) }
        key(KeyEvent.KEYCODE_ENTER); idle()
        assertEquals(previous - 1, order().indexOf(current))
        assertFalse(current in ids("switcher")); assertEquals(preview, layout()); assertEquals(before, capture())
        for (id in ids("switcher").toList()) tap("workspace-pin-$id")
        assertTrue(ids("switcher").isEmpty()); assertEquals(listOf(current), ids("switcher_display"))
        tap("workspace-cancel"); assertEquals(before, capture())
        shot("unpinned-current")
        tap("workspace-switch-$current"); assertEquals(current, view().getString("id"))
        open(); tap("workspace-row-${defaults.first()}"); tap("workspace-confirm")
        assertEquals(listOf(defaults.first()), ids("switcher_display"))
        open(); tap("workspace-pin-$custom"); tap("workspace-cancel")
        assertTrue(custom in ids("switcher_display"))
        tap("workspace-switch-$custom"); assertEquals(before, capture())
        val savedOrder = order(); val savedPins = ids("switcher")
        scenario.close(); SystemClock.sleep(300); launch()
        assertEquals(savedOrder, order()); assertEquals(savedPins, ids("switcher")); assertEquals(custom, view().getString("id"))
        assertEquals(before, capture())
    }

    private fun menuRows() = view().getJSONObject("switcher_options").array("sections").getJSONArray(0).objects()
    private fun label(node: SemanticsNode, text: String): SemanticsNode? = node.find {
        it.config.getOrNull(SemanticsProperties.Text)?.any { value -> value.text == text } == true
    }
    private fun texts(node: SemanticsNode): List<String> =
        (node.config.getOrNull(SemanticsProperties.Text)?.map { it.text } ?: emptyList()) + node.children.flatMap(::texts)
    private fun tapMenu(text: String) {
        waitFor("menu receives native input") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
        instrumentation.runOnMainSync {
            val popup = checkNotNull(node("workspace-menu"))
            pressed = popup.first
            point = checkNotNull(label(popup.second, text)) { "Missing menu row $text" }.boundsInRoot.center
        }
        event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP); idle()
    }
    private fun assertOptions(current: String, savedLayout: Any?, savedOrder: List<String>) {
        waitFor("workspace options menu") { node("workspace-menu") != null }
        instrumentation.runOnMainSync {
            val popup = checkNotNull(node("workspace-menu")).second
            val options = view().getJSONObject("switcher_options")
            assertEquals(options.array("sections").values().flatMap {
                (it as JSONArray).objects().map { row -> row.getString("label") }
            }, texts(popup))
            menuRows().forEach { row ->
                val item = checkNotNull(label(popup, row.getString("label")))
                val semantics = generateSequence(item) { it.parent }.first { it.config.getOrNull(SemanticsProperties.Role) == Role.Checkbox }.config
                assertEquals(ToggleableState(row.optBoolean("selected")), semantics.getOrNull(SemanticsProperties.ToggleableState))
            }
        }
        assertEquals("Context input keeps the workspace", current, view().getString("id"))
        assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
    }
    private fun closeOptions() {
        waitFor("options native focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
        key(KeyEvent.KEYCODE_ESCAPE)
        waitFor("options dismissed") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
    }
    private fun focus(tag: String) {
        key(KeyEvent.KEYCODE_TAB, KeyEvent.META_SHIFT_ON)
        var focused: SemanticsNode? = null
        instrumentation.runOnMainSync {
            focused = checkNotNull(node(tag)).second.find { it.config.getOrNull(SemanticsActions.RequestFocus) != null }
            assertTrue(checkNotNull(focused).config[SemanticsActions.RequestFocus].action!!.invoke())
        }
        waitFor("$tag keyboard focus") { node(tag)?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null }
    }
    @Test fun nativeOptionsVisibilityInputsAndRestart() {
        val current = view().getString("id")
        val inactive = ids("switcher_display").first { it != current }
        val savedLayout = layout(); val savedOrder = order(); val durable = capture()
        for (theme in listOf("light", "dark")) {
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            assertEquals(bounds("workspace-switch-$current").height, bounds("workspace-switcher-options").height, 1f)
            assertEquals(20 * density, bounds("workspace-switcher-options").width, 1f)
            assertEquals(5 * density, bounds("workspace-switcher").right - bounds("workspace-switcher-options").right, 1f)
            for (tag in listOf("workspace-switch-$current", "workspace-switch-$inactive", "workspace-switcher-options", "workspace-switcher")) {
                tool = MotionEvent.TOOL_TYPE_MOUSE; button = MotionEvent.BUTTON_SECONDARY
                if (tag == "workspace-switcher") {
                    instrumentation.runOnMainSync {
                        val (root, well) = checkNotNull(node(tag)); pressed = root
                        point = Offset(well.boundsInRoot.center.x, well.boundsInRoot.top + 2 * density)
                    }
                    event(MotionEvent.ACTION_DOWN)
                } else down(tag)
                event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
                assertOptions(current, savedLayout, savedOrder); closeOptions()
            }
            tool = MotionEvent.TOOL_TYPE_FINGER
            tap("workspace-switcher-options"); assertOptions(current, savedLayout, savedOrder)
            shot("visibility-$theme"); closeOptions(); shot("header-$theme")
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                down("workspace-switch-$inactive")
                waitFor("$pointer hold opens workspace options") { node("workspace-menu") != null }
                assertOptions(current, savedLayout, savedOrder)
                event(MotionEvent.ACTION_UP)
                waitFor("hold menu retains focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
                assertEquals(current, view().getString("id")); closeOptions()
                down("workspace-switch-$inactive")
                waitFor("$pointer hold before Escape") { node("workspace-menu") != null }
                key(KeyEvent.KEYCODE_ESCAPE); event(MotionEvent.ACTION_UP); idle()
                assertNull("Escape retires the held menu", node("workspace-menu")); assertEquals(current, view().getString("id"))
            }
            focus("workspace-switcher-options")
            for ((code, modifiers) in listOf(KeyEvent.KEYCODE_MENU to 0, KeyEvent.KEYCODE_F10 to KeyEvent.META_SHIFT_ON)) {
                key(code, modifiers); assertOptions(current, savedLayout, savedOrder); closeOptions()
                instrumentation.runOnMainSync { assertNotNull("Escape returns control focus", node("workspace-switcher-options")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true }) }
            }
            val keyboardRow = menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == inactive }.getString("label")
            tap("workspace-switcher-options")
            for (pinned in listOf(false, true)) {
                waitFor("checkbox keyboard focus window") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
                key(KeyEvent.KEYCODE_TAB, KeyEvent.META_SHIFT_ON)
                instrumentation.runOnMainSync {
                    val item = checkNotNull(label(checkNotNull(node("workspace-menu")).second, keyboardRow))
                    val row = generateSequence(item) { it.parent }.first { it.config.getOrNull(SemanticsActions.RequestFocus) != null }
                    assertTrue(row.config[SemanticsActions.RequestFocus].action!!.invoke())
                }
                key(KeyEvent.KEYCODE_ENTER); idle()
                waitFor("keyboard checkbox saves $pinned") { (inactive in ids("switcher")) == pinned }
                assertNotNull("Visibility checkboxes keep their menu open", node("workspace-menu"))
                assertEquals(current, view().getString("id")); assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
            }
            for (id in ids("switcher").toList()) {
                tapMenu(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == id }.getString("label"))
                waitFor("$id pin cleared") { id !in ids("switcher") }
                assertEquals(current, view().getString("id")); assertEquals(savedOrder, order()); assertEquals(savedLayout, layout())
            }
            assertEquals(emptyList<String>(), ids("switcher")); assertEquals(listOf(current), ids("switcher_display"))
            assertOptions(current, savedLayout, savedOrder)
            assertFalse(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == current }.optBoolean("selected"))
            for (id in savedOrder) {
                tapMenu(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == id }.getString("label"))
            }
            assertEquals(savedOrder, ids("switcher")); assertEquals(durable, capture())
            tapMenu(view().getJSONObject("switcher_options").array("sections").getJSONArray(1).getJSONObject(0).getString("label"))
            waitFor("Manage opens full editor after dismissing options") { node("workspace-manager") != null && node("workspace-menu") == null }
            tap("workspace-cancel")
        }
        scenario.close(); SystemClock.sleep(300); launch()
        assertEquals(savedOrder, ids("switcher")); assertEquals(current, view().getString("id")); assertEquals(durable, capture())
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap("workspace-switch-$inactive"); assertEquals(inactive, view().getString("id"))
    }

    private fun sharedPopupOpen(): Boolean {
        var reply: JSONObject? = null
        val complete = java.util.concurrent.CountDownLatch(1)
        host.drain()
        instrumentation.runOnMainSync {
            host.chrome(obj("kind" to "contact", "canvas" to true, "position" to JSONArray(listOf(50, 50))), reply = { reply = it; complete.countDown() })
        }
        assertTrue(complete.await(10, java.util.concurrent.TimeUnit.SECONDS))
        return checkNotNull(reply).optBoolean("dismiss_popups")
    }
    private fun menuCheckbox(text: String): SemanticsNode? = node("workspace-menu")?.second?.let { root ->
        label(root, text)?.let { item -> generateSequence(item) { it.parent }.firstOrNull { it.config.getOrNull(SemanticsProperties.Role) == Role.Checkbox } }
    }
    @Test fun retainedOptionsRefreshTheirRowsAndOwnTheSharedPopup() {
        for (compact in listOf(false, true)) {
            if (compact) header(true)
            val current = view().getString("id"); val inactive = ids("switcher").first { it != current }
            val savedLayout = layout(); val savedOrder = order()
            val title = menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == inactive }.getString("label")
            if (compact) {
                tap("header-control-900")
                tapMenu(view().getJSONObject("switcher_menu").array("sections").getJSONArray(1).getJSONObject(0).getString("label"))
            }
            if (!compact) tap("workspace-switcher-options")
            waitFor("retained options native focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
            val popup = node("workspace-menu")!!.first.view
            for (pinned in listOf(false, true)) {
                tapMenu(title)
                waitFor("retained checkbox reflects $pinned") {
                    (inactive in ids("switcher")) == pinned && menuCheckbox(title)?.config?.getOrNull(SemanticsProperties.ToggleableState) == ToggleableState(pinned)
                }
                instrumentation.runOnMainSync { assertSame("Pin changes retain the native popup", popup, node("workspace-menu")!!.first.view) }
                assertTrue("Retained menu keeps shared popup ownership", sharedPopupOpen())
            }
            closeOptions()
            assertFalse("Dismissal releases shared popup ownership", sharedPopupOpen())
            assertEquals(current, view().getString("id")); assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
        }
    }

    private fun header(compact: Boolean = false) {
        val workspace = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString())
        var next = 1
        fun entry(kind: String) = obj("id" to next++, "item" to obj("kind" to kind))
        val left = if (compact) JSONArray(listOf(entry("capy"), entry("menu")) + List(40) { entry("space") }) else JSONArray()
        val selector = obj("id" to 900, "item" to obj("kind" to "workspaces"))
        if (!compact) left.put(selector)
        val center = if (compact) JSONArray(listOf(selector) + List(30) { entry("space") }) else JSONArray()
        workspace.getJSONObject("layout").put("header", obj("size" to "small", "next_id" to 901,
            "zones" to JSONArray(listOf(left, center, JSONArray()))))
        host.drain(obj("type" to "restore_workspace", "workspace" to workspace)); idle()
        waitFor("workspace header presentation") { node(if (compact) "header-control-900" else "workspace-switcher") != null }
    }
    @Test fun compactAndEditingContextMenusPreserveHeaderPlacement() {
        device.landscape(scenario)
        idle()
        for (theme in listOf("light", "dark")) for (compact in listOf(false, true)) {
            host.drain(obj("type" to "set_theme", "theme" to theme)); header(compact)
            val current = view().getString("id"); val savedLayout = layout(); val savedOrder = order()
            val tag = if (compact) "header-control-900" else "workspace-switch-${ids("switcher_display").first { it != current }}"
            val zone = if (compact) 1 else 0
            fun presentedSource(): String {
                    idle()
                    var source = ""
                    waitFor("workspace item resolves its editor geometry") {
                        source = listOf(tag, "workspace-switch-${ids("switcher_display").firstOrNull { it != current }}", "header-control-900", "header-overflow-item-900", "header-overflow-$zone").firstOrNull { node(it) != null } ?: ""
                        source.isNotEmpty()
                    }
                    if (source == "header-overflow-$zone") {
                        val pointer = tool; val buttons = button
                        tool = MotionEvent.TOOL_TYPE_FINGER; button = MotionEvent.BUTTON_PRIMARY
                        tap(source); tool = pointer; button = buttons
                        waitFor("workspace overflow representation") { node("header-overflow-item-900") != null }
                        source = "header-overflow-item-900"
                    }
                    return source
                }
            if (compact) {
                waitFor("compact header settles after workspace refresh") {
                    !view().optBoolean("switcher_busy") && node(tag) != null
                }
                tool = MotionEvent.TOOL_TYPE_FINGER; tap(tag)
                waitFor("compact switching menu") { node("workspace-menu") != null }
                val menu = view().getJSONObject("switcher_menu")
                instrumentation.runOnMainSync {
                    assertEquals(listOf(menu.getString("title")) + menu.array("sections").values().flatMap {
                        (it as JSONArray).objects().map { row -> row.getString("label") }
                    }, texts(checkNotNull(node("workspace-menu")).second))
                }
                tapMenu(menu.array("sections").getJSONArray(1).getJSONObject(0).getString("label"))
                instrumentation.runOnMainSync {
                    assertEquals(menuRows().map { it.getString("label") }, texts(checkNotNull(node("workspace-menu")).second).drop(1))
                }
                tapMenu(menuRows().first().getString("label"))
                assertEquals(current, view().getString("id")); assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
                tool = MotionEvent.TOOL_TYPE_MOUSE; button = MotionEvent.BUTTON_SECONDARY
                down(presentedSource()); event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
                assertOptions(current, savedLayout, savedOrder)
                tapMenu(menuRows().first().getString("label"))
            }
            for (editing in listOf(false, true)) {
                if (editing) {
                    host.drain(obj("type" to "invoke", "command" to "customize_workspace_ui"))
                    waitFor("title bar editor") { host.snapshot!!.getJSONObject("header").optBoolean("editing") }
                }
                val zones = host.snapshot!!.getJSONObject("header").getJSONObject("model").array("zones").values().map { it as JSONArray }
                val actualZone = zones.indexOfFirst { entries -> entries.objects().any { it.getInt("id") == 900 && it.getJSONObject("item").getString("kind") == "workspaces" } }
                assertEquals(zone, actualZone)
                android.util.Log.i("SwitcherAcceptance", "Context $theme compact=$compact editing=$editing choices=${ids("switcher_display")}")
                tool = MotionEvent.TOOL_TYPE_MOUSE; button = MotionEvent.BUTTON_SECONDARY
                down(presentedSource()); event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
                assertOptions(current, savedLayout, savedOrder); closeOptions()
                for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                    tool = pointer; down(presentedSource())
                    waitFor("$pointer workspace hold in editor=$editing") { node("workspace-menu") != null }
                    event(MotionEvent.ACTION_UP); assertOptions(current, savedLayout, savedOrder); closeOptions()
                }
                if (editing) {
                    tool = MotionEvent.TOOL_TYPE_STYLUS
                    val source = presentedSource()
                    val placementTag = if (source == "header-overflow-item-900") source else "header-item-900"
                    val placement = bounds(placementTag)
                    down(if (placementTag == source) source else "header-grip-900")
                    event(MotionEvent.ACTION_MOVE, point + Offset(40 * density, 0f))
                    waitFor("workspace placement starts without hold") {
                        if (placementTag == source) node("header-drag-ghost") != null else node(placementTag)?.second?.boundsInRoot?.left != placement.left
                    }
                    event(MotionEvent.ACTION_CANCEL); idle()
                    assertEquals(savedLayout, layout())
                    tap("header-edit-cancel")
                    waitFor("editor cancelled") { !host.snapshot!!.getJSONObject("header").optBoolean("editing") }
                }
            }
        }
    }
    @Test fun choicesShowHeldFeedbackAndSelectOnlyOnReleaseAcrossThemes() {
        val feedbackFailures = mutableListOf<String>()
        val choices = listOf(newWorkspace("Feedback one"), newWorkspace("Feedback two"))
        open()
        view().array("rows").objects().filter { it.getString("id") !in choices && it.getJSONObject("pin").getBoolean("selected") }.forEach {
            send(obj("type" to "edit_switcher", "edit" to it.getJSONObject("pin").getJSONObject("edit")))
        }
        send(obj("type" to "dismiss"))
        for (theme in listOf("light", "dark")) {
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                header()
                val current = view().getString("id")
                val inactive = choices.first { it != current }
                for (id in listOf(current, inactive)) {
                    val tag = "workspace-switch-$id"
                    val owner = checkNotNull(node(tag)).first
                    assertNull("Workspace choice must be enabled: ${view()}", node(tag)!!.second.config.getOrNull(SemanticsProperties.Disabled))
                    try { assertButtonContactFeedback(owner, bounds(tag), "$theme tool=$pointer selected=${id == current}",
                        minimumChangedFraction = if (id == current) .005f else .02f) { phase ->
                        if (phase == MotionEvent.ACTION_DOWN) down(tag) else event(phase)
                        assertEquals("Held or cancelled contact preserves the workspace", current, view().getString("id"))
                    } } catch (failure: AssertionError) {
                        feedbackFailures.add(failure.message ?: failure.toString())
                        instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "${feedbackFailures.last()}\n") })
                    }
                }
            }
        }
        for (theme in listOf("light", "dark")) {
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                header()
                val current = view().getString("id")
                val inactive = choices.first { it != current }
                val tag = "workspace-switch-$inactive"
                down(tag)
                assertEquals("A workspace press waits for release", current, view().getString("id"))
                event(MotionEvent.ACTION_UP); idle()
                assertEquals("Release adopts the workspace", inactive, view().getString("id"))
                waitFor("Released workspace is visibly selected") {
                    node(tag)?.second?.config?.getOrNull(SemanticsProperties.Selected) == true
                }
            }
        }
        assertTrue(feedbackFailures.joinToString("\n"), feedbackFailures.isEmpty())
    }

    @Test fun scrollingChoicesKeepsOptionsFixedAndDoesNotSwitch() {
        device.landscape(scenario)
        idle()
        repeat(15) { newWorkspace("Workspace ${it.toString().padStart(2, '0')} long name") }
        header()
        val current = view().getString("id"); val savedLayout = layout(); val savedOrder = order()
        val dots = bounds("workspace-switcher-options")
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            val choices = bounds("workspace-switcher-choices")
            instrumentation.runOnMainSync { pressed = node("workspace-switcher-choices")!!.first }
            point = choices.center + Offset(choices.width * .2f, 0f)
            event(MotionEvent.ACTION_DOWN)
            val start = point
            for (step in 1..10) event(MotionEvent.ACTION_MOVE, start - Offset(choices.width * .4f * step / 10, 0f))
            event(MotionEvent.ACTION_UP); idle()
            assertNull(node("workspace-menu")); assertEquals(dots, bounds("workspace-switcher-options"))
            assertEquals(current, view().getString("id")); assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
            tap("workspace-switcher-options"); assertOptions(current, savedLayout, savedOrder); closeOptions()
        }
        shot("scrolling-fixed-options")
    }

    private fun hold(pointer: Int) {
        if (pointer != MotionEvent.TOOL_TYPE_MOUSE) return waitFor("$pointer held menu") { node("workspace-row-menu") != null }
        SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() + 300)
        instrumentation.runOnMainSync { assertNull("Mouse holds never open menus", node("workspace-row-menu")) }
    }

    @Test fun gripMenusAndContextMetadataKeepPreviewAcrossThemesAndInputs() {
        val custom = newWorkspace("Grip options")
        val durable = capture()
        val originalOrder = order()
        val source = defaults[1]
        val target = defaults.last()
        val moved = originalOrder.toMutableList().apply { remove(source); add(indexOf(target) + 1, source) }
        fun reset() = send(obj("type" to "edit_switcher", "edit" to obj("type" to "move", "id" to source, "before" to target)))
        fun closeMenu() {
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("row menu dismissed") { node("workspace-row-menu") == null && node("workspace-manager")?.first?.view?.hasWindowFocus() == true }
        }
        for (theme in listOf("light", "dark")) {
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            open(); tap("workspace-row-$target")
            val preview = layout()
            val pins = ids("switcher")
            fun unchanged() {
                assertEquals(target, view().getString("selected"))
                assertEquals(custom, view().getString("id"))
                assertEquals(preview, layout()); assertEquals(durable, capture())
                assertEquals(pins.toSet(), ids("switcher").toSet())
            }
            fun moveMenu() {
                waitFor("grip menu native focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
                instrumentation.runOnMainSync {
                    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
                    assertEquals(listOf(copy.getString("move_up"), copy.getString("move_down")), texts(node("workspace-row-menu")!!.second))
                    assertNotNull(node("workspace-up")); assertNotNull(node("workspace-down"))
                    assertNull(node("workspace-rename")); assertNull(node("workspace-delete"))
                }
                assertEquals(originalOrder, order()); unchanged()
            }
            instrumentation.runOnMainSync {
                for (id in defaults) {
                    assertNull("Built-in rows have no options button", node("workspace-options-$id"))
                    assertEquals(0, view().array("rows").objects().first { it.getString("id") == id }.array("options").length())
                }
                assertNull("Custom rows have no options button", node("workspace-options-$custom"))
                for (id in originalOrder) {
                    val pin = node("workspace-pin-$id")!!.second.boundsInRoot; val grip = node("workspace-grip-$id")!!.second.boundsInRoot
                    assertTrue("Grip follows the pin on the right", grip.left >= pin.right - density)
                    assertTrue("Grip stays beside its pin", grip.left - pin.right <= 8 * density)
                }
            }
            for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                tap("workspace-grip-$source"); moveMenu(); closeMenu()
                val destination = bounds("workspace-row-$target").let { Offset(it.center.x, it.bottom - 5 * density) }
                down("workspace-grip-$source")
                event(MotionEvent.ACTION_MOVE, destination)
                waitFor("grip drag starts immediately") { node("workspace-row-drop-hint") != null && node("workspace-row-menu") == null }
                event(MotionEvent.ACTION_UP); idle()
                assertEquals(moved, order()); assertNull(node("workspace-row-menu")); unchanged(); reset()
                val pinned = source in ids("switcher")
                tap("workspace-pin-$source"); assertEquals(!pinned, source in ids("switcher"))
                assertEquals(target, view().getString("selected")); assertNull(node("workspace-row-menu"))
                tap("workspace-pin-$source"); unchanged()
            }
            tool = MotionEvent.TOOL_TYPE_FINGER
            for (code in listOf(KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_SPACE)) {
                focus("workspace-grip-$source"); key(code); idle(); moveMenu(); closeMenu()
            }
            focus("workspace-grip-$source"); key(KeyEvent.KEYCODE_ENTER); moveMenu()
            focus("workspace-up"); key(KeyEvent.KEYCODE_ENTER); idle()
            assertEquals(originalOrder.toMutableList().apply { remove(source); add(0, source) }, order())
            unchanged(); reset()
            tap("workspace-grip-$source"); moveMenu(); shot("grip-menu-$theme"); closeMenu()
            tool = MotionEvent.TOOL_TYPE_MOUSE; button = MotionEvent.BUTTON_SECONDARY
            down("workspace-row-$custom"); event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
            waitFor("custom options native focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
            instrumentation.runOnMainSync {
                val options = view().array("rows").objects().first { it.getString("id") == custom }.array("options").objects()
                assertEquals(listOf("rename", "delete"), options.map { it.getJSONObject("action").getString("type") })
                assertTrue(options.all { it.getBoolean("enabled") })
                val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
                assertEquals(options.map { it.getString("label") } + listOf(copy.getString("move_up"), copy.getString("move_down")), texts(node("workspace-row-menu")!!.second))
                assertNotNull(node("workspace-rename")); assertNotNull(node("workspace-delete"))
                assertNotNull(node("workspace-up")); assertNotNull(node("workspace-down"))
            }
            unchanged(); shot("custom-context-$theme"); closeMenu()
            tool = MotionEvent.TOOL_TYPE_FINGER
            tap("workspace-cancel"); assertEquals(durable, capture())
        }
    }

    @Test fun nativeRowPickupScrollingMenusAndCancellation() {
        open()
        val originalOrder = order()
        val a = originalOrder[0]; val b = originalOrder[1]; val c = originalOrder[2]
        tap("workspace-row-$c")
        val preview = layout(); val durable = capture(); val pins = ids("switcher")
        fun reset() { send(obj("type" to "edit_switcher", "edit" to obj("type" to "move", "id" to a, "before" to b))) }
        for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            if (pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                down("workspace-row-$a")
                event(MotionEvent.ACTION_MOVE, bounds("workspace-row-$c").let { Offset(it.center.x, it.bottom - 5 * density) })
                event(MotionEvent.ACTION_UP); idle()
                assertEquals("Mouse row starts immediately", listOf(b, c, a) + originalOrder.drop(3), order()); reset()
            }
            for (grip in listOf(true, false)) {
                for (cancel in listOf(true, false)) {
                    val destination = bounds("workspace-row-$c").let { Offset(it.center.x, it.bottom - 5 * density) }
                    down("workspace-${if (grip) "grip" else "row"}-$a")
                    if (!grip) hold(pointer)
                    event(MotionEvent.ACTION_MOVE, destination)
                    waitFor("$pointer/$grip insertion hint") { node("workspace-row-drop-hint") != null && node("workspace-row-menu") == null }
                    if (cancel) { key(KeyEvent.KEYCODE_ESCAPE); event(MotionEvent.ACTION_UP) }
                    else event(MotionEvent.ACTION_UP)
                    idle()
                    assertEquals(if (cancel) originalOrder else listOf(b, c, a) + originalOrder.drop(3), order())
                    assertEquals(preview, layout()); assertEquals(durable, capture())
                    assertEquals(pins.toSet(), ids("switcher").toSet())
                    reset()
                }
            }
            // Release preserves touch/pen menus. Mouse holds retain ordinary selection.
            down("workspace-row-$a"); hold(pointer); event(MotionEvent.ACTION_UP); idle()
            if (pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                instrumentation.runOnMainSync { assertNull(node("workspace-row-menu")) }
                tool = MotionEvent.TOOL_TYPE_FINGER; tap("workspace-row-$c"); tool = pointer
            } else {
                waitFor("retained menu focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
                key(KeyEvent.KEYCODE_ESCAPE)
                waitFor("menu dismissed and manager focused") { node("workspace-row-menu") == null && node("workspace-manager")?.first?.view?.hasWindowFocus() == true }
            }
            assertEquals(preview, layout()); assertEquals(durable, capture())
        }
        tool = MotionEvent.TOOL_TYPE_MOUSE; button = MotionEvent.BUTTON_SECONDARY
        down("workspace-row-$a"); event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
        waitFor("secondary menu") { node("workspace-row-menu") != null }; key(KeyEvent.KEYCODE_ESCAPE)
        tap("workspace-cancel")
        repeat(12) { newWorkspace("Scroll ${it.toString().padStart(2, '0')}") }
        open()
        assertNull("Long workspace lists have no search field", node("workspace-search"))
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer; scrollTop()
            val before = order(); val rows = bounds("workspace-rows")
            val first = before.first(); val top = bounds("workspace-row-$first").top
            val source = before[3]
            down("workspace-row-$source")
            val start = point
            event(MotionEvent.ACTION_MOVE, start - Offset(0f, 30 * density))
            event(MotionEvent.ACTION_MOVE, start - Offset(0f, 110 * density)); SystemClock.sleep(700)
            instrumentation.runOnMainSync {
                assertNull(node("workspace-row-drop-hint")); assertNull(node("workspace-row-menu"))
                assertTrue("Touch/pen swipe scrolls", node("workspace-row-$first")!!.second.boundsInRoot.height == 0f || node("workspace-row-$first")!!.second.boundsInRoot.top < top)
            }
            event(MotionEvent.ACTION_UP); idle(); assertEquals(before, order())
            scrollTop()
            down("workspace-grip-$first")
            event(MotionEvent.ACTION_MOVE, Offset(rows.center.x, rows.bottom - 8 * density)); SystemClock.sleep(1000)
            event(MotionEvent.ACTION_UP); idle()
            assertTrue("Drag autoscrolls toward later rows", order().indexOf(first) > 4)
        }
    }

    @Test fun inlineRenameContextSwipeDeleteAndEscapeAcrossThemes() {
        for (theme in listOf("light", "dark")) {
            waitFor("document accepts theme") { !host.documentInputBlocked }
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            waitFor("requested manager theme") { host.snapshot!!.getJSONObject("state").getString("theme") == theme }
            val custom = newWorkspace("Native name $theme")
            val savedOrder = order(); val savedPins = ids("switcher")
            open(); tap("workspace-row-$custom")
            val savedLayout = layout(); val durable = capture()
            for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                button = if (pointer == MotionEvent.TOOL_TYPE_MOUSE) MotionEvent.BUTTON_SECONDARY else MotionEvent.BUTTON_PRIMARY
                down("workspace-row-$custom")
                if (pointer != MotionEvent.TOOL_TYPE_MOUSE) waitFor("native held metadata menu") { node("workspace-row-menu") != null }
                event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
                waitFor("metadata menu receives native input") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
                assertNotNull(node("workspace-rename")); assertNotNull(node("workspace-delete"))
                assertEquals(custom, view().getString("selected")); assertEquals(savedOrder, order())
                assertEquals(savedLayout, layout()); assertEquals(durable, capture())
                key(KeyEvent.KEYCODE_ESCAPE); idle()
                assertNotNull(node("workspace-manager"))
            }
            tool = MotionEvent.TOOL_TYPE_MOUSE
            down("workspace-name-$custom"); event(MotionEvent.ACTION_UP)
            down("workspace-name-$custom"); event(MotionEvent.ACTION_UP); idle()
            renameInline(custom, "Double click $theme")
            focus("workspace-row-$custom"); key(KeyEvent.KEYCODE_F2); idle()
            renameInline(custom, "Keyboard name $theme")
            button = MotionEvent.BUTTON_SECONDARY
            down("workspace-row-$custom"); event(MotionEvent.ACTION_UP); button = MotionEvent.BUTTON_PRIMARY
            waitFor("rename context focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
            tap("workspace-rename"); renameInline(custom, "Context name $theme")
            assertEquals(savedOrder, order()); assertEquals(savedPins, ids("switcher"))
            assertEquals(savedLayout, layout()); assertEquals(durable, capture())
            focus("workspace-row-$custom"); key(KeyEvent.KEYCODE_F2); idle()
            key(KeyEvent.KEYCODE_ESCAPE); idle()
            assertNotNull("Escape cancels inline edit before closing manager", node("workspace-manager"))
            assertEquals("Context name $theme", view().array("rows").objects().first { it.getString("id") == custom }.getString("title"))
            tool = MotionEvent.TOOL_TYPE_FINGER
            down("workspace-row-$custom")
            val start = point
            for (step in 1..5) event(MotionEvent.ACTION_MOVE, start - Offset(24 * density * step, 0f))
            event(MotionEvent.ACTION_UP); idle()
            waitFor("swipe reveals Delete") { node("workspace-swipe-delete-$custom") != null }
            assertEquals(savedOrder, order()); assertNull(node("workspace-row-menu"))
            shot("inline-swipe-$theme")
            tap("workspace-swipe-delete-$custom")
            waitFor("swipe delete confirmation") { !view().isNull("prompt") }
            tap("workspace-submit")
            waitFor("swiped workspace removed") { custom !in order() && view().isNull("prompt") }
            assertNotNull("Swipe delete keeps manager open", node("workspace-manager"))
            key(KeyEvent.KEYCODE_ESCAPE); idle(); assertNull(node("workspace-manager"))
        }
    }

    @Test fun heldInlineRenameKeepsApplyRowAndPinsActiveAcrossThemes() {
        for (theme in listOf("light", "dark")) {
            waitFor("document accepts theme") { !host.documentInputBlocked }
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            waitFor("requested held rename theme") { host.snapshot!!.getJSONObject("state").getString("theme") == theme }
            val custom = newWorkspace("Held $theme")
            for ((action, pointer) in listOf("apply" to MotionEvent.TOOL_TYPE_MOUSE, "select" to MotionEvent.TOOL_TYPE_FINGER, "pin" to MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                val name = "Renamed $action $theme"
                val target = defaults.last { it != view().getString("id") }
                val rowTarget = defaults.first { it != target }
                open(); tap("workspace-row-$target")
                waitFor("$theme/$action preview is ready for Apply") { view().optBoolean("enabled") }
                focus("workspace-row-$custom"); key(KeyEvent.KEYCODE_F2); idle()
                waitFor("held inline editor") { node("workspace-name-$custom")?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } != null }
                instrumentation.runOnMainSync {
                    val editor = node("workspace-name-$custom")!!.second.find { it.config.getOrNull(SemanticsActions.SetText) != null }!!
                    assertTrue(editor.config[SemanticsActions.SetText].action!!.invoke(androidx.compose.ui.text.AnnotatedString(name)))
                }
                val pinned = custom in ids("switcher")
                val path = java.io.File(AppStorage.of(instrumentation.targetContext).workspaces, "workspaces.sqlite3-shm")
                val writer = ProcessBuilder("/apex/com.android.art/bin/dalvikvm64", "-cp",
                    instrumentation.context.applicationInfo.sourceDir + ":" + instrumentation.targetContext.applicationInfo.sourceDir,
                    "art.capycanvas.WorkspaceWriteLockProcess", path.absolutePath).redirectErrorStream(true).start()
                try {
                    assertEquals("LOCKED", writer.inputStream.bufferedReader().readLine())
                    key(KeyEvent.KEYCODE_ENTER)
                    waitFor("rename publication held") { view().optBoolean("busy") && view().objectOrNull("prompt_action")?.optString("type") == "rename" }
                    instrumentation.runOnMainSync {
                        for (tag in listOf("workspace-confirm", "workspace-row-$rowTarget", "workspace-pin-$custom"))
                            assertNull("$theme/$action $tag remains enabled during rename publication; shared=${view().optBoolean("enabled")}", node(tag)!!.second.config.getOrNull(SemanticsProperties.Disabled))
                    }
                    down(when (action) { "apply" -> "workspace-confirm"; "select" -> "workspace-row-$rowTarget"; else -> "workspace-pin-$custom" })
                    SystemClock.sleep(160)
                    event(MotionEvent.ACTION_UP)
                    shot("held-rename-$action-$theme")
                } finally {
                    writer.waitFor()
                    assertEquals(0, writer.exitValue())
                }
                idle()
                when (action) {
                    "apply" -> {
                        waitFor("queued Apply switches after rename") { view().getString("id") == target && view().isNull("page") }
                        open()
                    }
                    "select" -> assertEquals(rowTarget, view().getString("selected"))
                    "pin" -> assertEquals(!pinned, custom in ids("switcher"))
                }
                assertEquals(name, view().array("rows").objects().first { it.getString("id") == custom }.getString("title"))
                waitFor("$theme/$action inline editor retired") { view().isNull("prompt") && node("workspace-name-$custom")?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } == null }
                key(KeyEvent.KEYCODE_ESCAPE); idle(); assertNull("$theme/$action closes manager after commit; textOwner=${host.editingText}", node("workspace-manager"))
            }
        }
    }

    @Test fun nameGesturesSelectBuiltinsAndRenameCustomAcrossThemes() {
        val custom = newWorkspace("Name gestures")
        val builtin = defaults.first()
        fun doubleClickName(id: String) {
            down("workspace-name-$id"); event(MotionEvent.ACTION_UP)
            down("workspace-name-$id"); event(MotionEvent.ACTION_UP); idle()
        }
        for (theme in listOf("light", "dark")) {
            waitFor("document accepts gesture theme") { !host.documentInputBlocked }
            host.drain(obj("type" to "set_theme", "theme" to theme)); idle()
            waitFor("requested name gesture theme") { host.snapshot!!.getJSONObject("state").getString("theme") == theme }
            open(); tap("workspace-row-${defaults.last()}")
            waitFor("another row selected before builtin double click") { view().getString("selected") == defaults.last() }
            tool = MotionEvent.TOOL_TYPE_MOUSE
            doubleClickName(builtin)
            waitFor("built-in double click selects without Rename", 5000) { view().getString("selected") == builtin }
            assertTrue(view().isNull("prompt")); assertNull(node("workspace-row-menu"))
            tap("workspace-row-${defaults.last()}")
            waitFor("another row selected before mouse hold") { view().getString("selected") == defaults.last() }
            down("workspace-name-$builtin")
            SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() + 160)
            assertNull("Stationary mouse hold never opens metadata", node("workspace-row-menu"))
            event(MotionEvent.ACTION_UP); idle()
            waitFor("mouse hold release selects builtin") { view().getString("selected") == builtin }
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                down("workspace-name-$custom")
                waitFor("touch or pen name hold opens metadata") { node("workspace-row-menu") != null }
                event(MotionEvent.ACTION_UP)
                waitFor("held metadata owns native focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
                assertNotNull(node("workspace-rename")); assertNotNull(node("workspace-delete"))
                assertEquals(builtin, view().getString("selected"))
                key(KeyEvent.KEYCODE_ESCAPE); idle()
            }
            tool = MotionEvent.TOOL_TYPE_MOUSE
            doubleClickName(custom)
            val name = "Name gesture $theme"
            renameInline(custom, name)
            waitFor("custom name editor retired") { view().isNull("prompt") && node("workspace-name-$custom")?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } == null }
            assertEquals(builtin, view().getString("selected"))
            assertEquals(custom, view().getString("id"))
            shot("name-gestures-$theme")
            key(KeyEvent.KEYCODE_ESCAPE); idle(); assertNull(node("workspace-manager"))
        }
    }

    @Test fun pendingRowsRetireOnEscapeAndBlur() {
        open()
        val before = order(); val a = before.first(); val c = before[2]
        val durable = capture()
        for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            for (phase in listOf("pending", "held", "dragging")) {
                down("workspace-row-$a")
                if (phase != "pending") SystemClock.sleep(700)
                if (phase == "dragging") event(MotionEvent.ACTION_MOVE, bounds("workspace-row-$c").center)
                key(KeyEvent.KEYCODE_ESCAPE); SystemClock.sleep(700)
                event(MotionEvent.ACTION_UP); idle()
                instrumentation.runOnMainSync { assertNull(node("workspace-row-menu")); assertNull(node("workspace-row-drop-hint")); assertNotNull(node("workspace-manager")) }
                assertEquals(before, order())
            }
            down("workspace-row-$a"); SystemClock.sleep(700)
            event(MotionEvent.ACTION_MOVE, bounds("workspace-row-$c").center)
            lateinit var blocker: android.app.Dialog
            instrumentation.runOnMainSync {
                blocker = android.app.Dialog(pressed!!.view.context).apply {
                    setContentView(android.widget.TextView(context).apply { text = "Focus cancellation check" }); show()
                }
            }
            waitFor("dialog steals focus") { blocker.window?.decorView?.hasWindowFocus() == true }
            event(MotionEvent.ACTION_CANCEL)
            instrumentation.runOnMainSync { blocker.dismiss() }
            waitFor("manager regains focus") { node("workspace-manager")?.first?.view?.hasWindowFocus() == true }
            idle(); assertEquals(before, order())
        }
        assertEquals(durable, capture())
    }

    @Test fun backgroundPreferenceRefreshKeepsWorkspaceButtonsActive() {
        val target = defaults.first()
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "refresh_switcher")) }
        waitFor("background refresh pending") { view().optBoolean("switcher_busy") }
        instrumentation.runOnMainSync {
            assertNull("Preference refresh leaves switching available", node("workspace-switch-$target")!!.second.config.getOrNull(SemanticsProperties.Disabled))
        }
        tap("workspace-switch-$target")
        assertEquals(target, view().getString("id"))
    }
}

internal object WorkspaceWriteLockProcess {
    @JvmStatic fun main(args: Array<String>) {
        java.io.RandomAccessFile(args[0], "rw").use { file ->
            file.channel.lock(120L, 1L, false).use {
                System.out.println("LOCKED"); System.out.flush()
                Thread.sleep(3000)
            }
        }
    }
}
