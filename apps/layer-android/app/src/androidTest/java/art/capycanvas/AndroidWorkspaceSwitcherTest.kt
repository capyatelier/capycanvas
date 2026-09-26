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
import androidx.test.core.app.ActivityScenario
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
    private fun order() = view().array("order").values().map { it.toString() }
    private fun layout() = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").toString()
    private fun node(tag: String) = findTag(tag)
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync { result = node(tag)?.second?.boundsInRoot }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun waitFor(label: String, timeout: Long = 15000, condition: () -> Boolean) = host.awaitMain(label, timeout, { "${view()}" }, condition)
    private fun rowsEnabled() = view().optString("page") != "workspaces" || !view().isNull("form") || view().array("rows").objects()
        .all { node("workspace-row-${it.getString("id")}")?.second?.config?.getOrNull(SemanticsProperties.Disabled) == null }
    private fun idle() {
        SystemClock.sleep(220)
        waitFor("workspace idle") { !view().optBoolean("busy") && !view().optBoolean("switcher_busy") && !view().optBoolean("dirty") && rowsEnabled() }
        assertTrue(view().toString(), view().isNull("error")); assertTrue(view().toString(), view().isNull("switcher_error"))
    }
    private fun send(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; idle() }
    private fun capture() = host.workspaceCapture()
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
        send(obj("type" to "form", "kind" to "new")); send(obj("type" to "submit", "name" to name, "source" to null))
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
        val defaults = ids("defaults")
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

    @Test fun pendingRowsRetireOnEscapeBlurAndFiltering() {
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
            down("workspace-row-$a")
            send(obj("type" to "filter", "query" to "no rows match this")); SystemClock.sleep(700)
            event(MotionEvent.ACTION_UP); idle()
            instrumentation.runOnMainSync { assertNull(node("workspace-row-menu")); assertNull(node("workspace-row-drop-hint")) }
            send(obj("type" to "filter", "query" to "")); assertEquals(before, order())
        }
        assertEquals(durable, capture())
    }

    @Test fun backgroundPreferenceRefreshKeepsWorkspaceButtonsActive() {
        val target = ids("defaults").first()
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "refresh_switcher")) }
        waitFor("background refresh pending") { view().optBoolean("switcher_busy") }
        instrumentation.runOnMainSync {
            assertNull("Preference refresh leaves switching available", node("workspace-switch-$target")!!.second.config.getOrNull(SemanticsProperties.Disabled))
        }
        tap("workspace-switch-$target")
        assertEquals(target, view().getString("id"))
    }
}
