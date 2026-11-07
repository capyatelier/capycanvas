package art.capycanvas

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.json.JSONObject

@Composable internal fun SourceEditDialog(host: CanvasHost, request: JSONObject) {
    val id=request.getInt("id")
    val rasterize=request.getJSONObject("kind").getJSONObject("request").getString("type")=="rasterize_source"
    val job=remember(id) {DocumentColorJob(host,id,source=true)}
    var space by remember {mutableStateOf("Srgb")}
    var custom by remember {mutableStateOf<JSONObject?>(null)}
    var loaded by remember(id) { mutableStateOf(false) }
    LaunchedEffect(id) {
        try {
            if (!rasterize) space = JSONObject(host.withNative { Native.query(it, obj("type" to "document_color").toString()) }).getString("space")
            loaded = true
        } catch (e: Exception) { job.error = e }
    }
    val choice = if (rasterize) null else if (space == "custom") custom?.getJSONObject("profile") else obj("Builtin" to space)
    LaunchedEffect(loaded, rasterize, choice?.toString()) { if (loaded) job.prepare(choice) }
    DisposableEffect(job) {onDispose {job.close()}}
    val copy=host.catalog.getJSONObject("document_color_copy")
    LaunchedEffect(host.languageTag) { job.refreshCopy() }
    val common=copy.getJSONObject("common")
    val label=copy.getString(if(rasterize)"rasterize" else if(job.addsLayer)"add_source" else "apply_profile")
    DialogShell(host,copy.getJSONObject(if(job.applying)"progress_dialog" else "dialog"),copy.getString(if(rasterize)"rasterize_title" else "repair_title"),
        onCancel=job::close,onPrimary=job::apply,primaryLabel=label,primaryEnabled=job.ready&&!job.busy,tag="source-edit",
        responses=listOf(DialogResponse(common.getString("cancel"),"source-edit-cancel",action=job::close))+
            if(job.applying)emptyList() else listOf(DialogResponse(label,"source-edit-apply",accent=true,enabled=job.ready&&!job.busy,action=job::apply)),
        body={padding -> Column(Modifier.fillMaxWidth().padding(padding),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Text(copy.getString(if(rasterize)"rasterize_help" else "repair_help"))
            if(!rasterize) {
                    ColorChoice(copy.getString("correct_profile"),listOf("Srgb" to "sRGB","DisplayP3" to "Display P3","AdobeRgb" to "Adobe RGB (1998)","ProPhoto" to "ProPhoto RGB")+(custom?.let{listOf("custom" to profileCaption(host,it))}?:emptyList()),space,enabled=!job.applying){if(!job.applying){space=it;job.invalidate()}}
                    ImportProfileButton(host,enabled=!job.applying) {if(!job.applying){custom=it;space="custom";job.invalidate()}}
            }
            if(job.busy){CircularProgressIndicator();Text(copy.getString(if(rasterize)"rasterize_comparison" else "preparing_comparison"))}
            if(job.sourceProfile.isNotEmpty()){Text(copy.getString("current_source"));Text(job.sourceProfile)}
            if(job.addsLayer)Text(copy.getString("adds_layer"))
            if(job.clipped>0)Text(copy.getString("source_clipped"))
            job.previews.forEachIndexed {i,image->Text(copy.getString(if(i==0)"before" else "after"));Image(image,copy.getString(if(i==0)"original_composition" else "prepared_composition"),Modifier.fillMaxWidth().heightIn(max=180.dp))}
            ColorFailureText(host, job.error)
        }})
}
