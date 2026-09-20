package art.capycanvas

import androidx.compose.runtime.*
import androidx.compose.ui.graphics.ImageBitmap
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import org.json.JSONObject

/** One immutable full-resolution inspection; dismissal drains it before reuse. */
internal class OutputPreview(private val host:CanvasHost) {
    var busy by mutableStateOf(false)
    var images by mutableStateOf<List<ImageBitmap>>(emptyList())
    var sdr by mutableStateOf<ImageBitmap?>(null)
    var error by mutableStateOf<String?>(null)
    var clipped by mutableStateOf(0L)
    private var control=0L
    private var closed=false
    private var after:(()->Unit)?=null
    fun invalidate(){images=emptyList();sdr=null;error=null;clipped=0}
    fun close(done:(()->Unit)?=null){closed=true;after=done;if(control!=0L)Native.captureCancel(control);if(!busy){after?.invoke();after=null}}
    fun prepare(recipe:JSONObject) {
        if(busy||closed)return
        busy=true;invalidate();control=Native.captureControl()
        host.viewModelScope.launch {
            try {
                val result=withContext(NonCancellable) {
                    val task=host.withNative{Native.inspectionTask(it,control)}
                    withContext(Dispatchers.IO){val values=Native.inspectionOutput(task,recipe.toString());
                        JSONObject(values[0] as String) to values.drop(1).map{comparisonBitmap(it as ByteArray)}}
                }
                if(!closed){clipped=result.first.optLong("clipped_channels");images=result.second.take(2);sdr=result.second.getOrNull(2)}
            }catch(e:Exception){if(!closed)error=e.message ?: "Could not preview output"}
            finally{val flag=control;control=0;Native.captureFree(flag);busy=false;after?.invoke();after=null}
        }
    }
}
