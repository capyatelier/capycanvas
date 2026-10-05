package art.capycanvas

import android.os.ParcelFileDescriptor
import android.os.SystemClock
import androidx.compose.ui.test.*
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.lifecycle.viewModelScope
import android.view.WindowManager
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import kotlin.math.withSign
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest

/** Real JNI/file workers and Vulkan. Test files stay in this app's private cache. */
internal fun srgbLinear(vararg linear: Double, alpha: Double = 1.0) = obj("space" to "Srgb", "rgba" to org.json.JSONArray(linear.map { v ->
    val a = kotlin.math.abs(v); (if (a <= .0031308) 12.92 * a else 1.055 * Math.pow(a, 1 / 2.4) - .055).withSign(v) } + alpha))

class AndroidRasterTest {
    @get:Rule(order = 0) val device = CapyDeviceRule(nativeFileJobs = true)
    @get:Rule(order = 1) val compose = createEmptyComposeRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private val host get() = activity.host
    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        if (InstrumentationRegistry.getArguments().getString("presentationNarrow") == "true") {
            device.portrait(scenario)
            assertTrue(activity.resources.configuration.screenWidthDp <= 640)
            host.narrowPhotoPanels(compose)
        } else if (InstrumentationRegistry.getArguments().getString("presentationWide") == "true") {
            device.landscape(scenario)
            scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            assertTrue(activity.resources.configuration.screenWidthDp > 640)
        }
        compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true || host.failure!=null}
        assertNull(host.failure)
        compose.waitUntil(60_000) {host.workspaceManager?.optBoolean("ready")==true || host.workspaceManager?.isNull("error")==false}
        assertTrue("Workspace startup: ${host.workspaceManager}",host.workspaceManager?.optBoolean("ready")==true)
        compose.waitUntil(60_000) {host.workspaceManager?.optBoolean("busy")==false}
        // Slower devices can finish the selected brush before document commands
        // are enabled. Wait for the same admission state as the visible Open UI.
        compose.waitUntil(120_000) {host.failure!=null || native{state(it).array("commands").objects().any{c->c.optString("id")=="open_document"&&c.optBoolean("enabled")}}}
        assertNull(host.failure)
        InstrumentationRegistry.getArguments().getString("theme")?.let { theme ->
            require(theme == "light" || theme == "dark") { "theme must be light or dark" }
            native { Native.dispatch(it, obj("type" to "set_theme", "theme" to theme).toString()) }
            compose.runOnUiThread { host.documentChanged() }
            compose.waitUntil(30_000) { host.snapshot?.optJSONObject("state")?.optString("theme") == theme }
            assertEquals(theme, native { state(it).getString("theme") })
        }
    }
    @Before fun isolatedWindow() {
        wakeDevice()
        // A driver crash can strand the previous run's synthetic contact in
        // InputDispatcher. Cancel that injected device before opening a picker.
        for (source in listOf(android.view.InputDevice.SOURCE_STYLUS, android.view.InputDevice.SOURCE_TOUCHSCREEN, android.view.InputDevice.SOURCE_MOUSE)) {
            val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 7; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS })
            val coords = arrayOf(android.view.MotionEvent.PointerCoords())
            val now = SystemClock.uptimeMillis()
            val event = android.view.MotionEvent.obtain(now, now, android.view.MotionEvent.ACTION_CANCEL, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, source, 0)
            try { instrumentation.uiAutomation.injectInputEvent(event, true) } finally { event.recycle() }
        }
        launch()
    }
    @After fun closeWindow() {
        if (::scenario.isInitialized) scenario.close()
    }
    private fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    private fun builtinRecipe(index: Int): JSONObject {
        val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
        return runBlocking { ColorPreferencesStore.presets(activity, color, obj("type" to "get", "index" to index)) }.getJSONObject("recipe")
    }
    private val files get() = activity.cacheDir
    private fun tick() = native { val now=System.nanoTime(); Native.frame(it,now,now+16_666_667) }
    private fun refresh() { tick(); compose.runOnUiThread { host.documentChanged() }; compose.waitForIdle() }
    private fun send(value: JSONObject) { native { Native.dispatch(it, value.toString()) }; compose.waitUntil(30_000) { !tick() } }
    private fun invoke(id: String) = send(obj("type" to "invoke", "command" to id))
    private fun action(value: JSONObject) { native { Native.dispatch(it, value.toString()) }; scenario.onActivity { host.documentChanged() }; tick(); compose.waitForIdle() }
    private fun action(command: String) { compose.runOnUiThread { host.invoke(command) }; compose.waitForIdle() }
    private fun tabs(handle: Long) = JSONObject(Native.documentTabs(handle, obj("op" to "view").toString()))
    private fun tabs() = native { tabs(it) }
    private fun selectedSessionCapture() = native { Native.sessionCapture(it,tabs(it).getLong("selected")) }
    private fun ids() = tabs().array("tabs").objects().map { it.getLong("id") }
    private val sourceVisible = JSONObject.quote("Visible")
    private fun histogram(): JSONObject {
        val control = Native.captureControl()
        try {
            val task = native { Native.inspectionTask(it, control) }
            return JSONObject(Native.inspectionStatistics(task, sourceVisible, false, false, false)).getJSONObject("histogram")
        }
        finally { Native.captureFree(control) }
    }
    @Test fun diagnosticsSampleInOpenColumns() {
        fun action(value: JSONObject) {
            val done = java.util.concurrent.CountDownLatch(1)
            compose.runOnIdle { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
            assertTrue(done.await(10, java.util.concurrent.TimeUnit.SECONDS))
            compose.waitForIdle()
            assertNull(host.actionError)
        }
        fun customize(value: JSONObject) = action(obj("type" to "customize", "action" to value))
        customize(obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true))
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { "stats" in it.array("panels").values() }.getInt("id")
        customize(obj("type" to "set_column_collapsed", "group" to group, "collapsed" to true))
        val column = host.snapshot!!.getJSONObject("layout").array("collapsed").objects()
            .first { c -> c.array("groups").objects().any { it.getInt("group") == group } }.getInt("id")
        customize(obj("type" to "set_column_drawers", "column" to column, "drawers" to false))
        customize(obj("type" to "toggle_column_drawer", "group" to group, "panel" to "stats"))
        compose.onNodeWithTag("renderer-stats").assertIsDisplayed()
        repeat(8) {
            action(obj("type" to "set_layer_opacity", "opacity" to (.8 + it*.02)))
            SystemClock.sleep(30)
        }
        compose.waitUntil(10_000) {
            val stats = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
            stats.getJSONArray("samples").length() > 0 &&
                stats.array("rows").objects().filter { it.getString("label") in listOf("CPU · ms", "GPU · ms") }
                    .all { it.getString("value") !in listOf("—", "Unavailable") }
        }
        assertNull(host.failure)
    }
    private fun state(handle: Long): JSONObject {
        // Explicit invalidation through a harmless host presentation action.
        Native.dispatch(handle,obj("type" to "close_settings").toString())
        return JSONObject(Native.snapshot(handle)!!).getJSONObject("state")
    }
    private fun pressCanvasBar(command: String, timeout: Long = 30_000) {
        fun state() = host.snapshot?.getJSONObject("state")
        compose.waitUntil(timeout) { state()?.array("commands")?.objects()?.any { it.getString("id") == command && it.getBoolean("enabled") } == true }
        compose.waitUntil(timeout) { compose.onAllNodesWithTag("canvas-action-bar").fetchSemanticsNodes().isNotEmpty() }
        val tag = "canvas-bar-action-$command"
        if (compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()) compose.onNodeWithTag(tag).assertIsDisplayed().performClick()
        else {
            val label = state()!!.array("commands").objects().first { it.getString("id") == command }.getString("label")
            val entry = hasText(label) and !hasTestTag("tool-action-$command")
            compose.onNodeWithTag("canvas-bar-more").performClick()
            compose.waitUntil(10_000) { compose.onAllNodes(entry).fetchSemanticsNodes().isNotEmpty() }
            compose.onNode(entry).performClick()
        }
        compose.waitForIdle(); tick()
    }
    private fun request(handle: Long, command: String): Pair<Int,JSONObject> {
        try { Native.dispatch(handle,obj("type" to "invoke","command" to command).toString()) }
        catch(e: Exception) { throw AssertionError("$command: ${state(handle).getJSONObject("document_file")}; ${host.workspaceManager}",e) }
        var state=state(handle)
        var request=state.array("requests").objects().first { it.getJSONObject("kind").optString("type")=="document" }
        if(request.getJSONObject("kind").getJSONObject("request").getString("type")=="confirm_close") {
            Native.documentClose(handle,request.getInt("id"),"\"discard\"")
            state=state(handle)
            request=state.array("requests").objects().first { it.getJSONObject("kind").optString("type")=="document" }
        }
        return request.getInt("id") to state.getJSONObject("document_file")
    }
    private fun point(phase: Int, dx: Double, dy: Double) {
        native { handle ->
        val viewport=host.snapshot!!.getJSONObject("state").getJSONObject("camera").getJSONArray("viewport")
        val bytes=doubleArrayOf(viewport.getDouble(0)*.50+dx,viewport.getDouble(1)*.5+dy,.65,0.0,0.0,0.0,0.0,System.nanoTime().toDouble(),phase.toDouble())
        Native.pointer(handle,71,0,0,bytes,bytes.size,false,false)
        val now=System.nanoTime(); Native.frame(handle,now,now+16_666_667)
    }
        // Direct JNI input bypasses CanvasHost.wake(). Honor frame()'s retry
        // contract before capturing the completed stroke on a nonblocking surface.
        if (phase == 3) {
            try { compose.waitUntil(10_000) { !tick() } }
            catch (failure: Throwable) {
                val diagnostics=runCatching { native { handle ->
                    val display=JSONObject(Native.displayStatus(handle))
                    val state=state(handle)
                    obj("display" to display,"document_file" to state.optJSONObject("document_file"),"layer_tools" to state.optJSONObject("layer_tools"),"layers" to state.optJSONArray("layers"))
                } }.getOrElse { obj("inspection_error" to it.toString()) }
                throw AssertionError("Completed stroke kept requesting frames: $diagnostics",failure)
            }
        }
    }
    private fun stroke(dy: Double) {
        point(1,0.0,dy)
        for(i in 1..6) {SystemClock.sleep(10);point(2,i*15.0,dy)}
        point(3,90.0,dy);tick()
    }
    private fun captureSession(directory: File) {
        val capture = selectedSessionCapture()
        assertNotEquals("Session capture ready",0L,capture)
        val store = Native.sessionStoreOpen(directory.absolutePath)
        try {Native.sessionCommit(capture,store)} finally {Native.sessionFree(capture);Native.sessionStoreFree(store)}
    }
    private fun restoreSessionTask(directory: File, id: Long = 1): Long {
        val task = native {Native.sessionRestoreTask(it)}
        val store = Native.sessionStoreOpen(directory.absolutePath)
        try {
            val location=Native.sessionRead(task,store,true)
            val observed=host.documents.observeDestination(location.takeUnless {it=="null"}?.let {JSONObject(it).getString("uri")},task)
            Native.sessionPrepare(task,id,observed)
        } catch(e:Exception) {Native.sessionFree(task);throw e} finally {Native.sessionStoreFree(store)}
        return task
    }
    private fun adoptSession(task: Long, id: Long = 1) {
        compose.waitUntil(120_000) {tick();native {JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}}
        val result = native {JSONObject(Native.sessionAdopt(it,task,id,"[]"))}
        val selected = result.getJSONObject("ids").getLong(id.toString())
        if(result.getBoolean("preserved"))runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.drawingTabs.selectRestored(selected)}}
        compose.runOnUiThread {host.documentChanged()}
    }
    private fun saveTask(name: String): Pair<Long,Int> = native { handle ->
        val (id,file)=request(handle,"save_document_as")
        Native.projectTask(handle,id,obj("uri" to android.net.Uri.fromFile(File(files,name)).toString(),"name" to name).toString(),file.getLong("epoch"),file.getLong("revision")) to id
    }
    private fun finishSave(job: Pair<Long,Int>, name: String): ByteArray {
        val file=File(files,name)
        try {
            Native.projectWork(job.first,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0)
            val fingerprint = Native.sessionFingerprint(ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY).detachFd())
            val location = obj("uri" to android.net.Uri.fromFile(file).toString(),"name" to name)
            native {Native.sessionCompleteSave(it,job.second,location.toString(),fingerprint)}
            return file.readBytes()
        } finally {Native.projectFree(job.first)}
    }
    private fun save(name: String)=finishSave(saveTask(name),name)
    private fun adoptProject(task: Long) = runBlocking {
        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
            host.drawingTabs.beforeAdopt(task)
            try { host.withNative { Native.projectAdopt(it,task,"null") } }
            finally { host.drawingTabs.afterAdopt() }
        }
    }
    private fun open(file: File, corrupt: Boolean=false, input: (() -> ParcelFileDescriptor)?=null) {
        val job=native {handle -> val (id,state)=request(handle,"open_document")
            Native.projectTask(handle,id,"null",state.getLong("epoch"),state.getLong("revision")) to id }
        val incumbent=native {state(it).getJSONObject("document_file")}
        try {
            val prepared=try {
                Native.projectWork(job.first,(input?.invoke() ?: ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY)).detachFd(),0,0)
                true
            } catch(e: Exception) {
                if(!corrupt)throw e
                native {Native.documentComplete(it,job.second,false,JSONObject.quote(e.message ?: "Corrupt file"))}
                false
            }
            if(prepared) {
                val packageSummary=Native.projectPackagePrompt(job.first)
                if(corrupt) {
                    assertNotEquals("Corrupt native file requires a package view","null",packageSummary)
                    val summary=JSONObject(packageSummary);val capabilities=summary.getJSONObject("capabilities")
                    assertEquals("failed",summary.getString("disposition"));assertFalse(capabilities.getBoolean("edit"));assertFalse(capabilities.getBoolean("save"));assertFalse(capabilities.getBoolean("view"));assertFalse(capabilities.getBoolean("export"));assertTrue(capabilities.getBoolean("copy_original"))
                    assertEquals(0,Native.projectPackagePreview(job.first).size)
                    var presented=false;var dialog:Job?=null
                    compose.runOnUiThread {dialog=host.viewModelScope.launch {presented=host.documents.showPackage(job.first,android.net.Uri.fromFile(file))}}
                    compose.waitUntil(10_000){host.documents.packagePrompt!=null}
                    compose.onNodeWithTag("preserved-package-preview").assertIsDisplayed()
                    assertTrue(native{state(it).getJSONObject("document_file").getBoolean("busy")})
                    compose.onNode(hasText(summary.getString("close")) and hasAnyAncestor(hasTestTag("preserved-package-preview"))).performClick()
                    compose.waitUntil(10_000){dialog?.isCompleted==true&&host.documents.packagePrompt==null};assertTrue(presented)
                    native {Native.documentComplete(it,job.second,false,"null")}
                    val current=native{state(it).getJSONObject("document_file")};assertFalse(current.getBoolean("busy"));assertEquals(incumbent.getLong("epoch"),current.getLong("epoch"));assertEquals(incumbent.getLong("revision"),current.getLong("revision"))
                } else {
                    assertEquals("Editable input cannot be a package view","null",packageSummary)
                    adoptProject(job.first)
                }
            }
        } finally {Native.projectFree(job.first)}
        tick()
    }
    private fun png(name: String, recipe: JSONObject? = null): ByteArray {
        val id=native {request(it,"export_document").first}
        var task=0L
        val deadline=SystemClock.uptimeMillis()+60_000
        while(task==0L && SystemClock.uptimeMillis()<deadline) {
            task=native {Native.projectExportTask(it,id,System.nanoTime())}
            if(task==0L) {tick();SystemClock.sleep(10)}
        }
        assertNotEquals("Export became ready",0L,task)
        val file=File(files,name)
        try {
            if (recipe != null) Native.projectExportOptions(task, recipe.toString())
            Native.projectWork(task,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0)
            native {Native.documentComplete(it,id,true,"null")}
            return file.readBytes()
        } catch(e:Exception){native{Native.documentComplete(it,id,false,"null")};throw e} finally {Native.projectFree(task)}
    }
    private fun manifest(bytes: ByteArray): JSONObject = packageManifest(bytes)
    private fun sourceIdentity(manifest: JSONObject): String = manifest.originalIdentity()
    private fun hash(bytes: ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()

    @Test fun exportAgainRetainsDestinationRecipeAndDrawingOwnership() {
        fun idle() {compose.waitUntil(120_000) {!host.documents.working&&!host.documentInputBlocked&&!host.drawingTabs.switching&&!native {state(it).getJSONObject("document_file").getBoolean("busy")}};assertNull(host.failure)}
        fun document()=native {state(it).getJSONObject("document_file")}.apply {remove("busy");remove("close_ready");remove("export_uri")}.toString()
        fun repeatUri()=native {state(it).getJSONObject("document_file").optString("export_uri")}
        fun pixels(bytes:ByteArray):IntArray {
            val bitmap=android.graphics.BitmapFactory.decodeByteArray(bytes,0,bytes.size,android.graphics.BitmapFactory.Options().apply {inPremultiplied=false})!!
            try {return IntArray(bitmap.width*bitmap.height).also {bitmap.getPixels(it,0,bitmap.width,0,0,bitmap.width,bitmap.height)}} finally {bitmap.recycle()}
        }
        fun fresh() {
            DocumentController.nativeFileJobsForTest=true
            val task=native {h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
            try {Native.projectWork(task,-1,128,128);compose.waitUntil(60_000) {tick();native {Native.projectParkReady(it,task)}};native {Native.projectAdopt(it,task,"null")}} finally {Native.projectFree(task)}
            refresh();idle();invoke("fit_canvas");invoke("pen")
        }
        fun paint(y:Double) {
            val camera=native {state(it).getJSONObject("camera")};val zoom=camera.getDouble("zoom");val pan=camera.getJSONArray("translation");val viewport=camera.getJSONArray("viewport")
            assertEquals(0.0,camera.getDouble("rotation"),0.0)
            val x=32*zoom+pan.getDouble(0)-viewport.getDouble(0)*.5;val dy=y*zoom+pan.getDouble(1)-viewport.getDouble(1)*.5
            point(1,x,dy);for(i in 1..6) {SystemClock.sleep(10);point(2,x+i*64*zoom/6,dy)};point(3,x+64*zoom,dy);refresh()
        }
        fun menu(command:String):Long {
            val current=native {state(it)}
            File(activity.getExternalFilesDir(null),"export-again-state-${current.getString("theme")}.json").writeText(obj("command" to command,"core" to current,"host" to host.snapshot,"blocked" to host.documentInputBlocked,"tone" to native {JSONObject(Native.toneStatus(it))}).toString())
            val label=current.array("commands").objects().first {c->c.getString("id")==command}.getString("label")
            if(compose.onAllNodesWithTag("application-menu-file").fetchSemanticsNodes().isNotEmpty())compose.onNodeWithTag("application-menu-file").performClick()
            else {compose.onNodeWithTag("header-menu-labels-compact").performClick();compose.onNode(hasText("File") and hasAnyAncestor(isPopup())).performClick()}
            val entry=compose.onNode(hasText(label) and hasAnyAncestor(isPopup())).assertIsDisplayed().assertIsEnabled()
            if(command=="export_again") {
                val capture=instrumentation.uiAutomation.takeScreenshot();val theme=native {state(it).getString("theme")}
                File(activity.getExternalFilesDir(null),"export-again-menu-$theme.png").outputStream().use {capture.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};capture.recycle()
            }
            val started=SystemClock.uptimeMillis();entry.performClick();return started
        }
        val uris=mutableListOf<android.net.Uri>()
        fun destination(name:String)=activity.contentResolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,android.content.ContentValues().apply {
            put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME,"capy-again-${System.nanoTime()}-$name.png");put(android.provider.MediaStore.MediaColumns.MIME_TYPE,"image/png")
        })!!.also {uris.add(it)}
        fun bytes(uri:android.net.Uri)=activity.contentResolver.openInputStream(uri)!!.use {it.readBytes()}
        val result=java.util.concurrent.atomic.AtomicReference<android.net.Uri?>()
        val choices=java.util.concurrent.atomic.AtomicInteger()
        val intent=java.util.concurrent.atomic.AtomicReference<android.content.Intent>()
        val monitor=object:android.app.Instrumentation.ActivityMonitor() {
            override fun onStartActivity(value:android.content.Intent):android.app.Instrumentation.ActivityResult? {
                if(value.action!=android.content.Intent.ACTION_CREATE_DOCUMENT)return null
                intent.set(android.content.Intent(value));choices.incrementAndGet()
                val uri=result.get()
                return android.app.Instrumentation.ActivityResult(if(uri==null)android.app.Activity.RESULT_CANCELED else android.app.Activity.RESULT_OK,android.content.Intent().setData(uri).addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION))
            }
        }
        instrumentation.addMonitor(monitor)
        try {for(theme in listOf("light","dark")) {
            fresh();action(obj("type" to "set_theme","theme" to theme));paint(32.0)
            val masterName="again-master-$theme.capy";val master=save(masterName);val masterFile=File(files,masterName)
            val recipe=builtinRecipe(0).put("format","Png").put("depth","U8")
            val color=native {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
            val preset=runBlocking {ColorPreferencesStore.presets(activity,color,obj("type" to "save","name" to "Repeat PNG","recipe" to recipe))}.getInt("index")
            try {
                val uri=destination(theme);result.set(uri);val count=choices.get();val clean=document()
                DocumentController.nativeFileJobsForTest=false;menu("export_document")
                compose.waitUntil(30_000) {compose.onAllNodesWithTag("color-choice-Destination").fetchSemanticsNodes().isNotEmpty()}
                compose.onNodeWithTag("color-choice-Destination").performScrollTo().performClick();compose.onNode(hasText("Repeat PNG") and hasAnyAncestor(isPopup())).performClick()
                val ordinaryStarted=SystemClock.uptimeMillis();compose.onNodeWithTag("export-choose-file").performClick()
                compose.waitUntil(120_000) {choices.get()==count+1&&runCatching {bytes(uri).isNotEmpty()}.getOrDefault(false)};idle();val ordinaryMs=SystemClock.uptimeMillis()-ordinaryStarted
                assertEquals("image/png",intent.get().type);assertEquals(uri.toString(),repeatUri());assertEquals(clean,document());assertArrayEquals(master,masterFile.readBytes())
                val first=bytes(uri)
                val repeatStarted=menu("export_again");idle();val repeatMs=SystemClock.uptimeMillis()-repeatStarted
                assertEquals(count+1,choices.get());assertArrayEquals(first,bytes(uri));assertEquals(clean,document())
                File(activity.getExternalFilesDir(null),"export-again-timing-$theme.json").writeText(obj("model" to android.os.Build.MODEL,"width_dp" to activity.resources.configuration.screenWidthDp,"extent" to org.json.JSONArray(listOf(128,128)),"ordinary_ms" to ordinaryMs,"repeat_ms" to repeatMs,"same_pixels" to true).toString())
                result.set(null);menu("export_document");compose.onNodeWithTag("export-choose-file").performClick()
                compose.waitUntil(30_000) {choices.get()==count+2};idle();assertArrayEquals(first,bytes(uri));assertEquals(uri.toString(),repeatUri())
                DocumentController.nativeFileJobsForTest=true;paint(96.0);val expected=pixels(png("again-reference-$theme.png",recipe));refresh();idle();val dirty=document();assertEquals(uri.toString(),repeatUri())
                assertTrue(JSONObject(dirty).getBoolean("modified"));assertFalse(pixels(first).contentEquals(expected))
                DocumentController.nativeFileJobsForTest=false;menu("export_again")
                compose.waitUntil(120_000) {!bytes(uri).contentEquals(first)};idle()
                assertEquals(count+2,choices.get());assertTrue(compose.onAllNodesWithTag("export-choose-file").fetchSemanticsNodes().isEmpty())
                assertArrayEquals(expected,pixels(bytes(uri)));assertEquals(dirty,document());assertArrayEquals(master,masterFile.readBytes());assertNull(host.actionError)
                activity.contentResolver.delete(uri,null,null);result.set(null);menu("export_again")
                compose.waitUntil(30_000) {choices.get()==count+3};idle()
                assertTrue(intent.get().getStringExtra(android.content.Intent.EXTRA_TITLE)!!.endsWith(".png"));assertEquals(dirty,document())
                val bad=File(files,"unwritable-$theme.png").apply {mkdir()};result.set(android.net.Uri.fromFile(bad));menu("export_again")
                compose.waitUntil(30_000) {host.hostError!=null};idle();assertEquals(dirty,document());assertArrayEquals(master,masterFile.readBytes())
                compose.onNodeWithText(host.bootstrap!!.getJSONObject("common").getString("ok")).performClick();assertEquals(uri.toString(),repeatUri());bad.delete()
                val replacement=destination("recovered-$theme");result.set(replacement);menu("export_again")
                compose.waitUntil(120_000) {runCatching {bytes(replacement).isNotEmpty()}.getOrDefault(false)};idle();assertEquals(replacement.toString(),repeatUri());assertArrayEquals(expected,pixels(bytes(replacement)));assertEquals(dirty,document())
                val owner=tabs().getLong("selected");fresh()
                assertFalse(native {state(it).array("commands").objects().first {c->c.getString("id")=="export_again"}.getBoolean("enabled")})
                val task=native {Native.documentSwitch(it,owner,false)}
                if(task!=0L)try {Native.documentResumeWork(task);native {Native.documentResume(it,task)}} finally {Native.documentResumeFree(task)}
                refresh();idle();assertEquals(replacement.toString(),repeatUri());assertTrue(native {state(it).array("commands").objects().first {c->c.getString("id")=="export_again"}.getBoolean("enabled")})
                DocumentController.nativeFileJobsForTest=true;assertTrue(JSONObject(document()).getBoolean("modified"));assertArrayEquals(expected,pixels(png("again-owner-$theme.png",recipe)))
                assertArrayEquals(master,masterFile.readBytes());assertNull(host.actionError)
                val capture=instrumentation.uiAutomation.takeScreenshot()
                File(activity.getExternalFilesDir(null),"export-again-$theme.png").outputStream().use {capture.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};capture.recycle()
            } finally {runBlocking {ColorPreferencesStore.presets(activity,color,obj("type" to "remove","index" to preset))}}
        }} finally {instrumentation.removeMonitor(monitor);DocumentController.nativeFileJobsForTest=true;uris.forEach {activity.contentResolver.delete(it,null,null)}}
    }

    @Test fun webpExportThroughTheDialogDecodes() {
        fun idle(){compose.waitUntil(120_000){!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}};assertNull(host.failure);assertNull(host.actionError)}
        fun choice(label:String,text:String){
            compose.waitUntil(30_000){compose.onAllNodes(hasTestTag("color-choice-$label") and isEnabled()).fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("color-choice-$label").performScrollTo().performClick()
            compose.onAllNodesWithText(text).onLast().performClick();compose.waitForIdle()
        }
        fun pixels(bytes:ByteArray):Triple<Int,Int,IntArray> {
            val bitmap=android.graphics.BitmapFactory.decodeByteArray(bytes,0,bytes.size,android.graphics.BitmapFactory.Options().apply{inPremultiplied=false})!!
            try { return Triple(bitmap.width,bitmap.height,IntArray(bitmap.width*bitmap.height).also{bitmap.getPixels(it,0,bitmap.width,0,0,bitmap.width,bitmap.height)}) }
            finally { bitmap.recycle() }
        }
        invoke("fit_canvas");invoke("pen")
        stroke(-40.0);stroke(40.0)
        val color=native{JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
        val base=builtinRecipe(0)
        val webp=native{JSONObject(Native.query(it,obj("type" to "export_draft","recipe" to base,"action" to obj("type" to "format","value" to "Webp")).toString())).getJSONObject("recipe")}
        assertEquals("U8",webp.getString("depth"))
        val poster=JSONObject(webp.toString()).put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(20000,20000)),"enlarge" to true)))
        val saved=runBlocking{ColorPreferencesStore.presets(activity,color,obj("type" to "save","name" to "Poster WebP","recipe" to poster))}.getInt("index")
        val target=File(activity.getExternalFilesDir(null),"zoom-webp-${System.nanoTime()}.webp")
        val picked=java.util.concurrent.atomic.AtomicReference<android.content.Intent>()
        val monitor=object:android.app.Instrumentation.ActivityMonitor() {
            override fun onStartActivity(intent:android.content.Intent):android.app.Instrumentation.ActivityResult? {
                if(intent.action!=android.content.Intent.ACTION_CREATE_DOCUMENT)return null
                picked.set(android.content.Intent(intent))
                return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK,android.content.Intent().setData(android.net.Uri.fromFile(target)))
            }
        }
        instrumentation.addMonitor(monitor)
        try {
            DocumentController.nativeFileJobsForTest=false
            compose.runOnUiThread{host.invoke("export_document")}
            choice("Destination","Poster WebP")
            compose.onNodeWithTag("color-choice-Format").assertTextEquals("WebP · lossless")
            compose.onNodeWithTag("export-choose-file").performClick()
            compose.waitUntil(30_000){compose.onAllNodesWithText("WebP export is limited to 16,384 pixels per side",substring=true).fetchSemanticsNodes().isNotEmpty()}
            assertNull("The size refusal comes before the file picker",picked.get())
            compose.onNodeWithText("Cancel").performClick();idle()

            compose.runOnUiThread{host.invoke("export_document")}
            choice("Format","WebP · lossless")
            compose.onNodeWithTag("color-choice-Bit depth").performScrollTo().performClick()
            assertTrue("WebP is 8-bit only",compose.onAllNodesWithText("16-bit").fetchSemanticsNodes().isEmpty())
            compose.onAllNodesWithText("8-bit").onLast().performClick();compose.waitForIdle()
            compose.onNodeWithTag("export-choose-file").performClick()
            compose.waitUntil(120_000){picked.get()!=null&&target.length()>0&&!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}}
            assertEquals("image/webp",picked.get().type)
            assertTrue(picked.get().getStringExtra(android.content.Intent.EXTRA_TITLE)!!.endsWith(".webp"))
            assertEquals("image/webp",host.documents.exportMime())
            assertNull(host.actionError)
            DocumentController.nativeFileJobsForTest=true
            val bytes=target.readBytes()
            assertEquals("RIFF",bytes.copyOfRange(0,4).decodeToString());assertEquals("WEBP",bytes.copyOfRange(8,12).decodeToString())
            assertTrue("Lossless VP8L",bytes.decodeToString(throwOnInvalidSequence=false).contains("VP8L"))
            val extent=native{state(it)}.array("tabs").objects().first().let{it.getInt("width") to it.getInt("height")}
            val (width,height,decoded)=pixels(bytes)
            assertEquals(extent,width to height)
            val (pngWidth,pngHeight,reference)=pixels(png("webp-reference.png",JSONObject(webp.toString()).put("format","Png")))
            assertEquals(width to height,pngWidth to pngHeight)
            var differences=0;var ink=0
            for(i in decoded.indices){
                if((0 until 4).any{c->kotlin.math.abs(((decoded[i] shr (c*8)) and 255)-((reference[i] shr (c*8)) and 255))>1})differences++
                if(decoded[i]!=decoded[0])ink++
            }
            assertEquals("The WebP matches the 8-bit PNG delivery",0,differences)
            assertTrue("The strokes are in the WebP: $ink",ink>50)
            println("PASS WebP export: ${bytes.size} bytes, ${width}×$height, image/webp, refused beyond 16,384 px before the picker")
        } finally {
            instrumentation.removeMonitor(monitor)
            DocumentController.nativeFileJobsForTest=true
            target.delete()
            runBlocking{ColorPreferencesStore.presets(activity,color,obj("type" to "remove","index" to saved))}
        }
    }

    @Test fun photoMetadataExportKeepsCameraLensAndCopyrightWithoutLocation() {
        fun idle(){compose.waitUntil(120_000){!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}};assertNull(host.failure);assertNull(host.actionError)}
        fun choice(label:String,text:String){
            compose.waitUntil(30_000){compose.onAllNodes(hasTestTag("color-choice-$label") and isEnabled()).fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("color-choice-$label").performScrollTo().performClick()
            compose.onAllNodesWithText(text).onLast().performClick();compose.waitForIdle()
        }
        val camera=File(files,"camera-${System.nanoTime()}.jpg")
        val bitmap=android.graphics.Bitmap.createBitmap(160,120,android.graphics.Bitmap.Config.ARGB_8888).apply{eraseColor(android.graphics.Color.rgb(40,90,160))}
        camera.outputStream().use{bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG,92,it)};bitmap.recycle()
        fun text(value:String)=value.toByteArray()+byteArrayOf(0)
        fun u16(n:Int)=byteArrayOf((n and 255).toByte(),(n shr 8).toByte())
        fun u32(n:Int)=byteArrayOf((n and 255).toByte(),((n shr 8) and 255).toByte(),((n shr 16) and 255).toByte(),(n ushr 24).toByte())
        val directories=listOf(
            mutableListOf(Triple(0x010f,2,text("Capycam")),Triple(0x0110,2,text("C-1")),Triple(0x013b,2,text("Ada Painter")),Triple(0x8298,2,text("(c) 2026 Ada Painter"))),
            mutableListOf(Triple(0x829a,5,u32(1)+u32(250)),Triple(0x9003,2,text("2026:09:01 10:00:00")),Triple(0xa434,2,text("Capy 35mm F1.8"))),
            mutableListOf(Triple(0x0001,2,text("N")),Triple(0x0002,5,u32(38)+u32(1)+u32(42)+u32(1)+u32(30)+u32(1)),Triple(0x0012,2,text("WGS-84"))))
        fun size(entries:List<Triple<Int,Int,ByteArray>>)=6+12*entries.size+entries.filter{it.third.size>4}.sumOf{it.third.size+it.third.size%2}
        val exifAt=8+size(directories[0])+24
        directories[0].add(Triple(0x8769,4,u32(exifAt)));directories[0].add(Triple(0x8825,4,u32(exifAt+size(directories[1]))))
        val tiff=java.io.ByteArrayOutputStream().apply{write(byteArrayOf(0x49,0x49,0x2a,0,8,0,0,0))}
        for(entries in directories){
            var dataAt=tiff.size()+6+12*entries.size;val data=java.io.ByteArrayOutputStream()
            tiff.write(u16(entries.size))
            for((tag,kind,value) in entries){
                val width=when(kind){5->8;4->4;else->1}
                tiff.write(u16(tag));tiff.write(u16(kind));tiff.write(u32(value.size/width))
                if(value.size<=4)tiff.write(value.copyOf(4)) else {tiff.write(u32(dataAt));data.write(value);if(value.size%2==1)data.write(0);dataAt+=value.size+value.size%2}
            }
            tiff.write(u32(0));tiff.write(data.toByteArray())
        }
        fun segment(payload:ByteArray)=byteArrayOf(0xff.toByte(),0xe1.toByte(),((payload.size+2) shr 8).toByte(),((payload.size+2) and 255).toByte())+payload
        val xmp="<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\" photoshop:City=\"Lisbon\"><dc:creator><rdf:Seq><rdf:li>Ada Painter</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"
        val jpeg=camera.readBytes()
        camera.writeBytes(jpeg.copyOfRange(0,2)+segment("Exif".toByteArray()+byteArrayOf(0,0)+tiff.toByteArray())+segment(text("http://ns.adobe.com/xap/1.0/")+xmp.toByteArray())+jpeg.copyOfRange(2,jpeg.size))
        fun contains(bytes:ByteArray,text:String)=bytes.decodeToString(throwOnInvalidSequence=false).contains(text)
        assertTrue("The source photo carries a location",contains(camera.readBytes(),"WGS-84")&&contains(camera.readBytes(),"Lisbon"))
        open(camera)
        assertTrue("The opened photo keeps its metadata",native{JSONObject(Native.query(it,obj("type" to "export_form").toString()))}.getBoolean("metadata"))
        val picked=java.util.concurrent.atomic.AtomicReference<File>()
        val monitor=object:android.app.Instrumentation.ActivityMonitor() {
            override fun onStartActivity(intent:android.content.Intent):android.app.Instrumentation.ActivityResult? {
                if(intent.action!=android.content.Intent.ACTION_CREATE_DOCUMENT)return null
                val target=File(activity.getExternalFilesDir(null),"metadata-${System.nanoTime()}-${intent.getStringExtra(android.content.Intent.EXTRA_TITLE)}")
                picked.set(target)
                return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK,android.content.Intent().setData(android.net.Uri.fromFile(target)))
            }
        }
        instrumentation.addMonitor(monitor)
        try {
            DocumentController.nativeFileJobsForTest=false
            for((format,label) in listOf("jpg" to "JPEG","webp" to "WebP · lossless")) {
                picked.set(null)
                compose.runOnUiThread{host.invoke("export_document")}
                fun metadata(kept:String,location:Boolean){
                    compose.waitUntil(30_000){compose.onAllNodes(hasTestTag("color-choice-Metadata") and hasText(kept) and isEnabled()).fetchSemanticsNodes().isNotEmpty()}
                    assertEquals("Remove location with $kept",location,compose.onAllNodesWithTag("export-remove-location").fetchSemanticsNodes().isNotEmpty())
                    if(location)compose.onNodeWithTag("export-remove-location").performScrollTo().assertIsOn()
                }
                choice("Format",label)
                metadata("All",true)
                choice("Metadata","Copyright & Contact")
                metadata("Copyright & Contact",false)
                choice("Metadata","All")
                metadata("All",true)
                compose.onNodeWithTag("export-choose-file").performClick()
                compose.waitUntil(120_000){picked.get()?.let{it.length()>0}==true&&!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}}
                idle()
                val bytes=picked.get().readBytes()
                assertTrue(picked.get().name.endsWith(".$format"))
                for(kept in listOf("Capycam","C-1","Capy 35mm F1.8","Ada Painter","(c) 2026 Ada Painter","2026:09:01 10:00:00"))assertTrue("$format keeps $kept",contains(bytes,kept))
                for(location in listOf("WGS-84","Lisbon"))assertFalse("$format leaves out $location",contains(bytes,location))
                val exported=android.media.ExifInterface(java.io.ByteArrayInputStream(bytes))
                assertFalse("$format has no GPS",exported.getLatLong(FloatArray(2)))
                assertEquals("Capycam",exported.getAttribute(android.media.ExifInterface.TAG_MAKE))
                assertEquals(1,exported.getAttributeInt(android.media.ExifInterface.TAG_ORIENTATION,0))
                picked.get().delete()
                println("PASS metadata export: ${bytes.size} bytes $format keeps camera, lens and copyright without location")
            }
        } finally {
            instrumentation.removeMonitor(monitor)
            DocumentController.nativeFileJobsForTest=true
            camera.delete()
        }
    }

    @Test fun selectionToolsRenderAndCombineOnDevice() {
        fun selection() = native { state(it).getJSONObject("layer_tools").getBoolean("has_selection") }
        fun waitSelection() = compose.waitUntil(30_000) { tick(); selection() }
        fun drag(x1: Double, y1: Double, x2: Double, y2: Double) {
            point(1, x1, y1); point(2, x2, y2); point(3, x2, y2); waitSelection()
        }
        invoke("fit_canvas")
        for (id in listOf("rectangle_select", "ellipse_select", "polygon_select")) {
            if (selection()) invoke("deselect")
            invoke(id)
            if (id == "polygon_select") {
                for ((x, y) in listOf(-80.0 to -60.0, 80.0 to -60.0, 80.0 to 60.0)) {
                    point(1, x, y); point(3, x, y)
                }
                invoke("complete_selection"); waitSelection()
            } else drag(-80.0, -60.0, 80.0, 60.0)
            invoke("undo"); assertFalse(selection())
            invoke("redo"); assertTrue(selection())
        }
        invoke("deselect")
        send(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(1.0, 0.0, 0.0, 1.0))))
        send(obj("type" to "set_brush_opacity", "value" to 1.0))
        for (x in listOf(-180.0, 100.0)) {
            invoke("rectangle_select"); drag(x, -80.0, x + 80.0, 0.0)
            invoke("fill_selection"); invoke("deselect")
        }
        val camera = native { state(it).getJSONObject("camera") }
        fun pixels(name: String): List<Int> {
            val bytes = png(name)
            val image = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            val viewport = camera.getJSONArray("viewport"); val pan = camera.getJSONArray("translation")
            val zoom = camera.getDouble("zoom")
            return listOf(-140.0, 140.0).map { x ->
                image.getPixel(((viewport.getDouble(0) * .5 + x - pan.getDouble(0)) / zoom).toInt(),
                    ((viewport.getDouble(1) * .5 - 40 - pan.getDouble(1)) / zoom).toInt())
            }.also { image.recycle() }
        }
        val original = pixels("selection-islands.png")
        assertTrue(original.all { android.graphics.Color.red(it) > 240 && android.graphics.Color.blue(it) < 20 })
        send(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(0.0, 0.0, 1.0, 1.0))))
        invoke("rectangle_select")
        drag(-180.0, -80.0, -100.0, 0.0)
        invoke("selection_add")
        drag(100.0, -80.0, 180.0, 0.0)
        invoke("fill_selection")
        assertTrue("Added selection includes both rectangles", pixels("added-selection.png").all { android.graphics.Color.blue(it) > 240 })
        invoke("undo"); invoke("deselect"); invoke("selection_new")
        for (id in listOf("auto_select", "color_select")) {
            invoke(id)
            send(obj("type" to "set_tool_setting", "id" to "selection_feather", "value" to 4.0))
            point(1, -140.0, -40.0); point(3, -140.0, -40.0); waitSelection()
            invoke("fill_selection")
            val result = pixels("$id-islands.png")
            assertTrue(android.graphics.Color.blue(result[0]) > 240)
            if (id == "auto_select") assertEquals(original[1], result[1])
            else assertTrue("Color selection reaches the disconnected island", android.graphics.Color.blue(result[1]) > 240)
            invoke("undo"); invoke("deselect")
        }
        println("PASS native selection geometry, polygon completion, undo/redo, feathered GPU masks and disconnected color islands")
    }

    @Test fun encloseFillNativeContactsControlsAndHistory() {
        fun ui(value: JSONObject) { host.drain(value, 10); compose.waitForIdle() }
        fun published() = host.snapshot!!.getJSONObject("state")
        fun tool(tag: String) = compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("panel-body-brushes")))
        fun settings(tag: String) = compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("panel-body-tool_settings")))
        fun label(command: String) = published().array("commands").objects().first { it.getString("id") == command }.getString("label")
        fun selected(command: String) = published().array("commands").objects().first { it.getString("id") == command }.getBoolean("selected")
        fun chooseTool(tag: String, command: String) {
            tool(tag).performScrollTo().assertIsDisplayed().performClick(); host.drain()
            compose.waitUntil(10_000) { selected(command) }
        }
        val reference = File(files, "enclose-reference.png")
        val bitmap = android.graphics.Bitmap.createBitmap(256, 128, android.graphics.Bitmap.Config.ARGB_8888)
        try {
            val boxes = listOf(intArrayOf(20, 20, 64, 100), intArrayOf(80, 20, 124, 100),
                intArrayOf(144, 20, 176, 100), intArrayOf(192, 20, 236, 100))
            for ((index, box) in boxes.withIndex()) for (y in box[1] until box[3]) for (x in box[0] until box[2]) {
                val edge = x < box[0] + 3 || x >= box[2] - 3 || y < box[1] + 3 || y >= box[3] - 3
                if (edge && !(index == 2 && y < box[1] + 3 && x in 157..162)) bitmap.setPixel(x, y, android.graphics.Color.BLACK)
            }
            reference.outputStream().use { assertTrue(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)) }
        } finally { bitmap.recycle() }
        val recipe = builtinRecipe(2).put("format", "Png").put("depth", "U8")
        fun pixels(name: String): IntArray {
            val bytes = png(name, recipe)
            val image = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size,
                android.graphics.BitmapFactory.Options().apply { inPremultiplied = false })!!
            try {
                assertEquals(256, image.width); assertEquals(128, image.height)
                return IntArray(256 * 128).also { image.getPixels(it, 0, 256, 0, 0, 256, 128) }
            } finally { image.recycle() }
        }
        fun loop(cancel: Boolean) {
            val camera = native { state(it).getJSONObject("camera") }
            val pan = camera.getJSONArray("translation"); val zoom = camera.getDouble("zoom")
            val origin = host.surfaceOrigin; val start = SystemClock.uptimeMillis()
            val path = listOf(12 to 12, 216 to 12, 216 to 112, 12 to 112, 12 to 12)
            for ((index, point) in path.withIndex()) {
                val phase = when (index) {
                    0 -> android.view.MotionEvent.ACTION_DOWN
                    path.lastIndex -> if (cancel) android.view.MotionEvent.ACTION_CANCEL else android.view.MotionEvent.ACTION_UP
                    else -> android.view.MotionEvent.ACTION_MOVE
                }
                val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 7; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS })
                val coordinates = arrayOf(android.view.MotionEvent.PointerCoords().apply {
                    x = (origin.x + pan.getDouble(0) + point.first * zoom).toFloat()
                    y = (origin.y + pan.getDouble(1) + point.second * zoom).toFloat()
                    pressure = if (index == path.lastIndex) 0f else .7f
                })
                val event = android.view.MotionEvent.obtain(start, SystemClock.uptimeMillis(), phase, 1, properties, coordinates,
                    0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
                SystemClock.sleep(40)
            }
            compose.waitUntil(60_000) { !tick() }; refresh()
        }
        for (theme in listOf("light", "dark")) {
            open(reference); refresh(); invoke("fit_canvas")
            val owner = native { state(it).getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id") }
            send(obj("type" to "layer", "action" to obj("op" to "reference", "id" to owner)))
            invoke("add_layer"); send(obj("type" to "layer", "action" to obj("op" to "cancel_rename")))
            ui(obj("type" to "set_theme", "theme" to theme))
            ui(obj("type" to "invoke", "command" to "fill"))
            val toolGroup = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "brushes" in it.array("panels").values() }.getInt("id")
            ui(obj("type" to "select_panel_tab", "group" to toolGroup, "panel" to "brushes"))
            val fill = label("fill"); val lasso = label("lasso_fill"); val enclose = label("enclose_fill")
            tool("tool-group-$enclose").assertDoesNotExist()
            tool("tool-group-$lasso").performScrollTo().assertIsDisplayed().performClick(); host.drain()
            chooseTool("subtool-$lasso", "lasso_fill")
            chooseTool("subtool-$enclose", "enclose_fill")
            chooseTool("tool-group-$fill", "fill")
            chooseTool("tool-group-$lasso", "enclose_fill")
            val tools = published().getJSONObject("tool_set")
            assertEquals(listOf(fill, lasso), tools.array("groups").objects().map { it.getString("label") })
            assertEquals(listOf(lasso, enclose), tools.array("subtools").objects().map { it.getString("label") })
            assertEquals(enclose, tools.array("subtools").objects().single { it.getBoolean("selected") }.getString("label"))
            val group = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "tool_settings" in it.array("panels").values() }.getInt("id")
            ui(obj("type" to "select_panel_tab", "group" to group, "panel" to "tool_settings"))
            for (command in listOf("selection_visible", "selection_editing", "selection_reference")) {
                settings("tool-action-$command").performScrollTo().assertIsDisplayed().performClick(); host.drain()
                compose.waitUntil(10_000) { selected(command) }
                settings("tool-action-$command").assertIsOn()
                assertTrue(selected("enclose_fill"))
            }
            for (id in listOf("tolerance", "gap_closing", "expansion", "smoothing")) {
                settings("tool-setting-$id").performScrollTo().assertIsDisplayed()
                ui(obj("type" to "set_tool_setting", "id" to id, "value" to 0))
            }
            ui(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
            compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("customization").isNull("expanded") }
            SystemClock.sleep(300)
            val command = native { state(it).array("commands").objects().first { c -> c.getString("id") == "enclose_fill" } }
            assertTrue(command.getBoolean("selected")); assertEquals("Enclose and Fill", command.getString("label"))
            ui(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(1.0, 0.0, 0.0, 1.0))))
            ui(obj("type" to "set_brush_opacity", "value" to 1.0))
            invoke("fit_canvas"); refresh()
            val before = pixels("enclose-$theme-before.png")
            loop(true); assertArrayEquals("Cancelled enclosure leaves artwork intact", before, pixels("enclose-$theme-cancel.png"))
            loop(false)
            val filled = pixels("enclose-$theme-filled.png")
            for (x in listOf(40, 100)) {
                val color = filled[60 * 256 + x]
                assertTrue("$theme enclosed area $x is filled", android.graphics.Color.red(color) > 240 && android.graphics.Color.green(color) < 20 && android.graphics.Color.alpha(color) > 240)
            }
            for (x in listOf(8, 72, 160, 204, 248)) assertEquals("$theme open and partly enclosed areas stay intact at $x", before[60 * 256 + x], filled[60 * 256 + x])
            assertFalse(native { state(it).getJSONObject("layer_tools").getBoolean("has_selection") })
            invoke("undo"); assertArrayEquals(before, pixels("enclose-$theme-undo.png"))
            invoke("redo"); assertArrayEquals(filled, pixels("enclose-$theme-redo.png"))
            instrumentation.uiAutomation.takeScreenshot()?.let { image ->
                try { File(activity.getExternalFilesDir(null), "enclose-fill-$theme.png").outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                finally { image.recycle() }
            }
        }
        assertNull(host.failure); assertNull(host.actionError)
    }

    @Test fun tonal61MpRecoveryAndInteraction() {
        // Stream a generated RGB photo; never read an artist's image or workspace.
        val width=9504; val height=6336
        val compressed=java.io.ByteArrayOutputStream()
        val deflater=java.util.zip.Deflater(1)
        try {
            java.util.zip.DeflaterOutputStream(compressed,deflater).use { output ->
                val row=ByteArray(1+width*3)
                for(y in 0 until height) {
                    for(x in 0 until width) {
                        val value=((x*13+y*7+((x xor y) and 31)) and 255).toByte()
                        val at=1+x*3;row[at]=value;row[at+1]=value;row[at+2]=value
                    }
                    output.write(row)
                }
            }
        } finally {deflater.end()}
        val photo=File(files,"generated-61mp.png")
        java.io.DataOutputStream(photo.outputStream().buffered()).use { output ->
            output.write(byteArrayOf(137.toByte(),80,78,71,13,10,26,10))
            fun chunk(type:String,data:ByteArray) {
                val name=type.toByteArray(Charsets.US_ASCII);val crc=java.util.zip.CRC32()
                crc.update(name);crc.update(data);output.writeInt(data.size);output.write(name);output.write(data);output.writeInt(crc.value.toInt())
            }
            val header=ByteBuffer.allocate(13).order(ByteOrder.BIG_ENDIAN).putInt(width).putInt(height).put(8).put(2).put(0).put(0).put(0).array()
            chunk("IHDR",header);chunk("IDAT",compressed.toByteArray());chunk("IEND",byteArrayOf())
        }
        open(photo)
        fun send(value:JSONObject):Long {
            val start=SystemClock.elapsedRealtimeNanos()
            native {Native.dispatch(it,value.toString())}
            compose.waitUntil(60_000) {!tick()}
            assertNull(host.failure)
            return (SystemClock.elapsedRealtimeNanos()-start)/1_000_000
        }
        fun invoke(id:String)=send(obj("type" to "invoke","command" to id))
        invoke("fit_canvas")
        invoke("tonal_select")
        val timings=(1..4).map {index -> send(obj("type" to "tonal","action" to obj("kind" to "preset","index" to index)))}
        assertTrue("61 MP selection was published",native {state(it).getJSONObject("layer_tools").getBoolean("has_selection")})
        // Exercise the real recovery worker/atomic publication that reported the
        // metadata-limit error, and parse its candidate before any adoption.
        val recovery=File(files,"tonal-61mp-recovery.capy")
        val saveStart=SystemClock.elapsedRealtimeNanos()
        captureSession(recovery)
        val saveMs=(SystemClock.elapsedRealtimeNanos()-saveStart)/1_000_000
        val head=JSONObject(File(recovery,"head.json").readText())
        val index=JSONObject(File(recovery,"generations/${head.getString("current")}.json").readText())
        assertFalse(index.getJSONObject("current").getJSONObject("working").isNull("selection"))
        assertTrue("Selection/history metadata stays bounded",index.toString().length<16*1024*1024)
        invoke("quick_mask")
        val quick=send(obj("type" to "tonal","action" to obj("kind" to "preset","index" to 2)))
        assertTrue(native {state(it).getJSONObject("layer_tools").getBoolean("quick_mask")})
        invoke("undo");invoke("redo")
        // Reopening a second 61 MP copy alongside all undo masks is a separate
        // workspace admission request. Close this generated tab before recovery.
        invoke("close_document")
        native {h ->
            val close=state(h).array("requests").objects().firstOrNull {it.getJSONObject("kind").optString("type")=="document"}
            if(close!=null) Native.documentClose(h,close.getInt("id"),"\"discard\"")
        }
        runBlocking {
            val id=JSONObject(host.drawingTabs.query(obj("op" to "view"))).getLong("selected")
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.remove(id)}
        }
        val activation=native {Native.documentSwitch(it,0,true)}
        if(activation!=0L) try {Native.documentResumeWork(activation);native {Native.documentResume(it,activation)}} finally {Native.documentResumeFree(activation)}
        val restore=restoreSessionTask(recovery)
        try {
            adoptSession(restore)
        } finally {Native.sessionFree(restore)}
        compose.waitUntil(60_000) {!tick()}
        assertTrue("Recovery restores the 61 MP mask",native {state(it).getJSONObject("layer_tools").getBoolean("has_selection")})
        assertNull(host.actionError);assertNull(host.failure)
        val report="PASS native Android 61 MP RGB: first_mask=${timings.first()}ms; warm=${timings.drop(1)}ms; quick_mask=${quick}ms; recovery=${saveMs}ms; archive=${recovery.walkTopDown().filter {it.isFile}.sumOf {it.length()}} bytes; metadata=${index.toString().length} bytes"
        println(report)
        File(activity.getExternalFilesDir(null),"tonal-61mp-result.txt").writeText(report)
        assertTrue("Warm 61 MP adjustments should finish within one second: $timings",timings.drop(1).all {it<1000})
    }

    @Test fun tonalHdrCoverageAndSamplingOnDevice() {
        val job=native { h -> val(id,f)=request(h,"new_document"); Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision")) }
        try {
            Native.projectOptions(job,obj("extent" to org.json.JSONArray(listOf(500,200)),"color" to obj("space" to "Srgb","depth" to "F16"),"background" to "White").toString())
            Native.projectWork(job,-1,500,200);native { Native.projectAdopt(it,job,"null") }
        } finally { Native.projectFree(job) }
        compose.runOnUiThread { host.documentChanged() }
        fun tone(index:Int)=send(obj("type" to "tonal","action" to obj("kind" to "preset","index" to index)))
        fun pixel(name:String):Int {
            val data=png(name);val image=android.graphics.BitmapFactory.decodeByteArray(data,0,data.size)
            return image.getPixel(image.width/2,image.height/2).also { image.recycle() }
        }
        invoke("fit_canvas");invoke("select_all")
        send(obj("type" to "set_color","rgba" to org.json.JSONArray(listOf(1,1,1,1))))
        send(obj("type" to "color","action" to obj("op" to "hdr_intensity","stops" to 2)))
        invoke("fill_selection");invoke("deselect");invoke("tonal_select")
        assertEquals(listOf("tonal-bright-hdr","tonal-custom"),native { state(it).array("tool_extra").getJSONObject(0).getJSONObject("Choice").array("items").objects().takeLast(2).map { item -> item.getString("icon") } })
        tone(6)
        send(obj("type" to "set_color","rgba" to org.json.JSONArray(listOf(0,0,1,1))))
        send(obj("type" to "color","action" to obj("op" to "hdr_intensity","stops" to 0)))
        invoke("fill_selection")
        val blue=pixel("tonal-hdr-selected.png")
        // The SDR export rendition can lift the blue fill's red/green channels.
        assertTrue("Bright HDR selects +2-stop artwork: ${Integer.toHexString(blue)}",
            android.graphics.Color.blue(blue)>200 && android.graphics.Color.red(blue)<100 && android.graphics.Color.green(blue)<100)
        invoke("undo");tone(0);invoke("fill_selection")
        val white=pixel("tonal-hdr-excluded.png")
        assertTrue("Shadows excludes +2-stop artwork",android.graphics.Color.red(white)>200 && android.graphics.Color.green(white)>200)
        invoke("quick_mask")
        point(1,0.0,0.0);point(3,0.0,0.0)
        val limits=native { state(it).array("tool_settings").objects().filter { f -> f.getString("id") in listOf("tonal_lower","tonal_upper") }.map { f -> f.number("value") } }
        assertEquals(2,limits.size)
        assertTrue("Sampling reads HDR artwork through Quick Mask: $limits",limits[0]<2f && limits[1]>2f && limits[0]>1f)
        assertTrue(native { state(it).getJSONObject("layer_tools").getBoolean("quick_mask") })
        println("PASS Huion Vulkan HDR tonal coverage, excluded shadows and artwork sampling through Quick Mask")
    }

    @Test fun paintableSelectionsOnDevice() {
        fun send(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }
            compose.waitUntil(30_000) { !tick() }
            val published = java.util.concurrent.atomic.AtomicBoolean(false)
            compose.runOnUiThread { host.documentChanged { published.set(true) } }
            compose.waitUntil(30_000) { published.get() }
            compose.waitForIdle()
        }
        fun invoke(id: String) = send(obj("type" to "invoke", "command" to id))
        fun view() = host.snapshot!!.getJSONObject("state").getJSONObject("layer_tools")
        invoke("fit_canvas")
        val blank = hash(png("selection-blank.png"))
        invoke("selection_brush"); invoke("selection_add")
        point(1,-60.0,-45.0); point(2,60.0,-45.0); point(2,60.0,45.0); point(2,-60.0,45.0); point(3,-60.0,-45.0)
        send(obj("type" to "close_settings"))
        assertTrue(view().getBoolean("has_selection"))
        invoke("undo"); assertFalse(view().getBoolean("has_selection"))
        invoke("redo"); assertTrue(view().getBoolean("has_selection"))
        invoke("deselect")
        send(obj("type" to "select_brush", "id" to 1))
        send(obj("type" to "set_brush_size", "value" to 90))
        val artColors = host.snapshot!!.getJSONObject("state").getJSONObject("colors").toString()
        invoke("quick_mask")
        assertTrue(view().getBoolean("quick_mask"))
        invoke("quick_mask"); assertFalse(view().getBoolean("has_selection")); invoke("quick_mask")
        compose.onNodeWithTag("selection-mask-actions").assertDoesNotExist()
        compose.onNodeWithTag("layer-row-0").assertIsDisplayed()
        compose.waitUntil(30_000) { compose.onAllNodesWithTag("layer-thumbnail-0-false",useUnmergedTree=true).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("layer-thumbnail-0-false",useUnmergedTree=true).assertIsDisplayed()
        val properties=host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties")
        assertEquals(3,properties.array("controls").length())
        assertEquals(0,properties.array("controls").objects().first { it.getString("key")=="mask_mode" }.getJSONObject("value").getInt("value"))
        // Inject through Android's actual input dispatcher and CanvasSurfaceView.
        val automation = instrumentation.uiAutomation
        val camera = host.snapshot!!.getJSONObject("state").getJSONObject("camera")
        val viewport = camera.getJSONArray("viewport")
        val origin = host.surfaceOrigin
        val start = SystemClock.uptimeMillis()
        var previousMaskPixels=0
        for (i in 0..12) {
            val phase = when(i) { 0 -> android.view.MotionEvent.ACTION_DOWN; 12 -> android.view.MotionEvent.ACTION_UP; else -> android.view.MotionEvent.ACTION_MOVE }
            val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id=7; toolType=android.view.MotionEvent.TOOL_TYPE_STYLUS })
            val coordinates = arrayOf(android.view.MotionEvent.PointerCoords().apply {
                x=origin.x+viewport.getDouble(0).toFloat()/2-70+i*12; y=origin.y+viewport.getDouble(1).toFloat()/2
                pressure=if(i==12)0f else .7f
            })
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),phase,1,properties,coordinates,0,0,1f,1f,0,0,android.view.InputDevice.SOURCE_STYLUS,0)
            try { assertTrue(automation.injectInputEvent(event,true)) } finally { event.recycle() }
            SystemClock.sleep(16)
            if(i<=2) {
                SystemClock.sleep(80)
                val shot=requireNotNull(automation.takeScreenshot())
                val pixels=IntArray(280*160)
                try { shot.getPixels(pixels,0,280,(origin.x+viewport.getDouble(0)/2-140).toInt(),(origin.y+viewport.getDouble(1)/2-80).toInt(),280,160) } finally { shot.recycle() }
                val count=pixels.count { android.graphics.Color.red(it)>android.graphics.Color.green(it)+40 && android.graphics.Color.red(it)>android.graphics.Color.blue(it)+40 }
                if(i>0)assertTrue("G-Pen refreshes sub-spacing motion before lift: $previousMaskPixels -> $count",count>previousMaskPixels+5)
                previousMaskPixels=count
            }
        }
        compose.waitUntil(30_000) { !tick() }
        assertEquals(artColors,host.snapshot!!.getJSONObject("state").getJSONObject("colors").toString())
        for (theme in listOf("light","dark")) {
            send(obj("type" to "set_theme", "theme" to theme))
            SystemClock.sleep(150)
            val shot=requireNotNull(automation.takeScreenshot())
            try {
                File(activity.getExternalFilesDir(null),"quick-mask-$theme.png").outputStream().use { shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it) }
                val x=(origin.x+viewport.getDouble(0)/2-100).toInt()
                val y=(origin.y+viewport.getDouble(1)/2-45).toInt()
                val pixels=IntArray(200*90); shot.getPixels(pixels,0,200,x,y,200,90)
                assertTrue("$theme: painted mask reaches Vulkan presentation",pixels.count { android.graphics.Color.red(it)>android.graphics.Color.green(it)+40 && android.graphics.Color.red(it)>android.graphics.Color.blue(it)+40 }>100)
            } finally { shot.recycle() }
        }
        send(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to true)))
        if(compose.onAllNodesWithTag("layer-properties").fetchSemanticsNodes().isEmpty()) {
            val group=host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "properties" in it.array("panels").values() }.getInt("id")
            send(obj("type" to "select_panel_tab", "group" to group, "panel" to "properties"))
        }
        compose.onNodeWithText("Paint selection").performClick()
        compose.onNodeWithText("Grayscale mask").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="mask_mode" }.getJSONObject("value").getInt("value")==1 }
        compose.onNodeWithText("Grayscale mask").performClick()
        compose.onNodeWithText("Paint selection").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="mask_mode" }.getJSONObject("value").getInt("value")==0 }
        compose.waitForIdle()
        automation.takeScreenshot()?.let { image -> try { File(activity.getExternalFilesDir(null),"quick-mask-properties.png").outputStream().use {image.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)} } finally {image.recycle()} }
        send(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(0,.5,1,1))))
        compose.onNodeWithTag("mask-color-bucket").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="mask_color" }.getJSONObject("value").getJSONObject("value").getJSONArray("rgba").getDouble(1)==.5 }
        send(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to false)))
        compose.onNodeWithTag("selection-load-0",useUnmergedTree=true).performClick()
        compose.waitUntil(30_000) { !view().optBoolean("quick_mask") }
        assertTrue(view().getBoolean("has_selection"))
        invoke("quick_mask")
        invoke("save_selection_layer")
        assertFalse(view().getBoolean("quick_mask"))
        send(obj("type" to "layer", "action" to obj("op" to "cancel_rename")))
        val id=host.snapshot!!.getJSONObject("state").array("layers").objects().first {it.optBoolean("selection_layer")}.getLong("id")
        assertEquals(id,view().getJSONObject("mask_editing").getLong("layer"))
        invoke("return_to_artwork")
        assertFalse(host.snapshot!!.getJSONObject("state").array("layers").objects().first {it.getLong("id")==id}.getBoolean("visible"))
        val label=host.snapshot!!.getJSONObject("state").array("layers").objects().first { it.getLong("id")==id }.getString("label")
        compose.onNodeWithText(label).performTouchInput { doubleClick() }
        compose.waitUntil(10_000) { view().optLong("rename_layer",-1)==id }
        send(obj("type" to "layer", "action" to obj("op" to "cancel_rename")))
        send(obj("type" to "select_layer", "id" to id))
        assertEquals(id,view().getJSONObject("mask_editing").getLong("layer"))
        assertEquals("layer-brush-symbolic",host.snapshot!!.getJSONObject("state").array("layers").objects().first { it.getLong("id")==id }.getString("selection_icon"))
        val thumb=compose.onNodeWithTag("layer-thumbnail-$id-false",useUnmergedTree=true).fetchSemanticsNode().boundsInRoot
        val load=compose.onNodeWithTag("selection-load-$id",useUnmergedTree=true).fetchSemanticsNode().boundsInRoot
        assertTrue("Thumbnail $thumb and Load $load align",kotlin.math.abs(thumb.width-load.width)<=thumb.width*.1f && load.left>=thumb.right && load.left-thumb.right<20f)
        send(obj("type" to "selection", "action" to obj("op" to "begin_refine", "kind" to "grow", "layer" to id)))
        compose.onNodeWithText("Grow Selection").assertIsDisplayed()
        compose.onNodeWithText("Grow by").assertIsDisplayed()
        compose.onNodeWithTag("selection-refine-apply").performClick()
        compose.waitUntil(30_000) { !tick() }
        invoke("undo");invoke("redo")
        invoke("clear_selection_mask"); invoke("return_to_artwork")
        invoke("deselect")
        compose.onNodeWithTag("selection-load-$id",useUnmergedTree=true).assertIsDisplayed().performClick()
        compose.waitUntil(30_000) { view().getBoolean("has_selection") }
        assertTrue("An explicitly empty selection is retained",view().getBoolean("has_selection"))
        assertEquals("Selection overlays never enter exported artwork",blank,hash(png("selection-overlay-export.png")))
        invoke("rectangle_select");invoke("selection_new")
        fun selected(command:String)=host.snapshot!!.getJSONObject("state").array("commands").objects().first { it.getString("id")==command }.getBoolean("selected")
        for((meta,code,command) in listOf(
            Triple(android.view.KeyEvent.META_SHIFT_ON,android.view.KeyEvent.KEYCODE_SHIFT_LEFT,"selection_add"),
            Triple(android.view.KeyEvent.META_ALT_ON,android.view.KeyEvent.KEYCODE_ALT_LEFT,"selection_subtract"),
            Triple(android.view.KeyEvent.META_SHIFT_ON or android.view.KeyEvent.META_ALT_ON,android.view.KeyEvent.KEYCODE_SHIFT_LEFT,"selection_intersect"))) {
            val now=SystemClock.uptimeMillis()
            assertTrue(automation.injectInputEvent(android.view.KeyEvent(now,now,android.view.KeyEvent.ACTION_DOWN,code,0,meta),true))
            compose.waitUntil(10_000) { selected(command) }
            assertTrue(automation.injectInputEvent(android.view.KeyEvent(now,SystemClock.uptimeMillis(),android.view.KeyEvent.ACTION_UP,code,0,0),true))
            compose.waitUntil(10_000) { selected("selection_new") }
        }
        assertNull(host.failure); assertNull(host.actionError)
        println("PASS Selection Brush GPU history, Android stylus Quick Mask, independent colors, saved masks and clean artwork export")
    }

    @Test fun portablePhotoGainmapDelivery() {
        val root=InstrumentationRegistry.getArguments().getString("photoDirectory") ?: throw AssumptionViolatedException("Supply -e photoDirectory with the portable photo fixtures")
        require(Regex("/data/local/tmp/[A-Za-z0-9_/-]+").matches(root))
        val automation=instrumentation.uiAutomation
        fun fixture(name:String)=File(files,name).apply {
            writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $root/$name")).use{it.readBytes()})
            assertTrue("Fixture $name",length()>0)
        }
        fun idle(){compose.waitUntil(120_000){!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}};assertNull(host.failure);assertNull(host.actionError)}
        fun choice(label:String,text:String){
            compose.waitUntil(30_000){compose.onAllNodes(hasTestTag("color-choice-$label") and isEnabled()).fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("color-choice-$label").performScrollTo().performClick()
            compose.onNodeWithText(text).performClick();compose.waitForIdle()
        }
        val report=obj("model" to android.os.Build.MODEL,"imports" to org.json.JSONArray(),"exports" to org.json.JSONArray())
        for((name,depth) in listOf("p3-grid-8bit.heic" to "U8","p3-gray-10bit.heic" to "U16","p3-12bit.avif" to "U16","web-hdr.jpg" to "F16","web-hdr.avif" to "F16")) {
            open(fixture(name));refresh()
            assertEquals(name,depth,native{JSONObject(Native.query(it,obj("type" to "document_color").toString())).getString("depth")})
            report.getJSONArray("imports").put(name)
        }
        val original=fixture("web-hdr.avif")
        for((format,label,extension,mime) in listOf(
            listOf("JpegHdr","HDR JPEG · gain map","jpg","image/jpeg"),
            listOf("AvifHdr","HDR AVIF · gain map with transparency","avif","image/avif")
        )) {
            open(original);refresh()
            val master=save("portable-master.capy");val before=histogram()
            assertTrue(before.getJSONArray("channels").objects().any{it.getLong("above")>0})
            val savedName="capy-portable-${System.nanoTime()}.$extension"
            val uri=activity.contentResolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,android.content.ContentValues().apply {
                put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME,savedName)
                put(android.provider.MediaStore.MediaColumns.MIME_TYPE,mime)
            })!!
            val picked=java.util.concurrent.atomic.AtomicReference<android.content.Intent>()
            // Supply only the OS chooser's result; the real dialog, controller,
            // JNI capture/encoder and temporary-file publication all execute.
            val monitor=object:android.app.Instrumentation.ActivityMonitor() {
                override fun onStartActivity(intent:android.content.Intent):android.app.Instrumentation.ActivityResult? {
                    if(intent.action!=android.content.Intent.ACTION_CREATE_DOCUMENT)return null
                    picked.set(android.content.Intent(intent))
                    return android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK,android.content.Intent().setData(uri).addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION))
                }
            }
            instrumentation.addMonitor(monitor)
            try {
                DocumentController.nativeFileJobsForTest=false
                compose.runOnUiThread{host.invoke("export_document")}
                choice("Dynamic range",label)
                if(format=="JpegHdr")choice("Transparency","White background")
                compose.onNodeWithText("Preview Output").performScrollTo().performClick()
                compose.waitUntil(120_000){compose.onAllNodesWithContentDescription("Output preview").fetchSemanticsNodes().isNotEmpty()}
                choice("Preview rendition","Encoded SDR base")
                val sdr=compose.onNodeWithContentDescription("Output preview").performScrollTo().captureToImage()
                assertTrue(sdr.width>0&&sdr.height>0)
                choice("Preview rendition","HDR reconstruction · SDR preview")
                choice("Preview rendition","Encoded SDR base")
                compose.onNodeWithContentDescription("Output preview").performScrollTo()
                val output=File(activity.getExternalFilesDir(null),"portable-$extension.png")
                automation.takeScreenshot()?.let{shot->output.outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
                compose.onNodeWithTag("export-choose-file").performClick()
                compose.waitUntil(120_000){picked.get()!=null&&!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}}
                assertEquals(mime,picked.get().type)
                assertTrue(picked.get().getStringExtra(android.content.Intent.EXTRA_TITLE)!!.endsWith(".$extension"))
                assertEquals(mime,host.documents.exportMime())
                assertNull(native{state(it)}.opt("host_error").takeUnless{it==JSONObject.NULL})
                assertNull(host.actionError)
                DocumentController.nativeFileJobsForTest=true
                assertEquals(before.toString(),histogram().toString())
                assertArrayEquals(master,save("portable-unchanged.capy"))
                val bytes=activity.contentResolver.openInputStream(uri)!!.use{it.readBytes()}
                assertTrue(bytes.size>100)
                val delivery=File(activity.getExternalFilesDir(null),"portable-hdr.$extension").apply{writeBytes(bytes)}
                open(delivery);refresh()
                val restored=histogram()
                assertEquals("F16",restored.getJSONObject("color").getString("depth"))
                assertTrue(restored.getJSONArray("channels").objects().any{it.getLong("above")>0})
                if(format=="JpegHdr")assertEquals(0L,restored.getLong("transparent"))
                else assertTrue(restored.getLong("transparent")>0)
                report.getJSONArray("exports").put(obj("format" to format,"bytes" to bytes.size,"mime" to mime))
            } finally {
                instrumentation.removeMonitor(monitor)
                activity.contentResolver.delete(uri,null,null)
                DocumentController.nativeFileJobsForTest=true
            }
        }
        open(original);refresh()
        val initial=builtinRecipe(0)
        val recipe=native{JSONObject(Native.query(it,obj("type" to "export_draft","recipe" to initial,"action" to obj("type" to "format","value" to "AvifHdrMapped")).toString())).getJSONObject("recipe")}
        val color=native{JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
        val preset=runBlocking{ColorPreferencesStore.presets(activity,color,obj("type" to "save","name" to "Portable HDR","recipe" to recipe))}
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread{host.invoke("export_document")}
        choice("Destination","Portable HDR")
        compose.onNodeWithTag("color-choice-Dynamic range").assertTextContains("HDR AVIF · gain map with transparency")
        compose.onNodeWithText("Cancel").performClick();idle();DocumentController.nativeFileJobsForTest=true
        assertEquals("AvifHdrMapped",runBlocking{ColorPreferencesStore.presets(activity,color,obj("type" to "get","index" to preset.getInt("index")))}.getJSONObject("recipe").getString("format"))
        // Cancel an admitted export while its independent worker is active.
        recipe.put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(1024,1024)),"enlarge" to true)))
        val c=Native.captureControl();val id=native{request(it,"export_document").first}
        val task=native{Native.projectExportTask(it,id,System.nanoTime(),c)}
        assertNotEquals(0L,task);Native.projectExportOptions(task,recipe.toString())
        val output=File(files,"portable-cancelled.avif")
        val pool=java.util.concurrent.Executors.newSingleThreadExecutor()
        try {
            val entered=java.util.concurrent.CountDownLatch(1)
            val result=pool.submit<String> {
                entered.countDown()
                try {Native.projectWork(task,ParcelFileDescriptor.open(output,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0);"completed"}
                catch(e:Exception){e.message.orEmpty()}
            }
            assertTrue(entered.await(10,java.util.concurrent.TimeUnit.SECONDS));SystemClock.sleep(50)
            val started=SystemClock.uptimeMillis();Native.captureCancel(c)
            assertTrue(result.get(10,java.util.concurrent.TimeUnit.SECONDS).contains("cancel",ignoreCase=true))
            report.put("cancel_ms",SystemClock.uptimeMillis()-started)
            assertEquals(0L,output.length())
            native{Native.documentComplete(it,id,false,"null")}
        } finally {pool.shutdown();check(pool.awaitTermination(30,java.util.concurrent.TimeUnit.SECONDS)){"Export worker failed to drain"};Native.projectFree(task);Native.captureFree(c)}
        recipe.put("size","Original")
        val flag=Native.captureControl()
        try {val preview=Native.inspectionOutput(native{Native.inspectionTask(it,flag)},recipe.toString());assertEquals(4,preview.size);assertFalse((preview[2] as ByteArray).contentEquals(preview[3] as ByteArray))}
        finally{Native.captureFree(flag)}
        assertTrue(files.listFiles().orEmpty().none{it.name.startsWith("capy-save-")})
        File(activity.getExternalFilesDir(null),"portable-photo-report.json").writeText(report.toString(2))
        assertNull(host.failure)
    }

    @Test fun portablePhotoLargeDelivery() {
        val arguments=InstrumentationRegistry.getArguments()
        val path=arguments.getString("photoFile") ?: throw AssumptionViolatedException("Supply -e photoFile with an HDR photo")
        require(Regex("/data/local/tmp/[A-Za-z0-9_./-]+").matches(path))
        val automation=instrumentation.uiAutomation
        val input=File(files,"large-photo.avif").apply {
            writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $path")).use{it.readBytes()})
        }
        val quality=arguments.getString("photoQuality")?.toInt()?:90
        val report=obj("model" to android.os.Build.MODEL,"input" to path,"quality" to quality,"exports" to org.json.JSONArray())
        val output=File(activity.getExternalFilesDir(null),"portable-large-report.json")
        val watching=java.util.concurrent.atomic.AtomicBoolean(true)
        val peak=java.util.concurrent.atomic.AtomicLong()
        val sampler=Thread{while(watching.get()){peak.accumulateAndGet(android.os.Debug.getPss().toLong()*1024,::maxOf);SystemClock.sleep(250)}}.apply{start()}
        try {
            val started=SystemClock.uptimeMillis();open(input);refresh()
            report.put("open_ms",SystemClock.uptimeMillis()-started)
            val master=save("large-master.capy")
            val document=manifest(master).compositionSize()
            val extent=listOf(document.getInt(0),document.getInt(1))
            report.put("extent",org.json.JSONArray(extent))
            val original=histogram()
            assertEquals("F16",original.getJSONObject("color").getString("depth"))
            assertTrue(original.getJSONArray("channels").objects().any{it.getLong("above")>0})
            for((format,extension) in listOf("JpegHdrMapped" to "jpg","AvifHdrMapped" to "avif")) {
                open(File(files,"large-master.capy"));refresh()
                val basic=builtinRecipe(0)
                val recipe=native{h->
                    JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to basic,"action" to obj("type" to "format","value" to format)).toString())).getJSONObject("recipe")
                }.put("jpeg_quality",quality).put("background",if(extension=="jpg")"White" else "Preserve")
                val entry=obj("format" to format);report.getJSONArray("exports").put(entry)
                val c=Native.captureControl();val previewStart=SystemClock.uptimeMillis()
                try {
                    val preview=Native.inspectionOutput(native{Native.inspectionTask(it,c)},recipe.toString())
                    entry.put("preview_ms",SystemClock.uptimeMillis()-previewStart)
                    assertEquals(4,preview.size)
                    assertEquals(extent,JSONObject(preview[0] as String).getJSONArray("extent").values())
                }finally{Native.captureFree(c)}
                val encodeStart=SystemClock.uptimeMillis()
                val bytes=png("large-delivery.$extension",recipe)
                entry.put("export_ms",SystemClock.uptimeMillis()-encodeStart).put("bytes",bytes.size)
                assertArrayEquals(master,save("large-unchanged.capy"))
                assertEquals(original.toString(),histogram().toString())
                val reopened=SystemClock.uptimeMillis();open(File(files,"large-delivery.$extension"));refresh()
                entry.put("reopen_ms",SystemClock.uptimeMillis()-reopened)
                val color=histogram()
                assertEquals("F16",color.getJSONObject("color").getString("depth"))
                assertTrue(color.getJSONArray("channels").objects().any{it.getLong("above")>0})
                val restored=manifest(save("large-reopened.capy")).compositionSize()
                assertEquals(extent,listOf(restored.getInt(0),restored.getInt(1)))
                output.writeText(report.toString(2))
            }
            assertNull(host.failure)
        }catch(e:Throwable){report.put("error",e.toString());throw e}
        finally{watching.set(false);sampler.join(2000);report.put("peak_process_pss_bytes",peak.get());output.writeText(report.toString(2))}
    }

    @Test fun hdrBlackIntensityMarkerVisible() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile") ?: throw AssumptionViolatedException("Supply -e hdrFile")
        val automation=instrumentation.uiAutomation
        val input=File(files,"hdr-marker.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $sourcePath")).use{it.readBytes()})}
        open(input)
        native { Native.dispatch(it,obj("type" to "color","action" to obj("op" to "set_slot_intensity","slot" to "foreground","color" to obj("space" to "Srgb","rgba" to org.json.JSONArray(listOf(0,0,0,1))),"stops" to 2)).toString()) }
        tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
        automation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),"black-ev-marker.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        val image=compose.onNodeWithTag("color-hdr-intensity").captureToImage().toPixelMap()
        val density=activity.resources.displayMetrics.density
        val arc=JSONObject(Native.colorUi(obj("type" to "arc","size" to image.width/density,"fraction" to .5).toString()))
        val point=arc.getJSONArray("point");val x=point.getDouble(0)*density;val y=point.getDouble(1)*density;val radius=arc.getJSONObject("geometry").getDouble("marker_radius")*density
        assertTrue("EV handle fits within the panel: ${image.width} x ${image.height}, center=($x,$y), radius=$radius",x-radius>=0&&x+radius<image.width&&y-radius>=0&&y+radius<image.height)
        val center=image[x.toInt(),y.toInt()]
        assertTrue("Exposure preserves black",center.red<.02f&&center.green<.02f&&center.blue<.02f)
        for(i in 0 until 16){
            val angle=i*Math.PI/8
            val pixel=image[kotlin.math.round(x+radius*kotlin.math.cos(angle)).toInt(),kotlin.math.round(y+radius*kotlin.math.sin(angle)).toInt()]
            assertTrue("Black EV handle has a visible white ring at $i: $pixel",pixel.red>.85f&&pixel.green>.85f&&pixel.blue>.85f)
        }
        fun point(fraction:Double):androidx.compose.ui.geometry.Offset {
            val p=JSONObject(Native.colorUi(obj("type" to "arc","size" to image.width/density,"fraction" to fraction).toString())).getJSONArray("point")
            return androidx.compose.ui.geometry.Offset(p.getDouble(0).toFloat()*density,p.getDouble(1).toFloat()*density)
        }
        compose.onNodeWithTag("color-hdr-intensity").performTouchInput { swipe(point(.3),point(.7),300) }
        compose.waitForIdle()
        assertEquals("EV drag follows the visible arc",3.6,host.snapshot!!.getJSONObject("color_panel").getDouble("intensity"),.06)
        compose.onNodeWithTag("color-hdr-intensity").performTouchInput { down(point(.7));moveTo(point(.4));cancel() }
        compose.waitForIdle()
        assertEquals("Cancelled EV drag restores its value",3.6,host.snapshot!!.getJSONObject("color_panel").getDouble("intensity"),.06)
    }

    @Test fun hdrArcCapsAcceptTouchPenAndMouseWithoutBrushPreparation() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile") ?: throw AssumptionViolatedException("Supply -e hdrFile")
        val input=File(files,"hdr-arc.png").apply { writeBytes(ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand("cat $sourcePath")).use { it.readBytes() }) }
        open(input);refresh()
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true && host.snapshot?.objectOrNull("color_panel")?.optBoolean("hdr") == true }
        for(theme in listOf("light","dark")) {
            send(obj("type" to "set_theme","theme" to theme));refresh()
            val arc=compose.onNodeWithTag("color-hdr-intensity").fetchSemanticsNode()
            val density=activity.resources.displayMetrics.density
            val side=arc.boundsInRoot.width/density
            lateinit var root: androidx.compose.ui.platform.ViewRootForTest
            instrumentation.runOnMainSync { root=findTag("color-hdr-intensity")!!.first }
            fun input(tool:Int,action:Int,fraction:Double,down:Long) {
                val p=JSONObject(Native.colorUi(obj("type" to "arc","size" to side,"fraction" to fraction).toString())).getJSONArray("point")
                val local=arc.boundsInRoot.topLeft+androidx.compose.ui.geometry.Offset((p.getDouble(0)*density).toFloat(),(p.getDouble(1)*density).toFloat())
                val event=motion(tool,action,local,down)
                try { instrumentation.runOnMainSync { assertTrue("$theme $tool $fraction reaches the color panel",root.view.dispatchTouchEvent(event)) } }
                finally { event.recycle() }
            }
            for(tool in listOf(android.view.MotionEvent.TOOL_TYPE_FINGER,android.view.MotionEvent.TOOL_TYPE_STYLUS,android.view.MotionEvent.TOOL_TYPE_MOUSE)) {
                for((fraction,expected) in listOf(0.0 to -2.0,1.0 to 6.0)) {
                    val down=SystemClock.uptimeMillis()
                    input(tool,android.view.MotionEvent.ACTION_DOWN,fraction,down)
                    compose.waitForIdle()
                    input(tool,android.view.MotionEvent.ACTION_UP,fraction,down)
                    compose.waitUntil(10_000) { kotlin.math.abs(host.snapshot?.objectOrNull("color_panel")?.optDouble("intensity")?.minus(expected) ?: 100.0)<.05 }
                    assertTrue("$theme $tool $fraction keeps the brush ready",host.snapshot!!.optBoolean("brush_ready"))
                }
                val down=SystemClock.uptimeMillis()
                for(step in 0..24) {
                    val fraction=step/24.0
                    val action=when(step) {0->android.view.MotionEvent.ACTION_DOWN;24->android.view.MotionEvent.ACTION_UP;else->android.view.MotionEvent.ACTION_MOVE}
                    input(tool,action,fraction,down)
                    compose.waitForIdle()
                    assertTrue("$theme $tool EV drag keeps the brush ready at $step",host.snapshot!!.optBoolean("brush_ready"))
                }
            }
        }
    }

    @Test fun hdrEditingProofDeliveryAndRecovery() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile")
        Assume.assumeTrue("Supply an independently encoded PQ PNG with -e hdrFile",sourcePath!=null)
        val automation=instrumentation.uiAutomation
        val input=File(files,"hdr-input.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $sourcePath")).use{it.readBytes()})}
        fun action(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};refresh()}
        fun histogram(): String {
            val control = Native.captureControl()
            try {
                val task = native { Native.inspectionTask(it, control) }
                return JSONObject(Native.inspectionStatistics(task, sourceVisible, false, false, false))
                    .getJSONObject("histogram").toString()
            } finally { Native.captureFree(control) }
        }
        fun form()=native{JSONObject(Native.query(it,obj("type" to "proof_form").toString()))}
        fun ready(){compose.waitUntil(120_000){native{JSONObject(Native.toneStatus(it)).getBoolean("ready")}};assertNull(host.failure)}
        open(input);refresh();ready()
        var original=histogram()
        assertEquals("F16",JSONObject(original).getJSONObject("color").getString("depth"))
        assertTrue(JSONObject(original).getJSONArray("channels").objects().any{it.getLong("above")>0})
        fun captureColor(name:String){automation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),name).outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}}
        captureColor("color-panel.png")
        compose.onNodeWithTag("color-edit-button").performClick()
        compose.waitForIdle();SystemClock.sleep(300);captureColor("edit-color.png")
        fun intensity(text:String){
            if(compose.onAllNodesWithTag("color-value-ev-input").fetchSemanticsNodes().isEmpty())compose.onNodeWithTag("color-value-ev").performClick()
            compose.onNodeWithTag("color-value-ev-input").performTextReplacement(text);compose.onNodeWithTag("color-value-ev-input").performImeAction();compose.waitForIdle()
        }
        for(text in listOf("","-",".","17","1e999")) {
            intensity(text)
            compose.onNodeWithTag("color-use").assertIsNotEnabled()
        }
        intensity("-0.5")
        compose.onNodeWithTag("color-use").assertIsEnabled()
        intensity("3")
        compose.onNodeWithTag("color-use").performClick();refresh()
        assertEquals(3.0,host.snapshot!!.getJSONObject("color_panel").getDouble("intensity"),.001)
        compose.onAllNodesWithText("Palettes…").assertCountEquals(0)
        native {h->
            val color=srgbLinear(-4.0,4.0,1.0)
            Native.dispatch(h,obj("type" to "select_brush","id" to 1).toString());Native.dispatch(h,obj("type" to "color","action" to obj("op" to "set_slot","slot" to "foreground","color" to color)).toString())
        };refresh()
        motion(android.view.MotionEvent.TOOL_TYPE_STYLUS,30,256.0 to 192.0);refresh()
        val painted=histogram();assertTrue(JSONObject(painted).getJSONArray("channels").objects().any{it.getLong("below")>0});assertNotEquals(original,painted)
        action("undo");assertEquals(original,histogram());action("redo");assertEquals(painted,histogram());original=painted
        val first=manifest(save("hdr-original.capy"))
        action("sdr_rendition")
        compose.waitUntil(10_000){compose.onAllNodesWithTag("sdr-tone-pad").fetchSemanticsNodes().isNotEmpty()}
        val before=form().getJSONObject("rendition").toString()
        compose.runOnUiThread{host.customize(obj("type" to "set_panel_visible","panel" to "layers","visible" to true))};refresh()
        val layerGroup=host.snapshot!!.getJSONObject("layout").array("groups").objects().first{ "layers" in it.array("panels").values() }.getInt("id")
        compose.runOnUiThread{host.dispatch(obj("type" to "select_panel_tab","group" to layerGroup,"panel" to "layers"))};refresh()
        val thumbnailId=host.snapshot!!.getJSONObject("state").array("layers").objects().first{it.getBoolean("has_thumbnail")}.getLong("id")
        val thumbnailTag="layer-thumbnail-$thumbnailId-false"
        try{compose.waitUntil(10_000){compose.onAllNodesWithTag(thumbnailTag,useUnmergedTree=true).fetchSemanticsNodes().isNotEmpty()}}
        catch(e:AssertionError){File(activity.getExternalFilesDir(null),"thumbnail-failure-tree.txt").writeText(compose.onRoot(useUnmergedTree=true).printToString());File(activity.getExternalFilesDir(null),"thumbnail-failure-state.json").writeText(host.snapshot.toString());throw e}
        fun thumbnail():List<Byte> {
            val image=compose.onNodeWithTag(thumbnailTag,useUnmergedTree=true).captureToImage().toPixelMap()
            val bytes=ByteArray(image.width*image.height*3)
            for(y in 0 until image.height)for(x in 0 until image.width){val color=image[x,y];val i=(y*image.width+x)*3
                bytes[i]=(color.red*255).toInt().toByte();bytes[i+1]=(color.green*255).toInt().toByte();bytes[i+2]=(color.blue*255).toInt().toByte()}
            return hash(bytes)
        }
        SystemClock.sleep(400)
        val originalThumbnail=thumbnail()
        compose.onNodeWithTag("sdr-tone-pad").performTouchInput {down(center);moveTo(center+androidx.compose.ui.geometry.Offset(50f,-30f));cancel()}
        refresh();assertEquals(before,form().getJSONObject("rendition").toString())
        // Inject through Android InputDispatcher with a stylus tool, not a mouse.
        val padNode=compose.onNodeWithTag("sdr-tone-pad").fetchSemanticsNode()
        val padCenter=padNode.layoutInfo.coordinates.localToScreen(androidx.compose.ui.geometry.Offset(padNode.size.width/2f,padNode.size.height/2f))
        val start=SystemClock.uptimeMillis()
        for((index,phase)in listOf(android.view.MotionEvent.ACTION_DOWN,android.view.MotionEvent.ACTION_MOVE,android.view.MotionEvent.ACTION_UP).withIndex()){
            val properties=arrayOf(android.view.MotionEvent.PointerProperties().apply{id=7;toolType=android.view.MotionEvent.TOOL_TYPE_STYLUS})
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply{x=padCenter.x+(if(index==0)0f else 45f);y=padCenter.y-(if(index==0)0f else 30f);pressure=if(index==2)0f else .7f})
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),phase,1,properties,coords,0,0,1f,1f,0,0,android.view.InputDevice.SOURCE_STYLUS,0)
            try{assertTrue(automation.injectInputEvent(event,true))}finally{event.recycle()}
            SystemClock.sleep(30)
        }
        refresh();val changed=form().getJSONObject("rendition").toString();assertNotEquals(before,changed)
        compose.waitUntil(10_000){thumbnail()!=originalThumbnail}
        val changedThumbnail=thumbnail()
        action("undo");assertEquals(before,form().getJSONObject("rendition").toString())
        compose.waitUntil(10_000){thumbnail()==originalThumbnail}
        action("redo");assertEquals(changed,form().getJSONObject("rendition").toString())
        compose.waitUntil(10_000){thumbnail()==changedThumbnail}
        val density=activity.resources.displayMetrics.density
        val arc=JSONObject(Native.colorUi(obj("type" to "proof_dial","size" to padNode.size.width/density,"recipe" to form().getJSONObject("rendition")).toString())).getJSONArray("arcs").getJSONObject(0).getJSONArray("path")
        fun arcPoint(i:Int)=arc.getJSONArray(i).let{androidx.compose.ui.geometry.Offset(it.getDouble(0).toFloat()*density,it.getDouble(1).toFloat()*density)}
        compose.onNodeWithTag("sdr-tone-pad").performTouchInput {down(arcPoint(16));moveTo(arcPoint(48));cancel()}
        refresh();assertEquals(changed,form().getJSONObject("rendition").toString())
        compose.onNodeWithTag("sdr-tone-pad").performTouchInput {down(arcPoint(16));moveTo(arcPoint(48));up()}
        refresh();assertNotEquals(JSONObject(changed).getDouble("exposure"),form().getJSONObject("rendition").getDouble("exposure"),.01)
        action("undo");assertEquals(changed,form().getJSONObject("rendition").toString())
        // External history must refresh the visible dial as well as its Rust recipe.
        compose.waitUntil(10_000){compose.onNodeWithTag("sdr-tone-pad").fetchSemanticsNode().config[SemanticsProperties.StateDescription].contains(", Brightness 0%,")}
        val keys=compose.onNodeWithTag("sdr-tone-pad")
        keys.performSemanticsAction(SemanticsActions.RequestFocus)
        keys.performKeyInput {keyDown(Key.DirectionRight);keyUp(Key.DirectionRight)}
        refresh();assertEquals((JSONObject(changed).getDouble("balance")+.01).coerceAtMost(1.0),form().getJSONObject("rendition").getDouble("balance"),1e-6)
        action("undo");assertEquals(changed,form().getJSONObject("rendition").toString())
        keys.performKeyInput{keyDown(Key.DirectionUp);keyDown(Key.Escape);keyUp(Key.Escape);keyUp(Key.DirectionUp)}
        refresh();assertEquals(changed,form().getJSONObject("rendition").toString())
        for((part,key,step)in listOf(Triple(1,"exposure",.04),Triple(2,"highlight_color",.01))) {
            val arcKeys=compose.onNodeWithTag("sdr-arc-$part")
            arcKeys.performSemanticsAction(SemanticsActions.RequestFocus)
            arcKeys.performKeyInput{keyDown(Key.DirectionUp);keyUp(Key.DirectionUp)}
            refresh();assertEquals(JSONObject(changed).getDouble(key)+step,form().getJSONObject("rendition").getDouble(key),1e-6)
            action("undo");assertEquals(changed,form().getJSONObject("rendition").toString())
        }
        compose.waitUntil(10_000){compose.onNodeWithTag("sdr-tone-pad").fetchSemanticsNode().config[SemanticsProperties.StateDescription].endsWith("Color intensity 30%") }
        assertEquals(original,histogram())
        automation.takeScreenshot()?.let { screenshot->File(activity.getExternalFilesDir(null),"hdr-proof.png").outputStream().use{screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};screenshot.recycle() }
        compose.runOnUiThread{host.customize(obj("type" to "set_panel_visible","panel" to "proof","visible" to false))};refresh()
        val saved=save("hdr-master.capy");val manifest=manifest(saved)
        assertEquals(first.rasterResources().toString(),manifest.rasterResources().toString())
        assertEquals(first.artworkRecords().toString(),manifest.artworkRecords().toString())
        open(File(files,"hdr-master.capy"));refresh();ready();assertEquals(original,histogram());assertEquals(changed,form().getJSONObject("rendition").toString())
        File(activity.getExternalFilesDir(null),"hdr-sdr-rendition.json").writeText(form().getJSONObject("rendition").toString())
        val sdr=png("hdr-sdr.png")
        val basic=builtinRecipe(0);val recipe=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to basic,"action" to obj("type" to "format","value" to "PngHdr")).toString())).getJSONObject("recipe")}
        assertTrue("Strict PQ delivery rejects out-of-range paint",runCatching{png("hdr-strict-rejected.png",recipe)}.isFailure)
        val clipped=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to recipe,"action" to obj("type" to "format","value" to "PngHdrMapped")).toString())).getJSONObject("recipe")}
        png("hdr-pq.png",clipped)
        val exr=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to recipe,"action" to obj("type" to "format","value" to "Exr")).toString())).getJSONObject("recipe")}
        val exrBytes=png("hdr-exact.exr",exr);assertArrayEquals(byteArrayOf(0x76,0x2f,0x31,0x01),exrBytes.copyOfRange(0,4))
        val recovery=File(files,"hdr-session");captureSession(recovery)
        native{Native.destroyGpuForTest(it)};compose.runOnUiThread{host.documentChanged()};compose.waitUntil(10_000){host.failure!=null}
        compose.runOnUiThread{host.restartCanvas()};compose.waitUntil(60_000){host.surfaceReady&&host.snapshot?.optBoolean("brush_ready")==true};ready()
        assertEquals(original,histogram());assertEquals(changed,form().getJSONObject("rendition").toString());assertEquals(hash(sdr),hash(png("hdr-recovered-sdr.png")))
        save("hdr-after-gpu-recovery.capy")
        val restore=restoreSessionTask(recovery)
        try{adoptSession(restore)}finally{Native.sessionFree(restore)}
        refresh();ready();assertEquals(original,histogram());assertTrue(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        scenario.recreate();scenario.onActivity{activity=it};compose.waitUntil(60_000){host.surfaceReady};refresh();ready();assertEquals(original,histogram())
        open(File(files,"hdr-exact.exr"));refresh();ready();val exact=JSONObject(histogram());assertEquals("F32",exact.getJSONObject("color").getString("depth"));assertTrue(exact.getJSONArray("channels").objects().any{it.getLong("below")>0})
        open(File(files,"hdr-pq.png"));refresh();ready();assertEquals("F16",JSONObject(histogram()).getJSONObject("color").getString("depth"))
        open(File(files,"hdr-sdr.png"));refresh();assertEquals("U8",JSONObject(histogram()).getJSONObject("color").getString("depth"))
        println("HDR PQ open; GTK picker; touch cancel/stylus SDR appearance; exact master/rendition save/reopen; HDR/SDR delivery; GPU, recovery and Activity recreation passed")
    }

    @Test fun screenStatusFlagsAndHighlightsClippedColors() {
        val output=File(activity.getExternalFilesDir(null),"screen-status").apply{deleteRecursively();mkdirs()}
        val automation=instrumentation.uiAutomation
        val evidence=JSONObject()
        fun shot(name:String):Int {
            refresh();SystemClock.sleep(300)
            val bitmap=automation.takeScreenshot()!!
            File(output,"$name.png").outputStream().use{bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)}
            val pixel=bitmap.getPixel(bitmap.width/2,bitmap.height/2)
            evidence.put(name,obj("pixel" to Integer.toHexString(pixel),"color_space" to bitmap.colorSpace?.name,"wide" to (bitmap.colorSpace?.isWideGamut==true)))
            bitmap.recycle()
            return pixel
        }
        fun screen()=native{state(it).getJSONObject("screen")}
        fun surface()=native{JSONObject(Native.displayStatus(it))}
        fun chip()=compose.onAllNodesWithTag("screen-status").fetchSemanticsNodes().firstOrNull()?.config
            ?.getOrElseNullable(androidx.compose.ui.semantics.SemanticsProperties.Text){null}?.joinToString()
        fun await(what:String,condition:()->Boolean)=try{compose.waitUntil(20_000){refresh();condition()}}
            catch(e:androidx.compose.ui.test.ComposeTimeoutException){throw AssertionError("$what: chip=${chip()} screen=${screen()}",e)}
        fun fill(space:String,rgba:List<Double>) {
            send(obj("type" to "color","action" to obj("op" to "set_slot","slot" to "foreground",
                "color" to obj("space" to space,"rgba" to org.json.JSONArray(rgba)))))
            invoke("select_all");invoke("fill_selection");invoke("deselect")
        }
        val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
        try {
            Native.projectOptions(task,obj("extent" to org.json.JSONArray(listOf(512,384)),"color" to obj("space" to "ProPhoto","depth" to "U8"),"background" to "White").toString())
            Native.projectWork(task,-1,512,384);native{Native.projectAdopt(it,task,"null")}
        } finally {Native.projectFree(task)}
        invoke("fit_canvas")
        compose.waitUntil(10_000){screen().getJSONObject("assessment").getString("basis")=="System"}
        val wideScreen=activity.resources.configuration.isScreenWideColorGamut
        val saturatedWidePanel=!wideScreen&&activity.getSystemService(android.hardware.display.DisplayManager::class.java).getDisplay(android.view.Display.DEFAULT_DISPLAY).isWideColorGamut&&
            ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("getprop persist.sys.sf.native_mode")).use{it.readBytes()}.decodeToString().trim()=="1"
        compose.waitUntil(10_000){refresh();surface().optBoolean("display_wide")==wideScreen}
        val status=surface()
        val p3Surface=wideScreen&&status.getJSONArray("formats").objects().any{
            it.getString("format")==status.getString("format")&&"DISPLAY_P3" in it.getString("color_spaces")}
        compose.waitUntil(10_000){refresh();surface().getString("color_space")==if(p3Surface)"DisplayP3" else "Srgb"}
        evidence.put("wide_screen",wideScreen).put("saturated_wide_panel",saturatedWidePanel).put("surface",surface())
        fill("ProPhoto",listOf(0.0,1.0,0.0,1.0))
        await("ProPhoto green clips"){chip()=="Colors clipped"}
        val green=shot("clipped")
        compose.onNodeWithTag("screen-status").performClick()
        compose.waitUntil(5_000){compose.onAllNodesWithTag("screen-details").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Some colors can’t be shown accurately on this screen").assertExists()
        shot("clipped-details")
        compose.onNodeWithTag("screen-highlight").performClick()
        compose.waitUntil(5_000){screen().getBoolean("show_clipped")}
        tick()
        val marked=shot("clipped-highlighted")
        assertTrue("Clipped green is on screen: ${Integer.toHexString(green)}",android.graphics.Color.green(green)>200&&android.graphics.Color.blue(green)<100)
        assertTrue("Highlighted pixels are blue: ${Integer.toHexString(marked)}",android.graphics.Color.blue(marked)>200&&android.graphics.Color.red(marked)<80)
        compose.onNodeWithTag("screen-highlight").performClick()
        compose.waitUntil(5_000){!screen().getBoolean("show_clipped")}
        fill("DisplayP3",listOf(0.0,1.0,0.0,1.0))
        if(p3Surface) {
            await("Display P3 green fits"){screen().optBoolean("clipped",true)==false&&chip()==null}
            val p3=shot("display-p3-green")
            File(output,"display-p3-surfaceflinger.txt").writeBytes(ParcelFileDescriptor.AutoCloseInputStream(
                automation.executeShellCommand("dumpsys SurfaceFlinger")).use{it.readBytes()})
            if(evidence.getJSONObject("display-p3-green").getBoolean("wide"))
                assertTrue("Display P3 green reaches the screen unclipped: ${Integer.toHexString(p3)}",android.graphics.Color.red(p3)<60&&android.graphics.Color.green(p3)>230)
        } else {
            await("Display P3 green clips on sRGB"){chip()=="Colors clipped"}
            shot("display-p3-green")
            val body=screen().getJSONObject("details").optString("body")
            if(saturatedWidePanel)assertTrue("The color setting is named: $body",body.startsWith("Your operating system is limiting apps to sRGB colors on this screen"))
        }
        fill("DisplayP3",listOf(0.5,0.5,0.5,1.0))
        await("Gray fits"){screen().optBoolean("clipped",true)==false&&chip()==null}
        shot("fits")
        File(output,"evidence.json").writeText(evidence.toString(2))
    }
    @Test fun hdrDisplayNegotiation() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile") ?: throw AssumptionViolatedException("Supply -e hdrFile for the display regression")
        val automation=instrumentation.uiAutomation
        fun shell(command:String)=ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use{it.readBytes()}
        val output=File(activity.getExternalFilesDir(null),"display").apply{mkdirs()}
        val input=File(files,"display-hdr.png").apply{writeBytes(shell("cat $sourcePath"))}
        fun tone()=native{JSONObject(Native.toneStatus(it))}
        fun mode(value:String){native{Native.proofControl(it,obj("type" to "mode","mode" to value).toString())};compose.runOnUiThread{host.documentChanged()};tick()}
        fun surfacePixels(name:String):JSONObject {
            // Navigator placement arrives from Compose layout on a different
            // queue from tone publication. Drain both before reading pixels.
            compose.waitForIdle()
            compose.waitUntil(10_000) { !tick() }
            // Preserve HDR values: read the shared image or redraw the current
            // view into a newly acquired FIFO image using the same presenter.
            val status=native{JSONObject(Native.displayStatus(it))}
            assertTrue(status.getString("present_mode") in listOf("SharedDemandRefresh", "Fifo"))
            val format=status.getString("format")
            val hdr=status.getString("color_space")=="Bt2100Pq"
            val extent=status.getJSONArray("extent")
            val physicalWidth=extent.getInt(0);val physicalHeight=extent.getInt(1)
            val turns=status.getInt("surface_quarter_turns")
            val width=if(turns%2==0)physicalWidth else physicalHeight
            val height=if(turns%2==0)physicalHeight else physicalWidth
            val f16=format=="Rgba16Float"
            val bytes=ByteBuffer.wrap(native{Native.surfacePixelsForTest(it)}).order(ByteOrder.LITTLE_ENDIAN)
            fun channel(x:Int,y:Int,c:Int):Float {
                val (px,py)=when(turns){1->height-1-y to x;2->width-1-x to height-1-y;3->y to width-1-x;else->x to y}
                val offset=(py*physicalWidth+px)*(if(f16)8 else 4)
                val v=if(f16)android.util.Half.toFloat(bytes.getShort(offset+c*2)).toDouble()
                    else (bytes.get(offset+(if(format.startsWith("Bgra"))2-c else c)).toInt() and 255)/255.0
                if(!hdr)return v.toFloat()
                val p=Math.pow(v,32.0/2523.0)
                return (10000.0/203.0*Math.pow(maxOf(p-3424.0/4096.0,0.0)/(2413.0/128.0-2392.0/128.0*p),16384.0/2610.0)).toFloat()
            }
            fun range(left:Int,top:Int,right:Int,bottom:Int):JSONObject {
                var low=Float.POSITIVE_INFINITY;var high=Float.NEGATIVE_INFINITY
                var above=0;var count=0
                var digest=1469598103934665603L
                for(y in top.coerceAtLeast(0) until bottom.coerceAtMost(height) step 3)
                    for(x in left.coerceAtLeast(0) until right.coerceAtMost(width) step 3){
                        for(v in (0..2).map{channel(x,y,it)}){low=minOf(low,v);high=maxOf(high,v);if(v>1.001f)above++;count++;digest=(digest xor v.toRawBits().toLong())*1099511628211L}
                    }
                return obj("min" to low,"max" to high,"above_sdr" to above,"samples" to count,"digest" to digest.toString())
            }
            val nav=compose.onNodeWithTag("navigator-overview").fetchSemanticsNode().boundsInRoot.translate(-host.surfaceOrigin)
            val pixels=obj("capture" to "retained-surface-readback",
                "format" to format,"color_space" to status.getString("color_space"),
                "canvas" to range(width/3,height/3,width*2/3,height*2/3),
                "navigator" to range(nav.left.toInt()+3,nav.top.toInt()+3,nav.right.toInt()-3,nav.bottom.toInt()-3))
            File(output,"$name-pixels.json").writeText(pixels.toString(2));return pixels
        }
        fun record(name:String){
            File(output,"$name.json").writeText(tone().put("surface",native{JSONObject(Native.displayStatus(it))}).toString(2))
            File(output,"$name-display.txt").writeBytes(shell("dumpsys display"))
            File(output,"$name-surfaceflinger.txt").writeBytes(shell("dumpsys SurfaceFlinger"))
            automation.takeScreenshot()?.let{shot->File(output,"$name.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        }
        open(input);mode("off")
        compose.waitUntil(30_000){tone().getBoolean("ready")}
        compose.waitUntil(5_000){native{JSONObject(Native.displayStatus(it)).optLong("presented_tone_publications",-1)}==tone().getLong("publications")}
        val supports=tone().getBoolean("display_hdr")
        fun sdrSpace(s:JSONObject)=if(s.optBoolean("display_wide")&&s.getJSONArray("formats").objects()
            .firstOrNull{it.getString("format").endsWith("Srgb")}?.getString("color_spaces")?.contains("DISPLAY_P3")==true)"DisplayP3" else "Srgb"
        fun awaitSurface(hdr:Boolean) {
            compose.waitUntil(10_000){native{JSONObject(Native.displayStatus(it)).let{s->
                s.opt("presented_hdr")==hdr&&s.optString("color_space")==if(hdr)"Bt2100Pq" else sdrSpace(s)
            }}}
            val surface=native{JSONObject(Native.displayStatus(it))}
            assertTrue(surface.getString("present_mode") in listOf("SharedDemandRefresh", "Fifo"))
            val expected=if(hdr)"Rgba16Float" else surface.getJSONArray("formats").objects()
                .firstOrNull{it.getString("format").endsWith("Srgb")}?.getString("format")
            if(expected!=null)assertEquals("Presentation format follows the output mode",expected,surface.getString("format"))
        }
        awaitSurface(supports)
        record("hdr-off")
        val actualPixels=surfacePixels("actual-display")
        if(InstrumentationRegistry.getArguments().getString("requireHdr")=="true")assertTrue("Expected a negotiated HDR surface",supports)
        if(supports) {
            for(region in listOf("canvas","navigator"))assertTrue("PQ $region contains HDR: $actualPixels",actualPixels.getJSONObject(region).getDouble("max")>1.0)
            val revision=native{state(it).getJSONObject("document_file").getLong("revision")}
            // A display move/lost HDR capability must restore SDR, then the same HDR pixels.
            compose.runOnUiThread{host.displayInfo(false)}
            awaitSurface(false)
            val fallback=surfacePixels("sdr-display-fallback")
            for(region in listOf("canvas","navigator")) {
                val range=fallback.getJSONObject(region)
                assertTrue(range.getDouble("max")<=1.001)
                assertTrue("SDR $region has visible image content: $fallback",range.getDouble("max")-range.getDouble("min")>0.05)
            }
            compose.runOnUiThread{host.displayInfo(true)}
            awaitSurface(true)
            val restored=surfacePixels("hdr-display-restored")
            for(region in listOf("canvas","navigator"))assertEquals("Display restore preserves $region",actualPixels.getJSONObject(region).getString("digest"),restored.getJSONObject(region).getString("digest"))
            assertEquals(revision,native{state(it).getJSONObject("document_file").getLong("revision")})
            compose.waitUntil(5_000){compose.onAllNodesWithText("HDR").fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("screen-status").assertTextEquals("HDR")
        }
        val info=compose.onNodeWithTag("screen-status").fetchSemanticsNode().boundsInRoot
        val zoom=compose.onNodeWithTag("camera-readout").fetchSemanticsNode().boundsInRoot
        assertTrue("Screen status belongs on the left",info.right<zoom.left)
        assertEquals("Matching footer bubble height",zoom.height,info.height,1f)
        compose.onNodeWithTag("screen-status").performClick()
        compose.waitUntil(5_000){compose.onAllNodesWithTag("screen-details").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText(if(supports)"Showing HDR" else "Showing the SDR version").assertExists()
        record("screen-details")
        compose.onNodeWithTag("screen-status").performClick()
        mode("sdr");awaitSurface(false);record("sdr-proof")
        val sdrPixels=surfacePixels("sdr-proof")
        awaitSurface(false)
        for(region in listOf("canvas","navigator")) {
            assertTrue("SDR proof is bounded: $sdrPixels",sdrPixels.getJSONObject(region).getDouble("max")<=1.001)
            if(supports)assertNotEquals("Proof changes $region",actualPixels.getJSONObject(region).getString("digest"),sdrPixels.getJSONObject(region).getString("digest"))
        }
        mode("print");awaitSurface(false);record("print-proof")
        mode("off");awaitSurface(supports)
        compose.runOnUiThread{host.restartCanvas()}
        compose.waitUntil(60_000){host.failure!=null||host.snapshot?.optBoolean("brush_ready")==true}
        assertNull(host.failure)
        awaitSurface(supports)
        record("hdr-recovered")
        assertEquals(supports,tone().getBoolean("display_hdr"))
    }

    @Test fun proofWorkspaceDragCancelAndDrawer() {
        val source=InstrumentationRegistry.getArguments().getString("hdrFile")
        Assume.assumeTrue("Supply -e hdrFile",source!=null)
        val automation=instrumentation.uiAutomation
        val file=File(files,"workspace-hdr.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $source")).use{it.readBytes()})}
        open(file);tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
        fun edit(value:JSONObject){compose.runOnUiThread{host.customize(value)};compose.waitForIdle()}
        fun group()=host.snapshot!!.getJSONObject("layout").getJSONArray("groups").objects().first{it.getJSONArray("panels").values().contains("proof")}
        fun recipe()=native{JSONObject(Native.query(it,obj("type" to "proof_form").toString())).getJSONObject("rendition").toString()}
        action("sdr_rendition")
        compose.waitUntil(10_000){compose.onAllNodesWithTag("sdr-tone-pad").fetchSemanticsNodes().isNotEmpty()}
        val original=group().getInt("id");val appearance=recipe()
        // A workspace tab starts moving after slop, without a touch hold.
        compose.onNodeWithTag("tab-proof").performTouchInput {down(center);moveTo(center+androidx.compose.ui.geometry.Offset(24f,-24f),16);moveTo(center+androidx.compose.ui.geometry.Offset(550f,-320f),64);up()}
        compose.waitUntil(10_000){group().optBoolean("floating")}
        action("undo_workspace");compose.waitUntil(10_000){group().getInt("id")==original&&!group().getBoolean("floating")}
        action("redo_workspace");compose.waitUntil(10_000){group().getBoolean("floating")}
        val before=native{state(it).getJSONObject("workspace").getJSONObject("layout").toString()}
        compose.onNodeWithTag("tab-proof").performTouchInput {down(center);moveTo(center+androidx.compose.ui.geometry.Offset(80f,60f),64);cancel()}
        compose.waitForIdle();assertEquals(before,native{state(it).getJSONObject("workspace").getJSONObject("layout").toString()})
        action("undo_workspace");compose.waitUntil(10_000){group().getInt("id")==original&&!group().getBoolean("floating")}
        edit(obj("type" to "set_column_collapsed","group" to original,"collapsed" to true))
        compose.waitUntil(10_000){host.snapshot!!.getJSONObject("layout").getJSONArray("collapsed").objects().any{it.getJSONArray("groups").objects().any{g->g.getInt("group")==original}}}
        val column=host.snapshot!!.getJSONObject("layout").getJSONArray("collapsed").objects().first{it.getJSONArray("groups").objects().any{g->g.getInt("group")==original}}.getInt("id")
        edit(obj("type" to "set_column_drawers","column" to column,"drawers" to true))
        action("sdr_rendition")
        compose.waitUntil(10_000){compose.onAllNodesWithTag("sdr-tone-pad").fetchSemanticsNodes().isNotEmpty()}
        fun drawers()=native{state(it).getJSONObject("customization").getJSONArray("column_drawers").length()}
        compose.waitUntil(10_000){drawers()==1};action("sdr_rendition");assertEquals(1,drawers())
        compose.onNodeWithTag("sdr-tone-pad").performTouchInput{down(center);moveTo(center+androidx.compose.ui.geometry.Offset(30f,-20f));cancel()}
        compose.waitForIdle();assertEquals(appearance,recipe())
        automation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),"hdr-proof-drawer.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        edit(obj("type" to "set_column_collapsed","group" to original,"collapsed" to false))
        compose.waitUntil(10_000){compose.onAllNodesWithTag("tab-proof").fetchSemanticsNodes().isNotEmpty()}
        assertEquals(appearance,recipe());assertNull(host.actionError)
        println("Proof workspace touch tab drag, one-step layout history, cancellation and collapsed drawer passed")
    }

    @Test fun proofSetupCompareEditPortabilityExportAndRecovery() {
        val profilePath=InstrumentationRegistry.getArguments().getString("proofProfile")
        Assume.assumeTrue("Supply -e proofProfile /data/local/tmp/capy-proof-cmyk.icc",profilePath!=null)
        val targetBytes=ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand("cat $profilePath")).use{it.readBytes()}
        val target=runBlocking{ProfileStore.import(activity,targetBytes)}
        fun hide(){compose.runOnUiThread{host.customize(obj("type" to "set_panel_visible","panel" to "proof","visible" to false))};compose.waitForIdle()}
        fun cancel(){compose.onNodeWithTag("proof-mode-off").performScrollTo().performClick();hide()}
        var selectedName=""
        fun setup(command:String="soft_proof_setup") {
            action(command);compose.waitUntil(10_000){compose.onAllNodesWithText("Proof").fetchSemanticsNodes().isNotEmpty()}
            compose.waitUntil(10_000){compose.onAllNodes(hasTestTag("proof-mode-print") and isEnabled()).fetchSemanticsNodes().isNotEmpty()}
            compose.onNodeWithTag("proof-mode-print").performScrollTo().performClick()
            compose.waitUntil(10_000){compose.onAllNodesWithTag("proof-profile").fetchSemanticsNodes().isNotEmpty()}
        }
        fun pick(name:String){
            selectedName=name
            compose.onNodeWithTag("proof-profile").performScrollTo().performClick()
            compose.waitUntil(10_000){compose.onAllNodes(hasText(name) and hasAnyAncestor(hasTestTag("proof-profile-picker"))).fetchSemanticsNodes().isNotEmpty()}
            compose.onAllNodes(hasText(name) and hasAnyAncestor(hasTestTag("proof-profile-picker"))).onFirst().performScrollTo().performClick()
            compose.waitUntil(10_000){compose.onAllNodesWithTag("proof-profile-picker").fetchSemanticsNodes().isEmpty()}
            compose.onNodeWithTag("proof-profile").assertTextContains(name)
        }
        fun apply(){
            if(host.proof.error!=null)pick(selectedName)
            compose.waitUntil(120_000){!host.proof.busy&&native{JSONObject(Native.query(it,obj("type" to "proof_form").toString())).getJSONObject("recipe").getString("name")}==selectedName&&native{JSONObject(Native.query(it,obj("type" to "proof_form").toString())).getJSONObject("recipe").getJSONObject("profile").has("Icc")}}
            assertNull(host.proof.error);hide()
        }
        fun current()=native{JSONObject(Native.query(it,obj("type" to "proof_form").toString())).getJSONObject("recipe")}
        fun status()=native{JSONObject(Native.query(it,obj("type" to "proof_status").toString()))}
        fun hist(): String {
            val control = Native.captureControl()
            try {
                val task = native { Native.inspectionTask(it, control) }
                return JSONObject(Native.inspectionStatistics(task, sourceVisible, false, false, false))
                    .getJSONObject("histogram").toString()
            } finally { Native.captureFree(control) }
        }
        // Real first-use dialog, cancellation, sensible defaults.
        setup("soft_proof");assertFalse(native{state(it).getBoolean("soft_proof")})
        compose.onNodeWithText("Black ink").assertExists()
        compose.onNodeWithText("Choose Profile…").assertExists();SystemClock.sleep(400);cancel()
        assertTrue(native{JSONObject(Native.query(it,obj("type" to "proof_form").toString())).isNull("document_profile")})
        // Obtain a portable RGB ICC through the real profiled file pipeline.
        val wide=builtinRecipe(1)
        png("proof-original.png",wide);open(File(files,"proof-original.png"))
        val profiles=native{JSONObject(Native.query(it,obj("type" to "export_form").toString())).getJSONArray("profiles")}
        val original=profiles.objects().first{it.getJSONObject("profile").has("Icc")}
        val array=original.getJSONObject("profile").getJSONArray("Icc")
        val originalBytes=ByteArray(array.length()){array.getInt(it).toByte()}
        val embedded=runBlocking{ProfileStore.import(activity,originalBytes)}
        compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
        setup();pick(embedded.getString("name"))
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread{host.invoke("export_document")}
        compose.waitUntil(120_000){compose.onAllNodesWithTag("export-choose-file").fetchSemanticsNodes().isNotEmpty()}
        assertEquals(embedded.getString("name"),current().getString("name"))
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000){!native{state(it).getJSONObject("document_file").getBoolean("busy")}}
        DocumentController.nativeFileJobsForTest=true
        compose.waitForIdle();SystemClock.sleep(300)
        instrumentation.uiAutomation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),"proof-print.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        apply()
        assertTrue("The saved ICC, not the same-named builtin, must be selected",current().getJSONObject("profile").has("Icc"))
        val originalEntry=runBlocking{ProfileStore.list(activity).first{it.getString("name")==embedded.getString("name")}}
        runBlocking{ProfileStore.remove(activity,originalEntry.getString("id"))}
        val baseline=manifest(save("proof-original.capy"))
        setup();compose.onNodeWithTag("proof-profile").performScrollTo().performClick()
        compose.onNodeWithText("Document Profile").assertExists()
        compose.onAllNodesWithText(embedded.getString("name")).onFirst().assertExists()
        compose.onNodeWithText("Done").performClick();pick(target.getString("name"));cancel()
        assertFalse(runBlocking{ProfileStore.list(activity).any{it.getString("name")==embedded.getString("name")}})
        assertEquals(baseline.outputData().getJSONObject("proof").toString(),manifest(save("proof-cancel.capy")).outputData().getJSONObject("proof").toString())
        // An unwritable library path fails before document/history publication.
        val oldDirectory=AppStorage.directoryForTest
        val blocked=File(files,"proof-blocked-${System.nanoTime()}").apply{writeText("not a directory")}
        AppStorage.directoryForTest=blocked
        setup()
        // Select through the standard/profile callback with the already loaded ICC;
        // changing the library path prevents the picker from resolving its entry.
        compose.runOnUiThread{host.proof.edit("profile",target)}
        compose.onNodeWithTag("proof-profile").assertTextContains(target.getString("name"))
        println("Preparing replacement with blocked local profile storage")
        compose.waitUntil(120_000){!host.proof.busy&&(host.proof.error!=null||compose.onAllNodesWithText("Proof").fetchSemanticsNodes().isEmpty())}
        assertNotNull("Preservation must fail before replacing ${current().getString("name")}",host.proof.error)
        assertEquals(embedded.getString("name"),current().getString("name"))
        AppStorage.directoryForTest=oldDirectory
        apply()
        val preserved=runBlocking{ProfileStore.list(activity).first{it.getString("name")==embedded.getString("name")}}
        assertEquals(array.toString(),runBlocking{ProfileStore.get(activity,preserved.getString("id"))}.getJSONObject("profile").getJSONArray("Icc").toString())
        val replacement=manifest(save("proof-replacement.capy"))
        assertEquals(target.getString("name"),replacement.outputData().getJSONObject("proof").getString("name"))
        assertEquals(baseline.rasterResources().toString(),replacement.rasterResources().toString())
        println("Proof UI first use, cancel, Document Profile retention, preservation failure/retry and exact original ICC copy passed")
        native{Native.dispatch(it,obj("type" to "select_brush","id" to 1).toString());Native.dispatch(it,obj("type" to "set_color","rgba" to org.json.JSONArray(listOf(1.0,0.0,.7,1.0))).toString())}
        val before=hist();stroke(0.0);val painted=hist();assertNotEquals(before,painted)
        action("undo");tick();assertEquals(before,hist());action("redo");tick();assertEquals(painted,hist())
        val master=save("proof-painted.capy")
        val on=png("proof-on.png")
        action("gamut_warning");action("soft_proof");tick()
        assertFalse(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        assertEquals(painted,hist());assertEquals(hash(on),hash(png("proof-warning.png")))
        assertFalse(native{state(it).getBoolean("gamut_warning")});tick();assertEquals("",status().getString("text"));assertEquals(hash(on),hash(png("proof-off.png")))
        // Removing both local entries models moving the file to another machine.
        runBlocking{ProfileStore.list(activity).forEach{ProfileStore.remove(activity,it.getString("id"))}}
        open(File(files,"proof-painted.capy"));compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
        assertFalse(native{state(it).getBoolean("soft_proof")||state(it).getBoolean("gamut_warning")})
        assertEquals("",status().getString("text"));assertEquals(target.getString("name"),current().getString("name"))
        assertEquals(manifest(master).rasterResources().toString(),manifest(save("proof-reopened.capy")).rasterResources().toString())
        action("soft_proof");compose.waitUntil(120_000){status().getString("text").startsWith("Proof:")}
        assertEquals(painted,hist())
        compose.runOnUiThread{host.restartCanvas()};compose.waitUntil(60_000){host.snapshot?.optBoolean("brush_ready")==true&&host.failure==null}
        tick();assertEquals(painted,hist());assertEquals(hash(on),hash(png("proof-recovered.png")))
        scenario.recreate();scenario.onActivity{activity=it};compose.waitUntil(60_000){host.snapshot?.optBoolean("brush_ready")==true};tick()
        assertEquals(target.getString("name"),current().getString("name"));assertEquals(painted,hist())
        assertNull(host.failure);assertNull(host.actionError)
        println("Proofed editing/history, clean viewing toggles, exact histogram/export, portable save/reopen and GPU/Activity replacement passed")
        // Cancel while the native CPU worker is preparing, then reject a fully
        // prepared result whose dialog request has been dismissed.
        val retained=current().toString()
        repeat(2){case->
            setup()
            val flag=Native.captureControl()
            val task=native{h->Native.dispatch(h,obj("type" to "invoke","command" to "soft_proof_setup").toString());val request=state(h).getJSONArray("requests").objects().first{it.getJSONObject("kind").getString("type")=="soft_proof_setup"}.getInt("id");Native.proofTask(h,request,retained,flag)}
            try{
                if(case==0){
                    val failure=java.util.concurrent.atomic.AtomicReference<Throwable?>()
                    val started=java.util.concurrent.CountDownLatch(1)
                    val worker=kotlin.concurrent.thread{started.countDown();try{Native.proofWork(task)}catch(e:Throwable){failure.set(e)}}
                    assertTrue(started.await(5,java.util.concurrent.TimeUnit.SECONDS))
                    SystemClock.sleep(20);Native.captureCancel(flag);worker.join(30_000)
                    assertFalse("Cancelled proof worker must stop",worker.isAlive)
                    assertNotNull("Cancellation must reject preparation",failure.get())
                }else Native.proofWork(task)
                hide()
                assertTrue("Dismissed request must reject prepared results",runCatching{native{Native.proofCheck(it,task)}}.isFailure)
                assertEquals(retained,current().toString())
                assertTrue(runBlocking{ProfileStore.list(activity).isEmpty()})
            }finally{Native.proofRelease(task);Native.captureFree(flag)}
        }
        println("Native preparation cancellation and stale-result rejection passed")
        InstrumentationRegistry.getArguments().getString("proofPortableFile")?.let{path->
            val bytes=ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand("cat $path")).use{it.readBytes()}
            val portable=File(files,"proof-from-web.capy").apply{writeBytes(bytes)}
            open(portable);compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
            assertTrue(runBlocking{ProfileStore.list(activity).isEmpty()})
            assertEquals(target.getJSONObject("profile").toString(),current().getJSONObject("profile").toString())
            assertEquals("",status().getString("text"))
            val plain=png("web-portable-normal.png");val exact=hist()
            action("soft_proof");compose.waitUntil(120_000){status().getString("text").startsWith("Proof:")}
            assertEquals(exact,hist());assertEquals(hash(plain),hash(png("web-portable-proof.png")))
            val archive=manifest(save("proof-from-web-resaved.capy"))
            assertEquals(1,archive.resourcesOf("capy.icc/1").length())
            assertEquals("display_p3",archive.compositionColor().optString("space","srgb"))
            println("Web-created P3/U16 file opened, proofed, resaved and exported on Android without installed profiles")
        }
    }

    private fun summary(values: org.json.JSONArray): JSONObject? {
        if(values.length()==0)return null
        val sorted=(0 until values.length()).map {values.getDouble(it)}.sorted()
        return obj("count" to sorted.size,"p50" to sorted[((sorted.size-1)*.5).toInt()],"p95" to sorted[((sorted.size-1)*.95).toInt()],"p99" to sorted[((sorted.size-1)*.99).toInt()],"max" to sorted.last())
    }
    private fun motion(tool: Int, steps: Int, center: Pair<Double, Double> = 1000.0 to 750.0, during: (() -> Unit)? = null): JSONObject {
        fun shell(command: String) = ParcelFileDescriptor.AutoCloseInputStream(
            instrumentation.uiAutomation.executeShellCommand(command)
        ).use { it.readBytes().decodeToString() }
        // Read the published state; Native.snapshot would consume the
        // publication before Compose can receive it.
        compose.waitForIdle()
        val camera=host.snapshot!!.getJSONObject("state").getJSONObject("camera");val zoom=camera.getDouble("zoom");val translation=camera.getJSONArray("translation")
        var origin=androidx.compose.ui.geometry.Offset.Zero
        scenario.onActivity { origin=host.surfaceOrigin }
        val cx=(center.first*zoom+translation.getDouble(0)+origin.x).toFloat();val cy=(center.second*zoom+translation.getDouble(1)+origin.y).toFloat()
        if(steps>=180) {
            val output=File(activity.getExternalFilesDir(null),"motion-start-$tool")
            output.resolveSibling(output.name+".json").writeText(obj("camera" to camera,"x" to cx,"y" to cy).toString(2))
            instrumentation.uiAutomation.takeScreenshot()?.let{shot->output.resolveSibling(output.name+".png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        }
        val start=SystemClock.uptimeMillis();val source=when(tool){android.view.MotionEvent.TOOL_TYPE_MOUSE->android.view.InputDevice.SOURCE_MOUSE;android.view.MotionEvent.TOOL_TYPE_FINGER->android.view.InputDevice.SOURCE_TOUCHSCREEN;else->android.view.InputDevice.SOURCE_STYLUS}
        host.measurementReport(true)
        val duration = if (steps >= 180) InstrumentationRegistry.getArguments()
            .getString("motionDurationMs")?.toLong()?.coerceIn(5_000L, 30_000L) ?: 5_000L else 1_000L
        var i = 0
        while (true) {
            val elapsed = SystemClock.uptimeMillis() - start
            val phase=when {i==0->android.view.MotionEvent.ACTION_DOWN;elapsed>=duration->android.view.MotionEvent.ACTION_UP;else->android.view.MotionEvent.ACTION_MOVE}
            val properties=arrayOf(android.view.MotionEvent.PointerProperties().apply {id=7;toolType=tool})
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply {
                x=cx+40*kotlin.math.sin(elapsed/250.0).toFloat()
                y=cy+20*kotlin.math.cos(elapsed/310.0).toFloat()
                pressure=if(phase==android.view.MotionEvent.ACTION_UP)0f else .65f
            })
            val buttons=if(tool==android.view.MotionEvent.TOOL_TYPE_MOUSE&&phase!=android.view.MotionEvent.ACTION_UP)android.view.MotionEvent.BUTTON_PRIMARY else 0
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),phase,1,properties,coords,0,buttons,1f,1f,0,0,source,0)
            try {assertTrue("Injected tool=$tool phase=$phase at (${coords[0].x}, ${coords[0].y})",instrumentation.uiAutomation.injectInputEvent(event,phase==android.view.MotionEvent.ACTION_UP))}finally{event.recycle()}
            if (phase == android.view.MotionEvent.ACTION_UP) break
            if (i == 10) during?.invoke()
            i++; SystemClock.sleep(4)
        }
        if(tool==android.view.MotionEvent.TOOL_TYPE_STYLUS) {
            // Finish virtual pen proximity before the next independent touch run.
            val properties=arrayOf(android.view.MotionEvent.PointerProperties().apply{id=7;toolType=tool})
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply{x=cx;y=cy;pressure=0f})
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),android.view.MotionEvent.ACTION_HOVER_EXIT,1,properties,coords,0,0,1f,1f,0,0,source,0)
            try{instrumentation.uiAutomation.injectInputEvent(event,true)}finally{event.recycle()}
        }
        // The host's Choreographer is the only frame producer during
        // motion. Capture its timeline independently of GPU timings.
        SystemClock.sleep(100)
        native { Unit } // Drain delivered input, without creating a frame.
        val timeline = host.measurementReport(false)
        assertNull(host.failure)
        if (timeline.getJSONArray("inputs").length() == 0) {
            instrumentation.uiAutomation.takeScreenshot()?.let { screenshot ->
                try { File(activity.getExternalFilesDir(null), "image-placement-input-missing.png").outputStream().use { screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                finally { screenshot.recycle() }
            }
        }
        assertTrue("The canvas received input at ($cx,$cy), camera=$camera; state=${host.snapshot?.getJSONObject("state")?.getJSONObject("layer_tools")}", timeline.getJSONArray("inputs").length() > 0)
        val layer = shell("dumpsys SurfaceFlinger --list").lineSequence()
            .firstOrNull { it.contains("SurfaceView[${activity.packageName}/") && it.contains("(BLAST)") }
            ?.removePrefix("RequestedLayerState{")?.substringBefore(" parentId=")
        // UiAutomation executes an argument vector; shell quote marks
        // would become part of this layer name (which has no spaces).
        val latency = layer?.let { shell("dumpsys SurfaceFlinger --latency $it") }
        val stats=native { JSONObject(Native.query(it,obj("type" to "renderer_stats").toString())) }

        return obj("tool" to tool,"cpu_ms" to summary(stats.getJSONArray("samples")),"gpu_ms" to summary(stats.getJSONArray("gpu_samples")),
            "camera" to camera, "input_origin" to org.json.JSONArray(listOf(cx,cy)),
            "timeline" to timeline, "surface_layer" to layer, "surface_latency" to latency, "renderer_stats" to stats, "failure" to host.failure,
            "tracked_canvas_bytes" to stats.getLong("resident_bytes"),"process_pss_bytes" to android.os.Debug.getPss().toLong()*1024,
            "process_mappings" to File("/proc/self/maps").useLines { it.count() })
    }

    @Test fun imagePlacementBatchHistoryAndStaleRequests() {
        val affineSmoke = InstrumentationRegistry.getArguments().getString("imagePlacementAffineSmoke") == "true"
        fun invoke(command: String) { native { Native.dispatch(it,obj("type" to "invoke","command" to command).toString()) }; tick(); scenario.onActivity { host.documentChanged() } }
        native { Native.dispatch(it,obj("type" to "preferences","action" to obj("type" to "edit","id" to "missing_profile","value" to 0)).toString()) }
        val newTask = native { h -> val (id, f) = request(h,"new_document"); Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision")) }
        try { Native.projectWork(newTask,-1,2000,1500); native { Native.projectAdopt(it,newTask,"null") } } finally { Native.projectFree(newTask) }
        scenario.onActivity { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        val configured = InstrumentationRegistry.getArguments().getString("imagePlacementPhotos")
        val photos = configured?.split(',')?.map { File(activity.filesDir,it) } ?: listOf(3000 to 2400,800 to 600).mapIndexed { i,(w,h) ->
            File(files,"placement-$i.png").also { file ->
                val bitmap = android.graphics.Bitmap.createBitmap(w,h,android.graphics.Bitmap.Config.ARGB_8888)
                val canvas = android.graphics.Canvas(bitmap)
                val paint = android.graphics.Paint().apply { shader = android.graphics.LinearGradient(0f,0f,w.toFloat(),h.toFloat(),android.graphics.Color.RED,android.graphics.Color.BLUE,android.graphics.Shader.TileMode.CLAMP) }
                canvas.drawRect(0f,0f,w.toFloat(),h.toFloat(),paint)
                file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it) }; bitmap.recycle()
            }
        }
        fun count() = native { state(it).array("layers").length() }
        fun batch(inputs: List<File>, afterRead: ((Long,Int,Long)->Unit)? = null) {
            val control = Native.captureControl()
            val (task,id) = native { h ->
                val (id,_) = request(h,"import_image")
                Native.imageImportTask(h,id,Native.imageImportContext(h,"null","null"),control) to id
            }
            try {
                for (file in inputs) Native.imageImportRead(task,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),file.name)
                if (afterRead == null) native { Native.imageImportAdopt(it,task) } else afterRead(task,id,control)
            } catch (e: Exception) { native { Native.documentComplete(it,id,false,"null") }; throw e }
            finally { Native.imageImportFree(task); Native.captureFree(control) }
            tick(); scenario.onActivity { host.documentChanged() }
        }
        fun press(command: String) = pressCanvasBar(command, 20_000)
        fun memoryStage(label: String) {
            val stats = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
            val maps = File("/proc/self/maps").readLines()
            val directory = File(activity.getExternalFilesDir(null), "validation/pixel-bake/maps").apply { mkdirs() }
            val name = label.replace(Regex("[^A-Za-z0-9]+"), "-")
            File(directory, "$name.txt").writeText(maps.joinToString("\n"))
            val counts = maps.groupingBy { it.trim().split(Regex("\\s+"), limit = 6).getOrNull(5) ?: "[anonymous]" }.eachCount()
            File(directory, "$name.json").writeText(JSONObject(counts).toString(2))
            android.util.Log.i("CapyPlacementTest", "$label: pss=${android.os.Debug.getPss()} KiB; mappings=${File("/proc/self/maps").useLines { it.count() }}; canvas=${stats.getLong("resident_bytes")} bytes; status=${File("/proc/self/status").readLines().filter { it.startsWith("Vm") }}")
        }
        memoryStage("before batch")
        val baseCount=count()
        batch(photos);assertEquals(baseCount+photos.size,count());memoryStage("provisional batch")
        if (!affineSmoke) {
            assertEquals("Recovery defers while a placement is provisional", 0L, selectedSessionCapture())
            press("cancel_transform");assertEquals(baseCount,count())
            batch(photos)
        }
        press("apply_transform");memoryStage("applied batch")
        val fitted=manifest(save("batch-placement.capy"));val identity=sourceIdentity(fitted)
        val images=fitted.originalImages()
        for(i in photos.indices) {
            val extent=images.getJSONObject(i).getJSONArray("extent");val w=extent.getDouble(0);val h=extent.getDouble(1);val scale=minOf(1.0,2000/w,1500/h)
            val pose=fitted.occurrenceRecords().getJSONObject(i).getJSONObject("data").authoredAffine()
            assertEquals(scale,pose.getDouble(0),1e-6);assertEquals(scale,pose.getDouble(3),1e-6)
            assertEquals((2000-w*scale)/2,pose.getDouble(4),.01);assertEquals((1500-h*scale)/2,pose.getDouble(5),.01)
        }
        if (!affineSmoke) {
        invoke("undo");assertEquals(baseCount,count());invoke("redo");assertEquals(baseCount+photos.size,count())
        memoryStage("before reopen")
        open(File(files,"batch-placement.capy"));memoryStage("after reopen");assertEquals(identity,sourceIdentity(manifest(save("batch-reopened.capy"))))
        invoke("scale_rotate");press("placement_original_size");press("apply_transform")
        val originalSize=manifest(save("batch-original-size.capy"))
        memoryStage("after original size")
        assertEquals(1.0,originalSize.occurrenceRecords().getJSONObject(0).getJSONObject("data").authoredAffine().getDouble(0),1e-6)
        assertEquals(identity,sourceIdentity(originalSize))
        val before=count();val malformed=File(files,"batch-malformed.png").apply { writeText("not a photo") }
        try { batch(listOf(photos.first(),malformed)); fail("Malformed second file was accepted") } catch (_: Exception) { assertEquals(before,count()) }
        batch(listOf(photos.first())) { task,id,control ->
            Native.captureCancel(control)
            try { native { Native.imageImportAdopt(it,task) }; fail("Cancelled batch adopted") } catch (_: Exception) { assertEquals(before,count()) }
            native { Native.documentComplete(it,id,false,"null") }
        }
        batch(listOf(photos.first())) { task,id,_ ->
            native { h -> val last=state(h).array("layers").objects().last().getLong("id"); Native.dispatch(h,obj("type" to "layer","action" to obj("op" to "select","id" to last,"mask" to false)).toString()) }
            try { native { Native.imageImportAdopt(it,task) }; fail("Stale target adopted") } catch (_: Exception) { assertEquals(before,count()) }
            native { Native.documentComplete(it,id,false,"null") }
        }
        assertEquals(identity,sourceIdentity(manifest(save("batch-after-errors.capy"))))
        }
        fun backing(project: JSONObject) = project.rasterResources().objects().map {
            JSONObject(it.toString()).toString()
        }.sorted()
        val originalTheme = host.snapshot!!.getJSONObject("state").opt("settings")?.let { it as JSONObject }?.opt("theme") ?: JSONObject.NULL
        val motions = org.json.JSONArray()
        try {
            for (theme in if (affineSmoke) listOf("light") else listOf("light", "dark")) {
                if (!affineSmoke) open(File(files, "batch-placement.capy"))
                memoryStage("$theme: before affine")
                action(obj("type" to "set_theme", "theme" to theme))
                val owner = fitted.occurrenceRecords().getJSONObject(0).getString("id")
                val ownerToken = native { state(it).array("layers").getJSONObject(0).getLong("id") }
                action(obj("type" to "layer", "action" to obj("op" to "select", "id" to ownerToken, "mask" to false)))
                invoke("fit_canvas")
                if (!affineSmoke || InstrumentationRegistry.getArguments().getString("affineSmokeWatercolor") == "true") {
                    invoke("brush")
                    action(obj("type" to "select_brush", "id" to 21))
                    action(obj("type" to "set_brush_size", "value" to 180.0))
                    compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
                    stroke(0.0)
                }
                invoke("scale_rotate")
                compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.any { it.getString("id") == "apply_transform" && it.getBoolean("enabled") } == true }
                val affineDrags = if (affineSmoke) InstrumentationRegistry.getArguments().getString("affineSmokeDrags", "1")!!.toInt() else 1
                require(affineDrags > 0)
                repeat(affineDrags) { index ->
                    motions.put(motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 30).put("theme", theme).put("canvas", listOf(2000, 1500)))
                    if (affineDrags > 1) memoryStage("$theme: after affine drag ${index + 1}")
                }
                if (affineDrags > 1) {
                    android.os.SystemClock.sleep(2000)
                    memoryStage("$theme: after affine idle")
                }
                memoryStage("$theme: after affine motion")
                press("apply_transform")
                memoryStage("$theme: after affine Apply")
                var retained = manifest(save("$theme-retained-affine.capy"))
                if (affineSmoke) {
                    assertEquals(identity, sourceIdentity(retained))
                    assertNull(host.failure)
                    File(activity.getExternalFilesDir(null), "validation/pixel-bake/motion-3mp.json").apply { parentFile!!.mkdirs() }.writeText(motions.toString(2))
                    println("Android minimal retained-photo affine smoke passed")
                    return
                }
                fun ownerRaster(project: JSONObject) = project.paintData(owner)
                fun assertMaterial(project: JSONObject) {
                    val raster = ownerRaster(project)
                    assertFalse("$theme: watercolor style remains stored", raster.optJSONObject("material")?.isNull("watercolor") != false)
                    val planes = raster.array("tiles").objects().map { it.getString("plane") }
                    assertTrue("$theme: native pigment remains stored", "color" in planes)
                    assertTrue("$theme: native wetness remains stored", "watercolor_wetness" in planes)
                }
                assertMaterial(retained)
                fun ownerProperties(project: JSONObject) = project.occurrenceRecords().objects().first { it.getString("id") == owner }.getJSONObject("data")
                fun published() = host.snapshot!!.getJSONObject("state")
                fun setting(id: String) = published().array("tool_settings").objects().first { it.getString("id") == id }.getDouble("value")
                fun key(code: Int, pressed: Boolean, repeat: Int = 0) {
                    val now = SystemClock.uptimeMillis()
                    assertTrue(instrumentation.uiAutomation.injectInputEvent(android.view.KeyEvent(now, now, if (pressed) android.view.KeyEvent.ACTION_DOWN else android.view.KeyEvent.ACTION_UP, code, repeat), true))
                    SystemClock.sleep(80); refresh()
                }
                fun dragDocument(from: Pair<Double, Double>, to: Pair<Double, Double>) {
                    val camera = published().getJSONObject("camera")
                    val shift = camera.getJSONArray("translation"); val zoom = camera.getDouble("zoom")
                    var origin = androidx.compose.ui.geometry.Offset.Zero
                    scenario.onActivity { origin = host.surfaceOrigin }
                    val started = SystemClock.uptimeMillis()
                    for (step in 0..12) {
                        val fraction = step / 12.0
                        val props = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 7; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS })
                        val coords = arrayOf(android.view.MotionEvent.PointerCoords().apply {
                            x = ((from.first + (to.first - from.first) * fraction) * zoom + shift.getDouble(0) + origin.x).toFloat()
                            y = ((from.second + (to.second - from.second) * fraction) * zoom + shift.getDouble(1) + origin.y).toFloat()
                            pressure = if (step == 12) 0f else .7f
                        })
                        val phase = when (step) { 0 -> android.view.MotionEvent.ACTION_DOWN; 12 -> android.view.MotionEvent.ACTION_UP; else -> android.view.MotionEvent.ACTION_MOVE }
                        val event = android.view.MotionEvent.obtain(started, SystemClock.uptimeMillis(), phase, 1, props, coords, 0, 0, 1f, 1f, 0, 0, android.view.InputDevice.SOURCE_STYLUS, 0)
                        try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
                        SystemClock.sleep(16)
                    }
                    refresh()
                }
                action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "tool_settings", "visible" to true)))
                val settingsGroup = host.panelGroup("tool_settings")
                if (settingsGroup.getString("active") != "tool_settings") action(obj("type" to "select_panel_tab", "group" to settingsGroup.getLong("id"), "panel" to "tool_settings"))
                invoke("scale_rotate")
                compose.waitUntil(30_000) { published().optJSONObject("canvas_bar") != null && published().array("tool_extra").objects().any { it.optJSONObject("Choice")?.optString("id") == "transform-reference" } }
                val referenceHull = published().getJSONObject("canvas_bar").getJSONArray("anchor")
                val density = activity.resources.displayMetrics.density
                var settingsContainer = "panel-body-tool_settings"
                fun settings(tag: String) = compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag(settingsContainer)))
                val settingsDrawer = compose.onAllNodesWithTag("column-icon-tool_settings").fetchSemanticsNodes().isNotEmpty()
                fun toggleSettingsDrawer() {
                    compose.onNodeWithTag("column-icon-tool_settings").performTouchInput { click(center) }
                    refresh()
                }
                if (settingsDrawer) {
                    toggleSettingsDrawer()
                    compose.waitUntil(10_000) { published().getJSONObject("customization").array("column_drawers").objects().any { it.getJSONObject("anchor").optString("origin") == "tool_settings" } }
                    val drawer = published().getJSONObject("customization").array("column_drawers").objects().first { it.getJSONObject("anchor").optString("origin") == "tool_settings" }
                    settingsContainer = "column-drawer-${drawer.getJSONObject("anchor").getInt("column")}"
                }
                compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("tool-segments-transform-reference") and hasAnyAncestor(hasTestTag(settingsContainer))).fetchSemanticsNodes().size == 1 }
                val reference = settings("tool-segments-transform-reference").fetchSemanticsNode().boundsInRoot
                val positionX = settings("tool-setting-transform_x").fetchSemanticsNode().boundsInRoot
                val positionY = settings("tool-setting-transform_y").fetchSemanticsNode().boundsInRoot
                assertEquals("Position reference remains compact", 54.0, (reference.width / density).toDouble(), 1.0)
                assertEquals(54.0, (reference.height / density).toDouble(), 1.0)
                assertEquals("Coordinates stay beside the reference", 12.0, ((positionX.left - reference.right) / density).toDouble(), 1.0)
                assertTrue(positionX.top >= reference.top - density && positionY.bottom <= reference.bottom + density)
                File(activity.getExternalFilesDir(null), "validation/$theme-transform-position.png").also { it.parentFile!!.mkdirs() }.outputStream().use { instrumentation.uiAutomation.takeScreenshot().compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                for (index in 0..8) {
                    settings("tool-segment-transform-reference-$index").assertIsDisplayed().performTouchInput { click(center) }
                    refresh()
                    val choice = published().array("tool_extra").objects().first { it.optJSONObject("Choice")?.optString("id") == "transform-reference" }.getJSONObject("Choice")
                    assertTrue(choice.array("items").getJSONObject(index).getBoolean("selected"))
                    val x = referenceHull.getDouble(0) + (referenceHull.getDouble(2) - referenceHull.getDouble(0)) * (index % 3) / 2.0
                    val y = referenceHull.getDouble(1) + (referenceHull.getDouble(3) - referenceHull.getDouble(1)) * (index / 3) / 2.0
                    assertEquals("Reference publishes absolute document X", x, setting("transform_x"), .01)
                    assertEquals("Reference publishes absolute document Y", y, setting("transform_y"), .01)
                    assertEquals(referenceHull.toString(), published().getJSONObject("canvas_bar").getJSONArray("anchor").toString())
                }
                settings("tool-segment-transform-reference-4").performTouchInput { click(center) }; refresh()
                val center = (referenceHull.getDouble(0) + referenceHull.getDouble(2)) / 2 to (referenceHull.getDouble(1) + referenceHull.getDouble(3)) / 2
                val pivot = center.first - (referenceHull.getDouble(2) - referenceHull.getDouble(0)) * .1 to center.second - (referenceHull.getDouble(3) - referenceHull.getDouble(1)) * .1
                if (settingsDrawer) toggleSettingsDrawer()
                assertTrue("Canvas pivot contact has no expanded panel overlay", published().getJSONObject("customization").isNull("expanded"))
                dragDocument(center, pivot)
                assertEquals("Pivot drag leaves artwork bounds fixed", referenceHull.toString(), published().getJSONObject("canvas_bar").getJSONArray("anchor").toString())
                if (settingsDrawer) toggleSettingsDrawer()
                settings("number-value-transform_angle").performScrollTo().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .5f, height * .5f)) }
                val angleLabel = published().array("tool_settings").objects().first { it.getString("id") == "transform_angle" }.getString("label")
                compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("number-$angleLabel") and hasAnyAncestor(hasTestTag(settingsContainer))).fetchSemanticsNodes().isNotEmpty() }
                settings("number-$angleLabel").performTextReplacement("30")
                settings("number-$angleLabel").performImeAction(); refresh()
                if (settingsDrawer) toggleSettingsDrawer()
                press("apply_transform")
                val rotated = manifest(save("$theme-reference-pivot.capy"))
                val original = ownerProperties(retained).authoredAffine()
                val rotatedMap = ownerProperties(rotated).authoredAffine()
                val determinant = original.getDouble(0) * original.getDouble(3) - original.getDouble(1) * original.getDouble(2)
                val px = pivot.first - original.getDouble(4); val py = pivot.second - original.getDouble(5)
                val u = (original.getDouble(3) * px - original.getDouble(2) * py) / determinant
                val v = (-original.getDouble(1) * px + original.getDouble(0) * py) / determinant
                assertEquals("Native custom pivot remains fixed under numeric rotation", pivot.first, rotatedMap.getDouble(0) * u + rotatedMap.getDouble(2) * v + rotatedMap.getDouble(4), .01)
                assertEquals(pivot.second, rotatedMap.getDouble(1) * u + rotatedMap.getDouble(3) * v + rotatedMap.getDouble(5), .01)
                assertEquals(sourceIdentity(retained), sourceIdentity(rotated)); assertEquals(backing(retained), backing(rotated))
                invoke("undo")
                assertEquals(ownerProperties(retained).toString(), ownerProperties(manifest(save("$theme-pivot-undone.capy"))).toString())
                invoke("move")
                key(android.view.KeyEvent.KEYCODE_DPAD_RIGHT, true)
                key(android.view.KeyEvent.KEYCODE_DPAD_RIGHT, true, 1)
                key(android.view.KeyEvent.KEYCODE_DPAD_LEFT, false)
                key(android.view.KeyEvent.KEYCODE_DPAD_RIGHT, false)
                val nudged = manifest(save("$theme-nudged.capy"))
                assertNotEquals(ownerProperties(retained).toString(), ownerProperties(nudged).toString())
                invoke("undo"); assertEquals("Held native nudge has one undo", ownerProperties(retained).toString(), ownerProperties(manifest(save("$theme-nudge-undone.capy"))).toString())
                invoke("redo")
                key(android.view.KeyEvent.KEYCODE_DPAD_LEFT, true)
                key(android.view.KeyEvent.KEYCODE_ESCAPE, true); key(android.view.KeyEvent.KEYCODE_ESCAPE, false)
                key(android.view.KeyEvent.KEYCODE_DPAD_LEFT, false)
                assertEquals("Escape then release preserves the accepted map", ownerProperties(nudged).toString(), ownerProperties(manifest(save("$theme-nudge-cancelled.capy"))).toString())
                invoke("transform_again")
                val again = manifest(save("$theme-again.capy"))
                assertNotEquals(ownerProperties(nudged).toString(), ownerProperties(again).toString())
                invoke("undo"); assertEquals(ownerProperties(nudged).toString(), ownerProperties(manifest(save("$theme-again-undone.capy"))).toString())
                invoke("undo"); assertEquals(ownerProperties(retained).toString(), ownerProperties(manifest(save("$theme-nudge-restored.capy"))).toString())
                if (InstrumentationRegistry.getArguments().getString("imagePlacementPresentationOnly") == "true") continue
                invoke("scale_rotate")
                compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.optJSONObject("canvas_bar") != null }
                invoke("transform_distort")
                val corner = host.snapshot!!.getJSONObject("state").getJSONObject("canvas_bar").getJSONArray("anchor")
                val zoom = host.snapshot!!.getJSONObject("state").getJSONObject("camera").getDouble("zoom")
                motions.put(motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 30,
                    corner.getDouble(2) to corner.getDouble(3) - 20.0 / zoom).put("theme", theme).put("mode", "Distort"))
                press("apply_transform")
                val distorted = manifest(save("$theme-retained-distort.capy"))
                val properties = distorted.occurrenceRecords().objects().first { it.getString("id") == owner }.getJSONObject("data")
                val outer = properties.authoredPlacement().getJSONArray("projective")
                assertTrue("$theme: corner drag retains a projective map", outer.getDouble(6) != 0.0 || outer.getDouble(7) != 0.0)
                assertEquals(sourceIdentity(retained), sourceIdentity(distorted))
                assertEquals(backing(retained), backing(distorted))
                invoke("scale_rotate")
                compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.optJSONObject("canvas_bar") != null }
                invoke("transform_warp")
                val warpZoom = host.snapshot!!.getJSONObject("state").getJSONObject("camera").getDouble("zoom")
                motions.put(motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 30,
                    outer.getDouble(2) / outer.getDouble(8) to outer.getDouble(5) / outer.getDouble(8) - 20.0 / warpZoom)
                    .put("theme", theme).put("mode", "Warp"))
                press("apply_transform")
                retained = manifest(save("$theme-retained-warp.capy"))
                val warpedProperties = retained.occurrenceRecords().objects().first { it.getString("id") == owner }.getJSONObject("data")
                assertFalse("$theme: bent grid persists", warpedProperties.authoredPlacement().isNull("mesh"))
                assertEquals(sourceIdentity(distorted), sourceIdentity(retained))
                assertEquals(backing(distorted), backing(retained))
                open(File(files, "$theme-retained-warp.capy"))
                val reopenedRetained = manifest(save("$theme-retained-warp-reopened.capy"))
                assertEquals(warpedProperties.toString(), reopenedRetained.occurrenceRecords().objects().first { it.getString("id") == owner }.getJSONObject("data").toString())
                assertEquals(backing(retained), backing(reopenedRetained))
                assertMaterial(reopenedRetained)
                invoke("scale_rotate")
                compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.optJSONObject("canvas_bar") != null }
                invoke("transform_warp")
                invoke("warp_split_cross")
                val splitHull = host.snapshot!!.getJSONObject("state").getJSONObject("canvas_bar").getJSONArray("anchor")
                val splitZoom = host.snapshot!!.getJSONObject("state").getJSONObject("camera").getDouble("zoom")
                motions.put(motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 30,
                    (splitHull.getDouble(0) + splitHull.getDouble(2)) * .5 to
                        (splitHull.getDouble(1) + splitHull.getDouble(3)) * .5 - 20.0 / splitZoom)
                    .put("theme", theme).put("mode", "Warp split cross"))
                press("apply_transform")
                retained = manifest(save("$theme-retained-split.capy"))
                val splitPlacement = retained.occurrenceRecords().objects().first { it.getString("id") == owner }
                    .getJSONObject("data").authoredPlacement()
                val splitBreakpoints = splitPlacement.getJSONObject("mesh").getJSONArray("breakpoints")
                assertEquals("$theme: cross split inserts a vertical line", 5, splitBreakpoints.getJSONArray(0).length())
                assertEquals("$theme: cross split inserts a horizontal line", 5, splitBreakpoints.getJSONArray(1).length())
                assertEquals(sourceIdentity(reopenedRetained), sourceIdentity(retained))
                assertEquals(backing(reopenedRetained), backing(retained))
                open(File(files, "$theme-retained-split.capy"))
                val reopenedSplit = manifest(save("$theme-retained-split-reopened.capy"))
                assertEquals(splitPlacement.toString(), reopenedSplit.occurrenceRecords().objects().first { it.getString("id") == owner }
                    .getJSONObject("data").authoredPlacement().toString())
                assertEquals(backing(retained), backing(reopenedSplit))
                memoryStage("$theme: before pixel bake")
                if (compose.onAllNodesWithTag("application-menu-edit").fetchSemanticsNodes().isNotEmpty()) compose.onNodeWithTag("application-menu-edit").performClick()
                else {
                    compose.onNodeWithTag("header-menu-labels-compact").performClick()
                    val label = host.snapshot!!.array("application_menus").objects().first { it.getString("id") == "edit" }.getString("label")
                    compose.onNodeWithText(label).performClick()
                }
                val bakeLabel = published().array("commands").objects().first { it.getString("id") == "apply_transform_pixels" }.getString("label")
                compose.onNodeWithText(bakeLabel).performClick()
                compose.waitUntil(120_000) { tick(); native { state(it).isNull("canvas_bar") } }
                refresh()
                memoryStage("$theme: after pixel bake")
                val baked = manifest(save("$theme-baked.capy"))
                assertMaterial(baked)
                assertEquals(ownerRaster(retained).getJSONObject("material").getJSONObject("watercolor").toString(), ownerRaster(baked).getJSONObject("material").getJSONObject("watercolor").toString())
                assertEquals("$theme: only the chosen source is baked", retained.originalImages().length() - 1, baked.originalImages().length())
                val bakedOwner = baked.occurrenceRecords().objects().first { it.getString("id") == owner }
                val pose = bakedOwner.getJSONObject("data").authoredAffine()
                assertEquals(listOf(1.0, 0.0, 0.0, 1.0, 0.0, 0.0), (0 until 6).map { pose.getDouble(it) })
                invoke("brush"); action(obj("type" to "select_brush", "id" to 1)); action(obj("type" to "set_brush_size", "value" to 80.0))
                stroke(0.0)
                val painted = manifest(save("$theme-baked-painted.capy"))
                assertNotEquals("$theme: painting edits the baked native planes", backing(baked), backing(painted))
                invoke("liquify"); action(obj("type" to "select_brush", "id" to 37)); action(obj("type" to "set_brush_size", "value" to 180.0))
                stroke(0.0)
                val liquified = manifest(save("$theme-baked-liquified.capy"))
                assertNotEquals("$theme: Liquify edits the baked native planes", backing(painted), backing(liquified))
                invoke("undo"); assertEquals(backing(painted), backing(manifest(save("$theme-undo-liquify.capy"))))
                invoke("undo"); assertEquals(backing(baked), backing(manifest(save("$theme-undo-paint.capy"))))
                invoke("undo")
                val restored = manifest(save("$theme-undo-bake.capy"))
                assertEquals(sourceIdentity(retained), sourceIdentity(restored))
                assertEquals(backing(retained), backing(restored))
                invoke("redo"); assertEquals(backing(baked), backing(manifest(save("$theme-redo-bake.capy"))))
                open(File(files, "$theme-baked.capy"))
                val reopened = manifest(save("$theme-baked-reopened.capy"))
                assertMaterial(reopened)
                assertEquals(ownerRaster(baked).toString(), ownerRaster(reopened).toString())
                assertEquals(backing(baked), backing(reopened))
                assertEquals(sourceIdentity(baked), sourceIdentity(reopened))
                instrumentation.uiAutomation.takeScreenshot()?.let { image ->
                    try { File(activity.getExternalFilesDir(null), "validation/pixel-bake/$theme.png").apply { parentFile!!.mkdirs() }.outputStream().use { image.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                    finally { image.recycle() }
                }
            }
        } finally { action(obj("type" to "set_theme", "theme" to originalTheme)) }
        File(activity.getExternalFilesDir(null), "validation/pixel-bake/motion-3mp.json").apply { parentFile!!.mkdirs() }.writeText(motions.toString(2))
        assertNull(host.failure)
        println("Android image placement: batch Apply/Cancel, fit, exact source retention, one-step history, reopen, Original Size, malformed/cancelled/stale requests; light/dark affine bake, paint, Liquify, exact undo/redo and reopen passed")
    }

    @Test fun imagePlacementSystemPickerAndExternalDrag() {
        fun action(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }; tick()
            scenario.onActivity { host.documentChanged() }
            compose.waitForIdle()
        }
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun count() = native { state(it).array("layers").length() }
        fun press(command: String) = pressCanvasBar(command)
        fun systemNode(predicate: (android.view.accessibility.AccessibilityNodeInfo) -> Boolean): android.view.accessibility.AccessibilityNodeInfo? {
            fun find(node: android.view.accessibility.AccessibilityNodeInfo?): android.view.accessibility.AccessibilityNodeInfo? {
                node ?: return null
                if (predicate(node)) return node
                for (i in 0 until node.childCount) find(node.getChild(i))?.let { return it }
                return null
            }
            return find(instrumentation.uiAutomation.rootInActiveWindow)
        }
        fun systemClick(name: String, long: Boolean = false) {
            instrumentation.uiAutomation.waitForIdle(300, 5_000)
            val deadline = SystemClock.uptimeMillis() + 15_000
            var node: android.view.accessibility.AccessibilityNodeInfo? = null
            while (node == null && SystemClock.uptimeMillis() < deadline) {
                node = systemNode {
                    val matches = it.text?.toString()?.contains(name, ignoreCase = true) == true || it.contentDescription?.toString()?.contains(name, ignoreCase = true) == true ||
                        (name == "Open" && it.text?.toString()?.equals("Select", ignoreCase = true) == true)
                    matches && (name.startsWith("capy-placement-") || it.isClickable || it.parent?.isClickable == true || it.parent?.parent?.isClickable == true)
                }
                if (node == null) SystemClock.sleep(100)
            }
            assertNotNull("DocumentsUI item $name", node)
            var target = node!!
            if (long || name.startsWith("capy-placement-")) {
                val bounds = android.graphics.Rect(); target.getBoundsInScreen(bounds)
                assertFalse("DocumentsUI file has bounds", bounds.isEmpty)
                android.util.Log.i("CapyPlacementTest", "Picker contact $name long=$long bounds=$bounds description=${target.contentDescription}")
                val x = bounds.centerX(); val y = bounds.centerY()
                val command = if (long) "input touchscreen swipe $x $y $x $y ${android.view.ViewConfiguration.getLongPressTimeout() + 200}" else "input touchscreen tap $x $y"
                ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }
            } else {
                while (!target.isClickable && target.parent != null) target = target.parent
                assertTrue("DocumentsUI action $name", target.performAction(android.view.accessibility.AccessibilityNodeInfo.ACTION_CLICK))
            }
            SystemClock.sleep(200)
        }
        action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 0)))
        val newTask = native { h -> val (id, file) = request(h, "new_document"); Native.projectTask(h, id, "null", file.getLong("epoch"), file.getLong("revision")) }
        try { Native.projectWork(newTask, -1, 2000, 1500); native { Native.projectAdopt(it, newTask, "null") } } finally { Native.projectFree(newTask) }
        scenario.onActivity { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        invoke("fit_canvas")
        val resolver = activity.contentResolver
        val prefix = "capy-placement-${System.currentTimeMillis()}"
        val configured = InstrumentationRegistry.getArguments().getString("imagePlacementPhotos")?.split(',')?.map { File(activity.filesDir, it) }
        val suffix = if (configured == null) "png" else "jpg"
        val uris = mutableListOf<android.net.Uri>()
        try {
            for (i in 0..1) {
                val uri = resolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI, android.content.ContentValues().apply {
                    put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME, "$prefix-$i.$suffix")
                    put(android.provider.MediaStore.MediaColumns.MIME_TYPE, if (suffix == "jpg") "image/jpeg" else "image/png")
                })!!
                uris.add(uri)
                if (configured != null) resolver.openOutputStream(uri)!!.use { output -> configured[i].inputStream().use { it.copyTo(output) } }
                else {
                    val bitmap = android.graphics.Bitmap.createBitmap(80 + i * 20, 60, android.graphics.Bitmap.Config.ARGB_8888)
                    bitmap.eraseColor(if (i == 0) android.graphics.Color.RED else android.graphics.Color.BLUE)
                    try { resolver.openOutputStream(uri)!!.use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() }
                }
            }
            val before = count()
            DocumentController.nativeFileJobsForTest = false
            invoke("import_image")
            compose.waitUntil(15_000) { host.documents.images.choosing || host.actionError != null }
            compose.waitForIdle()
            assertNull(host.actionError)
            systemClick("Show roots")
            systemClick("Recent")
            systemClick("$prefix-0.$suffix", long = true)
            systemClick("$prefix-1.$suffix")
            systemClick("Open")
            compose.waitUntil(30_000) { count() == before + 2 && !host.documents.images.working }
            press("cancel_transform"); assertEquals(before, count())
            println("Android DocumentsUI: real multiple selection, URI result, batch Cancel passed")

            fun dragTo(destination: androidx.compose.ui.geometry.Offset) {
                lateinit var source: android.view.View
                lateinit var root: android.widget.FrameLayout
                var start = androidx.compose.ui.geometry.Offset.Zero
                var started = false
                scenario.onActivity { owner ->
                    root = owner.findViewById(android.R.id.content)
                    source = android.view.View(owner).apply {
                        setBackgroundColor(android.graphics.Color.MAGENTA)
                        setOnTouchListener { view, event ->
                            if (event.actionMasked == android.view.MotionEvent.ACTION_DOWN) {
                                val clip = android.content.ClipData.newUri(resolver, "Placement test", uris.first())
                                started = view.startDragAndDrop(clip, android.view.View.DragShadowBuilder(view), null,
                                    android.view.View.DRAG_FLAG_GLOBAL or android.view.View.DRAG_FLAG_GLOBAL_URI_READ)
                            }
                            true
                        }
                    }
                    root.addView(source, android.widget.FrameLayout.LayoutParams(64, 64).apply { leftMargin = 100; topMargin = 100 })
                }
                instrumentation.waitForIdleSync()
                scenario.onActivity {
                    val location = IntArray(2); source.getLocationOnScreen(location)
                    start = androidx.compose.ui.geometry.Offset(location[0] + 32f, location[1] + 32f)
                }
                val down = SystemClock.uptimeMillis()
                try {
                    for (i in 0..13) {
                        val fraction = (i / 12f).coerceAtMost(1f)
                        val point = start + (destination - start) * fraction
                        val phase = when (i) { 0 -> android.view.MotionEvent.ACTION_DOWN; 13 -> android.view.MotionEvent.ACTION_UP; else -> android.view.MotionEvent.ACTION_MOVE }
                        val event = android.view.MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, point.x, point.y, 0).apply { setSource(android.view.InputDevice.SOURCE_TOUCHSCREEN) }
                        try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
                        SystemClock.sleep(50)
                    }
                    assertTrue("Android framework started global URI drag", started)
                } finally { scenario.onActivity { root.removeView(source) } }
            }
            val camera = native { state(it).getJSONObject("camera") }
            val translation = camera.getJSONArray("translation"); val zoom = camera.getDouble("zoom")
            val point = androidx.compose.ui.geometry.Offset((400 * zoom + translation.getDouble(0)).toFloat(), (300 * zoom + translation.getDouble(1)).toFloat())
            dragTo(point + host.surfaceOrigin)
            compose.waitUntil(30_000) { count() == before + 1 && !host.documents.images.working }
            press("apply_transform")
            DocumentController.nativeFileJobsForTest = true
            val dropped = manifest(save("external-drop.capy"))
            val pose = dropped.occurrenceRecords().getJSONObject(0).getJSONObject("data").authoredAffine()
            val extent = dropped.originalImages().getJSONObject(0).getJSONArray("extent")
            assertEquals(400.0 - extent.getDouble(0) * pose.getDouble(0) / 2, pose.getDouble(4), 2.0)
            assertEquals(300.0 - extent.getDouble(1) * pose.getDouble(3) / 2, pose.getDouble(5), 2.0)
            invoke("undo"); assertEquals(before, count())
            DocumentController.nativeFileJobsForTest = false
            action(obj("type" to "layer", "action" to obj("op" to "new", "group" to true, "clipped" to false)))
            val group = native { state(it).array("layers").objects().first().getLong("id") }
            val row = compose.onNodeWithTag("layer-row-$group").fetchSemanticsNode().boundsInWindow
            val windowOrigin = IntArray(2); scenario.onActivity { it.window.decorView.getLocationOnScreen(windowOrigin) }
            dragTo(row.center + androidx.compose.ui.geometry.Offset(windowOrigin[0].toFloat(), windowOrigin[1].toFloat()))
            compose.waitUntil(30_000) { count() == before + 2 && !host.documents.images.working }
            press("apply_transform")
            DocumentController.nativeFileJobsForTest = true
            val nested = manifest(save("external-group-drop.capy"))
            val groupName = native { state(it).array("layers").objects().single { it.getLong("id") == group }.getString("label") }
            val authoredGroup = nested.occurrenceRecords().objects().single {it.getJSONObject("data").optString("name") == groupName}
            val children = nested.packageData(authoredGroup.getJSONObject("data").getJSONObject("content").getJSONObject("stack").getString("ref")).getJSONArray("entries")
            assertTrue(children.objects().any {nested.packageData(it.getString("ref")).getJSONObject("content").has("paint")})
            println("Android global URI drag: canvas captured point, group center insertion, Apply and one-step Undo passed")

            val clipboard = activity.getSystemService(android.content.ClipboardManager::class.java)
            var previous: android.content.ClipData? = null
            val beforePaste = count()
            try {
                DocumentController.nativeFileJobsForTest = false
                compose.runOnUiThread {
                    previous = clipboard.primaryClip
                    clipboard.setPrimaryClip(android.content.ClipData.newUri(resolver, "Placement clipboard test", uris[0]).apply { addItem(android.content.ClipData.Item(uris[1])) })
                    host.invoke("paste_image")
                }
                compose.waitUntil(60_000) { count() == beforePaste + 2 && !host.documents.images.working }
                press("cancel_transform"); assertEquals(beforePaste, count())
            } finally {
                DocumentController.nativeFileJobsForTest = true
                compose.runOnUiThread { previous?.let { clipboard.setPrimaryClip(it) } ?: clipboard.clearPrimaryClip() }
            }

            val retained = nested.originalIdentity()
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
            assertEquals(retained, manifest(save("placement-recreated.capy")).originalIdentity())
            val control = Native.captureControl()
            val (task, requestId) = native { h -> val (id, _) = request(h, "import_image"); Native.imageImportTask(h, id, Native.imageImportContext(h, "null", "null"), control) to id }
            try {
                Native.imageImportRead(task, resolver.openFileDescriptor(uris.first(), "r")!!.detachFd(), "late.png")
                native { Native.destroyGpuForTest(it) }; scenario.onActivity { host.documentChanged() }
                compose.waitUntil(10_000) { host.failure != null }
                scenario.onActivity { host.restartCanvas() }
                compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
                try { native { Native.imageImportAdopt(it, task) }; fail("Prepared batch survived a GPU generation change") } catch (_: IllegalStateException) { }
                native { Native.documentComplete(it, requestId, false, "null") }
                assertEquals(retained, manifest(save("placement-gpu-recreated.capy")).originalIdentity())
            } finally { Native.imageImportFree(task); Native.captureFree(control) }
            assertNull(host.failure)
            println("Android placed source survived activity/GPU replacement; retired GPU batch rejected")
            if (configured != null) for ((file, expected) in configured.zip(listOf(9504 to 6336, 4000 to 6000))) {
                open(file); invoke("fit_canvas")
                val opened = manifest(save("opened-${file.name}.capy"))
                assertEquals(expected.first, opened.compositionSize().getInt(0))
                assertEquals(expected.second, opened.compositionSize().getInt(1))
            }
        } finally {
            DocumentController.nativeFileJobsForTest = true
            for (uri in uris) resolver.delete(uri, null, null)
        }
    }

    @Test fun clipboardCopyLatency24mp() {
        val photo = File(files, "clipboard-24mp.jpg")
        val bitmap = android.graphics.Bitmap.createBitmap(6000, 4000, android.graphics.Bitmap.Config.ARGB_8888)
        android.graphics.Canvas(bitmap).drawPaint(android.graphics.Paint().apply {
            shader = android.graphics.LinearGradient(0f, 0f, 6000f, 4000f, android.graphics.Color.RED, android.graphics.Color.BLUE, android.graphics.Shader.TileMode.CLAMP)
        })
        photo.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 92, it) }
        bitmap.recycle()
        try {
            open(photo); invoke("fit_canvas")
            val clipboard = activity.getSystemService(android.content.ClipboardManager::class.java)
            fun nonce(): String? { var value: String? = null; compose.runOnUiThread { value = clipboard.primaryClipDescription?.extras?.getString(ClipboardController.NONCE) }; return value }
            fun idle() = native { state(it) }.let { s ->
                !s.getJSONObject("document_file").getBoolean("busy") && s.array("requests").objects().none { it.getJSONObject("kind").optString("type") == "document" }
            }
            fun copy(label: String) {
                val before = nonce()
                DocumentController.nativeFileJobsForTest = false
                val started = SystemClock.elapsedRealtime()
                try {
                    compose.runOnUiThread { host.invoke("copy") }
                    compose.waitUntil(180_000) { tick(); nonce().let { it != null && it != before } && idle() }
                } finally { DocumentController.nativeFileJobsForTest = true }
                println("Clipboard latency: $label copied in ${SystemClock.elapsedRealtime() - started} ms")
            }
            invoke("select_all"); copy("24 MP photo, Select All")
            invoke("deselect"); invoke("brush"); stroke(0.0)
            invoke("select_all"); copy("24 MP photo after a stroke, Select All")
            assertNull(host.failure)
        } finally { photo.delete() }
    }

    @Test fun spatialFilterWindowsKeepPaintAndHistory() {
        val source = InstrumentationRegistry.getArguments().getString("spatialPhoto")
            ?: throw AssumptionViolatedException("Supply -e spatialPhoto with the 24 MP reference photo")
        val photo = File(files, "spatial-window.jpg").apply {
            writeBytes(ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand("cat $source")).use { it.readBytes() })
        }
        action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 0)))
        open(photo); refresh()
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        assertEquals(24_000_000L, histogram().getLong("pixels"))
        action(obj("type" to "layer", "action" to obj("op" to "new", "group" to false, "clipped" to false)))
        val paint = native { state(it).getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id") }
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "gaussian_blur")))
        val filter = native { state(it).getJSONObject("layer_properties").getLong("layer") }
        val output = File(activity.getExternalFilesDir(null), "spatial-filter-windows").apply { mkdirs() }
        for (theme in listOf("dark", "light")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "set_zoom", "zoom" to .5))
            for (sigma in listOf(0.0, 21.0, 7.0)) {
                action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to filter, "key" to "sigma", "value" to obj("kind" to "number", "value" to sigma))))
                action(obj("type" to "select_layer", "id" to paint))
                action(obj("type" to "invoke", "command" to "hand"))
                motion(android.view.MotionEvent.TOOL_TYPE_FINGER, 30, 3000.0 to 2000.0)
                action(obj("type" to "invoke", "command" to "fit_canvas"))
                action(obj("type" to "set_zoom", "zoom" to .5))
            }
            val before = manifest(save("spatial-before.capy"))
            action(obj("type" to "invoke", "command" to "brush")); action(obj("type" to "select_brush", "id" to 1))
            action(obj("type" to "set_brush_size", "value" to 120))
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(1.0, .1, .3, 1.0))))
            motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 30, 3000.0 to 2000.0)
            compose.waitUntil(60_000) { !tick() }
            val painted = manifest(save("spatial-painted.capy"))
            assertNotEquals(before.paintRecords().toString(), painted.paintRecords().toString())
            assertEquals(sourceIdentity(before), sourceIdentity(painted))
            invoke("undo")
            assertEquals(before.paintRecords().toString(), manifest(save("spatial-undo.capy")).paintRecords().toString())
            invoke("redo")
            assertEquals(painted.paintRecords().toString(), manifest(save("spatial-redo.capy")).paintRecords().toString())
            assertNull(host.failure); assertNull(host.actionError)
            instrumentation.uiAutomation.takeScreenshot()?.let { shot ->
                try { File(output, "$theme.png").outputStream().use { shot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                finally { shot.recycle() }
            }
            invoke("undo")
        }
        photo.delete()
    }

    @Test fun attachedFilterPreviewsReferenceAndRecoveryKeepOwnerInput() {
        compose.runOnUiThread { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:painter")) }
        compose.waitUntil(30_000) { host.workspaceManager?.optString("id") == "builtin:workspace:painter" && host.workspaceManager?.optBoolean("busy") == false }
        compose.runOnUiThread { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to "builtin:workspace:painter"))) }
        compose.waitUntil(10_000) { host.workspaceManager?.isNull("prompt") == false }
        compose.runOnUiThread { host.workspaceInput(obj("type" to "submit")) }
        compose.waitUntil(30_000) { host.workspaceManager?.isNull("prompt") == true && host.workspaceManager?.optBoolean("busy") == false }
        fun image(name: String, color: Int, stripe: Boolean = false) = File(files, name).also { file ->
            val bitmap = android.graphics.Bitmap.createBitmap(64, 64, android.graphics.Bitmap.Config.ARGB_8888)
            try {
                bitmap.eraseColor(color)
                if (stripe) android.graphics.Canvas(bitmap).drawRect(30f, 0f, 34f, 64f, android.graphics.Paint().apply {
                    this.color = android.graphics.Color.argb(128, 20, 40, 230)
                    xfermode = android.graphics.PorterDuffXfermode(android.graphics.PorterDuff.Mode.SRC)
                })
                file.outputStream().use { assertTrue(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)) }
            } finally { bitmap.recycle() }
        }
        open(image("reference-base.png", android.graphics.Color.rgb(20, 240, 30)))
        val input = image("reference-owner.png", android.graphics.Color.argb(128, 230, 40, 20), true)
        val control = Native.captureControl()
        val task = native { handle ->
            val id = request(handle, "import_image").first
            Native.imageImportTask(handle, id, Native.imageImportContext(handle, "null", "null"), control)
        }
        try {
            Native.imageImportRead(task, ParcelFileDescriptor.open(input, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), input.name)
            native { Native.imageImportAdopt(it, task) }
        } finally { Native.imageImportFree(task); Native.captureFree(control) }
        refresh(); pressCanvasBar("apply_transform")
        fun current() = native { state(it).getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id") }
        fun layer(value: JSONObject) = action(obj("type" to "layer", "action" to value))
        fun row(id: Long) = native { state(it).array("layers").objects().first { row -> row.getLong("id") == id } }
        val owner = current()
        layer(obj("op" to "rename", "id" to owner, "name" to "Clipped owner"))
        layer(obj("op" to "clip", "id" to owner, "value" to true))
        action(obj("type" to "set_layer_opacity", "opacity" to .35))
        layer(obj("op" to "reference", "id" to owner))
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "motion_blur")))
        val blur = current()
        layer(obj("op" to "attach_effect", "id" to blur, "owner" to owner))
        fun sample(source: String): org.json.JSONArray {
            val capture = Native.captureControl()
            try {
                val query = native { Native.inspectionTask(it, capture) }
                return JSONObject(Native.inspectionSample(query, source, 32.5f, 32.5f, 1)).getJSONObject("sample").getJSONArray("Color")
            } finally { Native.captureFree(capture) }
        }
        fun referencesMatch(): org.json.JSONArray {
            val visible = sample(sourceVisible)
            assertEquals("Clipped output includes its opaque base", 1.0, visible.getDouble(3), 1e-5)
            assertTrue("Attached blur spreads red across the blue stripe", visible.getDouble(0) > visible.getDouble(2))
            val expected = jsonValue(histogram())
            compose.runOnUiThread { host.dispatch(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "histogram", "visible" to true))) }; compose.waitForIdle()
            compose.waitUntil(10_000) { host.snapshot?.getJSONObject("layout")?.array("groups")?.objects()
                ?.any { "histogram" in it.array("panels").values() } == true }
            val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
                .first { "histogram" in it.array("panels").values() }.getInt("id")
            compose.runOnUiThread {
                host.dispatch(obj("type" to "select_panel_tab", "group" to group, "panel" to "histogram"))
                host.dispatch(obj("type" to "histogram", "action" to obj("type" to "source", "index" to 2)))
            }
            compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.getJSONObject("histogram")?.let {
                !it.isNull("data") && it.optString("status") == "Exact" && it.optString("captured_source") == "Reference"
            } == true }
            assertEquals("Reference statistics retain the clipped owner, attached blur and base", expected,
                jsonValue(host.snapshot!!.getJSONObject("state").getJSONObject("histogram").getJSONObject("data")))
            compose.runOnUiThread { host.dispatch(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "histogram", "visible" to false))) }; compose.waitForIdle()
            return visible
        }
        val ownerInput = sample(snapshotSource("EffectInput", blur))
        assertEquals("Attached input excludes outer owner opacity", 128.0 / 255.0, ownerInput.getDouble(3), 1e-5)
        assertTrue("Attached input retains the unblurred blue stripe", ownerInput.getDouble(2) > ownerInput.getDouble(0))
        val expected = referencesMatch()
        val results = org.json.JSONArray()
        for ((theme, target) in listOf("light" to owner, "light" to blur, "dark" to owner, "dark" to blur)) {
            val previous = host.filterPreviewCache.images["curves"]?.key
            action(obj("type" to "set_theme", "theme" to theme))
            layer(obj("op" to "select", "id" to target, "mask" to false))
            val header = host.snapshot!!.getJSONObject("header").getJSONObject("model").array("zones").values()
                .flatMap { (it as org.json.JSONArray).objects() }.first {
                it.getJSONObject("item").objectOrNull("control")?.optString("panel") == "adjustments"
            }.getInt("id")
            action(obj("type" to "activate_header_item", "id" to header))
            action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
            action(obj("type" to "filter_picker", "action" to obj("op" to "search", "query" to "Curves")))
            val started = SystemClock.uptimeMillis()
            compose.waitUntil(30_000) { host.filterPreviewCache.images["curves"]?.let { it.key != previous } == true }
            val preview = host.filterPreviewCache.images.getValue("curves").image.toPixelMap()
            val pixel = preview[preview.width / 2, preview.height / 2]
            assertEquals("Attached replacement preview retains owner-local alpha", 128f / 255f, pixel.alpha, .02f)
            assertTrue("Owner preview excludes the green clipping base", maxOf(pixel.red, pixel.blue) > pixel.green)
            results.put(obj("theme" to theme, "target" to target, "preview_elapsed_ms" to SystemClock.uptimeMillis() - started,
                "preview_alpha" to pixel.alpha, "visible_sample" to sample(sourceVisible)))
            instrumentation.uiAutomation.takeScreenshot()?.let { bitmap ->
                try { File(activity.getExternalFilesDir(null), "attached-motion-blur-preview-$theme-$target.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                finally { bitmap.recycle() }
            }
            action(obj("type" to "activate_header_item", "id" to header))
        }
        layer(obj("op" to "select", "id" to owner, "mask" to false))
        println("PASS attached previews in both themes and Reference statistics")
        val ripple = native { handle ->
            Native.dispatch(handle, obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "ripple")).toString())
            val id = state(handle).getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
            Native.dispatch(handle, obj("type" to "layer", "action" to obj("op" to "attach_effect", "id" to id, "owner" to owner)).toString())
            Native.dispatch(handle, obj("type" to "layer", "action" to obj("op" to "visibility", "id" to owner, "value" to false)).toString())
            id
        }
        refresh()
        assertTrue(row(ripple).getBoolean("visibility_blocked"))
        compose.waitUntil(30_000) { !tick() }
        SystemClock.sleep(100)
        assertFalse("An animated effect on a hidden owner stays idle", tick())
        layer(obj("op" to "delete", "id" to ripple))
        layer(obj("op" to "visibility", "id" to owner, "value" to true))
        layer(obj("op" to "rename", "id" to owner, "name" to "Renamed owner"))
        invoke("undo")
        println("PASS hidden attached animation is idle")
        val recovery = File(files, "attached-motion-blur-session")
        captureSession(recovery)
        val restore = restoreSessionTask(recovery)
        try { adoptSession(restore) } finally { Native.sessionFree(restore) }
        assertEquals(owner, row(blur).getJSONObject("relationship").getLong("target"))
        invoke("redo"); assertEquals("Renamed owner", row(owner).getString("label"))
        invoke("undo"); assertEquals("Clipped owner", row(owner).getString("label"))
        val restored = referencesMatch()
        for (channel in 0..3) assertEquals("Recovery sample channel $channel", expected.getDouble(channel), restored.getDouble(channel), 1e-5)
        assertNull(host.failure); assertNull(host.actionError)
        File(activity.getExternalFilesDir(null), "attached-filter-owner-journey.json").writeText(obj("previews" to results,
            "visible_sample" to restored, "reference_statistics" to "PASS", "owner" to row(owner), "effect" to row(blur), "recovery_history" to "PASS").toString(2))
    }

    @Test fun largePhotoFilterPreviews() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("filterPhoto") == "true")
        val photo = File(activity.filesDir, "filter-memory-test.jpg")
        assertTrue(photo.isFile)
        open(photo)
        scenario.onActivity { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("shaders_ready") == true }
        fun storage() = native {
            JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())).getLong("resident_bytes")
        }
        val tab = native { state(it).array("tabs").getJSONObject(0) }
        assertEquals(9504, tab.getInt("width")); assertEquals(6336, tab.getInt("height"))
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "adjustments", "visible" to true)))
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { "adjustments" in it.array("panels").values() }.getInt("id")
        val before = storage()
        val started = SystemClock.uptimeMillis()
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to false)))
        action(obj("type" to "select_panel_tab", "group" to group, "panel" to "adjustments"))
        compose.waitUntil(60_000) { host.filterPreviewCache.images["curves"] != null }
        val after = storage()
        val image = host.filterPreviewCache.images.getValue("curves").image.toPixelMap()
        assertTrue("The preview contains photo pixels", (0 until image.width).any { image[it, image.height / 2].alpha > .5f })
        assertTrue("Pointwise previews retain bounded scratch", after - before < 96L * 1024 * 1024)
        assertNull(host.failure)
        val result = obj("before_bytes" to before, "after_bytes" to after,
            "elapsed_ms" to (SystemClock.uptimeMillis() - started), "process_pss_kib" to android.os.Debug.getPss())
        File(activity.getExternalFilesDir(null), "filter-memory.json").writeText(result.toString(2))
        println("61 MP All filters: $result")
    }

    @Test fun largePhotoFilterPreviewDrawing() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("filterDrawing") == "true")
        open(File(activity.filesDir, "filter-memory-test.jpg"))
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("shaders_ready") == true }
        if (host.snapshot!!.getJSONObject("state").getJSONObject("workspace").optBoolean("zen_mode")) {
            action(obj("type" to "invoke", "command" to "zen_mode"))
        }
        action(obj("type" to "invoke", "command" to "fit_canvas"))
        action(obj("type" to "select_brush", "id" to 1))
        action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(1.0, 0.0, .7, .5))))
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "adjustments", "visible" to true)))
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { "adjustments" in it.array("panels").values() }.getInt("id")
        fun panel(visible: Boolean) {
            action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to !visible)))
            if (visible) {
                val current = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { it.getInt("id") == group }
                if (current.optString("active") != "adjustments") action(obj("type" to "select_panel_tab", "group" to group, "panel" to "adjustments"))
                // Selecting an already-active tab expands its controls. That
                // consumes the next canvas contact as dismissal, not painting.
                action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
            }
        }
        panel(false)
        // Warm the brush/source before comparing timed strokes. Motion uses real
        // OS stylus events and the host's real Choreographer; no manual frames.
        motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 60, 4752.0 to 3168.0)
        val runs = org.json.JSONArray()
        val output = File(activity.getExternalFilesDir(null), "filter-preview-drawing.json")
        for (visible in listOf(false, true, true, false)) {
            panel(visible)
            if (visible) {
                // Invalidate the source without changing its geometry, then
                // start drawing while the large-photo probe is still pending.
                action(obj("type" to "set_layer_opacity", "opacity" to if (runs.length() % 2 == 0) .99 else 1.0))
                compose.waitUntil(10_000) { host.filterPreviewCache.pending }
            }
            val previous = host.filterPreviewCache.images["curves"]?.key
            val beforeRevision = native { state(it).getJSONObject("document_file").getLong("revision") }
            val result = motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 180, 4752.0 to 3168.0)
            result.put("filters_visible", visible)
            val afterRevision = native { state(it).getJSONObject("document_file").getLong("revision") }
            assertTrue("The timed stylus contact must paint, not dismiss UI", afterRevision > beforeRevision)
            result.put("before_revision", beforeRevision).put("after_revision", afterRevision)
            if (visible) {
                val released = SystemClock.uptimeMillis()
                compose.waitUntil(60_000) {
                    host.filterPreviewCache.images["curves"]?.let { it.key != previous } == true
                }
                result.put("preview_after_release_ms", SystemClock.uptimeMillis() - released)
                assertNull(host.failure)
            }
            runs.put(result)
            output.writeText(obj("runs" to runs).toString(2))
        }
        println("Filter preview drawing results: ${output.absolutePath}")
        fun p95(values: List<Double>) = values.sorted()[((values.size - 1) * .95).toInt()]
        fun metric(run: JSONObject, field: String): Double {
            val timeline = run.getJSONObject("timeline")
            val frames = timeline.array("frames").values().map { it as org.json.JSONArray }
            return when (field) {
                "queue" -> p95(timeline.array("inputs").values().map { it as org.json.JSONArray }
                    .map { (it.getLong(2) - it.getLong(1)) / 1e6 })
                "cpu" -> p95(frames.map { it.getLong(10) / 1e6 })
                else -> p95(frames.zipWithNext { a, b -> (b.getLong(0) - a.getLong(0)) / 1e6 })
            }
        }
        val controls = runs.objects().filter { !it.getBoolean("filters_visible") }
        for (run in runs.objects().filter { it.getBoolean("filters_visible") }) {
            for ((field, tolerance) in listOf("queue" to 2.0, "cpu" to 2.0, "gap" to .5)) {
                assertTrue("Preview $field p95 must stay within $tolerance ms of the drawing control",
                    metric(run, field) <= controls.maxOf { metric(it, field) } + tolerance)
            }
            assertTrue("Preview must resume promptly after drawing", run.getLong("preview_after_release_ms") < 20_000)
        }
    }

    @Test fun largePhotoFilterPreviewLifecycle() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("filterPhoto") == "true")
        open(File(activity.filesDir, "filter-memory-test.jpg"))
        scenario.onActivity { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("shaders_ready") == true }
        action(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to null)))
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "adjustments", "visible" to true)))
        val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
            .first { "adjustments" in it.array("panels").values() }.getInt("id")
        fun collapsed(value: Boolean) = action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to value)))
        collapsed(false)
        action(obj("type" to "select_panel_tab", "group" to group, "panel" to "adjustments"))
        action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
        fun pending() = compose.waitUntil(10_000) { host.filterPreviewCache.pending }
        fun completed(previous: String? = null) {
            compose.waitUntil(20_000) { host.filterPreviewCache.images["curves"]?.let { it.key != previous } == true }
            assertNull(host.failure)
        }
        pending(); collapsed(true)
        compose.waitUntil(5000) { !host.filterPreviewCache.pending }
        val idleRequests = host.filterPreviewCache.request
        SystemClock.sleep(300)
        assertEquals("Hidden previews admit no new work", idleRequests, host.filterPreviewCache.request)
        collapsed(false); completed()

        var old = host.filterPreviewCache.images.getValue("curves").key
        action(obj("type" to "set_layer_opacity", "opacity" to .98)); pending()
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
        SystemClock.sleep(300)
        assertFalse("Background work stopped", host.filterPreviewCache.pending)
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        completed(old)

        old = host.filterPreviewCache.images.getValue("curves").key
        action(obj("type" to "set_layer_opacity", "opacity" to .97)); pending()
        scenario.onActivity { host.restartCanvas() }
        compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("shaders_ready") == true }
        completed(old)

        action(obj("type" to "set_layer_opacity", "opacity" to .96)); pending()
        val replacement = File(files, "filter-replacement.png")
        val bitmap = android.graphics.Bitmap.createBitmap(32, 32, android.graphics.Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(android.graphics.Color.GREEN)
        replacement.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
        bitmap.recycle()
        old = host.filterPreviewCache.images["curves"]?.key ?: ""
        val previousEpoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
        open(replacement); scenario.onActivity { host.documentChanged() }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch") != previousEpoch }
        completed(old)
        val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
        val tile = host.filterPreviewCache.images.getValue("curves")
        assertTrue("Only the replacement document is presented", tile.key.startsWith("$epoch:"))
        val image = tile.image.toPixelMap()
        assertTrue("Replacement pixels reached the native bitmap", (0 until image.width).any {
            val p = image[it, image.height / 2]; p.green > p.red + .5f && p.green > p.blue + .5f
        })
        assertNull(host.failure)
    }

    @Test fun sixteenBitWideColorSurvivesSaveAndGpuReplacement() {
        val job = native { handle ->
            val (id, file) = request(handle, "new_document")
            Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision"))
        }
        try {
            Native.projectOptions(job, obj("extent" to org.json.JSONArray(listOf(513, 257)),
                "color" to obj("space" to "ProPhoto", "depth" to "U16"), "background" to "White").toString())
            Native.projectWork(job, -1, 513, 257)
            native { Native.projectAdopt(it, job, "null") }
        } finally { Native.projectFree(job) }
        native { Native.dispatch(it, obj("type" to "invoke", "command" to "fit_canvas").toString()) }; tick()
        stroke(0.0)
        val before = manifest(save("wide16.capy"))
        assertEquals("pro_photo", before.compositionColor().optString("space","srgb"))
        assertEquals("u16", before.compositionColor().optString("depth","u8"))
        assertTrue(before.rasterResources().length() > 0)
        assertTrue(before.rasterResources().objects().all { it.getJSONObject("data").optString("depth","u8") == "u16" })
        native { Native.destroyGpuForTest(it) }; compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) { host.failure != null }
        compose.runOnUiThread { host.restartCanvas() }
        compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
        assertNull(host.failure)
        assertEquals(before.rasterResources().toString(), manifest(save("wide16-recovered.capy")).rasterResources().toString())
        open(File(files, "wide16.capy"))
        val after = manifest(save("wide16-reopened.capy"))
        assertEquals(before.compositionColor().toString(), after.compositionColor().toString())
        assertEquals(before.rasterResources().toString(), after.rasterResources().toString())
        val recipe = builtinRecipe(2).put("format", "Png")
        val output = png("wide16.png", recipe)
        assertEquals("PNG uses 16-bit samples", 16, output[24].toInt())
        open(File(files, "wide16.png"))
        val imported = manifest(save("wide16-image.capy"))
        val original = imported
        assertEquals("u16", original.originalImages().getJSONObject(0).getJSONObject("interpretation").optString("depth","u8"))
        val profiles = native { JSONObject(Native.query(it, obj("type" to "export_form").toString())).getJSONArray("profiles") }
        recipe.put("profile", profiles.getJSONObject(profiles.length()-1))
        for ((format, extension) in listOf("Png" to "png", "Tiff" to "tif")) {
            png("identity.$extension", JSONObject(recipe.toString()).put("format", format))
            open(File(files, "identity.$extension"))
            val restored = manifest(save("identity-$extension.capy"))
            assertEquals(original.originalTileIdentity(original.originalImages().getJSONObject(0)), restored.originalTileIdentity(restored.originalImages().getJSONObject(0)))
            assertEquals(original.profileIdentity(), restored.profileIdentity())
        }
        // Remove only interpretation chunks; the unchanged IDAT and its CRC
        // exercise Ask without introducing a second decoder or image codec.
        val untagged = java.io.ByteArrayOutputStream().apply {
            write(output, 0, 8); var offset = 8
            while (offset < output.size) {
                val size = ByteBuffer.wrap(output, offset, 4).order(ByteOrder.BIG_ENDIAN).int
                val type = output.copyOfRange(offset+4, offset+8).decodeToString()
                if (type !in listOf("iCCP", "sRGB", "gAMA", "cHRM", "cICP")) write(output, offset, size+12)
                offset += size+12
            }
        }.toByteArray()
        val untaggedFile = File(files, "untagged16.png").apply { writeBytes(untagged) }
        native { Native.dispatch(it, obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 1)).toString()) }
        val epoch = native { state(it).getJSONObject("document_file").getLong("epoch") }
        val pending = native { handle -> val (id, state) = request(handle, "open_document")
            Native.projectTask(handle, id, "null", state.getLong("epoch"), state.getLong("revision")) }
        try {
            Native.projectWork(pending, ParcelFileDescriptor.open(untaggedFile, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
            assertNotEquals("null", Native.projectProfilePrompt(pending))
            assertEquals(epoch, native { state(it).getJSONObject("document_file").getLong("epoch") })
            Native.projectAssumeProfile(pending, obj("Builtin" to "AdobeRgb").toString())
            Native.projectWork(pending, -1, 0, 0)
            compose.waitUntil(120_000) { tick(); native { Native.projectParkReady(it, pending) } }
            native { Native.projectAdopt(it, pending, "null") }
        } finally { Native.projectFree(pending) }
        val assumed = manifest(save("assumed16.capy"))
        assertEquals("adobe_rgb", assumed.compositionColor().optString("space","srgb"))
        assertEquals(original.originalTileIdentity(original.originalImages().getJSONObject(0)), assumed.originalTileIdentity(assumed.originalImages().getJSONObject(0)))
        native { Native.dispatch(it, obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 0)).toString()) }
    }
    @Test fun profileLibraryKeepsExactCopiesAndPresetOwnership() {
        val copy=host.catalog.getJSONObject("profile_copy")
        val wide=builtinRecipe(1)
        png("profile-library.png",wide);open(File(files,"profile-library.png"))
        val form=native {JSONObject(Native.query(it,obj("type" to "export_form").toString()))}
        val profile=form.getJSONArray("profiles").objects().first{it.getJSONObject("profile").has("Icc")}
        val array=profile.getJSONObject("profile").getJSONArray("Icc");val bytes=ByteArray(array.length()){array.getInt(it).toByte()}
        runBlocking {
            ProfileStore.import(activity,bytes);ProfileStore.import(activity,bytes)
            val entries=ProfileStore.list(activity);assertEquals(1,entries.size)
            val id=entries[0].getString("id");val file=File(AppStorage.of(activity).colorProfiles,"$id.icc")
            assertArrayEquals(bytes,file.readBytes())
            assertEquals(array.toString(),ProfileStore.get(activity,id).getJSONObject("profile").getJSONArray("Icc").toString())
            file.writeBytes(byteArrayOf(1,2,3));assertTrue(ProfileStore.list(activity)[0].has("issue"))
            try {ProfileStore.get(activity,id);fail("Corrupt profile was accepted")}catch(e:Exception){assertEquals("ProfileChanged",(e as ColorFeatureFailure).reason)}
            ProfileStore.import(activity,bytes)
            val color=native {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
            val recipe=ColorPreferencesStore.presets(activity,color,obj("type" to "get","index" to 0)).getJSONObject("recipe").put("profile",profile)
            val saved=ColorPreferencesStore.presets(activity,color,obj("type" to "save","name" to "Embedded library copy","recipe" to recipe))
            ProfileStore.remove(activity,id)
            assertTrue(ProfileStore.list(activity).isEmpty())
            assertEquals(array.toString(),ColorPreferencesStore.presets(activity,color,obj("type" to "get","index" to saved.getInt("index"))).getJSONObject("recipe").getJSONObject("profile").getJSONObject("profile").getJSONArray("Icc").toString())
            ProfileStore.import(activity,bytes)
        }
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("export_document")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText(copy.getString("saved_dialog")).fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText(copy.getString("saved_dialog")).performScrollTo().performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText(copy.getString("use_profile")).fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText(copy.getString("use_profile")).performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText(copy.getString("library_title")).fetchSemanticsNodes().isEmpty()}
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        DocumentController.nativeFileJobsForTest=true
        compose.runOnUiThread {host.dispatch(obj("type" to "open_settings","page" to "color"))}
        val manage=copy.getString("manage")
        compose.waitUntil(10_000) {compose.onAllNodesWithText(manage).fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText(manage).performScrollTo().performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText(copy.getString("remove")).fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText(copy.getString("remove")).performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText(copy.getString("empty")).fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithTag("profile-library-done").performClick()
        compose.runOnUiThread {host.dispatch(obj("type" to "close_settings"))}
        assertNull(host.failure)
    }

    @Test fun exportPresetsPersistAndRestoreEveryDeliveryChoice() {
        val color=native {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
        fun store(action:JSONObject)=runBlocking{ColorPreferencesStore.presets(activity,color,action)}
        fun canonical(recipe:JSONObject)=native{Native.query(it,obj("type" to "export_validate","recipe" to recipe).toString())}
        val recipe=store(obj("type" to "get","index" to 1)).getJSONObject("recipe")
            .put("depth","U16").put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(321,123)),"enlarge" to false))).put("resolution",obj("Ppi" to 287))
        val saved=store(obj("type" to "save","name" to "Tablet test delivery","recipe" to recipe))
        val index=saved.getInt("index");assertEquals(4,index)
        assertEquals(canonical(recipe),canonical(store(obj("type" to "get","index" to index)).getJSONObject("recipe")))
        val persisted=AppStorage.of(activity).exportPresets.readBytes()
        assertArrayEquals("CAPYPRESETS".toByteArray(Charsets.US_ASCII),persisted.copyOfRange(0,11))
        // Reopen through the real export dialog and load the persisted named recipe.
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("export_document")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Web / Share").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Web / Share").performClick()
        compose.onNodeWithText("Tablet test delivery").performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText("16-bit").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("16-bit").assertExists()
        compose.onNodeWithText("321").assertExists();compose.onNodeWithText("123").assertExists()
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        assertNull(host.failure)
    }

    @Test fun acceptedRecoveryCheckpointDrainsBeforeAdoption() {
        host.awaitReady(120_000,compose)
        compose.waitUntil(120_000) {tick();native {Native.sessionSettle(it,System.nanoTime());JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}}
        compose.waitUntil(120_000) {host.recovery.ready && !host.recovery.working}
        assertNull(host.failure);assertNull(host.actionError)
        val task=native { handle -> val(id,file)=request(handle,"new_document")
            Native.projectTask(handle,id,"null",file.getLong("epoch"),file.getLong("revision")) }
        val entered=java.util.concurrent.CountDownLatch(1)
        val release=java.util.concurrent.CountDownLatch(1)
        var blocker:Job?=null;var transition:Job?=null
        try {
            Native.projectWork(task,-1,128,128)
            blocker=runBlocking { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                host.viewModelScope.launch {host.withNative {entered.countDown();check(release.await(60,java.util.concurrent.TimeUnit.SECONDS))}}
            } }
            assertTrue(entered.await(10,java.util.concurrent.TimeUnit.SECONDS))
            val accepted=runBlocking { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {checkNotNull(host.recovery.capture())} }
            assertTrue(accepted.isActive)
            transition=runBlocking { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                host.viewModelScope.launch {host.drawingTabs.beforeAdopt(task)}
            } }
            compose.waitUntil(10_000) {host.drawingTabs.switching}
            assertSame(accepted,runBlocking { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.capture()} })
            assertFalse(transition.isCompleted)
            release.countDown();runBlocking {transition.join()}
            assertTrue(accepted.isCompleted)
            runBlocking { kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                try {host.withNative {Native.projectAdopt(it,task,"null")}}
                finally {host.drawingTabs.afterAdopt()}
            } }
            assertNull(host.failure);assertNull(host.actionError)
        } finally {
            release.countDown()
            runBlocking {blocker?.join();transition?.join()}
            Native.projectFree(task)
        }
    }

    @Test fun countFilterControlsStepAndPersistWholeValues() {
        val task = native { handle -> val (id,file)=request(handle,"new_document")
            Native.projectTask(handle,id,"null",file.getLong("epoch"),file.getLong("revision")) }
        try { Native.projectWork(task,-1,128,128); adoptProject(task) } finally { Native.projectFree(task) }
        refresh()
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to true)))
        val group=host.panelGroup("properties")
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group.getInt("id"), "collapsed" to false)))
        action(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to "properties"))
        for ((id,key,label) in listOf(Triple("posterize","levels","Levels"),Triple("kaleidoscope","segments","Segments"))) {
            action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to id)))
            val layerLabel=native {handle -> val layer=state(handle).getJSONObject("layer_properties").getLong("layer");state(handle).array("layers").objects().first {it.getLong("id")==layer}.getString("label")}
            fun control()=native { state(it).getJSONObject("layer_properties").array("controls").objects().first { c -> c.getString("key")==key } }
            fun value()=control().getJSONObject("value").getDouble("value")
            val numeric=control().getJSONObject("kind").getJSONObject("numeric")
            assertEquals(1.0,numeric.getDouble("step"),0.0);assertEquals(0,numeric.getInt("digits"))
            compose.onNodeWithTag("number-value-$key").performScrollTo().performClick()
            compose.onNodeWithTag("number-$label").performTextReplacement("7")
            compose.onNodeWithTag("number-$label").performKeyInput { pressKey(Key.DirectionUp) }
            compose.onNodeWithTag("number-$label").performImeAction()
            compose.waitUntil(30_000) { value()==8.0 };refresh()
            val name="count-$id.capy";val saved=manifest(save(name))
            open(File(files,name));refresh()
            val reopenedLayer=native {state(it).array("layers").objects().single {row -> row.getString("label")==layerLabel}.getLong("id")}
            action(obj("type" to "select_layer", "id" to reopenedLayer))
            assertEquals(8.0,value(),0.0)
            assertEquals(saved.artworkRecords().toString(),manifest(save("count-reopened.capy")).artworkRecords().toString())
            assertNull(host.failure);assertNull(host.actionError)
        }
    }

    @Test fun pointwiseColorEffectsPersistAllParametersAndOriginalSource() {
        for ((space, depth) in listOf("Srgb" to "U8", "DisplayP3" to "U16", "ProPhoto" to "F16", "Srgb" to "F32")) {
            val task = native { handle ->
                val (id, file) = request(handle, "new_document")
                Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision"))
            }
            try {
                Native.projectOptions(task, obj("extent" to org.json.JSONArray(listOf(128, 128)), "color" to obj("space" to space, "depth" to depth), "background" to "White").toString())
                Native.projectWork(task, -1, 128, 128); adoptProject(task)
            } finally { Native.projectFree(task) }
            refresh(); invoke("fit_canvas"); stroke(0.0)
            png("p21-source.png", builtinRecipe(2).put("format", "Png").put("depth", "U8"))
            val imported = native { handle ->
                val (id, file) = request(handle, "import_image")
                Native.projectTask(handle, id, obj("uri" to "test:p21-source.png", "name" to "p21-source.png").toString(), file.getLong("epoch"), file.getLong("revision"))
            }
            try {
                Native.projectWork(imported, ParcelFileDescriptor.open(File(files, "p21-source.png"), ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                native { Native.projectAdopt(it, imported, "null") }
            } finally { Native.projectFree(imported) }
            refresh(); invoke("apply_transform")
            val original = sourceIdentity(manifest(save("p21-original.capy")))
            for (id in listOf("hue_saturation", "invert", "desaturate", "threshold", "photo_filter")) {
                send(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to id)))
                val layer = native { state(it).getJSONObject("layer_properties").getLong("layer") }
                fun set(key: String, kind: String, value: Any) = send(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to key, "value" to obj("kind" to kind, "value" to value))))
                when (id) {
                    "hue_saturation" -> {
                        for ((key, value) in listOf("hue" to 17, "saturation" to -23, "lightness" to 9, "colorize_hue" to 193, "colorize_saturation" to 44)) set(key, "number", value)
                        for (range in listOf("reds", "yellows", "greens", "cyans", "blues", "magentas")) for ((key, value) in listOf("hue" to 31, "saturation" to -27, "lightness" to 13, "center" to 73, "width" to 19, "feather" to 41)) set("${range}_$key", "number", value)
                        set("colorize", "toggle", true)
                    }
                    "threshold" -> set("threshold", "number", if (depth.startsWith("F")) 2.0 else .73)
                    "photo_filter" -> {
                        set("density", "number", 67); set("preserve_luminance", "toggle", false)
                        set("color", "color", obj("space" to "DisplayP3", "rgba" to org.json.JSONArray(listOf(.2, .7, .9, 1))))
                    }
                }
            }
            val name = "p21-$space-$depth.capy"
            val saved = manifest(save(name))
            assertEquals(original, sourceIdentity(saved))
            open(File(files, name)); refresh()
            val reopened = manifest(save("p21-reopened.capy"))
            assertEquals(saved.artworkRecords().toString(), reopened.artworkRecords().toString())
            assertEquals(saved.compositionColor().toString(), reopened.compositionColor().toString())
            assertEquals(original, sourceIdentity(reopened))
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            refresh()
            val recreated = manifest(save("p21-recreated.capy"))
            assertEquals(reopened.artworkRecords().toString(), recreated.artworkRecords().toString())
            assertEquals(original, sourceIdentity(recreated))
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun gaussianRadius85ControlsRetainPixelsAndHistory() {
        val recipe = builtinRecipe(2).put("format", "Png").put("depth", "U8")
        val records = org.json.JSONArray()
        fun properties() = native { state(it).getJSONObject("layer_properties") }
        fun radius() = properties().array("controls").objects().first { it.getString("key") == "sigma" }
        fun value() = radius().getJSONObject("value").getDouble("value")
        fun ready() { compose.waitUntil(120_000) { tick(); host.snapshot?.optBoolean("shaders_ready") == true && !native { Native.renderingPending(it) } }; refresh() }
        fun pixels(name: String) = hash(png(name, recipe))
        for (portrait in listOf(false, true)) for (theme in listOf("light", "dark")) for (effect in listOf("gaussian_blur", "unsharp_mask")) {
            if (portrait) device.portrait(scenario) else device.landscape(scenario)
            val task = native { handle -> val (id, file) = request(handle, "new_document")
                Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision")) }
            try {
                Native.projectOptions(task, obj("extent" to org.json.JSONArray(listOf(256,256)), "color" to obj("space" to "Srgb", "depth" to "U8"), "background" to "White").toString())
                Native.projectWork(task,-1,256,256); native { Native.projectAdopt(it,task,"null") }
            } finally { Native.projectFree(task) }
            refresh(); action(obj("type" to "set_theme", "theme" to theme)); invoke("fit_canvas"); ready()
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.08,.1,.15,1))))
            invoke("select_all"); invoke("fill_selection"); invoke("deselect"); invoke("pen")
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.8,.25,.08,1))))
            action(obj("type" to "set_brush_size", "value" to 64)); stroke(0.0); ready()
            pixels("p27-source.png")
            send(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effect))); ready()
            val initial = value(); val before = pixels("p27-before.png")
            assertEquals("Radius", radius().getString("label"))
            val numeric = radius().getJSONObject("kind").getJSONObject("numeric")
            assertEquals(85.0,numeric.getDouble("max"),.0001); assertEquals(21.0,numeric.getDouble("soft_max"),.0001)
            val memoryBefore = native { JSONObject(Native.rendererMemory(it)) }
            val began = SystemClock.elapsedRealtimeNanos()
            compose.onNodeWithTag("number-value-sigma").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-Radius")
            field.performTextReplacement("85"); field.performImeAction()
            compose.waitUntil(30_000) { value() == 85.0 }; ready()
            val completed = SystemClock.elapsedRealtimeNanos()
            val adjusted = pixels("p27-adjusted.png"); assertNotEquals(before,adjusted)
            invoke("undo"); ready(); assertEquals(initial,value(),.0001); assertEquals(before,pixels("p27-undo.png"))
            invoke("redo"); ready(); assertEquals(85.0,value(),.0001); assertEquals(adjusted,pixels("p27-redo.png"))
            val name = "p27-$effect-$theme-$portrait.capy"; val saved = manifest(save(name))
            open(File(files,name)); ready(); assertEquals(85.0,value(),.0001); assertEquals(adjusted,pixels("p27-reopened.png"))
            assertEquals(saved.artworkRecords().toString(),manifest(save("p27-reopened.capy")).artworkRecords().toString())
            records.put(obj("effect" to effect,"theme" to theme,"portrait" to portrait,"typed_radius" to value(),"typed_completion_ms" to (completed-began)/1e6,
                "memory_before" to memoryBefore,"memory_after" to native { JSONObject(Native.rendererMemory(it)) }))
            assertNull(host.failure); assertNull(host.actionError)
        }
        File(activity.getExternalFilesDir(null),"p27-native-journey.json").writeText(records.toString(2))
    }

    @Test fun localAdjustmentsUpdateStackedSourcesAndPersistExactOutput() {
        val recipe = builtinRecipe(2).put("format", "Png").put("depth", "U8")
        fun pixels(name: String) = hash(png(name, recipe))
        fun properties() = native { state(it).getJSONObject("layer_properties") }
        fun ready() {
            compose.waitUntil(120_000) {
                tick()
                val description = properties().getString("description")
                assertNotEquals("Could not update this adjustment.", description)
                description != "Updating…"
            }
            refresh(); assertNull(host.failure); assertNull(host.actionError)
        }
        fun select(layer: Long) { action(obj("type" to "select_layer", "id" to layer)); ready() }
        fun value(key: String) = properties().getJSONArray("controls").objects().first { it.getString("key") == key }.getJSONObject("value").getDouble("value")
        fun edit(key: String, literal: String) {
            val label = properties().getJSONArray("controls").objects().first { it.getString("key") == key }.getString("label")
            compose.onNodeWithTag("number-value-$key").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-$label")
            field.assertIsFocused(); field.performTextReplacement(literal); field.performImeAction()
            compose.waitUntil(30_000) { kotlin.math.abs(value(key) - literal.toDouble()) < .0001 }
            ready()
            compose.waitUntil(10_000) { activity.window.decorView.rootWindowInsets?.isVisible(android.view.WindowInsets.Type.ime()) != true }
        }
        fun paint(y: Double) {
            val camera = native { state(it).getJSONObject("camera") }
            val pan = camera.getJSONArray("translation"); val viewport = camera.getJSONArray("viewport")
            val zoom = camera.getDouble("zoom")
            fun contact(phase: Int, x: Double) = point(phase, pan.getDouble(0) + x * zoom - viewport.getDouble(0) * .5, pan.getDouble(1) + y * zoom - viewport.getDouble(1) * .5)
            contact(1, 80.0)
            for (i in 1..6) { SystemClock.sleep(10); contact(2, 80.0 + i * 12) }
            contact(3, 152.0)
        }
        for (theme in listOf("light", "dark")) {
            val task = native { handle -> val (id, file) = request(handle, "new_document")
                Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision")) }
            try {
                Native.projectOptions(task, obj("extent" to org.json.JSONArray(listOf(256, 256)), "color" to obj("space" to "Srgb", "depth" to "U8"), "background" to "White").toString())
                Native.projectWork(task, -1, 256, 256); native { Native.projectAdopt(it, task, "null") }
            } finally { Native.projectFree(task) }
            refresh(); invoke("fit_canvas"); action(obj("type" to "set_theme", "theme" to theme))
            val sourceLayer = native { state(it).getJSONObject("layer_properties").getLong("layer") }
            action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.02,.02,.02,1))))
            invoke("select_all"); invoke("fill_selection"); invoke("deselect"); invoke("pen")
            action(obj("type" to "set_brush_size", "value" to 64))
            for ((index, color) in listOf(listOf(.12,.08,.2,1), listOf(.7,.4,.18,1), listOf(.25,.7,.45,1)).withIndex()) {
                action(obj("type" to "set_color", "rgba" to org.json.JSONArray(color))); paint(96.0 + index * 32)
            }
            val original = pixels("local-$theme-source.png")
            val layers = mutableListOf<Long>()
            for ((effect, keys) in listOf("shadows_highlights" to listOf("shadows", "highlights"), "clarity" to listOf("amount"), "dehaze" to listOf("amount"))) {
                send(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effect))); refresh(); ready()
                layers += properties().getLong("layer")
                assertEquals(keys, properties().getJSONArray("controls").objects().map { it.getString("key") })
                val before = pixels("local-$theme-$effect-before.png")
                edit(keys.first(), "63")
                val adjusted = pixels("local-$theme-$effect-adjusted.png")
                assertNotEquals("$effect must change source pixels", before, adjusted)
                invoke("undo"); ready(); assertEquals(0.0, value(keys.first()), .0001)
                assertEquals(before, pixels("local-$theme-$effect-undo.png"))
                invoke("redo"); ready(); assertEquals(63.0, value(keys.first()), .0001)
                assertEquals(adjusted, pixels("local-$theme-$effect-redo.png"))
                if (effect == "shadows_highlights") edit("highlights", "39")
                if (effect in listOf("clarity", "dehaze")) {
                    edit("amount", "-28")
                    assertNotEquals("Negative $effect changes the correction", adjusted, pixels("local-$theme-$effect-negative.png"))
                    edit("amount", "63")
                    assertEquals(adjusted, pixels("local-$theme-$effect-positive.png"))
                }
                SystemClock.sleep(300)
                instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
                    try { File(files, "local-$theme-$effect-controls.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() }
                }
            }
            val stacked = pixels("local-$theme-stacked.png"); assertNotEquals(original, stacked)
            select(sourceLayer); action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(.9,.15,.08,1)))); paint(144.0)
            for (layer in layers) select(layer)
            val painted = pixels("local-$theme-painted.png"); assertNotEquals(stacked, painted)
            invoke("undo"); for (layer in layers) select(layer)
            assertEquals(stacked, pixels("local-$theme-source-undo.png"))
            invoke("redo"); for (layer in layers) select(layer)
            assertEquals(painted, pixels("local-$theme-source-redo.png"))
            val name = "local-$theme.capy"; val saved = manifest(save(name))
            open(File(files, name)); refresh(); for (layer in layers) select(layer)
            assertEquals(painted, pixels("local-$theme-reopened.png"))
            val reopened = manifest(save("local-$theme-reopened.capy"))
            assertEquals(saved.artworkRecords().toString(), reopened.artworkRecords().toString())
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            refresh(); for (layer in layers) select(layer)
            assertEquals(painted, pixels("local-$theme-recreated.png"))
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun colorLookupImportPresetsAndResourceLifetime() {
        val fixture = File(requireNotNull(InstrumentationRegistry.getArguments().getString("lutArchive")) { "lutArchive must identify the private embedded-LUT fixture" })
        assertTrue(fixture.isFile)
        val measurements = org.json.JSONArray()
        fun memory() = android.os.Debug.MemoryInfo().also { android.os.Debug.getMemoryInfo(it) }.let { obj("pss_kb" to it.totalPss, "private_dirty_kb" to it.totalPrivateDirty) }
        val recipe = builtinRecipe(2).put("format", "Png").put("depth", "U8")
        fun pixels(name: String): List<Byte> {
            val bytes = png(name, recipe)
            File(activity.getExternalFilesDir(null), name).writeBytes(bytes)
            return hash(bytes)
        }
        fun assertClipboardPixels(expectedName: String, actualName: String, rgbTolerance: Int) {
            fun decode(name: String) = checkNotNull(android.graphics.BitmapFactory.decodeFile(File(activity.getExternalFilesDir(null), name).absolutePath,
                android.graphics.BitmapFactory.Options().apply { inPreferredConfig = android.graphics.Bitmap.Config.ARGB_8888; inPremultiplied = false; inScaled = false }))
            val expected = decode(expectedName)
            try {
                val actual = decode(actualName)
                try {
                    assertEquals("$actualName width", expected.width, actual.width)
                    assertEquals("$actualName height", expected.height, actual.height)
                    val reference = IntArray(expected.width * expected.height)
                    val pixels = IntArray(reference.size)
                    expected.getPixels(reference, 0, expected.width, 0, 0, expected.width, expected.height)
                    actual.getPixels(pixels, 0, actual.width, 0, 0, actual.width, actual.height)
                    for (i in reference.indices) {
                        assertEquals("$actualName alpha at pixel $i", reference[i] ushr 24, pixels[i] ushr 24)
                        for (shift in listOf(0, 8, 16)) {
                            val channel = (reference[i] ushr shift) and 255
                            val copied = (pixels[i] ushr shift) and 255
                            assertTrue("$actualName RGB channel ${shift / 8} at pixel $i: $channel != $copied", kotlin.math.abs(channel - copied) <= rgbTolerance)
                        }
                    }
                } finally { actual.recycle() }
            } finally { expected.recycle() }
        }
        fun resources(bytes: ByteArray): String {
            val index = manifest(bytes)
            val lookups = index.resourcesOf("capy.lut3d/1")
            assertTrue(lookups.length() > 0)
            for (payload in lookups.objects()) {
                assertTrue("Small LUT descriptor", payload.getJSONObject("data").toString().length < 4096)
                val location = payload.getJSONObject("location")
                val pack = packageMember(bytes,location.getString("pack"))
                val start = location.getString("offset").toInt()
                val binary = pack.copyOfRange(start,start+payload.getString("bytes").toInt())
                val crc = java.util.zip.CRC32().apply {update(binary)}.value.toString(16).padStart(8,'0')
                assertEquals(payload.getString("crc32"),crc)
            }
            return index.getJSONArray("resources").objects().map { payload ->
                val location = payload.getJSONObject("location"); val pack = packageMember(bytes,location.getString("pack"))
                val start = location.getString("offset").toInt()
                JSONObject(payload.toString()).apply {remove("location");put("payload_sha256",org.json.JSONArray(hash(pack.copyOfRange(start,start+payload.getString("bytes").toInt())).map {it.toInt() and 255}))}.toString()
            }.sorted().toString()
        }
        val original = manifest(fixture.readBytes())
        val resourceIdentity = resources(fixture.readBytes())
        val source = sourceIdentity(original)
        val authoredLookup = original.occurrenceRecords().objects().single {
            val effect = it.getJSONObject("data").getJSONObject("content").optJSONObject("effect") ?: return@single false
            val definition = original.packageData(original.packageData(effect.getString("ref")).getJSONObject("definition").getString("ref"))
            definition.optString("builtin") == "color_lookup"
        }
        val lookupName = authoredLookup.getJSONObject("data").optString("name")
        fun selectLookup() {
            val token = native { state(it).array("layers").objects().single { row -> row.getString("label") == lookupName }.getLong("id") }
            action(obj("type" to "select_layer", "id" to token))
        }
        for (portrait in listOf(false, true)) for (theme in listOf("light", "dark")) {
            if (portrait) device.portrait(scenario) else device.landscape(scenario)
            open(fixture); refresh(); invoke("fit_canvas")
            action(obj("type" to "set_theme", "theme" to theme))
            selectLookup()
            fun properties() = native { state(it).getJSONObject("layer_properties") }
            fun value(key: String) = properties().getJSONArray("controls").objects().first { it.getString("key") == key }.getJSONObject("value").getDouble("value")
            assertEquals(listOf("color_space", "intensity"), properties().getJSONArray("controls").objects().map { it.getString("key") })
            assertEquals(4, properties().getJSONArray("actions").objects().count { it.getJSONObject("action").getString("op") == "lookup_preset" })
            val baseline = pixels("p23-$theme-original.png")
            refresh()
            instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
                try { File(files, "p24-$theme-$portrait-controls.png").outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } } finally { bitmap.recycle() }
            }
            val retainedBefore = memory()
            for (preset in properties().getJSONArray("actions").objects().filter { it.getJSONObject("action").getString("op") == "lookup_preset" }) {
                val began = SystemClock.elapsedRealtimeNanos()
                compose.onNodeWithTag("property-resource-choice").performScrollTo().performClick()
                compose.onNode(hasText(preset.getString("label")) and hasAnyAncestor(isPopup())).performClick()
                refresh()
                assertFalse(properties().isNull("resource_selection"))
                pixels("p24-$theme-preset.png")
                measurements.put(obj("kind" to "preset", "preset" to preset.getString("label"), "portrait" to portrait, "theme" to theme, "completion_ms" to (SystemClock.elapsedRealtimeNanos()-began)/1e6))
                invoke("undo"); refresh()
                assertEquals(baseline, pixels("p24-$theme-preset-undo.png"))
            }
            val cube = File(files, "p24-native.cube").apply {
                writeText("TITLE \"Native inverse\"\nLUT_3D_SIZE 2\n" + (0..1).flatMap { blue -> (0..1).flatMap { green -> (0..1).map { red -> "${1-red} ${1-green} ${1-blue}\n" } } }.joinToString(""))
            }
            val picked = java.util.concurrent.atomic.AtomicReference<android.content.Intent>()
            var cancel = true
            var chosen = cube
            val monitor = object : android.app.Instrumentation.ActivityMonitor() {
                override fun onStartActivity(intent: android.content.Intent): android.app.Instrumentation.ActivityResult? {
                    if (intent.action != android.content.Intent.ACTION_OPEN_DOCUMENT) return null
                    picked.set(android.content.Intent(intent))
                    return if (cancel) android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_CANCELED, null)
                    else android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK, android.content.Intent().setData(android.net.Uri.fromFile(chosen)))
                }
            }
            instrumentation.addMonitor(monitor)
            try {
                DocumentController.nativeFileJobsForTest = false
                compose.onNodeWithTag("import-lookup").performScrollTo().performClick()
                compose.waitUntil(30_000) { picked.get() != null && !host.documents.working && host.documents.picker == null }
                assertEquals("*/*", picked.get().type)
                assertEquals(baseline, pixels("p24-$theme-cancel.png"))
                cancel = false; picked.set(null)
                chosen = File(files, "p24-picker-invalid.cube").apply { writeText("LUT_3D_SIZE 2\n0 0 0\n") }
                compose.onNodeWithTag("import-lookup").performScrollTo().performClick()
                compose.waitUntil(30_000) { picked.get() != null && !host.documents.working && host.hostError != null }
                compose.onNodeWithText(host.hostError!!).assertIsDisplayed()
                assertEquals(baseline, pixels("p24-$theme-picker-refused.png"))
                compose.runOnUiThread { host.dismissHostError() }
                chosen = cube; picked.set(null)
                val importBegan = SystemClock.elapsedRealtimeNanos()
                compose.onNodeWithTag("import-lookup").performScrollTo().performClick()
                compose.waitUntil(30_000) { picked.get() != null && !host.documents.working && properties().optString("resource_name") == "Native inverse" }
                DocumentController.nativeFileJobsForTest = true
                val imported = pixels("p24-$theme-imported.png")
                assertNotEquals(baseline, imported)
                measurements.put(obj("kind" to "import", "portrait" to portrait, "theme" to theme, "completion_ms" to (SystemClock.elapsedRealtimeNanos()-importBegan)/1e6, "before" to retainedBefore, "after" to memory()))
                File(activity.getExternalFilesDir(null), "p24-native-lut-timing.json").writeText(measurements.toString(2))
                invoke("undo"); assertEquals(baseline, pixels("p24-$theme-import-undo.png"))
                invoke("redo"); assertEquals(imported, pixels("p24-$theme-import-redo.png"))
                val importedFile = save("p24-$theme-imported.capy")
                open(File(files, "p24-$theme-imported.capy")); refresh()
                assertEquals(imported, pixels("p24-$theme-imported-reopened.png"))
                assertEquals(source, sourceIdentity(manifest(importedFile)))
                open(fixture); refresh(); selectLookup()
            } finally {
                DocumentController.nativeFileJobsForTest = true
                instrumentation.removeMonitor(monitor)
            }
            for (stale in listOf(false, true)) {
                val input = if (stale) cube else File(files, "p24-invalid.cube").apply { writeText("LUT_3D_SIZE 2\n0 0 0\n") }
                var requestId = 0
                val lookupTask = native { handle ->
                    Native.dispatch(handle, state(handle).getJSONObject("layer_properties").getJSONArray("actions").objects().first { it.getJSONObject("action").getString("op") == "import_lookup" }.getJSONObject("action").let { obj("type" to "effect", "action" to it) }.toString())
                    val pending = state(handle).array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }
                    requestId = pending.getInt("id")
                    val file = state(handle).getJSONObject("document_file")
                    Native.projectTask(handle, requestId, obj("uri" to "test:${input.name}", "name" to input.name).toString(), file.getLong("epoch"), file.getLong("revision"))
                }
                try {
                    if (stale) {
                        Native.projectWork(lookupTask, ParcelFileDescriptor.open(input, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                        val other = native { handle ->
                            val current = state(handle).getJSONObject("layer_properties").getLong("layer")
                            state(handle).array("layers").objects().first { it.getLong("id") != current }.getLong("id")
                        }
                        action(obj("type" to "select_layer", "id" to other))
                        val unchanged = native { state(it).getJSONObject("document_file").getLong("revision") }
                        native { Native.projectAdopt(it, lookupTask, "null") }
                        assertEquals(unchanged, native { state(it).getJSONObject("document_file").getLong("revision") })
                        selectLookup()
                    } else {
                        assertNotNull("Incomplete cube must reject", runCatching { Native.projectWork(lookupTask, ParcelFileDescriptor.open(input, ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0) }.exceptionOrNull())
                        native { Native.documentComplete(it, requestId, false, "null") }
                    }
                } finally { Native.projectFree(lookupTask) }
                assertEquals(baseline, pixels("p24-$theme-rejected-$stale.png"))
            }

            val before = value("intensity")
            assertTrue("Fixture LUT must be active", before > 0)
            compose.onNodeWithTag("number-value-intensity").performScrollTo().performClick()
            val field = compose.onNodeWithTag("number-Intensity")
            field.performTextReplacement("0"); field.performImeAction()
            compose.waitUntil(30_000) { value("intensity") == 0.0 }
            val unadjusted = pixels("p23-$theme-zero.png")
            assertNotEquals("Embedded LUT changes pixels", baseline, unadjusted)
            invoke("undo"); assertEquals(before, value("intensity"), .0001)
            assertEquals(baseline, pixels("p23-$theme-undo.png"))
            invoke("redo"); assertEquals(0.0, value("intensity"), .0001)
            assertEquals(unadjusted, pixels("p23-$theme-redo.png")); invoke("undo")
            val beforeSpace = value("color_space")
            val selectedSpace = if (beforeSpace == 0.0) 1.0 else 0.0
            compose.onNodeWithTag("property-color_space").performScrollTo().performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .8f, height * .5f)) }
            compose.onNode(hasText(if (selectedSpace == 1.0) "Display P3" else "sRGB") and hasAnyAncestor(isPopup())).performClick()
            compose.waitUntil(30_000) { value("color_space") == selectedSpace }
            invoke("undo"); assertEquals(beforeSpace, value("color_space"), .0001)
            var task = 0L; var clip = 0L
            val control = Native.captureControl()
            try {
                val id = native { handle -> request(handle, "copy_merged").first.also { task = Native.clipTask(handle, it) } }
                val consumed = task; task = 0
                clip = Native.clipRun(consumed, control, "p23-$theme")
                Native.clipWritePng(clip, File(activity.getExternalFilesDir(null), "p23-$theme-copy.png").absolutePath)
                assertClipboardPixels("p23-$theme-original.png", "p23-$theme-copy.png", 0)
                val adopted = clip; clip = 0
                native { Native.clipAdopt(it, id, adopted) }
                native { handle -> Native.pasteClip(handle, request(handle, "paste_in_place").first) }
                refresh(); pixels("p23-$theme-paste.png")
                assertClipboardPixels("p23-$theme-original.png", "p23-$theme-paste.png", 2)
                invoke("undo")
            } finally {
                if (task != 0L) Native.clipTaskFree(task)
                if (clip != 0L) Native.clipFree(clip)
                Native.captureFree(control)
            }
            val name = "p23-$theme.capy"
            val saved = save(name)
            assertEquals(resourceIdentity, resources(saved)); assertEquals(source, sourceIdentity(manifest(saved)))
            open(File(files, name)); refresh()
            assertEquals(baseline, pixels("p23-$theme-reopened.png"))
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            refresh()
            assertEquals(baseline, pixels("p23-$theme-recreated.png"))
            val recreated = save("p23-$theme-recreated.capy")
            assertEquals(resourceIdentity, resources(recreated)); assertEquals(source, sourceIdentity(manifest(recreated)))
            assertEquals(original.compositionColor().toString(), manifest(recreated).compositionColor().toString())
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun selectiveColorAndMixerPersistHiddenParametersAndOriginalSource() {
        for ((space, depth) in listOf("Srgb" to "U8", "DisplayP3" to "U16", "ProPhoto" to "F16", "Srgb" to "F32")) {
            val task = native { handle ->
                val (id, file) = request(handle, "new_document")
                Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision"))
            }
            try {
                Native.projectOptions(task, obj("extent" to org.json.JSONArray(listOf(128, 128)), "color" to obj("space" to space, "depth" to depth), "background" to "White").toString())
                Native.projectWork(task, -1, 128, 128); native { Native.projectAdopt(it, task, "null") }
            } finally { Native.projectFree(task) }
            refresh(); invoke("fit_canvas"); stroke(0.0)
            png("p22-source.png", builtinRecipe(2).put("format", "Png").put("depth", "U8"))
            val imported = native { handle ->
                val (id, file) = request(handle, "import_image")
                Native.projectTask(handle, id, obj("uri" to "test:p22-source.png", "name" to "p22-source.png").toString(), file.getLong("epoch"), file.getLong("revision"))
            }
            try {
                Native.projectWork(imported, ParcelFileDescriptor.open(File(files, "p22-source.png"), ParcelFileDescriptor.MODE_READ_ONLY).detachFd(), 0, 0)
                native { Native.projectAdopt(it, imported, "null") }
            } finally { Native.projectFree(imported) }
            refresh(); invoke("apply_transform")
            val original = sourceIdentity(manifest(save("p22-original.capy")))
            val originalPixels = hash(png("p22-original.png", builtinRecipe(2).put("format", "Png").put("depth", "U8")))
            for (id in listOf("selective_color", "channel_mixer")) {
                send(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to id)))
                val layer = native { state(it).getJSONObject("layer_properties").getLong("layer") }
                fun set(key: String, kind: String, value: Any) = send(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer, "key" to key, "value" to obj("kind" to kind, "value" to value))))
                if (id == "selective_color") {
                    for (range in listOf("reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks"))
                        for ((key, value) in listOf("cyan" to -17.25, "magenta" to 23.5, "yellow" to -31.75, "black" to 9.25)) set("${range}_$key", "number", value)
                    set("mode", "choice", 1)
                } else {
                    for (channel in listOf("red", "green", "blue", "gray"))
                        for ((key, value) in listOf("red" to 83.25, "green" to -12.5, "blue" to 24.75, "constant" to 3.25)) set("${channel}_$key", "number", value)
                    set("monochrome", "toggle", true)
                }
            }
            val pixels = hash(png("p22-adjusted.png", builtinRecipe(2).put("format", "Png").put("depth", "U8")))
            assertNotEquals(originalPixels, pixels)
            val name = "p22-$space-$depth.capy"
            val saved = manifest(save(name))
            assertEquals(original, sourceIdentity(saved))
            open(File(files, name)); refresh()
            val reopened = manifest(save("p22-reopened.capy"))
            assertEquals(saved.artworkRecords().toString(), reopened.artworkRecords().toString())
            assertEquals(saved.compositionColor().toString(), reopened.compositionColor().toString())
            assertEquals(original, sourceIdentity(reopened))
            assertEquals(pixels, hash(png("p22-reopened.png", builtinRecipe(2).put("format", "Png").put("depth", "U8"))))
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            refresh()
            val recreated = manifest(save("p22-recreated.capy"))
            assertEquals(reopened.artworkRecords().toString(), recreated.artworkRecords().toString())
            assertEquals(original, sourceIdentity(recreated))
            assertEquals(pixels, hash(png("p22-recreated.png", builtinRecipe(2).put("format", "Png").put("depth", "U8"))))
            assertNull(host.failure); assertNull(host.actionError)
        }
    }


    @Test fun retainedPlacePasteAndDocumentDetails() {
        fun fresh(space:String, depth:String) {
            val task=native { h -> val(id,f)=request(h,"new_document")
                Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision")) }
            try {
                Native.projectOptions(task,obj("extent" to org.json.JSONArray(listOf(513,257)),"color" to obj("space" to space,"depth" to depth),"background" to "White").toString())
                Native.projectWork(task,-1,513,257); native {Native.projectAdopt(it,task,"null")}
            } finally {Native.projectFree(task)}
            native {Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString())};tick()
        }
        fresh("ProPhoto","U16");stroke(0.0)
        val recipe=builtinRecipe(2).put("format","Png")
        val pixels=png("placement-original.png",recipe)
        open(File(files,"placement-original.png"))
        val source=manifest(save("placement-original.capy"))
        fresh("DisplayP3","U8")
        val before=manifest(save("placement-master.capy"))
        val fileBefore=native {state(it).getJSONObject("document_file")}
        val task=native { h -> val(id,f)=request(h,"import_image")
            Native.projectTask(h,id,obj("uri" to "test:placement-original.png","name" to "placement-original.png").toString(),f.getLong("epoch"),f.getLong("revision")) }
        try {
            Native.projectWork(task,ParcelFileDescriptor.open(File(files,"placement-original.png"),ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
            native {Native.projectAdopt(it,task,"null")}
        } finally {Native.projectFree(task)}
        tick()
        val fileAfter=native {state(it).getJSONObject("document_file")}
        assertEquals(fileBefore.getLong("epoch"),fileAfter.getLong("epoch"))
        assertEquals(fileBefore.getJSONObject("location").toString(),fileAfter.getJSONObject("location").toString())
        native { Native.dispatch(it,obj("type" to "invoke","command" to "apply_transform").toString()) };tick()
        assertTrue(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        val placed=manifest(save("placement-result.capy"))
        assertEquals(before.compositionColor().toString(),placed.compositionColor().toString())
        assertEquals(source.originalIdentity(),placed.originalIdentity())
        assertEquals(source.profileIdentity(),placed.profileIdentity())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick()
        assertEquals(before.originalIdentity(),manifest(save("placement-undo.capy")).originalIdentity())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString())};tick()
        assertEquals(placed.originalIdentity(),manifest(save("placement-redo.capy")).originalIdentity())
        open(File(files,"placement-result.capy"))
        assertEquals(placed.originalIdentity(),manifest(save("placement-reopened.capy")).originalIdentity())
        val infoTask=native {Native.documentInfoTask(it)}
        val info=Native.documentInfo(infoTask)
        assertTrue(info.contains("Display P3"));assertTrue(info.contains("16-bit RGB"));assertTrue(info.contains("embedded ICC retained"))
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("document_properties")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("placement-original").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Done").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        // Exercise Android's real URI clipboard transport, without Bitmap decoding.
        val resolver=activity.contentResolver
        val values=android.content.ContentValues().apply {
            put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME,"capy-m2-paste-${System.nanoTime()}.png")
            put(android.provider.MediaStore.MediaColumns.MIME_TYPE,"image/png")
        }
        val uri=resolver.insert(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,values)!!
        val clipboard=activity.getSystemService(android.content.ClipboardManager::class.java)
        var previous:android.content.ClipData?=null
        try {
            resolver.openOutputStream(uri)!!.use {it.write(pixels)}
            compose.runOnUiThread {
                previous=clipboard.primaryClip
                clipboard.setPrimaryClip(android.content.ClipData.newUri(resolver,"Capy test images",uri).apply { addItem(android.content.ClipData.Item(uri)) })
                host.invoke("paste_image")
            }
            compose.waitUntil(30_000) {host.snapshot?.getJSONObject("state")?.getJSONArray("layers")?.length()==placed.occurrenceRecords().length()+2}
            compose.waitUntil(30_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
            assertNull(host.actionError)
            pressCanvasBar("apply_transform")
            compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.first { it.getString("id")=="placement_original_size" }?.getBoolean("enabled")==false }
            DocumentController.nativeFileJobsForTest=true
            val pasted=manifest(save("placement-pasted.capy"))
            assertEquals("u8",pasted.compositionColor().optString("depth","u8"))
            assertEquals(placed.occurrenceRecords().length()+2,pasted.occurrenceRecords().length())
            assertEquals(placed.originalImages().length()+2,pasted.originalImages().length())
            for(image in pasted.originalImages().objects()) {
                assertEquals("u16",image.getJSONObject("interpretation").optString("depth","u8"))
                assertEquals(source.originalTileIdentity(source.originalImages().getJSONObject(0)),pasted.originalTileIdentity(image))
            }
            val retainedProfiles = pasted.profileRecordsById()
            assertEquals("ICC resource IDs are distinct",pasted.resourcesOf("capy.icc/1").length(),retainedProfiles.size)
            for ((id, record) in placed.profileRecordsById()) {
                assertEquals("Existing ICC resource $id survives unchanged",record,retainedProfiles[id])
            }
            assertEquals("Every pasted ICC resource retains the original contents",source.profileContentIdentity(),pasted.profileContentIdentity())
        } finally {
            DocumentController.nativeFileJobsForTest=true
            compose.runOnUiThread {previous?.let {clipboard.setPrimaryClip(it)} ?: clipboard.clearPrimaryClip()}
            resolver.delete(uri,null,null)
        }
        sourceEdits()
        assertNull(host.failure)
    }

    private fun sourceEdits() {
        fun change(command:String,profile:JSONObject?=null,apply:Boolean=true,cancel:Boolean=false):JSONObject? {
            val flag=Native.captureControl();if(cancel)Native.captureCancel(flag)
            val id=native {request(it,command).first};val task=native {Native.sourceTask(it,id,flag)}
            try {
                if(cancel){
                    try {Native.sourceWork(task,profile?.toString() ?: "null");fail("Cancelled source conversion succeeded")}
                    catch(e:Exception){assertTrue(e.message.orEmpty().contains("cancel",ignoreCase=true))}
                    native {Native.documentComplete(it,id,false,"null")};return null
                }
                Native.sourceWork(task,profile?.toString() ?: "null")
                native {Native.sourcePrepareComparison(it,task)}
                val stats=JSONObject(Native.sourceCompare(task))
                for(after in listOf(false,true))assertTrue(Native.sourcePreview(task,after).size>8)
                if(apply)native {Native.sourceAdopt(it,task)}else native {Native.documentComplete(it,id,false,"null")}
                tick();return stats
            }finally{Native.sourceFree(task);Native.captureFree(flag)}
        }
        fun backingEqual(a:JSONObject,b:JSONObject){
            assertEquals(a.rasterResources().toString(),b.rasterResources().toString())
            assertEquals(a.paintRecords().toString(),b.paintRecords().toString())
            assertEquals(a.originalIdentity(),b.originalIdentity())
        }
        fun activeSource(m:JSONObject):JSONObject = m.originalImages().getJSONObject(0)
        val before=manifest(save("source-before.capy"))
        change("rasterize_source",cancel=true);backingEqual(before,manifest(save("source-cancel-worker.capy")))
        change("repair_source_profile",obj("Builtin" to "AdobeRgb"),apply=false);backingEqual(before,manifest(save("source-cancel-preview.capy")))
        assertFalse(change("repair_source_profile",obj("Builtin" to "AdobeRgb"))!!.getBoolean("adds_layer"))
        val repaired=manifest(save("source-repaired.capy"))
        assertEquals(before.rasterResources().toString(),repaired.rasterResources().toString())
        assertEquals("adobe_rgb",activeSource(repaired).getJSONObject("interpretation").getJSONObject("profile").getString("builtin"))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick();backingEqual(before,manifest(save("source-repair-undo.capy")))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString())};tick();backingEqual(repaired,manifest(save("source-repair-redo.capy")))
        change("rasterize_source")
        val rasterized=manifest(save("source-rasterized.capy"));val image=activeSource(rasterized)
        assertEquals("rasterized",image.optString("role","original"));assertEquals("u8",image.getJSONObject("interpretation").optString("depth","u8"));assertEquals("display_p3",image.getJSONObject("interpretation").getJSONObject("profile").getString("builtin"))
        assertEquals(activeSource(repaired).getJSONArray("extent").toString(),image.getJSONArray("extent").toString())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick();backingEqual(repaired,manifest(save("source-rasterize-undo.capy")))
        native {
            Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString())
            Native.dispatch(it,obj("type" to "invoke","command" to "pen").toString())
        };tick();stroke(0.0)
        val painted=manifest(save("source-painted.capy"));val oldId=painted.occurrenceRecords().objects().first {it.getJSONObject("data").getJSONObject("content").has("paint")}.getString("id")
        assertTrue(change("repair_source_profile",obj("Builtin" to "ProPhoto"))!!.getBoolean("adds_layer"))
        val added=manifest(save("source-corrected-layer.capy"))
        assertEquals(painted.occurrenceRecords().length()+1,added.occurrenceRecords().length())
        assertEquals(painted.occurrenceRecords().objects().first{it.getString("id")==oldId}.toString(),added.occurrenceRecords().objects().first{it.getString("id")==oldId}.toString())
        assertEquals(painted.rasterResources().toString(),added.rasterResources().toString())
        open(File(files,"source-corrected-layer.capy"));backingEqual(added,manifest(save("source-corrected-reopened.capy")))
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("rasterize_source")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Preview Complete Result").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Preview Complete Result").performClick()
        compose.waitUntil(60_000) {compose.onAllNodesWithContentDescription("Prepared composition").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        DocumentController.nativeFileJobsForTest=true
        backingEqual(added,manifest(save("source-ui-cancel.capy")))
    }

    @Test fun documentColorChangesPreserveExactHistoryAndCancel() {
        val fresh = native { handle -> val (id, f) = request(handle, "new_document")
            Native.projectTask(handle, id, "null", f.getLong("epoch"), f.getLong("revision")) }
        try {
            Native.projectOptions(fresh, obj("extent" to org.json.JSONArray(listOf(513,257)), "color" to obj("space" to "DisplayP3", "depth" to "U16"), "background" to "White").toString())
            Native.projectWork(fresh,-1,513,257); native { Native.projectAdopt(it,fresh,"null") }
        } finally { Native.projectFree(fresh) }
        native { Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString())
            Native.dispatch(it,obj("type" to "color","action" to obj("op" to "set_slot", "slot" to "foreground", "color" to obj("space" to "DisplayP3","rgba" to org.json.JSONArray(listOf(.8,.2,.1,1.0))))).toString()) }
        tick();stroke(0.0)
        fun change(command:String, choice:JSONObject? = null, cancel:Boolean = false): JSONObject? {
            val flag=Native.captureControl(); if(cancel)Native.captureCancel(flag)
            val id=native {request(it,command).first};val task=native {Native.colorTask(it,id,flag)}
            try {
                if(cancel) {
                    try {Native.colorWork(task,choice.toString());fail("Cancelled color change succeeded")}
                    catch(e:Exception){assertTrue(e.message.orEmpty().contains("cancel",ignoreCase=true))}
                    native {Native.documentComplete(it,id,false,"null")};return null
                }
                val result=JSONObject(Native.colorWork(task,choice?.toString() ?: "null"))
                if(choice!=null) for(after in listOf(false,true)) {
                    val preview=Native.colorPreview(task,after)
                    val dimensions=ByteBuffer.wrap(preview).order(ByteOrder.LITTLE_ENDIAN)
                    val width=dimensions.int;val height=dimensions.int
                    assertEquals(8+width*height*4,preview.size);assertTrue(width<=512 && height<=384)
                }
                native {Native.colorAdopt(it,task)};tick();return result
            } finally {Native.colorFree(task);Native.captureFree(flag)}
        }
        fun sameBacking(a:JSONObject,b:JSONObject) {
            assertEquals(a.rasterResources().toString(),b.rasterResources().toString())
            assertEquals(a.compositionColor().toString(),b.compositionColor().toString())
        }
        val before=manifest(save("color-before.capy"))
        change("assign_profile",obj("Assign" to "AdobeRgb"),true)
        sameBacking(before,manifest(save("color-cancel.capy")))
        change("assign_profile",obj("Assign" to "AdobeRgb"))
        val assigned=manifest(save("color-assigned.capy"))
        assertEquals(before.rasterResources().toString(),assigned.rasterResources().toString())
        assertEquals("adobe_rgb",assigned.compositionColor().optString("space","srgb"))
        change("undo");sameBacking(before,manifest(save("color-undo.capy")))
        change("redo");sameBacking(assigned,manifest(save("color-redo.capy")))
        change("convert_color_space",obj("Convert" to obj("space" to "ProPhoto","options" to obj("intent" to "RelativeColorimetric","black_point_compensation" to false))))
        val converted=manifest(save("color-converted.capy"))
        assertEquals("pro_photo",converted.compositionColor().optString("space","srgb"))
        assertNotEquals(assigned.rasterResources().toString(),converted.rasterResources().toString())
        change("change_bit_depth",obj("Depth" to obj("depth" to "U8","dither" to "None")))
        val reduced=manifest(save("color-depth.capy"))
        assertTrue(reduced.rasterResources().objects().all {it.getJSONObject("data").optString("depth","u8")=="u8"})
        change("undo");sameBacking(converted,manifest(save("color-depth-undo.capy")))
        change("redo");sameBacking(reduced,manifest(save("color-depth-redo.capy")))

        val copyFile=File(files,"converted-copy.capy")
        val originalState=native {state(it).getJSONObject("document_file")}
        val flag=Native.captureControl()
        val copyId=native {request(it,"convert_color_space").first}
        val copyTask=native {Native.colorTask(it,copyId,flag)}
        try {
            Native.colorWork(copyTask,obj("Convert" to obj("space" to "Srgb","options" to obj("intent" to "RelativeColorimetric","black_point_compensation" to false))).toString(),true)
            assertTrue(Native.colorPreview(copyTask,true).size>8)
            Native.colorWriteCopy(copyTask,ParcelFileDescriptor.open(copyFile,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd())
            try {native {Native.colorAdopt(it,copyTask)};fail("Copy adopted over master")}catch(e:Exception){assertTrue(e.message.orEmpty().contains("separate document"))}
            native {Native.documentComplete(it,copyId,true,"null")}
        } finally {Native.colorFree(copyTask);Native.captureFree(flag)}
        val afterCopy=native {state(it).getJSONObject("document_file")}
        for(key in listOf("epoch","revision","modified","location"))assertEquals(originalState.get(key).toString(),afterCopy.get(key).toString())
        sameBacking(reduced,manifest(save("copy-master-unchanged.capy")))
        val copied=manifest(copyFile.readBytes())
        assertEquals(1,copied.occurrenceRecords().length())
        assertEquals("srgb",copied.compositionColor().optString("space","srgb"))
        assertEquals("u8",copied.compositionColor().optString("depth","u8"))
        assertEquals("rasterized",copied.originalImages().getJSONObject(0).optString("role","original"))
        open(copyFile);sameBacking(copied,manifest(save("copy-reopened.capy")))

        open(File(files,"color-depth.capy"));sameBacking(reduced,manifest(save("color-reopened.capy")))
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("convert_color_space")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Preview Complete Result").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithTag("color-choice-Result").performScrollTo().performClick()
        compose.onNodeWithText("Save flattened copy").performClick()
        compose.onNodeWithText("Preview Complete Result").performScrollTo().performClick()
        compose.waitUntil(60_000) {compose.onAllNodesWithContentDescription("Prepared composition").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        DocumentController.nativeFileJobsForTest=true
        sameBacking(reduced,manifest(save("color-ui-cancel.capy")))
        assertNull(host.failure)
    }

    @Test fun boundedStatisticsKeepFrozenSourcesAcrossEditsAndRecreation() {
        val input = File(files, "precision-sources.png")
        val bitmap = android.graphics.Bitmap.createBitmap(257, 129, android.graphics.Bitmap.Config.ARGB_8888)
        try {
            val pixels = IntArray(257 * 129) { index ->
                val x = index % 257
                android.graphics.Color.argb(if (x < 32) 0 else 255, x % 256, (x * 3) % 256, (index / 257 * 7) % 256)
            }
            bitmap.setPixels(pixels, 0, 257, 0, 0, 257, 129)
            input.outputStream().use { assertTrue(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)) }
        } finally { bitmap.recycle() }
        open(input); refresh()
        fun ready() { compose.waitUntil(60_000) { !tick() }; refresh() }
        fun inspect(source: String, waveform: Boolean = false): JSONObject {
            val control = Native.captureControl()
            try {
                val task = native { Native.inspectionTask(it, control) }
                return JSONObject(Native.inspectionStatistics(task, source, false, false, waveform))
            } finally { Native.captureFree(control) }
        }
        fun properties() = native { state(it).getJSONObject("layer_properties") }
        fun waveformPresented() {
            val chart = hasTestTag("scope-waveform-chart")
            compose.onNode(chart and hasAnyAncestor(hasTestTag("panel-body-waveform"))).assertIsDisplayed()
            val customization = host.snapshot!!.getJSONObject("state").getJSONObject("customization")
            val drawers = customization.array("column_drawers").objects().map {
                "column-drawer-${it.getJSONObject("anchor").getInt("column")}" to it
            } + listOfNotNull(customization.objectOrNull("drawer")?.let { "tool-drawer" to it })
            for ((tag, drawer) in drawers) if (drawer.array("columns").values().any { "waveform" in (it as org.json.JSONArray).values() }) {
                val node = compose.onNode(chart and hasAnyAncestor(hasTestTag(tag))).performScrollTo().assertIsDisplayed()
                val bounds = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
                val plot = node.fetchSemanticsNode().boundsInRoot
                assertTrue("The live Waveform fits its shared $tag owner", bounds.contains(plot.topLeft)
                    && bounds.contains(plot.bottomRight - androidx.compose.ui.geometry.Offset(1f, 1f)))
            }
        }
        fun exposure(value: Double) = action(obj("type" to "effect", "action" to obj(
            "op" to "set", "layer" to properties().getLong("layer"), "key" to "exposure",
            "value" to obj("kind" to "number", "value" to value))))
        ready()
        val paintTarget = native { JSONObject(Native.imageImportContext(it, "null", "null")).getJSONObject("placement").getJSONObject("target") }
        assertTrue("The imported original has its own paint source", paintTarget.has("Paint"))
        val layerSource = snapshotSource(paintTarget)
        val source = sourceIdentity(manifest(save("precision-source.capy")))
        val layerHistogram = inspect(layerSource).getJSONObject("histogram").toString()
        val lower = inspect(sourceVisible).getJSONObject("histogram").toString()
        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "exposure")))
        val effect = properties().getLong("layer")
        val inputSource = snapshotSource("EffectInput", effect)
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); exposure(0.0); ready()
            val committed = native { state(it).getJSONObject("document_file") }
            val control = Native.captureControl()
            try {
                val frozen = native { Native.inspectionTask(it, control) }
                exposure(1.0); ready()
                val result = JSONObject(Native.inspectionStatistics(frozen, sourceVisible, false, false, false))
                assertEquals(committed.getLong("epoch"), result.getLong("epoch"))
                assertEquals(committed.getLong("revision"), result.getLong("revision"))
                assertTrue(result.getDouble("time").isFinite())
                assertEquals(lower, result.getJSONObject("histogram").toString())
            } finally { Native.captureFree(control) }
            assertEquals(lower, inspect(inputSource).getJSONObject("histogram").toString())
            assertEquals(layerHistogram, inspect(layerSource).getJSONObject("histogram").toString())
            assertNotEquals(lower, inspect(sourceVisible).getJSONObject("histogram").toString())
            val exact = jsonValue(inspect(sourceVisible, true).getJSONObject("histogram"))
            action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "waveform", "visible" to true)))
            val scopeGroup = host.snapshot!!.getJSONObject("layout").array("groups").objects()
                .single { "waveform" in it.array("panels").values() }.getInt("id")
            action(obj("type" to "select_panel_tab", "group" to scopeGroup, "panel" to "waveform"))
            action(obj("type" to "histogram", "action" to obj("type" to "source", "index" to 0)))
            ready()
            waveformPresented()
            compose.waitUntil(60_000) {
                host.snapshot?.getJSONObject("state")?.getJSONObject("waveform")?.let { it.optJSONObject("data") != null && it.optString("status") == "Exact" } == true
            }
            assertEquals("Live Waveform preserves every independent exact count", exact,
                jsonValue(host.snapshot!!.getJSONObject("state").getJSONObject("waveform").getJSONObject("data")))
            waveformPresented()
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            ready()
            compose.waitUntil(60_000) {
                host.snapshot?.getJSONObject("state")?.getJSONObject("waveform")?.let { it.optJSONObject("data") != null && it.optString("status") == "Exact" } == true
            }
            assertEquals("Live Waveform preserves every independent exact count", exact,
                jsonValue(host.snapshot!!.getJSONObject("state").getJSONObject("waveform").getJSONObject("data")))
            waveformPresented()
            assertEquals(exact, jsonValue(inspect(sourceVisible, true).getJSONObject("histogram")))
            assertEquals(source, sourceIdentity(manifest(save("precision-$theme-recreated.capy"))))
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

    @Test fun histogramCapturesCommittedDocumentAndCancelsIndependently() {
        val epoch = native { state(it).getJSONObject("document_file").getLong("epoch") }
        for (cancelled in listOf(true, false)) {
            val control = Native.captureControl()
            try {
                if (cancelled) Native.captureCancel(control)
                val committed = native { state(it).getJSONObject("document_file") }
                val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
                val task = native { Native.inspectionTask(it, control) }
                try {
                    val result = JSONObject(Native.inspectionStatistics(task, sourceVisible, false, false, false))
                    if (cancelled) fail("Cancelled histogram completed")
                    assertEquals(committed.getLong("epoch"), result.getLong("epoch"))
                    assertEquals(committed.getLong("revision"), result.getLong("revision"))
                    assertTrue(result.getDouble("time").isFinite())
                    val histogram = result.getJSONObject("histogram")
                    assertEquals(color.toString(), histogram.getJSONObject("color").toString())
                    assertEquals(2048L*1536L, histogram.getLong("pixels"))
                    assertEquals(0L, histogram.getLong("transparent"))
                    for (channel in histogram.getJSONArray("channels").objects()) {
                        assertEquals(histogram.getLong("pixels"), channel.getLong("white"))
                        assertEquals(0L, channel.getLong("above"))
                    }
                } catch (e: IllegalStateException) { if (!cancelled) throw e; assertTrue(e.message.orEmpty().contains("cancel", ignoreCase=true)) }
            } finally { Native.captureFree(control) }
        }
        assertEquals(epoch, native { state(it).getJSONObject("document_file").getLong("epoch") })
        val outputControl = Native.captureControl()
        val id = native { request(it, "export_document").first }
        Native.captureCancel(outputControl)
        val outputTask = native { Native.projectExportTask(it, id, System.nanoTime(), outputControl) }
        assertNotEquals(0L, outputTask)
        try {
            val file = File(files, "cancelled-output.png")
            try {
                Native.projectWork(outputTask, ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(), 0, 0)
                fail("Cancelled output completed")
            } catch (e: IllegalStateException) { assertTrue(e.message.orEmpty().contains("cancel", ignoreCase=true)) }
            native { Native.documentComplete(it, id, false, "null") }
            assertEquals(0L, file.length())
        } finally { Native.projectFree(outputTask); Native.captureFree(outputControl) }
        val recipe=builtinRecipe(0)
            .put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(128,128)),"enlarge" to false)))
        for(cancelled in listOf(false,true)) {
            val flag=Native.captureControl();if(cancelled)Native.captureCancel(flag)
            val task=native {Native.inspectionTask(it,flag)}
            try {
                val result=Native.inspectionOutput(task,recipe.toString())
                if(cancelled)fail("Cancelled output preview succeeded")
                val image=result[2] as ByteArray;val header=ByteBuffer.wrap(image).order(ByteOrder.LITTLE_ENDIAN)
                assertEquals(128,header.int);assertEquals(96,header.int)
                assertTrue(image.drop(8).all{(it.toInt() and 255)==255})
            }catch(e:Exception){if(!cancelled)throw e;assertTrue(e.message.orEmpty().contains("cancel",ignoreCase=true))}
            finally{Native.captureFree(flag)}
        }
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("export_document")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Preview Output").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Preview Output").performScrollTo().performClick()
        compose.waitUntil(60_000) {compose.onAllNodesWithContentDescription("Output preview").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        DocumentController.nativeFileJobsForTest=true
        assertEquals(epoch,native {state(it).getJSONObject("document_file").getLong("epoch")})
        fun scopeVisible(panel: String, visible: Boolean) {
            compose.runOnUiThread {
                host.dispatch(obj("type" to "customize", "action" to obj(
                    "type" to "set_panel_visible", "panel" to panel, "visible" to visible)))
            }
            compose.waitForIdle()
        }
        fun revealHistogram() {
            scopeVisible("histogram", true)
            compose.waitUntil(10_000) {
                host.snapshot?.getJSONObject("layout")?.array("groups")?.objects()
                    ?.any { "histogram" in it.array("panels").values() } == true
            }
            val group = host.snapshot!!.getJSONObject("layout").array("groups").objects()
                .first { "histogram" in it.array("panels").values() }.getInt("id")
            compose.runOnUiThread {
                host.dispatch(obj("type" to "select_panel_tab", "group" to group, "panel" to "histogram"))
                host.dispatch(obj("type" to "histogram", "action" to obj("type" to "source", "index" to 0)))
            }
        }
        fun scope() = host.snapshot?.getJSONObject("state")?.getJSONObject("histogram")
        revealHistogram()
        compose.waitUntil(60_000) {
            scope()?.let { !it.isNull("data") && it.optString("status") == "Exact"
                && it.optString("captured_source") == "Visible" } == true
        }
        val exact = jsonValue(scope()!!.getJSONObject("data"))
        assertEquals(jsonValue(histogram()), exact)
        scopeVisible("waveform", false)
        scopeVisible("histogram", false)
        compose.waitUntil(10_000) { scope()?.isNull("data") == true }
        SystemClock.sleep(300)
        assertTrue("Hidden monitor consumers reject late data", scope()!!.isNull("data"))
        assertEquals(epoch, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch"))
        revealHistogram()
        compose.waitUntil(60_000) { scope()?.optString("status") == "Exact" && scope()?.isNull("data") == false }
        assertEquals(exact, jsonValue(scope()!!.getJSONObject("data")))
        assertNull(host.failure)
    }
    @Test fun navigationBuffersAndPenReturnsToFrontBuffer() {
        fun display() = native { JSONObject(Native.displayStatus(it)) }
        repeat(3) { index ->
            point(1, 0.0, index * 20.0)
            assertEquals("The first ink frame uses the front buffer", "SharedDemandRefresh", display().getString("present_mode"))
            for (step in 1..6) { SystemClock.sleep(10); point(2, step * 15.0, index * 20.0) }
            point(3, 90.0, index * 20.0)
            assertEquals("SharedDemandRefresh", display().getString("present_mode"))
            assertTrue(display().getBoolean("retained_target"))
            assertEquals(1, display().getInt("desired_maximum_frame_latency"))
            val painted = hash(png("presentation-before-$index.png"))
            val revision = native { state(it).getJSONObject("document_file").getLong("revision") }
            native { Native.dispatch(it, obj("type" to "invoke", "command" to "zoom_in").toString()) }
            compose.waitUntil(10_000) { !tick() }
            assertEquals("Fifo", display().getString("present_mode"))
            assertFalse(display().getBoolean("retained_target"))
            assertTrue(display().getInt("desired_maximum_frame_latency") >= 3)
            assertEquals(revision, native { state(it).getJSONObject("document_file").getLong("revision") })
            assertEquals(painted, hash(png("presentation-after-$index.png")))
            val pixels = native { Native.surfacePixelsForTest(it) }
            assertTrue("Buffered view contains artwork", (pixels.indices step 97).map { pixels[it] }.toSet().size > 8)
        }
        stroke(80.0)
        assertEquals("SharedDemandRefresh", display().getString("present_mode"))
        assertTrue(display().getBoolean("retained_target"))
        assertTrue(display().getLong("presentation_switches") >= 6)
        assertNull(host.failure)
    }
    @Test fun frontBufferSurfaceLifecycle() {
        fun ready() {
            compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.let { it.optBoolean("brush_ready") && it.optBoolean("shaders_ready") }==true }
            compose.waitUntil(10_000) { !tick() }
            assertNull(host.failure)
            val display=native { JSONObject(Native.displayStatus(it)) }
            assertTrue(display.getString("present_mode") in listOf("SharedDemandRefresh", "Fifo"))
            assertEquals(display.getString("present_mode") == "SharedDemandRefresh", display.getBoolean("retained_target"))
        }
        fun pendingBufferedFrame(transition: (Long) -> Unit) = native { handle ->
            val submitted=JSONObject(Native.displayStatus(handle)).getLong("submitted_frames")
            val zoom=state(handle).getJSONObject("camera").getDouble("zoom")
            Native.dispatch(handle,obj("type" to "set_zoom","zoom" to zoom*1.1).toString())
            val now=System.nanoTime()
            assertTrue("A submitted buffered frame requests its presentation retry",Native.frame(handle,now,now+16_666_667))
            val display=JSONObject(Native.displayStatus(handle))
            assertEquals("Fifo",display.getString("present_mode"))
            assertEquals(submitted+1,display.getLong("submitted_frames"))
            transition(handle)
        }
        ready()
        stroke(0.0)
        val painted=hash(png("front-painted.png"))
        pendingBufferedFrame { handle ->
            val bytes=Native.surfacePixelsForTest(handle)
            assertTrue("Pending buffered capture contains artwork",(bytes.indices step 97).map {bytes[it]}.toSet().size>8)
        }
        ready()
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        ready()
        assertEquals(painted,hash(png("front-resumed.png")))
        pendingBufferedFrame { Native.detach(it) }
        scenario.recreate()
        scenario.onActivity { activity=it }
        ready()
        assertEquals(painted,hash(png("front-recreated.png")))
        val automation=instrumentation.uiAutomation
        val originalRotation=activity.display!!.rotation
        val automaticRotation=android.provider.Settings.System.getInt(activity.contentResolver,android.provider.Settings.System.ACCELEROMETER_ROTATION,0)!=0
        try {
            // Android 16 large screens may ignore Activity orientation requests.
            // Rotate the test display itself, preserving the user's rotation mode.
            for(rotation in listOf(android.view.Surface.ROTATION_0,android.view.Surface.ROTATION_90)) {
                assertTrue(automation.setRotation(rotation))
                compose.waitUntil(15_000) {activity.display!!.rotation==rotation && host.surfaceReady}
                compose.waitForIdle()
                ready()
                assertEquals(painted,hash(png("front-rotated-$rotation.png")))
                val bytes=native { Native.surfacePixelsForTest(it) }
                assertTrue("Rotated surface image has content",(bytes.indices step 97).map {bytes[it]}.toSet().size>8)
            }
        } finally {
            automation.setRotation(originalRotation)
            if(automaticRotation)automation.setRotation(android.app.UiAutomation.ROTATION_UNFREEZE)
        }
        compose.waitUntil(60_000) {host.snapshot?.optBoolean("shaders_ready")==true}
        // Let the restored orientation's workspace animation/layout finish.
        SystemClock.sleep(1000)
        compose.waitForIdle()
        ready()
        val submitted=native {JSONObject(Native.displayStatus(it)).getLong("submitted_frames")}
        SystemClock.sleep(300)
        assertEquals("An idle front buffer stops submitting",submitted,native {JSONObject(Native.displayStatus(it)).getLong("submitted_frames")})
        pendingBufferedFrame { Native.destroyGpuForTest(it); Native.detach(it) }
        compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) { host.failure!=null }
        compose.runOnUiThread { host.restartCanvas() }
        ready()
        assertEquals(painted,hash(png("front-recovered.png")))
        native { Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString()) }
        compose.waitUntil(10_000) { !tick() }
        assertNotEquals(painted,hash(png("front-undone.png")))
        native { Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString()) }
        compose.waitUntil(10_000) { !tick() }
        assertEquals(painted,hash(png("front-redone.png")))
        assertNull(host.failure)
    }
    @Test fun exactSnapshotsSurviveFilesGpuReplacementAndRecovery() {
        fun history(command:String) {
            compose.waitUntil(15_000) {tick();native {state(it).array("commands").objects().any {c->c.optString("id")==command && c.optBoolean("enabled")}}}
            native { Native.dispatch(it,obj("type" to "invoke","command" to command).toString()) }
            compose.waitUntil(10_000) { !tick() }
        }
        compose.waitUntil(60_000) { !tick() }
        stroke(0.0)
        val first=save("first.capy")
        assertTrue(manifest(first).rasterResources().length()>0)
        assertEquals("preview.png", manifest(first).outputData().getJSONObject("representation").getString("member"))
        java.util.zip.ZipInputStream(java.io.ByteArrayInputStream(first)).use { zip ->
            var preview = false
            while (true) {
                val entry = zip.nextEntry ?: break
                if (entry.name == "preview.png") {
                    val bytes = zip.readBytes()
                    preview = true
                    val image = android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
                    assertNotNull("Saved preview decodes", image)
                    assertTrue(image.width <= 1024 && image.height <= 1024)
                    image.recycle()
                }
            }
            assertTrue("Normal save includes preview pixels", preview)
        }
        val firstPng=png("first.png")
        point(1,0.0,120.0);point(2,40.0,120.0)
        val during=saveTask("during.capy") // Must exclude this active contact without blocking it.
        point(3,90.0,120.0)
        val duringBytes=finishSave(during,"during.capy")
        assertEquals(manifest(first).rasterResources().toString(),manifest(duringBytes).rasterResources().toString())
        assertTrue(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        val secondPng=png("second.png")
        assertNotEquals(hash(firstPng),hash(secondPng))
        val beforeFailedSave = native { state(it).getJSONObject("document_file").toString() }
        val previousFile = File(files,"failed-save.capy").apply { writeBytes(first) }
        val failedSave = saveTask("failed-save.capy")
        try {
            assertNotNull("Read-only JNI output rejects the save", runCatching {
                Native.projectWork(failedSave.first,ParcelFileDescriptor.open(previousFile,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
            }.exceptionOrNull())
            assertArrayEquals("Failed write preserves the previous file",first,previousFile.readBytes())
            native { Native.documentComplete(it,failedSave.second,false,"null") }
        } finally { Native.projectFree(failedSave.first) }
        assertEquals("Failed save preserves the file checkpoint",beforeFailedSave,native { state(it).getJSONObject("document_file").toString() })
        assertNotNull("Finished old save request cannot acknowledge newer paint",runCatching {
            native { Native.documentComplete(it,during.second,true,"null") }
        }.exceptionOrNull())
        assertEquals("Stale acknowledgement preserves the file checkpoint",beforeFailedSave,native { state(it).getJSONObject("document_file").toString() })
        assertTrue(native { state(it).getJSONObject("document_file").getBoolean("modified") })
        assertEquals(hash(secondPng),hash(png("failed-save-retained.png")))
        native { Native.destroyGpuForTest(it) }
        compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) {host.failure != null}
        compose.runOnUiThread {host.restartCanvas()}
        compose.waitUntil(60_000) {host.surfaceReady && host.snapshot?.optBoolean("brush_ready")==true}
        assertNull(host.failure)
        assertEquals(hash(secondPng),hash(png("replaced.png")))
        history("undo")
        assertFalse("Undo reaches the acknowledged save boundary",native { state(it).getJSONObject("document_file").getBoolean("modified") })
        assertEquals(hash(firstPng),hash(png("undo.png")))
        history("redo")
        assertTrue("Redo retains newer unsaved paint",native { state(it).getJSONObject("document_file").getBoolean("modified") })
        assertEquals(hash(secondPng),hash(png("redo.png")))
        val pipe=ParcelFileDescriptor.createPipe()
        val writer=java.util.concurrent.CompletableFuture.runAsync {
            ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use { output ->
                File(files,"first.capy").inputStream().use { it.copyTo(output) }
            }
        }
        try {
            val seekFailure=runCatching { android.system.Os.lseek(pipe[0].fileDescriptor,0,android.system.OsConstants.SEEK_CUR) }.exceptionOrNull()
            assertTrue("Provider input is a real non-seekable pipe",seekFailure is android.system.ErrnoException && seekFailure.errno==android.system.OsConstants.ESPIPE)
            open(File(files,"first.capy"),input={pipe[0]})
            writer.get(30,java.util.concurrent.TimeUnit.SECONDS)
        } finally {
            pipe.forEach { runCatching { it.close() } }
            writer.cancel(true)
        }
        assertEquals(hash(firstPng),hash(png("opened.png")))
        assertEquals(manifest(first).rasterResources().toString(),manifest(save("roundtrip.capy")).rasterResources().toString())
        val corrupt=File(files,"corrupt.capy");corrupt.writeBytes(first.copyOf().also {it[it.lastIndex]=(it.last().toInt() xor 1).toByte()})
        val epoch=native {state(it).getJSONObject("document_file").getLong("epoch")}
        open(corrupt,true)
        assertEquals(epoch,native {state(it).getJSONObject("document_file").getLong("epoch")})
        invoke("select_all");invoke("save_selection_layer")
        send(obj("type" to "layer", "action" to obj("op" to "cancel_rename")))
        val selectionLayer=native {state(it).array("layers").objects().single {row -> row.optBoolean("selection_layer")}.getLong("id")}
        val selectionLabel=native {state(it).array("layers").objects().first {row -> row.getLong("id")==selectionLayer}.getString("label")}
        invoke("return_to_artwork")
        val beforeEye=manifest(save("selection-eye-before.capy"))
        val eyeCheckpoint=native {JSONObject(Native.sessionStamp(it,tabs(it).getLong("selected"))).getLong("checkpoint")}
        send(obj("type" to "layer", "action" to obj("op" to "visibility", "id" to selectionLayer, "value" to true)))
        fun selectionVisible()=native {state(it).array("layers").objects().single {row -> row.optBoolean("selection_layer") && row.getString("label")==selectionLabel}.getBoolean("visible")}
        assertTrue(selectionVisible())
        assertEquals(eyeCheckpoint,native {JSONObject(Native.sessionStamp(it,tabs(it).getLong("selected"))).getLong("checkpoint")})
        assertFalse(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        assertEquals(beforeEye.artworkRecords().toString(),manifest(save("selection-eye-after.capy")).artworkRecords().toString())
        val recovery=File(files,"atomic-recovery.capy")
        val capturedFile=native {state(it).getJSONObject("document_file")}
        captureSession(recovery)
        val stale=restoreSessionTask(recovery)
        try {
            compose.runOnUiThread {host.restartCanvas()}
            try { compose.waitUntil(15_000) {host.surfaceReady && host.snapshot?.optBoolean("brush_ready")==true} }
            catch(e: Exception) { throw AssertionError("Restart: surface=${host.surfaceReady}; brush=${host.snapshot?.optBoolean("brush_ready")}; failure=${host.failure}; activity=${scenario.state}; focus=${activity.hasWindowFocus()}", e) }
            try {native {Native.sessionAdopt(it,stale,1,"[]")};fail("Candidate from the retired device was adopted")}
            catch(e: IllegalStateException) {assertTrue(e.message.orEmpty().contains("canvas changed"))}
            assertEquals(hash(firstPng),hash(png("stale-candidate-retained.png")))
        } finally {Native.sessionFree(stale)}
        val restore=restoreSessionTask(recovery)
        try {
            adoptSession(restore)
        } finally {Native.sessionFree(restore)}
        tick()
        assertEquals(hash(firstPng),hash(png("recovered.png")))
        assertTrue(selectionVisible())
        val recovered=native {state(it).getJSONObject("document_file")}
        assertEquals(capturedFile.getBoolean("modified"),recovered.getBoolean("modified"))
        assertEquals(jsonValue(capturedFile.opt("location")),jsonValue(recovered.opt("location")))
        assertTrue(recovered.getBoolean("recovered"))
        // Real Activity/surface recreation retains the ViewModel and history.
        val retained = host
        scenario.recreate()
        scenario.onActivity { activity = it }
        assertSame(retained, host)
        compose.waitUntil(60_000) { host.surfaceReady }
        assertEquals(hash(firstPng),hash(png("surface-recreated.png")))
        val expectedTabs = tabs().array("tabs").objects().map {it.getString("title")}
        val expectedSelected = tabs().getLong("selected")
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main){host.recovery.flush()}})
        assertNull(host.actionError)
        scenario.close(); launch()
        compose.waitUntil(120_000) {host.recovery.ready && !host.recovery.working}
        assertNotSame(retained,host)
        assertNull(host.recovery.candidate); assertNull(host.actionError)
        assertEquals(expectedTabs,tabs().array("tabs").objects().map {it.getString("title")})
        assertEquals(expectedSelected,tabs().getLong("selected"))
        assertEquals(hash(firstPng),hash(png("controller-restored.png")))
        assertTrue(selectionVisible())
        activity.getExternalFilesDir(null)!!.resolve("raster-result.txt").writeText("PASS exact snapshots, active-contact save, undo/redo, GPU replacement, corrupt-file retention, Activity recreation and seamless restart\n")
    }

    @Test fun drawingTabsKeepHistorySpillAndLifecycle() {
        fun closeDecision(label:String)=native { handle ->
            val state=state(handle)
            state.array("requests").objects().firstOrNull{r->r.getJSONObject("kind").optString("type")=="document"}?.getInt("id")
                ?: error("Missing $label: file=${state.getJSONObject("document_file")}; requests=${state.array("requests")}")
        }
        // Parking readiness does not mean the restored brush is ready for a
        // new contact: the host deliberately defers contacts during warmup.
        fun ready() {compose.waitUntil(120_000){tick();native {
            val parked=JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")
            Native.dispatch(it,obj("type" to "close_settings").toString())
            parked && JSONObject(Native.snapshot(it)!!).getBoolean("brush_ready")
        }}}
        fun action(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};tick()}
        fun fresh():Long {
            val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
            try{Native.projectWork(task,-1,640,480);compose.waitUntil(60_000){tick();native{Native.projectParkReady(it,task)}};native{Native.projectAdopt(it,task,"null")}}
            finally{Native.projectFree(task)}
            tick();ready();return tabs().getLong("selected")
        }
        fun select(id:Long,close:Boolean=false) {
            ready();val task=native{Native.documentSwitch(it,id,close)}
            if(task!=0L)try{Native.documentResumeWork(task);native{Native.documentResume(it,task)}}finally{Native.documentResumeFree(task)}
            tick();if(ids().isNotEmpty())ready()
        }
        fun order(value:JSONObject){native{Native.documentTabs(it,value.toString())}}
        fun trim(){while(true){val task=native{Native.documentSpillTask(it)};if(task==0L)break;Native.documentSpillWork(task)}}
        val first=tabs().getLong("selected")
        stroke(0.0);val exact=manifest(save("tabs-exact.capy")).rasterResources().toString()
        action("undo") // Its only ink is now retained exclusively by redo history.
        val second=fresh();assertEquals(listOf(first,second),ids())
        trim();assertEquals(0,tabs().getInt("parked_renderers"))
        action("add_layer");val secondLayers=native{state(it).array("layers").length()}
        select(first);action("redo")
        assertEquals(exact,manifest(save("tabs-redo.capy")).rasterResources().toString())
        select(second);assertEquals(secondLayers,native{state(it).array("layers").length()})
        action("undo");assertEquals(secondLayers-1,native{state(it).array("layers").length()})
        val third=fresh()
        order(obj("op" to "reorder","id" to third,"before" to first));assertEquals(listOf(third,first,second),ids())
        select(first);order(obj("op" to "history","redo" to false));assertEquals(listOf(first,second,third),ids());assertEquals(first,tabs().getLong("selected"))
        order(obj("op" to "history","redo" to true));assertEquals(listOf(third,first,second),ids())
        val retained=host;scenario.recreate();scenario.onActivity{activity=it};assertSame(retained,host)
        compose.waitUntil(60_000){host.surfaceReady};assertEquals(listOf(third,first,second),ids())
        assertEquals(exact,manifest(save("tabs-recreated.capy")).rasterResources().toString())
        val bad=File(files,"tabs-invalid.capy").apply{writeText("invalid")};open(bad,true);assertEquals(3,ids().size)
        select(second);stroke(80.0);ready()
        assertTrue("The close fixture must contain committed ink",native{state(it).getJSONObject("document_file").getBoolean("modified")})
        action("close_document")
        val close=closeDecision("cancel decision")
        native{Native.documentClose(it,close,"\"cancel\"")};assertEquals(3,ids().size)
        action("close_document")
        val discard=closeDecision("discard decision")
        native{Native.documentClose(it,discard,"\"discard\"")};select(second,true)
        assertEquals(first,tabs().getLong("selected"));assertEquals(listOf(third,first),ids())
        assertFalse(tabs().getBoolean("can_undo"))
        while(ids().isNotEmpty()) {
            action("close_document")
            val file=native{state(it).getJSONObject("document_file")}
            if(!file.getBoolean("close_ready")) {
                val request=closeDecision("final decision")
                native{Native.documentClose(it,request,"\"discard\"")}
            }
            select(tabs().getLong("selected"),true)
        }
        assertEquals(0,tabs().getLong("selected"));assertEquals(0,tabs().getInt("parked_renderers"))
        activity.getExternalFilesDir(null)!!.resolve("drawing-tabs-native.txt").writeText("PASS independent history, exact redo-only disk backing, device reuse, order undo, Activity recreation, corrupt open, close cancellation/neighbour/final ownership")
    }

    @Test fun drawingTabsRestoreMultipleInactiveDrawingsWithoutPrompt() {
        compose.waitUntil(120_000){host.recovery.ready}
        fun action(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};tick()}
        action("add_layer")
        val first = tabs().getLong("selected")
        val firstLayers=native{state(it).array("layers").length()}
        save("restart-saved.capy")
        val firstFile=native {state(it).getJSONObject("document_file")}
        val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
        try{Native.projectWork(task,-1,640,480);compose.waitUntil(60_000){tick();native{Native.projectParkReady(it,task)}};native{Native.projectAdopt(it,task,"null")}}finally{Native.projectFree(task)}
        tick();compose.waitUntil(120_000){native{JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}}
        val second = tabs().getLong("selected")
        action("add_layer");action("add_layer")
        action("zoom_in")
        val secondLayers=native{state(it).array("layers").length()}
        val expectedZoom=native {state(it).getJSONObject("camera").getDouble("zoom")}
        native {Native.documentTabs(it,obj("op" to "reorder","id" to second,"before" to first).toString())}
        val expected = tabs().array("tabs").objects().map {it.getLong("id")}
        val expectedFile = native {state(it).getJSONObject("document_file")}
        val protected = runBlocking{kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main){host.recovery.flush()}}
        assertTrue("Session checkpoint: ${host.actionError}",protected)
        assertNull(host.actionError)
        scenario.close();launch()
        compose.waitUntil(120_000){host.recovery.ready&&!host.recovery.working}
        assertNull(host.failure);assertNull(host.actionError);assertNull(host.recovery.candidate)
        compose.onNodeWithTag("recover-drawing").assertDoesNotExist()
        assertEquals(expected,tabs().array("tabs").objects().map{it.getLong("id")})
        assertEquals(second,tabs().getLong("selected"))
        assertEquals(expectedFile.getBoolean("modified"),native{state(it).getJSONObject("document_file").getBoolean("modified")})
        assertEquals(secondLayers,native{state(it).array("layers").length()})
        assertEquals(expectedZoom,native{state(it).getJSONObject("camera").getDouble("zoom")},1e-9)
        action("undo");assertEquals(secondLayers-1,native{state(it).array("layers").length()})
        action("redo");assertEquals(secondLayers,native{state(it).array("layers").length()})
        compose.runOnUiThread{host.drawingTabs.select(first)}
        compose.waitUntil(120_000){!host.drawingTabs.switching&&tabs().getLong("selected")==first}
        assertEquals(firstLayers,native{state(it).array("layers").length()})
        assertEquals(jsonValue(firstFile.getJSONObject("location")),jsonValue(native{state(it).getJSONObject("document_file").getJSONObject("location")}))
        assertFalse(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        action("undo");assertEquals(firstLayers-1,native{state(it).array("layers").length()})
        assertTrue(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        action("redo");assertEquals(firstLayers,native{state(it).array("layers").length()})
        assertFalse(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        activity.getExternalFilesDir(null)!!.resolve("drawing-tabs-recovery.txt").writeText("PASS automatic multiple-tab restore, order, active drawing, independent bounded undo/redo and modified state")
    }

    @Test fun failedInactiveSessionRetriesWithoutLosingNewDrawing() {
        compose.waitUntil(120_000) {host.recovery.ready}
        fun command(name:String) {native {Native.dispatch(it,obj("type" to "invoke","command" to name).toString())};tick()}
        command("add_layer")
        val first = tabs().getLong("selected")
        host.newDocument(640,480);refresh();command("add_layer");command("add_layer")
        val failed = tabs().getLong("selected")
        val failedLayers = native {state(it).array("layers").length()}
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close()
        val session = device.recovery.walkTopDown().first {it.name == "session.json"}
        awaitSessionRelease(session)
        val row = JSONObject(Native.sessionManifestRead(session.absolutePath)).array("drawings").objects().first {it.getLong("id") == failed}
        val generations = File(session.parentFile,row.getString("key")+"/generations").listFiles().orEmpty().filter {it.extension == "json"}.associateWith {it.readBytes()}
        assertTrue(generations.isNotEmpty())
        generations.keys.forEach {it.writeText("broken checkpoint")}
        launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNotNull(host.recovery.candidate);assertNotNull(host.actionError)
        compose.runOnUiThread {host.clearActionError()}
        assertEquals(listOf(first),ids())
        host.newDocument(320,240);refresh();command("add_layer")
        val added = tabs().getLong("selected")
        assertNotEquals(failed,added)
        val before = native {state(it).getJSONObject("document_file").getLong("revision")}
        generations.forEach {(file,bytes)->file.writeBytes(bytes)}
        compose.runOnUiThread {host.recovery.recover()}
        compose.waitUntil(120_000) {!host.recovery.working&&host.recovery.candidate == null&&failed in ids()}
        assertNull(host.failure);assertNull(host.actionError)
        assertEquals(added,tabs().getLong("selected"))
        assertEquals(before,native {state(it).getJSONObject("document_file").getLong("revision")})
        assertEquals(setOf(first,failed,added),ids().toSet())
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close();launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNull(host.recovery.candidate);assertEquals(setOf(first,failed,added),ids().toSet())
        assertEquals(added,tabs().getLong("selected"))
        compose.runOnUiThread {host.drawingTabs.select(failed)}
        compose.waitUntil(120_000) {!host.drawingTabs.switching&&tabs().getLong("selected") == failed}
        assertEquals(failedLayers,native {state(it).array("layers").length()})
        command("undo");assertEquals(failedLayers-1,native {state(it).array("layers").length()})
    }

    @Test fun failedInactiveSessionDiscardRetiresOnlyItsCopy() {
        compose.waitUntil(120_000) {host.recovery.ready}
        fun command(name:String) {native {Native.dispatch(it,obj("type" to "invoke","command" to name).toString())};tick()}
        command("add_layer")
        val first = tabs().getLong("selected")
        host.newDocument(640,480);refresh();command("add_layer")
        val failed = tabs().getLong("selected")
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close()
        val session = device.recovery.walkTopDown().first {it.name == "session.json"}
        awaitSessionRelease(session)
        val copies = JSONObject(Native.sessionManifestRead(session.absolutePath)).array("drawings").objects().associate {it.getLong("id") to File(session.parentFile,it.getString("key"))}
        File(copies.getValue(failed),"generations").listFiles().orEmpty().filter {it.extension == "json"}.forEach {it.writeText("broken checkpoint")}
        launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNotNull(host.recovery.candidate);assertNotNull(host.actionError)
        compose.runOnUiThread {host.clearActionError()}
        compose.onNodeWithTag("discard-recovery").performClick()
        compose.waitUntil(120_000) {!host.recovery.working&&host.recovery.candidate == null}
        assertNull(host.actionError)
        assertFalse(File(copies.getValue(failed),"head.json").exists())
        assertTrue(File(copies.getValue(first),"head.json").isFile)
        assertEquals(listOf(copies.getValue(first).name),JSONObject(Native.sessionManifestRead(session.absolutePath)).array("drawings").objects().map {it.getString("key")})
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close();launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNull(host.recovery.candidate);assertNull(host.actionError);assertEquals(listOf(first),ids())
    }

    @Test fun unreadableWindowSessionDiscardRetiresItsCopies() {
        compose.waitUntil(120_000) {host.recovery.ready}
        native {Native.dispatch(it,obj("type" to "invoke","command" to "add_layer").toString())};tick()
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close()
        val session = device.recovery.walkTopDown().first {it.name == "session.json"}
        awaitSessionRelease(session)
        val copies = session.parentFile!!.listFiles().orEmpty().filter {it.isDirectory}
        assertTrue(copies.all {File(it,"head.json").isFile}&&copies.isNotEmpty())
        session.writeText("broken window metadata")
        launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNotNull(host.recovery.candidate);assertNotNull(host.actionError)
        compose.runOnUiThread {host.clearActionError()}
        compose.onNodeWithTag("discard-recovery").performClick()
        compose.waitUntil(120_000) {!host.recovery.working&&host.recovery.candidate == null}
        assertNull(host.actionError);assertFalse(session.exists())
        assertTrue(copies.none {File(it,"head.json").exists()})
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close();launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNull(host.recovery.candidate);assertNull(host.actionError)
    }

    private fun awaitSessionRelease(session:File) {
        compose.waitUntil(30_000) {
            runCatching {
                java.nio.channels.FileChannel.open(File(session.parentFile,"owner.lock").toPath(),java.nio.file.StandardOpenOption.WRITE).use {channel ->
                    channel.tryLock()?.let {it.release();true} ?: false
                }
            }.getOrDefault(false)
        }
    }
    private fun wholeManifestRetry(liveWork:Boolean) {
        fun artwork(): String {
            val selected = tabs().getLong("selected")
            return native {JSONObject(Native.sessionStamp(it,selected)).getString("artwork")}
        }
        compose.waitUntil(120_000) {host.recovery.ready}
        native {Native.dispatch(it,obj("type" to "invoke","command" to "add_layer").toString())};tick()
        val originalLayers = native {state(it).array("layers").length()}
        val originalArtwork = artwork()
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close()
        val session = device.recovery.walkTopDown().first {it.name == "session.json"}
        awaitSessionRelease(session)
        val original = session.readBytes();session.writeText("broken window metadata")
        launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        assertNotNull(host.recovery.candidate);assertNotNull(host.actionError)
        compose.runOnUiThread {host.clearActionError()}
        if(liveWork) {native {Native.dispatch(it,obj("type" to "invoke","command" to "add_layer").toString())};tick()}
        val currentArtwork = artwork()
        val currentLayers = native {state(it).array("layers").length()}
        session.writeBytes(original)
        compose.runOnUiThread {host.recovery.recover()}
        compose.waitUntil(120_000) {!host.recovery.working}
        if(liveWork) {
            assertNotNull(host.recovery.candidate);assertNotNull(host.actionError)
            assertEquals(currentArtwork,artwork())
            assertEquals(currentLayers,native {state(it).array("layers").length()})
            assertArrayEquals(original,session.readBytes())
            compose.runOnUiThread {host.clearActionError()}
        } else {
            assertNull(host.recovery.candidate);assertNull(host.actionError)
            assertEquals(originalArtwork,artwork())
            assertEquals(originalLayers,native {state(it).array("layers").length()})
        }
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
    }
    @Test fun repairedWindowManifestRefusesToReplaceLiveDrawing() = wholeManifestRetry(true)
    @Test fun repairedWindowManifestAdoptsIntoPristineBlank() = wholeManifestRetry(false)

    @Test fun stoppedStartupReleasesSessionAndRetainsDrawing() {
        compose.waitUntil(120_000) {host.recovery.ready}
        native {Native.dispatch(it,obj("type" to "invoke","command" to "add_layer").toString())};tick()
        val layers = native {state(it).array("layers").length()}
        val selected = tabs().getLong("selected")
        val artwork = native {JSONObject(Native.sessionStamp(it,selected)).getString("artwork")}
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        scenario.close()
        val session = device.recovery.walkTopDown().first {it.name == "session.json"}
        awaitSessionRelease(session)
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        val control = Native.captureControl()
        val monitor = androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry.getInstance()
        val callback = androidx.test.runner.lifecycle.ActivityLifecycleCallback {created,stage ->
            if(created is MainActivity&&stage==androidx.test.runner.lifecycle.Stage.CREATED)
                created.host.drawingTabs.registerInspection(control,created.host.viewModelScope.launch {gate.await()})
        }
        var liveArtwork = "";var liveLayers = 0
        instrumentation.runOnMainSync {monitor.addLifecycleCallback(callback)}
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            scenario.onActivity {activity=it;it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)}
            compose.waitUntil(120_000) {JSONObject(Native.sessionManifestRead(session.absolutePath)).array("restoring").length()>0}
            val liveId = tabs().getLong("selected")
            liveArtwork = native {JSONObject(Native.sessionStamp(it,liveId)).getString("artwork")}
            assertNotEquals("The edit precedes restored owner adoption",artwork,liveArtwork)
            native {Native.dispatch(it,obj("type" to "invoke","command" to "add_layer").toString())}
            liveLayers = native {state(it).array("layers").length()}
            scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
            scenario.close()
            awaitSessionRelease(session)
        } finally {
            gate.complete(Unit)
            instrumentation.runOnMainSync {monitor.removeLifecycleCallback(callback);host.drawingTabs.releaseInspection(control)}
            Native.captureFree(control)
        }
        launch();compose.waitUntil(120_000) {host.recovery.ready&&!host.recovery.working}
        val saved = tabs().array("tabs").objects().single {row ->native {JSONObject(Native.sessionStamp(it,row.getLong("id"))).getString("artwork")} == artwork}.getLong("id")
        assertEquals(2,tabs().array("tabs").length())
        compose.runOnUiThread {host.drawingTabs.select(saved)}
        compose.waitUntil(120_000) {!host.drawingTabs.switching&&tabs().getLong("selected")==saved}
        assertEquals(layers,native {state(it).array("layers").length()})
        val live = tabs().array("tabs").objects().single {row ->native {JSONObject(Native.sessionStamp(it,row.getLong("id"))).getString("artwork")} == liveArtwork}.getLong("id")
        compose.runOnUiThread {host.drawingTabs.select(live)}
        compose.waitUntil(120_000) {!host.drawingTabs.switching&&tabs().getLong("selected")==live}
        assertEquals(liveLayers,native {state(it).array("layers").length()})
        assertNull(host.recovery.candidate);assertNull(host.failure)
    }

    @Test fun drawingTabsNativePointerReorder() {
        fun settled(){compose.waitUntil(60_000){!host.drawingTabs.switching&&native{JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}};compose.runOnUiThread{host.documentChanged()};compose.waitForIdle();assertNull(host.failure);assertNull(host.actionError)}
        val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
        try{Native.projectWork(task,-1,640,480);compose.waitUntil(60_000){tick();native{Native.projectParkReady(it,task)}};native{Native.projectAdopt(it,task,"null")}}finally{Native.projectFree(task)}
        val order=ids();val selected=tabs().getLong("selected")
        val workspace=native{state(it).getJSONObject("workspace")}
        workspace.getJSONObject("layout").put("header",obj("size" to "large","next_id" to 2,"zones" to org.json.JSONArray(listOf(org.json.JSONArray(),org.json.JSONArray(listOf(obj("id" to 1,"item" to obj("kind" to "document_title")))),org.json.JSONArray()))))
        native{Native.dispatch(it,obj("type" to "restore_workspace","workspace" to workspace).toString())};settled()
        fun locate(tag:String,within:android.view.View?=null):Pair<android.view.View,androidx.compose.ui.geometry.Rect> {
            var found:Pair<android.view.View,androidx.compose.ui.geometry.Rect>?=null
            instrumentation.runOnMainSync{found=semanticsRoots().filter{within==null||it.view===within}.firstNotNullOfOrNull{root->root.find(hasTag(tag))?.let{root.view to it.boundsInRoot}}}
            return checkNotNull(found){"Missing $tag"}
        }
        fun drag(tool:Int,handle:Boolean=false,cancel:Boolean=false,vertical:Boolean=handle,hold:Long=0,outside:Boolean=false) {
            val (view,anchor)=locate(if(vertical)"drawing-handle-${order.first()}"else"drawing-tab-${order.first()}")
            val start=if(vertical&&!handle)locate("drawing-tab-${order.first()}",view).second else anchor
            val end=locate("drawing-tab-${order.last()}",view).second
            val from=androidx.compose.ui.geometry.Offset(start.left+start.width*.30f,start.center.y)
            val to=if(vertical)androidx.compose.ui.geometry.Offset(end.center.x,end.bottom-8f)else androidx.compose.ui.geometry.Offset(end.right-8f,end.center.y)
            val down=SystemClock.uptimeMillis()
            fun event(action:Int,point:androidx.compose.ui.geometry.Offset) {
                val event=motion(tool,action,point,down)
                try{instrumentation.runOnMainSync{view.dispatchTouchEvent(event)}}finally{event.recycle()}
                SystemClock.sleep(40)
            }
            event(android.view.MotionEvent.ACTION_DOWN,from)
            if(hold>0){SystemClock.sleep(hold);compose.mainClock.advanceTimeBy(hold);instrumentation.runOnMainSync {}}
            event(android.view.MotionEvent.ACTION_MOVE,to)
            val away=androidx.compose.ui.geometry.Offset(to.x,to.y+start.height*3)
            if(!vertical) {
                fun slid(swapped:Boolean)=compose.waitUntil(5_000){
                    val a=locate("drawing-tab-${order.first()}",view).second.left;val b=locate("drawing-tab-${order.last()}",view).second.left
                    kotlin.math.abs(a-(if(swapped)end else start).left)<1.5f&&kotlin.math.abs(b-(if(swapped)start else end).left)<1.5f
                }
                slid(true)
                event(android.view.MotionEvent.ACTION_MOVE,away);slid(false)
                if(!outside){event(android.view.MotionEvent.ACTION_MOVE,to);slid(true)}
            }
            event(if(cancel)android.view.MotionEvent.ACTION_CANCEL else android.view.MotionEvent.ACTION_UP,if(outside)away else to)
        }
        for(tool in listOf(android.view.MotionEvent.TOOL_TYPE_MOUSE,android.view.MotionEvent.TOOL_TYPE_STYLUS,android.view.MotionEvent.TOOL_TYPE_FINGER)) {
            drag(tool);compose.waitUntil(10_000){ids()==order.reversed()};assertEquals(selected,tabs().getLong("selected"))
            native{Native.documentTabs(it,obj("op" to "history","redo" to false).toString())};settled();assertEquals(order,ids())
            drag(tool,cancel=true);settled();assertEquals(order,ids())
            drag(tool,outside=true);settled();assertEquals("Release outside the strip cancels",order,ids())
        }
        compose.runOnUiThread{host.drawingTabs.selector=true};compose.onNodeWithTag("drawing-selector").assertIsDisplayed()
        for(tool in listOf(android.view.MotionEvent.TOOL_TYPE_MOUSE,android.view.MotionEvent.TOOL_TYPE_STYLUS,android.view.MotionEvent.TOOL_TYPE_FINGER)) {
            drag(tool,handle=true);compose.waitUntil(10_000){ids()==order.reversed()};assertEquals(selected,tabs().getLong("selected"))
            compose.onNodeWithTag("drawing-order-undo").performClick();compose.waitUntil(10_000){ids()==order};settled()
        }
        for(tool in listOf(android.view.MotionEvent.TOOL_TYPE_STYLUS,android.view.MotionEvent.TOOL_TYPE_FINGER)) {
            drag(tool,vertical=true);settled();assertEquals("Row bodies preserve pre-hold scrolling",order,ids());assertEquals(selected,tabs().getLong("selected"))
            drag(tool,vertical=true,hold=android.view.ViewConfiguration.getLongPressTimeout().toLong()+120)
            compose.waitUntil(10_000){ids()==order.reversed()}
            compose.onNodeWithTag("drawing-order-undo").performClick();compose.waitUntil(10_000){ids()==order};settled()
        }
        compose.runOnUiThread{host.drawingTabs.selector=false;DocumentController.nativeFileJobsForTest=false}
    }

    @Test fun drawingTabsFileBatchKeepsDuplicateOwnersAndContinuesFailures() {
        stroke(0.0);save("tab-batch-source.capy")
        val good=android.net.Uri.fromFile(File(files,"tab-batch-source.capy"))
        val bad=android.net.Uri.fromFile(File(files,"tab-batch-bad.capy").apply{writeText("corrupt")})
        val before=native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}.getLong("selected")
        compose.runOnUiThread{DocumentController.nativeFileJobsForTest=false;assertTrue(host.documents.openUris(listOf(good,bad,good)))}
        compose.waitUntil(120_000){!host.documents.working&&!host.drawingTabs.switching&&native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString())).array("tabs").length()==3}}
        val tabs=native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}
        val rows=tabs.array("tabs").objects();assertEquals(before,rows.first().getLong("id"))
        assertEquals(listOf(good.toString(),good.toString()),rows.drop(1).map{it.getString("uri")})
        assertNotEquals(rows[1].getLong("id"),rows[2].getLong("id"));assertEquals(rows[2].getLong("id"),tabs.getLong("selected"))
        assertFalse(rows[1].getBoolean("modified"));assertFalse(rows[2].getBoolean("modified"))
        // The corrupt middle entry reports its error without redirecting a later
        // result or replacing the initiating editor.
        assertNull(host.failure)
        activity.getExternalFilesDir(null)!!.resolve("drawing-tabs-files.txt").writeText("PASS serial URI batch; corrupt middle file reported and skipped; repeated URI creates independent clean owners; last successful drawing selected")
    }

    @Test fun registeredDehazeMatchesIndependentTwoPixelAirlightTie() {
        val colors = listOf(listOf(160, 190, 220), listOf(100, 130, 160))
        fun linear(byte: Int): Double {
            val value = byte / 255.0
            return if (value <= 0.04045) value / 12.92 else Math.pow((value + 0.055) / 1.055, 2.4)
        }
        for (reverse in listOf(false, true)) {
            val source = if (reverse) colors.reversed() else colors
            val file = File(files, "dehaze-tie-$reverse.png")
            val bitmap = android.graphics.Bitmap.createBitmap(2, 1, android.graphics.Bitmap.Config.ARGB_8888)
            try {
                for (x in 0..1) bitmap.setPixel(x, 0, android.graphics.Color.rgb(source[x][0], source[x][1], source[x][2]))
                file.outputStream().use { assertTrue(bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it)) }
            } finally { bitmap.recycle() }
            open(file); refresh()
            send(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "dehaze")))
            refresh()
            val layer = native { state(it).getJSONObject("layer_properties").getLong("layer") }
            fun sample(source: String, x: Float): JSONObject {
                val control = Native.captureControl()
                try {
                    val task = native { Native.inspectionTask(it, control) }
                    val result = JSONObject(Native.inspectionSample(task, source, x, 0.5f, 1))
                    val document = native { state(it).getJSONObject("document_file") }
                    assertEquals(document.getLong("epoch"), result.getLong("epoch"))
                    assertEquals(document.getLong("revision"), result.getLong("revision"))
                    return result
                } finally { Native.captureFree(control) }
            }
            val input = source.map { rgb -> rgb.map(::linear) }
            val air = input[0]
            val darkness = (0..2).minOf { channel -> input.minOf { it[channel] } / air[channel] }
            for (amount in listOf(0.0, 50.0, 100.0, -100.0)) {
                send(obj("type" to "effect", "action" to obj("op" to "set", "layer" to layer,
                    "key" to "amount", "value" to obj("kind" to "number", "value" to amount))))
                refresh()
                val transmission = maxOf(0.1, 1.0 - 0.95 * kotlin.math.abs(amount) * 0.01 * darkness)
                for (x in 0..1) {
                    val captured = sample(snapshotSource("EffectInput", layer), x + 0.5f).getJSONObject("sample").getJSONArray("Color")
                    val actual = sample(sourceVisible, x + 0.5f).getJSONObject("sample").getJSONArray("Color")
                    assertEquals(1.0, actual.getDouble(3), 0.0)
                    for (channel in 0..2) {
                        assertEquals(input[x][channel], captured.getDouble(channel), 1e-5)
                        val expected = if (amount >= 0.0) (input[x][channel] - air[channel]) / transmission + air[channel]
                            else input[x][channel] * transmission + air[channel] * (1.0 - transmission)
                        assertTrue(actual.getDouble(channel).isFinite())
                        assertEquals("UInt tie/header reverse=$reverse amount=$amount x=$x channel=$channel",
                            expected, actual.getDouble(channel), 1e-5)
                    }
                }
            }
            assertNull(host.failure); assertNull(host.actionError)
        }
    }

}
