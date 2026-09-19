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
                if(!setup || running?.isActive!=true){
                    val next=view.getLong("generation")
                    if(next!=generation || !view.getBoolean("needed"))cancel()
                    generation=next
                    if(view.getBoolean("needed") && running?.isActive!=true)start(0,null)
                }
            } finally {observing=false}
        }
    }
    fun apply(id:Int,recipe:JSONObject) { if(!committing){cancel();start(id,recipe)} }
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
                    if(id>0)setup=false
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
    val request=state.array("requests").objects().firstOrNull{it.getJSONObject("kind").getString("type")in listOf("soft_proof_setup","sdr_rendition")}
    if(request!=null)key(request.getInt("id")){ProofDialog(host,request.getInt("id"),request.getJSONObject("kind").getString("type")=="sdr_rendition")}
}

@Composable private fun ProofDialog(host:CanvasHost,id:Int,sdr:Boolean) {
    val controller=host.proof
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    var mode by remember { mutableStateOf("off") }
    var formGeneration by remember { mutableIntStateOf(0) }
    var form by remember{mutableStateOf<JSONObject?>(null)}
    var profiles by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var saved by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var selection by remember{mutableIntStateOf(0)}
    var intent by remember{mutableStateOf("RelativeColorimetric")}
    var bpc by remember{mutableStateOf(true)}
    var simulation by remember{mutableStateOf("1")}
    var retry by remember{mutableIntStateOf(0)}
    var library by remember{mutableStateOf(false)}
    var picker by remember{mutableStateOf(false)}
    var localError by remember{mutableStateOf<String?>(null)}
    LaunchedEffect(id){
        controller.open()
        try{
            if(sdr)host.withNative{Native.proofControl(it,obj("type" to "mode","mode" to "sdr").toString())}
            val model=JSONObject(host.withNative{Native.proofForm(it)})
            val recipe=model.getJSONObject("recipe")
            val original=model.objectOrNull("document_profile")
            profiles=listOfNotNull(original)+model.getJSONArray("profiles").objects()
            selection=if(original!=null)0 else profiles.indexOfFirst{it.getJSONObject("profile").toString()==recipe.getJSONObject("profile").toString()}.coerceAtLeast(0)
            intent=recipe.getJSONObject("conversion").getString("intent");bpc=recipe.getJSONObject("conversion").getBoolean("black_point_compensation")
            simulation=if(recipe.getBoolean("simulate_paper"))"2" else if(recipe.getBoolean("simulate_black_ink"))"1" else "0"
            saved=ProfileStore.list(context)
            form=model;mode=model.getString("mode")
        }catch(e:Exception){localError=e.message}
    }
    DisposableEffect(id){onDispose{if(!controller.committing)controller.close(id)}}
    fun action(value:JSONObject) { scope.launch { try { host.withNative{Native.proofControl(it,value.toString())};host.documentChanged();formGeneration++ } catch(e:Exception){localError=e.message} } }
    LaunchedEffect(formGeneration,
        host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("revision"),
        host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch"),
        host.snapshot?.objectOrNull("state")?.optBoolean("soft_proof"),
        host.snapshot?.objectOrNull("state")?.optBoolean("preview_sdr")) {
        if(form!=null) { val updated=JSONObject(host.withNative{Native.proofForm(it)});form=updated;mode=updated.getString("mode") }
    }
    LaunchedEffect(mode,selection,intent,bpc,simulation,form!=null,retry) {
        if(form!=null && mode=="print") {
            controller.cancel();delay(180)
            val p=profiles[selection]
            controller.apply(-1,obj("name" to p.getString("name"),"profile" to p.getJSONObject("profile"),
                "conversion" to obj("intent" to intent,"black_point_compensation" to (bpc&&intent!="AbsoluteColorimetric")),"simulate_paper" to (simulation=="2"),"simulate_black_ink" to (simulation!="0")))
        }
    }
    androidx.compose.ui.window.Popup(alignment=androidx.compose.ui.Alignment.TopEnd,properties=androidx.compose.ui.window.PopupProperties(focusable=false)) {
        Surface(Modifier.padding(top=64.dp,end=12.dp).width(320.dp),shape=MaterialTheme.shapes.large,shadowElevation=8.dp) {
            Column(Modifier.padding(16.dp).heightIn(max=600.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(8.dp)) {
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween){Text("Proof",style=MaterialTheme.typography.titleLarge);TextButton({controller.close(id)},enabled=!controller.committing){Text("Close")}}
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(4.dp)) {
                    (listOf("off" to "Off")+(if(form?.optBoolean("hdr")==true)listOf("sdr" to "SDR")else emptyList())+listOf("print" to "Print")).forEach{(value,label)->
                        FilterChip(selected=mode==value,onClick={controller.cancel();mode=value;action(obj("type" to "mode","mode" to value))},label={Text(label)},enabled=form!=null&&!controller.committing,modifier=Modifier.weight(1f).testTag("proof-mode-$value"))
                    }
                }
                if(mode=="sdr") form?.let{ProofSdrControls(it,::action)}
                if(mode=="print") {
                if(form!=null){
                    Text("Proof profile",style=MaterialTheme.typography.labelMedium)
                    TextButton({picker=true},Modifier.testTag("proof-profile"),enabled=!controller.committing){Text(profiles[selection].getString("name"))}
                    ColorChoice("Rendering intent",listOf("RelativeColorimetric" to "Relative colorimetric","Perceptual" to "Perceptual","Saturation" to "Saturation","AbsoluteColorimetric" to "Absolute colorimetric"),intent,enabled=!controller.committing){intent=it;if(it=="AbsoluteColorimetric")bpc=false}
                    Row(verticalAlignment=androidx.compose.ui.Alignment.CenterVertically){Checkbox(bpc,{bpc=it},enabled=!controller.committing&&intent!="AbsoluteColorimetric");Text("Black point compensation")}
                    ColorChoice("Print simulation",listOf("0" to "Colors only","1" to "Black ink","2" to "Paper and ink"),simulation,enabled=!controller.committing){simulation=it}
                }

                    Row { Checkbox(host.panelContent?.objectOrNull("state")?.optBoolean("gamut_warning")==true,{host.invoke("gamut_warning")},enabled=form?.objectOrNull("document_profile")!=null);Text("Gamut warning") }
                    if(controller.busy){CircularProgressIndicator();Text(if(controller.committing)"Applying proof…" else "Preparing preview…");TextButton({controller.cancel()},enabled=!controller.committing){Text("Cancel preparation")}}
                }
                (localError?:controller.error)?.let{Text(it,color=MaterialTheme.colorScheme.error);if(mode=="print")TextButton({retry++}){Text("Retry")}}
            }
        }
    }
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
