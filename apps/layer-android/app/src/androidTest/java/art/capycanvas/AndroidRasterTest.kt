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
        val root = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir, "raster-test-${System.nanoTime()}")
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
        assertArrayEquals("CAPYRASTER\u0004\u0000".toByteArray(),bytes.copyOfRange(0,12))
        val size=ByteBuffer.wrap(bytes,12,8).order(ByteOrder.LITTLE_ENDIAN).long.toInt()
        return JSONObject(bytes.copyOfRange(52,52+size).decodeToString())
    }
    private fun hash(bytes: ByteArray)=MessageDigest.getInstance("SHA-256").digest(bytes).toList()
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
        open(File(files,"color-depth.capy"));sameBacking(reduced,manifest(save("color-reopened.capy")))
        DocumentController.nativeFileJobsForTest=false
        compose.runOnUiThread {host.invoke("assign_profile")}
        compose.waitUntil(10_000) {compose.onAllNodesWithText("Preview Complete Result").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithText("Preview Complete Result").performClick()
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
