package art.capycanvas

import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real window-dispatched finger/pen contacts, including popup focus and CANCEL.
 * Keep the real frame clock: a held contact must survive opening a native popup. */
class AndroidInteractionTest {
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var owner: ViewRootForTest
    private lateinit var surface: CanvasSurfaceView
    private lateinit var saved: JSONObject
    private lateinit var fixture: JSONObject
    private var density = 1f
    private var downAt = 0L
    private var contact = false
    private var point = Offset.Zero
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var systemInput = true
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()

    private inline fun <reified T> findView(view: View): T? {
        val pending = ArrayDeque<View>().apply { add(view) }
        while (pending.isNotEmpty()) {
            val next = pending.removeFirst()
            if (next is T) return next
            if (next is ViewGroup) for (i in 0 until next.childCount) pending.add(next.getChildAt(i))
        }
        return null
    }
    private fun find(node: SemanticsNode, tag: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node
        else node.children.firstNotNullOfOrNull { find(it, tag) }
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        scenario.onActivity { result = find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)?.boundsInRoot }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun exists(tag: String) = find(owner.semanticsOwner.unmergedRootSemanticsNode, tag) != null
    private fun snapshot() = host.snapshot!!
    private fun state() = snapshot().getJSONObject("state")
    private fun workspace() = state().getJSONObject("workspace").toString()
    private fun group(panel: String) = snapshot().getJSONObject("layout").array("groups").objects()
        .first { panel in it.array("panels").values() }
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + timeout
        do {
            var ready = false
            scenario.onActivity { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
            if (ready) return
            SystemClock.sleep(16)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("Timed out: $label")
    }
    private fun settle() { SystemClock.sleep(180); instrumentation.waitForIdleSync() }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        scenario.onActivity { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        settle()
    }
    private fun customize(value: JSONObject) = action(obj("type" to "customize", "action" to value))
    private fun restore() = action(obj("type" to "restore_workspace", "workspace" to fixture))
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); contact = true }
        val location = IntArray(2)
        scenario.onActivity { owner.view.getLocationOnScreen(location) }
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            x = next.x + location[0]; y = next.y + location[1]; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f
        })
        val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
            0, 0, 1f, 1f, 0, 0, if (tool == MotionEvent.TOOL_TYPE_STYLUS) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
        try {
            if (systemInput) assertTrue(instrumentation.uiAutomation.injectInputEvent(motion, true))
            else scenario.onActivity { motion.offsetLocation(-location[0].toFloat(), -location[1].toFloat()); owner.view.dispatchTouchEvent(motion) }
        } finally { motion.recycle() }
        if (systemInput && action == MotionEvent.ACTION_MOVE) {
            // Android resamples a lone high-velocity event beyond its supplied
            // endpoint. A stationary sample represents the deliberate pause
            // used to inspect a halfway/resize boundary on the real tablet.
            SystemClock.sleep(24)
            val stopped = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
                0, 0, 1f, 1f, 0, 0, if (tool == MotionEvent.TOOL_TYPE_STYLUS) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(stopped, true)) } finally { stopped.recycle() }
        }
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) contact = false
    }
    private fun tap(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(40); event(MotionEvent.ACTION_UP) }
    private fun doubleTap(at: Offset) { tap(at); SystemClock.sleep(80); tap(at); settle() }
    private fun back() { instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); settle() }
    private fun popupCount(): Int {
        var count = 0
        instrumentation.runOnMainSync {
            count = android.view.inspector.WindowInspector.getGlobalWindowViews().count { view ->
                findView<ViewRootForTest>(view)?.let { find(it.semanticsOwner.unmergedRootSemanticsNode, "workspace-menu") != null } == true
            }
        }
        return count
    }
    @Before fun ready() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {
            host = it.host; owner = findView<ViewRootForTest>(it.window.decorView)!!
            surface = findView<CanvasSurfaceView>(it.window.decorView)!!
            density = it.resources.displayMetrics.density
        }
        waitFor("brush ready", 60_000) { snapshot().optBoolean("brush_ready") }
        scenario.onActivity { saved = JSONObject(workspace()) }
        val defaults = Native.create(false)
        try { fixture = JSONObject(Native.snapshot(defaults)!!).getJSONObject("state").getJSONObject("workspace") }
        finally { Native.destroy(defaults) }
        fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
            "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon_name")
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 390, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 330, "root" to tabs(43, "navigator", "layers", "properties")),
                obj("id" to 44, "edge" to "top", "extent" to 42, "root" to tabs(45, "toolbar")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray())
            put("next_id", maxOf(46, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        restore()
    }
    @After fun cleanup() {
        if (contact) event(MotionEvent.ACTION_CANCEL)
        if (::saved.isInitialized) action(obj("type" to "restore_workspace", "workspace" to saved))
        if (::scenario.isInitialized) scenario.close()
    }

    @Test fun longPressRetainsEveryWorkspaceDragSource() {
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            for (kind in listOf("tab", "group", "floating", "drawer-tab", "drawer-grip", "drawer-tile", "tile", "ribbon", "column")) {
                restore()
                val drawerToolbar = kind == "drawer-tile"
                if (drawerToolbar) action(obj("type" to "move_panel", "panel" to "toolbar",
                    "target" to obj("kind" to "tab", "group" to 41, "index" to 3),
                    "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
                if (kind == "floating") action(obj("type" to "move_group", "group" to 41,
                    "target" to obj("kind" to "float", "position" to JSONArray(listOf(460, 250))),
                    "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
                if (kind.startsWith("drawer") || kind == "column") {
                    customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                    if (kind.startsWith("drawer")) { tap(bounds(if (drawerToolbar) "column-icon-toolbar" else "column-icon-brushes").center); waitFor("drawer") { exists("column-drawer-41") }; settle() }
                }
                val tag = when (kind) {
                    "tab" -> "tab-sizes"
                    "group", "floating" -> "group-grip-41"
                    "drawer-tab" -> "drawer-tab-sizes"
                    "drawer-grip" -> "column-drawer-grip-41"
                    "tile", "drawer-tile" -> "tile-toolbar-${host.panelContent!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects().first().getInt("id") }"
                    "ribbon" -> "ribbon-grip-toolbar"
                    else -> "column-grip-41"
                }
                val before = workspace()
                val press = bounds(tag).center
                event(MotionEvent.ACTION_DOWN, press)
                SystemClock.sleep(700)
                assertEquals("$pointer/$kind opens its menu during contact", 1, popupCount())
                event(MotionEvent.ACTION_UP)
                settle()
                assertEquals("$pointer/$kind release retains menu", 1, popupCount())
                back()
                assertEquals(0, popupCount())
                event(MotionEvent.ACTION_DOWN, press)
                SystemClock.sleep(700)
                assertEquals("$pointer/$kind second hold", 1, popupCount())
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center)
                waitFor("$pointer/$kind continues original drag") { surface.pointerIcon == PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRABBING) }
                assertEquals("$pointer/$kind drag closes menu", 0, popupCount())
                event(MotionEvent.ACTION_CANCEL)
                waitFor("$pointer/$kind cancel") { workspace() == before && surface.pointerIcon != PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRABBING) }
            }
        }
    }

    @Test fun frozenVariableWidthTabsClampReverseDetachAndUndo() {
        systemInput = false // Inspect exact halfway points without OS resampling.
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer; restore()
            val before = workspace()
            val source = bounds("tab-sizes")
            val next = bounds("tab-tool_settings")
            val press = Offset(source.left + 7 * density, source.center.y)
            val halfway = next.width / 2
            event(MotionEvent.ACTION_DOWN, press)
            event(MotionEvent.ACTION_MOVE, press + Offset(halfway - density, 0f)); settle()
            assertEquals(0f, host.workspaceGeometry!!.tab!!.offsets[2]!!, .01f)
            event(MotionEvent.ACTION_MOVE, press + Offset(halfway + density, 0f)); settle()
            assertEquals(-source.width / density, host.workspaceGeometry!!.tab!!.offsets[2]!!, 1f)
            event(MotionEvent.ACTION_MOVE, press + Offset(halfway - density, 0f)); settle()
            assertEquals("Reversal uses frozen slots", 0f, host.workspaceGeometry!!.tab!!.offsets[2]!!, .01f)
            event(MotionEvent.ACTION_MOVE, Offset(bounds("group-grip-41").right + 20 * density, press.y)); settle()
            val preview = host.workspaceGeometry!!.tab!!
            assertEquals("Clamped to tab strip", bounds("group-grip-41").left / density,
                source.right / density + preview.sourceOffset, 1f)
            val away = bounds("workspace").center
            event(MotionEvent.ACTION_MOVE, away); settle()
            assertTrue(group("sizes").getBoolean("floating"))
            assertEquals("Detached panel preserves tab-relative grab", away.x / density - 7,
                host.workspaceGeometry!!.bounds!!.left, 1f)
            event(MotionEvent.ACTION_CANCEL)
            waitFor("cancel restores attached tabs") { workspace() == before }
            event(MotionEvent.ACTION_DOWN, press)
            event(MotionEvent.ACTION_MOVE, press + Offset(halfway + density, 0f)); settle()
            event(MotionEvent.ACTION_UP); settle()
            assertEquals(listOf("brushes", "tool_settings", "sizes"), group("sizes").array("panels").values())
            action(obj("type" to "invoke", "command" to "undo_workspace"))
            assertEquals(before, workspace())
            action(obj("type" to "invoke", "command" to "redo_workspace"))
            assertEquals(listOf("brushes", "tool_settings", "sizes"), group("sizes").array("panels").values())
        }
    }

    @Test fun columnResizeReversesAndNavigatorFollowsCollapse() {
        // Exact threshold checks bypass the system's touch resampling, while
        // exercising the same native Compose MotionEvent dispatch and JNI path.
        systemInput = false
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) for (right in listOf(false, true)) {
            tool = pointer; restore()
            val id = if (right) 43 else 41
            val band = if (right) 42 else 40
            val direction = if (right) -1 else 1
            doubleTap(bounds("group-grip-$id").center)
            waitFor("header collapse") { exists("collapsed-column-$id") }
            if (right) assertFalse("Navigator hides with column", exists("navigator-overview"))
            val before = workspace()
            val press = bounds("divider-$band").center
            fun move(distance: Float) { event(MotionEvent.ACTION_MOVE, press + Offset(distance * direction * density, 0f)); settle() }
            event(MotionEvent.ACTION_DOWN, press)
            move(35f); assertEquals(before, workspace())
            move(37f); assertFalse(exists("collapsed-column-$id"))
            if (right) assertTrue("Navigator returns with expansion", exists("navigator-overview"))
            val expanded = workspace()
            val expandedEdge = bounds("divider-$band").center
            val edge = (expandedEdge.x - press.x) * direction / density
            assertTrue(edge > 37)
            move(edge - 1); assertEquals("Wait for expanded edge", expanded, workspace())
            move(35f); assertEquals("Reverse before reaching edge", before, workspace())
            move(37f); assertEquals(expanded, workspace())
            move(edge + 45); assertNotEquals(expanded, workspace())
            event(MotionEvent.ACTION_CANCEL)
            waitFor("cancel restores collapsed width") { workspace() == before }
            event(MotionEvent.ACTION_DOWN, press)
            move(37f); event(MotionEvent.ACTION_UP); settle()
            action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
            // Double tap empty strip background expands without toggling a drawer.
            val strip = bounds("collapsed-column-$id")
            doubleTap(Offset(strip.center.x, strip.bottom - 55 * density))
            waitFor("collapsed background expands") { !exists("collapsed-column-$id") }
        }
    }

    @Test fun canvasEdgeDoubleTapUsesRecursiveDefaultWidths() {
        fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
            "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon_name")
        fun split(id: Int, axis: String, first: JSONObject, second: JSONObject) = obj("kind" to "split", "id" to id,
            "axis" to axis, "fraction" to .4, "first" to first, "second" to second)
        val nested = JSONObject(fixture.toString())
        nested.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(obj("id" to 40, "edge" to "left", "extent" to 650,
                "root" to split(60, "vertical", tabs(61, "sizes", "layers"),
                    split(62, "horizontal", tabs(63, "brushes"),
                        split(64, "vertical", tabs(65, "navigator"), tabs(66, "tool_settings"))))))))
            put("next_id", maxOf(67, getInt("next_id")))
        }
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = pointer
            action(obj("type" to "restore_workspace", "workspace" to nested))
            val before = workspace()
            doubleTap(bounds("divider-40").center)
            waitFor("recursive default width") {
                state().getJSONObject("workspace").getJSONObject("layout").array("bands").getJSONObject(0).number("extent") == 508f
            }
            assertEquals(502f, group("sizes").getJSONObject("bounds").number("width"), .1f)
            assertEquals(242f, group("brushes").getJSONObject("bounds").number("width"), .1f)
            assertEquals(254f, group("navigator").getJSONObject("bounds").number("width"), .1f)
            action(obj("type" to "invoke", "command" to "undo_workspace"))
            assertEquals(before, workspace())
        }
    }

    @Test fun openDrawersAndCollapsedCanvasSidesAcceptTouchAndPenDrops() {
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS))
            for (right in listOf(false, true)) for (open in listOf(false, true)) {
                tool = pointer; restore()
                val column = if (right) 43 else 41
                val band = if (right) 42 else 40
                val source = if (right) "brushes" else "layers"
                val target = if (right) "navigator" else "brushes"
                customize(obj("type" to "set_column_collapsed", "group" to column, "collapsed" to true))
                if (open) { tap(bounds("column-icon-$target").center); waitFor("open target drawer") { exists("column-drawer-$column") }; settle() }
                val before = workspace()
                event(MotionEvent.ACTION_DOWN, bounds("tab-$source").center)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle()
                val destination = if (open) bounds("drawer-tab-$target").let { Offset(it.left + 5 * density, it.center.y) }
                else bounds("divider-$band").let { Offset(if (right) it.left - 70 * density else it.right + 70 * density, it.center.y) }
                event(MotionEvent.ACTION_MOVE, destination); settle()
                assertNotNull("$pointer/$right/$open drop hint", host.workspaceGeometry?.hint)
                event(MotionEvent.ACTION_UP); settle()
                if (open) {
                    // Collapsed groups are represented by their drawer projection.
                    val drawer = state().getJSONObject("customization").array("column_drawers").objects()
                        .first { it.getJSONObject("anchor").getInt("column") == column }
                    assertTrue(source in drawer.getJSONObject("tabs").array("panels").values())
                } else {
                    assertFalse("Drop docks the panel", group(source).getBoolean("floating"))
                    assertTrue("Original column stays collapsed", exists("collapsed-column-$column"))
                    assertTrue("New column beside collapsed strip", group(source).getJSONObject("bounds").number("height") > bounds("workspace").height / density / 2)
                }
                action(obj("type" to "invoke", "command" to "undo_workspace"))
                assertEquals(before, workspace())
            }
    }

    @Test fun layerHandleLongPressDragCancelAndUndo() {
        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
        val original = state().array("layers").objects().map { it.getLong("id") }
        fun layer(value: JSONObject) = action(obj("type" to "layer", "action" to value))
        var additions = 0
        var committedDrop = false
        try {
            repeat(2) { layer(obj("op" to "new", "group" to false, "clipped" to false)); additions++ }
            val before = state().array("layers").objects().map { it.getLong("id") }
            val source = before[0]; val target = before[1]
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = pointer
                val sourceBounds = bounds("layer-row-$source")
                val press = Offset(sourceBounds.right - 12 * density, sourceBounds.center.y)
                val destination = bounds("layer-row-$target").let { Offset(it.right - 12 * density, it.bottom - 3 * density) }
                event(MotionEvent.ACTION_DOWN, press); SystemClock.sleep(700)
                assertEquals("Layer context opens while held", 1, popupCount())
                event(MotionEvent.ACTION_UP); settle(); assertEquals(1, popupCount()); back()
                event(MotionEvent.ACTION_DOWN, press); SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE, destination); settle(); assertEquals(0, popupCount())
                event(MotionEvent.ACTION_CANCEL); settle()
                assertEquals("Canceled layer drop", before, state().array("layers").objects().map { it.getLong("id") })
                event(MotionEvent.ACTION_DOWN, press); SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE, destination); settle(); event(MotionEvent.ACTION_UP); settle()
                committedDrop = true
                assertEquals(listOf(target, source) + before.drop(2), state().array("layers").objects().map { it.getLong("id") })
                action(obj("type" to "invoke", "command" to "undo")); committedDrop = false
                assertEquals(before, state().array("layers").objects().map { it.getLong("id") })
            }
        } finally {
            if (contact) event(MotionEvent.ACTION_CANCEL)
            if (committedDrop) action(obj("type" to "invoke", "command" to "undo"))
            repeat(additions) { action(obj("type" to "invoke", "command" to "undo")) }
            assertEquals(original, state().array("layers").objects().map { it.getLong("id") })
        }
    }
}
