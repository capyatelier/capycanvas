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
    private var displayAvailable:Boolean?=null
    private var requestedHeadroom:Float?=null
    fun bindSurface(view:CanvasSurfaceView){surface=view;displayAvailable=null;requestedHeadroom=null;updateDisplay(false)}
    fun unbindSurface(view:CanvasSurfaceView){if(surface===view){surface=null;displayAvailable=null;requestedHeadroom=null}}
    private fun updateDisplay(hdr:Boolean) {
        val view=surface?:return
        val display=view.display
        val available=android.os.Build.VERSION.SDK_INT>=35&&display?.hdrCapabilities?.supportedHdrTypes?.any {
            it==android.view.Display.HdrCapabilities.HDR_TYPE_HDR10||it==android.view.Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS
        }==true
        // Zero lets the HDR surface use Android’s normal brightness policy.
        val desired=if(available&&hdr)0f else 1f
        if(android.os.Build.VERSION.SDK_INT>=35&&requestedHeadroom!=desired){view.setDesiredHdrHeadroom(desired);requestedHeadroom=desired}
        if(available!=displayAvailable){displayAvailable=available;host.displayInfo(available)}
    }
    fun pause(){paused=true;lifecycle++;if(flag!=0L)Native.captureCancel(flag);loop?.cancel();loop=null}
    fun resume(){if(!paused)return;paused=false
        loop=host.viewModelScope.launch {
            while(isActive&&!paused){
                try{
                    val state=JSONObject(host.withNative{Native.toneStatus(it)})
                    val hdr=state.optBoolean("hdr_output")
                    updateDisplay(hdr)
                    val next=state.getInt("generation")
                    if(generation!=next){
                        generation=next;changed=android.os.SystemClock.elapsedRealtime()
                        if(flag!=0L)Native.captureCancel(flag)
                        host.documentChanged()
                    }
                    status=when {
                        !state.getBoolean("hdr")->""
                        hdr->"HDR"
                        !state.isNull("error")->"SDR preview unavailable"
                        !state.getBoolean("retained")->"Preparing SDR…"
                        state.optString("proof_mode")=="sdr"->"SDR preview"
                        state.optString("proof_mode")=="print"->"Print proof"
                        else->"Showing SDR"
                    }
                    details=when {
                        hdr->"The canvas and Navigator show HDR artwork. Choose SDR in Proof to preview SDR output."
                        !state.isNull("error")->"SDR preview unavailable: ${state.getString("error")}"
                        state.optString("proof_mode")=="sdr"->"Showing the saved SDR appearance. Choose Off in Proof to view HDR when available."
                        state.optString("proof_mode")=="print"->"Showing the SDR print preview."
                        !state.optBoolean("display_hdr")->"Showing SDR on this display. The HDR artwork is preserved."
                        else->"Showing the saved SDR appearance."
                    }
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
