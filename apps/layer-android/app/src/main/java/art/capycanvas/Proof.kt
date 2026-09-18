package art.capycanvas

import android.app.Application
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

/** One preparation at a time, independent of ink and file workers. Cancellation
 * is atomic; the task is freed only after its native worker has returned. */
internal class ProofController(private val host: CanvasHost) {
    var status by mutableStateOf("")
    var error by mutableStateOf<String?>(null)
    var busy by mutableStateOf(false)
    var committing by mutableStateOf(false)
    private val lane=Mutex()
    private var control=0L
    private var running:Job?=null
    private var generation=-1L
    private var serial=0L
    private var setup=false
    private var paused=false
    private var observing=false
    fun cancel() { if(committing)return;serial++;if(control!=0L)Native.captureCancel(control) }
    fun pause() { paused=true;cancel() }
    fun resume() { paused=false;sync() }
    fun open() { setup=true;cancel();error=null }
    fun close(id:Int) {
        if(committing||!setup)return
        cancel();setup=false
        host.dispatch(obj("type" to "complete_request","id" to id))
        sync()
    }
    fun sync() {
        if(observing||paused)return
        observing=true
        host.viewModelScope.launch {
            try {
                val view=JSONObject(host.withNative{Native.proofStatus(it)})
                status=view.getString("text")
                if(!setup){
                    val next=view.getLong("generation")
                    if(next!=generation || !view.getBoolean("needed"))cancel()
                    generation=next
                    if(view.getBoolean("needed") && running?.isActive!=true)start(0,null)
                }
            } finally {observing=false}
        }
    }
    fun apply(id:Int,recipe:JSONObject) { if(!committing&&!busy){cancel();start(id,recipe)} }
    private fun start(id:Int,recipe:JSONObject?) {
        val ticket=serial
        running=host.viewModelScope.launch {
            lane.withLock {
                if(ticket!=serial||paused)return@withLock
                var task=0L
                var flag=0L
                busy=true;error=null
                try {
                    flag=Native.captureControl();control=flag
                    task=host.withNative{Native.proofTask(it,id,recipe?.toString()?:"null",flag)}
                    withContext(Dispatchers.Default){Native.proofWork(task)}
                    if(ticket!=serial||paused)return@withLock
                    host.withNative{Native.proofCheck(it,task)}
                    // Once durable preservation starts this Apply is committing;
                    // dismissal cannot claim cancellation after copying the ICC.
                    committing=true
                    val bytes=Native.proofPreservation(task)
                    if(bytes!=null)ProfileStore.import(host.getApplication<Application>(),bytes)
                    host.withNative{Native.proofApply(it,task,bytes!=null)}
                    if(id!=0)setup=false
                    host.documentChanged()
                } catch(e:Exception) {
                    if(ticket==serial&&!paused){
                        error=e.message?:"Could not prepare proof"
                        if(id==0&&task!=0L)runCatching{host.withNative{Native.proofFailed(it,task,error!!)}}
                    }
                } finally {
                    control=0
                    withContext(NonCancellable+Dispatchers.Default){if(task!=0L)Native.proofRelease(task);if(flag!=0L)Native.captureFree(flag)}
                    busy=false;committing=false
                }
            }
        }.also{job->job.invokeOnCompletion{host.viewModelScope.launch{if(running===job)running=null;sync()}}}
    }
}

@Composable internal fun ProofRequests(host:CanvasHost,state:JSONObject) {
    LaunchedEffect(state.optLong("revision")){host.proof.sync()}
    val request=state.array("requests").objects().firstOrNull{it.getJSONObject("kind").getString("type")=="soft_proof_setup"}
    if(request!=null)key(request.getInt("id")){ProofDialog(host,request.getInt("id"))}
}

@Composable private fun ProofDialog(host:CanvasHost,id:Int) {
    val controller=host.proof
    val context=LocalContext.current
    var form by remember{mutableStateOf<JSONObject?>(null)}
    var profiles by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var saved by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var selection by remember{mutableIntStateOf(0)}
    var intent by remember{mutableStateOf("RelativeColorimetric")}
    var bpc by remember{mutableStateOf(true)}
    var simulation by remember{mutableStateOf("1")}
    var library by remember{mutableStateOf(false)}
    var picker by remember{mutableStateOf(false)}
    var localError by remember{mutableStateOf<String?>(null)}
    LaunchedEffect(id){
        controller.open()
        try{
            val model=JSONObject(host.withNative{Native.proofForm(it)})
            val recipe=model.getJSONObject("recipe")
            val original=model.objectOrNull("document_profile")
            profiles=listOfNotNull(original)+model.getJSONArray("profiles").objects()
            selection=if(original!=null)0 else profiles.indexOfFirst{it.getJSONObject("profile").toString()==recipe.getJSONObject("profile").toString()}.coerceAtLeast(0)
            intent=recipe.getJSONObject("conversion").getString("intent");bpc=recipe.getJSONObject("conversion").getBoolean("black_point_compensation")
            simulation=if(recipe.getBoolean("simulate_paper"))"2" else if(recipe.getBoolean("simulate_black_ink"))"1" else "0"
            saved=ProfileStore.list(context)
            form=model
        }catch(e:Exception){localError=e.message}
    }
    DisposableEffect(id){onDispose{if(!controller.committing)controller.close(id)}}
    AlertDialog(onDismissRequest={controller.close(id)},title={Text("Proof Setup")},
        dismissButton={TextButton({controller.close(id)},enabled=!controller.committing){Text("Cancel")}},
        confirmButton={TextButton({
            val profile=profiles[selection]
            controller.apply(id,obj("name" to profile.getString("name"),"profile" to profile.getJSONObject("profile"),
                "conversion" to obj("intent" to intent,"black_point_compensation" to bpc),"simulate_paper" to (simulation=="2"),"simulate_black_ink" to (simulation!="0")))
        },enabled=form!=null&&!controller.busy){Text("Apply")}},text={
            Column(Modifier.fillMaxWidth().heightIn(max=600.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(8.dp)){
                Text("Preview how colors will look in print.")
                if(form!=null&&!controller.busy){
                    Text("Proof profile",style=MaterialTheme.typography.labelMedium)
                    TextButton({picker=true},Modifier.testTag("proof-profile")){Text(profiles[selection].getString("name"))}
                    ColorChoice("Rendering intent",listOf("RelativeColorimetric" to "Relative colorimetric","Perceptual" to "Perceptual","Saturation" to "Saturation","AbsoluteColorimetric" to "Absolute colorimetric"),intent){intent=it;if(it=="AbsoluteColorimetric")bpc=false}
                    Row(verticalAlignment=androidx.compose.ui.Alignment.CenterVertically){Checkbox(bpc,{bpc=it},enabled=intent!="AbsoluteColorimetric");Text("Black point compensation")}
                    ColorChoice("Print simulation",listOf("0" to "Colors only","1" to "Black ink","2" to "Paper and ink"),simulation){simulation=it}
                }
                if(controller.busy){CircularProgressIndicator();Text(if(controller.committing)"Applying proof…" else "Preparing preview…")}
                (localError?:controller.error)?.let{Text(it,color=MaterialTheme.colorScheme.error)}
            }
        })
    val scope=rememberCoroutineScope()
    fun select(p:JSONObject){profiles=profiles+p;selection=profiles.lastIndex;picker=false}
    if(picker)AlertDialog(onDismissRequest={picker=false},title={Text("Proof profile")},confirmButton={TextButton({picker=false}){Text("Done")}},text={
        Column(Modifier.fillMaxWidth().heightIn(max=520.dp).verticalScroll(rememberScrollState()).testTag("proof-profile-picker")){
            val original=form?.objectOrNull("document_profile")
            if(original!=null){Text("Document Profile");TextButton({selection=0;picker=false}){Text(original.getString("name"))}}
            Text("Saved Profiles")
            saved.filter{!it.has("issue")&&it.optBoolean("visible",true)}.forEach{entry->TextButton({scope.launch{try{select(ProfileStore.get(context,entry.getString("id")))}catch(e:Exception){localError=e.message}}}){Text(entry.getString("name"))}}
            Text("Standard Color Spaces")
            form?.getJSONArray("profiles")?.objects()?.forEach{p->TextButton({select(p)}){Text(p.getString("name"))}}
            ProfileFileButton("Add Profile…"){select(it);scope.launch{saved=ProfileStore.list(context)}}
            TextButton({picker=false;library=true}){Text("Manage Profiles…")}
        }
    })
    if(library)ProfileLibraryDialog({library=false;scope.launch{saved=ProfileStore.list(context)}})
}
