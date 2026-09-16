package art.capycanvas

import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.Choreographer
import android.view.WindowManager
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Test
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.*

/** Synthetic two-finger records through the ordinary owner queue and vsync path.
 * Does not claim hardware input-to-photon latency; retain raw host timestamps. */
class AndroidPhotoNavigationBenchmarkTest {
    @Test fun fullSizePhotoNavigation() {
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        assumeTrue(InstrumentationRegistry.getArguments().getString("photoBenchmark")=="true")
        val root=File(instrumentation.targetContext.cacheDir,"photo-bench-${System.nanoTime()}")
        CanvasHost.workspaceDirectoryForTest=File(root,"workspace").absolutePath
        RecoveryController.directoryForTest=File(root,"recovery")
        DocumentController.nativeFileJobsForTest=true
        try { ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var activity:MainActivity
            scenario.onActivity {activity=it;it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)}
            val host=activity.host
            fun <T> native(block:(Long)->T):T=runBlocking{host.withNative(block)}
            fun waitFor(condition:()->Boolean){val start=SystemClock.uptimeMillis();while(!condition()){assertNull(host.failure);check(SystemClock.uptimeMillis()-start<120_000){"Photo benchmark did not settle: ${host.actionError}"};SystemClock.sleep(20)}}
            fun report(reset:Boolean):JSONObject{val done=CountDownLatch(1);var result=JSONObject();host.measurements(reset){result=it;done.countDown()};check(done.await(30,TimeUnit.SECONDS));return result}
            waitFor {host.snapshot?.optBoolean("shaders_ready")==true && host.workspaceManager?.optBoolean("ready")==true && host.workspaceManager?.optBoolean("busy")==false}
            val task=native { h -> Native.dispatch(h,obj("type" to "invoke","command" to "open_document").toString());val state=JSONObject(Native.snapshot(h)!!).getJSONObject("state");val request=state.getJSONArray("requests").getJSONObject(0);val f=state.getJSONObject("document_file");Native.projectTask(h,request.getInt("id"),"null",f.getLong("epoch"),f.getLong("revision")) }
            val started=SystemClock.elapsedRealtime()
            try {Native.projectWork(task,ParcelFileDescriptor.open(File(activity.filesDir,"photo-benchmark.jpg"),ParcelFileDescriptor.MODE_READ_ONLY).detachFd(),0,0);native{Native.projectAdopt(it,task,"null")}}finally{Native.projectFree(task)}
            scenario.onActivity {host.documentChanged();host.invoke("fit_canvas")}
            waitFor{host.snapshot?.getJSONObject("state")?.getJSONArray("tabs")?.getJSONObject(0)?.optInt("width")==9504 && host.snapshot?.optBoolean("shaders_ready")==true}
            val importMs=SystemClock.elapsedRealtime()-started
            val camera=host.snapshot!!.getJSONObject("state").getJSONObject("camera")
            val area=camera.getJSONArray("work_area");val cx=area.getDouble(0)+area.getDouble(2)/2;val cy=area.getDouble(1)+area.getDouble(3)/2
            val output=activity.getExternalFilesDir(null)!!;val results=JSONArray()
            repeat(3){run ->
                report(true)
                val done=CountDownLatch(1);var index=0
                val cadence=JSONArray()
                fun point(id:Long,phase:Int,x:Double,y:Double,time:Long){val records=host.pointerBuffer(9);doubleArrayOf(x,y,1.0,0.0,0.0,0.0,0.0,time.toDouble(),phase.toDouble()).copyInto(records);host.pointer(id,3,0,records,9)}
                scenario.onActivity {
                    val clock=Choreographer.getInstance()
                    clock.postFrameCallback(object:Choreographer.FrameCallback{override fun doFrame(time:Long){
                        cadence.put(time)
                        val t=index/360.0;val radius=30.0*20.0.pow((1-cos(t*2*PI))/2);val angle=.8*sin(t*2*PI)
                        val x=cx+90*sin(t*4*PI);val y=cy+50*sin(t*2*PI)
                        val phase=if(index==0)1 else if(index==360)3 else 2
                        point(7101,phase,x-radius*cos(angle),y-radius*sin(angle),time)
                        point(7102,phase,x+radius*cos(angle),y+radius*sin(angle),time)
                        if(index++<360)clock.postFrameCallback(this)else done.countDown()
                    }})
                }
                check(done.await(60,TimeUnit.SECONDS));SystemClock.sleep(200)
                val measured=report(false);measured.put("input_vsync_ns",cadence);measured.put("run",run)
                measured.put("renderer",native{JSONObject(Native.query(it,obj("type" to "renderer_stats").toString()))})
                output.resolve("photo-navigation-$run.json").writeText(measured.toString())
                results.put(obj("run" to run,"frames" to measured.getJSONArray("frames").length(),"input_ticks" to cadence.length()))
            }
            output.resolve("photo-navigation-info.json").writeText(obj("import_ms" to importMs,"camera" to camera,"runs" to results).toString(2))
            println("Photo navigation completed: $results; import_ms=$importMs")
            assertNull(host.failure);assertNull(host.actionError)
        }} finally {CanvasHost.workspaceDirectoryForTest=null;RecoveryController.directoryForTest=null;DocumentController.nativeFileJobsForTest=false}
    }
}
