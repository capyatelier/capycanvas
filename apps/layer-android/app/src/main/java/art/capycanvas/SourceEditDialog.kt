package art.capycanvas

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
    DisposableEffect(job) {onDispose {job.close()}}
    val copy=host.catalog.getJSONObject("document_color_copy")
    LaunchedEffect(host.languageTag) { job.refreshCopy() }
    val common=copy.getJSONObject("common")
    AlertDialog(onDismissRequest=job::close,title={Text(copy.getString(if(rasterize)"rasterize_title" else "repair_title"))},
        dismissButton={TextButton(job::close){Text(common.getString("cancel"))}},
        confirmButton={TextButton(job::apply,enabled=job.ready&&!job.busy){Text(copy.getString(if(rasterize)"rasterize" else if(job.addsLayer)"add_source" else "apply_profile"))}},
        text={Column(Modifier.fillMaxWidth().heightIn(max=600.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Text(copy.getString(if(rasterize)"rasterize_help" else "repair_help"))
            if(!job.busy) {
                if(!rasterize) {
                    ColorChoice(copy.getString("correct_profile"),listOf("Srgb" to "sRGB","DisplayP3" to "Display P3","AdobeRgb" to "Adobe RGB (1998)","ProPhoto" to "ProPhoto RGB")+(custom?.let{listOf("custom" to profileCaption(host,it))}?:emptyList()),space){space=it;job.invalidate()}
                    ImportProfileButton(host) {custom=it;space="custom";job.invalidate()}
                }
                TextButton({job.prepare(if(rasterize)null else if(space=="custom")custom!!.getJSONObject("profile") else obj("Builtin" to space))}){Text(copy.getString("preview"))}
            }
            if(job.busy){CircularProgressIndicator();Text(copy.getString(if(rasterize)"rasterize_comparison" else "preparing_comparison"))}
            if(job.sourceProfile.isNotEmpty()){Text(copy.getString("current_source"));Text(job.sourceProfile)}
            if(job.addsLayer)Text(copy.getString("adds_layer"))
            if(job.clipped>0)Text(copy.getString("source_clipped"))
            job.previews.forEachIndexed {i,image->Text(copy.getString(if(i==0)"before" else "after"));Image(image,copy.getString(if(i==0)"original_composition" else "prepared_composition"),Modifier.fillMaxWidth().heightIn(max=180.dp))}
            ColorFailureText(host, job.error)
        }})
}
