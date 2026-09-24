package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
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
import kotlin.math.*

/** Production Compose, shared Rust and typed tablet input, with isolated workspace storage. */
class AndroidColorPanelTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private lateinit var host: CanvasHost
    private lateinit var owner: ViewRootForTest
    private lateinit var savedSettings: JSONObject
    private lateinit var fixture: JSONObject
    private var referenceHandle = 0L
    private var density = 1f
    private var downAt = 0L
    private var contact = false
    private var point = Offset.Zero
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var button = MotionEvent.BUTTON_PRIMARY
    private var waitForInput = true
    private var replayOrigin: IntArray? = null
    private val systemInput = InstrumentationRegistry.getArguments().getString("systemInput") == "true"
    private val tools = listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_MOUSE)
    private val output get() = File(activity.getExternalFilesDir(null), "validation/color-panel").apply { mkdirs() }

    private fun findOwner(view: View): ViewRootForTest? {
        if (view is ViewRootForTest) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) findOwner(view.getChildAt(i))?.let { return it }
        return null
    }
    private fun find(node: SemanticsNode, tag: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node else node.children.firstNotNullOfOrNull { find(it, tag) }
    private fun node(tag: String): SemanticsNode {
        var result: SemanticsNode? = null
        instrumentation.runOnMainSync { result = find(owner.semanticsOwner.unmergedRootSemanticsNode, tag) }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun bounds(tag: String) = node(tag).boundsInRoot
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun colors() = state().getJSONObject("colors")
    private fun view() = host.panelContent!!.getJSONObject("color_panel")
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
    private fun settle() { SystemClock.sleep(100); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun action(action: JSONObject) {
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync { host.dispatch(action); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS)); settle()
    }
    private fun color(action: JSONObject) = action(obj("type" to "color", "action" to action))
    private fun shape(shape: String) = color(obj("op" to "shape", "shape" to shape))
    private fun resize(width: Int, height: Int = width + 36) {
        val workspace = JSONObject(fixture.toString())
        val floating = workspace.getJSONObject("layout").getJSONArray("floating").getJSONObject(0)
        floating.put("width", width); floating.put("height", height)
        action(obj("type" to "restore_workspace", "workspace" to workspace))
        waitFor("color panel") { find(owner.semanticsOwner.unmergedRootSemanticsNode, "color-panel") != null }; settle()
    }
    @Before fun ready() {
        val root = File(instrumentation.targetContext.cacheDir, "color-panel-tests/${java.util.UUID.randomUUID()}")
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        RecoveryController.directoryForTest = File(root, "recovery")
        ColorPreferencesStore.directoryForTest = File(root, "color-preferences")
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {
            activity = it; host = it.host; owner = findOwner(it.window.decorView)!!; density = it.resources.displayMetrics.density
        }
        waitFor("brush ready", 60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        waitFor("workspace ready", 60_000) { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
        savedSettings = JSONObject(state().getJSONObject("settings").toString())
        action(obj("type" to "close_settings"))
        val defaults = Native.create(false)
        try { action(obj("type" to "restore_workspace", "workspace" to JSONObject(Native.snapshot(defaults)!!).getJSONObject("state").getJSONObject("workspace"))) }
        finally { Native.destroy(defaults) }
        if (state().getJSONObject("workspace").optBoolean("zen_mode")) action(obj("type" to "invoke", "command" to "zen_mode"))
        action(obj("type" to "move_panel", "panel" to "color", "target" to obj("kind" to "float", "position" to JSONArray(listOf(440, 120))),
            "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
        fixture = JSONObject(state().getJSONObject("workspace").toString())
        resize(280)
        referenceHandle = Native.create(false)
    }
    @After fun cleanup() {
        try {
            if (contact) event(MotionEvent.ACTION_CANCEL)
            if (::savedSettings.isInitialized) {
                action(obj("type" to "restore_settings", "settings" to savedSettings))
                action(obj("type" to "set_theme", "theme" to savedSettings.opt("theme")))
            }
        } finally {
            if (referenceHandle != 0L) Native.destroy(referenceHandle)
            if (::scenario.isInitialized) scenario.close()
            CanvasHost.workspaceDirectoryForTest = null
            RecoveryController.directoryForTest = null
            ColorPreferencesStore.directoryForTest = null
        }
    }
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); contact = true }
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 7; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = next.x; y = next.y; pressure = if (action in listOf(MotionEvent.ACTION_UP,MotionEvent.ACTION_HOVER_ENTER,MotionEvent.ACTION_HOVER_MOVE,MotionEvent.ACTION_HOVER_EXIT)) 0f else .7f })
        val source = when (tool) { MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS; MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE; else -> InputDevice.SOURCE_TOUCHSCREEN }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) button else 0
        val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, buttons, 1f, 1f, 0, 0, source, 0)
        try {
            if (systemInput) {
                val origin = replayOrigin ?: IntArray(2).also { instrumentation.runOnMainSync { owner.view.getLocationOnScreen(it) } }
                motion.offsetLocation(origin[0].toFloat(), origin[1].toFloat())
                assertTrue("Android accepts typed pointer input", instrumentation.uiAutomation.injectInputEvent(motion, waitForInput))
            } else instrumentation.runOnMainSync { owner.view.dispatchTouchEvent(motion) }
        } finally { motion.recycle() }
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) contact = false
    }
    private fun tap(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(30); event(MotionEvent.ACTION_UP); settle() }
    private fun capture(name: String): Bitmap {
        settle()
        val shot = instrumentation.uiAutomation.takeScreenshot()
        val bounds = bounds("color-panel")
        val origin = IntArray(2)
        instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
        val crop = Bitmap.createBitmap(shot, (bounds.left + origin[0]).roundToInt(), (bounds.top + origin[1]).roundToInt(), bounds.width.roundToInt(), bounds.height.roundToInt())
        File(output, "$name.png").outputStream().use { crop.compress(Bitmap.CompressFormat.PNG, 100, it) }
        shot.recycle()
        return crop
    }
    private fun picker() = host.colorPreview?.objectOrNull("picker") ?: state().getJSONObject("color_picker")
    private fun canvasPoint(): Offset {
        val area=state().getJSONObject("camera").array("work_area")
        return Offset((area.getDouble(0)+area.getDouble(2)*.72).toFloat(),(area.getDouble(1)+area.getDouble(3)*.70).toFloat())
    }
    private fun secondFinger(down:Boolean) {
        val properties=(7..8).map{id->MotionEvent.PointerProperties().apply{this.id=id;toolType=MotionEvent.TOOL_TYPE_FINGER}}.toTypedArray()
        val coords=(0..1).map{i->MotionEvent.PointerCoords().apply{x=point.x+i*100f;y=point.y;pressure=.7f}}.toTypedArray()
        val event=MotionEvent.obtain(downAt,SystemClock.uptimeMillis(),(if(down)MotionEvent.ACTION_POINTER_DOWN else MotionEvent.ACTION_POINTER_UP) or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),2,properties,coords,0,0,1f,1f,0,0,InputDevice.SOURCE_TOUCHSCREEN,0)
        try {
            val origin=IntArray(2);instrumentation.runOnMainSync{owner.view.getLocationOnScreen(origin)}
            event.offsetLocation(origin[0].toFloat(),origin[1].toFloat())
            assertTrue(instrumentation.uiAutomation.injectInputEvent(event,true))
        } finally {event.recycle()}
    }
    private fun fullCapture(name:String) {
        val image=instrumentation.uiAutomation.takeScreenshot()
        File(output,"$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG,100,it) };image.recycle()
    }
    @Test fun glassPickerInputAndSettings() {
        action(obj("type" to "invoke","command" to "fit_canvas"))
        val p=canvasPoint()
        tool=MotionEvent.TOOL_TYPE_STYLUS
        action(obj("type" to "select_brush","id" to 1))
        action(obj("type" to "set_brush_size","value" to 80))
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(1,0,0,1))))
        event(MotionEvent.ACTION_DOWN,p-Offset(12f,0f));event(MotionEvent.ACTION_MOVE,p);event(MotionEvent.ACTION_MOVE,p+Offset(12f,0f));event(MotionEvent.ACTION_UP);settle()
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(0,1,0,1))))
        val original=colors().toString()
        action(obj("type" to "invoke","command" to "eyedropper"))
        event(MotionEvent.ACTION_HOVER_MOVE,p)
        event(MotionEvent.ACTION_HOVER_MOVE,p)
        waitFor("hover sample") {picker().objectOrNull("preview")!=null}
        assertEquals(original,colors().toString())
        assertTrue("Red sample: ${picker()}",picker().getJSONObject("preview").array("rgba").getDouble(0)>.8 && picker().getJSONObject("preview").array("rgba").getDouble(1)<.2)
        fullCapture("picker-pen-hover")
        event(MotionEvent.ACTION_DOWN,p);settle()
        assertEquals("Pen down remains a preview",original,colors().toString())
        event(MotionEvent.ACTION_UP,p)
        waitFor("pen lift accepts") {state().getJSONObject("layer_tools").getString("tool")=="paint"}
        assertTrue(colors().getJSONObject("foreground").array("rgba").getDouble(0)>.8)
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(0,1,0,1))))
        val metrics=activity.resources.displayMetrics
        val offset=(metrics.ydpi*10f/25.4f).coerceIn(36f*density,64f*density)
        tool=MotionEvent.TOOL_TYPE_FINGER
        event(MotionEvent.ACTION_DOWN,p+Offset(0f,offset))
        waitFor("finger hold samples above contact") {picker().objectOrNull("preview")!=null}
        assertTrue("Red sample: ${picker()}",picker().getJSONObject("preview").array("rgba").getDouble(0)>.8 && picker().getJSONObject("preview").array("rgba").getDouble(1)<.2)
        secondFinger(true);settle()
        assertTrue("Second finger selects layer",picker().getBoolean("layer"))
        fullCapture("picker-touch-offset")
        secondFinger(false)
        event(MotionEvent.ACTION_UP)
        waitFor("finger lift accepts") {state().getJSONObject("layer_tools").getString("tool")=="paint"}
        action(obj("type" to "invoke","command" to "eyedropper"));tap(p)
        assertEquals("Finger tap cancels", "paint",state().getJSONObject("layer_tools").getString("tool"))
        for(key in listOf(KeyEvent.KEYCODE_I,KeyEvent.KEYCODE_ESCAPE)) {
            instrumentation.sendKeyDownUpSync(key)
            waitFor("picker keyboard shortcut") {state().getJSONObject("layer_tools").getString("tool")==if(key==KeyEvent.KEYCODE_I)"pick_layer" else "paint"}
        }
        instrumentation.runOnMainSync {host.workspaceInput(obj("type" to "switch","id" to "builtin:workspace:painter"))}
        waitFor("Sketch workspace",30000) {host.workspaceManager?.optString("id")=="builtin:workspace:painter" && host.workspaceManager?.optBoolean("busy")==false}
        settle()
        val panel=state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first {p->p.getJSONObject("content").optJSONArray("tiles")?.objects()?.any{it.getJSONObject("control").optString("kind")=="color_picker"}==true}
        val tile=panel.getJSONObject("content").array("tiles").objects().first {it.getJSONObject("control").optString("kind")=="color_picker"}
        val b=bounds("tile-${panel.getString("id")}-${tile.getInt("id")}")
        tap(b.center);tap(b.center)
        waitFor("settings drawer") {state().getJSONObject("customization").objectOrNull("drawer")!=null}
        assertEquals("[[\"tool_settings\"]]",state().getJSONObject("customization").getJSONObject("drawer").array("columns").toString())
        assertEquals("explicit",state().getJSONObject("customization").getJSONObject("drawer").getString("dismissal"))
        waitFor("settings controls") {find(owner.semanticsOwner.unmergedRootSemanticsNode,"picker-setting-size")!=null}
        SystemClock.sleep(350) // Wait for drawer placement before aiming a real contact.
        tap(bounds("picker-setting-size").let{Offset(it.right-40*density,it.center.y)})
        fullCapture("picker-size-popup")
        // Dropdown opens as a native popup; choose its real semantic row.
        var popupPoint=Offset.Zero
        waitFor("native size menu") {
            fun label(node:SemanticsNode):SemanticsNode?=if(node.config.getOrNull(SemanticsProperties.Text)?.any{it.text=="101 px circle"}==true)node else node.children.firstNotNullOfOrNull(::label)
            android.view.inspector.WindowInspector.getGlobalWindowViews().any { view ->
                findOwner(view)?.let { root -> label(root.semanticsOwner.unmergedRootSemanticsNode)?.let { target ->
                    val a=IntArray(2);val b=IntArray(2);root.view.getLocationOnScreen(a);owner.view.getLocationOnScreen(b)
                    popupPoint=target.boundsInRoot.center+Offset((a[0]-b[0]).toFloat(),(a[1]-b[1]).toFloat());true
                } }==true
            }
        }
        tap(popupPoint)
        waitFor("101 pixel setting") {picker().getInt("sample_width")==101}
        fullCapture("picker-sketch-settings")
        action(obj("type" to "invoke","command" to "eyedropper"))
    }
    @Test fun pickerWheelPreviewPerformance() {
        action(obj("type" to "invoke","command" to "fit_canvas"))
        val p=canvasPoint()
        tool=MotionEvent.TOOL_TYPE_STYLUS
        action(obj("type" to "select_brush","id" to 1));action(obj("type" to "set_brush_size","value" to 220))
        for((i,rgba) in listOf(listOf(1,0,0,1),listOf(0,0,1,1),listOf(0,1,0,1)).withIndex()) {
            action(obj("type" to "set_color","rgba" to JSONArray(rgba)));tap(p+Offset((i-1)*85f,0f))
        }
        action(obj("type" to "set_color_sample_size","width" to 101))
        fun measurements(reset:Boolean):JSONObject {
            val done=CountDownLatch(1);var result:JSONObject?=null
            instrumentation.runOnMainSync {host.measurements(reset){result=it;done.countDown()}}
            assertTrue(done.await(10,TimeUnit.SECONDS));return result!!
        }
        val reports=JSONArray()
        for(visible in listOf(false,true,false,true)) {
            action(obj("type" to "customize","action" to obj("type" to "set_panel_visible","panel" to "color","visible" to visible)))
            waitFor("wheel visibility") {(find(owner.semanticsOwner.unmergedRootSemanticsNode,"color-wheel")!=null)==visible}
            action(obj("type" to "invoke","command" to "eyedropper"))
            event(MotionEvent.ACTION_HOVER_MOVE,p);event(MotionEvent.ACTION_HOVER_MOVE,p)
            waitFor("preview ready"){picker().objectOrNull("preview")!=null};SystemClock.sleep(300)
            measurements(true)
            replayOrigin=IntArray(2).also { instrumentation.runOnMainSync { owner.view.getLocationOnScreen(it) } }
            val start=SystemClock.uptimeMillis()
            waitForInput=false
            repeat(600) { i->
                val delay=start+i*5-SystemClock.uptimeMillis();if(delay>0)SystemClock.sleep(delay)
                event(MotionEvent.ACTION_HOVER_MOVE,p+Offset(100f*cos(i*.03f),20f*sin(i*.03f)))
            }
            val elapsed=SystemClock.uptimeMillis()-start
            waitForInput=true
            replayOrigin=null
            val report=measurements(false).put("wheel",visible).put("duration_ms",elapsed).put("sample_width",101)
            reports.put(report)
            assertEquals("Hover retains panel models",0,report.getInt("panel_content_changes"))
            event(MotionEvent.ACTION_HOVER_EXIT,p)
            action(obj("type" to "invoke","command" to "eyedropper"))
        }
        File(output,"picker-performance.json").writeText(reports.toString())
    }
    @Test fun compactGeometryAndRastersInBothThemes() {
        val report = JSONArray()
        for (theme in listOf("light", "dark")) for (width in listOf(144, 160, 200, 280, 360)) {
            resize(width)
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.2, .72, .58, 1))))
            for (shape in listOf("circle", "square", "triangle")) {
                shape(shape)
                val stage = bounds("color-panel")
                val wheel = bounds("color-wheel")
                assertTrue("Wheel and footer at $width", stage.height >= stage.width)
                assertTrue("Whole picker fits $width", stage.width <= (width - 16) * density + 1.5f && stage.width >= 128 * density - 1.5f)
                val layout = JSONObject(Native.colorPanelLayout(stage.width / density))
                for (slot in listOf("foreground", "background", "transparent", "black", "white", "swap", "wheel")) {
                    val b = bounds(when (slot) { "wheel", "swap" -> "color-$slot"; "black", "white" -> "color-quick-$slot"; else -> "color-swatch-$slot" })
                    val expected = layout.getJSONArray(slot)
                    assertEquals("$slot x", expected.getDouble(0).toFloat() * density, b.left - stage.left, 1.5f)
                    assertEquals("$slot y", expected.getDouble(1).toFloat() * density, b.top - stage.top, 1.5f)
                    assertEquals("$slot width", expected.getDouble(2).toFloat() * density, b.width, 1.5f)
                    assertTrue("$slot fits", b.left >= stage.left - 1 && b.right <= stage.right + 1 && b.top >= stage.top - 1 && b.bottom <= stage.bottom + 1)
                }
                val foreground = bounds("color-swatch-foreground")
                val background = bounds("color-swatch-background")
                assertTrue(foreground.width > background.width && foreground.overlaps(background))
                assertEquals(background.width, bounds("color-swatch-transparent").width, 1f)
                for (model in listOf("shape", "rgb")) {
                    if (colors().getString("readout") != model) color(obj("op" to "toggle_readout"))
                    assertEquals(if (model == "rgb") "RGB" else mapOf("circle" to "OKLCH", "square" to "HSB", "triangle" to "HLS")[shape], view().getString("readout_label"))
                    val shot = capture("$theme-$width-$shape-$model")
                    // Compare a field interior sample against Rust's actual selected color.
                    val relative = wheel.center - stage.topLeft
                    val pixel = shot.getPixel(relative.x.roundToInt(), relative.y.roundToInt())
                    assertEquals("Opaque center", 255, android.graphics.Color.alpha(pixel))
                    val expected = expectedPick("field", stage.topLeft + Offset(relative.x.roundToInt() + .5f, relative.y.roundToInt() + .5f)).getJSONObject("foreground").getJSONArray("rgba")
                    val channels = listOf(android.graphics.Color.red(pixel), android.graphics.Color.green(pixel), android.graphics.Color.blue(pixel))
                    channels.forEachIndexed { i, actual -> assertEquals("$theme $width $shape rendered channel $i", expected.getDouble(i) * 255, actual.toDouble(), 4.0) }
                    // Exercise the retained hue shader after shape/size changes.
                    // Sample the stroke interior, away from its moving marker.
                    val geometry = view().getJSONObject("geometry")
                    val radius = (geometry.number("inner") + geometry.number("outer")) * wheel.width / 2
                    val marker = view().array("wheel_hue_marker").let { wheel.topLeft + Offset(it.getDouble(0).toFloat(), it.getDouble(1).toFloat()) * wheel.width }
                    val stops = JSONArray(Native.colorHueStops(shape, "Srgb")).objects()
                    for (degrees in listOf(15, 75, 135, 195, 255, 315)) {
                        val radians = degrees * PI / 180
                        val at = wheel.center + Offset(cos(radians).toFloat(), sin(radians).toFloat()) * radius
                        if ((at - marker).getDistance() < 16 * density) continue
                        val x = (at.x - stage.left).roundToInt(); val y = (at.y - stage.top).roundToInt()
                        val angle = atan2(y + .5 - relative.y, x + .5 - relative.x) * 180 / PI
                        val t = ((angle - view().number("wheel_hue_start_degrees") + 720) % 360 / 360).toFloat()
                        val last = stops.indexOfFirst { it.number("offset") >= t }.coerceAtLeast(1)
                        val a = stops[last - 1]; val b = stops[last]
                        val mix = (t - a.number("offset")) / (b.number("offset") - a.number("offset"))
                        val ring = shot.getPixel(x, y)
                        listOf(android.graphics.Color.red(ring), android.graphics.Color.green(ring), android.graphics.Color.blue(ring)).forEachIndexed { i, actual ->
                            val wanted = a.array("color").getDouble(i) * (1 - mix) + b.array("color").getDouble(i) * mix
                            assertEquals("$theme $width $shape hue ring $degrees channel $i", wanted * 255, actual.toDouble(), 4.0)
                        }
                    }
                    shot.recycle()
                }
                report.put(obj("theme" to theme, "width" to width, "shape" to shape, "side" to stage.width / density, "layout" to layout))
            }
        }
        File(output, "geometry.json").writeText(report.toString(2))
        // Height-constrained floating panels retain a square instead of clipping controls.
        resize(360, 190)
        assertTrue(bounds("color-panel").width < 344 * density)
        capture("short-panel").recycle()
    }
    private fun expectedPick(part: String, at: Offset): JSONObject {
        val wheel = bounds("color-wheel")
        val handle = referenceHandle
        Native.dispatch(handle, obj("type" to "set_color", "rgba" to colors().getJSONObject("foreground").getJSONArray("rgba")).toString())
        Native.dispatch(handle, obj("type" to "color", "action" to obj("op" to "shape", "shape" to view().getString("shape"))).toString())
        // An achromatic RGB value does not carry its remembered hue.
        val hueMarker = view().getJSONArray("wheel_hue_marker")
        Native.dispatch(handle, obj("type" to "color", "action" to obj("op" to "pick_wheel", "part" to "hue", "size" to wheel.width,
            "point" to JSONArray(listOf(hueMarker.getDouble(0) * wheel.width, hueMarker.getDouble(1) * wheel.width)))).toString())
        Native.dispatch(handle, obj("type" to "color", "action" to obj("op" to "pick_wheel", "part" to part, "size" to wheel.width,
            "point" to JSONArray(listOf(at.x - wheel.left, at.y - wheel.top)))).toString())
        return JSONObject(Native.snapshot(handle)!!).getJSONObject("state").getJSONObject("colors")
    }

    private fun assertPaint(expected: JSONObject, label: String = "pick") {
        try {
            waitFor("shared color acknowledgement", 2_000) {
                val a = colors().getJSONObject("foreground").getJSONArray("rgba"); val b = expected.getJSONObject("foreground").getJSONArray("rgba")
                (0..3).all { abs(a.getDouble(it) - b.getDouble(it)) < .0001 }
            }
        } catch (failure: AssertionError) {
            fail("$label tool=$tool expected=$expected actual=${colors()}")
        }
    }
    @Test fun touchPenAndMousePickWithoutHoldAndKeepCapture() {
        for (width in listOf(144, 280)) for (pointer in tools) for (shape in listOf("circle", "square", "triangle")) {
            resize(width); tool = pointer; shape(shape)
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.2, .72, .58, 1))))
            val wheel = bounds("color-wheel")
            val g = view().getJSONObject("geometry")
            val ringRadius = (g.number("outer") + g.number("inner")) * .5f * wheel.width
            // Every quadrant, including underneath the curved readout and corner controls.
            for (degrees in 0 until 360 step 15) {
                val a = degrees * PI.toFloat() / 180
                val at = wheel.center + Offset(cos(a), sin(a)) * ringRadius
                val expected = expectedPick("hue", at)
                tap(at); assertPaint(expected, "$width $shape ring $degrees at $at")
            }
            val start = wheel.center + Offset(wheel.width * .05f, -wheel.width * .08f)
            var expected = expectedPick("field", start)
            event(MotionEvent.ACTION_DOWN, start); assertPaint(expected) // before native hold timeout
            val outside = wheel.topLeft - Offset(wheel.width * .15f, wheel.height * .15f)
            expected = expectedPick("field", outside)
            event(MotionEvent.ACTION_MOVE, outside); assertPaint(expected)
            event(MotionEvent.ACTION_UP); settle()
            // CANCEL keeps the last accepted color, ignores stale motion, and releases ownership.
            expected = expectedPick("field", start)
            event(MotionEvent.ACTION_DOWN, start); assertPaint(expected)
            event(MotionEvent.ACTION_CANCEL); settle()
            val cancelled = colors().toString()
            event(MotionEvent.ACTION_MOVE, wheel.bottomRight); settle()
            assertEquals(cancelled, colors().toString())
            tap(start); assertPaint(expected)
            val readout = bounds("color-readout")
            val before = colors().getString("readout")
            tap(readout.topLeft + Offset(8 * density, 7 * density))
            assertNotEquals(before, colors().getString("readout"))
            for (slot in listOf("background", "foreground", "transparent")) {
                tap(bounds("color-swatch-$slot").center)
                assertEquals(slot, colors().getString("slot"))
            }
            val remembered = colors().getJSONObject("foreground").getJSONArray("rgba").toString()
            val alternative = view().array("other_shapes").getString(0)
            tap(bounds("color-shape-$alternative").center)
            assertEquals(alternative, colors().getString("shape"))
            assertEquals(remembered, colors().getJSONObject("foreground").getJSONArray("rgba").toString())
            val fg = colors().getJSONObject("foreground").getJSONArray("rgba").toString(); val bg = colors().getJSONObject("background").getJSONArray("rgba").toString()
            tap(bounds("color-swap").center)
            assertEquals(bg, colors().getJSONObject("foreground").getJSONArray("rgba").toString())
            assertEquals(fg, colors().getJSONObject("background").getJSONArray("rgba").toString())
            color(obj("op" to "select", "slot" to "foreground"))
        }
    }
    @Test fun neutralShortcutsPreserveRememberedColorsWithTouchPenAndMouse() {
        resize(280, 340)
        for (device in tools) {
            tool = device
            color(obj("op" to "select", "slot" to "foreground"))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.2, .7, .4, 1))))
            val foreground = colors().getJSONObject("foreground").toString()
            val background = colors().getJSONObject("background").toString()
            tap(bounds("color-swatch-transparent").center)
            assertEquals("transparent", colors().getString("slot"))
            for (white in listOf(false, true, false)) {
                tap(bounds(if (white) "color-quick-white" else "color-quick-black").center)
                assertEquals("temporary", colors().getString("slot"))
                assertEquals(foreground, colors().getJSONObject("foreground").toString())
                assertEquals(background, colors().getJSONObject("background").toString())
                val rgba = colors().getJSONObject("temporary").getJSONArray("rgba")
                for (i in 0..2) assertEquals(if (white) 1.0 else 0.0, rgba.getDouble(i), .00001)
            }
            tap(bounds("color-swatch-foreground").center)
            tap(bounds("color-quick-white").center)
            assertEquals("foreground", colors().getString("slot"))
            assertEquals(background, colors().getJSONObject("background").toString())
            assertEquals(1.0, colors().getJSONObject("foreground").getJSONArray("rgba").getDouble(0), .00001)
        }
        capture("neutral-shortcuts")
    }

    private fun menuOpen(): Boolean {
        var present = false
        instrumentation.runOnMainSync {
            present = android.view.inspector.WindowInspector.getGlobalWindowViews().any { view ->
                findOwner(view)?.let { find(it.semanticsOwner.unmergedRootSemanticsNode, "color-swap-menu") != null } == true
            }
        }
        return present
    }
    @Test fun swatchMenusAndTransparentMemory() {
        for (pointer in tools) {
            resize(144); tool = pointer
            color(obj("op" to "select", "slot" to "transparent"))
            val before = colors().toString()
            event(MotionEvent.ACTION_DOWN, bounds("color-swatch-foreground").center)
            SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 120)
            assertEquals("Mouse hold never opens a menu", pointer != MotionEvent.TOOL_TYPE_MOUSE, menuOpen())
            event(MotionEvent.ACTION_UP); settle()
            if (pointer != MotionEvent.TOOL_TYPE_MOUSE) {
                assertTrue("Held release retains menu", menuOpen())
                assertEquals("Hold does not select paint", before, colors().toString())
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); settle()
            }
            if (pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                button = MotionEvent.BUTTON_SECONDARY
                tap(bounds("color-swatch-foreground").center)
                assertTrue(menuOpen())
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); settle()
                button = MotionEvent.BUTTON_PRIMARY
            }
            color(obj("op" to "select", "slot" to "foreground"))
            val wheel = bounds("color-wheel")
            tap(wheel.center + Offset(wheel.width * .1f, -wheel.width * .1f))
            val marker = view().getJSONArray("wheel_marker").toString()
            val paint = colors().getJSONObject("foreground").getJSONArray("rgba").toString()
            tap(bounds("color-swatch-transparent").center)
            assertEquals(marker, view().getJSONArray("wheel_marker").toString())
            assertEquals(paint, colors().getJSONObject("foreground").getJSONArray("rgba").toString())
            tap(bounds("color-swatch-foreground").center)
            assertEquals(marker, view().getJSONArray("wheel_marker").toString())
        }
    }
    @Test fun dockedAndRetainedDrawerUseTheSameControls() {
        for (theme in listOf("light", "dark")) for (pointer in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            resize(280); tool = pointer
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "move_panel", "panel" to "color", "target" to obj("kind" to "edge", "edge" to "left", "outer" to false),
                "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
            shape("circle")
            tap(bounds("color-shape-square").center)
            assertEquals("square", view().getString("shape"))
            capture("docked-$theme-$pointer").recycle()
            val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "color" in it.array("panels").values() }.getInt("id")
            action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to true)))
            action(obj("type" to "customize", "action" to obj("type" to "set_column_drawers", "column" to group, "drawers" to true)))
            tap(bounds("column-icon-color").center)
            waitFor("retained drawer") { find(owner.semanticsOwner.unmergedRootSemanticsNode, "color-panel") != null }
            settle()
            tap(bounds("color-shape-triangle").center)
            assertEquals("triangle", view().getString("shape"))
            val at = bounds("color-wheel").center
            val expected = expectedPick("field", at)
            tap(at); assertPaint(expected)
            tap(bounds("color-readout").topLeft + Offset(8 * density, 7 * density))
            assertEquals("rgb", colors().getString("readout"))
            val remembered = colors().toString()
            capture("drawer-$theme-$pointer").recycle()
            // Close via its own icon so test contacts never draw on the document.
            tap(bounds("column-icon-color").center)
            tap(bounds("column-icon-color").center)
            waitFor("reopened drawer") { find(owner.semanticsOwner.unmergedRootSemanticsNode, "color-panel") != null }
            assertEquals(remembered, colors().toString())
        }
    }
    @Test fun keyboardActivationAndFocusLoss() {
        // System dispatch must leave touch mode before requesting native focus.
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
        val focus = node("color-readout").config[SemanticsActions.RequestFocus].action!!
        instrumentation.runOnMainSync { assertTrue(focus()) }
        waitFor("readout focus") { host.colorControlFocus != null }
        for (key in listOf(KeyEvent.KEYCODE_SPACE, KeyEvent.KEYCODE_ENTER)) {
            val before = colors().getString("readout")
            instrumentation.sendKeyDownUpSync(key); settle()
            assertNotEquals("Native button activation", before, colors().getString("readout"))
        }
        capture("keyboard-readout-focus").recycle()
        val wheel = bounds("color-wheel")
        tool = MotionEvent.TOOL_TYPE_STYLUS
        event(MotionEvent.ACTION_DOWN, wheel.center)
        settle()
        // A real native dialog transfers window focus and cancels the picker.
        lateinit var dialog: android.app.AlertDialog
        instrumentation.runOnMainSync { dialog = android.app.AlertDialog.Builder(activity).setMessage("Focus test").create(); dialog.show() }
        waitFor("dialog focus") { dialog.window!!.decorView.hasWindowFocus() }
        val saved = colors().toString()
        event(MotionEvent.ACTION_MOVE, wheel.topLeft)
        event(MotionEvent.ACTION_UP); settle()
        assertEquals("Focus loss releases color capture", saved, colors().toString())
        instrumentation.runOnMainSync { dialog.dismiss() }
        waitFor("activity focus") { activity.window.decorView.hasWindowFocus() }
        val expected = expectedPick("field", wheel.center + Offset(10 * density, 0f))
        tap(wheel.center + Offset(10 * density, 0f)); assertPaint(expected)
    }
    @Test fun systemContactsAcrossShapes() {
        resize(144)
        for (pointer in tools) for (shape in listOf("circle", "square", "triangle")) {
            tool = pointer; shape(shape)
            color(obj("op" to "select", "slot" to "foreground"))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.2, .72, .58, 1))))
            val wheel = bounds("color-wheel")
            for ((part, at) in listOf("hue" to wheel.center + Offset(-wheel.width * .435f, 0f),
                    "field" to wheel.center + Offset(wheel.width * .08f, -wheel.width * .05f))) {
                val expected = expectedPick(part, at)
                tap(at); assertPaint(expected, "System $shape $part")
            }
            tap(bounds("color-swatch-background").center)
            assertEquals("background", colors().getString("slot"))
            tap(bounds("color-swatch-foreground").center)
            assertEquals("foreground", colors().getString("slot"))
            if (pointer == MotionEvent.TOOL_TYPE_MOUSE) {
                val before = colors().toString()
                for (nonPrimary in listOf(MotionEvent.BUTTON_SECONDARY, MotionEvent.BUTTON_TERTIARY)) {
                    button = nonPrimary; tap(wheel.center)
                    assertEquals("Non-primary mouse buttons do not pick", before, colors().toString())
                }
                button = MotionEvent.BUTTON_PRIMARY
            }
            capture("system-$pointer-$shape").recycle()
        }
    }
}
