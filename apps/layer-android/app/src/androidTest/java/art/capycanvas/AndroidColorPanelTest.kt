package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.InputDevice
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
import androidx.compose.ui.text.AnnotatedString
import kotlinx.coroutines.runBlocking
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.*

/** Production Compose, shared Rust and typed tablet input, with isolated workspace storage. */
class AndroidColorPanelTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private lateinit var host: CanvasHost
    private lateinit var owner: ViewRootForTest
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

    private fun find(tag: String) = owner.find(hasTag(tag))
    private fun node(tag: String): SemanticsNode {
        var result: SemanticsNode? = null
        instrumentation.runOnMainSync { result = find(tag) }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun bounds(tag: String) = node(tag).boundsInRoot
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun colors() = state().getJSONObject("colors")
    private fun view() = host.snapshot!!.getJSONObject("color_panel")
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) = host.awaitMain(label, timeout, condition = condition)
    private fun settle() { SystemClock.sleep(100); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun action(action: JSONObject) { host.drain(action, 10); settle() }
    private fun color(action: JSONObject) = action(obj("type" to "color", "action" to action))
    private fun shape(shape: String) = color(obj("op" to "shape", "shape" to shape))
    private fun resize(width: Int, height: Int = width + 36) {
        val workspace = JSONObject(fixture.toString())
        val floating = workspace.getJSONObject("layout").getJSONArray("floating").getJSONObject(0)
        floating.put("width", width); floating.put("height", height)
        action(obj("type" to "restore_workspace", "workspace" to workspace))
        waitFor("color panel") { find("color-panel") != null }; settle()
    }
    @Before fun ready() {
        scenario = launchCapy()
        device.landscape(scenario)
        scenario.onActivity {
            activity = it; host = it.host; owner = it.window.decorView.descendant<ViewRootForTest>()!!; density = it.resources.displayMetrics.density
        }
        action(obj("type" to "close_settings"))
        val defaults = createEnglishHostForTest()
        try { action(obj("type" to "restore_workspace", "workspace" to JSONObject(Native.snapshot(defaults)!!).getJSONObject("state").getJSONObject("workspace"))) }
        finally { Native.destroy(defaults) }
        if (state().getJSONObject("workspace").optBoolean("zen_mode")) action(obj("type" to "invoke", "command" to "zen_mode"))
        action(obj("type" to "move_panel", "panel" to "color", "target" to obj("kind" to "float", "position" to JSONArray(listOf(440, 120))),
            "viewport" to JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))))
        fixture = JSONObject(state().getJSONObject("workspace").toString())
        resize(280)
        referenceHandle = createEnglishHostForTest()
    }
    @After fun cleanup() {
        try { if (contact) event(MotionEvent.ACTION_CANCEL) }
        finally {
            if (referenceHandle != 0L) Native.destroy(referenceHandle)
            if (::scenario.isInitialized) scenario.close()
        }
    }
    private fun event(action: Int, next: Offset = point) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); contact = true }
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 7; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = next.x; y = next.y; pressure = if (action in listOf(MotionEvent.ACTION_UP,MotionEvent.ACTION_HOVER_ENTER,MotionEvent.ACTION_HOVER_MOVE,MotionEvent.ACTION_HOVER_EXIT)) 0f else .7f })
        val source = when (tool) { MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS; MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE; else -> InputDevice.SOURCE_TOUCHSCREEN }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL,
            MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT)) button else 0
        deliver(MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, buttons, 1f, 1f, 0, 0, source, 0))
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) contact = false
    }
    private fun deliver(motion: MotionEvent) {
        try {
            if (systemInput) {
                val origin = replayOrigin ?: IntArray(2).also { instrumentation.runOnMainSync { owner.view.getLocationOnScreen(it) } }
                motion.offsetLocation(origin[0].toFloat(), origin[1].toFloat())
                assertTrue("Android accepts typed pointer input", instrumentation.uiAutomation.injectInputEvent(motion, waitForInput))
            } else instrumentation.runOnMainSync {
                if (motion.actionMasked in listOf(MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT))
                    owner.view.dispatchGenericMotionEvent(motion)
                else owner.view.dispatchTouchEvent(motion)
            }
        } finally { motion.recycle() }
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
        deliver(MotionEvent.obtain(downAt,SystemClock.uptimeMillis(),(if(down)MotionEvent.ACTION_POINTER_DOWN else MotionEvent.ACTION_POINTER_UP) or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),2,properties,coords,0,0,1f,1f,0,0,InputDevice.SOURCE_TOUCHSCREEN,0))
    }
    private fun fullCapture(name:String) {
        val image=instrumentation.uiAutomation.takeScreenshot()
        File(output,"$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG,100,it) };image.recycle()
    }
    private fun paintPair() = host.snapshot!!.getJSONObject("paint_pair")
    private fun paintPairFixture() {
        val workspace = JSONObject(fixture.toString())
        val layout = workspace.getJSONObject("layout")
        layout.array("panels").objects().first { it.getString("id") == "toolbar" }.apply {
            put("tile_style", "large")
            getJSONObject("content").put("tiles", JSONArray().put(obj("id" to 900, "control" to obj("kind" to "color"))))
        }
        layout.put("next_tile_id", 901)
        layout.put("header", obj("size" to "small", "next_id" to 903, "zones" to JSONArray(listOf(
            JSONArray().put(obj("id" to 902, "item" to obj("kind" to "tool", "control" to obj("kind" to "color")))), JSONArray(), JSONArray()))))
        action(obj("type" to "restore_workspace", "workspace" to workspace))
        val viewport = JSONArray(listOf(bounds("workspace").width / density, bounds("workspace").height / density))
        action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "float", "position" to JSONArray(listOf(120, 120))), "viewport" to viewport))
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "palettes", "visible" to true)))
        action(obj("type" to "move_panel", "panel" to "palettes", "target" to obj("kind" to "tab", "group" to colorGroup()), "viewport" to viewport))
        action(obj("type" to "select_panel_tab", "group" to colorGroup(), "panel" to "color"))
        waitFor("live paint icons") { paintIconTags.all { find(it) != null } && find("tab-icon-palettes") != null }
        assertEquals("palette", host.snapshot!!.array("panels").objects().first { it.getString("id") == "palettes" }.getString("icon"))
    }
    private val paintIconTags = listOf("tile-icon-toolbar-900", "header-control-902", "tab-icon-color")
    private fun colorGroup() = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "color" in it.array("panels").values() }.getInt("id")
    private fun assertPaintIcons(name: String) {
        settle()
        val committed = CountDownLatch(1)
        instrumentation.runOnMainSync {
            owner.view.viewTreeObserver.registerFrameCommitCallback { committed.countDown() }
            owner.view.invalidate()
        }
        assertTrue("Live icons commit a frame", committed.await(5, TimeUnit.SECONDS))
        val pair = paintPair()
        File(output, "$name-pair.json").writeText(pair.toString())
        val screenshot = instrumentation.uiAutomation.takeScreenshot()
        val origin = IntArray(2)
        instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
        val header = host.snapshot!!.getJSONObject("header")
        val iconSize = header.array("sizes").objects().first { it.getString("id") == header.getJSONObject("model").getString("size") }.number("icon") * density
        val headerCenter = bounds("header-control-902").center
        val regions = listOf(bounds("tile-icon-toolbar-900"), androidx.compose.ui.geometry.Rect(headerCenter - Offset(iconSize / 2, iconSize / 2), androidx.compose.ui.geometry.Size(iconSize, iconSize)), bounds("tab-icon-color"))
        for ((index, region) in regions.withIndex()) {
            val crop = Bitmap.createBitmap(screenshot, (region.left + origin[0]).roundToInt(), (region.top + origin[1]).roundToInt(), region.width.roundToInt(), region.height.roundToInt())
            File(output, "$name-icon-$index.png").outputStream().use { crop.compress(Bitmap.CompressFormat.PNG, 100, it) }
            val slots = pair.array("swatches").objects().associateBy { it.getString("slot") }
            val front = pair.getString("front_swatch")
            fun geometry(slot: String) = if (slot == "foreground") Offset(6.75f, 6.75f) to 6f else Offset(11f, 11f) to 4.25f
            for (probe in listOf(Offset(4f, 4f), Offset(8f, 4f), Offset(12.8f, 12.8f), Offset(14f, 10.5f), Offset(9.5f, 9.5f))) {
                val x = (probe.x / 16 * crop.width).toInt().coerceIn(0, crop.width - 1)
                val y = (probe.y / 16 * crop.height).toInt().coerceIn(0, crop.height - 1)
                val actualPoint = Offset((x + .5f) / crop.width * 16, (y + .5f) / crop.height * 16)
                val slot = listOf(front, if (front == "foreground") "background" else "foreground").first { candidate ->
                    val (center, radius) = geometry(candidate); (actualPoint - center).getDistance() < radius - .6f
                }
                val (center, radius) = geometry(slot)
                val cell = pair.number("checker_cell")
                val phase = (floor((actualPoint.x - center.x + radius) / cell).toInt() + floor((actualPoint.y - center.y + radius) / cell).toInt()) % 2
                val expected = slots.getValue(slot).array("checker").getJSONArray(phase)
                val pixel = crop.getPixel(x, y)
                for ((channel, shift) in listOf(16, 8, 0).withIndex()) assertEquals("$name/$index/$slot/$probe channel $channel", (expected.getDouble(channel) * 255).roundToInt().toDouble(), (pixel shr shift and 255).toDouble(), 8.0)
                assertEquals(255, pixel ushr 24)
            }
            val (center, radius) = geometry(front)
            val rear = geometry(if (front == "foreground") "background" else "foreground")
            val rim = (0 until crop.height).flatMap { y -> (0 until crop.width).map { x -> x to y } }.filter { (x, y) ->
                val p = Offset((x + .5f) / crop.width * 16, (y + .5f) / crop.height * 16)
                abs((p - center).getDistance() - radius) < .12f && (p - rear.first).getDistance() < rear.second - .8f
            }
            assertTrue("Canonical overlapping rim exists", rim.isNotEmpty())
            val ink = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("text"))
            for ((x, y) in rim) for (shift in listOf(16, 8, 0)) assertEquals("$name/$index front rim", (ink shr shift and 255).toDouble(), (crop.getPixel(x, y) shr shift and 255).toDouble(), 8.0)
            crop.recycle()
        }
        screenshot.recycle()
    }
    private fun editActivePaint(channel: String) {
        action(obj("type" to "customize", "action" to obj("type" to "open_control", "control" to "brush_color")))
        val label = host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("edit")
        waitFor("compact active color") { findTag("property-color-$label", owner) != null }
        instrumentation.runOnMainSync { assertTrue(findTag("property-color-$label", owner)!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
        waitFor("active color editor") { findTag("color-value-hex") != null }
        val opened = JSONObject(Native.colorUi(obj("type" to "editor_open", "colors" to state().displayColors(), "color" to paintPair().getJSONObject("definition"), "display_space" to "Srgb").toString(), host.languageTag))
        instrumentation.runOnMainSync {
            assertTrue("active definition", findTag("color-value-hex")!!.second.config[SemanticsProperties.ContentDescription].single().endsWith(opened.getJSONObject("view").getString("hex")))
            assertTrue(findTag("color-value-0-0")!!.second.config[SemanticsActions.OnClick].action!!.invoke())
        }
        waitFor("typing red") { findTag("color-value-0-0-input") != null }
        instrumentation.runOnMainSync { assertTrue(findTag("color-value-0-0-input")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString((channel.toFloat() * 255).roundToInt().toString()))) }
        settle()
        instrumentation.runOnMainSync { assertTrue(findTag("color-value-0-0-input")!!.second.config[SemanticsActions.OnImeAction].action!!.invoke()) }
        waitFor("red accepted") { findTag("color-value-0-0-input") == null }
        instrumentation.runOnMainSync { assertTrue(findTag("color-use")!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
        waitFor("color editor closed") { findTag("color-use") == null }; settle()
        action(obj("type" to "customize", "action" to obj("type" to "close_control")))
    }
    @Test fun editColorRowsSheetAndCanvasPick() {
        fun click(tag: String) = instrumentation.runOnMainSync { assertTrue(tag, findTag(tag)!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
        fun description(tag: String): String { var text = ""; instrumentation.runOnMainSync { text = findTag(tag)!!.second.config[SemanticsProperties.ContentDescription].single() }; return text }
        fun enabled(tag: String): Boolean { var on = false; instrumentation.runOnMainSync { on = findTag(tag)!!.second.config.getOrNull(SemanticsProperties.Disabled) == null }; return on }
        fun type(name: String, text: String) {
            if (findTag("color-value-$name-input") == null) click("color-value-$name")
            waitFor("typing $name") { findTag("color-value-$name-input") != null }
            instrumentation.runOnMainSync { assertTrue(findTag("color-value-$name-input")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(text))) }
            settle()
            instrumentation.runOnMainSync { assertTrue(findTag("color-value-$name-input")!!.second.config[SemanticsActions.OnImeAction].action!!.invoke()) }
            settle()
        }
        fun hex(color: JSONObject) = JSONObject(Native.colorUi(obj("type" to "editor_open", "colors" to state().displayColors(), "color" to color, "display_space" to "Srgb").toString(), host.languageTag)).getJSONObject("view").getString("hex")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            color(obj("op" to "definition", "color" to obj("space" to "Srgb", "rgba" to JSONArray(listOf(1, 1, 1, 1)))))
            color(obj("op" to "editor_memory", "memory" to obj("forms" to JSONArray(listOf("rgb", "hsl", "oklch")), "search" to "")))
            val original = colors().getJSONObject("foreground").toString()
            click("color-edit-button")
            waitFor("Edit Color") { findTag("color-value-hex") != null }; settle()
            fullCapture("edit-color-$theme")
            fun box(tag: String) = findTag(tag)!!.second.boundsInRoot
            assertEquals("the title is centered", box("color-editor").center.x, box("color-editor-title").center.x, 1.5f)
            assertEquals("Current and New match the eyedropper height", box("color-pair").height, box("color-pick").height, 1f)
            for (tag in listOf("color-pick", "color-value-hex")) assertEquals("$tag centers on Current and New", box("color-pair").center.y, box(tag).center.y, 1f)
            val opened = box("color-editor").size
            assertTrue(description("color-value-hex").endsWith("#FFFFFF"))
            assertEquals("remembered row format", "HSL", description("color-format-1"))
            type("hex", "#CA4B35")
            assertTrue(description("color-value-hex").endsWith("#CA4B35"))
            assertTrue(description("color-value-0-0").endsWith(" 202"))
            type("0-0", "lots")
            assertNotNull("a refused number stays open", findTag("color-value-0-0-input"))
            assertNotNull(findTag("color-editor-error"))
            assertFalse(enabled("color-use"))
            fullCapture("edit-color-refused-$theme")
            instrumentation.runOnMainSync { findTag("color-value-0-0-input")!!.first.view.dispatchKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ESCAPE)) }
            waitFor("refusal dismissed") { findTag("color-value-0-0-input") == null }
            assertTrue(enabled("color-use"))
            val before = description("color-value-0-2").substringAfterLast(' ').toInt()
            val cell = findTag("color-value-0-2")!!.second.boundsInRoot
            tool = MotionEvent.TOOL_TYPE_FINGER
            event(MotionEvent.ACTION_DOWN, cell.center)
            for (step in 1..6) { SystemClock.sleep(16); event(MotionEvent.ACTION_MOVE, cell.center - Offset(0f, step * 8f * density)) }
            event(MotionEvent.ACTION_UP); settle()
            val after = description("color-value-0-2").substringAfterLast(' ').toInt()
            assertTrue("dragging a number up raises it: $before -> $after", after > before)
            assertNull("a drag never opens the field", findTag("color-value-0-2-input"))
            click("color-copy-hex"); settle()
            val shownHex = description("color-value-hex").substringAfterLast(' ')
            instrumentation.runOnMainSync {
                val clip = activity.getSystemService(android.content.ClipboardManager::class.java).primaryClip!!
                assertEquals(shownHex, clip.getItemAt(0).text.toString())
            }
            click("color-current"); settle()
            assertTrue(description("color-value-hex").endsWith("#FFFFFF"))
            fun slide(open: Boolean): List<Float> {
                click(if (open) "color-swatches" else "color-sheet-close")
                return (0 until 40).mapNotNull { SystemClock.sleep(10); findTag("color-sheet")?.second?.boundsInRoot?.top?.takeIf { it > 0f } }
            }
            val opening = slide(true)
            waitFor("swatch sheet") { findTag("color-sheet-search") != null }; settle()
            val rest = box("color-sheet").top
            val rising = opening.dropWhile { it <= rest + 1f }
            assertTrue("the sheet slides up from below: $opening / $rest", rising.isNotEmpty() && rising.zipWithNext().all { (a, b) -> b <= a + 1f })
            assertEquals("the swatch list reaches the divider", box("color-editor-divider").top, box("color-sheet-list").bottom, 1.5f)
            instrumentation.runOnMainSync { assertTrue(findTag("color-sheet-search")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString("zzzz-no-color"))) }
            waitFor("no swatches match") { findTag("color-sheet-empty") != null }
            instrumentation.runOnMainSync { assertTrue(findTag("color-sheet-search")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(""))) }
            waitFor("swatches listed") { findTag("color-sheet-tile") != null }
            fullCapture("edit-color-sheet-$theme")
            click("color-sheet-tile"); settle()
            assertFalse(description("color-value-hex").endsWith("#FFFFFF"))
            val closing = slide(false)
            assertTrue("the sheet slides down: $closing / $rest", closing.any { it > rest + 1f } && closing.dropWhile { it <= rest + 1f }.zipWithNext().all { (a, b) -> b >= a - 1f })
            waitFor("sheet closed") { findTag("color-sheet-search") == null }
            assertEquals("the dialog keeps its size", opened, box("color-editor").size)
            type("hex", "#CA4B35")
            click("color-pick")
            waitFor("picking strip") { findTag("color-strip") != null && picker().optBoolean("editor") }
            settle()
            val strip = findTag("color-strip")!!.second.boundsInRoot
            val area = state().getJSONObject("camera").array("work_area").let { a -> Rect(a.getDouble(0).toFloat(), a.getDouble(1).toFloat(), (a.getDouble(0) + a.getDouble(2)).toFloat(), (a.getDouble(1) + a.getDouble(3)).toFloat()).translate(host.surfaceOrigin) }
            assertTrue("strip sits inside the canvas work area: $strip / $area", area.contains(strip.topLeft) && area.contains(strip.bottomRight))
            fullCapture("edit-color-strip-$theme")
            val touch = tool
            tool = MotionEvent.TOOL_TYPE_MOUSE
            val sample = canvasPoint()
            assertTrue("the sample lies inside the canvas work area", area.contains(sample))
            assertFalse("the floating Color panel leaves the sample visible", bounds("color-panel").contains(sample))
            assertFalse("the picking strip leaves the sample visible", strip.contains(sample))
            event(MotionEvent.ACTION_HOVER_MOVE, sample)
            waitFor("the canvas sample follows the mouse") { picker().optJSONArray("sample_point") != null }
            event(MotionEvent.ACTION_HOVER_MOVE, Offset(strip.left - 4f, strip.center.y))
            waitFor("the strip moves away from the sample") { findTag("color-strip")!!.second.boundsInRoot.topLeft != strip.topLeft }
            val hovered = findTag("color-strip")!!.second.boundsInRoot.center
            event(MotionEvent.ACTION_HOVER_MOVE, hovered)
            waitFor("hovering the strip moves it away") { !findTag("color-strip")!!.second.boundsInRoot.contains(hovered) }
            event(MotionEvent.ACTION_HOVER_EXIT); tool = touch
            click("color-strip")
            waitFor("strip goes back") { findTag("color-value-hex") != null && !picker().optBoolean("editor") }
            assertTrue("the strip keeps the draft", description("color-value-hex").endsWith("#CA4B35"))
            click("color-pick")
            waitFor("picking again") { findTag("color-strip") != null && picker().optBoolean("editor") }
            tap(canvasPoint())
            waitFor("dialog returns with the sample") { findTag("color-value-hex") != null && !picker().optBoolean("editor") }
            val picked = state().getJSONObject("color_picker").getJSONObject("picked")
            assertTrue(description("color-value-hex").endsWith(hex(picked)))
            assertEquals("picking only changes the draft", original, colors().getJSONObject("foreground").toString())
            click("color-use")
            waitFor("Edit Color closed") { findTag("color-use") == null }; settle()
            assertEquals(hex(picked), hex(colors().getJSONObject("foreground")))
            assertEquals("hsl", colors().getJSONObject("editor").getJSONArray("forms").getString(1))
            color(obj("op" to "editor_memory", "memory" to obj("forms" to JSONArray(listOf("linear_rgb", "hsb", "oklab")), "search" to "")))
            click("color-edit-button")
            waitFor("Edit Color with wider formats") { findTag("color-value-hex") != null }; settle()
            assertEquals("wider formats keep the dialog size", opened, box("color-editor").size)
            fullCapture("edit-color-linear-$theme")
            click("color-cancel")
            waitFor("Edit Color closed") { findTag("color-cancel") == null }
        }
    }
    @Test fun retainedPaintIconsAndCompactControlFollowCommittedContext() {
        paintPairFixture()
        val ids = paintIconTags.map { node(it).id }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((slot, rgba) in listOf("foreground" to listOf(.85, .12, .18, 1), "background" to listOf(.08, .28, .9, 1)))
                color(obj("op" to "set_slot", "slot" to slot, "color" to obj("space" to "Srgb", "rgba" to JSONArray(rgba))))
            for (slot in listOf("foreground", "background")) {
                val definitions = paintPair().array("swatches").toString()
                color(obj("op" to "select", "slot" to slot))
                assertEquals(definitions, paintPair().array("swatches").toString())
                assertPaintIcons("$theme-selected-$slot")
            }
            color(obj("op" to "toggle_transparent"))
            assertEquals("background", paintPair().getString("front_swatch"))
            assertPaintIcons("$theme-transparent")
            val foreground = colors().getJSONObject("foreground").toString()
            editActivePaint("0.25")
            assertEquals("transparent edit preserves foreground", foreground, colors().getJSONObject("foreground").toString())
            assertEquals("background", paintPair().getString("front_swatch"))
            assertPaintIcons("$theme-transparent-edited")
            val swatches = paintPair().array("swatches").toString()
            color(obj("op" to "select", "slot" to "transparent"))
            color(obj("op" to "quick_color", "white" to true))
            assertEquals(1.0, paintPair().getJSONObject("definition").array("rgba").getDouble(0), 0.0)
            editActivePaint("0.4")
            assertEquals("temporary edit preserves both stored paints", swatches, paintPair().array("swatches").toString())
            assertEquals(.4, paintPair().getJSONObject("definition").array("rgba").getDouble(0), .001)
            assertPaintIcons("$theme-temporary")
            color(obj("op" to "select", "slot" to "foreground"))
            for (slot in listOf("foreground", "background")) color(obj("op" to "set_slot", "slot" to slot,
                "color" to obj("space" to "Srgb", "rgba" to JSONArray(listOf(.05, .3, .7, if (slot == "foreground") .35 else .6)))))
            assertPaintIcons("$theme-alpha")
            val artwork = colors().toString()
            action(obj("type" to "invoke", "command" to "quick_mask"))
            assertNotNull("Mask painting is active", state().getJSONObject("layer_tools").objectOrNull("mask_editing"))
            color(obj("op" to "select", "slot" to "background"))
            editActivePaint("0.25")
            assertEquals("mask edit preserves artwork", artwork, colors().toString())
            val maskColors = state().getJSONObject("layer_tools").getJSONObject("mask_editing").getJSONObject("colors")
            for (swatch in paintPair().array("swatches").objects()) assertEquals("Mask icon uses mask paint", jsonValue(maskColors.getJSONObject(swatch.getString("slot"))), jsonValue(swatch.getJSONObject("definition")))
            assertPaintIcons("$theme-mask")
            action(obj("type" to "invoke", "command" to "quick_mask"))
            assertPaintIcons("$theme-artwork-restored")
            assertEquals(ids, paintIconTags.map { node(it).id })
        }
    }
    @Test fun retainedPaintIconsUseMappedRenditionAndIgnorePickerHover() {
        paintPairFixture()
        fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
        val task = native { handle ->
            val (id, file) = documentRequest(handle, "new_document")
            Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision"))
        }
        try {
            Native.projectOptions(task, obj("extent" to JSONArray(listOf(320, 240)), "color" to obj("space" to "Srgb", "depth" to "F16"), "background" to "White").toString())
            Native.projectWork(task, -1, 320, 240)
            native { Native.projectAdopt(it, task, "null") }
        } finally { Native.projectFree(task) }
        instrumentation.runOnMainSync { host.documentChanged() }
        waitFor("HDR document ready", 60_000) { view().optBoolean("hdr") && host.snapshot?.optBoolean("brush_ready") == true }
        val ids = paintIconTags.map { node(it).id }
        color(obj("op" to "definition", "color" to srgbLinear(4.0, 1.0, .25, alpha = .5)))
        val definition = paintPair().getJSONObject("definition").toString()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            val initial = paintPair().array("swatches").toString()
            assertPaintIcons("$theme-hdr-initial")
            val recipe = native { JSONObject(Native.query(it, obj("type" to "proof_form").toString())).getJSONObject("rendition") }
            val changed = JSONObject(recipe.toString()).put("exposure", recipe.getDouble("exposure") - 2)
            native { handle ->
                for ((phase, value) in listOf("down" to recipe, "up" to changed)) Native.proofControl(handle, obj("type" to "rendition", "phase" to phase, "recipe" to value).toString())
            }
            instrumentation.runOnMainSync { host.documentChanged() }
            waitFor("committed rendition updates live icons") { paintPair().array("swatches").toString() != initial }
            assertEquals("Rendition retains authored paint", definition, paintPair().getJSONObject("definition").toString())
            assertPaintIcons("$theme-hdr-mapped")
            assertEquals(ids, paintIconTags.map { node(it).id })
            val committed = paintPair().toString()
            action(obj("type" to "invoke", "command" to "fit_canvas"))
            action(obj("type" to "invoke", "command" to "eyedropper"))
            tool = MotionEvent.TOOL_TYPE_MOUSE
            event(MotionEvent.ACTION_HOVER_ENTER, canvasPoint())
            event(MotionEvent.ACTION_HOVER_MOVE, canvasPoint() + Offset(1f, 0f))
            waitFor("picker previews white canvas") { picker().objectOrNull("preview") != null }
            assertEquals("Picker hover preserves committed paint icons", committed, paintPair().toString())
            assertPaintIcons("$theme-hdr-picker-hover")
            event(MotionEvent.ACTION_HOVER_EXIT)
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            waitFor("picker cancelled") { state().getJSONObject("layer_tools").getString("tool") == "paint" }
        }
    }
    @Test fun glassPickerInputAndSettings() {
        assumeTrue("Requires -e systemInput true", systemInput)
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
        waitFor("settings controls") {find("picker-setting-size")!=null}
        SystemClock.sleep(350) // Wait for drawer placement before aiming a real contact.
        tap(bounds("picker-setting-size").let{Offset(it.right-40*density,it.center.y)})
        fullCapture("picker-size-popup")
        // Dropdown opens as a native popup; choose its real semantic row.
        var popupPoint=Offset.Zero
        waitFor("native size menu") {
            fun label(node:SemanticsNode):SemanticsNode?=if(node.config.getOrNull(SemanticsProperties.Text)?.any{it.text=="101 px circle"}==true)node else node.children.firstNotNullOfOrNull(::label)
            semanticsRoots().any { root ->
                label(root.semanticsOwner.unmergedRootSemanticsNode)?.let { target ->
                    val a=IntArray(2);val b=IntArray(2);root.view.getLocationOnScreen(a);owner.view.getLocationOnScreen(b)
                    popupPoint=target.boundsInRoot.center+Offset((a[0]-b[0]).toFloat(),(a[1]-b[1]).toFloat());true
                }==true
            }
        }
        tap(popupPoint)
        waitFor("101 pixel setting") {picker().getInt("sample_width")==101}
        fullCapture("picker-sketch-settings")
        action(obj("type" to "invoke","command" to "eyedropper"))
    }
    @Test fun pickerRetiresRestingContactsAndPendingHolds() {
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:painter")) }
        waitFor("Sketch workspace", 30000) {
            host.workspaceManager?.optString("id") == "builtin:workspace:painter" && host.workspaceManager?.optBoolean("busy") == false
        }
        action(obj("type" to "invoke", "command" to "brush"))
        val panel = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first { p ->
            p.getJSONObject("content").optJSONArray("tiles")?.objects()?.any { it.getJSONObject("control").optString("kind") == "color_picker" } == true
        }
        val tile = panel.getJSONObject("content").array("tiles").objects().first { it.getJSONObject("control").optString("kind") == "color_picker" }
        val tag = "tile-${panel.getString("id")}-${tile.getInt("id")}"
        val p = canvasPoint()
        tool = MotionEvent.TOOL_TYPE_FINGER
        for (terminal in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) {
            for (pendingHold in listOf(false, true)) {
                event(MotionEvent.ACTION_DOWN, p - Offset(120f, 0f))
                if (!pendingHold) event(MotionEvent.ACTION_MOVE, p - Offset(60f, 0f))
                // Activate the actual Compose toolbar control while the OS
                // finger stream remains down; do not replace it with pen input.
                val pickerNode = node(tag)
                instrumentation.runOnMainSync {
                    fun click(node: SemanticsNode): (() -> Boolean)? =
                        node.config.getOrNull(SemanticsActions.OnClick)?.action ?: node.children.firstNotNullOfOrNull(::click)
                    assertTrue(checkNotNull(click(pickerNode)) { "Picker tile needs a click action" }.invoke())
                }
                waitFor("toolbar picker") { state().getJSONObject("layer_tools").getString("tool").startsWith("pick_") }
                SystemClock.sleep(ViewConfiguration.getLongPressTimeout().toLong() + 150)
                assertTrue("Pending hold must preserve toolbar picking: terminal=$terminal pendingHold=$pendingHold",
                    state().getJSONObject("layer_tools").getString("tool").startsWith("pick_"))
                event(terminal)
                settle()
                assertTrue("Old touch must neither accept nor cancel toolbar picking: terminal=$terminal pendingHold=$pendingHold",
                    state().getJSONObject("layer_tools").getString("tool").startsWith("pick_"))
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
                waitFor("cancel picker") { state().getJSONObject("layer_tools").getString("tool") == "paint" }
                val camera = state().getJSONObject("camera").toString()
                repeat(2) {
                    event(MotionEvent.ACTION_DOWN, p)
                    event(MotionEvent.ACTION_MOVE, p + Offset(70f, 50f))
                    event(MotionEvent.ACTION_UP)
                    settle()
                    assertEquals("No ghost contact may transform a single finger", camera, state().getJSONObject("camera").toString())
                }
            }
        }
        event(MotionEvent.ACTION_DOWN, p)
        waitFor("touch owns held picker") { state().getJSONObject("layer_tools").getString("tool").startsWith("pick_") }
        event(MotionEvent.ACTION_CANCEL)
        waitFor("cancel owned touch picker") { state().getJSONObject("layer_tools").getString("tool") == "paint" }
        // Touch navigation still works with two real contacts.
        val before = state().getJSONObject("camera")
        event(MotionEvent.ACTION_DOWN, p)
        secondFinger(true)
        // The first contact moves while the second remains at its original point.
        val properties = (7..8).map { id -> MotionEvent.PointerProperties().apply { this.id = id; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray()
        val coords = listOf(p + Offset(-40f, 50f), p + Offset(100f, 0f)).map { at ->
            MotionEvent.PointerCoords().apply { x = at.x; y = at.y; pressure = .7f }
        }.toTypedArray()
        // The OS cancellation must contain both live pointers. Compose may
        // replace it with an anonymous cancellation when forwarding to canvas.
        for (action in listOf(MotionEvent.ACTION_MOVE, MotionEvent.ACTION_CANCEL))
            deliver(MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 2, properties, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0))
        contact = false
        waitFor("two finger navigation") { state().getJSONObject("camera").getDouble("zoom") != before.getDouble("zoom") }
        assertNotEquals(before.getDouble("rotation"), state().getJSONObject("camera").getDouble("rotation"))
        val cancelled = state().getJSONObject("camera").toString()
        event(MotionEvent.ACTION_DOWN, p)
        event(MotionEvent.ACTION_MOVE, p + Offset(70f, 50f))
        event(MotionEvent.ACTION_UP)
        settle()
        assertEquals("Anonymous cancellation retires every captured finger", cancelled, state().getJSONObject("camera").toString())
    }
    @Test fun pickerWheelPreviewPerformance() {
        assumeTrue("Requires -e systemInput true", systemInput)
        action(obj("type" to "invoke","command" to "fit_canvas"))
        val p=canvasPoint()
        tool=MotionEvent.TOOL_TYPE_STYLUS
        action(obj("type" to "select_brush","id" to 1));action(obj("type" to "set_brush_size","value" to 220))
        for((i,rgba) in listOf(listOf(1,0,0,1),listOf(0,0,1,1),listOf(0,1,0,1)).withIndex()) {
            action(obj("type" to "set_color","rgba" to JSONArray(rgba)));tap(p+Offset((i-1)*85f,0f))
        }
        action(obj("type" to "set_color_sample_size","width" to 101))
        val reports=JSONArray()
        for(visible in listOf(false,true,false,true)) {
            action(obj("type" to "customize","action" to obj("type" to "set_panel_visible","panel" to "color","visible" to visible)))
            waitFor("wheel visibility") {(find("color-wheel")!=null)==visible}
            action(obj("type" to "invoke","command" to "eyedropper"))
            event(MotionEvent.ACTION_HOVER_MOVE,p);event(MotionEvent.ACTION_HOVER_MOVE,p)
            waitFor("preview ready"){picker().objectOrNull("preview")!=null};SystemClock.sleep(300)
            host.measurementReport(true)
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
            val report=host.measurementReport(false).put("wheel",visible).put("duration_ms",elapsed).put("sample_width",101)
            reports.put(report)
            assertEquals("Hover retains panel models",0,report.getInt("panel_content_changes"))
            event(MotionEvent.ACTION_HOVER_EXIT,p)
            action(obj("type" to "invoke","command" to "eyedropper"))
        }
        File(output,"picker-performance.json").writeText(reports.toString())
    }
    @Test fun fillThumbnailEditsItsColor() {
        fun description(tag: String): String { var text = ""; instrumentation.runOnMainSync { text = findTag(tag)!!.second.config[SemanticsProperties.ContentDescription].single() }; return text }
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.2, .4, .8, 1))))
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "solid_color")))
        val id = state().array("layers").objects().first { it.getString("label") == "Solid Color" }.getLong("id")
        waitFor("fill row") { findTag("layer-content-$id") != null }; settle()
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap(findTag("layer-content-$id")!!.second.boundsInRoot.center)
        waitFor("Edit Color from the fill thumbnail") { findTag("color-value-hex") != null }; settle()
        assertTrue("the fill thumbnail opens its color", description("color-value-hex").endsWith("#3366CC"))
        instrumentation.runOnMainSync { assertTrue(findTag("color-value-hex")!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
        waitFor("typing hex") { findTag("color-value-hex-input") != null }
        instrumentation.runOnMainSync { assertTrue(findTag("color-value-hex-input")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString("#D7263D"))) }
        settle()
        instrumentation.runOnMainSync { assertTrue(findTag("color-value-hex-input")!!.second.config[SemanticsActions.OnImeAction].action!!.invoke()) }
        settle()
        fullCapture("fill-thumbnail-editor")
        instrumentation.runOnMainSync { assertTrue(findTag("color-use")!!.second.config[SemanticsActions.OnClick].action!!.invoke()) }
        waitFor("Use Color sets the fill") {
            val rgba = state().array("layers").objects().first { it.getLong("id") == id }.optJSONObject("fill_color")?.getJSONObject("color")?.array("rgba")
            rgba != null && listOf(215, 38, 61).withIndex().all { (i, v) -> abs(rgba.getDouble(i) * 255 - v) < 1.5 }
        }
        fullCapture("fill-thumbnail")
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
                val layout = JSONObject(Native.colorUi(obj("type" to "layout", "size" to (stage.width / density).coerceAtLeast(128f)).toString()))
                for (slot in listOf("foreground", "background", "transparent", "swap", "wheel")) {
                    val b = bounds(when (slot) { "wheel", "swap" -> "color-$slot"; else -> "color-swatch-$slot" })
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
    private fun menuOpen(): Boolean {
        var present = false
        instrumentation.runOnMainSync { present = findTag("color-swap-menu") != null }
        return present
    }
    @Test fun selectedSwatchOwnsOverlapAndKeepsItsRim() {
        fun sample(front: String, predicate: (Float, Float) -> Boolean, score: (Float, Float) -> Float): Offset {
            val back = if (front == "foreground") "background" else "foreground"
            val b = bounds("color-swatch-$front")
            val behind = bounds("color-swatch-$back")
            val stage = bounds("color-panel")
            fun inset(at: Offset) = b.width / 2 - (at - b.center).getDistance()
            fun backInset(at: Offset) = behind.width / 2 - (at - behind.center).getDistance()
            return (floor(b.top - stage.top).toInt()..ceil(b.bottom - stage.top).toInt()).flatMap { y ->
                (floor(b.left - stage.left).toInt()..ceil(b.right - stage.left).toInt()).map { x ->
                    stage.topLeft + Offset(x + .5f, y + .5f)
                }
            }.filter { predicate(inset(it), backInset(it)) }
                .maxByOrNull { score(inset(it), backInset(it)) }
                ?: error("No $front overlap sample")
        }
        fun pixel(shot: Bitmap, at: Offset) = bounds("color-panel").let { stage ->
            shot.getPixel(floor(at.x - stage.left).toInt(), floor(at.y - stage.top).toInt())
        }
        fun assertPixel(label: String, actual: Int, expected: Int) {
            for (channel in listOf(16, 8, 0)) assertEquals("$label channel $channel",
                (expected shr channel and 255).toDouble(), (actual shr channel and 255).toDouble(), 8.0)
        }
        for (theme in listOf("light", "dark")) for (width in listOf(144, 280)) {
            resize(width)
            action(obj("type" to "set_theme", "theme" to theme))
            color(obj("op" to "select", "slot" to "foreground"))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.85, .12, .18, 1))))
            color(obj("op" to "select", "slot" to "background"))
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(.08, .28, .9, 1))))
            val identities = listOf("foreground", "background", "transparent").associateWith { node("color-swatch-$it").id }
            val paints = listOf("foreground", "background").associateWith { colors().getJSONObject(it).toString() }
            for (pointer in tools) for (front in listOf("foreground", "background")) {
                tool = pointer
                color(obj("op" to "select", "slot" to front))
                assertEquals(front, view().getString("front_swatch"))
                identities.forEach { (slot, id) -> assertEquals("Retained $slot control", id, node("color-swatch-$slot").id) }
                val back = if (front == "foreground") "background" else "foreground"
                val frontPadding = if (front == "foreground") 3 else 1
                val backPadding = if (back == "foreground") 3 else 1
                val overlap = sample(front, { a, b -> a > (frontPadding + 2) * density && b > (backPadding + 2) * density }, ::min)
                val rim = sample(front, { a, b -> abs(a - density) < .3f * density && b > (backPadding + 2) * density }, { a, _ -> -abs(a - density) })
                val clipped = sample(front, { a, b -> a < -density && b > (backPadding + 2) * density }, { a, b -> min(-a, b) })
                val rgba = view().array("swatches").objects().first { it.getString("slot") == front }.array("rgba")
                val fill = android.graphics.Color.rgb((rgba.getDouble(0) * 255).roundToInt(), (rgba.getDouble(1) * 255).roundToInt(), (rgba.getDouble(2) * 255).roundToInt())
                val text = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("text"))
                capture("overlap-$theme-$width-$pointer-$front").let { shot ->
                    assertPixel("Selected $front overlap", pixel(shot, overlap), fill)
                    assertPixel("Selected $front rim over $back", pixel(shot, rim), text)
                    shot.recycle()
                }
                color(obj("op" to "select", "slot" to "transparent"))
                assertEquals("Transparent keeps remembered paint in front", front, view().getString("front_swatch"))
                capture("transparent-overlap-$theme-$width-$pointer-$front").let { shot ->
                    assertPixel("Remembered $front overlap", pixel(shot, overlap), fill)
                    shot.recycle()
                }
                if (pointer != MotionEvent.TOOL_TYPE_FINGER) {
                    event(MotionEvent.ACTION_HOVER_ENTER, bounds("color-swatch-$back").center)
                    event(MotionEvent.ACTION_HOVER_MOVE, overlap)
                    event(MotionEvent.ACTION_HOVER_MOVE, overlap + Offset(.5f, .5f))
                    capture("hover-overlap-$theme-$width-$pointer-$front").let { shot ->
                        assertPixel("Hovered remembered $front rim", pixel(shot, rim), text)
                        shot.recycle()
                    }
                    assertEquals("Hover preserves transparency", "transparent", colors().getString("slot"))
                    color(obj("op" to "select", "slot" to back))
                    color(obj("op" to "select", "slot" to "transparent"))
                    event(MotionEvent.ACTION_HOVER_MOVE, overlap + Offset(1f, 0f))
                    val movedRim = sample(back, { a, b -> abs(a - density) < .3f * density && b > (frontPadding + 2) * density }, { a, _ -> -abs(a - density) })
                    val rearRim = sample(front, { a, b -> abs(a - .5f * density) < .2f * density && b < -2 * density }, { a, _ -> -abs(a - .5f * density) })
                    val panel = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("panel"))
                    fun faded(channel: Int) = ((text shr channel and 255) * .25 + (panel shr channel and 255) * .75).roundToInt()
                    val quiet = android.graphics.Color.rgb(faded(16), faded(8), faded(0))
                    capture("hover-switch-$theme-$width-$pointer-$back").let { shot ->
                        assertPixel("Hover follows newly visible $back after selection", pixel(shot, movedRim), text)
                        assertPixel("Hover leaves rear $front after selection", pixel(shot, rearRim), quiet)
                        shot.recycle()
                    }
                    event(MotionEvent.ACTION_HOVER_EXIT)
                    color(obj("op" to "select", "slot" to front))
                    color(obj("op" to "select", "slot" to "transparent"))
                }
                SystemClock.sleep(ViewConfiguration.getDoubleTapTimeout().toLong() + 30)
                tap(overlap)
                assertEquals("Visible overlap receives $pointer contact", front, colors().getString("slot"))
                tap(clipped)
                assertEquals("Circular clipping lets $pointer reach $back", back, colors().getString("slot"))
                paints.forEach { (slot, paint) -> assertEquals("Contact preserves $slot paint", paint, colors().getJSONObject(slot).toString()) }
            }
        }
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
            waitFor("retained drawer") { find("color-panel") != null }
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
            waitFor("reopened drawer") { find("color-panel") != null }
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
