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
        val task = native { handle ->
            Native.dispatch(handle, obj("type" to "invoke", "command" to "new_document").toString())
            var published = JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
            var request = published.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
            if (request.getJSONObject("kind").getJSONObject("request").getString("type") == "confirm_close") {
                Native.documentClose(handle, request.getInt("id"), "\"discard\"")
                published = JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
                request = published.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
            }
            val file = published.getJSONObject("document_file")
            Native.projectTask(handle, request.getInt("id"), "null", file.getLong("epoch"), file.getLong("revision"))
        }
        try {
            Native.projectOptions(task, obj("extent" to JSONArray(listOf(128, 128)), "color" to obj("space" to "Srgb", "depth" to "F32"), "background" to "White").toString())
            Native.projectWork(task, -1, 128, 128)
            native { Native.projectAdopt(it, task, "null") }
        } finally { Native.projectFree(task) }
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
            val toolBefore=state().getJSONArray("tool_extra").toString()
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_ESCAPE)
            compose.waitUntil(5_000) {compose.onAllNodes(hasTestTag("gradient-editor") and hasAnyAncestor(isPopup())).fetchSemanticsNodes().isEmpty()}
            assertEquals(toolBefore,state().getJSONArray("tool_extra").toString())
            action(obj("type" to "invoke","command" to "fit_canvas"))
            val shapeNodes=compose.onAllNodes(SemanticsMatcher("Gradient shape tag") {it.config.getOrNull(SemanticsProperties.TestTag)?.contains("gradient-shape")==true},useUnmergedTree=true).fetchSemanticsNodes()
            File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-shape-$theme.json").writeText(obj("tool_extra" to state().getJSONArray("tool_extra"),"tags" to JSONArray(shapeNodes.map {node->obj("tag" to node.config.getOrNull(SemanticsProperties.TestTag),"bounds" to node.boundsInRoot.toString())})).toString())
            fun output(phase:String):ByteArray {
                val file=File(File(compose.activity.getExternalFilesDir(null),"validation").apply {mkdirs()},"p30-contact-$theme.json")
                fun published()=runBlocking {host.withNative {obj("snapshot" to (Native.snapshot(it)?.let(::JSONObject) ?: host.snapshot!!),"tone" to JSONObject(Native.toneStatus(it)))}}
                file.writeText(obj("phase" to phase,"before" to published()).toString())
                runBlocking {host.glassPresented()}
                try {
                    compose.waitUntil(5_000) {published().getJSONObject("tone").getBoolean("idle")}
                    file.writeText(obj("phase" to phase,"retired" to published()).toString())
                    return gradientOutput()
                } catch(error:Throwable) {
                    file.writeText(obj("phase" to phase,"failed" to published(),"error" to error.toString()).toString())
                    throw error
                }
            }
            for(index in 0..2) {
                panelNode("tool-segment-gradient-shape-$index").performClick()
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
                    androidx.compose.ui.geometry.Offset(source.number("x") + 10, source.number("y") + 10), PointerIcon.TYPE_GRAB)
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
                try { instrumentation.runOnMainSync { icon = native.onResolvePointerIcon(event, 0) } } finally { event.recycle() }
                return icon
            }
            runCatching { compose.waitUntil(5_000) { resolved() == expected } }
            assertEquals("Native cursor at $point for tool $tool", expected, resolved())
        }
        fun hover(point: androidx.compose.ui.geometry.Offset, type: Int, penType: Int? = if (type == PointerIcon.TYPE_GRAB || type == PointerIcon.TYPE_GRABBING) null else type) {
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
        for (tag in listOf("tab-brushes", "tab-sizes", "group-grip-41", "ribbon-grip-toolbar")) hover(bounds(tag).center, PointerIcon.TYPE_GRAB)
        val grip = bounds("group-grip-41")
        hover(androidx.compose.ui.geometry.Offset(grip.left - 8f, grip.center.y), PointerIcon.TYPE_GRAB)
        // Bare canvas hides the mouse and leaves the native pen icon unspecified.
        hover(away, PointerIcon.TYPE_NULL, null)
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
        val before = saved()
        val start = bounds("tab-sizes").center
        root.performMouseInput { moveTo(start); press(); moveTo(away, 300) }; settle()
        compose.waitUntil(10_000) { group("sizes").getBoolean("floating") }
        icon(away, PointerIcon.TYPE_GRABBING)
        icon(away, null, MotionEvent.TOOL_TYPE_STYLUS)
        // The native SurfaceView must also keep the active cursor across canvas.
        compose.runOnIdle { assertEquals(PointerIcon.getSystemIcon(native.context, PointerIcon.TYPE_GRABBING), findCanvas(compose.activity.window.decorView)!!.pointerIcon) }
        root.performMouseInput { release() }; settle()
        icon(away, PointerIcon.TYPE_GRAB)
        compose.runOnIdle { assertEquals(PointerIcon.getSystemIcon(native.context, PointerIcon.TYPE_NULL), findCanvas(compose.activity.window.decorView)!!.pointerIcon) }
        val floated = saved()
        val floatingId = group("sizes").getInt("id")
        for ((edge, type) in listOf("left" to PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW, "right" to PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW,
            "top" to PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW, "bottom" to PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW,
            "top_left" to PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW, "bottom_right" to PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW,
            "top_right" to PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW, "bottom_left" to PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW)) {
            val resize = bounds("resize-$floatingId-$edge").center
            hover(resize, type)
            val end = resize + androidx.compose.ui.geometry.Offset(40f, 40f)
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
        for (tag in listOf("drawer-tab-brushes", "drawer-tab-sizes", "column-drawer-grip-41")) hover(bounds(tag).center, PointerIcon.TYPE_GRAB)
        val drawerGrip = bounds("column-drawer-grip-41")
        hover(androidx.compose.ui.geometry.Offset(drawerGrip.left - 8f, drawerGrip.center.y), PointerIcon.TYPE_GRAB)
        val collapsed = saved()
        root.performMouseInput { moveTo(bounds("column-grip-41").center); press(); moveTo(away, 250) }; settle()
        icon(away, PointerIcon.TYPE_GRABBING)
        val edge = androidx.compose.ui.geometry.Offset(root.fetchSemanticsNode().boundsInRoot.width - 2f, away.y)
        root.performMouseInput { moveTo(edge, 250) }; settle(); icon(edge, PointerIcon.TYPE_GRABBING)
        root.performMouseInput { cancel() }; settle(); assertEquals(collapsed, saved())
        compose.runOnIdle { assertEquals(PointerIcon.getSystemIcon(native.context, PointerIcon.TYPE_NULL), findCanvas(compose.activity.window.decorView)!!.pointerIcon) }
        hover(bounds("column-grip-41").center, PointerIcon.TYPE_GRAB)
        hover(away, PointerIcon.TYPE_NULL, null)
        assertNull(host.actionError)
    }

    @Test fun panelHeadersDoNotHighlightOnMouseOrStylusHoverOrPress() {
        val fixture = JSONObject(defaultWorkspace)
        fixture.getJSONObject("layout").apply {
            put("bands", JSONArray(listOf(
                obj("id" to 40, "edge" to "left", "extent" to 252, "root" to tabs(41, "brushes", "sizes", "tool_settings")),
                obj("id" to 42, "edge" to "right", "extent" to 252, "root" to tabs(43, "layers", "properties", "adjustments")))))
            put("floating", JSONArray()); put("collapsed", JSONArray()); put("column_scroll", JSONArray()); put("fit_tab_groups", JSONArray()); put("fit_height_groups", JSONArray()); put("column_stacks", JSONArray())
            put("next_id", maxOf(44, getInt("next_id")))
        }
        fixture.put("zen_mode", false)
        val root = compose.onNodeWithTag("workspace")
        fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot.translate(-root.fetchSemanticsNode().boundsInRoot.topLeft)
        fun settle() { compose.waitForIdle(); SystemClock.sleep(180); compose.waitForIdle() }
        fun hover(point: androidx.compose.ui.geometry.Offset, tool: Int, eventAction: Int = MotionEvent.ACTION_HOVER_MOVE) {
            if (tool == MotionEvent.TOOL_TYPE_MOUSE) {
                root.performMouseInput {
                    when (eventAction) {
                        MotionEvent.ACTION_HOVER_EXIT -> exit()
                        MotionEvent.ACTION_DOWN -> press()
                        MotionEvent.ACTION_CANCEL -> { cancel(); moveTo(point) }
                        else -> moveTo(point)
                    }
                }
            } else {
                val window = root.fetchSemanticsNode().positionInWindow + point
                instrumentation.runOnMainSync {
                    val coords = MotionEvent.PointerCoords().apply { x = window.x; y = window.y }
                    val props = MotionEvent.PointerProperties().apply { id = 0; toolType = tool }
                    val time = SystemClock.uptimeMillis()
                    val event = MotionEvent.obtain(time, time, eventAction, 1, arrayOf(props), arrayOf(coords), 0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
                    try {
                        if (eventAction == MotionEvent.ACTION_DOWN || eventAction == MotionEvent.ACTION_CANCEL)
                            compose.activity.window.decorView.dispatchTouchEvent(event)
                        else compose.activity.window.decorView.dispatchGenericMotionEvent(event)
                    }
                    finally { event.recycle() }
                }
            }
            settle()
        }
        for (theme in listOf("light", "dark")) for (placement in listOf("docked", "floating", "drawer")) {
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
            val points = listOf("active" to bounds("${tab}brushes").center, "inactive" to bounds("${tab}sizes").center,
                "empty" to androidx.compose.ui.geometry.Offset(grip.left - 8f, grip.center.y), "grip" to grip.center)
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
                hover(away, tool, MotionEvent.ACTION_HOVER_ENTER)
                // Prove each native hover stream reaches Compose, and that the
                // suppression stays scoped to panel headers, not other controls.
                val menu = compose.onNodeWithTag("application-menu-file")
                val menuBefore = menu.captureToImage().toPixelMap()
                hover(bounds("application-menu-file").center, tool)
                val menuHovered = menu.captureToImage().toPixelMap()
                assertTrue("$tool hover still highlights ordinary controls", (0 until menuBefore.height).any { y ->
                    (0 until menuBefore.width).any { x -> menuBefore[x, y] != menuHovered[x, y] }
                })
                hover(away, tool)
                val baseline = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                for ((part, point) in points) {
                    hover(point, tool)
                    val actual = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                    var changed = 0
                    for (y in 0 until baseline.height) for (x in 0 until baseline.width) {
                        if (baseline[x, y] != actual[x, y]) changed++
                    }
                    if (changed > 0 || (tool == MotionEvent.TOOL_TYPE_MOUSE && part == "active")) {
                        capture("panel-header-hover-$theme-$placement-$tool-$part")
                    }
                    assertEquals("$theme $placement $tool $part hover must leave header pixels unchanged", 0, changed)
                    hover(point, tool, MotionEvent.ACTION_DOWN)
                    try {
                        val pressed = compose.onNodeWithTag(header).captureToImage().toPixelMap()
                        var pressChanges = 0
                        for (y in 0 until baseline.height) for (x in 0 until baseline.width) {
                            if (baseline[x, y] != pressed[x, y]) pressChanges++
                        }
                        if (pressChanges > 0) capture("panel-header-press-$theme-$placement-$tool-$part")
                        assertEquals("$theme $placement $tool $part press must leave header pixels unchanged", 0, pressChanges)
                    } finally { hover(point, tool, MotionEvent.ACTION_CANCEL) }
                }
                hover(away, tool, MotionEvent.ACTION_HOVER_EXIT)
            }
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
        val task = native { handle ->
            Native.dispatch(handle, obj("type" to "invoke", "command" to "new_document").toString())
            var published = JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
            var request = published.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
            if (request.getJSONObject("kind").getJSONObject("request").getString("type") == "confirm_close") {
                Native.documentClose(handle, request.getInt("id"), "\"discard\"")
                published = JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
                request = published.array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
            }
            val file = published.getJSONObject("document_file")
            Native.projectTask(handle, request.getInt("id"), "null", file.getLong("epoch"), file.getLong("revision"))
        }
        try {
            Native.projectOptions(task, obj("extent" to JSONArray(listOf(128, 128)), "color" to obj("space" to "Srgb", "depth" to "F32"), "background" to "White").toString())
            Native.projectWork(task, -1, 128, 128)
            native { Native.projectAdopt(it, task, "null") }
        } finally { Native.projectFree(task) }
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
            waitState { it.getJSONObject("color_picker").getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals("Stylus Down does not change the curve", beforePoints, points())
            assertEquals(beforePickRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals(beforePoints, points())
            assertEquals(beforePickRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_UP, listOf(white), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") && points() != beforePoints }
            val calibrated = JSONArray(points())
            assertEquals("Gray calibration uses the original white input endpoint", 2, calibrated.length())
            assertEquals(1.0, calibrated.getJSONArray(1).getDouble(0), 0.0)
            val picked = archive("gray-release")
            assertEquals(sourceBacking, backing(picked))
            invoke("undo"); assertEquals("One release has one Undo", savedEffect(beforePick), savedEffect(archive("gray-undo")))
            invoke("redo"); assertEquals(savedEffect(picked), savedEffect(archive("gray-redo")))
            invoke("undo")
            propertyAction("calibrate", "gray")
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            waitState { !it.getJSONObject("color_picker").isNull("preview") }
            val preview = state().getJSONObject("color_picker").getJSONObject("preview").getJSONArray("rgba")
            for (component in 0..2) assertEquals("Held touch previews the original white input", 1.0, preview.getDouble(component), 1e-5)
            assertEquals("A held preview does not edit the curve", beforePoints, points())
            canvasEvent(MotionEvent.ACTION_CANCEL, listOf(white))
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_UP, listOf(white))
            settle()
            assertEquals("A canceled loupe and its late Up do not edit", savedEffect(beforePick), savedEffect(archive("gray-touch-cancel")))
            val targetBefore = archive("target-before")
            propertyAction("target_curve")
            waitState { it.getJSONObject("color_picker").getBoolean("calibrating") }
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(white))
            SystemClock.sleep(550)
            val moved = white + androidx.compose.ui.geometry.Offset(0f, .04f)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(moved))
            waitState { points() != beforePoints }
            assertEquals("The targeted source is the white input endpoint", 2, JSONArray(points()).length())
            canvasEvent(MotionEvent.ACTION_UP, listOf(moved))
            settle()
            assertTrue("Targeted mode stays armed after Up", state().getJSONObject("color_picker").getBoolean("calibrating"))
            propertyAction("target_curve")
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") }
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
            assertTrue("Contact cancellation retains the armed targeted tool", state().getJSONObject("color_picker").getBoolean("calibrating"))
            propertyAction("target_curve")
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") }
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
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") && points() == beforePoints }
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
            waitState { it.getJSONObject("color_picker").getBoolean("calibrating") }
            val balancePoint = samplePoint()
            canvasEvent(MotionEvent.ACTION_DOWN, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            canvasEvent(MotionEvent.ACTION_MOVE, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            assertEquals(balanceRevision, state().getJSONObject("document_file").getLong("revision"))
            canvasEvent(MotionEvent.ACTION_UP, listOf(balancePoint), MotionEvent.TOOL_TYPE_STYLUS)
            waitState { !it.getJSONObject("color_picker").getBoolean("calibrating") }
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
        compose.waitUntil(20_000) { host.filterPreviewCache.images[choices.first()] != null }
        compose.onNodeWithTag("filter-preview-${choices.first()}", useUnmergedTree=true).assertHeightIsEqualTo(40.dp)
        val preview = host.filterPreviewCache.images.getValue(choices.first()).image.toPixelMap()
        assertTrue("GPU preview has opaque artwork", (0 until preview.width).any { preview[it,preview.height/2].alpha>.5f })
        assertEquals("Silhouette has transparent corners", 0f, preview[0,0].alpha, .01f)
        capture("adjustments-picker")
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to "distort")))
        compose.onNodeWithTag("filter-search-toggle").performClick()
        compose.waitUntil(10_000) { compose.onAllNodes(hasSetTextAction() and hasAnyAncestor(hasTestTag("filter-search"))).fetchSemanticsNodes().isNotEmpty() }
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
    private fun capyTag() = "header-control-" + host.snapshot!!.getJSONObject("header").array("items").objects()
        .first { it.getString("label") == "Capy (Zen Mode)" }.get("id")
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
            assertEquals("Automatic names fill tabs from the left: $names", names.sortedDescending(), names)
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

    private fun withZenEdgeReveal(test: () -> Unit) {
        val saved = state().getJSONObject("settings").getBoolean("zen_reveal_at_edges")
        fun reveal(value: Boolean) = action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "zen_reveal_at_edges", "value" to value)))
        reveal(true)
        try { test() } finally { reveal(saved) }
    }
    @Test fun zenFloatingDragOnlyMergesFloatsUntilOccupiedEdgeRevealsDocks() = withZenEdgeReveal {
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

    @Test fun zenMouseCanMoveFromCanvasOntoRevealedPanel() = withZenEdgeReveal {
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
                compose.onNodeWithTag("zen-button").assertDoesNotExist()
                compose.onNodeWithTag("group-$floating").assertIsDisplayed()
                edit("zen_show_capy", true)
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("zen-button").fetchSemanticsNodes().isNotEmpty() }
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
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            open()
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
            compose.waitUntil(10_000) { host.brushPreviews.get("1-$theme.png") != null }
            val preview = host.brushPreviews.get("1-$theme.png")
            compose.onNodeWithTag("brush-preview-1", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
            action(obj("type" to "select_brush", "id" to 42))
            waitState { it.getJSONObject("brush").getInt("preset") == 42 }
            action(obj("type" to "select_brush", "id" to 1))
            waitState { it.getJSONObject("brush").getInt("preset") == 1 }
            assertSame(preview, host.brushPreviews.get("1-$theme.png"))
            compose.onNodeWithTag("brush-preview-1", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        }
        assertNotSame(host.brushPreviews.get("1-light.png"), host.brushPreviews.get("1-dark.png"))
    }

    @Test fun editorGeometryStaysConsistent() {
        action(obj("type" to "customize", "action" to obj("type" to "set_control_visible", "panel" to "sizes", "control" to "brush_size", "visible" to true)))
        val toolbar = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
        val firstTile = toolbar.array("tiles").objects().first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$firstTile").assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag(capyTag()).assertWidthIsEqualTo(36.dp).assertHeightIsEqualTo(36.dp)
        compose.onNodeWithTag("tab-name-tool_settings", useUnmergedTree = true).assertTextEquals("Tool")
        compose.onNodeWithTag("tab-sizes").performClick()
        val valueNode = compose.onAllNodesWithTag("number-value-Brush size").onFirst()
        valueNode.assertHeightIsEqualTo(36.dp)
        val numericSlider = compose.onAllNodesWithTag("number-slider-Brush size").onFirst()
        numericSlider.assertHeightIsEqualTo(16.dp)
        val rangeBounds = numericSlider.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val valueBounds = valueNode.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
        val density = compose.activity.resources.displayMetrics.density
        assertTrue(valueBounds.width <= 80f * density + 1f)
        assertTrue(valueBounds.left - rangeBounds.right >= 8f * density - 1f)
        numericSlider.performTouchInput { swipe(center, centerRight, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") > 1000.0 }
        numericSlider.performTouchInput { swipe(center, centerLeft, 300) }
        waitState { it.getJSONObject("brush").getDouble("diameter") < 2.0 }
        val brush = state().getJSONObject("brush").getInt("preset")
        compose.onNodeWithTag("brush-preview-$brush", useUnmergedTree = true).assertHeightIsEqualTo(40.dp)
        compose.onAllNodesWithContentDescription("Move panel group").onFirst().assertWidthIsEqualTo(20.dp)
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
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            compose.onNodeWithTag("tab-sizes").performClick()
            for (tool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                action(obj("type" to "set_brush_size", "value" to 42))
                val send = numericContact(valueNode(), tool)
                send(MotionEvent.ACTION_CANCEL, 0f)
                try {
                    send(MotionEvent.ACTION_DOWN, 0f); send(MotionEvent.ACTION_MOVE, 20f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 47.0 }
                    send(MotionEvent.ACTION_MOVE, 4f)
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 43.0 }
                    valueNode().assertTextEquals("43.0 px")
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
                    valueNode().performClick()
                    val field = compose.onAllNodesWithTag("number-Brush size").onFirst()
                    field.performTextReplacement("12345678901234567890 +")
                    val editor = field.fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    val slider = compose.onAllNodesWithTag("number-slider-Brush size").onFirst().fetchSemanticsNode().layoutInfo.coordinates.boundsInRoot()
                    assertTrue(editor.width <= 80f * density + 1f)
                    assertTrue(editor.left - slider.right >= 8f * density - 1f)
                    field.performTextReplacement("42.5"); field.performImeAction()
                    waitState { it.getJSONObject("brush").getDouble("diameter") == 42.5 }
                    assertEquals(42.5, diameter(), 0.0)
                    val valueTop = valueNode().fetchSemanticsNode().boundsInRoot.top
                    val title = compose.onNode(hasText("Brush size") and SemanticsMatcher("numeric row label") { kotlin.math.abs(it.boundsInRoot.top - valueTop) < 4f * density })
                    title.performTouchInput { doubleClick(androidx.compose.ui.geometry.Offset(24f * density, center.y)) }
                    waitState { it.getJSONObject("brush").getDouble("diameter") == defaultSize }
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

    @Test fun zenKeepsChromeThroughDrawerDismissalAndPanelDrag() = withZenEdgeReveal {
        compose.onNodeWithTag(capyTag()).performClick()
        waitState { it.getJSONObject("workspace").getBoolean("zen_mode") }
        val edge = androidx.compose.ui.geometry.Offset(0.01f, 0.5f)
        val center = androidx.compose.ui.geometry.Offset(0.7f, 0.6f)
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        val tile = host.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects().first().getInt("id")
        compose.onNodeWithTag("tile-toolbar-$tile").performClick()
        waitState { it.getJSONObject("customization").objectOrNull("drawer") != null }
        compose.waitForIdle()
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        waitState { it.getJSONObject("customization").isNull("drawer") }
        assertFalse("First outside contact closes only the drawer", host.snapshot!!.getBoolean("chrome_hidden"))
        canvasEvent(MotionEvent.ACTION_DOWN, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        canvasEvent(MotionEvent.ACTION_UP, listOf(center), MotionEvent.TOOL_TYPE_STYLUS)
        compose.waitUntil(10_000) { host.snapshot!!.getBoolean("chrome_hidden") }
        canvasEvent(MotionEvent.ACTION_HOVER_MOVE, listOf(edge), MotionEvent.TOOL_TYPE_MOUSE)
        compose.waitUntil(10_000) { !host.snapshot!!.getBoolean("chrome_hidden") }
        val source = compose.onNodeWithTag("tab-brushes")
        val target = compose.onNodeWithTag("tab-layers").fetchSemanticsNode().boundsInRoot.center
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
