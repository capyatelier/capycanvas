package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.core.app.ActivityScenario
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.pow
import kotlin.math.roundToInt

/** Real Android windows/input on an isolated workspace, including stylus contacts. */
class AndroidCommandSearchTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var mainWindow: View
    private var previousRotation = 0
    private var autoRotate = true
    private fun main(block: () -> Unit) = instrumentation.runOnMainSync(block)
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun search() = state().objectOrNull("command_search")
    private fun labelled(text: String) = findNode(hasLabel(text))
    private fun tagged(tag: String) = findTag(tag)
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) = host.awaitMain(label, timeout, {
        capture("timeout")
        "query=${search()?.optString("query")} parameter=${search()?.objectOrNull("parameter")?.optString("id")} error=${search()?.optString("error")}"
    }, condition)
    private fun action(value: JSONObject) { main { host.dispatch(value) }; SystemClock.sleep(150) }
    private fun closeWithEscape(label: String) {
        for (attempt in 0 until 2) {
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            val until = SystemClock.uptimeMillis() + 1_000
            var closed = false
            while (!closed && SystemClock.uptimeMillis() < until) {
                main { closed = tagged("command-bar") == null }
                if (!closed) SystemClock.sleep(16)
            }
            if (closed) break
        }
        waitFor(label) { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
    }
    private fun open() {
        // Model publication precedes Compose's native dialog removal by a frame.
        waitFor("editor window focus") { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
        SystemClock.sleep(40)
        instrumentation.waitForIdleSync()
        pressKey(KeyEvent.KEYCODE_K, KeyEvent.META_CTRL_ON)
        waitFor("search focus") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
    }
    private fun query(value: String) {
        main { assertTrue(tagged("command-search")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(value))) }
        if (search()?.objectOrNull("parameter") == null) waitFor("query $value") { search()?.optString("query") == value }
    }
    private fun detail() = tagged("command-detail")?.second?.config?.getOrNull(SemanticsProperties.Text)?.joinToString("") { it.text }
    private fun capture(name: String, inspect: (Bitmap) -> Unit = {}) {
        SystemClock.sleep(220)
        screenshot("validation/command-search/$name.png", inspect)
    }
    private fun canvasSurface(view: View): View? =
        view as? CanvasSurfaceView ?: (view as? ViewGroup)?.let { group -> (0 until group.childCount).firstNotNullOfOrNull { canvasSurface(group.getChildAt(it)) } }
    private fun screenOrigin(view: View) = IntArray(2).also(view::getLocationOnScreen).let { Offset(it[0].toFloat(), it[1].toFloat()) }
    private fun row(pixels: Bitmap, y: Float, span: ClosedFloatingPointRange<Float>) =
        (span.start.roundToInt() until span.endInclusive.roundToInt()).map { x ->
            pixels.getPixel(x, y.roundToInt()).let { floatArrayOf(android.graphics.Color.red(it) / 255f, android.graphics.Color.green(it) / 255f, android.graphics.Color.blue(it) / 255f) }
        }
    private fun mean(row: List<FloatArray>) = FloatArray(3) { i -> row.map { it[i] }.average().toFloat() }
    private fun blurred(row: List<FloatArray>) = FloatArray(3) { i ->
        val linear = row.map { it[i].let { c -> if (c <= .04045f) c / 12.92f else ((c + .055f) / 1.055f).pow(2.4f) } }.average().toFloat()
        if (linear <= .0031308f) linear * 12.92f else 1.055f * linear.pow(1 / 2.4f) - .055f
    }
    private fun sharpness(row: List<FloatArray>) = row.map { .2126f * it[0] + .7152f * it[1] + .0722f * it[2] }.zipWithNext { a, b -> abs(b - a) }.average()
    private fun tap(point: Offset, tool: Int) {
        val now = SystemClock.uptimeMillis()
        for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event = motion(tool, action, point, now)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
        }
    }
    private fun tapTag(tag: String, tool: Int) {
        tapNode(tool) { tagged(tag)!! }
    }
    private fun tapNode(tool: Int, lookup: () -> Pair<ViewRootForTest, SemanticsNode>) {
        var point = Offset.Zero
        main {
            val (owner, node) = lookup()
            val origin = IntArray(2); owner.view.getLocationOnScreen(origin)
            point = node.boundsInRoot.center + Offset(origin[0].toFloat(), origin[1].toFloat())
        }
        tap(point, tool)
    }
    @Before fun ready() {
        scenario = launchCapy()
        scenario.onActivity {
            host = it.host; mainWindow = it.window.decorView
            previousRotation = mainWindow.display.rotation
            autoRotate = android.provider.Settings.System.getInt(it.contentResolver, android.provider.Settings.System.ACCELEROMETER_ROTATION, 1) != 0
        }
        action(obj("type" to "close_settings"))
    }
    @After fun cleanup() {
        instrumentation.uiAutomation.setRotation(if (autoRotate) android.app.UiAutomation.ROTATION_UNFREEZE else previousRotation)
        if (::scenario.isInitialized) scenario.close()
    }
    @Test fun nativeKeyboardTouchPenAndPerformance() {
        if (mainWindow.resources.configuration.orientation != android.content.res.Configuration.ORIENTATION_LANDSCAPE) {
            assertTrue(instrumentation.uiAutomation.setRotation(if (mainWindow.display.rotation % 2 == 0) android.app.UiAutomation.ROTATION_FREEZE_90 else android.app.UiAutomation.ROTATION_FREEZE_0))
            waitFor("landscape") { mainWindow.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_LANDSCAPE }
            scenario.onActivity { mainWindow = it.window.decorView }
        }
        repeat(3) {
            open()
            instrumentation.sendStringSync("pencil")
            pressKey(KeyEvent.KEYCODE_ENTER)
            waitFor("pencil committed") { search() == null && state().getJSONObject("brush").optString("tool") == "pencil" }
        }
        action(obj("type" to "set_theme", "theme" to "light"))
        open(); query("undo")
        assertFalse(search()!!.getJSONArray("results").getJSONObject(0).getBoolean("enabled"))
        waitFor("disabled reason footer") { detail() == "Nothing to undo" && search()?.getString("detail") == detail() }
        capture("light")
        query("brush size")
        waitFor("tool setting footer") {
            val description = search()?.getJSONArray("results")?.getJSONObject(0)?.getString("description")
            description != null && detail() == description && Regex("Current .+ · Range .+").containsMatchIn(description)
        }
        pressKey(KeyEvent.KEYCODE_ENTER)
        waitFor("parameter") {
            search()?.objectOrNull("parameter") != null &&
                tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith("Brush size") } == true
        }
        query("bad input"); pressKey(KeyEvent.KEYCODE_ENTER)
        waitFor("validation") { search()?.optString("error")?.contains("null") == false }
        waitFor("validation footer") { search()?.optString("error")?.let { it.isNotEmpty() && detail() == it } == true }
        query("24"); pressKey(KeyEvent.KEYCODE_ENTER)
        waitFor("size committed") { search() == null && state().getJSONObject("brush").optDouble("diameter") == 24.0 }
        action(obj("type" to "set_theme", "theme" to "dark"))
        open(); query("select"); pressKey(KeyEvent.KEYCODE_DPAD_DOWN)
        waitFor("selected row") { search()?.optInt("selected") == 1 }
        capture("dark")
        val retained = host.panelContent
        val timings = mutableListOf<Double>()
        repeat(40) { index ->
            val value = listOf("undo", "select", "pencil", "brush size")[index % 4]
            val done = CountDownLatch(1); val start = System.nanoTime()
            var duration = 0.0
            main {
                val owner = tagged("command-search")!!.first.view
                val listener = object : ViewTreeObserver.OnDrawListener {
                    override fun onDraw() {
                        if (search()?.optString("query") != value || duration != 0.0) return
                        duration = (System.nanoTime() - start) / 1_000_000.0
                        owner.post { owner.viewTreeObserver.removeOnDrawListener(this); done.countDown() }
                    }
                }
                owner.viewTreeObserver.addOnDrawListener(listener)
                tagged("command-search")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(value))
            }
            assertTrue("query draws", done.await(5, TimeUnit.SECONDS)); if (index >= 20) timings.add(duration)
            main { assertSame("Search retains panel content", retained, host.panelContent) }
        }
        android.util.Log.i("CommandSearchTest", "query_to_android_draw_p95_ms=${timings.sorted()[18]} samples=$timings")
        pressKey(KeyEvent.KEYCODE_ESCAPE); waitFor("closed") { search() == null }
        for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            open(); query("eraser"); tapTag("command-result-0", tool)
            waitFor("contact executes") { search() == null && state().getJSONObject("brush").getString("tool") == "eraser" }
        }
        open()
        val before = state().getJSONObject("document_file").getLong("revision")
        tap(Offset(16f, 400f), MotionEvent.TOOL_TYPE_FINGER)
        waitFor("outside dismissal") { search() == null }
        assertEquals(before, state().getJSONObject("document_file").getLong("revision"))
        waitFor("editor focus") { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
        tapTag("application-menu-edit", MotionEvent.TOOL_TYPE_FINGER)
        waitFor("visible search menu item") { labelled("Search Commands…") != null }
        tapNode(MotionEvent.TOOL_TYPE_FINGER) { labelled("Search Commands…")!! }
        waitFor("menu opens search") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
        query("brush size"); pressKey(KeyEvent.KEYCODE_ENTER)
        waitFor("parameter step") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith("Brush size") } == true }
        pressKey(KeyEvent.KEYCODE_ESCAPE)
        waitFor("back to query") { search()?.objectOrNull("parameter") == null && search()?.optString("query") == "brush size" }
        closeWithEscape("menu search closed")
        pressKey(KeyEvent.KEYCODE_P, KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON)
        waitFor("primary opener") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
        closeWithEscape("primary opener closed")
        // Large-screen Android can ignore requestedOrientation; rotate the
        // native test display and restore the device's rotation policy afterward.
        assertTrue(instrumentation.uiAutomation.setRotation(android.app.UiAutomation.ROTATION_FREEZE_0))
        waitFor("portrait") { mainWindow.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT }
        scenario.onActivity { mainWindow = it.window.decorView }
        open(); query("select"); capture("portrait")
        pressKey(KeyEvent.KEYCODE_ESCAPE)
    }
    @Test fun heldAltSamplesColorAndReturnsToTheBrush() {
        action(obj("type" to "invoke", "command" to "brush"))
        waitFor("brush") { state().getJSONObject("layer_tools").getString("tool") == "paint" }
        val preset = state().getJSONObject("brush").getInt("preset")
        val now = SystemClock.uptimeMillis()
        val alt = { action: Int, meta: Int -> instrumentation.sendKeySync(KeyEvent(now, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_ALT_LEFT, 0, meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD)) }
        alt(KeyEvent.ACTION_DOWN, KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON)
        waitFor("sampling while Alt is held") { state().getJSONObject("layer_tools").getString("tool") == "pick_visible" }
        alt(KeyEvent.ACTION_UP, 0)
        waitFor("brush restored on release") {
            state().getJSONObject("layer_tools").getString("tool") == "paint" && state().getJSONObject("brush").getInt("preset") == preset
        }
    }

    @Test fun fingerTapsAndPenSideButtons() {
        action(obj("type" to "invoke", "command" to "brush"))
        action(obj("type" to "invoke", "command" to "add_layer"))
        fun enabled(id: String) = state().array("commands").objects().first { it.getString("id") == id }.getBoolean("enabled")
        fun tool() = state().getJSONObject("layer_tools").getString("tool")
        waitFor("undoable edit") { enabled("undo") && !enabled("redo") && tool() == "paint" }
        lateinit var canvas: View
        main { canvas = canvasSurface(mainWindow)!! }
        val origin = screenOrigin(canvas)
        fun point(i: Int) = origin.x + canvas.width * (.42f + .07f * i) to origin.y + canvas.height * .55f
        fun motion(down: Long, action: Int, points: List<Pair<Float, Float>>, tool: Int, source: Int, buttons: Int = 0): MotionEvent {
            val properties = points.indices.map { MotionEvent.PointerProperties().apply { id = it; toolType = tool } }.toTypedArray()
            val coordinates = points.map { (x, y) -> MotionEvent.PointerCoords().apply { this.x = x; this.y = y; pressure = if (source == InputDevice.SOURCE_TOUCHSCREEN) 1f else 0f; size = .1f } }.toTypedArray()
            return MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, points.size, properties, coordinates, 0, buttons, 1f, 1f, 0, 0, source, 0)
        }
        fun inject(down: Long, action: Int, points: List<Pair<Float, Float>>, tool: Int, source: Int) {
            val event = motion(down, action, points, tool, source)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
        }
        fun tap(fingers: Int) {
            val down = SystemClock.uptimeMillis()
            val points = (0 until fingers).map(::point)
            val finger = MotionEvent.TOOL_TYPE_FINGER
            inject(down, MotionEvent.ACTION_DOWN, points.take(1), finger, InputDevice.SOURCE_TOUCHSCREEN)
            for (i in 1 until fingers) inject(down, MotionEvent.ACTION_POINTER_DOWN or (i shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points.take(i + 1), finger, InputDevice.SOURCE_TOUCHSCREEN)
            for (i in fingers - 1 downTo 1) inject(down, MotionEvent.ACTION_POINTER_UP or (i shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points.take(i + 1), finger, InputDevice.SOURCE_TOUCHSCREEN)
            inject(down, MotionEvent.ACTION_UP, points.take(1), finger, InputDevice.SOURCE_TOUCHSCREEN)
        }
        val zoom = state().getJSONObject("camera").getDouble("zoom")
        tap(2)
        waitFor("two-finger tap undoes") { enabled("redo") }
        tap(3)
        waitFor("three-finger tap redoes") { !enabled("redo") }
        assertEquals(zoom, state().getJSONObject("camera").getDouble("zoom"), 0.0)
        val hover = SystemClock.uptimeMillis()
        fun stylus(action: Int, buttons: Int) {
            val local = point(1).let { (x, y) -> x - origin.x to y - origin.y }
            val event = motion(hover, action, listOf(local), MotionEvent.TOOL_TYPE_STYLUS, InputDevice.SOURCE_STYLUS, buttons)
            try { main { canvas.dispatchGenericMotionEvent(event) } } finally { event.recycle() }
            SystemClock.sleep(120)
        }
        stylus(MotionEvent.ACTION_HOVER_ENTER, 0)
        stylus(MotionEvent.ACTION_HOVER_MOVE, MotionEvent.BUTTON_STYLUS_PRIMARY)
        assertEquals("unbound side buttons stay with the driver", "paint", tool())
        stylus(MotionEvent.ACTION_HOVER_MOVE, 0)
        action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "pen_button", "value" to 1)))
        stylus(MotionEvent.ACTION_HOVER_MOVE, MotionEvent.BUTTON_STYLUS_PRIMARY)
        waitFor("bound side button samples while held") { tool() == "pick_visible" }
        stylus(MotionEvent.ACTION_HOVER_MOVE, 0)
        waitFor("release restores the brush") { tool() == "paint" }
        stylus(MotionEvent.ACTION_HOVER_EXIT, 0)
        action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "pen_button")))
        action(obj("type" to "invoke", "command" to "undo"))
    }

    @Test fun remoteKeysGamepadButtonsAndSticks() {
        fun enabled(id: String) = state().array("commands").objects().first { it.getString("id") == id }.getBoolean("enabled")
        fun record(id: String, key: String) {
            action(obj("type" to "invoke", "command" to "keyboard_shortcuts"))
            action(obj("type" to "preferences", "action" to obj("type" to "begin_shortcut", "id" to id)))
            main { host.input(obj("type" to "key", "key" to key, "pressed" to true, "repeat" to false)) }
            main { host.input(obj("type" to "key", "key" to key, "pressed" to false, "repeat" to false)) }
            SystemClock.sleep(150)
            action(obj("type" to "preferences", "action" to obj("type" to "confirm_shortcut", "replace" to true)))
            action(obj("type" to "close_settings"))
        }
        fun device(code: Int, source: Int) {
            val now = SystemClock.uptimeMillis()
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
                instrumentation.sendKeySync(KeyEvent(now, now, action, code, 0, 0, -1, 0, 0, source))
        }
        lateinit var activity: MainActivity
        scenario.onActivity { activity = it }
        record("command.Undo", "volumedown")
        record("tool_setting.size.increase", "gamepad_r1")
        action(obj("type" to "invoke", "command" to "brush"))
        action(obj("type" to "invoke", "command" to "add_layer"))
        waitFor("undoable edit") { enabled("undo") && !enabled("redo") }
        val audio = activity.getSystemService(android.media.AudioManager::class.java)
        val volume = audio.getStreamVolume(android.media.AudioManager.STREAM_MUSIC)
        device(KeyEvent.KEYCODE_VOLUME_DOWN, InputDevice.SOURCE_KEYBOARD)
        waitFor("a claimed volume key undoes") { enabled("redo") }
        assertEquals("the claimed key leaves the volume alone", volume, audio.getStreamVolume(android.media.AudioManager.STREAM_MUSIC))
        val size = state().getJSONObject("brush").getDouble("diameter")
        device(KeyEvent.KEYCODE_BUTTON_R1, InputDevice.SOURCE_GAMEPAD)
        waitFor("gamepad button steps the brush") { state().getJSONObject("brush").getDouble("diameter") > size }
        val camera = state().getJSONObject("camera").getJSONArray("translation").getDouble(0)
        fun stick(x: Float) {
            val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_UNKNOWN })
            val coordinates = arrayOf(MotionEvent.PointerCoords().apply { setAxisValue(MotionEvent.AXIS_X, x) })
            val event = MotionEvent.obtain(0, SystemClock.uptimeMillis(), MotionEvent.ACTION_MOVE, 1, properties, coordinates, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_JOYSTICK, 0)
            try { main { activity.dispatchGenericMotionEvent(event) } } finally { event.recycle() }
        }
        stick(.9f)
        waitFor("the left stick pans") { state().getJSONObject("camera").getJSONArray("translation").getDouble(0) < camera - 20 }
        stick(0f)
        SystemClock.sleep(100)
        val stopped = state().getJSONObject("camera").getJSONArray("translation").getDouble(0)
        SystemClock.sleep(250)
        assertEquals("a centered stick stops", stopped, state().getJSONObject("camera").getJSONArray("translation").getDouble(0), 0.0)
        for (id in listOf("command.Undo", "tool_setting.size.increase"))
            action(obj("type" to "preferences", "action" to obj("type" to "reset_shortcut", "id" to id)))
    }

    @Test fun keymapPresetsImportAndEditor() {
        fun press(tag: String) {
            waitFor(tag) { tagged(tag) != null }
            main { tagged(tag)!!.second.config[SemanticsActions.OnClick].action!!() }
            SystemClock.sleep(150)
        }
        fun keymap() = state().getJSONObject("settings").optJSONObject("keymap")?.getString("id")
        action(obj("type" to "invoke", "command" to "keyboard_shortcuts"))
        press("keymap-preset")
        press("keymap-choice-krita")
        waitFor("Krita keymap") { keymap() == "krita" }
        press("keymap-differences")
        waitFor("differences") { labelled("5") != null }
        val text = obj("format" to "capycanvas-keymap", "version" to 1, "keymap" to obj("id" to "photoshop", "revision" to 1)).toString()
        main { host.preference(obj("type" to "import_keymap", "text" to text)) }
        press("keymap-confirm-import")
        waitFor("imported keymap") { keymap() == "photoshop" }
        action(obj("type" to "preferences", "action" to obj("type" to "edit_shortcut", "id" to "command.Move")))
        waitFor("editor context") {
            tagged("shortcut-editor-context")?.second?.config?.getOrNull(SemanticsProperties.Text)?.any { it.text.contains("Photoshop-inspired") } == true
        }
        action(obj("type" to "preferences", "action" to obj("type" to "close_shortcut_editor")))
        action(obj("type" to "preferences", "action" to obj("type" to "select_keymap", "id" to "capy")))
        waitFor("default keymap") { keymap() == null }
        action(obj("type" to "close_settings"))
    }

    @Test fun panelGlassAndPlacement() {
        val (width, height) = 2048 to 1536
        val stripes = ByteArray(width * height * 4) { i -> if (i % 4 == 3 || i / 4 % width / 8 % 2 == 0) -1 else 0 }
        main { host.importLayer("Stripes", width, height, stripes) }
        waitFor("stripes") { state().getJSONObject("layer_tools").getJSONObject("editing_layer").getString("label") == "Stripes" }
        action(obj("type" to "invoke", "command" to "zen_mode"))
        fun covered() = state().getJSONObject("camera").let { camera ->
            val zoom = camera.getDouble("zoom"); val (x, y) = camera.getJSONArray("translation").let { it.getDouble(0) to it.getDouble(1) }
            val viewport = camera.getJSONArray("viewport")
            x <= 0 && y <= 0 && x + zoom * width >= viewport.getInt(0) && y + zoom * height >= viewport.getInt(1)
        }
        var style = JSONObject()
        main { style = host.catalog.getJSONObject("command_search_style") }
        val levels = listOf("off", "low", "medium", "high")
        val (natural, turned) = android.view.Surface.ROTATION_0 to android.view.Surface.ROTATION_90
        for ((theme, level, rotation) in listOf(Triple("dark", "low", natural), Triple("dark", "high", natural), Triple("light", "high", natural), Triple("light", "low", turned))) {
            if (mainWindow.display.rotation != rotation) {
                assertTrue(instrumentation.uiAutomation.setRotation(rotation))
                waitFor("rotation $rotation") { mainWindow.display.rotation == rotation }
                scenario.onActivity { mainWindow = it.window.decorView }
                waitFor("rotated viewport") {
                    val surface = canvasSurface(mainWindow) ?: return@waitFor false
                    state().getJSONObject("camera").getJSONArray("viewport").let { it.getInt(0) == surface.width && it.getInt(1) == surface.height }
                }
            }
            if (!covered()) {
                action(obj("type" to "invoke", "command" to "fit_canvas"))
                repeat(12) { if (!covered()) action(obj("type" to "invoke", "command" to "zoom_in")) }
                assertTrue("stripes cover the canvas", covered())
            }
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to levels.indexOf(level))))
            open()
            val name = "glass-$theme-$level-" + if (mainWindow.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT) "portrait" else "landscape"
            var card = Rect.Zero
            var region = Rect.Zero
            var observed = ""
            fun registered(box: FloatArray) = abs(box[0] - region.left) <= 1f && abs(box[1] - region.top) <= 1f &&
                abs(box[2] - region.width) <= 1f && abs(box[3] - region.height) <= 1f
            try {
                waitFor("$name placement and glass") {
                    val (owner, node) = tagged("command-bar") ?: return@waitFor false
                    val density = owner.view.resources.displayMetrics.density
                    val insets = ViewCompat.getRootWindowInsets(owner.view) ?: return@waitFor false
                    val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
                    val keyboard = owner.view.resources.configuration.keyboard != android.content.res.Configuration.KEYBOARD_NOKEYS
                    val top = ((owner.view.height - ime) / 5f).coerceIn(style.getInt("top_min") * density, style.getInt("top_max") * density)
                    card = node.boundsInRoot.translate(screenOrigin(owner.view))
                    region = card.translate(-screenOrigin(canvasSurface(mainWindow)!!))
                    val radius = style.getInt("radius") * density
                    val boxes = host.glassBoxesForTest
                    observed = "top=${node.boundsInRoot.top} expected=$top ime=$ime region=$region boxes=${boxes.map { it.toList() }}"
                    (keyboard || insets.isVisible(WindowInsetsCompat.Type.ime())) && abs(node.boundsInRoot.top - top) <= 1f &&
                        boxes.any { registered(it) && it.drop(4).all { r -> abs(r - radius) < .5f } }
                }
            } catch (e: AssertionError) { throw AssertionError("$name: $observed", e) }
            android.util.Log.i("CommandSearchTest", "$name card=$card $observed")
            val glass = state().getJSONObject("palette").getJSONObject("glass").getJSONArray("panel").let { c -> FloatArray(4) { c.getDouble(it).toFloat() } }
            val span = card.left + 16f..card.right - 16f
            capture(name) { pixels ->
                val behind = row(pixels, card.top - 40f, span)
                assertTrue("$name: stripes surround the bar", sharpness(behind) > .05)
                val expected = FloatArray(3) { glass[it] * glass[3] + blurred(behind)[it] * (1 - glass[3]) }
                for (y in listOf(card.top + 6f, card.bottom - 6f)) {
                    val inside = row(pixels, y, span)
                    assertTrue("$name y=$y: ${mean(inside).toList()} vs ${expected.toList()}", mean(inside).zip(expected).all { (a, b) -> abs(a - b) <= .05f })
                    assertTrue("$name y=$y: glass blurs the stripes", sharpness(inside) < .01)
                }
            }
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            waitFor("$name closed") { search() == null && tagged("command-bar") == null && host.glassBoxesForTest.none(::registered) }
            capture("$name-closed") { pixels ->
                for (y in listOf(card.top + 6f, card.bottom - 6f))
                    assertTrue("$name y=$y: no stale blur after closing", sharpness(row(pixels, y, span)) > .05)
            }
        }
        action(obj("type" to "invoke", "command" to "zen_mode"))
        action(obj("type" to "invoke", "command" to "undo"))
        waitFor("stripes removed") { state().getJSONArray("commands").objects().none { it.getString("id") == "undo" && it.getBoolean("enabled") } }
    }
}
