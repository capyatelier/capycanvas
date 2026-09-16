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
    AlertDialog(onDismissRequest=job::close,title={Text(if(rasterize)"Rasterize Retained Source" else "Repair Source Profile")},
        dismissButton={TextButton(job::close){Text(if(job.busy)"Cancel operation" else "Cancel")}},
        confirmButton={TextButton(job::apply,enabled=job.ready&&!job.busy){Text(if(rasterize)"Rasterize" else if(job.addsLayer)"Add Corrected Source" else "Apply Profile")}},
        text={Column(Modifier.fillMaxWidth().heightIn(max=600.dp).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Text(if(rasterize)"Convert the original to the document color space and bit depth at its full size. Existing paint, position, masks and adjustments stay intact. Undo restores the original profile and precision."
                else "Change how original image numbers are interpreted, keeping their samples and depth. A layer with pixel edits receives a separate corrected original at the same position; the existing edits remain intact.")
            if(!job.busy) {
                if(!rasterize) {
                    ColorChoice("Correct source profile",listOf("Srgb" to "sRGB","DisplayP3" to "Display P3","AdobeRgb" to "Adobe RGB (1998)","ProPhoto" to "ProPhoto RGB")+(custom?.let{listOf("custom" to it.getString("name"))}?:emptyList()),space){space=it;job.invalidate()}
                    ImportProfileButton {custom=it;space="custom";job.invalidate()}
                }
                TextButton({job.prepare(if(rasterize)null else if(space=="custom")custom!!.getJSONObject("profile") else obj("Builtin" to space))}){Text("Preview Complete Result")}
            }
            if(job.busy){CircularProgressIndicator();Text("Preparing complete source result…")}
            if(job.sourceProfile.isNotEmpty())Text("Current source profile: ${job.sourceProfile}")
            if(job.addsLayer)Text("Apply adds a corrected original as a new layer. The existing layer keeps its edits, masks and adjustments.")
            if(job.clipped>0)Text("Some source colors exceed the document gamut and will be clipped. Compare before applying.")
            job.previews.forEachIndexed {i,image->Text(if(i==0)"Before" else "After");Image(image,if(i==0)"Original composition" else "Prepared composition",Modifier.fillMaxWidth().heightIn(max=180.dp))}
            job.error?.let{Text(it,color=MaterialTheme.colorScheme.error)}
        }})
}
