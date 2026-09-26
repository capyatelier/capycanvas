package art.capycanvas

import androidx.compose.runtime.*
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject

/** Poll the native display status; the render owner schedules HDR analysis. */
internal class HdrController(private val host:CanvasHost) {
    var status by mutableStateOf("")
    var details by mutableStateOf("")
    private var paused=true
    private var loop:Job?=null
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
    fun pause(){paused=true;loop?.cancel();loop=null}
    fun resume(){if(!paused)return;paused=false
        loop=host.viewModelScope.launch {
            while(isActive&&!paused){
                try{
                    val state=JSONObject(host.withNative{Native.toneStatus(it)})
                    val hdr=state.optBoolean("hdr_output")
                    updateDisplay(hdr)
                    if(state.getBoolean("changed"))host.documentChanged()
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
                }catch(e:CancellationException){throw e}catch(e:Exception){status="Display status unavailable";details=e.message?:"Could not read the display status"}
                delay(200)
            }
        }
    }
}
