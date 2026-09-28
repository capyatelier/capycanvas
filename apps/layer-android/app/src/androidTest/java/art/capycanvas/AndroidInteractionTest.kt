package art.capycanvas

import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.View
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.io.File

/** Native mouse/finger/pen MotionEvents on the tablet, including popup focus and CANCEL.
 * Keep the real frame clock: a held contact must survive opening a native popup. */
class AndroidInteractionTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private lateinit var host: CanvasHost
    private lateinit var owner: ViewRootForTest
    private lateinit var surface: CanvasSurfaceView
    private lateinit var fixture: JSONObject
    private var density = 1f
    private var downAt = 0L
    private var contact = false
    private var popupInput = false
    private var inputWindow: View? = null
    private var point = Offset.Zero
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var mouseButton = MotionEvent.BUTTON_PRIMARY
    // View dispatch keeps exact geometry deterministic. Opt into the OS input
    // dispatcher with -e systemInput true where system injection is available.
    private var systemInput = InstrumentationRegistry.getArguments().getString("systemInput") == "true"

    private fun find(tag: String) = owner.find(hasTag(tag))
    private fun tagged(tag: String) = findTag(tag, owner)
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync {
            tagged(tag)?.let { (root,node) ->
                val origin = IntArray(2); val base = IntArray(2)
                root.view.getLocationOnScreen(origin); owner.view.getLocationOnScreen(base)
                result = node.boundsInRoot.translate(Offset((origin[0]-base[0]).toFloat(), (origin[1]-base[1]).toFloat()))
            }
        }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun exists(tag: String) = tagged(tag) != null
    private fun shown(tag: String): Boolean { var placed = false; onMain { placed = tagged(tag)?.second?.layoutInfo?.isPlaced == true }; return placed }
    private fun snapshot() = host.snapshot!!
    private fun state() = snapshot().getJSONObject("state")
    private fun workspace() = state().getJSONObject("workspace").toString()
    private fun group(panel: String) = snapshot().getJSONObject("layout").array("groups").objects()
        .first { panel in it.array("panels").values() }
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) =
        host.awaitMain(label, timeout, { "workspace manager: ${host.workspaceManager}" }, condition)
    // A held contact at a scroll edge can keep native overscroll animation
    // alive. Drain main-thread work without waiting forever for global idleness.
    private fun settle() { SystemClock.sleep(180); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun action(value: JSONObject) { host.drain(value, 10); settle() }
    private fun customize(value: JSONObject) = action(obj("type" to "customize", "action" to value))
    private fun transparency() = listOf("off", "low", "medium", "high").indexOf(state().getJSONObject("settings").getString("transparency"))
    private fun transparency(value: Int) = action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to value)))
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
            else instrumentation.runOnMainSync {
                if (action == MotionEvent.ACTION_DOWN) {
                    inputWindow = owner.view
                    if (popupInput) android.view.inspector.WindowInspector.getGlobalWindowViews().lastOrNull { view ->
                        view.descendant<ViewRootForTest>()?.let { root -> listOf("brush-slider-preview", "workspace-menu", "toolbar-number-menu", "zoom-menu").any { root.find(hasTag(it)) != null } } == true
                    }?.let { view ->
                        val p = IntArray(2); view.getLocationOnScreen(p)
                        if (coords[0].x >= p[0] && coords[0].x < p[0]+view.width && coords[0].y >= p[1] && coords[0].y < p[1]+view.height) inputWindow = view
                        else MotionEvent.obtain(motion).also { outside -> outside.action = MotionEvent.ACTION_OUTSIDE; view.dispatchTouchEvent(outside); outside.recycle() }
                    }
                }
                val target = inputWindow ?: owner.view; val p = IntArray(2); target.getLocationOnScreen(p)
                motion.offsetLocation(-p[0].toFloat(), -p[1].toFloat()); target.dispatchTouchEvent(motion)
                if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) inputWindow = null
            }
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
            semanticsRoots().any { it.view.hasWindowFocus() && it.find(hasTag("workspace-menu")) != null }
        }
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        waitFor("native menu closes") { popupCount()==0 && owner.view.hasWindowFocus() }; settle()
    }
    private fun popupCount(): Int {
        fun count() = semanticsRoots().count { it.find(hasTag("workspace-menu")) != null }
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
            find("layer-rows")!!.config[
                androidx.compose.ui.semantics.SemanticsActions.ScrollToIndex].action!!.invoke(0)
        }
        waitFor("list reset") { exists("layer-row-$first") }; settle()
    }
    @Before fun ready() {
        scenario = launchCapy()
        device.landscape(scenario)
        scenario.onActivity {
            activity=it
            host = it.host; owner = it.window.decorView.descendant<ViewRootForTest>()!!
            surface = it.window.decorView.descendant<CanvasSurfaceView>()!!
            density = it.resources.displayMetrics.density
        }
        val defaults = Native.create(false)
        try { fixture = JSONObject(Native.snapshot(defaults)!!).getJSONObject("state").getJSONObject("workspace") }
        finally { Native.destroy(defaults) }
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 390, "root" to tabs(41, "brushes", "sizes", "tool_settings", style = "icon_name")),
                obj("id" to 42, "edge" to "right", "extent" to 330, "root" to tabs(43, "navigator", "layers", "properties", style = "icon_name")),
                obj("id" to 44, "edge" to "top", "extent" to 42, "root" to tabs(45, "toolbar", style = "icon_name")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(46, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        restore()
    }
    @After fun cleanup() {
        try { if (contact) event(MotionEvent.ACTION_CANCEL) }
        finally { if (::scenario.isInitialized) scenario.close() }
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
                    if (kind.startsWith("drawer")) { customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true)); tap(bounds(if (drawerToolbar) "column-icon-toolbar" else "column-icon-brushes").center); waitFor("drawer") { exists("column-drawer-41") }; settle() }
                }
                val tag = when (kind) {
                    "tab" -> "tab-sizes"
                    "group", "floating" -> "group-grip-41"
                    "drawer-tab" -> "drawer-tab-sizes"
                    "drawer-grip" -> "column-drawer-grip-41"
                    "tile", "drawer-tile" -> "tile-toolbar-${host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects().first().getInt("id") }"
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
                    customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                    tap(bounds("column-icon-toolbar").center)
                    waitFor("toolbar drawer") { exists("column-drawer-41") }; settle()
                }
            }
            val tiles=host.snapshot!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").objects()
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
                if(kind.startsWith("drawer")) customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
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
                val retained = host.snapshot
                val positions = mutableListOf<Rect>()
                for ((index, position) in listOf(first, wide).withIndex()) {
                    event(MotionEvent.ACTION_MOVE, position); settle()
                    assertNull(host.actionError)
                    assertTrue("Resize retains $panel controls", retained === host.snapshot)
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
                        val tiles = host.snapshot!!.array("panels").objects().first { it.getString("id") == panel }.array("tiles").objects()
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
        fun split(id: Int, axis: String, first: JSONObject, second: JSONObject) = obj("kind" to "split", "id" to id,
            "axis" to axis, "fraction" to .4, "first" to first, "second" to second)
        val nested = JSONObject(fixture.toString())
        nested.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(obj("id" to 40, "edge" to "left", "extent" to 650,
                "root" to split(60, "vertical", tabs(61, "sizes", "layers", style = "icon_name"),
                    split(62, "horizontal", tabs(63, "brushes", style = "icon_name"),
                        split(64, "vertical", tabs(65, "navigator", style = "icon_name"), tabs(66, "tool_settings", style = "icon_name"))))))))
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

    @Test fun menuBodyAndExtendedTabDropsAcrossDevices() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        try { for (right in listOf(false, true)) {
            val theme = if (right) "light" else "dark"
            action(obj("type" to "set_theme", "theme" to theme))
            for (pointer in pointerTools) for (zone in listOf("menu", "stack-menu", "body", "tabs-top", "tabs-lower"))
                for (source in listOf("panel", "group", "toolbar", "column")) {
                    if (source == "column" && !zone.endsWith("menu")) continue
                    tool = pointer
                    val label = "$theme/$pointer/$source/$zone"
                    android.util.Log.i("CapyLayoutDrops", label)
                    val layout = JSONObject(fixture.toString())
                    layout.getJSONObject("layout").apply {
                        put("bands", JSONArray(listOf(
                            obj("id" to 40, "edge" to if (right) "right" else "left", "extent" to 252,
                                "root" to obj("kind" to "split", "id" to 41, "axis" to "vertical", "fraction" to .5,
                                    "first" to tabs(42, "brushes", style = "icon_name"), "second" to tabs(43, "sizes", style = "icon_name"))),
                            obj("id" to 44, "edge" to if (right) "left" else "right", "extent" to 310,
                                "root" to tabs(45, "layers", "adjustments", "properties", style = "icon_name")),
                            obj("id" to 46, "edge" to "top", "extent" to 36, "root" to tabs(47, "toolbar", style = "icon_name")))))
                        put("column_stacks", JSONArray()); put("next_id", maxOf(50, getInt("next_id")))
                    }
                    action(obj("type" to "restore_workspace", "workspace" to layout))
                    if (zone == "stack-menu") customize(obj("type" to "set_column_collapsed", "group" to 42, "collapsed" to true))
                    if (source == "column") customize(obj("type" to "set_column_collapsed", "group" to 45, "collapsed" to true))
                    if (source == "group") {
                        action(obj("type" to "select_panel_tab", "group" to 45, "panel" to "properties"))
                        action(obj("type" to "move_group", "group" to 45, "target" to obj("kind" to "float", "position" to JSONArray(listOf(500, 230))),
                            "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
                    }
                    val before = workspace()
                    val targetId = if (zone == "tabs-lower") 43 else 42
                    val b = bounds(if (zone == "stack-menu") "collapsed-column-41" else "group-$targetId")
                    val tabHeight = snapshot().getJSONObject("layout").number("tab_bar_height") * density
                    val destination = when {
                        zone.endsWith("menu") -> Offset(b.center.x, bounds("title-bar").center.y)
                        zone.startsWith("tabs") -> bounds(if (zone == "tabs-lower") "tab-sizes" else "tab-brushes").let {
                            Offset(if (zone == "tabs-lower") it.right - 4 * density else it.left + 4 * density, b.top + tabHeight + 3 * density)
                        }
                        else -> b.center
                    }
                    val press = bounds(when (source) {
                        "column" -> "column-grip-45"; "toolbar" -> "ribbon-grip-toolbar"
                        "group" -> "group-grip-45"; else -> "tab-layers"
                    }).center
                    fun begin() {
                        waitFor("workspace window focus") { owner.view.hasWindowFocus() }
                        event(MotionEvent.ACTION_DOWN, press)
                        event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle()
                        event(MotionEvent.ACTION_MOVE, destination); settle()
                    }
                    begin()
                    val hint = host.workspaceGeometry?.hint ?: error("$label missing hint at $destination; dragging=${workspaceDragging()}")
                    val expected = when (zone) {
                        "menu" -> obj("kind" to "split", "group" to 42, "edge" to "top")
                        "stack-menu" -> obj("kind" to "stack_column", "column" to 41, "before" to true)
                        else -> obj("kind" to "tab", "group" to targetId, "index" to if (zone == "tabs-lower") 1 else 0)
                    }
                    assertEquals(label, expected.toString(), hint.getJSONObject("target").toString())
                    val shown = bounds("workspace-drop-hint")
                    val hb = hint.getJSONObject("bounds")
                    assertEquals(label, bounds("workspace").left + hb.number("x") * density, shown.left, 1.1f)
                    assertEquals(label, hb.number("width") * density, shown.width, 1.1f)
                    if (zone == "body") {
                        assertEquals(label, b.top + tabHeight, shown.top, 1.1f)
                        assertEquals(label, b.bottom, shown.bottom, 1.1f)
                        if (source == "group" && pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                            val image = instrumentation.uiAutomation.takeScreenshot()
                            val output = File(activity.getExternalFilesDir(null), "validation/layout-drops").apply { mkdirs() }
                            File(output, "$theme-body.png").outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                            image.recycle()
                        }
                    } else if (zone.startsWith("tabs")) {
                        assertEquals(label, b.top, shown.top, 1.1f)
                        assertEquals(label, 3 * density, shown.width, 1.1f)
                    }
                    if (source == "panel") {
                        event(MotionEvent.ACTION_CANCEL); settle(); assertEquals("$label cancel", before, workspace()); begin()
                    }
                    event(MotionEvent.ACTION_UP); settle()
                    val after = workspace()
                    val moved = if (source == "toolbar") listOf("toolbar") else if (source in listOf("group", "column")) listOf("layers", "adjustments", "properties") else listOf("layers")
                    if (zone == "body" || zone.startsWith("tabs")) {
                        val target = group(moved[0])
                        val old = listOf(if (zone == "tabs-lower") "sizes" else "brushes")
                        assertEquals(label, targetId, target.getInt("id"))
                        assertEquals(label, if (zone == "tabs-lower") old + moved else moved + old, target.array("panels").values())
                        assertEquals(label, if (source == "group") "properties" else moved[0], target.getString("active"))
                    } else if (zone == "stack-menu") {
                        val stack = JSONObject(after).getJSONObject("layout").array("column_stacks").objects().first { 41 in it.array("members").values() }
                        assertEquals(label, 2, stack.array("members").length())
                        assertEquals(label, 41, stack.array("members").getInt(1))
                        assertFalse("New stacks open whole columns", stack.getBoolean("drawers"))
                    } else {
                        assertTrue(label, group(moved[0]).getJSONObject("bounds").number("y") < group("brushes").getJSONObject("bounds").number("y"))
                        assertEquals(label, 0, JSONObject(after).getJSONObject("layout").array("collapsed").length())
                    }
                    assertFalse(label, exists("workspace-drop-hint"))
                    action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals("$label undo", before, workspace())
                    action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals("$label redo", after, workspace())
                }
        } } finally { action(obj("type" to "set_theme", "theme" to originalTheme)) }
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
                if (open) { customize(obj("type" to "set_column_drawers", "column" to column, "drawers" to true)); tap(bounds("column-icon-$target").center); waitFor("open target drawer") { exists("column-drawer-$column") }; settle() }
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

    private fun stackFixture() {
        fun split(id: Int, first: JSONObject, second: JSONObject) = obj("kind" to "split", "id" to id,
            "axis" to "vertical", "fraction" to .5, "first" to first, "second" to second)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 42, "root" to split(51,
                    split(41, split(42, tabs(43, "brushes", "sizes"), tabs(44, "navigator")), tabs(45, "color")), tabs(50, "tool_settings"))),
                obj("id" to 46, "edge" to "right", "extent" to 252, "root" to split(47,
                    tabs(48, "layers", "properties", "adjustments"), tabs(49, "toolbar"))))))
            put("collapsed", JSONArray(listOf(51, 42, 45, 50).map { obj("root" to it, "expanded_width" to 246) }))
            put("column_stacks", JSONArray(listOf(obj("column" to 51, "members" to JSONArray(listOf(42, 45, 50)), "drawers" to false, "auto_hide" to false))))
            put("next_id", maxOf(52, getInt("next_id")))
        }
        restore()
    }
    private fun collapsed() = snapshot().getJSONObject("layout").array("collapsed").objects()
    private fun columnFor(panel: String) = collapsed().first { c ->
        c.array("groups").objects().any { g -> g.array("icons").objects().any { it.getString("panel") == panel } }
    }
    private fun openTile(panel: String) {
        tap(bounds("column-icon-$panel").center)
        waitFor("open column for $panel") { columnFor(panel).objectOrNull("open") != null }; settle()
    }
    @Test fun stackedColumnsOpenAndResizeOrdinaryGroups() {
        stackFixture()
        for (pointer in pointerTools) {
            tool = pointer; restore()
            val columns = collapsed()
            assertEquals(3, columns.size)
            assertEquals(6 * density, bounds("collapsed-column-45").top - bounds("collapsed-column-42").bottom, 1f)
            assertFalse(exists("column-divider-42-0"))
            assertFalse("Closed stack has no resize affordance", exists("divider-40"))
            assertTrue(snapshot().getJSONObject("layout").array("dividers").objects().first { it.getInt("id") == 40 }.getBoolean("fixed"))
            openTile("brushes")
            val opened = columnFor("brushes").getJSONObject("open").getJSONObject("bounds").rect()
            assertEquals(bounds("collapsed-column-42").top / density, opened.top, 1f)
            assertEquals(bounds("collapsed-column-50").bottom / density, opened.bottom, 1f)
            assertTrue(exists("group-43") && exists("group-44"))
            for (panel in listOf("brushes", "navigator")) {
                assertTrue(exists("column-connection-42-$panel"))
                instrumentation.runOnMainSync {
                    assertEquals(true, find("column-icon-$panel")!!.config.getOrNull(SemanticsProperties.Selected))
                }
            }
            val beforeResize = workspace()
            val edge = bounds("divider-40").center
            event(MotionEvent.ACTION_DOWN, edge); event(MotionEvent.ACTION_MOVE, edge + Offset(80 * density, 0f)); settle()
            assertTrue(columnFor("brushes").getJSONObject("open").getJSONObject("bounds").number("width") > opened.width + 60f)
            val c = columnFor("brushes").getJSONObject("open").array("connections").getJSONArray(0).getJSONObject(1).getJSONObject("bounds").rect()
            assertEquals(c.left * density, bounds("column-connection-42-brushes").left, 1f)
            event(MotionEvent.ACTION_UP); settle()
            action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(beforeResize, workspace())
            openTile("sizes"); assertEquals("sizes", group("sizes").getString("active"))
            openTile("color"); assertEquals(1, collapsed().count { it.objectOrNull("open") != null })
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
            waitFor("Back closes open member") { collapsed().all { it.objectOrNull("open") == null } }
            customize(obj("type" to "set_column_auto_hide", "column" to 42, "auto_hide" to true))
            openTile("brushes")
            tap(snapshot().getJSONObject("layout").getJSONObject("work_area").rect().center * density)
            waitFor("Auto-hide closes open column") { collapsed().all { it.objectOrNull("open") == null } }
            customize(obj("type" to "set_column_drawers", "column" to 42, "drawers" to true))
            tap(bounds("column-icon-brushes").center)
            waitFor("Individual panel drawer") { exists("column-drawer-42") }
            assertTrue(collapsed().all { it.objectOrNull("open") == null })
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); settle()
        }
    }

    @Test fun stackedColumnDropsKeepFooterAppendTargets() {
        stackFixture()
        for (pointer in pointerTools) for (source in listOf("panel", "group", "toolbar", "tile", "drawer")) {
            for (mode in listOf("append", "member", "cancel", "middle")) {
                if (mode == "middle" && source != "group") continue
                tool = pointer; restore()
                if (source == "drawer") {
                    customize(obj("type" to "set_column_collapsed", "group" to 48, "collapsed" to true))
                    customize(obj("type" to "set_column_drawers", "column" to 47, "drawers" to true))
                    tap(bounds("column-icon-layers").center)
                    waitFor("source drawer") { exists("drawer-tab-layers") }; settle()
                }
                val before = workspace()
                val targetId = if (mode == "middle") 45 else 42
                val target = collapsed().first { it.getInt("id") == targetId }
                val last = target.array("groups").objects().last().getJSONObject("bounds").rect()
                val append = mode in listOf("append", "middle")
                val destination = if (append) Offset(last.center.x, last.bottom + 3f) * density else bounds("column-grip-$targetId").center
                val tag = when (source) {
                    "panel" -> "tab-layers"
                    "group" -> "group-grip-48"
                    "toolbar" -> "ribbon-grip-toolbar"
                    "drawer" -> "drawer-tab-layers"
                    else -> "column-icon-tool_settings"
                }
                val panels = when (source) {
                    "group" -> listOf("layers", "properties", "adjustments")
                    "toolbar" -> listOf("toolbar")
                    "tile" -> listOf("tool_settings")
                    else -> listOf("layers")
                }
                event(MotionEvent.ACTION_DOWN, bounds(tag).center)
                if (source == "tile") SystemClock.sleep(700)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle()
                event(MotionEvent.ACTION_MOVE, destination)
                waitFor("$pointer/$source/$mode drop preview") { host.workspaceGeometry?.hint != null && exists("workspace-drop-hint") }; settle()
                assertEquals(0, popupCount())
                val hint = host.workspaceGeometry!!.hint!!.getJSONObject("target")
                assertEquals(if (append) "split" else "stack_column", hint.getString("kind"))
                if (append) {
                    assertEquals("bottom", hint.getString("edge"))
                    assertEquals(last.bottom * density, bounds("workspace-drop-hint").center.y, 1f)
                }
                event(if (mode == "cancel") MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP); settle()
                assertFalse(exists("workspace-drop-hint"))
                if (mode == "cancel") { assertEquals(before, workspace()); continue }
                val after = workspace()
                val member = columnFor(panels[0])
                val groups = member.array("groups").objects()
                val moved = groups.first { g -> g.array("icons").objects().any { it.getString("panel") == panels[0] } }
                assertEquals(panels, moved.array("icons").objects().map { it.getString("panel") })
                val stack = state().getJSONObject("workspace").getJSONObject("layout").array("column_stacks").objects()
                    .first { member.getInt("id") in it.array("members").values() }
                assertEquals((if (source == "tile") 2 else 3) + (if (append) 0 else 1), stack.array("members").length())
                assertEquals(if (append) (if (mode == "middle") 2 else 3) else 1, groups.size)
                assertFalse(stack.getBoolean("drawers")); assertFalse(stack.getBoolean("auto_hide"))
                openTile(panels[0])
                if (pointer == MotionEvent.TOOL_TYPE_MOUSE && source == "group" && mode == "middle") {
                    val image = instrumentation.uiAutomation.takeScreenshot()
                    val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/stacked-column-append.png")
                    file.parentFile!!.mkdirs(); file.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }; image.recycle()
                }
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(after, workspace())
            }
        }
    }

    @Test fun collapsedDividerDropsHaveForgivingTargetsAndAlignedPreviews() {
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

    @Test fun hoverTooltipsStayReadableAndFollowTileEdges() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val layout = fixture.getJSONObject("layout")
        val bands = layout.array("bands").objects()
        val toolbarBand = bands.first { it.getInt("id") == 44 }
        val settingsId = layout.getJSONObject("header").array("zones").values()
            .flatMap { (it as JSONArray).objects() }
            .first { it.getJSONObject("item").getString("kind") == "settings" }.getInt("id")
        val tile = layout.array("panels").objects().first { it.getString("id") == "toolbar" }
            .getJSONObject("content").array("tiles").objects().first { it.getJSONObject("control").getString("kind") != "divider" }.getInt("id")
        fun hover(at: Offset, pointer: Int, exit: Boolean = false) {
            val now = SystemClock.uptimeMillis()
            val event = MotionEvent.obtain(now, now, if (exit) MotionEvent.ACTION_HOVER_EXIT else MotionEvent.ACTION_HOVER_ENTER, 1,
                arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = pointer }),
                arrayOf(MotionEvent.PointerCoords().apply { x = at.x; y = at.y }),
                0, 0, 1f, 1f, 0, 0, if (pointer == MotionEvent.TOOL_TYPE_MOUSE) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_STYLUS, 0)
            try { instrumentation.runOnMainSync {
                owner.view.dispatchGenericMotionEvent(event)
                if (!exit) { event.action = MotionEvent.ACTION_HOVER_MOVE; owner.view.dispatchGenericMotionEvent(event) }
            } }
            finally { event.recycle() }
        }
        fun tooltip() = findTag("hover-tooltip")
        try {
            for (theme in listOf("light", "dark")) for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                action(obj("type" to "set_theme", "theme" to theme))
                for (placement in listOf("below", "above", "left", "right", "column-left", "column-right")) {
                    toolbarBand.put("edge", when (placement) { "above" -> "bottom"; "left" -> "left"; else -> "top" })
                    layout.put("bands", JSONArray(if (placement == "left") listOf(toolbarBand) else bands))
                    restore()
                    if (placement.startsWith("column-")) customize(obj("type" to "set_column_collapsed", "group" to if (placement == "column-left") 41 else 43, "collapsed" to true))
                    val tag = when (placement) {
                        "right" -> "header-control-$settingsId"
                        "column-left" -> "column-icon-brushes"
                        "column-right" -> "column-icon-navigator"
                        else -> "tile-toolbar-$tile"
                    }
                    val anchor = bounds(tag)
                    hover(anchor.center, pointer)
                    waitFor("$theme/$pointer/$placement tooltip") { tooltip() != null }
                    SystemClock.sleep(180)
                    var tip = Rect.Zero
                    val origin = IntArray(2)
                    instrumentation.runOnMainSync {
                        owner.view.getLocationOnScreen(origin)
                        val (root, node) = checkNotNull(tooltip())
                        val location = IntArray(2); root.view.getLocationOnScreen(location)
                        tip = node.boundsInRoot.translate(Offset(location[0].toFloat(), location[1].toFloat()))
                        assertTrue("Tooltip does not take window focus", owner.view.hasWindowFocus())
                    }
                    val button = anchor.translate(Offset(origin[0].toFloat(), origin[1].toFloat()))
                    if (placement in listOf("below", "above")) assertEquals("Tooltip centers on the tile", button.center.x, tip.center.x, 2f)
                    val image = instrumentation.uiAutomation.takeScreenshot()
                    try {
                        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/tooltips/$theme-$pointer-$placement.png")
                        file.parentFile!!.mkdirs(); file.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                        // A bottom dock may still leave room below it because of
                        // window insets. Flip only when the actual tooltip cannot fit.
                        if (button.bottom + 4 * density + tip.height > image.height)
                            assertEquals("Tooltip flips above when it cannot fit below", button.top - 4 * density, tip.bottom, 2f)
                        else assertEquals("Tooltip is below its tile", button.bottom + 4 * density, tip.top, 2f)
                        assertTrue("Tooltip stays on screen", tip.left >= 0 && tip.top >= 0 && tip.right <= image.width && tip.bottom <= image.height)
                        val backdrop = image.getPixel(tip.center.x.toInt(), (tip.top + 3 * density).toInt())
                        assertTrue("Tooltip has a dark backdrop in either theme", listOf(0, 8, 16).all { (backdrop shr it and 255) < 100 })
                        var white = 0
                        for (y in tip.top.toInt() until tip.bottom.toInt()) for (x in tip.left.toInt() until tip.right.toInt()) {
                            val pixel = image.getPixel(x, y)
                            if (listOf(0, 8, 16).all { (pixel shr it and 255) > 230 }) white++
                        }
                        assertTrue("Tooltip has readable white lettering", white > 10)
                    } finally { image.recycle() }
                    hover(anchor.center, pointer, exit = true)
                    waitFor("Tooltip leaves with hover") { tooltip() == null }
                }
            }
        } finally { hover(Offset.Zero, MotionEvent.TOOL_TYPE_MOUSE, exit = true); action(obj("type" to "set_theme", "theme" to originalTheme)) }
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
            waitFor("tool drawer shown for ${ids[i]}") { exists("tool-drawer") }
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
            customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
            tap(bounds("column-icon-toolbar").center); waitFor("nested toolbar") { exists(tag(2)) }; settle()
            click(2); check(2); click(1, 2); check(1); click(0, 1); check(0)
        } finally {
            action(obj("type" to "select_brush", "id" to originalPreset))
            originalTool?.let { action(obj("type" to "invoke", "command" to it)) }
        }
    }

    @Test fun drawerButtonsAndBridgesKeepTheirColors() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val originalTransparency = transparency()
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
                if (selected) assertEquals("$name: open button uses the selection color",
                    android.graphics.Color.parseColor(state().getJSONObject("palette").getString("selection")), fill)
                else assertEquals("$name: toolbar source is not darkened", panelColor, fill)
                (corners + far).forEach { point ->
                    val color = sample(point)
                    assertTrue("$name: square source corners match its fill ($fill vs $color)", listOf(0, 8, 16).all {
                        kotlin.math.abs((fill shr it and 255) - (color shr it and 255)) <= 1
                    })
                }
            }
        }
        try { transparency(0); for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); restore()
            val panelColor = android.graphics.Color.parseColor(if (theme == "light") "#ededed" else "#414141")
            for ((column, panel) in listOf(41 to "brushes", 43 to "navigator")) {
                customize(obj("type" to "set_column_collapsed", "group" to column, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to column, "drawers" to true))
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
            layout.array("bands").getJSONObject(0).put("root", obj("kind" to "split", "id" to 46,
                "axis" to "vertical", "fraction" to .5, "first" to tabs(41, "brushes", "tool_settings", style = "icon_name"), "second" to tabs(47, "sizes", style = "icon_name")))
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme)); restore()
                customize(obj("type" to "set_column_collapsed", "group" to 46, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 46, "drawers" to true))
                assertEquals("First tile retains standard top padding", 6 * density,
                    bounds("column-icon-brushes").top - bounds("collapsed-column-46").top, 1f)
                instrumentation.runOnMainSync { assertFalse(exists("column-divider-46-0")) }
                assertEquals("Collapsed group spacing matches toolbar divider and gaps", 12 * density,
                    bounds("column-icon-sizes").top - bounds("column-icon-tool_settings").bottom, 1f)
                fun line(tag: String, horizontal: Boolean, name: String, slotDp: Float = 8f) {
                    waitFor("divider layout $tag") {
                        find(tag)?.boundsInRoot?.let {
                            kotlin.math.abs((if (horizontal) it.height else it.width) - slotDp * density) < 1f
                        } == true
                    }
                    val b = bounds(tag)
                    assertEquals("Divider slot is ${slotDp}dp", slotDp * density, if (horizontal) b.height else b.width, 1f)
                    capture("$theme-$name") { sample ->
                        assertNotEquals("Divider line is visible", sample(b.center), sample(b.center +
                            if (horizontal) Offset(0f, 2 * density) else Offset(2 * density, 0f)))
                    }
                }
                line("column-divider-46-1", true, "column-group-divider")
                line("tile-toolbar-$nextTile", false, "toolbar-divider-horizontal")
                moveToolbar(obj("kind" to "edge", "edge" to "right", "outer" to true))
                line("tile-toolbar-$nextTile", true, "toolbar-divider-vertical")
                moveToolbar(obj("kind" to "tab", "group" to 41, "index" to null))
                tap(bounds("column-icon-toolbar").center); waitFor("nested toolbar divider") { exists("tile-toolbar-$nextTile") }
                line("tile-toolbar-$nextTile", true, "toolbar-divider-in-drawer")
            }
        } finally { action(obj("type" to "set_theme", "theme" to originalTheme)); transparency(originalTransparency) }
    }

    @Test fun drawerTabsKeepActiveColorsAndPadding() {
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val originalTransparency = transparency()
        try { transparency(0); for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); restore()
            waitFor("previous drawer closes") { !exists("column-drawer-41") }; settle()
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
            customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
            tap(bounds("column-icon-brushes").center); waitFor("Drawer tabs") { exists("drawer-tab-brushes") }; settle()
            val drawer = bounds("drawer-tab-brushes")
            assertEquals("Drawer uses the same tab width", docked.width, drawer.width, 1f)
            assertEquals("Drawer uses the same tab height", docked.height, drawer.height, 1f)
            assertEquals("Drawer name padding", name.left - docked.left, bounds("tab-name-brushes").left - drawer.left, 1f)
            assertEquals("Drawer icon padding", icon.left - docked.left, bounds("tab-icon-brushes").left - drawer.left, 1f)
            assertEquals("Drawer active background and text match the docked tab in $theme", colors, pixels("drawer-tab-brushes"))
        } } finally { action(obj("type" to "set_theme", "theme" to originalTheme)); transparency(originalTransparency) }
    }

    @Test fun detachedPanelsKeepBodiesAndWiderResizeTargets() {
        systemInput = false
        val root = fixture.getJSONObject("layout").array("bands").getJSONObject(1).getJSONObject("root")
        root.put("panels", JSONArray(listOf("navigator", "layers", "properties", "adjustments", "color"))).put("tab_style", "icon")
        for (pointer in pointerTools)
            for (panel in listOf("properties", "adjustments", "layers", "color", "navigator")) for (wholeGroup in listOf(false, true)) {
                tool = pointer; restore()
                if (wholeGroup) action(obj("type" to "select_panel_tab", "group" to 43, "panel" to panel))
                val before = workspace()
                val sourceHeight = group(panel).getJSONObject("bounds").number("height")
                event(MotionEvent.ACTION_DOWN, bounds(if (wholeGroup) "group-grip-43" else "tab-$panel").center)
                event(MotionEvent.ACTION_MOVE, bounds("workspace").center)
                waitFor("$panel detaches") { group(panel).optBoolean("floating") }
                settle()
                assertTrue("$panel floating body is visible during contact", bounds("panel-body-$panel").height > 60 * density)
                val content = when (panel) { "adjustments" -> "filter-list"; "properties" -> "layer-properties"; "color" -> "color-panel"; "navigator" -> "navigator-overview"; else -> "layer-rows" }
                assertTrue("$panel controls are painted below the header", bounds(content).height > 20 * density)
                event(MotionEvent.ACTION_MOVE, point + Offset(24 * density, 30 * density)); settle()
                assertTrue("$panel body remains visible while moving", bounds(content).height > 20 * density)
                val rootBounds = bounds("workspace")
                val offsetY = point.y / density - host.workspaceGeometry!!.bounds!!.top
                for (y in listOf(rootBounds.bottom - 30 * density, rootBounds.bottom - 2 * density)) {
                    event(MotionEvent.ACTION_MOVE, Offset(rootBounds.center.x, y)); settle()
                    instrumentation.runOnMainSync {
                        val moving = host.workspaceGeometry!!
                        val preview = moving.bounds!!
                        assertEquals("$panel $pointer: contact follows the lower edge", y / density - offsetY, preview.top, 1f)
                        assertEquals("$panel $pointer: preview keeps source height", sourceHeight, preview.height, 1f)
                        val node = find("group-${moving.group}")!!
                        assertEquals("Native allocation stays full while clipped", preview.height * density, node.size.height.toFloat(), 1f)
                        assertTrue("Preview extends beyond workspace", preview.bottom * density > rootBounds.bottom)
                    }
                }
                event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(before, workspace())
                event(MotionEvent.ACTION_DOWN, bounds(if (wholeGroup) "group-grip-43" else "tab-$panel").center)
                event(MotionEvent.ACTION_MOVE, Offset(rootBounds.center.x, rootBounds.bottom - 2 * density)); settle()
                event(MotionEvent.ACTION_UP); settle(); settle()
                assertTrue(group(panel).optBoolean("floating"))
                val floating = workspace()
                val floatingId = group(panel).getInt("id")
                val settledGroup = group(panel)
                val placed = settledGroup.getJSONObject("bounds").rect()
                val measured = snapshot().array("panel_measurements").objects().first { it.getString("panel") == panel }
                val chrome = if (settledGroup.getBoolean("tabs_visible")) 36f else settledGroup.getJSONObject("footer_grip").number("height")
                val natural = measured.number("content_height") + chrome
                val scroll = measured.objectOrNull("scroll")
                val minimum = if (scroll == null) natural else minOf(natural, chrome + scroll.number("fixed_height") + 4 * scroll.number("unit_height").takeIf { it > 0f }.let { it ?: 36f })
                assertEquals("$panel $pointer whole=$wholeGroup: useful height from $measured", minimum, placed.height, 1f)
                assertTrue("Dropped panel fits above the screen bottom", placed.bottom * density <= rootBounds.bottom + 1f)
                if (panel == "layers") assertTrue("Short layer list fits its rows", placed.height < 300f)
                if (panel == "color") assertEquals("Color uses its full-width square", placed.width, measured.number("content_height"), 1f)
                if (pointer == MotionEvent.TOOL_TYPE_MOUSE && wholeGroup) {
                    val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/drop-$panel.png")
                    file.parentFile!!.mkdirs()
                    instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
                        file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                        bitmap.recycle()
                    }
                }

                val edge = bounds("resize-$floatingId-right")
                assertTrue("Floating side target is at least 12dp", edge.width >= 12 * density - 1)
                val press = Offset(edge.right - 2 * density, edge.center.y)
                event(MotionEvent.ACTION_DOWN, press)
                event(MotionEvent.ACTION_MOVE, press + Offset(40 * density, 0f)); settle()
                assertNotEquals("Outer part of the widened handle resizes", floating, workspace())
                event(MotionEvent.ACTION_CANCEL); settle(); assertEquals(floating, workspace())
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
            }
        val layout = fixture.getJSONObject("layout")
        val band = layout.array("bands").getJSONObject(1)
        val originalRoot = band.getJSONObject("root")
        val colorConfig = layout.array("panels").objects().first { it.getString("id") == "color" }
        val hidden = colorConfig.optBoolean("hide_tab")
        try {
            for (pointer in pointerTools) for (case in listOf("squashed", "usable", "footer", "navigator-squashed")) {
                tool = pointer
                band.put("root", originalRoot); colorConfig.put("hide_tab", hidden); restore()
                val usableHeight = bounds("workspace").height / density - 48f - 6f - layout.number("bottom_inset")
                val budget = minOf(400f, usableHeight * .5f)
                val panel = when(case) { "footer" -> "color"; "navigator-squashed" -> "navigator"; else -> "adjustments" }
                val first = obj("kind" to "tabs", "id" to 43, "panels" to JSONArray(listOf(panel)), "active" to panel, "tab_style" to "icon")
                if (case == "footer") {
                    band.put("root", first); colorConfig.put("hide_tab", true)
                } else {
                    val requested = if (case.endsWith("squashed")) 120f else budget - 4f
                    band.put("root", obj("kind" to "split", "id" to 46, "axis" to "vertical", "fraction" to (requested / usableHeight),
                        "first" to first, "second" to obj("kind" to "tabs", "id" to 47, "panels" to JSONArray(listOf("layers")), "active" to "layers", "tab_style" to "icon")))
                    layout.put("next_id", maxOf(48, layout.getInt("next_id")))
                }
                restore()
                val before = workspace()
                val source = group(panel).getJSONObject("bounds").rect()
                val workspaceBounds = bounds("workspace")
                event(MotionEvent.ACTION_DOWN, bounds("group-grip-43").center)
                event(MotionEvent.ACTION_MOVE, if (case == "footer") Offset(workspaceBounds.center.x, workspaceBounds.bottom - 2*density) else Offset(workspaceBounds.center.x, 140*density))
                settle()
                assertEquals("$case: preview preserves source size", source.height, host.workspaceGeometry!!.bounds!!.height, 1f)
                event(MotionEvent.ACTION_UP); settle(); settle()
                val final = group(panel).getJSONObject("bounds").rect()
                val navigator = snapshot().array("panel_measurements").objects().first { it.getString("panel") == "navigator" }.number("content_height") + 36f
                val expected = when (case) { "squashed" -> budget; "usable" -> source.height; "navigator-squashed" -> navigator; else -> final.width + 20f }
                assertEquals("$pointer $case: sensible drop height", expected, final.height, 1.5f)
                val committed = workspace()
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, workspace())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(committed, workspace())
            }
        } finally { band.put("root", originalRoot); colorConfig.put("hide_tab", hidden) }
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
            val tile=host.snapshot!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").getJSONObject(0).getInt("id")
            tap(bounds("tile-toolbar-$tile").center)
            waitFor("Secondary tile click opens menu") { popupCount()==1 }; back()
        } finally { mouseButton=MotionEvent.BUTTON_PRIMARY }
    }

    @Test fun collapsedIconsKeepTheirSourceWhenDrawerTabsAreVisible() {
        for(pointer in pointerTools) for(panel in listOf("brushes","sizes")) {
            tool=pointer; restore()
            customize(obj("type" to "set_column_collapsed","group" to 41,"collapsed" to true))
            customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
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
        val tiles=host.snapshot!!.array("panels").objects().first { it.getString("id")=="toolbar" }.array("tiles").objects()
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
    private fun switchToolbarWorkspace(id: String) {
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:$id")) }
        waitFor("switch $id", 30000) { host.workspaceManager?.let { it.optString("id") == "builtin:workspace:$id" && !it.optBoolean("busy") } == true }
        settle()
    }
    private fun toolbarComponent(kind: String): Pair<String, Int> = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects()
        .firstNotNullOf { panel -> panel.getJSONObject("content").optJSONArray("tiles")?.objects()?.firstOrNull { it.getJSONObject("control").getString("kind") == kind }
            ?.let { panel.getString("id") to it.getInt("id") } }
    private fun captureToolbar(name: String) {
        val directory = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/toolbar-components").apply { mkdirs() }
        instrumentation.uiAutomation.takeScreenshot()?.let { image ->
            File(directory, "$name.png").outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }; image.recycle()
        }
    }
    @Test fun toolbarComponentsAcrossDevicesAndLayouts() {
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        switchToolbarWorkspace("photographer"); invoke("brush")
        waitFor("inline size") { exists("number-value-toolbar-size") }
        val original = bounds("number-value-toolbar-size")
        for (value in listOf(.5f, 31.9f, 32f, 2048f)) {
            action(obj("type" to "set_tool_setting", "id" to "size", "value" to value))
            assertEquals("Fixed readout width", original, bounds("number-value-toolbar-size"))
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); invoke("rectangle_select")
            for ((i, device) in pointerTools.withIndex()) {
                tool = device; tap(bounds("toolbar-segment-selection-mode-${i + 1}").center)
                val command = listOf("selection_add", "selection_subtract", "selection_intersect")[i]
                waitFor("$command selected") { state().array("commands").objects().any { it.getString("id") == command && it.getBoolean("selected") } }
            }
            val segments = bounds("toolbar-segments-selection-mode"); val choice = bounds("toolbar-choice-variant")
            assertEquals("Segments match the dropdown height", 24 * density, segments.height, 1f)
            assertEquals("Segments center with the dropdown", choice.center.y, segments.center.y, 1f)
            captureToolbar("photo-$theme")
        }
        invoke("auto_select"); waitFor("list choice remains") { exists("toolbar-choice-selection-source") }
        val options = toolbarComponent("tool_options")
        for (edge in listOf("top", "bottom", "left", "right")) for (alignment in listOf("start", "center", "end")) {
            action(obj("type" to "move_panel", "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density)), "panel" to options.first, "target" to obj("kind" to "compact_edge", "edge" to edge, "alignment" to alignment)))
            assertTrue(state().getJSONObject("workspace").getJSONObject("layout").array("bands").objects().any { it.optString("alignment") == alignment && it.getString("edge") == edge })
        }
        action(obj("type" to "move_panel", "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density)), "panel" to options.first, "target" to obj("kind" to "edge", "edge" to "left", "outer" to true)))
        invoke("brush"); captureToolbar("vertical-options")
        switchToolbarWorkspace("painter"); invoke("brush")
        val size = toolbarComponent("brush_size_slider")
        for (device in pointerTools) {
            tool = device
            action(obj("type" to "set_tool_setting", "id" to "size", "value" to 5f))
            val slider = bounds("component-slider-${size.second}")
            val start = Offset(slider.center.x, slider.top + slider.height * .8f)
            val end = Offset(slider.center.x, slider.top + slider.height * .2f)
            event(MotionEvent.ACTION_DOWN, start); SystemClock.sleep(30)
            event(MotionEvent.ACTION_MOVE, end); SystemClock.sleep(80); event(MotionEvent.ACTION_UP); settle()
            waitFor("slider $device") { state().getJSONObject("brush").number("diameter") > 5f }
        }
        captureToolbar("sketch-sliders")
        val presented = group(size.first).getJSONObject("tiles")
        val undo = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects()
            .first { it.getString("id") == size.first }.getJSONObject("content").array("tiles").objects()
            .first { it.getJSONObject("control").optString("command") == "undo" }.getInt("id")
        assertEquals("Tile icons follow the presented tile size", presented.number("tile_icon_size") * density,
            bounds("tile-icon-${size.first}-$undo").width, 1f)
        val viewport = JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))
        for (device in pointerTools) for (edge in listOf("left", "right", "top", "bottom")) for (alignment in listOf("start", "center", "end")) {
            tool = device
            val workspaceBounds = bounds("workspace")
            action(obj("type" to "move_panel", "panel" to size.first, "viewport" to viewport,
                "target" to obj("kind" to "float", "position" to JSONArray(listOf(workspaceBounds.width / density / 2 - 120, workspaceBounds.height / density / 2 - 120)))))
            val top = bounds("title-bar").bottom
            val horizontal = edge == "top" || edge == "bottom"
            val along = when (alignment) {
                "start" -> (if (horizontal) workspaceBounds.left else top) + 70 * density
                "end" -> (if (horizontal) workspaceBounds.right else workspaceBounds.bottom) - 70 * density
                else -> if (horizontal) workspaceBounds.center.x else (top + workspaceBounds.bottom) / 2
            }
            val destination = if (horizontal) Offset(along, if (edge == "top") top + 3 * density else workspaceBounds.bottom - 3 * density)
                else Offset(if (edge == "left") workspaceBounds.left + 3 * density else workspaceBounds.right - 3 * density, along)
            event(MotionEvent.ACTION_DOWN, bounds("ribbon-grip-${size.first}").center)
            event(MotionEvent.ACTION_MOVE, workspaceBounds.center); settle()
            event(MotionEvent.ACTION_MOVE, destination); settle()
            val target = host.workspaceGeometry?.hint?.getJSONObject("target") ?: run { captureToolbar("missing-target"); error("Missing $device/$edge/$alignment target; focus=${owner.view.hasWindowFocus()} dragging=${workspaceDragging()} geometry=${host.workspaceGeometry} grip=${bounds("ribbon-grip-${size.first}")} destination=$destination") }
            assertEquals("compact_edge", target.getString("kind")); assertEquals(edge, target.getString("edge")); assertEquals(alignment, target.getString("alignment"))
            event(MotionEvent.ACTION_UP); settle()
            assertTrue(state().getJSONObject("workspace").getJSONObject("layout").array("bands").objects().any { it.optString("alignment") == alignment && it.getString("edge") == edge })
            invoke("undo_workspace")
            assertTrue(state().getJSONObject("workspace").getJSONObject("layout").array("floating").length() > 0)
            invoke("redo_workspace")
        }

    }

    @Test fun toolbarEditorsAndOverflow() {
        switchToolbarWorkspace("photographer")
        action(obj("type" to "invoke", "command" to "brush"))
        val options = toolbarComponent("tool_options")
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap(bounds("number-value-toolbar-size").center)
        waitFor("inline toolbar editor") { exists("number-Brush size") }
        tap(bounds("workspace").center)
        waitFor("outside tap finishes editor") { !exists("number-Brush size") }
        tap(bounds("toolbar-more-${options.second}").center)
        waitFor("complete tool drawer") { exists("tool-drawer") }; settle()
        captureToolbar("options-drawer")
        customize(obj("type" to "close_expanded"))
        val viewport = JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))
        action(obj("type" to "move_panel", "panel" to options.first, "viewport" to viewport,
            "target" to obj("kind" to "edge", "edge" to "left", "outer" to true)))
        action(obj("type" to "set_tool_setting", "id" to "size", "value" to 2048))
        for (style in listOf("small", "medium", "large", "medium_labeled", "labeled")) {
            customize(obj("type" to "set_tile_style", "panel" to options.first, "style" to style))
            if (style == "small" || style == "medium") waitFor("vertical field $style") { exists("toolbar-setting-size") }
            else {
                settle()
                if (!exists("toolbar-setting-size")) {
                    tap(bounds("toolbar-more-${options.second}").center)
                    waitFor("overflowing field $style reachable") { exists("tool-drawer") }
                    customize(obj("type" to "close_expanded"))
                    waitFor("overflow closed $style") { !exists("tool-drawer") }
                }
            }
            captureToolbar("options-$style")
        }
        customize(obj("type" to "set_tile_style", "panel" to options.first, "style" to "small"))
        waitFor("vertical field") { exists("toolbar-setting-size") }
        tap(bounds("toolbar-setting-size").center); settle()
        captureToolbar("vertical-value-popup")
        instrumentation.runOnMainSync { assertTrue("The value popup leaves window focus with the canvas", owner.view.hasWindowFocus()) }
        popupInput = true
        tap(bounds("number-value-Brush size").center)
        waitFor("typing moves focus into the value popup") { exists("number-Brush size") && !owner.view.hasWindowFocus() }
        instrumentation.sendStringSync("123"); instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
        waitFor("typed value applies") { state().getJSONObject("brush").number("diameter") == 123f }
        popupInput = false
        action(obj("type" to "invoke", "command" to "eraser")); settle()
        assertTrue(owner.view.hasWindowFocus())
        switchToolbarWorkspace("painter"); action(obj("type" to "invoke", "command" to "brush"))
        val size = toolbarComponent("brush_size_slider")
        popupInput = true
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            val slider = bounds("component-slider-${size.second}")
            val mark = slider.center - Offset(0f, 23*density)
            val near = mark + Offset(0f, 16*density)
            val far = mark + Offset(0f, 24*density)
            tap(mark)
            waitFor("tap retains stamp $device") { exists("brush-slider-preview") }
            val saved = state().getJSONObject("brush").number("diameter")
            tap(bounds("slider-bookmark").center); settle()
            captureToolbar("stamp-$device")
            action(obj("type" to "set_tool_setting", "id" to "size", "value" to 2048f))
            captureToolbar("stamp-large-$device")
            action(obj("type" to "set_tool_setting", "id" to "size", "value" to 3f))
            tap(far); settle()
            assertTrue("Distant tap does not snap $device", saved != state().getJSONObject("brush").number("diameter"))
            tap(near); settle()
            assertEquals("Nearby tap recalls exact bookmark $device", saved, state().getJSONObject("brush").number("diameter"), .001f)
            event(MotionEvent.ACTION_DOWN, far)
            event(MotionEvent.ACTION_MOVE, near); settle()
            event(MotionEvent.ACTION_UP); settle()
            assertTrue("Dragging near bookmark does not snap $device", saved != state().getJSONObject("brush").number("diameter"))
            tap(near); settle()
            assertEquals(saved, state().getJSONObject("brush").number("diameter"), .001f)
            captureToolbar("bookmark-centered-$device")
            tap(bounds("slider-bookmark").center); settle()
            tap(bounds("workspace").center)
            waitFor("outside tap dismisses stamp") { !exists("brush-slider-preview") }
            event(MotionEvent.ACTION_DOWN, slider.center)
            event(MotionEvent.ACTION_MOVE, slider.center - Offset(0f,20*density)); settle()
            waitFor("drag shows stamp") { exists("brush-slider-preview") }
            event(MotionEvent.ACTION_UP); settle()
            waitFor("lift dismisses stamp") { !exists("brush-slider-preview") }
            val value = state().getJSONObject("brush").number("diameter")
            event(MotionEvent.ACTION_DOWN, bounds("slider-cap-${size.second}").center)
            SystemClock.sleep(700)
            event(MotionEvent.ACTION_MOVE, bounds("workspace").center); settle()
            assertEquals("Held cap reorders without editing", value, state().getJSONObject("brush").number("diameter"), .01f)
            event(MotionEvent.ACTION_CANCEL); settle()
        }

        for (edge in listOf("left", "right", "top", "bottom")) {
            action(obj("type" to "move_panel", "panel" to size.first, "viewport" to viewport,
                "target" to obj("kind" to "compact_edge", "edge" to edge, "alignment" to "center")))
            settle()
            for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = device
                tap(bounds("component-slider-${size.second}").center)
                waitFor("edge preview $edge/$device") { exists("brush-slider-preview") }; settle()
                val toolbar = bounds("panel-body-${size.first}")
                val preview = bounds("brush-slider-preview")
                val gap = when (edge) {
                    "left" -> preview.left-toolbar.right
                    "right" -> toolbar.left-preview.right
                    "top" -> preview.top-toolbar.bottom
                    else -> toolbar.top-preview.bottom
                } / density
                assertTrue("Preview clears toolbar $edge/$device: $gap", gap >= 7f && gap <= 16f)
                captureToolbar("preview-$edge-$device")
                tap(bounds("workspace").center)
                waitFor("edge preview dismissed") { !exists("brush-slider-preview") }
            }
        }

    }

    @Test fun tonalRangePanelsAndToolbarAcrossDevices() {
        waitFor("document commands ready",120_000) {
            state().array("commands").objects().any { it.optString("id")=="open_document" && it.optBoolean("enabled") }
        }
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun values() = listOf("tonal_lower", "tonal_upper").map { id -> state().array("tool_settings").objects().first { it.getString("id")==id }.number("value") }
        fun number(prefix: String, id: String, text: String) {
            tool=MotionEvent.TOOL_TYPE_MOUSE
            tap(bounds("number-value-$prefix-$id").center);settle()
            instrumentation.sendStringSync(text)
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER);settle()
        }
        fun handles(prefix: String) {
            val beforeWorkspace=workspace()
            for(device in pointerTools) for(index in 0..1) {
                tool=device
                val before=values()
                val id=listOf("tonal_lower","tonal_upper")[index]
                val p=bounds("$prefix-range-handle-$id").center+Offset(if(index==0)-4*density else 4*density,0f)
                event(MotionEvent.ACTION_DOWN,p);SystemClock.sleep(24)
                event(MotionEvent.ACTION_MOVE,p+Offset(if(index==0)-8*density else 8*density,0f));SystemClock.sleep(80)
                event(MotionEvent.ACTION_UP);settle()
                waitFor("$prefix $device endpoint $index") { if(index==0)values()[0]<before[0] else values()[1]>before[1] }
                assertEquals(before[1-index],values()[1-index])
                assertEquals("Range contact cannot reorder",beforeWorkspace,workspace())
            }
        }
        switchToolbarWorkspace("photographer")
        val options=toolbarComponent("tool_options")
        // Give the complete form a lane on the tablet's 1200dp viewport.
        val w=JSONObject(workspace())
        w.getJSONObject("layout").array("panels").objects().first { it.getString("id")==options.first }.getJSONObject("content").let { content ->
            content.put("tiles",JSONArray(content.array("tiles").objects().filter { it.getJSONObject("control").getString("kind")=="tool_options" }))
        }
        action(obj("type" to "restore_workspace","workspace" to w))
        invoke("tonal_select")
        assertEquals(6,state().array("tool_extra").getJSONObject(0).getJSONObject("Choice").array("items").length())
        tap(bounds("toolbar-segment-tonal-tones-4").center)
        waitFor("immediate highlight selection",30_000) { state().getJSONObject("layer_tools").getBoolean("has_selection") }
        assertFalse(state().getJSONObject("layer_tools").getBoolean("quick_mask"))
        assertEquals(24*density,bounds("toolbar-segments-tonal-tones").height,1f)
        invoke("quick_mask")
        assertTrue(state().getJSONObject("layer_tools").getBoolean("quick_mask"))
        captureToolbar("tonal-quick-mask")
        instrumentation.uiAutomation.takeScreenshot()!!.let { image ->
            val p=bounds("workspace").center;val origin=IntArray(2);instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
            val color=image.getPixel((p.x+origin[0]).toInt(),(p.y+origin[1]).toInt());image.recycle()
            assertTrue("GPU highlight mask shades white artwork",android.graphics.Color.red(color)>android.graphics.Color.green(color)+30)
        }
        tap(bounds("toolbar-segment-tonal-tones-5").center);settle()
        waitFor("custom inline interval") { exists("toolbar-range-track") }
        assertEquals(28*density,bounds("toolbar-range-tonal").height,1f)
        assertTrue(bounds("toolbar-range-track").width>=180*density)
        handles("toolbar")
        val keyboardBefore=values()
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DPAD_RIGHT);settle()
        assertEquals(keyboardBefore[0],values()[0]);assertEquals(keyboardBefore[1]+.1f,values()[1],.001f)
        assertEquals("Keyboard retains the tonal tool", "tonal", state().getJSONObject("layer_tools").getJSONObject("tool").getJSONObject("selection").getString("kind"))
        number("toolbar","tonal_lower","-20.0");number("toolbar","tonal_upper","12.0")
        assertEquals(listOf(-20f,12f),values())
        number("toolbar","tonal_lower","13");assertEquals(listOf(12f,12f),values())
        number("toolbar","tonal_lower","-7.2");number("toolbar","tonal_upper","2.3")
        val before=values();val p=bounds("toolbar-range-handle-tonal_lower").center-Offset(4*density,0f)
        event(MotionEvent.ACTION_DOWN,p);event(MotionEvent.ACTION_MOVE,p-Offset(10*density,0f));settle()
        event(MotionEvent.ACTION_CANCEL);settle();assertEquals("Cancellation restores endpoint",before,values())
        for(theme in listOf("light","dark")) { action(obj("type" to "set_theme","theme" to theme));captureToolbar("tonal-toolbar-$theme") }
        tap(bounds("toolbar-more-${options.second}").center)
        waitFor("range in complete panel") { exists("tool-range-track") };settle()
        assertEquals(36*density,bounds("tool-segments-tonal-tones").height,1f)
        assertEquals(28*density,bounds("tool-range-tonal").height,1f)
        handles("tool")
        for(id in listOf("tonal_lower","tonal_upper"))assertTrue("Compact endpoint",bounds("tool-setting-$id").width<=48*density)
        for(theme in listOf("light","dark")) { action(obj("type" to "set_theme","theme" to theme));captureToolbar("tonal-panel-$theme") }
        customize(obj("type" to "close_expanded"))
        invoke("save_selection_layer");action(obj("type" to "layer","action" to obj("op" to "cancel_rename")))
        assertTrue(state().array("layers").objects().any { it.optBoolean("selection_layer") })
        tap(bounds("toolbar-segment-tonal-tones-4").center);settle()
        assertNotNull(state().getJSONObject("layer_tools").objectOrNull("mask_editing"))
        invoke("return_to_artwork");invoke("undo");invoke("redo")
        invoke("brush");assertFalse(exists("toolbar-range-track"))
        println("PASS Huion tonal interval: mouse/finger/stylus, native numbers, cancellation, compact panel/toolbar, GPU Quick Mask and saved masks")
    }

    private fun canvasBar() = state().optJSONObject("canvas_bar")
    private fun barKind() = canvasBar()?.getJSONObject("context")?.getString("kind")
    private fun nativeGlassRegions() = kotlinx.coroutines.runBlocking {
        host.withNative { JSONObject(Native.displayStatus(it)).optInt("glass_regions", -1) }
    }
    private fun selectionPreviews() = kotlinx.coroutines.runBlocking {
        host.withNative { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())).optLong("selection_previews") }
    }
    private fun onMain(block: () -> Unit) = if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) block() else instrumentation.runOnMainSync(block)
    private fun glassBoxes(): Int { var count = 0; onMain { count = host.glassBoxesForTest.size }; return count }
    private fun textBounds(text: String): Rect? {
        var result: Rect? = null
        onMain {
            findNode(hasLabel(text), owner)?.let { (root, node) ->
                val base = IntArray(2); owner.view.getLocationOnScreen(base)
                val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                result = node.boundsInRoot.translate(Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat()))
            }
        }
        return result
    }
    private fun drag(from: Offset, to: Offset, steps: Int = 8, hold: () -> Unit = {}) {
        event(MotionEvent.ACTION_DOWN, from)
        for (i in 1..steps) { SystemClock.sleep(16); event(MotionEvent.ACTION_MOVE, from + (to - from) * (i / steps.toFloat())) }
        hold()
        event(MotionEvent.ACTION_UP)
    }
    private fun captureCanvasBar(name: String, directory: String = "canvas-bar", check: (android.graphics.Bitmap, Offset) -> Unit = { _, _ -> }) {
        SystemClock.sleep(400); settle()
        val image = instrumentation.uiAutomation.takeScreenshot()
        val origin = IntArray(2)
        instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
        try {
            val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/$directory/$name.png")
            file.parentFile!!.mkdirs(); file.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
            check(image, Offset(origin[0].toFloat(), origin[1].toFloat()))
        } finally { image.recycle() }
    }

    @Test fun canvasNoticesExplainRefusalsAcrossDevices() {
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun layer(value: JSONObject) = action(obj("type" to "layer", "action" to value))
        fun notice() = state().optJSONObject("notice")
        fun active() = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        fun layers() = state().array("layers").objects()
        fun idle(label: String) = waitFor("$label: the canvas interaction finishes", 5_000) {
            state().array("commands").objects().any { it.getString("id") == "add_layer" && it.getBoolean("enabled") }
        }
        fun modeless(label: String) = onMain {
            assertNull("$label opens no dialog", host.dialogError); assertNull("$label opens no dialog", host.hostError)
            assertTrue("$label leaves window focus with the canvas", owner.view.hasWindowFocus())
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                restore()
                layer(obj("op" to "new", "group" to false, "clipped" to false))
                val top = active()
                invoke("fit_canvas")
                val work = bounds("workspace")
                val point = Offset(work.left + 720 * density, work.top + 300 * density)
                tool = device
                val drawing = if (device == MotionEvent.TOOL_TYPE_FINGER) MotionEvent.TOOL_TYPE_STYLUS else device
                fun onCanvas(gesture: () -> Unit) { tool = drawing; try { gesture() } finally { tool = device } }
                invoke("auto_select"); invoke("selection_reference")
                for (theme in if (device == MotionEvent.TOOL_TYPE_STYLUS) listOf("light", "dark") else listOf(null)) {
                    theme?.let { action(obj("type" to "set_theme", "theme" to it)) }
                    val before = notice()?.optLong("id") ?: 0L
                    onCanvas { tap(point) }
                    waitFor("$name Wand offers a reference", 5_000) {
                        (notice()?.optLong("id") ?: 0L) > before && notice()!!.optJSONObject("action") != null && shown("canvas-notice-action")
                    }
                    theme?.let { captureCanvasBar("wand-$it", "canvas-notice") }
                }
                val offer = notice()!!
                assertEquals("This tool samples reference layers, and none is marked", offer.getString("text"))
                assertNotNull("$name the notice shows the core's text", textBounds(offer.getString("text")))
                val label = offer.getJSONObject("action").getString("label")
                assertNotNull("$name the notice shows its action", textBounds(label))
                val below = layers().first { "Use ${it.getString("label")} as Reference" == label }.getLong("id")
                modeless("$name Wand notice")
                tap(bounds("canvas-notice-action").center)
                waitFor("$name the action marks the layer below as a reference", 5_000) {
                    notice() == null && !shown("canvas-notice") && layers().first { it.getLong("id") == below }.getBoolean("reference")
                }
                assertEquals("$name the active layer stays", top, active())
                modeless("$name accepted notice")
                onCanvas { tap(point) }
                waitFor("$name the canvas keeps rendering and the Wand samples the reference", 10_000) { barKind() == "selection" && shown("canvas-action-bar") }
                assertNull("$name sampling a reference raises nothing", notice())

                idle(name)
                layer(obj("op" to "lock", "id" to top, "value" to true))
                invoke("move")
                val shift = Offset(40 * density, 24 * density)
                onCanvas { drag(point, point + shift) }
                waitFor("$name Move on a locked layer explains", 5_000) { notice()?.optString("text") == "The active layer is locked" && shown("canvas-notice") }
                val raised = bounds("canvas-notice")
                assertTrue("$name the refusal has no action", notice()!!.isNull("action") && !exists("canvas-notice-action"))
                modeless("$name Move notice")
                waitFor("$name the selection bar returns beside the notice", 3_000) { shown("canvas-action-bar") }
                val bar = bounds("canvas-action-bar"); val bubble = bounds("canvas-notice")
                assertEquals("$name the notice keeps its place when the bar returns", raised, bubble)
                assertFalse("$name the notice never covers the bar: $bubble vs $bar", bubble.overlaps(bar))
                if (bar.bottom > work.bottom - 120 * density) assertTrue("$name the notice sits above a bottom-edge bar: $bubble vs $bar", bubble.bottom <= bar.top)
                if (device == MotionEvent.TOOL_TYPE_STYLUS) captureCanvasBar("move-locked-dark", "canvas-notice")
                val disabled = (canvasBar()!!.array("items").objects() + canvasBar()!!.array("completion").objects())
                    .mapNotNull { it.getJSONObject("option").optJSONObject("Action")?.getJSONObject("state") }
                    .firstOrNull { !it.getBoolean("enabled") && shown("canvas-bar-action-${it.getString("id")}") }
                    ?: throw AssertionError("$name a locked layer disables a bar item: ${canvasBar()}")
                val reason = disabled.getString("disabled_reason")
                val item = bounds("canvas-bar-action-${disabled.getString("id")}")
                fun reasonShown() = findTag("hover-tooltip") != null && textBounds(reason) != null
                tap(item.center)
                waitFor("$name a tap shows why ${disabled.getString("id")} is disabled", 3_000, ::reasonShown)
                if (device != MotionEvent.TOOL_TYPE_MOUSE) {
                    waitFor("$name the reason hides", 5_000) { findTag("hover-tooltip") == null }
                    event(MotionEvent.ACTION_DOWN, item.center)
                    waitFor("$name a hold shows the reason", 3_000, ::reasonShown)
                    event(MotionEvent.ACTION_UP)
                    assertEquals("$name a hold opens nothing else", 0, popupCount())
                    modeless("$name disabled item hold")
                }
                val first = notice()?.optLong("id") ?: 0L
                onCanvas { drag(point, point + shift) }
                waitFor("$name a repeated refusal shows again", 5_000) { (notice()?.optLong("id") ?: 0) > first && shown("canvas-notice") }

                idle(name)
                invoke("hand")
                tap(point)
                waitFor("$name the next canvas contact hides the notice", 3_000) { notice() == null && !shown("canvas-notice") }

                invoke("move")
                onCanvas { drag(point, point + shift) }
                waitFor("$name the refusal shows before its timeout", 5_000) { shown("canvas-notice") }
                val shownAt = SystemClock.uptimeMillis()
                waitFor("$name the notice times out", 8_000) { notice() == null && !shown("canvas-notice") }
                assertTrue("$name the notice stays about 4 s", SystemClock.uptimeMillis() - shownAt in 3_000..6_000)

                idle(name)
                layer(obj("op" to "lock", "id" to top, "value" to false))
                layer(obj("op" to "reference", "id" to below))
                invoke("deselect")
                println("PASS canvas notice device=$name")
            }
        } finally {
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS canvas notices: Wand reference offer and action, Move on a locked layer, repeat, contact and timeout dismissal, bar clearance, no dialog or focus change")
    }
    private fun zoomMenuRoot(): ViewRootForTest? = semanticsRoots().firstOrNull { it.find(hasTag("zoom-menu")) != null }
    private fun zoomMenuShown(): Boolean { var open = false; onMain { open = zoomMenuRoot() != null }; return open }
    private fun zoomItem(text: String): Rect? {
        var result: Rect? = null
        onMain {
            zoomMenuRoot()?.let { root ->
                root.find(hasLabel(text))?.let { node ->
                    val base = IntArray(2); owner.view.getLocationOnScreen(base)
                    val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                    result = node.boundsInRoot.translate(Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat()))
                }
            }
        }
        return result
    }
    @Test fun zoomReadoutMenuAndFieldAcrossDevices() {
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun camera() = state().getJSONObject("camera")
        fun zoom() = camera().getDouble("zoom")
        fun whole() = camera().getJSONArray("translation").let { t -> (0 until t.length()).all { t.getDouble(it) % 1.0 == 0.0 } }
        fun readout(text: String) = host.cameraReadout.let { "${it.zoomPercent}% · ${it.rotationDegrees}°" } == text && textBounds(text) != null
        fun canvasFocus(label: String) = onMain { assertTrue("$label leaves window focus with the canvas", owner.view.hasWindowFocus()) }
        fun open(name: String) {
            tap(bounds("camera-readout").center)
            waitFor("$name opens the zoom menu", 5_000) { zoomMenuShown() && zoomItem("200%") != null && zoomItem("Actual Pixels") != null }
            val settled = SystemClock.uptimeMillis() + 3_000
            var placed = bounds("number-value-Zoom")
            while (true) {
                SystemClock.sleep(60)
                val now = bounds("number-value-Zoom")
                if (now == placed) break
                placed = now
                check(SystemClock.uptimeMillis() < settled) { "$name the zoom menu finishes opening" }
            }
            canvasFocus("$name opening the zoom menu")
        }
        val footer = JSONObject(fixture.toString()).apply { getJSONObject("layout").put("canvas_info", obj("visible" to true)) }
        action(obj("type" to "restore_workspace", "workspace" to footer))
        waitFor("the zoom readout shows", 5_000) { shown("camera-readout") }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                tool = device
                action(obj("type" to "set_zoom", "zoom" to .37))
                open(name)
                if (device == MotionEvent.TOOL_TYPE_STYLUS) for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    waitFor("$name the menu stays open across themes", 3_000) { zoomMenuShown() }
                    captureCanvasBar("zoom-menu-$theme", "zoom-readout")
                }
                tap(zoomItem("200%")!!.center)
                waitFor("$name 200% applies and closes the menu", 5_000) { zoom() == 2.0 && !zoomMenuShown() }
                assertTrue("$name 200% lands on whole device pixels", whole())
                waitFor("$name the readout follows the camera", 3_000) { readout("200% · 0°") }
                canvasFocus("$name choosing 200%")

                invoke("rotate_right"); action(obj("type" to "set_zoom", "zoom" to .37))
                open(name)
                tap(zoomItem("Actual Pixels")!!.center)
                waitFor("$name Actual Pixels applies", 5_000) { zoom() == 1.0 && !zoomMenuShown() }
                assertTrue("$name a quarter-turned 1:1 view lands on whole device pixels", whole())
                waitFor("$name the readout shows the turned 1:1 view", 3_000) { readout("100% · 90°") }
                canvasFocus("$name Actual Pixels")
                invoke("rotate_left")

                open(name)
                tap(bounds("number-value-Zoom").center)
                waitFor("$name the typed field takes the keys", 5_000) {
                    var typing = false; onMain { typing = zoomMenuRoot()?.let { it.view.hasWindowFocus() && it.find(hasTag("number-Zoom")) != null } == true }; typing
                }
                instrumentation.sendStringSync("50")
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
                waitFor("$name a typed percentage applies", 5_000) { zoom() == 0.5 }
                assertTrue("$name a typed zoom lands on whole device pixels", whole())
                assertTrue("$name typing keeps the menu open", zoomMenuShown())
                waitFor("$name the readout shows the typed zoom", 3_000) { readout("50% · 0°") }
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                waitFor("$name Back closes the menu and hands focus back", 5_000) { !zoomMenuShown() && owner.view.hasWindowFocus() }
                assertNull(host.actionError)
                println("PASS zoom readout device=$name")
            }
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS zoom readout: shared menu levels, Actual Pixels at a quarter turn, typed zoom and Back with mouse, finger and stylus, without taking focus")
    }
    @Test fun canvasActionBarJourneysAcrossDevices() {
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun enabled(command: String) = state().array("commands").objects().any { it.getString("id") == command && it.getBoolean("enabled") }
        fun clear() {
            if (enabled("cancel_transform")) invoke("cancel_transform")
            if (enabled("deselect")) invoke("deselect")
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val originalTransparency = transparency()
        fixture.getJSONObject("layout").put("bands", JSONArray(fixture.getJSONObject("layout").array("bands").objects().filter { it.getInt("id") == 44 }))
        fun choice(id: String) = canvasBar()?.array("items")?.objects()?.firstNotNullOfOrNull { item ->
            item.getJSONObject("option").optJSONObject("Choice")?.takeIf { it.getString("id") == id }
        }
        fun chosen(id: String) = choice(id)?.array("items")?.objects()?.firstOrNull { it.getBoolean("selected") }?.getString("label")
        fun onDocument(x: Double, y: Double) = state().array("tabs").getJSONObject(0).let { documentPoint(it.getInt("width") * x, it.getInt("height") * y) }
        fun corner() = canvasBar()!!.getJSONArray("anchor").let { documentPoint(it.getDouble(2), it.getDouble(3)) }
        val devices = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)
        popupInput = true
        try {
            for (device in devices) {
                restore()
                clear(); invoke("fit_canvas"); invoke("rectangle_select")
                waitFor("no bar without a selection") { canvasBar() == null && !shown("canvas-action-bar") }
                val glassBefore = glassBoxes()
                val work = bounds("workspace")
                val selection = Rect(onDocument(.38, .22), onDocument(.62, .42))
                tool = MotionEvent.TOOL_TYPE_STYLUS
                drag(selection.topLeft, selection.bottomRight)
                waitFor("$device selection bar", 5_000) { barKind() == "selection" && shown("canvas-action-bar") }
                invoke("fill_selection")
                settle()
                var bar = bounds("canvas-action-bar")
                assertTrue("$device selection bar sits below the selection: $bar vs $selection", bar.top >= selection.bottom && bar.top - selection.bottom < 60 * density)
                assertEquals("$device bar centres on the selection", selection.center.x, bar.center.x, 2 * density)
                waitFor("$device glass registered once", 5_000) { glassBoxes() == glassBefore + 1 }
                val nativeShown = nativeGlassRegions()

                tool = device
                val selectionContext = canvasBar()!!.getJSONObject("context").toString()
                val padding = Offset(bar.left + 3 * density, bar.center.y)
                tap(padding); settle()
                assertEquals("$device bar padding tap keeps the selection", selectionContext, canvasBar()?.getJSONObject("context")?.toString())
                assertTrue(shown("canvas-action-bar"))

                tap(bounds("canvas-bar-action-scale_rotate").center)
                waitFor("$device transform bar", 5_000) { barKind() == "transform" && shown("canvas-bar-action-apply_transform") }
                assertTrue("$device bar tap keeps window focus", owner.view.hasWindowFocus())
                settle()
                bar = bounds("canvas-action-bar")
                val anchor = canvasBar()!!.opt("anchor").toString()
                tap(Offset(bar.left + 3 * density, bar.center.y)); settle()
                assertEquals("$device bar tap never reaches the transform", anchor, canvasBar()!!.opt("anchor").toString())
                assertEquals("Free", chosen("transform-mode"))
                assertTrue("$device flips are icon-only", canvasBar()!!.array("items").objects().any { it.getString("label").isEmpty() })
                tap(bounds("canvas-bar-segment-transform-mode-2").center)
                waitFor("$device Distort", 5_000) { chosen("transform-mode") == "Distort" && shown("canvas-bar-action-transform_perspective") }
                settle()
                tap(bounds("canvas-bar-segment-transform-mode-0").center)
                waitFor("$device back to Free", 5_000) { chosen("transform-mode") == "Free" && !shown("canvas-bar-action-transform_perspective") }
                settle()
                tap(bounds("canvas-bar-choice-transform-interpolation").center)
                waitFor("$device interpolation menu", 5_000) { popupCount() == 1 && textBounds("Nearest") != null }
                instrumentation.runOnMainSync { assertTrue("$device the interpolation menu leaves window focus with the canvas", owner.view.hasWindowFocus()) }
                tap(textBounds("Nearest")!!.center)
                waitFor("$device Nearest", 5_000) { chosen("transform-interpolation") == "Nearest" && popupCount() == 0 }
                settle()
                tap(bounds("canvas-bar-segment-transform-mode-2").center)
                waitFor("$device Distort for a corner drag", 5_000) { chosen("transform-mode") == "Distort" }
                settle()
                val quad = canvasBar()!!.opt("anchor").toString()
                val start = corner()
                drag(start, start + Offset(36 * density, 28 * density))
                waitFor("$device Distort corner drag reshapes the transform", 5_000) { canvasBar()?.opt("anchor")?.toString() != quad }
                waitFor("$device bar returns after the corner drag", 3_000) { shown("canvas-action-bar") }
                assertEquals("Distort", chosen("transform-mode"))
                settle()

                invoke("reset_transform")
                waitFor("$device reset before Warp", 5_000) { chosen("transform-mode") == "Free" }
                settle()
                tap(bounds("canvas-bar-segment-transform-mode-3").center)
                waitFor("$device Warp shows its grid choice", 5_000) { chosen("transform-mode") == "Warp" && shown("canvas-bar-choice-transform-warp-grid") }
                settle()
                val grid = chosen("transform-warp-grid")!!
                tap(bounds("canvas-bar-choice-transform-warp-grid").center)
                waitFor("$device Grid menu lists its presets", 5_000) { popupCount() == 1 && listOf("3 × 3", "4 × 4", "5 × 5").all { textBounds(it) != null } }
                tap(textBounds(grid)!!.center)
                waitFor("$device Grid menu closes", 5_000) { popupCount() == 0 && chosen("transform-warp-grid") == grid }
                settle()
                val hull = canvasBar()!!.getJSONArray("anchor")
                val edgeNode = documentPoint(hull.getDouble(0) + (hull.getDouble(2) - hull.getDouble(0)) / 3, hull.getDouble(1))
                val edge = hull.getDouble(1)
                val base = hull.getDouble(3)
                val name = listOf("mouse", "finger", "stylus")[devices.indexOf(device)]
                drag(edgeNode, edgeNode - Offset(0f, 40 * density)) { captureCanvasBar("warp-held-$name") }
                waitFor("$device Warp edge node drag bends the top edge", 5_000) { canvasBar()?.getJSONArray("anchor")?.getDouble(1)?.let { it < edge - 10 } == true }
                assertEquals("$device the Warp node drag leaves the bottom edge in place", base, canvasBar()!!.getJSONArray("anchor").getDouble(3), 1.0)
                waitFor("$device bar returns after the Warp drag", 3_000) { shown("canvas-action-bar") }
                captureCanvasBar("warp-$name")
                invoke("reset_transform")
                waitFor("$device reset returns to Free", 5_000) { chosen("transform-mode") == "Free" }
                settle()

                var hiddenWhileHeld = false
                var glassWhileHeld = -1
                drag(selection.center, selection.center + Offset(30 * density, 20 * density)) {
                    waitFor("$device bar hides during a canvas contact") { !shown("canvas-action-bar") }
                    hiddenWhileHeld = true
                    waitFor("$device glass leaves with the bar") { glassBoxes() == glassBefore }
                    glassWhileHeld = nativeGlassRegions()
                }
                assertTrue(hiddenWhileHeld)
                assertTrue("$device native glass region count falls: $glassWhileHeld < $nativeShown", glassWhileHeld in 0 until nativeShown)
                SystemClock.sleep(60)
                assertFalse("$device bar waits for input to settle", shown("canvas-action-bar"))
                waitFor("$device bar returns after the contact", 3_000) { shown("canvas-action-bar") }
                val returned = SystemClock.uptimeMillis()
                while (SystemClock.uptimeMillis() - returned < 500) { assertTrue("$device bar returns once", shown("canvas-action-bar")); SystemClock.sleep(16) }
                waitFor("$device glass returns") { glassBoxes() == glassBefore + 1 }

                tap(bounds("canvas-bar-more").center)
                waitFor("$device More menu", 5_000) { popupCount() == 1 }
                instrumentation.runOnMainSync { assertTrue("$device More leaves window focus with the canvas", owner.view.hasWindowFocus()) }
                assertEquals("$device More keeps the transform", "transform", barKind())
                if (device == MotionEvent.TOOL_TYPE_MOUSE) {
                    tap(bounds("canvas-bar-more").center)
                    waitFor("More toggles closed") { popupCount() == 0 }
                    SystemClock.sleep(300)
                    assertEquals("More stays closed after its toggle", 0, popupCount())
                } else {
                    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                    waitFor("$device Back closes More") { popupCount() == 0 }
                }
                assertEquals("$device closing More keeps the transform", "transform", barKind())

                if (device == MotionEvent.TOOL_TYPE_STYLUS) {
                    tap(bounds("canvas-bar-more").center)
                    waitFor("More reopens", 5_000) { popupCount() == 1 && textBounds("Show canvas action bar") != null }
                    tap(textBounds("Show canvas action bar")!!.center)
                    waitFor("Hide the bar from More", 5_000) { popupCount() == 0 && !state().getJSONObject("workspace").getJSONObject("layout").getBoolean("canvas_bar") }
                    waitFor("completion-only bar", 5_000) { shown("canvas-bar-action-apply_transform") && !shown("canvas-bar-action-transform_flip_horizontal") }
                    settle()
                    val edge = bounds("canvas-action-bar")
                    assertTrue("completion-only bar sits on the bottom edge: $edge", edge.top > selection.bottom + 100 * density)
                    assertEquals("transform", barKind())
                    tap(bounds("canvas-bar-action-apply_transform").center)
                    waitFor("Apply ends the transform", 5_000) { barKind() != "transform" }
                    waitFor("no selection bar while the toggle is off") { !shown("canvas-action-bar") }
                    invoke("show_canvas_action_bar")
                    waitFor("selection bar returns with the toggle", 5_000) { barKind() == "selection" && shown("canvas-action-bar") }
                } else {
                    tap(bounds("canvas-bar-action-cancel_transform").center)
                    waitFor("$device Cancel ends the transform", 5_000) { barKind() == "selection" && shown("canvas-action-bar") }
                }

                instrumentation.runOnMainSync { host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(work.width / density / 2, work.height / density / 2)))) }
                invoke("zen_mode")
                waitFor("$device Zen hides docked chrome") { snapshot().optBoolean("chrome_hidden") }
                waitFor("$device Zen keeps the bar", 3_000) { shown("canvas-action-bar") }
                invoke("zen_mode")
                waitFor("$device Zen ends") { !snapshot().optBoolean("chrome_hidden") }
                invoke("deselect")
                waitFor("$device Deselect removes the bar") { canvasBar() == null && !shown("canvas-action-bar") }
                waitFor("$device glass count returns", 3_000) { glassBoxes() == glassBefore }
                println("PASS canvas bar device=$device")
            }

            tool = MotionEvent.TOOL_TYPE_STYLUS
            restore(); clear(); invoke("fit_canvas"); invoke("rectangle_select")
            val stripes = Rect(onDocument(.3, .18), onDocument(.7, .5))
            val stripe = stripes.width / 21
            invoke("brush")
            for (i in 0..10) drag(Offset(stripes.left + 2 * i * stripe, stripes.top), Offset(stripes.left + (2 * i + 1) * stripe, stripes.bottom), 6)
            invoke("rectangle_select")
            drag(onDocument(.38, .2), onDocument(.62, .36))
            waitFor("selection bar for captures", 5_000) { barKind() == "selection" && shown("canvas-action-bar") }
            tap(bounds("canvas-bar-action-scale_rotate").center)
            waitFor("transform bar for captures", 5_000) { shown("canvas-bar-action-apply_transform") }
            for (theme in listOf("light", "dark")) for (level in listOf(0, 3)) {
                action(obj("type" to "set_theme", "theme" to theme)); transparency(level)
                waitFor("$theme/$level bar", 5_000) { shown("canvas-bar-action-apply_transform") }
                val apply = bounds("canvas-bar-action-apply_transform")
                val accent = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("accent"))
                captureCanvasBar("$theme-level$level") { image, origin ->
                    val pixel = image.getPixel((apply.left + origin.x + 3 * density).toInt(), (apply.center.y + origin.y).toInt())
                    for (shift in listOf(0, 8, 16)) assertEquals("$theme/$level Apply uses the accent", (accent shr shift and 255).toFloat(), (pixel shr shift and 255).toFloat(), 24f)
                }
            }
            clear()
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme)); transparency(originalTransparency)
        }
        println("PASS canvas action bar: selection and transform bars, chrome taps, hide and return, More, toggle, Apply/Cancel, Zen, glass, light/dark")
    }

    private fun command(id: String) = action(obj("type" to "invoke", "command" to id))
    private fun layerAction(value: JSONObject) = action(obj("type" to "layer", "action" to value))
    private fun layerStates() = state().array("layers").objects()
    private fun editingLayer() = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
    private fun hasSelection() = state().getJSONObject("layer_tools").getBoolean("has_selection")
    private fun paintRevision(id: Long) = layerStates().first { it.getLong("id") == id }.getLong("paint_revision")
    private fun blue(pixel: Int) = android.graphics.Color.blue(pixel) - android.graphics.Color.red(pixel) > 80
    /** Where a document position is drawn, in the window's coordinates. */
    private fun documentPoint(x: Double, y: Double): Offset {
        val camera = state().getJSONObject("camera")
        val zoom = camera.getDouble("zoom"); val translation = camera.getJSONArray("translation")
        val work = bounds("workspace")
        return Offset(work.left + (x * zoom + translation.getDouble(0)).toFloat(), work.top + (y * zoom + translation.getDouble(1)).toFloat())
    }
    private fun screenPixels(points: List<Offset>): List<Int> {
        val origin = IntArray(2)
        onMain { owner.view.getLocationOnScreen(origin) }
        val image = instrumentation.uiAutomation.takeScreenshot()
        try { return points.map { image.getPixel((it.x + origin[0]).toInt(), (it.y + origin[1]).toInt()) } } finally { image.recycle() }
    }
    private fun awaitPixels(label: String, points: List<Offset>, check: (List<Int>) -> Boolean) {
        val until = SystemClock.uptimeMillis() + 5_000
        var last = screenPixels(points)
        while (!check(last)) {
            if (SystemClock.uptimeMillis() > until) fail("$label: ${last.map { "#%06x".format(it and 0xffffff) }}")
            SystemClock.sleep(100); last = screenPixels(points)
        }
    }
    /** An item's label in the open windowless menu, never the bar or panels beneath it. */
    private fun menuText(text: String): Rect? {
        var result: Rect? = null
        onMain {
            val base = IntArray(2); owner.view.getLocationOnScreen(base)
            semanticsRoots().filter { it !== owner && it.find(hasTag("workspace-menu")) != null }
                .firstNotNullOfOrNull { root -> root.find(hasLabel(text))?.let { root to it } }?.let { (root, node) ->
                    val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                    result = node.boundsInRoot.translate(Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat()))
                }
        }
        return result
    }
    /** Open a bar menu, or its submenu in More when it does not fit, and choose `path` in it. */
    private fun chooseFromBarMenu(menu: String, path: List<String>) {
        val tag = "canvas-bar-menu-$menu"
        val onBar = shown(tag)
        val label = canvasBar()!!.array("items").objects().first { it.optString("menu") == menu }.getString("label")
        tap(bounds(if (onBar) tag else "canvas-bar-more").center)
        waitFor("$menu opens", 5_000) { popupCount() == 1 }
        onMain { assertTrue("the $menu menu leaves window focus with the canvas", owner.view.hasWindowFocus()) }
        for (text in if (onBar) path else listOf(label) + path) {
            waitFor("$text in the $menu menu", 5_000) { menuText(text) != null }
            settle()
            tap(menuText(text)!!.center)
        }
        waitFor("the $menu menu closes", 5_000) { popupCount() == 0 }
        onMain { assertTrue("choosing from $menu leaves window focus with the canvas", owner.view.hasWindowFocus()) }
    }
    private class BlueSelection(val layer: Long, val inside: Offset, val outside: Offset)
    /** Remove the layers a journey added, then select part of a new blue layer with the pen. */
    private fun blueSelection(keep: Set<Long>): BlueSelection {
        if (state().array("commands").objects().any { it.getString("id") == "deselect" && it.getBoolean("enabled") }) command("deselect")
        layerStates().map { it.getLong("id") }.filter { it !in keep }.forEach { layerAction(obj("op" to "delete", "id" to it)) }
        layerAction(obj("op" to "new", "group" to false, "clipped" to false))
        val layer = editingLayer()
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .3, .9, 1))))
        command("select_all"); command("fill_selection"); command("deselect")
        command("rectangle_select")
        val extent = state().array("tabs").objects().first { it.getBoolean("active") }
        val (width, height) = extent.getInt("width").toDouble() to extent.getInt("height").toDouble()
        val device = tool
        tool = MotionEvent.TOOL_TYPE_STYLUS
        drag(documentPoint(width * .4, height * .35), documentPoint(width * .6, height * .55))
        tool = device
        waitFor("the selection bar", 5_000) { hasSelection() && barKind() == "selection" && shown("canvas-action-bar") }
        settle()
        return BlueSelection(layer, documentPoint(width * .5, height * .45), documentPoint(width * .2, height * .45))
    }
    private fun filterCategory(filter: String) = kotlinx.coroutines.runBlocking {
        host.withNative { JSONObject(Native.query(it, obj("type" to "application_menu", "menu" to "filter").toString())) }
    }.array("sections").getJSONArray(0).objects().first { category ->
        category.array("sections").values().any { section -> (section as JSONArray).objects().any { it.getString("label") == filter } }
    }.getString("label")

    @Test fun canvasBarSelectionMenusAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val bands = fixture.getJSONObject("layout").array("bands").objects()
        val wide = JSONObject(fixture.toString()).apply { getJSONObject("layout").put("bands", JSONArray(bands.filter { it.getInt("id") == 44 })) }
        val docked = JSONObject(fixture.toString()).apply {
            getJSONObject("layout").array("bands").objects().first { it.getInt("id") == 42 }.put("extent", 620)
        }
        val curves = filterCategory("Curves")
        popupInput = true
        try {
            for ((layout, workspace) in listOf("wide" to wide, "docked" to docked)) for (device in pointerTools) {
                val name = "$layout ${listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]}"
                action(obj("type" to "restore_workspace", "workspace" to workspace)); command("fit_canvas")
                SystemClock.sleep(300)
                tool = device

                var selection = blueSelection(keep)
                if (layout == "docked") assertFalse("$name: Adjust and Clear do not fit beside the docks",
                    shown("canvas-bar-menu-adjust") || shown("canvas-bar-menu-clear"))
                else assertTrue("$name: every menu fits on the bar",
                    listOf("copy_to_layer", "adjust", "clear").all { shown("canvas-bar-menu-$it") })
                val count = layerStates().size
                chooseFromBarMenu("copy_to_layer", listOf("Copy Selection to New Layer"))
                waitFor("$name: Copy to Layer adds a layer and consumes the selection", 5_000) {
                    layerStates().size == count + 1 && !hasSelection() && editingLayer() != selection.layer
                }
                command("undo")
                waitFor("$name: one undo step removes the copy and restores the selection", 5_000) {
                    layerStates().size == count && hasSelection() && editingLayer() == selection.layer
                }
                command("redo")
                waitFor("$name: redo", 5_000) { layerStates().size == count + 1 && !hasSelection() }
                layerAction(obj("op" to "visibility", "id" to selection.layer, "value" to false))
                awaitPixels("$name: the copy holds only the selected pixels", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && !blue(outside)
                }

                selection = blueSelection(keep)
                val filled = paintRevision(selection.layer)
                chooseFromBarMenu("clear", listOf("Clear Outside Selection"))
                waitFor("$name: Clear Outside edits the layer and keeps the selection", 5_000) {
                    paintRevision(selection.layer) != filled && hasSelection()
                }
                awaitPixels("$name: Clear Outside keeps only the selected pixels", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && !blue(outside)
                }
                command("undo")
                awaitPixels("$name: one undo step restores the cleared pixels", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && blue(outside)
                }
                if (layout == "wide") {
                    fun clear() = canvasBar()?.array("items")?.objects()?.first { it.optString("menu") == "clear" }
                        ?.getJSONObject("option")?.getJSONObject("Action")?.getJSONObject("state")
                    layerAction(obj("op" to "lock", "id" to selection.layer, "value" to true))
                    waitFor("$name: a locked layer disables Clear ▾", 5_000) { clear()?.getBoolean("enabled") == false && shown("canvas-action-bar") }
                    val reason = clear()!!.getString("disabled_reason")
                    tap(bounds("canvas-bar-menu-clear").center)
                    waitFor("$name: a tap on the disabled Clear ▾ shows why", 3_000) { findTag("hover-tooltip") != null && textBounds(reason) != null }
                    assertEquals("$name: the disabled menu stays closed", 0, popupCount())
                    layerAction(obj("op" to "lock", "id" to selection.layer, "value" to false))
                }

                selection = blueSelection(keep)
                val before = layerStates().size
                chooseFromBarMenu("adjust", listOf(curves, "Curves"))
                waitFor("$name: Adjust › Curves adds a Curves layer masked by the selection, which it consumes", 5_000) {
                    val active = layerStates().first { it.getLong("id") == editingLayer() }
                    layerStates().size == before + 1 && active.getString("label") == "Curves" && active.getBoolean("has_mask") && !hasSelection()
                }
                command("undo")
                waitFor("$name: one undo step removes the effect and restores the selection", 5_000) {
                    layerStates().size == before && hasSelection() && editingLayer() == selection.layer
                }
                println("PASS canvas bar menus $name")
            }
        } finally { popupInput = false }
        println("PASS canvas bar menus: Copy to Layer, Clear ▾ › Clear Outside and Adjust ▾ › Curves on the bar and through More, with mouse, finger and stylus")
    }

    private fun barLabel(): String? {
        var text: String? = null
        onMain { text = find("canvas-bar-label")?.config?.getOrNull(SemanticsProperties.Text)?.joinToString("") { it.text } }
        return text
    }
    private fun barCaption(command: String, text: String): Boolean {
        var found = false
        onMain { found = find("canvas-bar-action-$command")?.find(hasLabel(text)) != null }
        return found
    }
    private fun tapBar(command: String) {
        waitFor("$command on the bar", 5_000) { shown("canvas-bar-action-$command") }
        settle()
        tap(bounds("canvas-bar-action-$command").center)
    }
    private fun paintRevisionText(row: JSONObject) = row.get("paint_revision").toString()
    /** A mode bar reads [label] along the bottom edge and ends with its accented exit. */
    private fun awaitModeBar(name: String, kind: String, label: String) {
        waitFor("$name: the $kind bar reads $label", 5_000) { barKind() == kind && shown("canvas-action-bar") && barLabel() == label }
        settle()
        val bar = bounds("canvas-action-bar"); val work = bounds("workspace")
        assertEquals("$name: the $kind bar uses the bottom edge", "bottom_edge", canvasBar()!!.getString("placement"))
        assertTrue("$name: the $kind bar sits on the bottom edge: $bar in $work", bar.bottom > work.bottom - 80 * density)
        assertTrue("$name: the $kind exit uses the accent", canvasBar()!!.array("completion").getJSONObject(0).getBoolean("accent"))
    }

    @Test fun modeBarsLeaveFromTheirExitsAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        fixture.getJSONObject("layout").put("bands", JSONArray(fixture.getJSONObject("layout").array("bands").objects().filter { it.getInt("id") == 44 }))
        var paint = 0L
        fun row(id: Long) = layerStates().first { it.getLong("id") == id }
        fun artwork() = editingLayer() == paint && !row(paint).getBoolean("mask_selected")
            && !state().getJSONObject("layer_tools").getBoolean("quick_mask") && state().getJSONObject("layer_tools").isNull("mask_editing")
        fun idle(label: String) = waitFor("$label: the canvas interaction finishes", 5_000) {
            state().array("commands").objects().any { it.getString("id") == "add_layer" && it.getBoolean("enabled") }
        }
        popupInput = true
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                restore(); command("fit_canvas")
                layerStates().map { it.getLong("id") }.filter { it !in keep }.forEach { layerAction(obj("op" to "delete", "id" to it)) }
                layerAction(obj("op" to "new", "group" to false, "clipped" to false))
                paint = editingLayer()
                val paintName = row(paint).getString("label")
                tool = device

                command("select_all"); command("quick_mask")
                awaitModeBar(name, "quick_mask", "Quick Mask")
                fun quickMask() = layerStates().firstOrNull { it.getBoolean("quick_mask") }?.let(::paintRevisionText)
                val coverage = quickMask()
                tapBar("invert_selection")
                waitFor("$name: Invert inverts the Quick Mask", 5_000) { quickMask().let { it != null && it != coverage } }
                assertEquals("$name: Invert stays in Quick Mask", "quick_mask", barKind())
                tapBar("return_to_artwork")
                waitFor("$name: Exit leaves Quick Mask", 5_000) { artwork() && barKind() != "quick_mask" }

                command("select_all"); command("save_selection_layer"); layerAction(obj("op" to "cancel_rename"))
                val saved = editingLayer()
                assertTrue("$name: saving edits the new Selection Layer", row(saved).getBoolean("selection_layer"))
                awaitModeBar(name, "selection_layer", "Editing ${row(saved).getString("label")}")
                val stored = paintRevisionText(row(saved))
                tapBar("invert_selection_layer")
                waitFor("$name: Invert inverts the stored coverage", 5_000) { paintRevisionText(row(saved)) != stored }
                assertEquals("$name: Invert stays on the Selection Layer", saved, editingLayer())
                assertEquals("selection_layer", barKind())
                tapBar("return_to_artwork")
                waitFor("$name: Return to Artwork leaves Selection Layer editing", 5_000) { artwork() && barKind() != "selection_layer" }
                layerAction(obj("op" to "delete", "id" to saved))

                command("select_all"); command("mask_selection")
                awaitModeBar(name, "layer_mask", "Editing $paintName mask")
                assertTrue("$name: an enabled mask offers Disable", barCaption("layer_mask_enabled", "Disable"))
                if (device == MotionEvent.TOOL_TYPE_STYLUS) {
                    val plain = bounds("canvas-bar-action-invert_layer_mask"); val toggle = bounds("canvas-bar-action-layer_mask_enabled")
                    captureCanvasBar("mask-mode") { image, origin ->
                        fun fill(r: Rect) = image.getPixel((r.left + origin.x + 3 * density).toInt(), (r.center.y + origin.y).toInt())
                        for (shift in listOf(0, 8, 16)) assertEquals("Disable is drawn as a plain button, not a pressed toggle",
                            (fill(plain) shr shift and 255).toFloat(), (fill(toggle) shr shift and 255).toFloat(), 12f)
                    }
                }
                tapBar("layer_mask_enabled")
                waitFor("$name: Disable turns the mask off and the button offers Enable", 5_000) {
                    !row(paint).getBoolean("mask_enabled") && barCaption("layer_mask_enabled", "Enable")
                }
                assertEquals("$name: Disable keeps mask editing", "layer_mask", barKind())
                tapBar("edit_layer_content")
                waitFor("$name: Edit Content leaves mask editing", 5_000) { artwork() && canvasBar() == null && !shown("canvas-action-bar") }
                layerAction(obj("op" to "delete_mask", "id" to paint))
                println("PASS mode bars device=$name")
            }

            command("select_all"); command("mask_selection")
            awaitModeBar("Escape", "layer_mask", "Editing ${row(paint).getString("label")} mask")
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Escape leaves mask editing", 5_000) { artwork() && canvasBar() == null }
            command("select_all"); command("save_selection_layer"); layerAction(obj("op" to "cancel_rename"))
            val saved = editingLayer()
            awaitModeBar("Escape", "selection_layer", "Editing ${row(saved).getString("label")}")
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Escape leaves Selection Layer editing", 5_000) { artwork() && barKind() != "selection_layer" }
            layerAction(obj("op" to "delete", "id" to saved))

            tool = MotionEvent.TOOL_TYPE_STYLUS
            command("mask_selection")
            awaitModeBar("notice", "layer_mask", "Editing ${row(paint).getString("label")} mask")
            idle("notice")
            command("move")
            layerAction(obj("op" to "lock", "id" to paint, "value" to true))
            val work = bounds("workspace")
            val point = Offset(work.left + 720 * density, work.top + 300 * density)
            drag(point, point + Offset(40 * density, 24 * density))
            waitFor("Move on the locked mask explains", 5_000) {
                state().optJSONObject("notice")?.optString("text") == "The active layer is locked" && shown("canvas-notice")
            }
            waitFor("the mode bar stays through the refused contact", 3_000) { barKind() == "layer_mask" && shown("canvas-action-bar") }
            val bar = bounds("canvas-action-bar"); val bubble = bounds("canvas-notice")
            assertTrue("the notice sits above the bottom-edge mode bar: $bubble vs $bar", bubble.bottom <= bar.top && !bubble.overlaps(bar))
            captureCanvasBar("mask-mode-notice")
            idle("unlock")
            layerAction(obj("op" to "lock", "id" to paint, "value" to false))
            command("edit_layer_content")
            layerAction(obj("op" to "delete_mask", "id" to paint))
        } finally { popupInput = false }
        println("PASS mode bars: Quick Mask → Invert → Exit, Selection Layer → Invert → Return to Artwork and layer mask → Disable → Edit Content with mouse, finger and stylus; Escape; a notice above the bottom-edge bar")
    }

    @Test fun guideBarDeletesTheSelectedGuideAcrossDevices() {
        popupInput = true
        try {
            restore(); command("fit_canvas"); command("ruler")
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                val work = bounds("workspace")
                val from = Offset(work.left + 560 * density, work.top + 200 * density)
                val to = from + Offset(200 * density, 60 * density)
                tool = if (device == MotionEvent.TOOL_TYPE_FINGER) MotionEvent.TOOL_TYPE_STYLUS else device
                drag(from, to)
                tool = device
                waitFor("$name: the guide bar", 5_000) { barKind() == "guide" && shown("canvas-action-bar") }
                settle()
                val bar = bounds("canvas-action-bar")
                assertNull("$name: the guide bar has no label", barLabel())
                assertEquals("$name: the guide bar sits beside the guide", "near_object", canvasBar()!!.getString("placement"))
                assertTrue("$name: the guide bar sits below the guide's handles: $bar vs $from $to", bar.top > maxOf(from.y, to.y) && bar.top - maxOf(from.y, to.y) < 120 * density)
                tapBar("delete_ruler")
                waitFor("$name: Delete removes the guide and its bar", 5_000) {
                    barKind() != "guide" && state().array("commands").objects().none { it.getString("id") == "delete_ruler" && it.getBoolean("enabled") }
                }
                println("PASS guide bar device=$name")
            }
        } finally { popupInput = false }
        println("PASS guide bar: a selected guide's bar deletes it with mouse, finger and stylus")
    }

    @Test fun canvasBarRefineAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val wide = JSONObject(fixture.toString()).apply {
            getJSONObject("layout").put("bands", JSONArray(getJSONObject("layout").array("bands").objects().filter { it.getInt("id") == 44 }))
        }
        fun refine() = state().getJSONObject("layer_tools").objectOrNull("selection_resize")
        fun anchor() = canvasBar()?.optJSONArray("anchor")?.toString()
        fun edges() = canvasBar()!!.getJSONArray("anchor").let { a -> (0 until 4).map(a::getDouble) }
        fun close(a: Int, b: Int) = listOf(16, 8, 0).all { kotlin.math.abs((a shr it and 255) - (b shr it and 255)) <= 6 }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                action(obj("type" to "restore_workspace", "workspace" to wide)); command("fit_canvas")
                SystemClock.sleep(300)
                tool = device

                var selection = blueSelection(keep)
                var pixels = paintRevision(selection.layer)
                val hard = anchor()
                val undimmed = screenPixels(listOf(selection.outside)).single()
                chooseFromBarMenu("refine", listOf("Feather…"))
                waitFor("$name: Feather… opens the Refine panel", 5_000) {
                    refine()?.getString("kind") == "feather" && shown("selection-refine-panel")
                }
                assertEquals("Feather Selection", refine()!!.getString("title"))
                assertNotNull("$name: the panel names the value", textBounds("Feather radius"))
                onMain {
                    assertNotNull("$name: the panel is part of the workspace window", owner.find(hasTag("selection-refine-panel")))
                    assertTrue("$name: the panel leaves window focus with the canvas", owner.view.hasWindowFocus())
                }
                waitFor("$name: the default radius previews live", 10_000) { anchor() != hard }
                assertTrue("$name: the canvas behind the panel is not dimmed", close(undimmed, screenPixels(listOf(selection.outside)).single()))
                val previewed = anchor()
                val panel = bounds("selection-refine-panel")
                assertTrue("$name: the panel stays clear of the selection", panel.top > selection.inside.y)
                val slider = bounds("setting-slider-selection-refine")
                fun along(f: Float) = Offset(slider.left + slider.width * f, slider.center.y)
                val drawn = selectionPreviews()
                val anchors = mutableSetOf<String?>()
                event(MotionEvent.ACTION_DOWN, along(.2f))
                for (i in 1..90) {
                    SystemClock.sleep(16)
                    event(MotionEvent.ACTION_MOVE, along(.2f + .2f * i / 90))
                    anchors += anchor()
                }
                val previews = selectionPreviews() - drawn
                assertTrue("$name: a continuously dragged value keeps updating the preview ($previews previews)", previews >= 2)
                assertTrue("$name: previews leave the bar in place ($anchors)", anchors.size < previews)
                event(MotionEvent.ACTION_UP)
                waitFor("$name: the released value previews", 10_000) {
                    (refine()?.number("radius") ?: 0f) > 8f && anchor() != previewed
                }
                for (theme in if (device == pointerTools.first()) listOf("light", "dark") else listOf(null)) {
                    theme?.let { action(obj("type" to "set_theme", "theme" to it)) }
                    captureCanvasBar(listOfNotNull("refine-feather", name, theme).joinToString("-"))
                }
                val radius = refine()!!.number("radius")
                assertTrue("$name: one decimal for Feather ($radius)", kotlin.math.abs(radius * 10 - kotlin.math.round(radius * 10)) < 1e-3)
                tap(bounds("selection-refine-apply").center)
                waitFor("$name: Apply closes the panel", 10_000) { refine() == null && !exists("selection-refine-panel") }
                waitFor("$name: the feathered selection is kept", 5_000) { anchor() != null && anchor() != hard }
                val feathered = anchor()
                assertEquals("$name: Feather edits no pixels", pixels, paintRevision(selection.layer))
                command("undo")
                waitFor("$name: one Undo restores the hard edge", 5_000) { anchor() == hard }
                command("redo")
                waitFor("$name: Redo feathers it again", 5_000) { anchor() == feathered }

                selection = blueSelection(keep)
                pixels = paintRevision(selection.layer)
                val outline = edges()
                chooseFromBarMenu("refine", listOf("Transform Outline"))
                waitFor("$name: Transform Outline opens its bar", 5_000) {
                    barKind() == "transform" && canvasBar()?.optString("label") == "Transform Outline" && shown("canvas-action-bar")
                }
                assertFalse("$name: an outline has no interpolation", exists("canvas-bar-choice-transform-interpolation"))
                val frame = edges()
                val handle = documentPoint(frame[2], (frame[1] + frame[3]) / 2)
                drag(handle, handle + Offset(48 * density, 0f))
                waitFor("$name: dragging the edge handle widens the outline", 5_000) {
                    edges().let { it[2] > frame[2] + 10 && kotlin.math.abs(it[0] - frame[0]) < 1 }
                }
                waitFor("$name: the bar returns after the drag", 3_000) { shown("canvas-action-bar") }
                captureCanvasBar("transform-outline-$name")
                tap(bounds("canvas-bar-action-apply_transform").center)
                waitFor("$name: Apply keeps the wider outline", 5_000) {
                    barKind() == "selection" && edges()[2] > outline[2] + 10
                }
                assertEquals("$name: Transform Outline moves no pixels", pixels, paintRevision(selection.layer))
                awaitPixels("$name: the layer keeps its pixels", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && blue(outside)
                }
                command("undo")
                waitFor("$name: one Undo restores the outline", 5_000) { edges() == outline }
                println("PASS canvas bar refine $name")
            }
        } finally { popupInput = false; action(obj("type" to "set_theme", "theme" to originalTheme)) }
        println("PASS canvas bar refine: Refine ▾ › Feather… keeps previewing while the value is dragged and undoes in one step, and Transform Outline moves only the selection, with mouse, finger and stylus")
    }

    @Test fun selectionBarOverflowsIntoMoreInBothOrientations() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        popupInput = true
        try {
            for (portrait in listOf(false, true)) {
                val name = if (portrait) "portrait" else "landscape"
                if (portrait) device.portrait(scenario) else device.landscape(scenario)
                restore(); command("fit_canvas")
                SystemClock.sleep(500)
                tool = MotionEvent.TOOL_TYPE_STYLUS
                val selection = blueSelection(keep)
                val items = canvasBar()!!.array("items").objects()
                fun tag(item: JSONObject) = if (!item.isNull("menu")) "canvas-bar-menu-${item.getString("menu")}"
                    else "canvas-bar-action-${item.getJSONObject("option").getJSONObject("Action").getJSONObject("state").getString("id")}"
                val hidden = items.filter { !shown(tag(it)) }
                assertTrue("$name: the selection bar overflows into More (${items.size - hidden.size} of ${items.size} shown)",
                    hidden.isNotEmpty() && hidden.size < items.size)
                assertEquals("$name: the bar shows a leading run of items", items.takeLast(hidden.size), hidden)
                val bar = bounds("canvas-action-bar")
                var window = Rect.Zero
                onMain { window = Rect(0f, 0f, owner.view.width.toFloat(), owner.view.height.toFloat()) }
                assertTrue("$name: the bar stays in the window: $bar in $window", bar.left >= window.left && bar.right <= window.right)
                captureCanvasBar("selection-overflow-$name")
                tap(bounds("canvas-bar-more").center)
                waitFor("$name: More lists every hidden item", 5_000) {
                    popupCount() == 1 && hidden.all { item ->
                        menuText(if (!item.isNull("menu")) item.getString("label")
                            else item.getJSONObject("option").getJSONObject("Action").getJSONObject("state").getString("label")) != null
                    }
                }
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                waitFor("$name: Back closes More") { popupCount() == 0 }

                val count = layerStates().size
                chooseFromBarMenu("copy_to_layer", listOf("Cut Selection to New Layer"))
                waitFor("$name: Cut to Layer adds a layer and consumes the selection", 5_000) {
                    layerStates().size == count + 1 && !hasSelection()
                }
                awaitPixels("$name: Cut leaves a hole in the source, filled by the new layer", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && blue(outside)
                }
                layerAction(obj("op" to "visibility", "id" to editingLayer(), "value" to false))
                awaitPixels("$name: hiding the cut pixels shows the hole", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    !blue(inside) && blue(outside)
                }
                println("PASS selection bar overflow $name")
            }
        } finally { popupInput = false }
    }

    @Test fun hardwareDeleteClearsSelectedPixels() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        tool = MotionEvent.TOOL_TYPE_STYLUS
        command("fit_canvas")
        val selection = blueSelection(keep)
        for ((key, name) in listOf(KeyEvent.KEYCODE_FORWARD_DEL to "Delete", KeyEvent.KEYCODE_DEL to "Backspace")) {
            val filled = paintRevision(selection.layer)
            instrumentation.sendKeyDownUpSync(key)
            waitFor("$name clears the selected pixels", 5_000) { paintRevision(selection.layer) != filled }
            awaitPixels("$name clears only the selected pixels", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                !blue(inside) && blue(outside)
            }
            assertTrue("$name keeps the selection", hasSelection())
            command("undo")
            awaitPixels("one undo step restores what $name cleared", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                blue(inside) && blue(outside)
            }
        }

        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
        layerAction(obj("op" to "begin_rename", "id" to selection.layer))
        waitFor("the rename field takes keys", 5_000) { host.editingText }
        val renaming = paintRevision(selection.layer)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_FORWARD_DEL)
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DEL)
        settle()
        assertEquals("a text field keeps Delete and Backspace", renaming, paintRevision(selection.layer))
        layerAction(obj("op" to "cancel_rename"))
        waitFor("the rename field closes", 5_000) { !host.editingText }

        var first = 0L
        onMain { first = host.drawingTabs.selected }
        val task = kotlinx.coroutines.runBlocking { host.withNative { h ->
            val (id, file) = documentRequest(h, "new_document")
            Native.projectTask(h, id, "null", file.getLong("epoch"), file.getLong("revision"))
        } }
        try {
            Native.projectWork(task, -1, 640, 480)
            val until = SystemClock.uptimeMillis() + 60_000
            while (!kotlinx.coroutines.runBlocking { host.withNative { Native.projectParkReady(it, task) } }) {
                assertTrue("the first drawing parks", SystemClock.uptimeMillis() < until); SystemClock.sleep(16)
            }
            kotlinx.coroutines.runBlocking { host.withNative { Native.projectAdopt(it, task, "null") } }
        } finally { Native.projectFree(task) }
        onMain { host.documentChanged() }
        waitFor("a second drawing", 30_000) { !host.drawingTabs.switching && host.drawingTabs.rows.size == 2 && host.drawingTabs.selected != first }
        onMain { host.drawingTabs.select(first) }
        waitFor("the first drawing returns", 30_000) { !host.drawingTabs.switching && host.drawingTabs.selected == first && hasSelection() }
        waitFor("drawing tabs in the header", 10_000) { exists("drawing-tab-$first") }
        val kept = paintRevision(selection.layer)
        instrumentation.setInTouchMode(false)
        try {
            waitFor("keyboard mode") { !owner.view.isInTouchMode }
            onMain { assertTrue(find("drawing-tab-$first")!!.config[androidx.compose.ui.semantics.SemanticsActions.RequestFocus].action!!.invoke()) }
            waitFor("the drawing tab takes key focus") { host.drawingTabs.focused == first }
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_FORWARD_DEL)
            waitFor("Delete closes the focused drawing", 30_000) { findTag("document-close-cancel") != null }
            assertEquals("a focused drawing tab keeps Delete from the canvas", kept, paintRevision(selection.layer))
            assertTrue(hasSelection())
            onMain { findTag("document-close-cancel")!!.second.config[androidx.compose.ui.semantics.SemanticsActions.OnClick].action!!.invoke() }
            waitFor("the drawing stays open", 30_000) { findTag("document-close-cancel") == null && !host.drawingTabs.switching && host.drawingTabs.rows.size == 2 }
        } finally {
            instrumentation.setInTouchMode(true)
            waitFor("touch mode returns") { owner.view.isInTouchMode }
        }
        var second = 0L
        onMain { second = host.drawingTabs.rows.first { it.getLong("id") != first }.getLong("id"); host.drawingTabs.select(second) }
        waitFor("the second drawing", 30_000) { !host.drawingTabs.switching && host.drawingTabs.selected == second }
        onMain { host.drawingTabs.select(second, true) }
        waitFor("the second drawing closes", 30_000) { !host.drawingTabs.switching && host.drawingTabs.rows.size == 1 }
        println("PASS hardware Delete and Backspace clear selected pixels in one undo step each; a text field and a focused drawing tab keep them")
    }

    /** Open an application menu from the title bar, or through the compact menu, and choose `path` in it. */
    private fun chooseFromApplicationMenu(menu: String, path: List<String>) {
        val label = snapshot().array("application_menus").objects().first { it.getString("id") == menu }.getString("label")
        val labelled = shown("application-menu-$menu")
        tap(bounds(if (labelled) "application-menu-$menu" else "header-menu-labels-compact").center)
        waitFor("the $label menu opens", 5_000) { popupCount() == 1 }
        for (text in if (labelled) path else listOf(label) + path) {
            waitFor("$text in the $label menu", 5_000) { menuText(text) != null }
            settle()
            tap(menuText(text)!!.center)
        }
        waitFor("the $label menu closes", 5_000) { popupCount() == 0 }
    }
    /** Remove the layers a journey added and return the document's extent. */
    private fun cleanDocument(keep: Set<Long>): Pair<Double, Double> {
        restore()
        if (hasSelection()) command("deselect")
        layerStates().map { it.getLong("id") }.filter { it !in keep }.forEach { layerAction(obj("op" to "delete", "id" to it)) }
        command("fit_canvas")
        SystemClock.sleep(300)
        val extent = state().array("tabs").objects().first { it.getBoolean("active") }
        return extent.getInt("width").toDouble() to extent.getInt("height").toDouble()
    }
    private fun control(key: String) = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == key }
        .getJSONObject("value").getJSONObject("value")
    private fun shows(pixel: Int, rgba: List<Double>) = listOf(android.graphics.Color.red(pixel), android.graphics.Color.green(pixel), android.graphics.Color.blue(pixel))
        .zip(rgba).all { (value, expected) -> kotlin.math.abs(value - 255 * expected) < 45 }
    private fun same(a: Int, b: Int) = listOf(16, 8, 0).all { shift -> kotlin.math.abs((a shr shift and 0xff) - (b shr shift and 0xff)) <= 12 }

    @Test fun solidColorFillMasksTheSelectionAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val colors = listOf(listOf(.85, .08, .05, 1.0), listOf(.05, .6, .1, 1.0), listOf(.8, .1, .7, 1.0))
        popupInput = true
        try {
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                val (width, height) = cleanDocument(keep)
                val inside = documentPoint(width * .5, height * .45)
                val outside = documentPoint(width * .2, height * .45)
                val rgba = colors[index]
                action(obj("type" to "set_color", "rgba" to JSONArray(rgba)))
                command("rectangle_select")
                tool = MotionEvent.TOOL_TYPE_STYLUS
                drag(documentPoint(width * .4, height * .35), documentPoint(width * .6, height * .55))
                waitFor("$name: the selection", 5_000) { hasSelection() && barKind() == "selection" && shown("canvas-action-bar") }
                settle()
                tool = device
                val before = screenPixels(listOf(inside, outside))
                val count = layerStates().size
                val base = editingLayer()
                chooseFromApplicationMenu("layer", listOf("New", "Solid Color Fill"))
                waitFor("$name: Layer › New › Solid Color Fill adds a fill layer masked by the selection, which it consumes", 5_000) {
                    val active = layerStates().first { it.getLong("id") == editingLayer() }
                    layerStates().size == count + 1 && active.getString("label") == "Solid Color" && active.getBoolean("has_mask") && !hasSelection()
                }
                val layers = layerStates()
                assertEquals("$name: the fill sits directly above the active layer",
                    layers.indexOfFirst { it.getLong("id") == base }, layers.indexOfFirst { it.getLong("id") == editingLayer() } + 1)
                val fill = control("color").getJSONArray("rgba")
                assertTrue("$name: the fill uses the current colour: $fill", rgba.indices.all { kotlin.math.abs(fill.getDouble(it) - rgba[it]) < 1e-5 })
                awaitPixels("$name: the current colour fills the selection and nothing else", listOf(inside, outside)) { (i, o) ->
                    shows(i, rgba) && same(o, before[1])
                }
                if (index == 0) captureCanvasBar("solid-color-fill", "photo-edit")
                command("undo")
                waitFor("$name: one undo step removes the fill", 5_000) { layerStates().size == count && editingLayer() == base }
                awaitPixels("$name: undo shows the layer beneath again", listOf(inside)) { (i) -> same(i, before[0]) }
                println("PASS solid color fill $name")
            }
        } finally { popupInput = false }
        println("PASS Layer › New › Solid Color Fill masks a selection in the current colour in one undo step, with mouse, finger and stylus")
    }

    @Test fun cropAndCanvasSizeRestoreHiddenPixelsAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val corners = listOf("bottom_right", "top_left", "bottom_left")
        val colors = listOf(listOf(.85, .08, .05, 1.0), listOf(.05, .6, .1, 1.0), listOf(.1, .2, .85, 1.0))
        fun size() = state().array("tabs").objects().first { it.getBoolean("active") }.let { it.getInt("width") to it.getInt("height") }
        fun panel() = state().getJSONObject("layer_tools").optJSONObject("canvas_size")
        fun enabled(id: String) = state().array("commands").objects().first { it.getString("id") == id }.getBoolean("enabled")
        fun history(id: String) { waitFor("$id is available", 10_000) { enabled(id) }; command(id) }
        fun type(field: String, text: String) {
            tap(bounds("setting-number-canvas-size-$field").center)
            waitFor("$field takes the keys", 5_000) { host.editingText }
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_MOVE_END)
            repeat(16) { instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DEL) }
            instrumentation.sendStringSync(text)
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        var original = 0 to 0
        popupInput = true
        try {
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                val (width, height) = cleanDocument(keep)
                original = width.toInt() to height.toInt()
                command("zoom_out"); SystemClock.sleep(300)
                val corner = corners[index]; val rgba = colors[index]
                val right = corner.endsWith("right"); val bottom = corner.startsWith("bottom")
                val cut = listOf(if (right) Math.round(width * .45).toDouble() else -40.0, if (bottom) Math.round(height * .45).toDouble() else -40.0)
                val far = listOf(if (right) width + 40 else Math.round(width * .55).toDouble(), if (bottom) height + 40 else Math.round(height * .55).toDouble())
                val inside = listOf((maxOf(cut[0], 0.0) + minOf(far[0], width)) / 2, (maxOf(cut[1], 0.0) + minOf(far[1], height)) / 2)
                val hidden = listOf(if (right) width * .2 else width * .8, if (bottom) height * .2 else height * .8)
                layerAction(obj("op" to "new", "group" to false, "clipped" to false))
                action(obj("type" to "set_color", "rgba" to JSONArray(rgba)))
                command("pen"); action(obj("type" to "select_brush", "id" to 1)); action(obj("type" to "set_brush_size", "value" to 90))
                tool = MotionEvent.TOOL_TYPE_STYLUS
                for (at in listOf(hidden, inside)) { drag(documentPoint(at[0] - 120, at[1]), documentPoint(at[0] + 120, at[1])); settle() }
                val hiddenPoint = documentPoint(hidden[0], hidden[1]); val insidePoint = documentPoint(inside[0], inside[1])
                awaitPixels("$name: both strokes are painted", listOf(hiddenPoint, insidePoint)) { (h, i) -> shows(h, rgba) && shows(i, rgba) }

                command("rectangle_select")
                drag(documentPoint(cut[0], cut[1]), documentPoint(far[0], far[1]))
                waitFor("$name: the selection bar", 5_000) { hasSelection() && barKind() == "selection" && shown("canvas-action-bar") }
                settle()
                tool = device
                val crop = "canvas-bar-action-crop_canvas_to_selection"
                if (shown(crop)) {
                    assertTrue("$name: the bar item reads Crop", barCaption("crop_canvas_to_selection", "Crop"))
                    tap(bounds(crop).center)
                } else {
                    tap(bounds("canvas-bar-more").center)
                    waitFor("$name: More opens", 5_000) { popupCount() == 1 }
                    waitFor("$name: Crop in More", 5_000) { menuText("Crop Canvas to Selection") != null }
                    settle(); tap(menuText("Crop Canvas to Selection")!!.center)
                }
                waitFor("$name: Crop shrinks the canvas to the selection", 5_000) { size().first < width && size().second < height }
                val cropped = size()
                awaitPixels("$name: the image stays in place and the cropped stroke is hidden", listOf(hiddenPoint, insidePoint)) { (h, i) -> !shows(h, rgba) && shows(i, rgba) }
                history("undo")
                waitFor("$name: one undo step restores the canvas", 5_000) { size() == original }
                awaitPixels("$name: undo shows the cropped stroke again", listOf(hiddenPoint)) { (h) -> shows(h, rgba) }
                history("redo")
                waitFor("$name: redo crops again", 5_000) { size() == cropped }
                if (hasSelection()) command("deselect")

                chooseFromApplicationMenu("edit", listOf("Image", "Canvas Size…"))
                waitFor("$name: the Canvas Size panel", 5_000) { panel() != null && shown("canvas-size-panel") }
                onMain { assertTrue("$name: the panel leaves window focus with the canvas", owner.view.hasWindowFocus()) }
                assertFalse("$name: no field is edited when the panel opens", host.editingText)
                assertFalse("$name: Apply is disabled at the current size", panel()!!.getBoolean("can_apply"))
                var values = listOf(width, height)
                when (device) {
                    MotionEvent.TOOL_TYPE_FINGER -> {
                        tap(bounds("canvas-size-relative").center)
                        waitFor("$name: Relative", 5_000) { panel()!!.getBoolean("relative") }
                        values = listOf(width - cropped.first, height - cropped.second)
                    }
                    MotionEvent.TOOL_TYPE_STYLUS -> {
                        tap(bounds("canvas-size-unit-percent").center)
                        waitFor("$name: Percent", 5_000) { panel()!!.getString("unit") == "percent" }
                        values = listOf(width / cropped.first * 100, height / cropped.second * 100).map { Math.round(it * 100) / 100.0 }
                    }
                }
                fun text(value: Double) = if (value % 1.0 == 0.0) value.toLong().toString() else value.toString()
                type("width", text(values[0]))
                type("height", text(values[1]))
                tap(bounds("canvas-size-anchor-$corner").center)
                waitFor("$name: typed values are committed before the anchor changes", 5_000) {
                    val view = panel()!!
                    view.getString("anchor") == corner && !host.editingText &&
                        (0..1).all { kotlin.math.abs(view.getJSONArray("values").getDouble(it) - values[it]) < 1e-6 }
                }
                onMain { assertTrue("$name: the anchor picker keeps window focus with the canvas", owner.view.hasWindowFocus()) }
                assertEquals("New size: ${original.first} × ${original.second} px", panel()!!.getString("message"))
                assertTrue(panel()!!.getBoolean("can_apply"))
                if (index == 0) for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    captureCanvasBar("canvas-size-$theme", "canvas-size")
                }
                tap(bounds("canvas-size-apply").center)
                waitFor("$name: Apply restores the original size and closes the panel", 5_000) { panel() == null && size() == original && !exists("canvas-size-panel") }
                awaitPixels("$name: the hidden stroke reappears in place", listOf(hiddenPoint, insidePoint)) { (h, i) -> shows(h, rgba) && shows(i, rgba) }
                history("undo")
                waitFor("$name: one undo step returns to the cropped canvas", 5_000) { size() == cropped }
                awaitPixels("$name: undo hides the stroke again", listOf(hiddenPoint)) { (h) -> !shows(h, rgba) }
                history("redo")
                waitFor("$name: redo restores the size", 5_000) { size() == original }
                awaitPixels("$name: redo shows the stroke again", listOf(hiddenPoint)) { (h) -> shows(h, rgba) }
                if (device == MotionEvent.TOOL_TYPE_STYLUS) {
                    chooseFromApplicationMenu("edit", listOf("Image", "Canvas Size…"))
                    waitFor("$name: the panel reopens", 5_000) { shown("canvas-size-panel") }
                    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                    waitFor("$name: Back cancels the panel", 5_000) { panel() == null && !exists("canvas-size-panel") && size() == original }
                }
                println("PASS canvas size $name")
            }
        } finally {
            popupInput = false
            if (panel() != null) action(obj("type" to "canvas_size", "action" to obj("op" to "cancel")))
            for (attempt in 0 until 12) if (original.first == 0 || size() == original || runCatching { history("undo") }.isFailure) break
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS Crop on the selection bar and Canvas Size with the matching anchor hide and restore pixels in one undo step each, with mouse, finger and stylus")
    }

    @Test fun blackWhiteTintRowAppliesTheCurrentColorAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val category = filterCategory("Black & White")
        val tints = listOf(listOf(.2, .5, .1, 1.0), listOf(.7, .3, .9, 1.0), listOf(.9, .7, .1, 1.0))
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                cleanDocument(keep)
                tool = device
                val count = layerStates().size
                chooseFromApplicationMenu("filter", listOf(category, "Black & White"))
                waitFor("$name: the Filter menu inserts Black & White", 5_000) {
                    layerStates().size == count + 1 && state().getJSONObject("layer_properties").getString("title") == "Black & White"
                }
                if (!shown("layer-properties")) action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "properties"))
                waitFor("$name: the Tint row", 5_000) { shown("tint-color-bucket") && textBounds("Tint color") != null }
                val bucket = bounds("tint-color-bucket")
                val swatch = bounds("property-color-Tint color")
                val label = textBounds("Tint color")!!
                assertTrue("$name: Tint keeps a labelled row: label $label, swatch $swatch, bucket $bucket",
                    label.right <= swatch.left && swatch.right <= bucket.left
                        && listOf(label, swatch).all { it.center.y in bucket.top..bucket.bottom })
                if (index == 0) for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    captureCanvasBar("tint-row-$theme", "photo-edit")
                }
                val original = control("tint_color").toString()
                action(obj("type" to "set_color", "rgba" to JSONArray(tints[index])))
                tap(bounds("tint-color-bucket").center)
                waitFor("$name: the bucket applies the current colour to Tint", 5_000) {
                    control("tint_color").toString() == state().getJSONObject("colors").getJSONObject("foreground").toString()
                }
                command("undo")
                waitFor("$name: one undo step restores the Tint", 5_000) { control("tint_color").toString() == original }
                command("undo")
                waitFor("$name: undo removes Black & White", 5_000) { layerStates().size == count }
                println("PASS black and white tint $name")
            }
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS Black & White keeps a labelled Tint row whose bucket applies the current colour in one undo step, with mouse, finger and stylus")
    }

    @Test fun liquifyPinchStrokeMovesThePixelsUnderTheStylus() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val (width, height) = cleanDocument(keep)
        tool = MotionEvent.TOOL_TYPE_STYLUS
        layerAction(obj("op" to "new", "group" to false, "clipped" to false))
        val paint = editingLayer()
        command("brush")
        action(obj("type" to "select_brush", "id" to 1))
        action(obj("type" to "set_brush_size", "value" to height * .03))
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.05, .1, .6, 1.0))))
        for (row in 0..6) {
            val y = height * (.25 + .07 * row)
            drag(documentPoint(width * .15, y), documentPoint(width * .85, y), 12)
        }
        waitFor("the striped pattern is painted", 5_000) { paintRevision(paint) > 0 }
        command("liquify")
        action(obj("type" to "select_brush", "id" to 37))
        action(obj("type" to "set_brush_size", "value" to height * .3))
        waitFor("Liquify Pinch", 5_000) { state().getJSONObject("brush").getInt("preset") == 37 && host.snapshot?.optBoolean("brush_ready") == true }
        SystemClock.sleep(500)
        val probes = (0..8).flatMap { i -> listOf(.39, .42, .48, .51).map { y -> documentPoint(width * (.3 + .05 * i), height * y) } }
        val far = documentPoint(width * .5, height * .9)
        val before = screenPixels(probes + far)
        val revision = paintRevision(paint)
        val from = documentPoint(width * .3, height * .45)
        val to = documentPoint(width * .7, height * .45)
        val base = IntArray(2)
        onMain { owner.view.getLocationOnScreen(base) }
        val interval = 1000.0 / 240
        val duration = 1500
        fun inject(action: Int, down: Long, at: Offset) {
            val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS })
            val coords = arrayOf(MotionEvent.PointerCoords().apply {
                x = at.x + base[0]; y = at.y + base[1]; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f
            })
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
            try { assertTrue("the system accepts the stylus stroke", instrumentation.uiAutomation.injectInputEvent(event, false)) } finally { event.recycle() }
        }
        kotlinx.coroutines.runBlocking { host.withNative { Native.completionTimings(it, true) } }
        host.measurementReport(true)
        val count = (duration / interval).toInt()
        val began = System.nanoTime()
        val down = SystemClock.uptimeMillis()
        for (i in 0..count) {
            val left = began + (i * interval * 1e6).toLong() - System.nanoTime()
            if (left > 0) java.util.concurrent.locks.LockSupport.parkNanos(left)
            inject(if (i == 0) MotionEvent.ACTION_DOWN else if (i == count) MotionEvent.ACTION_UP else MotionEvent.ACTION_MOVE, down, from + (to - from) * (i / count.toFloat()))
        }
        val ended = System.nanoTime()
        SystemClock.sleep(300)
        val completions = kotlinx.coroutines.runBlocking { host.withNative { JSONArray(Native.completionTimings(it, false)) } }
        val frames = host.measurementReport(false).getJSONArray("frames").let { rows -> (0 until rows.length()).map { rows.getJSONArray(it) } }
            .filter { it.getLong(1) in began..ended && it.getLong(6) > 0 }
        waitFor("the Pinch stroke edits the pattern", 5_000) { paintRevision(paint) != revision }
        assertNull(host.actionError)
        awaitPixels("the Pinch stroke moves the stripes under it and nothing far from it", probes + far) { after ->
            probes.indices.count { !same(after[it], before[it]) } >= probes.size / 4 && same(after.last(), before.last())
        }
        fun quantiles(values: List<Double>) = values.sorted().let { v ->
            if (v.isEmpty()) obj("n" to 0)
            else obj("n" to v.size, "p50" to v[(v.size - 1) / 2], "p99" to v[((v.size - 1) * .99).toInt()], "max" to v.last(), "over_8_33" to v.count { it > 8.333 })
        }
        val completed = (0 until completions.length()).map { completions.getJSONArray(it) }.filter { it.getLong(1) in began..ended }.map { it.getLong(2) }.sorted()
        val seconds = (ended - began) / 1e9
        var refresh = 0f
        onMain { refresh = owner.view.display.refreshRate }
        val result = obj("display_hz" to refresh, "seconds" to seconds, "debuggable" to BuildConfig.DEBUG, "input_hz" to 1000 / interval,
            "submitted_hz" to frames.size / seconds, "completed_hz" to completed.size / seconds,
            "submitted_vsync_interval_ms" to quantiles(frames.map { it.getLong(0) }.sorted().zipWithNext { a, b -> (b - a) / 1e6 }),
            "gpu_completion_interval_ms" to quantiles(completed.zipWithNext { a, b -> (b - a) / 1e6 }),
            "owner_cpu_ms" to quantiles(frames.map { it.getLong(17) / 1e6 }))
        File(instrumentation.targetContext.getExternalFilesDir(null), "validation/photo-edit/pinch-frames.json").apply { parentFile!!.mkdirs() }.writeText(result.toString(2))
        println("PINCH FRAMES $result")
        assertTrue("the stroke presents frames throughout: $result", completed.size > 60 * seconds)
        captureCanvasBar("liquify-pinch", "photo-edit")
        command("undo")
        awaitPixels("one undo step restores the pattern", probes) { after -> probes.indices.all { same(after[it], before[it]) } }
        println("PASS Liquify Pinch: a stylus stroke moves the striped pixels under it, leaves distant pixels, and undoes in one step")
    }
}
