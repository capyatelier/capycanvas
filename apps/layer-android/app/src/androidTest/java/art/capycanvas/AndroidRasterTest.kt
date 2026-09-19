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
import android.view.WindowManager
import kotlinx.coroutines.Job
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest

/** Real JNI/file workers and Vulkan. Test files stay in this app's private cache. */
class AndroidRasterTest {
    @get:Rule val compose = createEmptyComposeRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private val host get() = activity.host
    private lateinit var recoveryDirectory: File
    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        compose.waitUntil(60_000) {host.snapshot?.optBoolean("brush_ready")==true || host.failure!=null}
        assertNull(host.failure)
        compose.waitUntil(60_000) {host.workspaceManager?.optBoolean("ready")==true || host.workspaceManager?.isNull("error")==false}
        assertTrue("Workspace startup: ${host.workspaceManager}",host.workspaceManager?.optBoolean("ready")==true)
        compose.waitUntil(60_000) {host.workspaceManager?.optBoolean("busy")==false}
        // Slower devices can finish the selected brush before document commands
        // are enabled. Wait for the same admission state as the visible Open UI.
        compose.waitUntil(120_000) {host.failure!=null || native{state(it).array("commands").objects().any{c->c.optString("id")=="open_document"&&c.optBoolean("enabled")}}}
        assertNull(host.failure)
    }
    @Before fun isolatedWindow() {
        DocumentController.nativeFileJobsForTest = true
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("input keyevent KEYCODE_WAKEUP", "wm dismiss-keyguard")) {
            android.os.ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use { it.readBytes() }
        }
        // A driver crash can strand the previous run's synthetic contact in
        // InputDispatcher. Cancel that injected device before opening a picker.
        for (source in listOf(android.view.InputDevice.SOURCE_STYLUS, android.view.InputDevice.SOURCE_TOUCHSCREEN, android.view.InputDevice.SOURCE_MOUSE)) {
            val properties = arrayOf(android.view.MotionEvent.PointerProperties().apply { id = 7; toolType = android.view.MotionEvent.TOOL_TYPE_STYLUS })
            val coords = arrayOf(android.view.MotionEvent.PointerCoords())
            val now = SystemClock.uptimeMillis()
            val event = android.view.MotionEvent.obtain(now, now, android.view.MotionEvent.ACTION_CANCEL, 1, properties, coords, 0, 0, 1f, 1f, 0, 0, source, 0)
            try { automation.injectInputEvent(event, true) } finally { event.recycle() }
        }
        val root = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir, "raster-test-${System.nanoTime()}")
        ColorPreferencesStore.directoryForTest=File(root,"color-preferences")
        recoveryDirectory = File(root, "recovery")
        RecoveryController.directoryForTest = recoveryDirectory
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        launch()
    }
    @After fun closeWindow() {
        if (::scenario.isInitialized) scenario.close()
        DocumentController.nativeFileJobsForTest = false
        RecoveryController.directoryForTest = null
        CanvasHost.workspaceDirectoryForTest = null
        ColorPreferencesStore.directoryForTest=null
    }
    private fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    private val files get() = activity.cacheDir
    private fun tick() = native { val now=System.nanoTime(); Native.frame(it,now,now+16_666_667) }
    @Test fun displaySurfaceCapabilities() {
        val report=native{JSONObject(Native.displayStatus(it))}
        compose.runOnIdle {
            val display=activity.display!!
            report.put("display_hdr",display.isHdr)
            val capabilities=display.hdrCapabilities
            val types=if(android.os.Build.VERSION.SDK_INT>=34)display.mode.supportedHdrTypes else capabilities?.supportedHdrTypes?:intArrayOf()
            report.put("android_hdr_types",org.json.JSONArray(types.toList()))
            report.put("desired_max_luminance",capabilities?.desiredMaxLuminance)
            if(android.os.Build.VERSION.SDK_INT>=34)report.put("hdr_sdr_ratio",display.hdrSdrRatio)
            report.put("wide_color_gamut",display.isWideColorGamut)
        }
        File(activity.filesDir,"display-capabilities.json").writeText(report.toString(2))
        assertFalse(report.has("error"))
        assertTrue(report.getJSONArray("formats").length()>0)
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
    private fun point(phase: Int, dx: Double, dy: Double) = native { handle ->
        val viewport=host.snapshot!!.getJSONObject("state").getJSONObject("camera").getJSONArray("viewport")
        val bytes=doubleArrayOf(viewport.getDouble(0)*.50+dx,viewport.getDouble(1)*.5+dy,.65,0.0,0.0,0.0,0.0,System.nanoTime().toDouble(),phase.toDouble())
        Native.pointer(handle,71,0,0,bytes,bytes.size,false)
        val now=System.nanoTime(); Native.frame(handle,now,now+16_666_667)
    }
    private fun stroke(dy: Double) {
        point(1,0.0,dy)
        for(i in 1..6) {SystemClock.sleep(10);point(2,i*15.0,dy)}
        point(3,90.0,dy);tick()
    }
    private fun saveTask(): Pair<Long,Int> = native { handle ->
        val (id,file)=request(handle,"save_document_as")
        Native.projectTask(handle,id,obj("uri" to "test:private.capy","name" to "private.capy").toString(),file.getLong("epoch"),file.getLong("revision")) to id
    }
    private fun finishSave(job: Pair<Long,Int>, name: String): ByteArray {
        val file=File(files,name)
        try {
            Native.projectWork(job.first,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0)
            native {Native.documentComplete(it,job.second,true,"null")}
            return file.readBytes()
        } finally {Native.projectFree(job.first)}
    }
    private fun save(name: String)=finishSave(saveTask(),name)
    private fun open(file: File, corrupt: Boolean=false) {
        val job=native {handle -> val (id,state)=request(handle,"open_document")
            Native.projectTask(handle,id,"null",state.getLong("epoch"),state.getLong("revision")) to id }
        try {
            try {
                Native.projectWork(job.first,ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
                if(corrupt)fail("Corrupt file was accepted")
                native {Native.projectAdopt(it,job.first,"null")}
            } catch(e: Exception) {
                if(!corrupt)throw e
                native {Native.documentComplete(it,job.second,false,JSONObject.quote(e.message ?: "Corrupt file"))}
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
    private fun manifest(bytes: ByteArray): JSONObject {
        assertArrayEquals("CAPYRASTER".toByteArray(),bytes.copyOfRange(0,10))
        assertTrue("Native archive version", bytes[10].toInt() in 4..5 && bytes[11].toInt() == 0)
        val size=ByteBuffer.wrap(bytes,12,8).order(ByteOrder.LITTLE_ENDIAN).long.toInt()
        return JSONObject(bytes.copyOfRange(52,52+size).decodeToString())
    }
    private fun hash(bytes: ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()

    @Test fun hdrEditingProofDeliveryAndRecovery() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile")
        Assume.assumeTrue("Supply an independently encoded PQ PNG with -e hdrFile",sourcePath!=null)
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
        val input=File(files,"hdr-input.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $sourcePath")).use{it.readBytes()})}
        fun refresh(){tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()}
        fun action(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};refresh()}
        fun histogram():String {val flag=Native.captureControl();try{val task=native{Native.inspectionTask(it,flag)};return JSONObject(Native.inspectionHistogram(task)).getJSONObject("histogram").toString()}finally{Native.captureFree(flag)}}
        fun form()=native{JSONObject(Native.proofForm(it))}
        fun ready(){compose.waitUntil(120_000){native{JSONObject(Native.toneStatus(it)).getBoolean("ready")}};assertNull(host.failure)}
        open(input);refresh();ready()
        var original=histogram()
        assertEquals("F16",JSONObject(original).getJSONObject("color").getString("depth"))
        assertTrue(JSONObject(original).getJSONArray("channels").objects().any{it.getLong("above")>0})
        fun captureColor(name:String){automation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),name).outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}}
        captureColor("color-panel.png")
        compose.onNodeWithTag("color-edit-button").performClick()
        compose.waitForIdle();SystemClock.sleep(300);captureColor("edit-color.png")
        for(text in listOf("","-",".","17","1e999")) {
            compose.onNodeWithTag("color-intensity-value").performTextReplacement(text)
            compose.onNodeWithText("Use Color").assertIsNotEnabled()
        }
        compose.onNodeWithTag("color-intensity-value").performTextReplacement("-0.5")
        compose.onNodeWithText("Use Color").assertIsEnabled()
        compose.onNodeWithTag("color-intensity-value").performTextReplacement("3")
        compose.onNodeWithText("Use Color").performClick();refresh()
        assertEquals(3.0,host.panelContent!!.getJSONObject("color_panel").getDouble("intensity"),.001)
        compose.onAllNodesWithText("Palettes…").assertCountEquals(0)
        native {h->
            val color=JSONObject(Native.colorUi(obj("type" to "form","request" to obj("color" to obj("space" to "Srgb","rgba" to org.json.JSONArray(listOf(0,0,0,1))),"document_space" to "Srgb","document_depth" to "F16","model" to "linear_rgb","intensity" to 0,"fields" to org.json.JSONArray(listOf("-4","4","1","100")))).toString())).getJSONObject("value")
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
        val thumbnailId=host.panelContent!!.getJSONObject("state").array("layers").objects().first{!it.optBoolean("group")&&it.isNull("content_icon")}.getLong("id")
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
        assertEquals(first.getJSONArray("blobs").toString(),manifest.getJSONArray("blobs").toString())
        assertEquals(first.getJSONObject("document").getJSONArray("layers").toString(),manifest.getJSONObject("document").getJSONArray("layers").toString())
        open(File(files,"hdr-master.capy"));refresh();ready();assertEquals(original,histogram());assertEquals(changed,form().getJSONObject("rendition").toString())
        val cancel=Native.captureControl();var task=0L
        try{task=native{Native.toneTask(it,cancel)};Native.captureCancel(cancel);assertTrue(runCatching{Native.toneWork(task)}.isFailure);assertFalse(native{Native.toneApply(it,task)})}finally{Native.toneRelease(task);Native.captureFree(cancel)}
        File(activity.getExternalFilesDir(null),"hdr-sdr-rendition.json").writeText(form().getJSONObject("rendition").toString())
        val sdr=png("hdr-sdr.png")
        val recipe=native{h->val basic=JSONObject(Native.query(h,obj("type" to "export_form").toString())).getJSONArray("recipes").getJSONArray(0).getJSONObject(1);JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to basic,"action" to obj("type" to "format","value" to "PngHdr")).toString())).getJSONObject("recipe")}
        assertTrue("Strict PQ delivery rejects out-of-range paint",runCatching{png("hdr-strict-rejected.png",recipe)}.isFailure)
        val clipped=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to recipe,"action" to obj("type" to "format","value" to "PngHdrMapped")).toString())).getJSONObject("recipe")}
        png("hdr-pq.png",clipped)
        val exr=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to recipe,"action" to obj("type" to "format","value" to "Exr")).toString())).getJSONObject("recipe")}
        val exrBytes=png("hdr-exact.exr",exr);assertArrayEquals(byteArrayOf(0x76,0x2f,0x31,0x01),exrBytes.copyOfRange(0,4))
        val recovery=File(files,"hdr-recovery.capy");val capture=native{Native.projectRecoveryTask(it,false)}
        try{Native.projectPublish(capture,recovery.absolutePath)}finally{Native.projectFree(capture)}
        native{Native.destroyGpuForTest(it)};compose.runOnUiThread{host.documentChanged()};compose.waitUntil(10_000){host.failure!=null}
        compose.runOnUiThread{host.restartCanvas()};compose.waitUntil(60_000){host.surfaceReady&&host.snapshot?.optBoolean("brush_ready")==true};ready()
        assertEquals(original,histogram());assertEquals(changed,form().getJSONObject("rendition").toString());assertEquals(hash(sdr),hash(png("hdr-recovered-sdr.png")))
        save("hdr-after-gpu-recovery.capy")
        val restore=native{Native.projectRecoveryTask(it,true)}
        try{Native.projectWork(restore,ParcelFileDescriptor.open(recovery,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0);native{Native.projectAdopt(it,restore,"null")}}finally{Native.projectFree(restore)}
        refresh();ready();assertEquals(original,histogram());assertTrue(native{state(it).getJSONObject("document_file").getBoolean("modified")})
        scenario.recreate();scenario.onActivity{activity=it};compose.waitUntil(60_000){host.surfaceReady};refresh();ready();assertEquals(original,histogram())
        open(File(files,"hdr-exact.exr"));refresh();ready();val exact=JSONObject(histogram());assertEquals("F32",exact.getJSONObject("color").getString("depth"));assertTrue(exact.getJSONArray("channels").objects().any{it.getLong("below")>0})
        open(File(files,"hdr-pq.png"));refresh();ready();assertEquals("F16",JSONObject(histogram()).getJSONObject("color").getString("depth"))
        open(File(files,"hdr-sdr.png"));refresh();assertEquals("U8",JSONObject(histogram()).getJSONObject("color").getString("depth"))
        println("HDR PQ open; GTK picker; touch cancel/stylus SDR appearance; exact master/rendition save/reopen; cancelled analysis; HDR/SDR delivery; GPU, recovery and Activity recreation passed")
    }

    @Test fun gpuToneRetainsPreviewAndRejectsLatePublication() {
        val source=InstrumentationRegistry.getArguments().getString("hdrFile")
        Assume.assumeTrue("Supply -e hdrFile",source!=null)
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
        val input=File(files,"gpu-tone-input.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $source")).use{it.readBytes()})}
        fun status()=native{JSONObject(Native.toneStatus(it))}
        fun refresh(){tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()}
        fun ready(){compose.waitUntil(120_000){val s=status();s.getBoolean("ready")||!s.isNull("error")};assertTrue(status().isNull("error"))}
        open(input)
        native{Native.proofControl(it,obj("type" to "mode","mode" to "sdr").toString())};refresh();ready()
        assertFalse("Exercise the mapped SDR presenter on HDR-capable devices too",status().getBoolean("hdr_output"))
        native{Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString());Native.dispatch(it,obj("type" to "select_brush","id" to 1).toString())};refresh()
        val first=status().getInt("publications")
        val control=Native.captureControl();val task=native{Native.toneTask(it,control)}
        try {
            val oracle=JSONObject(Native.toneReferenceDifference(task))
            for (i in 0..2) assertTrue("GPU/CPU guide agreement: $oracle",oracle.getJSONArray("max_error").getDouble(i)<0.0003)
            Native.toneWork(task)
            val extent=native{state(it).getJSONArray("tabs").getJSONObject(0)}
            val motion=motion(android.view.MotionEvent.TOOL_TYPE_STYLUS,60,extent.getDouble("width")/2 to extent.getDouble("height")/2) {
                val held=status()
                assertFalse("Stroke must own the document",held.getBoolean("idle"))
                assertTrue("Previous GPU guide stays bound",held.getBoolean("retained"))
                assertEquals(first,held.getInt("publications"))
                assertTrue("Pen down cancels immediately",Native.captureCancelled(control))
                assertFalse("Late candidate cannot publish",native{Native.toneApply(it,task)})
            }
            val released=SystemClock.uptimeMillis();ready()
            assertTrue(status().getInt("publications")>first)
            val report=obj("pen_up_wait_ms" to (SystemClock.uptimeMillis()-released),"status" to status(),"oracle" to oracle,"motion" to motion)
            File(activity.getExternalFilesDir(null),"gpu-tone-retention.json").writeText(report.toString(2))
            println("GPU_TONE "+report)
            val second=status().getInt("publications")
            native{Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};refresh();ready()
            assertTrue(status().getInt("publications")>second)
        } finally {Native.toneRelease(task);Native.captureFree(control)}
        assertNull(host.failure)
    }

    @Test fun hdrDisplayNegotiation() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile")
        requireNotNull(sourcePath){"Supply -e hdrFile for the display regression"}
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
        fun shell(command:String)=ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use{it.readBytes()}
        val output=File(activity.getExternalFilesDir(null),"display").apply{mkdirs()}
        val input=File(files,"display-hdr.png").apply{writeBytes(shell("cat $sourcePath"))}
        fun tone()=native{JSONObject(Native.toneStatus(it))}
        fun mode(value:String){native{Native.proofControl(it,obj("type" to "mode","mode" to value).toString())};compose.runOnUiThread{host.documentChanged()};tick()}
        fun surfacePixels(name:String):JSONObject {
            fun find(view:android.view.View):CanvasSurfaceView? {
                if(view is CanvasSurfaceView)return view
                if(view is android.view.ViewGroup)for(i in 0 until view.childCount)find(view.getChildAt(i))?.let{return it}
                return null
            }
            lateinit var surface:CanvasSurfaceView
            compose.runOnUiThread{surface=requireNotNull(find(activity.window.decorView))}
            val image=android.graphics.Bitmap.createBitmap(surface.width,surface.height,android.graphics.Bitmap.Config.RGBA_F16,false,
                android.graphics.ColorSpace.get(android.graphics.ColorSpace.Named.LINEAR_EXTENDED_SRGB))
            val done=java.util.concurrent.CountDownLatch(1);var result=-1
            compose.runOnUiThread{android.view.PixelCopy.request(surface,image,{result=it;done.countDown()},android.os.Handler(android.os.Looper.getMainLooper()))}
            assertTrue(done.await(10,java.util.concurrent.TimeUnit.SECONDS));assertEquals(android.view.PixelCopy.SUCCESS,result)
            fun range(left:Int,top:Int,right:Int,bottom:Int):JSONObject {
                var low=Float.POSITIVE_INFINITY;var high=Float.NEGATIVE_INFINITY
                var above=0;var count=0
                var digest=1469598103934665603L
                for(y in top.coerceAtLeast(0) until bottom.coerceAtMost(image.height) step 3)
                    for(x in left.coerceAtLeast(0) until right.coerceAtMost(image.width) step 3){
                        val p=image.getColor(x,y)
                        for(v in listOf(p.red(),p.green(),p.blue())){low=minOf(low,v);high=maxOf(high,v);if(v>1.001f)above++;count++;digest=(digest xor v.toRawBits().toLong())*1099511628211L}
                    }
                return obj("min" to low,"max" to high,"above_sdr" to above,"samples" to count,"digest" to digest.toString())
            }
            val nav=compose.onNodeWithTag("navigator-overview").fetchSemanticsNode().boundsInRoot.translate(-host.surfaceOrigin)
            val pixels=obj("format" to image.config.toString(),"color_space" to image.colorSpace.toString(),
                "canvas" to range(image.width/3,image.height/3,image.width*2/3,image.height*2/3),
                "navigator" to range(nav.left.toInt()+3,nav.top.toInt()+3,nav.right.toInt()-3,nav.bottom.toInt()-3))
            File(output,"$name-pixels.json").writeText(pixels.toString(2));image.recycle();return pixels
        }
        fun record(name:String){
            File(output,"$name.json").writeText(tone().put("surface",native{JSONObject(Native.displayStatus(it))}).toString(2))
            File(output,"$name-display.txt").writeBytes(shell("dumpsys display"))
            File(output,"$name-surfaceflinger.txt").writeBytes(shell("dumpsys SurfaceFlinger"))
            automation.takeScreenshot()?.let{shot->File(output,"$name.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        }
        open(input);mode("off")
        compose.waitUntil(30_000){tone().getBoolean("ready")}
        compose.waitUntil(5_000){native{JSONObject(Native.displayStatus(it)).optInt("presented_tone_generation",-1)}==tone().getInt("generation")}
        val supports=tone().getBoolean("display_hdr")
        fun awaitSurface(hdr:Boolean) {
            compose.waitUntil(10_000){native{JSONObject(Native.displayStatus(it)).let{s->
                s.opt("presented_hdr")==hdr&&s.optString("color_space")==if(hdr)"Bt2100Pq" else "Srgb"
            }}}
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
            for(region in listOf("canvas","navigator"))assertTrue(fallback.getJSONObject(region).getDouble("max")<=1.001)
            compose.runOnUiThread{host.displayInfo(true)}
            awaitSurface(true)
            val restored=surfacePixels("hdr-display-restored")
            for(region in listOf("canvas","navigator"))assertEquals("Display restore preserves $region",actualPixels.getJSONObject(region).getString("digest"),restored.getJSONObject(region).getString("digest"))
            assertEquals(revision,native{state(it).getJSONObject("document_file").getLong("revision")})
            compose.waitUntil(5_000){host.hdr.status=="HDR"}
            compose.onNodeWithTag("hdr-status").assertTextEquals("HDR")
        }
        val info=compose.onNodeWithTag("hdr-status").fetchSemanticsNode().boundsInRoot
        val zoom=compose.onNodeWithTag("camera-readout").fetchSemanticsNode().boundsInRoot
        assertTrue("Display status belongs on the left",info.right<zoom.left)
        assertEquals("Matching footer bubble height",zoom.height,info.height,1f)
        compose.onNodeWithTag("hdr-status").performClick()
        compose.onNodeWithText("Display Details").assertExists()
        record("display-details")
        compose.onNodeWithText("Close").performClick()
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

    @Test fun hdrLargeDocumentMeasurements() {
        val names=InstrumentationRegistry.getArguments().getString("hdrWorkloads")?.split(',') ?: listOf("sparse4k","hdr24.png","hdr45.png","hdr60.png")
        val report=obj("device" to android.os.Build.MODEL,"sdk" to android.os.Build.VERSION.SDK_INT,"runs" to org.json.JSONArray())
        val destination=File(activity.getExternalFilesDir(null),"hdr-performance.json")
        fun persist(){destination.writeText(report.toString(2))}
        fun refresh(){tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()}
        fun invoke(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};refresh()}
        fun ready(){compose.waitUntil(150_000){native{val s=JSONObject(Native.toneStatus(it));s.getBoolean("ready")||!s.isNull("error")}};assertTrue(native{JSONObject(Native.toneStatus(it)).isNull("error")})}
        for(name in names) {
            val entry=obj("name" to name,"runs" to org.json.JSONArray());report.getJSONArray("runs").put(entry)
            val peaks=java.util.concurrent.atomic.AtomicLong()
            val heartbeat=java.util.concurrent.CopyOnWriteArrayList<Double>()
            val watching=java.util.concurrent.atomic.AtomicBoolean(true)
            val handler=android.os.Handler(android.os.Looper.getMainLooper())
            var last=SystemClock.uptimeMillis()
            val pulse=object:Runnable{override fun run(){val now=SystemClock.uptimeMillis();heartbeat.add((now-last).toDouble());last=now;if(watching.get())handler.postDelayed(this,16)}}
            handler.post(pulse)
            val sampler=Thread{while(watching.get()){peaks.accumulateAndGet(android.os.Debug.getPss().toLong()*1024,::maxOf);SystemClock.sleep(500)}}.apply{start()}
            try {
                val started=SystemClock.uptimeMillis()
                if(name=="sparse4k") {
                    val job=native{h->val(id,f)=request(h,"new_document");Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision"))}
                    try{Native.projectOptions(job,obj("extent" to org.json.JSONArray(listOf(3840,2160)),"color" to obj("space" to "Srgb","depth" to "F16"),"background" to "White").toString());Native.projectWork(job,-1,3840,2160);native{Native.projectAdopt(it,job,"null")}}finally{Native.projectFree(job)}
                }else open(File(activity.filesDir,name))
                native{Native.proofControl(it,obj("type" to "mode","mode" to "sdr").toString())}
                refresh();entry.put("open_ms",SystemClock.uptimeMillis()-started);ready();entry.put("ready_ms",SystemClock.uptimeMillis()-started)
                entry.put("cold_heartbeat_ms",summary(org.json.JSONArray(heartbeat.toList())));heartbeat.clear()
                if(InstrumentationRegistry.getArguments().getString("hdrDiagnostics")=="true") {
                    native{Native.dispatch(it,obj("type" to "customize","action" to obj("type" to "set_panel_visible","panel" to "stats","visible" to true)).toString())};refresh()
                    val viewport=host.snapshot!!.getJSONObject("state").getJSONObject("camera").getJSONArray("viewport")
                    native{Native.dispatch(it,obj("type" to "move_panel","panel" to "stats","viewport" to viewport,"target" to obj("kind" to "float","position" to org.json.JSONArray(listOf(80,80)))).toString())};refresh()
                }
                invoke("fit_canvas");invoke("pen");native{Native.dispatch(it,obj("type" to "select_brush","id" to 1).toString());Native.dispatch(it,obj("type" to "color","action" to obj("op" to "set_slot","slot" to "foreground","color" to obj("space" to "Srgb","rgba" to org.json.JSONArray(listOf(1.8,.3,.1,1.0))))).toString())};refresh()
                val center=if(name=="sparse4k")1920.0 to 1080.0 else when(name){"hdr24.png"->3000.0 to 2000.0;"hdr45.png"->4128.0 to 2752.0;else->4752.0 to 3168.0}
                repeat(3){i->
                    val previous=native{JSONObject(Native.toneStatus(it))}.getInt("publications")
                    val run=motion(if(i==1)android.view.MotionEvent.TOOL_TYPE_FINGER else android.view.MotionEvent.TOOL_TYPE_STYLUS,180,center) {
                        val held=native{JSONObject(Native.toneStatus(it))}
                        assertTrue(held.getBoolean("retained"));assertEquals(previous,held.getInt("publications"))
                    }
                    val released=SystemClock.uptimeMillis();ready()
                    run.put("pen_up_wait_ms",SystemClock.uptimeMillis()-released)
                    run.put("guide",native{JSONObject(Native.toneStatus(it))})
                    entry.getJSONArray("runs").put(run);persist()
                }
                ready()
                val flag=Native.captureControl();val inspection=native{Native.inspectionTask(it,flag)}
                var inspectionError:String?=null
                val worker=Thread{try{Native.inspectionHistogram(inspection)}catch(e:Exception){inspectionError=e.message}}
                worker.start();SystemClock.sleep(40);val cancelled=SystemClock.uptimeMillis();Native.captureCancel(flag);worker.join(10_000)
                try{assertFalse("Inspection cancellation must finish",worker.isAlive);entry.put("histogram_cancel_ms",SystemClock.uptimeMillis()-cancelled);entry.put("histogram_cancel_result",inspectionError)}finally{if(!worker.isAlive)Native.captureFree(flag)}
                val concurrentFlag=Native.captureControl();val concurrent=native{Native.inspectionTask(it,concurrentFlag)};var concurrentError:String?=null
                val background=Thread{try{Native.inspectionHistogram(concurrent)}catch(e:Exception){concurrentError=e.message}};background.start()
                val saveStart=SystemClock.uptimeMillis();val saved=save("hdr-performance.capy");entry.put("save_ms",SystemClock.uptimeMillis()-saveStart);entry.put("saved_bytes",saved.size);background.join(150_000)
                try{assertFalse(background.isAlive);assertNull(concurrentError)}finally{if(!background.isAlive)Native.captureFree(concurrentFlag)}
                // Output uses the same atomic cancellation contract as the UI.
                val exportFlag=Native.captureControl();val exportId=native{request(it,"export_document").first}
                var exportTask=0L
                val deadline=SystemClock.uptimeMillis()+60_000
                while(exportTask==0L&&SystemClock.uptimeMillis()<deadline){exportTask=native{Native.projectExportTask(it,exportId,System.nanoTime(),exportFlag)};if(exportTask==0L){tick();SystemClock.sleep(10)}}
                assertNotEquals(0L,exportTask)
                val temporary=File(files,"cancelled-output.png");var exportFailure:Exception?=null
                val exporting=Thread{try{Native.projectWork(exportTask,ParcelFileDescriptor.open(temporary,ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or ParcelFileDescriptor.MODE_READ_WRITE).detachFd(),0,0)}catch(e:Exception){exportFailure=e}}
                exporting.start();SystemClock.sleep(40);val cancelStart=SystemClock.uptimeMillis();Native.captureCancel(exportFlag);exporting.join(10_000)
                try{assertFalse(exporting.isAlive);assertNotNull(exportFailure);entry.put("export_cancel_ms",SystemClock.uptimeMillis()-cancelStart);native{Native.documentComplete(it,exportId,false,"null")}}
                finally{if(!exporting.isAlive){Native.projectFree(exportTask);Native.captureFree(exportFlag);temporary.delete()}}
                // Cancel a real file decode using an independent atomic handle.
                if(name!="sparse4k") {
                    val control=Native.captureControl();val job=native{h->val(id,f)=request(h,"open_document");Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision")) to id}
                    Native.projectOpenControl(job.first,control);var failure:Exception?=null
                    val opening=Thread{try{Native.projectWork(job.first,ParcelFileDescriptor.open(File(activity.filesDir,name),ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)}catch(e:Exception){failure=e}}
                    opening.start();SystemClock.sleep(40);val start=SystemClock.uptimeMillis();Native.captureCancel(control);opening.join(10_000)
                    try{assertFalse(opening.isAlive);assertNotNull(failure);assertTrue(runCatching{native{Native.projectAdopt(it,job.first,"null")}}.isFailure);entry.put("open_cancel_ms",SystemClock.uptimeMillis()-start);native{Native.documentComplete(it,job.second,false,"null")}}
                    finally{if(!opening.isAlive){Native.projectFree(job.first);Native.captureFree(control)}}
                }
                entry.put("heartbeat_ms",summary(org.json.JSONArray(heartbeat.toList())))
                assertNull(host.failure)
            }catch(e:Throwable){entry.put("error",e.toString());throw e}
            finally{watching.set(false);handler.removeCallbacks(pulse);sampler.join(2000);entry.put("peak_process_pss_bytes",peaks.get());persist();println("HDR_PERFORMANCE "+entry)}
        }
    }

    @Test fun proofWorkspaceDragCancelAndDrawer() {
        val source=InstrumentationRegistry.getArguments().getString("hdrFile")
        Assume.assumeTrue("Supply -e hdrFile",source!=null)
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
        val file=File(files,"workspace-hdr.png").apply{writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $source")).use{it.readBytes()})}
        open(file);tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
        fun action(command:String){compose.runOnUiThread{host.invoke(command)};compose.waitForIdle()}
        fun edit(value:JSONObject){compose.runOnUiThread{host.customize(value)};compose.waitForIdle()}
        fun group()=host.snapshot!!.getJSONObject("layout").getJSONArray("groups").objects().first{it.getJSONArray("panels").values().contains("proof")}
        fun recipe()=native{JSONObject(Native.proofForm(it)).getJSONObject("rendition").toString()}
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
        val targetBytes=ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand("cat $profilePath")).use{it.readBytes()}
        val target=runBlocking{ProfileStore.import(activity,targetBytes)}
        fun action(command:String){compose.runOnUiThread{host.invoke(command)};compose.waitForIdle()}
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
            compose.waitUntil(120_000){!host.proof.busy&&native{JSONObject(Native.proofForm(it)).getJSONObject("recipe").getString("name")}==selectedName&&native{JSONObject(Native.proofForm(it)).getJSONObject("recipe").getJSONObject("profile").has("Icc")}}
            assertNull(host.proof.error);hide()
        }
        fun current()=native{JSONObject(Native.proofForm(it)).getJSONObject("recipe")}
        fun status()=native{JSONObject(Native.proofStatus(it))}
        fun hist():String {val flag=Native.captureControl();try{val task=native{Native.inspectionTask(it,flag)};return JSONObject(Native.inspectionHistogram(task)).getJSONObject("histogram").toString()}finally{Native.captureFree(flag)}}
        // Real first-use dialog, cancellation, sensible defaults.
        setup("soft_proof");assertFalse(native{state(it).getBoolean("soft_proof")})
        compose.onNodeWithText("Black ink").assertExists()
        compose.onNodeWithText("Choose Profile…").assertExists();SystemClock.sleep(400);cancel()
        assertTrue(native{JSONObject(Native.proofForm(it)).isNull("document_profile")})
        // Obtain a portable RGB ICC through the real profiled file pipeline.
        val wide=native{JSONObject(Native.query(it,obj("type" to "export_form").toString())).getJSONArray("recipes").getJSONArray(1).getJSONObject(1)}
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
        InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()?.let{shot->File(activity.getExternalFilesDir(null),"proof-print.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
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
        assertEquals(baseline.getJSONObject("tiled_sources").getJSONObject("proof").toString(),manifest(save("proof-cancel.capy")).getJSONObject("tiled_sources").getJSONObject("proof").toString())
        // An unwritable library path fails before document/history publication.
        val oldDirectory=ColorPreferencesStore.directoryForTest
        val blocked=File(files,"proof-blocked-${System.nanoTime()}").apply{writeText("not a directory")}
        ColorPreferencesStore.directoryForTest=blocked
        setup()
        // Select through the standard/profile callback with the already loaded ICC;
        // changing the library path prevents the picker from resolving its entry.
        compose.runOnUiThread{host.proof.edit("profile",target)}
        compose.onNodeWithTag("proof-profile").assertTextContains(target.getString("name"))
        println("Preparing replacement with blocked local profile storage")
        compose.waitUntil(120_000){!host.proof.busy&&(host.proof.error!=null||compose.onAllNodesWithText("Proof").fetchSemanticsNodes().isEmpty())}
        assertNotNull("Preservation must fail before replacing ${current().getString("name")}",host.proof.error)
        assertEquals(embedded.getString("name"),current().getString("name"))
        ColorPreferencesStore.directoryForTest=oldDirectory
        apply()
        val preserved=runBlocking{ProfileStore.list(activity).first{it.getString("name")==embedded.getString("name")}}
        assertEquals(array.toString(),runBlocking{ProfileStore.get(activity,preserved.getString("id"))}.getJSONObject("profile").getJSONArray("Icc").toString())
        val replacement=manifest(save("proof-replacement.capy"))
        assertEquals(target.getString("name"),replacement.getJSONObject("tiled_sources").getJSONObject("proof").getString("name"))
        assertEquals(baseline.getJSONArray("blobs").toString(),replacement.getJSONArray("blobs").toString())
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
        assertEquals(manifest(master).getJSONArray("blobs").toString(),manifest(save("proof-reopened.capy")).getJSONArray("blobs").toString())
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
            val bytes=ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand("cat $path")).use{it.readBytes()}
            val portable=File(files,"proof-from-web.capy").apply{writeBytes(bytes)}
            open(portable);compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()
            assertTrue(runBlocking{ProfileStore.list(activity).isEmpty()})
            assertEquals(target.getJSONObject("profile").toString(),current().getJSONObject("profile").toString())
            assertEquals("",status().getString("text"))
            val plain=png("web-portable-normal.png");val exact=hist()
            action("soft_proof");compose.waitUntil(120_000){status().getString("text").startsWith("Proof:")}
            assertEquals(exact,hist());assertEquals(hash(plain),hash(png("web-portable-proof.png")))
            val archive=manifest(save("proof-from-web-resaved.capy"))
            assertEquals(1,archive.getJSONObject("tiled_sources").getJSONArray("profiles").length())
            assertEquals("DisplayP3",archive.getJSONObject("document").getJSONObject("color").getString("space"))
            println("Web-created P3/U16 file opened, proofed, resaved and exported on Android without installed profiles")
        }
    }

    private fun summary(values: org.json.JSONArray): JSONObject? {
        if(values.length()==0)return null
        val sorted=(0 until values.length()).map {values.getDouble(it)}.sorted()
        return obj("count" to sorted.size,"p50" to sorted[((sorted.size-1)*.5).toInt()],"p95" to sorted[((sorted.size-1)*.95).toInt()],"p99" to sorted[((sorted.size-1)*.99).toInt()],"max" to sorted.last())
    }
    private fun motion(tool: Int, steps: Int, center: Pair<Double, Double> = 1000.0 to 750.0, during: (() -> Unit)? = null): JSONObject {
        fun measurements(reset: Boolean): JSONObject {
            val done = java.util.concurrent.CountDownLatch(1)
            var result: JSONObject? = null
            host.measurements(reset) { result = it; done.countDown() }
            assertTrue(done.await(10, java.util.concurrent.TimeUnit.SECONDS))
            return result!!
        }
        fun shell(command: String) = ParcelFileDescriptor.AutoCloseInputStream(
            InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)
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
            InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()?.let{shot->output.resolveSibling(output.name+".png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()}
        }
        val start=SystemClock.uptimeMillis();val source=when(tool){android.view.MotionEvent.TOOL_TYPE_MOUSE->android.view.InputDevice.SOURCE_MOUSE;android.view.MotionEvent.TOOL_TYPE_FINGER->android.view.InputDevice.SOURCE_TOUCHSCREEN;else->android.view.InputDevice.SOURCE_STYLUS}
        measurements(true)
        val duration = if (steps >= 180) InstrumentationRegistry.getArguments()
            .getString("motionDurationMs")?.toLong()?.coerceIn(5_000L, 30_000L) ?: 5_000L else 1_000L
        var i = 0
        while (true) {
            val elapsed = SystemClock.uptimeMillis() - start
            val phase=when {i==0->android.view.MotionEvent.ACTION_DOWN;elapsed>=duration->android.view.MotionEvent.ACTION_UP;else->android.view.MotionEvent.ACTION_MOVE}
            val properties=arrayOf(android.view.MotionEvent.PointerProperties().apply {id=7;toolType=tool})
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply {x=cx+40*kotlin.math.sin(elapsed/250.0).toFloat();y=cy+20*kotlin.math.cos(elapsed/310.0).toFloat();pressure=if(phase==android.view.MotionEvent.ACTION_UP)0f else .65f})
            val buttons=if(tool==android.view.MotionEvent.TOOL_TYPE_MOUSE&&phase!=android.view.MotionEvent.ACTION_UP)android.view.MotionEvent.BUTTON_PRIMARY else 0
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),phase,1,properties,coords,0,buttons,1f,1f,0,0,source,0)
            try {assertTrue("Injected tool=$tool phase=$phase at (${coords[0].x}, ${coords[0].y})",InstrumentationRegistry.getInstrumentation().uiAutomation.injectInputEvent(event,phase==android.view.MotionEvent.ACTION_UP))}finally{event.recycle()}
            if (phase == android.view.MotionEvent.ACTION_UP) break
            if (i == 10) during?.invoke()
            i++; SystemClock.sleep(4)
        }
        if(tool==android.view.MotionEvent.TOOL_TYPE_STYLUS) {
            // Finish virtual pen proximity before the next independent touch run.
            val properties=arrayOf(android.view.MotionEvent.PointerProperties().apply{id=7;toolType=tool})
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply{x=cx;y=cy;pressure=0f})
            val event=android.view.MotionEvent.obtain(start,SystemClock.uptimeMillis(),android.view.MotionEvent.ACTION_HOVER_EXIT,1,properties,coords,0,0,1f,1f,0,0,source,0)
            try{InstrumentationRegistry.getInstrumentation().uiAutomation.injectInputEvent(event,true)}finally{event.recycle()}
        }
        // The host's Choreographer is the only frame producer during
        // motion. Capture its timeline independently of GPU timings.
        SystemClock.sleep(100)
        native { Unit } // Drain delivered input, without creating a frame.
        val timeline = measurements(false)
        assertNull(host.failure)
        if (timeline.getJSONArray("inputs").length() == 0) {
            InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()?.let { screenshot ->
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
            "timeline" to timeline, "surface_layer" to layer, "surface_latency" to latency,
            "tracked_canvas_bytes" to stats.getLong("resident_bytes"),"process_pss_bytes" to android.os.Debug.getPss().toLong()*1024,
            "process_mappings" to File("/proc/self/maps").useLines { it.count() })
    }

    @Test fun imagePlacementBatchHistoryAndStaleRequests() {
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
        fun sourceIdentity(manifest: JSONObject): String {
            val sources = JSONObject(manifest.getJSONObject("tiled_sources").toString())
            for (image in sources.getJSONArray("images").objects()) for (tile in image.getJSONArray("tiles").objects()) {
                val blob=JSONObject(manifest.getJSONArray("blobs").getJSONObject(tile.getInt("blob")).toString());blob.remove("offset");tile.put("blob",blob)
            }
            return sources.getJSONArray("images").toString()
        }
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
        fun press(command: String) {
            compose.waitUntil(20_000) { host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.any { it.getString("id") == command && it.getBoolean("enabled") } == true }
            compose.onNodeWithTag(command).assertIsDisplayed().performClick()
            compose.waitForIdle(); tick()
        }
        fun memoryStage(label: String) {
            val stats = native { JSONObject(Native.query(it, obj("type" to "renderer_stats").toString())) }
            android.util.Log.i("CapyPlacementTest", "$label: pss=${android.os.Debug.getPss()} KiB; mappings=${File("/proc/self/maps").useLines { it.count() }}; canvas=${stats.getLong("resident_bytes")} bytes; status=${File("/proc/self/status").readLines().filter { it.startsWith("Vm") }}")
        }
        memoryStage("before batch")
        val baseCount=count()
        batch(photos);assertEquals(baseCount+photos.size,count());memoryStage("provisional batch")
        assertEquals("Recovery defers while a placement is provisional", 0L, native { Native.projectRecoveryTask(it, false) })
        press("cancel_transform");assertEquals(baseCount,count())
        val loadingStart=SystemClock.uptimeMillis()
        batch(photos);press("apply_transform");val loadingMs=SystemClock.uptimeMillis()-loadingStart;memoryStage("applied batch")
        val fitted=manifest(save("batch-placement.capy"));val identity=sourceIdentity(fitted)
        val images=fitted.getJSONObject("tiled_sources").getJSONArray("images")
        for(i in photos.indices) {
            val extent=images.getJSONObject(i).getJSONArray("extent");val w=extent.getDouble(0);val h=extent.getDouble(1);val scale=minOf(1.0,2000/w,1500/h)
            val pose=fitted.getJSONObject("document").getJSONArray("layers").getJSONObject(i).getJSONObject("properties").getJSONArray("placement")
            assertEquals(scale,pose.getDouble(0),1e-6);assertEquals(scale,pose.getDouble(3),1e-6)
            assertEquals((2000-w*scale)/2,pose.getDouble(4),.01);assertEquals((1500-h*scale)/2,pose.getDouble(5),.01)
        }
        invoke("undo");assertEquals(baseCount,count());invoke("redo");assertEquals(baseCount+photos.size,count())
        memoryStage("before reopen")
        open(File(files,"batch-placement.capy"));memoryStage("after reopen");assertEquals(identity,sourceIdentity(manifest(save("batch-reopened.capy"))))
        invoke("scale_rotate");press("placement_original_size");press("apply_transform")
        val originalSize=manifest(save("batch-original-size.capy"))
        memoryStage("after original size")
        assertEquals(1.0,originalSize.getJSONObject("document").getJSONArray("layers").getJSONObject(0).getJSONObject("properties").getJSONArray("placement").getDouble(0),1e-6)
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
        if (InstrumentationRegistry.getArguments().getString("imagePlacementMotion") == "true") {
            val movingOnly = InstrumentationRegistry.getArguments().getString("imagePlacementMovingOnly") == "true"
            val report = obj("device" to android.os.Build.MODEL, "hardware" to android.os.Build.HARDWARE, "soc" to android.os.Build.SOC_MODEL, "loading_ms" to loadingMs,
                "runs" to org.json.JSONArray(), "pss_before_bytes" to android.os.Debug.getPss().toLong()*1024)
            val output=File(activity.getExternalFilesDir(null),"image-placement-motion.json")
            val layerIds=fitted.getJSONObject("document").getJSONArray("layers").objects().take(photos.size).map { it.getLong("id") }
            fun rasterIdentity(manifest: JSONObject): String {
                val rasters=org.json.JSONArray(manifest.getJSONArray("rasters").toString())
                for(raster in rasters.objects())for(tile in raster.getJSONArray("tiles").objects())tile.put("blob",manifest.getJSONArray("blobs").getJSONObject(tile.getInt("blob")).getJSONArray("digest"))
                return rasters.toString()
            }
            fun action(value: JSONObject) { native { Native.dispatch(it,value.toString()) }; scenario.onActivity {host.documentChanged()};tick() }
            fun stats(): JSONObject = native { JSONObject(Native.query(it,obj("type" to "renderer_stats").toString())) }
            try {
                action(obj("type" to "customize","action" to obj("type" to "set_panel_visible","panel" to "stats","visible" to true)))
                val statsGroup=host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "stats" in it.array("panels").values() }.getInt("id")
                action(obj("type" to "customize","action" to obj("type" to "set_column_collapsed","group" to statsGroup,"collapsed" to false)))
                action(obj("type" to "select_panel_tab","group" to statsGroup,"panel" to "stats"))
                invoke("fit_canvas")
                for(index in photos.indices)for(factor in listOf(1.1,1.2,2.0)) {
                    for(i in layerIds.indices)action(obj("type" to "set_layer_visibility","id" to layerIds[i],"visible" to (i==index)))
                    action(obj("type" to "layer","action" to obj("op" to "select","id" to layerIds[index],"mask" to false)))
                    invoke("scale_rotate")
                    val extent=images.getJSONObject(index).getJSONArray("extent");val scale=minOf(1.0,2000/extent.getDouble(0),1500/extent.getDouble(1))*factor
                    action(obj("type" to "set_tool_setting","id" to "transform_width","value" to scale))
                    val entry=obj("extent" to extent,"factor" to factor,"translation" to org.json.JSONArray())
                    for(tool in listOf(android.view.MotionEvent.TOOL_TYPE_MOUSE,android.view.MotionEvent.TOOL_TYPE_FINGER,android.view.MotionEvent.TOOL_TYPE_STYLUS))
                        entry.getJSONArray("translation").put(motion(tool,if(tool==android.view.MotionEvent.TOOL_TYPE_STYLUS)180 else 20))
                    if (movingOnly) {
                        report.getJSONArray("runs").put(entry);output.writeText(report.toString(2))
                        println("Photo moving: extent=$extent factor=$factor")
                        invoke("apply_transform")
                        continue
                    }
                    invoke("apply_transform");invoke("pen");action(obj("type" to "select_brush","id" to 1))
                    action(obj("type" to "set_color","rgba" to org.json.JSONArray(listOf(1.0,0.0,.7,.5))))
                    entry.put("drawing",motion(android.view.MotionEvent.TOOL_TYPE_STYLUS,180))
                    val painted=manifest(save("motion-painted.capy"));assertEquals(identity,sourceIdentity(painted))
                    assertTrue("The stylus paints source-local tiles",painted.getJSONArray("blobs").length()>fitted.getJSONArray("blobs").length())
                    invoke("undo");val undone=manifest(save("motion-undone.capy"));invoke("redo");val redone=manifest(save("motion-redone.capy"))
                    assertNotEquals(rasterIdentity(undone),rasterIdentity(redone))
                    assertEquals(rasterIdentity(painted),rasterIdentity(redone))
                    report.getJSONArray("runs").put(entry);output.writeText(report.toString(2));println("Photo motion: $entry")
                    memoryStage("completed photo $index at $factor")
                }
                memoryStage("before both visible")
                for(id in layerIds)action(obj("type" to "set_layer_visibility","id" to id,"visible" to true))
                memoryStage("both visible")
                action(obj("type" to "layer","action" to obj("op" to "select","id" to layerIds.first(),"mask" to false)));invoke(if(movingOnly) "scale_rotate" else "pen")
                report.put("two_layers",motion(android.view.MotionEvent.TOOL_TYPE_STYLUS,180))
                InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()?.let { screenshot ->
                    try { File(activity.getExternalFilesDir(null), "image-placement-two-photos.png").outputStream().use { screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) } }
                    finally { screenshot.recycle() }
                }
            } finally {output.writeText(report.toString(2))}
        }
        assertNull(host.failure)
        println("Android image placement: batch Apply/Cancel, fit, exact source retention, one-step history, reopen, Original Size, malformed/cancelled/stale requests passed")
    }

    @Test fun imagePlacementSystemPickerAndExternalDrag() {
        fun action(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }; tick()
            scenario.onActivity { host.documentChanged() }
            compose.waitForIdle()
        }
        fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
        fun count() = native { state(it).array("layers").length() }
        fun press(command: String) {
            compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.any { it.getString("id") == command && it.getBoolean("enabled") } == true }
            compose.onNodeWithTag(command).assertIsDisplayed().performClick(); compose.waitForIdle(); tick()
        }
        fun systemNode(predicate: (android.view.accessibility.AccessibilityNodeInfo) -> Boolean): android.view.accessibility.AccessibilityNodeInfo? {
            fun find(node: android.view.accessibility.AccessibilityNodeInfo?): android.view.accessibility.AccessibilityNodeInfo? {
                node ?: return null
                if (predicate(node)) return node
                for (i in 0 until node.childCount) find(node.getChild(i))?.let { return it }
                return null
            }
            return find(InstrumentationRegistry.getInstrumentation().uiAutomation.rootInActiveWindow)
        }
        fun systemClick(name: String, long: Boolean = false) {
            InstrumentationRegistry.getInstrumentation().uiAutomation.waitForIdle(300, 5_000)
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
                ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)).use { it.readBytes() }
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
                InstrumentationRegistry.getInstrumentation().waitForIdleSync()
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
                        try { assertTrue(InstrumentationRegistry.getInstrumentation().uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
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
            val pose = dropped.getJSONObject("document").getJSONArray("layers").getJSONObject(0).getJSONObject("properties").getJSONArray("placement")
            val extent = dropped.getJSONObject("tiled_sources").getJSONArray("images").getJSONObject(0).getJSONArray("extent")
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
            assertTrue(nested.getJSONObject("document").getJSONArray("layers").objects().any { it.getJSONObject("properties").optLong("parent", -1) == group })
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

            val retained = nested.getJSONObject("tiled_sources").toString()
            scenario.recreate(); scenario.onActivity { activity = it }
            compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
            assertEquals(retained, manifest(save("placement-recreated.capy")).getJSONObject("tiled_sources").toString())
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
                assertEquals(retained, manifest(save("placement-gpu-recreated.capy")).getJSONObject("tiled_sources").toString())
            } finally { Native.imageImportFree(task); Native.captureFree(control) }
            assertNull(host.failure)
            println("Android placed source survived activity/GPU replacement; retired GPU batch rejected")
            if (configured != null) for ((file, expected) in configured.zip(listOf(9504 to 6336, 4000 to 6000))) {
                open(file); invoke("fit_canvas")
                val opened = manifest(save("opened-${file.name}.capy"))
                assertEquals(expected.first, opened.getJSONObject("document").getInt("width"))
                assertEquals(expected.second, opened.getJSONObject("document").getInt("height"))
            }
        } finally {
            DocumentController.nativeFileJobsForTest = true
            for (uri in uris) resolver.delete(uri, null, null)
        }
    }

    @Test fun largePhotoFilterPreviews() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("filterPhoto") == "true")
        val photo = File(activity.filesDir, "filter-memory-test.jpg")
        assertTrue(photo.isFile)
        open(photo)
        scenario.onActivity { host.documentChanged() }
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("shaders_ready") == true }
        fun action(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }
            scenario.onActivity { host.documentChanged() }
            tick(); compose.waitForIdle()
        }
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
        fun action(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }
            scenario.onActivity { host.documentChanged() }
            tick(); compose.waitForIdle()
        }
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
        fun action(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }
            scenario.onActivity { host.documentChanged() }
            tick(); compose.waitForIdle()
        }
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

    @Test fun largePhotoSustainedDrawing() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("photoWorkflow") == "true")
        val photo = File(activity.filesDir, "photo-benchmark.jpg")
        assertTrue(photo.isFile)
        open(photo)
        fun action(value: JSONObject) { native { Native.dispatch(it,value.toString()) }; scenario.onActivity { host.documentChanged() }; tick(); compose.waitForIdle() }
        action(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "stats", "visible" to true)))
        val group=host.snapshot!!.getJSONObject("layout").array("groups").objects().first { "stats" in it.array("panels").values() }.getInt("id")
        action(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to group, "collapsed" to false)))
        action(obj("type" to "select_panel_tab", "group" to group, "panel" to "stats"))
        action(obj("type" to "invoke", "command" to "fit_canvas"))
        action(obj("type" to "select_brush", "id" to 1))
        action(obj("type" to "set_color", "rgba" to org.json.JSONArray(listOf(1.0, 0.0, .7, .5))))
        val runs=org.json.JSONArray()
        val output=File(activity.getExternalFilesDir(null), "large-photo-drawing.json")
        repeat(3) {
            runs.put(motion(android.view.MotionEvent.TOOL_TYPE_STYLUS, 180, 4752.0 to 3168.0))
            output.writeText(obj("extent" to org.json.JSONArray(listOf(9504, 6336)), "runs" to runs).toString(2))
        }
        assertNull(host.failure)
        save("large-photo-sustained.capy")
    }

    @Test fun largeJpegGpenPreservesPhotoThroughSaveAndRecovery() {
        Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("photoWorkflow") == "true")
        val photo = File(activity.filesDir, "photo-benchmark.jpg")
        assertTrue("Copy the 61 MP test JPEG into the target app's files directory", photo.isFile)
        fun send(action: JSONObject) { native { Native.dispatch(it, action.toString()) }; tick() }
        fun histogram(): JSONObject {
            val control = Native.captureControl()
            try { return JSONObject(Native.inspectionHistogram(native { Native.inspectionTask(it, control) })).getJSONObject("histogram") }
            finally { Native.captureFree(control) }
        }
        fun opaque(): String {
            val h = histogram()
            assertEquals("Painting must preserve the photo outside the stroke", 0L, h.getLong("transparent"))
            assertEquals(9504L * 6336, h.getLong("pixels"))
            return h.toString()
        }
        open(photo)
        send(obj("type" to "invoke", "command" to "fit_canvas"))
        val original = opaque()
        send(obj("type" to "select_brush", "id" to 1))
        send(obj("type" to "color", "action" to obj("op" to "set_slot", "slot" to "foreground",
            "color" to obj("space" to "Srgb", "rgba" to org.json.JSONArray(listOf(1.0, 0.0, .7, 1.0 / 3))))))
        stroke(0.0)
        val painted = opaque()
        assertNotEquals("G-Pen must actually change the photograph", original, painted)
        val saved = manifest(save("large-photo-painted.capy"))
        send(obj("type" to "invoke", "command" to "undo")); assertEquals(original, opaque())
        send(obj("type" to "invoke", "command" to "redo")); assertEquals(painted, opaque())
        open(File(files, "large-photo-painted.capy")); assertEquals(painted, opaque())
        val reopened = manifest(save("large-photo-reopened.capy"))
        assertEquals(saved.getJSONArray("blobs").toString(), reopened.getJSONArray("blobs").toString())
        native { Native.destroyGpuForTest(it) }; compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) { host.failure != null }
        compose.runOnUiThread { host.restartCanvas() }
        compose.waitUntil(120_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
        assertNull(host.failure)
        assertEquals(painted, opaque())
        assertNull(host.actionError)
        println("61 MP JPEG: G-Pen, preserved opacity, exact undo/redo, native save/reopen and GPU replacement passed")
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
        assertEquals("ProPhoto", before.getJSONObject("document").getJSONObject("color").getString("space"))
        assertEquals("U16", before.getJSONObject("document").getJSONObject("color").getString("depth"))
        assertTrue(before.getJSONArray("blobs").length() > 0)
        assertTrue(before.getJSONArray("blobs").objects().all { it.getJSONObject("descriptor").getInt("bits_per_channel") == 16 })
        native { Native.destroyGpuForTest(it) }; compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) { host.failure != null }
        compose.runOnUiThread { host.restartCanvas() }
        compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true }
        assertNull(host.failure)
        assertEquals(before.getJSONArray("blobs").toString(), manifest(save("wide16-recovered.capy")).getJSONArray("blobs").toString())
        open(File(files, "wide16.capy"))
        val after = manifest(save("wide16-reopened.capy"))
        assertEquals(before.getJSONObject("document").getJSONObject("color").toString(), after.getJSONObject("document").getJSONObject("color").toString())
        assertEquals(before.getJSONArray("blobs").toString(), after.getJSONArray("blobs").toString())
        val form = native { JSONObject(Native.query(it, obj("type" to "export_form").toString())) }
        val recipe = JSONObject(form.getJSONArray("recipes").getJSONArray(2).getJSONObject(1).toString()).put("format", "Png")
        val output = png("wide16.png", recipe)
        assertEquals("PNG uses 16-bit samples", 16, output[24].toInt())
        open(File(files, "wide16.png"))
        val imported = manifest(save("wide16-image.capy"))
        val original = imported.getJSONObject("tiled_sources")
        assertEquals("U16", original.getJSONArray("images").getJSONObject(0).getString("depth"))
        val profiles = native { JSONObject(Native.query(it, obj("type" to "export_form").toString())).getJSONArray("profiles") }
        recipe.put("profile", profiles.getJSONObject(profiles.length()-1))
        for ((format, extension) in listOf("Png" to "png", "Tiff" to "tif")) {
            png("identity.$extension", JSONObject(recipe.toString()).put("format", format))
            open(File(files, "identity.$extension"))
            val restored = manifest(save("identity-$extension.capy")).getJSONObject("tiled_sources")
            assertEquals(original.getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString(), restored.getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString())
            assertEquals(original.getJSONArray("profiles").toString(), restored.getJSONArray("profiles").toString())
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
            native { Native.projectAdopt(it, pending, "null") }
        } finally { Native.projectFree(pending) }
        val assumed = manifest(save("assumed16.capy"))
        assertEquals("AdobeRgb", assumed.getJSONObject("document").getJSONObject("color").getString("space"))
        assertEquals(original.getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString(), assumed.getJSONObject("tiled_sources").getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString())
        native { Native.dispatch(it, obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 0)).toString()) }
    }
    @Test fun photoCorrectionsAndMasksRemainRevisableAfterReopen() {
        val task=native { h -> val(id,f)=request(h,"new_document");Native.projectTask(h,id,"null",f.getLong("epoch"),f.getLong("revision")) }
        try {
            Native.projectOptions(task,obj("extent" to org.json.JSONArray(listOf(513,257)),"color" to obj("space" to "ProPhoto","depth" to "U16"),"background" to "White").toString())
            Native.projectWork(task,-1,513,257);native {Native.projectAdopt(it,task,"null")}
        }finally{Native.projectFree(task)}
        native {Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString())};tick();stroke(0.0)
        val recipe=native {JSONObject(Native.query(it,obj("type" to "export_form").toString())).getJSONArray("recipes").getJSONArray(2).getJSONObject(1)}
        png("correction-source.png",recipe);open(File(files,"correction-source.png"))
        val source=manifest(save("correction-source.capy")).getJSONObject("tiled_sources").toString()
        val controls=listOf(Triple("exposure","exposure",.75),Triple("white_balance","temperature",25.0),Triple("levels","gamma",.9),Triple("curves","curve_0",0.0),Triple("hue_saturation","hue",10.0),Triple("color_balance","midtones_red",12.0))
        val ids=mutableListOf<Long>()
        fun send(value:JSONObject){native {Native.dispatch(it,value.toString())};tick()}
        fun value(index:Int,changed:Boolean)=if(index==3)obj("kind" to "curve","value" to org.json.JSONArray(if(changed)"[[0,0],[1,1]]" else "[[0,0],[0.213,0.13],[0.79,0.9],[1,1]]")) else obj("kind" to "number","value" to (if(!changed)controls[index].third else if(index==2)1.2 else -controls[index].third))
        fun set(index:Int,changed:Boolean)=send(obj("type" to "effect","action" to obj("op" to "set","layer" to ids[index],"key" to controls[index].second,"value" to value(index,changed))))
        for((index,control)in controls.withIndex()){
            send(obj("type" to "effect","action" to obj("op" to "insert","effect" to control.first)))
            ids.add(native {state(it).getJSONObject("layer_properties").getLong("layer")})
            set(index,false);send(obj("type" to "layer","action" to obj("op" to "add_mask","id" to ids.last(),"replace" to false)))
        }
        val edited=manifest(save("corrections.capy"));assertEquals(source,edited.getJSONObject("tiled_sources").toString())
        assertEquals(6,edited.getJSONObject("document").getJSONArray("layers").objects().count {it.objectOrNull("effect")!=null && it.objectOrNull("mask")!=null})
        fun histogram():String {val flag=Native.captureControl();try{return JSONObject(Native.inspectionHistogram(native{Native.inspectionTask(it,flag)})).getJSONObject("histogram").toString()}finally{Native.captureFree(flag)}}
        val before=histogram();open(File(files,"corrections.capy"))
        val reopened=manifest(save("corrections-reopened.capy"));assertEquals(edited.getJSONObject("document").getJSONArray("layers").toString(),reopened.getJSONObject("document").getJSONArray("layers").toString());assertEquals(before,histogram())
        for(index in controls.indices){set(index,true);assertNotEquals(controls[index].first,before,histogram());set(index,false);assertEquals(controls[index].first,before,histogram())}
        send(obj("type" to "layer","action" to obj("op" to "invert_mask","id" to ids[0])));assertNotEquals(before,histogram())
        send(obj("type" to "invoke","command" to "undo"));assertEquals(before,histogram())
        assertEquals(source,manifest(save("corrections-final.capy")).getJSONObject("tiled_sources").toString());assertNull(host.failure)
    }

    @Test fun profileLibraryKeepsExactCopiesAndPresetOwnership() {
        val wide=native{JSONObject(Native.query(it,obj("type" to "export_form").toString())).getJSONArray("recipes").getJSONArray(1).getJSONObject(1)}
        png("profile-library.png",wide);open(File(files,"profile-library.png"))
        val form=native {JSONObject(Native.query(it,obj("type" to "export_form").toString()))}
        val profile=form.getJSONArray("profiles").objects().first{it.getJSONObject("profile").has("Icc")}
        val array=profile.getJSONObject("profile").getJSONArray("Icc");val bytes=ByteArray(array.length()){array.getInt(it).toByte()}
        runBlocking {
            ProfileStore.import(activity,bytes);ProfileStore.import(activity,bytes)
            val entries=ProfileStore.list(activity);assertEquals(1,entries.size)
            val id=entries[0].getString("id");val file=File(ColorPreferencesStore.directoryForTest,"color-profiles/$id.icc")
            assertArrayEquals(bytes,file.readBytes())
            assertEquals(array.toString(),ProfileStore.get(activity,id).getJSONObject("profile").getJSONArray("Icc").toString())
            file.writeBytes(byteArrayOf(1,2,3));assertTrue(ProfileStore.list(activity)[0].has("issue"))
            try {ProfileStore.get(activity,id);fail("Corrupt profile was accepted")}catch(e:Exception){assertTrue(e.message.orEmpty().contains("changed"))}
            ProfileStore.import(activity,bytes)
            val color=native {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
            val recipe=JSONObject(form.getJSONArray("recipes").getJSONArray(0).getJSONObject(1).toString()).put("profile",profile)
            val saved=ColorPreferencesStore.presets(activity,color,obj("type" to "save","name" to "Embedded library copy","recipe" to recipe))
            ProfileStore.remove(activity,id)
            assertTrue(ProfileStore.list(activity).isEmpty())
            assertEquals(array.toString(),ColorPreferencesStore.presets(activity,color,obj("type" to "get","index" to saved.getInt("index"))).getJSONObject("recipe").getJSONObject("profile").getJSONObject("profile").getJSONArray("Icc").toString())
            ProfileStore.import(activity,bytes)
        }
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("export_document")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Saved Profiles…").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Saved Profiles…").performScrollTo().performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Use Profile").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Use Profile").performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Color Profile Library").fetchSemanticsNodes().isEmpty()}
        compose.onNodeWithText("Cancel").performClick()
        compose.waitUntil(10_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
        DocumentController.nativeFileJobsForTest=true
        compose.runOnUiThread {host.dispatch(obj("type" to "open_settings","page" to "color"))}
        compose.onNodeWithText("Manage Color Profiles…").performScrollTo().performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Remove").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Remove").performClick()
        compose.waitUntil(10_000) {compose.onAllNodesWithText("No imported profiles").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithTag("profile-library-done").performClick()
        compose.runOnUiThread {host.dispatch(obj("type" to "close_settings"))}
        assertNull(host.failure)
    }

    @Test fun exportPresetsPersistAndRestoreEveryDeliveryChoice() {
        val color=native {JSONObject(Native.query(it,obj("type" to "document_color").toString()))}
        val form=native {JSONObject(Native.query(it,obj("type" to "export_form").toString()))}
        fun store(action:JSONObject)=runBlocking{ColorPreferencesStore.presets(activity,color,action)}
        fun canonical(recipe:JSONObject)=native{Native.query(it,obj("type" to "export_validate","recipe" to recipe).toString())}
        val recipe=JSONObject(form.getJSONArray("recipes").getJSONArray(1).getJSONObject(1).toString())
            .put("depth","U16").put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(321,123)),"enlarge" to false))).put("resolution",obj("Ppi" to 287))
        val saved=store(obj("type" to "save","name" to "Tablet test delivery","recipe" to recipe))
        val index=saved.getInt("index");assertEquals(4,index)
        assertEquals(canonical(recipe),canonical(store(obj("type" to "get","index" to index)).getJSONObject("recipe")))
        assertTrue(File(ColorPreferencesStore.directoryForTest!!,"color-export-presets.json").length()>0)
        val updated=JSONObject(recipe.toString()).put("background","White")
        store(obj("type" to "update","index" to index,"recipe" to updated))
        store(obj("type" to "remember","index" to 3,"recipe" to updated))
        assertEquals(canonical(updated),canonical(store(obj("type" to "get","index" to 3)).getJSONObject("recipe")))
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
        DocumentController.nativeFileJobsForTest=true
        store(obj("type" to "remove","index" to index));assertEquals(4,store(obj("type" to "list")).getJSONArray("names").length())
        store(obj("type" to "reset","index" to 3));assertNotEquals(canonical(updated),canonical(store(obj("type" to "get","index" to 3)).getJSONObject("recipe")))
        assertNull(host.failure)
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
        val form=native {JSONObject(Native.query(it,obj("type" to "export_form").toString()))}
        val recipe=JSONObject(form.getJSONArray("recipes").getJSONArray(2).getJSONObject(1).toString()).put("format","Png")
        val pixels=png("placement-original.png",recipe)
        open(File(files,"placement-original.png"))
        val source=manifest(save("placement-original.capy")).getJSONObject("tiled_sources")
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
        assertEquals(before.getJSONObject("document").getJSONObject("color").toString(),placed.getJSONObject("document").getJSONObject("color").toString())
        assertEquals(source.getJSONArray("images").toString(),placed.getJSONObject("tiled_sources").getJSONArray("images").toString())
        assertEquals(source.getJSONArray("profiles").toString(),placed.getJSONObject("tiled_sources").getJSONArray("profiles").toString())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick()
        assertEquals(before.getJSONObject("tiled_sources").toString(),manifest(save("placement-undo.capy")).getJSONObject("tiled_sources").toString())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString())};tick()
        assertEquals(placed.getJSONObject("tiled_sources").toString(),manifest(save("placement-redo.capy")).getJSONObject("tiled_sources").toString())
        open(File(files,"placement-result.capy"))
        assertEquals(placed.getJSONObject("tiled_sources").toString(),manifest(save("placement-reopened.capy")).getJSONObject("tiled_sources").toString())
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
            compose.waitUntil(30_000) {host.snapshot?.getJSONObject("state")?.getJSONArray("layers")?.length()==placed.getJSONObject("document").getJSONArray("layers").length()+2}
            compose.waitUntil(30_000) {host.snapshot?.getJSONObject("state")?.getJSONObject("document_file")?.optBoolean("busy")==false}
            assertNull(host.actionError)
            compose.onNodeWithTag("apply_transform").assertIsDisplayed().performClick()
            compose.waitUntil(30_000) { host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.first { it.getString("id")=="placement_original_size" }?.getBoolean("enabled")==false }
            DocumentController.nativeFileJobsForTest=true
            val pasted=manifest(save("placement-pasted.capy"))
            assertEquals("U8",pasted.getJSONObject("document").getJSONObject("color").getString("depth"))
            for(image in pasted.getJSONObject("tiled_sources").getJSONArray("images").objects()) {
                assertEquals("U16",image.getString("depth"))
                assertEquals(source.getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString(),image.getJSONArray("tiles").toString())
            }
            assertEquals(source.getJSONArray("profiles").toString(),pasted.getJSONObject("tiled_sources").getJSONArray("profiles").toString())
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
        fun backingEqual(a:JSONObject,b:JSONObject){for(key in listOf("blobs","rasters","tiled_sources"))assertEquals(key,a.get(key).toString(),b.get(key).toString())}
        fun activeSource(m:JSONObject):JSONObject {
            val id=m.getJSONObject("document").getLong("active_layer");val s=m.getJSONObject("tiled_sources")
            val index=s.getJSONArray("layers").objects().first{it.getLong("target")==id}.getInt("image")
            return s.getJSONArray("images").getJSONObject(index)
        }
        val before=manifest(save("source-before.capy"))
        change("rasterize_source",cancel=true);backingEqual(before,manifest(save("source-cancel-worker.capy")))
        change("repair_source_profile",obj("Builtin" to "AdobeRgb"),apply=false);backingEqual(before,manifest(save("source-cancel-preview.capy")))
        assertFalse(change("repair_source_profile",obj("Builtin" to "AdobeRgb"))!!.getBoolean("adds_layer"))
        val repaired=manifest(save("source-repaired.capy"))
        assertEquals(before.getJSONArray("blobs").toString(),repaired.getJSONArray("blobs").toString())
        assertEquals("AdobeRgb",activeSource(repaired).getJSONObject("profile").getString("Builtin"))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick();backingEqual(before,manifest(save("source-repair-undo.capy")))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString())};tick();backingEqual(repaired,manifest(save("source-repair-redo.capy")))
        change("rasterize_source")
        val rasterized=manifest(save("source-rasterized.capy"));val image=activeSource(rasterized)
        assertEquals("Rasterized",image.getString("kind"));assertEquals("U8",image.getString("depth"));assertEquals("DisplayP3",image.getJSONObject("profile").getString("Builtin"))
        assertEquals(activeSource(repaired).getJSONArray("extent").toString(),image.getJSONArray("extent").toString())
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick();backingEqual(repaired,manifest(save("source-rasterize-undo.capy")))
        native {
            Native.dispatch(it,obj("type" to "invoke","command" to "fit_canvas").toString())
            Native.dispatch(it,obj("type" to "invoke","command" to "pen").toString())
        };tick();stroke(0.0)
        val painted=manifest(save("source-painted.capy"));val oldId=painted.getJSONObject("document").getLong("active_layer")
        assertTrue(change("repair_source_profile",obj("Builtin" to "ProPhoto"))!!.getBoolean("adds_layer"))
        val added=manifest(save("source-corrected-layer.capy"))
        assertEquals(painted.getJSONObject("document").getJSONArray("layers").length()+1,added.getJSONObject("document").getJSONArray("layers").length())
        assertEquals(painted.getJSONObject("document").getJSONArray("layers").objects().first{it.getLong("id")==oldId}.toString(),added.getJSONObject("document").getJSONArray("layers").objects().first{it.getLong("id")==oldId}.toString())
        assertEquals(painted.getJSONArray("blobs").toString(),added.getJSONArray("blobs").toString())
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
            assertEquals(a.getJSONArray("blobs").toString(),b.getJSONArray("blobs").toString())
            assertEquals(a.getJSONObject("document").getJSONObject("color").toString(),b.getJSONObject("document").getJSONObject("color").toString())
        }
        val before=manifest(save("color-before.capy"))
        change("assign_profile",obj("Assign" to "AdobeRgb"),true)
        sameBacking(before,manifest(save("color-cancel.capy")))
        change("assign_profile",obj("Assign" to "AdobeRgb"))
        val assigned=manifest(save("color-assigned.capy"))
        assertEquals(before.getJSONArray("blobs").toString(),assigned.getJSONArray("blobs").toString())
        assertEquals("AdobeRgb",assigned.getJSONObject("document").getJSONObject("color").getString("space"))
        change("undo");sameBacking(before,manifest(save("color-undo.capy")))
        change("redo");sameBacking(assigned,manifest(save("color-redo.capy")))
        change("convert_color_space",obj("Convert" to obj("space" to "ProPhoto","options" to obj("intent" to "RelativeColorimetric","black_point_compensation" to false))))
        val converted=manifest(save("color-converted.capy"))
        assertEquals("ProPhoto",converted.getJSONObject("document").getJSONObject("color").getString("space"))
        assertNotEquals(assigned.getJSONArray("blobs").toString(),converted.getJSONArray("blobs").toString())
        change("change_bit_depth",obj("Depth" to obj("depth" to "U8","dither" to "None")))
        val reduced=manifest(save("color-depth.capy"))
        assertTrue(reduced.getJSONArray("blobs").objects().all {it.getJSONObject("descriptor").getInt("bits_per_channel")==8})
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
        assertEquals(1,copied.getJSONObject("document").getJSONArray("layers").length())
        assertEquals("Srgb",copied.getJSONObject("document").getJSONObject("color").getString("space"))
        assertEquals("U8",copied.getJSONObject("document").getJSONObject("color").getString("depth"))
        assertEquals("Rasterized",copied.getJSONObject("tiled_sources").getJSONArray("images").getJSONObject(0).getString("kind"))
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

    @Test fun histogramCapturesCommittedDocumentAndCancelsIndependently() {
        val epoch = native { state(it).getJSONObject("document_file").getLong("epoch") }
        for (cancelled in listOf(true, false)) {
            val control = Native.captureControl()
            try {
                if (cancelled) Native.captureCancel(control)
                val task = native { Native.inspectionTask(it, control) }
                try {
                    val result = JSONObject(Native.inspectionHistogram(task))
                    if (cancelled) fail("Cancelled histogram completed")
                    val histogram = result.getJSONObject("histogram")
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
        val form=native {JSONObject(Native.query(it,obj("type" to "export_form").toString()))}
        val recipe=JSONObject(form.getJSONArray("recipes").getJSONArray(0).getJSONObject(1).toString())
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
        native { Native.dispatch(it, obj("type" to "invoke", "command" to "histogram").toString()) }
        compose.runOnUiThread { host.documentChanged() }
        compose.onNodeWithText("Histogram").assertIsDisplayed()
        compose.waitUntil(30_000) { compose.onAllNodesWithText("Current committed drawing").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Close").performClick()
        assertNull(host.failure)
    }
    @Test fun exactSnapshotsSurviveFilesGpuReplacementAndRecovery() {
        stroke(0.0)
        val first=save("first.capy")
        assertTrue(manifest(first).getJSONArray("blobs").length()>0)
        val firstPng=png("first.png")
        point(1,0.0,120.0);point(2,40.0,120.0)
        val during=saveTask() // Must exclude this active contact without blocking it.
        point(3,90.0,120.0)
        val duringBytes=finishSave(during,"during.capy")
        assertEquals(manifest(first).getJSONArray("blobs").toString(),manifest(duringBytes).getJSONArray("blobs").toString())
        assertTrue(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        val secondPng=png("second.png")
        assertNotEquals(hash(firstPng),hash(secondPng))
        native { Native.destroyGpuForTest(it) }
        compose.runOnUiThread { host.documentChanged() }
        compose.waitUntil(10_000) {host.failure != null}
        compose.runOnUiThread {host.restartCanvas()}
        compose.waitUntil(60_000) {host.surfaceReady && host.snapshot?.optBoolean("brush_ready")==true}
        assertNull(host.failure)
        assertEquals(hash(secondPng),hash(png("replaced.png")))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};tick()
        assertEquals(hash(firstPng),hash(png("undo.png")))
        native {Native.dispatch(it,obj("type" to "invoke","command" to "redo").toString())};tick()
        assertEquals(hash(secondPng),hash(png("redo.png")))
        open(File(files,"first.capy"))
        assertEquals(hash(firstPng),hash(png("opened.png")))
        assertEquals(manifest(first).getJSONArray("blobs").toString(),manifest(save("roundtrip.capy")).getJSONArray("blobs").toString())
        val corrupt=File(files,"corrupt.capy");corrupt.writeBytes(first.copyOf().also {it[it.lastIndex]=(it.last().toInt() xor 1).toByte()})
        val epoch=native {state(it).getJSONObject("document_file").getLong("epoch")}
        open(corrupt,true)
        assertEquals(epoch,native {state(it).getJSONObject("document_file").getLong("epoch")})
        val recovery=File(files,"atomic-recovery.capy")
        val capture=native {Native.projectRecoveryTask(it,false)}
        try {Native.projectPublish(capture,recovery.absolutePath)} finally {Native.projectFree(capture)}
        val stale=native {Native.projectRecoveryTask(it,true)}
        try {
            Native.projectWork(stale,ParcelFileDescriptor.open(recovery,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
            compose.runOnUiThread {host.restartCanvas()}
            try { compose.waitUntil(15_000) {host.surfaceReady && host.snapshot?.optBoolean("brush_ready")==true} }
            catch(e: Exception) { throw AssertionError("Restart: surface=${host.surfaceReady}; brush=${host.snapshot?.optBoolean("brush_ready")}; failure=${host.failure}; activity=${scenario.state}; focus=${activity.hasWindowFocus()}", e) }
            try {native {Native.projectAdopt(it,stale,"null")};fail("Candidate from the retired device was adopted")}
            catch(e: IllegalStateException) {assertTrue(e.message.orEmpty().contains("canvas changed"))}
            assertEquals(hash(firstPng),hash(png("stale-candidate-retained.png")))
        } finally {Native.projectFree(stale)}
        val restore=native {Native.projectRecoveryTask(it,true)}
        try {
            Native.projectWork(restore,ParcelFileDescriptor.open(recovery,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
            native {Native.projectAdopt(it,restore,"null")}
        } finally {Native.projectFree(restore)}
        tick()
        assertEquals(hash(firstPng),hash(png("recovered.png")))
        val recovered=native {state(it).getJSONObject("document_file")}
        assertTrue(recovered.getBoolean("modified"));assertTrue(recovered.isNull("location"))
        // Real Activity/surface recreation retains the ViewModel and history.
        val retained = host
        scenario.recreate()
        scenario.onActivity { activity = it }
        assertSame(retained, host)
        compose.waitUntil(60_000) { host.surfaceReady }
        assertEquals(hash(firstPng),hash(png("surface-recreated.png")))
        // Exercise the production controller and offer UI with a fresh session.
        val changed = java.util.concurrent.CountDownLatch(1)
        compose.runOnUiThread { host.documentChanged { changed.countDown() } }
        assertTrue(changed.await(10,java.util.concurrent.TimeUnit.SECONDS))
        var write: Job? = null
        compose.runOnUiThread { write = host.recovery.capture() }
        runBlocking { write?.join() }
        assertEquals(1,recoveryDirectory.listFiles().orEmpty().count { it.extension == "capy" })
        assertNull(host.actionError)
        scenario.close()
        launch()
        assertNotSame(retained,host)
        compose.waitUntil(10_000) {host.recovery.candidate != null}
        compose.onNodeWithTag("recover-drawing").performClick()
        compose.waitUntil(60_000) {host.recovery.candidate == null && !host.recovery.working}
        assertNull(host.actionError)
        assertEquals(hash(firstPng),hash(png("controller-recovered.png")))
        assertTrue(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        assertTrue(native {state(it).getJSONObject("document_file").isNull("location")})
        assertEquals(1,recoveryDirectory.listFiles().orEmpty().count { it.extension == "capy" })
        activity.getExternalFilesDir(null)!!.resolve("raster-result.txt").writeText("PASS: exact snapshots, active-contact save, undo/redo, GPU replacement, corrupt-file retention, atomic recovery, Activity recreation, recovery offer/adoption\n")
    }
}
