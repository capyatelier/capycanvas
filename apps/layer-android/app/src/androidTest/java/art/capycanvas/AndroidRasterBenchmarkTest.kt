package art.capycanvas

import android.os.SystemClock
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import android.view.WindowManager
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread
import kotlin.math.sin

/** Display-callback measurements with synthetic pen records on the real device.
 * Can also run unchanged on pre-raster main for the frame-creation comparison. */
class AndroidRasterBenchmarkTest {
    companion object {
        private val recoveryClass = try { Class.forName("art.capycanvas.RecoveryController") } catch (_: ClassNotFoundException) { null }
        private fun recoveryDirectory(file: File?) {
            recoveryClass?.getDeclaredField("directoryForTest")?.apply { isAccessible = true }?.set(null,file)
        }
        @JvmStatic @BeforeClass fun isolate() {
            val root = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir,"raster-bench-${System.nanoTime()}")
            CanvasHost.workspaceDirectoryForTest = File(root,"workspace").absolutePath
            recoveryDirectory(File(root,"recovery"))
        }
        @JvmStatic @AfterClass fun reset() { CanvasHost.workspaceDirectoryForTest = null; recoveryDirectory(null) }
    }
    @get:Rule val compose = createEmptyComposeRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private val host get() = activity.host
    private fun measured(reset: Boolean): JSONObject {
        val done=CountDownLatch(1);var result: JSONObject?=null
        host.measurements(reset) {result=it;done.countDown()}
        check(done.await(10,TimeUnit.SECONDS));return result!!
    }
    @Before fun launch() {
        val automation = InstrumentationRegistry.getInstrumentation().uiAutomation
        for (command in listOf("input keyevent KEYCODE_WAKEUP", "wm dismiss-keyguard")) {
            android.os.ParcelFileDescriptor.AutoCloseInputStream(automation.executeShellCommand(command)).use { it.readBytes() }
        }
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { activity = it; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
    }
    @After fun close() { if (::scenario.isInitialized) scenario.close() }
    @Test fun frameCreationWithConcurrentSnapshots() {
        compose.waitUntil(60_000) {host.snapshot?.optBoolean("shaders_ready")==true || host.failure!=null}
        assertNull(host.failure)
        compose.waitUntil(60_000) {host.workspaceManager?.optBoolean("ready")==true && host.workspaceManager?.optBoolean("busy")==false}
        compose.runOnIdle {host.invoke("new_document")}
        compose.waitUntil(10_000) {compose.onAllNodesWithTag("new-document-create").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithTag("new-document-width").performTextReplacement("6000")
        compose.onNodeWithTag("new-document-height").performTextReplacement("4000")
        compose.onNodeWithTag("new-document-create").performClick()
        compose.waitUntil(60_000) {host.snapshot?.getJSONObject("state")?.getJSONArray("tabs")?.getJSONObject(0)?.optInt("width")==6000 && host.snapshot?.optBoolean("brush_ready")==true}
        val viewport=host.snapshot!!.getJSONObject("state").getJSONObject("camera").getJSONArray("viewport")
        val width=viewport.getDouble(0);val height=viewport.getDouble(1)
        val capture=Native::class.java.methods.firstOrNull {it.name=="projectRecoveryTask"}
        val publish=Native::class.java.methods.firstOrNull {it.name=="projectPublish"}
        val report=JSONArray()
        repeat(3) {run ->
            measured(true)
            var save: Thread?=null;var saveMs: Double?=null;var saveError: Throwable?=null
            for(contact in 0..4) {
                fun point(phase: Int,index: Int) {
                    val bytes=host.pointerBuffer(9)
                    val sample=doubleArrayOf(width*.4+index*2.5,height*.4+contact*height*.025+sin(index*.06)*15,.6,0.0,0.0,0.0,0.0,System.nanoTime().toDouble(),phase.toDouble())
                    sample.copyInto(bytes);host.pointer((800+contact+run*5).toLong(),0,0,bytes,9)
                }
                point(1,0)
                for(index in 1..192) {
                    point(2,index);SystemClock.sleep(8)
                    if(contact==2 && index==20 && capture!=null && publish!=null) {
                        val task=runBlocking {host.withNative {capture.invoke(null,it,false) as Long}}
                        save=thread(name="raster-benchmark-save") {
                            val start=System.nanoTime()
                            try {publish.invoke(null,task,File(activity.cacheDir,"raster-bench-$run.capy").absolutePath);saveMs=(System.nanoTime()-start)/1e6}
                            catch(e: Throwable){saveError=e}
                            finally {Native.projectFree(task)}
                        }
                    }
                }
                point(3,192);SystemClock.sleep(40)
                if(contact==0)measured(true) // Warm the current brush/capture worker.
            }
            save?.join(30_000);assertFalse(save?.isAlive==true);assertNull(saveError)
            val frames=measured(false).getJSONArray("frames")
            val paint=(0 until frames.length()).map {frames.getJSONArray(it).getLong(4)/1e6}.sorted()
            val callback=(0 until frames.length()).map {frames.getJSONArray(it).getLong(2)/1e6}.sorted()
            assertTrue("Enough display callbacks in run $run: ${paint.size}; canvas=${host.failure}; surface=${host.surfaceReady}",paint.size>100)
            fun q(values: List<Double>,p: Double)=values[(values.size*p).toInt().coerceAtMost(values.lastIndex)]
            report.put(obj("run" to run,"frames" to paint.size,"paint_p50_ms" to q(paint,.5),"paint_p95_ms" to q(paint,.95),"paint_p99_ms" to q(paint,.99),"paint_max_ms" to paint.last(),"render_present_p99_ms" to q(callback,.99),"save_ms" to saveMs))
            activity.getExternalFilesDir(null)!!.resolve("raster-frames-$run.json").writeText(frames.toString())
        }
        activity.getExternalFilesDir(null)!!.resolve("raster-benchmark.json").writeText(report.toString(2))
        println("Raster frame benchmark: $report")
        assertNull(host.failure)
    }
}
