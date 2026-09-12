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

/** Native mouse/finger/pen MotionEvents on the tablet, including popup focus and CANCEL.
 * Keep the real frame clock: a held contact must survive opening a native popup. */
class AndroidInteractionTest {
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
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
    private var mouseButton = MotionEvent.BUTTON_PRIMARY
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
        instrumentation.runOnMainSync { result = find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)?.boundsInRoot }
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
            instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
            if (ready) return
            SystemClock.sleep(16)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("Timed out: $label")
    }
    // A held contact at a scroll edge can keep native overscroll animation
    // alive. Drain main-thread work without waiting forever for global idleness.
    private fun settle() { SystemClock.sleep(180); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        settle()
    }
    private fun customize(value: JSONObject) = action(obj("type" to "customize", "action" to value))
    private fun restore() = action(obj("type" to "restore_workspace", "workspace" to fixture))
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); contact = true }
        val location = IntArray(2)
        instrumentation.runOnMainSync { owner.view.getLocationOnScreen(location) }
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            x = next.x + location[0]; y = next.y + location[1]; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f
        })
        val source = when (tool) {
            MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE
            MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS
            else -> InputDevice.SOURCE_TOUCHSCREEN
        }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) mouseButton else 0
        val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
            0, buttons, 1f, 1f, 0, 0, source, 0)
        try {
            if (systemInput) {
                val accepted = instrumentation.uiAutomation.injectInputEvent(motion, true)
                if (!accepted && action == MotionEvent.ACTION_DOWN) contact = false
                assertTrue("System accepts ${MotionEvent.actionToString(action)} at $next (screen ${coords[0].x}, ${coords[0].y}; origin ${location.toList()}; rotation ${owner.view.display.rotation})", accepted)
            }
            else instrumentation.runOnMainSync { motion.offsetLocation(-location[0].toFloat(), -location[1].toFloat()); owner.view.dispatchTouchEvent(motion) }
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
    private fun back() {
        // Composition can expose a menu before WindowManager transfers focus.
        // Sending Back in that gap can finish the Activity instead of the menu.
        waitFor("native menu receives keyboard focus") {
            android.view.inspector.WindowInspector.getGlobalWindowViews().any { view ->
                view.hasWindowFocus() && findView<ViewRootForTest>(view)?.let {
                    find(it.semanticsOwner.unmergedRootSemanticsNode,"workspace-menu")!=null
                }==true
            }
        }
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        waitFor("native menu closes") { popupCount()==0 && owner.view.hasWindowFocus() }; settle()
    }
    private fun popupCount(): Int {
        fun count() = android.view.inspector.WindowInspector.getGlobalWindowViews().count { view ->
                findView<ViewRootForTest>(view)?.let { find(it.semanticsOwner.unmergedRootSemanticsNode, "workspace-menu") != null } == true
            }
        if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) return count()
        var result = 0
        instrumentation.runOnMainSync { result = count() }
        return result
    }
    private fun workspaceDragging() = surface.pointerIcon == PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRABBING)
    private val holdMenuCount get()=if(tool==MotionEvent.TOOL_TYPE_MOUSE) 0 else 1
    private val pointerTools = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)
    private fun resetLayerScroll(first: Long) {
        instrumentation.runOnMainSync {
            find(owner.semanticsOwner.unmergedRootSemanticsNode,"layer-rows")!!.config[
                androidx.compose.ui.semantics.SemanticsActions.ScrollToIndex].action!!.invoke(0)
        }
        waitFor("list reset") { exists("layer-row-$first") }; settle()
    }
    @Before fun ready() {
        CanvasHost.workspaceDirectoryForTest = File(instrumentation.targetContext.filesDir, "interaction-workspace-tests/${java.util.UUID.randomUUID()}").absolutePath
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {
            activity=it
            host = it.host; owner = findView<ViewRootForTest>(it.window.decorView)!!
            surface = findView<CanvasSurfaceView>(it.window.decorView)!!
            density = it.resources.displayMetrics.density
        }
        waitFor("brush ready", 60_000) { snapshot().optBoolean("brush_ready") }
        waitFor("workspace ready", 60_000) { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
        instrumentation.runOnMainSync {
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
            finally { if (::scenario.isInitialized) scenario.close(); CanvasHost.workspaceDirectoryForTest = null }
        }
    }

    @Test fun longPressRetainsEveryWorkspaceDragSource() {
        for (pointer in pointerTools) {
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
                assertEquals("$pointer/$kind only touch/pen holds open menus", holdMenuCount, popupCount())
                instrumentation.runOnMainSync { assertTrue("Held menu preserves the original window contact", owner.view.hasWindowFocus()) }
                event(MotionEvent.ACTION_UP)
                settle()
                assertEquals("$pointer/$kind release retains touch/pen menu", holdMenuCount, popupCount())
                if(holdMenuCount>0)back()
                assertEquals(0, popupCount())
                event(MotionEvent.ACTION_DOWN, press)
                SystemClock.sleep(700)
                assertEquals("$pointer/$kind second hold", holdMenuCount, popupCount())
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center)
                waitFor("$pointer/$kind continues original drag") { surface.pointerIcon == PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRABBING) }
                waitFor("$pointer/$kind drag closes menu") { popupCount() == 0 }
                event(MotionEvent.ACTION_CANCEL)
                waitFor("$pointer/$kind cancel") { workspace() == before && surface.pointerIcon != PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRABBING) }
            }
        }
    }

    @Test fun tilePickupRequiresStationaryHoldAcrossPresentations() {
        for (pointer in pointerTools) for (kind in listOf("tile", "divider", "drawer-tile", "drawer-divider", "floating", "vertical", "wrapped", "column")) {
            tool=pointer; restore()
            val viewport=JSONArray(listOf(bounds("workspace").width/density,bounds("workspace").height/density))
            if (kind.startsWith("drawer") || kind=="wrapped") action(obj("type" to "move_panel", "panel" to "toolbar",
                "target" to obj("kind" to "tab", "group" to 41, "index" to 3), "viewport" to viewport))
            if (kind=="wrapped") action(obj("type" to "select_panel_tab", "group" to 41, "panel" to "toolbar"))
            if (kind=="floating") action(obj("type" to "move_group", "group" to 45,
                "target" to obj("kind" to "float", "position" to JSONArray(listOf(460,250))), "viewport" to viewport))
            if (kind=="vertical") {
                val vertical=JSONObject(fixture.toString())
                vertical.getJSONObject("layout").array("bands").getJSONObject(2).put("edge","left")
                action(obj("type" to "restore_workspace", "workspace" to vertical))
            }
            if (kind.startsWith("drawer") || kind=="column") {
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                if (kind.startsWith("drawer")) {
                    tap(bounds("column-icon-toolbar").center)
                    waitFor("toolbar drawer") { exists("column-drawer-41") }; settle()
                }
            }
            val tiles=host.panelContent!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").objects()
            val source=tiles.first { (it.getJSONObject("control").getString("kind")=="divider")==kind.endsWith("divider") }
            val tag=if(kind=="column") "column-icon-sizes"
                else "tile-toolbar-${source.getInt("id")}"
            val press=bounds(tag).center
            val destination=if(kind=="column") bounds("workspace").center else {
                val target=tiles.first { it.getInt("id")!=source.getInt("id") && it.getJSONObject("control").getString("kind")!="divider" &&
                    (bounds("tile-toolbar-${it.getInt("id")}").center-press).getDistance()>30*density }
                bounds("tile-toolbar-${target.getInt("id")}").let { Offset(it.right-2*density,it.bottom-2*density) }
            }
            val before=workspace()
            val label="$pointer/$kind"
            // Moving before the deadline retires the hold, even if contact then
            // pauses long enough to trigger a child control's long-click timer.
            event(MotionEvent.ACTION_DOWN,press); event(MotionEvent.ACTION_MOVE,destination)
            SystemClock.sleep(700)
            assertFalse("$label cannot pick up early",workspaceDragging())
            assertEquals("$label cannot open a menu after early motion",0,popupCount())
            event(MotionEvent.ACTION_UP); settle(); assertEquals("$label early release",before,workspace())
            event(MotionEvent.ACTION_DOWN,press); event(MotionEvent.ACTION_CANCEL); SystemClock.sleep(700)
            assertEquals("$label canceled hold",0,popupCount()); assertFalse(workspaceDragging())
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
            assertEquals("$label only touch/pen holds open menus",holdMenuCount,popupCount()); assertEquals(before,workspace())
            event(MotionEvent.ACTION_CANCEL); settle()
            assertEquals("$label canceled menu",0,popupCount()); assertFalse(workspaceDragging())
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
            event(MotionEvent.ACTION_UP); settle(); assertEquals("$label release retains touch/pen menu",holdMenuCount,popupCount())
            if(holdMenuCount>0)back()
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
            event(MotionEvent.ACTION_MOVE,destination); waitFor("$label held pickup") { workspaceDragging() }
            waitFor("$label closes menu for drag") { popupCount()==0 }
            event(MotionEvent.ACTION_CANCEL); settle(); assertEquals("$label canceled drag",before,workspace())
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
            event(MotionEvent.ACTION_MOVE,destination); settle(); event(MotionEvent.ACTION_UP); settle()
            val after=workspace(); assertNotEquals("$label commits drop",before,after)
            action(obj("type" to "invoke","command" to "undo_workspace")); assertEquals("$label one undo",before,workspace())
            action(obj("type" to "invoke","command" to "redo_workspace")); assertEquals("$label one redo",after,workspace())
            assertFalse("$label releases cursor",workspaceDragging())
        }
    }

    @Test fun tabsAndGripsStillPickUpImmediately() {
        for(pointer in pointerTools) for(kind in listOf("tab","ribbon","group","drawer-tab","drawer-grip","column")) {
            tool=pointer; restore()
            if(kind.startsWith("drawer") || kind=="column") {
                customize(obj("type" to "set_column_collapsed","group" to 41,"collapsed" to true))
                if(kind.startsWith("drawer")) { tap(bounds("column-icon-brushes").center); waitFor("drawer") { exists("column-drawer-41") }; settle() }
            }
            val tag=when(kind) { "tab"->"tab-sizes"; "ribbon"->"ribbon-grip-toolbar"; "group"->"group-grip-41"
                "drawer-tab"->"drawer-tab-sizes"; "drawer-grip"->"column-drawer-grip-41"; else->"column-grip-41" }
            val before=workspace()
            event(MotionEvent.ACTION_DOWN,bounds(tag).center); event(MotionEvent.ACTION_MOVE,bounds("workspace").center)
            waitFor("$pointer/$kind immediate pickup",400) { workspaceDragging() }
            assertEquals(0,popupCount()); event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(before,workspace())
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

    @Test fun collapsedDividerDropsHaveForgivingTargetsAndAlignedPreviews() {
        fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
            "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon")
        for (edge in listOf("left", "right")) {
            fixture.getJSONObject("layout").put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to edge, "extent" to 252, "root" to obj("kind" to "split", "id" to 41,
                    "axis" to "vertical", "fraction" to .5, "first" to tabs(42, "brushes"), "second" to tabs(43, "sizes"))),
                obj("id" to 44, "edge" to if (edge == "left") "right" else "left", "extent" to 252,
                    "root" to tabs(45, "layers", "properties", "adjustments")))))
            for (pointer in pointerTools) for (mode in listOf("-8", "-5", "0", "5", "8", "cancel")) {
                val offset = mode.toFloatOrNull() ?: 5f
                val merge = kotlin.math.abs(offset) > 6f
                tool = pointer; restore()
                for (id in listOf(42, 45)) customize(obj("type" to "set_column_collapsed", "group" to id, "collapsed" to true))
                val before = workspace()
                val tile = bounds("column-icon-sizes")
                val divider = bounds("column-divider-41-1").center
                assertEquals("Native/shared separator alignment", tile.top - 6 * density, divider.y, 1f)
                val destination = divider + Offset(0f, offset * density)
                event(MotionEvent.ACTION_DOWN, bounds("column-icon-layers").center); SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle()
                event(MotionEvent.ACTION_MOVE, destination)
                waitFor("$edge/$pointer/$mode preview") { host.workspaceGeometry?.hint != null && exists("workspace-drop-hint") }
                settle()
                assertEquals("Pickup closes the held menu", 0, popupCount())
                val hint = host.workspaceGeometry!!.hint!!
                val target = hint.getJSONObject("target")
                if (merge) {
                    assertEquals("tab", target.getString("kind"))
                    assertEquals(if (offset < 0) 42 else 43, target.getInt("group"))
                } else {
                    assertEquals("split", target.getString("kind")); assertEquals(43, target.getInt("group"))
                    assertEquals("top", target.getString("edge"))
                    assertEquals("Preview stays on the divider", divider.y, bounds("workspace-drop-hint").center.y, 1f)
                }
                event(if (mode == "cancel") MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP); settle()
                assertFalse(exists("workspace-drop-hint"))
                if (mode == "cancel") { assertEquals(before, workspace()); continue }
                val after = workspace()
                assertEquals(0, state().getJSONObject("workspace").getJSONObject("layout").array("floating").length())
                val column = snapshot().getJSONObject("layout").array("collapsed").objects().first { it.getInt("id") == 41 }
                val groups = column.array("groups").objects()
                assertEquals(if (merge) 2 else 3, groups.size)
                val panels = groups[if (merge && offset < 0) 0 else 1].array("icons").objects().map { it.getString("panel") }
                if (merge) assertTrue("Adjacent tile still merges tabs", "layers" in panels && (if (offset < 0) "brushes" else "sizes") in panels)
                else assertEquals(listOf("layers"), panels)
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(after, workspace())
            }
        }
    }

    @Test fun toolbarDividerDropsCreateSeparateToolGroups() {
        val layout = fixture.getJSONObject("layout")
        val first = layout.getInt("next_tile_id")
        val ids = (first until first + 5).toList()
        layout.put("next_tile_id", first + 5)
        val controls = listOf(obj("kind" to "command", "command" to "brush"), obj("kind" to "command", "command" to "eraser"),
            obj("kind" to "divider"), obj("kind" to "command", "command" to "lasso"), obj("kind" to "command", "command" to "hand"))
        layout.array("panels").objects().first { it.getString("id") == "toolbar" }.apply {
            put("tile_style", "small")
            getJSONObject("content").put("tiles", JSONArray(ids.mapIndexed { index, id -> obj("id" to id, "control" to controls[index]) }))
        }
        for (edge in listOf("top", "left")) {
            layout.put("bands", JSONArray(listOf(obj("id" to 40, "edge" to edge, "extent" to 36,
                "root" to obj("kind" to "tabs", "id" to 41, "panels" to JSONArray(listOf("toolbar")), "active" to "toolbar", "tab_style" to "icon")))))
            for (pointer in pointerTools) for (mode in listOf("-5", "5", "8", "cancel")) {
                tool = pointer; restore()
                val before = workspace()
                val divider = bounds("tile-toolbar-${ids[2]}").center
                val offset = (mode.toFloatOrNull() ?: 5f) * density
                val destination = divider + if (edge == "top") Offset(offset, 0f) else Offset(0f, offset)
                event(MotionEvent.ACTION_DOWN, bounds("tile-toolbar-${ids[0]}").center); SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE, destination)
                waitFor("$edge/$pointer/$mode toolbar preview") { exists("workspace-drop-hint") && popupCount() == 0 }
                settle()
                if (mode != "8") {
                    val line = bounds("workspace-drop-hint").center
                    assertEquals("Divider preview x", divider.x, line.x, 1f)
                    assertEquals("Divider preview y", divider.y, line.y, 1f)
                }
                event(if (mode == "cancel") MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP)
                waitFor("toolbar drop finishes") { !workspaceDragging() && !exists("workspace-drop-hint") }; settle()
                if (mode == "cancel") { assertEquals(before, workspace()); continue }
                val after = workspace()
                val tiles = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects()
                    .first { it.getString("id") == "toolbar" }.getJSONObject("content").array("tiles").objects()
                assertEquals(if (mode == "8") listOf(ids[1], ids[2], ids[0], ids[3], ids[4])
                    else listOf(ids[1], ids[2], ids[0], first + 5, ids[3], ids[4]), tiles.map { it.getInt("id") })
                assertEquals(if (mode == "8") 1 else 2, tiles.count { it.getJSONObject("control").getString("kind") == "divider" })
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(after, workspace())
                if (mode != "8") {
                    val last = bounds("tile-toolbar-${ids[4]}")
                    val end = if (edge == "top") Offset(last.right - 3f * density, last.center.y)
                        else Offset(last.center.x, last.bottom - 3f * density)
                    event(MotionEvent.ACTION_DOWN, bounds("tile-toolbar-${ids[0]}").center); SystemClock.sleep(700)
                    event(MotionEvent.ACTION_MOVE, end)
                    waitFor("empty-group move preview") { exists("workspace-drop-hint") }
                    event(MotionEvent.ACTION_UP)
                    waitFor("empty-group move finishes") { !workspaceDragging() && !exists("workspace-drop-hint") }; settle()
                    val collapsed = workspace()
                    val remaining = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects()
                        .first { it.getString("id") == "toolbar" }.getJSONObject("content").array("tiles").objects()
                    assertEquals("Empty group retains its first divider", listOf(ids[1], ids[2], ids[3], ids[4], ids[0]), remaining.map { it.getInt("id") })
                    assertFalse("Redundant divider view is removed", exists("tile-toolbar-${first + 5}"))
                    action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals("One undo restores the group and divider IDs", after, workspace())
                    assertTrue(exists("tile-toolbar-${first + 5}"))
                    action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(collapsed, workspace())
                }
            }
        }
    }

    @Test fun mouseTilesShowPointerThenGrabThenGrabbing() {
        tool = MotionEvent.TOOL_TYPE_MOUSE
        customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
        val tiles = fixture.getJSONObject("layout").array("panels").objects().first { it.getString("id") == "toolbar" }
            .getJSONObject("content").array("tiles").objects()
        val normal = tiles.first { it.getJSONObject("control").getString("kind") != "divider" }.getInt("id")
        val divider = tiles.first { it.getJSONObject("control").getString("kind") == "divider" }.getInt("id")
        for (tag in listOf("tile-toolbar-$normal", "tile-toolbar-$divider", "column-icon-sizes")) {
            val point = bounds(tag).center
            val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE })
            val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y })
            val hover = MotionEvent.obtain(0, SystemClock.uptimeMillis(), MotionEvent.ACTION_HOVER_MOVE, 1, properties, coords,
                0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_MOUSE, 0)
            instrumentation.runOnMainSync { owner.view.dispatchGenericMotionEvent(hover) }; hover.recycle(); settle()
            assertEquals("$tag starts with normal pointer", PointerIcon.getSystemIcon(owner.view.context, PointerIcon.TYPE_ARROW), owner.view.pointerIcon)
            val before = workspace()
            event(MotionEvent.ACTION_DOWN, point); SystemClock.sleep(700)
            waitFor("$tag armed cursor") { surface.pointerIcon == PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_GRAB) }
            assertEquals(0, popupCount())
            event(MotionEvent.ACTION_MOVE, bounds("workspace").center)
            waitFor("$tag dragging cursor") { workspaceDragging() }
            event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(before, workspace())
            assertEquals("Canceled pickup clears the hand", PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_NULL), surface.pointerIcon)
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

    @Test fun toolbarDrawersSwitchToolsOnFirstTap() {
        val originalPreset = state().getJSONObject("brush").getInt("preset")
        val toolCommands = setOf("pen", "pencil", "brush", "eraser", "airbrush", "decoration", "blend", "liquify",
            "lasso", "move", "hand", "eyedropper", "gradient", "figure", "ruler", "auto_select", "fill")
        val originalTool = state().array("commands").objects().firstOrNull {
            it.optBoolean("selected") && it.getString("id") in toolCommands
        }?.getString("id")
        val layout = fixture.getJSONObject("layout")
        val ids = (0..2).map { layout.getInt("next_tile_id") + it }
        layout.put("next_tile_id", ids.last() + 1)
        layout.array("panels").objects().first { it.getString("id") == "toolbar" }.getJSONObject("content").put("tiles", JSONArray(ids.mapIndexed { i, id ->
            obj("id" to id, "control" to if (i == 2) obj("kind" to "color") else obj("kind" to "command", "command" to if (i == 0) "brush" else "eraser"))
        }))
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun tag(i: Int) = "tile-toolbar-${ids[i]}"
        fun click(i: Int, previous: Int? = null) {
            event(MotionEvent.ACTION_DOWN, bounds(tag(i)).center)
            SystemClock.sleep(40); instrumentation.waitForIdleSync()
            try { if (previous != null) assertEquals("Press retains the previous drawer", ids[previous], drawer()?.getJSONObject("anchor")?.getInt("tile")) }
            finally { event(MotionEvent.ACTION_UP) }
            settle()
        }
        fun check(i: Int) {
            waitFor("drawer moves to ${ids[i]}") { drawer()?.getJSONObject("anchor")?.optInt("tile") == ids[i] }
            assertEquals(if (i == 2) 1 else 2, drawer()!!.array("columns").length())
            if (i != 2) {
                val command = if (i == 0) "brush" else "eraser"
                assertEquals(command, state().getJSONObject("brush").getString("tool"))
                assertTrue("New tool activates on the same tap", state().array("commands").objects().first { it.getString("id") == command }.getBoolean("selected"))
            }
            assertTrue(exists("tool-drawer"))
        }
        fun move(target: JSONObject) = action(obj("type" to "move_panel", "panel" to "toolbar", "target" to target,
            "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
        try {
            restore()
            for (edge in listOf("top", "bottom", "left", "right")) {
                move(obj("kind" to "edge", "edge" to edge, "outer" to true))
                action(obj("type" to "invoke", "command" to "pen"))
                click(0); assertNull("Closed drawers still require selecting first", drawer())
                click(0); check(0)
                click(1, 0); check(1)
                click(2, 1); check(2)
                click(0, 2); check(0)
                click(0); waitFor("current opener closes") { !exists("tool-drawer") }; assertNull(drawer())
            }
            move(obj("kind" to "tab", "group" to 41, "index" to null))
            customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
            tap(bounds("column-icon-toolbar").center); waitFor("nested toolbar") { exists(tag(2)) }; settle()
            click(2); check(2); click(1, 2); check(1); click(0, 1); check(0)
        } finally {
            action(obj("type" to "select_brush", "id" to originalPreset))
            originalTool?.let { action(obj("type" to "invoke", "command" to it)) }
        }
    }

    @Test fun drawerButtonsAndBridgesKeepTheirColors() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val toolbar = fixture.getJSONObject("layout").array("panels").objects().first { it.getString("id") == "toolbar" }
        val tile = toolbar.getJSONObject("content").array("tiles").getJSONObject(0).getInt("id")
        val alternateTile = fixture.getJSONObject("layout").getInt("next_tile_id")
        fixture.getJSONObject("layout").put("next_tile_id", alternateTile + 1)
        toolbar.getJSONObject("content").put("tiles", JSONArray(listOf(
            obj("id" to tile, "control" to obj("kind" to "panel", "panel" to "color")),
            obj("id" to alternateTile, "control" to obj("kind" to "panel", "panel" to "sizes")))))
        val toolTag = "tile-toolbar-$tile"
        val alternateToolTag = "tile-toolbar-$alternateTile"
        fun moveToolbar(target: JSONObject) = action(obj("type" to "move_panel", "panel" to "toolbar", "target" to target,
            "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
        fun capture(name: String, check: (sample: (Offset) -> Int) -> Unit) {
            // Wait for the native touch ripple to finish before checking resting colors.
            SystemClock.sleep(700)
            settle()
            val image = instrumentation.uiAutomation.takeScreenshot()
            val location = IntArray(2)
            instrumentation.runOnMainSync { owner.view.getLocationOnScreen(location) }
            try {
                val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/drawer-style-$name.png")
                file.parentFile!!.mkdirs()
                file.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                check { p -> image.getPixel((p.x + location[0]).toInt(), (p.y + location[1]).toInt()) }
            } finally { image.recycle() }
        }
        fun checkJoin(source: String, drawer: String, name: String, panelColor: Int, selected: Boolean) {
            val b = bounds(source); val d = bounds(drawer)
            val horizontal = d.left >= b.right - density || d.right <= b.left + density
            val positive = if (horizontal) d.left >= b.right - density else d.top >= b.bottom - density
            val near = if (horizontal) (if (positive) b.right - density else b.left + density)
                else (if (positive) b.bottom - density else b.top + density)
            val middle = if (horizontal) Offset(near, b.center.y) else Offset(b.center.x, near)
            val far = if (horizontal) Offset(if (positive) b.left + density else b.right - density, b.center.y)
                else Offset(b.center.x, if (positive) b.top + density else b.bottom - density)
            val corners = if (horizontal) listOf(Offset(near, b.top + density), Offset(near, b.bottom - density))
                else listOf(Offset(b.left + density, near), Offset(b.right - density, near))
            val bridge = if (horizontal) Offset(if (positive) (b.right + d.left) / 2 else (b.left + d.right) / 2, b.center.y)
                else Offset(b.center.x, if (positive) (b.bottom + d.top) / 2 else (b.top + d.bottom) / 2)
            capture(name) { sample ->
                assertEquals("$name: connector is not darkened by the drawer shadow", panelColor, sample(bridge))
                val fill = sample(middle)
                if (selected) assertTrue("$name: open button is blue", android.graphics.Color.blue(fill) > android.graphics.Color.red(fill))
                else assertEquals("$name: toolbar source is not darkened", panelColor, fill)
                (corners + far).forEach { point ->
                    val color = sample(point)
                    assertTrue("$name: square source corners match its fill ($fill vs $color)", listOf(0, 8, 16).all {
                        kotlin.math.abs((fill shr it and 255) - (color shr it and 255)) <= 1
                    })
                }
            }
        }
        try { for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); restore()
            val panelColor = android.graphics.Color.parseColor(if (theme == "light") "#ededed" else "#414141")
            for ((column, panel) in listOf(41 to "brushes", 43 to "navigator")) {
                customize(obj("type" to "set_column_collapsed", "group" to column, "collapsed" to true))
                val tag = "column-icon-$panel"
                fun checkClosed() {
                    val b = bounds(tag)
                    capture("$theme-closed-$column") { sample ->
                        assertEquals("Closed column button is plain grey", panelColor, sample(Offset(b.center.x, b.top + 2 * density)))
                    }
                }
                checkClosed(); tap(bounds(tag).center); waitFor("open drawer") { exists("column-drawer-$column") }; settle()
                checkJoin(tag, "column-drawer-$column", "$theme-column-$column", panelColor, true)
                val alternate = if (column == 41) "sizes" else "layers"
                tap(bounds("drawer-tab-$alternate").center)
                waitFor("drawer anchor follows tab") {
                    state().getJSONObject("customization").array("column_drawers").objects().any {
                        it.getJSONObject("anchor").getInt("column") == column && it.getJSONObject("anchor").getString("origin") == alternate
                    }
                }
                settle()
                checkJoin("column-icon-$alternate", "column-drawer-$column", "$theme-switched-column-$column", panelColor, true)
                checkClosed()
                tap(bounds("column-icon-$alternate").center); waitFor("close drawer") { !exists("column-drawer-$column") }; checkClosed()
            }
            for (edge in listOf("top", "bottom", "left", "right")) {
                moveToolbar(obj("kind" to "edge", "edge" to edge, "outer" to true))
                tap(bounds(toolTag).center); waitFor("toolbar drawer") { exists("tool-drawer") }; settle()
                checkJoin(toolTag, "tool-drawer", "$theme-toolbar-$edge", panelColor, false)
                tap(bounds(alternateToolTag).center); settle()
                checkJoin(alternateToolTag, "tool-drawer", "$theme-toolbar-$edge-switched", panelColor, false)
                tap(bounds(toolTag).center); settle()
                checkJoin(toolTag, "tool-drawer", "$theme-toolbar-$edge-switched-back", panelColor, false)
                tap(bounds(toolTag).center); waitFor("close toolbar drawer") { !exists("tool-drawer") }
            }
            moveToolbar(obj("kind" to "tab", "group" to 41, "index" to null))
            tap(bounds("column-icon-toolbar").center); waitFor("toolbar column drawer") { exists(toolTag) }; settle()
            tap(bounds(alternateToolTag).center); waitFor("nested drawer") { exists("tool-drawer") }; settle()
            checkJoin(alternateToolTag, "tool-drawer", "$theme-nested-toolbar", panelColor, false)
            tap(bounds(toolTag).center); settle()
            checkJoin(toolTag, "tool-drawer", "$theme-nested-toolbar-switched", panelColor, false)
        }
            val layout = fixture.getJSONObject("layout")
            val nextTile = layout.getInt("next_tile_id")
            layout.put("next_tile_id", nextTile + 2).put("next_id", maxOf(48, layout.getInt("next_id")))
            toolbar.getJSONObject("content").put("tiles", JSONArray(listOf(
                obj("id" to tile, "control" to obj("kind" to "panel", "panel" to "color")),
                obj("id" to nextTile, "control" to obj("kind" to "divider")),
                obj("id" to nextTile + 1, "control" to obj("kind" to "panel", "panel" to "color")))))
            fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id,
                "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "icon_name")
            layout.array("bands").getJSONObject(0).put("root", obj("kind" to "split", "id" to 46,
                "axis" to "vertical", "fraction" to .5, "first" to tabs(41, "brushes", "tool_settings"), "second" to tabs(47, "sizes")))
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme)); restore()
                customize(obj("type" to "set_column_collapsed", "group" to 46, "collapsed" to true))
                assertEquals("Leading divider uses toolbar spacing below the expand button", 12 * density,
                    bounds("column-icon-brushes").top - bounds("expand-column-46").bottom, 1f)
                assertEquals("Collapsed group spacing matches toolbar divider and gaps", 12 * density,
                    bounds("column-icon-sizes").top - bounds("column-icon-tool_settings").bottom, 1f)
                fun line(tag: String, horizontal: Boolean, name: String) {
                    waitFor("divider layout $tag") {
                        find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)?.boundsInRoot?.let {
                            kotlin.math.abs((if (horizontal) it.height else it.width) - 8 * density) < 1f
                        } == true
                    }
                    val b = bounds(tag)
                    assertEquals("Divider slot is 8dp", 8 * density, if (horizontal) b.height else b.width, 1f)
                    capture("$theme-$name") { sample ->
                        assertNotEquals("Divider line is visible", sample(b.center), sample(b.center +
                            if (horizontal) Offset(0f, 2 * density) else Offset(2 * density, 0f)))
                    }
                }
                line("column-divider-46-0", true, "column-divider")
                line("column-divider-46-1", true, "column-group-divider")
                line("tile-toolbar-$nextTile", false, "toolbar-divider-horizontal")
                moveToolbar(obj("kind" to "edge", "edge" to "right", "outer" to true))
                line("tile-toolbar-$nextTile", true, "toolbar-divider-vertical")
                moveToolbar(obj("kind" to "tab", "group" to 41, "index" to null))
                tap(bounds("column-icon-toolbar").center); waitFor("nested toolbar divider") { exists("tile-toolbar-$nextTile") }
                line("tile-toolbar-$nextTile", true, "toolbar-divider-in-drawer")
            }
        } finally { action(obj("type" to "set_theme", "theme" to originalTheme)) }
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
                instrumentation.runOnMainSync { owner.view.getLocationOnScreen(location) }
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
            for (pointer in pointerTools) for (handle in listOf(true, false)) {
                tool = pointer
                val sourceBounds = bounds("layer-row-$source")
                val press = Offset(if (handle) sourceBounds.right - 12 * density else sourceBounds.center.x, sourceBounds.center.y)
                val destination = bounds("layer-row-$target").let { Offset(it.right - 12 * density, it.bottom - 3 * density) }
                event(MotionEvent.ACTION_DOWN, press); SystemClock.sleep(700)
                assertEquals("Only touch/pen holds open layer context", holdMenuCount, popupCount())
                instrumentation.runOnMainSync { assertTrue("Layer menu preserves window focus during contact", owner.view.hasWindowFocus()) }
                event(MotionEvent.ACTION_UP); settle(); assertEquals(holdMenuCount, popupCount()); if(holdMenuCount>0)back()
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

    @Test fun layerBodiesReserveTouchAndPenForScrollingUntilHeld() {
        action(obj("type" to "select_panel_tab","group" to 43,"panel" to "layers"))
        val original=state().array("layers").objects().map { it.getLong("id") }
        var additions=0
        var committed=false
        try {
            repeat(28) { action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false))); additions++ }
            val before=state().array("layers").objects().map { it.getLong("id") }
            for(pointer in pointerTools) for(handle in listOf(false,true)) {
                tool=pointer
                resetLayerScroll(before[0])
                val source=bounds("layer-row-${before[0]}")
                val target=bounds("layer-row-${before[1]}")
                val press=Offset(if(handle)source.right-12*density else source.center.x,source.center.y)
                val destination=Offset(press.x,target.bottom-3*density)
                event(MotionEvent.ACTION_DOWN,press); event(MotionEvent.ACTION_MOVE,destination); settle()
                val direct=handle || pointer==MotionEvent.TOOL_TYPE_MOUSE
                assertEquals("$pointer/$handle pickup before hold",direct,exists("layer-drag-preview"))
                if(!direct) { SystemClock.sleep(700); assertEquals("Early motion retires row hold",0,popupCount()); assertFalse(exists("layer-drag-preview")) }
                event(MotionEvent.ACTION_UP); settle()
                if(direct) {
                    committed=true
                    val after=listOf(before[1],before[0])+before.drop(2)
                    assertEquals(after,state().array("layers").objects().map { it.getLong("id") })
                    action(obj("type" to "invoke","command" to "undo")); committed=false
                    assertEquals(before,state().array("layers").objects().map { it.getLong("id") })
                    action(obj("type" to "invoke","command" to "redo")); committed=true
                    assertEquals(after,state().array("layers").objects().map { it.getLong("id") })
                    action(obj("type" to "invoke","command" to "undo")); committed=false
                }
                assertEquals(before,state().array("layers").objects().map { it.getLong("id") })
            }
            for(pointer in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)) {
                tool=pointer
                resetLayerScroll(before[0])
                val press=bounds("layer-rows").center
                event(MotionEvent.ACTION_DOWN,press)
                repeat(5) { step -> event(MotionEvent.ACTION_MOVE,press-Offset(0f,(step+1)*30*density)); SystemClock.sleep(25) }
                SystemClock.sleep(700)
                assertEquals("$pointer scrolling does not open menu",0,popupCount())
                assertFalse("$pointer scrolling does not reorder",exists("layer-drag-preview"))
                event(MotionEvent.ACTION_UP); settle()
                assertFalse("$pointer moved the native list",exists("layer-row-${before[0]}"))
                assertEquals(before,state().array("layers").objects().map { it.getLong("id") })
            }
        } finally {
            if(contact)event(MotionEvent.ACTION_CANCEL)
            if(committed)action(obj("type" to "invoke","command" to "undo"))
            repeat(additions) { action(obj("type" to "invoke","command" to "undo")) }
            assertEquals(original,state().array("layers").objects().map { it.getLong("id") })
        }
    }

    @Test fun pendingTileSourceRemovalAndWindowBlurRetirePickup() {
        for(pointer in pointerTools) for(mode in listOf("removed","held-removed","blur","drag-blur")) {
            tool=pointer; restore()
            customize(obj("type" to "set_column_collapsed","group" to 41,"collapsed" to true))
            val before=workspace()
            val press=bounds("column-icon-sizes").center
            event(MotionEvent.ACTION_DOWN,press)
            var blurWindow: android.app.Dialog?=null
            if(mode.endsWith("removed")) {
                if(mode=="held-removed") { SystemClock.sleep(700); assertEquals(holdMenuCount,popupCount()) }
                customize(obj("type" to "set_column_collapsed","group" to 41,"collapsed" to false))
            }
            else {
                if(mode=="drag-blur") {
                    SystemClock.sleep(700); event(MotionEvent.ACTION_MOVE,bounds("workspace").center)
                    waitFor("drag before blur") { workspaceDragging() }
                }
                instrumentation.runOnMainSync {
                    blurWindow=android.app.Dialog(activity).apply { setContentView(View(activity)); show() }
                }
                waitFor("native window loses focus") { !owner.view.hasWindowFocus() }
            }
            SystemClock.sleep(700)
            assertEquals("$pointer/$mode retires menu",0,popupCount()); assertFalse(workspaceDragging())
            assertEquals("$pointer/$mode retires pickup cursor", PointerIcon.getSystemIcon(surface.context, PointerIcon.TYPE_NULL), surface.pointerIcon)
            event(MotionEvent.ACTION_CANCEL)
            if(!mode.endsWith("removed")) {
                instrumentation.runOnMainSync { blurWindow!!.dismiss() }
                waitFor("native window regains focus") { owner.view.hasWindowFocus() }; settle()
                assertEquals("$pointer/$mode restores layout",before,workspace())
            }
        }
    }

    @Test fun rowChildHoldsSuppressClicksAndSecondaryClickKeepsContext() {
        action(obj("type" to "select_panel_tab","group" to 43,"panel" to "layers"))
        val id=state().array("layers").objects().first().getLong("id")
        fun row()=state().array("layers").objects().first { it.getLong("id")==id }
        for(pointer in pointerTools) for(offset in listOf(18f,44f,84f,150f)) {
            tool=pointer
            val r=bounds("layer-row-$id")
            val press=Offset(r.left+offset*density,r.center.y)
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
            waitFor("$pointer/$offset only touch/pen child holds open menus") { popupCount()==holdMenuCount }
            val visible=row().getBoolean("visible")
            val selected=row().getBoolean("selected")
            event(MotionEvent.ACTION_UP); settle()
            assertEquals("Held visibility control does not click",visible,row().getBoolean("visible"))
            assertEquals("Held selection control does not click",selected,row().getBoolean("selected"))
            assertEquals(holdMenuCount,popupCount()); if(holdMenuCount>0)back()
            event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700); event(MotionEvent.ACTION_CANCEL); settle()
            assertEquals("Canceled child hold clears row menu",0,popupCount())
        }
        tool=MotionEvent.TOOL_TYPE_MOUSE; mouseButton=MotionEvent.BUTTON_SECONDARY
        try {
            tap(bounds("layer-row-$id").center)
            waitFor("Secondary row click opens menu") { popupCount()==1 }; back()
            val tile=host.panelContent!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").getJSONObject(0).getInt("id")
            tap(bounds("tile-toolbar-$tile").center)
            waitFor("Secondary tile click opens menu") { popupCount()==1 }; back()
        } finally { mouseButton=MotionEvent.BUTTON_PRIMARY }
    }

    @Test fun collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible() {
        for(pointer in pointerTools) for(panel in listOf("brushes","sizes")) {
            tool=pointer; restore()
            customize(obj("type" to "set_column_collapsed","group" to 41,"collapsed" to true))
            tap(bounds("column-icon-brushes").center); waitFor("drawer") { exists("column-drawer-41") }; settle()
            val before=workspace()
            val press=bounds("column-icon-$panel").center
            val destination=bounds("workspace").center
            for(cancel in listOf(true,false)) {
                event(MotionEvent.ACTION_DOWN,press); SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE,destination)
                waitFor("$pointer/$panel icon pickup") { workspaceDragging() }
                assertNull("Icon pickup is not tab sliding",host.workspaceGeometry?.tab)
                event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP); settle()
                if(cancel)assertEquals(before,workspace())
                else {
                    val after=workspace(); assertNotEquals(before,after)
                    action(obj("type" to "invoke","command" to "undo_workspace")); assertEquals(before,workspace())
                    action(obj("type" to "invoke","command" to "redo_workspace")); assertEquals(after,workspace())
                }
            }
        }
    }

    @Test fun mouseZenHoldsStaySilentWhileSecondaryClickOpensMenu() {
        tool=MotionEvent.TOOL_TYPE_MOUSE
        action(obj("type" to "invoke","command" to "zen_mode"))
        waitFor("Zen chrome") { exists("zen-button") }; settle()
        val before=workspace()
        val targets=mutableListOf("zen-button")
        val tiles=host.panelContent!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").objects()
        tiles.firstOrNull { exists("tile-toolbar-${it.getInt("id")}") }?.let { targets.add("tile-toolbar-${it.getInt("id")}") }
        for(tag in targets) {
            event(MotionEvent.ACTION_DOWN,bounds(tag).center); SystemClock.sleep(700)
            assertEquals("Mouse hold stays silent on $tag",0,popupCount())
            event(MotionEvent.ACTION_UP); settle(); assertEquals(before,workspace())
        }
        mouseButton=MotionEvent.BUTTON_SECONDARY
        try {
            tap(bounds("zen-button").center)
            waitFor("Secondary click opens Zen menu") { popupCount()==1 }; back()
        } finally { mouseButton=MotionEvent.BUTTON_PRIMARY }
    }
}
