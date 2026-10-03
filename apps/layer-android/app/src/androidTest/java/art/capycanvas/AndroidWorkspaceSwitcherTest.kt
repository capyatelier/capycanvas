package art.capycanvas

import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ViewConfiguration
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.state.ToggleableState
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
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) downAt = SystemClock.uptimeMillis()
        val event = motion(tool, action, next, downAt, button)
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
        android.util.Log.i("SwitcherAcceptance", "Tap $tag")
        down(tag); event(MotionEvent.ACTION_UP); idle()
    }
    private fun key(code: Int, meta: Int = 0) { pressKey(code, meta); SystemClock.sleep(220) }
    private fun open() { send(obj("type" to "open", "page" to "workspaces")); waitFor("dialog focus") { node("workspace-manager")?.first?.view?.hasWindowFocus() == true } }
    private fun options(id: String, action: String) { tap("workspace-options-$id"); tap("workspace-$action") }
    private fun newWorkspace(name: String): String {
        send(obj("type" to "form", "action" to obj("type" to "new"))); send(obj("type" to "submit", "name" to name))
        return view().getString("id")
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

    @Test fun pinsOrderingPreviewKeyboardAndRestart() {
        val custom = newWorkspace("Tablet Switcher")
        assertTrue("New workspaces start pinned", custom in ids("switcher"))
        val before = capture()
        val current = view().getString("id")
        open(); tap("workspace-row-${defaults.last()}")
        shot("manager")
        val preview = layout()
        options(current, "pin")
        assertFalse(current in ids("switcher")); assertEquals(current, ids("switcher_display").first())
        assertEquals(preview, layout()); assertEquals(before, capture())
        val previous = order().indexOf(current)
        tap("workspace-options-$current")
        shot("options")
        waitFor("menu keyboard focus") { node("workspace-row-menu")?.first?.view?.hasWindowFocus() == true }
        key(KeyEvent.KEYCODE_TAB) // Leave native touch mode before requesting focus.
        instrumentation.runOnMainSync { assertTrue(node("workspace-up")!!.second.config[SemanticsActions.RequestFocus].action!!.invoke()) }
        key(KeyEvent.KEYCODE_ENTER); idle()
        assertEquals(previous - 1, order().indexOf(current))
        assertFalse(current in ids("switcher")); assertEquals(preview, layout()); assertEquals(before, capture())
        for (id in ids("switcher").toList()) options(id, "pin")
        assertTrue(ids("switcher").isEmpty()); assertEquals(listOf(current), ids("switcher_display"))
        tap("workspace-cancel"); assertEquals(before, capture())
        shot("unpinned-current")
        tap("workspace-switch-$current"); assertEquals(current, view().getString("id"))
        open(); tap("workspace-row-${defaults.first()}"); tap("workspace-confirm")
        assertEquals(listOf(defaults.first()), ids("switcher_display"))
        open(); options(custom, "pin"); tap("workspace-cancel")
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
            assertEquals(listOf(options.getString("title")) + options.array("sections").values().flatMap {
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
            for (pinned in listOf(false, true)) {
                tap("workspace-switcher-options")
                waitFor("checkbox keyboard focus window") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
                instrumentation.runOnMainSync {
                    val item = checkNotNull(label(checkNotNull(node("workspace-menu")).second, keyboardRow))
                    val row = generateSequence(item) { it.parent }.first { it.config.getOrNull(SemanticsActions.RequestFocus) != null }
                    assertTrue(row.config[SemanticsActions.RequestFocus].action!!.invoke())
                }
                key(KeyEvent.KEYCODE_ENTER); idle()
                waitFor("keyboard checkbox saves $pinned") { (inactive in ids("switcher")) == pinned }
                assertEquals(current, view().getString("id")); assertEquals(savedLayout, layout()); assertEquals(savedOrder, order())
            }
            for (id in ids("switcher").toList()) {
                tap("workspace-switcher-options")
                tapMenu(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == id }.getString("label"))
                waitFor("$id pin cleared") { id !in ids("switcher") }
                assertEquals(current, view().getString("id")); assertEquals(savedOrder, order()); assertEquals(savedLayout, layout())
            }
            assertEquals(emptyList<String>(), ids("switcher")); assertEquals(listOf(current), ids("switcher_display"))
            tap("workspace-switcher-options"); assertOptions(current, savedLayout, savedOrder)
            assertFalse(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == current }.optBoolean("selected"))
            closeOptions()
            for (id in savedOrder) {
                tap("workspace-switcher-options")
                tapMenu(menuRows().first { it.getJSONObject("action").getJSONObject("command").getString("id") == id }.getString("label"))
            }
            assertEquals(savedOrder, ids("switcher")); assertEquals(durable, capture())
            tap("workspace-switcher-options")
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
            var popup: android.view.View? = null
            val database = android.database.sqlite.SQLiteDatabase.openDatabase(java.io.File(device.root, "workspace/workspaces.sqlite3").path,
                null, android.database.sqlite.SQLiteDatabase.OPEN_READWRITE)
            try {
                database.execSQL("BEGIN IMMEDIATE")
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "edit_switcher", "edit" to obj("type" to "show", "id" to inactive, "visible" to false))) }
                waitFor("visibility write waits for database") { view().optBoolean("switcher_busy") }
                if (!compact) { down("workspace-switcher-options"); event(MotionEvent.ACTION_UP) }
                waitFor("retained checkbox waits for write") { menuCheckbox(title)?.config?.getOrNull(SemanticsProperties.Disabled) != null }
                instrumentation.runOnMainSync { popup = node("workspace-menu")!!.first.view }
            } finally { database.execSQL("COMMIT"); database.close() }
            waitFor("retained options popup") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
            waitFor("retained checkbox becomes available with confirmed check") {
                !view().optBoolean("switcher_busy") && menuCheckbox(title)?.let {
                    it.config.getOrNull(SemanticsProperties.Disabled) == null && it.config.getOrNull(SemanticsProperties.ToggleableState) == ToggleableState.Off
                } == true
            }
            instrumentation.runOnMainSync { assertSame("Refresh retains the native popup", popup, node("workspace-menu")!!.first.view) }
            assertTrue("Options register shared popup ownership", sharedPopupOpen())
            tapMenu(title); waitFor("refreshed action restores the pin") { inactive in ids("switcher") }
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
            if (compact) {
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
                tap(tag); tapMenu(menu.array("sections").getJSONArray(1).getJSONObject(0).getString("label")); tapMenu(menuRows().first().getString("label"))
            }
            for (editing in listOf(false, true)) {
                if (editing) {
                    host.drain(obj("type" to "invoke", "command" to "customize_workspace_ui"))
                    waitFor("title bar editor") { host.snapshot!!.getJSONObject("header").optBoolean("editing") }
                }
                val zones = host.snapshot!!.getJSONObject("header").getJSONObject("model").array("zones").values().map { it as JSONArray }
                val zone = zones.indexOfFirst { entries -> entries.objects().any { it.getInt("id") == 900 && it.getJSONObject("item").getString("kind") == "workspaces" } }
                assertEquals(if (compact) 1 else 0, zone)
                fun presentedSource(): String {
                    var source = ""
                    waitFor("workspace item resolves its editor geometry") {
                        source = listOf(tag, "header-control-900", "header-overflow-item-900", "header-overflow-$zone").firstOrNull { node(it) != null } ?: ""
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
