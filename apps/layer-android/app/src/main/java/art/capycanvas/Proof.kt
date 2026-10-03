package art.capycanvas

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

/** One shared draft and worker for all dock/drawer views, as in GTK. */
internal class ProofController(private val host: CanvasHost) {
    var status by mutableStateOf(""); private set
    var error by mutableStateOf<Exception?>(null); private set
    var busy by mutableStateOf(false); private set
    var committing by mutableStateOf(false); private set
    var form by mutableStateOf<JSONObject?>(null); private set
    var settings by mutableStateOf<JSONObject?>(null); private set
    var texture by mutableStateOf<ImageBitmap?>(null); private set
    private var textureJob:Job?=null
    private val lane=Mutex()
    private var control=0L
    private var running:Job?=null
    private var debounce:Job?=null
    private var generation=-1L
    private var serial=0L
    private var identity:String?=null
    private var saved:String?=null
    private var dirty=false
    private var paused=false
    private var observing=false
    private var observedGeneration=-1L
    private var copyGeneration=0L
    fun ensureTexture() {
        if(textureJob!=null)return
        textureJob=host.viewModelScope.launch {
            texture=withContext(Dispatchers.Default){Bitmap.createBitmap(Native.proofTexture(512),512,512,Bitmap.Config.ARGB_8888).asImageBitmap()}
        }
    }
    fun cancel() {
        if(committing)return
        serial++;debounce?.cancel();debounce=null
        if(control!=0L)Native.captureCancel(control)
    }
    fun pause() { paused=true;cancel() }
    suspend fun pauseAndDrain() { finishPending(); pause(); running?.join() }
    fun resume() { paused=false;sync() }
    fun action(value:JSONObject) { host.viewModelScope.launch {
        try { host.withNative{Native.proofControl(it,value.toString())};host.documentChanged();sync() }
        catch(e:Exception){error=e}
    } }
    fun edit(key:String,value:Any) {
        if(committing)return
        val next=JSONObject(settings!!.toString()).put(key,value)
        settings=next;dirty=true;error=null;cancel()
        debounce=host.viewModelScope.launch {delay(180);debounce=null;prepareDraft()}
    }
    private fun prepareDraft() {
        if(paused||!dirty||form?.optString("mode")!="print")return
        val draft=settings?:return
        if(draft.objectOrNull("profile")==null)return
        val recipe=try{JSONObject(Native.colorUi(obj("type" to "print_proof","settings" to draft).toString(), host.languageTag))}
            catch(e:Exception){error=e;return}
        if(recipe.toString()==form?.objectOrNull("document_profile")?.toString()){dirty=false;return}
        start(-1,recipe)
    }
    fun hasPending() = debounce?.isActive==true || (dirty&&running?.isActive==true)
    suspend fun finishPending() {
        debounce?.join();running?.join()
        if(form?.optString("mode")=="print")error?.let{throw it}
    }
    fun sync() {
        if(observing||paused)return
        observing=true
        host.viewModelScope.launch {
            try {
                val (model,view,key)=host.withNative { h ->
                    val model=JSONObject(Native.query(h,obj("type" to "proof_form").toString()));val view=JSONObject(Native.query(h,obj("type" to "proof_status").toString()))
                    Triple(model,view,model.getJSONArray("identity").toString())
                }
                observedGeneration=view.getLong("generation")
                val proof=model.objectOrNull("document_profile")?.toString()
                val changed=identity!=key
                if(changed||saved!=proof) {
                    cancel();dirty=false;settings=model.getJSONObject("print_settings");error=null
                    identity=key;saved=proof
                }
                val oldMode=form?.optString("mode");form=model;status=view.getString("text")
                if(model.getString("mode")!="print") {if(oldMode=="print")cancel();return@launch}
                if(dirty) {if(running?.isActive!=true&&debounce==null)prepareDraft();return@launch}
                val next=view.getLong("generation")
                if(next!=generation){if(running?.isActive==true)cancel();generation=next}
                if(view.getBoolean("needed")&&running?.isActive!=true)start(0,null)
            } catch(e:Exception){error=e}
            finally {observing=false;refreshCopy()}
        }
    }
    fun refreshCopy() {
        if(paused)return
        val ticket=++copyGeneration
        val language=host.languageTag
        host.viewModelScope.launch {
            try {
                val (copy,view)=host.withNative { h ->
                    JSONObject(Native.query(h,obj("type" to "proof_copy").toString())) to JSONObject(Native.query(h,obj("type" to "proof_status").toString()))
                }
                currentCoroutineContext().ensureActive()
                if(ticket!=copyGeneration||language!=host.languageTag||paused)return@launch
                val retained=form
                if(retained==null||copy.getJSONArray("identity").toString()!=identity||view.getLong("generation")!=observedGeneration||copy.getString("mode")!=retained.getString("mode")) {
                    sync();return@launch
                }
                val next=retained.shallowCopy()
                copy.keys().forEach { key -> next.put(key,copy.get(key)) }
                form=next;status=view.getString("text")
            } catch(e:Exception){if(ticket==copyGeneration&&language==host.languageTag)error=e}
        }
    }
    private fun start(id:Int,recipe:JSONObject?) {
        val ticket=serial
        running=host.viewModelScope.launch {
            lane.withLock {
                if(ticket!=serial||paused)return@withLock
                var task=0L;var flag=0L
                busy=true;error=null
                try {
                    flag=Native.captureControl();control=flag
                    task=host.withNative{Native.proofTask(it,id,recipe?.toString()?:"null",flag)}
                    withContext(Dispatchers.Default){Native.proofWork(task)}
                    if(ticket!=serial||paused)return@withLock
                    host.withNative{Native.proofCheck(it,task)}
                    committing=true
                    val bytes=Native.proofPreservation(task)
                    if(bytes!=null)ProfileStore.import(host.getApplication<Application>(),bytes)
                    host.withNative{Native.proofApply(it,task,bytes!=null)}
                    dirty=false;host.documentChanged()
                } catch(e:Exception) {
                    if(ticket==serial&&!paused){
                        error=e
                        if(id==0&&task!=0L)runCatching{host.withNative{ h ->
                            if(e is ColorFeatureFailure)Native.proofFailedReason(h,task,org.json.JSONArray(listOf(e.reason)).toString().let{it.substring(1,it.length-1)}) else Native.proofFailed(h,task,e.message.orEmpty())
                        }}
                        if(id<0){dirty=false;settings=form?.getJSONObject("print_settings")}
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
    LaunchedEffect(state.optLong("revision"),host.languageTag){host.proof.refreshCopy()}
    val request=state.array("requests").objects().firstOrNull{it.getJSONObject("kind").getString("type")in listOf("soft_proof_setup","sdr_rendition")}
    if(request!=null)LaunchedEffect(request.getInt("id")) {
        try {
            if(request.getJSONObject("kind").getString("type")=="sdr_rendition")host.withNative{Native.proofControl(it,obj("type" to "mode","mode" to "sdr").toString())}
            host.withNative{Native.proofControl(it,obj("type" to "reveal").toString())}
            host.dispatch(obj("type" to "complete_request","id" to request.getInt("id")))
        }catch(e:Exception){host.dispatch(obj("type" to "complete_request","id" to request.getInt("id"),"error" to e.message))}
    }
}

@Composable internal fun ProofPanel(host:CanvasHost,modifier:Modifier=Modifier,scrollable:Boolean=true,onHeight:(Float)->Unit={}) {
    val density=LocalDensity.current.density
    val controller=host.proof
    val colors=LocalPalette.current
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    val model=controller.form
    val copy=host.catalog.getJSONObject("proof_copy")
    val profiles=host.catalog.getJSONObject("profile_copy")
    val settings=controller.settings
    val mode=model?.optString("mode")?:"off"
    var library by remember{mutableStateOf(false)}
    var picker by remember{mutableStateOf(false)}
    var saved by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var rawSaved by remember{mutableStateOf<List<JSONObject>>(emptyList())}
    var localError by remember{mutableStateOf<Exception?>(null)}
    LaunchedEffect(picker){if(picker)try{rawSaved=ProfileStore.list(context)}catch(e:Exception){localError=e}}
    LaunchedEffect(rawSaved,host.languageTag) {
        val language=host.languageTag
        val retained=rawSaved
        val projected=profileEntriesCopy(host,retained)
        if(language==host.languageTag && retained===rawSaved)saved=projected
    }
    LaunchedEffect(mode){if(mode=="sdr")controller.ensureTexture()}
    BoxWithConstraints(modifier) {
        val dialSide=minOf(maxWidth-16.dp,maxHeight-52.dp).coerceAtLeast(128.dp)
        Column(Modifier.fillMaxWidth().then(if(scrollable&&constraints.hasBoundedHeight)Modifier.verticalScroll(rememberScrollState())else Modifier)
            .onSizeChanged{onHeight(it.height/density)}.padding(horizontal=8.dp,vertical=6.dp),verticalArrangement=Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(2.dp)) {
                (listOf("off" to copy.getString("mode_off"))+(if(model?.optBoolean("hdr")==true)listOf("sdr" to "SDR")else emptyList())+listOf("print" to copy.getString("mode_print"))).forEach{(value,label)->
                    val selected=mode==value
                    Box(Modifier.weight(1f).heightIn(min=34.dp).clip(ControlShape)
                        .background(if(selected)colors.text.copy(alpha=.12f)else Color.Transparent)
                        .selectable(selected,enabled=model!=null&&!controller.committing,role=Role.RadioButton){controller.cancel();controller.action(obj("type" to "mode","mode" to value))}
                        .testTag("proof-mode-$value"),contentAlignment=Alignment.Center){Text(label,color=colors.text)}
                }
            }
            if(mode=="sdr"&&model!=null)Box(Modifier.width(dialSide).align(Alignment.CenterHorizontally)){ProofSdrControls(host,model,controller.texture,controller::action)}
            if(mode=="print"&&model!=null&&settings!=null) {
                for(control in model.getJSONArray("print_controls").objects()) {
                    val label=control.getString("label")
                    when(control.getString("id")) {
                        "profile"->ProofOptionRow(label){PanelChoiceButton(settings.objectOrNull("profile")?.let{profileCaption(host,it)}?:profiles.getString("choose"),Modifier.testTag("proof-profile"),!controller.committing){picker=true}}
                        "simulation"->ProofChoice(label,model.getJSONArray("simulations").objects().map{it.getString("value") to it.getString("label")},settings.getString("simulation"),!controller.committing){controller.edit("simulation",it)}
                        "intent"->ProofChoice(label,model.getJSONArray("intents").objects().map{it.getString("value") to it.getString("label")},settings.getString("intent"),!controller.committing){controller.edit("intent",it)}
                        "black_point_compensation"->ProofCheck(label,settings.getBoolean("bpc")&&settings.getString("intent")!="AbsoluteColorimetric",!controller.committing&&settings.getString("intent")!="AbsoluteColorimetric"){controller.edit("bpc",it)}
                        "gamut_warning"->ProofCheck(label,host.snapshot?.objectOrNull("state")?.optBoolean("gamut_warning")==true,model.objectOrNull("document_profile")!=null){host.invoke("gamut_warning")}
                    }
                }
                if(controller.busy)CircularProgressIndicator(Modifier.size(20.dp).align(Alignment.CenterHorizontally))
            }
            if (localError != null) ColorFailureText(host,localError,true) else ColorFailureText(host,controller.error,proof=true)
        }
    }
    fun select(profile:JSONObject){controller.edit("profile",profile);picker=false;localError=null}
    if(picker)AlertDialog(onDismissRequest={picker=false},title={Text(copy.getString("profile_choice"))},confirmButton={TextButton({picker=false}){Text(copy.getJSONObject("common").getString("done"))}},text={
        Column(Modifier.fillMaxWidth().heightIn(max=520.dp).verticalScroll(rememberScrollState()).testTag("proof-profile-picker")){
            val original=model?.objectOrNull("print_settings")?.objectOrNull("profile")
            if(original!=null){Text(profiles.getString("document"));TextButton({select(original)}){Text(profileCaption(host,original))}}
            Text(profiles.getString("saved"))
            saved.filter{!it.has("issue")&&it.optBoolean("visible",true)}.forEach{entry->TextButton({scope.launch{try{select(ProfileStore.get(context,entry.getString("id")))}catch(e:Exception){localError=e}}}){Text(entry.getString("name"))}}
            Text(profiles.getString("standard"))
            model?.getJSONArray("profiles")?.objects()?.forEach{p->TextButton({select(p)}){Text(p.getString("name"))}}
            ProfileFileButton(host,host.catalog.getJSONObject("profile_copy").getString("add_profile_dialog")){select(it);scope.launch{rawSaved=ProfileStore.list(context)}}
            TextButton({picker=false;library=true}){Text(profiles.getString("manage"))}
        }
    })
    if(library)ProfileLibraryDialog(host,{library=false;scope.launch{rawSaved=ProfileStore.list(context)}})
}

@Composable private fun ProofOptionRow(label:String,content:@Composable ()->Unit) {
    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        Text(label,Modifier.width(72.dp),style=MaterialTheme.typography.bodyMedium)
        Box(Modifier.weight(1f)){content()}
    }
}
@Composable private fun ProofChoice(label:String,choices:List<Pair<String,String>>,value:String,enabled:Boolean,onChange:(String)->Unit) {
    var open by remember{mutableStateOf(false)}
    ProofOptionRow(label){Box {
        PanelChoiceButton(choices.firstOrNull{it.first==value}?.second?:value,Modifier.testTag("color-choice-$label"),enabled){open=true}
        DropdownMenu(open&&enabled,{open=false}){choices.forEach{(id,title)->DropdownMenuItem(text={Text(title)},onClick={open=false;onChange(id)})}}
    }}
}

@Composable private fun ProofCheck(label:String,checked:Boolean,enabled:Boolean,onChange:(Boolean)->Unit) {
    Row(Modifier.fillMaxWidth().toggleable(checked,enabled=enabled,role=Role.Checkbox,onValueChange=onChange),
        verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        EditorCheck(checked,label,Modifier.clearAndSetSemantics{},enabled,onChange)
        Text(label,color=LocalPalette.current.text.copy(alpha=if(enabled)1f else .5f))
    }
}
