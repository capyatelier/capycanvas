package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.inspector.WindowInspector
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
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.pow
import kotlin.math.roundToInt

/** Real Android windows/input on an isolated workspace, including stylus contacts. */
class AndroidCommandSearchTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var mainWindow: View
    private var previousRotation = 0
    private var autoRotate = true
    private lateinit var root: File
    private fun main(block: () -> Unit) = instrumentation.runOnMainSync(block)
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun search() = state().objectOrNull("command_search")
    private fun findView(view: View): ViewRootForTest? {
        if (view is ViewRootForTest) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) findView(view.getChildAt(i))?.let { return it }
        return null
    }
    private fun find(node: SemanticsNode, tag: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.TestTag) == tag) node else node.children.firstNotNullOfOrNull { find(it, tag) }
    private fun label(node: SemanticsNode, text: String): SemanticsNode? =
        if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == text } == true) node else node.children.firstNotNullOfOrNull { label(it, text) }
    private fun labelled(text: String): Pair<ViewRootForTest, SemanticsNode>? = WindowInspector.getGlobalWindowViews().firstNotNullOfOrNull { view ->
        findView(view)?.let { owner -> label(owner.semanticsOwner.unmergedRootSemanticsNode, text)?.let { owner to it } }
    }
    private fun tagged(tag: String): Pair<ViewRootForTest, SemanticsNode>? = WindowInspector.getGlobalWindowViews().firstNotNullOfOrNull { view ->
        findView(view)?.let { owner -> find(owner.semanticsOwner.unmergedRootSemanticsNode, tag)?.let { owner to it } }
    }
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) {
        val until = SystemClock.uptimeMillis() + timeout
        do {
            var ready = false
            main { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
            if (ready) return
            SystemClock.sleep(16)
        } while (SystemClock.uptimeMillis() < until)
        capture("timeout")
        android.util.Log.e("CommandSearchTest", "$label query=${search()?.optString("query")} parameter=${search()?.objectOrNull("parameter")?.optString("id")} error=${search()?.optString("error")}")
        fail("Timed out: $label")
    }
    private fun action(value: JSONObject) { main { host.dispatch(value) }; SystemClock.sleep(150) }
    private fun key(code: Int, meta: Int = 0) {
        val now = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
            instrumentation.sendKeySync(KeyEvent(now, now, action, code, 0, meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
    }
    private fun open() {
        // Model publication precedes Compose's native dialog removal by a frame.
        waitFor("editor window focus") { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
        SystemClock.sleep(40)
        instrumentation.waitForIdleSync()
        key(KeyEvent.KEYCODE_K, KeyEvent.META_CTRL_ON)
        waitFor("search focus") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
    }
    private fun query(value: String) {
        main { assertTrue(tagged("command-search")!!.second.config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(value))) }
        if (search()?.objectOrNull("parameter") == null) waitFor("query $value") { search()?.optString("query") == value }
    }
    private fun detail() = tagged("command-detail")?.second?.config?.getOrNull(SemanticsProperties.Text)?.joinToString("") { it.text }
    private fun capture(name: String, inspect: (Bitmap) -> Unit = {}) {
        SystemClock.sleep(220)
        val output = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/command-search").apply { mkdirs() }
        val bitmap = instrumentation.uiAutomation.takeScreenshot()
        try {
            File(output, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.copy(Bitmap.Config.ARGB_8888, false).let { pixels -> try { inspect(pixels) } finally { pixels.recycle() } }
        } finally { bitmap.recycle() }
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
            val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool })
            val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f })
            val event = MotionEvent.obtain(now, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0,
                if (tool == MotionEvent.TOOL_TYPE_STYLUS) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
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
        root = File(instrumentation.targetContext.cacheDir, "command-tests/${java.util.UUID.randomUUID()}")
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        RecoveryController.directoryForTest = File(root, "recovery")
        ColorPreferencesStore.directoryForTest = File(root, "colors")
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {
            host = it.host; mainWindow = it.window.decorView
            previousRotation = mainWindow.display.rotation
            autoRotate = android.provider.Settings.System.getInt(it.contentResolver, android.provider.Settings.System.ACCELEROMETER_ROTATION, 1) != 0
        }
        waitFor("brush ready", 60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        waitFor("workspace ready", 60_000) { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
        action(obj("type" to "close_settings"))
    }
    @After fun cleanup() {
        instrumentation.uiAutomation.setRotation(if (autoRotate) android.app.UiAutomation.ROTATION_UNFREEZE else previousRotation)
        if (::scenario.isInitialized) scenario.close()
        CanvasHost.workspaceDirectoryForTest = null
        RecoveryController.directoryForTest = null
        ColorPreferencesStore.directoryForTest = null
        if (::root.isInitialized) root.deleteRecursively()
    }
    @Test fun nativeKeyboardTouchPenAndPerformance() {
        repeat(3) {
            open()
            instrumentation.sendStringSync("pencil")
            key(KeyEvent.KEYCODE_ENTER)
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
        key(KeyEvent.KEYCODE_ENTER)
        waitFor("parameter") {
            search()?.objectOrNull("parameter") != null &&
                tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith("Brush size") } == true
        }
        query("bad input"); key(KeyEvent.KEYCODE_ENTER)
        waitFor("validation") { search()?.optString("error")?.contains("null") == false }
        waitFor("validation footer") { search()?.optString("error")?.let { it.isNotEmpty() && detail() == it } == true }
        query("24"); key(KeyEvent.KEYCODE_ENTER)
        waitFor("size committed") { search() == null && state().getJSONObject("brush").optDouble("diameter") == 24.0 }
        action(obj("type" to "set_theme", "theme" to "dark"))
        open(); query("select"); key(KeyEvent.KEYCODE_DPAD_DOWN)
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
        key(KeyEvent.KEYCODE_ESCAPE); waitFor("closed") { search() == null }
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
        query("brush size"); key(KeyEvent.KEYCODE_ENTER)
        waitFor("parameter step") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith("Brush size") } == true }
        key(KeyEvent.KEYCODE_ESCAPE)
        waitFor("back to query") { search()?.objectOrNull("parameter") == null && search()?.optString("query") == "brush size" }
        key(KeyEvent.KEYCODE_ESCAPE); waitFor("menu search closed") { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
        key(KeyEvent.KEYCODE_P, KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON)
        waitFor("primary opener") { tagged("command-search")?.second?.config?.getOrNull(SemanticsProperties.Focused) == true }
        key(KeyEvent.KEYCODE_ESCAPE); waitFor("primary opener closed") { tagged("command-bar") == null && mainWindow.hasWindowFocus() }
        // Large-screen Android can ignore requestedOrientation; rotate the
        // native test display and restore the device's rotation policy afterward.
        assertTrue(instrumentation.uiAutomation.setRotation(android.app.UiAutomation.ROTATION_FREEZE_0))
        waitFor("portrait") { mainWindow.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT }
        scenario.onActivity { mainWindow = it.window.decorView }
        open(); query("select"); capture("portrait")
        key(KeyEvent.KEYCODE_ESCAPE)
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
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("$name closed") { search() == null && tagged("command-bar") == null && host.glassBoxesForTest.none(::registered) }
            capture("$name-closed") { pixels ->
                for (y in listOf(card.top + 6f, card.bottom - 6f))
                    assertTrue("$name y=$y: no stale blur after closing", sharpness(row(pixels, y, span)) > .05)
            }
        }
    }
}
