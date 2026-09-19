package art.capycanvas

import androidx.compose.runtime.*
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject

/** Poll on the owner, analyse an immutable snapshot on one worker. Supersession
 * cancels its atomic control; a successor waits until native work releases it. */
internal class HdrController(private val host:CanvasHost) {
    var status by mutableStateOf("")
    var details by mutableStateOf("")
    private var flag=0L
    private var paused=true
    private var loop:Job?=null
    private var running:Job?=null
    private var lifecycle=0
    private var generation=-1
    private var changed=0L
    private var surface:CanvasSurfaceView?=null
    private var displayInfo:Pair<Boolean,Float>?=null
    private var requestedHeadroom:Float?=null
    fun bindSurface(view:CanvasSurfaceView){surface=view;displayInfo=null;requestedHeadroom=null;updateDisplay(1f)}
    fun unbindSurface(view:CanvasSurfaceView){if(surface===view){surface=null;displayInfo=null;requestedHeadroom=null}}
    private fun updateDisplay(requested:Float) {
        val view=surface?:return
        val display=view.display
        val available=android.os.Build.VERSION.SDK_INT>=35&&display?.isHdr==true&&display.isHdrSdrRatioAvailable
        val desired=if(available)requested else 1f
        if(android.os.Build.VERSION.SDK_INT>=35&&requestedHeadroom!=desired){view.setDesiredHdrHeadroom(desired);requestedHeadroom=desired}
        val ratio=if(available)display!!.hdrSdrRatio.takeIf{it.isFinite()&&it>=1f}?:1f else 1f
        val info=available to ratio
        if(info!=displayInfo){displayInfo=info;host.displayInfo(available,ratio)}
    }
    fun pause(){paused=true;lifecycle++;if(flag!=0L)Native.captureCancel(flag);loop?.cancel();loop=null}
    fun resume(){if(!paused)return;paused=false
        loop=host.viewModelScope.launch {
            while(isActive&&!paused){
                try{
                    val state=JSONObject(host.withNative{Native.toneStatus(it)})
                    updateDisplay(state.number("requested_headroom",1.0))
                    val next=state.getInt("generation")
                    if(generation!=next){
                        generation=next;changed=android.os.SystemClock.elapsedRealtime()
                        if(flag!=0L)Native.captureCancel(flag)
                        host.documentChanged()
                    }
                    val headroom=state.number("display_headroom",1.0)
                    val reported=state.number("reported_headroom",1.0)
                    status=when {
                        !state.getBoolean("hdr")->""
                        headroom>1f->"HDR"
                        !state.isNull("error")->"SDR preview unavailable"
                        !state.getBoolean("retained")->"Preparing SDR…"
                        state.optString("proof_mode")=="sdr"->"SDR preview"
                        state.optString("proof_mode")=="print"->"Print proof"
                        else->"Showing SDR"
                    }
                    val route=if(state.optBoolean("display_hdr"))"Linear extended-range HDR surface."
                        else "Android has not offered a supported HDR surface and brightness-control path for this window."
                    val viewing=when {
                        headroom>1f->"HDR presentation · ${String.format(java.util.Locale.ROOT,"%.3f",headroom)}× Android headroom."
                        !state.isNull("error")->"SDR preview unavailable: ${state.getString("error")}"
                        state.optString("proof_mode")=="sdr"->"Showing the saved SDR appearance. Turn Proof Off to view HDR when available."
                        state.optString("proof_mode")=="print"->"Showing the SDR print preview."
                        reported>1f->"Showing the saved SDR appearance. Android currently grants only ${String.format(java.util.Locale.ROOT,"%.3f",reported)}× headroom; at least 1.05× is needed to switch this canvas to HDR."
                        else->"Showing the saved SDR appearance. Android has not reported HDR headroom for this window."
                    }
                    details="$viewing\n\n$route\n\nArtwork reference white: 203 cd/m². Display limits come from Android, not a brightness measurement. The HDR master is preserved."
                    if(!state.getBoolean("idle")){
                        if(flag!=0L)Native.captureCancel(flag)
                        changed=android.os.SystemClock.elapsedRealtime()
                    }
                    if(state.getBoolean("needed")&&running==null&&android.os.SystemClock.elapsedRealtime()-changed>=180)start(generation)
                }catch(e:CancellationException){throw e}catch(e:Exception){status="Display status unavailable";details=e.message?:"Could not read the display status"}
                delay(200)
            }
        }
    }
    private fun start(ticket:Int){
        val owner=lifecycle
        running=host.viewModelScope.launch {
            var task=0L
            val control=Native.captureControl();flag=control
            try{
                // Allocation and consumption cannot be abandoned between owners.
                withContext(NonCancellable){
                    task=host.withNative{Native.toneTask(it,control)}
                    withContext(Dispatchers.Default){Native.toneWork(task)}
                }
                ensureActive()
                if(!paused&&ticket==generation&&owner==lifecycle){if(host.withNative{Native.toneApply(it,task)})host.documentChanged()}
            }catch(e:CancellationException){throw e}
            catch(e:Exception){if(!paused&&ticket==generation&&owner==lifecycle&&!Native.captureCancelled(control))host.withNative{Native.toneFailed(it,ticket,e.message?:"HDR analysis failed")}}
            finally{
                flag=0
                withContext(NonCancellable+Dispatchers.Default){if(task!=0L)Native.toneRelease(task);Native.captureFree(control)}
            }
        }.also{job->job.invokeOnCompletion{host.viewModelScope.launch{if(running===job)running=null}}}
    }
}
