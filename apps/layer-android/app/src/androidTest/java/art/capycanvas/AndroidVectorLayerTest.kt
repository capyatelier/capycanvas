package art.capycanvas

import android.graphics.Bitmap
import android.view.WindowManager
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Vector layer journeys with stylus contacts on the device: discovering vector layers, wrong brushes, coloring line art, selections, eraser modes and merges, plus G-Pen lines, Figure shapes, Path, Edit Points, Properties, the Move frame and the eraser. */
class AndroidVectorLayerTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    @Before fun ready() {
        wakeDevice()
        compose.activity.runOnUiThread { compose.activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        host.awaitReady(compose = compose)
    }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        compose.runOnIdle { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(15, TimeUnit.SECONDS))
        compose.waitForIdle()
        assertNull(host.actionError)
        assertNull(host.failure)
    }
    private fun canvasView(): CanvasSurfaceView {
        fun find(view: android.view.View): CanvasSurfaceView? {
            if (view is CanvasSurfaceView) return view
            if (view is android.view.ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
            return null
        }
        return find(compose.activity.window.decorView)!!
    }
    private fun canvasEvent(phase: Int, point: Offset) {
        instrumentation.runOnMainSync {
            val canvas = canvasView()
            val properties = android.view.MotionEvent.PointerProperties().apply { id = 0; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS }
            val coords = android.view.MotionEvent.PointerCoords().apply { x = point.x * canvas.width; y = point.y * canvas.height; pressure = .8f }
            val now = android.os.SystemClock.uptimeMillis()
            val event = android.view.MotionEvent.obtain(now - 30, now, phase, 1, arrayOf(properties), arrayOf(coords), 0, 0, 1f, 1f, 1, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
            assertTrue(canvas.dispatchTouchEvent(event)); event.recycle()
        }
        android.os.SystemClock.sleep(8)
    }
    private fun drag(from: Offset, to: Offset) {
        canvasEvent(android.view.MotionEvent.ACTION_DOWN, from)
        for (i in 1..24) canvasEvent(android.view.MotionEvent.ACTION_MOVE, from + (to - from) * (i / 24f))
        canvasEvent(android.view.MotionEvent.ACTION_UP, to)
    }
    private fun pixel(point: Offset): Int {
        var origin = IntArray(2); var size = IntArray(2)
        instrumentation.runOnMainSync { val canvas = canvasView(); canvas.getLocationOnScreen(origin); size = intArrayOf(canvas.width, canvas.height) }
        val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
        val readable = bitmap.copy(Bitmap.Config.ARGB_8888, false)
        val color = readable.getPixel(origin[0] + (point.x * size[0]).toInt(), origin[1] + (point.y * size[1]).toInt())
        readable.recycle(); bitmap.recycle(); return color
    }
    private fun green(color: Int) = (color shr 8 and 255) - maxOf(color shr 16 and 255, color and 255) > 25
    private fun orange(color: Int) = (color shr 16 and 255) - (color and 255) > 90
    private fun awaitPixel(name: String, point: Offset, inked: Boolean, ink: (Int) -> Boolean = ::green) {
        try { compose.waitUntil(20_000) { ink(pixel(point)) == inked || host.failure != null || host.actionError != null } }
        catch (e: Throwable) { screenshot("vector-$name-failure.png"); throw e }
        assertNull(host.failure); assertNull(host.actionError)
        assertEquals(name, inked, ink(pixel(point)))
    }
    private fun tap(point: Offset) {
        canvasEvent(android.view.MotionEvent.ACTION_DOWN, point)
        canvasEvent(android.view.MotionEvent.ACTION_UP, point)
    }
    private fun tool(tool: JSONObject) = action(obj("type" to "layer", "action" to obj("op" to "tool", "tool" to tool)))
    private fun figure(shape: String, paint: String) = tool(obj("figure" to obj("shape" to shape, "paint" to paint)))
    private fun selectedLayer() = host.snapshot!!.getJSONObject("state").getJSONArray("layers").objects().first { it.getBoolean("selected") }.getLong("id")
    @Test fun gPenLinesDrawMoveAndUndoOnVectorLayers() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.1, .7, .2, 1.0))))
            action(obj("type" to "layer", "action" to obj("op" to "new_vector")))
            action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_brush_size", "value" to 24))
            val y = .5f
            val left = Offset(.38f, y)
            val press = Offset(.48f, y)
            val probe = Offset(.43f, y)
            val below = Offset(.43f, y + .1f)
            drag(left, Offset(.6f, y))
            awaitPixel("$theme-drawn", probe, true)
            awaitPixel("$theme-empty-below", below, false)
            screenshot("vector-$theme-drawn.png")
            action(obj("type" to "invoke", "command" to "move"))
            drag(press, press + (below - probe))
            awaitPixel("$theme-moved-away", probe, false)
            awaitPixel("$theme-moved-to", below, true)
            screenshot("vector-$theme-moved.png")
            action(obj("type" to "invoke", "command" to "undo"))
            awaitPixel("$theme-undone", probe, true)
            awaitPixel("$theme-undone-below", below, false)
            action(obj("type" to "invoke", "command" to "undo"))
            action(obj("type" to "invoke", "command" to "pen"))
        }
    }
    @Test fun vectorToolsDrawEditRestyleScaleAndErase() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "layer", "action" to obj("op" to "new_vector")))
            action(obj("type" to "invoke", "command" to "fit_canvas"))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.85, .35, .1, 1.0))))
            action(obj("type" to "set_brush_size", "value" to 24))
            figure("rectangle", "fill")
            drag(Offset(.34f, .34f), Offset(.44f, .46f))
            awaitPixel("$theme-rectangle", Offset(.39f, .4f), true, ::orange)
            awaitPixel("$theme-beside-rectangle", Offset(.49f, .4f), false, ::orange)
            screenshot("vector-$theme-figure.png")

            figure("path", "outline")
            for (at in listOf(Offset(.55f, .34f), Offset(.65f, .34f))) tap(at)
            drag(Offset(.65f, .46f), Offset(.7f, .5f))
            screenshot("vector-$theme-path-drafting.png")
            instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_ENTER)
            awaitPixel("$theme-path", Offset(.6f, .34f), true, ::orange)
            screenshot("vector-$theme-path.png")

            action(obj("type" to "invoke", "command" to "edit_points"))
            awaitPixel("$theme-before-edit", Offset(.6f, .41f), false, ::orange)
            drag(Offset(.55f, .34f), Offset(.55f, .48f))
            awaitPixel("$theme-edited", Offset(.6f, .41f), true, ::orange)
            screenshot("vector-$theme-edit-points.png")

            val wide = Offset(.635f, .4f)
            awaitPixel("$theme-before-restyle", wide, false, ::orange)
            action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to selectedLayer(), "key" to "path_width",
                "value" to obj("kind" to "number", "value" to 160))))
            awaitPixel("$theme-restyled", wide, true, ::orange)
            screenshot("vector-$theme-properties.png")

            action(obj("type" to "invoke", "command" to "move"))
            drag(Offset(.33f, .31f), Offset(.47f, .5f))
            screenshot("vector-$theme-frame.png")
            awaitPixel("$theme-before-scale", Offset(.47f, .5f), false, ::orange)
            drag(Offset(.44f, .46f), Offset(.5f, .54f))
            awaitPixel("$theme-scaled", Offset(.47f, .5f), true, ::orange)
            screenshot("vector-$theme-scaled.png")

            action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.1, .7, .2, 1.0))))
            drag(Offset(.34f, .68f), Offset(.7f, .68f))
            awaitPixel("$theme-inked", Offset(.5f, .68f), true)
            action(obj("type" to "invoke", "command" to "eraser"))
            action(obj("type" to "set_brush_size", "value" to 40))
            drag(Offset(.5f, .62f), Offset(.5f, .74f))
            awaitPixel("$theme-erased", Offset(.5f, .68f), false)
            awaitPixel("$theme-kept", Offset(.4f, .68f), true)
            screenshot("vector-$theme-erased.png")
            action(obj("type" to "layer", "action" to obj("op" to "delete_selected")))
            action(obj("type" to "invoke", "command" to "pen"))
        }
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun awaitState(name: String, done: (JSONObject) -> Boolean) {
        try { compose.waitUntil(20_000) { done(state()) || host.failure != null || host.actionError != null } }
        catch (e: Throwable) { screenshot("vector-$name-failure.png"); throw e }
        assertNull(host.failure); assertNull(host.actionError)
    }
    private fun selectedRow() = state().getJSONArray("layers").objects().first { it.getBoolean("selected") }
    private fun vectorSelected() = selectedRow().optString("content_icon") == "layer-path-symbolic"
    private fun brushId(label: String) = host.catalog.getJSONArray("brush_categories").objects().flatMap { it.getJSONArray("brushes").objects() }.first { it.getString("label") == label }.getLong("id")
    private fun headerTool(key: String, value: String): String {
        val zones = state().getJSONObject("workspace").getJSONObject("layout").getJSONObject("header").getJSONArray("zones")
        val item = (0 until zones.length()).flatMap { zones.getJSONArray(it).objects() }
            .first { it.optJSONObject("item")?.optJSONObject("control")?.optString(key) == value }
        return "header-item-${item.get("id")}"
    }
    private fun fromDrawer(key: String, value: String, shown: () -> Boolean, act: () -> Unit) {
        val opener = if (shown()) null else headerTool(key, value)
        opener?.let { compose.onNodeWithTag(it).performClick(); compose.waitUntil(20_000, shown) }
        act()
        opener?.let { compose.onNodeWithTag(it).performClick(); compose.waitUntil(20_000) { !shown() } }
    }
    private fun clickNewVectorLayer() {
        val label = host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("new_vector_layer")
        fromDrawer("panel", "layers", { compose.onAllNodesWithContentDescription(label).fetchSemanticsNodes().isNotEmpty() }) {
            compose.onAllNodesWithContentDescription(label).onFirst().performClick()
            compose.waitForIdle()
            awaitState("new-vector-layer") { vectorSelected() }
        }
    }
    private fun noticeAction(id: String) {
        awaitState("notice-$id") { s -> s.optJSONObject("notice")?.getJSONArray("actions")?.objects()?.any { it.getString("id") == id } == true }
        compose.onNodeWithTag("canvas-notice-action-$id").performClick()
        compose.waitForIdle()
    }
    private fun thumbnailGreen(tag: String): Boolean {
        val center = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInWindow.center
        val origin = IntArray(2).also { instrumentation.runOnMainSync { compose.activity.window.decorView.getLocationOnScreen(it) } }
        val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: return false
        val readable = bitmap.copy(Bitmap.Config.ARGB_8888, false)
        try { return green(readable.getPixel((center.x + origin[0]).toInt(), (center.y + origin[1]).toInt())) }
        finally { readable.recycle(); bitmap.recycle() }
    }
    private fun awaitThumbnail(id: Long) {
        val deadline = android.os.SystemClock.uptimeMillis() + 30_000
        while (compose.onAllNodesWithTag("layer-thumbnail-$id-false", useUnmergedTree = true).fetchSemanticsNodes().isEmpty() || !thumbnailGreen("layer-content-$id")) {
            assertTrue("the thumbnail of layer $id shows its fill", android.os.SystemClock.uptimeMillis() < deadline)
            compose.mainClock.advanceTimeBy(120)
            android.os.SystemClock.sleep(20)
        }
    }
    private fun dark(color: Int) = maxOf(color shr 16 and 255, color shr 8 and 255, color and 255) < 100
    private fun marked(color: Int) = (color shr 16 and 255) + (color shr 8 and 255) + (color and 255) < 600
    @Test fun vectorMilestoneDiscoverColorEraseAndMerge() {
        val pencil = brushId("Pencil"); val gPen = brushId("G-Pen")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
            action(obj("type" to "invoke", "command" to "fit_canvas"))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.1, .1, .12, 1.0))))
            action(obj("type" to "set_brush_size", "value" to 16))
            action(obj("type" to "select_brush", "id" to pencil))
            clickNewVectorLayer()
            assertEquals("a pen that draws paths is chosen", gPen, state().getJSONObject("brush").getLong("preset"))
            drag(Offset(.36f, .38f), Offset(.6f, .4f))
            awaitPixel("$theme-discover-ink", Offset(.48f, .39f), true, ::dark)
            screenshot("vector-$theme-discover.png")

            action(obj("type" to "select_brush", "id" to pencil))
            drag(Offset(.36f, .45f), Offset(.6f, .47f))
            screenshot("vector-$theme-wrong-brush.png")
            noticeAction("use_vector_pen")
            awaitState("$theme-use-g-pen") { it.getJSONObject("brush").getLong("preset") == gPen }
            drag(Offset(.36f, .45f), Offset(.6f, .47f))
            awaitPixel("$theme-pen-line", Offset(.48f, .46f), true, ::dark)
            action(obj("type" to "select_brush", "id" to pencil))
            drag(Offset(.36f, .52f), Offset(.6f, .54f))
            noticeAction("new_paint_layer")
            awaitState("$theme-paint-layer") { !vectorSelected() }
            action(obj("type" to "set_brush_size", "value" to 30))
            drag(Offset(.36f, .52f), Offset(.6f, .54f))
            awaitPixel("$theme-pencil-on-paint", Offset(.48f, .53f), true, ::marked)

            clickNewVectorLayer()
            figure("ellipse", "outline")
            drag(Offset(.62f, .36f), Offset(.7f, .5f))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.15, .7, .25, 1.0))))
            tool(obj("region" to obj("fill" to true, "source" to "visible")))
            tap(Offset(.66f, .43f))
            awaitPixel("$theme-filled-inside", Offset(.66f, .47f), true)
            screenshot("vector-$theme-color.png")

            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.1, .1, .12, 1.0))))
            clickNewVectorLayer()
            action(obj("type" to "invoke", "command" to "pen"))
            action(obj("type" to "select_brush", "id" to gPen))
            action(obj("type" to "set_brush_size", "value" to 16))
            for ((from, to) in listOf(Offset(.36f, .62f) to Offset(.62f, .62f), Offset(.44f, .56f) to Offset(.44f, .7f), Offset(.54f, .56f) to Offset(.54f, .7f))) drag(from, to)
            val intersection = state().getJSONArray("commands").objects().first { it.getString("id") == "erase_to_intersection" }.getString("label")
            action(obj("type" to "invoke", "command" to "eraser"))
            action(obj("type" to "set_brush_size", "value" to 16))
            fromDrawer("command", "eraser", { compose.onAllNodesWithTag("subtool-$intersection").fetchSemanticsNodes().isNotEmpty() }) {
                compose.onAllNodesWithTag("subtool-$intersection").onFirst().performClick()
                awaitState("$theme-erase-mode") { s -> s.getJSONArray("commands").objects().first { it.getString("id") == "erase_to_intersection" }.getBoolean("selected") }
            }
            drag(Offset(.49f, .6f), Offset(.49f, .64f))
            awaitPixel("$theme-between-crossings", Offset(.52f, .62f), false, ::dark)
            awaitPixel("$theme-kept-left", Offset(.39f, .62f), true, ::dark)
            awaitPixel("$theme-kept-right", Offset(.59f, .62f), true, ::dark)
            awaitState("$theme-erase-mode-kept") { s -> s.getJSONObject("tool_set").getJSONArray("subtools").objects().first { it.getString("label") == intersection }.getBoolean("selected") }
            screenshot("vector-$theme-erase-to-intersection.png")

            val vectors = state().getJSONArray("layers").objects().count { it.optString("content_icon") == "layer-path-symbolic" }
            val id = selectedRow().getLong("id")
            action(obj("type" to "layer", "action" to obj("op" to "new_below")))
            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to id, "mask" to false)))
            action(obj("type" to "invoke", "command" to "merge_down"))
            noticeAction("merge")
            awaitState("$theme-merged") { s -> s.getJSONArray("layers").objects().count { it.optString("content_icon") == "layer-path-symbolic" } == vectors - 1 }
            awaitPixel("$theme-merged-pixels", Offset(.39f, .62f), true, ::dark)
            screenshot("vector-$theme-merged.png")
            for (row in state().getJSONArray("layers").objects().filter { it.isNull("paper") }) {
                action(obj("type" to "layer", "action" to obj("op" to "select", "id" to row.getLong("id"), "mask" to false)))
                action(obj("type" to "layer", "action" to obj("op" to "delete_selected")))
            }
            action(obj("type" to "invoke", "command" to "pen"))
        }
    }

    private fun source(tool: Int) = when (tool) {
        android.view.MotionEvent.TOOL_TYPE_STYLUS, android.view.MotionEvent.TOOL_TYPE_ERASER -> android.view.InputDevice.SOURCE_STYLUS
        android.view.MotionEvent.TOOL_TYPE_MOUSE -> android.view.InputDevice.SOURCE_MOUSE
        else -> android.view.InputDevice.SOURCE_TOUCHSCREEN
    }
    private var downTime = 0L
    private var paper = floatArrayOf(0f, 0f, 1f, 1f)
    private fun onPaper(point: Offset) = Offset(paper[0] + point.x * paper[2], paper[1] + point.y * paper[3])
    private fun fitPaper() {
        action(obj("type" to "invoke", "command" to "fit_canvas"))
        var last = ""
        compose.waitUntil(10_000) { camera().let { next -> (next == last).also { last = next; android.os.SystemClock.sleep(100) } } }
        val camera = state().getJSONObject("camera")
        val tab = state().getJSONArray("tabs").objects().first { it.optBoolean("active") }
        val zoom = camera.getDouble("zoom").toFloat()
        val (tx, ty) = camera.getJSONArray("translation").let { it.getDouble(0).toFloat() to it.getDouble(1).toFloat() }
        val (vw, vh) = camera.getJSONArray("viewport").let { it.getInt(0).toFloat() to it.getInt(1).toFloat() }
        paper = floatArrayOf(tx / vw, ty / vh, tab.getInt("width") * zoom / vw, tab.getInt("height") * zoom / vh)
    }
    private fun motion(action: Int, pointers: List<Pair<Int, Offset>>, buttons: Int = 0) {
        instrumentation.runOnMainSync {
            val canvas = canvasView()
            val masked = action and android.view.MotionEvent.ACTION_MASK
            if (masked == android.view.MotionEvent.ACTION_DOWN || masked == android.view.MotionEvent.ACTION_HOVER_ENTER) downTime = android.os.SystemClock.uptimeMillis()
            val hover = masked in listOf(android.view.MotionEvent.ACTION_HOVER_ENTER, android.view.MotionEvent.ACTION_HOVER_MOVE, android.view.MotionEvent.ACTION_HOVER_EXIT)
            val properties = pointers.mapIndexed { i, (tool, _) -> android.view.MotionEvent.PointerProperties().apply { id = i; toolType = tool } }.toTypedArray()
            val coords = pointers.map { (_, point) -> android.view.MotionEvent.PointerCoords().apply { x = onPaper(point).x * canvas.width; y = onPaper(point).y * canvas.height; pressure = if (hover || masked == android.view.MotionEvent.ACTION_UP) 0f else .8f; size = .05f } }.toTypedArray()
            val sources = pointers.map { source(it.first) }.reduce { a, b -> a or b }
            val event = android.view.MotionEvent.obtain(downTime, android.os.SystemClock.uptimeMillis(), action, pointers.size, properties, coords, 0, buttons, 1f, 1f, 1, 0, sources, 0)
            try { if (hover) canvas.dispatchGenericMotionEvent(event) else canvas.dispatchTouchEvent(event) } finally { event.recycle() }
        }
        android.os.SystemClock.sleep(10)
    }
    private fun stroke(tool: Int, from: Offset, to: Offset, steps: Int = 24, between: Pair<Int, () -> Unit>? = null) {
        val buttons = if (tool == android.view.MotionEvent.TOOL_TYPE_MOUSE) android.view.MotionEvent.BUTTON_PRIMARY else 0
        motion(android.view.MotionEvent.ACTION_DOWN, listOf(tool to from), buttons)
        for (i in 1..steps) {
            motion(android.view.MotionEvent.ACTION_MOVE, listOf(tool to from + (to - from) * (i / steps.toFloat())), buttons)
            if (between?.first == i) between.second()
        }
        motion(android.view.MotionEvent.ACTION_UP, listOf(tool to to))
        compose.waitForIdle()
    }
    private fun screenPixels(point: Offset, radius: Int, inspect: (List<Int>) -> Unit) {
        var origin = IntArray(2); var size = IntArray(2)
        instrumentation.runOnMainSync { val canvas = canvasView(); canvas.getLocationOnScreen(origin); size = intArrayOf(canvas.width, canvas.height) }
        val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
        val readable = bitmap.copy(Bitmap.Config.ARGB_8888, false)
        val at = onPaper(point)
        val cx = origin[0] + (at.x * size[0]).toInt(); val cy = origin[1] + (at.y * size[1]).toInt()
        val colors = mutableListOf<Int>()
        for (y in (cy - radius).coerceAtLeast(0)..(cy + radius).coerceAtMost(readable.height - 1)) for (x in (cx - radius).coerceAtLeast(0)..(cx + radius).coerceAtMost(readable.width - 1)) colors.add(readable.getPixel(x, y))
        readable.recycle(); bitmap.recycle(); inspect(colors)
    }
    private fun ink(point: Offset): Boolean {
        var found = false
        screenPixels(point, 5) { colors -> found = colors.any { minOf(it shr 16 and 255, it shr 8 and 255, it and 255) < 110 } }
        return found
    }
    private fun awaitInk(name: String, point: Offset, wanted: Boolean) {
        motion(android.view.MotionEvent.ACTION_HOVER_EXIT, listOf(stylus to Offset(.99f, .99f)))
        try { compose.waitUntil(20_000) { ink(point) == wanted || host.failure != null } }
        catch (e: Throwable) { screenshot("vector-edges-$name-failure.png"); throw AssertionError("$name: wanted ink=$wanted at $point", e) }
        assertNull(host.failure)
    }
    private fun refusalCursor(tool: Int, point: Offset): Boolean {
        val hover = point + Offset(.002f, .002f)
        motion(android.view.MotionEvent.ACTION_HOVER_ENTER, listOf(tool to point))
        motion(android.view.MotionEvent.ACTION_HOVER_MOVE, listOf(tool to hover))
        compose.waitForIdle(); android.os.SystemClock.sleep(250)
        var red = 0
        screenPixels(hover, 24) { colors -> red = colors.count { (it shr 16 and 255) > 170 && (it shr 8 and 255) < 90 && (it and 255) < 90 } }
        motion(android.view.MotionEvent.ACTION_HOVER_EXIT, listOf(tool to hover))
        return red > 4
    }
    private fun camera() = state().getJSONObject("camera").toString()
    private fun key(code: Int, meta: Int = 0) {
        for (phase in listOf(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.ACTION_UP)) {
            val now = android.os.SystemClock.uptimeMillis()
            instrumentation.sendKeySync(android.view.KeyEvent(now, now, phase, code, 0, meta, -1, 0, 0, android.view.InputDevice.SOURCE_KEYBOARD))
        }
        compose.waitForIdle()
    }
    private val control = android.view.KeyEvent.META_CTRL_ON or android.view.KeyEvent.META_CTRL_LEFT_ON
    private fun toolName() = state().getJSONObject("layer_tools").opt("tool").toString()
    private fun enabled(id: String) = state().getJSONArray("commands").objects().first { it.getString("id") == id }.getBoolean("enabled")
    private fun vectorCount() = state().getJSONArray("layers").objects().count { it.optString("content_icon") == "layer-path-symbolic" }
    private fun freshVector(theme: String) {
        action(obj("type" to "set_theme", "theme" to theme))
        for (row in state().getJSONArray("layers").objects().filter { it.isNull("paper") }) {
            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to row.getLong("id"), "mask" to false)))
            action(obj("type" to "layer", "action" to obj("op" to "delete_selected")))
        }
        action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
        action(obj("type" to "layer", "action" to obj("op" to "new_vector")))
        fitPaper()
        action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.1, .1, .12, 1.0))))
        action(obj("type" to "invoke", "command" to "pen"))
        action(obj("type" to "select_brush", "id" to brushId("G-Pen")))
        action(obj("type" to "set_brush_size", "value" to 24))
    }
    private val stylus = android.view.MotionEvent.TOOL_TYPE_STYLUS
    private val eraser = android.view.MotionEvent.TOOL_TYPE_ERASER
    private val mouse = android.view.MotionEvent.TOOL_TYPE_MOUSE
    private val finger = android.view.MotionEvent.TOOL_TYPE_FINGER

    @Test fun vectorEdgeInputCursorsGesturesAndBackground() {
        val findings = mutableListOf<String>()
        val rows = listOf(.35f, .45f, .55f)
        for (theme in listOf("light", "dark")) {
            freshVector(theme)
            stroke(stylus, Offset(.3f, rows[0]), Offset(.7f, rows[0]))
            awaitInk("$theme-stylus", Offset(.5f, rows[0]), true)
            stroke(mouse, Offset(.3f, rows[1]), Offset(.7f, rows[1]))
            awaitInk("$theme-mouse", Offset(.5f, rows[1]), true)
            val view = camera()
            stroke(finger, Offset(.3f, rows[2]), Offset(.7f, rows[2]), 12)
            awaitInk("$theme-finger-draws-nothing", Offset(.5f, rows[2]), false)
            if (camera() != view) findings.add("$theme: one finger moved the view")
            stroke(stylus, Offset(.3f, rows[2]), Offset(.7f, rows[2]))
            screenshot("vector-edges-$theme-input.png")

            action(obj("type" to "set_brush_size", "value" to 80))
            stroke(eraser, Offset(.5f, .28f), Offset(.5f, .62f))
            for (row in rows) awaitInk("$theme-eraser-end-$row", Offset(.5f, row), false)
            awaitInk("$theme-eraser-end-keeps", Offset(.36f, rows[0]), true)
            if (state().getJSONObject("brush").getLong("preset") != brushId("G-Pen")) findings.add("$theme: the eraser end left another brush chosen")
            screenshot("vector-edges-$theme-eraser-end.png")
            action(obj("type" to "invoke", "command" to "undo"))
            awaitInk("$theme-eraser-end-undone", Offset(.5f, rows[0]), true)
            action(obj("type" to "set_brush_size", "value" to 24))

            val empty = Offset(.8f, .8f)
            val pencil = brushId("Pencil")
            for ((name, setup, refused) in listOf<Triple<String, () -> Unit, Boolean>>(
                Triple("G-Pen", { action(obj("type" to "select_brush", "id" to brushId("G-Pen"))) }, false),
                Triple("Pencil", { action(obj("type" to "select_brush", "id" to pencil)) }, true),
                Triple("Eraser", { action(obj("type" to "invoke", "command" to "eraser")) }, false),
                Triple("Figure", { figure("star", "both") }, false),
                Triple("Fill", { tool(obj("region" to obj("fill" to true, "source" to "visible"))) }, false),
                Triple("Gradient", { tool(obj("gradient" to obj("shape" to "Linear"))) }, true),
                Triple("Move", { action(obj("type" to "invoke", "command" to "move")) }, false),
                Triple("Edit Points", { action(obj("type" to "invoke", "command" to "edit_points")) }, false),
            )) {
                setup()
                for ((device, tool) in listOf("stylus" to stylus, "mouse" to mouse)) {
                    val seen = refusalCursor(tool, empty)
                    if (seen != refused) findings.add("$theme $name by $device: refusal cursor $seen, expected $refused")
                }
                if (name == "Gradient") screenshot("vector-edges-$theme-cursor-refused.png")
            }

            action(obj("type" to "invoke", "command" to "move"))
            for ((index, pair) in listOf("stylus" to stylus, "mouse" to mouse, "finger" to finger).withIndex()) {
                val start = Offset(.4f, rows[index]); val end = start + Offset(0f, .04f)
                stroke(pair.second, start, end, 8)
                android.os.SystemClock.sleep(400)
                val moved = !ink(start) && ink(end)
                if (!moved) findings.add("$theme Move by ${pair.first}: the path did not follow the drag")
                else { action(obj("type" to "invoke", "command" to "undo")); awaitInk("$theme-undo-${pair.first}", start, true) }
            }

            action(obj("type" to "invoke", "command" to "pen"))
            action(obj("type" to "select_brush", "id" to brushId("G-Pen")))
            val before = camera()
            val from = Offset(.3f, .7f); val to = Offset(.7f, .7f)
            motion(android.view.MotionEvent.ACTION_DOWN, listOf(stylus to from))
            for (i in 1..24) {
                val pen = stylus to from + (to - from) * (i / 24f)
                when {
                    i < 8 || i > 16 -> motion(android.view.MotionEvent.ACTION_MOVE, listOf(pen))
                    i == 8 -> {
                        motion(android.view.MotionEvent.ACTION_POINTER_DOWN or (1 shl android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, finger to Offset(.8f, .2f)))
                        motion(android.view.MotionEvent.ACTION_POINTER_DOWN or (2 shl android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, finger to Offset(.8f, .2f), finger to Offset(.9f, .3f)))
                    }
                    i < 16 -> { val k = (i - 8) * .02f; motion(android.view.MotionEvent.ACTION_MOVE, listOf(pen, finger to Offset(.8f - k, .2f + k), finger to Offset(.9f + k, .3f + k))) }
                    else -> {
                        motion(android.view.MotionEvent.ACTION_POINTER_UP or (2 shl android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, finger to Offset(.66f, .36f), finger to Offset(1.04f, .44f)))
                        motion(android.view.MotionEvent.ACTION_POINTER_UP or (1 shl android.view.MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, finger to Offset(.66f, .36f)))
                    }
                }
            }
            motion(android.view.MotionEvent.ACTION_UP, listOf(stylus to to))
            compose.waitForIdle()
            if (camera() != before) { findings.add("$theme: two fingers moved the view while the stylus drew"); fitPaper() }
            awaitInk("$theme-line-under-fingers", Offset(.65f, .7f), true)
            screenshot("vector-edges-$theme-two-fingers.png")

            val scenario = compose.activityRule.scenario
            motion(android.view.MotionEvent.ACTION_DOWN, listOf(stylus to Offset(.3f, .8f)))
            for (i in 1..8) motion(android.view.MotionEvent.ACTION_MOVE, listOf(stylus to Offset(.3f + i * .02f, .8f)))
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
            android.os.SystemClock.sleep(600)
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
            host.awaitReady(compose = compose)
            for (i in 9..16) motion(android.view.MotionEvent.ACTION_MOVE, listOf(stylus to Offset(.3f + i * .02f, .8f)))
            motion(android.view.MotionEvent.ACTION_UP, listOf(stylus to Offset(.62f, .8f)))
            compose.waitForIdle(); android.os.SystemClock.sleep(500)
            assertNull("the app keeps working after the background trip", host.failure)
            val kept = ink(Offset(.4f, .8f))
            screenshot("vector-edges-$theme-background-$kept.png")
            stroke(stylus, Offset(.3f, .88f), Offset(.7f, .88f))
            awaitInk("$theme-after-background", Offset(.5f, .88f), true)
        }
        assertTrue("findings:\n${findings.joinToString("\n")}", findings.isEmpty())
    }

    private fun fits(tagOrLabel: String, byTag: Boolean, place: String, findings: MutableList<String>, within: String? = null) {
        val window = compose.onRoot().fetchSemanticsNode().boundsInWindow
        val container = within?.let { tag -> compose.onAllNodesWithTag(tag, useUnmergedTree = true).fetchSemanticsNodes().firstOrNull { it.boundsInWindow.width > 0f }?.boundsInWindow }
        val nodes = (if (byTag) compose.onAllNodesWithTag(tagOrLabel, useUnmergedTree = true) else compose.onAllNodesWithContentDescription(tagOrLabel, useUnmergedTree = true)).fetchSemanticsNodes().filter { it.boundsInWindow.width > 0f }
        if (nodes.isEmpty()) findings.add("$place: nothing shows $tagOrLabel")
        for (node in nodes) for ((bounds, name) in listOfNotNull(window to "the window", container?.let { it to within })) {
            val b = node.boundsInWindow
            if (b.left < bounds.left - 1 || b.top < bounds.top - 1 || b.right > bounds.right + 1 || b.bottom > bounds.bottom + 1) findings.add("$place: $tagOrLabel at $b leaves $name at $bounds")
        }
    }
    private fun noticeShape(place: String, findings: MutableList<String>) {
        val dp = compose.activity.resources.displayMetrics.density
        fun box(tag: String) = compose.onAllNodesWithTag(tag, useUnmergedTree = true).fetchSemanticsNodes().firstOrNull()?.boundsInWindow
        val bubble = box("canvas-notice") ?: run { findings.add("$place: no notice shows"); return }
        val text = box("canvas-notice-text")!!
        val actions = state().getJSONObject("notice").getJSONArray("actions").objects().mapNotNull { box("canvas-notice-action-${it.getString("id")}") }
        val beside = actions.all { it.top < text.bottom }
        if (beside && kotlin.math.abs(bubble.height - 36 * dp) > 1) findings.add("$place: a one-row notice is ${bubble.height / dp} dp tall, not the bar's 36")
        for (action in actions) if (kotlin.math.abs(action.height - 26 * dp) > 1) findings.add("$place: an action is ${action.height / dp} dp tall, not the bar's 26")
        if (!beside && actions.any { it.top < text.bottom }) findings.add("$place: stacked actions overlap the text")
        if (!beside && kotlin.math.abs(bubble.right - actions.maxOf { it.right } - 5 * dp) > 1) findings.add("$place: stacked actions end ${(bubble.right - actions.maxOf { it.right }) / dp} dp from the right edge")
    }
    private fun tooltipSays(text: String) = findTag("hover-tooltip") != null && findNode(hasLabel(text)) != null
    private fun reveal(tag: String) {
        runCatching { compose.onAllNodesWithTag(tag, useUnmergedTree = true).onFirst().performScrollTo() }
        compose.waitForIdle()
    }
    private fun german(language: String) {
        if (host.languageTag == language) return
        val tags = host.bootstrap!!.array("shipped_tags").let { a -> (0 until a.length()).map { a.getString(it) } }
        host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to tags.indexOf(language) + 1)))
        host.awaitMain("language $language", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == language }
        compose.waitForIdle()
    }

    private fun switchWorkspace(id: String) {
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:$id")) }
        host.awaitMain("workspace $id", 30_000, { "${host.workspaceManager}" }, compose) { host.workspaceManager?.optString("id") == "builtin:workspace:$id" && host.workspaceManager?.optBoolean("busy") == false }
        compose.waitForIdle()
    }
    @Test fun vectorEdgeKeysTouchPointsNoticesAndGermanLabels() {
        val findings = mutableListOf<String>()
        val pencil = brushId("Pencil")
        val rows = listOf(.35f, .45f, .55f)
        try { for (theme in listOf("light", "dark")) {
            freshVector(theme)
            for (row in rows) stroke(stylus, Offset(.3f, row), Offset(.7f, row))
            awaitInk("$theme-three-lines", Offset(.5f, rows[2]), true)
            instrumentation.runOnMainSync { canvasView().requestFocus() }
            key(android.view.KeyEvent.KEYCODE_A, control)
            awaitState("$theme-select-all") { enabled("deselect") && toolName() == "move" }
            key(android.view.KeyEvent.KEYCODE_ESCAPE)
            awaitState("$theme-escape-clears") { !enabled("deselect") }
            tap(onPaper(Offset(.5f, rows[0])))
            awaitState("$theme-tap-selects") { enabled("deselect") }
            key(android.view.KeyEvent.KEYCODE_ENTER)
            awaitState("$theme-enter-edit-points") { toolName() == "edit_points" }
            key(android.view.KeyEvent.KEYCODE_ESCAPE)
            awaitState("$theme-escape-to-move") { toolName() == "move" && enabled("deselect") }
            key(android.view.KeyEvent.KEYCODE_FORWARD_DEL)
            awaitInk("$theme-delete", Offset(.5f, rows[0]), false)
            key(android.view.KeyEvent.KEYCODE_Z, control)
            awaitInk("$theme-ctrl-z-restores", Offset(.5f, rows[0]), true)

            stroke(stylus, Offset(.5f, rows[1]), Offset(.5f, rows[1] + .06f), 8, 4 to { key(android.view.KeyEvent.KEYCODE_ESCAPE) })
            awaitInk("$theme-escape-mid-drag-stays", Offset(.5f, rows[1]), true)
            awaitInk("$theme-escape-mid-drag-no-preview", Offset(.5f, rows[1] + .06f), false)
            android.os.SystemClock.sleep(500)
            stroke(stylus, Offset(.5f, rows[1]), Offset(.5f, rows[1] + .06f), 8, 4 to { key(android.view.KeyEvent.KEYCODE_Z, control) })
            android.os.SystemClock.sleep(500)
            motion(android.view.MotionEvent.ACTION_HOVER_EXIT, listOf(stylus to Offset(.99f, .99f)))
            val landed = ink(Offset(.5f, rows[1] + .06f)); val kept = rows.all { ink(Offset(.4f, it)) || it == rows[1] }
            if (!landed || !kept) findings.add("$theme: Ctrl+Z during a path drag: the drag landed $landed, the other lines stayed $kept")
            screenshot("vector-edges-$theme-undo-mid-drag.png")

            freshVector(theme)
            for (row in rows) stroke(stylus, Offset(.3f, row), Offset(.7f, row))
            stroke(stylus, Offset(.3f, .7f), Offset(.7f, .7f), 24, 12 to { key(android.view.KeyEvent.KEYCODE_Z, control) })
            android.os.SystemClock.sleep(500)
            motion(android.view.MotionEvent.ACTION_HOVER_EXIT, listOf(stylus to Offset(.99f, .99f)))
            if (!ink(Offset(.65f, .7f)) || !ink(Offset(.5f, rows[2]))) findings.add("$theme: Ctrl+Z during a G-Pen stroke: stroke kept ${ink(Offset(.65f, .7f))}, earlier line kept ${ink(Offset(.5f, rows[2]))}")
            screenshot("vector-edges-$theme-undo-mid-stroke.png")
            freshVector(theme)
            for (row in rows) stroke(stylus, Offset(.3f, row), Offset(.7f, row))
            awaitInk("$theme-lines-again", Offset(.5f, rows[2]), true)

            action(obj("type" to "invoke", "command" to "move"))
            tap(onPaper(Offset(.5f, rows[2])))
            action(obj("type" to "invoke", "command" to "edit_points"))
            stroke(finger, Offset(.3f, rows[2]), Offset(.3f, rows[2] + .08f), 8)
            android.os.SystemClock.sleep(400)
            if (!ink(Offset(.3f, rows[2] + .08f))) findings.add("$theme: Edit Points by touch did not move the anchor")
            screenshot("vector-edges-$theme-touch-points.png")

            action(obj("type" to "invoke", "command" to "pen"))
            action(obj("type" to "select_brush", "id" to pencil))
            stroke(mouse, Offset(.3f, .8f), Offset(.6f, .8f), 6)
            awaitState("$theme-notice") { s -> s.optJSONObject("notice")?.getJSONArray("actions")?.length() == 3 }
            val notice = state().getJSONObject("notice").getLong("id")
            compose.onNodeWithTag("canvas-notice").assertIsDisplayed()
            action(obj("type" to "set_theme", "theme" to if (theme == "light") "dark" else "light"))
            if (state().optJSONObject("notice")?.getLong("id") != notice || compose.onAllNodesWithTag("canvas-notice").fetchSemanticsNodes().isEmpty()) findings.add("$theme: switching the theme hid the notice")
            screenshot("vector-edges-$theme-notice-switched.png")
            action(obj("type" to "set_theme", "theme" to theme))
            val actionNode = compose.onNodeWithTag("canvas-notice-action-new_paint_layer").fetchSemanticsNode()
            if (!actionNode.config.contains(androidx.compose.ui.semantics.SemanticsActions.RequestFocus)) findings.add("$theme: notice actions take no keyboard focus")
            val bubbleName = compose.onNodeWithTag("canvas-notice").fetchSemanticsNode().config.getOrElseNullable(androidx.compose.ui.semantics.SemanticsProperties.PaneTitle) { null }
            if (bubbleName != state().getJSONObject("notice").getString("name")) findings.add("$theme: the notice is named $bubbleName, not ${state().getJSONObject("notice").getString("name")}")
            val paint = state().getJSONArray("layers").objects().first { !it.getBoolean("selected") && it.isNull("paper") && it.optString("content_icon") != "layer-path-symbolic" }.getLong("id")
            val vector = selectedLayer()
            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to paint, "mask" to false)))
            if (compose.onAllNodesWithTag("canvas-notice-action-new_paint_layer").fetchSemanticsNodes().isNotEmpty()) {
                val count = state().getJSONArray("layers").length()
                compose.onNodeWithTag("canvas-notice-action-new_paint_layer").performClick()
                compose.waitForIdle(); android.os.SystemClock.sleep(400)
                if (state().getJSONArray("layers").length() != count) findings.add("$theme: a stale notice action added a layer")
                val stale = host.actionError ?: host.hostError ?: state().optJSONObject("notice")?.optString("text")
                if (stale.isNullOrEmpty()) findings.add("$theme: a stale notice action showed nothing")
                screenshot("vector-edges-$theme-notice-stale.png")
                compose.waitUntil(15_000) { host.actionError == null }
            }
            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to vector, "mask" to false)))

            german("de")
            for ((orientation, turn) in listOf<Pair<String, () -> Unit>>("landscape" to { device.landscape(compose.activityRule.scenario) }, "portrait" to { device.portrait(compose.activityRule.scenario) })) {
                turn(); host.awaitReady(compose = compose); compose.waitForIdle()
                for (workspace in listOf("illustrator", "painter")) {
                    switchWorkspace(workspace)
                    fitPaper()
                    val place = "$theme $orientation $workspace"
                    if (workspace == "illustrator") {
                        val layers = host.catalog.getJSONObject("native_copy").getJSONObject("layers")
                        for (label in listOf("new_vector_layer", "delete_selected", "actions")) fits(layers.getString(label), false, "$place footer", findings, "layer-inspector")
                        action(obj("type" to "invoke", "command" to "eraser"))
                        for (id in listOf("erase_touched_part", "erase_whole_paths", "erase_to_intersection")) {
                            val label = state().getJSONArray("commands").objects().first { it.getString("id") == id }.getString("label")
                            reveal("subtool-$label")
                            fits("subtool-$label", true, "$place eraser mode", findings)
                        }
                        screenshot("vector-edges-$theme-$orientation-$workspace-de-panels.png")
                    }
                    action(obj("type" to "select_brush", "id" to pencil))
                    stroke(mouse, Offset(.3f, .5f), Offset(.6f, .5f), 6)
                    awaitState("$place-de-notice") { s -> s.optJSONObject("notice")?.getJSONArray("actions")?.length() == 3 }
                    fits("canvas-notice", true, "$place notice", findings)
                    for (id in listOf("use_vector_pen", "new_paint_layer", "rasterize_layer")) fits("canvas-notice-action-$id", true, "$place notice", findings, "canvas-notice")
                    noticeShape("$place", findings)
                    screenshot("vector-edges-$theme-$orientation-$workspace-de-notice.png")
                    compose.waitUntil(15_000) { state().isNull("notice") }
                }
            }
            german("en")
        } } finally { runCatching { german("en") } }
        assertTrue("findings:\n${findings.joinToString("\n")}", findings.isEmpty())
    }

    private fun layerMenu(id: Long): JSONObject {
        val done = CountDownLatch(1); var menu: JSONObject? = null
        compose.runOnIdle { host.query(obj("type" to "layer_menu", "id" to id, "mask" to false)) { menu = it as? JSONObject; done.countDown() } }
        assertTrue(done.await(15, TimeUnit.SECONDS))
        return menu!!
    }
    private fun menuPath(sections: org.json.JSONArray, wanted: (JSONObject) -> Boolean): List<String>? {
        for (section in 0 until sections.length()) for (item in sections.getJSONArray(section).objects()) {
            if (wanted(item)) return listOf(item.getString("label"))
            menuPath(item.array("sections"), wanted)?.let { return listOf(item.getString("label")) + it }
        }
        return null
    }
    private fun labels(sections: org.json.JSONArray): List<String> = (0 until sections.length()).flatMap { i -> sections.getJSONArray(i).objects().flatMap { listOf(it.getString("label")) + labels(it.array("sections")) } }
    private fun files() = compose.activity.cacheDir
    private fun tapTag(tag: String) {
        val center = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInWindow.center
        instrumentation.runOnMainSync {
            val root = compose.activity.window.decorView
            val now = android.os.SystemClock.uptimeMillis()
            for (phase in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
                val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 0; toolType = finger })
                val coords = arrayOf(android.view.MotionEvent.PointerCoords().apply { x = center.x; y = center.y; pressure = 1f; size = .05f })
                val event = android.view.MotionEvent.obtain(now, now + if (phase == android.view.MotionEvent.ACTION_UP) 60 else 0, phase, 1, properties, coords, 0, 0, 1f, 1f, 1, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
                try { root.dispatchTouchEvent(event) } finally { event.recycle() }
            }
        }
        compose.waitForIdle()
    }

    @Test fun vectorSketchExplainsDimmedToolsAndNoticesKeepTheBarShape() {
        val findings = mutableListOf<String>()
        val rough = "Rough G-Pen"
        try {
            switchWorkspace("painter")
            for (theme in listOf("light", "dark")) {
                freshVector(theme)
                val row = state().getJSONObject("tool_set").getJSONArray("subtools").objects().first { it.getString("label") == rough }
                val reason = row.optString("disabled_reason")
                if (!row.getBoolean("enabled") || reason.isEmpty()) findings.add("$theme: $rough is not dimmed and choosable: $row")
                fromDrawer("command", "drawing_brush", { compose.onAllNodesWithTag("subtool-$rough").fetchSemanticsNodes().isNotEmpty() }) {
                    reveal("subtool-$rough")
                    compose.onNodeWithTag("subtool-$rough").performMouseInput { moveTo(center) }
                    try { compose.waitUntil(5_000) { tooltipSays(reason) } } catch (_: Throwable) { findings.add("$theme: hovering $rough does not say $reason") }
                    screenshot("vector-sketch-$theme-dimmed-pen.png")
                    compose.onNodeWithTag("subtool-$rough").performMouseInput { exit() }
                    tapTag("subtool-$rough")
                    awaitState("$theme-dimmed-pen-explains") { s -> s.getJSONObject("brush").getLong("preset") == brushId(rough)
                        && s.optJSONObject("notice")?.getJSONArray("actions")?.objects()?.firstOrNull()?.let { it.getString("id") == "use_vector_pen" && it.getBoolean("accent") } == true }
                    compose.waitForIdle()
                    noticeShape("$theme dimmed $rough", findings)
                    screenshot("vector-sketch-$theme-dimmed-pen-notice.png")
                }
                noticeAction("use_vector_pen")
                awaitState("$theme-use-g-pen") { it.getJSONObject("brush").getLong("preset") == brushId("G-Pen") }

                if (theme == "dark") {
                    device.portrait(compose.activityRule.scenario); host.awaitReady(compose = compose); compose.waitForIdle(); fitPaper()
                    action(obj("type" to "layer", "action" to obj("op" to "rename", "id" to selectedLayer(), "name" to "Inked outlines for the second chapter cover, final pass")))
                }
                val vector = selectedLayer()
                action(obj("type" to "layer", "action" to obj("op" to "lock", "id" to vector, "value" to true)))
                val locked = state().getJSONArray("commands").objects().first { it.getString("id") == "scale_rotate" }.optString("disabled_reason")
                val scaleRotate = headerTool("command", "scale_rotate").replace("header-item-", "header-control-")
                tapTag(scaleRotate)
                try { compose.waitUntil(5_000) { tooltipSays(locked) } } catch (_: Throwable) { findings.add("$theme: tapping greyed Scale/rotate does not say \"$locked\"") }
                screenshot("vector-sketch-$theme-locked-scale-rotate.png")
                action(obj("type" to "layer", "action" to obj("op" to "lock", "id" to vector, "value" to false)))
            }
        } finally { runCatching { action(obj("type" to "set_theme", "theme" to "light")) } }
        assertTrue("findings:\n${findings.joinToString("\n")}", findings.isEmpty())
    }

    @Test fun vectorEdgeRowsFilesAndRecreate() {
        val findings = mutableListOf<String>()
        val saved = java.util.concurrent.atomic.AtomicReference<java.io.File?>()
        val monitor = object : android.app.Instrumentation.ActivityMonitor() {
            override fun onStartActivity(intent: android.content.Intent): android.app.Instrumentation.ActivityResult? {
                if (intent.action != android.content.Intent.ACTION_CREATE_DOCUMENT && intent.action != android.content.Intent.ACTION_OPEN_DOCUMENT) return null
                val file = saved.get() ?: return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_CANCELED, null)
                return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK, android.content.Intent().setData(android.net.Uri.fromFile(file))
                    .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION))
            }
        }
        instrumentation.addMonitor(monitor)
        val jobs = DocumentController.nativeFileJobsForTest
        try { for (theme in listOf("light", "dark")) {
            switchWorkspace("illustrator")
            freshVector(theme)
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.85, .35, .1, 1.0))))
            stroke(stylus, Offset(.3f, .35f), Offset(.7f, .35f))
            figure("rectangle", "both"); stroke(stylus, Offset(.3f, .45f), Offset(.45f, .6f), 6)
            figure("star", "outline"); stroke(stylus, Offset(.55f, .45f), Offset(.7f, .65f), 6)
            val layer = selectedLayer()
            action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to "path_width", "value" to obj("kind" to "number", "value" to 12))))
            awaitInk("$theme-files-line", Offset(.5f, .35f), true)

            compose.onNodeWithTag("layer-row-$layer").performTouchInput { longClick(center) }
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isNotEmpty() }
            val menu = layerMenu(layer)
            for (label in labels(menu.getJSONArray("sections"))) if (compose.onAllNodes(hasText(label) and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isEmpty() && menuPath(menu.getJSONArray("sections")) { it.getString("label") == label }!!.size == 1) findings.add("$theme: the vector row's menu lacks $label")
            val rasterize = state().getJSONArray("commands").objects().first { it.getString("id") == "rasterize_layer" }.getString("label")
            if (rasterize !in labels(menu.getJSONArray("sections"))) findings.add("$theme: the vector row's menu has no $rasterize")
            screenshot("vector-edges-$theme-row-menu.png")
            val duplicate = menuPath(menu.getJSONArray("sections")) { it.optJSONObject("action")?.toString()?.contains("\"duplicate") == true }!!
            for (step in duplicate) { compose.onNode(hasText(step) and hasAnyAncestor(isPopup())).performClick(); compose.waitForIdle() }
            awaitState("$theme-duplicate") { vectorCount() == 2 }
            val copy = selectedLayer()

            action(obj("type" to "layer", "action" to obj("op" to "begin_rename", "id" to copy)))
            compose.waitUntil(10_000) { compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("layer-row-$copy"))).fetchSemanticsNodes().isNotEmpty() }
            compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("layer-row-$copy"))).performTextReplacement("Ink")
            compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("layer-row-$copy"))).performImeAction()
            awaitState("$theme-rename") { s -> s.getJSONArray("layers").objects().any { it.getLong("id") == copy && it.getString("label") == "Ink" } }
            instrumentation.runOnMainSync { compose.activity.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(compose.activity.window.decorView.windowToken, 0) }
            compose.waitForIdle(); android.os.SystemClock.sleep(500)

            tapTag("layer-eye-$copy"); tapTag("layer-eye-$layer")
            awaitInk("$theme-both-hidden", Offset(.5f, .35f), false)
            screenshot("vector-edges-$theme-hidden.png")
            tapTag("layer-eye-$copy"); tapTag("layer-eye-$layer")
            awaitInk("$theme-shown", Offset(.5f, .35f), true)

            val before = state().getJSONArray("layers").objects().map { it.getLong("id") }
            val below = before[before.indexOf(layer) + 1]
            val target = compose.onNodeWithTag("layer-row-$below", useUnmergedTree = true).fetchSemanticsNode().boundsInWindow
            val grip = compose.onNodeWithTag("layer-grip-$copy", useUnmergedTree = true).fetchSemanticsNode().boundsInWindow.center
            val down = android.os.SystemClock.uptimeMillis()
            fun send(phase: Int, point: Offset) {
                instrumentation.runOnMainSync {
                    val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 0; toolType = finger })
                    val coords = arrayOf(android.view.MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f; size = .05f })
                    val event = android.view.MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), phase, 1, properties, coords, 0, 0, 1f, 1f, 1, 0, android.view.InputDevice.SOURCE_TOUCHSCREEN, 0)
                    try { compose.activity.window.decorView.dispatchTouchEvent(event) } finally { event.recycle() }
                }
                android.os.SystemClock.sleep(30)
            }
            send(android.view.MotionEvent.ACTION_DOWN, grip)
            for (i in 1..12) send(android.view.MotionEvent.ACTION_MOVE, grip + Offset(0f, (target.bottom - 4f - grip.y) * i / 12f))
            send(android.view.MotionEvent.ACTION_UP, Offset(grip.x, target.bottom - 4f))
            compose.waitForIdle()
            awaitState("$theme-reorder") { s -> s.getJSONArray("layers").objects().map { it.getLong("id") } != before }
            val after = state().getJSONArray("layers").objects().map { it.getLong("id") }
            if (after.indexOf(copy) < after.indexOf(layer)) findings.add("$theme: dragging the vector row did not move it below: $before -> $after")
            screenshot("vector-edges-$theme-reordered.png")
            action(obj("type" to "invoke", "command" to "undo"))
            awaitState("$theme-reorder-undone") { s -> s.getJSONArray("layers").objects().map { it.getLong("id") } == before }

            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to copy, "mask" to false)))
            action(obj("type" to "layer", "action" to obj("op" to "lock", "id" to copy, "value" to true)))
            action(obj("type" to "invoke", "command" to "pen")); action(obj("type" to "select_brush", "id" to brushId("G-Pen")))
            stroke(stylus, Offset(.3f, .72f), Offset(.7f, .72f))
            awaitState("$theme-locked-refusal") { !it.isNull("notice") }
            compose.waitUntil(15_000) { state().isNull("notice") }
            awaitInk("$theme-locked-empty", Offset(.5f, .72f), false)
            action(obj("type" to "layer", "action" to obj("op" to "lock", "id" to copy, "value" to false)))
            run {
                val delete = compose.onAllNodesWithContentDescription(host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("delete_selected"), useUnmergedTree = true).fetchSemanticsNodes().first { it.boundsInWindow.width > 0f }.boundsInWindow.center
                instrumentation.runOnMainSync {
                    val now = android.os.SystemClock.uptimeMillis()
                    for (phase in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP)) {
                        val event = android.view.MotionEvent.obtain(now, now + if (phase == android.view.MotionEvent.ACTION_UP) 60 else 0, phase, delete.x, delete.y, 0)
                        event.source = android.view.InputDevice.SOURCE_TOUCHSCREEN
                        try { compose.activity.window.decorView.dispatchTouchEvent(event) } finally { event.recycle() }
                    }
                }
                compose.waitForIdle()
            }
            awaitState("$theme-deleted") { vectorCount() == 1 }
            action(obj("type" to "invoke", "command" to "undo"))
            awaitState("$theme-delete-undone") { s -> s.getJSONArray("layers").objects().any { it.getString("label") == "Ink" } }
            action(obj("type" to "layer", "action" to obj("op" to "delete_selected")))
            action(obj("type" to "layer", "action" to obj("op" to "select", "id" to layer, "mask" to false)))

            action(obj("type" to "invoke", "command" to "eraser"))
            val whole = state().getJSONArray("commands").objects().first { it.getString("id") == "erase_whole_paths" }.getString("label")
            reveal("subtool-$whole")
            compose.onAllNodesWithTag("subtool-$whole").onFirst().performClick()
            awaitState("$theme-whole-paths") { it.getJSONObject("settings").optString("vector_eraser") == "whole_paths" }

            DocumentController.nativeFileJobsForTest = false
            val file = java.io.File(files(), "vector-edges-$theme.capy").apply { delete() }
            saved.set(file)
            action(obj("type" to "invoke", "command" to "save_document_as"))
            compose.waitUntil(60_000) { file.length() > 0 && !state().getJSONObject("document_file").getBoolean("busy") && !state().getJSONObject("document_file").getBoolean("modified") }
            val first = packageManifest(file.readBytes()).artworkRecords().toString()
            val tabs = state().getJSONArray("tabs").length()
            val copyFile = java.io.File(files(), "vector-edges-$theme-copy.capy").apply { writeBytes(file.readBytes()) }
            saved.set(copyFile)
            action(obj("type" to "invoke", "command" to "open_document"))
            try { compose.waitUntil(60_000) { state().getJSONObject("document_file").let { f -> !f.getBoolean("busy") && f.optJSONObject("location")?.optString("name") == copyFile.name } } }
            catch (e: Throwable) { screenshot("vector-edges-$theme-reopen-failure.png"); throw AssertionError("reopen: tabs ${state().getJSONArray("tabs").length()} after $tabs, file ${state().getJSONObject("document_file")}, host ${host.hostError}, action ${host.actionError}, prompt ${host.documents.packagePrompt}", e) }
            host.awaitReady(compose = compose)
            fitPaper()
            awaitInk("$theme-reopened-line", Offset(.5f, .35f), true)
            awaitInk("$theme-reopened-rectangle", Offset(.375f, .45f), true)
            if (vectorCount() != 1) findings.add("$theme: the reopened drawing shows ${vectorCount()} vector layers")
            screenshot("vector-edges-$theme-reopened.png")
            val again = java.io.File(files(), "vector-edges-$theme-again.capy").apply { delete() }
            saved.set(again)
            action(obj("type" to "invoke", "command" to "save_document_as"))
            compose.waitUntil(60_000) { again.length() > 0 && !state().getJSONObject("document_file").getBoolean("busy") && !state().getJSONObject("document_file").getBoolean("modified") }
            if (packageManifest(again.readBytes()).artworkRecords().toString() != first) findings.add("$theme: paths, styles or live shapes changed through save and reopen")
            DocumentController.nativeFileJobsForTest = jobs
            saved.set(null); file.delete(); again.delete(); copyFile.delete()

            compose.activityRule.scenario.recreate()
            host.awaitReady(compose = compose)
            if (state().getJSONObject("settings").optString("vector_eraser") != "whole_paths") findings.add("$theme: the eraser mode did not survive recreating the activity")
            action(obj("type" to "restore_saved_settings", "saved" to state().getJSONObject("settings").put("vector_eraser", "touched_part").toString()))
        } } finally { instrumentation.removeMonitor(monitor); DocumentController.nativeFileJobsForTest = jobs; runCatching { switchWorkspace("painter") } }
        assertTrue("findings:\n${findings.joinToString("\n")}", findings.isEmpty())
    }
}
