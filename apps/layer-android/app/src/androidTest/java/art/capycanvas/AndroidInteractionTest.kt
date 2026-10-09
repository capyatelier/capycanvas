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
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
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
    private var stylusButtons = 0
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
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) mouseButton
            else if (tool == MotionEvent.TOOL_TYPE_STYLUS) stylusButtons else 0
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
                        view.descendant<ViewRootForTest>()?.let { it !== owner } == true
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
        val defaults = createEnglishHostForTest()
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

    @Test fun imageRowsTouchPickingMenusAndRefusalActions() {
        fun image(name: String, color: Int, width: Int, height: Int) = java.io.File(activity.cacheDir, name).also { file ->
            val bitmap = android.graphics.Bitmap.createBitmap(width, height, android.graphics.Bitmap.Config.ARGB_8888)
            try { bitmap.eraseColor(color); file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() }
        }
        fun rows() = state().array("layers").objects()
        fun imageLayer() = rows().single { it.getInt("object_count") > 0 }
        fun images() = imageLayer().array("objects").objects()
        fun imageRow(id: Long) = images().single { it.getLong("id") == id }
        fun menuItem(label: String): Offset {
            var result: Offset? = null
            waitFor("menu item $label") {
                result = findNode(hasLabel(label))?.let { (root, node) ->
                    val origin = IntArray(2); val base = IntArray(2)
                    root.view.getLocationOnScreen(origin); owner.view.getLocationOnScreen(base)
                    node.boundsInRoot.center + Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat())
                }
                result != null
            }
            return result!!
        }
        fun menuLabel(id: Long, op: String) = kotlinx.coroutines.runBlocking { host.withNative { JSONObject(Native.query(it, obj("type" to "object_menu", "id" to id).toString())) } }
            .array("sections").values().flatMap { (it as JSONArray).objects() }.first { it.optJSONObject("action")?.optJSONObject("action")?.optString("op") == op }.getString("label")
        fun menuTap(at: Offset) { popupInput = true; try { tap(at) } finally { popupInput = false }; settle() }
        fun hold(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout() + 250L); event(MotionEvent.ACTION_UP) }
        fun drag(from: Offset, to: Offset) {
            event(MotionEvent.ACTION_DOWN, from)
            for (step in 1..10) { SystemClock.sleep(16); event(MotionEvent.ACTION_MOVE, from + (to - from) * (step / 10f)) }
            event(MotionEvent.ACTION_UP, to); settle()
        }
        fun twoFingerPan(first: Offset, second: Offset, delta: Offset) {
            val started = SystemClock.uptimeMillis()
            val properties = Array(2) { index -> MotionEvent.PointerProperties().apply { id = index; toolType = MotionEvent.TOOL_TYPE_FINGER } }
            fun send(action: Int, count: Int, shift: Offset) {
                val coords = Array(count) { index -> MotionEvent.PointerCoords().apply { val p = (if (index == 0) first else second) + shift; x = p.x; y = p.y; pressure = .7f } }
                val motion = MotionEvent.obtain(started, SystemClock.uptimeMillis(), action, count, properties.copyOf(count).requireNoNulls(), coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
                try { instrumentation.runOnMainSync { owner.view.dispatchTouchEvent(motion) } } finally { motion.recycle() }
                SystemClock.sleep(16)
            }
            send(MotionEvent.ACTION_DOWN, 1, Offset.Zero)
            send(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, Offset.Zero)
            for (step in 1..10) send(MotionEvent.ACTION_MOVE, 2, delta * (step / 10f))
            send(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, delta)
            send(MotionEvent.ACTION_UP, 1, delta)
            settle()
        }
        val red = image("rows-red.png", android.graphics.Color.RED, 300, 200)
        val blue = image("rows-blue.png", android.graphics.Color.BLUE, 200, 160)
        for (theme in listOf("light", "dark")) {
            tool = MotionEvent.TOOL_TYPE_FINGER
            host.newDocument(800, 600)
            action(obj("type" to "set_theme", "theme" to theme))
            if (group("layers").getString("active") != "layers") action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
            for (file in listOf(red, blue)) { host.importImage(file); action(obj("type" to "invoke", "command" to "apply_transform")) }
            val layer = imageLayer().getLong("id")
            assertEquals("Second placement enters the active image layer", 2, imageLayer().getInt("object_count"))
            if (!imageLayer().getBoolean("expanded")) tap(bounds("layer-expand-$layer").center)
            waitFor("expanded image rows") { imageLayer().getBoolean("expanded") && images().size == 2 }
            val (front, back) = images().map { it.getLong("id") }
            waitFor("child rows") { shown("image-object-row-$front") && shown("image-object-row-$back") }
            waitFor("image row previews", 30_000) { exists("image-object-thumbnail-$front") && exists("image-object-thumbnail-$back") }
            assertEquals("Child rows keep layer row height", bounds("layer-row-$layer").height, bounds("image-object-row-$front").height, 1f)
            tap(bounds("image-object-row-$back").center)
            waitFor("row tap selects the obscured image") { imageRow(back).getBoolean("selected") && !imageRow(front).getBoolean("selected") }
            tap(bounds("image-object-eye-$front").center)
            waitFor("image hidden") { !imageRow(front).getBoolean("visible") }
            tap(bounds("image-object-eye-$front").center)
            waitFor("image shown") { imageRow(front).getBoolean("visible") }
            screenshot("validation/image-rows/rows-$theme.png")
            hold(bounds("image-object-row-$back").center)
            val duplicate = menuItem(menuLabel(back, "duplicate"))
            screenshot("validation/image-rows/menu-$theme.png")
            menuTap(duplicate)
            waitFor("duplicate adds an image") { images().size == 3 }
            val copy = images().map { it.getLong("id") }.single { it != front && it != back }
            waitFor("copy row") { shown("image-object-row-$copy") }
            hold(bounds("image-object-row-$copy").center)
            menuTap(menuItem(menuLabel(copy, "delete")))
            waitFor("delete removes the copy") { images().size == 2 }
            val grip = bounds("image-object-row-$back").let { Offset(it.right - 8 * density, it.center.y) }
            drag(grip, bounds("image-object-row-$front").let { Offset(it.center.x, it.top + 4 * density) })
            waitFor("drag reorders within the layer") { images().map { it.getLong("id") } == listOf(back, front) }
            action(obj("type" to "invoke", "command" to "undo"))
            waitFor("reorder undo") { images().map { it.getLong("id") } == listOf(front, back) }
            action(obj("type" to "invoke", "command" to "move"))
            action(obj("type" to "object", "action" to obj("op" to "deselect")))
            waitFor("deselected") { images().none { it.getBoolean("selected") } }
            tap(documentPoint(400.0, 300.0))
            host.awaitMain("touch selects the frontmost image", 10_000, { "images=${images()} active=${state().getJSONObject("layer_tools").optJSONObject("editing_layer")?.optLong("id")} layer=$layer camera=${state().getJSONObject("camera")} tap=${documentPoint(400.0, 300.0)}" }) { imageRow(front).getBoolean("selected") }
            val camera = state().getJSONObject("camera").getJSONArray("translation").toString()
            twoFingerPan(documentPoint(40.0, 40.0), documentPoint(40.0, 140.0), Offset(120 * density, 60 * density))
            waitFor("empty two-finger touch navigates") { state().getJSONObject("camera").getJSONArray("translation").toString() != camera }
            assertEquals("Navigation keeps the image selection", true, imageRow(front).getBoolean("selected"))
            screenshot("validation/image-rows/picked-$theme.png")
            val before = rows().size
            action(obj("type" to "invoke", "command" to "brush"))
            waitFor("brush ready", 60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            tool = MotionEvent.TOOL_TYPE_STYLUS
            drag(documentPoint(300.0, 250.0), documentPoint(420.0, 330.0))
            waitFor("image refusal offers actions") { exists("canvas-notice-action-new_paint_layer") && exists("canvas-notice-action-add_mask") && exists("canvas-notice-action-rasterize_layer") }
            screenshot("validation/image-rows/refusal-$theme.png")
            tool = MotionEvent.TOOL_TYPE_FINGER
            tap(bounds("canvas-notice-action-new_paint_layer").center)
            waitFor("new paint layer above the image layer") { rows().size == before + 1 }
            val created = rows().first { it.getBoolean("editing") }.getLong("paint_revision")
            SystemClock.sleep(500); settle()
            assertEquals("The refused stroke is not replayed", created, rows().first { it.getBoolean("editing") }.getLong("paint_revision"))
            val ink = documentPoint(360.0, 290.0)
            val pixel = onScreen { image, origin -> image.getPixel((ink.x + origin[0]).toInt(), (ink.y + origin[1]).toInt()) }
            assertTrue("The image stays visible where the refused stroke passed", android.graphics.Color.blue(pixel) > 200 && android.graphics.Color.red(pixel) < 60)
            assertNull(host.actionError)
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
        for (theme in listOf("light", "dark")) for (pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER)) {
            action(obj("type" to "set_theme", "theme" to theme)); tool = pointer
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
                if (panel == "sizes") {
                    assertEquals(bounds("size-preset-0.7").top, bounds("size-preset-3").top, 1.1f)
                    assertEquals(44 * density, bounds("size-preset-0.7").height, 1.1f)
                    tap(bounds("size-preset-1.5").center); settle(); assertEquals(1.5f, state().getJSONObject("brush").number("diameter"), .001f)
                }
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
                        val presets = host.catalog.array("brush_sizes").objects()
                        val firstPreset = bounds("size-preset-${presets[0].getString("label")}")
                        val thirdPreset = bounds("size-preset-${presets[2].getString("label")}")
                        if (index == 0) assertTrue("Narrow presets wrap live", thirdPreset.top > firstPreset.top)
                        else assertEquals("Wide panels fit more than six sizes", firstPreset.top, bounds("size-preset-4").top, 1.1f)
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
                        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/resize-$panel-$index-$theme.png")
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
        fun idle(label: String) {
            repeat(2) { host.drain() }
            waitFor("$label: the canvas interaction finishes", 5_000) {
                state().array("commands").objects().any { it.getString("id") == "add_layer" && it.getBoolean("enabled") }
            }
        }
        fun modeless(label: String) = onMain {
            assertNull("$label opens no dialog", host.dialogError); assertNull("$label opens no dialog", host.hostError)
            assertTrue("$label leaves window focus with the canvas", owner.view.hasWindowFocus())
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val bands = fixture.getJSONObject("layout").array("bands").objects()
        val wide = JSONObject(fixture.toString()).apply { getJSONObject("layout").put("bands", JSONArray(bands.filter { it.getInt("id") == 44 })) }
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                action(obj("type" to "restore_workspace", "workspace" to wide))
                layer(obj("op" to "new", "group" to false, "clipped" to false))
                val top = active()
                invoke("fit_canvas")
                val work = bounds("workspace")
                val extent = state().array("tabs").objects().first { it.getBoolean("active") }
                val point = documentPoint(extent.getInt("width") * .7, extent.getInt("height") * .3)
                tool = device
                val drawing = if (device == MotionEvent.TOOL_TYPE_FINGER) MotionEvent.TOOL_TYPE_STYLUS else device
                fun onCanvas(gesture: () -> Unit) { tool = drawing; try { gesture() } finally { tool = device } }
                invoke("auto_select"); invoke("selection_reference")
                for (theme in if (device == MotionEvent.TOOL_TYPE_STYLUS) listOf("light", "dark") else listOf(null)) {
                    theme?.let { action(obj("type" to "set_theme", "theme" to it)) }
                    val before = notice()?.optLong("id") ?: 0L
                    onCanvas { tap(point) }
                    waitFor("$name Wand offers a reference", 5_000) {
                        (notice()?.optLong("id") ?: 0L) > before && (notice()!!.optJSONArray("actions")?.length() ?: 0) > 0 && shown("canvas-notice-action-use_reference")
                    }
                    theme?.let { captureCanvasBar("wand-$it", "canvas-notice") }
                }
                val offer = notice()!!
                assertEquals("This tool samples reference layers, and none is marked", offer.getString("text"))
                assertNotNull("$name the notice shows the core's text", textBounds(offer.getString("text")))
                val label = offer.getJSONArray("actions").getJSONObject(0).getString("label")
                assertNotNull("$name the notice shows its action", textBounds(label))
                val below = layers().first { "Use ${it.getString("label")} as Reference" == label }.getLong("id")
                modeless("$name Wand notice")
                tap(bounds("canvas-notice-action-use_reference").center)
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
                assertTrue("$name the refusal has no action", notice()!!.getJSONArray("actions").length() == 0 && !exists("canvas-notice-action-use_reference"))
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
    @Test fun navigationControlsPreservePaintAndRestoreTool() {
        fun camera() = state().getJSONObject("camera")
        fun zoom() = camera().getDouble("zoom")
        fun rotation() = camera().getDouble("rotation")
        fun undo() = state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled")
        fun revisions() = layerStates().map { it.getLong("id") to it.getLong("paint_revision") }
        fun key(code: Int, down: Boolean, meta: Int = 0) {
            val now = SystemClock.uptimeMillis()
            instrumentation.sendKeySync(KeyEvent(now, now, if (down) KeyEvent.ACTION_DOWN else KeyEvent.ACTION_UP,
                code, 0, meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
            settle()
        }
        val control = KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON
        val alt = KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON
        val shift = KeyEvent.META_SHIFT_ON or KeyEvent.META_SHIFT_LEFT_ON
        tool = MotionEvent.TOOL_TYPE_MOUSE
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            command("reset_view"); command("pen")
            onMain { surface.requestFocus() }
            val work = camera().getJSONArray("work_area")
            val center = Offset(work.getDouble(0).toFloat() + work.getDouble(2).toFloat() / 2,
                work.getDouble(1).toFloat() + work.getDouble(3).toFloat() / 2)
            val kept = revisions(); val history = undo()
            pressKey(KeyEvent.KEYCODE_Z)
            waitFor("Z selects Zoom") { canvasTool() == "zoom" }
            val initial = zoom(); tap(center)
            waitFor("Zoom click increases magnification") { zoom() > initial }
            val first = zoom(); tap(center)
            waitFor("rapid Zoom canvas clicks keep increasing magnification") { zoom() > first }
            val clicked = zoom(); drag(center, center + Offset(80 * density, 64 * density))
            waitFor("Zoom drag changes magnification") { zoom() != clicked }
            command("reset_view"); command("pen")
            val painting = canvasTool(); val before = camera()
            key(KeyEvent.KEYCODE_CTRL_LEFT, true, control); key(KeyEvent.KEYCODE_SPACE, true, control)
            event(MotionEvent.ACTION_DOWN, center)
            event(MotionEvent.ACTION_MOVE, center + Offset(48 * density, 0f))
            waitFor("Ctrl+Space drag right zooms in") { zoom() > before.getDouble("zoom") }
            val zoomed = camera(); val anchored = listOf(center.x, center.y)
            for (axis in 0..1) assertEquals("zoom retains its anchor",
                (anchored[axis] - before.getJSONArray("translation").getDouble(axis)) / before.getDouble("zoom"),
                (anchored[axis] - zoomed.getJSONArray("translation").getDouble(axis)) / zoomed.getDouble("zoom"), .05)
            key(KeyEvent.KEYCODE_SPACE, false, control); key(KeyEvent.KEYCODE_CTRL_LEFT, false)
            event(MotionEvent.ACTION_MOVE, center + Offset(80 * density, 0f))
            waitFor("key release keeps captured zoom") { zoom() > zoomed.getDouble("zoom") }
            event(MotionEvent.ACTION_UP)
            waitFor("temporary zoom restores painting") { canvasTool() == painting }
            val enlarged = zoom()
            key(KeyEvent.KEYCODE_ALT_LEFT, true, alt); key(KeyEvent.KEYCODE_SPACE, true, alt)
            tap(center)
            key(KeyEvent.KEYCODE_SPACE, false, alt); key(KeyEvent.KEYCODE_ALT_LEFT, false)
            waitFor("Alt+Space click zooms out") { zoom() < enlarged }
            val start = center + Offset(80 * density, 0f)
            key(KeyEvent.KEYCODE_SHIFT_LEFT, true, shift); key(KeyEvent.KEYCODE_SPACE, true, shift)
            event(MotionEvent.ACTION_DOWN, start)
            event(MotionEvent.ACTION_MOVE, start + Offset(0f, 64 * density))
            waitFor("Shift+Space rotates the view") { kotlin.math.abs(rotation()) > .01 }
            val turned = rotation()
            key(KeyEvent.KEYCODE_SPACE, false, shift); key(KeyEvent.KEYCODE_SHIFT_LEFT, false)
            event(MotionEvent.ACTION_MOVE, center + Offset(0f, 80 * density))
            event(MotionEvent.ACTION_UP)
            waitFor("key release keeps captured rotation") { kotlin.math.abs(rotation() - turned) > .01 }
            assertEquals(painting, canvasTool())
            pressKey(KeyEvent.KEYCODE_R)
            waitFor("R selects Rotate View") { canvasTool() == "rotate_view" }
            val selected = rotation(); pressKey(KeyEvent.KEYCODE_MINUS)
            waitFor("minus rotates left") { rotation() != selected }
            pressKey(KeyEvent.KEYCODE_5)
            waitFor("5 resets rotation") { kotlin.math.abs(rotation()) < .00001 }
            for ((code, meta) in listOf(KeyEvent.KEYCODE_SEMICOLON to control, KeyEvent.KEYCODE_EQUALS to control,
                KeyEvent.KEYCODE_EQUALS to (control or shift), KeyEvent.KEYCODE_NUMPAD_ADD to control)) {
                val beforeKey = zoom(); pressKey(code, meta)
                waitFor("$code/$meta zoom shortcut applies") { zoom() > beforeKey }
            }
            action(obj("type" to "set_rotation", "rotation" to .4))
            command("flip_horizontal"); command("fit_canvas")
            assertEquals("Fit preserves rotation", .4, rotation(), .00001)
            assertTrue("Fit preserves reflection", camera().getJSONArray("flipped").getBoolean(0))
            command("reset_rotation")
            assertEquals(0.0, rotation(), .00001)
            assertTrue("Reset Rotation preserves reflection", camera().getJSONArray("flipped").getBoolean(0))
            command("reset_view")
            assertEquals(0.0, rotation(), .00001)
            assertEquals("[false,false]", camera().getJSONArray("flipped").toString())
            assertEquals("navigation never changes paint", kept, revisions())
            assertEquals("navigation preserves artwork history", history, undo())
            captureCanvasBar("navigation-$theme", "navigation-controls")
            command("pen"); drag(center, center + Offset(32 * density, 12 * density))
            waitFor("painting works after navigation") { revisions() != kept }
            assertTrue(undo())
            command("undo")
            println("PASS navigation controls theme=$theme")
        }
    }

    @Test fun navigationToolButtonsDoubleClickWithoutResettingCanvasClicks() {
        val ids = (0..2).map { fixture.getJSONObject("layout").getInt("next_tile_id") + it }
        fixture.getJSONObject("layout").apply { put("next_tile_id", ids.last() + 1) }
        fixture.getJSONObject("layout").array("panels").objects()
            .first { it.getString("id") == "toolbar" }.getJSONObject("content")
            .put("tiles", JSONArray(ids.mapIndexed { index, id ->
                obj("id" to id, "control" to obj("kind" to "command", "command" to listOf("hand", "zoom", "rotate_view")[index]))
            }))
        restore()
        tool = MotionEvent.TOOL_TYPE_MOUSE
        fun camera() = state().getJSONObject("camera")
        fun zoom() = camera().getDouble("zoom")
        fun rotation() = camera().getDouble("rotation")
        fun button(index: Int) = bounds("tile-toolbar-${ids[index]}").center
        val work = camera().getJSONArray("work_area")
        val center = Offset(work.getDouble(0).toFloat() + work.getDouble(2).toFloat() / 2,
            work.getDouble(1).toFloat() + work.getDouble(3).toFloat() / 2)
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            command("fit_canvas")
            val fitted = zoom()
            command("zoom_in")
            command("hand")
            doubleTap(button(0))
            waitFor("Hand double click fits the canvas") { kotlin.math.abs(zoom() - fitted) < .00001 }
            assertEquals("hand", canvasTool())
            action(obj("type" to "set_zoom", "zoom" to .5))
            command("zoom")
            doubleTap(button(1))
            waitFor("Zoom double click shows actual pixels") { kotlin.math.abs(zoom() - 1.0) < .00001 }
            assertEquals("zoom", canvasTool())
            val beforeClicks = zoom()
            doubleTap(center)
            waitFor("rapid Zoom canvas clicks keep zooming") { zoom() > beforeClicks * 1.5 }
            action(obj("type" to "set_rotation", "rotation" to .4))
            command("rotate_view")
            doubleTap(button(2))
            waitFor("Rotate View double click resets rotation") { kotlin.math.abs(rotation()) < .00001 }
            assertEquals("rotate_view", canvasTool())
        }
    }

    @Test fun zoomReadoutMenuAndFieldAcrossDevices() {
        fun choose(text: String) {
            revealInMenu(hasLabel(text), "zoom-menu")
            tap(zoomItem(text)!!.center)
        }
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
            for (theme in listOf("light", "dark")) for (device in pointerTools) {
                action(obj("type" to "set_theme", "theme" to theme))
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                tool = device
                action(obj("type" to "set_zoom", "zoom" to .37))
                open(name)
                onMain {
                    val root = zoomMenuRoot()!!
                    val positions = listOf(hasTag("zoom-field"), hasLabel("Lock zoom"), hasTag("rotation-field"),
                        hasLabel("Reset rotation"), hasLabel("Lock rotation")).map { root.find(it)!!.positionInRoot.y }
                    assertTrue("$name zoom controls precede rotation controls", positions.zipWithNext().all { (a, b) -> a < b })
                    assertEquals("$name both sliders use one row", root.find(hasTag("zoom-field"))!!.size.height, root.find(hasTag("rotation-field"))!!.size.height)
                    assertNull("$name rotation has no visible label", root.find(hasLabel("Rotation")))
                }
                if (device == MotionEvent.TOOL_TYPE_STYLUS) captureCanvasBar("zoom-menu-$theme", "zoom-readout")
                choose("Lock rotation")
                waitFor("$name rotation lock applies", 5_000) { camera().getBoolean("rotation_locked") && !zoomMenuShown() }
                open(name)
                choose("Lock zoom")
                waitFor("$name zoom lock applies", 5_000) { camera().getBoolean("zoom_locked") && !zoomMenuShown() }
                open(name)
                onMain {
                    for (label in listOf("Lock rotation", "Lock zoom")) {
                        val row = generateSequence(zoomMenuRoot()?.find(hasLabel(label))) { it.parent }
                            .firstOrNull { it.config.getOrNull(SemanticsProperties.Selected) != null }
                        assertEquals("$label has a checkmark", true, row?.config?.getOrNull(SemanticsProperties.Selected))
                    }
                }
                if (device == MotionEvent.TOOL_TYPE_STYLUS) captureCanvasBar("zoom-menu-locked-$theme", "zoom-readout")
                for (id in listOf("zoom_in", "zoom_out", "rotate_right", "rotate_left", "flip_horizontal", "flip_vertical")) {
                    val before = camera()
                    revealInMenu(hasTag("zoom-$id"), "zoom-menu")
                    tap(bounds("zoom-$id").center)
                    waitFor("$name $id works while locked", 5_000) { camera().getLong("revision") > before.getLong("revision") }
                    val after = camera()
                    if (id.startsWith("zoom_")) assertNotEquals(before.getDouble("zoom"), after.getDouble("zoom"))
                    else if (id.startsWith("rotate_")) assertNotEquals(before.getDouble("rotation"), after.getDouble("rotation"))
                    else assertNotEquals(before.getJSONArray("flipped").toString(), after.getJSONArray("flipped").toString())
                    assertTrue("$name navigation buttons keep the menu open", zoomMenuShown())
                }
                revealInMenu(hasTag("number-value-Rotation"), "zoom-menu")
                tap(bounds("number-value-Rotation").center)
                waitFor("$name rotation editor takes keyboard focus", 5_000) {
                    var typing = false
                    onMain { typing = zoomMenuRoot()?.let { it.view.hasWindowFocus() && it.find(hasTag("number-Rotation")) != null } == true }
                    typing
                }
                instrumentation.sendStringSync("45")
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
                waitFor("$name typed rotation works while locked", 5_000) { kotlin.math.abs(camera().getDouble("rotation") - Math.PI / 4) < .00001 }
                revealInMenu(hasTag("rotation-field"), "zoom-menu")
                val slider = settledBounds("rotation-field")
                val value = bounds("number-value-Rotation")
                tap(Offset(slider.left + (value.left - slider.left) * .3f, slider.center.y))
                waitFor("$name rotation slider works while locked", 5_000) { kotlin.math.abs(camera().getDouble("rotation") - Math.PI / 4) > .01 }
                choose("Reset rotation")
                waitFor("$name reset rotation applies", 5_000) { kotlin.math.abs(camera().getDouble("rotation")) < .00001 && !zoomMenuShown() }
                open(name); choose("Lock rotation")
                open(name); choose("Lock zoom")
                invoke("flip_horizontal"); invoke("flip_vertical")
                open(name)
                choose("200%")
                waitFor("$name 200% applies and closes the menu", 5_000) { zoom() == 2.0 && !zoomMenuShown() }
                assertTrue("$name 200% lands on whole device pixels", whole())
                waitFor("$name the readout follows the camera", 3_000) { readout("200% · 0°") }
                canvasFocus("$name choosing 200%")

                action(obj("type" to "set_rotation", "rotation" to Math.PI / 2)); action(obj("type" to "set_zoom", "zoom" to .37))
                open(name)
                choose("Actual Pixels")
                waitFor("$name Actual Pixels applies", 5_000) { zoom() == 1.0 && !zoomMenuShown() }
                assertTrue("$name a quarter-turned 1:1 view lands on whole device pixels", whole())
                waitFor("$name the readout shows the turned 1:1 view", 3_000) { readout("100% · 90°") }
                canvasFocus("$name Actual Pixels")
                action(obj("type" to "set_rotation", "rotation" to 0))

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
                println("PASS zoom readout device=$name theme=$theme")
            }
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS zoom readout: shared menu levels, Actual Pixels at a quarter turn, typed zoom and Back with mouse, finger and stylus, without taking focus")
    }
    @Test fun layerColorModesAndLocalFilterMenu() {
        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
        layerAction(obj("op" to "new", "group" to false, "clipped" to false))
        val owner = editingLayer()
        popupInput = true
        try {
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "properties"))
                assertFalse(shown("layer-color-mode"))
                captureCanvasBar(theme, "layer-properties")
                for ((device, label) in pointerTools.zip(listOf("Grayscale", "Two-tone (black & white)", "Full color"))) {
                    tool = device
                    tap(bounds("property-color_mode").center)
                    waitFor("color modes open") { menuText(label) != null }; settle()
                    tap(menuText(label)!!.center)
                    waitFor("color mode selected") { popupCount() == 0 && state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "color_mode" }.getJSONObject("value").getInt("value") == listOf("Full color", "Grayscale", "Two-tone (black & white)").indexOf(label) }
                }
                val before = state().array("layers").length()
                for ((index, filter) in listOf("Exposure", "Curves", "Levels").withIndex()) {
                    if (index == 0) tap(bounds("properties-add-filter").center)
                    else {
                        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
                        if (index == 1) tap(bounds("layer-add-filter").center)
                        else {
                            tool = MotionEvent.TOOL_TYPE_STYLUS
                            event(MotionEvent.ACTION_DOWN, bounds("layer-row-${editingLayer()}").center); SystemClock.sleep(700)
                            event(MotionEvent.ACTION_UP); settle()
                            waitFor("layer Add Filter menu") { menuText("Add Filter") != null }
                            tap(menuText("Add Filter")!!.center)
                        }
                    }
                    waitFor("filter categories open") { menuText("Tone") != null }; settle()
                    tap(menuText("Tone")!!.center)
                    waitFor("filter submenu open") { menuText(filter) != null }; settle()
                    tap(menuText(filter)!!.center)
                    waitFor("local filter selected") { popupCount() == 0 && state().array("layers").length() == before + index + 1 && state().getJSONObject("layer_tools").getJSONObject("editing_layer").optBoolean("adjustment_effect") }
                    assertTrue(state().getJSONObject("layer_tools").has("add_filter"))
                }
                assertEquals(before + 3, state().array("layers").length())
                repeat(3) { action(obj("type" to "invoke", "command" to "undo")) }
                assertEquals(owner, editingLayer())
                action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
            }
        } finally { popupInput = false }
    }

    @Test fun layerBlendMenuAcrossDevices() {
        fun blend() = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getString("blend_label")
        fun groups() = kotlinx.coroutines.runBlocking {
            host.withNative { JSONObject(Native.query(it, obj("type" to "layer_blend_menu", "id" to editingLayer()).toString())) }
        }.array("sections").let { sections -> (0 until sections.length()).map { i -> sections.getJSONArray(i).objects().map { it.getString("label") } } }
        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
        layerAction(obj("op" to "new", "group" to false, "clipped" to false))
        waitFor("the blend control shows", 5_000) { shown("layer-blend") }
        assertEquals(listOf(listOf("Normal"), listOf("Darken", "Multiply", "Color Burn", "Linear Burn"),
            listOf("Lighten", "Screen", "Color Dodge", "Add"),
            listOf("Overlay", "Soft Light", "Hard Light", "Vivid Light", "Linear Light", "Pin Light", "Hard Mix"),
            listOf("Difference", "Exclusion", "Subtract", "Divide"), listOf("Hue", "Saturation", "Color", "Luminosity")), groups())
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            for ((device, mode) in pointerTools.zip(listOf("Multiply", "Overlay", "Screen"))) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                tool = device
                tap(bounds("layer-blend").center)
                waitFor("$name opens the grouped blend menu", 5_000) { popupCount() == 1 && menuText("Color Burn") != null && menuText(mode) != null }
                if (device == MotionEvent.TOOL_TYPE_STYLUS) for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    waitFor("$name the menu stays open across themes", 3_000) { popupCount() == 1 }
                    captureCanvasBar("blend-menu-$theme", "blend-menu")
                }
                settle()
                tap(menuText(mode)!!.center)
                waitFor("$name chooses $mode", 5_000) { popupCount() == 0 && blend() == mode }
                waitFor("$name the control shows $mode", 3_000) { textBounds(mode) != null }
                println("PASS layer blend menu device=$name")
            }
            action(obj("type" to "invoke", "command" to "undo"))
            assertEquals("each choice is one undo step", "Overlay", blend())
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS layer blend menu: shared groups, choices with mouse, finger and stylus, one undo step each")
    }
    @Test fun passThroughGroupAndNewGroupPreferenceAcrossDevices() {
        fun blend() = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getString("blend_label")
        fun firstGroup() = kotlinx.coroutines.runBlocking {
            host.withNative { JSONObject(Native.query(it, obj("type" to "layer_blend_menu", "id" to editingLayer()).toString())) }
        }.array("sections").getJSONArray(0).objects().map { it.getString("label") }
        fun preference(value: Boolean) = action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "pass_through_groups", "value" to value)))
        fun spread(name: String): Int {
            waitFor("the canvas is ready for a pixel check", 120_000) { snapshot().optBoolean("shaders_ready") && snapshot().optBoolean("brush_ready") }
            var spread = 0
            captureCanvasBar(name, "pass-through") { image, origin ->
                val center = bounds("workspace").center
                val pixel = image.getPixel((center.x + origin.x).toInt(), (center.y + origin.y).toInt())
                val channels = listOf(android.graphics.Color.red(pixel), android.graphics.Color.green(pixel), android.graphics.Color.blue(pixel))
                spread = channels.max() - channels.min()
            }
            return spread
        }
        val settings = state().getJSONObject("settings")
        val originalTheme = settings.opt("theme") ?: JSONObject.NULL
        val originalPass = settings.getBoolean("pass_through_groups")
        action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
        popupInput = true
        try {
            preference(false)
            action(obj("type" to "invoke", "command" to "fit_canvas"))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(0.9, 0.08, 0.05, 1.0))))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "solid_color")))
            val fill = editingLayer()
            layerAction(obj("op" to "new", "group" to true, "clipped" to false))
            val group = editingLayer()
            assertEquals("new groups are isolated by default", "Normal", blend())
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "black_white")))
            action(obj("type" to "select_panel_tab", "group" to 43, "panel" to "layers"))
            layerAction(obj("op" to "select", "id" to group, "mask" to false))
            waitFor("the group is active", 5_000) { editingLayer() == group && shown("layer-blend") }
            assertTrue("inside an isolated group the adjustment leaves the red fill alone", spread("isolated") > 100)
            assertEquals(listOf("Pass Through", "Normal"), firstGroup())
            tool = MotionEvent.TOOL_TYPE_FINGER
            tap(bounds("layer-blend").center)
            waitFor("a finger opens the group's blend menu", 5_000) { popupCount() == 1 && menuText("Pass Through") != null }
            settle()
            tap(menuText("Pass Through")!!.center)
            waitFor("a finger chooses Pass Through", 5_000) { popupCount() == 0 && blend() == "Pass Through" }
            waitFor("the control shows Pass Through", 3_000) { textBounds("Pass Through") != null }
            assertTrue("the adjustment now reaches the fill below the group", spread("passing") < 8)
            action(obj("type" to "invoke", "command" to "undo"))
            assertEquals("choosing Pass Through is one undo step", "Normal", blend())
            action(obj("type" to "invoke", "command" to "redo"))
            assertEquals("Pass Through", blend())
            tool = MotionEvent.TOOL_TYPE_STYLUS
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                tap(bounds("layer-blend").center)
                waitFor("the stylus opens the blend menu in $theme", 5_000) { popupCount() == 1 && menuText("Pass Through") != null }
                captureCanvasBar("menu-$theme", "pass-through")
                back()
            }
            layerAction(obj("op" to "select", "id" to fill, "mask" to false))
            waitFor("the fill is active", 5_000) { editingLayer() == fill }
            tool = MotionEvent.TOOL_TYPE_MOUSE
            tap(bounds("layer-blend").center)
            waitFor("the mouse opens the fill's blend menu", 5_000) { popupCount() == 1 && menuText("Normal") != null }
            assertNull("only groups offer Pass Through", menuText("Pass Through"))
            back()
            action(obj("type" to "open_settings", "page" to "canvas"))
            waitFor("the Layers preferences show", 5_000) { shown("preference-pass_through_groups") }
            captureCanvasBar("preferences-opened", "pass-through")
            tool = MotionEvent.TOOL_TYPE_FINGER
            tap(bounds("preference-pass_through_groups").center)
            waitFor("a finger turns on Use Pass Through for new groups", 5_000) {
                state().getJSONObject("settings").getBoolean("pass_through_groups")
            }
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                captureCanvasBar("preferences-$theme", "pass-through")
            }
            action(obj("type" to "close_settings"))
            layerAction(obj("op" to "new", "group" to true, "clipped" to false))
            waitFor("New Group passes through", 5_000) { editingLayer() != group && blend() == "Pass Through" }
        } finally {
            popupInput = false
            preference(originalPass)
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS pass through: a finger sets a group to Pass Through, its adjustment reaches the fill below in one undo step; only groups offer it; the preference makes New Group pass through; light and dark")
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
                clear()
                val linkedPaint = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
                if (!state().getJSONObject("layer_tools").getJSONObject("editing_layer").getBoolean("has_mask")) {
                    layerAction(obj("op" to "add_mask", "id" to linkedPaint, "replace" to false))
                    layerAction(obj("op" to "select", "id" to linkedPaint, "mask" to false))
                }
                invoke("fit_canvas"); invoke("rectangle_select")
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
                invoke("cancel_transform")
                waitFor("$device cached transform cancelled", 5_000) { barKind() == "selection" }
                val catalogBaselineStarted = android.os.SystemClock.elapsedRealtimeNanos()
                host.drain(seconds = 10)
                val catalogBaselineMs = (android.os.SystemClock.elapsedRealtimeNanos() - catalogBaselineStarted) / 1_000_000.0
                instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "No-action Catalog host round-trip device=$device ms=$catalogBaselineMs\n") })
                val cachedTransformStarted = android.os.SystemClock.elapsedRealtimeNanos()
                instrumentation.runOnMainSync { host.dispatch(obj("type" to "invoke", "command" to "scale_rotate")) }
                waitFor("$device cached transform shared state", 5_000) { barKind() == "transform" }
                val cachedSharedMs = (android.os.SystemClock.elapsedRealtimeNanos() - cachedTransformStarted) / 1_000_000.0
                instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "Linked cached Transform dispatch-to-shared-state device=$device ms=$cachedSharedMs\n") })
                waitFor("$device cached transform ready", 5_000) { shown("canvas-bar-action-apply_transform") }
                val cachedTransformMs = (android.os.SystemClock.elapsedRealtimeNanos() - cachedTransformStarted) / 1_000_000.0
                instrumentation.sendStatus(0, android.os.Bundle().apply { putString("stream", "Linked cached Transform dispatch-to-composed-ready device=$device ms=$cachedTransformMs\n") })
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
    private fun <T> onScreen(read: (android.graphics.Bitmap, IntArray) -> T): T {
        val origin = IntArray(2)
        onMain { owner.view.getLocationOnScreen(origin) }
        val image = instrumentation.uiAutomation.takeScreenshot()
        try { return read(image, origin) } finally { image.recycle() }
    }
    private fun screenPixels(points: List<Offset>): List<Int> =
        onScreen { image, origin -> points.map { image.getPixel((it.x + origin[0]).toInt(), (it.y + origin[1]).toInt()) } }
    /** The mean 0–1 RGB of the square of `radius` screen pixels around each point. */
    private fun screenMeans(points: List<Offset>, radius: Int): List<List<Double>> = onScreen { image, origin ->
        points.map { p ->
            val (x, y) = (p.x + origin[0]).toInt() to (p.y + origin[1]).toInt()
            val colors = (y - radius..y + radius).flatMap { row -> (x - radius..x + radius).map { image.getPixel(it, row) } }
            listOf(16, 8, 0).map { shift -> colors.sumOf { (it shr shift and 255) / 255.0 } / colors.size }
        }
    }
    private fun <T> awaitScreen(label: String, read: () -> T, check: (T) -> Boolean, describe: (T) -> String = { "$it" }) {
        val until = SystemClock.uptimeMillis() + 5_000
        var last = read()
        while (!check(last)) {
            if (SystemClock.uptimeMillis() > until) fail("$label: ${describe(last)}")
            SystemClock.sleep(100); last = read()
        }
    }
    private fun awaitPixels(label: String, points: List<Offset>, check: (List<Int>) -> Boolean) =
        awaitScreen(label, { screenPixels(points) }, check) { last -> "${last.map { "#%06x".format(it and 0xffffff) }}" }
    /** An item's label in the open windowless menu, never the bar or panels beneath it. */
    private fun menuText(text: String): Rect? {
        var result: Rect? = null
        onMain {
            val base = IntArray(2); owner.view.getLocationOnScreen(base)
            semanticsRoots().filter { it !== owner }
                .firstNotNullOfOrNull { root -> root.find(hasLabel(text))?.let { root to it } }?.let { (root, node) ->
                    val origin = IntArray(2); root.view.getLocationOnScreen(origin)
                    result = node.boundsInRoot.translate(Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat()))
                }
        }
        return result
    }
    /** Scroll the open menu until `text` lies inside it and the scroll has stopped. */
    private fun revealInMenu(text: String) = revealInMenu(hasLabel(text), "workspace-menu")
    private fun revealInMenu(match: (SemanticsNode) -> Boolean, menuTag: String) {
        var scrolled = false
        onMain { scrolled = scrollMenuTo(match, menuTag) }
        if (scrolled) SystemClock.sleep(1_000)
    }
    private fun scrollMenuTo(text: String): Boolean = scrollMenuTo(hasLabel(text), "workspace-menu")
    private fun scrollMenuTo(match: (SemanticsNode) -> Boolean, menuTag: String): Boolean =
        semanticsRoots().filter { it !== owner && it.find(hasTag(menuTag)) != null }
            .firstNotNullOfOrNull { root -> root.find(match)?.let { root to it } }?.let { (root, item) ->
                generateSequence(item.parent) { it.parent }.firstOrNull { it.config.getOrNull(SemanticsActions.ScrollBy) != null }?.let { list ->
                    val margin = item.size.height.toFloat()
                    val shownTop = list.boundsInRoot.top + margin
                    val shownBottom = minOf(list.boundsInRoot.bottom, root.view.height.toFloat()) - margin
                    val top = item.positionInRoot.y; val bottom = top + item.size.height
                    val by = if (bottom > shownBottom) bottom - shownBottom else if (top < shownTop) top - shownTop else 0f
                    by != 0f && list.config[SemanticsActions.ScrollBy].action!!.invoke(0f, by)
                }
            } == true
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

    /** Journey 26: select, then drag the selected pixels with Move, with and without Leave Copy. */
    @Test fun moveDragsSelectedPixelsAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        command("fit_canvas")
        fun leaveCopy() = state().array("commands").objects().first { it.getString("id") == "move_leave_copy" }.getBoolean("selected")
        fun anchor() = canvasBar()?.optJSONArray("anchor")?.let { a -> List(4) { a.getDouble(it) } }
        for (device in pointerTools) for (leave in listOf(false, true)) {
            val name = "${listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]}, Leave Copy $leave"
            if (state().array("commands").objects().any { it.getString("id") == "deselect" && it.getBoolean("enabled") }) command("deselect")
            layerStates().map { it.getLong("id") }.filter { it !in keep }.forEach { layerAction(obj("op" to "delete", "id" to it)) }
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .3, .9, 1))))
            command("rectangle_select")
            val extent = state().array("tabs").objects().first { it.getBoolean("active") }
            val (width, height) = extent.getInt("width").toDouble() to extent.getInt("height").toDouble()
            tool = MotionEvent.TOOL_TYPE_STYLUS
            drag(documentPoint(width * .4, height * .35), documentPoint(width * .6, height * .55))
            waitFor("$name: the selection", 5_000) { hasSelection() }
            command("fill_selection")
            command("move")
            waitFor("$name: Move offers Leave Copy on the selection bar", 5_000) {
                barKind() == "selection" && shown("canvas-bar-action-move_leave_copy")
            }
            settle()
            tool = device
            if (leaveCopy() != leave) {
                tap(bounds("canvas-bar-action-move_leave_copy").center)
                waitFor("$name: Leave Copy toggles on the bar", 5_000) { leaveCopy() == leave }
            }
            if (device == MotionEvent.TOOL_TYPE_MOUSE && leave) {
                val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
                try {
                    for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        captureCanvasBar("leave-copy-$theme", "move-selection")
                    }
                } finally { action(obj("type" to "set_theme", "theme" to originalTheme)) }
            }
            val before = anchor()!!
            val kept = documentPoint(width * .42, height * .37)
            val arrived = documentPoint(width * .64, height * .6)
            awaitPixels("$name: the filled selection", listOf(kept, arrived)) { (k, a) -> blue(k) && !blue(a) }
            var during = ""
            drag(documentPoint(width * .5, height * .45), documentPoint(width * .7, height * .65)) {
                during = "${barKind()} ${state().getJSONObject("layer_tools").getString("tool")}"
            }
            assertEquals("$name: the bar keeps the selection context and Move stays the tool", "selection move", during)
            waitFor("$name: the selection follows the pixels", 5_000) { anchor()?.let { it[0] > before[0] + width * .15 } == true }
            val moved = anchor()!!
            assertTrue("$name: whole pixels $before $moved", (0..1).all { i -> (moved[i] - before[i]).let { kotlin.math.abs(it - kotlin.math.round(it)) < 1e-3 } })
            awaitPixels("$name: the pixels arrive and the original ${if (leave) "stays" else "is cut"}", listOf(kept, arrived)) { (k, a) ->
                blue(a) && blue(k) == leave
            }
            command("undo")
            awaitPixels("$name: one undo puts the pixels back", listOf(kept, arrived)) { (k, a) -> blue(k) && !blue(a) }
            println("PASS move selection $name")
        }
        println("PASS move selection: Move drags the selected pixels with mouse, finger and stylus, with and without Leave Copy, in one undo step")
    }

    private fun clipboardManager() = instrumentation.targetContext.getSystemService(android.content.ClipboardManager::class.java)
    /** The nonce the system clipboard carries for a copy made in Capy Canvas. */
    private fun clipboardNonce(): String? { var nonce: String? = null; onMain { nonce = clipboardManager().primaryClipDescription?.extras?.getString(ClipboardController.NONCE) }; return nonce }
    private fun documentIdle() = !state().getJSONObject("document_file").getBoolean("busy") &&
        state().array("requests").objects().none { it.getJSONObject("kind").getString("type") == "document" }
    /** Read the clipboard's image as another app would, through its content URI. */
    private fun clipboardImage(): android.graphics.Bitmap {
        val finished = java.util.concurrent.CountDownLatch(1)
        var result: android.os.Bundle? = null
        val receiver = object : android.os.ResultReceiver(android.os.Handler(android.os.Looper.getMainLooper())) {
            override fun onReceiveResult(code: Int, data: android.os.Bundle) { result = data; finished.countDown() }
        }
        val reader = android.content.Intent().setClassName(instrumentation.context.packageName, ClipboardReaderActivity::class.java.name)
            .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK).putExtra("result", receiver)
        onMain { activity.startActivity(reader) }
        assertTrue("The foreground clipboard recipient replies", finished.await(15, java.util.concurrent.TimeUnit.SECONDS))
        val delivered = checkNotNull(result)
        assertNotEquals("The recipient runs under a different UID", android.os.Process.myUid(), delivered.getInt("uid"))
        assertNull("The system grants the recipient URI access", delivered.getString("error"))
        waitFor("the editor regains focus") { activity.window.decorView.hasWindowFocus() }
        val bytes = checkNotNull(delivered.getByteArray("png"))
        return checkNotNull(android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)) { "Another app cannot decode ${bytes.size} clipboard bytes" }
    }

    @Test fun clipboardSelectedGroupAndFocusedMask() {
        InstrumentationRegistry.getArguments().getString("theme")?.let { theme ->
            require(theme in listOf("light", "dark"))
            action(obj("type" to "set_theme", "theme" to theme))
        }
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val selection = blueSelection(keep)
        command("deselect")
        layerAction(obj("op" to "new", "group" to false, "clipped" to false))
        val second = editingLayer()
        command("select_all"); command("fill_selection"); command("deselect")
        action(obj("type" to "set_layer_opacity", "id" to second, "opacity" to .37))
        layerAction(obj("op" to "toggle_selection", "id" to selection.layer))
        layerAction(obj("op" to "group_selected"))
        val group = editingLayer()
        assertTrue(layerStates().first { it.getLong("id") == group }.getBoolean("group"))
        command("rectangle_select")
        val extent = state().array("tabs").objects().first { it.getBoolean("active") }
        val width = extent.getInt("width").toDouble(); val height = extent.getInt("height").toDouble()
        tool = MotionEvent.TOOL_TYPE_STYLUS
        drag(documentPoint(width * .4, height * .35), documentPoint(width * .6, height * .55))
        waitFor("the group region is selected") { hasSelection() }
        val originals = layerStates().map { it.getLong("id") }.toSet()
        fun publish(id: String) {
            val nonce = clipboardNonce()
            command(id)
            waitFor("$id publishes the native clipboard", 30_000) { clipboardNonce().let { it != null && it != nonce } && documentIdle() }
        }
        publish("copy")
        clipboardImage().let { image ->
            try { assertTrue("the grouped region exports blue pixels", blue(image.getPixel(image.width / 2, image.height / 2))) }
            finally { image.recycle() }
        }
        command("paste_in_place")
        waitFor("the selected group pastes as three editable rows", 30_000) { layerStates().size == originals.size + 3 && documentIdle() }
        val added = layerStates().filter { it.getLong("id") !in originals }
        assertEquals(1, added.count { it.getBoolean("group") })
        assertTrue("child opacity survives selected-region copy", added.any { !it.getBoolean("group") && kotlin.math.abs(it.getDouble("opacity") - .37) < 1e-6 })
        command("undo")
        waitFor("one undo removes the whole pasted group") { layerStates().size == originals.size }
        val revisions = listOf(selection.layer, second).associateWith(::paintRevision)
        publish("cut")
        waitFor("Cut erases each child without deleting the group", 30_000) {
            layerStates().map { it.getLong("id") }.toSet() == originals && revisions.all { (id, revision) -> paintRevision(id) != revision }
        }
        command("undo")
        waitFor("one undo restores both cut children") { revisions.all { (id, revision) -> paintRevision(id) == revision } }
        command("deselect")
        layerAction(obj("op" to "select", "id" to selection.layer, "mask" to false))
        command("select_all"); command("mask_selection")
        waitFor("the child mask owns editing") { barKind() == "layer_mask" }
        fun maskRevision() = layerStates().first { it.getLong("id") == selection.layer }.getLong("mask_revision")
        val rows = layerStates().size
        val contentRevision = paintRevision(selection.layer)
        val mask = maskRevision()
        publish("copy")
        clipboardImage().let { image ->
            try { assertEquals("the focused mask exports raw white coverage", android.graphics.Color.WHITE, image.getPixel(image.width / 2, image.height / 2)) }
            finally { image.recycle() }
        }
        publish("cut")
        waitFor("Cut changes only the focused mask", 30_000) { maskRevision() != mask && documentIdle() }
        assertEquals(rows, layerStates().size)
        assertEquals(contentRevision, paintRevision(selection.layer))
        assertTrue(layerStates().first { it.getLong("id") == selection.layer }.getBoolean("has_mask"))
        val erased = maskRevision()
        command("paste_image")
        waitFor("Paste writes into the focused mask", 30_000) { maskRevision() != erased && documentIdle() }
        assertEquals(rows, layerStates().size)
        assertEquals(contentRevision, paintRevision(selection.layer))
        val restored = maskRevision()
        val external = File(AppStorage.of(instrumentation.targetContext).clipboard, "mask-external.png")
        external.parentFile!!.mkdirs()
        android.graphics.Bitmap.createBitmap(32, 24, android.graphics.Bitmap.Config.ARGB_8888).let { image ->
            try { image.eraseColor(android.graphics.Color.BLACK); external.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
            finally { image.recycle() }
        }
        try {
            val uri = androidx.core.content.FileProvider.getUriForFile(instrumentation.targetContext, "${instrumentation.targetContext.packageName}.clipboard", external)
            onMain { clipboardManager().setPrimaryClip(android.content.ClipData.newUri(instrumentation.targetContext.contentResolver, "External mask", uri)) }
            command("paste_image")
            waitFor("external PNG writes to the same focused mask", 30_000) { maskRevision() != restored && documentIdle() }
            assertEquals(rows, layerStates().size)
            assertEquals(contentRevision, paintRevision(selection.layer))
            assertEquals("layer_mask", barKind())
            command("undo")
            waitFor("one undo restores the mask before external paste") { maskRevision() == restored }
        } finally { external.delete() }
    }

    @Test fun clipboardCopyPasteAcrossDevices() {
        InstrumentationRegistry.getArguments().getString("theme")?.let { theme ->
            require(theme in listOf("light", "dark"))
            action(obj("type" to "set_theme", "theme" to theme))
            assertEquals(theme, state().getString("theme"))
        }
        val keep = layerStates().map { it.getLong("id") }.toSet()
        popupInput = true
        try {
            for (device in pointerTools) {
                val name = listOf("mouse", "finger", "stylus")[pointerTools.indexOf(device)]
                tool = device
                val selection = blueSelection(keep)
                val before = clipboardNonce()
                chooseFromBarMenu("copy", listOf("Copy Merged"))
                waitFor("$name: the copy reaches the clipboard", 30_000) { clipboardNonce().let { it != null && it != before } && documentIdle() }
                val image = clipboardImage()
                val extent = state().array("tabs").objects().first { it.getBoolean("active") }
                assertEquals("$name: another app reads the selection's width", extent.getInt("width") * .2, image.width.toDouble(), 2.0)
                assertEquals("$name: and its height", extent.getInt("height") * .2, image.height.toDouble(), 2.0)
                assertTrue("$name: the PNG holds the blue selection", blue(image.getPixel(image.width / 2, image.height / 2)))
                val count = layerStates().size
                command("paste_in_place")
                waitFor("$name: Paste in Place adds a layer", 10_000) { layerStates().size == count + 1 && documentIdle() }
                assertNotEquals("$name: a copy from Capy pastes with no handles", "placement", barKind())
                layerAction(obj("op" to "visibility", "id" to selection.layer, "value" to false))
                awaitPixels("$name: the pasted layer covers the selection only", listOf(selection.inside, selection.outside)) { (inside, outside) ->
                    blue(inside) && !blue(outside)
                }
                println("$name: Copy ▾ › Copy Merged, another app reads the PNG, Paste in Place")
            }
            tool = MotionEvent.TOOL_TYPE_STYLUS
            val selection = blueSelection(keep)
            val filled = paintRevision(selection.layer)
            val before = clipboardNonce()
            chooseFromBarMenu("copy", listOf("Cut"))
            waitFor("Cut reaches the clipboard and erases the pixels", 30_000) {
                clipboardNonce().let { it != null && it != before } && documentIdle() && paintRevision(selection.layer) != filled
            }
            val count = layerStates().size
            command("paste_into")
            waitFor("Paste Into adds a masked layer from the selection", 10_000) {
                layerStates().size == count + 1 && documentIdle() && !hasSelection() &&
                    layerStates().first { it.getLong("id") == editingLayer() }.getBoolean("has_mask")
            }
            command("undo")
            waitFor("one undo step restores the selection", 5_000) { layerStates().size == count && hasSelection() }

            command("deselect")
            val sourceLayer = editingLayer()
            action(obj("type" to "set_layer_opacity", "id" to sourceLayer, "opacity" to .37))
            val beforeLayerCopy = clipboardNonce()
            command("copy")
            waitFor("Copy without a selection publishes the whole layer", 30_000) {
                clipboardNonce().let { it != null && it != beforeLayerCopy } && documentIdle()
            }
            val copiedNonce = clipboardNonce()!!
            val otherWindow = createEnglishHostForTest()
            try {
                assertEquals("A separate native window retains the rich clip", copiedNonce, Native.clipNonce(otherWindow))
            } finally { Native.destroy(otherWindow) }
            assertEquals("Closing another window keeps clipboard ownership", copiedNonce, kotlinx.coroutines.runBlocking { host.withNative { Native.clipNonce(it) } })
            command("paste_image")
            waitFor("Paste adds an editable copied layer", 30_000) { layerStates().size == count + 1 && documentIdle() }
            val copiedLayer = editingLayer()
            assertNotEquals(sourceLayer, copiedLayer)
            assertEquals("Copy preserves layer opacity", .37, layerStates().first { it.getLong("id") == copiedLayer }.getDouble("opacity"), 1e-6)
            val beforeLayerCut = clipboardNonce()
            command("cut")
            waitFor("Cut without a selection publishes and removes the layer", 30_000) {
                clipboardNonce().let { it != null && it != beforeLayerCut } && documentIdle() &&
                    layerStates().size == count && layerStates().none { it.getLong("id") == copiedLayer }
            }
            assertTrue("Cut leaves the source layer", layerStates().any { it.getLong("id") == sourceLayer })
            command("paste_image")
            waitFor("Paste restores the cut layer", 30_000) { layerStates().size == count + 1 && documentIdle() }
            assertEquals(.37, layerStates().first { it.getLong("id") == editingLayer() }.getDouble("opacity"), 1e-6)
            command("undo")
            waitFor("one undo removes the pasted cut layer", 5_000) { layerStates().size == count }
            command("undo")
            waitFor("one undo restores the cut layer identity", 5_000) { layerStates().any { it.getLong("id") == copiedLayer } }
            command("undo"); command("undo")
            waitFor("Undo restores the source layer and opacity", 5_000) {
                layerStates().size == count && layerStates().first { it.getLong("id") == sourceLayer }.getDouble("opacity") == 1.0
            }

            fun imageObjects() = kotlinx.coroutines.runBlocking { host.withNative { JSONArray(Native.imageObjects(it)).objects() } }
            val external = File(AppStorage.of(instrumentation.targetContext).clipboard, "external.png")
            external.parentFile!!.mkdirs()
            android.graphics.Bitmap.createBitmap(64, 48, android.graphics.Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.RED) }
                .compress(android.graphics.Bitmap.CompressFormat.PNG, 100, external.outputStream())
            val uri = androidx.core.content.FileProvider.getUriForFile(instrumentation.targetContext, "${instrumentation.targetContext.packageName}.clipboard", external)
            val missing = androidx.core.content.FileProvider.getUriForFile(instrumentation.targetContext, "${instrumentation.targetContext.packageName}.clipboard", File(external.parentFile, "missing.png"))
            fun externalClip(second: android.net.Uri? = null) = android.content.ClipData.newUri(instrumentation.targetContext.contentResolver, "Another app", missing).apply {
                addItem(android.content.ClipData.Item(uri)); second?.let { addItem(android.content.ClipData.Item(it)) }
                addItem(android.content.ClipData.Item("Accompanying text"))
            }
            val beforeExternal = imageObjects().map { it.getString("id") }.toSet()
            onMain { clipboardManager().setPrimaryClip(externalClip()) }
            command("paste_image")
            waitFor("a missing URI does not discard the valid image", 30_000) { layerStates().size == count + 1 && barKind() == "placement" && layerStates().sumOf { it.getInt("object_count") } == beforeExternal.size + 1 }
            assertEquals(listOf(64, 48), imageObjects().single { it.getString("id") !in beforeExternal }.getJSONArray("extent").let { listOf(it.getInt(0), it.getInt(1)) })
            command("cancel_transform")
            waitFor("cancelling removes it", 10_000) { layerStates().size == count && documentIdle() }
            assertEquals(beforeExternal, imageObjects().map { it.getString("id") }.toSet())
            val second = File(external.parentFile, "second.png")
            android.graphics.Bitmap.createBitmap(24, 16, android.graphics.Bitmap.Config.ARGB_8888).let { bitmap ->
                try { bitmap.eraseColor(android.graphics.Color.GREEN); second.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                finally { bitmap.recycle() }
            }
            val secondUri = androidx.core.content.FileProvider.getUriForFile(instrumentation.targetContext, "${instrumentation.targetContext.packageName}.clipboard", second)
            onMain { clipboardManager().setPrimaryClip(externalClip(secondUri)) }
            command("paste_image")
            waitFor("both valid image URIs enter one placement", 30_000) { layerStates().size == count + 1 && barKind() == "placement" && layerStates().sumOf { it.getInt("object_count") } == beforeExternal.size + 2 }
            val batch = imageObjects().filter { it.getString("id") !in beforeExternal }
            assertEquals(setOf(listOf(64, 48), listOf(24, 16)), batch.map { it.getJSONArray("extent").let { a -> listOf(a.getInt(0), a.getInt(1)) } }.toSet())
            batch.forEach { image -> assertEquals(1.0, image.getJSONArray("affine").getDouble(0), 1e-9); assertEquals(1.0, image.getJSONArray("affine").getDouble(3), 1e-9) }
            command("cancel_transform")
            waitFor("cancelling removes the complete image batch", 10_000) { layerStates().size == count && documentIdle() }
            assertEquals(beforeExternal, imageObjects().map { it.getString("id") }.toSet())
            second.delete()
            onMain { clipboardManager().setPrimaryClip(externalClip()) }
            command("paste_in_place")
            waitFor("Paste in Place centres another app's image without handles", 30_000) { layerStates().size == count + 1 && documentIdle() }
            assertNotEquals("placement", barKind())
            fun newImage() {
                val drawings = host.drawingTabs.rows.map { it.getLong("id") }
                command("paste_as_new_image")
                waitFor("Paste as New Image prepares and selects a new drawing", 60_000) {
                    host.drawingTabs.rows.size == drawings.size + 1 && host.drawingTabs.selected !in drawings &&
                        !host.drawingTabs.switching && !host.documents.images.working && documentIdle()
                }
                assertTrue("The source drawings remain open", host.drawingTabs.rows.map { it.getLong("id") }.containsAll(drawings))
                val extent = state().array("tabs").objects().first { it.getBoolean("active") }
                assertEquals(64, extent.getInt("width")); assertEquals(48, extent.getInt("height"))
                assertTrue("The pasted drawing needs its own save", state().getJSONObject("document_file").getBoolean("modified"))
                assertTrue(state().getJSONObject("document_file").isNull("location"))
                assertNotEquals("placement", barKind())
            }
            repeat(2) {
                newImage()
                val previous = clipboardNonce()
                command("copy")
                waitFor("A new drawing can immediately copy back to another app", 30_000) {
                    clipboardNonce().let { it != null && it != previous } && documentIdle()
                }
                val image = clipboardImage()
                try {
                    assertEquals(64, image.width); assertEquals(48, image.height)
                    assertEquals(android.graphics.Color.RED, image.getPixel(32, 24))
                } finally { image.recycle() }
            }
            fun objectIds() = imageObjects().map { it.getString("id") }.toSet()
            fun addedImageBounds(previous: Set<String>): List<Double> {
                val image = imageObjects().single { it.getString("id") !in previous }
                val affine = image.getJSONArray("affine")
                listOf(1.0, 0.0, 0.0, 1.0).forEachIndexed { index, expected -> assertEquals(expected, affine.getDouble(index), 1e-9) }
                val extent = image.getJSONArray("extent")
                val x = affine.getDouble(4); val y = affine.getDouble(5)
                return listOf(x, y, x + extent.getInt(0), y + extent.getInt(1))
            }
            command("convert_to_object")
            waitFor("the red layer becomes an editable image", 30_000) { layerStates().first { it.getLong("id") == editingLayer() }.getInt("object_count") > 0 && documentIdle() }
            command("move"); command("select_all")
            val beforeObjectCopy = clipboardNonce()
            command("copy")
            waitFor("the editable image reaches the clipboard", 30_000) { clipboardNonce().let { it != null && it != beforeObjectCopy } && documentIdle() }
            command("fit_canvas")
            val beforeViewPaste = objectIds()
            command("paste_at_view")
            waitFor("Paste at View completes", 30_000) { documentIdle() && layerStates().sumOf { it.getInt("object_count") } == beforeViewPaste.size + 1 }
            val viewBounds = addedImageBounds(beforeViewPaste)
            listOf(0.0, 0.0, 64.0, 48.0).forEachIndexed { index, expected -> assertEquals("Paste at View centres the clip", expected, viewBounds[index], 1.0) }
            val cursor = documentPoint(48.0, 31.0)
            val surfaceLocation = IntArray(2)
            onMain { surface.getLocationInWindow(surfaceLocation) }
            val now = SystemClock.uptimeMillis()
            val hover = MotionEvent.obtain(now, now, MotionEvent.ACTION_HOVER_ENTER, 1,
                arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE }),
                arrayOf(MotionEvent.PointerCoords().apply { x = cursor.x - surfaceLocation[0]; y = cursor.y - surfaceLocation[1] }), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_MOUSE, 0)
            try { onMain {
                assertTrue(surface.dispatchGenericMotionEvent(hover))
                hover.action = MotionEvent.ACTION_HOVER_MOVE; assertTrue(surface.dispatchGenericMotionEvent(hover))
            } } finally { hover.recycle() }
            settle()
            val beforeCursorPaste = objectIds()
            command("paste_at_cursor")
            waitFor("Paste at Cursor completes", 30_000) { documentIdle() && layerStates().sumOf { it.getInt("object_count") } == beforeCursorPaste.size + 1 }
            val cursorBounds = addedImageBounds(beforeCursorPaste)
            listOf(16.0, 7.0, 80.0, 55.0).forEachIndexed { index, expected -> assertEquals("Paste at Cursor uses the hovered document point", expected, cursorBounds[index], 1.0) }
            val oversized = File(AppStorage.of(instrumentation.targetContext).clipboard, "oversized.png")
            android.graphics.Bitmap.createBitmap(128, 96, android.graphics.Bitmap.Config.ARGB_8888).apply { eraseColor(android.graphics.Color.GREEN) }
                .let { bitmap -> try { oversized.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() } }
            val oversizedUri = androidx.core.content.FileProvider.getUriForFile(instrumentation.targetContext, "${instrumentation.targetContext.packageName}.clipboard", oversized)
            val existingImages = imageObjects().map { it.getString("id") }.toSet()
            onMain { clipboardManager().setPrimaryClip(android.content.ClipData.newUri(instrumentation.targetContext.contentResolver, "Oversized image", oversizedUri)) }
            command("paste_image")
            waitFor("an oversized external image opens placement at full size", 30_000) { barKind() == "placement" && layerStates().sumOf { it.getInt("object_count") } == existingImages.size + 1 }
            val added = imageObjects().single { it.getString("id") !in existingImages }
            assertEquals(128, added.getJSONArray("extent").getInt(0)); assertEquals(96, added.getJSONArray("extent").getInt(1))
            assertEquals(1.0, added.getJSONArray("affine").getDouble(0), 1e-9); assertEquals(1.0, added.getJSONArray("affine").getDouble(3), 1e-9)
            command("cancel_transform")
            waitFor("Cancel removes only the oversized placement", 10_000) { layerStates().sumOf { it.getInt("object_count") } == existingImages.size && documentIdle() }
            assertEquals(existingImages, imageObjects().map { it.getString("id") }.toSet())
            oversized.delete()
            external.delete()
        } finally {
            popupInput = false
            tool = MotionEvent.TOOL_TYPE_FINGER
        }
        println("PASS clipboard: mouse/finger/stylus menus, URI pixels, Paste Into, whole-layer Copy/Cut/Undo, external/internal New Image, Paste at View/Cursor and full-size external placement")
    }

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
        fixture.getJSONObject("layout").put("bands", JSONArray(fixture.getJSONObject("layout").array("bands").objects().filter { it.getInt("id") == 44 }))
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
    private fun chooseFromApplicationMenu(menu: String, path: List<String>, lastShown: () -> Unit = {}) {
        val label = snapshot().array("application_menus").objects().first { it.getString("id") == menu }.getString("label")
        val labelled = shown("application-menu-$menu")
        tap(bounds(if (labelled) "application-menu-$menu" else "header-menu-labels-compact").center)
        waitFor("the $label menu opens", 5_000) { popupCount() == 1 }
        for (text in if (labelled) path else listOf(label) + path) {
            waitFor("$text in the $label menu", 5_000) { menuText(text) != null }
            revealInMenu(text)
            if (text == path.last()) lastShown()
            settle()
            var at: Rect? = null
            var still = 0
            waitFor("$text rests in the $label menu", 5_000) {
                val now = menuText(text)
                still = if (now != null && !now.isEmpty && now == at) still + 1 else 0
                at = now
                still >= 4
            }
            tap(at!!.center)
        }
        waitFor("the $label menu closes", 5_000) { popupCount() == 0 }
    }
    private fun size() = state().array("tabs").objects().first { it.getBoolean("active") }.let { it.getInt("width") to it.getInt("height") }
    private fun setting(id: String) = state().array("tool_settings").objects().firstOrNull { it.getString("id") == id }?.getDouble("value") ?: Double.NaN
    private fun commandState(id: String) = state().array("commands").objects().first { it.getString("id") == id }
    private fun selected(id: String) = commandState(id).getBoolean("selected")
    private fun canvasTool() = state().getJSONObject("layer_tools").get("tool").toString()
    private fun cropping() = canvasTool() == "crop" && barKind() == "crop" && shown("canvas-action-bar")
    private fun history(id: String) { waitFor("$id is available", 10_000) { commandState(id).getBoolean("enabled") }; command(id) }
    /** Choose `path` in the open windowless menu. */
    private fun chooseInMenu(path: List<String>) {
        for (text in path) {
            waitFor("$text in the menu", 5_000) { menuText(text) != null }
            settle(); tap(menuText(text)!!.center)
        }
        waitFor("the menu closes", 5_000) { popupCount() == 0 }
    }
    private fun viaMore(path: List<String>) {
        tap(bounds("canvas-bar-more").center)
        waitFor("More opens", 5_000) { popupCount() == 1 }
        chooseInMenu(path)
    }
    /** Press a crop bar command, through More when the bar has no room for it. */
    private fun pressBar(id: String) {
        val tag = "canvas-bar-action-$id"
        waitFor("the crop bar offers $id", 5_000) { cropping() }
        if (shown(tag)) tap(bounds(tag).center) else viaMore(listOf(commandState(id).getString("label")))
    }
    /** The Crop tool: C with the mouse, Edit › Image › Crop with the others. */
    private fun openCrop(device: Int) {
        if (device == MotionEvent.TOOL_TYPE_MOUSE) instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_C)
        else chooseFromApplicationMenu("edit", listOf("Image", "Crop"))
        waitFor("the crop bar", 5_000) { cropping() }
        settle()
    }
    private fun applyCrop(name: String) {
        pressBar("apply_transform")
        waitFor("$name: Apply finishes the crop", 5_000) { canvasTool() != "crop" && barKind() != "crop" }
        assertNull(host.actionError)
    }
    private fun keyboard(): Boolean {
        var visible = false
        onMain { visible = ViewCompat.getRootWindowInsets(owner.view)?.isVisible(WindowInsetsCompat.Type.ime()) == true }
        return visible
    }
    /** A control's bounds once it stops moving with the keyboard. */
    private fun settledBounds(tag: String): Rect {
        val end = SystemClock.uptimeMillis() + 5_000
        var last = bounds(tag)
        var since = SystemClock.uptimeMillis()
        while (SystemClock.uptimeMillis() < end) {
            SystemClock.sleep(40)
            val next = bounds(tag)
            val still = SystemClock.uptimeMillis() - since
            if (next != last) { last = next; since = SystemClock.uptimeMillis() }
            else if (still >= 300 && (keyboard() == host.editingText || still >= 1_500)) return last
        }
        error("$tag keeps moving with the keyboard")
    }
    /** Replace a settings number field's text with [text], leaving it being edited. */
    private fun typeNumber(tag: String, text: String) {
        tap(settledBounds(tag).center)
        waitFor("$tag takes the keys", 5_000) { host.editingText && tagged(tag)?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_MOVE_END)
        repeat(16) { instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DEL) }
        instrumentation.sendStringSync(text)
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

    @Test fun mergeDownAndStampVisibleAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            val crossing = documentPoint(width * .5, height * .5)
            val beside = documentPoint(width * .35, height * .5)
            tool = MotionEvent.TOOL_TYPE_STYLUS
            command("pen"); action(obj("type" to "select_brush", "id" to 1)); action(obj("type" to "set_brush_size", "value" to 40))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .3, .85, 1.0))))
            val enabled = { id: String -> state().array("commands").objects().any { it.getString("id") == id && it.getBoolean("enabled") } }
            val stroke = { from: Offset, to: Offset ->
                val layer = editingLayer()
                val before = paintRevision(layer)
                drag(from, to)
                waitFor("the stroke is committed", 5_000) { paintRevision(layer) != before }
            }
            stroke(documentPoint(width * .3, height * .5), documentPoint(width * .7, height * .5))
            command("add_layer")
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.9, .6, .05, .7))))
            stroke(documentPoint(width * .5, height * .3), documentPoint(width * .5, height * .7))
            SystemClock.sleep(300)
            val points = listOf(crossing, beside)
            val before = screenPixels(points)
            val count = layerStates().size
            val unchanged = { pixels: List<Int> -> pixels.zip(before).all { (a, b) -> same(a, b) } }
            val now = SystemClock.uptimeMillis()
            val control = KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON
            instrumentation.sendKeySync(KeyEvent(now, now, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_E, 0, control))
            instrumentation.sendKeySync(KeyEvent(now, now, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_E, 0, control))
            waitFor("Ctrl+E merges down", 5_000) { layerStates().size == count - 1 }
            awaitPixels("Ctrl+E keeps the canvas", points, unchanged)
            command("undo")
            waitFor("one undo step restores both layers", 5_000) { layerStates().size == count && enabled("merge_down") }
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                tool = device
                chooseFromApplicationMenu("layer", listOf("Merge Down"))
                waitFor("$name: Layer › Merge Down replaces both layers with one", 5_000) { layerStates().size == count - 1 }
                awaitPixels("$name: Merge Down keeps the canvas", points, unchanged)
                command("undo")
                waitFor("$name: one undo step restores both layers", 5_000) { layerStates().size == count && enabled("stamp_visible") }
                chooseFromApplicationMenu("layer", listOf("Stamp Visible"))
                waitFor("$name: Stamp Visible adds the visible image on top", 5_000) {
                    layerStates().size == count + 1 && layerStates()[0].getString("label") == "Visible" && editingLayer() == layerStates()[0].getLong("id")
                }
                if (index == 0) captureCanvasBar("stamp-visible", "merge")
                val hidden = layerStates().drop(1).map { it.getLong("id") }
                hidden.forEach { layerAction(obj("op" to "visibility", "id" to it, "value" to false)) }
                awaitPixels("$name: the stamp alone shows the crossing", listOf(crossing)) { (pixel) -> same(pixel, before[0]) }
                repeat(hidden.size + 1) { command("undo") }
                waitFor("$name: undo removes the stamp and shows every layer", 5_000) {
                    layerStates().size == count && layerStates().all { it.getBoolean("visible") } && enabled("merge_down")
                }
                println("PASS merges $name")
            }
        } finally { popupInput = false }
        println("PASS Ctrl+E and Layer › Merge Down keep the canvas in one undo step, and Stamp Visible adds the visible image on top, with mouse, finger and stylus")
    }

    @Test fun documentBlendingAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            if (!selected("blend_perceptual")) command("blend_perceptual")
            val center = listOf(documentPoint(width * .5, height * .5))
            tool = MotionEvent.TOOL_TYPE_STYLUS
            command("pen"); action(obj("type" to "select_brush", "id" to 1)); action(obj("type" to "set_brush_size", "value" to 60))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0.0, 0.0, 0.0, .5))))
            command("add_layer")
            val layer = editingLayer()
            val revision = paintRevision(layer)
            drag(documentPoint(width * .3, height * .5), documentPoint(width * .7, height * .5))
            waitFor("the stroke is committed", 5_000) { paintRevision(layer) != revision }
            SystemClock.sleep(300)
            val perceptual = screenPixels(center)[0]
            val red = { pixel: Int -> android.graphics.Color.red(pixel) }
            assertTrue("half-covered black paint darkens the white page: ${red(perceptual)}", red(perceptual) in 60..235)
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                tool = device
                val (label, id) = if (index % 2 == 0) "Linear Light Blending" to "blend_linear" else "Perceptual Blending" to "blend_perceptual"
                chooseFromApplicationMenu("edit", listOf("Blending", label)) {
                    if (device == MotionEvent.TOOL_TYPE_STYLUS) for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        waitFor("$name: the menu stays open across themes", 3_000) { popupCount() >= 1 && menuText(label) != null }
                        captureCanvasBar("blending-menu-$theme", "blending")
                    }
                }
                waitFor("$name: Edit › Blending › $label", 5_000) { selected(id) }
                if (id == "blend_linear") awaitPixels("$name: linear light shows the half-covered paint lighter", center) { (pixel) -> red(pixel) > red(perceptual) + 15 }
                else awaitPixels("$name: Perceptual returns the canvas", center) { (pixel) -> same(pixel, perceptual) }
                println("PASS document blending $name")
            }
            command("undo")
            waitFor("one undo step restores Perceptual", 5_000) { selected("blend_perceptual") && !selected("blend_linear") }
            awaitPixels("undo restores the canvas", center) { (pixel) -> same(pixel, perceptual) }
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS Edit › Blending with mouse, finger and stylus changes the canvas in one undo step each")
    }

    @Test fun dodgeBurnAndFrequencySeparationAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        fun separation() = state().getJSONObject("layer_tools").objectOrNull("frequency_separation")
        fun labels() = layerStates().map { it.getString("label") }
        fun brightness(pixel: Int) = listOf(16, 8, 0).sumOf { pixel shr it and 255 }
        fun rgb(vararg values: Double) = JSONArray(values.toList())
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            if (!selected("blend_perceptual")) command("blend_perceptual")
            val stroke = { from: Offset, to: Offset ->
                val layer = editingLayer()
                val before = paintRevision(layer)
                val device = tool
                tool = MotionEvent.TOOL_TYPE_STYLUS
                drag(from, to)
                tool = device
                waitFor("the stroke is committed", 5_000) { paintRevision(layer) != before }
            }
            fun at(x: Double, y: Double) = documentPoint(width * x, height * y)
            command("pen"); action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_brush_size", "value" to height * .3)); action(obj("type" to "set_color", "rgba" to rgb(.2, .35, .6, 1.0)))
            stroke(at(.15, .5), at(.85, .5))
            action(obj("type" to "set_brush_size", "value" to height * .06)); action(obj("type" to "set_color", "rgba" to rgb(.95, .8, .2, 1.0)))
            stroke(at(.6, .4), at(.6, .6))
            val photo = editingLayer()
            val count = layerStates().size
            val flat = at(.3, .5)
            val edge = at(.6 + height * .035 / width, .5)
            val across = (-12..12).map { at(.6 + it * 5.0 / width, .5) }
            val mark = at(.75, .5)
            SystemClock.sleep(300)
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                tool = device
                val original = screenPixels(listOf(flat, edge, mark))
                val sharp = screenPixels(across)
                chooseFromApplicationMenu("layer", listOf("New", "New Dodge & Burn Layer"))
                waitFor("$name: a Dodge & Burn layer is added and active", 5_000) {
                    layerStates().size == count + 1 && layerStates().first { it.getLong("id") == editingLayer() }.getString("label") == "Dodge & Burn"
                }
                awaitPixels("$name: the gray layer leaves the canvas as it was", listOf(flat, edge)) { pixels -> pixels.zip(original).all { (a, b) -> same(a, b) } }
                command("airbrush"); action(obj("type" to "set_brush_size", "value" to height * .08)); action(obj("type" to "set_brush_opacity", "value" to .3))
                action(obj("type" to "set_color", "rgba" to rgb(1.0, 1.0, 1.0, 1.0)))
                stroke(at(.2, .45), at(.4, .45))
                action(obj("type" to "set_color", "rgba" to rgb(0.0, 0.0, 0.0, 1.0)))
                stroke(at(.2, .55), at(.4, .55))
                awaitPixels("$name: white dodges and black burns", listOf(at(.3, .45), at(.3, .55))) { (dodged, burned) ->
                    brightness(dodged) > brightness(original[0]) + 6 && brightness(burned) < brightness(original[0]) - 6
                }
                repeat(3) { command("undo") }
                waitFor("$name: undo removes the strokes and the layer", 5_000) { layerStates().size == count }

                layerAction(obj("op" to "select", "id" to photo, "mask" to false))
                chooseFromApplicationMenu("filter", listOf("Frequency Separation…"))
                waitFor("$name: the Frequency Separation panel opens", 5_000) { separation() != null && shown("frequency-separation-panel") }
                assertEquals("Frequency Separation", separation()!!.getString("title"))
                assertNotNull("$name: the panel names the value", textBounds("Radius"))
                val slider = settledBounds("setting-slider-frequency-separation")
                fun along(f: Float) = Offset(slider.left + slider.width * f, slider.center.y)
                drag(along(.19f), along(.45f), 20)
                waitFor("$name: dragging the slider changes the radius", 5_000) { (separation()?.number("radius") ?: 0f) > 6f }
                assertEquals("$name: the preview adds no layer", count, layerStates().size)
                awaitPixels("$name: the canvas previews the blur", across) { pixels -> pixels.zip(sharp).count { (a, b) -> !same(a, b) } >= 2 }
                if (index == 0) {
                    for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        captureCanvasBar("frequency-separation-$theme", "retouch-layers")
                    }
                    action(obj("type" to "set_theme", "theme" to originalTheme))
                    tap(bounds("frequency-separation-cancel").center)
                    waitFor("$name: Cancel closes the panel", 5_000) { separation() == null && !exists("frequency-separation-panel") }
                    assertEquals("$name: Cancel leaves nothing", count, layerStates().size)
                    awaitPixels("$name: Cancel leaves the canvas as it was", listOf(flat, edge)) { pixels -> pixels.zip(original).all { (a, b) -> same(a, b) } }
                    layerAction(obj("op" to "select", "id" to photo, "mask" to false))
                    chooseFromApplicationMenu("filter", listOf("Frequency Separation…"))
                    waitFor("$name: the panel opens again", 5_000) { separation() != null && shown("frequency-separation-panel") }
                }
                tap(bounds("frequency-separation-apply").center)
                waitFor("$name: Apply splits the layer", 10_000) {
                    separation() == null && labels().take(3) == listOf("Frequency Separation", "High", "Low") &&
                        layerStates().first { it.getLong("id") == editingLayer() }.getString("label") == "High"
                }
                assertFalse("$name: the photo stays below, hidden", layerStates().first { it.getLong("id") == photo }.getBoolean("visible"))
                awaitPixels("$name: Low and High recombine", listOf(flat, edge, mark)) { pixels -> pixels.zip(original).all { (a, b) -> same(a, b) } }
                command("pen"); action(obj("type" to "set_brush_size", "value" to height * .01)); action(obj("type" to "set_brush_opacity", "value" to 1.0))
                action(obj("type" to "set_color", "rgba" to rgb(.9, .1, .1, 1.0)))
                stroke(at(.72, .5), at(.78, .5))
                awaitPixels("$name: a small brush paints on High", listOf(mark)) { (pixel) -> !same(pixel, original[2]) }
                command("undo")
                awaitPixels("$name: one undo removes the stroke", listOf(mark)) { (pixel) -> same(pixel, original[2]) }
                command("undo")
                waitFor("$name: one undo removes Frequency Separation", 5_000) {
                    layerStates().size == count && layerStates().all { it.getBoolean("visible") }
                }
                layerAction(obj("op" to "select", "id" to photo, "mask" to false))
                println("PASS retouch layers $name")
            }
        } finally {
            popupInput = false
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS New Dodge & Burn Layer and Frequency Separation from the menus with mouse, finger and stylus, each one undo step")
    }

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
        fun panel() = state().getJSONObject("layer_tools").optJSONObject("canvas_size")
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
                typeNumber("setting-number-canvas-size-width", text(values[0]))
                typeNumber("setting-number-canvas-size-height", text(values[1]))
                tap(settledBounds("canvas-size-anchor-$corner").center)
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
                tap(settledBounds("canvas-size-apply").center)
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

    @Test fun cropJourneysAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val blueFill = listOf(.1, .3, .8, 1.0)
        val paper = listOf(1.0, 1.0, 1.0, 1.0)
        fun translation() = state().getJSONObject("camera").getJSONArray("translation").let { it.getDouble(0) to it.getDouble(1) }
        fun pick(group: String, label: String) {
            val tag = "canvas-bar-choice-$group"
            if (!shown(tag)) {
                val choice = canvasBar()!!.array("items").objects().first { it.getJSONObject("option").optJSONObject("Choice")?.getString("id") == group }
                return viaMore(listOf(choice.getJSONObject("option").getJSONObject("Choice").getString("label"), label))
            }
            tap(bounds(tag).center)
            waitFor("the $group dropdown opens", 5_000) { popupCount() == 1 }
            onMain { assertTrue("the $group dropdown leaves window focus with the canvas", owner.view.hasWindowFocus()) }
            chooseInMenu(listOf(label))
        }
        fun twoFingers(from: Offset, by: Offset) {
            fun send(action: Int, points: List<Offset>) {
                val properties = points.indices.map { i -> MotionEvent.PointerProperties().apply { id = i; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray()
                val coords = points.map { p -> MotionEvent.PointerCoords().apply { x = p.x; y = p.y; pressure = .7f } }.toTypedArray()
                val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, points.size, properties, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
                try { onMain { owner.view.dispatchTouchEvent(motion) } } finally { motion.recycle() }
            }
            val pair = listOf(from - Offset(0f, 60f), from + Offset(0f, 60f))
            downAt = SystemClock.uptimeMillis()
            send(MotionEvent.ACTION_DOWN, pair.take(1))
            send(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), pair)
            for (i in 1..8) { SystemClock.sleep(16); send(MotionEvent.ACTION_MOVE, pair.map { it + by * (i / 8f) }) }
            send(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), pair.map { it + by })
            send(MotionEvent.ACTION_UP, pair.take(1).map { it + by })
        }
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            command("zoom_out"); SystemClock.sleep(300)
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            action(obj("type" to "set_color", "rgba" to JSONArray(blueFill)))
            command("rectangle_select")
            tool = MotionEvent.TOOL_TYPE_STYLUS
            drag(documentPoint(width * .2, height * .2), documentPoint(width * .8, height * .8))
            waitFor("the selection", 5_000) { hasSelection() }
            command("fill_selection"); command("deselect")
            val center = documentPoint(width * .5, height * .5)
            awaitPixels("the fill paints the middle of the canvas", listOf(center)) { (c) -> shows(c, blueFill) }
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                tool = device
                openCrop(device)
                if (selected("crop_delete_cropped_pixels")) {
                    pressBar("crop_delete_cropped_pixels")
                    waitFor("$name: Delete Cropped Pixels turns off", 5_000) { !selected("crop_delete_cropped_pixels") }
                }
                assertEquals("$name: the frame starts as the whole canvas", listOf(width, height, 0.0), listOf(setting("crop_width"), setting("crop_height"), setting("crop_angle")))
                pick("crop-ratio", "1:1")
                waitFor("$name: 1:1 constrains the crop", 5_000) { selected("crop_ratio_square") }
                val side = minOf(width, height)
                assertEquals("$name: 1:1 fits the largest square", listOf(side, side), listOf(setting("crop_width"), setting("crop_height")))
                val corner = documentPoint((width - side) / 2, (height - side) / 2)
                drag(corner, corner + Offset(120f, 60f))
                waitFor("$name drags the corner handle", 5_000) { setting("crop_width") < side - 20 }
                val cropped = setting("crop_width")
                assertEquals("$name: the ratio holds", cropped, setting("crop_height"), .01)
                waitFor("$name: the crop bar returns after the drag", 5_000) { cropping() }
                if (index == 0) {
                    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_O)
                    waitFor("O cycles the overlay", 5_000) { selected("crop_overlay_grid") }
                    pick("crop-overlay", "Diagonal")
                    waitFor("the Overlay dropdown chooses Diagonal", 5_000) { selected("crop_overlay_diagonal") }
                    pick("crop-overlay", "Thirds")
                    waitFor("the Overlay dropdown chooses Thirds", 5_000) { selected("crop_overlay_thirds") }
                    val inside = documentPoint((width + side) / 2 - 80, height - 80)
                    val outside = documentPoint(width * .02, height * .1)
                    awaitPixels("the shield keeps a fifth of the light outside the frame only", listOf(inside, outside)) { (i, o) ->
                        shows(i, paper) && listOf(16, 8, 0).all { (o shr it and 0xff) in 96..154 }
                    }
                    for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        captureCanvasBar("crop-$theme", "crop")
                    }
                    action(obj("type" to "set_theme", "theme" to originalTheme))
                    snapshot().getJSONObject("layout").array("groups").objects().firstOrNull { "tool_settings" in it.array("panels").values() }?.let {
                        action(obj("type" to "select_panel_tab", "group" to it.getLong("id"), "panel" to "tool_settings"))
                    }
                    if (shown("tool-setting-crop_width")) {
                        tap(bounds("number-value-Width").center)
                        waitFor("Width opens for editing", 5_000) { exists("number-Width") }
                        tap(bounds("number-Width").center)
                        waitFor("Width takes the keys", 5_000) { host.editingText }
                        val typed = Math.round(cropped) - 100
                        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_MOVE_END)
                        repeat(8) { instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DEL) }
                        instrumentation.sendStringSync(typed.toString())
                        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
                        waitFor("Tool Options sets the width, keeping 1:1", 5_000) { setting("crop_width") == typed.toDouble() && setting("crop_height") == typed.toDouble() && !host.editingText }
                    } else println("Tool Options is not shown in this workspace; its width is set through the session")
                    action(obj("type" to "set_tool_setting", "id" to "crop_width", "value" to cropped))
                }
                applyCrop(name)
                val square = size()
                assertTrue("$name: Apply crops to the square frame: $square for $cropped", square.first == square.second && kotlin.math.abs(square.first - cropped) <= 1)
                awaitPixels("$name: the fill stays in place after the crop", listOf(center)) { (c) -> shows(c, blueFill) }
                history("undo")
                waitFor("$name: one undo step restores the canvas", 5_000) { size() == (width.toInt() to height.toInt()) }

                openCrop(device)
                assertTrue("$name: the crop keeps its ratio", selected("crop_ratio_square"))
                pick("crop-ratio", "Free")
                waitFor("$name: Free", 5_000) { selected("crop_ratio_free") }
                pressBar("reset_transform")
                waitFor("$name: Reset returns the frame to the canvas", 5_000) { setting("crop_width") == width }
                pressBar("crop_straighten")
                waitFor("$name: Straighten arms line drawing", 5_000) { selected("crop_straighten") }
                if (shown("canvas-bar-action-crop_straighten")) waitFor("$name: Straighten shows as on", 5_000) {
                    tagged("canvas-bar-action-crop_straighten")?.second?.config?.getOrNull(SemanticsProperties.ToggleableState) == androidx.compose.ui.state.ToggleableState.On
                }
                val angle = .1
                val from = listOf(width * .3, height * .5)
                val to = listOf(from[0] + width * .4 * kotlin.math.cos(angle), from[1] + width * .4 * kotlin.math.sin(angle))
                drag(documentPoint(from[0], from[1]), documentPoint(to[0], to[1]))
                waitFor("$name: the line levels the crop", 5_000) { !selected("crop_straighten") }
                assertEquals("$name: the frame turns to the line", angle, setting("crop_angle"), .01)
                val (w, h) = setting("crop_width") to setting("crop_height")
                applyCrop(name)
                val straight = size()
                assertTrue("$name: Apply cuts the turned frame: $straight for $w × $h", kotlin.math.abs(straight.first - w) <= 1 && kotlin.math.abs(straight.second - h) <= 1)
                fun around(x: Double, y: Double) = documentPoint(straight.first / 2 + x, straight.second / 2 + y)
                val edge = -height * .3 + 20
                awaitPixels("$name: the image turns, lowering the fill's top-left corner and raising its top-right", listOf(around(0.0, 0.0), around(-width * .22, edge), around(width * .22, edge))) { (c, left, right) ->
                    shows(c, blueFill) && shows(left, paper) && shows(right, blueFill)
                }
                history("undo")
                waitFor("$name: one undo step restores the straightened drawing", 5_000) { size() == (width.toInt() to height.toInt()) }

                openCrop(device)
                pressBar("crop_delete_cropped_pixels")
                waitFor("$name: Delete Cropped Pixels turns on", 5_000) { selected("crop_delete_cropped_pixels") }
                drag(documentPoint(0.0, 0.0), documentPoint(width * .4, height * .4))
                waitFor("$name: the top-left handle moves", 5_000) { setting("crop_width") < width * .7 }
                applyCrop(name)
                val kept = size()
                val origin = listOf(width - kept.first, height - kept.second)
                command("canvas_size")
                for (value in listOf(obj("op" to "anchor", "anchor" to "bottom_right"), obj("op" to "width", "value" to width), obj("op" to "height", "value" to height), obj("op" to "apply")))
                    action(obj("type" to "canvas_size", "action" to value))
                waitFor("$name: Canvas Size grows the canvas back", 5_000) { size() == (width.toInt() to height.toInt()) }
                val deleted = documentPoint(origin[0] * .75, height * .5)
                awaitPixels("$name: Canvas Size shows no deleted pixels, and the kept fill stays", listOf(deleted, documentPoint(width * .6, height * .6))) { (d, k) ->
                    shows(d, paper) && shows(k, blueFill)
                }
                history("undo"); history("undo")
                waitFor("$name: two undo steps return to the uncropped drawing", 5_000) { size() == (width.toInt() to height.toInt()) }
                awaitPixels("$name: undo brings the deleted pixels back", listOf(documentPoint(origin[0] * .75, height * .5))) { (d) -> shows(d, blueFill) }
                println("PASS crop $name")
            }
            tool = MotionEvent.TOOL_TYPE_FINGER
            openCrop(tool)
            val before = translation()
            val inside = documentPoint(width * .5, height * .5)
            drag(inside, inside + Offset(90f, 50f))
            settle()
            assertEquals("A finger inside the frame leaves the frame", listOf(width, height), listOf(setting("crop_width"), setting("crop_height")))
            assertEquals("One finger inside the frame does not move the view", before, translation())
            twoFingers(inside, Offset(96f, 0f))
            waitFor("Two fingers inside the frame pan the view", 5_000) { translation().first - before.first > 20 }
            var panned = translation()
            do { SystemClock.sleep(200); val still = panned == translation(); panned = translation() } while (!still)
            assertEquals("Two fingers pan the view, not the frame", listOf(width, height), listOf(setting("crop_width"), setting("crop_height")))
            pressBar("crop_delete_cropped_pixels")
            waitFor("Delete Cropped Pixels turns off", 5_000) { !selected("crop_delete_cropped_pixels") }
            applyCrop("finger")
            assertEquals("The frame never moved, so Apply leaves the drawing and the view", listOf(width.toInt() to height.toInt(), panned), listOf(size(), translation()))
            openCrop(MotionEvent.TOOL_TYPE_MOUSE)
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Escape cancels the crop", 5_000) { canvasTool() != "crop" && size() == (width.toInt() to height.toInt()) }
        } finally {
            popupInput = false
            if (canvasTool() == "crop") command("cancel_transform")
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS crop: with mouse, finger and stylus, Ratio ▾ 1:1, a handle drag and Apply crop in one undo step with the image in place; Straighten by a drawn line turns the image; Delete Cropped Pixels leaves nothing for Canvas Size to reveal; a finger on a handle drags it, one finger inside the frame leaves it and two pan the view; C, O, the Overlay dropdown and Tool Options; light and dark captures")
    }

    @Test fun imageCommandsAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val blueFill = listOf(.1, .3, .8, 1.0)
        val paper = listOf(1.0, 1.0, 1.0, 1.0)
        val fill = listOf(.2, .2, .6, .8)
        fun panel() = state().getJSONObject("layer_tools").optJSONObject("image_size")
        fun notice() = state().optJSONObject("notice")?.getString("text")
        fun paperLayer() = layerStates().first { it.getLong("id") == 2L }
        fun showPaper(shown: Boolean) {
            layerAction(obj("op" to "visibility", "id" to paperLayer().getLong("id"), "value" to shown))
            waitFor("the paper is ${if (shown) "shown" else "hidden"}", 5_000) { paperLayer().getBoolean("visible") == shown }
        }
        fun near(label: String, value: Int, expected: Double, tolerance: Int) = assertTrue("$label: $value vs $expected", kotlin.math.abs(value - expected) <= tolerance)
        fun type(field: String, text: String) = typeNumber("setting-number-image-size-$field", text)
        fun image(path: String) = chooseFromApplicationMenu("edit", listOf("Image", path))
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            val original = width.toInt() to height.toInt()
            assertNotEquals("a non-square canvas", original.first, original.second)
            command("zoom_out"); SystemClock.sleep(300)
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            action(obj("type" to "set_color", "rgba" to JSONArray(blueFill)))
            command("rectangle_select")
            tool = MotionEvent.TOOL_TYPE_STYLUS
            drag(documentPoint(width * fill[0], height * fill[1]), documentPoint(width * fill[2], height * fill[3]))
            waitFor("the selection", 5_000) { hasSelection() }
            command("fill_selection"); command("deselect")
            awaitPixels("the fill paints left of the middle", listOf(documentPoint(width * .5, height * .5))) { (c) -> shows(c, blueFill) }
            val filled = (fill[2] - fill[0]) * width to (fill[3] - fill[1]) * height
            for ((index, device) in pointerTools.withIndex()) {
                val name = listOf("mouse", "finger", "stylus")[index]
                tool = device

                image("Image Size…")
                waitFor("$name: the Image Size panel", 5_000) { panel() != null && shown("image-size-panel") }
                onMain { assertTrue("$name: the panel leaves window focus with the canvas", owner.view.hasWindowFocus()) }
                assertFalse("$name: no field is edited when the panel opens", host.editingText)
                assertTrue("$name: Constrain proportions starts on", panel()!!.getBoolean("constrain"))
                assertFalse("$name: Apply is disabled at the current size", panel()!!.getBoolean("can_apply"))
                val half = original.first / 2 to original.second / 2
                fun values() = panel()!!.getJSONArray("values").let { it.getDouble(0) to it.getDouble(1) }
                when (device) {
                    MotionEvent.TOOL_TYPE_FINGER -> {
                        tap(settledBounds("image-size-constrain").center)
                        waitFor("$name: Constrain proportions turns off", 5_000) { !panel()!!.getBoolean("constrain") }
                        type("width", half.first.toString())
                        waitFor("$name: without Constrain the height stays", 5_000) { values() == half.first.toDouble() to height }
                        tap(settledBounds("image-size-constrain").center)
                        waitFor("$name: Constrain proportions makes the height follow", 5_000) {
                            panel()!!.getBoolean("constrain") && !host.editingText && values() == half.first.toDouble() to half.second.toDouble()
                        }
                    }
                    else -> {
                        tap(settledBounds("image-size-unit-percent").center)
                        waitFor("$name: Percent", 5_000) { panel()!!.getString("unit") == "percent" }
                        type(if (device == MotionEvent.TOOL_TYPE_STYLUS) "height" else "width", "50")
                        waitFor("$name: Constrain proportions makes the other side follow", 5_000) { values() == 50.0 to 50.0 }
                        if (device == MotionEvent.TOOL_TYPE_MOUSE) {
                            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ENTER)
                            waitFor("$name: Enter ends typing", 5_000) { !host.editingText }
                            onMain { assertTrue("$name: the canvas has window focus again", owner.view.hasWindowFocus()) }
                        }
                        if (device == MotionEvent.TOOL_TYPE_STYLUS) {
                            tap(settledBounds("image-size-resample").center)
                            waitFor("$name: the Resample menu opens", 5_000) { popupCount() == 1 }
                            onMain { assertTrue("$name: the Resample menu leaves window focus with the canvas", owner.view.hasWindowFocus()) }
                            assertFalse("$name: opening the menu ends typing", host.editingText)
                            chooseInMenu(listOf("Bicubic"))
                            waitFor("$name: Resample Bicubic", 5_000) { panel()!!.getString("resample") == "bicubic" && values() == 50.0 to 50.0 }
                        }
                    }
                }
                assertEquals("New size: ${half.first} × ${half.second} px", panel()!!.getString("message"))
                assertTrue(panel()!!.getBoolean("can_apply"))
                if (index == 0) {
                    for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        captureCanvasBar("image-size-$theme", "image-commands")
                    }
                    action(obj("type" to "set_theme", "theme" to originalTheme))
                }
                tap(settledBounds("image-size-apply").center)
                waitFor("$name: Apply scales the image and closes the panel", 10_000) { panel() == null && size() == half && !exists("image-size-panel") }
                assertNull(host.actionError)
                command("fit_canvas")
                awaitPixels("$name: the fill scales with the image", listOf(documentPoint(half.first / 2.0, half.second / 2.0), documentPoint(width * .4, half.second / 2.0))) { (f, p) ->
                    shows(f, blueFill) && shows(p, paper)
                }
                history("undo")
                waitFor("$name: one undo step restores the size", 10_000) { size() == original }
                println("PASS image size $name")

                image("Rotate Image 90° Right")
                waitFor("$name: the canvas turns", 10_000) { size() == original.second to original.first }
                command("fit_canvas")
                awaitPixels("$name: the fill left of the middle turns to above it", listOf(documentPoint(height * .5, width * .3), documentPoint(height * .5, width * .75))) { (f, p) ->
                    shows(f, blueFill) && shows(p, paper)
                }
                history("undo")
                waitFor("$name: one undo step turns it back", 10_000) { size() == original }
                println("PASS rotate image $name")

                command("fit_canvas"); command("zoom_out"); SystemClock.sleep(300)
                openCrop(device)
                if (!selected("crop_ratio_free")) command("crop_ratio_free")
                if (selected("crop_delete_cropped_pixels")) command("crop_delete_cropped_pixels")
                drag(documentPoint(0.0, 0.0), documentPoint(width * .4, height * .4))
                waitFor("$name: the top-left handle moves into the fill", 5_000) { setting("crop_width") < width * .7 }
                applyCrop(name)
                val cropped = size()
                assertTrue("$name: the crop hides the fill's left part: $cropped", cropped.first < width * .7 && cropped.second < height * .7)
                image("Reveal All")
                waitFor("$name: Reveal All grows the canvas", 10_000) { size().first > cropped.first }
                val revealed = size()
                near("$name: Reveal All reaches the fill's hidden left edge", revealed.first, width * (1 - fill[0]), 2)
                near("$name: Reveal All reaches the fill's hidden top edge", revealed.second, height * (1 - fill[1]), 2)
                command("fit_canvas")
                awaitPixels("$name: the hidden fill shows again at the new top left", listOf(documentPoint(6.0, 6.0))) { (f) -> shows(f, blueFill) }
                history("undo")
                waitFor("$name: one undo step returns to the crop", 10_000) { size() == cropped }
                history("undo")
                waitFor("$name: another returns to the whole drawing", 10_000) { size() == original }
                println("PASS reveal all $name")

                image("Trim")
                waitFor("$name: with the paper showing, Trim explains that nothing changes", 10_000) { notice() == "The visible pixels already reach every edge of the canvas" }
                assertEquals(original, size())
                showPaper(false)
                image("Trim")
                waitFor("$name: Trim shrinks the canvas", 10_000) { size().first < original.first }
                val trimmed = size()
                near("$name: Trim fits the width to the fill", trimmed.first, filled.first, 2)
                near("$name: Trim fits the height to the fill", trimmed.second, filled.second, 2)
                command("fit_canvas")
                awaitPixels("$name: the fill reaches the trimmed edges", listOf(documentPoint(3.0, 3.0), documentPoint(trimmed.first - 4.0, trimmed.second - 4.0))) { (a, b) ->
                    shows(a, blueFill) && shows(b, blueFill)
                }
                history("undo")
                waitFor("$name: one undo step", 10_000) { size() == original }
                println("PASS trim $name")

                command("fit_canvas"); command("zoom_out"); SystemClock.sleep(300)
                openCrop(device)
                assertTrue("$name: Fit Content on the crop bar", barCaption("crop_fit_content", "Fit Content") || !shown("canvas-bar-action-crop_fit_content"))
                pressBar("crop_fit_content")
                waitFor("$name: Fit Content frames the fill", 10_000) { setting("crop_width") < width * .7 }
                near("$name: the frame's width", setting("crop_width").toInt(), filled.first, 3)
                near("$name: the frame's height", setting("crop_height").toInt(), filled.second, 3)
                if (index == 0) {
                    for (theme in listOf("light", "dark")) {
                        action(obj("type" to "set_theme", "theme" to theme))
                        captureCanvasBar("crop-fit-content-$theme", "image-commands")
                    }
                    action(obj("type" to "set_theme", "theme" to originalTheme))
                }
                applyCrop(name)
                near("$name: Apply crops to the fill's width", size().first, filled.first, 3)
                near("$name: Apply crops to the fill's height", size().second, filled.second, 3)
                history("undo")
                waitFor("$name: one undo step", 10_000) { size() == original }
                showPaper(true)
                println("PASS fit content $name")
            }
        } finally {
            popupInput = false
            if (panel() != null) action(obj("type" to "image_size", "action" to obj("op" to "cancel")))
            if (canvasTool() == "crop") command("cancel_transform")
            if (!paperLayer().getBoolean("visible")) showPaper(true)
            action(obj("type" to "set_theme", "theme" to originalTheme))
        }
        println("PASS image commands: with mouse, finger and stylus, Image Size to 50% with Constrain proportions, Rotate Image 90° Right on a non-square canvas, a crop then Reveal All, Trim and Fit Content on the crop bar each change the canvas in one undo step; the panel and its menu leave window focus with the canvas; light and dark captures")
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

    /** The source disc, its bar, Set Source and the strokes of the retouching journeys, which paint `target`. */
    private inner class Retouching(val target: Long) {
        fun point(at: List<Double>) = documentPoint(at[0], at[1])
        fun discBar() = barKind() == "clone_source" && shown("canvas-action-bar")
        fun disc() = canvasBar()?.takeIf { barKind() == "clone_source" }?.optJSONArray("anchor")?.let {
            listOf((it.getDouble(0) + it.getDouble(2)) / 2, (it.getDouble(1) + it.getDouble(3)) / 2)
        }
        fun near(a: List<Double>?, b: List<Double>, within: Double) = a != null && kotlin.math.abs(a[0] - b[0]) <= within && kotlin.math.abs(a[1] - b[1]) <= within
        fun armed() = selected("clone_source_arm")
        fun painted() = paintRevision(target)
        fun alt(down: Boolean) {
            val now = SystemClock.uptimeMillis()
            instrumentation.sendKeySync(KeyEvent(now, now, if (down) KeyEvent.ACTION_DOWN else KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ALT_LEFT, 0,
                if (down) KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON else 0, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
        }
        /** Alt held over a tap with the current device, which never paints. */
        fun altTap(at: List<Double>) {
            alt(true)
            waitFor("Alt arms Set Source", 5_000) { armed() }
            val before = painted()
            tap(point(at))
            alt(false)
            waitFor("releasing Alt disarms Set Source", 5_000) { !armed() }
            assertEquals("Alt and a tap paint nothing", before, painted())
        }
        fun hover(action: Int, at: Offset, buttons: Int) {
            val base = IntArray(2); val origin = IntArray(2)
            onMain { owner.view.getLocationOnScreen(base); surface.getLocationOnScreen(origin) }
            val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS })
            val coords = arrayOf(MotionEvent.PointerCoords().apply { x = at.x + base[0] - origin[0]; y = at.y + base[1] - origin[1] })
            val now = SystemClock.uptimeMillis()
            val motion = MotionEvent.obtain(now, now, action, 1, properties, coords, 0, buttons, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
            try { onMain { surface.dispatchGenericMotionEvent(motion) } } finally { motion.recycle() }
            SystemClock.sleep(120)
        }
        /** The primary side button held over a stylus tap, which sets the source without painting. */
        fun sideButtonTap(at: List<Double>) {
            hover(MotionEvent.ACTION_HOVER_ENTER, point(at), 0)
            hover(MotionEvent.ACTION_HOVER_MOVE, point(at), MotionEvent.BUTTON_STYLUS_PRIMARY)
            waitFor("the side button arms Set Source", 5_000) { armed() }
            val before = painted()
            stylusButtons = MotionEvent.BUTTON_STYLUS_PRIMARY
            tap(point(at))
            stylusButtons = 0
            hover(MotionEvent.ACTION_HOVER_MOVE, point(at), MotionEvent.BUTTON_STYLUS_PRIMARY)
            hover(MotionEvent.ACTION_HOVER_MOVE, point(at), 0)
            hover(MotionEvent.ACTION_HOVER_EXIT, point(at), 0)
            waitFor("releasing the side button disarms Set Source", 5_000) { !armed() }
            assertEquals("the side button and a tap paint nothing", before, painted())
        }
        fun clickSetting(tag: String) {
            waitFor("$tag in the settings", 5_000) { findTag(tag) != null }
            onMain { findTag(tag)!!.second.config[androidx.compose.ui.semantics.SemanticsActions.OnClick].action!!.invoke() }
            settle()
        }
        fun bindSideButton() {
            action(obj("type" to "open_settings", "page" to "input"))
            clickSetting("trigger-pen.button.primary")
            clickSetting("pen-button-same")
            clickSetting("pen-button-action-retouching")
            clickSetting("action-command.CloneSourceArm")
            waitFor("the side button sets the source with retouching tools", 5_000) {
                state().getJSONObject("settings").optJSONObject("pen_buttons")?.optJSONObject("pen.button.primary")?.optString("retouching") == "command.CloneSourceArm"
            }
            action(obj("type" to "close_settings"))
        }
        fun showBar(at: List<Double>, label: String): List<Double> {
            tap(point(at))
            waitFor("$label: a tap on the disc shows its bar", 5_000) { discBar() }
            return disc()!!
        }
        fun hideBar(at: List<Double>) {
            tap(point(at))
            waitFor("a second tap on the disc hides its bar", 5_000) { barKind() != "clone_source" }
        }
        fun awaitDisc(at: List<Double>, within: Double, label: String) = waitFor("$label: the disc reaches $at", 5_000) { discBar() && near(disc(), at, within) }
        fun press(id: String) {
            waitFor("the source bar offers $id", 5_000) { discBar() }
            val tag = "canvas-bar-action-$id"
            if (shown(tag)) tap(bounds(tag).center) else viaMore(listOf(commandState(id).getString("label")))
        }
        fun pick(label: String) {
            val tag = "canvas-bar-choice-selection-source"
            waitFor("the source bar offers Source", 5_000) { discBar() }
            if (!shown(tag)) return viaMore(listOf("Source", label))
            tap(bounds(tag).center)
            waitFor("Source ▾ opens", 5_000) { popupCount() == 1 }
            chooseInMenu(listOf(label))
        }
        fun stroke(from: List<Double>, to: List<Double>, hold: () -> Unit = {}) {
            val before = painted()
            drag(point(from), point(to), 12, hold)
            waitFor("the stroke paints", 10_000) { painted() != before }
        }
        fun dragDisc(from: List<Double>, to: List<Double>, name: String) {
            val before = painted()
            val view = state().getJSONObject("camera").getJSONArray("translation").toString()
            drag(point(from), point(to), 10)
            settle()
            assertEquals("$name: dragging the disc paints nothing", before, painted())
            assertEquals("$name: dragging the disc never pans the canvas", view, state().getJSONObject("camera").getJSONArray("translation").toString())
        }
        /** The mean color around `at` on screen, `radius` screen pixels each way. */
        fun mean(at: List<Double>, radius: Int) = screenMeans(listOf(point(at)), radius)[0]
        fun awaitMean(label: String, at: List<Double>, radius: Int, check: (List<Double>) -> Boolean) =
            awaitScreen(label, { mean(at, radius) }, check) { last -> last.joinToString { "%.3f".format(it) } }
    }
    private fun like(expected: List<Double>, within: Double) = { rgb: List<Double> -> rgb.indices.all { kotlin.math.abs(rgb[it] - expected[it]) <= within } }
    /** Reset the primary side button and apply the preference actions `also` in the open settings. */
    private fun restorePreferences(vararg also: JSONObject) {
        if (!state().getBoolean("settings_open")) host.drain(obj("type" to "open_settings", "page" to "input"), 10)
        for (value in listOf(obj("type" to "preferences", "action" to obj("type" to "reset_trigger", "trigger" to "pen.button.primary"))) + also +
            obj("type" to "close_settings"))
            host.drain(value, 10)
    }

    /** Clone Stamp on an empty layer over a reference, with Alt, a side button bound to Set Source, and the source disc. */
    @Test fun cloneStampAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
        val paper = listOf(1.0, 1.0, 1.0, 1.0)
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            fun at(x: Double, y: Double) = listOf(width * x, height * y)
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            val reference = editingLayer()
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.1, .3, .8, 1.0))))
            command("rectangle_select")
            tool = MotionEvent.TOOL_TYPE_STYLUS
            drag(documentPoint(width * .1, height * .2), documentPoint(width * .35, height * .8))
            waitFor("the selection", 5_000) { hasSelection() }
            command("fill_selection"); command("deselect")
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            val target = editingLayer()
            command("use_reference_below")
            waitFor("the filled layer is a reference", 5_000) { layerStates().first { it.getLong("id") == reference }.getBoolean("reference") }
            command("clone")
            action(obj("type" to "set_brush_size", "value" to 48))
            waitFor("Clone Stamp is ready", 10_000) { state().getJSONObject("brush").getString("tool") == "clone" && host.snapshot?.optBoolean("brush_ready") == true }
            assertEquals("Clone Stamp copies the reference layers, aligned", listOf(true, true, false), listOf(selected("selection_reference"), selected("clone_aligned"), selected("clone_flip_horizontal")))
            with(Retouching(target)) {
                fun copied(label: String, at: List<Double>) = awaitPixels(label, listOf(point(at))) { (p) -> blue(p) }
                fun clean(label: String, at: List<Double>) = awaitPixels(label, listOf(point(at))) { (p) -> shows(p, paper) }

                tool = MotionEvent.TOOL_TYPE_MOUSE
                val s1 = at(.15, .5)
                altTap(s1)
                assertTrue("Alt-click sets the source", near(showBar(s1, "mouse"), s1, 1.5))
                assertFalse("Reset Offset waits for an offset", commandState("clone_reset_offset").getBoolean("enabled"))
                for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    copied("$theme keeps the reference visible", at(.25, .4))
                    captureCanvasBar("clone-bar-$theme", "clone") { image, origin ->
                        val p = point(at(.25, .4)) + origin
                        assertTrue("$theme keeps the reference in the captured canvas", blue(image.getPixel(p.x.toInt(), p.y.toInt())))
                    }
                }
                action(obj("type" to "set_theme", "theme" to originalTheme))
                pick("Editing layer")
                waitFor("Source ▾ chooses the editing layer", 5_000) { selected("selection_editing") && !selected("selection_reference") }
                pick("Reference layers")
                waitFor("Source ▾ chooses the reference layers again", 5_000) { selected("selection_reference") }
                val moved = at(.17, .42); val followed = at(.27, .42)
                dragDisc(s1, moved, "mouse")
                awaitDisc(moved, 2.0, "the mouse drags the disc")
                hideBar(moved)
                stroke(at(.6, .42), at(.7, .42))
                copied("the mouse stroke copies the reference", at(.62, .42))
                assertTrue("an aligned source follows the mouse stroke", near(showBar(followed, "mouse after a stroke"), followed, 3.0))
                assertTrue("an aligned stroke keeps an offset", commandState("clone_reset_offset").getBoolean("enabled"))
                hideBar(followed)
                history("undo")
                clean("one undo removes the mouse stroke", at(.62, .42))
                showBar(followed, "mouse after undo")
                press("clone_reset_offset")
                waitFor("Reset Offset", 5_000) { !commandState("clone_reset_offset").getBoolean("enabled") }
                press("clone_aligned")
                waitFor("Aligned turns off", 5_000) { !selected("clone_aligned") }
                hideBar(followed)
                stroke(at(.6, .62), at(.68, .62))
                copied("a stroke that is not aligned starts copying at the disc", at(.62, .62))
                assertTrue("a source that is not aligned stays at the disc", near(showBar(followed, "mouse after an unaligned stroke"), followed, 1.5))
                hideBar(followed)
                history("undo")
                clean("one undo removes it", at(.62, .62))
                showBar(followed, "mouse before Aligned")
                press("clone_aligned")
                waitFor("Aligned turns on", 5_000) { selected("clone_aligned") }
                for (on in listOf(true, false)) {
                    press("clone_flip_horizontal")
                    waitFor("Flip H is $on", 5_000) { selected("clone_flip_horizontal") == on }
                }
                hideBar(followed)
                println("PASS clone mouse")

                tool = MotionEvent.TOOL_TYPE_FINGER
                val touched = at(.2, .5)
                dragDisc(followed, touched, "finger")
                assertNotEquals("a drag is not a tap", "clone_source", barKind())
                assertTrue("a finger drags the disc", near(showBar(touched, "finger"), touched, 2.0))
                for (on in listOf(true, false)) {
                    press("clone_flip_vertical")
                    waitFor("Flip V is $on", 5_000) { selected("clone_flip_vertical") == on }
                }
                pick("Editing layer")
                waitFor("a finger chooses the editing layer", 5_000) { selected("selection_editing") }
                pick("Reference layers")
                waitFor("a finger chooses the reference layers", 5_000) { selected("selection_reference") }
                val untouched = painted()
                altTap(at(.6, .3))
                awaitDisc(touched, 1.0, "Alt with a finger never sets the source")
                drag(point(at(.6, .3)), point(at(.7, .35)))
                settle()
                assertEquals("a finger never paints with Clone Stamp", untouched, painted())
                assertTrue("a finger elsewhere never moves the source", near(disc(), touched, 1.0))
                command("fit_canvas"); SystemClock.sleep(300)
                hideBar(touched)
                println("PASS clone finger")

                tool = MotionEvent.TOOL_TYPE_STYLUS
                bindSideButton()
                waitFor("Clone Stamp is ready again", 10_000) { host.snapshot?.optBoolean("brush_ready") == true }
                val s3 = at(.18, .35)
                sideButtonTap(s3)
                assertTrue("the side button and a stylus tap set the source", near(showBar(s3, "stylus"), s3, 1.5))
                hideBar(s3)
                val s4 = at(.15, .45)
                altTap(s4)
                assertTrue("Alt and a stylus tap set the source", near(showBar(s4, "stylus after Alt"), s4, 1.5))
                val penMoved = at(.17, .4); val last = at(.27, .55)
                dragDisc(s4, penMoved, "stylus")
                awaitDisc(penMoved, 2.0, "the stylus drags the disc")
                hideBar(penMoved)
                for (y in listOf(.45, .6)) stroke(at(.6, y), at(.7, y))
                awaitPixels("both stylus strokes copy the reference", listOf(point(at(.62, .45)), point(at(.62, .6)))) { pixels -> pixels.all { blue(it) } }
                assertTrue("an aligned source follows both stylus strokes", near(showBar(last, "stylus after two strokes"), last, 3.0))
                hideBar(last)
                history("undo")
                awaitPixels("undo removes only the last stylus stroke", listOf(point(at(.62, .45)), point(at(.62, .6)))) { (first, second) -> blue(first) && shows(second, paper) }
                history("undo")
                clean("each stylus stroke is one undo step", at(.62, .45))
                showBar(last, "stylus after undo")
                press("clone_reset_offset")
                waitFor("Reset Offset", 5_000) { !commandState("clone_reset_offset").getBoolean("enabled") }
                hideBar(last)
            }
            assertNull(host.actionError)
        } finally {
            popupInput = false
            stylusButtons = 0
            restorePreferences()
            for (value in listOf(obj("type" to "set_theme", "theme" to originalTheme), obj("type" to "invoke", "command" to "brush"))) host.drain(value, 10)
        }
        println("PASS clone: Alt and a side button bound to Set Source in the pen-button settings set the source with the mouse and stylus, never a finger; the disc drags at once with mouse, finger and stylus without painting or panning; a tap shows its bar, whose Source ▾, Aligned, Flip and Reset Offset work; aligned and unaligned strokes copy the reference in one undo step each; light and dark captures")
    }

    /** A photo the size of the canvas: a light textured area, and a darker one with a red scratch across it and a yellow dot. */
    private fun texturedPhoto(width: Int, height: Int): File {
        val light = listOf(199, 184, 158); val dark = listOf(107, 92, 77)
        val noise = java.util.Random(7)
        val pixels = IntArray(width * height) { i ->
            val (x, y) = i % width to i / width
            val n = (noise.nextInt(9) - 4) * 2.5
            when {
                x >= width * .62 && x < width * .68 && kotlin.math.abs(y - height * .45) < 6 -> android.graphics.Color.rgb(230, 64, 51)
                kotlin.math.hypot(x - width * .8, y - height * .72) < 11 -> android.graphics.Color.rgb(242, 217, 77)
                else -> (if (x >= width * .05 && x < width * .4 && y >= height * .15 && y < height * .85) light else dark)
                    .map { (it + n).toInt() }.let { (r, g, b) -> android.graphics.Color.rgb(r, g, b) }
            }
        }
        val file = File(instrumentation.targetContext.cacheDir, "Texture.png")
        val bitmap = android.graphics.Bitmap.createBitmap(pixels, width, height, android.graphics.Bitmap.Config.ARGB_8888)
        try { file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() }
        return file
    }

    /** Healing and Spot Healing in the Photo workspace over a textured photo marked as a reference, with mouse, finger and stylus. */
    @Test fun healingAcrossDevices() {
        val keep = layerStates().map { it.getLong("id") }.toSet()
        val settings = state().getJSONObject("settings")
        val originalTheme = settings.opt("theme") ?: JSONObject.NULL
        val keymap = settings.optJSONObject("keymap")?.optString("id") ?: "capy"
        fun selectKeymap(id: String) = obj("type" to "preferences", "action" to obj("type" to "select_keymap", "id" to id))
        popupInput = true
        try {
            val (width, height) = cleanDocument(keep)
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:photographer")) }
            waitFor("the Photo workspace", 30_000) { host.workspaceManager?.optString("id") == "builtin:workspace:photographer" && host.workspaceManager?.optBoolean("busy") == false }
            command("fit_canvas"); SystemClock.sleep(300)
            fun at(x: Double, y: Double) = listOf(width * x, height * y)
            host.importImage(texturedPhoto(width.toInt(), height.toInt()))
            waitFor("the placement bar", 10_000) { barKind() == "placement" }
            assertEquals("the photo covers the canvas", listOf(0.0, 0.0, width, height), canvasBar()!!.array("anchor").let { a -> (0 until 4).map { a.getDouble(it) } })
            command("apply_transform")
            waitFor("the photo is placed", 10_000) { canvasBar() == null }
            val photo = editingLayer()
            layerAction(obj("op" to "new", "group" to false, "clipped" to false))
            val target = editingLayer()
            command("use_reference_below")
            waitFor("the photo is a reference", 5_000) { layerStates().first { it.getLong("id") == photo }.getBoolean("reference") }
            fun brush() = state().getJSONObject("brush").getString("tool")
            fun ready(tool: String) = waitFor("$tool is ready", 10_000) { brush() == tool && host.snapshot?.optBoolean("brush_ready") == true }
            fun choose(tool: String) {
                val tile = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first { it.getString("id") == "toolbar" }
                    .getJSONObject("content").array("tiles").objects().first { it.optJSONObject("control")?.optString("command") == tool }.getInt("id")
                tap(bounds("tile-toolbar-$tile").center)
                ready(tool)
            }
            fun key(code: Int, meta: Int, tool: String, label: String) { pressKey(code, meta); waitFor(label, 5_000) { brush() == tool } }

            tool = MotionEvent.TOOL_TYPE_MOUSE
            choose("heal")
            command("brush")
            for (expected in listOf("clone", "heal", "spot_heal")) key(KeyEvent.KEYCODE_S, 0, expected, "S cycles to $expected")
            for (value in listOf(obj("type" to "open_settings", "page" to "shortcuts"), selectKeymap("photoshop"), obj("type" to "close_settings"))) action(value)
            key(KeyEvent.KEYCODE_J, 0, "spot_heal", "J chooses Spot Healing in the Photoshop keymap")
            key(KeyEvent.KEYCODE_J, KeyEvent.META_SHIFT_ON or KeyEvent.META_SHIFT_LEFT_ON, "heal", "Shift+J chooses Healing in the Photoshop keymap")
            for (value in listOf(obj("type" to "open_settings", "page" to "shortcuts"), selectKeymap(keymap), obj("type" to "close_settings"))) action(value)
            ready("heal")
            action(obj("type" to "set_brush_size", "value" to 120))
            assertEquals("Healing copies the reference layers below, aligned", listOf(true, true), listOf(selected("selection_reference"), selected("clone_aligned")))
            with(Retouching(target)) {
                val scratch = at(.65, .45)
                val light = mean(at(.25, .3), 6); val dark = mean(at(.65, .33), 6)
                assertTrue("the photo shows its light and darker areas: $light $dark", light[0] - dark[0] > .25)
                val scratched = { rgb: List<Double> -> rgb[0] - rgb[1] > .3 }
                fun heal(source: List<Double>): List<Double> {
                    awaitMean("the scratch before healing", scratch, 1, scratched)
                    ready("heal")
                    stroke(at(.59, .45), at(.71, .45)) {
                        SystemClock.sleep(300)
                        val live = mean(scratch, 3)
                        assertTrue("while the pen is down the stroke is the clone of the light area: $live against $light", like(light, .12)(live))
                    }
                    awaitMean("the healed stroke takes the tone of the darker area around it ($dark)", scratch, 3, like(dark, .05))
                    return listOf(source[0] + width * .12, source[1])
                }
                fun undoHeal() {
                    history("undo")
                    awaitMean("one undo brings the scratch back", scratch, 1, scratched)
                }

                val s1 = at(.15, .45)
                altTap(s1)
                assertTrue("Alt-click sets the healing source", near(showBar(s1, "mouse"), s1, 1.5))
                hideBar(s1)
                val followed = heal(s1)
                undoHeal()
                assertTrue("an aligned source follows the healing stroke", near(showBar(followed, "mouse after a stroke"), followed, 3.0))
                val moved = at(.17, .5)
                dragDisc(followed, moved, "mouse")
                awaitDisc(moved, 2.0, "the mouse drags the disc")
                hideBar(moved)
                println("PASS heal mouse")

                tool = MotionEvent.TOOL_TYPE_FINGER
                val touched = at(.2, .55)
                dragDisc(moved, touched, "finger")
                assertTrue("a finger drags the disc", near(showBar(touched, "finger"), touched, 2.0))
                val untouched = painted()
                altTap(at(.6, .25))
                awaitDisc(touched, 1.0, "Alt with a finger never sets the source")
                drag(point(at(.6, .25)), point(at(.7, .3)))
                settle()
                assertEquals("a finger never paints with Healing", untouched, painted())
                assertTrue("a finger elsewhere never moves the source", near(disc(), touched, 1.0))
                command("fit_canvas"); SystemClock.sleep(300)
                hideBar(touched)
                println("PASS heal finger")

                tool = MotionEvent.TOOL_TYPE_STYLUS
                bindSideButton()
                ready("heal")
                val s3 = at(.18, .35)
                sideButtonTap(s3)
                assertTrue("the side button and a stylus tap set the healing source", near(showBar(s3, "stylus"), s3, 1.5))
                hideBar(s3)
                val s4 = at(.14, .4)
                altTap(s4)
                assertTrue("Alt and a stylus tap set the healing source", near(showBar(s4, "stylus after Alt"), s4, 1.5))
                val penMoved = at(.15, .45)
                dragDisc(s4, penMoved, "stylus")
                awaitDisc(penMoved, 2.0, "the stylus drags the disc")
                hideBar(penMoved)
                val penFollowed = heal(penMoved)
                assertTrue("an aligned source follows the stylus stroke", near(showBar(penFollowed, "stylus after a stroke"), penFollowed, 3.0))
                for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    captureCanvasBar("heal-$theme", "heal")
                }
                action(obj("type" to "set_theme", "theme" to originalTheme))
                hideBar(penFollowed)
                undoHeal()
                println("PASS heal stylus")

                tool = MotionEvent.TOOL_TYPE_FINGER
                choose("spot_heal")
                for (id in listOf("clone_source_arm", "clone_aligned", "clone_flip_horizontal", "clone_flip_vertical", "clone_reset_offset"))
                    assertEquals("$id with Spot Healing", false to "Spot Healing finds its own source",
                        commandState(id).let { it.getBoolean("enabled") to it.optString("disabled_reason") })
                val quiet = painted()
                alt(true)
                SystemClock.sleep(300)
                assertFalse("Alt does nothing with Spot Healing", armed())
                assertTrue("and raises no notice", state().isNull("notice"))
                assertNull(host.actionError)
                alt(false)
                assertEquals(quiet, painted())
                action(obj("type" to "set_brush_size", "value" to 100))
                val dot = at(.8, .72)
                val beside = mean(at(.8, .62), 3); val yellow = mean(dot, 1)
                assertTrue("the dot is yellow: $yellow", yellow[2] < yellow[0] - .3)
                for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                    tool = device
                    ready("spot_heal")
                    stroke(listOf(dot[0] - 12, dot[1]), listOf(dot[0] + 12, dot[1]))
                    awaitMean("one stroke heals the dot into the texture around it ($beside)", dot, 3, like(beside, .05))
                    if (device == MotionEvent.TOOL_TYPE_STYLUS) {
                        for (theme in listOf("light", "dark")) {
                            action(obj("type" to "set_theme", "theme" to theme))
                            captureCanvasBar("spot-heal-$theme", "heal")
                        }
                        action(obj("type" to "set_theme", "theme" to originalTheme))
                    }
                    history("undo")
                    awaitMean("one undo brings the dot back", dot, 1, like(yellow, .08))
                }
                println("PASS spot heal")
            }
            assertNull(host.actionError)
        } finally {
            popupInput = false
            stylusButtons = 0
            restorePreferences(selectKeymap(keymap))
            for (value in listOf(obj("type" to "set_theme", "theme" to originalTheme), obj("type" to "invoke", "command" to "brush"))) host.drain(value, 10)
        }
        println("PASS heal: the Photo toolbar, S and the Photoshop keymap's J and Shift+J choose Healing and Spot Healing; Alt and a side button bound to Set Source set the healing source with the mouse and stylus, never a finger; the disc drags with mouse, finger and stylus; a healing stroke previews as the clone and heals into the darker area around it, and Spot Healing removes a dot, with mouse and stylus, one undo step each; Spot Healing's source commands are disabled and Alt does nothing; light and dark captures")
    }
}
