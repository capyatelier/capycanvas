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
    private fun builtinRecipe(index: Int): JSONObject {
        val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
        return runBlocking { ColorPreferencesStore.presets(activity, color, obj("type" to "get", "index" to index)) }.getJSONObject("recipe")
    }
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
    private fun point(phase: Int, dx: Double, dy: Double) {
        native { handle ->
        val viewport=host.snapshot!!.getJSONObject("state").getJSONObject("camera").getJSONArray("viewport")
        val bytes=doubleArrayOf(viewport.getDouble(0)*.50+dx,viewport.getDouble(1)*.5+dy,.65,0.0,0.0,0.0,0.0,System.nanoTime().toDouble(),phase.toDouble())
        Native.pointer(handle,71,0,0,bytes,bytes.size,false)
        val now=System.nanoTime(); Native.frame(handle,now,now+16_666_667)
    }
        // Direct JNI input bypasses CanvasHost.wake(). Honor frame()'s retry
        // contract before capturing the completed stroke on a nonblocking surface.
        if (phase == 3) compose.waitUntil(10_000) { !tick() }
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
                compose.waitUntil(120_000){tick();native{Native.projectParkReady(it,job.first)}}
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
        assertTrue("Native archive version", bytes[10].toInt() == 7 && bytes[11].toInt() == 0)
        val size=ByteBuffer.wrap(bytes,12,8).order(ByteOrder.LITTLE_ENDIAN).long.toInt()
        return JSONObject(bytes.copyOfRange(52,52+size).decodeToString())
    }
    private fun hash(bytes: ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()

    @Test fun selectionToolsRenderAndCombineOnDevice() {
        fun send(value: JSONObject) {
            native { Native.dispatch(it, value.toString()) }
            compose.waitUntil(30_000) { !tick() }
        }
        fun invoke(id: String) = send(obj("type" to "invoke", "command" to id))
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
        val capture=native {Native.projectRecoveryTask(it,false)}
        try {Native.projectPublish(capture,recovery.absolutePath)} finally {Native.projectFree(capture)}
        val saveMs=(SystemClock.elapsedRealtimeNanos()-saveStart)/1_000_000
        val index=manifest(recovery.readBytes())
        assertTrue(index.getJSONObject("selections").getJSONArray("pixels").length()>0)
        assertTrue("Selection metadata stays small",index.toString().length<512*1024)
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
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.retire(id,closedTab=true)?.join()}
        }
        val activation=native {Native.documentSwitch(it,0,true)}
        if(activation!=0L) try {Native.documentResumeWork(activation);native {Native.documentResume(it,activation)}} finally {Native.documentResumeFree(activation)}
        val restore=native {Native.projectRecoveryTask(it,true)}
        try {
            Native.projectWork(restore,ParcelFileDescriptor.open(recovery,ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0)
            compose.waitUntil(120_000) {tick();native {Native.projectParkReady(it,restore)}}
            native {Native.projectAdopt(it,restore,"null")}
        } finally {Native.projectFree(restore)}
        compose.waitUntil(60_000) {!tick()}
        assertTrue("Recovery restores the 61 MP mask",native {state(it).getJSONObject("layer_tools").getBoolean("has_selection")})
        assertNull(host.actionError);assertNull(host.failure)
        val report="PASS native Android 61 MP RGB: first_mask=${timings.first()}ms; warm=${timings.drop(1)}ms; quick_mask=${quick}ms; recovery=${saveMs}ms; archive=${recovery.length()} bytes; metadata=${index.toString().length} bytes"
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
        fun send(value:JSONObject) { native { Native.dispatch(it,value.toString()) }; compose.waitUntil(30_000) { !tick() } }
        fun invoke(id:String)=send(obj("type" to "invoke","command" to id))
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
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
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
        compose.onNodeWithTag("paper-color-bucket").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="mask_color" }.getJSONObject("value").getJSONObject("value").getJSONArray("rgba").getDouble(1)==.5 }
        send(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to "properties", "visible" to false)))
        compose.onNodeWithTag("selection-load-0").performClick()
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
        val load=compose.onNodeWithTag("selection-load-$id").fetchSemanticsNode().boundsInRoot
        assertTrue("Thumbnail $thumb and Load $load align",kotlin.math.abs(thumb.width-load.width)<=thumb.width*.1f && load.left>=thumb.right && load.left-thumb.right<20f)
        send(obj("type" to "selection", "action" to obj("op" to "begin_resize", "grow" to true, "layer" to id)))
        compose.onNodeWithText("Grow Selection").assertIsDisplayed()
        compose.onNodeWithText("Apply").performClick()
        compose.waitUntil(30_000) { !tick() }
        invoke("undo");invoke("redo")
        invoke("clear_selection_mask"); invoke("return_to_artwork")
        invoke("deselect")
        compose.onNodeWithTag("selection-load-$id").assertIsDisplayed().performClick()
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
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        val automation=instrumentation.uiAutomation
        fun fixture(name:String)=File(files,name).apply {
            writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $root/$name")).use{it.readBytes()})
            assertTrue("Fixture $name",length()>0)
        }
        fun refresh(){tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()}
        fun idle(){compose.waitUntil(120_000){!host.documents.working&&!native{state(it).getJSONObject("document_file").getBoolean("busy")}};assertNull(host.failure);assertNull(host.actionError)}
        fun histogram():JSONObject {val c=Native.captureControl();try{return JSONObject(Native.inspectionHistogram(native{Native.inspectionTask(it,c)})).getJSONObject("histogram")}finally{Native.captureFree(c)}}
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
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
        val input=File(files,"large-photo.avif").apply {
            writeBytes(ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand("cat $path")).use{it.readBytes()})
        }
        val quality=arguments.getString("photoQuality")?.toInt()?:90
        val report=obj("model" to android.os.Build.MODEL,"input" to path,"quality" to quality,"exports" to org.json.JSONArray())
        val output=File(activity.getExternalFilesDir(null),"portable-large-report.json")
        val watching=java.util.concurrent.atomic.AtomicBoolean(true)
        val peak=java.util.concurrent.atomic.AtomicLong()
        val sampler=Thread{while(watching.get()){peak.accumulateAndGet(android.os.Debug.getPss().toLong()*1024,::maxOf);SystemClock.sleep(250)}}.apply{start()}
        fun refresh(){tick();compose.runOnUiThread{host.documentChanged()};compose.waitForIdle()}
        fun histogram():JSONObject {
            val c=Native.captureControl()
            try{return JSONObject(Native.inspectionHistogram(native{Native.inspectionTask(it,c)})).getJSONObject("histogram")}
            finally{Native.captureFree(c)}
        }
        try {
            val started=SystemClock.uptimeMillis();open(input);refresh()
            report.put("open_ms",SystemClock.uptimeMillis()-started)
            val master=save("large-master.capy")
            val document=manifest(master).getJSONObject("document")
            val extent=listOf(document.getInt("width"),document.getInt("height"))
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
                val restored=manifest(save("large-reopened.capy")).getJSONObject("document")
                assertEquals(extent,listOf(restored.getInt("width"),restored.getInt("height")))
                output.writeText(report.toString(2))
            }
            assertNull(host.failure)
        }catch(e:Throwable){report.put("error",e.toString());throw e}
        finally{watching.set(false);sampler.join(2000);report.put("peak_process_pss_bytes",peak.get());output.writeText(report.toString(2))}
    }

    @Test fun hdrBlackIntensityMarkerVisible() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile") ?: throw AssumptionViolatedException("Supply -e hdrFile")
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
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
        assertEquals("EV drag follows the visible arc",3.6,host.panelContent!!.getJSONObject("color_panel").getDouble("intensity"),.06)
        compose.onNodeWithTag("color-hdr-intensity").performTouchInput { down(point(.7));moveTo(point(.4));cancel() }
        compose.waitForIdle()
        assertEquals("Cancelled EV drag restores its value",3.6,host.panelContent!!.getJSONObject("color_panel").getDouble("intensity"),.06)
    }

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
        val basic=builtinRecipe(0);val recipe=native{h->JSONObject(Native.query(h,obj("type" to "export_draft","recipe" to basic,"action" to obj("type" to "format","value" to "PngHdr")).toString())).getJSONObject("recipe")}
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
            val report=obj("pen_up_wait_ms" to (SystemClock.uptimeMillis()-released),"status" to status(),"motion" to motion)
            File(activity.getExternalFilesDir(null),"gpu-tone-retention.json").writeText(report.toString(2))
            println("GPU_TONE "+report)
            val second=status().getInt("publications")
            native{Native.dispatch(it,obj("type" to "invoke","command" to "undo").toString())};refresh();ready()
            assertTrue(status().getInt("publications")>second)
        } finally {Native.toneRelease(task);Native.captureFree(control)}
        assertNull(host.failure)
    }

    @Test fun hdrDisplayNegotiation() {
        val sourcePath=InstrumentationRegistry.getArguments().getString("hdrFile") ?: throw AssumptionViolatedException("Supply -e hdrFile for the display regression")
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
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
        compose.waitUntil(5_000){native{JSONObject(Native.displayStatus(it)).optInt("presented_tone_generation",-1)}==tone().getInt("generation")}
        val supports=tone().getBoolean("display_hdr")
        fun awaitSurface(hdr:Boolean) {
            compose.waitUntil(10_000){native{JSONObject(Native.displayStatus(it)).let{s->
                s.opt("presented_hdr")==hdr&&s.optString("color_space")==if(hdr)"Bt2100Pq" else "Srgb"
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
            val coords=arrayOf(android.view.MotionEvent.PointerCoords().apply {
                x=cx+40*kotlin.math.sin(elapsed/250.0).toFloat()
                y=cy+20*kotlin.math.cos(elapsed/310.0).toFloat()
                pressure=if(phase==android.view.MotionEvent.ACTION_UP)0f else .65f
            })
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
            "timeline" to timeline, "surface_layer" to layer, "surface_latency" to latency, "renderer_stats" to stats, "failure" to host.failure,
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
        batch(photos);press("apply_transform");memoryStage("applied batch")
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
        val recipe = builtinRecipe(2).put("format", "Png")
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
            compose.waitUntil(120_000) { tick(); native { Native.projectParkReady(it, pending) } }
            native { Native.projectAdopt(it, pending, "null") }
        } finally { Native.projectFree(pending) }
        val assumed = manifest(save("assumed16.capy"))
        assertEquals("AdobeRgb", assumed.getJSONObject("document").getJSONObject("color").getString("space"))
        assertEquals(original.getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString(), assumed.getJSONObject("tiled_sources").getJSONArray("images").getJSONObject(0).getJSONArray("tiles").toString())
        native { Native.dispatch(it, obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "missing_profile", "value" to 0)).toString()) }
    }
    @Test fun profileLibraryKeepsExactCopiesAndPresetOwnership() {
        val wide=builtinRecipe(1)
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
            val recipe=ColorPreferencesStore.presets(activity,color,obj("type" to "get","index" to 0)).getJSONObject("recipe").put("profile",profile)
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
        fun store(action:JSONObject)=runBlocking{ColorPreferencesStore.presets(activity,color,action)}
        fun canonical(recipe:JSONObject)=native{Native.query(it,obj("type" to "export_validate","recipe" to recipe).toString())}
        val recipe=store(obj("type" to "get","index" to 1)).getJSONObject("recipe")
            .put("depth","U16").put("size",obj("Fit" to obj("bounds" to org.json.JSONArray(listOf(321,123)),"enlarge" to false))).put("resolution",obj("Ppi" to 287))
        val saved=store(obj("type" to "save","name" to "Tablet test delivery","recipe" to recipe))
        val index=saved.getInt("index");assertEquals(4,index)
        assertEquals(canonical(recipe),canonical(store(obj("type" to "get","index" to index)).getJSONObject("recipe")))
        val persisted=File(ColorPreferencesStore.directoryForTest!!,"color-export-presets.json").readBytes()
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
        native { Native.dispatch(it, obj("type" to "invoke", "command" to "histogram").toString()) }
        compose.runOnUiThread { host.documentChanged() }
        compose.onNodeWithText("Histogram").assertIsDisplayed()
        compose.waitUntil(30_000) { compose.onAllNodesWithText("Current committed drawing").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Close").performClick()
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
            assertEquals(3, display().getInt("desired_maximum_frame_latency"))
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
            compose.waitUntil(60_000) { host.surfaceReady && host.snapshot?.optBoolean("brush_ready")==true }
            compose.waitUntil(10_000) { !tick() }
            assertNull(host.failure)
            val display=native { JSONObject(Native.displayStatus(it)) }
            assertTrue(display.getString("present_mode") in listOf("SharedDemandRefresh", "Fifo"))
            assertEquals(display.getString("present_mode") == "SharedDemandRefresh", display.getBoolean("retained_target"))
        }
        stroke(0.0)
        val painted=hash(png("front-painted.png"))
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.CREATED)
        scenario.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        ready()
        assertEquals(painted,hash(png("front-resumed.png")))
        scenario.recreate()
        scenario.onActivity { activity=it }
        ready()
        assertEquals(painted,hash(png("front-recreated.png")))
        val automation=InstrumentationRegistry.getInstrumentation().uiAutomation
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
        native { Native.destroyGpuForTest(it) }
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
        history("undo")
        assertEquals(hash(firstPng),hash(png("undo.png")))
        history("redo")
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
        // Opening/recovering now creates drawing tabs, each with its own copy.
        val modifiedTabs=native{h->JSONObject(Native.documentTabs(h,obj("op" to "view").toString())).array("tabs").objects().count {tab->
            JSONObject(Native.documentTabs(h,obj("op" to "recovery","id" to tab.getLong("id")).toString())).getBoolean("modified")
        }}
        assertEquals(modifiedTabs,recoveryDirectory.listFiles().orEmpty().count { it.extension == "capy" })
        assertNull(host.actionError)
        scenario.close()
        launch()
        assertNotSame(retained,host)
        compose.waitUntil(10_000) {host.recovery.candidate != null}
        val offered=host.recovery.candidate
        compose.onNodeWithTag("recover-drawing").performClick()
        compose.waitUntil(60_000) {host.recovery.candidate != offered && !host.recovery.working}
        assertNull(host.actionError)
        assertEquals(hash(firstPng),hash(png("controller-recovered.png")))
        assertTrue(native {state(it).getJSONObject("document_file").getBoolean("modified")})
        assertTrue(native {state(it).getJSONObject("document_file").isNull("location")})
        assertEquals(modifiedTabs,recoveryDirectory.listFiles().orEmpty().count { it.extension == "capy" })
        activity.getExternalFilesDir(null)!!.resolve("raster-result.txt").writeText("PASS: exact snapshots, active-contact save, undo/redo, GPU replacement, corrupt-file retention, atomic recovery, Activity recreation, recovery offer/adoption\n")
    }
    @Test fun drawingTabsKeepHistorySpillAndLifecycle() {
        fun tabs()=native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}
        fun ids()=tabs().array("tabs").objects().map{it.getLong("id")}
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
        val first=tabs().getLong("selected");val generation=tabs().getLong("gpu_generation")
        stroke(0.0);val exact=manifest(save("tabs-exact.capy")).getJSONArray("blobs").toString()
        action("undo") // Its only ink is now retained exclusively by redo history.
        val second=fresh();assertEquals(listOf(first,second),ids())
        order(obj("op" to "budget","bytes" to 0));trim()
        assertEquals(0,tabs().getLong("resident_bytes"));assertEquals(0,tabs().getInt("parked_renderers"))
        action("add_layer");val secondLayers=native{state(it).array("layers").length()}
        select(first);action("redo")
        assertEquals(exact,manifest(save("tabs-redo.capy")).getJSONArray("blobs").toString())
        assertEquals(generation,tabs().getLong("gpu_generation"))
        select(second);assertEquals(secondLayers,native{state(it).array("layers").length()})
        action("undo");assertEquals(secondLayers-1,native{state(it).array("layers").length()})
        val third=fresh()
        order(obj("op" to "reorder","id" to third,"before" to first));assertEquals(listOf(third,first,second),ids())
        select(first);order(obj("op" to "history","redo" to false));assertEquals(listOf(first,second,third),ids());assertEquals(first,tabs().getLong("selected"))
        order(obj("op" to "history","redo" to true));assertEquals(listOf(third,first,second),ids())
        val retained=host;scenario.recreate();scenario.onActivity{activity=it};assertSame(retained,host)
        compose.waitUntil(60_000){host.surfaceReady};assertEquals(listOf(third,first,second),ids())
        assertEquals(exact,manifest(save("tabs-recreated.capy")).getJSONArray("blobs").toString())
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

    @Test fun drawingTabsRecoverMultipleInactiveDrawings() {
        fun tabs()=native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}
        fun action(command:String){native{Native.dispatch(it,obj("type" to "invoke","command" to command).toString())};tick()}
        action("add_layer")
        val firstLayers=native{state(it).array("layers").length()}
        val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
        try{Native.projectWork(task,-1,640,480);compose.waitUntil(60_000){tick();native{Native.projectParkReady(it,task)}};native{Native.projectAdopt(it,task,"null")}}finally{Native.projectFree(task)}
        tick();compose.waitUntil(120_000){native{JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}}
        action("add_layer");action("add_layer")
        val secondLayers=native{state(it).array("layers").length()}
        var write:Job?=null
        compose.runOnUiThread{host.documentChanged();write=host.recovery.capture()};runBlocking{write?.join()}
        assertEquals(2,recoveryDirectory.listFiles().orEmpty().count{it.extension=="capy"})
        assertEquals(listOf(true,true),tabs().array("tabs").objects().map{it.getBoolean("modified")})
        scenario.close();launch()
        repeat(2) {
            compose.waitUntil(30_000){host.recovery.candidate!=null&&!host.recovery.working}
            compose.onNodeWithTag("recover-drawing").performClick()
            compose.waitUntil(120_000){!host.recovery.working&&tabs().array("tabs").length()==it+2}
            assertNull(host.failure);assertNull(host.actionError)
        }
        assertEquals(3,tabs().array("tabs").length());assertNull(host.recovery.candidate)
        val counts=tabs().array("tabs").objects().drop(1).map { tab ->
            assertTrue(tab.getBoolean("modified"));assertTrue(tab.isNull("uri"))
            val capture=native{Native.projectRecoveryFor(it,tab.getLong("id"))};val file=File(files,"recovered-tab-${tab.getLong("id")}.capy")
            try{Native.projectPublish(capture,file.absolutePath)}finally{Native.projectFree(capture)}
            manifest(file.readBytes()).getJSONObject("document").getJSONArray("layers").length()
        }
        assertEquals(setOf(firstLayers,secondLayers),counts.toSet())
        assertEquals(2,recoveryDirectory.listFiles().orEmpty().count{it.extension=="capy"})
        activity.getExternalFilesDir(null)!!.resolve("drawing-tabs-recovery.txt").writeText("PASS two independently owned inactive/active recovery snapshots; sequential offers append unsaved independent drawings; durable origins retired only after publication")
    }

    @Test fun drawingTabsNativePointerReorder() {
        fun tabs()=native{JSONObject(Native.documentTabs(it,obj("op" to "view").toString()))}
        fun ids()=tabs().array("tabs").objects().map{it.getLong("id")}
        fun settled(){compose.waitUntil(60_000){!host.drawingTabs.switching&&native{JSONObject(Native.documentTabs(it,obj("op" to "ready").toString())).getBoolean("park")}};compose.runOnUiThread{host.documentChanged()};compose.waitForIdle();assertNull(host.failure);assertNull(host.actionError)}
        val task=native{h->val(id,file)=request(h,"new_document");Native.projectTask(h,id,"null",file.getLong("epoch"),file.getLong("revision"))}
        try{Native.projectWork(task,-1,640,480);compose.waitUntil(60_000){tick();native{Native.projectParkReady(it,task)}};native{Native.projectAdopt(it,task,"null")}}finally{Native.projectFree(task)}
        val order=ids();val selected=tabs().getLong("selected")
        val workspace=native{state(it).getJSONObject("workspace")}
        workspace.getJSONObject("layout").put("header",obj("size" to "large","next_id" to 2,"zones" to org.json.JSONArray(listOf(org.json.JSONArray(),org.json.JSONArray(listOf(obj("id" to 1,"item" to obj("kind" to "document_title")))),org.json.JSONArray()))))
        native{Native.dispatch(it,obj("type" to "restore_workspace","workspace" to workspace).toString())};settled()
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        fun roots(view:android.view.View):List<androidx.compose.ui.platform.ViewRootForTest> = when(view) {
            is androidx.compose.ui.platform.ViewRootForTest -> listOf(view)
            is android.view.ViewGroup -> (0 until view.childCount).flatMap{roots(view.getChildAt(it))}
            else -> emptyList()
        }
        fun find(node:androidx.compose.ui.semantics.SemanticsNode,tag:String):androidx.compose.ui.semantics.SemanticsNode? {
            if(node.config.contains(SemanticsProperties.TestTag)&&node.config[SemanticsProperties.TestTag]==tag)return node
            return node.children.firstNotNullOfOrNull{find(it,tag)}
        }
        fun locate(tag:String,within:android.view.View?=null):Pair<android.view.View,androidx.compose.ui.geometry.Rect> {
            var found:Pair<android.view.View,androidx.compose.ui.geometry.Rect>?=null
            instrumentation.runOnMainSync{found=android.view.inspector.WindowInspector.getGlobalWindowViews().flatMap(::roots).filter{within==null||it.view===within}.firstNotNullOfOrNull{root->find(root.semanticsOwner.unmergedRootSemanticsNode,tag)?.let{root.view to it.boundsInRoot}}}
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
                val source=when(tool){android.view.MotionEvent.TOOL_TYPE_MOUSE->android.view.InputDevice.SOURCE_MOUSE;android.view.MotionEvent.TOOL_TYPE_STYLUS->android.view.InputDevice.SOURCE_STYLUS;else->android.view.InputDevice.SOURCE_TOUCHSCREEN}
                val buttons=if(tool==android.view.MotionEvent.TOOL_TYPE_MOUSE&&action!=android.view.MotionEvent.ACTION_UP&&action!=android.view.MotionEvent.ACTION_CANCEL)android.view.MotionEvent.BUTTON_PRIMARY else 0
                val event=android.view.MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,arrayOf(android.view.MotionEvent.PointerProperties().apply{id=0;toolType=tool}),arrayOf(android.view.MotionEvent.PointerCoords().apply{x=point.x;y=point.y;pressure=.7f}),0,buttons,1f,1f,0,0,source,0)
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

}
