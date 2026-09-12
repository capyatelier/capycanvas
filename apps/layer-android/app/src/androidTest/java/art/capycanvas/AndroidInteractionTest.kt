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
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Native finger/pen MotionEvents on the tablet, including popup focus and CANCEL.
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
    // View dispatch keeps exact geometry deterministic. Opt into the OS input
    // dispatcher with -e systemInput true where system injection is available.
    private var systemInput = InstrumentationRegistry.getArguments().getString("systemInput") == "true"
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val recovery get() = File(instrumentation.targetContext.filesDir, "interaction-workspace-recovery.json")

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
        val source = when (tool) {
            MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE
            MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS
            else -> InputDevice.SOURCE_TOUCHSCREEN
        }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) MotionEvent.BUTTON_PRIMARY else 0
        val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
            0, buttons, 1f, 1f, 0, 0, source, 0)
        try {
            if (systemInput) {
                val accepted = instrumentation.uiAutomation.injectInputEvent(motion, true)
                if (!accepted && action == MotionEvent.ACTION_DOWN) contact = false
                assertTrue("System accepts ${MotionEvent.actionToString(action)} at $next (screen ${coords[0].x}, ${coords[0].y}; origin ${location.toList()}; rotation ${owner.view.display.rotation})", accepted)
            }
            else scenario.onActivity { motion.offsetLocation(-location[0].toFloat(), -location[1].toFloat()); owner.view.dispatchTouchEvent(motion) }
        } finally { motion.recycle() }
        if (systemInput && action == MotionEvent.ACTION_MOVE) {
            // Android resamples a lone high-velocity event beyond its supplied
            // endpoint. A stationary sample represents the deliberate pause
            // used to inspect a halfway/resize boundary on the real tablet.
            SystemClock.sleep(24)
            val stopped = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
                0, buttons, 1f, 1f, 0, 0, source, 0)
            try {
                assertTrue(instrumentation.uiAutomation.injectInputEvent(stopped, true))
            } finally { stopped.recycle() }
        }
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) contact = false
    }
    private fun tap(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(40); event(MotionEvent.ACTION_UP) }
    private fun doubleTap(at: Offset) { tap(at); SystemClock.sleep(80); tap(at); settle() }
    private fun back() { instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); settle() }
    private fun popupCount(): Int {
        fun count() = android.view.inspector.WindowInspector.getGlobalWindowViews().count { view ->
                findView<ViewRootForTest>(view)?.let { find(it.semanticsOwner.unmergedRootSemanticsNode, "workspace-menu") != null } == true
            }
        if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) return count()
        var result = 0
        instrumentation.runOnMainSync { result = count() }
        return result
    }
    @Before fun ready() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {
            host = it.host; owner = findView<ViewRootForTest>(it.window.decorView)!!
            surface = findView<CanvasSurfaceView>(it.window.decorView)!!
            density = it.resources.displayMetrics.density
        }
        waitFor("brush ready", 60_000) { snapshot().optBoolean("brush_ready") }
        scenario.onActivity {
            saved = if (recovery.exists()) JSONObject(recovery.readText())
                else JSONObject(workspace()).also { recovery.writeText(it.toString()) }
        }
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
        try { if (contact) event(MotionEvent.ACTION_CANCEL) }
        finally {
            try { if (::saved.isInitialized) {
                action(obj("type" to "restore_workspace", "workspace" to saved))
                recovery.delete()
            } }
            finally { if (::scenario.isInitialized) scenario.close() }
        }
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
                scenario.onActivity { assertTrue("Held menu preserves the original window contact", owner.view.hasWindowFocus()) }
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
                waitFor("$pointer/$kind drag closes menu") { popupCount() == 0 }
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
            waitFor("tab slide preview") { host.workspaceGeometry?.tab != null }
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

    @Test fun columnResizeRetainsControlsAndReflowsAtNativeSize() {
        waitFor("resources ready", 60_000) { snapshot().optBoolean("shaders_ready") && !state().getJSONObject("filter_load").optBoolean("pending") }
        for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER)) {
            tool = pointer
            for (panel in listOf("sizes", "toolbar", "navigator")) {
                val configured = JSONObject(fixture.toString())
                val bands = configured.getJSONObject("layout").getJSONArray("bands")
                bands.getJSONObject(0).apply {
                    put("extent", 252)
                    put("root", obj("kind" to "tabs", "id" to 41, "panels" to JSONArray(listOf(panel)), "active" to panel, "tab_style" to "icon"))
                }
                // Keep each panel in one place, including the standalone toolbar.
                bands.getJSONObject(1).put("root", obj("kind" to "tabs", "id" to 43,
                    "panels" to JSONArray(listOf("layers")), "active" to "layers", "tab_style" to "icon"))
                bands.remove(2)
                action(obj("type" to "restore_workspace", "workspace" to configured))
                assertNull(host.actionError)
                val before = workspace()
                val press = bounds("divider-40").center
                val origin = bounds("workspace").left
                val first = Offset(origin + (if (panel == "navigator") 220 else 120) * density, press.y)
                val wide = Offset(origin + 420 * density, press.y)
                event(MotionEvent.ACTION_DOWN, press)
                event(MotionEvent.ACTION_MOVE, first); settle()
                val retained = host.panelContent
                val positions = mutableListOf<Rect>()
                for ((index, position) in listOf(first, wide).withIndex()) {
                    event(MotionEvent.ACTION_MOVE, position); settle()
                    assertNull(host.actionError)
                    assertTrue("Resize retains $panel controls", retained === host.panelContent)
                    val allocation = group(panel).getJSONObject("bounds")
                    val shown = bounds("group-41")
                    assertEquals(allocation.number("width") * density, shown.width, 1.1f)
                    assertEquals(allocation.number("height") * density, shown.height, 1.1f)
                    positions.add(shown)
                    if (panel == "sizes") {
                        val presets = host.catalog.array("brush_sizes").values()
                        val firstPreset = bounds("size-preset-${(presets[0] as Number).toInt()}")
                        val thirdPreset = bounds("size-preset-${(presets[2] as Number).toInt()}")
                        if (index == 0) assertTrue("Narrow presets wrap live", thirdPreset.top > firstPreset.top)
                        else assertEquals("Wide presets share a row", firstPreset.top, thirdPreset.top, 1.1f)
                    }
                    if (panel == "toolbar") {
                        val tiles = host.panelContent!!.array("panels").objects().first { it.getString("id") == panel }.array("tiles").objects()
                        group(panel).getJSONObject("tiles").array("tiles").objects().forEachIndexed { tileIndex, rect ->
                            if (tiles[tileIndex].getJSONObject("control").getString("kind") == "divider") return@forEachIndexed
                            val tile = bounds("tile-toolbar-${tiles[tileIndex].getInt("id")}")
                            assertEquals("Tile x follows shared reflow", shown.left + rect.number("x") * density, tile.left, 1.1f)
                            assertEquals("Tile y follows shared reflow", shown.top + rect.number("y") * density, tile.top, 1.1f)
                        }
                    }
                    if (panel == "navigator") {
                        val overview = bounds("navigator-overview")
                        assertTrue(shown.contains(overview.center))
                        assertEquals(shown.width - 16 * density, overview.width, 1.1f)
                    }
                    if (pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/resize-$panel-$index.png")
                        file.parentFile!!.mkdirs()
                        instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
                            file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                            bitmap.recycle()
                        }
                    }
                }
                assertTrue(positions[1].width > positions[0].width + 100 * density)
                event(MotionEvent.ACTION_CANCEL); settle()
                assertEquals("Cancel restores the workspace", before, workspace())
                event(MotionEvent.ACTION_DOWN, press)
                event(MotionEvent.ACTION_MOVE, wide); event(MotionEvent.ACTION_UP); settle()
                val committed = workspace()
                assertNotEquals(before, committed)
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(committed, workspace())
            }
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

    @Test fun emptyHeadersCollapseButTabsDoNot() {
        fixture.getJSONObject("layout").array("bands").objects().forEach { it.getJSONObject("root").put("tab_style", "icon") }
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) for (id in listOf(41, 43)) {
            tool = pointer; restore()
            val active = if (id == 41) "brushes" else "navigator"
            doubleTap(bounds("tab-$active").center)
            assertFalse("A double tap on a tab keeps its column open", exists("collapsed-column-$id"))
            val last = bounds("tab-${if (id == 41) "tool_settings" else "properties"}")
            val grip = bounds("group-grip-$id")
            val empty = Offset((last.right + grip.left) / 2, grip.center.y)
            assertTrue("Fixture has empty header space", empty.x > last.right + 10 * density)
            doubleTap(empty)
            waitFor("Empty header collapses $id") { exists("collapsed-column-$id") }
            action(obj("type" to "invoke", "command" to "undo_workspace"))
            assertFalse(exists("collapsed-column-$id"))
            // CANCEL and a long hold must not count as the first half of a double tap.
            event(MotionEvent.ACTION_DOWN, empty); event(MotionEvent.ACTION_CANCEL); tap(empty); settle()
            assertFalse(exists("collapsed-column-$id"))
        }
    }

    @Test fun drawerTabsKeepActiveColorsAndPadding() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        try { for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); restore()
            val docked = bounds("tab-brushes")
            val name = bounds("tab-name-brushes")
            val icon = bounds("tab-icon-brushes")
            fun pixels(tag: String): Pair<Int, Int> {
                val rect = bounds(tag)
                val image = instrumentation.uiAutomation.takeScreenshot()
                val location = IntArray(2)
                scenario.onActivity { owner.view.getLocationOnScreen(location) }
                fun sample(x: Float, y: Float) = image.getPixel((x + location[0]).toInt(), (y + location[1]).toInt())
                val background = sample(rect.center.x, rect.top + 3 * density)
                val text = bounds("tab-name-brushes")
                val counts = mutableMapOf<Int, Int>()
                for (y in text.top.toInt() until text.bottom.toInt()) for (x in text.left.toInt() until text.right.toInt()) {
                    val color = sample(x.toFloat(), y.toFloat())
                    if (color != background) counts[color] = (counts[color] ?: 0) + 1
                }
                image.recycle()
                return background to counts.maxBy { it.value }.key
            }
            val colors = pixels("tab-brushes")
            customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
            tap(bounds("column-icon-brushes").center); waitFor("Drawer tabs") { exists("drawer-tab-brushes") }; settle()
            val drawer = bounds("drawer-tab-brushes")
            assertEquals("Drawer uses the same tab width", docked.width, drawer.width, 1f)
            assertEquals("Drawer uses the same tab height", docked.height, drawer.height, 1f)
            assertEquals("Drawer name padding", name.left - docked.left, bounds("tab-name-brushes").left - drawer.left, 1f)
            assertEquals("Drawer icon padding", icon.left - docked.left, bounds("tab-icon-brushes").left - drawer.left, 1f)
            assertEquals("Drawer active background and text match the docked tab in $theme", colors, pixels("drawer-tab-brushes"))
        } } finally { action(obj("type" to "set_theme", "theme" to originalTheme)) }
    }

    @Test fun detachedPanelsKeepBodiesAndWiderResizeTargets() {
        systemInput = false
        val root = fixture.getJSONObject("layout").array("bands").getJSONObject(1).getJSONObject("root")
        root.put("panels", JSONArray(listOf("navigator", "layers", "properties", "adjustments"))).put("tab_style", "icon")
        for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS))
            for (panel in listOf("properties", "adjustments", "layers")) for (wholeGroup in listOf(false, true)) {
                tool = pointer; restore()
                if (wholeGroup) action(obj("type" to "select_panel_tab", "group" to 43, "panel" to panel))
                val before = workspace()
                event(MotionEvent.ACTION_DOWN, bounds(if (wholeGroup) "group-grip-43" else "tab-$panel").center)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center)
                waitFor("$panel detaches") { group(panel).optBoolean("floating") }
                settle()
                assertTrue("$panel floating body is visible during contact", bounds("panel-body-$panel").height > 60 * density)
                val content = when (panel) { "adjustments" -> "filter-list"; "properties" -> "layer-properties"; else -> "layer-rows" }
                assertTrue("$panel controls are painted below the header", bounds(content).height > 20 * density)
                event(MotionEvent.ACTION_MOVE, point + Offset(24 * density, 30 * density)); settle()
                assertTrue("$panel body remains visible while moving", bounds(content).height > 20 * density)
                event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(before, workspace())
                event(MotionEvent.ACTION_DOWN, bounds(if (wholeGroup) "group-grip-43" else "tab-$panel").center)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle(); event(MotionEvent.ACTION_UP); settle()
                assertTrue(group(panel).optBoolean("floating"))
                val floating = workspace()
                val floatingId = group(panel).getInt("id")
                val edge = bounds("resize-$floatingId-right")
                assertTrue("Floating side target is at least 12dp", edge.width >= 12 * density - 1)
                val press = Offset(edge.right - 2 * density, edge.center.y)
                event(MotionEvent.ACTION_DOWN, press)
                event(MotionEvent.ACTION_MOVE, press + Offset(40 * density, 0f)); settle()
                assertNotEquals("Outer part of the widened handle resizes", floating, workspace())
                event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(floating, workspace())
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
            }
        restore()
        val divider = bounds("divider-40")
        assertTrue("Column resize target is at least 16dp", divider.width >= 16 * density - 1)
        val before = workspace()
        val press = Offset(divider.right - density, divider.center.y)
        event(MotionEvent.ACTION_DOWN, press); event(MotionEvent.ACTION_MOVE, press + Offset(45 * density, 0f)); settle()
        assertNotEquals("Wider column target resizes", before, workspace())
        event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(before, workspace())
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
            for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) for (handle in listOf(true, false)) {
                tool = pointer
                val sourceBounds = bounds("layer-row-$source")
                val press = Offset(if (handle) sourceBounds.right - 12 * density else sourceBounds.center.x, sourceBounds.center.y)
                val destination = bounds("layer-row-$target").let { Offset(it.right - 12 * density, it.bottom - 3 * density) }
                event(MotionEvent.ACTION_DOWN, press); SystemClock.sleep(700)
                assertEquals("Layer context opens while held", 1, popupCount())
                scenario.onActivity { assertTrue("Layer menu preserves window focus during contact", owner.view.hasWindowFocus()) }
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
