package art.capycanvas

import android.graphics.Bitmap
import android.content.ContentValues
import android.provider.MediaStore
import android.os.SystemClock
import android.view.KeyEvent
import android.view.InputDevice
import android.view.MotionEvent
import android.view.Choreographer
import android.view.PointerIcon
import android.view.View
import androidx.compose.ui.test.*
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.snapshots.SnapshotStateObserver
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject
import org.json.JSONArray
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking

/** Real native widgets, JNI and Vulkan in the tablet emulator. No fake renderer. */
class AndroidHostTest {
    companion object {
        private val runId = System.currentTimeMillis().toString()
        // Ask an isolated, GPU-less Rust session for its defaults, not a Kotlin
        // copy of the workspace schema. Repeated runs must not collect toolbars.
        private val defaultWorkspace by lazy {
            val handle = createEnglishHostForTest()
            try { JSONObject(Native.snapshot(handle)!!).getJSONObject("state").getJSONObject("workspace").toString() }
            finally { Native.destroy(handle) }
        }
    }
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    @Before fun ready() {
        val narrow = androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("presentationNarrow") == "true"
        if (narrow) {
            device.portrait(compose.activityRule.scenario)
            assertTrue(compose.activity.resources.configuration.screenWidthDp <= 640)
            host.narrowPhotoPanels(compose)
        } else if (androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("presentationWide") == "true") {
            device.landscape(compose.activityRule.scenario)
            assertTrue(compose.activity.resources.configuration.screenWidthDp > 640)
        }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        host.awaitReady(compose = compose)
        val expected = JSONObject(defaultWorkspace)
        compose.runOnIdle {
            host.dispatch(obj("type" to "close_settings"))
            host.dispatch(obj("type" to "set_theme", "theme" to "light"))
            if (!narrow) host.dispatch(obj("type" to "restore_workspace", "workspace" to expected))
        }
        host.awaitMain("restored workspace", 10_000, { "expected=$expected actual=${state().getJSONObject("workspace")}" }, compose) {
            state().optString("theme") == "light" && (narrow || jsonValue(state().getJSONObject("workspace")) == jsonValue(expected))
        }
        compose.waitForIdle()
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
    private fun settle() { compose.waitForIdle(); SystemClock.sleep(120); compose.waitForIdle() }
    @Test fun brushSizeUpdatesInvalidateOnlyTheirReaders() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_brush_size", "value" to 32))
            val invalidated = mutableSetOf<String>()
            val changed: (String) -> Unit = { invalidated.add(it) }
            val observer = SnapshotStateObserver { callback -> compose.activity.runOnUiThread(callback) }
            try {
                compose.runOnIdle {
                    observer.start()
                    observer.observeReads("size", changed) { state().getJSONObject("brush").getDouble("diameter") }
                    observer.observeReads("opacity", changed) { state().getJSONObject("brush").getDouble("opacity") }
                    observer.observeReads("groups", changed) { state().getJSONObject("tool_set").getJSONArray("groups") }
                }
                action(obj("type" to "set_brush_size", "value" to 64))
                compose.runOnIdle {
                    assertEquals(setOf("size"), invalidated)
                    assertEquals(64.0, state().getJSONObject("brush").getDouble("diameter"), 0.0)
                }
            } finally {
                compose.runOnIdle { observer.stop(); observer.clear() }
            }
        }
    }
    @Test fun nativeSdrTaggedColorsAndGradientEditor() {
        val color = obj("space" to "DisplayP3", "rgba" to JSONArray(listOf(1.0, .01, .23, 1.0)))
        action(obj("type" to "color", "action" to obj("op" to "set_slot", "slot" to "foreground", "color" to color)))
        assertEquals("DisplayP3", state().getJSONObject("colors").getJSONObject("foreground").getString("space"))
        action(obj("type" to "set_brush_size", "value" to 64))
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(androidx.compose.ui.geometry.Offset(.4f,.4f)), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(androidx.compose.ui.geometry.Offset(.5f,.5f)), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(androidx.compose.ui.geometry.Offset(.5f,.5f)), MotionEvent.TOOL_TYPE_STYLUS)
        fun enabled(command: String) = waitState { it.array("commands").objects().first { c -> c.getString("id") == command }.getBoolean("enabled") }
        enabled("undo"); action(obj("type" to "invoke", "command" to "undo"))
        enabled("redo"); action(obj("type" to "invoke", "command" to "redo"))
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "gradient_map")))
        val view = state().getJSONObject("layer_properties")
        action(obj("type" to "effect", "action" to obj("op" to "gradient", "target" to view.array("controls").objects().first { it.getString("key")=="gradient" }.getJSONObject("gradient").getJSONObject("destination"),
            "edit" to obj("kind" to "stop", "index" to 1, "position" to 1.0, "color" to color, "remove" to false))))
        compose.onNodeWithTag("effect-gradient").assertIsDisplayed()
        compose.onNodeWithTag("effect-gradient").performTouchInput { click(androidx.compose.ui.geometry.Offset(width - 8f, height - 4f)) }
        val before = state().getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").toString()
        compose.onNodeWithTag("property-color-Color").performScrollTo().performClick()
        compose.onNodeWithText("Edit Color").assertIsDisplayed()
        compose.onNodeWithTag("color-format-0").performClick()
        compose.onNode(hasTestTag("color-form-rgb_unit") and hasAnyAncestor(isPopup())).performClick()
        compose.onNodeWithTag("color-use").performClick()
        assertEquals(before, state().getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").toString())
        assertNull(host.failure)
        assertNull(host.actionError)
        capture("native-sdr-tagged-gradient")
    }

    private fun gradientOutput():ByteArray {
            val color=runBlocking {host.withNative {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}}
            val recipe=runBlocking {ColorPreferencesStore.presets(compose.activity,color,obj("type" to "get","index" to 0))}.getJSONObject("recipe")
            val flag=Native.captureControl()
            try {return Native.inspectionOutput(runBlocking {host.withNative {Native.inspectionTask(it,flag)}},recipe.toString())[1] as ByteArray} finally {Native.captureFree(flag)}
        }
    @Test fun nativeGradientStopContactsAndCompactControlsRetainDefinition() {
        fun <T> native(block:(Long)->T):T=runBlocking {host.withNative(block)}
        host.createDrawing(128, 128, depth = "F32")
        compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            action(obj("type" to "invoke","command" to "add_layer"))
            action(obj("type" to "invoke","command" to "pen"));penStroke(8)
            compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true}
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to "gradient_map")))
            fun definition()=state().getJSONObject("layer_properties").array("controls").objects().first {it.getString("key")=="gradient"}.getJSONObject("value").getJSONObject("value").toString()
            compose.onNodeWithTag("gradient-editor").assertIsDisplayed()
            compose.onNodeWithTag("effect-gradient").performTouchInput {click(center)}
            waitState {it.getJSONObject("layer_properties").array("controls").objects().first {c->c.getString("key")=="gradient"}.getJSONObject("value").getJSONObject("value").getJSONArray("stops").length()==3}
            fun stopContact(tool:Int,cancel:Boolean=false) {
                val bounds=compose.onNodeWithTag("effect-gradient").fetchSemanticsNode().boundsInRoot
                val source=when(tool){MotionEvent.TOOL_TYPE_STYLUS->InputDevice.SOURCE_STYLUS;MotionEvent.TOOL_TYPE_MOUSE->InputDevice.SOURCE_MOUSE;else->InputDevice.SOURCE_TOUCHSCREEN}
                val location=IntArray(2);instrumentation.runOnMainSync {compose.activity.window.decorView.getLocationOnScreen(location)}
                val down=SystemClock.uptimeMillis()
                val stops=JSONObject(definition()).getJSONArray("stops");val start=stops.getJSONObject(1).getDouble("position").toFloat()
                for((phase,offset) in listOf(MotionEvent.ACTION_DOWN to 0f,MotionEvent.ACTION_MOVE to .04f,(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP) to .04f)) {
                    val point=MotionEvent.PointerCoords().apply {x=location[0]+bounds.left+6*compose.activity.resources.displayMetrics.density+(bounds.width-12*compose.activity.resources.displayMetrics.density)*(start+offset);y=location[1]+bounds.center.y;pressure=.7f}
                    val properties=MotionEvent.PointerProperties().apply {id=0;toolType=tool}
                    val event=MotionEvent.obtain(down,SystemClock.uptimeMillis(),phase,1,arrayOf(properties),arrayOf(point),0,if(tool==MotionEvent.TOOL_TYPE_MOUSE)MotionEvent.BUTTON_PRIMARY else 0,1f,1f,0,0,source,0)
                    try {assertTrue(instrumentation.uiAutomation.injectInputEvent(event,true))} finally {event.recycle()}
                    SystemClock.sleep(30)
                }
                compose.waitForIdle()
            }
            for(tool in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS,MotionEvent.TOOL_TYPE_MOUSE)) {
                val before=definition();stopContact(tool,true);assertEquals(before,definition())
                stopContact(tool);val dragged=definition();assertNotEquals(before,dragged)
                action(obj("type" to "invoke","command" to "undo"));assertEquals(before,definition())
                action(obj("type" to "invoke","command" to "redo"));assertEquals(dragged,definition())
            }
            compose.onNodeWithTag("number-value-gradient-position").assertIsEnabled().performClick()
            compose.waitUntil(5_000) {compose.onAllNodesWithTag("number-Position").fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("number-Position").performTextReplacement("62.5%")
            compose.onNodeWithTag("number-Position").performImeAction()
            waitState {JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getDouble("position")==.625}
            compose.onNodeWithTag("number-value-gradient-position").assertIsEnabled()
            compose.onNodeWithTag("gradient-reverse").performClick()
            compose.onNodeWithTag("number-value-gradient-position").assertIsEnabled()
            assertEquals(.375,JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getDouble("position"),0.0)
            compose.onNodeWithTag("gradient-interpolation").assertIsDisplayed()
            val modes=state().getJSONObject("layer_properties").array("controls").objects().first {it.getString("key")=="gradient"}.getJSONObject("gradient").getJSONArray("interpolations")
            assertEquals(3,modes.length())
            for(choice in (0 until modes.length()).map {modes.getJSONArray(it)}) {
                compose.onNodeWithTag("gradient-interpolation").performClick()
                compose.onNode(hasText(choice.getString(1)) and hasAnyAncestor(isPopup())).performClick()
                waitState {JSONObject(definition()).getString("interpolation")==choice.getString(0)}
                assertEquals(choice.getString(0),JSONObject(definition()).getString("interpolation"))
                compose.onNodeWithTag("number-value-gradient-position").assertIsEnabled()
            }
            fun focusStop()=compose.onNodeWithTag("effect-gradient").performTouchInput {
                val position=JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getDouble("position").toFloat()
                val inset=6*compose.activity.resources.displayMetrics.density
                click(androidx.compose.ui.geometry.Offset(inset+(width-2*inset)*position,height*.85f))
            }
            fun keyboardIdle(phase:String,beforeRelease:String=definition()) {
                val file=File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-keyboard-$theme.json")
                fun status()=native {JSONObject(Native.toneStatus(it))}
                file.writeText(obj("phase" to phase,"before" to status(),"definition" to definition()).toString())
                try {compose.waitUntil(5_000) {status().getBoolean("idle")}}
                finally {file.writeText(obj("phase" to phase,"before_release" to JSONObject(beforeRelease),"after" to status(),"definition" to definition()).toString())}
            }
            focusStop();val keyBefore=definition()
            fun arrow(phase:Int,repeat:Int=0) {val now=SystemClock.uptimeMillis();instrumentation.sendKeySync(KeyEvent(now,now,phase,KeyEvent.KEYCODE_DPAD_RIGHT,repeat))}
            arrow(KeyEvent.ACTION_DOWN);arrow(KeyEvent.ACTION_DOWN,1)
            waitState {definition()!=keyBefore}
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE);arrow(KeyEvent.ACTION_UP);keyboardIdle("cancel")
            waitState {definition()==keyBefore}
            focusStop();arrow(KeyEvent.ACTION_DOWN);arrow(KeyEvent.ACTION_DOWN,1);arrow(KeyEvent.ACTION_DOWN,2)
            waitState {JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getDouble("position")>.4}
            val beforeRelease=definition();arrow(KeyEvent.ACTION_UP);keyboardIdle("up",beforeRelease);waitState {JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getDouble("position")>.4};val keyAfter=definition()
            action(obj("type" to "invoke","command" to "undo"));assertEquals(keyBefore,definition())
            action(obj("type" to "invoke","command" to "redo"));assertEquals(keyAfter,definition())
            compose.onNodeWithTag("property-color-Color").performClick()
            val alpha=JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getJSONObject("color").getJSONArray("rgba").getDouble(3)
            compose.onNodeWithTag("color-format-0").performClick()
            compose.onNode(hasTestTag("color-form-linear_rgb") and hasAnyAncestor(isPopup())).performClick()
            for((name,text) in listOf("0-0" to "1","0-1" to "0","0-2" to "0","ev" to "2")) {
                compose.onNodeWithTag("color-value-$name").performClick()
                compose.onNodeWithTag("color-value-$name-input").performTextReplacement(text)
                compose.onNodeWithTag("color-value-$name-input").performImeAction()
                compose.waitForIdle()
            }
            compose.onNodeWithTag("color-use").assertIsEnabled().performClick()
            compose.waitUntil(5_000) {compose.onAllNodesWithTag("color-use").fetchSemanticsNodes().isEmpty()}
            val tagged=JSONObject(definition()).getJSONArray("stops").getJSONObject(1).getJSONObject("color")
            assertTrue(tagged.getJSONArray("rgba").getDouble(0)>1.0)
            assertEquals(alpha,tagged.getJSONArray("rgba").getDouble(3),1e-6)
            val committed=definition();val expectedPixels=gradientOutput()
            val archive=File(device.root,"p30-gradient-$theme.capy")
            host.writeDrawingCopy(archive)
            compose.runOnUiThread {assertTrue(host.documents.openUris(listOf(android.net.Uri.fromFile(archive))))}
            compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true&&state().getJSONObject("document_file").optString("location").contains(archive.name)}
            fun publishedGradient(phase:String) {
                val file=File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-$phase-$theme.json")
                val before=JSONObject(host.snapshot!!.toString())
                file.writeText(obj("phase" to phase,"before" to before).toString())
                try {waitState {it.getJSONObject("layer_properties").array("controls").objects().any {c->c.getString("key")=="gradient"}}}
                catch(error:Throwable) {capture("p30-reopen-failure-$theme");throw error}
                finally {file.writeText(obj("phase" to phase,"before" to before,"after" to host.snapshot).toString())}
            }
            fun selectGradient(id:Long,phase:String) {
                val file=File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-selection-$phase-$theme.json")
                val before=obj("blocked" to host.documentInputBlocked,"switching" to host.drawingTabs.switching,"snapshot" to host.snapshot)
                file.writeText(before.toString())
                compose.waitUntil(10_000) {!host.documentInputBlocked&&!host.drawingTabs.switching}
                action(obj("type" to "select_layer","id" to id))
                try {waitState {it.array("layers").objects().any {row->row.getLong("id")==id&&row.getBoolean("selected")}}}
                finally {file.writeText(obj("before" to before,"blocked" to host.documentInputBlocked,"switching" to host.drawingTabs.switching,"after" to host.snapshot).toString())}
            }
            val label=state().array("adjustments").objects().first {it.getString("id")=="gradient_map"}.getString("label")
            val reopenedLayer=state().array("layers").objects().first {it.optString("label")==label}.getLong("id")
            selectGradient(reopenedLayer,"reopen")
            publishedGradient("reopen")
            assertEquals(committed,definition());assertArrayEquals(expectedPixels,gradientOutput())
            compose.activityRule.scenario.recreate();compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true}
            selectGradient(state().array("layers").objects().first {it.optString("label")==label}.getLong("id"),"recreate")
            publishedGradient("recreate")
            assertEquals(committed,definition());assertArrayEquals(expectedPixels,gradientOutput())
            compose.onNodeWithTag("property-color-Color").performClick()
            compose.onNodeWithText("Edit Color").assertIsDisplayed()
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to "gradient_map")))
            val replacement=definition()
            compose.onAllNodesWithText("Use Color").fetchSemanticsNodes().firstOrNull()?.let {compose.onNodeWithText("Use Color").performClick()}
            assertEquals(replacement,definition());assertNotEquals(committed,replacement)
            compose.activityRule.scenario.recreate()
            waitState {it.getJSONObject("layer_properties").array("controls").objects().any {c->c.getString("key")=="gradient"}}
            assertEquals(replacement,definition());assertNull(host.failure);assertNull(host.actionError)
            capture("p30-gradient-$theme")
        }
    }

    @Test fun gradientToolPanelAndToolbarUseNativeGeometryContacts() {
        for(theme in listOf("light","dark")) {
            action(obj("type" to "restore_workspace","workspace" to JSONObject(defaultWorkspace)))
            action(obj("type" to "set_theme","theme" to theme))
            val commands=state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first {it.getString("id")=="commands"}
            commands.getJSONObject("content").array("tiles").objects().forEach {customize(obj("type" to "remove_tool","panel" to "commands","tile" to it.getInt("id")))}
            customize(obj("type" to "insert_tools","panel" to "commands","before" to null))
            customize(obj("type" to "picker_select","control" to obj("kind" to "tool_options","style" to obj("text" to true,"sliders" to true)),"selected" to true))
            customize(obj("type" to "confirm_tools"))
            assertEquals("tool_options",state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first {it.getString("id")=="commands"}.getJSONObject("content").array("tiles").getJSONObject(0).getJSONObject("control").getString("kind"))
            action(obj("type" to "move_panel","panel" to "commands","viewport" to viewport(),"target" to obj("kind" to "edge","edge" to "top","outer" to true)))
            action(obj("type" to "invoke","command" to "add_layer"))
            action(obj("type" to "invoke","command" to "gradient"))
            customize(obj("type" to "set_panel_visible","panel" to "tool_settings","visible" to true))
            floatPanel("tool_settings",20f,140f)
            action(obj("type" to "select_panel_tab","group" to group("tool_settings").getLong("id"),"panel" to "tool_settings"))
            fun panelNode(tag:String)=compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("panel-body-tool_settings")),useUnmergedTree=true)
            panelNode("tool-segments-gradient-shape").performScrollTo().assertIsDisplayed()
            panelNode("gradient-editor").assertIsDisplayed()
            val toolbarTags=compose.onAllNodes(SemanticsMatcher("Toolbar tag") {it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("toolbar-")==true},useUnmergedTree=true).fetchSemanticsNodes()
            File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-toolbar-$theme.json").writeText(obj("state" to state(),"tags" to JSONArray(toolbarTags.map {node->obj("tag" to node.config.getOrNull(SemanticsProperties.TestTag),"bounds" to node.boundsInRoot.toString())})).toString())
            compose.onNode(hasTestTag("toolbar-gradient") and hasAnyAncestor(hasTestTag("panel-body-commands")),useUnmergedTree=true).assertIsDisplayed().performClick()
            compose.onNode(hasTestTag("gradient-editor") and hasAnyAncestor(isPopup())).assertIsDisplayed()
            compose.onNode(hasTestTag("effect-gradient") and hasAnyAncestor(isPopup())).performTouchInput {click(center)}
            waitState {it.array("tool_extra").objects().firstOrNull {option->option.has("Gradient")}?.getJSONObject("Gradient")?.getJSONObject("value")?.getJSONObject("value")?.getJSONArray("stops")?.length()==3}
            fun gradientDefinition() = state().array("tool_extra").objects().single { it.has("Gradient") }.getJSONObject("Gradient").getJSONObject("value").toString()
            val toolBefore=gradientDefinition()
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            compose.waitUntil(5_000) {compose.onAllNodes(hasTestTag("gradient-editor") and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isEmpty()}
            assertEquals(toolBefore,gradientDefinition())
            action(obj("type" to "invoke","command" to "fit_canvas"))
            val shapeNodes=compose.onAllNodes(SemanticsMatcher("Gradient shape tag") {it.config.getOrNull(SemanticsProperties.TestTag)?.contains("gradient-shape")==true},useUnmergedTree=true).fetchSemanticsNodes()
            File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-shape-$theme.json").writeText(obj("tool_extra" to state().getJSONArray("tool_extra"),"tags" to JSONArray(shapeNodes.map {node->obj("tag" to node.config.getOrNull(SemanticsProperties.TestTag),"bounds" to node.boundsInRoot.toString())})).toString())
            fun output(phase:String):ByteArray {
                val file=File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-contact-$theme.json")
                fun published()=runBlocking {host.withNative {obj("snapshot" to (Native.snapshot(it)?.let(::JSONObject) ?: host.snapshot!!),"tone" to JSONObject(Native.toneStatus(it)))}}
                file.writeText(obj("phase" to phase,"before" to published()).toString())
                runBlocking {host.glassPresented()}
                try {
                    compose.waitUntil(5_000) {
                        published().getJSONObject("tone").getBoolean("idle") && runBlocking { host.withNative { !Native.renderingPending(it) } }
                    }
                    file.writeText(obj("phase" to phase,"retired" to published()).toString())
                    return gradientOutput()
                } catch(error:Throwable) {
                    file.writeText(obj("phase" to phase,"failed" to published(),"error" to error.toString()).toString())
                    throw error
                }
            }
            for(index in 0..2) {
                panelNode("tool-setting-grain_depth").performScrollTo().assertIsDisplayed()
                action(obj("type" to "set_tool_setting", "id" to "grain_depth", "value" to index.toDouble()))
                assertEquals(index.toDouble(), state().array("tool_settings").objects().single { it.getString("id") == "grain_depth" }.getDouble("value"), 0.0)
                panelNode("tool-segment-gradient-shape-$index").performScrollTo().performClick()
                for(tool in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS,MotionEvent.TOOL_TYPE_MOUSE)) {
                action(obj("type" to "invoke","command" to "fit_canvas"))
                val camera=state().getJSONObject("camera");val viewport=camera.getJSONArray("viewport");val area=camera.getJSONArray("work_area")
                fun point(x:Double)=androidx.compose.ui.geometry.Offset(((area.getDouble(0)+area.getDouble(2)*x)/viewport.getDouble(0)).toFloat(),((area.getDouble(1)+area.getDouble(3)*.5)/viewport.getDouble(1)).toFloat())
                val before=output("$index/$tool/before")
                canvasEvent(MotionEvent.ACTION_DOWN,listOf(point(.35)),tool);canvasEvent(MotionEvent.ACTION_MOVE,listOf(point(if(tool==MotionEvent.TOOL_TYPE_MOUSE).75 else .65)),tool);canvasEvent(MotionEvent.ACTION_CANCEL,listOf(point(if(tool==MotionEvent.TOOL_TYPE_MOUSE).75 else .65)),tool)
                assertArrayEquals(before,output("$index/$tool/cancel"))
                canvasEvent(MotionEvent.ACTION_DOWN,listOf(point(.35)),tool);canvasEvent(MotionEvent.ACTION_MOVE,listOf(point(if(tool==MotionEvent.TOOL_TYPE_MOUSE).75 else .65)),tool);canvasEvent(MotionEvent.ACTION_UP,listOf(point(if(tool==MotionEvent.TOOL_TYPE_MOUSE).75 else .65)),tool)
                compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true}
                val after=output("$index/$tool/up")
                if(tool==MotionEvent.TOOL_TYPE_FINGER)assertArrayEquals(before,after)
                else {
                    assertFalse(before.contentEquals(after))
                    action(obj("type" to "invoke","command" to "undo"));assertArrayEquals(before,output("$index/$tool/undo"))
                    action(obj("type" to "invoke","command" to "redo"));assertArrayEquals(after,output("$index/$tool/redo"))
                }
                assertNull(host.failure);assertNull(host.actionError)
                }
            }
            panelNode("gradient-reverse").performClick()
            capture("p30-tool-gradient-$theme")
        }
    }

    @Test fun gradientDefinitionsRetainTaggedStopsAndRejectRetiredDestinations() {
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to "gradient_map")))
            fun control()=state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="gradient" }
            fun definition()=control().getJSONObject("value").getJSONObject("value")
            val destination=control().getJSONObject("gradient").getJSONObject("destination")
            fun edit(value:JSONObject)=action(obj("type" to "effect","action" to obj("op" to "gradient","target" to control().getJSONObject("gradient").getJSONObject("destination"),"edit" to value)))
            assertEquals("effect",destination.getString("kind"))
            assertEquals("Oklab",definition().getString("interpolation"))
            assertFalse(definition().has("dither"))
            assertFalse(control().getJSONObject("gradient").has("shape"))
            assertFalse(control().getJSONObject("gradient").has("shapes"))
            edit(obj("kind" to "stop","index" to JSONObject.NULL,"position" to .375,"color" to obj("space" to "DisplayP3","rgba" to JSONArray(listOf(2.0,.125,.5,.4))),"remove" to false))
            assertEquals(3,definition().getJSONArray("stops").length())
            val stop=definition().getJSONArray("stops").getJSONObject(1)
            assertEquals(.375,stop.getDouble("position"),0.0)
            assertEquals("DisplayP3",stop.getJSONObject("color").getString("space"))
            assertEquals(2.0,stop.getJSONObject("color").getJSONArray("rgba").getDouble(0),0.0)
            assertEquals(.4,stop.getJSONObject("color").getJSONArray("rgba").getDouble(3),1e-6)
            for(space in listOf("Oklab","LinearRgb","Classic")) {
                edit(obj("kind" to "interpolation","value" to space))
                assertEquals(space,definition().getString("interpolation"))
            }
            val before=definition().toString()
            edit(obj("kind" to "position","index" to 1,"operation" to obj("type" to "expression","text" to "62.5%")))
            assertEquals(.625,definition().getJSONArray("stops").getJSONObject(1).getDouble("position"),0.0)
            action(obj("type" to "invoke","command" to "undo"));assertEquals(before,definition().toString())
            action(obj("type" to "invoke","command" to "redo"));assertEquals(.625,definition().getJSONArray("stops").getJSONObject(1).getDouble("position"),0.0)
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to "gradient_map")))
            val replacement=definition().toString()
            action(obj("type" to "effect","action" to obj("op" to "gradient","target" to destination,"edit" to obj("kind" to "interpolation","value" to "Classic"))));assertEquals(replacement,definition().toString())
            assertNull(host.failure);assertNull(host.actionError)
        }
    }

    @Test fun opaqueFilterColorsHideAlphaInBothThemes() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "black_white")))
            val layer = state().getJSONObject("layer_properties").getLong("layer")
            val color = obj("space" to "DisplayP3", "rgba" to JSONArray(listOf(.8, .2, .1, .25)))
            action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to "tint_color", "value" to obj("kind" to "color", "value" to color))))
            val control = state().getJSONObject("layer_properties").array("controls").objects().single { it.getString("key") == "tint_color" }
            assertTrue(control.getJSONObject("kind").getBoolean("opaque"))
            assertEquals(1.0, control.getJSONObject("value").getJSONObject("value").getJSONArray("rgba").getDouble(3), 0.0)
            compose.onNodeWithTag("property-color-Tint color").performScrollTo().performClick()
            compose.onNodeWithText("Edit Color").assertIsDisplayed()
            compose.onNodeWithTag("color-input-3").assertDoesNotExist()
            compose.onNodeWithText("Use Color").performClick()
            assertNull(host.failure)
            assertNull(host.actionError)
        }
    }

    @Test fun completedDropFeedbackSurvivesNewerMotion() {
        val dock = DockInteraction(host)
        val source = group("brushes").getJSONObject("bounds")
        val destination = group("layers")
        val target = destination.getJSONObject("bounds")
        val size = viewport()
        val before = state().getJSONObject("workspace").toString()
        val ownerReached = CountDownLatch(1)
        val releaseOwner = CountDownLatch(1)
        try {
            compose.runOnIdle {
                dock.viewport = size
                dock.start(obj("type" to "drag_workspace", "item" to obj("kind" to "panel", "panel" to "brushes")),
                    androidx.compose.ui.geometry.Offset(source.number("x") + 10, source.number("y") + 10))
                dock.move(androidx.compose.ui.geometry.Offset(target.number("x") + 30, target.number("y") + 10))
                // Delay main-thread delivery until a newer move exists, then
                // hold its native result. This reproduces the response ordering
                // deterministically without relying on a slow tablet/GPU.
                CoroutineScope(Dispatchers.Main.immediate).launch {
                    host.withNative { Choreographer.getInstance().postFrameCallback {
                        ownerReached.countDown(); releaseOwner.await(10, TimeUnit.SECONDS)
                    } }
                }
                assertTrue(ownerReached.await(5, TimeUnit.SECONDS))
                dock.move(androidx.compose.ui.geometry.Offset(size.getDouble(0).toFloat() / 2, size.getDouble(1).toFloat() / 2))
            }
            compose.waitUntil(2_000) { dock.hint != null }
            assertEquals(destination.getInt("id"), dock.hint!!.getJSONObject("target").getInt("group"))
            releaseOwner.countDown()
            action(obj("type" to "close_settings"))
            assertNull("The newest result clears feedback over empty canvas", dock.hint)
        } finally {
            releaseOwner.countDown()
            compose.runOnUiThread { dock.finish(cancel = true) }
            action(obj("type" to "close_settings"))
        }
        assertNull(dock.hint)
        assertEquals(before, state().getJSONObject("workspace").toString())
    }

    @Test fun dropIndicatorsTrackContinuousMouseAndTouchMotion() {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val root = compose.onNodeWithTag("workspace")
        val owner = root.fetchSemanticsNode().root as ViewRootForTest
        val origin = root.fetchSemanticsNode().positionInRoot
        val density = compose.activity.resources.displayMetrics.density
        fun findHint(node: SemanticsNode): androidx.compose.ui.geometry.Rect? =
            if (node.config.getOrNull(SemanticsProperties.TestTag) == "workspace-drop-hint") node.boundsInRoot
            else node.children.firstNotNullOfOrNull(::findHint)
        for (mouse in listOf(true, false)) for (drawer in listOf(false, true)) for (attached in listOf(false, true)) {
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            if (drawer) {
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                compose.onNodeWithTag("column-icon-brushes").performClick()
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-grip-41").fetchSemanticsNodes().isNotEmpty() }
                SystemClock.sleep(300); compose.waitForIdle()
            }
            if (attached) action(obj("type" to "select_panel_tab", "group" to 41, "panel" to "tool_settings"))
            val before = state().getJSONObject("workspace").toString()
            val tabPrefix = if (drawer) "drawer-tab" else "tab"
            val source = bounds(if (attached) "$tabPrefix-tool_settings" else "tab-layers").center
            val targets = listOf("sizes", if (attached) "brushes" else "tool_settings").map { bounds("$tabPrefix-$it") }
            val away = root.fetchSemanticsNode().boundsInRoot.center
            val downAt = SystemClock.uptimeMillis()
            fun event(action: Int, point: androidx.compose.ui.geometry.Offset): androidx.compose.ui.geometry.Rect? {
                var hint: androidx.compose.ui.geometry.Rect? = null
                instrumentation.runOnMainSync {
                    val properties = arrayOf(MotionEvent.PointerProperties().apply {
                        id = 0; toolType = if (mouse) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
                    })
                    val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f })
                    val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords,
                        0, if (mouse && action != MotionEvent.ACTION_UP && action != MotionEvent.ACTION_CANCEL) MotionEvent.BUTTON_PRIMARY else 0,
                        1f, 1f, 0, 0, if (mouse) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_TOUCHSCREEN, 0)
                    owner.view.dispatchTouchEvent(motion); motion.recycle()
                    hint = findHint(owner.semanticsOwner.unmergedRootSemanticsNode)
                }
                return hint
            }
            event(MotionEvent.ACTION_DOWN, source)
            try {
                event(MotionEvent.ACTION_MOVE, if (attached) targets.first().center else away)
                // Keep emitting real native moves while observing the laid-out
                // marker. Waiting for Compose idle between moves hides starvation.
                for ((index, target) in targets.withIndex()) {
                    val began = SystemClock.uptimeMillis()
                    var firstVisible: Long? = null
                    var samples = 0
                    var lastHint: androidx.compose.ui.geometry.Rect? = null
                    do {
                        val point = androidx.compose.ui.geometry.Offset(target.left + (3 + samples % 4) * density, target.center.y)
                        val hint = event(MotionEvent.ACTION_MOVE, point)
                        lastHint = hint
                        if (hint != null && kotlin.math.abs(hint.center.x - target.left) <= 2 * density && firstVisible == null)
                            firstVisible = SystemClock.uptimeMillis() - began
                        samples++
                        compose.mainClock.advanceTimeByFrame()
                        SystemClock.sleep(8)
                    } while (SystemClock.uptimeMillis() - began < 600)
                    android.util.Log.i("CapyDropTest", "mouse=$mouse drawer=$drawer attached=$attached slot=$index first_visible_ms=$firstVisible samples=$samples hint=$lastHint target=$target")
                    assertNotNull("Tab insertion marker must appear during continuous motion ($mouse/$drawer/$attached/$index)", firstVisible)
                    assertTrue("Tab insertion marker took ${firstVisible}ms ($mouse/$drawer/$attached/$index)", firstVisible!! < 250)
                }
                compose.waitForIdle()
                val hint = bounds("workspace-drop-hint")
                assertEquals(3 * density, hint.width, 1f)
                assertEquals(targets.last().height, hint.height, 1f)
                val pixels = root.captureToImage().toPixelMap()
                val color = pixels[(hint.center.x - origin.x).toInt(), (hint.center.y - origin.y).toInt()]
                val accent = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("accent"))
                assertEquals("Visible accent marker", android.graphics.Color.red(accent) / 255f, color.red, .02f)
                assertEquals(android.graphics.Color.green(accent) / 255f, color.green, .02f)
                assertEquals(android.graphics.Color.blue(accent) / 255f, color.blue, .02f)
            } finally {
                event(MotionEvent.ACTION_CANCEL, away)
            }
            action(obj("type" to "close_settings")) // Drain the native owner, including cancellation.
            assertEquals(before, state().getJSONObject("workspace").toString())
            compose.onNodeWithTag("workspace-drop-hint").assertDoesNotExist()
        }
    }

    @Test fun detachedTabsUseUpdatedSourceSlots() {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to obj("kind" to "tabs", "id" to 41,
                    "panels" to JSONArray(listOf("brushes", "sizes")), "active" to "brushes", "tab_style" to "icon_name")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to obj("kind" to "tabs", "id" to 43,
                    "panels" to JSONArray(listOf("layers", "properties")), "active" to "layers", "tab_style" to "icon_name")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val workspace = compose.onNodeWithTag("workspace")
        val root = workspace.fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        fun settle() { compose.waitForIdle(); SystemClock.sleep(100); compose.waitForIdle() }
        for (mouse in listOf(true, false)) {
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            val source = bounds("tab-layers").center - root.topLeft
            val away = root.center - root.topLeft
            var pressed = true
            try {
                if (mouse) workspace.performMouseInput { moveTo(source); press(); moveTo(away, 200) }
                else workspace.performTouchInput { down(source); moveTo(away, 200) }
                settle()
                assertTrue(group("layers").getBoolean("floating"))
                val remaining = bounds("tab-properties")
                val target = androidx.compose.ui.geometry.Offset(remaining.right + 12 * density, remaining.center.y) - root.topLeft
                if (mouse) workspace.performMouseInput { moveTo(target, 200) }
                else workspace.performTouchInput { moveTo(target, 200) }
                settle()
                val marker = bounds("workspace-drop-hint")
                assertEquals("Insertion follows the remaining source tab, not its old frozen slot", remaining.right, marker.center.x, 2f)
                if (mouse) workspace.performMouseInput { release() } else workspace.performTouchInput { up() }
                pressed = false
                action(obj("type" to "close_settings"))
                assertEquals(43, group("layers").getInt("id"))
                assertEquals(listOf("properties", "layers"), group("layers").array("panels").values().map { it.toString() })
                action(obj("type" to "invoke", "command" to "undo_workspace"))
                assertEquals(listOf("layers", "properties"), group("layers").array("panels").values().map { it.toString() })
                action(obj("type" to "invoke", "command" to "redo_workspace"))
                assertEquals(listOf("properties", "layers"), group("layers").array("panels").values().map { it.toString() })
            } finally {
                if (pressed) { if (mouse) workspace.performMouseInput { cancel() } else workspace.performTouchInput { cancel() } }
            }
        }
    }

    @Test fun workspaceUsesNativeMouseAndPenCursors() {
        for (theme in listOf("light", "dark")) {
        action(obj("type" to "set_theme", "theme" to theme))
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to obj("kind" to "split", "id" to 43,
                    "axis" to "vertical", "fraction" to .5, "first" to tabs(44, "layers", "properties"), "second" to tabs(45, "adjustments"))),
                obj("id" to 46, "edge" to "top", "extent" to 42, "root" to tabs(47, "toolbar")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(48, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        action(obj("type" to "restore_workspace", "workspace" to fixture))
        val root = compose.onNodeWithTag("workspace")
        val native = (root.fetchSemanticsNode().root as ViewRootForTest).view
        val decor = compose.activity.window.decorView
        fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot.translate(-root.fetchSemanticsNode().boundsInRoot.topLeft)
        fun saved() = state().getJSONObject("workspace").toString()
        fun event(point: androidx.compose.ui.geometry.Offset, tool: Int): MotionEvent {
            val local = point + root.fetchSemanticsNode().positionInRoot
            val coords = MotionEvent.PointerCoords().apply { x = local.x; y = local.y }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = tool }
            val time = SystemClock.uptimeMillis()
            return MotionEvent.obtain(time, time, MotionEvent.ACTION_HOVER_MOVE, 1, arrayOf(props), arrayOf(coords), 0, 0, 1f, 1f, 1, 0,
                if (tool == MotionEvent.TOOL_TYPE_MOUSE) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_STYLUS, 0)
        }
        fun icon(point: androidx.compose.ui.geometry.Offset, type: Int?, tool: Int = MotionEvent.TOOL_TYPE_MOUSE) {
            val expected = type?.let { PointerIcon.getSystemIcon(native.context, it) }
            fun resolved(): PointerIcon? {
                val event = event(point, tool)
                var icon: PointerIcon? = null
                try { instrumentation.runOnMainSync {
                    val source = IntArray(2); val target = IntArray(2)
                    native.getLocationOnScreen(source); decor.getLocationOnScreen(target)
                    event.offsetLocation((source[0] - target[0]).toFloat(), (source[1] - target[1]).toFloat())
                    icon = decor.onResolvePointerIcon(event, 0)
                } } finally { event.recycle() }
                return icon
            }
            runCatching { compose.waitUntil(5_000) {
                val actual = resolved()
                actual == expected
            } }
            val actual = resolved()
            val actualType = listOf(PointerIcon.TYPE_ARROW, PointerIcon.TYPE_HAND, PointerIcon.TYPE_GRAB, PointerIcon.TYPE_GRABBING, PointerIcon.TYPE_NULL).firstOrNull { PointerIcon.getSystemIcon(native.context, it) == actual }
            assertEquals("Native cursor at $point for tool $tool (expected type=$type, resolved type=$actualType)", expected, actual)
        }
        fun hover(point: androidx.compose.ui.geometry.Offset, type: Int, penType: Int? = if (type in listOf(PointerIcon.TYPE_ARROW, PointerIcon.TYPE_GRAB, PointerIcon.TYPE_GRABBING)) null else type) {
            root.performMouseInput { moveTo(point) }; settle(); icon(point, type)
            root.performMouseInput { exit() }
            val event = event(point, MotionEvent.TOOL_TYPE_STYLUS)
            try { instrumentation.runOnMainSync {
                event.action = MotionEvent.ACTION_HOVER_ENTER
                native.dispatchGenericMotionEvent(event)
                event.action = MotionEvent.ACTION_HOVER_MOVE
                native.dispatchGenericMotionEvent(event)
            } }
            finally { event.recycle() }
            settle(); icon(point, penType, MotionEvent.TOOL_TYPE_STYLUS)
            val exit = event(point, MotionEvent.TOOL_TYPE_STYLUS).apply { action = MotionEvent.ACTION_HOVER_EXIT }
            try { instrumentation.runOnMainSync { native.dispatchGenericMotionEvent(exit) } }
            finally { exit.recycle() }
        }
        val away = root.fetchSemanticsNode().boundsInRoot.let { androidx.compose.ui.geometry.Offset(it.width * .6f, it.height * .6f) }
        for (tag in listOf("tab-brushes", "tab-sizes")) hover(bounds(tag).center, PointerIcon.TYPE_ARROW)
        hover(bounds("group-grip-41").center, PointerIcon.TYPE_GRAB)
        hover(bounds("ribbon-grip-toolbar").center, PointerIcon.TYPE_GRAB)
        val grip = bounds("group-grip-41")
        hover(androidx.compose.ui.geometry.Offset(grip.left - 8f, grip.center.y), PointerIcon.TYPE_ARROW)
        hover(away, PointerIcon.TYPE_NULL, PointerIcon.TYPE_NULL)
        val dividers = host.snapshot!!.getJSONObject("layout").array("dividers").objects()
        assertEquals(setOf("horizontal", "vertical"), dividers.map { it.getString("axis") }.toSet())
        for (divider in dividers) {
            val type = if (divider.getString("axis") == "horizontal") PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW else PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
            val start = bounds("divider-${divider.getInt("id")}").center
            hover(start, type)
            val before = saved()
            val end = start + androidx.compose.ui.geometry.Offset(35f, 35f)
            root.performMouseInput { moveTo(start); press(); moveTo(end, 200) }; settle(); icon(end, type)
            root.performMouseInput { cancel() }; settle(); assertEquals(before, saved())
        }
        val start = bounds("tab-sizes").center
        root.performMouseInput { moveTo(start); press() }; settle()
        assertEquals("sizes", group("sizes").getString("active"))
        val before = saved()
        root.performMouseInput { moveTo(away, 300) }; settle()
        compose.waitUntil(10_000) { group("sizes").getBoolean("floating") }
        icon(away, PointerIcon.TYPE_GRABBING)
        icon(away, PointerIcon.TYPE_GRABBING, MotionEvent.TOOL_TYPE_STYLUS)
        root.performMouseInput { release() }; settle()
        icon(away, PointerIcon.TYPE_ARROW)
        val floated = saved()
        val floatingId = group("sizes").getInt("id")
        for ((edge, type) in listOf("left" to PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW, "right" to PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW,
            "top" to PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW, "bottom" to PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW,
            "top_left" to PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW, "bottom_right" to PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW,
            "top_right" to PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW, "bottom_left" to PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW)) {
            val resize = bounds("resize-$floatingId-$edge").center
            hover(resize, type)
            val extent = root.fetchSemanticsNode().boundsInRoot
            val end = androidx.compose.ui.geometry.Offset((resize.x + 40f).coerceIn(2f, extent.width - 2f), (resize.y + 40f).coerceIn(2f, extent.height - 2f))
            root.performMouseInput { moveTo(resize); press(); moveTo(end, 200) }; settle(); icon(end, type)
            root.performMouseInput { cancel() }; settle(); assertEquals(floated, saved())
        }
        action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, saved())
        action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(floated, saved())
        action(obj("type" to "restore_workspace", "workspace" to fixture))
        customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
        customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
        hover(bounds("column-grip-41").center, PointerIcon.TYPE_GRAB)
        compose.onNodeWithTag("column-icon-brushes").performTouchInput { click() }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-header-41").fetchSemanticsNodes().isNotEmpty() }
        settle()
        for (tag in listOf("drawer-tab-brushes", "drawer-tab-sizes")) hover(bounds(tag).center, PointerIcon.TYPE_ARROW)
        hover(bounds("column-drawer-grip-41").center, PointerIcon.TYPE_GRAB)
        val drawerGrip = bounds("column-drawer-grip-41")
        hover(androidx.compose.ui.geometry.Offset(drawerGrip.left - 8f, drawerGrip.center.y), PointerIcon.TYPE_ARROW)
        val collapsed = saved()
        root.performMouseInput { moveTo(bounds("column-grip-41").center); press(); moveTo(away, 250) }; settle()
        icon(away, PointerIcon.TYPE_GRABBING)
        val edge = androidx.compose.ui.geometry.Offset(root.fetchSemanticsNode().boundsInRoot.width - 2f, away.y)
        root.performMouseInput { moveTo(edge, 250) }; settle(); icon(edge, PointerIcon.TYPE_GRABBING)
        root.performMouseInput { cancel() }; settle(); assertEquals(collapsed, saved())
        hover(bounds("column-grip-41").center, PointerIcon.TYPE_GRAB)
        hover(away, PointerIcon.TYPE_NULL, PointerIcon.TYPE_NULL)
        assertNull(host.actionError)
        }
    }

    @Test fun panelTabsHighlightOnHoverAndSelectOnPress() = panelTabsHighlightOnHoverAndSelectOnPress("light")
    @Test fun panelTabsHighlightOnHoverAndSelectOnPressInDarkTheme() = panelTabsHighlightOnHoverAndSelectOnPress("dark")

    private fun panelTabsHighlightOnHoverAndSelectOnPress(theme: String) {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        fun selectedPanel(value: Any? = state().getJSONObject("workspace").getJSONObject("layout")): String? = when (value) {
            is JSONObject -> if (value.optString("kind") == "tabs" && value.optInt("id") == 41) value.getString("active")
                else value.keys().asSequence().mapNotNull { selectedPanel(value.opt(it)) }.firstOrNull()
            is JSONArray -> (0 until value.length()).firstNotNullOfOrNull { selectedPanel(value.opt(it)) }
            else -> null
        }
        val root = compose.onNodeWithTag("workspace")
        fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot.translate(-root.fetchSemanticsNode().boundsInRoot.topLeft)
        fun settle() { compose.waitForIdle(); SystemClock.sleep(180); compose.waitForIdle() }
        fun hover(point: androidx.compose.ui.geometry.Offset, tool: Int, eventAction: Int = MotionEvent.ACTION_HOVER_MOVE) {
            val window = root.fetchSemanticsNode().positionInWindow + point
            instrumentation.runOnMainSync {
                val coords = MotionEvent.PointerCoords().apply { x = window.x; y = window.y; pressure = if (eventAction == MotionEvent.ACTION_DOWN) 1f else 0f }
                val props = MotionEvent.PointerProperties().apply { id = 0; toolType = tool }
                val time = SystemClock.uptimeMillis()
                val mouse = tool == MotionEvent.TOOL_TYPE_MOUSE
                val event = MotionEvent.obtain(time, time, eventAction, 1, arrayOf(props), arrayOf(coords), 0,
                    if (mouse && eventAction == MotionEvent.ACTION_DOWN) MotionEvent.BUTTON_PRIMARY else 0, 1f, 1f, 1, 0,
                    if (mouse) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_STYLUS, 0)
                try {
                    if (eventAction == MotionEvent.ACTION_DOWN || eventAction == MotionEvent.ACTION_UP || eventAction == MotionEvent.ACTION_CANCEL)
                        compose.activity.window.decorView.dispatchTouchEvent(event)
                    else compose.activity.window.decorView.dispatchGenericMotionEvent(event)
                } finally { event.recycle() }
            }
            settle()
        }
        for (placement in listOf("docked", "floating", "drawer")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            if (placement == "floating") {
                action(obj("type" to "move_group", "group" to 41,
                    "target" to obj("kind" to "float", "position" to JSONArray(listOf(350, 240))), "viewport" to viewport()))
            } else if (placement == "drawer") {
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                compose.onNodeWithTag("column-icon-brushes").performTouchInput { click() }
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-header-41").fetchSemanticsNodes().isNotEmpty() }
            }
            settle()
            val header = if (placement == "drawer") "column-drawer-header-41" else "group-header-41"
            val tab = if (placement == "drawer") "drawer-tab-" else "tab-"
            val grip = bounds(if (placement == "drawer") "column-drawer-grip-41" else "group-grip-41")
            val away = root.fetchSemanticsNode().boundsInRoot.let { androidx.compose.ui.geometry.Offset(it.width * .7f, it.height * .8f) }
            val points = mutableListOf("active" to bounds("${tab}brushes").center, "inactive" to bounds("${tab}sizes").center, "grip" to grip.center)
            val tabsRight = bounds("${tab}tool_settings").right
            if (grip.left - tabsRight > 2f) points.add("empty" to androidx.compose.ui.geometry.Offset((tabsRight + grip.left) / 2f, grip.center.y))
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                hover(away, tool, MotionEvent.ACTION_HOVER_ENTER)
                val menu = compose.onNodeWithTag("application-menu-file")
                val menuBefore = menu.captureToImage().toPixelMap()
                hover(bounds("application-menu-file").center, tool)
                val menuHovered = menu.captureToImage().toPixelMap()
                assertTrue("$tool hover still highlights ordinary controls", (0 until menuBefore.height).any { y ->
                    (0 until menuBefore.width).any { x -> menuBefore[x, y] != menuHovered[x, y] }
                })
                hover(away, tool)
                for ((part, point) in points) {
                    action(obj("type" to "select_panel_tab", "group" to 41, "panel" to "brushes"))
                    hover(away, tool)
                    val baseline = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                    if (part == "grip") capture("panel-header-grip-baseline-$theme-$placement-$tool")
                    hover(point, tool)
                    val actual = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                    if (part == "grip") capture("panel-header-grip-hover-$theme-$placement-$tool")
                    var changed = 0
                    for (y in 0 until baseline.height) for (x in 0 until baseline.width) {
                        if (baseline[x, y] != actual[x, y]) changed++
                    }
                    if (part == "inactive") {
                        assertTrue("$theme $placement $tool inactive tab has visible hover", changed > 0)
                        val active = bounds("${tab}brushes")
                        val headerOrigin = bounds(header).topLeft
                        for (y in 0 until baseline.height) for (x in 0 until baseline.width) {
                            val position = headerOrigin + androidx.compose.ui.geometry.Offset(x.toFloat(), y.toFloat())
                            if (position.x < active.left || position.x >= active.right) continue
                            assertEquals("Hover preserves the active tab", baseline[x, y], actual[x, y])
                        }
                        val footY = baseline.height - 1
                        for (edge in listOf(active.left - 1f, active.right + 1f)) {
                            val footX = (edge - headerOrigin.x).toInt()
                            if (footX in 0 until baseline.width)
                                assertEquals("Hover preserves the active tab shoulder", baseline[footX, footY], actual[footX, footY])
                        }
                    }
                    else assertEquals("$theme $placement $tool $part hover leaves header unchanged", 0, changed)
                    val expanded = state().getJSONObject("customization").optString("expanded")
                    hover(point, tool, MotionEvent.ACTION_DOWN)
                    var pressed = true
                    try {
                        val selected = if (part == "inactive") "sizes" else "brushes"
                        assertEquals("Tab selects before release", selected, selectedPanel())
                        hover(point, tool, MotionEvent.ACTION_UP)
                        pressed = false
                        assertEquals("Release keeps tab selected", selected, selectedPanel())
                        assertEquals("Release keeps customization unchanged", expanded, state().getJSONObject("customization").optString("expanded"))
                        capture("panel-header-press-$theme-$placement-$tool-$part")
                    } finally { if (pressed) hover(point, tool, MotionEvent.ACTION_CANCEL) }
                }
                hover(away, tool, MotionEvent.ACTION_HOVER_EXIT)
            }
        }
    }

    @Test fun panelHoverCornersFollowEveryActiveTab() = panelHoverCornersFollowEveryActiveTab("light")
    @Test fun panelHoverCornersFollowEveryActiveTabInDarkTheme() = panelHoverCornersFollowEveryActiveTab("dark")

    private fun panelHoverCornersFollowEveryActiveTab(theme: String) {
        val panels = listOf("brushes", "sizes", "tool_settings", "navigator")
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, *panels.toTypedArray())),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        action(obj("type" to "set_theme", "theme" to theme))
        val root = compose.onNodeWithTag("workspace")
        fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        fun hover(point: androidx.compose.ui.geometry.Offset, entering: Boolean = false, exiting: Boolean = false) {
            val position = point - root.fetchSemanticsNode().boundsInRoot.topLeft + root.fetchSemanticsNode().positionInWindow
            instrumentation.runOnMainSync {
                val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE }
                val coords = MotionEvent.PointerCoords().apply { x = position.x; y = position.y }
                val time = SystemClock.uptimeMillis()
                val event = MotionEvent.obtain(time, time, if (exiting) MotionEvent.ACTION_HOVER_EXIT else if (entering) MotionEvent.ACTION_HOVER_ENTER else MotionEvent.ACTION_HOVER_MOVE,
                    1, arrayOf(props), arrayOf(coords), 0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_MOUSE, 0)
                try { compose.activity.window.decorView.dispatchGenericMotionEvent(event) } finally { event.recycle() }
            }
            settle()
        }
        for (drawer in listOf(false, true)) {
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            if (drawer) {
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                compose.onNodeWithTag("column-icon-brushes").performTouchInput { click() }
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-header-41").fetchSemanticsNodes().isNotEmpty() }
            }
            val header = if (drawer) "column-drawer-header-41" else "group-header-41"
            val prefix = if (drawer) "drawer-tab-" else "tab-"
            val away = bounds(if (drawer) "column-drawer-grip-41" else "group-grip-41").center
            hover(away, entering = true)
            for (active in panels.indices) for (hovered in panels.indices) {
                hover(away)
                action(obj("type" to "select_panel_tab", "group" to 41, "panel" to panels[active]))
                val headerBounds = bounds(header)
                val activeBounds = bounds("$prefix${panels[active]}")
                val target = bounds("$prefix${panels[hovered]}")
                val baseline = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                hover(target.center)
                val actual = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                fun pixel(point: androidx.compose.ui.geometry.Offset, changed: Boolean, label: String) {
                    val x = (point.x - headerBounds.left).toInt()
                    val y = (point.y - headerBounds.top).toInt()
                    if (x !in 0 until baseline.width || y !in 0 until baseline.height) return
                    val actualChange = baseline[x, y] != actual[x, y]
                    val neighbors = if (changed == actualChange) "" else (maxOf(0, y - 2)..minOf(baseline.height - 1, y + 2)).joinToString(";") { row ->
                        (maxOf(0, x - 2)..minOf(baseline.width - 1, x + 2)).joinToString(",") { column ->
                            "%06X>%06X".format(baseline[column, row].toArgb() and 0xFFFFFF, actual[column, row].toArgb() and 0xFFFFFF)
                        }
                    }
                    assertEquals("$theme drawer=$drawer active=$active hovered=$hovered $label at $point neighbors=$neighbors", changed, actualChange)
                }
                pixel(androidx.compose.ui.geometry.Offset(target.center.x, target.top + 5f * compose.activity.resources.displayMetrics.density), active != hovered, "face")
                val bottom = target.bottom - 1f
                for (left in listOf(true, false)) {
                    val neighbor = hovered + if (left) -1 else 1
                    val inside = if (left) target.left + 1f else target.right - 2f
                    val outside = if (left) target.left - 1f else target.right + 1f
                    if (hovered != active) {
                        pixel(androidx.compose.ui.geometry.Offset(inside, target.top + 1f), false, "rounded top corner")
                        if (neighbor !in panels.indices) pixel(androidx.compose.ui.geometry.Offset(inside, bottom), true, "square end")
                        else if (neighbor == active) pixel(androidx.compose.ui.geometry.Offset(inside, bottom), false, "active-neighbor convex corner")
                        else pixel(androidx.compose.ui.geometry.Offset(outside, bottom), true, "inactive-neighbor concave foot")
                    }
                    val activeOutside = if (left) activeBounds.left - 1f else activeBounds.right + 1f
                    pixel(androidx.compose.ui.geometry.Offset(activeOutside, activeBounds.bottom - 1f), false, "active shoulder")
                }
                if (active == 1 && hovered == 2) capture("panel-hover-corners-$theme-drawer-$drawer")
            }
            hover(away, exiting = true)
        }
    }

    @Test fun panelHeadersRetainVisibleKeyboardFocus() {
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_TAB)
        val tab = compose.onNodeWithTag("tab-brushes")
        tab.performSemanticsAction(SemanticsActions.RequestFocus) { assertTrue(it()) }
        tab.assertIsFocused()
        val pixels = tab.captureToImage().toPixelMap()
        val accent = android.graphics.Color.parseColor(state().getJSONObject("palette").getString("accent"))
        assertTrue("Keyboard focus has a visible accent outline", (0 until pixels.height).any { y ->
            (0 until pixels.width).any { x -> pixels[x,y].let { color ->
                kotlin.math.abs(color.red - android.graphics.Color.red(accent)/255f) < .02f &&
                    kotlin.math.abs(color.green - android.graphics.Color.green(accent)/255f) < .02f &&
                    kotlin.math.abs(color.blue - android.graphics.Color.blue(accent)/255f) < .02f
            } }
        })
    }

    @Test fun columnDrawersUseNativeMouseAndTouchDrag() {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val root = compose.onNodeWithTag("workspace", useUnmergedTree = true)
        fun snapshot() = state().getJSONObject("workspace").toString()
        fun bounds(tag: String) = compose.onNodeWithTag(tag, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot.translate(-root.fetchSemanticsNode().boundsInRoot.topLeft)
        fun history(before: String) {
            val after = snapshot(); assertNotEquals(before, after)
            action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, snapshot())
            action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(after, snapshot())
        }
        for (mouse in listOf(true, false)) {
            fun press(point: androidx.compose.ui.geometry.Offset) {
                if (mouse) root.performMouseInput { moveTo(point); press() } else root.performTouchInput { down(point) }
            }
            fun move(point: androidx.compose.ui.geometry.Offset) {
                if (mouse) root.performMouseInput { moveTo(point, 160) } else root.performTouchInput { moveTo(point, 160) }
                settle()
            }
            fun finishGesture(cancelled: Boolean = false) {
                if (mouse) root.performMouseInput { if (cancelled) cancel() else release() }
                else root.performTouchInput { if (cancelled) cancel() else up() }
                settle(); assertNull(host.actionError)
            }
            fun open() {
                action(obj("type" to "restore_workspace", "workspace" to fixture))
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                val point = bounds("column-icon-brushes").center
                press(point); finishGesture()
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-grip-41", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
                settle()
            }
            fun away() = root.fetchSemanticsNode().boundsInRoot.let { androidx.compose.ui.geometry.Offset(it.width * .5f, it.height * .55f) }
            for (source in listOf("active", "inactive", "grip", "empty")) {
                open()
                val box = bounds("column-drawer-41"); val grip = bounds("column-drawer-grip-41")
                assertEquals("Fixed top-right grip", box.right, grip.right, 2f)
                val start = when (source) {
                    "grip" -> grip.center
                    "empty" -> androidx.compose.ui.geometry.Offset(grip.left - 12f, grip.center.y)
                    "inactive" -> bounds("drawer-tab-sizes").center
                    else -> bounds("drawer-tab-brushes").center
                }
                val before = snapshot()
                press(start); move(start + androidx.compose.ui.geometry.Offset(20f, 0f)); assertEquals(before, snapshot())
                move(away())
                compose.waitUntil(10_000) { state().getJSONObject("workspace").getJSONObject("layout").array("floating").length() == 1 }
                finishGesture()
                assertEquals("$mouse $source", if (source in listOf("grip", "empty")) 3 else 1,
                    host.panelGroup(if (source == "inactive") "sizes" else "brushes").array("panels").length())
                history(before)
            }
            open()
            var before = snapshot()
            press(bounds("drawer-tab-tool_settings").center)
            var box = bounds("column-drawer-41")
            move(androidx.compose.ui.geometry.Offset(box.left + 4f, bounds("column-drawer-header-41").center.y)); finishGesture()
            assertEquals(listOf("tool_settings", "brushes", "sizes"), host.panelGroup("brushes").array("panels").values())
            history(before)
            open(); before = snapshot()
            press(bounds("drawer-tab-brushes").center); move(away()); finishGesture(true)
            assertEquals(before, snapshot())
            compose.onNodeWithTag("column-drawer-41", useUnmergedTree = true).assertIsDisplayed()
            // A mostly clipped tab must insert using its visible midpoint.
            val overflow = JSONObject(fixture.toString())
            overflow.getJSONObject("layout").array("bands").getJSONObject(0).getJSONObject("root").apply {
                put("panels", JSONArray(listOf("brushes", "sizes", "tool_settings", "navigator", "stats")))
                put("tab_style", "icon_name")
            }
            action(obj("type" to "restore_workspace", "workspace" to overflow))
            customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
            customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
            press(bounds("column-icon-brushes").center); finishGesture(); settle()
            val gripBeforeScroll = bounds("column-drawer-grip-41")
            val scrollBy = bounds("drawer-tab-brushes").width + bounds("drawer-tab-sizes").width * .75f
            compose.onNodeWithTag("column-drawer-tabs-41", useUnmergedTree = true).performSemanticsAction(SemanticsActions.ScrollBy) { it(scrollBy, 0f) }
            settle()
            assertEquals(gripBeforeScroll, bounds("column-drawer-grip-41"))
            val clippedTab = bounds("drawer-tab-sizes")
            assertTrue("Sizes is partially visible", clippedTab.width > 4f)
            before = snapshot()
            press(bounds("tab-layers").center); move(away())
            move(androidx.compose.ui.geometry.Offset(clippedTab.left + 2f, clippedTab.center.y)); finishGesture()
            assertEquals(listOf("brushes", "layers", "sizes", "tool_settings", "navigator", "stats"), host.panelGroup("layers").array("panels").values())
            history(before)
            for (zone in listOf("tab", "merge", "top", "bottom")) {
                open(); before = snapshot()
                press(bounds(if (zone == "merge") "group-grip-43" else "tab-layers").center); move(away())
                box = bounds("column-drawer-41")
                val header = bounds("column-drawer-header-41")
                val destination = when (zone) {
                    "tab" -> androidx.compose.ui.geometry.Offset(box.left + 4f, header.center.y)
                    "top" -> androidx.compose.ui.geometry.Offset(box.left + 4f, header.bottom + 4f)
                    "bottom" -> androidx.compose.ui.geometry.Offset(box.center.x, box.bottom - 4f)
                    else -> box.center
                }
                move(destination)
                compose.onNodeWithTag("workspace-drop-hint", useUnmergedTree = true).assertExists()
                finishGesture()
                val target = host.panelGroup("layers")
                if (zone == "bottom") assertNotEquals(41, target.getInt("id")) else assertEquals(41, target.getInt("id"))
                if (zone != "bottom") assertEquals("layers", target.array("panels").getString(0))
                if (zone == "merge") assertEquals(6, target.array("panels").length())
                assertEquals(0, state().getJSONObject("workspace").getJSONObject("layout").array("floating").length())
                history(before)
            }
            capture("column-drawer-drag-${if (mouse) "mouse" else "touch"}")
        }
    }

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
            compose.onNode(hasText(choice.getString("label")) and hasAnyAncestor(hasTestTag("layer-rows"))).assertIsDisplayed()
            val row = state().array("layers").objects().first { it.getLong("id") == layer }
            val icon = row.getString("content_icon")
            assertTrue(icon.startsWith("layer-") && icon.endsWith("-symbolic"))
            instrumentation.targetContext.assets.open("$icon.svg").use { assertTrue(it.read() >= 0) }
            assertNull(host.failure)
            action(obj("type" to "layer", "action" to obj("op" to "delete", "id" to layer)))
        }
    }
    @Test fun curvesPagesNativeContactsAndExactCoordinates() {
        fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
        host.createDrawing(128, 128, depth = "F32")
        compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        action(obj("type" to "invoke", "command" to "fit_canvas"))
        penStroke(12)
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled")
            && !it.getJSONObject("filter_load").getBoolean("pending") }
        val paintLayer = state().getJSONObject("layer_properties").getLong("layer")
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "curves")))
        for (panel in listOf("navigator", "proof", "layers")) action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to false)))
        fun properties() = state().getJSONObject("layer_properties")
        fun picker() = host.colorPreview?.objectOrNull("picker") ?: state().getJSONObject("color_picker")
        fun control() = properties().array("controls").objects().first { !it.isNull("curve") }
        fun points() = control().getJSONObject("value").getJSONArray("value").toString()
        fun graph() = compose.onNodeWithTag("effect-curve").performScrollTo()
        fun effect(value: JSONObject) = action(obj("type" to "effect", "action" to value))
        fun set(key: String, kind: String, value: Any) = effect(obj("op" to "set", "layer" to properties().getLong("layer"), "key" to key, "value" to obj("kind" to kind, "value" to value)))
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun key(code: Int, pressed: Boolean, repeat: Int = 0, modifiers: Int = 0) {
            val now = SystemClock.uptimeMillis()
            assertTrue(instrumentation.uiAutomation.injectInputEvent(KeyEvent(now, now, if (pressed) KeyEvent.ACTION_DOWN else KeyEvent.ACTION_UP, code, repeat, modifiers), true))
            settle()
        }
        fun coordinate(axis: String) = control().getJSONObject("curve").getJSONObject(axis)
        fun numberTag(axis: String) = "number-curve-${control().getString("key")}-$axis"
        fun focusNumber(axis: String): SemanticsNodeInteraction {
            if (compose.onAllNodesWithTag(numberTag(axis)).fetchSemanticsNodes().isEmpty())
                compose.onNodeWithTag("number-value-curve-${control().getString("key")}-$axis").performScrollTo().performClick()
            return compose.onNodeWithTag(numberTag(axis)).performScrollTo().performClick()
        }
        fun edit(axis: String, text: String, wanted: Double = text.toDouble()) {
            val field = focusNumber(axis)
            field.performClick().performTextReplacement(text)
            field.performImeAction()
            waitState {
                val actual = coordinate(axis).getDouble("value")
                if (wanted == 0.0) actual == 0.0 else kotlin.math.abs(actual / wanted - 1.0) < 1e-5
            }
        }
        fun genericNumberUndo(key: String) {
            fun number() = properties().array("controls").objects().first { it.getString("key") == key }
            val before = number().getJSONObject("value").getDouble("value")
            val slider = compose.onNodeWithTag("number-slider-${number().getString("label")}")
            slider.performScrollTo()
            slider.performTouchInput { swipe(androidx.compose.ui.geometry.Offset(width * .2f, height * .5f), androidx.compose.ui.geometry.Offset(width * .45f, height * .5f), 250) }
            native { Native.documentTabs(it, obj("op" to "view").toString()) }; compose.waitForIdle()
            waitState { number().getJSONObject("value").getDouble("value") != before }
            val after = number().getJSONObject("value").getDouble("value")
            assertNotEquals("Native generic $key slider changes its value", before, after)
            invoke("undo"); assertEquals("A generic numeric contact has one undo", before, number().getJSONObject("value").getDouble("value"), 0.0)
            invoke("redo"); assertEquals(after, number().getJSONObject("value").getDouble("value"), 0.0)
            invoke("undo")
        }
        fun selectPage(index: Int) {
            val page = properties().array("pages").getJSONObject(index)
            if (properties().getString("page") == page.getString("id")) return
            compose.onNodeWithTag("properties-page").performScrollTo().performTouchInput { click(center) }
            compose.onNode(hasText(page.getString("label")) and hasAnyAncestor(isPopup())).performTouchInput { click(center) }
            waitState { it.getJSONObject("layer_properties").getString("page") == page.getString("id") }
        }
        val curvesLayer = properties().getLong("layer")
        val effectKeys = mutableMapOf(curvesLayer to "curves")
        var themeArchive = "setup"
        fun propertyAction(op: String, role: String? = null) {
            val item = properties().array("actions").objects().single {
                val a = it.getJSONObject("action")
                a.getString("op") == op && (role == null || a.optString("role") == role)
            }
            val a = item.getJSONObject("action")
            assertEquals(properties().getLong("layer"), a.getLong("layer"))
            assertEquals(properties().getLong("epoch"), a.getLong("epoch"))
            val tag = if (role == null) "property-action-$op" else {
                val group = item.getJSONObject("group")
                assertEquals("calibration", group.getString("id"))
                compose.onNodeWithTag("property-action-group-calibration").performScrollTo()
                    .assert(hasText(group.getString("label")) or hasContentDescription(group.getString("label"))).performClick()
                "property-calibrate-$role"
            }
            val button = compose.onNodeWithTag(tag)
            if (role == null) button.performScrollTo()
            button.assert(hasText(item.getString("label")) or hasContentDescription(item.getString("label"))).performClick()
        }
        fun archive(name: String): Pair<JSONObject, ByteArray> {
            val file = File(device.root, "precision-$themeArchive-$name.capy")
            host.writeDrawingCopy(file)
            val bytes = file.readBytes()
            return packageManifest(bytes) to bytes
        }
        fun backing(saved: Pair<JSONObject, ByteArray>): Pair<Any?, List<List<Byte>>> {
            val (index, bytes) = saved
            val paint = JSONArray(index.paintRecords().objects().filter {
                val data = it.getJSONObject("data")
                data.optJSONArray("tiles")?.length()?.let { it > 0 } == true || data.has("material") || data.has("base")
            })
            val resourceIndex = index.getJSONArray("resources").objects().associateBy { it.getString("id") }
            val objectIndex = index.getJSONArray("objects").objects().associateBy { it.getString("id") }
            val referenced = mutableSetOf<String>()
            fun visit(value: Any?) {
                when (value) {
                    is JSONObject -> {
                        value.optString("ref").takeIf { it in objectIndex }?.let { id -> visit(objectIndex.getValue(id).getJSONObject("data")) }
                        value.optString("ref").takeIf { it in resourceIndex }?.let { id ->
                            if (referenced.add(id)) visit(resourceIndex.getValue(id))
                        }
                        value.keys().forEach { visit(value.get(it)) }
                    }
                    is JSONArray -> for (i in 0 until value.length()) visit(value.get(i))
                }
            }
            visit(paint)
            val resources = referenced.sorted().map { resourceIndex.getValue(it) }
            val payloads = resources.map { resource ->
                val location = resource.getJSONObject("location")
                val pack = packageMember(bytes, location.getString("pack"))
                val start = location.getString("offset").toInt()
                val payload = pack.copyOfRange(start, start + resource.getString("bytes").toInt())
                assertEquals(resource.getString("crc32"), java.util.zip.CRC32().apply { update(payload) }.value.toString(16).padStart(8, '0'))
                object : AbstractList<Byte>() {
                    override val size: Int get() = payload.size
                    override fun get(index: Int): Byte = payload[index]
                    override fun toString(): String = "${resource.getString("id")}: ${payload.size} exact bytes"
                }
            }
            assertTrue("The painted fixture must have actual saved backing", paint.length() > 0 && resources.any { it.getString("type") == "capy.raster-tile/1" })
            val paintIds = paint.objects().map { it.getString("id") }.toSet()
            val occurrences = JSONArray(index.occurrenceRecords().objects().filter {
                it.getJSONObject("data").getJSONObject("content").optJSONObject("paint")?.optString("ref") in paintIds
            }.sortedBy { it.getString("id") })
            val descriptors = JSONArray(resources.map { JSONObject(it.toString()).apply { remove("location") } })
            return jsonValue(obj("paint" to paint, "occurrences" to occurrences, "resources" to descriptors)) to payloads
        }
        fun savedEffect(saved: Pair<JSONObject, ByteArray>, layer: Long = curvesLayer): Any? {
            val index = saved.first
            val application = index.getJSONArray("objects").objects().single {
                it.getString("type") == "capy.effect/2" && it.getJSONObject("data").optString("builtin") == effectKeys.getValue(layer)
            }
            return jsonValue(application)
        }
        fun samplePoint(): androidx.compose.ui.geometry.Offset {
            invoke("fit_canvas")
            val camera = state().getJSONObject("camera")
            assertEquals(0.0, camera.getDouble("rotation"), 0.0)
            assertEquals(listOf(false, false), camera.getJSONArray("flipped").values())
            val t = camera.getJSONArray("translation"); val zoom = camera.getDouble("zoom")
            val viewport = camera.getJSONArray("viewport")
            return androidx.compose.ui.geometry.Offset(((t.getDouble(0) + 32 * zoom) / viewport.getDouble(0)).toFloat(),
                ((t.getDouble(1) + 96 * zoom) / viewport.getDouble(1)).toFloat())
        }
        fun scopes(theme: String) {
            val revision = state().getJSONObject("document_file").getLong("revision")
            for (kind in listOf("histogram", "waveform")) {
                customize(obj("type" to "set_panel_visible", "panel" to kind, "visible" to true))
                floatPanel(kind, 12f, 120f)
                val view = state().getJSONObject(kind)
                compose.onNodeWithTag("scope-$kind-source").performTouchInput { click(center) }
                compose.onNode(hasText(view.getJSONArray("sources").getString(3)) and hasAnyAncestor(isPopup())).performClick()
                waitState { it.getJSONObject(kind).getInt("source") == 3 && it.getJSONObject(kind).isNull("data") }
                compose.onNodeWithTag("scope-$kind-source").performTouchInput { click(center) }
                compose.onNode(hasText(view.getJSONArray("sources").getString(0)) and hasAnyAncestor(isPopup())).performClick()
                waitState { !it.getJSONObject(kind).isNull("data") && it.getJSONObject(kind).getInt("source") == 0 }
                val channel = if (kind == "waveform") 2 else 1
                compose.onNodeWithTag("scope-$kind-channel").performTouchInput { click(center) }
                compose.onNode(hasText(view.getJSONArray("channels").getString(channel)) and hasAnyAncestor(isPopup())).performClick()
                waitState { it.getJSONObject(kind).getInt("channel") == channel }
                val histogramLog = state().getJSONObject("histogram").getBoolean("logarithmic")
                val log = state().getJSONObject(kind).getBoolean("logarithmic")
                compose.onNodeWithTag("scope-$kind-log").performClick()
                waitState { it.getJSONObject(kind).getBoolean("logarithmic") != log }
                if (kind == "waveform") {
                    assertEquals(1, state().getJSONObject("histogram").getInt("channel"))
                    assertEquals("Waveform Log is independent", histogramLog, state().getJSONObject("histogram").getBoolean("logarithmic"))
                }
                for (name in listOf("shadows", "highlights")) {
                    val before = state().getJSONObject(kind).getBoolean(name)
                    compose.onNodeWithTag("scope-$kind-$name").performClick()
                    waitState { it.getJSONObject(kind).getBoolean(name) != before }
                    compose.onNodeWithTag("scope-$kind-$name").performClick()
                    waitState { it.getJSONObject(kind).getBoolean(name) == before }
                }
                val panel = screenBounds(compose.onNodeWithTag("scope-$kind"))
                val chart = screenBounds(compose.onNodeWithTag("scope-$kind-chart"))
                assertTrue("$kind plot fits the visible panel", chart.width > 0 && chart.height > 0 && panel.contains(chart.topLeft) && panel.contains(chart.bottomRight - androidx.compose.ui.geometry.Offset(1f, 1f)))
                for (name in listOf("source", "channel", "log", "shadows", "highlights", "status")) {
                    val child = screenBounds(compose.onNodeWithTag("scope-$kind-$name"))
                    assertTrue("$kind $name fits the visible panel", panel.contains(child.topLeft) && panel.contains(child.bottomRight - androidx.compose.ui.geometry.Offset(1f, 1f)))
                }
                compose.onNodeWithTag("scope-$kind-status").assertTextContains(state().getJSONObject(kind).getString("status"))
                val image = capture("precision-$theme-$kind")
                try {
                    var colored = 0
                    for (y in chart.top.toInt().coerceAtLeast(0) until chart.bottom.toInt().coerceAtMost(image.height))
                        for (x in chart.left.toInt().coerceAtLeast(0) until chart.right.toInt().coerceAtMost(image.width)) {
                            val pixel = image.getPixel(x, y)
                            val rgb = listOf(android.graphics.Color.red(pixel), android.graphics.Color.green(pixel), android.graphics.Color.blue(pixel))
                            if (rgb.max() - rgb.min() > 20 && rgb.max() > 60) colored++
                        }
                    assertTrue("The actual $kind chart presents colored data", colored > 10)
                } finally { image.recycle() }
                customize(obj("type" to "set_panel_visible", "panel" to kind, "visible" to false))
            }
            assertEquals("Native scope choices are presentation only", revision, state().getJSONObject("document_file").getLong("revision"))
            waitState { it.getJSONObject("histogram").isNull("data") && it.getJSONObject("waveform").isNull("data") }
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            selectPage(0)
            set("domain", "choice", 0)
            val curveKey = control().getString("key")
            set(curveKey, "curve", JSONArray("[[0,0],[0.5,0.5],[1,1]]"))
            val original = points()
            set(curveKey, "curve", JSONArray("[[0,0],[1,1]]"))
            graph().performTouchInput { doubleClick(center) }
            waitState { control().getJSONObject("value").getJSONArray("value").length() == 3 }
            val inserted = JSONArray(points())
            assertEquals("Double-tapping empty graph inserts one surviving knot", 3, inserted.length())
            assertEquals("[0,0]", inserted.getJSONArray(0).toString())
            assertEquals("[1,1]", inserted.getJSONArray(2).toString())
            assertTrue(inserted.getJSONArray(1).getDouble(0) in 0.0..1.0)
            set(curveKey, "curve", JSONArray(original))
            graph().performTouchInput { click(center) }
            waitState { !it.getJSONObject("layer_properties").array("controls").objects().first { c -> !c.isNull("curve") }.getJSONObject("curve").isNull("selected") }
            assertEquals("Selecting an existing knot preserves its exact coordinates", original, points())
            assertEquals("127.500", coordinate("output").getString("text"))
            capture("curves-$theme-encoded-midpoint")
            if (androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("curvePresentationOnly") == "true") {
                set(curveKey, "curve", JSONArray("[[0,0],[0.5,0.12345679],[1,1]]"))
                graph().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .5f, height * (1f - .12345679f))) }
                waitState { !control().getJSONObject("curve").isNull("output") }
                assertEquals("31.481", coordinate("output").getString("text"))
                capture("curves-$theme-encoded-precise")
                val precise = points()
                edit("output", "127.5", .5)
                assertEquals(.5, coordinate("output").getDouble("value"), 0.0)
                invoke("undo"); assertEquals(precise, points())
                continue
            }
            graph().performTouchInput { swipe(center, center + androidx.compose.ui.geometry.Offset(12f, -16f), 250) }
            waitState { points() != original }
            val dragged = points()
            assertNotEquals(original, dragged)
            invoke("undo"); assertEquals("One native contact has one undo", original, points())
            invoke("redo"); assertEquals(dragged, points())
            invoke("undo")
            graph().performTouchInput { click(center) }
            waitState { !control().getJSONObject("curve").isNull("selected") }
            graph().assertIsFocused()
            assertNotNull(host.pointControlFocus)
            key(KeyEvent.KEYCODE_DPAD_UP, true)
            assertNotEquals("Focused native graph ArrowUp edits the selected knot", original, points())
            key(KeyEvent.KEYCODE_DPAD_UP, true, 1)
            key(KeyEvent.KEYCODE_DPAD_LEFT, false)
            key(KeyEvent.KEYCODE_DPAD_UP, false, modifiers = KeyEvent.META_CTRL_ON)
            val repeated = points()
            assertNotEquals(original, repeated)
            invoke("undo"); assertEquals("Matching modified key-up completes one gesture", original, points())
            invoke("redo"); assertEquals(repeated, points())
            invoke("undo")
            graph().performTouchInput { click(center) }
            key(KeyEvent.KEYCODE_DPAD_UP, true)
            key(KeyEvent.KEYCODE_ESCAPE, true); key(KeyEvent.KEYCODE_ESCAPE, false)
            key(KeyEvent.KEYCODE_DPAD_UP, false)
            assertEquals("Escape followed by release preserves the original knots", original, points())
            graph().performTouchInput { click(center) }
            key(KeyEvent.KEYCODE_FORWARD_DEL, true); key(KeyEvent.KEYCODE_FORWARD_DEL, false)
            waitState { control().getJSONObject("value").getJSONArray("value").length() == 2 }
            assertEquals(2, control().getJSONObject("value").getJSONArray("value").length())
            invoke("undo"); assertEquals(original, points())
            graph().performMouseInput { click(center, button = MouseButton.Secondary) }
            waitState { control().getJSONObject("value").getJSONArray("value").length() == 2 }
            assertEquals("Native right-click removes the knot", 2, control().getJSONObject("value").getJSONArray("value").length())
            invoke("undo"); assertEquals(original, points())
            graph().performTouchInput { doubleClick(center) }
            waitState { control().getJSONObject("value").getJSONArray("value").length() == 2 }
            assertEquals("Native double-click removes the knot", 2, control().getJSONObject("value").getJSONArray("value").length())
            invoke("undo"); assertEquals(original, points())
            graph().performTouchInput { click(center) }
            focusNumber("output")
            key(KeyEvent.KEYCODE_DPAD_UP, true); key(KeyEvent.KEYCODE_DPAD_UP, true, 1)
            key(KeyEvent.KEYCODE_DPAD_LEFT, false); key(KeyEvent.KEYCODE_DPAD_UP, false)
            val numericRepeat = points()
            assertNotEquals(original, numericRepeat)
            invoke("undo"); assertEquals("Held native numeric key has one undo", original, points())
            invoke("redo"); assertEquals(numericRepeat, points())
            invoke("undo")
            focusNumber("output")
            key(KeyEvent.KEYCODE_DPAD_DOWN, true)
            key(KeyEvent.KEYCODE_ESCAPE, true); key(KeyEvent.KEYCODE_ESCAPE, false)
            key(KeyEvent.KEYCODE_DPAD_DOWN, false)
            assertEquals("Numeric Escape followed by release retires the edit", original, points())
            focusNumber("output")
            key(KeyEvent.KEYCODE_DPAD_UP, true)
            val firstKey = points()
            key(KeyEvent.KEYCODE_DPAD_DOWN, true)
            key(KeyEvent.KEYCODE_DPAD_UP, false); key(KeyEvent.KEYCODE_DPAD_DOWN, false)
            invoke("undo"); assertEquals("Changing native numeric key commits the preceding gesture", firstKey, points())
            invoke("undo"); assertEquals(original, points())
            focusNumber("output")
            key(KeyEvent.KEYCODE_DPAD_UP, true)
            val blurValue = points()
            focusNumber("input")
            key(KeyEvent.KEYCODE_DPAD_UP, false)
            assertEquals("Focus loss accepts once before later key-up", blurValue, points())
            invoke("undo"); assertEquals(original, points())
            selectPage(1)
            graph().performTouchInput { click(center) }
            waitState { control().getJSONObject("value").getJSONArray("value").length() == 3 }
            assertEquals(3, control().getJSONObject("value").getJSONArray("value").length())
            val red = points()
            selectPage(0); assertEquals("Page navigation retains hidden RGB points", original, points())
            selectPage(1); assertEquals(red, points())
            selectPage(0)
            set(curveKey, "curve", JSONArray("[[0,0],[0.5,0.12345679],[1,1]]"))
            graph().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .5f, height * (1f - .12345679f))) }
            waitState { !control().getJSONObject("curve").isNull("output") }
            assertEquals("31.481", coordinate("output").getString("text"))
            capture("curves-$theme-encoded-precise")
            val domain = properties().array("controls").objects().first { it.getString("key") == "domain" }
            compose.onNodeWithTag("property-domain").performScrollTo().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .8f, height * .5f)) }
            compose.onNode(hasText(domain.getJSONObject("kind").array("options").getString(1)) and hasAnyAncestor(isPopup())).performClick()
            waitState { it.getJSONObject("layer_properties").array("controls").objects().first { c -> !c.isNull("curve") }.getJSONObject("curve").getJSONObject("domain").getString("kind") == "log_hdr" }
            genericNumberUndo("hdr_stops")
            for (literal in listOf("1e-20", "8", "0")) {
                edit("output", literal)
                val wanted = literal.toDouble(); val actual = coordinate("output").getDouble("value")
                assertTrue("Exact physical HDR $literal: $actual", if (wanted == 0.0) actual == 0.0 else kotlin.math.abs(actual / wanted - 1.0) < 1e-5)
                if (literal != "0") capture("curves-$theme-hdr-${if (literal == "8") "eight" else "tiny"}")
            }
            themeArchive = theme
            set("domain", "choice", 0)
            selectPage(0)
            set("rgb", "curve", JSONArray("[[0,0],[0.5,0.5],[1,1]]"))
            for (channel in listOf("red", "green", "blue")) set(channel, "curve", JSONArray("[[0,0],[1,1]]"))
            val sourceArchive = archive("before-actions")
            val sourceBacking = backing(sourceArchive)
            scopes(theme)
            graph().performTouchInput { click(center) }
            waitState { !control().getJSONObject("curve").isNull("selected") && !it.getJSONObject("tonal_histogram").isNull("data") }
            val statistics = state().getJSONObject("tonal_histogram").getJSONObject("data").toString()
            val draft = focusNumber("output")
            draft.performTextReplacement("123.4567890123")
            assertEquals("A valid native draft has not edited the master coordinate", "[[0,0],[0.5,0.5],[1,1]]", points())
            draft.performTextInputSelection(androidx.compose.ui.text.TextRange(3, 8))
            val fieldBounds = screenBounds(draft)
            val propertiesBounds = screenBounds(compose.onNodeWithTag("panel-body-properties"))
            assertTrue("The exact coordinate fits the visible Properties body", fieldBounds.width > 20 && propertiesBounds.contains(fieldBounds.topLeft)
                && propertiesBounds.contains(fieldBounds.bottomRight - androidx.compose.ui.geometry.Offset(1f, 1f)))
            val retained = draft.fetchSemanticsNode().config
            set("red", "curve", JSONArray("[[0,0],[1,0.7]]"))
            waitState { !it.getJSONObject("tonal_histogram").isNull("data") && it.getJSONObject("tonal_histogram").getJSONObject("data").toString() != statistics }
            draft.assertIsFocused()
            assertEquals(retained[SemanticsProperties.EditableText], draft.fetchSemanticsNode().config[SemanticsProperties.EditableText])
            assertEquals(retained[SemanticsProperties.TextSelectionRange], draft.fetchSemanticsNode().config[SemanticsProperties.TextSelectionRange])
            assertEquals("A sibling edit leaves the focused valid master draft uncommitted", "[[0,0],[0.5,0.5],[1,1]]", points())
            key(KeyEvent.KEYCODE_ESCAPE, true); key(KeyEvent.KEYCODE_ESCAPE, false)
            assertEquals("A sibling channel edit did not commit the dirty master coordinate", "[[0,0],[0.5,0.5],[1,1]]", points())
            selectPage(1)
            val white = samplePoint()
            val captureControl = Native.captureControl()
            try {
                val sampleTask = native { Native.inspectionTask(it, captureControl) }
                val sampled = JSONObject(Native.inspectionSample(sampleTask, snapshotSource("EffectInput", curvesLayer), 32f, 96f, 5))
                assertEquals(state().getJSONObject("document_file").getLong("epoch"), sampled.getLong("epoch"))
                assertEquals(state().getJSONObject("document_file").getLong("revision"), sampled.getLong("revision"))
                val input = sampled.getJSONObject("sample").getJSONArray("Color")
                for (component in 0..3) assertEquals("Known original white input", 1.0, input.getDouble(component), 1e-6)
            } finally { Native.captureFree(captureControl) }
            val beforePick = archive("before-gray")
            val beforePoints = points()
            val beforePickRevision = state().getJSONObject("document_file").getLong("revision")
            propertyAction("calibrate", "gray")
            waitState { picker().getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals("Stylus Down does not change the curve", beforePoints, points())
            assertEquals(beforePickRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals(beforePoints, points())
            assertEquals(beforePickRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_UP, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { !picker().getBoolean("calibrating") && points() != beforePoints }
            val calibrated = JSONArray(points())
            assertEquals("Gray calibration uses the original white input endpoint", 2, calibrated.length())
            assertEquals(1.0, calibrated.getJSONArray(1).getDouble(0), 0.0)
            val picked = archive("gray-release")
            assertEquals(sourceBacking, backing(picked))
            invoke("undo"); assertEquals("One release has one Undo", savedEffect(beforePick), savedEffect(archive("gray-undo")))
            invoke("redo"); assertEquals(savedEffect(picked), savedEffect(archive("gray-redo")))
            invoke("undo")
            propertyAction("calibrate", "gray")
            waitState { picker().getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            compose.waitUntil(10_000) { host.colorPreview?.objectOrNull("picker")?.objectOrNull("preview") != null }
            val preview = host.colorPreview!!.getJSONObject("picker").getJSONObject("preview").getJSONArray("rgba")
            for (component in 0..2) assertEquals("Held touch previews the original white input", 1.0, preview.getDouble(component), 1e-5)
            assertEquals("A held preview does not edit the curve", beforePoints, points())
            canvasEvent(MotionEvent.ACTION_CANCEL, listOf(white))
            waitState { !picker().getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_UP, listOf(white))
            settle()
            assertEquals("A canceled loupe and its late Up do not edit", savedEffect(beforePick), savedEffect(archive("gray-touch-cancel")))
            val targetBefore = archive("target-before")
            propertyAction("target_curve")
            waitState { picker().getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            val moved = white + androidx.compose.ui.geometry.Offset(0f, .04f)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(moved))
            waitState { points() != beforePoints }
            assertEquals("The targeted source is the white input endpoint", 2, JSONArray(points()).length())
            canvasEvent(MotionEvent.ACTION_UP, listOf(moved))
            settle()
            assertTrue("Targeted mode stays armed after Up", picker().getBoolean("calibrating"))
            propertyAction("target_curve")
            waitState { !picker().getBoolean("calibrating") }
            val targeted = archive("target-release")
            assertEquals(sourceBacking, backing(targeted))
            invoke("undo"); assertEquals("A held targeted contact has one Undo", savedEffect(targetBefore), savedEffect(archive("target-undo")))
            invoke("redo"); assertEquals(savedEffect(targeted), savedEffect(archive("target-redo")))
            invoke("undo")
            propertyAction("target_curve")
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(moved))
            waitState { points() != beforePoints }
            canvasEvent(MotionEvent.ACTION_CANCEL, listOf(moved))
            waitState { points() == beforePoints }
            canvasEvent(MotionEvent.ACTION_UP, listOf(moved))
            settle(); assertEquals("A late Up after cancellation cannot edit", beforePoints, points())
            assertTrue("Contact cancellation retains the armed targeted tool", picker().getBoolean("calibrating"))
            propertyAction("target_curve")
            waitState { !picker().getBoolean("calibrating") }
            val cancelled = archive("target-cancel")
            assertEquals(savedEffect(targetBefore), savedEffect(cancelled)); assertEquals(sourceBacking, backing(cancelled))
            val retainedHost = host
            propertyAction("target_curve")
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(moved))
            waitState { points() != beforePoints }
            compose.activityRule.scenario.recreate()
            assertSame("Recreation retains the authoritative drawing owner", retainedHost, host)
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            waitState { !picker().getBoolean("calibrating") && points() == beforePoints }
            canvasEvent(MotionEvent.ACTION_UP, listOf(moved))
            settle()
            val recreated = archive("target-recreated")
            assertEquals("Surface destruction rolls back the unfinished gesture", savedEffect(targetBefore), savedEffect(recreated))
            assertEquals(sourceBacking, backing(recreated))
            effect(obj("op" to "insert", "effect" to "levels"))
            val levelsLayer = properties().getLong("layer")
            effectKeys[levelsLayer] = "levels"
            set("gamma", "number", 2)
            val beforeAuto = archive("auto-before")
            propertyAction("auto_levels")
            waitState { properties().array("controls").objects().first { c -> c.getString("key") == "gamma" }.getJSONObject("value").getDouble("value") == 1.0 }
            val automatic = archive("auto-after")
            assertEquals(sourceBacking, backing(automatic))
            invoke("undo"); assertEquals("Native Auto has one Undo", savedEffect(beforeAuto, levelsLayer), savedEffect(archive("auto-undo"), levelsLayer))
            invoke("redo"); assertEquals(savedEffect(automatic, levelsLayer), savedEffect(archive("auto-redo"), levelsLayer))
            action(obj("type" to "layer", "action" to obj("op" to "delete", "id" to levelsLayer)))
            action(obj("type" to "select_layer", "id" to curvesLayer))
            set("red", "curve", JSONArray("[[0,0],[1,1]]"))
            effect(obj("op" to "insert", "effect" to "white_balance"))
            val balanceLayer = properties().getLong("layer")
            effectKeys[balanceLayer] = "white_balance"
            set("temperature", "number", 25); set("tint", "number", 10)
            val beforeBalance = archive("balance-before")
            val balanceRevision = state().getJSONObject("document_file").getLong("revision")
            propertyAction("calibrate")
            waitState { picker().getBoolean("calibrating") }
            val balancePoint = samplePoint()
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals(balanceRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_UP, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { !picker().getBoolean("calibrating") }
            for (name in listOf("temperature", "tint")) {
                val value = properties().array("controls").objects().first { it.getString("key") == name }.getJSONObject("value").getDouble("value")
                assertEquals("Original white input neutralizes $name before the nonneutral White Balance", 0.0, value, 1e-5)
            }
            val balanced = archive("balance-release")
            assertEquals(sourceBacking, backing(balanced))
            invoke("undo"); assertEquals("Native White Balance release has one Undo", savedEffect(beforeBalance, balanceLayer), savedEffect(archive("balance-undo"), balanceLayer))
            invoke("redo"); assertEquals(savedEffect(balanced, balanceLayer), savedEffect(archive("balance-redo"), balanceLayer))
            action(obj("type" to "layer", "action" to obj("op" to "delete", "id" to balanceLayer)))
            action(obj("type" to "select_layer", "id" to curvesLayer))
            set("red", "curve", JSONArray("[[0,0],[1,0.7]]"))
            action(obj("type" to "select_layer", "id" to paintLayer))
            penStroke(12)
            val resumed = backing(archive("resumed-pen"))
            assertNotEquals("Fresh native pen input paints after cancellation and recreation", sourceBacking, resumed)
            invoke("undo")
            assertEquals("One normal pen stroke restores the original backing with Undo", sourceBacking, backing(archive("resumed-pen-undo")))
            action(obj("type" to "select_layer", "id" to curvesLayer))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "exposure")))
            genericNumberUndo("exposure")
            action(obj("type" to "layer", "action" to obj("op" to "delete", "id" to properties().getLong("layer"))))
            action(obj("type" to "select_layer", "id" to curvesLayer))
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun pointwiseColorPagesUseNativeControlsAndRetainHiddenValues() {
        fun p21Capture(name: String) {
            host.drain(); settle()
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true && !state().getJSONObject("filter_load").getBoolean("pending") }
            capture(name)
        }
        fun properties() = state().getJSONObject("layer_properties")
        fun controls() = properties().array("controls").objects()
        fun value(key: String) = controls().first { it.getString("key") == key }.getJSONObject("value").getDouble("value")
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun page(index: Int) {
            val selected = properties().array("pages").getJSONObject(index)
            if (properties().getString("page") == selected.getString("id")) return
            compose.onNodeWithTag("properties-page").performScrollTo().performClick()
            compose.onNodeWithText(selected.getString("label")).performClick()
            waitState { properties().getString("page") == selected.getString("id") }
        }
        fun edit(key: String, literal: String) {
            val label = controls().first { it.getString("key") == key }.getString("label")
            compose.onNodeWithTag("number-value-$key").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-$label")
            field.performTextReplacement(literal); field.performImeAction()
            waitState { kotlin.math.abs(value(key) - literal.toDouble()) < .0001 }
        }
        fun toggle(key: String) {
            val label = controls().first { it.getString("key") == key }.getString("label")
            compose.onNode(isToggleable() and hasAnySibling(hasText(label))).performScrollTo().performClick()
            settle()
        }
        action(obj("type" to "set_brush_size", "value" to 120))
        for ((index, color) in listOf(listOf(.9,.12,.08,1), listOf(.08,.7,.15,1), listOf(.1,.2,.9,1)).withIndex()) {
            action(obj("type" to "set_color", "rgba" to JSONArray(color)))
            val x = .4f + index * .1f
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(androidx.compose.ui.geometry.Offset(x, .4f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(androidx.compose.ui.geometry.Offset(x + .03f, .6f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(androidx.compose.ui.geometry.Offset(x + .03f, .6f)), MotionEvent.TOOL_TYPE_STYLUS)
        }
        host.drain(); settle()
        for (panel in listOf("navigator", "proof", "layers")) customize(obj("type" to "set_panel_visible", "panel" to panel, "visible" to false))
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "hue_saturation")))
            assertEquals(7, properties().array("pages").length())
            edit("hue", "17"); edit("saturation", "23"); edit("lightness", "-9")
            for ((index, range) in listOf("reds", "yellows", "greens", "cyans", "blues", "magentas").withIndex()) {
                page(index + 1)
                for ((key, literal) in listOf("hue" to "31", "saturation" to "-27", "lightness" to "13", "center" to "73", "width" to "19", "feather" to "41")) edit("${range}_$key", literal)
                p21Capture("p21-$theme-$range")
            }
            page(0)
            val hue = controls().first { it.getString("key") == "hue" }
            val slider = compose.onNodeWithTag("number-slider-${hue.getString("label")}").performScrollTo()
            slider.performTouchInput { swipe(androidx.compose.ui.geometry.Offset(width * .4f, height * .5f), androidx.compose.ui.geometry.Offset(width * .6f, height * .5f), 250) }
            waitState { value("hue") != 17.0 }
            host.drain(); settle()
            val after = value("hue")
            invoke("undo"); assertEquals(17.0, value("hue"), 0.0)
            invoke("redo"); assertEquals(after, value("hue"), 0.0)
            slider.performMouseInput {
                moveTo(androidx.compose.ui.geometry.Offset(width * .3f, height * .5f)); press()
                moveTo(androidx.compose.ui.geometry.Offset(width * .55f, height * .5f)); release()
            }
            waitState { value("hue") != after }
            host.drain(); settle()
            val mouseAfter = value("hue")
            invoke("undo"); assertEquals(after, value("hue"), 0.0)
            invoke("redo"); assertEquals(mouseAfter, value("hue"), 0.0)
            invoke("undo")
            val node = slider.fetchSemanticsNode()
            val owner = node.root as ViewRootForTest
            val bounds = node.boundsInRoot
            val downAt = SystemClock.uptimeMillis()
            val start = androidx.compose.ui.geometry.Offset(bounds.left + bounds.width * .2f, bounds.center.y)
            val end = androidx.compose.ui.geometry.Offset(bounds.left + bounds.width * .7f, bounds.center.y)
            touch(owner.view, downAt, MotionEvent.ACTION_DOWN, start, MotionEvent.TOOL_TYPE_STYLUS)
            touch(owner.view, downAt, MotionEvent.ACTION_MOVE, end, MotionEvent.TOOL_TYPE_STYLUS)
            touch(owner.view, downAt, MotionEvent.ACTION_UP, end, MotionEvent.TOOL_TYPE_STYLUS)
            waitState { value("hue") != after }
            host.drain(); settle()
            val penAfter = value("hue")
            invoke("undo"); assertEquals(after, value("hue"), 0.0)
            invoke("redo"); assertEquals(penAfter, value("hue"), 0.0)
            invoke("undo")
            toggle("colorize")
            assertEquals(1, properties().array("pages").length())
            compose.onNodeWithTag("properties-page").assertDoesNotExist()
            assertEquals(listOf("colorize_hue", "colorize_saturation", "lightness", "colorize"), controls().map { it.getString("key") })
            edit("colorize_hue", "193"); edit("colorize_saturation", "44")
            assertEquals(-9.0, value("lightness"), 0.0)
            p21Capture("p21-$theme-colorize")
            toggle("colorize")
            assertEquals(7, properties().array("pages").length())
            assertEquals(after, value("hue"), 0.0)
            for ((index, range) in listOf("reds", "yellows", "greens", "cyans", "blues", "magentas").withIndex()) {
                page(index + 1); assertEquals(31.0, value("${range}_hue"), 0.0)
                assertEquals(41.0, value("${range}_feather"), 0.0)
            }
            action(obj("type" to "set_layer_visibility", "id" to properties().getLong("layer"), "visible" to false))
            for (id in listOf("invert", "desaturate", "brightness_to_opacity", "threshold", "photo_filter")) {
                action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to id)))
                when (id) {
                    "invert", "desaturate", "brightness_to_opacity" -> {
                        assertTrue(controls().isEmpty())
                        if (id == "brightness_to_opacity") p21Capture("p21-$theme-brightness-to-opacity")
                    }
                    "threshold" -> {
                        fun choice(key: String, index: Int) {
                            val control = controls().single { it.getString("key") == key }
                            compose.onNodeWithTag("property-$key").performScrollTo().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .8f, height * .5f)) }
                            compose.onNode(hasText(control.getJSONObject("kind").array("options").getString(index)) and hasAnyAncestor(isPopup())).performClick()
                            waitState { controls().single { it.getString("key") == key }.getJSONObject("value").getInt("value") == index }
                        }
                        edit("threshold", "0.73")
                        assertEquals(listOf("threshold", "colors", "transparency"), controls().map { it.getString("key") })
                        choice("colors", 1); choice("transparency", 1); edit("alpha_threshold", "37")
                        p21Capture("p21-$theme-threshold")
                        choice("transparency", 0)
                        assertFalse(controls().any { it.getString("key") == "alpha_threshold" })
                        invoke("undo"); assertEquals(37.0, value("alpha_threshold"), 0.0)
                        invoke("redo"); choice("transparency", 1)
                        assertEquals(37.0, value("alpha_threshold"), 0.0)
                        choice("colors", 2); p21Capture("p21-$theme-threshold-white")
                        val authoredThreshold = value("threshold")
                        val archive = File(device.root, "illustration-$theme.capy")
                        host.writeDrawingCopy(archive, compose)
                        compose.runOnUiThread { assertTrue(host.documents.openUris(listOf(android.net.Uri.fromFile(archive)))) }
                        host.awaitMain("reopened illustration", 60_000, {
                            "blocked=${host.documentInputBlocked}, switching=${host.drawingTabs.switching}, brush=${host.snapshot?.optBoolean("brush_ready")}, workspace=${host.workspaceManager}, file=${state().getJSONObject("document_file")}, requests=${state().array("requests")}, load=${state().getJSONObject("filter_load")}, commands=${state().array("commands").objects().filter { it.getString("id") in listOf("open_document", "save_document_as") }}"
                        }, compose) {
                            host.snapshot?.optBoolean("brush_ready") == true &&
                                state().getJSONObject("document_file").optString("location").contains(archive.name) &&
                                !host.documentInputBlocked && !host.drawingTabs.switching
                        }
                        val reopened = state().array("layers").objects().single { it.optString("label") == "Threshold" && it.getBoolean("visible") }.getLong("id")
                        action(obj("type" to "select_layer", "id" to reopened))
                        assertEquals(authoredThreshold, value("threshold"), 0.0)
                        assertEquals(2.0, value("colors"), 0.0)
                        assertEquals(1.0, value("transparency"), 0.0)
                        assertEquals(37.0, value("alpha_threshold"), 0.0)
                        p21Capture("p21-$theme-threshold-reopened")
                    }
                    "photo_filter" -> {
                        edit("density", "67"); toggle("preserve_luminance")
                        val tagged = controls().first { it.getString("key") == "color" }.getJSONObject("value").toString()
                        compose.onNodeWithTag("property-color-${controls().first { it.getString("key") == "color" }.getString("label")}").performScrollTo().performClick()
                        compose.onNodeWithText("Use Color").performClick()
                        compose.waitUntil(10_000) { compose.onAllNodesWithText("Use Color").fetchSemanticsNodes().isEmpty() }
                        host.drain(); settle()
                        waitState { host.snapshot?.optBoolean("brush_ready") == true && !it.getJSONObject("filter_load").getBoolean("pending") }
                        assertEquals(tagged, controls().first { it.getString("key") == "color" }.getJSONObject("value").toString())
                        p21Capture("p21-$theme-photo-filter")
                    }
                }
                action(obj("type" to "set_layer_visibility", "id" to properties().getLong("layer"), "visible" to false))
            }
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun selectiveColorAndMixerRetainNativePagesAndHiddenValues() {
        fun p22Capture(name: String) {
            host.drain(); settle()
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true && !state().getJSONObject("filter_load").getBoolean("pending") }
            capture(name)
        }
        fun properties() = state().getJSONObject("layer_properties")
        fun controls() = properties().array("controls").objects()
        fun value(key: String) = controls().first { it.getString("key") == key }.getJSONObject("value").getDouble("value")
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun page(index: Int) {
            val selected = properties().array("pages").getJSONObject(index)
            if (properties().getString("page") == selected.getString("id")) return
            compose.onNodeWithTag("properties-page").performScrollTo().performClick()
            compose.onNode(hasText(selected.getString("label")) and hasClickAction()).performClick()
            waitState { properties().getString("page") == selected.getString("id") }
        }
        fun edit(key: String, literal: String) {
            val label = controls().first { it.getString("key") == key }.getString("label")
            compose.onNodeWithTag("number-value-$key").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-$label")
            field.performTextReplacement(literal); field.performImeAction()
            waitState { kotlin.math.abs(value(key) - literal.toDouble()) < .0001 }
        }
        fun toggle(key: String) {
            val label = controls().first { it.getString("key") == key }.getString("label")
            compose.onNode(isToggleable() and hasAnySibling(hasText(label))).performScrollTo().performClick()
            settle()
        }
        action(obj("type" to "set_brush_size", "value" to 120))
        for ((index, color) in listOf(listOf(.9,.12,.08,1), listOf(.08,.7,.15,1), listOf(.1,.2,.9,1)).withIndex()) {
            action(obj("type" to "set_color", "rgba" to JSONArray(color)))
            val x = .4f + index * .1f
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(androidx.compose.ui.geometry.Offset(x, .4f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(androidx.compose.ui.geometry.Offset(x + .03f, .6f)), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(androidx.compose.ui.geometry.Offset(x + .03f, .6f)), MotionEvent.TOOL_TYPE_STYLUS)
        }
        host.drain(); settle()
        for (panel in listOf("navigator", "proof", "layers")) customize(obj("type" to "set_panel_visible", "panel" to panel, "visible" to false))
        fun numberUndo(key: String) {
            val label = controls().first { it.getString("key") == key }.getString("label")
            val slider = compose.onNodeWithTag("number-slider-$label").performScrollTo()
            for (tool in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                val before = value(key)
                when (tool) {
                    MotionEvent.TOOL_TYPE_FINGER -> slider.performTouchInput { swipe(androidx.compose.ui.geometry.Offset(width * .4f, height * .5f), androidx.compose.ui.geometry.Offset(width * .6f, height * .5f), 250) }
                    MotionEvent.TOOL_TYPE_MOUSE -> slider.performMouseInput {
                        moveTo(androidx.compose.ui.geometry.Offset(width * .3f, height * .5f)); press()
                        moveTo(androidx.compose.ui.geometry.Offset(width * .55f, height * .5f)); release()
                    }
                    else -> {
                        val node = slider.fetchSemanticsNode(); val owner = node.root as ViewRootForTest
                        val bounds = node.boundsInRoot; val downAt = SystemClock.uptimeMillis()
                        val start = androidx.compose.ui.geometry.Offset(bounds.left + bounds.width * .2f, bounds.center.y)
                        val end = androidx.compose.ui.geometry.Offset(bounds.left + bounds.width * .7f, bounds.center.y)
                        touch(owner.view, downAt, MotionEvent.ACTION_DOWN, start, tool)
                        touch(owner.view, downAt, MotionEvent.ACTION_MOVE, end, tool)
                        touch(owner.view, downAt, MotionEvent.ACTION_UP, end, tool)
                    }
                }
                host.drain(); settle()
                val after = value(key); assertNotEquals(before, after)
                invoke("undo"); assertEquals(before, value(key), 0.0)
                invoke("redo"); assertEquals(after, value(key), 0.0)
                invoke("undo")
            }
        }
        fun cancelDraft(key: String) {
            val before = value(key)
            val label = controls().first { it.getString("key") == key }.getString("label")
            compose.onNodeWithTag("number-value-$key").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-$label")
            field.assertIsFocused(); field.performTextReplacement("47.125")
            val now = SystemClock.uptimeMillis()
            for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
                assertTrue(instrumentation.uiAutomation.injectInputEvent(KeyEvent(now, now, phase, KeyEvent.KEYCODE_ESCAPE, 0), true))
            host.drain(); settle()
            assertEquals(before, value(key), 0.0)
        }
        fun choice(key: String, index: Int) {
            val c = controls().first { it.getString("key") == key }
            compose.onNodeWithTag("property-$key").performScrollTo().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .8f, height * .5f)) }
            compose.onNodeWithText(c.getJSONObject("kind").array("options").getString(index)).performClick()
            host.drain(); settle()
            assertEquals(index, controls().first { it.getString("key") == key }.getJSONObject("value").getInt("value"))
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "selective_color")))
            val ranges = listOf("reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks")
            assertEquals(ranges, properties().array("pages").objects().map { it.getString("id") })
            for ((index, range) in ranges.withIndex()) {
                page(index)
                for ((key, literal) in listOf("cyan" to "-17.25", "magenta" to "23.5", "yellow" to "-31.75", "black" to "9.25")) edit("${range}_$key", literal)
                if (index == 0) { cancelDraft("reds_cyan"); numberUndo("reds_cyan") }
                p22Capture("p22-$theme-selective-$range")
            }
            choice("mode", 1)
            page(0); assertEquals(-17.25, value("reds_cyan"), 0.0)
            choice("mode", 0)
            for ((index, range) in ranges.withIndex()) { page(index); assertEquals(9.25, value("${range}_black"), 0.0) }
            action(obj("type" to "set_layer_visibility", "id" to properties().getLong("layer"), "visible" to false))
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "channel_mixer")))
            assertEquals(listOf("red", "green", "blue"), properties().array("pages").objects().map { it.getString("id") })
            for ((index, channel) in listOf("red", "green", "blue").withIndex()) {
                page(index)
                for ((key, literal) in listOf("red" to "83.25", "green" to "-12.5", "blue" to "24.75", "constant" to "3.25")) edit("${channel}_$key", literal)
                p22Capture("p22-$theme-mixer-$channel")
            }
            cancelDraft("blue_red"); numberUndo("blue_red")
            toggle("monochrome")
            assertEquals(listOf("gray"), properties().array("pages").objects().map { it.getString("id") })
            compose.onNodeWithTag("properties-page").assertDoesNotExist()
            assertEquals(listOf("gray_red", "gray_green", "gray_blue", "gray_constant", "monochrome"), controls().map { it.getString("key") })
            for ((key, literal) in listOf("red" to "31.25", "green" to "62.5", "blue" to "6.25", "constant" to "-4.25")) edit("gray_$key", literal)
            numberUndo("gray_green"); p22Capture("p22-$theme-mixer-gray")
            toggle("monochrome")
            assertEquals(3, properties().array("pages").length())
            for ((index, channel) in listOf("red", "green", "blue").withIndex()) { page(index); assertEquals(83.25, value("${channel}_red"), 0.0) }
            toggle("monochrome"); assertEquals(31.25, value("gray_red"), 0.0)
            action(obj("type" to "set_layer_visibility", "id" to properties().getLong("layer"), "visible" to false))
            assertNull(host.failure); assertNull(host.actionError)
        }
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
        assertEquals(48, choices.size)
        assertEquals("The picker lists both fill generators", listOf("solid_color","gradient_fill"), choices.filter { it in listOf("solid_color","gradient_fill") })
        action(obj("type" to "select_panel_tab", "group" to group("adjustments").getLong("id"), "panel" to "adjustments"))
        compose.waitUntil(20_000) { host.bundledPreviews.get("${choices.first()}.webp") != null }
        val strip = compose.onNodeWithTag("filter-preview-${choices.first()}", useUnmergedTree=true).getUnclippedBoundsInRoot()
        assertEquals("Filter previews keep the tiles' 6:1 shape", 6f, (strip.right - strip.left).value / (strip.bottom - strip.top).value, .1f)
        val preview = host.bundledPreviews.get("${choices.first()}.webp")!!.toPixelMap()
        assertTrue("Bundled preview shows the tiles", (0 until preview.width).any { preview[it,preview.height/2].alpha>.5f })
        assertEquals("Built-in rows request no GPU previews", 0L, host.filterPreviewCache.request)
        capture("adjustments-picker")
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to "distort")))
        compose.onNodeWithTag("filter-search-toggle").performClick()
        compose.waitUntil(10_000) { compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search"))).fetchSemanticsNodes().isNotEmpty() }
        val filterSearch = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search")))
        filterSearch.performTextInput("glass")
        waitState { it.array("adjustments").objects().map { c -> c.getString("id") }==listOf("glass","rainy_glass") }
        compose.waitUntil(20_000) { host.bundledPreviews.get("rainy_glass.webp") != null }
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
                waitState {it.getJSONObject("layer_properties").getJSONArray("controls").getJSONObject(0).getJSONObject("value").getJSONObject("value").getJSONArray("stops").length()==3}
                action(obj("type" to "effect", "action" to obj("op" to "gradient", "target" to controls.first { it.getString("key")=="gradient" }.getJSONObject("gradient").getJSONObject("destination"),
                    "edit" to obj("kind" to "stop", "index" to 1, "position" to .5, "color" to obj("space" to "Srgb", "rgba" to JSONArray(listOf(.8,.2,.1,1))), "remove" to false))))
                if(controls.any { it.getString("key")=="amount" }) action(obj("type" to "effect", "action" to obj("op" to "reset", "layer" to view.getLong("layer"), "key" to "amount")))
            }
            capture("adjustment-$id")
            action(obj("type" to "set_layer_visibility", "id" to view.getLong("layer"), "visible" to false))
        }
    }
    @Test fun strokeRecordingSavesRawStylusInput() {
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true))
        floatPanel("stats", 500f, 120f)
        // Diagnostics arrive asynchronously and move the recording button down.
        compose.waitUntil(5_000) { compose.onAllNodesWithTag("renderer-stats-chart").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("stroke-recording").performScrollTo().assertIsDisplayed().assertIsEnabled().performClick()
        compose.waitUntil(5_000) { host.strokeRecording.status?.optBoolean("recording") == true }
        compose.onNodeWithTag("stroke-recording").assertTextContains("Stop stroke recording")
        // Keep the floating panel clear of injected pen input on smaller tablets.
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to false))
        penStroke(40)
        compose.waitUntil(5_000) { (host.strokeRecording.status?.optLong("raw_events") ?: 0) >= 41 }
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true))
        compose.onNodeWithTag("stroke-recording").performScrollTo().performClick()
        fun node(predicate: (android.view.accessibility.AccessibilityNodeInfo) -> Boolean): android.view.accessibility.AccessibilityNodeInfo? {
            fun find(n: android.view.accessibility.AccessibilityNodeInfo?): android.view.accessibility.AccessibilityNodeInfo? {
                n ?: return null
                if (predicate(n)) return n
                for (i in 0 until n.childCount) find(n.getChild(i))?.let { return it }
                return null
            }
            return find(instrumentation.uiAutomation.rootInActiveWindow)
        }
        fun chooser(): android.view.accessibility.AccessibilityNodeInfo {
            val until = SystemClock.uptimeMillis() + 15_000
            while (SystemClock.uptimeMillis() < until) {
                node { it.isEditable && it.packageName?.toString()?.contains("documentsui") == true }?.let { return it }
                SystemClock.sleep(100)
            }
            error("Recording system save chooser did not open")
        }
        compose.waitUntil(5_000) { host.strokeRecording.busy && host.strokeRecording.status?.optBoolean("ready") == true }
        chooser()
        instrumentation.uiAutomation.performGlobalAction(android.accessibilityservice.AccessibilityService.GLOBAL_ACTION_BACK)
        compose.waitUntil(5_000) { !host.strokeRecording.busy }
        assertTrue(host.strokeRecording.status!!.getBoolean("ready"))
        compose.onNodeWithTag("stroke-recording").performScrollTo().assertTextContains("Save stroke recording").performClick()
        compose.waitUntil(5_000) { host.strokeRecording.busy }
        val name = "capy-stroke-test-${System.currentTimeMillis()}.capystrokes"
        assertTrue(chooser().performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_SET_TEXT, android.os.Bundle().apply {
            putCharSequence(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, name)
        }))
        val save = node { it.isClickable && it.text?.toString()?.equals("save", ignoreCase = true) == true }
        assertNotNull("System Save action", save)
        assertTrue(save!!.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
        compose.waitUntil(15_000) { !host.strokeRecording.busy && host.strokeRecording.status?.optBoolean("ready") == false }
        assertNull(host.actionError)
        compose.onNodeWithTag("stroke-recording").assertTextContains("Start stroke recording")
        // The native workspace owner revalidates asynchronously after SAF returns.
        compose.waitUntil(10_000) {
            kotlinx.coroutines.runBlocking {
                host.withNative { handle -> runCatching {
                    Native.dispatch(handle, obj("type" to "set_theme", "theme" to "light").toString())
                }.isSuccess }
            }
        }
        println("STROKE_RECORDING_FILE=$name")
    }

    @Test fun diagnosticsFollowSharedOrder() {
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true))
        floatPanel("stats", 650f, 120f)
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("renderer-stats-chart").fetchSemanticsNodes().isNotEmpty() }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            val gpu = compose.onNodeWithText("GPU · ms").fetchSemanticsNode().boundsInRoot
            val chart = compose.onNodeWithTag("renderer-stats-chart").fetchSemanticsNode().boundsInRoot
            val frames = compose.onNodeWithText("Frames").fetchSemanticsNode().boundsInRoot
            val storage = compose.onNodeWithText("Canvas storage").fetchSemanticsNode().boundsInRoot
            assertTrue(chart.top >= gpu.bottom && frames.top >= chart.bottom && storage.top >= frames.bottom)
            capture("diagnostics-order-$theme")
        }
    }
    private fun preferences() = host.snapshot!!.getJSONObject("preferences")
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
        val button = compose.onNode(hasText("Window") and hasClickAction())
        val anchor = screenBounds(button)
        button.performClick()
        val menu = screenBounds(compose.onNodeWithTag("workspace-menu"))
        assertEquals("Workspace menu aligns with its header button", anchor.left, menu.left, 2f)
        // Material dropdowns reserve 48 dp at the top and bottom of the screen.
        val menuTop = maxOf(anchor.bottom, 48f * compose.activity.resources.displayMetrics.density)
        assertEquals("Workspace menu opens below its header button within the screen margin", menuTop, menu.top, 2f)
    }
    private fun capyTag() = "zen-button"
    private fun touch(view: View, downAt: Long, action: Int, point: androidx.compose.ui.geometry.Offset, tool: Int = MotionEvent.TOOL_TYPE_FINGER) = instrumentation.runOnMainSync {
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = 1f })
        val event = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, if (tool == MotionEvent.TOOL_TYPE_STYLUS) InputDevice.SOURCE_STYLUS else InputDevice.SOURCE_TOUCHSCREEN, 0)
        view.dispatchTouchEvent(event); event.recycle()
    }
    private fun slide(view: View, downAt: Long, from: androidx.compose.ui.geometry.Offset, to: androidx.compose.ui.geometry.Offset) {
        for (step in 1..12) { touch(view, downAt, MotionEvent.ACTION_MOVE, from + (to - from) * (step / 12f)); SystemClock.sleep(16) }
    }
    private fun holdDrag(source: SemanticsNodeInteraction, target: androidx.compose.ui.geometry.Offset) {
        val node = source.fetchSemanticsNode()
        val view = (node.root as ViewRootForTest).view
        val start = node.boundsInRoot.center
        val downAt = SystemClock.uptimeMillis()
        touch(view, downAt, MotionEvent.ACTION_DOWN, start)
        compose.mainClock.advanceTimeBy(android.view.ViewConfiguration.getLongPressTimeout() + 100L)
        slide(view, downAt, start, target)
        touch(view, downAt, MotionEvent.ACTION_UP, target)
    }
    private fun quickAccessToolbarsMenu() {
        workspaceMenu()
        compose.onNodeWithText("Quick Access Toolbars").performClick()
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
    private fun openSettings() {
        compose.onNodeWithContentDescription("Settings").assertIsDisplayed().performClick()
        compose.waitUntil(10_000) { host.snapshot?.objectOrNull("preferences") != null }
    }
    private fun waitEnabled(tag: String) = compose.waitUntil(10_000) {
        compose.onAllNodesWithTag(tag).fetchSemanticsNodes().singleOrNull()?.config?.contains(SemanticsProperties.Disabled) == false
    }
    private fun waitState(test: (JSONObject) -> Boolean) = compose.waitUntil(10_000) { test(state()) }
    private fun findCanvas(view: View) = view.descendant<CanvasSurfaceView>()
    private fun penStroke(steps: Int = 60) {
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        instrumentation.runOnMainSync {
            canvas = findCanvas(compose.activity.window.decorView)!!
            canvas.getLocationOnScreen(location)
        }
        val area = host.snapshot!!.getJSONObject("layout").getJSONObject("work_area")
        val density = compose.activity.resources.displayMetrics.density
        val down = SystemClock.uptimeMillis()
        for (i in 0..steps) {
            val coords = MotionEvent.PointerCoords().apply {
                x = (area.number("x") + area.number("width") * (0.35f + i.toFloat() / steps * 0.3f)) * density + location[0]
                y = (area.number("y") + area.number("height") * (0.45f + kotlin.math.sin(i / 12f) * 0.08f)) * density + location[1]
                pressure = 0.3f + i.toFloat() / steps * 0.6f
                setAxisValue(MotionEvent.AXIS_TILT, 0.3f)
                setAxisValue(MotionEvent.AXIS_ORIENTATION, 0.2f)
            }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val action = when (i) { 0 -> MotionEvent.ACTION_DOWN; steps -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, arrayOf(props), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
            assertTrue("Stylus event accepted", instrumentation.uiAutomation.injectInputEvent(event, true))
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
        action(obj("type" to "select_panel_tab", "group" to group("sizes").getInt("id"), "panel" to "sizes"))
        val presets = listOf("0.7","1","1.5","2","2.5","3").map { compose.onNodeWithTag("size-preset-$it").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot() }
        presets.zipWithNext().forEach { (left,right) ->
            assertEquals(left.top,right.top,1f)
            assertTrue("Tooltip wrappers must preserve the six-column grid",left.right<=right.left+1f)
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
        compose.onNodeWithContentDescription(host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("add_mask")).performClick()
        waitState { it.getJSONObject("layer_tools").getJSONObject("editing_layer").getBoolean("has_mask") }
        SystemClock.sleep(800)
        capture("layers-paint-mask-light")
        action(obj("type" to "set_theme","theme" to "dark"))
        SystemClock.sleep(300)
        capture("layers-paint-mask-dark")
        compose.onNodeWithContentDescription("Layer actions").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Delete mask").fetchSemanticsNodes().isNotEmpty() }
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
    @Test fun panelDraggingPreservesTabVisibility() {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            fun tabs(id: Int, vararg panels: String) = obj("kind" to "tabs", "id" to id, "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to "automatic")
            put("bands", JSONArray(listOf(obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "sizes")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray()); put("column_scroll", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val workspace = compose.onNodeWithTag("workspace")
        fun saved() = state().getJSONObject("workspace").toString()
        fun visible(hidden: Boolean) {
            assertEquals(!hidden, group("sizes").getBoolean("tabs_visible"))
            if (hidden) compose.onNodeWithTag("tab-sizes").assertDoesNotExist()
            else compose.onNodeWithTag("tab-sizes").assertIsDisplayed()
        }
        for (mouse in listOf(true, false)) for (hidden in listOf(false, true)) {
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            customize(obj("type" to "set_tab_hidden", "panel" to "sizes", "hidden" to hidden))
            val id = group("sizes").getInt("id")
            val root = workspace.fetchSemanticsNode().boundsInRoot
            fun begin(end: androidx.compose.ui.geometry.Offset) {
                val start = compose.onNodeWithTag("group-grip-$id").fetchSemanticsNode().boundsInRoot.center - root.topLeft
                if (mouse) workspace.performMouseInput { moveTo(start); press(); moveTo(end, 300) }
                else workspace.performTouchInput { down(start); moveTo(end, 300) }
                compose.waitForIdle()
            }
            fun finish(cancelled: Boolean = false) {
                if (mouse) workspace.performMouseInput { if (cancelled) cancel() else release() }
                else workspace.performTouchInput { if (cancelled) cancel() else up() }
                compose.waitForIdle(); assertNull(host.actionError)
            }
            fun history(before: String, after: String) {
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(before, saved())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(after, saved())
            }
            val before = saved()
            val center = androidx.compose.ui.geometry.Offset(root.width * .5f, root.height * .55f)
            begin(center)
            compose.waitUntil(10_000) { group("sizes").getBoolean("floating") }
            visible(hidden)
            finish(true); assertEquals(before, saved()); visible(hidden)
            begin(center)
            compose.waitUntil(10_000) { group("sizes").getBoolean("floating") }
            visible(hidden); finish()
            val floated = saved(); history(before, floated)
            begin(androidx.compose.ui.geometry.Offset(root.width - 2f, root.height * .5f)); finish()
            compose.waitUntil(10_000) { !group("sizes").getBoolean("floating") }
            visible(hidden)
            val docked = saved(); history(floated, docked); visible(hidden)
            capture("panel-drag-tabs-${if (mouse) "mouse" else "touch"}-$hidden")
        }
    }

    @Test fun workspaceMenusManageVisibilityNamesAndHistory() {
        fun toolSet() = compose.onNode(hasText("Tool Set") and hasAnyAncestor(hasTestTag("workspace-menu")))
        workspaceMenu()
        capture("workspace-menu-light")
        toolSet().performClick()
        compose.waitUntil(10_000) { groups().none { "brushes" in it.array("panels").values() } }
        workspaceMenu(); toolSet().performClick()
        compose.waitUntil(10_000) { groups().any { "brushes" in it.array("panels").values() } }
        val destination = group("layers").getInt("id")
        contextGrip("group-grip-$destination")
        compose.onNodeWithText("Add built-in panel").performClick()
        assertContextBeside("group-grip-$destination")
        capture("workspace-panel-grip-add-panel")
        compose.onNodeWithText("Tool Set panel").performClick()
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
        workspaceMenu(); compose.onNodeWithText("Undo Layout Change").performClick()
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
        quickAccessToolbarsMenu(); compose.onNodeWithText("Manage Toolbars…").performClick()
        compose.onNodeWithTag("managed-toolbar-$copy").performClick()
        waitEnabled("delete-managed-toolbar")
        compose.onNodeWithTag("delete-managed-toolbar").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
        val messageLayout = mutableListOf<TextLayoutResult>()
        compose.onNodeWithTag("toolbar-prompt-message").performSemanticsAction(SemanticsActions.GetTextLayoutResult) {
            assertTrue(it(messageLayout))
        }
        val text = messageLayout.single()
        assertEquals("Copy remains verbatim from Rust", host.snapshot!!.getJSONObject("toolbar_prompt").getString("message"), text.layoutInput.text.text)
        capture("workspace-delete-prompt")
        compose.onNodeWithText("Delete Toolbar", substring = false).performClick()
        compose.waitUntil(10_000) { groups().none { copy in it.array("panels").values() } }
        compose.onNodeWithTag("close-toolbar-manager").performClick()
        workspaceMenu(); compose.onNodeWithText("Undo Layout Change").performClick()
        compose.waitUntil(10_000) { groups().any { copy in it.array("panels").values() } }
        action(obj("type" to "set_theme", "theme" to "dark"))
        workspaceMenu(); capture("workspace-menu-dark")
        compose.onNodeWithText("Redo Layout Change").performClick()
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
            quickAccessToolbarsMenu(); compose.onNodeWithText("Manage Toolbars…").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_manager") != null }
            val managed = host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").length()
            compose.onNodeWithTag("delete-managed-toolbar").assertIsNotEnabled()
            capture("toolbar-manager-$theme-initial")
            compose.onNodeWithTag("managed-toolbar-$hidden").performClick()
            waitEnabled("delete-managed-toolbar")
            capture("toolbar-manager-$theme-selected")
            compose.onNodeWithTag("delete-managed-toolbar").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
            capture("toolbar-manager-$theme-confirm")
            compose.onNodeWithText("Cancel").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") == null }
            assertEquals(before, state().getJSONObject("workspace").toString())
            compose.onNodeWithTag("delete-managed-toolbar").performClick()
            compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
            compose.onNodeWithText("Delete Toolbar", substring = false).performClick()
            compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").length() == managed - 1 }
            compose.onNodeWithTag("delete-managed-toolbar").assertIsNotEnabled()
            capture("toolbar-manager-$theme-deleted")
            while (host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").length() > 0) {
                val panel = host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").objects().first().getString("panel")
                compose.onNodeWithTag("managed-toolbar-$panel").performClick()
                waitEnabled("delete-managed-toolbar")
                compose.onNodeWithTag("delete-managed-toolbar").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("toolbar_prompt") != null }
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
        fun automaticNames(panels: List<String>): List<Boolean> {
            for (panel in panels) compose.onNodeWithTag("tab-icon-$panel", useUnmergedTree = true).assertExists()
            val names = panels.map { compose.onAllNodesWithTag("tab-name-$it", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
            val resolved = group(panels.first())
            val members = resolved.array("panels").values()
            assertEquals("Automatic names follow the resolved layout", panels.map { panel ->
                resolved.array("tab_presentations").getJSONObject(members.indexOf(panel)).getBoolean("show_name")
            }, names)
            return names
        }
        for (theme in listOf("dark", "light")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((style, label) in listOf("automatic" to "Automatic", "active_name" to "Icons and active tab name", "icon_name" to "Icons and names", "name" to "Names only", "icon" to "Icons only")) {
                contextGrip("group-grip-$id")
                compose.onNodeWithText(label).performClick()
                var fitted: List<Boolean>? = null
                for (active in listOf("brushes", "sizes", "layers")) {
                    compose.onNodeWithTag("tab-$active").performClick()
                    compose.waitUntil(10_000) { group(active).getString("active") == active }
                    if (style == "automatic") {
                        val names = automaticNames(listOf("brushes", "sizes", "layers"))
                        assertEquals("Selection does not change automatic names", fitted ?: names, names); fitted = names
                    } else for (panel in listOf("brushes", "sizes", "layers")) {
                        val icon = compose.onNodeWithTag("tab-icon-$panel", useUnmergedTree = true)
                        val name = compose.onNodeWithTag("tab-name-$panel", useUnmergedTree = true)
                        if (style != "name") icon.assertExists() else icon.assertDoesNotExist()
                        if (style == "icon_name" || style == "name" || (style == "active_name" && panel == active)) name.assertExists() else name.assertDoesNotExist()
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
            automaticNames(listOf("brushes", "sizes", "layers"))
        }
        contextGrip("tab-layers")
        compose.onNodeWithText("Icons only").assertDoesNotExist()
        compose.onNodeWithText("Configure Layers panel…").performClick()
        customize(obj("type" to "close_expanded"))
    }

    @Test fun crowdedTabReordersKeepPanelWidthsAndBodies() {
        val members=listOf("brushes","stats","sizes","color","palettes","tool_settings")
        val fixture=JSONObject(defaultWorkspace)
        fun tabs(id:Int,panels:List<String>,active:String=panels.first())=obj("kind" to "tabs","id" to id,
            "panels" to JSONArray(panels),"active" to active,"tab_style" to "automatic")
        fixture.getJSONObject("layout").apply {
            put("bands",JSONArray(listOf(
                obj("id" to 200,"edge" to "left","extent" to 252,"root" to obj("kind" to "split","id" to 202,
                    "axis" to "vertical","fraction" to .65,"first" to tabs(201,members,"tool_settings"),"second" to tabs(203,listOf("navigator")))),
                obj("id" to 210,"edge" to "right","extent" to 252,"root" to tabs(211,listOf("layers"))))))
            put("floating",JSONArray());put("collapsed",JSONArray());put("column_stacks",JSONArray())
            put("fit_tab_groups",JSONArray(listOf(201)));put("fit_height_groups",JSONArray(listOf(201)))
            put("next_id",maxOf(212,getInt("next_id")))
        }
        fun nativeTabsReady():Boolean {
            val requests=host.snapshot!!.getJSONObject("layout").array("measurement_requests").objects()
            if(!members.all {panel->requests.any {it.getString("panel")==panel && it.getBoolean("tab_measured")}} ||
                requests.none {it.getString("panel")=="tool_settings" && it.getBoolean("body_measured")})return false
            return members.map {panel->
                compose.onAllNodesWithTag("tab-$panel").fetchSemanticsNodes().singleOrNull()?.boundsInRoot?.left
            }.let {positions->positions.all {it!=null} && positions.filterNotNull().zipWithNext().all {(a,b)->a<b}}
        }
        for(tag in listOf("en","ru"))for(theme in listOf("light","dark")) {
            val language=host.bootstrap!!.array("shipped_tags").values().indexOf(tag)+1
            action(obj("type" to "preferences","action" to obj("type" to "edit","id" to "language","value" to language)))
            host.awaitMain("tab language $tag",30_000,{host.languageTag},compose){host.languageTag==tag}
            action(obj("type" to "set_theme","theme" to theme))
            action(obj("type" to "restore_workspace","workspace" to fixture))
            host.awaitMain("crowded tab fixture $tag $theme",10_000,{state().getJSONObject("workspace").toString()},compose) {
                state().getString("theme")==theme && state().getJSONObject("workspace").getJSONObject("layout").array("fit_tab_groups").values()==listOf(201) &&
                    groups().any {it.getInt("id")==201 && it.array("panels").values()==members}
            }
            compose.waitUntil(10_000){nativeTabsReady()}
            val body=compose.onNodeWithTag("panel-body-tool_settings",useUnmergedTree=true)
            val controls=compose.onNodeWithTag("tool-setting-size",useUnmergedTree=true)
            val identity=controls.fetchSemanticsNode().id
            val divider=compose.onNodeWithTag("divider-200")
            divider.performSemanticsAction(SemanticsActions.RequestFocus){it()}
            compose.runOnIdle {assertEquals(200,host.dividerFocus)}
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_DPAD_LEFT)
            host.awaitMain("crowded tab resize $tag $theme",10_000,{
                state().getJSONObject("workspace").getJSONObject("layout").array("fit_tab_groups").toString()
            },compose){201 !in state().getJSONObject("workspace").getJSONObject("layout").array("fit_tab_groups").values()}
            compose.waitUntil(10_000){nativeTabsReady()}
            assertEquals(identity,controls.fetchSemanticsNode().id)
            val before=group("tool_settings").array("panels").values()
            val source=compose.onNodeWithTag("tab-tool_settings")
            val origin=source.fetchSemanticsNode().boundsInRoot.topLeft
            val target=compose.onNodeWithTag("tab-${before.first()}").fetchSemanticsNode().boundsInRoot.center
            source.performTouchInput { swipe(center,target-origin,700) }
            compose.waitUntil(10_000){groups().any {
                it.getInt("id")==201 && it.array("panels").values().let {order->order.toSet()==before.toSet() && order!=before}
            }}
            assertEquals(identity,controls.fetchSemanticsNode().id)
            val widths=mutableListOf<Float>()
            val finished=CountDownLatch(1)
            compose.runOnIdle {
                val clock=Choreographer.getInstance()
                clock.postFrameCallback(object:Choreographer.FrameCallback {
                    override fun doFrame(time:Long) {
                        widths.add(group("tool_settings").getJSONObject("bounds").number("width"))
                        if(widths.size==90)finished.countDown() else clock.postFrameCallback(this)
                    }
                })
            }
            assertTrue(finished.await(10,TimeUnit.SECONDS))
            assertEquals("$tag $theme widths after internal drop: $widths",1,widths.distinct().size)
            val native=body.fetchSemanticsNode().boundsInRoot.width
            assertEquals(widths.first()*compose.activity.resources.displayMetrics.density,native,1f)
            capture("crowded-tabs-$tag-$theme")
        }
    }

    @Test fun activeTabsMatchTheirPanelsAtEveryTransparency() {
        val density = compose.activity.resources.displayMetrics.density
        val levels = listOf("off", "low", "medium", "high")
        val mismatches = mutableListOf<String>()
        for (theme in listOf("dark", "light")) for ((index, level) in levels.withIndex()) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to index)))
            compose.waitUntil(10_000) { state().optString("theme") == theme && state().getJSONObject("settings").getString("transparency") == level }
            settle()
            val image = capture("active-tab-$theme-$level")
            val tabbed = groups().filter { it.getBoolean("tabs_visible") }
            assertTrue("The workspace has docked tab groups", tabbed.isNotEmpty())
            for (group in tabbed) {
                val active = group.getString("active")
                val tab = screenBounds(compose.onNodeWithTag("tab-$active"))
                val body = screenBounds(compose.onNodeWithTag("panel-body-$active", useUnmergedTree = true))
                val x = (tab.left + 4 * density).toInt()
                val tabPixel = image.getPixel(x, (tab.bottom - 3 * density).toInt())
                val bodyPixel = image.getPixel(x, (body.top + 3 * density).toInt())
                if (listOf(16, 8, 0).any { kotlin.math.abs((tabPixel shr it and 255) - (bodyPixel shr it and 255)) > 2 })
                    mismatches += "$theme $level group ${group.getInt("id")} $active: tab #%06X panel #%06X".format(tabPixel and 0xFFFFFF, bodyPixel and 0xFFFFFF)
            }
        }
        assertTrue("Active tabs differ from their panels:\n${mismatches.joinToString("\n")}", mismatches.isEmpty())
    }

    @Test fun activeTabContoursCompositeTheirBackgroundOnce() {
        fun coverage(path: android.graphics.Path, width: Int, height: Int): Bitmap {
            val node = android.graphics.RenderNode("tab-contour").apply { setPosition(0, 0, width, height) }
            val canvas = node.beginRecording(width, height)
            canvas.drawPath(path, android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply { color = android.graphics.Color.WHITE })
            node.endRecording()
            val reader = android.media.ImageReader.newInstance(width, height, android.graphics.PixelFormat.RGBA_8888, 1,
                android.hardware.HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE or android.hardware.HardwareBuffer.USAGE_GPU_COLOR_OUTPUT)
            val renderer = android.graphics.HardwareRenderer().apply {
                setOpaque(false); setSurface(reader.surface); setContentRoot(node)
            }
            try {
                renderer.createRenderRequest().setWaitForPresent(true).syncAndDraw()
                return checkNotNull(reader.acquireNextImage()).use { frame ->
                    checkNotNull(frame.hardwareBuffer).use { buffer ->
                        val bitmap = checkNotNull(Bitmap.wrapHardwareBuffer(buffer, android.graphics.ColorSpace.get(android.graphics.ColorSpace.Named.SRGB)))
                        try { bitmap.copy(Bitmap.Config.ARGB_8888, false) } finally { bitmap.recycle() }
                    }
                }
            } finally { renderer.destroy(); reader.close(); node.discardDisplayList() }
        }
        val panels = listOf("brushes", "sizes", "tool_settings", "navigator")
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, *panels.toTypedArray())),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val density = compose.activity.resources.displayMetrics.density
        val directory = File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }
        val mismatches = mutableListOf<String>()
        var covered = 0
        for (theme in listOf("light", "dark")) for (transparency in listOf(0, 3)) for (drawer in listOf(false, true)) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to transparency)))
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            if (drawer) {
                customize(obj("type" to "set_column_collapsed", "group" to 41, "collapsed" to true))
                customize(obj("type" to "set_column_drawers", "column" to 41, "drawers" to true))
                compose.onNodeWithTag("column-icon-brushes").performTouchInput { click() }
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-header-41").fetchSemanticsNodes().isNotEmpty() }
            }
            for (active in listOf(0, 1, 3)) {
                action(obj("type" to "select_panel_tab", "group" to 41, "panel" to panels[active]))
                settle()
                val header = screenBounds(compose.onNodeWithTag(if (drawer) "column-drawer-header-41" else "group-header-41"))
                val tab = screenBounds(compose.onNodeWithTag("${if (drawer) "drawer-" else ""}tab-${panels[active]}"))
                val inactive = screenBounds(compose.onNodeWithTag("${if (drawer) "drawer-" else ""}tab-${panels[if (active == 0) 1 else 0]}"))
                val image = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
                File(directory, "tab-contour-$theme-$transparency-$drawer-$active.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
                var mask: Bitmap? = null
                try {
                    val radius = 18f * density
                    val foot = 6f * density
                    val path = android.graphics.Path()
                    fun corner(center: androidx.compose.ui.geometry.Offset, start: androidx.compose.ui.geometry.Offset, end: androidx.compose.ui.geometry.Offset) {
                        squircleCorner(center, start, end).forEach { path.lineTo(it.x, it.y) }
                    }
                    fun point(x: Float, y: Float) = androidx.compose.ui.geometry.Offset(x, y)
                    path.moveTo(tab.left + radius, tab.top)
                    path.lineTo(tab.right - radius, tab.top)
                    corner(point(tab.right - radius, tab.top + radius), point(0f, -radius), point(radius, 0f))
                    path.lineTo(tab.right, tab.bottom - foot)
                    corner(point(tab.right + foot, tab.bottom - foot), point(-foot, 0f), point(0f, foot))
                    path.lineTo(tab.left - foot, tab.bottom)
                    corner(point(tab.left - foot, tab.bottom - foot), point(0f, foot), point(foot, 0f))
                    path.lineTo(tab.left, tab.top + radius)
                    corner(point(tab.left + radius, tab.top + radius), point(-radius, 0f), point(0f, -radius))
                    path.close()
                    val maskImage = coverage(path, image.width, image.height)
                    mask = maskImage
                    val panel = image.getPixel(tab.center.x.toInt(), (tab.top + 5f * density).toInt())
                    val strip = image.getPixel(inactive.center.x.toInt(), (inactive.top + 5f * density).toInt())
                    var recorded = false
                    for (y in tab.top.toInt() until tab.bottom.toInt()) for (x in maxOf(header.left.toInt() + 1, (tab.left - foot - 1).toInt())..minOf((header.right - 21 * density).toInt(), (tab.right + foot + 1).toInt())) {
                        if (x < header.left + radius && y < header.top + radius) continue
                        val coverage = android.graphics.Color.alpha(maskImage.getPixel(x, y))
                        if (coverage !in 12..243) continue
                        covered++
                        val actual = image.getPixel(x, y)
                        if (listOf(16, 8, 0).any { shift ->
                            val expected = (((panel shr shift and 255) * coverage + (strip shr shift and 255) * (255 - coverage)) / 255f).toInt()
                            kotlin.math.abs((actual shr shift and 255) - expected) > 5
                        } && !recorded) {
                            recorded = true
                            mismatches += "$theme transparency=$transparency drawer=$drawer active=$active ($x,$y) coverage=$coverage actual=#%06X panel=#%06X strip=#%06X".format(actual and 0xFFFFFF, panel and 0xFFFFFF, strip and 0xFFFFFF)
                        }
                    }
                } finally { mask?.recycle(); image.recycle() }
            }
        }
        assertTrue("The contour includes antialiased pixels", covered > 100)
        assertTrue("Active-tab backgrounds leave a seam:\n${mismatches.joinToString("\n")}", mismatches.isEmpty())
    }

    @Test fun dockedPanelHandlesCollapseTheirColumnOnFirstDoubleTap() {
        for ((theme, style) in listOf("light" to "automatic", "dark" to "icon")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
            val id = group("sizes").getInt("id")
            customize(obj("type" to "set_tab_style", "group" to id, "style" to style))
            val panels = state().getJSONObject("workspace").getJSONObject("layout").getJSONArray("panels").toString()
            compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
            compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("collapsed").objects().any { column ->
                column.array("groups").objects().any { it.getInt("group") == id }
            } }
            assertTrue(groups().none { "sizes" in it.array("panels").values() })
            assertEquals("Tab bar settings are unchanged", panels,
                state().getJSONObject("workspace").getJSONObject("layout").getJSONArray("panels").toString())
            capture("workspace-docked-handle-collapse-$style-$theme")
        }
    }

    @Test fun dockedToolbarHandlesRestoreSingleLanesOrNecessaryWrap() {
        val edge = "top"
        val style = "labeled"
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

    @Test fun floatingToolbarPresetsRefitTileSizesAndResetOnFirstDoubleClick() {
        floatPanel("toolbar")
        fun preset() = state().getJSONObject("workspace").getJSONObject("layout").array("floating").objects().first().getString("toolbar_layout")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (layout in listOf("compact", "vertical", "horizontal")) {
                assertEquals(layout, preset())
                for (style in listOf("small", "medium", "large", "medium_labeled", "labeled")) {
                    customize(obj("type" to "set_tile_style", "panel" to "toolbar", "style" to style))
                    assertEquals("Changing tile size keeps $layout", layout, preset())
                    val resolved = group("toolbar")
                    val tile = resolved.getJSONObject("tiles").array("tiles").getJSONObject(0)
                    val width = when (style) { "small" -> 36f; "medium" -> 54f; "large" -> 72f; else -> 108f }
                    val height = when (style) { "small" -> 36f; "medium", "medium_labeled" -> 54f; else -> 72f }
                    val iconSize = when (style) { "medium" -> 24; "large" -> 32; else -> 16 }
                    assertEquals(width, tile.number("width"), .01f)
                    assertEquals(height, tile.number("height"), .01f)
                    val first = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").getJSONObject(0).getInt("id")
                    compose.onNodeWithTag("tile-toolbar-$first").assertWidthIsEqualTo(width.dp).assertHeightIsEqualTo(height.dp)
                    compose.onNodeWithTag("tile-icon-toolbar-$first", useUnmergedTree = true).assertWidthIsEqualTo(iconSize.dp).assertHeightIsEqualTo(iconSize.dp)
                    if (style.endsWith("labeled")) {
                        val labels = compose.onAllNodes(SemanticsMatcher("toolbar label") { it.config.contains(SemanticsProperties.TestTag) && it.config[SemanticsProperties.TestTag].startsWith("tile-label-toolbar-") }, useUnmergedTree = true)
                        assertTrue(labels.fetchSemanticsNodes().isNotEmpty())
                        for (index in labels.fetchSemanticsNodes().indices) {
                            val results = mutableListOf<TextLayoutResult>()
                            labels[index].performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(results) }
                            val result = results.single()
                            assertEquals(if (style == "labeled") 3 else 2, result.layoutInput.maxLines)
                            assertEquals(if (style == "labeled") 700 else 400, result.layoutInput.style.fontWeight!!.weight)
                            assertTrue("Label must fit within its tile", result.size.height <= height * compose.activity.resources.displayMetrics.density)
                            assertTrue(result.lineCount <= result.layoutInput.maxLines)
                        }
                    }
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
        assertTrue(group("sizes").getBoolean("tabs_visible"))
        capture("workspace-live-tearoff")
        val natural = JSONObject(group("sizes").getJSONObject("bounds").toString())
        val density = compose.activity.resources.displayMetrics.density
        for (edge in listOf("left", "right", "top", "bottom", "top_left", "top_right", "bottom_left", "bottom_right")) {
            val before = JSONObject(group("sizes").getJSONObject("bounds").toString())
            val dx = if (edge.contains("left")) -20f else if (edge.contains("right")) 20f else 0f
            val dy = if (edge.contains("top")) -20f else if (edge.contains("bottom")) 20f else 0f
            compose.onNodeWithTag("resize-$id-$edge").performTouchInput { swipe(center, center + androidx.compose.ui.geometry.Offset(dx, dy) * density, 350) }
            compose.waitUntil(10_000) { group("sizes").getJSONObject("bounds").toString() != before.toString() }
            compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
            compose.waitUntil(10_000) { kotlin.math.abs(group("sizes").getJSONObject("bounds").number("width") - natural.number("width")) < .5f &&
                kotlin.math.abs(group("sizes").getJSONObject("bounds").number("height") - natural.number("height")) < .5f }
            assertTrue("First double-click preserves the tab after resetting $edge", group("sizes").getBoolean("tabs_visible"))
        }
        compose.onNodeWithTag("group-grip-$id").performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { !group("sizes").getBoolean("tabs_visible") }
        capture("workspace-floating-hidden-tab")
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

    @Test fun zenFloatingDragOnlyMergesFloatsAtEveryEdge() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
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
            capture("workspace-zen-hidden-edge-$theme")
            workspace.performTouchInput { up() }
            compose.waitForIdle()
            assertTrue(group("sizes").getBoolean("floating"))
            val target = group("layers").getJSONObject("bounds")
            val merge = androidx.compose.ui.geometry.Offset(target.number("x") + 2, target.number("y") + target.number("height") / 2) * density
            val source = grip("sizes")
            workspace.performTouchInput { down(source); moveTo(merge, 16) }
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-drop-hint").fetchSemanticsNodes().isNotEmpty() }
            assertTrue("Floating merge does not reveal docks", host.snapshot!!.getBoolean("chrome_hidden"))
            capture("workspace-zen-floating-merge-$theme")
            workspace.performTouchInput { up() }
            compose.waitUntil(10_000) { group("sizes").getInt("id") == group("layers").getInt("id") }
            assertEquals(2, group("layers").array("panels").length())
            assertTrue(group("layers").getBoolean("tabs_visible"))
            val start = grip("layers")
            val left = androidx.compose.ui.geometry.Offset(10 * density, root.height / 2)
            workspace.performTouchInput { down(start); moveTo(left, 16) }
            compose.waitForIdle()
            assertTrue(host.snapshot!!.getBoolean("chrome_hidden"))
            workspace.performTouchInput { moveTo(root.center - root.topLeft, 16) }
            compose.waitForIdle()
            assertTrue("Edge contact keeps docks hidden", host.snapshot!!.getBoolean("chrome_hidden"))
            capture("workspace-zen-edge-drag-$theme")
            workspace.performTouchInput { up() }
            compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
            assertTrue("Dropping in the center stays floating", group("layers").getBoolean("floating"))
            assertNull(host.actionError)
            action(obj("type" to "invoke", "command" to "zen_mode"))
        }
    }

    @Test fun toolbarConfigurationAndGroupCollapseUseTheSharedDefault() {
        val originalIcon = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("icon")
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
        compose.onNodeWithText("Medium Tiles").performScrollTo().performClick()
        compose.waitUntil(10_000) { host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("tile_style") == "medium" }
        capture("workspace-toolbar-configure-medium")
        compose.onNodeWithText("Medium Labeled Tiles").performScrollTo().performClick()
        compose.waitUntil(10_000) { host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("tile_style") == "medium_labeled" }
        capture("workspace-toolbar-configure-medium-labeled")
        compose.onNodeWithText("Large Labeled Tiles").performScrollTo().performClick()
        compose.waitUntil(10_000) { host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("tile_style") == "labeled" }
        capture("workspace-toolbar-configure-labeled")
        customize(obj("type" to "close_expanded"))
        customize(obj("type" to "set_panel_visible", "panel" to "layers", "visible" to false))
        val toolbar = group("toolbar")
        assertFalse(toolbar.getBoolean("tabs_visible"))
        assertEquals("compact", state().getJSONObject("workspace").getJSONObject("layout").array("floating").objects().first().getString("toolbar_layout"))
        val tiles = toolbar.getJSONObject("tiles").array("tiles").objects()
        assertEquals(2, tiles.count { it.number("y") == tiles[0].number("y") })
        // Keep the grip outside Material's screen-edge menu margin so this
        // configuration test can check adjacency without menu edge clamping.
        floatPanel("toolbar", y = 120f)
        capture("workspace-toolbar-collapse")
        contextGrip("ribbon-grip-toolbar")
        compose.onNodeWithText("Icons only").assertDoesNotExist()
        compose.onNodeWithText("Configure Tools toolbar…").performClick()
        customize(obj("type" to "close_expanded"))
        val tabIcon = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.getString("icon")
        assertEquals(originalIcon, tabIcon)
        // Workspace's New Toolbar command opens the same picker as the context menu.
        quickAccessToolbarsMenu(); compose.onNodeWithText("New Toolbar…").performClick()
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
        customize(obj("type" to "set_panel_visible", "panel" to "commands", "visible" to false))
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

    @Test fun physicalTiltControlsDryBrushCursorAndPigment() {
        fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun recording(action: String) = native { Native.query(it, obj("type" to "stroke_recording", "action" to action).toString()) }
        val directory = File(compose.activity.getExternalFilesDir(null), "validation/physical-tilt").apply { mkdirs() }
        val report = JSONArray()
        val inputs = JSONArray()
        var inclination = 0.0
        var azimuth = 0.0
        fun send(phase: Int) = instrumentation.runOnMainSync {
            val canvas = findCanvas(compose.activity.window.decorView)!!
            val properties = MotionEvent.PointerProperties().apply { id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val coords = MotionEvent.PointerCoords().apply {
                x = canvas.width * .5f; y = canvas.height * .5f; pressure = .9f
                orientation = kotlin.math.atan2(-kotlin.math.cos(azimuth), kotlin.math.sin(azimuth)).toFloat()
                setAxisValue(MotionEvent.AXIS_TILT, inclination.toFloat())
            }
            val now = SystemClock.uptimeMillis()
            val event = MotionEvent.obtain(now - 20, now, phase, 1, arrayOf(properties), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_TOUCHSCREEN or InputDevice.SOURCE_STYLUS, 0)
            inputs.put(obj("time_ms" to now, "phase" to phase, "tilt" to inclination, "orientation" to coords.orientation,
                "expected_projected" to JSONArray(listOf(kotlin.math.atan2(kotlin.math.sin(inclination) * kotlin.math.cos(azimuth), kotlin.math.cos(inclination)),
                    kotlin.math.atan2(kotlin.math.sin(inclination) * kotlin.math.sin(azimuth), kotlin.math.cos(inclination))))))
            try { assertTrue(if (phase == MotionEvent.ACTION_HOVER_MOVE) canvas.dispatchGenericMotionEvent(event) else canvas.dispatchTouchEvent(event)) }
            finally { event.recycle() }
        }
        fun cursor(mode: String) = action(obj("type" to "restore_settings", "settings" to
            JSONObject(state().getJSONObject("settings").toString()).put("cursor", mode).put("feedback", false).put("platform_prediction", false)))
        fun screen(name: String): IntArray {
            settle()
            val image = instrumentation.uiAutomation.takeScreenshot()
            val origin = IntArray(2)
            var left = 0; var top = 0
            instrumentation.runOnMainSync {
                val canvas = findCanvas(compose.activity.window.decorView)!!
                canvas.getLocationOnScreen(origin)
                left = origin[0] + canvas.width / 2 - 128; top = origin[1] + canvas.height / 2 - 128
            }
            val crop = Bitmap.createBitmap(image, left, top, 256, 256)
            File(directory, "$name.png").outputStream().use { crop.compress(Bitmap.CompressFormat.PNG, 100, it) }
            return IntArray(256 * 256).also { crop.getPixels(it, 0, 256, 0, 0, 256, 256); crop.recycle(); image.recycle() }
        }
        fun moments(width: Int, height: Int, weight: (Int) -> Double): JSONObject {
            var mass = 0.0; var x = 0.0; var y = 0.0; var xx = 0.0; var xy = 0.0; var yy = 0.0
            for (row in 0 until height) for (column in 0 until width) {
                val w = weight(row * width + column)
                mass += w; x += column * w; y += row * w
                xx += column * column * w; xy += column * row * w; yy += row * row * w
            }
            assertTrue("The measured image contains the cursor or pigment", mass > 100)
            val a = xx / mass - x * x / (mass * mass); val b = xy / mass - x * y / (mass * mass); val d = yy / mass - y * y / (mass * mass)
            val spread = kotlin.math.hypot(a - d, 2 * b)
            return obj("aspect" to kotlin.math.sqrt((a + d + spread) / (a + d - spread)), "angle" to (.5 * kotlin.math.atan2(2 * b, a - d)))
        }
        fun axisError(actual: Double) = kotlin.math.abs(kotlin.math.atan2(kotlin.math.sin(2 * (actual - azimuth)), kotlin.math.cos(2 * (actual - azimuth)))) / 2
        host.createDrawing(1024, 1024)
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        invoke("fit_canvas")
        val brushes = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 0, 1))))
        recording("start")
        try {
            for (theme in listOf("light", "dark")) for (brushName in listOf("Pencil", "Charcoal")) {
                action(obj("type" to "set_theme", "theme" to theme))
                action(obj("type" to "select_brush", "id" to brushes.first { it.getString("label") == brushName }.getInt("id")))
                action(obj("type" to "set_brush_size", "value" to 64))
                compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
                var writingCursor = 0.0; var writingInk = 0.0
                for ((degrees, axis) in listOf(0 to -45, 40 to -45, 60 to -45, 40 to 45, 60 to 45)) {
                    invoke("clear_layer"); inclination = Math.toRadians(degrees.toDouble()); azimuth = Math.toRadians(axis.toDouble())
                    val name = "$theme-$brushName-$degrees-$axis"
                    cursor("none"); send(MotionEvent.ACTION_HOVER_MOVE)
                    val emptyScreen = screen("$name-empty")
                    cursor("brush_size"); send(MotionEvent.ACTION_HOVER_MOVE)
                    val shown = screen("$name-cursor")
                    val cursorShape = moments(256, 256) { index -> (0..2).sumOf { channel ->
                        kotlin.math.abs(((emptyScreen[index] shr (channel * 8)) and 255) - ((shown[index] shr (channel * 8)) and 255)).toDouble()
                    }.let { if (it > 24) it else 0.0 } }
                    cursor("none"); settle()
                    val empty = gradientOutput()
                    send(MotionEvent.ACTION_DOWN); send(MotionEvent.ACTION_MOVE); send(MotionEvent.ACTION_UP); settle()
                    val painted = gradientOutput()
                    File(directory, "$name.rgba").writeBytes(painted)
                    val extent = java.nio.ByteBuffer.wrap(painted).order(java.nio.ByteOrder.LITTLE_ENDIAN)
                    val ink = moments(extent.int, extent.int) { index -> (0..3).sumOf { channel ->
                        kotlin.math.abs((painted[8 + index * 4 + channel].toInt() and 255) - (empty[8 + index * 4 + channel].toInt() and 255)).toDouble()
                    } }
                    val row = obj("theme" to theme, "brush" to brushName, "inclination_degrees" to degrees, "azimuth_degrees" to axis, "cursor" to cursorShape, "ink" to ink)
                    report.put(row); File(directory, "geometry.json").writeText(report.toString(2))
                    if (degrees == 0) {
                        assertTrue("Upright cursor is round: $row", cursorShape.getDouble("aspect") < 1.2)
                    } else {
                        assertTrue("Cursor follows physical barrel direction: $row", axisError(cursorShape.getDouble("angle")) < .16)
                        assertTrue("Pigment follows physical barrel direction: $row", axisError(ink.getDouble("angle")) < .16)
                        if (degrees == 40) {
                            writingCursor = cursorShape.getDouble("aspect"); writingInk = ink.getDouble("aspect")
                            assertTrue("Writing pigment has the narrow contact: $row", writingInk in 1.25..1.85)
                        } else {
                            assertTrue("Side cursor is wider than writing: $row", cursorShape.getDouble("aspect") > writingCursor * 1.25)
                            assertTrue("Side pigment is wider than writing: $row", ink.getDouble("aspect") > writingInk * 1.25)
                            assertTrue("Physical 60-degree tilt reaches the side pose: $row", ink.getDouble("aspect") > if (brushName == "Pencil") 3.4 else 2.125)
                        }
                    }
                }
            }
        } finally {
            File(directory, "api-inputs.json").writeText(inputs.toString(2))
            val raw = native { handle ->
                Native.query(handle, obj("type" to "stroke_recording", "action" to "stop").toString())
                Native.strokeRecordingData(handle).also { Native.query(handle, obj("type" to "stroke_recording", "action" to "saved").toString()) }
            }
            File(directory, "tilt.capystrokes").outputStream().use { output ->
                output.write("CAPYPEN4".toByteArray(Charsets.US_ASCII)); java.util.zip.GZIPOutputStream(output).use { it.write(raw) }
            }
        }
        assertNull(host.failure); assertNull(host.actionError)
    }

    @Test fun cursorHidesWhileDrawingAndReturnsOnRelease() {
        val saved = JSONObject(state().getJSONObject("settings").toString())
        val brush = JSONObject(state().getJSONObject("brush").toString())
        fun preference(id: String, value: Any) = action(obj("type" to "preferences",
            "action" to obj("type" to "edit", "id" to id, "value" to value)))
        val point = androidx.compose.ui.geometry.Offset(.5f, .5f)
        fun send(phase: Int, tool: Int) = canvasEvent(phase, listOf(point), tool)
        fun pixels(name: String): IntArray {
            val image = capture("cursor-$name")
            val origin = IntArray(2)
            var x = 0; var y = 0
            instrumentation.runOnMainSync {
                val canvas = findCanvas(compose.activity.window.decorView)!!
                canvas.getLocationOnScreen(origin)
                x = origin[0] + canvas.width / 2 - 50
                y = origin[1] + canvas.height / 2 - 50
            }
            return IntArray(100 * 100).also { image.getPixels(it, 0, 100, x, y, 100, 100) }
        }
        fun difference(a: IntArray, b: IntArray) = a.indices.count { i ->
            listOf(0, 8, 16).any { shift -> kotlin.math.abs(((a[i] shr shift) and 255) - ((b[i] shr shift) and 255)) > 8 }
        }
        try {
            action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_brush_size", "value" to 48))
            preference("feedback", false)
            action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "cursor")))
            action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "hide_cursor_while_drawing")))
            action(obj("type" to "open_settings", "page" to "input"))
            val toggle = compose.onNodeWithTag("preference-hide_cursor_while_drawing")
            toggle.performScrollTo().assertIsOn().performClick()
            waitState { !it.getJSONObject("settings").getBoolean("hide_cursor_while_drawing") }
            toggle.assertIsOff()
            assertFalse(state().getJSONObject("settings").getBoolean("hide_cursor_while_drawing"))
            toggle.performClick()
            waitState { it.getJSONObject("settings").getBoolean("hide_cursor_while_drawing") }
            toggle.assertIsOn()
            compose.onNodeWithTag("setting-choice-cursor").assertExists()
            capture("cursor-input-settings")
            action(obj("type" to "close_settings"))
            for (theme in listOf("light", "dark")) for (tool in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_MOUSE)) {
                action(obj("type" to "set_theme", "theme" to theme))
                for (mode in listOf("tool", "tool_brush_size")) {
                    action(obj("type" to "restore_settings", "settings" to JSONObject(saved.toString()).put("theme", theme).put("cursor", mode).put("feedback", false)))
                    val icons = mutableListOf<IntArray>()
                    for (command in listOf("pen", "pencil", "brush", "eraser", "lasso", "rectangle_select")) {
                        action(obj("type" to "invoke", "command" to command))
                        send(MotionEvent.ACTION_HOVER_MOVE, tool)
                        val icon = pixels("$tool-$mode-$command")
                        for (previous in icons) assertTrue("$command has a distinct tool cursor", difference(previous, icon) > 4)
                        icons.add(icon)
                    }
                }
                action(obj("type" to "select_brush", "id" to 1))
                action(obj("type" to "set_brush_size", "value" to 48))
                action(obj("type" to "restore_settings", "settings" to JSONObject(saved.toString()).put("theme", theme).put("cursor", "brush_size").put("feedback", false)))
                send(MotionEvent.ACTION_HOVER_MOVE, tool)
                val hover = pixels("$tool-hover")
                preference("cursor", 0)
                val empty = pixels("$tool-none")
                assertTrue("Hover cursor reaches the GPU", difference(hover, empty) > 4)
                action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "cursor")))
                send(MotionEvent.ACTION_DOWN, tool)
                send(MotionEvent.ACTION_MOVE, tool)
                val hidden = pixels("$tool-drawing-hidden")
                preference("hide_cursor_while_drawing", false)
                val visible = pixels("$tool-drawing-visible")
                assertTrue("Opting out shows the live cursor", difference(hidden, visible) > 4)
                preference("hide_cursor_while_drawing", true)
                assertEquals("Hiding clears the retained GPU cursor", 0, difference(hidden, pixels("$tool-hidden-again")))
                preference("cursor", 0)
                assertEquals("Hidden drawing matches No cursor", 0, difference(hidden, pixels("$tool-drawing-none")))
                action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "cursor")))
                send(MotionEvent.ACTION_UP, tool)
                var released = pixels("$tool-released")
                val presented = SystemClock.uptimeMillis() + 3_000
                while (difference(released, hidden) <= 4 && SystemClock.uptimeMillis() < presented) released = pixels("$tool-released")
                preference("cursor", 0)
                val painted = pixels("$tool-ink")
                assertTrue("Release restores hover", difference(released, painted) > 4)
                assertTrue("Contact still deposits ink", difference(empty, painted) > 20)
                action(obj("type" to "invoke", "command" to "undo"))
                assertEquals("One undo removes the contact", 0, difference(empty, pixels("$tool-undo")))
                action(obj("type" to "preferences", "action" to obj("type" to "reset", "id" to "cursor")))
            }
        } finally {
            send(MotionEvent.ACTION_CANCEL, MotionEvent.TOOL_TYPE_STYLUS)
            action(obj("type" to "close_settings"))
            action(obj("type" to "restore_settings", "settings" to saved))
            action(obj("type" to "select_brush", "id" to brush.getInt("preset")))
            action(obj("type" to "set_brush_size", "value" to brush.getDouble("diameter")))
        }
        assertNull(host.failure)
        assertNull(host.actionError)
    }

    @Test fun canvasFocusOutsideTouchModeKeepsPresentedColors() {
        val window = compose.activity.window.decorView
        val touchMode = window.isInTouchMode
        try {
            instrumentation.setInTouchMode(false)
            compose.waitUntil(5_000) { !window.isInTouchMode }
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, .2, 1, 1))))
            action(obj("type" to "set_brush_opacity", "value" to 1))
            action(obj("type" to "invoke", "command" to "figure"))
            action(state().getJSONObject("tool_set").array("groups").objects().first { it.getString("label") == "Rectangle" }.getJSONObject("action"))
            action(state().getJSONObject("tool_set").array("subtools").objects().first { it.getString("label") == "Fill" }.getJSONObject("action"))
            val from = androidx.compose.ui.geometry.Offset(.4f, .4f)
            val to = androidx.compose.ui.geometry.Offset(.6f, .6f)
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(from), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(to), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(to), MotionEvent.TOOL_TYPE_STYLUS)
            val center = IntArray(2)
            instrumentation.runOnMainSync {
                val canvas = findCanvas(window)!!
                assertTrue("Canvas takes focus outside touch mode", canvas.isFocused)
                canvas.getLocationOnScreen(center)
                center[0] += canvas.width / 2; center[1] += canvas.height / 2
            }
            fun presented(): Int {
                val screen = instrumentation.uiAutomation.takeScreenshot()!!
                return try { screen.getPixel(center[0], center[1]) } finally { screen.recycle() }
            }
            compose.waitUntil(20_000) { android.graphics.Color.blue(presented()) > 220 }
            val ink = capture("canvas-focus-ink").getPixel(center[0], center[1])
            assertEquals("Presented red", 0f, android.graphics.Color.red(ink).toFloat(), 4f)
            assertEquals("Presented green", 51f, android.graphics.Color.green(ink).toFloat(), 4f)
            assertEquals("Presented blue", 255f, android.graphics.Color.blue(ink).toFloat(), 4f)
        } finally {
            instrumentation.setInTouchMode(touchMode)
        }
        assertNull(host.failure)
        assertNull(host.actionError)
    }

    @Test fun zenModesIconsAndContextMenuUseSharedSettings() {
        val saved = JSONObject(state().getJSONObject("settings").toString())
        fun edit(id: String, value: Any) = action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to id, "value" to value)))
        val capy = capyTag()
        try {
            edit("zen_show_capy", true)
            floatPanel("sizes")
            val floating = group("sizes").getInt("id")
            for (theme in listOf("dark", "light")) {
                action(obj("type" to "set_theme", "theme" to theme))
                compose.onNodeWithTag(capy).performMouseInput { click(button = MouseButton.Secondary) }
                compose.waitUntil(10_000) { compose.onAllNodes(isPopup()).fetchSemanticsNodes().isNotEmpty() }
                capture("zen-menu-$theme")
                compose.onNodeWithText("Change icon…").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences")?.optString("reveal") == "zen_icon" }
                compose.onNodeWithTag("image-choice-0").assertIsDisplayed()
                compose.onNodeWithTag("zen-button").assertIsNotDisplayed()
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
                compose.onNodeWithTag(capy).performTouchInput { click() }
                compose.waitUntil(10_000) { host.snapshot!!.optBoolean("chrome_hidden") }
                compose.onNodeWithTag("zen-button").assertIsDisplayed()
                compose.onNodeWithTag("group-$floating").assertIsDisplayed()
                compose.onNodeWithContentDescription("Settings").assertDoesNotExist()
                capture("zen-button-only-$theme")
                compose.onNodeWithTag("zen-button").performTouchInput { longClick() }
                compose.waitUntil(10_000) { compose.onAllNodes(isPopup()).fetchSemanticsNodes().isNotEmpty() }
                assertTrue(state().getJSONObject("workspace").getBoolean("zen_mode"))
                compose.onNodeWithText("Change icon…").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
                compose.onNodeWithTag("image-choice-3").assertIsDisplayed()
                compose.onNodeWithTag("settings-done").performClick()
                compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
                edit("zen_show_capy", false)
                compose.waitUntil(10_000) { host.snapshot!!.optBoolean("chrome_hidden") }
                compose.onNodeWithTag("zen-button").assertIsNotDisplayed()
                compose.onNodeWithTag("group-$floating").assertIsDisplayed()
                edit("zen_show_capy", true)
                compose.waitUntil(10_000) { compose.onNodeWithTag("zen-button").isDisplayed() }
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
        openSettings()
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
        openSettings()
        capture("08-preferences-dark")
    }
    @Test fun settingsAndDetailsSlideWithinOneSurface() {
        compose.mainClock.autoAdvance = false
        try {
            openSettings()
            compose.mainClock.advanceTimeBy(80)
            val entering = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            compose.mainClock.advanceTimeBy(320)
            val settled = compose.onNodeWithTag("preferences-surface").fetchSemanticsNode().positionInRoot.y
            assertTrue("Settings slide down from above ($entering -> $settled)", entering < settled)

            compose.onNodeWithText("Keyboard Shortcuts").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "shortcuts" }
            compose.mainClock.advanceTimeBy(300)
            compose.onNodeWithTag("shortcut-category-Edit").performScrollTo().performClick()
            compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_page").optString("category") == "Edit" }
            compose.mainClock.advanceTimeBy(80)
            val enteringCategory = compose.onNodeWithTag("settings-content-category:Edit").fetchSemanticsNode().positionInRoot.x
            compose.mainClock.advanceTimeBy(300)
            val settledCategory = compose.onNodeWithTag("settings-content-category:Edit").fetchSemanticsNode().positionInRoot.x
            assertTrue("A category slides in from the right ($enteringCategory -> $settledCategory)", enteringCategory > settledCategory)
            val shortcut = preferences().array("shortcuts").objects().first { it.getBoolean("visible") }
            compose.onNode(hasText(shortcut.getString("label")) and hasClickAction()
                and hasAnyAncestor(hasTestTag("settings-content-category:Edit"))).performScrollTo().performClick()
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
            compose.onNodeWithTag("settings-content-category:Edit").assertIsDisplayed()
            compose.onNodeWithContentDescription("Back").performClick()
            compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_page").isNull("category") }
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

    @Test fun retainedSettingsReleasePopupsFocusAndInvalidDrafts() {
        openSettings()
        compose.onNodeWithTag("setting-choice-theme").performScrollTo().performClick()
        compose.onAllNodes(isPopup()).assertCountEquals(1)
        action(obj("type" to "close_settings"))
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
        openSettings()
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onNodeWithTag("settings-category-canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        val value = compose.onNodeWithTag("number-value-pan_speed", useUnmergedTree = true)
        value.performScrollTo()
        val before = state().getJSONObject("settings").number("pan_speed")
        value.performClick()
        compose.onNodeWithTag("setting-number-pan_speed").performTextReplacement("invalid")
        compose.onNodeWithTag("setting-number-pan_speed").performImeAction()
        action(obj("type" to "close_settings"))
        compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
        assertFalse("Hidden settings release text input ownership", host.editingText)
        openSettings()
        compose.onNodeWithTag("settings-category-canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        compose.onNodeWithTag("number-value-pan_speed", useUnmergedTree = true).performScrollTo().assertIsDisplayed()
        assertEquals(before, state().getJSONObject("settings").number("pan_speed"))
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun penTiltCalibrationUsesNativeSensorsAndPersistsLight() = penTiltCalibrationJourney("light")

    @Test fun penTiltCalibrationUsesNativeSensorsAndPersistsDark() = penTiltCalibrationJourney("dark")

    private fun penTiltCalibrationJourney(theme: String) {
        device.landscape(compose.activityRule.scenario)
        fun calibration() = state().getJSONObject("tilt_calibration")
        fun points() = calibration().getJSONObject("editor").getJSONArray("points").toString()
        fun saved() = state().getJSONObject("settings").getJSONObject("tilt_calibrations").toString()
        fun axes() = calibration().getJSONObject("axes")
        fun click(name: String) {
            val node = compose.onNodeWithTag("pen-tilt-$name")
            if (name in listOf("firmer", "lighter", "reverse-horizontal", "reverse-vertical", "swap-axes")) node.performScrollTo()
            node.performClick()
            host.drain()
            compose.waitForIdle()
        }
        fun exposeCanvas() {
            val density = compose.activity.resources.displayMetrics.density
            compose.onNodeWithTag("pen-tilt-title").performTouchInput {
                swipe(center, center - androidx.compose.ui.geometry.Offset(300f * density, 0f), 500)
            }
            host.drain()
            compose.waitForIdle()
        }
        fun open() {
            openSettings()
            compose.onNodeWithTag("settings-category-input").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "input" }
            compose.onNodeWithTag("setting-action-pen_tilt").performScrollTo().performClick()
            waitState { it.objectOrNull("tilt_calibration") != null }
            compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
            compose.onNodeWithTag("pen-tilt-dialog").assertIsDisplayed()
            host.drain()
            compose.waitForIdle()
            compose.onNodeWithTag("pen-tilt-close").assertWidthIsEqualTo(24.dp).assertHeightIsEqualTo(24.dp)
            assertEquals(bounds("pen-tilt-title").top, bounds("pen-tilt-close").top, 1f)
            compose.waitUntil(10_000) { bounds("effect-curve").let { kotlin.math.abs(it.width - it.height) < 1f } }
            val plotCenter = bounds("effect-curve").center.x
            assertEquals("Symmetric font-sized gutters center the graph", bounds("pen-tilt-dialog").center.x, plotCenter, 1f)
            compose.onNodeWithTag("pen-tilt-direction").performScrollTo().assertWidthIsEqualTo(72.dp).assertHeightIsEqualTo(72.dp)
            val plot = bounds("effect-curve")
            val direction = bounds("pen-tilt-direction")
            assertTrue("Direction preview stays inside the graph", plot.contains(direction.topLeft) && plot.contains(direction.bottomRight - androidx.compose.ui.geometry.Offset(1f, 1f)))
            assertTrue("Direction preview occupies the lower right of the graph", direction.center.x > plot.center.x && direction.center.y > plot.center.y)
            for (tag in listOf("firmer", "lighter", "reverse-horizontal", "reverse-vertical", "swap-axes"))
                compose.onNodeWithTag("pen-tilt-$tag").performScrollTo().assertHeightIsEqualTo(36.dp)
            val sensitivity = bounds("pen-tilt-firmer")
            for (tag in listOf("lighter", "reverse-horizontal", "reverse-vertical", "swap-axes"))
                assertEquals("Sensitivity and direction controls share one row", sensitivity.center.y, bounds("pen-tilt-$tag").center.y, 1f)
            val density = compose.activity.resources.displayMetrics.density
            val sensitivityTitle = bounds("pen-tilt-sensitivity-title")
            val directionTitle = bounds("pen-tilt-direction-title")
            assertEquals("Sensitivity label meets its buttons", 4f * density, sensitivity.left - sensitivityTitle.right, 1f)
            assertEquals("Direction label meets its buttons", 4f * density, bounds("pen-tilt-reverse-horizontal").left - directionTitle.right, 1f)
            assertTrue("Calibration control pairs keep at least eight pixels apart", directionTitle.left - bounds("pen-tilt-lighter").right >= 8f * density - 1f)
            assertEquals("Sensitivity controls align with the left content gutter", bounds("pen-tilt-dialog").left + 16f * density,
                sensitivityTitle.left, 1f)
            assertEquals("Direction controls align with the right content gutter", bounds("pen-tilt-dialog").right - 16f * density,
                bounds("pen-tilt-swap-axes").right, 1f)
            for (tag in listOf("sensitivity-title", "direction-title")) {
                val label = compose.onNodeWithTag("pen-tilt-$tag", useUnmergedTree = true)
                val layouts = mutableListOf<TextLayoutResult>()
                label.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
                val layout = layouts.single()
                assertEquals("Calibration labels use their text width", layout.multiParagraph.maxIntrinsicWidth,
                    label.fetchSemanticsNode().boundsInRoot.width, 1f)
                assertEquals("Calibration labels align with their buttons", sensitivity.center.y,
                    label.fetchSemanticsNode().boundsInRoot.center.y, 1f)
            }
            for (tag in listOf("reset", "cancel", "apply")) compose.onNodeWithTag("pen-tilt-$tag").assertHeightIsEqualTo(36.dp)
        }
        val stylus = InputDevice.getDeviceIds().asSequence().mapNotNull { InputDevice.getDevice(it) }
            .first { device -> device.motionRanges.any { it.axis == MotionEvent.AXIS_TILT } }
        fun point(tag: String): androidx.compose.ui.geometry.Offset {
            if (tag == "pen-tilt-direction") compose.onNodeWithTag(tag).performScrollTo()
            val node = compose.onNodeWithTag(tag).fetchSemanticsNode()
            val origin = IntArray(2)
            instrumentation.runOnMainSync { (node.root as ViewRootForTest).view.getLocationInWindow(origin) }
            return node.boundsInRoot.center + androidx.compose.ui.geometry.Offset(origin[0].toFloat(), origin[1].toFloat())
        }
        val down = SystemClock.uptimeMillis()
        fun send(phase: Int, at: androidx.compose.ui.geometry.Offset, inclination: Double = 30.0, azimuth: Double = 30.0) {
            instrumentation.runOnMainSync {
                val coordinate = MotionEvent.PointerCoords().apply {
                    x = at.x; y = at.y
                    pressure = if (phase in listOf(MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT, MotionEvent.ACTION_UP)) 0f else .7f
                    orientation = kotlin.math.atan2(-kotlin.math.cos(Math.toRadians(azimuth)), kotlin.math.sin(Math.toRadians(azimuth))).toFloat()
                    setAxisValue(MotionEvent.AXIS_TILT, Math.toRadians(inclination).toFloat())
                }
                val property = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
                val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, 1, arrayOf(property), arrayOf(coordinate),
                    0, 0, 1f, 1f, stylus.id, 0, InputDevice.SOURCE_STYLUS, 0)
                try {
                    if (phase in listOf(MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_EXIT))
                        compose.activity.dispatchGenericMotionEvent(event)
                    else compose.activity.dispatchTouchEvent(event)
                } finally { event.recycle() }
            }
            host.drain()
        }
        fun penClick(name: String) {
            val tag = "pen-tilt-$name"
            compose.onNodeWithTag(tag).performScrollTo()
            val at = point(tag)
            send(MotionEvent.ACTION_DOWN, at)
            send(MotionEvent.ACTION_UP, at)
            compose.waitForIdle()
        }
        fun direction(key: String, x: Double, y: Double) {
            waitState {
                calibration().optJSONArray(key)?.let { kotlin.math.abs(it.getDouble(0) - x) < .002 && kotlin.math.abs(it.getDouble(1) - y) < .002 } == true
            }
            val vector = calibration().getJSONArray(key)
            assertEquals("$key horizontal", x, vector.getDouble(0), .002)
            assertEquals("$key vertical", y, vector.getDouble(1), .002)
        }
        action(obj("type" to "set_theme", "theme" to theme))
        action(obj("type" to "restore_settings", "settings" to JSONObject(state().getJSONObject("settings").toString()).put("cursor", "none")))
        action(obj("type" to "invoke", "command" to "lasso"))
        val original = saved()
        open()
        assertFalse("The fresh editor has not observed a pen", calibration().getBoolean("ready"))
        compose.onNodeWithTag("pen-tilt-apply").assertIsNotEnabled()
        capture("tilt-calibration-$theme-default")
        val neutral = points()
        exposeCanvas()
        val beforeBounds = calibration().getJSONObject("bounds").toString()
        lateinit var canvas: CanvasSurfaceView
        val origin = IntArray(2)
        instrumentation.runOnMainSync {
            canvas = findCanvas(compose.activity.window.decorView)!!
            canvas.getLocationInWindow(origin)
        }
        val area = host.snapshot!!.getJSONObject("layout").getJSONObject("work_area")
        val density = compose.activity.resources.displayMetrics.density
        val camera = state().getJSONObject("camera")
        assertEquals(0.0, camera.getDouble("rotation"), 0.0)
        assertEquals(listOf(false, false), camera.getJSONArray("flipped").values())
        val translation = camera.getJSONArray("translation")
        val zoom = camera.number("zoom")
        val drawing = state().array("tabs").objects().first { it.getBoolean("active") }
        val sheet = androidx.compose.ui.geometry.Rect(origin[0] + translation.getDouble(0).toFloat(),
            origin[1] + translation.getDouble(1).toFloat(),
            origin[0] + translation.getDouble(0).toFloat() + drawing.getInt("width") * zoom,
            origin[1] + translation.getDouble(1).toFloat() + drawing.getInt("height") * zoom)
        val visible = sheet.intersect(androidx.compose.ui.geometry.Rect(origin[0] + area.number("x") * density,
            origin[1] + area.number("y") * density, origin[0] + (area.number("x") + area.number("width")) * density,
            origin[1] + (area.number("y") + area.number("height")) * density))
        val stroke = androidx.compose.ui.geometry.Offset(24f * density, 0f)
        val panel = host.calibrationPenBounds!!
        val canvasPoint = listOf(.5f, .75f, .25f).flatMap { y -> listOf(.85f, .15f).map { x ->
            androidx.compose.ui.geometry.Offset(visible.left + visible.width * x, visible.top + visible.height * y)
        } }.first { !panel.contains(it) && !panel.contains(it + stroke) && visible.contains(it + stroke) }
        assertFalse("The first Lasso contact uses uncovered canvas", host.calibrationPenBounds!!.contains(canvasPoint))
        send(MotionEvent.ACTION_DOWN, canvasPoint)
        waitState { calibration().getBoolean("ready") }
        assertEquals("First Lasso pen contact preserves utility placement", beforeBounds, calibration().getJSONObject("bounds").toString())
        assertEquals(neutral, points())
        send(MotionEvent.ACTION_UP, canvasPoint)
        compose.onNodeWithTag("pen-tilt-apply").assertIsEnabled()
        val beforePixels = gradientOutput()
        val beforeRevision = state().getJSONObject("document_file").getLong("revision")
        val beforeHistory = state().array("commands").objects().filter { it.getString("id") in listOf("undo", "redo") }.map { it.getBoolean("enabled") }
        val x = kotlin.math.sqrt(3.0) / 6.0
        val y = 1.0 / 6.0
        send(MotionEvent.ACTION_HOVER_ENTER, point("pen-tilt-title"))
        compose.runOnIdle { assertTrue(host.calibrationMotionActive) }
        direction("input_direction", x, y)
        direction("adjusted_direction", x, y)
        send(MotionEvent.ACTION_HOVER_MOVE, point("pen-tilt-direction"))
        send(MotionEvent.ACTION_HOVER_EXIT, point("pen-tilt-direction"))
        compose.runOnIdle { assertFalse(host.calibrationMotionActive) }
        send(MotionEvent.ACTION_DOWN, point("pen-tilt-direction"), 60.0, 60.0)
        send(MotionEvent.ACTION_MOVE, point("pen-tilt-direction"), 60.0, 60.0)
        direction("input_direction", 1.0 / 3.0, kotlin.math.sqrt(3.0) / 3.0)
        assertFalse(calibration().getJSONObject("editor").isNull("marker"))
        send(MotionEvent.ACTION_UP, point("pen-tilt-direction"), 60.0, 60.0)
        send(MotionEvent.ACTION_HOVER_ENTER, point("pen-tilt-title"))
        penClick("reverse-horizontal")
        direction("adjusted_direction", -x, y)
        assertFalse(axes().getBoolean("reverse_vertical")); assertFalse(axes().getBoolean("swap"))
        penClick("reverse-vertical")
        direction("adjusted_direction", -x, -y)
        assertTrue(axes().getBoolean("reverse_horizontal")); assertFalse(axes().getBoolean("swap"))
        penClick("swap-axes")
        direction("adjusted_direction", -y, -x)
        assertTrue(axes().getBoolean("reverse_horizontal")); assertTrue(axes().getBoolean("reverse_vertical"))
        capture("tilt-calibration-$theme-preview")
        assertEquals("Preview does not edit the response", neutral, points())
        penClick("firmer")
        waitState { points() != neutral }
        penClick("lighter")
        waitState { points() == neutral }
        assertEquals(beforeRevision, state().getJSONObject("document_file").getLong("revision"))
        assertEquals(beforeHistory, state().array("commands").objects().filter { it.getString("id") in listOf("undo", "redo") }.map { it.getBoolean("enabled") })
        assertArrayEquals("Dialog sensor preview never paints", beforePixels, gradientOutput())
        action(obj("type" to "invoke", "command" to "pencil"))
        host.awaitReady(compose = compose)
        send(MotionEvent.ACTION_DOWN, canvasPoint)
        val end = canvasPoint + stroke
        send(MotionEvent.ACTION_MOVE, end)
        send(MotionEvent.ACTION_UP, end)
        waitState { it.getJSONObject("document_file").getLong("revision") != beforeRevision }
        settle()
        assertFalse("The exposed canvas paints while the utility remains open", beforePixels.contentEquals(gradientOutput()))
        assertTrue(calibration().getBoolean("ready"))
        action(obj("type" to "invoke", "command" to "undo"))
        settle()
        assertArrayEquals("One Undo removes only the real canvas stroke", beforePixels, gradientOutput())
        penClick("firmer")
        waitState { points() != neutral }
        click("cancel")
        waitState { it.objectOrNull("tilt_calibration") == null }
        assertEquals(original, saved())
        open()
        assertEquals(neutral, points())
        penClick("reverse-horizontal"); penClick("firmer")
        waitState { axes().getBoolean("reverse_horizontal") && points() != neutral }
        val applied = points()
        val appliedAxes = axes().toString()
        click("apply")
        waitState { it.objectOrNull("tilt_calibration") == null && saved() != original }
        val committed = saved()
        val preferences = compose.activity.getSharedPreferences(CanvasHost.preferencesName, 0)
        compose.waitUntil(10_000) { preferences.getString("settings", null)?.let { JSONObject(it).getJSONObject("tilt_calibrations").toString() == committed } == true }
        val restored = runBlocking { host.withNative {
            val other = Native.create(preferences.getString("settings", null)!!, arrayOf("en"), "", false)
            try { JSONObject(Native.snapshot(other)!!).getJSONObject("state").getJSONObject("settings").getJSONObject("tilt_calibrations").toString() }
            finally { Native.destroy(other) }
        } }
        assertEquals("Saved calibration survives a new native session", committed, restored)
        compose.activityRule.scenario.recreate()
        host.awaitReady(compose = compose)
        open()
        assertEquals(applied, points()); assertEquals(appliedAxes, axes().toString())
        capture("tilt-calibration-$theme")
        device.portrait(compose.activityRule.scenario)
        host.awaitReady(compose = compose)
        send(MotionEvent.ACTION_HOVER_ENTER, point("pen-tilt-title"))
        waitState { axes().getBoolean("reverse_vertical") && !axes().getBoolean("reverse_horizontal") }
        direction("input_direction", x, y)
        direction("adjusted_direction", x * .975, -y * .975)
        assertEquals("Display rotation preserves the saved device-frame correction", committed, saved())
        device.landscape(compose.activityRule.scenario)
        host.awaitReady(compose = compose)
        send(MotionEvent.ACTION_HOVER_ENTER, point("pen-tilt-title"))
        waitState { axes().toString() == appliedAxes }
        click("reset")
        waitState { points() == neutral && !axes().getBoolean("swap") && !axes().getBoolean("reverse_horizontal") && !axes().getBoolean("reverse_vertical") }
        click("close")
        waitState { it.objectOrNull("tilt_calibration") == null }
        assertEquals("Closing discards Reset", committed, saved())
        open()
        click("reset")
        compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
        waitState { it.objectOrNull("tilt_calibration") == null }
        assertEquals("Back discards Reset", committed, saved())
        open()
        click("reset"); click("apply")
        waitState { it.objectOrNull("tilt_calibration") == null }
        open()
        assertEquals(neutral, points())
        assertFalse(axes().getBoolean("swap")); assertFalse(axes().getBoolean("reverse_horizontal")); assertFalse(axes().getBoolean("reverse_vertical"))
        compose.onNodeWithTag("effect-curve").performScrollTo()
        val tiltPlot = bounds("effect-curve")
        click("cancel")
        action(obj("type" to "invoke", "command" to "pen_pressure"))
        waitState { it.objectOrNull("pressure_calibration") != null }
        compose.waitUntil(10_000) { bounds("effect-curve").let { kotlin.math.abs(it.width - it.height) < 1f } }
        assertEquals("Pressure and tilt share their plot width", tiltPlot.width, bounds("effect-curve").width, 1f)
        assertEquals("Pressure and tilt share their plot height", tiltPlot.height, bounds("effect-curve").height, 1f)
        compose.onNodeWithTag("pen-pressure-cancel").performClick()
        waitState { it.objectOrNull("pressure_calibration") == null }
        val firstDrawing = host.drawingTabs.selected
        host.newDocument(128, 128)
        host.awaitMain("New drawing selected", 30_000, { "${host.drawingTabs.view}" }, compose) {
            host.drawingTabs.selected != firstDrawing && !host.documentInputBlocked
        }
        action(obj("type" to "invoke", "command" to "lasso"))
        open()
        assertFalse("The new native drawing session has no registered pen yet", calibration().getBoolean("ready"))
        exposeCanvas()
        assertFalse(host.calibrationPenBounds!!.contains(canvasPoint))
        send(MotionEvent.ACTION_DOWN, canvasPoint)
        waitState { calibration().getBoolean("ready") }
        send(MotionEvent.ACTION_UP, canvasPoint)
        click("cancel")
        compose.runOnIdle { host.drawingTabs.select(firstDrawing) }
        host.awaitMain("Original drawing selected", 30_000, { "${host.drawingTabs.view}" }, compose) {
            host.drawingTabs.selected == firstDrawing && !host.documentInputBlocked && !host.drawingTabs.switching
        }
        open()
        send(MotionEvent.ACTION_HOVER_ENTER, point("pen-tilt-title"))
        waitState { calibration().getBoolean("ready") }
        assertEquals(stylus.name, calibration().getString("device"))
        click("cancel")
        compose.runOnIdle { assertFalse(host.calibrationMotionActive) }
        val previousDrawing = host.drawingTabs.selected
        host.newDocument(128, 128)
        host.awaitMain("Cold curve drawing selected", 30_000, { "${host.drawingTabs.view}" }, compose) {
            host.drawingTabs.selected != previousDrawing && !host.documentInputBlocked
        }
        open()
        assertFalse("The curve receives the first pen contact without hover", calibration().getBoolean("ready"))
        val coldSaved = saved()
        val coldPoints = points()
        compose.onNodeWithTag("effect-curve").performScrollTo()
        val coldGraph = bounds("effect-curve")
        val coldDown = point("effect-curve")
        send(MotionEvent.ACTION_DOWN, coldDown)
        waitState { calibration().getBoolean("ready") && points() != coldPoints }
        val addedPoints = points()
        val coldMove = coldDown + androidx.compose.ui.geometry.Offset(coldGraph.width * .12f, -coldGraph.height * .15f)
        send(MotionEvent.ACTION_MOVE, coldMove)
        waitState { points() != addedPoints }
        send(MotionEvent.ACTION_UP, coldMove)
        compose.runOnIdle { assertFalse(host.calibrationMotionActive) }
        assertEquals("Cold curve preview leaves saved corrections unchanged", coldSaved, saved())
        click("cancel")
        waitState { it.objectOrNull("tilt_calibration") == null }
        assertEquals("Cancel discards the cold first-contact edit", coldSaved, saved())
        assertNull(host.failure); assertNull(host.actionError)
    }

    @Test fun penPressureCalibrationUsesNativeCurveAndPersistsAppliedResponse() {
        fun calibration() = state().getJSONObject("pressure_calibration")
        fun points() = calibration().getJSONObject("editor").getJSONArray("points")
        fun saved() = state().getJSONObject("settings").getJSONArray("pressure_curve").toString()
        fun open() {
            openSettings()
            compose.onNodeWithTag("settings-category-input").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "input" }
            compose.onNodeWithTag("setting-action-pen_pressure").performScrollTo().performClick()
            compose.waitUntil(10_000) { state().objectOrNull("pressure_calibration") != null }
            compose.onNodeWithTag("preferences-surface").assertDoesNotExist()
            compose.onNodeWithTag("pen-pressure-dialog").assertIsDisplayed()
            compose.onNodeWithTag("effect-curve").assertIsDisplayed()
            compose.onAllNodesWithTag("setting-number-pressure").assertCountEquals(0)
            compose.onAllNodesWithTag("setting-slider-pressure").assertCountEquals(0)
        }
        val original = saved()
        action(obj("type" to "invoke", "command" to "lasso"))
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            open()
            val dialog = compose.onNodeWithTag("pen-pressure-dialog").fetchSemanticsNode()
            val windowOrigin = IntArray(2)
            instrumentation.runOnMainSync {
                (dialog.root as ViewRootForTest).view.getLocationInWindow(windowOrigin)
                val coordinate = MotionEvent.PointerCoords().apply {
                    x = windowOrigin[0] + dialog.boundsInRoot.center.x
                    y = windowOrigin[1] + dialog.boundsInRoot.top + 16f * compose.activity.resources.displayMetrics.density
                    pressure = .4f; orientation = .2f
                    setAxisValue(MotionEvent.AXIS_TILT, .5f)
                }
                val property = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
                val now = SystemClock.uptimeMillis()
                val event = MotionEvent.obtain(now, now, MotionEvent.ACTION_HOVER_MOVE, 1, arrayOf(property), arrayOf(coordinate),
                    0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
                try { compose.activity.dispatchGenericMotionEvent(event) } finally { event.recycle() }
            }
            waitState { calibration().getJSONObject("editor").optJSONArray("marker")?.let { kotlin.math.abs(it.getDouble(0) - .4) < .001 } == true }
            assertEquals(3, points().length())
            val start = points().toString()
            compose.onNodeWithTag("pen-pressure-firmer").performClick()
            waitState { points().toString() != start }
            val firmer = points().getJSONArray(0).getDouble(1)
            compose.onNodeWithTag("pen-pressure-lighter").performClick()
            waitState { kotlin.math.abs(points().getJSONArray(0).getDouble(1) - .125) < .001 }
            assertTrue(firmer < points().getJSONArray(0).getDouble(1))
            val curve = compose.onNodeWithTag("effect-curve")
            curve.performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .55f, height * .04f)) }
            waitState { points().length() == 4 }
            assertEquals(.55, points().getJSONArray(2).getDouble(0), .03)
            assertEquals(1.0, points().getJSONArray(2).getDouble(1), .03)
            val graphForDrag = curve.fetchSemanticsNode().boundsInRoot
            val screen = IntArray(2)
            instrumentation.runOnMainSync { compose.activity.window.decorView.getLocationOnScreen(screen) }
            val startX = screen[0] + graphForDrag.left + graphForDrag.width * .55f
            val endX = screen[0] + graphForDrag.left - graphForDrag.width * .4f
            val dragY = screen[1] + graphForDrag.top + graphForDrag.height * .04f
            val downAt = SystemClock.uptimeMillis()
            fun sendDrag(step: Int, phase: Int) {
                val position = MotionEvent.PointerCoords().apply { x = startX + (endX - startX) * step / 12f; y = dragY; pressure = .7f }
                val property = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_FINGER }
                val event = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), phase, 1,
                    arrayOf(property), arrayOf(position), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) }
                finally { event.recycle() }
            }
            try {
                for (step in 0..11) {
                    sendDrag(step, if (step == 0) MotionEvent.ACTION_DOWN else MotionEvent.ACTION_MOVE)
                    SystemClock.sleep(16)
                }
                waitState { points().length() == 3 }
                assertEquals("Dragged control point disappears during Move: ${points()}", 3, points().length())
            } finally { sendDrag(12, MotionEvent.ACTION_UP) }
            compose.waitForIdle()
            assertEquals("Dragged control points: ${points()}", 3, points().length())
            compose.onNodeWithTag("pen-pressure-reset").performClick()
            waitState { points().toString() == start }
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                val graph = compose.onNodeWithTag("effect-curve").fetchSemanticsNode().boundsInRoot
                val location = IntArray(2)
                instrumentation.runOnMainSync { compose.activity.window.decorView.getLocationOnScreen(location) }
                val contact = MotionEvent.PointerCoords().apply {
                    x = location[0] + graph.left + graph.width * .47f
                    y = location[1] + graph.top + graph.height * .35f
                    pressure = .7f
                }
                val property = MotionEvent.PointerProperties().apply { id = 0; toolType = tool }
                val source = if (tool == MotionEvent.TOOL_TYPE_MOUSE) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_STYLUS
                val down = SystemClock.uptimeMillis()
                for (phase in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_UP)) {
                    if (phase != MotionEvent.ACTION_DOWN) contact.x += graph.width * .02f
                    val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, 1,
                        arrayOf(property), arrayOf(contact), 0,
                        if (tool == MotionEvent.TOOL_TYPE_MOUSE) MotionEvent.BUTTON_PRIMARY else 0,
                        1f, 1f, 0, 0, source, 0)
                    try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) }
                    finally { event.recycle() }
                    SystemClock.sleep(30)
                }
                compose.waitForIdle()
                assertEquals("Tool $tool curve points: ${points()}", 4, points().length())
                compose.onNodeWithTag("pen-pressure-reset").performClick()
                waitState { points().toString() == start }
            }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(androidx.compose.ui.geometry.Offset(.35f, .5f)), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { !calibration().getJSONObject("editor").isNull("marker") }
            canvasEvent(MotionEvent.ACTION_UP, listOf(androidx.compose.ui.geometry.Offset(.35f, .5f)), MotionEvent.TOOL_TYPE_STYLUS)
            compose.onNodeWithTag("pen-pressure-lighter").performClick()
            waitState { points().toString() != start }
            compose.onNodeWithTag("pen-pressure-cancel").performClick()
            waitState { it.objectOrNull("pressure_calibration") == null }
            assertEquals(original, saved())
            open()
            compose.onNodeWithTag("pen-pressure-lighter").performClick()
            compose.onNodeWithTag("pen-pressure-close").performClick()
            waitState { it.objectOrNull("pressure_calibration") == null }
            assertEquals(original, saved())
            open()
            compose.activityRule.scenario.onActivity { it.onBackPressedDispatcher.onBackPressed() }
            waitState { it.objectOrNull("pressure_calibration") == null }
            assertEquals(original, saved())
            open()
            compose.onNodeWithTag("pen-pressure-lighter").performClick()
            waitState { points().toString() != start }
            val applied = points().toString()
            compose.onNodeWithTag("pen-pressure-apply").performClick()
            waitState { it.objectOrNull("pressure_calibration") == null }
            assertEquals("Apply saves the edited pressure curve", applied, saved())
            action(obj("type" to "invoke", "command" to "pencil"))
            host.awaitReady(compose = compose)
            penStroke(15)
            waitState { it.array("commands").objects().first { command -> command.getString("id") == "undo" }.getBoolean("enabled") }
            capture("pressure-calibration-$theme")
            open()
            assertEquals(applied, points().toString())
            compose.onNodeWithTag("pen-pressure-reset").performClick()
            waitState { points().toString() == start }
            compose.onNodeWithTag("pen-pressure-apply").performClick()
            waitState { it.objectOrNull("pressure_calibration") == null && saved() == start }
        }
        assertNull(host.failure)
        assertNull(host.actionError)
    }

    @Test fun settingsPanesShareTopEdgeAndUseAppScale() {
        openSettings()
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
        openSettings()
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
        compose.onNodeWithTag("number-value-prediction_horizon").performScrollTo().performClick()
        val number = compose.onNodeWithTag("setting-number-prediction_horizon")
        number.performTextReplacement("12")
        instrumentation.runOnMainSync {
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
                compose.activity.dispatchKeyEvent(KeyEvent(action, KeyEvent.KEYCODE_3))
        }
        compose.waitForIdle()
        assertEquals("Number editing stays local", "", preferences().optString("query"))
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun baseColorsAreSwatchesWithValidatedCustomHexAndDriveTheNativePalette() {
        openSettings()
        for ((theme, color) in listOf("dark" to "#1C2C3C", "light" to "#C0B49C")) {
            compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
            waitState { it.getString("theme") == theme }
            compose.onNodeWithTag("settings-category-appearance").performClick()
            compose.onNodeWithTag("setting-${theme}_base-swatch-0").performScrollTo().performClick()
            val first = if (theme == "dark") "#1f1f1f" else "#a4a4a4"
            waitState { it.getJSONObject("palette").getString("bg") == first }
            compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-" + theme + "_base"))).assertCountEquals(0)
            compose.onNodeWithTag("setting-${theme}_base-swatch-4").performClick()
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

    @Test fun accentSwatchesFollowSystemPresetsAndCustomHex() {
        openSettings()
        compose.onNodeWithTag("settings-category-appearance").performClick()
        compose.onNodeWithTag("preference-accent").performScrollTo()
        compose.onNodeWithTag("setting-accent-swatch-0").assertContentDescriptionEquals("System").assertIsSelected()
        val system = state().getJSONObject("palette").getString("accent")
        compose.onNodeWithTag("setting-accent-swatch-6").performClick()
        waitState { it.getJSONObject("palette").getString("accent") == "#e62d42" }
        assertEquals("#e62d42", state().getJSONObject("settings").getString("accent"))
        compose.waitForIdle()
        val red = compose.onNodeWithTag("setting-accent-swatch-6").assertIsSelected().captureToImage().toPixelMap()
        val fill = (0 until red.width).flatMap { x -> (0 until red.height).map { y -> red[x, y].toArgb() } }
            .groupingBy { it }.eachCount().maxBy { it.value }.key
        val (r, g, b) = Triple(fill shr 16 and 0xff, fill shr 8 and 0xff, fill and 0xff)
        assertTrue("Red swatch fill #%06x".format(fill and 0xffffff), r > 190 && g < 70 && b < 90)
        compose.onNodeWithTag("setting-accent-swatch-10").performClick()
        val input = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-accent")))
        input.assertTextContains("#e62d42")
        input.performTextReplacement("#12ab56")
        input.performImeAction()
        waitState { it.getJSONObject("palette").getString("accent") == "#12ab56" }
        compose.onNodeWithTag("setting-accent-swatch-10").assertIsSelected()
        capture("41-accent-custom")
        compose.onNodeWithTag("setting-accent-swatch-0").performClick()
        waitState { it.getJSONObject("settings").isNull("accent") || !it.getJSONObject("settings").has("accent") }
        assertEquals(system, state().getJSONObject("palette").getString("accent"))
        compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("setting-text-accent"))).assertCountEquals(0)
        compose.onNodeWithTag("settings-done").performClick()
    }

    @Test fun inlineSettingsApplyValidateAndNeverPaintUnderneath() {
        openSettings()
        compose.onNodeWithTag("settings-category-canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        compose.onNodeWithTag("preference-pan_speed", useUnmergedTree = true).performScrollTo()
        compose.onNodeWithTag("settings-content-page:canvas", useUnmergedTree = true).assertIsDisplayed()
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        val slider = compose.onNodeWithTag("setting-slider-zoom_speed", useUnmergedTree = true).performScrollTo().assertTouchHeightIsEqualTo(48.dp)
        val track = slider.captureToImage().toPixelMap()
        val trackX = track.width * 9 / 10
        assertTrue("Inactive slider track remains visible on the light settings surface",
            track[trackX, track.height / 4].red - track[trackX, track.height / 2].red > .05f)
        capture("32-inline-numbers")
        val before = state().getJSONObject("settings").number("pan_speed")
        slider.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(width * .75f, center.y), 300) }
        compose.waitForIdle()
        waitState { it.getJSONObject("settings").number("zoom_speed") != 1f }
        val dragged = state().getJSONObject("settings").number("zoom_speed")
        assertTrue("A slider drag changes the value inside its range ($dragged)", dragged in .25f..4f)
        compose.onNodeWithTag("number-value-pan_speed", useUnmergedTree = true).performScrollTo().performClick()
        compose.onNodeWithTag("setting-number-pan_speed", useUnmergedTree = true).performTextReplacement("1/0")
        compose.onNodeWithTag("setting-number-pan_speed", useUnmergedTree = true).performImeAction()
        compose.onNodeWithText("Enter a finite number", substring = true).assertExists()
        assertEquals(before, state().getJSONObject("settings").number("pan_speed"))
        capture("33-number-invalid")
        compose.onNodeWithTag("setting-number-pan_speed", useUnmergedTree = true).performTextReplacement("1*2")
        compose.onNodeWithTag("setting-number-pan_speed", useUnmergedTree = true).performImeAction()
        waitState { it.getJSONObject("settings").number("pan_speed") == 2f }
        assertTrue(preferences().isNull("error"))
        compose.onNode(hasText("2", substring = true) and hasAnyAncestor(hasTestTag("number-value-pan_speed")),
            useUnmergedTree = true).assertIsDisplayed()
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
        compose.onNodeWithTag("settings-done", useUnmergedTree = true).performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertEquals(2f, state().getJSONObject("settings").number("pan_speed"))
        openSettings()
        compose.onNodeWithTag("settings-category-canvas").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "canvas" }
        compose.onNode(hasText("2", substring = true) and hasAnyAncestor(hasTestTag("number-value-pan_speed")),
            useUnmergedTree = true).performScrollTo().assertIsDisplayed()
        compose.runOnIdle {
            host.preference(obj("type" to "edit", "id" to "pan_speed", "value" to before))
            host.preference(obj("type" to "edit", "id" to "zoom_speed", "value" to 1.0))
        }
        waitState { it.getJSONObject("settings").number("pan_speed") == before }
        compose.onNodeWithTag("settings-done", useUnmergedTree = true).performClick()
    }

    @Test fun settingDefaultsResetFromContextAndEmptyCommits() {
        openSettings()
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
        compose.onNodeWithTag("setting-dark_base-swatch-4").performClick()
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
        compose.onNodeWithTag("number-value-prediction_horizon").performScrollTo().performClick()
        val number = compose.onNodeWithTag("setting-number-prediction_horizon")
        number.performTextReplacement("32"); number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 32f }
        compose.onNodeWithTag("number-value-prediction_horizon").performClick()
        number.performTextReplacement("")
        assertEquals(32f, state().getJSONObject("settings").number("prediction_ms"))
        number.performImeAction()
        waitState { it.getJSONObject("settings").number("prediction_ms") == 16f }
        compose.onNodeWithTag("settings-done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        assertNull(host.actionError)
    }

    @Test fun allPreferenceRowsRenderCoreMetadataAndTrailingControls() {
        openSettings()
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
        compose.onNodeWithTag("setting-slider-prediction_horizon").assertIsNotEnabled()
        compose.onNodeWithTag("preference-feedback").performClick()
        waitState { it.getJSONObject("settings").getBoolean("feedback") }
        compose.onNodeWithTag("setting-slider-prediction_horizon").assertIsEnabled()
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("37-inline-controls-dark")
        compose.onNodeWithTag("settings-done").performClick()
    }

    private fun prepareControlledLayersDrawer(theme: String, minimumLayers: Int = 5) {
        compose.mainClock.autoAdvance = true
        customize(obj("type" to "close_expanded"))
        compose.waitUntil(10_000) { findTag("tool-drawer") == null }
        action(obj("type" to "set_theme", "theme" to theme))
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").put("header", obj("size" to "medium", "next_id" to 3,
            "zones" to JSONArray(listOf(JSONArray(), JSONArray(listOf(obj("id" to 2,
                "item" to obj("kind" to "tool", "control" to obj("kind" to "panel", "panel" to "layers"))))), JSONArray()))))
        action(obj("type" to "restore_workspace", "workspace" to fixture))
        customize(obj("type" to "set_panel_visible", "panel" to "layers", "visible" to false))
        repeat((minimumLayers - state().array("layers").length()).coerceAtLeast(0)) {
            action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
        }
    }

    private fun controlledDrawerFrameHeight(): Float? {
        host.drain()
        compose.mainClock.advanceTimeByFrame()
        compose.waitForIdle()
        host.drain()
        compose.waitForIdle()
        return compose.runOnIdle {
            findTag("tool-drawer")?.second?.takeIf { it.layoutInfo.isPlaced }?.boundsInRoot?.height
        }
    }

    @Test fun controlledDrawerOpeningAndRetargetPreserveMeasuredContent() {
        val originalAdvance = compose.mainClock.autoAdvance
        try {
            for (theme in listOf("light", "dark")) {
                prepareControlledLayersDrawer(theme, minimumLayers = 7)
                val beforeCount = state().array("layers").length()
                val heights = mutableListOf<Float>()
                val placed = mutableListOf<Boolean>()
                val times = mutableListOf<Long>()
                var positive = 0
                var retarget = -1
                var initialTarget = 0f
                compose.mainClock.autoAdvance = false
                compose.runOnIdle { host.dispatch(obj("type" to "activate_header_item", "id" to 2)) }
                repeat(60) { index ->
                    val height = controlledDrawerFrameHeight()
                    heights.add(height ?: 0f); placed.add(height != null); times.add(compose.mainClock.currentTime)
                    if (height != null && height > 0f && ++positive == 3) {
                        retarget = index
                        compose.runOnIdle {
                            initialTarget = host.workspaceInteraction!!.drawer!!.getJSONObject("placement").array("columns").getJSONObject(0).number("height")
                            host.dispatch(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
                        }
                    }
                }
                File(compose.activity.getExternalFilesDir(null), "validation/batch2/controlled-drawer-$theme.json").apply {
                    parentFile!!.mkdirs()
                    writeText(obj("clock_ms" to JSONArray(times), "placed" to JSONArray(placed), "height_px" to JSONArray(heights),
                        "retarget_frame" to retarget, "initial_target_dp" to initialTarget, "before_layers" to beforeCount, "after_layers" to state().array("layers").length()).toString(2))
                }
                assertTrue("$theme measured drawer is actually placed", placed.any { it })
                assertTrue("$theme never places a zero-height placeholder: $heights", heights.indices.all { !placed[it] || heights[it] > 0f })
                val growing = heights.zipWithNext().count { (a, b) -> b - a > 1f }
                assertTrue("$theme has at least four growing controlled frames: $heights", growing >= 4)
                val final = heights.last()
                assertTrue("$theme opens a complete measured drawer", final > 120f * compose.activity.resources.displayMetrics.density)
                assertTrue("$theme has no snap larger than half the final drawer: $heights", heights.zipWithNext().all { (a, b) -> b - a <= final * .5f })
                assertEquals("$theme retarget adds exactly one real layer", beforeCount + 1, state().array("layers").length())
                assertTrue("$theme added layer grows the measured target: $initialTarget -> $final px", initialTarget > 0f && final / compose.activity.resources.displayMetrics.density > initialTarget + 1f)
                assertTrue("$theme retarget starts during opening", retarget >= 0 && heights[retarget] < final)
                assertTrue("$theme retarget continues from current geometry: $heights", heights.drop(retarget).all { it >= heights[retarget] - 1f })
                compose.runOnIdle { host.customize(obj("type" to "close_expanded")) }
                val closing = listOf(final) + List(30) { controlledDrawerFrameHeight() ?: 0f }
                File(compose.activity.getExternalFilesDir(null), "validation/batch2/controlled-close-$theme.json").apply {
                    parentFile!!.mkdirs(); writeText(obj("height_px" to JSONArray(closing)).toString(2))
                }
                assertTrue("$theme closes through at least four shrinking controlled frames: $closing", closing.zipWithNext().count { (a, b) -> a - b > 1f } >= 4)
                assertTrue("$theme closing never snaps by more than half its starting height: $closing", closing.zipWithNext().all { (a, b) -> a - b <= final * .5f })
                assertEquals("$theme closing removes the native drawer", 0f, closing.last(), 0f)
            }
        } finally { compose.mainClock.autoAdvance = originalAdvance }
    }

    @Test fun layersDrawerKeepsMeasuredSizeAfterStateEdits() {
        val originalAdvance = compose.mainClock.autoAdvance
        try {
            for (theme in listOf("light", "dark")) {
                prepareControlledLayersDrawer(theme)
                compose.mainClock.autoAdvance = false
                compose.runOnIdle { host.dispatch(obj("type" to "activate_header_item", "id" to 2)) }
                val opening = List(60) { controlledDrawerFrameHeight() ?: 0f }
                val initial = opening.last()
                assertTrue("$theme settled drawer contains measured rows", initial > 120f * compose.activity.resources.displayMetrics.density)
                val selected = state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
                val target = state().array("layers").objects().first { it.getLong("id") != selected && it.optBoolean("can_rename") }.getLong("id")
                val name = "Controlled $theme row"
                val opacity = if (theme == "light") .5 else .6
                val edits = listOf("select" to obj("type" to "layer", "action" to obj("op" to "select", "id" to target, "mask" to false)),
                    "rename" to obj("type" to "layer", "action" to obj("op" to "rename", "id" to target, "name" to name)),
                    "opacity" to obj("type" to "set_layer_opacity", "id" to target, "opacity" to opacity))
                for ((edit, value) in edits) {
                    compose.runOnIdle { host.dispatch(value) }
                    val heights = List(20) { controlledDrawerFrameHeight() ?: 0f }
                    val layer = state().array("layers").objects().single { it.getLong("id") == target }
                    File(compose.activity.getExternalFilesDir(null), "validation/batch2/controlled-layers-$theme-$edit.json").apply {
                        parentFile!!.mkdirs()
                        writeText(obj("initial_height_px" to initial, "height_px" to JSONArray(heights), "layer" to layer).toString(2))
                    }
                    when (edit) {
                        "select" -> assertEquals("$theme selection actually changes row", target, state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id"))
                        "rename" -> assertEquals("$theme rename actually changes row text", name, layer.getString("label"))
                        "opacity" -> assertEquals("$theme opacity actually changes row metadata", opacity, layer.getDouble("opacity"), .0001)
                    }
                    assertTrue("$theme/$edit keeps its settled measured height: $initial -> $heights", heights.all { it >= initial - 1f })
                }
            }
        } finally { compose.mainClock.autoAdvance = originalAdvance }
    }

    @Test fun drawerTileMeasurementsPublishOncePerComposition() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            val dock = compose.runOnIdle { DockInteraction(host).apply { density = compose.activity.resources.displayMetrics.density } }
            val tiles = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
                .array("tiles").objects().take(3).map { it.getInt("id") }
            assertEquals("$theme fixture supplies three distinct drawer tiles", 3, tiles.distinct().size)
            val stage = androidx.compose.runtime.mutableIntStateOf(0)
            lateinit var view: androidx.compose.ui.platform.ComposeView
            compose.runOnIdle {
                view = androidx.compose.ui.platform.ComposeView(compose.activity)
                compose.activity.addContentView(view, android.widget.FrameLayout.LayoutParams(1, 1).apply {
                    gravity = android.view.Gravity.BOTTOM or android.view.Gravity.END
                })
                view.setContent {
                    val current = stage.intValue
                    androidx.compose.runtime.SideEffect {
                        tiles.forEachIndexed { index, tile ->
                            val bounds = if (current == 1 || current == 2)
                                androidx.compose.ui.geometry.Rect(index * 36f, 0f, (index + 1) * 36f, 36f) else null
                            dock.measureDrawerTile(41, "toolbar", tile, bounds)
                        }
                    }
                    dock.publishMeasurements()
                }
            }
            try {
                compose.waitForIdle(); host.drain()
                val initial = compose.runOnIdle { dock.drawerTileRevision }
                compose.runOnIdle { stage.intValue = 1 }
                compose.waitForIdle(); host.drain()
                compose.runOnIdle { assertEquals("$theme three measurements publish one revision", initial + 1, dock.drawerTileRevision) }
                compose.runOnIdle { stage.intValue = 2 }
                compose.waitForIdle(); host.drain()
                compose.runOnIdle { assertEquals("$theme unchanged reports do not republish", initial + 1, dock.drawerTileRevision) }
                compose.runOnIdle { stage.intValue = 3 }
                compose.waitForIdle(); host.drain()
                compose.runOnIdle { assertEquals("$theme three removals publish one revision", initial + 2, dock.drawerTileRevision) }
            } finally {
                compose.runOnIdle { view.disposeComposition(); (view.parent as? android.view.ViewGroup)?.removeView(view) }
                host.drain()
            }
        }
    }



    @Test fun layersStandardUnitHeightDoesNotFollowVisibleMixedRows() {
        val original = android.content.res.Configuration(compose.activity.resources.configuration)
        fun configuration(value: android.content.res.Configuration) = compose.activityRule.scenario.onActivity { activity ->
            @Suppress("DEPRECATION")
            activity.resources.updateConfiguration(value, activity.resources.displayMetrics)
            activity.window.decorView.dispatchConfigurationChanged(value)
            activity.onConfigurationChanged(value)
        }
        try {
            for (fontScale in listOf(1f, 1.3f)) {
                configuration(android.content.res.Configuration(original).apply { this.fontScale = fontScale })
                for (theme in listOf("light", "dark")) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
                    repeat(24) { action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false))) }
                    val rows = state().array("layers").objects().take(24)
                    val ids = rows.map { it.getLong("id") }
                    for (id in ids.drop(12)) action(obj("type" to "set_layer_opacity", "id" to id, "opacity" to .5))
                    floatPanel("layers", 100f, 100f)
                    val fixture = JSONObject(state().getJSONObject("workspace").toString())
                    fixture.getJSONObject("layout").array("floating").objects().first { it.getJSONObject("root").array("panels").values().contains("layers") }
                        .put("height", 220).put("width", 300)
                    fixture.getJSONObject("layout").put("fit_height_groups", JSONArray())
                    action(obj("type" to "restore_workspace", "workspace" to fixture))
                    fun unit(): Float = host.snapshot!!.array("panel_measurements").objects().first { it.getString("panel") == "layers" }
                        .getJSONObject("scroll").number("unit_height")
                    compose.onNodeWithTag("layer-rows").performScrollToIndex(0)
                    settle()
                    val ordinary = bounds("layer-row-${ids[0]}").height / compose.activity.resources.displayMetrics.density
                    val expected = unit()
                    assertEquals("$theme/$fontScale standard unit follows the ordinary native row", ordinary, expected, 1f / compose.activity.resources.displayMetrics.density)
                    compose.onNodeWithTag("layer-rows").performScrollToIndex(18)
                    settle()
                    val described = bounds("layer-row-${ids[18]}").height / compose.activity.resources.displayMetrics.density
                    if (fontScale == 1.3f) assertTrue("$theme font scale 1.3 produces a taller described row", described > ordinary + 1f)
                    val units = mutableListOf<Float>()
                    for (index in listOf(0, 18, 2, 20)) {
                        compose.onNodeWithTag("layer-rows").performScrollToIndex(index)
                        settle(); units.add(unit())
                    }
                    File(compose.activity.getExternalFilesDir(null), "validation/batch2/layers-$theme-$fontScale-unit.json").apply {
                        parentFile!!.mkdirs(); writeText(obj("font_scale" to compose.activity.resources.configuration.fontScale,
                            "ordinary" to ordinary, "described" to described, "units" to JSONArray(units)).toString(2))
                    }
                    assertTrue("$theme/$fontScale visible row composition cannot change the standard unit: $units", units.all { kotlin.math.abs(it - expected) <= 1f / compose.activity.resources.displayMetrics.density })
                }
            }
        } finally { configuration(original) }
    }

    @Test fun mouseScrollbarThumbUsesTheNativeViewport() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to 0)))
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
            action(obj("type" to "select_brush", "id" to 1))
            floatPanel("tool_settings", 120f, 100f)
            val fixture = JSONObject(state().getJSONObject("workspace").toString())
            fixture.getJSONObject("layout").array("floating").objects().first { it.getJSONObject("root").array("panels").values().contains("tool_settings") }
                .put("height", 132).put("width", 300)
            fixture.getJSONObject("layout").put("fit_height_groups", JSONArray())
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            settle()
            val body = compose.onNodeWithTag("panel-body-tool_settings", useUnmergedTree = true)
            val node = body.fetchSemanticsNode().find { it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.maxValue?.invoke()?.let { max -> max > 0f } == true }
            assertNotNull("$theme Tool settings actually overflows", node)
            val maximum = node!!.config[SemanticsProperties.VerticalScrollAxisRange].maxValue()
            val frame = screenBounds(body)
            val density = compose.activity.resources.displayMetrics.density
            assertTrue("$theme fixture content exceeds three viewports", maximum > frame.height * 2f)
            fun hover(tool: Int) {
                val time = SystemClock.uptimeMillis()
                val event = MotionEvent.obtain(time, time, MotionEvent.ACTION_HOVER_MOVE, 1,
                    arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
                    arrayOf(MotionEvent.PointerCoords().apply { x = frame.center.x; y = frame.center.y }),
                    0, 0, 1f, 1f, 0, 0, if (tool == MotionEvent.TOOL_TYPE_MOUSE) InputDevice.SOURCE_MOUSE else InputDevice.SOURCE_STYLUS, 0)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
                settle()
            }
            hover(MotionEvent.TOOL_TYPE_STYLUS)
            val before = instrumentation.uiAutomation.takeScreenshot()!!.let { raw ->
                try { raw.copy(Bitmap.Config.ARGB_8888, false) } finally { raw.recycle() }
            }
            try {
                hover(MotionEvent.TOOL_TYPE_MOUSE)
                val after = instrumentation.uiAutomation.takeScreenshot()!!.let { raw ->
                    try { raw.copy(Bitmap.Config.ARGB_8888, false) } finally { raw.recycle() }
                }
                try {
                    val x = (frame.right - 4f * density).toInt()
                    val changed = ((frame.top + 2f * density).toInt() until (frame.bottom - 2f * density).toInt()).filter { y ->
                        val a = before.getPixel(x, y); val b = after.getPixel(x, y)
                        kotlin.math.abs(android.graphics.Color.red(a) - android.graphics.Color.red(b)) +
                            kotlin.math.abs(android.graphics.Color.green(a) - android.graphics.Color.green(b)) +
                            kotlin.math.abs(android.graphics.Color.blue(a) - android.graphics.Color.blue(b)) > 24
                    }
                    assertTrue("$theme mouse hover paints a real scrollbar thumb", changed.isNotEmpty())
                    val observed = changed.last() - changed.first() + 1f
                    val expected = (frame.height * frame.height / (frame.height + maximum)).coerceIn((28f * density).coerceAtMost(frame.height), frame.height)
                    val directory = File(compose.activity.getExternalFilesDir(null), "validation/batch2").apply { mkdirs() }
                    File(directory, "scrollbar-$theme.json").writeText(obj("viewport" to frame.height, "maximum" to maximum, "expected" to expected, "observed" to observed).toString(2))
                    File(directory, "scrollbar-$theme.png").outputStream().use { after.compress(Bitmap.CompressFormat.PNG, 100, it) }
                    assertEquals("$theme thumb is viewport squared / content, not the whole viewport", expected, observed, 4f * density)
                } finally { after.recycle() }
            } finally { before.recycle() }
        }
    }

    @Test fun panelDrawerAndDividerUseSharedLayout() {
        val source = group("brushes").getInt("id")
        customize(obj("type" to "set_column_collapsed", "group" to source, "collapsed" to true))
        val column = host.snapshot!!.getJSONObject("layout").array("collapsed").objects()
            .first { c -> c.array("groups").objects().any { it.getInt("group") == source } }.getInt("id")
        customize(obj("type" to "set_column_drawers", "column" to column, "drawers" to true))
        compose.onNodeWithTag("column-icon-brushes").performClick()
        waitState { it.getJSONObject("customization").array("column_drawers").length() == 1 }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("column-drawer-$column").fetchSemanticsNodes().isNotEmpty() }
        capture("09-brush-drawer")
        compose.onNodeWithTag("column-icon-brushes").performClick()
        waitState { it.getJSONObject("customization").array("column_drawers").length() == 0 }
        action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
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
        val target = compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[2]}").fetchSemanticsNode().boundsInRoot
        holdDrag(compose.onNodeWithTag("tile-${custom.getString("id")}-${ids[0]}"), target.centerRight - androidx.compose.ui.geometry.Offset(2f, 0f))
        compose.waitUntil(10_000) {
            host.snapshot!!.array("panels").objects().first { it.getString("id") == custom.getString("id") }
                .array("tiles").objects().map { it.getInt("id") } != ids
        }
        capture("12-custom-toolbar")
        assertNull(host.actionError)
    }

    @Test fun toolChangesReuseBrushPreviewsAcrossPanelsAndThemes() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "select_brush", "id" to 1))
            compose.waitUntil(10_000) { host.bundledPreviews.get("1-$theme.webp") != null }
            val preview = host.bundledPreviews.get("1-$theme.webp")
            compose.onNodeWithTag("brush-preview-1", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
            action(obj("type" to "select_brush", "id" to 42))
            waitState { it.getJSONObject("brush").getInt("preset") == 42 }
            action(obj("type" to "select_brush", "id" to 1))
            waitState { it.getJSONObject("brush").getInt("preset") == 1 }
            assertSame(preview, host.bundledPreviews.get("1-$theme.webp"))
            compose.onNodeWithTag("brush-preview-1", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        }
        assertNotSame(host.bundledPreviews.get("1-light.webp"), host.bundledPreviews.get("1-dark.webp"))
    }

    @Test fun compactPanelSpacingAndAlignmentAcrossContexts() {
        device.landscape(compose.activityRule.scenario)
        val density = compose.activity.resources.displayMetrics.density
        val control = host.catalog.getJSONObject("panel_metrics").getDouble("control_height").toFloat()
        val panelControl = control
        val reports = JSONArray()
        val output = File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }
        fun node(panel: String, tag: String) = compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("panel-body-$panel")), useUnmergedTree = true)
        fun area(panel: String, tag: String) = node(panel, tag).fetchSemanticsNode().boundsInRoot
        fun gap(panel: String, before: String, after: String, expected: Float, horizontal: Boolean = false) {
            val first = area(panel, before); val next = area(panel, after)
            val actual = if (horizontal) next.left - first.right else next.top - first.bottom
            assertEquals("$panel $before → $after allocated gap", expected * density, actual, 1f)
        }
        fun inspect(context: String, panel: String, theme: String, width: Int) {
            compose.waitForIdle()
            val body = compose.onNodeWithTag("panel-body-$panel", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val nodes = compose.onAllNodes(hasAnyAncestor(hasTestTag("panel-body-$panel")), useUnmergedTree = true).fetchSemanticsNodes()
            val controls = nodes.filter { it.config.getOrNull(SemanticsProperties.TestTag) != null || it.config.getOrNull(SemanticsProperties.Text) != null }
            reports.put(obj("context" to context, "theme" to theme, "requested_width" to width, "density" to density, "panel_metrics" to host.catalog.getJSONObject("panel_metrics"),
                "panel_bounds" to JSONArray(listOf(body.left, body.top, body.width, body.height)), "controls" to JSONArray(controls.map {
                    val bounds = it.boundsInRoot
                    obj("tag" to it.config.getOrNull(SemanticsProperties.TestTag), "text" to it.config.getOrNull(SemanticsProperties.Text)?.joinToString { text -> text.text },
                        "bounds" to JSONArray(listOf(bounds.left - body.left, bounds.top - body.top, bounds.width, bounds.height)))
                })))
            File(output, "compact-panel-controls.json").writeText(reports.toString(2))
            capture("compact-panels-$context-$theme-$width")
            controls.filter { it.boundsInRoot.width > 0 && it.boundsInRoot.height > 0 }.forEach {
                assertTrue("$context ${it.config.getOrNull(SemanticsProperties.TestTag)} fits its horizontal panel bounds", it.boundsInRoot.left >= body.left - 1 && it.boundsInRoot.right <= body.right + 1)
                val tag = it.config.getOrNull(SemanticsProperties.TestTag) ?: ""
                if (tag.startsWith("tool-action-") || tag.startsWith("property-action-") || tag == "properties-page") {
                    assertEquals("$context $tag compact single-line control", control * density, it.boundsInRoot.height, 1f)
                }
            }
        }
        action(obj("type" to "invoke", "command" to "add_layer"))
        action(obj("type" to "invoke", "command" to "select_all"))
        action(obj("type" to "layer", "action" to obj("op" to "fill_selection")))
        for (theme in listOf("light", "dark")) for (width in listOf(242, 320)) {
            val fixture = JSONObject(defaultWorkspace)
            fixture.getJSONObject("layout").apply {
                fun tabs(id: Int, panel: String) = obj("kind" to "tabs", "id" to id, "panels" to JSONArray(listOf(panel)), "active" to panel, "tab_style" to "automatic")
                put("bands", JSONArray(listOf(obj("id" to 40, "edge" to "left", "extent" to width, "root" to tabs(41, "tool_settings")),
                    obj("id" to 42, "edge" to "right", "extent" to width, "root" to tabs(43, "properties")))))
                put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
                put("next_id", maxOf(44, getInt("next_id")))
            }
            fixture.put("zen_mode", false)
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            action(obj("type" to "set_theme", "theme" to theme))
            for ((context, command) in listOf("painting" to "brush", "fill" to "fill", "zoom" to "zoom", "eyedropper" to "eyedropper", "gradient" to "gradient", "crop" to "crop")) {
                action(obj("type" to "invoke", "command" to command))
                inspect(context, "tool_settings", theme, width)
                val fields = state().array("tool_settings").objects()
                val compound = state().array("tool_extra").objects().mapNotNull { it.optJSONObject("Choice")?.optString("beside")?.takeIf(String::isNotEmpty) }.toSet()
                fields.filter { it.getJSONObject("numeric").getString("kind") == "slider" && it.getString("id") !in compound }.forEach { field ->
                    val id = field.getString("id")
                    val bounds = area("tool_settings", "tool-setting-$id")
                    val labels = compose.onAllNodes(hasText(field.getString("label")) and hasAnyAncestor(hasTestTag("tool-setting-$id")), useUnmergedTree = true).fetchSemanticsNodes()
                    assertEquals("$context $id plain label aligns with the field", bounds.left, labels.single().boundsInRoot.left, 1f)
                    assertEquals("$context $id compact slider row", 36f * density, bounds.height, 1f)
                    val slider = compose.onNode(hasTestTag("number-slider-${field.getString("label")}") and hasAnyAncestor(hasTestTag("tool-setting-$id")), useUnmergedTree = true).fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    val readout = node("tool_settings", "number-value-$id").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    assertEquals("$context $id reserves the upstream editor column", bounds.right - 76f * density, slider.right, 1f)
                    assertEquals("$context $id readout bottom aligns with the slider row", bounds.bottom, readout.bottom, 1f)
                    assertEquals("$context $id readout glyphs retain field padding", bounds.right - 6f * density, readout.right, 1f)
                    assertEquals("$context $id compact readout height", control * density, readout.height, 1f)
                }
                fields.zipWithNext().filter { (first, next) -> first.getString("group") == next.getString("group") && first.getString("id") !in compound && next.getString("id") !in compound }.forEach { (first, next) ->
                    gap("tool_settings", "tool-setting-${first.getString("id")}", "tool-setting-${next.getString("id")}", 4f)
                }
                when (context) {
                    "painting" -> {
                        val smoothing = fields.single { it.getString("id") == "smoothing_strength" }
                        assertEquals("Smoothing", smoothing.getString("label"))
                        assertEquals("", smoothing.getString("group_id"))
                        assertEquals("", smoothing.getString("group"))
                    }
                    "zoom" -> gap("tool_settings", "tool-segments-zoom-click", "tool-segments-zoom-drag", 4f)
                    "gradient" -> {
                        gap("tool_settings", "tool-segments-gradient-shape", "gradient-interpolation", 4f)
                        gap("tool_settings", "gradient-interpolation", "effect-gradient", 4f)
                        gap("tool_settings", "effect-gradient", "gradient-remove", 4f)
                        gap("tool_settings", "gradient-remove", "tool-setting-opacity", 4f)
                        gap("tool_settings", "gradient-reverse", "gradient-reset", 4f, horizontal = true)
                        val caption = host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("color")
                        gap("tool_settings", "gradient-remove", "property-color-$caption", 4f, horizontal = true)
                        for (tag in listOf("gradient-reverse", "gradient-reset", "gradient-remove", "gradient-use-color", "property-color-$caption")) {
                            assertEquals("Gradient $tag compact peer height", control * density, area("tool_settings", tag).height, 1f)
                        }
                    }
                    "eyedropper" -> {
                        gap("tool_settings", "picker-setting-source", "picker-setting-size", 4f)
                        val source = area("tool_settings", "picker-setting-source"); val size = area("tool_settings", "picker-setting-size")
                        assertEquals("Picker source compact row", control * density, source.height, 1f)
                        assertEquals("Picker size compact row", control * density, size.height, 1f)
                        fun choice(tag: String) = compose.onNode(hasClickAction() and hasAnyAncestor(hasTestTag(tag)), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
                        val sourceChoice = choice("picker-setting-source"); val sizeChoice = choice("picker-setting-size")
                        assertEquals("Picker dropdowns share their left column", sourceChoice.left, sizeChoice.left, 1f)
                        assertEquals("Picker dropdowns share their right rail", sourceChoice.right, sizeChoice.right, 1f)
                        val copy = host.catalog.getJSONObject("native_copy").getJSONObject("sampler")
                        for ((tag, caption) in listOf("picker-setting-source" to "source", "picker-setting-size" to "sample_size")) {
                            val label = compose.onNode(hasText(copy.getString(caption)) and hasAnyAncestor(hasTestTag(tag)), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
                            assertEquals("Picker plain caption aligns with its row", source.left, label.left, 1f)
                        }
                    }
                    "crop" -> gap("tool_settings", "tool-action-apply_transform", "tool-action-cancel_transform", 4f, horizontal = true)
                }
                if (context == "fill") {
                    val field = fields.first { it.getString("id") == "gap_closing" }
                    val editor = node("tool_settings", "number-${field.getString("label")}")
                    gap("tool_settings", "preference-label-gap_closing", "number-${field.getString("label")}", 4f, horizontal = true)
                    editor.performClick()
                    editor.performTextReplacement("3*2"); editor.performImeAction()
                    waitState { it.array("tool_settings").objects().first { setting -> setting.getString("id") == "gap_closing" }.getDouble("value") == 6.0 }
                    editor.performClick()
                    editor.performTextReplacement("sqrt(-1)"); editor.performImeAction()
                    editor.assertIsFocused()
                    assertEquals(6.0, state().array("tool_settings").objects().first { it.getString("id") == "gap_closing" }.getDouble("value"), 0.0)
                    node("tool_settings", "number-error-gap_closing").assertExists()
                    inspect("fill-invalid", "tool_settings", theme, width)
                    editor.performTextReplacement("0"); editor.performImeAction()
                }
                if (context == "crop") action(obj("type" to "invoke", "command" to "cancel_transform"))
            }
            action(obj("type" to "invoke", "command" to "rectangle_select"))
            action(obj("type" to "invoke", "command" to "select_all"))
            action(obj("type" to "layer", "action" to obj("op" to "fill_selection")))
            action(obj("type" to "invoke", "command" to "scale_rotate"))
            waitState { it.array("commands").objects().first { command -> command.getString("id") == "transform_free" }.getBoolean("enabled") }
            for (mode in listOf("free", "warp")) {
                action(obj("type" to "invoke", "command" to "transform_$mode"))
                gap("tool_settings", "tool-action-transform_flip_horizontal", "tool-action-transform_flip_vertical", 4f, horizontal = true)
                gap("tool_settings", "tool-action-transform_flip_horizontal", "tool-action-transform_rotate_left", 4f)
                gap("tool_settings", "tool-action-transform_rotate_left", "tool-action-transform_rotate_right", 4f, horizontal = true)
                gap("tool_settings", "tool-action-apply_transform", "tool-action-cancel_transform", 4f, horizontal = true)
                inspect("transform-$mode", "tool_settings", theme, width)
            }
            action(obj("type" to "invoke", "command" to "cancel_transform"))
            inspect("properties-paint", "properties", theme, width)
            val property = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "opacity" }
            val value = node("properties", "number-value-opacity").fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
            val label = compose.onNode(hasText(property.getString("label")) and hasAnyAncestor(hasTestTag("panel-body-properties")), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val body = compose.onNodeWithTag("panel-body-properties", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            assertEquals("Properties plain labels respect the eight-pixel panel rail", body.left + 8f * density, label.left, 1f)
            assertEquals("Properties numeric glyphs retain six-pixel field padding inside the panel rail", body.right - 14f * density, value.right, 1f)
            for (effect in listOf("levels", "curves")) {
                action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effect)))
                val controls = state().getJSONObject("layer_properties").array("controls").objects()
                fun slider(control: JSONObject) = control.getJSONObject("kind").getString("kind") == "number" && control.getJSONObject("kind").getJSONObject("numeric").getString("kind") == "slider"
                controls.zipWithNext().filter { (first, next) -> slider(first) && slider(next) && propertySectionId(first) == propertySectionId(next) }.forEach { (first, next) ->
                    val before = area("properties", "number-value-${first.getString("key")}"); val after = area("properties", "number-value-${next.getString("key")}")
                    assertEquals("$effect compact sliders keep four-pixel peer gaps", 40f * density, after.top - before.top, 1f)
                }
                inspect("properties-$effect", "properties", theme, width)
                action(obj("type" to "invoke", "command" to "undo"))
            }
            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "proof", "visible" to true)))
            fixture.getJSONObject("layout").getJSONArray("bands").apply {
                getJSONObject(0).getJSONObject("root").put("panels", JSONArray(listOf("adjustments"))).put("active", "adjustments")
                getJSONObject(1).getJSONObject("root").put("panels", JSONArray(listOf("proof"))).put("active", "proof")
            }
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to "distort")))
            val category = state().array("filter_categories").objects().first { it.optString("id") == "distort" }.getString("label")
            val categoryChoice = compose.onNode(hasClickAction() and hasText(category) and hasAnyAncestor(hasTestTag("panel-body-adjustments")))
            val categoryBounds = categoryChoice.fetchSemanticsNode().boundsInRoot
            val filterBody = compose.onNodeWithTag("panel-body-adjustments", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            assertEquals("Filters category compact height", control * density, categoryBounds.height, 1f)
            assertEquals("Filters category/search peer gap", 4f * density, area("adjustments", "filter-search-toggle").left - categoryBounds.right, 1f)
            assertEquals("Filters search button right rail", filterBody.right - 8f * density, area("adjustments", "filter-search-toggle").right, 1f)
            assertEquals("Filters header/list peer gap", 4f * density, area("adjustments", "filter-list").top - categoryBounds.bottom, 1f)
            assertEquals("Filters list left rail", filterBody.left + 8f * density, area("adjustments", "filter-list").left, 1f)
            inspect("filters-category", "adjustments", theme, width)
            node("adjustments", "filter-search-toggle").performClick()
            compose.waitUntil(10_000) { compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search"))).fetchSemanticsNodes().isNotEmpty() }
            val search = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search")))
            search.performClick()
            instrumentation.sendStringSync("glass")
            waitState { it.array("adjustments").objects().map { choice -> choice.getString("id") } == listOf("glass", "rainy_glass") }
            search.assertTextContains("glass").performImeAction()
            search.assertTextContains("glass").assertIsNotFocused()
            assertEquals("Filters search compact height", control * density, area("adjustments", "filter-search").height, 1f)
            assertEquals("Filters search left rail", filterBody.left + 8f * density, area("adjustments", "filter-search").left, 1f)
            gap("adjustments", "filter-search", "filter-search-toggle", 4f, horizontal = true)
            gap("adjustments", "filter-search", "filter-list", 4f)
            inspect("filters-search", "adjustments", theme, width)
            node("adjustments", "filter-search-toggle").performClick()
            action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
            compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("proof-mode-print") and isEnabled()).fetchSemanticsNodes().isNotEmpty() }
            node("proof", "proof-mode-print").performClick()
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("proof-profile").fetchSemanticsNodes().isNotEmpty() }
            val proofControls = host.proof.form!!.array("print_controls").objects().filter { it.getString("id") in listOf("profile", "simulation", "intent") }
            val proofTags = proofControls.map { if (it.getString("id") == "profile") "proof-profile" else "color-choice-${it.getString("label")}" }
            for ((control, tag) in proofControls.zip(proofTags)) {
                val caption = compose.onNode(hasText(control.getString("label")) and hasAnyAncestor(hasTestTag("panel-body-proof")), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
                assertEquals("Proof ${control.getString("id")} label/control peer gap", 4f * density, area("proof", tag).left - caption.right, 1f)
                assertEquals("Proof $tag compact height", panelControl * density, area("proof", tag).height, 1f)
            }
            val proofModes = listOf("off") + (if (host.proof.form!!.optBoolean("hdr")) listOf("sdr") else emptyList()) + "print"
            proofModes.zipWithNext().forEach { (first, next) -> gap("proof", "proof-mode-$first", "proof-mode-$next", 4f, horizontal = true) }
            proofTags.zipWithNext().forEach { (first, next) -> gap("proof", first, next, 4f) }
            inspect("proof-print", "proof", theme, width)
            node("proof", "proof-mode-off").performClick()
            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "palettes", "visible" to true)))
            fixture.getJSONObject("layout").getJSONArray("bands").getJSONObject(0).getJSONObject("root").put("panels", JSONArray(listOf("palettes"))).put("active", "palettes")
            action(obj("type" to "restore_workspace", "workspace" to fixture))
            val paletteBody = compose.onNodeWithTag("panel-body-palettes", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val footer = area("palettes", "palette-footer")
            assertEquals("Palette footer left rail", paletteBody.left + 8f * density, footer.left, 1f)
            assertEquals("Palette footer right rail", paletteBody.right - 8f * density, footer.right, 1f)
            assertEquals("Palette footer bottom inset", paletteBody.bottom - 8f * density, footer.bottom, 1f)
            assertEquals("Palette chooser compact height", control * density, area("palettes", "palette-chooser").height, 1f)
            assertEquals("Palette chooser text padding", 6f * density, area("palettes", "palette-selector-name").left - area("palettes", "palette-chooser").left, 1f)
            inspect("palettes-footer", "palettes", theme, width)
            node("palettes", "palette-chooser").performClick()
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("palette-search").fetchSemanticsNodes().isNotEmpty() }
            assertEquals("Palette search compact height", control * density, area("palettes", "palette-search").height, 1f)
            gap("palettes", "palette-search", "palette-library-add", 4f, horizontal = true)
            inspect("palettes-search", "palettes", theme, width)
            node("palettes", "palette-chooser").performClick()
        }
    }

    @Test fun editorGeometryStaysConsistent() {
        action(obj("type" to "customize", "action" to obj("type" to "set_control_visible", "panel" to "sizes", "control" to "brush_size", "visible" to true)))
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val firstTile = toolbar.array("tiles").objects().first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$firstTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag(capyTag()).assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag("tab-name-tool_settings", useUnmergedTree = true).assertTextEquals("Tool")
        compose.onNodeWithTag("tab-sizes").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("number-value-Brush size").fetchSemanticsNodes().isNotEmpty() }
        val valueNode = compose.onAllNodesWithTag("number-value-Brush size").onFirst()
        valueNode.assertHeightIsEqualTo(24.dp)
        val numericSlider = compose.onAllNodesWithTag("number-slider-Brush size").onFirst()
        numericSlider.assertHeightIsEqualTo(16.dp)
        val rangeBounds = numericSlider.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val valueBounds = valueNode.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val density = compose.activity.resources.displayMetrics.density
        assertEquals(rangeBounds.bottom, valueBounds.bottom, 1f)
        assertEquals(rangeBounds.right + 70f * density, valueBounds.right, 1f)
        assertTrue(valueBounds.left - rangeBounds.right >= 8f * density - 1f)
        numericSlider.performTouchInput { swipe(center, centerRight, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") > 1000.0 }
        numericSlider.performTouchInput { swipe(center, centerLeft, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") < 2.0 }
        val brush = state().getJSONObject("brush").getInt("preset")
        compose.onNodeWithTag("brush-preview-$brush", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        compose.onNodeWithTag("group-grip-${group("brushes").getInt("id")}", useUnmergedTree = true).assertWidthIsEqualTo(20.dp)
        compose.onNodeWithTag("tab-brushes").assertHeightIsEqualTo(36.dp)
        compose.runOnIdle { host.invoke("fit_canvas") }
        capture("25-editor-default-light")
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("26-editor-default-dark")
        openSettings()
        compose.waitForIdle()
        compose.onNodeWithTag("preferences-surface", useUnmergedTree = true).assertWidthIsEqualTo(compose.activity.resources.configuration.screenWidthDp.dp)
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        compose.onAllNodes(isPopup()).assertCountEquals(0)
        compose.onNodeWithTag("settings-done").performClick()
    }

    private fun numericContact(node: SemanticsNodeInteraction, tool: Int): (Int, Float) -> Unit {
        val bounds = node.fetchSemanticsNode().boundsInRoot
        val density = compose.activity.resources.displayMetrics.density
        val location = IntArray(2)
        instrumentation.runOnMainSync { compose.activity.window.decorView.getLocationOnScreen(location) }
        var down = SystemClock.uptimeMillis()
        return { phase, pixels ->
            if (phase == MotionEvent.ACTION_DOWN) down = SystemClock.uptimeMillis()
            val properties = MotionEvent.PointerProperties().apply { id = 0; toolType = tool }
            val point = MotionEvent.PointerCoords().apply { x = location[0] + bounds.center.x; y = location[1] + bounds.center.y - pixels * density; pressure = .7f }
            val source = when (tool) { MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE; MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS; else -> InputDevice.SOURCE_TOUCHSCREEN }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, 1, arrayOf(properties), arrayOf(point), 0, if (tool == MotionEvent.TOOL_TYPE_MOUSE) MotionEvent.BUTTON_PRIMARY else 0, 1f, 1f, 0, 0, source, 0)
            try {
                val delivered = instrumentation.uiAutomation.injectInputEvent(event, true)
                if (phase != MotionEvent.ACTION_CANCEL) assertTrue("tool=$tool phase=$phase point=${point.x},${point.y} bounds=$bounds", delivered)
            } finally { event.recycle() }
            SystemClock.sleep(40); compose.waitForIdle()
        }
    }

    @Test fun panelValuesScrubFineAndCancelAcrossContacts() {
        action(obj("type" to "customize", "action" to obj("type" to "set_control_visible", "panel" to "sizes", "control" to "brush_size", "visible" to true)))
        fun valueNode() = compose.onAllNodesWithTag("number-value-Brush size").onFirst()
        fun diameter() = state().getJSONObject("brush").getDouble("diameter")
        action(obj("type" to "reset_tool_setting", "id" to "size"))
        val defaultSize = diameter()
        val density = compose.activity.resources.displayMetrics.density
        val control = host.catalog.getJSONObject("panel_metrics").getDouble("control_height").toFloat()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            compose.onNodeWithTag("tab-sizes").performClick()
            val glyphs = JSONArray()
            for (size in listOf(999, 1000)) {
                action(obj("type" to "set_brush_size", "value" to size))
                waitState { diameter() == size.toDouble() }
                val textNode = compose.onAllNodes(hasText("$size px") and hasAnyAncestor(hasTestTag("number-value-Brush size")), useUnmergedTree = true).onFirst()
                val results = mutableListOf<TextLayoutResult>()
                textNode.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(results) }
                val layout = results.single()
                val bounds = textNode.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                glyphs.put(obj("value" to size, "text_bounds" to bounds.toString(), "paragraph_right" to (bounds.left + layout.getLineRight(0)), "last_glyph_right" to (bounds.left + layout.getBoundingBox(layout.layoutInput.text.length - 1).right), "font_size_sp" to layout.layoutInput.style.fontSize.value))
            }
            File(File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }, "number-glyphs-$theme.json").writeText(obj("density" to density, "font_scale" to compose.activity.resources.configuration.fontScale, "glyphs" to glyphs).toString())
            assertEquals("Numeric suffix endpoint stays fixed across digit counts", glyphs.getJSONObject(0).getDouble("last_glyph_right"), glyphs.getJSONObject(1).getDouble("last_glyph_right"), .01)
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                action(obj("type" to "set_brush_size", "value" to 42))
                val send = numericContact(valueNode(), tool)
                send(MotionEvent.ACTION_CANCEL, 0f)
                try {
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 47.0 }
                    send(MotionEvent.ACTION_MOVE, 4f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 43.0 }
                    valueNode().assertTextEquals("43 px")
                    send(MotionEvent.ACTION_CANCEL, 4f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 42.0 }
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f); send(MotionEvent.ACTION_UP, 20f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 47.0 }
                    valueNode().assertTextEquals("47 px")
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 52.0 }
                    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 47.0 }
                    send(MotionEvent.ACTION_UP, 20f)
                    val beforeSlider = compose.onAllNodesWithTag("number-slider-Brush size").onFirst().fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    val beforeValue = valueNode().fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    valueNode().performClick()
                    val field = compose.onAllNodesWithTag("number-Brush size").onFirst()
                    field.performTextReplacement("12345678901234567890 +")
                    val editor = field.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    val slider = compose.onAllNodesWithTag("number-slider-Brush size").onFirst().fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    assertEquals(68f * density, editor.width, 1f)
                    assertEquals(control * density, editor.height, 1f)
                    assertEquals(slider.bottom, editor.bottom, 1f)
                    assertEquals(beforeSlider, slider)
                    assertEquals(beforeValue.right + 6f * density, editor.right, 1f)
                    assertEquals(slider.right + 76f * density, editor.right, 1f)
                    File(File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }, "number-bounds-$theme-$tool.json").writeText(obj("density" to density, "readout" to beforeValue.toString(), "slider_before" to beforeSlider.toString(), "slider_editing" to slider.toString(), "editor" to editor.toString()).toString())
                    assertTrue(editor.left - slider.right >= 8f * density - 1f)
                    field.performTextReplacement("42.5"); field.performImeAction()
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 42.5 }
                    assertEquals(42.5, diameter(), 0.0)
                    val title = compose.onNode(hasText("Brush size") and hasAnyAncestor(hasTestTag("panel-body-sizes")), useUnmergedTree = true)
                    title.performTouchInput { doubleClick(androidx.compose.ui.geometry.Offset(24f * density, center.y)) }
                    waitState { it.getJSONObject("brush").getDouble("diameter") == defaultSize }
                    valueNode().performClick()
                    field.performTextReplacement("1200.5"); field.performImeAction()
                    waitState { diameter() == 1200.5 }
                    valueNode().assertTextEquals("1200.5 px")
                    val large = numericContact(valueNode(), tool)
                    try {
                        large(MotionEvent.ACTION_DOWN, 0f); large(MotionEvent.ACTION_MOVE, 20f)
                        waitState { diameter() == 1206.0 }
                        valueNode().assertTextEquals("1206 px")
                        large(MotionEvent.ACTION_MOVE, 4f)
                        waitState { diameter() == 1202.0 }
                        valueNode().assertTextEquals("1202 px")
                        large(MotionEvent.ACTION_CANCEL, 4f)
                        waitState { diameter() == 1200.5 }
                        valueNode().assertTextEquals("1200.5 px")
                        large(MotionEvent.ACTION_DOWN, 0f); large(MotionEvent.ACTION_MOVE, 20f)
                        waitState { diameter() == 1206.0 }
                        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
                        waitState { diameter() == 1200.5 }
                        valueNode().assertTextEquals("1200.5 px")
                        large(MotionEvent.ACTION_UP, 20f)
                        assertEquals(1200.5, diameter(), 0.0)
                        valueNode().assertTextEquals("1200.5 px")
                    } finally { large(MotionEvent.ACTION_CANCEL, 0f) }
                    action(obj("type" to "set_brush_size", "value" to 999.5))
                    val crossing = numericContact(valueNode(), tool)
                    try {
                        crossing(MotionEvent.ACTION_DOWN, 0f); crossing(MotionEvent.ACTION_MOVE, 20f)
                        waitState { diameter() == 1005.0 }
                        valueNode().assertTextEquals("1005 px")
                        crossing(MotionEvent.ACTION_MOVE, 0f)
                        waitState { diameter() == 999.5 }
                        valueNode().assertTextEquals("999.5 px")
                        crossing(MotionEvent.ACTION_MOVE, 20f); crossing(MotionEvent.ACTION_UP, 20f)
                        waitState { diameter() == 1005.0 }
                        valueNode().assertTextEquals("1005 px")
                    } finally { crossing(MotionEvent.ACTION_CANCEL, 0f) }
                } finally { send(MotionEvent.ACTION_CANCEL, 0f) }
            }
        }
    }

    @Test fun propertyValueScrubsHaveOneUndoAcrossContacts() {
        compose.onNodeWithTag("tab-properties").performClick()
        fun opacity() = state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key") == "opacity" }.getJSONObject("value").getDouble("value")
        val layer = state().getJSONObject("layer_properties").getLong("layer")
        val density = compose.activity.resources.displayMetrics.density
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                action(obj("type" to "effect", "action" to obj("op" to "number", "layer" to layer, "key" to "opacity", "operation" to obj("type" to "value", "value" to .6))))
                val node = compose.onAllNodesWithTag("number-value-opacity").onLast().performScrollTo()
                val send = numericContact(node, tool)
                val before = opacity()
                send(MotionEvent.ACTION_CANCEL, 0f)
                try {
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f); send(MotionEvent.ACTION_CANCEL, 20f)
                    waitState { opacity() == before }
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f); send(MotionEvent.ACTION_UP, 20f)
                    waitState { opacity() > before }
                    val after = opacity()
                    assertEquals(before + .05, after, .000001)
                    action(obj("type" to "invoke", "command" to "undo")); waitState { opacity() == before }
                    action(obj("type" to "invoke", "command" to "redo")); waitState { opacity() == after }
                } finally { send(MotionEvent.ACTION_CANCEL, 0f) }
            }
        }
    }


    @Test fun shortToolbarSlidersHideReadoutsWithoutChangingPreferences() {
        device.landscape(compose.activityRule.scenario)
        val fixture = JSONObject(defaultWorkspace)
        val layout = fixture.getJSONObject("layout")
        val panel = layout.array("panels").objects().first { it.getString("id") == "toolbar" }
        val panelId = panel.getString("id")
        val sliders = listOf("brush_size_slider", "brush_strength_slider").mapIndexed { index, kind ->
            obj("id" to index + 1, "control" to obj("kind" to kind), "slider_style" to obj("show_value" to true, "expand" to true))
        }
        panel.put("content", obj("kind" to "toolbar", "name" to JSONObject.NULL, "tiles" to JSONArray(sliders)))
        panel.put("tile_style", "small")
        for (key in listOf("bands", "collapsed", "column_stacks", "column_scroll", "fit_height_groups", "fit_tab_groups")) layout.put(key, JSONArray())
        val floating = obj("root" to tabs(2, panelId), "position" to JSONArray(listOf(30, 90)), "width" to 74,
            "height" to 300, "default_width" to JSONObject.NULL, "toolbar_layout" to "compact")
        layout.put("floating", JSONArray(listOf(floating)))
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (height in listOf(300, 100, 300)) {
                floating.put("height", height)
                action(obj("type" to "restore_workspace", "workspace" to fixture))
                for (tile in sliders) {
                    val id = tile.getInt("id")
                    val value = compose.onNodeWithTag("slider-value-$id", useUnmergedTree = true)
                    if (height == 100) value.assertDoesNotExist() else value.assertIsDisplayed()
                    compose.onNodeWithTag("component-slider-$id", useUnmergedTree = true).assertIsDisplayed()
                    val savedTile = state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects()
                        .first { it.getString("id") == panelId }.getJSONObject("content").array("tiles").objects().first { it.getInt("id") == id }
                    assertTrue("Available space preserves Show value", savedTile.getJSONObject("slider_style").getBoolean("show_value"))
                }
                capture("toolbar-slider-$theme-$height")
            }
        }
    }

    @Test fun compactNumberInputAndVerticalRibbonStayUsable() {
        action(obj("type" to "customize", "action" to obj("type" to "set_control_visible", "panel" to "sizes", "control" to "brush_size", "visible" to true)))
        // The full editor shows brush size in Tool Settings as well as Sizes.
        compose.onNodeWithTag("tab-sizes").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("number-value-Brush size").fetchSemanticsNodes().isNotEmpty() }
        fun sizeNode(matcher: SemanticsMatcher) = compose.onAllNodes(matcher).onFirst()
        sizeNode(hasTestTag("number-value-Brush size")).performClick()
        val field = sizeNode(hasTestTag("number-Brush size"))
        field.performTextReplacement("45/2")
        field.performImeAction()
        waitState { it.getJSONObject("brush").number("diameter") == 22.5f }
        sizeNode(hasTestTag("number-value-Brush size")).assertTextEquals("22.5 px")
        sizeNode(hasTestTag("number-value-Brush size")).performClick()
        field.performTextReplacement("23.5"); field.performImeAction()
        waitState { it.getJSONObject("brush").number("diameter") == 23.5f }
        sizeNode(hasTestTag("number-value-Brush size")).assertTextEquals("23.5 px")
        sizeNode(hasTestTag("number-value-Brush size")).performClick()
        field.performTextReplacement("22.5"); field.performImeAction()
        waitState { it.getJSONObject("brush").number("diameter") == 22.5f }

        action(obj("type" to "move_panel", "panel" to "toolbar", "target" to obj("kind" to "edge", "edge" to "top", "outer" to true), "viewport" to viewport()))
        fun toolbarGrip() = compose.onNode(hasContentDescription("Move toolbar") and hasAnyAncestor(hasTestTag("group-${group("toolbar").getInt("id")}")))
        val grip = toolbarGrip()
        val origin = grip.fetchSemanticsNode().boundsInRoot.topLeft
        val density = compose.activity.resources.displayMetrics.density
        grip.performTouchInput { swipe(center, androidx.compose.ui.geometry.Offset(density, 200 * density) - origin, 700) }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.getString("active") == "toolbar" && it.getString("axis") == "vertical"
        } }
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val lastTile = toolbar.array("tiles").objects().last().getInt("id")
        val tile = compose.onNodeWithTag("tile-toolbar-$lastTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        assertTrue("Vertical ribbon grip is below its tools", toolbarGrip().fetchSemanticsNode().boundsInRoot.top >= tile.fetchSemanticsNode().boundsInRoot.bottom)
        capture("31-vertical-ribbon")
    }

    @Test fun tabDragAppendsAndWholeGroupDragPreservesTabs() {
        fun drag(source: SemanticsNodeInteraction, target: androidx.compose.ui.geometry.Offset) {
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(center, target - origin, 700) }
        }
        val brushes = compose.onNodeWithTag("tab-brushes")
        val layers = compose.onNodeWithTag("tab-layers")
        drag(brushes, layers.fetchSemanticsNode().boundsInRoot.centerRight - androidx.compose.ui.geometry.Offset(2f, 0f))
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers"))
        } }
        val grip = compose.onNodeWithTag("group-grip-${group("brushes").getInt("id")}")
        // A whole-group drop onto Sizes must preserve both tab identities.
        drag(grip, compose.onNodeWithTag("tab-sizes").fetchSemanticsNode().boundsInRoot.center)
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
            it.array("panels").values().containsAll(listOf("brushes", "layers", "sizes"))
        } }
        assertNull(host.actionError)
        capture("13-tab-group-drag")
    }

    @Test fun shortcutSearchFindsModifiedKeysAndMarksChangedBindings() {
        openSettings()
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
        fun modified() = compose.onAllNodesWithTag("shortcut-reset-$id").fetchSemanticsNodes().isNotEmpty()
        assertFalse(modified())
        compose.onNodeWithTag("shortcut-$id").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        compose.onNodeWithContentDescription("Remove shortcut").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == 0 }
        action(obj("type" to "preferences", "action" to obj("type" to "close_shortcut_editor")))
        assertTrue("A changed shortcut offers its reset button", modified())
        compose.onNodeWithTag("shortcut-binding-$id", useUnmergedTree = true).assertTextEquals("Disabled")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            capture("shortcuts-modified-$theme")
        }
        preference("reset_shortcut")
        compose.waitForIdle()
        assertFalse(modified())
        compose.onNodeWithText("Done").performClick()
    }

    @Test fun shortcutPageRecordsMultipleBindingsAndPersists() {
        openSettings()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search or press a shortcut").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        // Instrumentation can run again against an already installed app.
        if (preferences().getJSONObject("shortcut_editor").getBoolean("modified")) {
            compose.onNodeWithTag("shortcut-editor-reset").performClick()
            compose.waitUntil(10_000) { !preferences().getJSONObject("shortcut_editor").getBoolean("modified") }
        }
        val original = preferences().getJSONObject("shortcut_editor").array("bindings").length()
        compose.onNodeWithText("Add Shortcut").performClick()
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
        compose.onNodeWithTag("confirm-shortcut").assertTextEquals("Reassign")
        compose.onNodeWithTag("cancel-shortcut").performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") == null }
        assertEquals(original, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithText("Add Shortcut").performScrollTo().performClick()
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
        compose.onNodeWithTag("confirm-shortcut").assertTextEquals("Add").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original + 1 }
        capture("14-shortcut-editor")
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.activityRule.scenario.recreate()
        compose.waitUntil(20_000) { host.snapshot!!.optBoolean("gpu_ready") }
        assertNull(host.failure)
        compose.waitUntil(20_000) { compose.activity.hasWindowFocus() && host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") && !it.optBoolean("switcher_busy") } == true }
        openSettings()
        compose.onNodeWithText("Keyboard Shortcuts").performClick()
        compose.onNodeWithText("Search or press a shortcut").performTextInput("Zen mode")
        compose.waitUntil(10_000) { preferences().array("shortcuts").objects().count { it.getBoolean("visible") } == 1 }
        compose.onNode(hasText("Zen mode") and !hasSetTextAction()).performScrollTo().performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("shortcut_editor") != null }
        assertEquals(original + 1, preferences().getJSONObject("shortcut_editor").array("bindings").length())
        compose.onNodeWithTag("shortcut-editor-reset").performClick()
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").length() == original }
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
    }

    @Test fun rotationSnappingAcrossThemesAndNativeContacts() {
        fun rotation() = state().getJSONObject("camera").getDouble("rotation") * 180 / Math.PI
        fun snapped() = state().getJSONObject("settings").getBoolean("snap_rotation")
        fun open() {
            val point = screenBounds(compose.onNodeWithTag("camera-readout")).center
            val down = SystemClock.uptimeMillis()
            for (phase in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, point.x, point.y, 0)
                event.source = InputDevice.SOURCE_TOUCHSCREEN
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
            }
            compose.waitUntil(10_000) { compose.onAllNodesWithText("Snap rotation").fetchSemanticsNodes().isNotEmpty() }
        }
        fun toggle() {
            val expected = !snapped()
            open()
            compose.onNodeWithText("Snap rotation").performScrollTo().performClick()
            compose.waitUntil(10_000) { compose.onAllNodesWithTag("zoom-menu").fetchSemanticsNodes().isEmpty() }
            waitState { it.getJSONObject("settings").getBoolean("snap_rotation") == expected }
        }
        val workspace = JSONObject(defaultWorkspace).apply { getJSONObject("layout").put("canvas_info", obj("visible" to true)) }
        action(obj("type" to "restore_workspace", "workspace" to workspace))
        assertTrue("Rotation snapping starts enabled", snapped())
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_rotation_snapping", "enabled" to true))
            open()
            compose.onNodeWithText("Snap rotation").performScrollTo().assertIsSelected()
            val image = instrumentation.uiAutomation.takeScreenshot()
            val directory = File(compose.activity.getExternalFilesDir(null), "rotation-snap").apply { mkdirs() }
            File(directory, "menu-$theme.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
            image.recycle()
            for (enabled in listOf(false, true)) {
                action(obj("type" to "set_rotation_snapping", "enabled" to enabled))
                compose.waitUntil(10_000) {
                    compose.onNodeWithText("Snap rotation").fetchSemanticsNode().config.getOrNull(SemanticsProperties.Selected) == enabled
                }
            }
            compose.onNodeWithText("Snap rotation").performClick()
            waitState { !it.getJSONObject("settings").getBoolean("snap_rotation") }
            toggle()
            assertTrue(snapped())
            for (enabled in listOf(true, false)) {
                if (snapped() != enabled) toggle()
                for (touch in listOf(true, false)) {
                    action(obj("type" to "set_rotation", "rotation" to 0))
                    action(obj("type" to "invoke", "command" to if (touch) "pen" else "rotate_view"))
                    if (!touch) assertEquals("rotate_view", state().getJSONObject("layer_tools").getString("tool"))
                    var width = 1f
                    var height = 1f
                    instrumentation.runOnMainSync {
                        val canvas = findCanvas(compose.activity.window.decorView)!!
                        width = canvas.width.toFloat(); height = canvas.height.toFloat()
                    }
                    val area = state().getJSONObject("camera").getJSONArray("work_area")
                    val center = androidx.compose.ui.geometry.Offset(
                        (area.getDouble(0) + area.getDouble(2) / 2).toFloat(),
                        (area.getDouble(1) + area.getDouble(3) / 2).toFloat())
                    val radius = minOf(area.getDouble(2), area.getDouble(3)).toFloat() * .4f
                    fun points(degrees: Double, scale: Float = 1f): List<androidx.compose.ui.geometry.Offset> {
                        val radians = degrees * Math.PI / 180
                        val offset = androidx.compose.ui.geometry.Offset(kotlin.math.cos(radians).toFloat() * radius * scale,
                            kotlin.math.sin(radians).toFloat() * radius * scale)
                        val positions = if (touch) listOf(center - offset, center + offset) else listOf(center + offset)
                        return positions.map { androidx.compose.ui.geometry.Offset(it.x / width, it.y / height) }
                    }
                    val tool = if (touch) MotionEvent.TOOL_TYPE_FINGER else MotionEvent.TOOL_TYPE_STYLUS
                    canvasEvent(MotionEvent.ACTION_DOWN, points(0.0).take(1), tool)
                    if (touch) canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(0.0), tool)
                    canvasEvent(MotionEvent.ACTION_MOVE, points(2.5), tool)
                    host.drain()
                    compose.waitUntil(10_000) { kotlin.math.abs(rotation() - if (enabled) 0.0 else 2.5) < .05 }
                    assertEquals("$theme touch=$touch enabled=$enabled holds small rotation", if (enabled) 0.0 else 2.5, rotation(), .05)
                    canvasEvent(MotionEvent.ACTION_MOVE, points(12.0), tool)
                    compose.waitUntil(10_000) { kotlin.math.abs(rotation() - 12.0) < .05 }
                    assertEquals("$theme touch=$touch enabled=$enabled escapes the snap", 12.0, rotation(), .05)
                    if (touch) canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(12.0), tool)
                    canvasEvent(MotionEvent.ACTION_UP, points(12.0).take(1), tool)
                    if (touch) {
                        action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "rotate_with_two_fingers", "value" to false)))
                        action(obj("type" to "set_rotation", "rotation" to .2))
                        val preserved = state().getJSONObject("camera").getDouble("rotation")
                        val zoom = state().getJSONObject("camera").getDouble("zoom")
                        canvasEvent(MotionEvent.ACTION_DOWN, points(0.0).take(1))
                        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(0.0))
                        canvasEvent(MotionEvent.ACTION_MOVE, points(12.0, .8f))
                        waitState { kotlin.math.abs(it.getJSONObject("camera").getDouble("zoom") - zoom) > .001 }
                        assertEquals("$theme enabled=$enabled pinching preserves the exact disabled rotation", preserved,
                            state().getJSONObject("camera").getDouble("rotation"), 0.0)
                        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points(12.0, .8f))
                        canvasEvent(MotionEvent.ACTION_UP, points(12.0, .8f).take(1))
                        action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "rotate_with_two_fingers", "value" to true)))
                    }
                    action(obj("type" to "set_rotation", "rotation" to 89 * Math.PI / 180))
                    assertEquals("Typed rotation stays exact", 89.0, rotation(), .001)
                }
            }
            toggle()
            assertTrue(snapped())
            toggle()
            compose.activityRule.scenario.recreate()
            host.awaitReady(compose = compose)
            assertFalse("$theme the disabled choice survives Activity recreation", snapped())
            toggle()
            assertTrue(snapped())
            assertNull(host.failure)
            assertNull(host.actionError)
        }
    }

    @Test fun cameraNavigationPublishesOnlyReadoutUpdates() {
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("shaders_ready") == true }
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
        assertTrue("Input arrays are reused, not allocated for every event",
            data.getLong("pointer_allocations") < data.array("inputs").length() / 2)
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

    @Test fun threeFingerDiagonalScrubClearsOnlyTheSelectedLayer() {
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun pixels(): ByteArray { settle(); return gradientOutput() }
        fun screenPixels(): IntArray {
            settle()
            val bitmap = instrumentation.uiAutomation.takeScreenshot()
            val left = bitmap.width * 2 / 5
            val top = bitmap.height * 2 / 5
            val width = bitmap.width / 5
            val height = bitmap.height / 5
            return IntArray(width * height).also {
                bitmap.getPixels(it, 0, width, left, top, width, height)
                bitmap.recycle()
            }
        }
        fun camera() = JSONObject(state().getJSONObject("camera").toString()).apply { remove("revision") }.toString()
        val contacts = listOf(
            androidx.compose.ui.geometry.Offset(.44f, .45f),
            androidx.compose.ui.geometry.Offset(.5f, .5f),
            androidx.compose.ui.geometry.Offset(.56f, .55f))
        fun begin() {
            canvasEvent(MotionEvent.ACTION_DOWN, contacts.take(1))
            for (index in 1..2) canvasEvent(MotionEvent.ACTION_POINTER_DOWN or
                (index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), contacts.take(index + 1))
        }
        fun end(offset: androidx.compose.ui.geometry.Offset) {
            val points = contacts.map { it + offset }
            for (index in 2 downTo 1) canvasEvent(MotionEvent.ACTION_POINTER_UP or
                (index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points.take(index + 1))
            canvasEvent(MotionEvent.ACTION_UP, points.take(1))
        }
        val diagonal = compose.runOnIdle {
            val canvas = findCanvas(compose.activity.window.decorView)!!
            val length = android.view.ViewConfiguration.get(canvas.context).scaledTouchSlop * 3f
            androidx.compose.ui.geometry.Offset(length * .6f / canvas.width, length * .8f / canvas.height)
        }
        for (theme in listOf("light", "dark")) {
            host.createDrawing(128, 128)
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            action(obj("type" to "set_theme", "theme" to theme))
            invoke("fit_canvas")
            val empty = pixels()
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 0, 1))))
            penStroke(8)
            val lower = pixels()
            assertFalse("The lower layer contains visible paint", empty.contentEquals(lower))
            canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(androidx.compose.ui.geometry.Offset(.7f, .3f)), MotionEvent.TOOL_TYPE_STYLUS)
            val lowerScreen = screenPixels()
            invoke("add_layer")
            action(obj("type" to "set_color", "rgba" to JSONArray(listOf(1, 0, 0, 1))))
            penStroke(8)
            val painted = pixels()
            assertFalse("The selected layer adds visible paint", lower.contentEquals(painted))
            canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(androidx.compose.ui.geometry.Offset(.7f, .3f)), MotionEvent.TOOL_TYPE_STYLUS)
            val paintedScreen = screenPixels()
            assertFalse("The screen shows the selected layer's paint", lowerScreen.contentEquals(paintedScreen))
            val before = camera()

            begin()
            for (scale in listOf(.1f, -.1f, .1f)) canvasEvent(MotionEvent.ACTION_MOVE, contacts.map { it + diagonal * scale })
            end(diagonal * .1f)
            assertArrayEquals("Three-finger jitter and release preserve paint", painted, pixels())
            begin(); end(androidx.compose.ui.geometry.Offset.Zero)
            assertArrayEquals("A three-finger tap does not clear paint", painted, pixels())

            val started = SystemClock.uptimeMillis()
            begin()
            for (scale in listOf(1f, 0f, 1f)) canvasEvent(MotionEvent.ACTION_MOVE, contacts.map { it + diagonal * scale })
            assertTrue("The scrub completes within two seconds", SystemClock.uptimeMillis() - started < 2_000)
            assertArrayEquals("The scrub clears only the selected layer before release", lowerScreen, screenPixels())
            assertEquals("The scrub preserves the camera", before, camera())
            invoke("undo")
            assertArrayEquals("One Undo restores the selected layer", paintedScreen, screenPixels())
            for (scale in listOf(0f, 1f, 0f, 1f)) canvasEvent(MotionEvent.ACTION_MOVE, contacts.map { it + diagonal * scale })
            assertArrayEquals("A contact group clears at most once", paintedScreen, screenPixels())
            end(diagonal)
            assertArrayEquals("Releasing the scrub does not issue a tap action", painted, pixels())
            assertEquals(before, camera())
            assertNull(host.failure)
            assertNull(host.actionError)
        }
    }

    @Test fun canvasQuickPinchAndHeldHistoryAcrossThemes() {
        fun pinch(outward: Boolean) {
            val from = if (outward) .05f else .1f
            val to = if (outward) .1f else .05f
            val a = androidx.compose.ui.geometry.Offset(.5f - from, .5f)
            val b = androidx.compose.ui.geometry.Offset(.5f + from, .5f)
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(a))
            canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(a, b))
            SystemClock.sleep(60)
            val end = listOf(androidx.compose.ui.geometry.Offset(.5f - to, .5f), androidx.compose.ui.geometry.Offset(.5f + to, .5f))
            canvasEvent(MotionEvent.ACTION_MOVE, end)
            canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), end)
            canvasEvent(MotionEvent.ACTION_UP, listOf(end[0]))
            settle()
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_zoom", "zoom" to 2))
            action(obj("type" to "set_rotation", "rotation" to .7))
            pinch(false)
            assertEquals(0f, state().getJSONObject("camera").number("rotation"), .001f)
            pinch(true)
            assertEquals(2f, state().getJSONObject("camera").number("zoom"), .001f)
            assertEquals(.7f, state().getJSONObject("camera").number("rotation"), .001f)
            val base = state().array("layers").length()
            repeat(3) { action(obj("type" to "invoke", "command" to "add_layer")) }
            for ((fingers, expected) in listOf(2 to base, 3 to base + 3)) {
                val points = (0 until fingers).map { androidx.compose.ui.geometry.Offset(.4f + .08f * it, .5f) }
                canvasEvent(MotionEvent.ACTION_DOWN, points.take(1))
                for (index in 1 until fingers) canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points.take(index + 1))
                SystemClock.sleep(1000)
                waitState { it.array("layers").length() == expected }
                for (index in fingers - 1 downTo 1) canvasEvent(MotionEvent.ACTION_POINTER_UP or (index shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), points.take(index + 1))
                canvasEvent(MotionEvent.ACTION_UP, points.take(1))
                settle()
                assertEquals(expected, state().array("layers").length())
            }
            repeat(3) { action(obj("type" to "invoke", "command" to "undo")) }
        }
    }

    @Test fun layerMultitouchActionsAcrossThemes() {
        fun layers() = state().array("layers").objects()
        val id = layers().first { it.getBoolean("editing") }.getLong("id")
        penStroke()
        fun row() = compose.onNodeWithTag("layer-row-$id")
        fun pair(dx: Float = 0f, hold: Boolean = false) {
            val center = screenBounds(row()).center
            val started = SystemClock.uptimeMillis()
            fun event(action: Int, count: Int, delta: Float = 0f) {
                val properties = (0 until count).map { index -> MotionEvent.PointerProperties().apply { this.id = index; toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray()
                val coordinates = (0 until count).map { index -> MotionEvent.PointerCoords().apply {
                    x = center.x + (if (index == 0) -25f else 25f) + delta; y = center.y; pressure = 1f; size = .2f
                } }.toTypedArray()
                val event = MotionEvent.obtain(started, SystemClock.uptimeMillis(), action, count, properties, coordinates, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
            }
            event(MotionEvent.ACTION_DOWN, 1)
            event(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2)
            SystemClock.sleep(if (hold) 700 else 60)
            if (hold) instrumentation.runOnMainSync {
                assertNull("Two held layer contacts must not open a competing bubble menu", findTag("workspace-menu"))
            }
            if (dx != 0f) for (step in 1..4) { event(MotionEvent.ACTION_MOVE, 2, dx * step / 4); SystemClock.sleep(16) }
            event(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, dx)
            event(MotionEvent.ACTION_UP, 1, dx)
            settle()
        }
        for (theme in listOf("light", "dark")) for (simple in listOf(false, true)) {
            val workspace = JSONObject(defaultWorkspace)
            workspace.getJSONObject("layout").put("header", obj("size" to "medium", "zones" to JSONArray().put(JSONArray()).put(JSONArray().put(obj(
                "id" to 2, "item" to obj("kind" to "tool", "control" to obj("kind" to "panel", "panel" to "layers"))
            ))).put(JSONArray()), "next_id" to 3))
            action(obj("type" to "restore_workspace", "workspace" to workspace))
            action(obj("type" to "set_theme", "theme" to theme))
            if (simple) {
                customize(obj("type" to "set_panel_visible", "panel" to "layers", "visible" to false))
                val bubbleViewport = viewport()
                compose.runOnIdle {
                    for ((phase, position) in listOf("down" to listOf(200, 60), "move" to listOf(500, 300), "up" to listOf(500, 300))) {
                        host.dispatch(obj("type" to "bubble", "action" to obj("type" to "drag", "grab" to obj("kind" to "header", "id" to 2),
                            "phase" to phase, "position" to JSONArray(position), "viewport" to bubbleViewport)))
                    }
                }
                settle()
            } else floatPanel("layers", 350f, 180f)
            pair()
            assertEquals("$theme/$simple: layer touch opens shared opacity; error=${host.actionError}", id, state().getJSONObject("layer_tools").optLong("opacity_popup", -1))
            compose.onNodeWithTag("layer-gesture-opacity").assertIsDisplayed()
            compose.onNode(SemanticsMatcher("numeric value") { it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("number-value-") == true } and hasAnyAncestor(hasTestTag("layer-gesture-opacity")), useUnmergedTree = true).performClick()
            compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("layer-gesture-opacity")), useUnmergedTree = true).performTextReplacement("42")
            compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("layer-gesture-opacity")), useUnmergedTree = true).performImeAction()
            waitState { it.array("layers").objects().first { l -> l.getLong("id") == id }.number("opacity") == .42f }
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
            action(obj("type" to "invoke", "command" to "undo"))
            assertEquals(1f, layers().first { it.getLong("id") == id }.number("opacity"), 0f)
            pair(hold = true)
            waitState { it.getJSONObject("layer_tools").getBoolean("has_selection") }
            compose.onAllNodesWithTag("workspace-menu").assertCountEquals(0)
            action(obj("type" to "invoke", "command" to "deselect"))
            val selected = layers().first { it.getLong("id") == id }.getBoolean("selected")
            pair(60f)
            assertEquals("$theme/$simple two-finger swipe leaves selection unchanged", selected, layers().first { it.getLong("id") == id }.getBoolean("selected"))
            assertFalse(layers().first { it.getLong("id") == id }.getBoolean("alpha_locked"))
            pair(60f)
            assertEquals(selected, layers().first { it.getLong("id") == id }.getBoolean("selected"))
            val swipeDistance = 80f * compose.activity.resources.displayMetrics.density
            row().performTouchInput { swipe(center, center + androidx.compose.ui.geometry.Offset(swipeDistance, 0f), 200) }
            host.awaitMain("$theme/$simple one-finger Alpha Lock", 5_000, { "error=${host.actionError}; row=${layers().first { it.getLong("id") == id }}" }, compose) {
                layers().first { it.getLong("id") == id }.getBoolean("alpha_locked")
            }
            action(obj("type" to "invoke", "command" to "undo"))
            repeat(2) { action(obj("type" to "invoke", "command" to "add_layer")) }
            val top = layers().first { it.getBoolean("editing") }.getLong("id")
            val a = bounds("layer-row-$top").center
            val b = bounds("layer-row-$id").center
            val list = bounds("layer-rows").topLeft
            val count = layers().size
            compose.onNodeWithTag("layer-rows").performTouchInput {
                down(0, a - list); down(1, b - list)
                moveTo(0, a + (b - a) * .35f - list); moveTo(1, b + (a - b) * .35f - list)
                up(1); up(0)
            }
            waitState { it.array("layers").length() == count - 2 }
            action(obj("type" to "invoke", "command" to "undo"))
            assertEquals(count, layers().size)
            repeat(2) { action(obj("type" to "invoke", "command" to "undo")) }
        }
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
            // The header's accessible label is distinct from the Preferences command tooltip.
            val settingsLabel = "Settings"
            // Configuration changes precede the resized Compose hierarchy.
            compose.waitUntil(10_000) { compose.onAllNodesWithContentDescription(settingsLabel).fetchSemanticsNodes().size == 1 }
            capture("16-workspace-portrait")
            compose.onNodeWithContentDescription(settingsLabel).performClick()
            capture("17-settings-portrait")
            compose.onNodeWithText("Pen & Input").performClick()
            compose.waitUntil(10_000) { preferences().getString("page") == "input" }
            compose.onNodeWithText("Prediction amount").assertExists()
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
        openSettings()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { preferences().getString("page") == "input" }
        capture("19-input-settings")
        val row = preferences().array("pages").objects().first { it.getString("id") == "input" }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getJSONObject("kind").getString("type") == "choice" }
        val kind = row.getJSONObject("kind")
        compose.onNodeWithTag("setting-choice-${row.getString("id")}").performScrollTo().performClick()
        capture("20-cursor-choices")
        val selection = (kind.getInt("selected") + 1) % kind.array("options").length()
        compose.onNodeWithTag("setting-choice-option-${row.getString("id")}-${selection}").performClick()
        compose.waitUntil(10_000) { preferences().array("pages").objects().first { it.getString("id") == "input" }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getString("id") == row.getString("id") }
            .getJSONObject("kind").getInt("selected") == selection }
        compose.onNodeWithTag("settings-content-page:input").assertIsDisplayed()
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
        openSettings()
        fun kind(page: String, id: String) = preferences().array("pages").objects().first { it.getString("id") == page }
            .array("groups").objects().flatMap { it.array("rows").objects() }.first { it.getString("id") == id }.getJSONObject("kind")
        // The same renderer handles ordinary choices and choices with previews.
        for ((page, id) in listOf("appearance" to "theme", "input" to "cursor")) {
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
        action(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 0, 1))))
        val pen = androidx.compose.ui.geometry.Offset(0.45f, 0.5f)
        val palm = androidx.compose.ui.geometry.Offset(0.6f, 0.6f)
        val hover = androidx.compose.ui.geometry.Offset(0.7f, 0.3f)
        val camera = jsonValue(state().getJSONObject("camera"))
        val tools = listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_MOVE, listOf(pen + androidx.compose.ui.geometry.Offset(0.1f, 0f), palm + androidx.compose.ui.geometry.Offset(0.05f, 0.05f)), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), listOf(pen, palm), MotionEvent.TOOL_TYPE_STYLUS, pointerTools = tools)
        canvasEvent(MotionEvent.ACTION_UP, listOf(pen), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.array("commands").objects().first { c -> c.getString("id") == "undo" }.getBoolean("enabled") }
        assertEquals("Palm contact must not move the camera", camera, jsonValue(state().getJSONObject("camera")))
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

    @Test fun zenKeepsFloatingPanelsThroughDrawerDismissalAndPanelDrag() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "restore_workspace", "workspace" to JSONObject(defaultWorkspace)))
            floatPanel("toolbar", 420f, 120f)
            floatPanel("brushes", 420f, 300f)
            floatPanel("layers", 740f, 300f)
            compose.onNodeWithTag(capyTag()).performClick()
            waitState { it.getJSONObject("workspace").getBoolean("zen_mode") }
            compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
            val tile = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects().first().getInt("id")
            compose.onNodeWithTag("tile-toolbar-$tile").performClick()
            waitState { it.getJSONObject("customization").objectOrNull("drawer") != null }
            compose.waitForIdle()
            capture("workspace-zen-before-drawer-dismiss-$theme")
            val center = compose.runOnIdle {
                val canvas = findCanvas(compose.activity.window.decorView)!!
                val chrome = host.glassBoxesForTest.map { androidx.compose.ui.geometry.Rect(it[0], it[1], it[0] + it[2], it[1] + it[3]) }
                listOf(.25f, .5f, .75f).flatMap { x -> listOf(.25f, .5f, .75f).map { y -> androidx.compose.ui.geometry.Offset(x, y) } }
                    .first { point -> chrome.none { it.contains(androidx.compose.ui.geometry.Offset(point.x * canvas.width, point.y * canvas.height)) } }
            }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { it.getJSONObject("customization").isNull("drawer") }
            assertFalse("First outside contact closes only the drawer", host.snapshot!!.getBoolean("chrome_hidden"))
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
            compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
            val workspace = compose.onNodeWithTag("workspace")
            val root = workspace.fetchSemanticsNode().boundsInRoot
            workspace.performMouseInput { moveTo(root.center - root.topLeft) }
            workspace.performMouseInput { moveTo(androidx.compose.ui.geometry.Offset(1f, root.height / 2)) }
            compose.waitForIdle()
            assertTrue("Canvas edge hover keeps docks hidden", host.snapshot!!.getBoolean("chrome_hidden"))
            val tab = compose.onNodeWithTag("tab-brushes").fetchSemanticsNode().boundsInRoot.center - root.topLeft
            workspace.performMouseInput { moveTo(tab) }
            compose.waitForIdle()
            assertTrue("Hovering a floating tab keeps docks hidden", host.snapshot!!.getBoolean("chrome_hidden"))
            capture("workspace-zen-mouse-on-panel-$theme")
            val source = compose.onNodeWithTag("tab-brushes")
            val target = compose.onNodeWithTag("tab-layers").fetchSemanticsNode().boundsInRoot.center
            val origin = source.fetchSemanticsNode().boundsInRoot.topLeft
            source.performTouchInput { swipe(this.center, target - origin, 700) }
            compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").array("groups").objects().any {
                it.array("panels").values().containsAll(listOf("brushes", "layers"))
            } }
            assertTrue("Dropping a floating panel keeps docks hidden", host.snapshot!!.getBoolean("chrome_hidden"))
            capture("24-zen-after-drag-$theme")
            compose.runOnIdle { host.invoke("zen_mode") }
        }
    }
}
