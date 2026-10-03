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
    var draft by remember { mutableStateOf<JSONObject?>(null) }
    var recipe by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<Exception?>(null) }
    var destination by remember { mutableStateOf("0") }
    var fit by remember { mutableStateOf(false) }
    var width by remember { mutableStateOf("2048") }
    var height by remember { mutableStateOf("2048") }
    var resolution by remember { mutableStateOf("Master") }
    var ppi by remember { mutableStateOf("300") }
    var quality by remember { mutableStateOf("90") }
    val context=LocalContext.current
    var profileNames by remember {mutableStateOf<List<String>>(emptyList())}
    var profileCaptions by remember {mutableStateOf<JSONArray?>(null)}
    var documentColor by remember {mutableStateOf<JSONObject?>(null)}
    var presetNames by remember {mutableStateOf<List<String>>(emptyList())}
    var rawPresetNames by remember {mutableStateOf<List<String>>(emptyList())}
    var presetName by remember {mutableStateOf("")}
    var preferenceBusy by remember {mutableStateOf(false)}
    var enlarge by remember {mutableStateOf(false)}
    var previewMode by remember {mutableStateOf("hdr")}
    val scope = rememberCoroutineScope()
    val preview = remember { OutputPreview(host) }
    DisposableEffect(preview) { onDispose { preview.close() } }
    LaunchedEffect(recipe,fit,width,height,enlarge,resolution,ppi,quality) { preview.invalidate() }
    fun selectedRecipe():JSONObject = recipe!!.shallowCopy().apply {
        put("jpeg_quality",quality.toIntOrNull() ?: 0)
        put("size",if(fit)obj("Fit" to obj("bounds" to JSONArray(listOf(width.toIntOrNull() ?: 0,height.toIntOrNull() ?: 0)),"enlarge" to enlarge))else "Original")
        put("resolution",if(resolution=="Ppi")obj("Ppi" to (ppi.toIntOrNull() ?: 0))else resolution)
    }
    fun dismiss() = preview.close(onDismiss)
    suspend fun preference(action:JSONObject) {
        preferenceBusy=true
        try {
            val result=ColorPreferencesStore.presets(context,documentColor!!,action)
            rawPresetNames=result.getJSONArray("names").values().map{it as String}
            if(!result.isNull("index"))destination=result.getInt("index").toString()
            result.objectOrNull("recipe")?.let {selected->
                val model=form!!.shallowCopy()
                if(model.getJSONArray("profiles").objects().none{it.getJSONObject("profile").toString()==selected.getJSONObject("profile").getJSONObject("profile").toString()})model.getJSONArray("profiles").put(selected.getJSONObject("profile"))
                form=model
                draft=JSONObject(host.withNative{Native.query(it,obj("type" to "export_draft","recipe" to selected,"action" to obj("type" to "refresh")).toString())})
                recipe=draft!!.getJSONObject("recipe");quality=selected.getInt("jpeg_quality").toString()
                val bounds=selected.optJSONObject("size")?.optJSONObject("Fit");fit=bounds!=null;enlarge=bounds?.optBoolean("enlarge")?:false
                bounds?.getJSONArray("bounds")?.let{width=it.getInt(0).toString();height=it.getInt(1).toString()}
                val dpi=selected.optJSONObject("resolution")?.optInt("Ppi")
                resolution=if(dpi!=null)"Ppi" else selected.getString("resolution");if(dpi!=null)ppi=dpi.toString()
            }
            error=null
        }catch(e:Exception){error=e}
        finally{preferenceBusy=false}
    }
    LaunchedEffect(Unit) {
        try {
            form = JSONObject(host.withNative { Native.query(it, obj("type" to "export_form").toString()) })
            profileNames=form!!.array("profile_names").values().map { it as String }
            profileCaptions=form!!.getJSONArray("profile_captions")
            documentColor = JSONObject(host.withNative { Native.query(it, obj("type" to "document_color").toString()) })
            preference(obj("type" to "get", "index" to 0))
        } catch (e: Exception) { error = e }
    }
    LaunchedEffect(profileCaptions,host.languageTag) {
        val retained=profileCaptions ?: return@LaunchedEffect
        val language=host.languageTag
        val projected=JSONArray(host.withNative { Native.query(it,obj("type" to "export_profile_captions_copy","captions" to retained).toString()) }).values().map { it as String }
        if(language==host.languageTag && retained===profileCaptions)profileNames=projected
    }
    LaunchedEffect(rawPresetNames, host.languageTag) {
        val language=host.languageTag
        val retained=rawPresetNames
        val projected=presetNamesCopy(host,retained)
        if(language==host.languageTag && retained===rawPresetNames)presetNames=projected
    }
    LaunchedEffect(recipe?.optString("format"),recipe?.objectOrNull("metadata")?.optString("keep"),host.languageTag) {
        val current=recipe ?: return@LaunchedEffect
        val language=host.languageTag
        val format=current.getString("format")
        val keep=current.getJSONObject("metadata").getString("keep")
        val projected=JSONObject(host.withNative { Native.query(it,obj("type" to "export_metadata_copy","format" to format,"keep" to keep).toString()) })
        if(language==host.languageTag && format==recipe?.optString("format") && keep==recipe?.getJSONObject("metadata")?.getString("keep")) {
            draft?.let { retained -> draft=JSONObject().apply { retained.keys().forEach { key -> put(key,retained.get(key)) };put("metadata",projected) } }
        }
    }
    fun change(key: String, value: Any) {
        if(preferenceBusy)return
        preferenceBusy=true
        scope.launch {
            try {
                val result=JSONObject(host.withNative{Native.query(it,obj("type" to "export_draft","recipe" to recipe!!,"action" to obj("type" to key,"value" to value)).toString())})
                draft=result;recipe=result.getJSONObject("recipe");error=null
            }catch(e:Exception){error=e}finally{preferenceBusy=false}
        }
    }
    fun choices(key:String, labels:List<Pair<String,String>>) = labels.filter { pair -> draft?.getJSONArray(key)?.values()?.contains(pair.first) != false }
    val copy = host.catalog.getJSONObject("export_copy")
    AlertDialog(onDismissRequest = ::dismiss, title = { Text(copy.getString("title")) },
        dismissButton = { TextButton(::dismiss) { Text(copy.getJSONObject("common").getString("cancel")) } },
        confirmButton = { TextButton(enabled = recipe != null && !preview.busy && !preferenceBusy && !preview.rangeBlocked, modifier = Modifier.testTag("export-choose-file"), onClick = {
            scope.launch {
                try {
                    val selected = selectedRecipe()
                    val refusal=org.json.JSONTokener(host.withNative { Native.query(it, obj("type" to "export_validation", "recipe" to selected).toString()) }).nextValue()
                    if(refusal !== JSONObject.NULL)throw ColorFeatureFailure(refusal)
                    onChoose(selected,destination.toInt())
                } catch (e: Exception) { error = e }
            }
        }) { Text(copy.getString("choose_file")) } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 580.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(copy.getString("help"))
                val value = recipe; val model = form
                if (value != null && model != null && !preview.busy && !preferenceBusy) {
                    ColorChoice(copy.getString("destination"),presetNames.mapIndexed{i,name->i.toString() to name},destination){scope.launch{preference(obj("type" to "get","index" to it.toInt()))}}
                    val hdrOutput=value.getString("format").contains("Hdr")||value.getString("format")=="Exr"
                    if(documentColor?.optString("depth") in listOf("F16","F32")) {
                        val format=value.getString("format")
                        val range=when {format.startsWith("JpegHdr")->"jpeg";format.startsWith("AvifHdr")->"avif";format.startsWith("PngHdr")->"hdr";format=="Exr"->"exr";else->"sdr"}
                        ColorChoice(copy.getString("range"),listOf("sdr" to copy.getString("sdr_rendition"),"jpeg" to copy.getString("jpeg_gainmap"),"avif" to copy.getString("avif_gainmap"),"hdr" to copy.getString("format_pq"),"exr" to copy.getString("format_exr")),range) {
                            change("format",when(it){"jpeg"->"JpegHdr";"avif"->"AvifHdr";"hdr"->"PngHdr";"exr"->"Exr";else->"Png"})
                        }
                        if(format.contains("Hdr")) {
                            Row { Checkbox(format.endsWith("Mapped"),{change("format",format.removeSuffix("Mapped")+if(it)"Mapped" else "")});Text(copy.getString("clip_hdr")) }
                        }
                    }
                    if(!hdrOutput) {
                    ColorChoice(copy.getString("format"), choices("formats",listOf("Png" to copy.getString("format_png"), "Tiff" to copy.getString("format_tiff"), "Jpeg" to copy.getString("format_jpeg"), "Webp" to copy.getString("format_webp"))), value.getString("format")) { change("format",it) }
                    val profiles = model.getJSONArray("profiles").objects()
                    val profile = remember(model.getJSONArray("profiles"), profiles.size, value.getJSONObject("profile").getJSONObject("profile")) { profiles.indexOfFirst { it.getJSONObject("profile").toString() == value.getJSONObject("profile").getJSONObject("profile").toString() }.coerceAtLeast(0) }
                    ColorChoice(copy.getString("profile"), profiles.mapIndexed { i, p -> i.toString() to if(i<profileNames.size)profileNames[i]else profileCaption(host,p) }, profile.toString()) { change("profile", profiles[it.toInt()]) }
                    ImportProfileButton(host) { imported ->
                        form = model.shallowCopy().apply { getJSONArray("profiles").put(imported) }
                        change("profile", imported)
                    }
                    ColorChoice(copy.getString("depth"), choices("depths",listOf("U8" to copy.getString("depth_8"), "U16" to copy.getString("depth_16"))), value.getString("depth")) { change("depth", it) }
                    }
                    if(!hdrOutput || value.getString("format").startsWith("JpegHdr")) {
                    ColorChoice(copy.getString("transparency"), choices("backgrounds",listOf("Preserve" to copy.getString("preserve"), "White" to copy.getString("white_background"), "Black" to copy.getString("black_background"))), value.getString("background")) { change("background", it) }
                    }
                    if(!hdrOutput) {
                    val encoding = value.getJSONObject("encoding")
                    val conversion = encoding.getJSONObject("conversion")
                    ColorChoice(copy.getString("intent"), listOf("RelativeColorimetric" to copy.getString("relative"), "Perceptual" to copy.getString("perceptual"), "Saturation" to copy.getString("saturation"), "AbsoluteColorimetric" to copy.getString("absolute")), conversion.getString("intent")) {
                        change("encoding", JSONObject(encoding.toString()).put("conversion", JSONObject(conversion.toString()).put("intent", it)))
                    }
                    ColorChoice(copy.getString("dither"), choices("dithers",listOf("None" to copy.getString("dither_none"), "Stochastic8" to copy.getString("dither_stochastic"))), encoding.getString("dither")) { change("encoding", JSONObject(encoding.toString()).put("dither", it)) }
                    }
                    if (value.getString("format").startsWith("Jpeg") || value.getString("format").startsWith("AvifHdr")) CoreTextField(quality, { quality = it }, modifier = Modifier.testTag("export-quality"), label = { Text(copy.getString("quality")) })
                    Row { Checkbox(fit, { fit = it }); Text(copy.getString("fit_bounds")) }
                    if (fit) { CoreTextField(width, { width = it }, modifier = Modifier.testTag("export-width"), label = { Text(copy.getString("maximum_width")) }); CoreTextField(height, { height = it }, modifier = Modifier.testTag("export-height"), label = { Text(copy.getString("maximum_height")) }) }
                    ColorChoice(copy.getString("resolution"), listOf("Master" to copy.getString("keep_resolution"), "Ppi" to copy.getString("ppi"), "Omit" to copy.getString("omit")), resolution) { resolution = it }
                    if (resolution == "Ppi") CoreTextField(ppi, { ppi = it }, modifier = Modifier.testTag("export-ppi"), label = { Text(copy.getString("ppi")) })
                    val metadata = draft?.optJSONObject("metadata")
                    if (model.optBoolean("metadata") && metadata != null) {
                        val kept = value.getJSONObject("metadata")
                        if (metadata.getBoolean("available")) {
                            ColorChoice(metadata.getString("label"), metadata.getJSONArray("choices").objects().map { it.getString("value") to it.getString("label") }, kept.getString("keep")) {
                                change("metadata", JSONObject(kept.toString()).put("keep", it))
                            }
                        }
                        if (metadata.getBoolean("location")) Row {
                            Checkbox(kept.getBoolean("remove_location"), { change("metadata", JSONObject(kept.toString()).put("remove_location", it)) }, Modifier.testTag("export-remove-location"))
                            Text(metadata.getString("remove_location"))
                        }
                        if (!metadata.isNull("note")) Text(metadata.getString("note"), Modifier.testTag("export-metadata-note"))
                    }
                    CoreTextField(presetName,{presetName=it},modifier=Modifier.testTag("export-preset-name"),maxLength=80,label={Text(copy.getString("preset_name"))})
                    fun store(type:String)=scope.launch{try{preference(obj("type" to type,"index" to destination.toInt(),"name" to presetName,"recipe" to selectedRecipe()).apply{if(type!="save")remove("name");if(type=="save")remove("index");if(type=="remove"||type=="reset")remove("recipe")})}catch(e:Exception){error=e}}
                    Row {TextButton({store("save")}){Text(copy.getString("save_preset"))};TextButton({store("update")},enabled=destination.toInt()>=4){Text(copy.getString("update_preset"))}}
                    Row {TextButton({store("remove")},enabled=destination.toInt()>=4){Text(copy.getString("delete_preset"))};TextButton({store("reset")},enabled=destination.toInt()<4){Text(copy.getString("reset_destination"))}}
                    TextButton({try{preview.prepare(selectedRecipe())}catch(e:Exception){error=e}}){Text(copy.getString("preview"))}
                } else if (error == null) CircularProgressIndicator()
                if(preview.busy)Text(copy.getString("preparing_comparison"))
                if(preview.sdr!=null)ColorChoice(copy.getString("preview_rendition"),listOf("hdr" to copy.getString("hdr_preview"),"sdr" to copy.getString("sdr_base")),previewMode){previewMode=it}
                preview.images.forEachIndexed {index,image->
                    val label=if(index==0)copy.getString("artwork") else if(preview.sdr!=null){if(previewMode=="sdr")copy.getString("sdr_base") else copy.getString("hdr_preview")}else copy.getString("output")
                    Text(label)
                    Image(if(index==1&&previewMode=="sdr")preview.sdr?:image else image,if(index==0)copy.getString("artwork_preview") else copy.getString("output_preview"),Modifier.fillMaxWidth().height(180.dp))
                }
                if(preview.images.isNotEmpty())Text(if(preview.sdr!=null)copy.getString("preview_gainmap") else if(recipe?.optString("format")=="Exr")copy.getString("preview_exr") else if(recipe?.optString("format")?.contains("Hdr")==true)copy.getString("preview_pq") else copy.getString("preview_sdr"))
                if(preview.clipped>0)Text(if(preview.rangeBlocked)copy.getString("outside_range") else copy.getString("outside_gamut"))
                ColorFailureText(host,preview.error)
                ColorFailureText(host,error)
            }
        })
}
