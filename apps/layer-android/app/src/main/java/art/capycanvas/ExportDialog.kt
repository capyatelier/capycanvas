package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.Image
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/** Delivery edits an explicit copy; the shared recipe validates every choice. */
@Composable internal fun ExportDialog(host: CanvasHost, onDismiss: () -> Unit, onChoose: (JSONObject, Int) -> Unit) {
    var form by remember { mutableStateOf<JSONObject?>(null) }
    var recipe by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var destination by remember { mutableStateOf("0") }
    var fit by remember { mutableStateOf(false) }
    var width by remember { mutableStateOf("2048") }
    var height by remember { mutableStateOf("2048") }
    var resolution by remember { mutableStateOf("Master") }
    var ppi by remember { mutableStateOf("300") }
    var quality by remember { mutableStateOf("90") }
    val context=LocalContext.current
    var documentColor by remember {mutableStateOf<JSONObject?>(null)}
    var presetNames by remember {mutableStateOf<List<String>>(emptyList())}
    var presetName by remember {mutableStateOf("")}
    var preferenceBusy by remember {mutableStateOf(false)}
    var enlarge by remember {mutableStateOf(false)}
    val scope = rememberCoroutineScope()
    val preview = remember { OutputPreview(host) }
    DisposableEffect(preview) { onDispose { preview.close() } }
    LaunchedEffect(recipe?.toString(),fit,width,height,resolution,ppi,quality) { preview.invalidate() }
    fun selectedRecipe():JSONObject = JSONObject(recipe!!.toString()).apply {
        put("jpeg_quality",quality.toIntOrNull() ?: error("Enter a JPEG quality from 1 to 100"))
        put("size",if(fit)obj("Fit" to obj("bounds" to JSONArray(listOf(width.toIntOrNull(),height.toIntOrNull())),"enlarge" to enlarge))else "Original")
        put("resolution",if(resolution=="Ppi")obj("Ppi" to (ppi.toIntOrNull() ?: error("Enter a resolution")))else resolution)
    }
    fun dismiss() = preview.close(onDismiss)
    suspend fun preference(action:JSONObject) {
        preferenceBusy=true
        try {
            val result=ColorPreferencesStore.presets(context,documentColor!!,action)
            presetNames=result.getJSONArray("names").values().map{it as String}
            if(!result.isNull("index"))destination=result.getInt("index").toString()
            result.objectOrNull("recipe")?.let {selected->
                val model=JSONObject(form!!.toString())
                if(model.getJSONArray("profiles").objects().none{it.getJSONObject("profile").toString()==selected.getJSONObject("profile").getJSONObject("profile").toString()})model.getJSONArray("profiles").put(selected.getJSONObject("profile"))
                form=model;recipe=selected;quality=selected.getInt("jpeg_quality").toString()
                val bounds=selected.optJSONObject("size")?.optJSONObject("Fit");fit=bounds!=null;enlarge=bounds?.optBoolean("enlarge")?:false
                bounds?.getJSONArray("bounds")?.let{width=it.getInt(0).toString();height=it.getInt(1).toString()}
                val dpi=selected.optJSONObject("resolution")?.optInt("Ppi")
                resolution=if(dpi!=null)"Ppi" else selected.getString("resolution");if(dpi!=null)ppi=dpi.toString()
            }
            error=null
        }catch(e:Exception){error=e.message ?: "Could not save export preferences"}
        finally{preferenceBusy=false}
    }
    LaunchedEffect(Unit) {
        try {
            form=JSONObject(host.withNative{Native.query(it,obj("type" to "export_form").toString())})
            documentColor=JSONObject(host.withNative{Native.query(it,obj("type" to "document_color").toString())})
            preference(obj("type" to "get","index" to 0))
        }catch(e:Exception){error=e.message}
    }
    fun change(key: String, value: Any) { recipe = JSONObject(recipe!!.toString()).put(key, value) }
    AlertDialog(onDismissRequest = ::dismiss, title = { Text("Export image") },
        dismissButton = { TextButton(::dismiss) { Text("Cancel") } },
        confirmButton = { TextButton(enabled = recipe != null && !preview.busy && !preferenceBusy, modifier = Modifier.testTag("export-choose-file"), onClick = {
            scope.launch {
                try {
                    val selected = selectedRecipe()
                    host.withNative { Native.query(it, obj("type" to "export_validate", "recipe" to selected).toString()) }
                    onChoose(selected,destination.toInt())
                } catch (e: Exception) { error = e.message ?: "Invalid export choices" }
            }
        }) { Text("Choose File…") } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 580.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Export a profiled copy. The editable drawing stays unchanged.")
                val value = recipe; val model = form
                if (value != null && model != null && !preview.busy && !preferenceBusy) {
                    ColorChoice("Destination",presetNames.mapIndexed{i,name->i.toString() to name},destination){scope.launch{preference(obj("type" to "get","index" to it.toInt()))}}
                    ColorChoice("Format", listOf("Png" to "PNG", "Tiff" to "TIFF", "Jpeg" to "JPEG"), value.getString("format")) {
                        recipe = JSONObject(value.toString()).put("format", it).apply {
                            if (it == "Jpeg") { put("depth", "U8"); if (getString("background") == "Preserve") put("background", "White") }
                        }
                    }
                    val profiles = model.getJSONArray("profiles").objects()
                    val profile = profiles.indexOfFirst { it.toString() == value.getJSONObject("profile").toString() }.coerceAtLeast(0)
                    ColorChoice("Output profile", profiles.mapIndexed { i, p -> i.toString() to p.getString("name") }, profile.toString()) { change("profile", profiles[it.toInt()]) }
                    ImportProfileButton { imported ->
                        form = JSONObject(model.toString()).apply { getJSONArray("profiles").put(imported) }
                        change("profile", imported)
                    }
                    ColorChoice("Bit depth", listOf("U8" to "8-bit", "U16" to "16-bit"), value.getString("depth")) { change("depth", it) }
                    ColorChoice("Transparency", listOf("Preserve" to "Preserve", "White" to "White background", "Black" to "Black background"), value.getString("background")) { change("background", it) }
                    val encoding = value.getJSONObject("encoding")
                    val conversion = encoding.getJSONObject("conversion")
                    ColorChoice("Rendering intent", listOf("RelativeColorimetric" to "Relative colorimetric", "Perceptual" to "Perceptual", "Saturation" to "Saturation", "AbsoluteColorimetric" to "Absolute colorimetric"), conversion.getString("intent")) {
                        change("encoding", JSONObject(encoding.toString()).put("conversion", JSONObject(conversion.toString()).put("intent", it)))
                    }
                    ColorChoice("Dither", listOf("None" to "None", "Stochastic8" to "Stochastic (8-bit output)"), encoding.getString("dither")) { change("encoding", JSONObject(encoding.toString()).put("dither", it)) }
                    if (value.getString("format") == "Jpeg") OutlinedTextField(quality, { quality = it }, label = { Text("JPEG quality (1–100)") }, singleLine = true)
                    Row { Checkbox(fit, { fit = it }); Text("Fit within pixel size") }
                    if (fit) { OutlinedTextField(width, { width = it }, label = { Text("Maximum width") }, singleLine = true); OutlinedTextField(height, { height = it }, label = { Text("Maximum height") }, singleLine = true) }
                    ColorChoice("Resolution metadata", listOf("Master" to "Keep original", "Ppi" to "Pixels per inch", "Omit" to "Omit"), resolution) { resolution = it }
                    if (resolution == "Ppi") OutlinedTextField(ppi, { ppi = it }, label = { Text("Pixels per inch") }, singleLine = true)
                    OutlinedTextField(presetName,{presetName=it.take(80)},label={Text("Preset name")},singleLine=true)
                    fun store(type:String)=scope.launch{try{preference(obj("type" to type,"index" to destination.toInt(),"name" to presetName,"recipe" to selectedRecipe()).apply{if(type!="save")remove("name");if(type=="save")remove("index");if(type=="remove"||type=="reset")remove("recipe")})}catch(e:Exception){error=e.message}}
                    Row {TextButton({store("save")}){Text("Save Preset")};TextButton({store("update")},enabled=destination.toInt()>=4){Text("Update Preset")}}
                    Row {TextButton({store("remove")},enabled=destination.toInt()>=4){Text("Delete Preset")};TextButton({store("reset")},enabled=destination.toInt()<4){Text("Reset Destination")}}
                    TextButton({try{preview.prepare(selectedRecipe())}catch(e:Exception){error=e.message}}){Text("Preview Output")}
                } else if (error == null) CircularProgressIndicator()
                if(preview.busy)Text("Preparing complete output comparison…")
                preview.images.forEachIndexed {index,image->Text(if(index==0)"Artwork" else "Output");Image(image,if(index==0)"Artwork preview" else "Output preview",Modifier.fillMaxWidth().heightIn(max=180.dp))}
                if(preview.images.isNotEmpty())Text("sRGB display preview · includes output size, profile, depth, transparency and dither; excludes JPEG compression artifacts.")
                if(preview.clipped>0)Text("Some colors exceed the output gamut and will be clipped.")
                preview.error?.let{Text(it,color=MaterialTheme.colorScheme.error)}
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}
