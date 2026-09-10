package art.capycanvas

import android.graphics.Bitmap
import android.content.ContentValues
import android.provider.MediaStore
import android.os.SystemClock
import android.os.ParcelFileDescriptor
import android.view.KeyEvent
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Before
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject
import org.json.JSONArray
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real native widgets, JNI and Vulkan in the tablet emulator. No fake renderer. */
class AndroidHostTest {
    companion object {
        private val runId = System.currentTimeMillis().toString()
        // Ask an isolated, GPU-less Rust session for its defaults, not a Kotlin
        // copy of the workspace schema. Repeated runs must not collect toolbars.
        private val defaultWorkspace by lazy {
            val handle = Native.create(false)
            try { JSONObject(Native.snapshot(handle)!!).getJSONObject("state").getJSONObject("workspace").toString() }
            finally { Native.destroy(handle) }
        }
    }
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private var originalWorkspace: JSONObject? = null
    @Before fun ready() {
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("gpu_ready") == true || host.failure != null }
        assertNull("GPU initialization", host.failure)
        originalWorkspace = JSONObject(state().getJSONObject("workspace").toString())
        compose.runOnIdle {
            host.dispatch(obj("type" to "close_settings"))
            host.dispatch(obj("type" to "set_theme", "theme" to "light"))
            host.dispatch(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
        }
        waitState { it.optString("theme") == "light" && it.getJSONObject("workspace").toString() == defaultWorkspace }
        compose.waitForIdle()
    }
    @After fun restoreWorkspace() {
        originalWorkspace?.let { workspace ->
            compose.runOnIdle {
                host.dispatch(obj("type" to "close_settings"))
                host.dispatch(obj("type" to "restore_workspace", "workspace" to workspace))
            }
            waitState { it.getJSONObject("workspace").toString() == workspace.toString() }
        }
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    @Test fun filterLayerIconsUsePackagedNames() {
        waitState { it.getLong("filter_catalog_revision") > 0 && !it.getJSONObject("filter_load").getBoolean("pending") }
        for (id in listOf("domain_warp", "curves", "color_balance")) {
            val choice = state().array("adjustments").objects().first { it.getString("id") == id }
            action(choice.getJSONObject("action"))
            val layer = state().getJSONObject("layer_properties").getLong("layer")
            // Insertion selects Properties; explicitly show Layers, where the
            // qualified core icon used to be prefixed/suffixed a second time.
            action(obj("type" to "select_panel_tab", "group" to group("layers").getLong("id"), "panel" to "layers"))
            compose.onNodeWithTag("layer-rows").assertIsDisplayed()
            compose.onNodeWithText(choice.getString("label")).assertIsDisplayed()
            val row = state().array("layers").objects().first { it.getLong("id") == layer }
            val icon = row.getString("content_icon")
            assertTrue(icon.startsWith("layer-") && icon.endsWith("-symbolic"))
            instrumentation.targetContext.assets.open("$icon.svg").use { assertTrue(it.read() >= 0) }
            assertNull(host.failure)
            action(obj("type" to "layer", "action" to obj("op" to "delete", "id" to layer)))
        }
    }
    @Test fun runtimeFilterPackages() {
        waitState { it.getLong("filter_catalog_revision") > 0 && !it.getJSONObject("filter_load").getBoolean("pending") }
        assertTrue(state().getJSONObject("filter_load").isNull("error"))
        assertEquals(40, state().array("adjustments").length())
        val assets = instrumentation.context.assets
        val manifest = assets.open("tent-blur/manifest.json").bufferedReader().use { it.readText() }
        val modules = JSONObject()
        for (name in listOf("prepare.wgsl","tent.wgsl")) modules.put(name, assets.open("tent-blur/$name").bufferedReader().use { it.readText() })
        fun load(text: String, mode: String) {
            val before = state().getJSONObject("filter_load").getLong("request_id")
            compose.runOnIdle { host.loadFilters(text,modules,mode) }
            waitState { val s=it.getJSONObject("filter_load");s.getLong("request_id")>before && !s.getBoolean("pending") }
        }
        load(manifest,"add")
        assertTrue(state().getJSONObject("filter_load").isNull("error"))
        assertEquals(41,state().array("adjustments").length())
        val pixels = ByteArray(1024*768*4)
        for(i in 0 until 1024*768) {
            val color = if((i%1024/32+i/1024/32)%2==0) intArrayOf(230,50,80,255) else intArrayOf(30,160,220,255)
            for(c in 0..3) pixels[i*4+c]=color[c].toByte()
        }
        compose.runOnIdle { host.importLayer("Runtime checker",1024,768,pixels) }
        waitState { it.getJSONObject("layer_tools").getJSONObject("editing_layer").getString("label")=="Runtime checker" }
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to "examples")))
        action(obj("type" to "select_panel_tab", "group" to group("adjustments").getLong("id"), "panel" to "adjustments"))
        compose.onNodeWithTag("adjustment-example:tent_blur").performClick()
        waitState { it.getJSONObject("layer_properties").getString("description")=="Tent Blur" }
        val layer = state().getJSONObject("layer_properties").getLong("layer")
        action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to "radius", "value" to obj("kind" to "number", "value" to 9))))
        val edited = manifest.replace("\"label\": \"Radius\"", "\"label\": \"Runtime radius\"")
        modules.put("prepare.wgsl",modules.getString("prepare.wgsl").replace("max(width-f32(i),0.)/(width*width)","select(0.,1./(2.*width-1.),i<=radius)"))
        load(edited,"replace")
        assertTrue(state().getJSONObject("filter_load").isNull("error"))
        assertEquals("Runtime radius",state().getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getString("label"))
        assertEquals(9.0,state().getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").getDouble("value"),0.0)
        val revision = state().getLong("filter_catalog_revision")
        modules.put("prepare.wgsl","invalid preparation WGSL")
        load(edited,"replace")
        assertFalse(state().getJSONObject("filter_load").isNull("error"))
        assertEquals(revision,state().getLong("filter_catalog_revision"))
        capture("runtime-filter-properties")
    }
    @Test fun adjustmentPanelsUseSharedSchema() {
        action(obj("type" to "set_theme", "theme" to "dark"))
        action(obj("type" to "set_brush_size", "value" to 220))
        listOf(listOf(.9,.12,.08,1),listOf(.08,.7,.15,1),listOf(.1,.2,.9,1)).forEachIndexed { i, color ->
            action(obj("type" to "set_color", "rgba" to JSONArray(color)))
            val x=.4f+i*.1f
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(androidx.compose.ui.geometry.Offset(x,.4f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(androidx.compose.ui.geometry.Offset(x+.03f,.6f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(androidx.compose.ui.geometry.Offset(x+.03f,.6f)), MotionEvent.TOOL_TYPE_STYLUS)
        }
        val choices=state().array("adjustments").objects().map { it.getString("id") }
        assertEquals(40, choices.size)
        action(obj("type" to "select_panel_tab", "group" to group("adjustments").getLong("id"), "panel" to "adjustments"))
        compose.waitUntil(20_000) { host.filterPreviewCache.images[choices.first()] != null }
        compose.onNodeWithTag("filter-preview-${choices.first()}", useUnmergedTree=true).assertHeightIsEqualTo(40.dp)
        val preview = host.filterPreviewCache.images.getValue(choices.first()).image.toPixelMap()
        assertTrue("GPU preview has opaque artwork", (0 until preview.width).any { preview[it,preview.height/2].alpha>.5f })
        assertEquals("Silhouette has transparent corners", 0f, preview[0,0].alpha, .01f)
        capture("adjustments-picker")
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to "distort")))
        compose.onNodeWithTag("filter-search-toggle").performClick()
        val filterSearch = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search")))
        filterSearch.performTextInput("glass")
        waitState { it.array("adjustments").objects().map { c -> c.getString("id") }==listOf("glass","rainy_glass") }
        compose.waitUntil(20_000) { host.filterPreviewCache.images["rainy_glass"] != null }
        filterSearch.assertTextContains("glass").performImeAction()
        SystemClock.sleep(300)
        filterSearch.assertTextContains("glass")
        compose.onNodeWithTag("adjustment-chromatic_aberration").assertDoesNotExist()
        compose.onNodeWithContentDescription("Rainy Glass · Animated", useUnmergedTree=true).assertExists()
        capture("adjustments-glass-search")
        compose.onNodeWithTag("filter-search-toggle").performClick()
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
        for(id in choices) {
            action(obj("type" to "select_panel_tab", "group" to group("adjustments").getLong("id"), "panel" to "adjustments"))
            compose.onNodeWithTag("filter-list").performScrollToNode(hasTestTag("adjustment-$id"))
            compose.onNodeWithTag("adjustment-$id").performClick()
            waitState { it.getJSONObject("layer_properties").getString("description") == it.array("adjustments").objects().first { c->c.getString("id")==id }.getString("label") }
            compose.onNodeWithTag("layer-properties").assertIsDisplayed()
            if(id=="color_balance") listOf("Shadows", "Midtones", "Highlights").forEach { compose.onNodeWithText(it).assertExists() }
            val view=state().getJSONObject("layer_properties");val controls=view.array("controls").objects()
            val number=controls.firstOrNull { it.getJSONObject("kind").getString("kind")=="number" }
            if(number!=null) action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to view.getLong("layer"), "key" to number.getString("key"),
                "value" to obj("kind" to "number", "value" to number.getJSONObject("kind").getJSONObject("numeric").number("min")))))
            else if(controls.any { it.getJSONObject("kind").getString("kind")=="curve" }) {
                compose.onNodeWithTag("effect-curve").performTouchInput { click(center) }
                waitState { it.getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").getJSONArray("value").length()==3 }
            }
            if(controls.any {it.getJSONObject("kind").getString("kind")=="gradient"}) {
                compose.onNodeWithTag("effect-gradient").performTouchInput {click(center)}
                waitState {it.getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").getJSONArray("value").length()==3}
                action(obj("type" to "effect", "action" to obj("op" to "gradient_stop", "layer" to view.getLong("layer"),
                    "key" to "gradient", "index" to 1, "position" to .5, "color" to JSONArray(listOf(.8,.2,.1,1)), "remove" to false)))
                action(obj("type" to "effect", "action" to obj("op" to "reset", "layer" to view.getLong("layer"), "key" to "amount")))
            }
            capture("adjustment-$id")
            action(obj("type" to "set_layer_visibility", "id" to view.getLong("layer"), "visible" to false))
        }
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true))
        floatPanel("stats",650f,120f)
        action(obj("type" to "set_layer_visibility", "id" to state().getJSONObject("layer_properties").getLong("layer"), "visible" to true))
        repeat(16) { action(obj("type" to "set_layer_opacity", "id" to state().getJSONObject("layer_properties").getLong("layer"), "opacity" to .8+it*.01)) }
        compose.onNodeWithTag("renderer-stats").assertIsDisplayed()
        capture("adjustments-stats-dark")
        action(obj("type" to "set_theme", "theme" to "light"));capture("adjustments-stats-light")
    }
    private fun preferences() = host.snapshot!!.getJSONObject("preferences")
    /** Opt-in platform sweep; normal correctness tests don't run a benchmark. */
    @Test fun measureFilterLibrary() {
        org.junit.Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("capyFilterBenchmark") == "true")
        fun stats(): JSONObject {
            val done=CountDownLatch(1);var result:JSONObject?=null
            host.query(obj("type" to "renderer_stats")) {result=it as JSONObject;done.countDown()}
            assertTrue(done.await(10,TimeUnit.SECONDS));return result!!
        }
        fun frames(view:JSONObject)=view.array("rows").objects().first {it.getString("label")=="Frames"}.getString("value").toLong()
        fun effect(a:JSONObject)=action(obj("type" to "effect","action" to a))
        customize(obj("type" to "set_panel_visible","panel" to "stats","visible" to true))
        action(obj("type" to "set_brush_size","value" to 24))
        penStroke(40)
        val choices=state().array("adjustments").objects().map {it.getString("id")}
        val expensive=listOf("motion_blur","gaussian_blur","domain_warp","painterly","denoise")
        val prepared=listOf("pencil","soft_focus","bloom","gaussian_blur","unsharp_mask")
        val cases=listOf("Baseline" to emptyList<String>())+choices.map {it to listOf(it)}+
            listOf("Five expensive" to expensive,"Prepared edits" to listOf("unsharp_mask"),"Five prepared edits" to prepared)
        val report=JSONArray()
        for((name,filters) in cases) {
            val ids=mutableListOf<Long>()
            for(id in filters) {
                effect(obj("op" to "insert","effect" to id))
                val view=state().getJSONObject("layer_properties");val layer=view.getLong("layer");ids.add(layer)
                val controls=view.array("controls").objects()
                if(controls.any {it.getString("key")=="animate"}) effect(obj("op" to "set","layer" to layer,"key" to "animate","value" to obj("kind" to "toggle","value" to false)))
                if(id=="curves") effect(obj("op" to "curve_point","layer" to layer,"key" to "curve_0","index" to null,"point" to JSONArray(listOf(.45,.65)),"remove" to false))
                else controls.firstOrNull {it.getJSONObject("kind").getString("kind")=="number" && it.getJSONObject("value").number("value")==0f && it.getString("key")!="time"}?.let {c ->
                    effect(obj("op" to "set","layer" to layer,"key" to c.getString("key"),"value" to obj("kind" to "number","value" to c.getJSONObject("kind").getJSONObject("numeric").number("max")*.25)))
                }
            }
            val modes=if(name.endsWith("edits")) listOf("relevant","unrelated") else if(name=="Five expensive") listOf("local","full","animation") else listOf("local")
            for(mode in modes) {
                if(mode=="animation") effect(obj("op" to "set","layer" to ids[2],"key" to "animate","value" to obj("kind" to "toggle","value" to true)))
                action(obj("type" to "select_layer","id" to 1))
                val before=frames(stats());var count=0;var after=before
                while(after-before<180 && count<1200) {
                    if(mode=="local") canvasEvent(if(count==0) MotionEvent.ACTION_DOWN else MotionEvent.ACTION_MOVE,
                        listOf(androidx.compose.ui.geometry.Offset(.5f+kotlin.math.sin(count*.1f)*.04f,.5f+kotlin.math.cos(count*.15f)*.03f)),MotionEvent.TOOL_TYPE_STYLUS)
                    if(mode=="full") host.dispatch(obj("type" to "set_layer_opacity","id" to 1,"opacity" to .7+(count%20)*.01))
                    if(mode=="relevant"||mode=="unrelated") host.dispatch(obj("type" to "effect","action" to obj("op" to "set","layer" to ids.last(),
                        "key" to if(mode=="relevant") "sigma" else "amount","value" to obj("kind" to "number","value" to if(mode=="relevant") 2+(count%20)*.5 else 50+(count%20)*5))))
                    SystemClock.sleep(9);count++
                    if(count%30==0)after=frames(stats())
                }
                if(mode=="local")canvasEvent(MotionEvent.ACTION_UP,listOf(androidx.compose.ui.geometry.Offset(.5f,.5f)),MotionEvent.TOOL_TYPE_STYLUS)
                assertTrue("$name $mode produced at least 180 updates",after-before>=180)
                val result=obj("filter" to name,"mode" to mode,"stats" to stats(),"frames" to after-before)
                report.put(result);android.util.Log.i("CapyFilterBenchmark",result.toString())
            }
            for(id in ids.reversed()) {action(obj("type" to "select_layer","id" to id));action(obj("type" to "layer","action" to obj("op" to "delete_selected")))}
            action(obj("type" to "select_layer","id" to 1))
        }
        val resolver=compose.activity.contentResolver
        val uri=resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME,"filter-android.json");put(MediaStore.Downloads.MIME_TYPE,"application/json")
            put(MediaStore.Downloads.RELATIVE_PATH,"Download/CapyCanvasValidation/$runId");put(MediaStore.Downloads.IS_PENDING,1)
        })!!
        resolver.openOutputStream(uri)!!.bufferedWriter().use {it.write(report.toString(2))}
        resolver.update(uri,ContentValues().apply {put(MediaStore.Downloads.IS_PENDING,0)},null,null)
    }
    private fun groups() = host.snapshot!!.getJSONObject("layout").array("groups").objects()
    private fun group(panel: String) = groups().first { panel in it.array("panels").values() }
    private fun viewport(): JSONArray {
        val bounds = compose.onNodeWithTag("workspace").fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        return JSONArray(listOf(bounds.width / density, bounds.height / density))
    }
    private fun action(action: JSONObject) {
        val done = CountDownLatch(1)
        compose.runOnIdle { host.dispatch(action); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        compose.waitForIdle()
        assertNull(host.actionError)
    }
    private fun customize(action: JSONObject) = action(obj("type" to "customize", "action" to action))
    private fun floatPanel(panel: String, x: Float = 500f, y: Float = 340f) = action(obj("type" to "move_panel", "panel" to panel,
        "target" to obj("kind" to "float", "position" to JSONArray(listOf(x, y))), "viewport" to viewport()))
    private fun screenBounds(node: SemanticsNodeInteraction): androidx.compose.ui.geometry.Rect {
        val semantics = node.fetchSemanticsNode()
        return androidx.compose.ui.geometry.Rect(semantics.positionOnScreen,
            androidx.compose.ui.geometry.Size(semantics.size.width.toFloat(), semantics.size.height.toFloat()))
    }
    private fun workspaceMenu() {
        val button = compose.onNode(hasText("Workspace") and hasClickAction())
        val anchor = screenBounds(button)
        button.performClick()
        val menu = screenBounds(compose.onNodeWithTag("workspace-menu"))
        assertEquals("Workspace menu aligns with its header button", anchor.left, menu.left, 2f)
        assertEquals("Workspace menu opens below its header button", anchor.bottom, menu.top, 2f)
    }
    private fun assertContextBeside(tag: String) {
        // The group menu is anchored to the draggable header, not its grip.
        // Compose can align to either end of that header as its width changes.
        val header = tag.replace("group-grip-", "group-header-")
        val anchorTag = if (header != tag && compose.onAllNodesWithTag(header).fetchSemanticsNodes().isNotEmpty()) header else tag
        val anchor = screenBounds(compose.onNodeWithTag(anchorTag))
        val menu = screenBounds(compose.onNodeWithTag("workspace-menu"))
        assertTrue("Menu $menu must remain beside its trigger $anchor",
            maxOf(anchor.left - menu.right, menu.left - anchor.right, 0f) <= 2f &&
                maxOf(anchor.top - menu.bottom, menu.top - anchor.bottom, 0f) <= 2f)
    }
    private fun contextGrip(tag: String) {
        compose.onNodeWithTag(tag).performTouchInput { longClick() }
        compose.waitUntil(10_000) { compose.onAllNodes(isPopup()).fetchSemanticsNodes().isNotEmpty() }
        assertContextBeside(tag)
    }
    private fun shell(command: String) = ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command))
        .bufferedReader().use { it.readText() }
    private fun waitState(test: (JSONObject) -> Boolean) = compose.waitUntil(10_000) { test(state()) }
    private fun findCanvas(view: View): CanvasSurfaceView? = when (view) {
        is CanvasSurfaceView -> view
        is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { findCanvas(view.getChildAt(it)) }
        else -> null
    }
    private fun penStroke(steps: Int = 60, synchronous: Boolean = true) {
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        instrumentation.runOnMainSync {
            canvas = findCanvas(compose.activity.window.decorView)!!
            canvas.getLocationOnScreen(location)
        }
        val down = SystemClock.uptimeMillis()
        for (i in 0..steps) {
            val coords = MotionEvent.PointerCoords().apply {
                x = canvas.width * (0.35f + i.toFloat() / steps * 0.3f) + location[0]
                y = canvas.height * (0.45f + kotlin.math.sin(i / 12f) * 0.08f) + location[1]
                pressure = 0.3f + i.toFloat() / steps * 0.6f
                setAxisValue(MotionEvent.AXIS_TILT, 0.3f)
                setAxisValue(MotionEvent.AXIS_ORIENTATION, 0.2f)
            }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val action = when (i) { 0 -> MotionEvent.ACTION_DOWN; steps -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, arrayOf(props), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
            assertTrue("Stylus event accepted", instrumentation.uiAutomation.injectInputEvent(event, synchronous))
            event.recycle()
            if (i != steps) SystemClock.sleep(8)
        }
    }
    /** Native dispatch tests retain full MotionEvent history and pointer IDs. */
    private fun canvasEvent(action: Int, points: List<androidx.compose.ui.geometry.Offset>,
        tool: Int = MotionEvent.TOOL_TYPE_FINGER, history: Boolean = false,
        pointerTools: List<Int> = List(points.size) { tool }) {
        instrumentation.runOnMainSync {
            val canvas = findCanvas(compose.activity.window.decorView)!!
            val coords = points.map { point -> MotionEvent.PointerCoords().apply {
                x = canvas.width * point.x; y = canvas.height * point.y; pressure = 0.7f
                setAxisValue(MotionEvent.AXIS_TILT, 0.4f)
            } }.toTypedArray()
            val props = points.indices.map { i -> MotionEvent.PointerProperties().apply { id = i; toolType = pointerTools[i] } }.toTypedArray()
            val time = SystemClock.uptimeMillis()
            val source = when (tool) {
                MotionEvent.TOOL_TYPE_FINGER -> InputDevice.SOURCE_TOUCHSCREEN
                MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE
                else -> InputDevice.SOURCE_STYLUS
            }
            val event = MotionEvent.obtain(time - 30, time - if (history) 2 else 0, action, points.size, props, coords, 0, 0, 1f, 1f, 1, 0, source, 0)
            if (history) event.addBatch(time, coords.map { old -> MotionEvent.PointerCoords(old).apply { x += 4f; pressure = 0.9f } }.toTypedArray(), 0)
            assertTrue(if (action == MotionEvent.ACTION_HOVER_MOVE) canvas.dispatchGenericMotionEvent(event) else canvas.dispatchTouchEvent(event))
            event.recycle()
        }
    }
    private fun capture(name: String): Bitmap {
        compose.waitForIdle()
        // Compose can be idle before SurfaceFlinger presents its last frame.
        // Allow two vsyncs for theme/visibility changes to reach the compositor.
        val presented = CountDownLatch(1)
        compose.runOnIdle {
            val view = compose.activity.window.decorView
            view.postOnAnimation { view.postOnAnimation { presented.countDown() } }
        }
        assertTrue(presented.await(5, TimeUnit.SECONDS))
        val bitmap = instrumentation.uiAutomation.takeScreenshot()
        val directory = File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }
        File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        // Gradle's connected-test runner uninstalls the app, removing its private
        // output directory. MediaStore test captures survive for visual review.
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, "$name.png")
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/CapyCanvasValidation/$runId")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        })!!
        resolver.openOutputStream(uri)!!.use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
        return bitmap
    }
    private fun darkPixels(image: Bitmap): Int {
        var dark = 0
        for (y in image.height * 35 / 100 until image.height * 65 / 100 step 2) {
            for (x in image.width * 35 / 100 until image.width * 65 / 100 step 2) {
                val c = image.getPixel(x, y)
                if (android.graphics.Color.red(c) < 100 && android.graphics.Color.green(c) < 100) dark++
            }
        }
        return dark
    }
    @Test fun layersReferencesMasksAndTools() {
        val presets = listOf(2,4,6,8).map { compose.onNodeWithTag("size-preset-$it").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot() }
        presets.zipWithNext().forEach { (left,right) ->
            assertEquals(left.top,right.top,1f)
            assertTrue("Tooltip wrappers must preserve the four-column grid",left.right<=right.left+1f)
        }
        fun layer(action: JSONObject) = action(obj("type" to "layer","action" to action))
        layer(obj("op" to "rename","id" to 1,"name" to "Linework"))
        compose.onNodeWithContentDescription("Use selected layers as references").performClick()
        waitState { it.array("layers").objects().first { l -> l.getLong("id")==1L }.getBoolean("reference") }
        compose.onNodeWithContentDescription("New layer").performClick()
        waitState { it.array("layers").length()==3 }
        val paint=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        layer(obj("op" to "rename","id" to paint,"name" to "Color wash"))
        layer(obj("op" to "toggle_selection","id" to 1))
        compose.onNodeWithContentDescription("Use selected layers as references").performClick()
        waitState { it.array("layers").objects().count { l -> l.getBoolean("reference") }==2 }
        assertEquals(1,state().array("layers").objects().count { it.getBoolean("selected") })
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(.8,.2,.12,1))))
        layer(obj("op" to "tool","tool" to "lasso_fill"))
        penStroke(30)
        compose.onNodeWithContentDescription("Add layer mask").performClick()
        waitState { it.getJSONObject("layer_tools").getJSONObject("editing_layer").getBoolean("has_mask") }
        SystemClock.sleep(800)
        capture("layers-paint-mask-light")
        action(obj("type" to "set_theme","theme" to "dark"))
        SystemClock.sleep(300)
        capture("layers-paint-mask-dark")
        compose.onNodeWithContentDescription("Layer actions").performClick()
        compose.onNodeWithText("Delete mask").assertExists()
        capture("layers-mask-menu-dark")
        compose.onNodeWithText("Delete mask").performClick()
        waitState { !it.getJSONObject("layer_tools").getJSONObject("editing_layer").getBoolean("has_mask") }
        for(command in listOf("lasso","move","brush")) {
            action(obj("type" to "invoke","command" to command))
            assertTrue(state().array("commands").objects().first { it.getString("id")==command }.getBoolean("selected"))
        }
        compose.onNodeWithText("Paper").performClick()
        waitState { it.getJSONObject("layer_tools").getJSONObject("editing_layer").getString("label")=="Paper" }
        assertNull(host.actionError)
        capture("layers-paper-selected")
    }
    @Test fun dockingLoneFloatingPanelShowsTabs() {
        floatPanel("sizes")
        val id = group("sizes").getInt("id")
        assertFalse(group("sizes").getBoolean("tabs_visible"))
        val workspace = compose.onNodeWithTag("workspace")
        val root = workspace.fetchSemanticsNode().boundsInRoot
        val start = compose.onNodeWithTag("group-grip-$id").fetchSemanticsNode().boundsInRoot.center - root.topLeft
        workspace.performTouchInput { down(start); moveTo(androidx.compose.ui.geometry.Offset(root.width-2f,root.height*.5f),300); up() }
        compose.waitUntil(10_000) { !group("sizes").getBoolean("floating") }
        assertTrue(group("sizes").getBoolean("tabs_visible"))
        compose.onNodeWithTag("tab-sizes").assertIsDisplayed()
        capture("panel-tab-shown-after-docking")
        action(obj("type" to "invoke","command" to "undo_workspace"))
        assertTrue(group("sizes").getBoolean("floating"))
        assertFalse(group("sizes").getBoolean("tabs_visible"))
        action(obj("type" to "invoke","command" to "redo_workspace"))
        assertFalse(group("sizes").getBoolean("floating"))
        assertTrue(group("sizes").getBoolean("tabs_visible"))
    }

    @Test fun workspaceMenusManageVisibilityNamesAndHistory() {
        workspaceMenu()
        capture("workspace-menu-light")
        compose.onNodeWithText("Brushes panel").performClick()
        compose.waitUntil(10_000) { groups().none { "brushes" in it.array("panels").values() } }
        workspaceMenu(); compose.onNodeWithText("Brushes panel").performClick()
        compose.waitUntil(10_000) { groups().any { "brushes" in it.array("panels").values() } }
        val destination = group("layers").getInt("id")
        contextGrip("group-grip-$destination")
        compose.onNodeWithText("Add built-in panel").performClick()
        assertContextBeside("group-grip-$destination")
        capture("workspace-panel-grip-add-panel")
        compose.onNodeWithText("Brushes panel").performClick()
        compose.waitUntil(10_000) { group("brushes").getInt("id") == destination }
        contextGrip("group-grip-$destination")
        compose.onNodeWithText("Add Toolbar").performClick()
        assertContextBeside("group-grip-$destination")
        capture("workspace-panel-grip-add-toolbar")
        compose.onNodeWithText("Tools toolbar").performClick()
        compose.waitUntil(10_000) { group("toolbar").getInt("id") == destination }
        val width = group("toolbar").getJSONObject("bounds").number("width")
        val tabWidths = listOf("layers", "brushes", "toolbar").sumOf {
            compose.onNodeWithTag("tab-$it").fetchSemanticsNode().boundsInRoot.width.toDouble() / compose.activity.resources.displayMetrics.density
        }
        assertTrue("Tabs grow the group", width >= tabWidths + 19)
        // Undoing workspace edits is independent of canvas undo and restores the toolbar.
        workspaceMenu(); compose.onNodeWithText("Undo Workspace Change").performClick()
        compose.waitUntil(10_000) { group("toolbar").getInt("id") != destination }
        compose.onNodeWithTag("ribbon-grip-toolbar").performMouseInput { click(button = MouseButton.Secondary) }
        compose.waitUntil(10_000) { compose.onAllNodes(isPopup()).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Duplicate Tools toolbar…").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
        val nameField = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("toolbar-name")))
        nameField.performTextReplacement("Brushes")
        compose.waitUntil(10_000) { !host.snapshot!!.getJSONObject("toolbar_prompt").getBoolean("can_confirm") }
        compose.onNodeWithText("Duplicate", substring = false).assertIsNotEnabled()
        nameField.performTextReplacement("Quick tools")
        nameField.performImeAction()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("toolbar_prompt").getBoolean("can_confirm") }
        capture("workspace-duplicate-prompt")
        compose.onNodeWithText("Duplicate", substring = false).performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") == null }
        val copy = host.snapshot!!.array("panels").objects().first { it.getString("title") == "Quick tools" }.getString("id")
        floatPanel(copy)
        contextGrip("ribbon-grip-$copy")
        compose.onNodeWithText("Rename Quick tools toolbar…").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
        compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("toolbar-name"))).performTextReplacement("Paint tools")
        compose.onNodeWithText("Rename", substring = false).performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") == null }
        assertEquals("Paint tools", host.snapshot!!.array("panels").objects().first { it.getString("id") == copy }.getString("title"))
        contextGrip("ribbon-grip-$copy")
        capture("workspace-renamed-menu")
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        workspaceMenu(); compose.onNodeWithText("Manage Toolbars…").performClick()
        compose.onNodeWithTag("managed-toolbar-$copy").performClick()
        compose.onNodeWithTag("delete-managed-toolbar").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
        val messageLayout = mutableListOf<TextLayoutResult>()
        compose.onNodeWithTag("toolbar-prompt-message").performSemanticsAction(SemanticsActions.GetTextLayoutResult) {
            assertTrue(it(messageLayout))
        }
        val text = messageLayout.single()
        assertEquals("Copy remains verbatim from Rust", host.snapshot!!.getJSONObject("toolbar_prompt").getString("message"), text.layoutInput.text.text)
        val arrow = text.placeholderRects.single()!!
        val preceding = text.getBoundingBox(text.layoutInput.text.text.indexOf('→') - 2)
        assertEquals("Menu-path arrow is centered beside the text", preceding.center.y, arrow.center.y, 2f)
        capture("workspace-delete-prompt")
        compose.onNodeWithText("Delete Toolbar", substring = false).performClick()
        compose.waitUntil(10_000) { groups().none { copy in it.array("panels").values() } }
        compose.onNodeWithTag("close-toolbar-manager").performClick()
        workspaceMenu(); compose.onNodeWithText("Undo Workspace Change").performClick()
        compose.waitUntil(10_000) { groups().any { copy in it.array("panels").values() } }
        action(obj("type" to "set_theme", "theme" to "dark"))
        workspaceMenu(); capture("workspace-menu-dark")
        compose.onNodeWithText("Redo Workspace Change").performClick()
        compose.waitUntil(10_000) { groups().none { copy in it.array("panels").values() } }
    }

    @Test fun toolbarManagerSelectsConfirmsDeletesAndRestores() {
        for (theme in listOf("dark", "light")) {
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
            action(obj("type" to "set_theme", "theme" to theme))
            for (name in listOf("Sketching", "Painting")) {
                customize(obj("type" to "duplicate_toolbar", "panel" to "toolbar"))
                customize(obj("type" to "toolbar_name", "name" to name))
                customize(obj("type" to "confirm_toolbar"))
            }
            val hidden = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().last().getString("id")
            customize(obj("type" to "set_panel_visible", "panel" to hidden, "visible" to false))
            val before = state().getJSONObject("workspace").toString()
            workspaceMenu(); compose.onNodeWithText("Manage Toolbars…").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_manager") != null }
            compose.onNodeWithTag("delete-managed-toolbar").assertIsNotEnabled()
            capture("toolbar-manager-$theme-initial")
            compose.onNodeWithTag("managed-toolbar-$hidden").performClick()
            compose.onNodeWithTag("delete-managed-toolbar").assertIsEnabled()
            capture("toolbar-manager-$theme-selected")
            compose.onNodeWithTag("delete-managed-toolbar").performClick()
            capture("toolbar-manager-$theme-confirm")
            compose.onNodeWithText("Cancel").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") == null }
            assertEquals(before, state().getJSONObject("workspace").toString())
            compose.onNodeWithTag("delete-managed-toolbar").performClick()
            compose.onNodeWithText("Delete Toolbar", substring = false).performClick()
            compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").length() == 2 }
            compose.onNodeWithTag("delete-managed-toolbar").assertIsNotEnabled()
            capture("toolbar-manager-$theme-deleted")
            while (host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").length() > 0) {
                val panel = host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").objects().first().getString("panel")
                compose.onNodeWithTag("managed-toolbar-$panel").performClick()
                compose.onNodeWithTag("delete-managed-toolbar").performClick()
                compose.onNodeWithText("Delete Toolbar", substring = false).performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") == null }
            }
            compose.onNodeWithTag("delete-managed-toolbar").assertIsNotEnabled()
            capture("toolbar-manager-$theme-empty")
            compose.onNodeWithTag("close-toolbar-manager").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_manager") == null }
            action(obj("type" to "invoke", "command" to "undo_workspace"))
            assertTrue(state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().any { it.getJSONObject("content").getString("kind") == "toolbar" })
            assertNull(host.actionError)
        }
    }

    @Test fun tabGroupStylesFollowSelectionAndHaveNoPanelOverrides() {
        val id = group("brushes").getInt("id")
        for (panel in listOf("sizes", "layers")) action(obj("type" to "move_panel", "panel" to panel,
            "target" to obj("kind" to "tab", "group" to id), "viewport" to viewport()))
        for (theme in listOf("dark", "light")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((style, label) in listOf("automatic" to "Automatic", "active_name" to "Icons and active tab name", "icon_name" to "Icons and names", "name" to "Names only", "icon" to "Icons only")) {
                contextGrip("group-grip-$id")
                compose.onNodeWithText(label).performClick()
                for (active in listOf("brushes", "sizes", "layers")) {
                    compose.onNodeWithTag("tab-$active").performClick()
                    compose.waitUntil(10_000) { group(active).getString("active") == active }
                    for (panel in listOf("brushes", "sizes", "layers")) {
                        val icon = compose.onNodeWithTag("tab-icon-$panel", useUnmergedTree = true)
                        val name = compose.onNodeWithTag("tab-name-$panel", useUnmergedTree = true)
                        if (style != "name") icon.assertExists() else icon.assertDoesNotExist()
                        if (style == "icon_name" || style == "name" || (style in listOf("automatic", "active_name") && panel == active)) name.assertExists() else name.assertDoesNotExist()
                    }
                }
                capture("group-tabs-$style-$theme")
            }
            customize(obj("type" to "set_tab_style", "group" to id, "style" to "automatic"))
            floatPanel("layers", 850f, 200f)
            for (panel in listOf("brushes", "sizes")) {
                compose.onNodeWithTag("tab-icon-$panel", useUnmergedTree = true).assertExists()
                compose.onNodeWithTag("tab-name-$panel", useUnmergedTree = true).assertExists()
            }
            capture("group-tabs-automatic-two-tabs-$theme")
            action(obj("type" to "move_panel", "panel" to "layers", "target" to obj("kind" to "tab", "group" to id), "viewport" to viewport()))
            for (panel in listOf("brushes", "sizes")) compose.onNodeWithTag("tab-name-$panel", useUnmergedTree = true).assertDoesNotExist()
            compose.onNodeWithTag("tab-name-layers", useUnmergedTree = true).assertExists()
        }
        compose.onNodeWithTag("tab-layers").performTouchInput { longClick() }
        compose.onNodeWithText("Icons only").assertDoesNotExist()
        compose.onNodeWithText("Configure Layers panel…").performClick()
        customize(obj("type" to "close_expanded"))
    }

    @Test fun dockedPanelHandlesToggleTabsOnFirstDoubleTap() {
        val id = group("sizes").getInt("id")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (style in listOf("automatic", "active_name", "icon_name", "name", "icon")) {
                customize(obj("type" to "set_tab_style", "group" to id, "style" to style))
                val bands = state().getJSONObject("workspace").getJSONObject("layout").getJSONArray("bands").toString()
                for (hidden in listOf(true, false)) {
                    compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
                    compose.waitUntil(10_000) { group("sizes").getBoolean("tabs_visible") == !hidden }
                    val layout = state().getJSONObject("workspace").getJSONObject("layout")
                    val config = layout.array("panels").objects().first { it.getString("id") == "sizes" }
                    assertEquals(hidden, config.getBoolean("hide_tab"))
                    assertFalse(group("sizes").getBoolean("floating"))
                    assertEquals("Dock dimensions remain unchanged", bands, layout.getJSONArray("bands").toString())
                    capture("workspace-docked-handle-$style-$hidden-$theme")
                }
            }
        }
    }

    @Test fun dockedToolbarHandlesRestoreSingleLanesOrNecessaryWrap() {
        for (edge in listOf("left", "right", "top", "bottom")) {
            for (style in listOf("small", "large", "labeled")) {
                action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
                customize(obj("type" to "set_tile_style", "panel" to "toolbar", "style" to style))
                action(obj("type" to "move_panel", "panel" to "toolbar", "viewport" to viewport(), "target" to obj("kind" to "edge", "edge" to edge, "outer" to true)))
                val natural = JSONObject(group("toolbar").toString())
                val oversized = JSONObject(state().getJSONObject("workspace").toString())
                val band = oversized.getJSONObject("layout").array("bands").objects().first { it.getJSONObject("root").getInt("id") == natural.getInt("id") }
                band.put("extent", band.number("extent") + 120f)
                action(obj("type" to "restore_workspace", "workspace" to oversized))
                compose.onNodeWithTag("ribbon-grip-toolbar").performTouchInput { doubleClick() }
                compose.waitUntil(10_000) { group("toolbar").getJSONObject("bounds").toString() == natural.getJSONObject("bounds").toString() }
                assertFalse(group("toolbar").getBoolean("tabs_visible"))
                assertFalse(group("toolbar").getBoolean("floating"))
                capture("workspace-docked-toolbar-reset-$edge-$style")
            }
        }
    }

    @Test fun floatingToolbarPresetsRefitTileSizesAndResetOnFirstDoubleClick() {
        floatPanel("toolbar")
        fun preset() = state().getJSONObject("workspace").getJSONObject("layout").array("floating").objects().first().getString("toolbar_layout")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (layout in listOf("compact", "vertical", "horizontal")) {
                assertEquals(layout, preset())
                for (style in listOf("small", "large", "labeled")) {
                    customize(obj("type" to "set_tile_style", "panel" to "toolbar", "style" to style))
                    assertEquals("Changing tile size keeps $layout", layout, preset())
                    val resolved = group("toolbar")
                    val tile = resolved.getJSONObject("tiles").array("tiles").getJSONObject(0)
                    assertEquals(if (style == "small") 36f else if (style == "large") 72f else 108f, tile.number("width"), .01f)
                    assertEquals(if (style == "small") 36f else 72f, tile.number("height"), .01f)
                    val grip = resolved.getJSONObject("tiles").getJSONObject("grip")
                    assertEquals(layout == "horizontal", grip.number("height") > grip.number("width"))
                    capture("workspace-$theme-$layout-$style")
                }
                compose.onNodeWithTag("ribbon-grip-toolbar").performTouchInput { doubleClick() }
                compose.waitUntil(10_000) { preset() != layout }
            }
        }
        val id = group("toolbar").getInt("id")
        val before = group("toolbar").getJSONObject("bounds").number("width")
        compose.onNodeWithTag("resize-$id-right").performTouchInput { swipe(center, center + androidx.compose.ui.geometry.Offset(60f, 0f), 400) }
        compose.waitUntil(10_000) { group("toolbar").getJSONObject("bounds").number("width") > before + 10 }
        compose.onNodeWithTag("ribbon-grip-toolbar").performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { kotlin.math.abs(group("toolbar").getJSONObject("bounds").number("width") - before) < .5f }
        assertEquals("First double-click resets instead of cycling", "compact", preset())
        compose.onNodeWithTag("ribbon-grip-toolbar").performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { preset() == "vertical" }
        capture("workspace-toolbar-first-reset")
    }

    @Test fun floatingPanelsTearOffResizeFromEverySideAndToggleHiddenTabs() {
        customize(obj("type" to "set_control_visible", "panel" to "sizes", "control" to "size_presets", "visible" to false))
        val root = compose.onNodeWithTag("workspace").fetchSemanticsNode().boundsInRoot
        val target = root.center - root.topLeft
        val source = compose.onNodeWithTag("tab-sizes")
        val start = source.fetchSemanticsNode().boundsInRoot.center - root.topLeft
        compose.onNodeWithTag("workspace").performTouchInput { down(start); moveTo(target, 16) }
        compose.waitUntil(10_000) { group("sizes").getBoolean("floating") }
        val first = JSONObject(group("sizes").getJSONObject("bounds").toString())
        compose.onNodeWithTag("workspace").performTouchInput { moveTo(target + androidx.compose.ui.geometry.Offset(60f, -30f), 300); up() }
        compose.waitUntil(10_000) { group("sizes").getJSONObject("bounds").number("x") > first.number("x") + 20 }
        assertTrue("Continued drag: $first -> ${group("sizes")}", group("sizes").getJSONObject("bounds").number("x") > first.number("x") + 20)
        val id = group("sizes").getInt("id")
        assertFalse(group("sizes").getBoolean("tabs_visible"))
        capture("workspace-live-tearoff")
        val density = compose.activity.resources.displayMetrics.density
        for (edge in listOf("left", "right", "top", "bottom", "top_left", "top_right", "bottom_left", "bottom_right")) {
            val before = JSONObject(group("sizes").getJSONObject("bounds").toString())
            val dx = if (edge.contains("left")) -20f else if (edge.contains("right")) 20f else 0f
            val dy = if (edge.contains("top")) -20f else if (edge.contains("bottom")) 20f else 0f
            compose.onNodeWithTag("resize-$id-$edge").performTouchInput { swipe(center, center + androidx.compose.ui.geometry.Offset(dx, dy) * density, 350) }
            compose.waitUntil(10_000) { group("sizes").getJSONObject("bounds").toString() != before.toString() }
            compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
            compose.waitUntil(10_000) { kotlin.math.abs(group("sizes").getJSONObject("bounds").number("width") - first.number("width")) < .5f &&
                kotlin.math.abs(group("sizes").getJSONObject("bounds").number("height") - first.number("height")) < .5f }
            assertFalse("First double-click resets $edge, not the tab", group("sizes").getBoolean("tabs_visible"))
        }
        compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { group("sizes").getBoolean("tabs_visible") }
        capture("workspace-floating-shown-tab")
        compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { !group("sizes").getBoolean("tabs_visible") }
        capture("workspace-floating-hidden-panel-before-menu")
        contextGrip("group-grip-$id")
        capture("workspace-floating-hidden-panel-menu")
        compose.onNodeWithText("Configure Brush size panel…").performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "sizes" }
        capture("workspace-hidden-panel-configure")
    }

    @Test fun zenFloatingDragOnlyMergesFloatsUntilOccupiedEdgeRevealsDocks() {
        customize(obj("type" to "set_control_visible", "panel" to "sizes", "control" to "size_presets", "visible" to false))
        floatPanel("sizes", 480f, 300f)
        floatPanel("layers", 750f, 360f)
        action(obj("type" to "invoke", "command" to "zen_mode"))
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        compose.onNodeWithTag("group-${group("sizes").getInt("id")}").assertIsDisplayed()
        val workspace = compose.onNodeWithTag("workspace")
        val root = workspace.fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        fun grip(panel: String) = compose.onNodeWithTag("group-grip-${group(panel).getInt("id")}").fetchSemanticsNode().boundsInRoot.center - root.topLeft
        val bottom = androidx.compose.ui.geometry.Offset(root.width / 2, root.height - 10 * density)
        workspace.performTouchInput { down(grip("sizes")); moveTo(bottom, 16) }
        compose.waitForIdle()
        assertTrue("An unoccupied bottom edge cannot reveal docks", host.snapshot!!.getBoolean("chrome_hidden"))
        compose.onNodeWithTag("workspace-drop-hint").assertDoesNotExist()
        capture("workspace-zen-hidden-edge")
        workspace.performTouchInput { up() }
        compose.waitForIdle()
        assertTrue(group("sizes").getBoolean("floating"))
        val target = group("layers").getJSONObject("bounds")
        val merge = androidx.compose.ui.geometry.Offset(target.number("x") + 2, target.number("y") + target.number("height") / 2) * density
        val source = grip("sizes")
        workspace.performTouchInput { down(source); moveTo(merge, 16) }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-drop-hint").fetchSemanticsNodes().isNotEmpty() }
        assertTrue("Floating merge does not reveal docks", host.snapshot!!.getBoolean("chrome_hidden"))
        capture("workspace-zen-floating-merge")
        workspace.performTouchInput { up() }
        compose.waitUntil(10_000) { group("sizes").getInt("id") == group("layers").getInt("id") }
        assertEquals(2, group("layers").array("panels").length())
        assertTrue(group("layers").getBoolean("tabs_visible"))
        val start = grip("layers")
        val left = androidx.compose.ui.geometry.Offset(10 * density, root.height / 2)
        workspace.performTouchInput { down(start); moveTo(left, 16) }
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        workspace.performTouchInput { moveTo(root.center - root.topLeft, 16) }
        compose.waitForIdle()
        assertFalse("Edge reveal lasts through the drag", host.snapshot!!.getBoolean("chrome_hidden"))
        capture("workspace-zen-revealed-drag")
        workspace.performTouchInput { up() }
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        assertTrue("Dropping in the center stays floating", group("layers").getBoolean("floating"))
        assertNull(host.actionError)
        action(obj("type" to "invoke", "command" to "zen_mode"))
    }

    @Test fun toolbarConfigurationAndGroupCollapseUseTheSharedDefault() {
        floatPanel("toolbar")
        compose.onNodeWithTag("ribbon-grip-toolbar").performTouchInput { doubleClick() }
        customize(obj("type" to "set_tile_style", "panel" to "toolbar", "style" to "large"))
        val id = group("toolbar").getInt("id")
        action(obj("type" to "move_panel", "panel" to "layers", "target" to obj("kind" to "tab", "group" to id), "viewport" to viewport()))
        assertTrue(group("toolbar").getBoolean("tabs_visible"))
        compose.onNodeWithTag("tab-toolbar").performClick()
        compose.onNodeWithTag("tab-toolbar").performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "toolbar" }
        capture("workspace-toolbar-configure")
        compose.onNodeWithText("Labeled Tiles").performScrollTo().performClick()
        compose.waitUntil(10_000) { host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("tile_style") == "labeled" }
        capture("workspace-toolbar-configure-labeled")
        customize(obj("type" to "close_expanded"))
        customize(obj("type" to "set_panel_visible", "panel" to "layers", "visible" to false))
        val toolbar = group("toolbar")
        assertFalse(toolbar.getBoolean("tabs_visible"))
        assertEquals("compact", state().getJSONObject("workspace").getJSONObject("layout").array("floating").objects().first().getString("toolbar_layout"))
        val tiles = toolbar.getJSONObject("tiles").array("tiles").objects()
        assertEquals(2, tiles.count { it.number("y") == tiles[0].number("y") })
        capture("workspace-toolbar-collapse")
        contextGrip("ribbon-grip-toolbar")
        compose.onNodeWithText("Icons only").assertDoesNotExist()
        compose.onNodeWithText("Configure Tools toolbar…").performClick()
        customize(obj("type" to "close_expanded"))
        val tabIcon = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("icon")
        assertEquals("brush", tabIcon)
        // Workspace's New Toolbar command opens the same picker as the context menu.
        workspaceMenu(); compose.onNodeWithText("New Toolbar…").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") != null }
        capture("workspace-new-toolbar")
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") == null }
        assertNull(host.actionError)
    }

    @Test fun narrowRibbonMergesTabsAndTopDockHintIsBelowTheAppHeader() {
        val view = viewport()
        action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "edge", "edge" to "left", "outer" to true), "viewport" to view))
        val ribbon = group("toolbar").getJSONObject("bounds")
        assertTrue("One-column dock", ribbon.number("width") < 72)
        val workspace = compose.onNodeWithTag("workspace")
        val root = workspace.fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        val start = compose.onNodeWithTag("tab-sizes").fetchSemanticsNode().boundsInRoot.center - root.topLeft
        val target = androidx.compose.ui.geometry.Offset(ribbon.number("x") + ribbon.number("width") / 2, ribbon.number("y") + ribbon.number("height") / 2) * density
        workspace.performTouchInput { down(start); moveTo(target, 16) }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-drop-hint").fetchSemanticsNodes().isNotEmpty() }
        capture("workspace-narrow-ribbon-merge")
        workspace.performTouchInput { up() }
        compose.waitUntil(10_000) { group("sizes").getInt("id") == group("toolbar").getInt("id") }
        assertTrue(group("toolbar").getBoolean("tabs_visible"))
        floatPanel("toolbar")
        val grip = compose.onNodeWithTag("ribbon-grip-toolbar").fetchSemanticsNode().boundsInRoot.center - root.topLeft
        val top = androidx.compose.ui.geometry.Offset(500 * density, 49 * density)
        workspace.performTouchInput { down(grip); moveTo(top, 16) }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-drop-hint").fetchSemanticsNodes().isNotEmpty() }
        val hint = compose.onNodeWithTag("workspace-drop-hint").fetchSemanticsNode().boundsInRoot
        assertEquals("Snap line is below the app header", 48f, (hint.top - root.top) / density, .6f)
        capture("workspace-top-edge-hint")
        workspace.performTouchInput { up() }
        compose.waitUntil(10_000) { !group("toolbar").getBoolean("floating") }
        assertEquals("horizontal", group("toolbar").getString("axis"))
        assertNull(host.actionError)
    }

    @Test fun zenMouseCanMoveFromCanvasOntoRevealedPanel() {
        action(obj("type" to "invoke", "command" to "zen_mode"))
        val workspace = compose.onNodeWithTag("workspace")
        workspace.performMouseInput { moveTo(center) }
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        workspace.performMouseInput { moveTo(androidx.compose.ui.geometry.Offset(1f, center.y)) }
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        val root = workspace.fetchSemanticsNode().boundsInRoot
        val tab = compose.onNodeWithTag("tab-brushes").fetchSemanticsNode().boundsInRoot.center - root.topLeft
        workspace.performMouseInput { moveTo(tab) }
        compose.waitForIdle()
        assertFalse("Hovering a revealed tab keeps its panel visible", host.snapshot!!.getBoolean("chrome_hidden"))
        capture("workspace-zen-mouse-on-panel")
        action(obj("type" to "invoke", "command" to "zen_mode"))
    }

    @Test fun stylusDrawsAndUndoRedoChangePixels() {
        penStroke()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        assertNull(host.failure)
        val painted = capture("01-stylus-light")
        val dark = darkPixels(painted)
        assertTrue("Stroke deposits visible pixels in the canvas, not just cursor state ($dark)", dark > 100)
        compose.onNodeWithContentDescription("Undo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "redo" && c.getBoolean("enabled") } }
        val undone = darkPixels(capture("02-undo"))
        assertTrue("Undo removes deposited pixels ($undone vs $dark)", undone < dark / 10)
        compose.onNodeWithContentDescription("Redo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        assertTrue("Redo restores deposited pixels", darkPixels(capture("03-redo")) >= dark * 9 / 10)
    }
    @Test fun measureHighRateStylusIngressAndRenderScheduling() {
        val options = InstrumentationRegistry.getArguments()
        val brush = options.getString("capyBrush", "G-Pen")!!
        val diameter = options.getString("capyBrushSize", "18")!!.toFloat()
        val preset = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }
            .first { it.getString("label") == brush }.getInt("id")
        // Warm pipelines and provide existing pigment for destination-aware tools.
        penStroke(60)
        compose.runOnIdle {
            host.dispatch(obj("type" to "select_brush", "id" to preset))
            host.dispatch(obj("type" to "set_brush_size", "value" to diameter))
        }
        waitState { it.getJSONObject("brush").getInt("preset") == preset && it.getJSONObject("brush").number("diameter") == diameter }
        penStroke(60)
        val cleared = CountDownLatch(1)
        host.measurements(true) { cleared.countDown() }
        assertTrue(cleared.await(10, TimeUnit.SECONDS))
        shell("dumpsys SurfaceFlinger --latency-clear")
        penStroke(600, synchronous = false)
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        val collected = CountDownLatch(1)
        var report: JSONObject? = null
        host.measurements { report = it; collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        val data = report!!
        data.put("brush", brush).put("diameter", diameter)
        val layers = shell("dumpsys SurfaceFlinger --list").lineSequence().filter {
            it.contains("SurfaceView[art.capycanvas/art.capycanvas.MainActivity](BLAST)")
        }.map { it.substringAfter("RequestedLayerState{").substringBefore(" parentId=").removeSuffix("}") }.toList()
        // UiAutomation tokenizes arguments directly, not through a shell; these
        // app-owned names contain no whitespace and must not include quotes.
        val samples = layers.associateWith { shell("dumpsys SurfaceFlinger --latency $it") }
        data.put("surface_layers", JSONObject(samples))
        val compositor = samples.values.maxByOrNull { it.length } ?: ""
        data.put("surface_flinger", compositor)
        assertTrue("Input stream reached the native host", data.array("inputs").length() > 100)
        assertTrue("Renderer produced continuous frames", data.array("frames").length() > 100)
        assertTrue("Input arrays are reused, not allocated for every event",
            data.getLong("pointer_allocations") < data.array("inputs").length() / 2)
        assertTrue("Unchanged drawing state does not build and serialize UI snapshots",
            data.getLong("snapshots_published") < data.getLong("snapshot_attempts") / 2)
        val rows = data.array("frames").values().map { it as org.json.JSONArray }
        val input = data.array("inputs").values().map { it as org.json.JSONArray }
        fun summary(values: List<Double>): JSONObject {
            val sorted = values.sorted()
            fun p(q: Double) = sorted[((sorted.size - 1) * q).toInt()]
            return obj("count" to sorted.size, "p50_ms" to p(0.5), "p95_ms" to p(0.95), "p99_ms" to p(0.99), "max_ms" to sorted.last())
        }
        val summary = obj("cpu_render_present" to summary(rows.map { it.getDouble(2) / 1e6 }),
            "cpu_callback" to summary(rows.map { it.getDouble(10) / 1e6 }),
            "publish_schedule" to summary(rows.map { it.getDouble(9) / 1e6 }),
            "cpu_paint" to summary(rows.map { it.getDouble(4) / 1e6 }),
            "surface_acquire" to summary(rows.map { it.getDouble(5) / 1e6 }),
            "cpu_viewport" to summary(rows.map { it.getDouble(6) / 1e6 }),
            "queue_present" to summary(rows.map { it.getDouble(7) / 1e6 }),
            "cpu_poll" to summary(rows.map { it.getDouble(8) / 1e6 }),
            "frame_interval" to summary(rows.zipWithNext { a, b -> (b.getDouble(0) - a.getDouble(0)) / 1e6 }),
            "input_delivery" to summary(input.map { (it.getDouble(1) - it.getDouble(0)) / 1e6 }),
            "input_queue" to summary(input.map { (it.getDouble(2) - it.getDouble(1)) / 1e6 }),
            "cpu_input" to summary(input.map { it.getDouble(3) / 1e6 }))
        val presented = compositor.lineSequence().drop(1).mapNotNull { line ->
            line.trim().split(Regex("\\s+")).getOrNull(1)?.toLongOrNull()?.takeIf { it > 0 && it < Long.MAX_VALUE }
        }.toList().distinct().sorted()
        if (presented.size > 2) {
            summary.put("composited_interval", summary(presented.zipWithNext { a, b -> (b - a) / 1e6 }))
            summary.put("composited_fps", (presented.size - 1) * 1e9 / (presented.last() - presented.first()))
        }
        android.util.Log.i("CapyBenchmark", summary.toString())
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, "latency.json")
            put(MediaStore.Downloads.MIME_TYPE, "application/json")
            put(MediaStore.Downloads.RELATIVE_PATH, "Download/CapyCanvasValidation/$runId")
            put(MediaStore.Downloads.IS_PENDING, 1)
        })!!
        data.put("summary", summary)
        resolver.openOutputStream(uri)!!.bufferedWriter().use { it.write(data.toString()) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
        capture("11-high-rate-stylus")
    }
    @Test fun zenModesIconsAndContextMenuUseSharedSettings() {
        val saved = JSONObject(state().getJSONObject("settings").toString())
        fun edit(id: String, value: Any) = action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to id, "value" to value)))
        try {
            edit("total_zen", false)
            floatPanel("sizes")
            val floating = group("sizes").getInt("id")
            for (theme in listOf("dark", "light")) {
                action(obj("type" to "set_theme", "theme" to theme))
                contextGrip("zen-button")
                capture("zen-menu-$theme")
                compose.onNodeWithText("Change icon…").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences")?.optString("reveal") == "zen_icon" }
                compose.onNodeWithTag("image-choice-0").assertIsDisplayed()
                compose.onNodeWithTag("zen-button").assertDoesNotExist()
                val first = compose.onNodeWithTag("image-choice-0").fetchSemanticsNode().boundsInRoot
                val last = compose.onNodeWithTag("image-choice-3").fetchSemanticsNode().boundsInRoot
                val row = compose.onNodeWithTag("preference-zen_icon").fetchSemanticsNode().boundsInRoot
                val density = compose.activity.resources.displayMetrics.density
                assertEquals(64 * density, first.width, 1f)
                assertEquals(first.top, last.top, 1f)
                assertEquals((row.left + row.right) / 2, (first.left + last.right) / 2, 1f)
                for ((index, name) in listOf("looking-up", "facing-forward", "bathing", "sleeping").withIndex()) {
                    compose.onNodeWithTag("image-choice-$index").performClick()
                    waitState { it.array("commands").objects().first { it.getString("id") == "zen_mode" }.getString("icon") == "zen-$name" }
                    compose.onNodeWithTag("image-choice-$index").assertIsSelected()
                    capture("zen-icons-$theme-$index")
                }
                compose.onNodeWithTag("settings-done").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
                edit("total_zen", false)
                compose.onNodeWithTag("zen-button").performTouchInput { click() }
                compose.waitUntil(10_000) { host.snapshot!!.optBoolean("hide_floating_panels") }
                compose.onNodeWithTag("zen-button").assertIsDisplayed()
                compose.onNodeWithTag("group-$floating").assertDoesNotExist()
                compose.onNodeWithContentDescription("Settings").assertDoesNotExist()
                capture("zen-button-only-$theme")
                contextGrip("zen-button")
                assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
                compose.onNodeWithText("Change icon…").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
                compose.onNodeWithTag("image-choice-3").assertIsDisplayed()
                compose.onNodeWithTag("settings-done").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
                edit("total_zen", true)
                compose.onNodeWithTag("zen-button").assertDoesNotExist()
                edit("total_zen", false)
                compose.onNodeWithTag("zen-button").performTouchInput { click() }
                compose.waitUntil(10_000) { !host.snapshot!!.optBoolean("chrome_hidden") }
                compose.onNodeWithTag("group-$floating").assertIsDisplayed()
                edit("total_zen", true)
                compose.onNodeWithTag("zen-button").performTouchInput { click() }
                compose.waitUntil(10_000) { host.snapshot!!.optBoolean("chrome_hidden") }
                compose.onNodeWithTag("group-$floating").assertIsDisplayed()
                compose.runOnIdle {
                    host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(600, 450))))
                    host.chrome(obj("kind" to "motion", "position" to JSONArray(listOf(24, 24))))
                }
                compose.waitUntil(10_000) { !host.snapshot!!.optBoolean("chrome_hidden") }
                capture("zen-active-$theme")
                compose.onNodeWithTag("zen-button").performTouchInput { click() }
                waitState { !it.getJSONObject("workspace").getBoolean("zen_mode") }
            }
        } finally {
            action(obj("type" to "close_settings"))
            action(obj("type" to "restore_settings", "settings" to saved))
        }
    }

    @Test fun preferencesSearchAndThemeAreCoreDriven() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot?.objectOrNull("preferences") != null }
        capture("04-preferences-light")
        compose.onNodeWithText("Search settings").performTextInput("prediction")
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").array("search_results").length() > 0 }
        capture("05-settings-search")
        compose.onNodeWithContentDescription("Clear search").performClick()
        compose.waitUntil(10_000) { preferences().getString("query").isEmpty() && !preferences().getBoolean("searching") }
        compose.onNodeWithText("Search settings").assertExists()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").getString("page") == "input" }
        capture("06-pen-input")
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("07-workspace-dark")
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        capture("08-preferences-dark")
    }
    @Test fun settingsAndDetailsSlideWithinOneSurface() {
        compose.mainClock.autoAdvance = false
        try {
            compose.onNodeWithContentDescription("Settings").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
            compose.mainClock.advanceTimeBy(80)
            val entering = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            compose.mainClock.advanceTimeBy(320)
            val settled = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            assertTrue("Settings slide down from above ($entering -> $settled)", entering < settled)

            compose.onNodeWithText("Keyboard Shortcuts").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "shortcuts" }
            compose.mainClock.advanceTimeBy(300)
            val shortcut = preferences().array("shortcuts").objects().first { it.getBoolean("visible") }
            compose.onNode(hasText(shortcut.getString("label")) and hasClickAction()
                and hasAnyAncestor(hasTestTag("settings-content-page:shortcuts"))).performScrollTo().performClick()
            compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
            compose.mainClock.advanceTimeBy(80)
            val tag = "settings-content-shortcut:" + shortcut.getString("id")
            val enteringDetail = compose.onNodeWithTag(tag).fetchSemanticsNode().positionInRoot.x
            compose.mainClock.advanceTimeBy(300)
            val settledDetail = compose.onNodeWithTag(tag).fetchSemanticsNode().positionInRoot.x
            assertTrue("Detail slides in from the right ($enteringDetail -> $settledDetail)", enteringDetail > settledDetail)
            compose.onAllNodes(isDialog()).assertCountEquals(0)
            compose.onAllNodes(isPopup()).assertCountEquals(0)

            compose.onNodeWithContentDescription("Back").performClick()
            compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") == null }
            compose.mainClock.advanceTimeBy(400)
            compose.onNodeWithTag("settings-content-page:shortcuts").assertIsDisplayed()
            compose.onNodeWithTag("settings-done").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
            compose.mainClock.advanceTimeBy(80)
            val leaving = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            assertTrue("Settings slide up on dismissal ($settled -> $leaving)", leaving < settled)
            compose.mainClock.advanceTimeBy(300)
            compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
        } finally { compose.mainClock.autoAdvance = true }
    }

    @Test fun settingsPanesShareTopEdgeAndUseAppScale() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        for (theme in listOf("light", "dark")) {
            compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
            waitState { it.getString("theme") == theme }
            fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val surface = bounds("preferences-surface")
            val sidebar = bounds("settings-sidebar")
            val main = bounds("settings-main-pane")
            assertEquals("Sidebar reaches the top; no global header", surface.top, sidebar.top, 1f)
            assertEquals("Sidebar reaches the bottom", surface.bottom, sidebar.bottom, 1f)
            assertEquals("Panes start at the same height", sidebar.top, main.top, 1f)
            assertEquals("Panes are adjacent", sidebar.right, main.left, 1f)
            val search = bounds("settings-search")
            compose.onNodeWithTag("settings-sidebar-title").assertDoesNotExist()
            assertEquals("Sidebar glyphs share a center line", bounds("settings-search-icon").center.x,
                bounds("settings-category-icon-appearance").center.x, 1f)
            assertTrue("Persistent search occupies the sidebar top", search.top < bounds("settings-category-appearance").top)
            compose.onNodeWithTag("settings-category-appearance").assertHeightIsEqualTo(48.dp)
            compose.onNodeWithTag("settings-category-icon-appearance", useUnmergedTree = true).assertWidthIsEqualTo(20.dp).assertHeightIsEqualTo(20.dp)
            compose.onNodeWithTag("settings-search").assertHeightIsEqualTo(48.dp)
            compose.onNodeWithTag("settings-done").assertHeightIsEqualTo(40.dp).assertTouchHeightIsEqualTo(48.dp)
            assertTrue("Done belongs to the main pane", bounds("settings-done").left >= main.left)
            val text = mutableListOf<TextLayoutResult>()
            compose.onNodeWithTag("settings-category-label-appearance", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Sidebar uses native settings body typography", 16f, text.single().layoutInput.style.fontSize.value, .01f)
            text.clear()
            compose.onNodeWithText("Done", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Done text is proportionate to its 40 dp surface", 16f, text.single().layoutInput.style.fontSize.value, .01f)
            text.clear()
            compose.onNodeWithTag("settings-group-title-Interface", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(text) }
            assertEquals("Group headings are larger than body copy", 18f, text.single().layoutInput.style.fontSize.value, .01f)
            val button = compose.onNodeWithTag("settings-done").captureToImage().toPixelMap()
            val fill = button[button.width / 2, button.height / 4]
            assertTrue("Done has a visible filled surface, not just colored text", fill.blue > fill.red + .2f)
            capture("36-settings-two-panes-$theme")
        }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun typingFromSettingsFocusesSearchWithoutLosingCharacters() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("settings-category-about").performClick()
        compose.waitForIdle()
        // Send a burst before Compose can transfer focus; Rust must retain it
        // and the field must put its caret after the complete query.
        instrumentation.runOnMainSync {
            for ((code, meta) in listOf(KeyEvent.KEYCODE_P to KeyEvent.META_SHIFT_ON, KeyEvent.KEYCODE_R to 0)) {
                for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
                    compose.activity.dispatchKeyEvent(KeyEvent(0, 0, action, code, 0, meta))
                }
            }
        }
        compose.waitUntil(10_000) { preferences().optString("query") == "Pr" }
        val search = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("settings-search")))
        search.assertTextEquals("Pr").assertIsFocused()
        search.performTextInput("essure")
        compose.waitUntil(10_000) { preferences().optString("query") == "Pressure" }
        compose.onNodeWithText("Pressure response", substring = true).assertExists()
        capture("38-type-to-search")
        compose.onNodeWithContentDescription("Clear search").performClick()
        compose.onNodeWithTag("settings-category-input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        compose.waitForIdle()
        val number = compose.onNodeWithTag("setting-number-prediction_horizon").performScrollTo()
        number.performTextReplacement("12")
        instrumentation.runOnMainSync {
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
                compose.activity.dispatchKeyEvent(KeyEvent(action, KeyEvent.KEYCODE_3))
        }
        compose.waitForIdle()
        assertEquals("Number editing stays local", "", preferences().optString("query"))
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun baseColorsAreValidatedTextAndDriveTheNativePalette() {
        compose.onNodeWithContentDescription("Settings").performClick()
        for ((theme, color) in listOf("dark" to "#1C2C3C", "light" to "#C0B49C")) {
            compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
            waitState { it.getString("theme") == theme }
            compose.onNodeWithTag("settings-category-appearance").performClick()
            val input = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-" + theme + "_base")))
            input.performScrollTo().performTextReplacement("invalid")
            input.performImeAction()
            compose.waitUntil(10_000) { !preferences().isNull("error") }
            assertNotEquals("invalid", state().getJSONObject("settings").getString(theme + "_base"))
            input.performTextReplacement(color)
            input.performImeAction()
            waitState { it.getJSONObject("settings").getString(theme + "_base") == color.lowercase() }
            assertTrue(preferences().isNull("error"))
            assertEquals(color.lowercase(), state().getJSONObject("palette").getString("bg"))
            val image = compose.onNodeWithTag("preferences-surface").captureToImage().toPixelMap()
            val expected = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("settings"))
            val pixel = image[image.width - 2, image.height - 2]
            assertEquals(android.graphics.Color.red(expected) / 255f, pixel.red, .01f)
            assertEquals(android.graphics.Color.green(expected) / 255f, pixel.green, .01f)
            assertEquals(android.graphics.Color.blue(expected) / 255f, pixel.blue, .01f)
            capture("40-custom-base-$theme")
        }
        compose.runOnIdle {
            host.preference(obj("type" to "edit", "id" to "dark_base", "value" to "#333333"))
            host.preference(obj("type" to "edit", "id" to "light_base", "value" to "#b8b8b8"))
        }
        waitState { it.getJSONObject("settings").getString("light_base") == "#b8b8b8" }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun inlineSettingsApplyValidateAndNeverPaintUnderneath() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.onNodeWithTag("preference-prediction_horizon").performScrollTo()
        compose.onNodeWithTag("settings-content-page:input").assertIsDisplayed()
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        val slider = compose.onNodeWithTag("setting-slider-pressure").performScrollTo().assertTouchHeightIsEqualTo(48.dp)
        val track = slider.captureToImage().toPixelMap()
        val trackX = track.width * 9 / 10
        assertTrue("Inactive slider track remains visible on the light settings surface",
            track[trackX, track.height / 4].red - track[trackX, track.height / 2].red > .05f)
        capture("32-inline-numbers")
        val before = state().getJSONObject("settings").number("prediction_ms")
        compose.onNodeWithTag("setting-number-prediction_horizon").performTextReplacement("1/0")
        compose.onNodeWithTag("setting-number-prediction_horizon").performImeAction()
        compose.onNodeWithText("Enter a finite number", substring = true).assertExists()
        assertEquals(before, state().getJSONObject("settings").number("prediction_ms"))
        capture("33-number-invalid")
        slider.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(width * .75f, center.y), 300) }
        compose.waitForIdle()
        waitState { it.getJSONObject("settings").number("pressure_gamma") != 1f }
        val dragged = state().getJSONObject("settings").number("pressure_gamma")
        assertTrue("A slider drag changes the value inside its range ($dragged)", dragged in .25f..4f)
        compose.onNodeWithTag("setting-number-prediction_horizon").performTextReplacement("32*2")
        compose.onNodeWithTag("setting-number-prediction_horizon").performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 64f }
        assertTrue(preferences().isNull("error"))
        compose.onNodeWithTag("setting-number-prediction_horizon").assertTextEquals("64 ms")
        compose.onNodeWithText("About").performClick()
        val collected = CountDownLatch(1)
        host.measurements(true) { collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        penStroke(15)
        val result = CountDownLatch(1)
        host.measurements {
            assertEquals("Settings surface intercepts stylus input instead of forwarding to canvas", 0, it.array("inputs").length())
            result.countDown()
        }
        assertTrue(result.await(10, TimeUnit.SECONDS))
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertEquals(64f, state().getJSONObject("settings").number("prediction_ms"))
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.onNodeWithTag("setting-number-prediction_horizon").assertTextEquals("64 ms")
        // Return this shared preference to its original accepted value.
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "prediction_horizon", "value" to before)) }
        waitState { it.getJSONObject("settings").number("prediction_ms") == before }
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun settingDefaultsResetFromContextAndEmptyCommits() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("settings-category-appearance").performClick()
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "dark_base", "value" to "#224466")) }
        waitState { it.getJSONObject("settings").getString("dark_base") == "#224466" }
        val label = compose.onNodeWithTag("preference-label-dark_base", useUnmergedTree = true).performScrollTo()
        label.performTouchInput { longClick() }
        compose.onNodeWithTag("preference-reset").assertIsEnabled()
        compose.onNodeWithText("#333333").assertExists()
        capture("36-setting-reset")
        compose.onNodeWithTag("preference-reset").performClick()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#333333" }
        label.performTouchInput { longClick() }
        compose.onNodeWithTag("preference-reset").assertIsNotEnabled()
        instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
        val text = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-dark_base")))
        text.performTextReplacement("#335577")
        text.performImeAction()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#335577" }
        text.performTextReplacement("")
        assertEquals("#335577", state().getJSONObject("settings").getString("dark_base"))
        text.performImeAction()
        waitState { it.getJSONObject("settings").getString("dark_base") == "#333333" }
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        val number = compose.onNodeWithTag("setting-number-prediction_horizon").performScrollTo()
        number.performTextReplacement("32"); number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 32f }
        number.performTextReplacement("")
        assertEquals(32f, state().getJSONObject("settings").number("prediction_ms"))
        number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 8f }
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertNull(host.actionError)
    }

    @Test fun allPreferenceRowsRenderCoreMetadataAndTrailingControls() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        // Iterate the actual core catalog: new rows using existing kinds are
        // automatically covered, with no duplicated IDs, defaults or ranges.
        for (page in preferences().array("pages").objects()) {
            if (page.array("groups").length() == 0) continue
            compose.onNodeWithTag("settings-category-" + page.getString("id")).performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == page.getString("id") }
            for (row in page.array("groups").objects().flatMap { it.array("rows").objects() }.filter { it.getBoolean("visible") }) {
                val id = row.getString("id")
                val node = compose.onNodeWithTag("preference-$id").performScrollTo()
                node.assert(hasAnyDescendant(hasText(row.getString("title"))) or hasText(row.getString("title")))
                row.optString("description").takeIf { it.isNotEmpty() }?.let {
                    compose.onNode(hasText(it) and hasAnyAncestor(hasTestTag("preference-$id")), useUnmergedTree = true).assertExists()
                }
                val kind = row.getJSONObject("kind")
                if (kind.getString("type") == "number") {
                    val label = compose.onNodeWithTag("preference-label-$id", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
                    val control = kind.getJSONObject("control")
                    val ranged = control.getString("kind") == "slider"
                    val field = compose.onNodeWithTag(if (ranged) "number-value-$id" else "setting-number-$id").assertHeightIsEqualTo(48.dp)
                    assertTrue("$id input is to the right of its description", field.fetchSemanticsNode().boundsInRoot.left > label.right)
                    if (ranged) {
                        val slider = compose.onNodeWithTag("setting-slider-$id").assertTouchHeightIsEqualTo(48.dp)
                        val bounds = slider.fetchSemanticsNode().boundsInRoot
                        assertTrue("$id slider is below all labels", bounds.top >= label.bottom)
                        val progress = slider.fetchSemanticsNode().config[SemanticsProperties.ProgressBarRangeInfo]
                        val formatted = JSONObject(Native.number(obj("control" to control, "value" to kind.number("value"), "operation" to obj("type" to "format")).toString()))
                        assertEquals(0f, progress.range.start); assertEquals(1f, progress.range.endInclusive)
                        assertEquals(formatted.number("fill"), progress.current)
                        val widthDp = bounds.width / compose.activity.resources.displayMetrics.density
                        assertTrue("$id slider uses available width with a 600dp control cap", widthDp in 150f..600f)
                    } else compose.onNodeWithTag("setting-slider-$id").assertDoesNotExist()
                }
            }
        }
        compose.onNodeWithTag("settings-category-input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        compose.onNodeWithTag("preference-feedback").performClick()
        waitState { !it.getJSONObject("settings").getBoolean("feedback") }
        compose.onNodeWithTag("setting-number-prediction_horizon").assertIsNotEnabled()
        compose.onNodeWithTag("setting-slider-tip_lock").assertIsNotEnabled()
        compose.onNodeWithTag("preference-feedback").performClick()
        waitState { it.getJSONObject("settings").getBoolean("feedback") }
        compose.onNodeWithTag("setting-number-prediction_horizon").assertIsEnabled()
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("37-inline-controls-dark")
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun panelDrawerAndDividerUseSharedLayout() {
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "brushes" }
        capture("09-brush-drawer")
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").isNull("expanded") }
        val before = host.snapshot!!.getJSONObject("layout").toString()
        val divider = host.snapshot!!.getJSONObject("layout").array("dividers").objects().first { !it.getBoolean("band") }
        val density = compose.activity.resources.displayMetrics.density
        compose.onNodeWithTag("divider-${divider.getInt("id")}").performTouchInput {
            swipe(center, center - androidx.compose.ui.geometry.Offset(0f, 70 * density), 600)
        }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").toString() != before }
        capture("10-divider-resize")
    }

    @Test fun nativeContextMenuCreatesToolbarAndToolsCanBeReordered() {
        compose.onAllNodesWithContentDescription("Move panel group").onFirst().performTouchInput { longClick() }
        capture("27-panel-menu")
        compose.onNodeWithText("New Toolbar…").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") != null }
        val picker = host.snapshot!!.getJSONObject("picker")
        val name = "Quick tools $runId"
        picker.optString("name_label").takeIf { it.isNotEmpty() }?.let { label ->
            val field = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("toolbar-name")))
            field.performTextReplacement(name)
            field.performImeAction()
        }
        val choices = picker.array("choices").objects().take(3)
        choices.forEach { choice ->
            compose.onAllNodes(hasText(choice.getString("label")) and isToggleable()).onFirst().performScrollTo().performClick()
        }
        capture("28-tool-picker")
        compose.onNodeWithText(picker.getString("confirm_label")).performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("picker") == null }
        val custom = host.snapshot!!.array("panels").objects().first { it.getString("title") == name }
        assertEquals(3, custom.array("tiles").length())
        val ids = custom.array("tiles").objects().map { it.getInt("id") }
        val source = compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[0]}")
        val target = compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[2]}").fetchSemanticsNode().boundsInRoot
        val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
        source.performTouchInput { swipe(center, target.centerRight - origin - androidx.compose.ui.geometry.Offset(2f, 0f), 700) }
        compose.waitUntil(10_000) {
            host.snapshot!!.array("panels").objects().first { it.getString("id") == custom.getString("id") }
                .array("tiles").objects().map { it.getInt("id") } != ids
        }
        capture("12-custom-toolbar")
        assertNull(host.actionError)
    }

    @Test fun editorGeometryStaysConsistent() {
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val firstTile = toolbar.array("tiles").objects().first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$firstTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithContentDescription("Zen mode").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag("number-value-Brush size").assertHeightIsEqualTo(24.dp)
        compose.onNodeWithTag("number-slider-Brush size").assertHeightIsEqualTo(24.dp)
        val numericSlider = compose.onNodeWithTag("number-slider-Brush size")
        // Visual spacing excludes Compose's expanded minimum touch targets.
        val rangeBounds = numericSlider.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val minusBounds = compose.onNodeWithContentDescription("Decrease Brush size").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val plusBounds = compose.onNodeWithContentDescription("Increase Brush size").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val gap = with(compose.density) { 6.dp.toPx() }
        assertEquals(gap, rangeBounds.left - minusBounds.right, 1f)
        assertEquals(gap, plusBounds.left - rangeBounds.right, 1f)
        numericSlider.performTouchInput { swipe(center, centerRight, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") > 1000.0 }
        numericSlider.performTouchInput { swipe(center, centerLeft, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") < 2.0 }
        val brush = state().getJSONObject("brush").getInt("preset")
        compose.onNodeWithTag("brush-preview-$brush", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        compose.onAllNodesWithContentDescription("Move panel group").onFirst().assertWidthIsEqualTo(20.dp)
        compose.onAllNodesWithText("Brushes").onFirst().assertHeightIsEqualTo(36.dp)
        compose.runOnIdle { host.invoke("fit_canvas") }
        capture("25-editor-default-light")
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("26-editor-default-dark")
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithTag("preferences-surface").assertWidthIsEqualTo(compose.activity.resources.configuration.screenWidthDp.dp)
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun compactNumberInputAndVerticalRibbonStayUsable() {
        compose.onNodeWithTag("number-value-Brush size").performClick()
        val field = compose.onNodeWithTag("number-Brush size")
        field.performTextReplacement("85/2")
        field.performImeAction()
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f }
        compose.onNodeWithContentDescription("Increase Brush size").performClick()
        val step = host.catalog.getJSONObject("brush_size").number("step")
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f + step }
        compose.onNodeWithTag("number-value-Brush size").assertTextEquals("%.1f px".format(java.util.Locale.ROOT, 42.5f + step))
        compose.onNodeWithContentDescription("Decrease Brush size").performClick()
        waitState { it.getJSONObject("brush").number("diameter") == 42.5f }

        val grip = compose.onNodeWithContentDescription("Move toolbar")
        val origin = grip.fetchSemanticsNode().boundsInRoot.topLeft
        val density = compose.activity.resources.displayMetrics.density
        grip.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(density, 200 * density) - origin, 700) }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.getString("active") == "toolbar" && it.getString("axis") == "vertical"
        } }
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val lastTile = toolbar.array("tiles").objects().last().getInt("id")
        val tile = compose.onNodeWithTag("tile-toolbar-$lastTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        assertTrue("Vertical ribbon grip is below its tools", grip.fetchSemanticsNode().boundsInRoot.top >= tile.fetchSemanticsNode().boundsInRoot.bottom)
        capture("31-vertical-ribbon")
    }

    @Test fun tabDragAppendsAndWholeGroupDragPreservesTabs() {
        fun drag(source: SemanticsNodeInteraction, target: androidx.compose.ui.geometry.Offset) {
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(center, target - origin, 700) }
        }
        val brushes = compose.onAllNodesWithText("Brushes").onFirst()
        val layers = compose.onAllNodesWithText("Layers").onFirst()
        drag(brushes, layers.fetchSemanticsNode().boundsInRoot.centerRight - androidx.compose.ui.geometry.Offset(2f, 0f))
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers"))
        } }
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.array("panels").values().contains("brushes") }
        val grip = compose.onAllNodesWithContentDescription("Move panel group").filterToOne(
            SemanticsMatcher("group grip") { node ->
                val density = compose.activity.resources.displayMetrics.density
                node.boundsInRoot.center.x > group.getJSONObject("bounds").number("x") * density
            })
        // A whole-group drop onto Sizes must preserve both tab identities.
        val sizeTitle = host.snapshot!!.array("panels").objects().first { it.getString("id") == "sizes" }.getString("title")
        val sizes = compose.onAllNodesWithText(sizeTitle).onFirst()
        drag(grip, sizes.fetchSemanticsNode().boundsInRoot.center)
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers", "sizes"))
        } }
        assertNull(host.actionError)
        capture("13-tab-group-drag")
    }

    @Test fun shortcutSearchFindsModifiedKeysAndMarksChangedBindings() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        val search = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("shortcuts-search")))
        search.performTextInput("z")
        compose.waitUntil(10_000) { preferences().getString("shortcut_query") == "z" }
        compose.waitForIdle()
        for (id in listOf("Undo", "Redo", "UndoWorkspace", "RedoWorkspace")) {
            compose.onNodeWithTag("shortcut-command.$id").performScrollTo().assertIsDisplayed()
        }
        search.performTextReplacement("Ctrl+Z")
        compose.waitUntil(10_000) { preferences().getString("shortcut_query") == "Ctrl+Z" }
        compose.waitForIdle()
        compose.onNodeWithTag("shortcut-command.Undo").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("shortcut-command.ZenMode").assertDoesNotExist()
        search.performTextReplacement("Zen mode")
        compose.waitUntil(10_000) { preferences().getString("shortcut_query") == "Zen mode" }
        val id = "command.ZenMode"
        fun preference(type: String) = action(obj("type" to "preferences", "action" to obj("type" to type, "id" to id)))
        preference("reset_shortcut")
        fun weight() : Int {
            val results = mutableListOf<TextLayoutResult>()
            compose.onNodeWithTag("shortcut-binding-$id", useUnmergedTree = true)
                .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { assertTrue(it(results)) }
            return results.single().layoutInput.style.fontWeight!!.weight
        }
        assertEquals(400, weight())
        compose.onNodeWithTag("shortcut-$id").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        compose.onNodeWithContentDescription("Remove shortcut").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == 0 }
        action(obj("type" to "preferences", "action" to obj("type" to "close_shortcut_editor")))
        assertEquals(700, weight())
        compose.onNodeWithTag("shortcut-binding-$id", useUnmergedTree = true).assertTextEquals("Disabled")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            capture("shortcuts-modified-$theme")
        }
        preference("reset_shortcut")
        assertEquals(400, weight())
        compose.onNodeWithText("Done").performClick()
    }

    @Test fun shortcutPageRecordsMultipleBindingsAndPersists() {
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search keyboard shortcuts").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        // Instrumentation can run again against an already installed app.
        if (preferences().getJSONObject("shortcut_editor").getBoolean("modified")) {
            compose.onNodeWithText("Restore default").performClick()
            compose.waitUntil(10_000) { !preferences().getJSONObject("shortcut_editor").getBoolean("modified") }
        }
        val original = preferences().getJSONObject("shortcut_editor").array("bindings").length()
        compose.onNodeWithText("Add shortcut").performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        compose.waitForIdle()
        val conflictTime = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
            assertTrue(instrumentation.uiAutomation.injectInputEvent(
                KeyEvent(conflictTime, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_E, 0), true))
        }
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").optString("conflict") == "Eraser" }
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        capture("35-shortcut-conflict-inline")
        compose.onNodeWithText("Cancel recording").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") == null }
        assertEquals(original, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithText("Add shortcut").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        compose.waitForIdle()
        val now = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
            val event = KeyEvent(now, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_J, 0, KeyEvent.META_CTRL_ON or KeyEvent.META_ALT_ON)
            assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
        }
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").objectOrNull("chord") != null }
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        capture("34-shortcut-recording-inline")
        compose.onNodeWithText("Use shortcut").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original + 1 }
        capture("14-shortcut-editor")
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.activityRule.scenario.recreate()
        compose.waitUntil(20_000) { host.snapshot!!.optBoolean("gpu_ready") }
        assertNull(host.failure)
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search keyboard shortcuts").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        assertEquals(original + 1, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithText("Restore default").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original }
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
    }

    @Test fun cameraNavigationPublishesOnlyReadoutUpdates() {
        // Warm the camera path, including any initial fit/command changes.
        var aspect = 1f
        instrumentation.runOnMainSync {
            val canvas = findCanvas(compose.activity.window.decorView)!!
            aspect = canvas.width.toFloat() / canvas.height
        }
        fun points(step: Int): List<androidx.compose.ui.geometry.Offset> {
            val angle = step * .008f
            val radius = .07f + step * .00015f
            val delta = androidx.compose.ui.geometry.Offset(kotlin.math.cos(angle) * radius, kotlin.math.sin(angle) * radius * aspect)
            val center = androidx.compose.ui.geometry.Offset(.5f + step * .0001f, .5f)
            return listOf(center - delta, center + delta)
        }
        canvasEvent(MotionEvent.ACTION_DOWN, points(0).take(1))
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(0))
        canvasEvent(MotionEvent.ACTION_MOVE, points(1))
        compose.waitForIdle()
        val reset = CountDownLatch(1)
        host.measurements(true) { reset.countDown() }
        assertTrue(reset.await(10, TimeUnit.SECONDS))
        val structuralSnapshot = host.snapshot
        val initialZoom = state().getJSONObject("camera").number("zoom")
        val initialRotation = state().getJSONObject("camera").number("rotation")
        for (i in 2..240) {
            canvasEvent(MotionEvent.ACTION_MOVE, points(i))
            SystemClock.sleep(8)
        }
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(240))
        canvasEvent(MotionEvent.ACTION_UP, points(240).take(1))
        waitState { it.getJSONObject("camera").number("zoom") > initialZoom * 1.2f }
        compose.waitForIdle()
        assertSame("Camera changes retain the structural Compose snapshot", structuralSnapshot, host.snapshot)
        assertTrue(kotlin.math.abs(state().getJSONObject("camera").number("rotation") - initialRotation) > .5f)
        val readout = host.cameraReadout
        compose.onNodeWithTag("camera-readout").assertTextEquals("${readout.zoomPercent}% · ${readout.rotationDegrees}°")
        val collected = CountDownLatch(1)
        var report: JSONObject? = null
        host.measurements { report = it; collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        val data = report!!
        assertEquals("No full UI snapshots during navigation", 0L, data.getLong("snapshots_published"))
        assertTrue("Readout stays live throughout navigation", data.getLong("camera_updates_published") > 30)
        assertTrue("Navigation produces canvas frames", data.array("frames").length() > 30)
        // Keep device-dependent timing as measurements, not flaky fps assertions.
        val frames = data.array("frames").values().map { it as JSONArray }
        val intervals = frames.zipWithNext { a, b -> (b.getDouble(0) - a.getDouble(0)) / 1e6 }.sorted()
        val callbacks = frames.map { it.getDouble(10) / 1e6 }.sorted()
        android.util.Log.i("CapyGesture", obj("frames" to frames.size,
            "full_snapshots" to data.getLong("snapshots_published"), "camera_updates" to data.getLong("camera_updates_published"),
            "interval_p50_ms" to intervals[intervals.size / 2], "interval_p99_ms" to intervals[((intervals.size - 1) * .99).toInt()],
            "callback_p50_ms" to callbacks[callbacks.size / 2], "callback_p99_ms" to callbacks[((callbacks.size - 1) * .99).toInt()]).toString())
        action(obj("type" to "set_brush_size", "value" to 42))
        assertEquals(42f, state().getJSONObject("brush").number("diameter"))
        assertNotSame("Non-camera changes still publish the full state", structuralSnapshot, host.snapshot)
        assertFalse(state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled"))
        assertNull(host.failure)
    }

    @Test fun touchNavigationHistoryCancellationAndSurfaceRecovery() {
        val a = androidx.compose.ui.geometry.Offset(0.45f, 0.5f)
        val b = androidx.compose.ui.geometry.Offset(0.55f, 0.5f)
        val initial = state().getJSONObject("camera").number("zoom")
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(a))
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(a, b))
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(a - androidx.compose.ui.geometry.Offset(0.03f, 0.02f), b + androidx.compose.ui.geometry.Offset(0.05f, 0.04f)))
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(a, b))
        canvasEvent(MotionEvent.ACTION_UP, listOf(a))
        waitState { it.getJSONObject("camera").number("zoom") > initial }
        assertFalse("Finger navigation must not deposit paint", state().array("commands").objects().first { it.getString("id") == "undo" }.getBoolean("enabled"))
        compose.runOnIdle { host.invoke("fit_canvas") }
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(a), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(b), MotionEvent.TOOL_TYPE_STYLUS, history = true)
        canvasEvent(MotionEvent.ACTION_CANCEL, listOf(b), MotionEvent.TOOL_TYPE_STYLUS)
        val completed = CountDownLatch(1)
        var sampleCount = 0L
        host.measurements { data ->
            sampleCount = data.array("inputs").values().maxOf { (it as org.json.JSONArray).getLong(4) }; completed.countDown()
        }
        assertTrue(completed.await(10, TimeUnit.SECONDS))
        assertTrue("Coalesced historical samples cross JNI together", sampleCount >= 2)
        // Backgrounding destroys SurfaceView, not the Rust document/device.
        penStroke()
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        fun cameraGeometry() = JSONObject(state().getJSONObject("camera").toString()).apply { remove("revision") }.toString()
        val camera = cameraGeometry()
        compose.activityRule.scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
        compose.activityRule.scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        compose.waitForIdle()
        assertNull(host.failure)
        assertEquals(camera, cameraGeometry())
        penStroke(20)
        assertNull(host.failure)
        capture("15-surface-recovery")
        // The emulator's natural orientation is landscape; the Wacom's is portrait.
        val originalRotation = compose.activity.window.decorView.display.rotation
        val portraitRotation = if (compose.activity.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT)
            originalRotation else (originalRotation + 1) % 4
        try {
            assertTrue(instrumentation.uiAutomation.setRotation(portraitRotation))
            compose.waitUntil(10_000) { compose.activity.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_PORTRAIT }
            val settingsLabel = state().array("commands").objects().first { it.getString("id") == "settings" }.getString("tooltip")
            // Configuration changes precede the resized Compose hierarchy.
            compose.waitUntil(10_000) { compose.onAllNodesWithContentDescription(settingsLabel).fetchSemanticsNodes().size == 1 }
            capture("16-workspace-portrait")
            compose.onNodeWithContentDescription(settingsLabel).performClick()
            capture("17-settings-portrait")
            compose.onNodeWithText("Pen & Input").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "input" }
            compose.onNodeWithText("Prediction time").assertExists()
            capture("29-settings-portrait-detail")
            if (compose.activity.resources.configuration.screenWidthDp < 840) {
                compose.onNodeWithContentDescription("Back").performClick()
            } else {
                // Large portrait tablets retain the settings sidebar.
                compose.onNodeWithContentDescription("Back").assertDoesNotExist()
            }
            compose.onNodeWithText("Canvas").assertExists()
            capture("30-settings-portrait-back")
        } finally {
            assertTrue(instrumentation.uiAutomation.setRotation(originalRotation))
        }
    }

    @Test fun menusCursorChoicesAndAboutUseCoreMetadata() {
        compose.onNodeWithText("View").performClick()
        capture("18-view-menu")
        compose.onNodeWithText("Fit canvas").performClick()
        compose.onNodeWithText("Fit canvas").assertDoesNotExist()
        compose.onNodeWithContentDescription("Settings").performClick()
        compose.onNodeWithText("Canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        capture("19-canvas-settings")
        val row = preferences().array("pages").objects().first { it.getString("id") == "canvas" }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getJSONObject("kind").getString("type") == "choice" }
        val kind = row.getJSONObject("kind")
        compose.onNodeWithTag("setting-choice-${row.getString("id")}").performScrollTo().performClick()
        capture("20-cursor-choices")
        val selection = (kind.getInt("selected") + 1) % kind.array("options").length()
        compose.onNodeWithTag("setting-choice-option-${row.getString("id")}-${selection}").performClick()
        compose.waitUntil(10_000) { preferences().array("pages").objects().first { it.getString("id") == "canvas" }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getString("id") == row.getString("id") }
            .getJSONObject("kind").getInt("selected") == selection }
        compose.onNodeWithTag("settings-content-page:canvas").assertIsDisplayed()
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onNodeWithText("About").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "about" }
        compose.onNodeWithText("capycanvas.art").assertExists()
        compose.onNodeWithText("github.com/capyatelier/capycanvas").assertExists()
        capture("21-about")
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertNull(host.actionError)
    }

    @Test fun settingChoicesStayOnPageAndDismissNatively() {
        compose.onNodeWithContentDescription("Settings").performClick()
        fun kind(page: String, id: String) = preferences().array("pages").objects().first { it.getString("id") == page }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getString("id") == id }.getJSONObject("kind")
        // The same renderer handles ordinary choices and choices with previews.
        for ((page, id) in listOf("appearance" to "theme", "canvas" to "cursor")) {
            action(obj("type" to "preferences", "action" to obj("type" to "page", "page" to page)))
            for (theme in listOf("light", "dark")) {
                action(obj("type" to "set_theme", "theme" to theme))
                val before = kind(page, id)
                val selected = before.getInt("selected")
                val control = compose.onNodeWithTag("setting-choice-$id")
                control.performScrollTo().performClick()
                compose.onAllNodes(isPopup()).assertCountEquals(1)
                compose.onAllNodes(isDialog()).assertCountEquals(0)
                compose.onNodeWithTag("settings-content-page:$page").assertIsDisplayed()
                assertNull(preferences().objectOrNull("shortcut_editor"))
                compose.onNodeWithTag("setting-choice-option-$id-$selected").assertIsSelected()
                before.array("options").values().forEachIndexed { index, label ->
                    compose.onNodeWithTag("setting-choice-option-$id-$index").assertTextContains(label.toString())
                    if (before.array("icons").optString(index).isNotEmpty()) {
                        compose.onNodeWithTag("setting-choice-icon-$id-$index", useUnmergedTree = true).assertIsDisplayed()
                    }
                }
                capture("settings-dropdown-$id-$theme")

                // Back dismisses only the menu, not its settings page.
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                compose.onAllNodes(isPopup()).assertCountEquals(0)
                assertEquals(page, preferences().getString("page"))
                assertEquals(selected, kind(page, id).getInt("selected"))

                control.performClick()
                // Tap outside the popup through the native window dispatcher.
                val outside = compose.onNodeWithTag("settings-page-title").fetchSemanticsNode().boundsInRoot.center
                val now = SystemClock.uptimeMillis()
                for (eventAction in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                    val event = MotionEvent.obtain(now, SystemClock.uptimeMillis(), eventAction, outside.x, outside.y, 0)
                    assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
                    event.recycle()
                }
                compose.onAllNodes(isPopup()).assertCountEquals(0)
                assertEquals(selected, kind(page, id).getInt("selected"))

                control.performClick()
                val next = (selected + 1) % before.array("options").length()
                compose.onNodeWithTag("setting-choice-option-$id-$next").performClick()
                compose.waitUntil(10_000) { kind(page, id).getInt("selected") == next }
                compose.onAllNodes(isPopup()).assertCountEquals(0)
                assertEquals(page, preferences().getString("page"))
                control.assertTextContains(before.array("options").getString(next))
                control.performClick()
                compose.onNodeWithTag("setting-choice-option-$id-$next").assertIsSelected()
                instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
            }
        }
        assertNull(host.actionError)
    }

    @Test fun palmDoesNotPanWhilePenDrawsAndEraserWorks() {
        val pen = androidx.compose.ui.geometry.Offset(0.45f, 0.5f)
        val palm = androidx.compose.ui.geometry.Offset(0.6f, 0.6f)
        val hover = androidx.compose.ui.geometry.Offset(0.7f, 0.3f)
        val camera = state().getJSONObject("camera").toString()
        val tools = listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f), palm + androidx.compose.ui.geometry.Offset(0.05f, 0.05f)), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        assertEquals("Palm contact must not move the camera", camera, state().getJSONObject("camera").toString())
        // The cursor is presentation, not pigment: move it out of the sampled
        // area before both captures (its size changes for the eraser).
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(hover), MotionEvent.TOOL_TYPE_STYLUS)
        val painted = darkPixels(capture("22-pen-with-palm"))
        assertTrue("Pen still deposits pigment during palm contact", painted > 100)
        compose.runOnIdle { host.dispatch(obj("type" to "set_brush_size", "value" to 64)) }
        waitState { it.getJSONObject("brush").number("diameter") == 64f }
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f)), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_ERASER)
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(hover), MotionEvent.TOOL_TYPE_ERASER)
        assertTrue("The eraser removes deposited pigment", darkPixels(capture("23-eraser")) < painted / 5)
        assertNull(host.failure)
    }

    @Test fun zenKeepsChromeThroughDrawerDismissalAndPanelDrag() {
        compose.onNodeWithContentDescription("Zen mode").performClick()
        waitState { it.getJSONObject("workspace").getBoolean("zen_mode") }
        val edge = androidx.compose.ui.geometry.Offset(0.01f, 0.01f)
        val center = androidx.compose.ui.geometry.Offset(0.7f, 0.6f)
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        compose.onAllNodesWithText("Brushes").onFirst().performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "brushes" }
        compose.waitForIdle()
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.getJSONObject("customization").isNull("expanded") }
        assertFalse("First outside contact closes only the drawer", host.snapshot!!.getBoolean("chrome_hidden"))
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        val source = compose.onAllNodesWithText("Brushes").onFirst()
        val target = compose.onAllNodesWithText("Layers").onFirst().fetchSemanticsNode().boundsInRoot.center
        val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
        source.performTouchInput { swipe(this.center, target - origin, 700) }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers"))
        } }
        assertFalse("Dropping a panel keeps its workspace visible", host.snapshot!!.getBoolean("chrome_hidden"))
        capture("24-zen-after-drag")
        compose.runOnIdle { host.invoke("zen_mode") }
    }
}
