package art.capycanvas

import android.os.ParcelFileDescriptor
import android.os.SystemClock
import androidx.compose.ui.test.*
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
        } finally {Native.projectFree(task)}
    }
    private fun manifest(bytes: ByteArray): JSONObject {
        assertArrayEquals("CAPYRASTER".toByteArray(),bytes.copyOfRange(0,10))
        assertTrue("Native archive version", bytes[10].toInt() in 4..5 && bytes[11].toInt() == 0)
        val size=ByteBuffer.wrap(bytes,12,8).order(ByteOrder.LITTLE_ENDIAN).long.toInt()
        return JSONObject(bytes.copyOfRange(52,52+size).decodeToString())
    }
    private fun hash(bytes: ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()

    private fun summary(values: org.json.JSONArray): JSONObject? {
        if(values.length()==0)return null
        val sorted=(0 until values.length()).map {values.getDouble(it)}.sorted()
        return obj("count" to sorted.size,"p50" to sorted[((sorted.size-1)*.5).toInt()],"p95" to sorted[((sorted.size-1)*.95).toInt()],"max" to sorted.last())
    }
    private fun motion(tool: Int, steps: Int, center: Pair<Double, Double> = 1000.0 to 750.0): JSONObject {
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
            try {assertTrue(InstrumentationRegistry.getInstrumentation().uiAutomation.injectInputEvent(event,phase==android.view.MotionEvent.ACTION_UP))}finally{event.recycle()}
            if (phase == android.view.MotionEvent.ACTION_UP) break
            i++; SystemClock.sleep(4)
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
