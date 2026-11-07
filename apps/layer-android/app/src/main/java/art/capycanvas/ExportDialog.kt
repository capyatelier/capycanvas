package art.capycanvas

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

@Composable internal fun ExportDialog(host:CanvasHost,id:Int,visible:Boolean,onDismiss:()->Unit,onChoose:(JSONObject,JSONObject)->Unit) {
    val context=LocalContext.current
    val scope=rememberCoroutineScope()
    var view by remember {mutableStateOf<JSONObject?>(null)}
    var profiles by remember {mutableStateOf<List<JSONObject>>(emptyList())}
    var error by remember {mutableStateOf<Exception?>(null)}
    val preview=remember {OutputPreview(host)}
    val commits=remember {linkedMapOf<Any,(Boolean)->Boolean>()}
    fun commit()=commits.values.toList().all{it(false)}
    DisposableEffect(preview){onDispose{preview.close()}}
    suspend fun query(type:String,vararg fields:Pair<String,Any?>):Any = JSONTokener(host.withNative {
        Native.query(it,obj("type" to type,"id" to id,*fields).toString())
    }).nextValue()
    suspend fun update(action:JSONObject) {view=query("export_dialog_action","action" to action) as JSONObject}
    fun send(action:JSONObject) {scope.launch {try{update(action);error=null}catch(e:Exception){error=e}}}
    fun edit(type:String,value:Any) = send(obj("type" to "edit","action" to obj("type" to type,"value" to value)))
    fun close()=preview.close(onDismiss)
    fun save(operation:String) {scope.launch {
        try {
            val candidate=query("export_dialog_preset_write","operation" to operation) as JSONObject
            ColorPreferencesStore.replace(context,candidate)
            update(obj("type" to "preset_result","result" to obj("Ok" to candidate.getJSONObject("result"))))
        } catch(e:Exception) {
            val reason=if(e is ColorFeatureFailure)e.reason else obj("Diagnostic" to (e.message ?: host.bootstrap!!.getString("action_failed")))
            update(obj("type" to "preset_result","result" to obj("Err" to reason)))
        }
    }}
    fun choose() {if(!commit())return;scope.launch {
        try {
            val recipe=query("export_dialog_recipe") as JSONObject
            val remember=query("export_dialog_remember_write") as JSONObject
            preview.close {onChoose(recipe,remember)}
        }catch(e:Exception){error=e}
    }}
    LaunchedEffect(id) {
        try {
            val bytes=ColorPreferencesStore.read(context)
            view=query("begin_export_dialog","presets" to JSONArray(bytes.map {it.toInt() and 255}),
                "capabilities" to obj("formats" to JSONArray(listOf("Png","Tiff","Jpeg","Webp","Exr","PngHdr","JpegHdr","AvifHdr")),"profile_import" to true)) as JSONObject
        }catch(e:Exception){error=e}
    }
    LaunchedEffect(host.languageTag){if(view != null && visible)view=query("export_dialog_view") as JSONObject}
    LaunchedEffect(view?.optLong("profile_revision")){if(view != null && visible)profiles=(query("export_dialog_profiles") as JSONArray).objects()}
    LaunchedEffect(view?.optLong("preview_revision")) {
        if(view != null && visible)try {preview.prepare(query("export_dialog_recipe") as JSONObject)}catch(e:Exception){preview.invalidate()}
    }
    LaunchedEffect(preview.ready) {
        if(preview.ready && visible)try {update(obj("type" to "preview","value" to obj("ready" to true,"transparent" to preview.transparent,"range_exceeded" to preview.rangeBlocked)))}catch(e:Exception){error=e}
    }
    val model=view
    if(model == null) {
        ColorFailureText(host,error)
        return
    }
    val copy=model.getJSONObject("copy")
    val page=model.getString("page")
    val fields=model.getJSONObject("fields")
    val title=when(page){"size"->copy.getString("image_size");"color"->model.getString("color_title");"presets"->copy.getString("presets");"save_preset"->copy.getString("save_preset_title");else->copy.getString("title")}
    fun navigate(page:String){if(commit())send(obj("type" to "navigate","page" to page))}
    fun back(){if(commit())send(obj("type" to if(page=="save_preset")"preset_cancel" else "back"))}
    var conversion by remember {mutableStateOf(false)}
    var profileLibrary by remember {mutableStateOf(false)}
    val confirm={if(page=="save_preset")save("save") else choose()}
    DialogShell(host,if(page=="save_preset")copy.getJSONObject("preset_dialog") else model.getJSONObject("dialog"),title,
        subpage=page!="root",onCancel={if(page=="save_preset")back() else close()},onBack=::back,header=page=="save_preset",tag="export-dialog",onPrimary=confirm,shown=visible,
        primaryEnabled=if(page=="save_preset")model.getBoolean("name_valid") else model.getBoolean("can_export"),
        primaryLabel=copy.getString(if(page=="save_preset")"save_preset" else "choose_file"),
        responses=if(page!="save_preset")emptyList() else listOf(
            DialogResponse(copy.getJSONObject("common").getString("cancel"),"export-preset-cancel",action=::back),
            DialogResponse(copy.getString("save_preset"),"export-preset-save",accent=true,enabled=model.getBoolean("name_valid"),action=confirm)),
        navigation=if(page=="save_preset")null else ({
                val header=remember(model.getJSONObject("dialog").toString(),page) {JSONObject(Native.dialogUi(obj("kind" to "header","intent" to model.getJSONObject("dialog"),"subpage" to (page!="root")).toString()))}
                DialogHeader(host,header,title,"export-dialog",::close,::back,confirm,model.getBoolean("can_export"),copy.getString("choose_file"),"export-choose-file","export-back")
                HorizontalDivider(color=LocalPalette.current.dialogSeparator)
        }),
        secondary=if(page!="root")null else ({TextButton({scope.launch{try{preview.prepare(query("export_dialog_recipe") as JSONObject)}catch(e:Exception){error=e}}},Modifier.testTag("export-preview")){Text(copy.getString("preview"))}}),
        body={padding ->
            Column(Modifier.fillMaxWidth()) {
                Column(Modifier.fillMaxWidth().padding(padding),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                    @Composable fun choices(label:String,key:String,value:Any,enabled:Boolean=true,action:(Any)->Unit) {
                        val values=model.array(key).objects()
                        ExportChoiceRow(label,"color-choice-$label",values.map{it.getString("label")},
                            values.indexOfFirst{it.get("value").toString()==value.toString()},enabled){action(values[it].get("value"))}
                    }
                    @Composable fun toggle(label:String,selected:Boolean,tag:String,subtitle:String="",change:(Boolean)->Unit) {
                        SettingRow(label,tag,subtitle,onClick={change(!selected)}) {Switch(selected,change)}
                    }
                    @Composable fun number(label:String,value:Int,tag:String,spec:String="dimension",change:(Int)->Unit) {
                        NumericSetting(label,value.toDouble(),model.getJSONObject("numeric").getJSONObject(spec),Modifier.fillMaxWidth().testTag(tag),
                            settings=true,id=tag,registerCommit={key,finish -> if(finish==null)commits.remove(key) else commits[key]=finish}) {change(it.toInt())}
                    }
                    when(page) {
                        "root" -> {
                            SettingsCard(rows=buildList {
                                if(model.getBoolean("show_output"))add {choices(copy.getString("output"),"outputs",model.get("output")){send(obj("type" to "output","value" to it))}}
                                if(model.getBoolean("show_conversion"))add {choices(copy.getString("format"),"formats",fields.get("format")){edit("format",it)}}
                                if(model.getBoolean("show_flatten"))add {toggle(copy.getString("flatten"),model.getBoolean("flatten"),"export-flatten"){send(obj("type" to "flatten","enabled" to it))}}
                                if(model.getBoolean("show_quality"))add {number(copy.getString("quality"),fields.getInt("jpeg_quality"),"export-quality","quality"){edit("quality",it)}}
                                if(model.getBoolean("show_metadata"))add {
                                    val metadata=model.getJSONObject("metadata")
                                    val options=metadata.array("choices").objects()
                                    ExportChoiceRow(metadata.getString("label"),"export-metadata",options.map{it.getString("label")},options.indexOfFirst{it.getString("value")==fields.getJSONObject("metadata").getString("keep")}) {send(obj("type" to "metadata_keep","value" to options[it].getString("value")))}
                                }
                                if(model.getBoolean("show_location"))add {toggle(model.getJSONObject("metadata").getString("remove_location"),fields.getJSONObject("metadata").getBoolean("remove_location"),"export-remove-location"){send(obj("type" to "remove_location","enabled" to it))}}
                            })
                            if(model.getBoolean("show_metadata_note"))Text(model.getJSONObject("metadata").getString("note"),Modifier.testTag("export-metadata-note"))
                            if(preview.busy)Text(copy.getString("preparing_comparison"))
                            if(preview.images.isNotEmpty())BoxWithConstraints(Modifier.fillMaxWidth()) {
                                val split=Native.dialogUi(obj("kind" to "panes_fit","width" to maxWidth.value,"minimum" to JSONArray(listOf(160,160)),"gap" to 12).toString()).toBoolean()
                                @Composable fun picture(index:Int,modifier:Modifier) {
                                    val label=model.getJSONArray("comparison_labels").getString(index)
                                    Column(modifier,horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(6.dp)) {
                                        Text(label)
                                        Image(if(index==1 && model.getBoolean("preview_sdr"))preview.sdr ?: preview.images[index] else preview.images[index],label,Modifier.fillMaxWidth().height(120.dp))
                                    }
                                }
                                if(split)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)) {preview.images.indices.forEach{picture(it,Modifier.weight(1f))}}
                                else Column(verticalArrangement=Arrangement.spacedBy(12.dp)){preview.images.indices.forEach{picture(it,Modifier.fillMaxWidth())}}
                            }
                            if(model.getBoolean("show_rendition"))choices(copy.getString("preview_rendition"),"renditions",model.get("preview_sdr")){send(obj("type" to "rendition","sdr" to it))}
                            if(model.getBoolean("compression_note"))Text(copy.getString("compression_note"))
                            ColorFailureText(host,preview.error)
                            SettingsCard(rows=buildList {
                                add {SettingRow(copy.getString("size"),"export-size",model.getString("size_subtitle"),onClick={navigate("size")}) {Chevron()}}
                                if(model.getBoolean("show_color"))add {SettingRow(model.getString("color_title"),"export-color",model.getString("color_subtitle"),onClick={navigate("color")}) {Chevron()}}
                                add {SettingRow(copy.getString("preset"),"export-presets",model.getJSONArray("preset_names").getString(model.getInt("preset")),onClick={navigate("presets")}) {Chevron()}}
                            })
                            if(model.getBoolean("show_clip"))toggle(copy.getString("clip_hdr"),model.getBoolean("clip"),"export-clip"){edit("clip_hdr_range",it)}

                        }
                        "size" -> {
                            SettingsCard(rows=buildList {
                                add {ExportChoiceRow(copy.getString("size"),"export-fit",listOf(copy.getString("original_size"),copy.getString("fit_bounds")),if(model.getBoolean("show_size"))1 else 0){send(obj("type" to "fit","enabled" to (it==1)))}}
                                if(model.getBoolean("show_size")) {
                                    add {number(copy.getString("maximum_width"),model.getJSONArray("bounds").getInt(0),"export-width"){send(obj("type" to "bound","axis" to 0,"value" to it))}}
                                    add {number(copy.getString("maximum_height"),model.getJSONArray("bounds").getInt(1),"export-height"){send(obj("type" to "bound","axis" to 1,"value" to it))}}
                                    add {toggle(copy.getString("enlarge"),model.getBoolean("enlarge"),"export-enlarge"){send(obj("type" to "enlarge","enabled" to it))}}
                                }
                                add {choices(copy.getString("resolution"),"resolutions",fields.get("resolution")){edit("resolution",it)}}
                                if(model.getBoolean("show_ppi"))add {number(copy.getString("ppi"),model.getInt("ppi"),"export-ppi","ppi"){send(obj("type" to "ppi","value" to it))}}
                            })
                            Text(model.getString("size_summary"))
                        }
                        "color" -> {
                            SettingsCard(rows=buildList {
                                if(model.getBoolean("show_conversion"))add {
                                    ExportChoiceRow(copy.getString("profile"),"color-choice-"+copy.getString("profile"),profiles.map{profileCaption(host,it)},fields.getInt("profile"),extra=if(!model.getBoolean("profile_import"))null else ({closeMenu ->
                                        ProfileFileButton(host){send(obj("type" to "profile","result" to obj("Ok" to it)));closeMenu()}
                                        TextButton({closeMenu();profileLibrary=true}){Text(host.catalog.getJSONObject("profile_copy").getString("saved_dialog"))}
                                    })) {send(obj("type" to "profile","result" to obj("Ok" to profiles[it])))}
                                }
                                if(model.getBoolean("show_depth"))add {choices(copy.getString("depth"),"depths",fields.get("depth"),model.getBoolean("depth_enabled")){edit("depth",it)}}
                                if(model.getBoolean("show_background"))add {choices(copy.getString("background"),"backgrounds",fields.get("background")){edit("background",it)}}
                                if(model.getBoolean("show_print_profile"))add {SettingRow(copy.getString("use_print_profile"),"export-print-profile",model.nullableString("print_profile_name") ?: "",onClick=if(!model.getBoolean("profile_ready"))null else ({scope.launch {
                                    try {
                                        val profile=query("export_dialog_print_profile") as JSONArray
                                        val channels=query("profile_channels","profile" to profile.get(0))
                                        update(obj("type" to "intent","value" to profile.get(2)))
                                        update(obj("type" to "profile","result" to obj("Ok" to obj("profile" to profile.get(0),"name" to profile.get(1),"channels" to channels))))
                                    }catch(e:Exception){error=e}
                                };Unit})) {Chevron()}}
                                if(model.getBoolean("show_dither"))add {toggle(copy.getString("reduce_banding"),model.getBoolean("dither"),"export-dither",copy.getString("dither_gradients")){send(obj("type" to "dither","enabled" to it))}}
                            })
                            model.nullableString("precision_note")?.let {Text(it)}
                            if(model.getBoolean("show_conversion"))SettingsCard(rows=buildList {
                                add {SettingRow(copy.getString("conversion"),"export-advanced",onClick={conversion=!conversion}){SharedIcon("chevron-down",null,Modifier.size(16.dp).rotate(if(conversion)180f else 0f))}}
                                if(conversion)add {choices(copy.getString("intent"),"intents",fields.getJSONObject("encoding").getJSONObject("conversion").get("intent")){send(obj("type" to "intent","value" to it))}}
                            })
                        }
                        "presets" -> {
                            val names=model.array("preset_names").values().map{it.toString()}
                            SettingsCard(rows=listOf {ExportChoiceRow(copy.getString("preset"),"export-preset",names,model.getInt("preset"),!model.getBoolean("preset_busy"),available=model.array("preset_enabled").values().map{it as Boolean}){send(obj("type" to "preset","index" to it))}})
                            SettingsCard(rows=buildList {
                                listOf("save","update","remove","reset").forEachIndexed {index,operation ->
                                    if(model.getBoolean("preset_$operation"))add {
                                        SettingRow(model.getJSONArray("preset_labels").getString(index),"export-preset-$operation",
                                            titleColor=if(operation=="remove")LocalPalette.current.error else LocalPalette.current.text,enabled=!model.getBoolean("preset_busy"),onClick={scope.launch {
                                                try {update(obj("type" to "begin_preset","operation" to operation));if(operation!="save")save(operation)}catch(e:Exception){error=e}
                                            }})
                                    }
                                }
                            })
                            model.nullableString("preset_status")?.let{Text(it,color=if(model.getBoolean("preset_error"))MaterialTheme.colorScheme.error else LocalPalette.current.settingsSecondary)}
                        }
                        "save_preset" -> CoreTextField(model.getString("name"),{send(obj("type" to "name","value" to it))},Modifier.fillMaxWidth().testTag("export-preset-name"),label={Text(copy.getString("preset_name"))},background=LocalPalette.current.dialogField,focusRequest=1)
                    }
                    model.nullableString("error")?.let{Text(it,color=MaterialTheme.colorScheme.error)}
                    ColorFailureText(host,error)
                }
            }
        })
    if(profileLibrary)ProfileLibraryDialog(host,{profileLibrary=false}) {send(obj("type" to "profile","result" to obj("Ok" to it)));profileLibrary=false}
}

@Composable private fun ExportChoiceRow(label:String,tag:String,choices:List<String>,selected:Int,enabled:Boolean=true,available:List<Boolean> = emptyList(),extra:(@Composable (() -> Unit)->Unit)?=null,onChange:(Int)->Unit) {
    var open by remember {mutableStateOf(false)}
    Box {
        SettingRow(label,tag,enabled=enabled,onClick={open=true}) {
            Text(choices.getOrNull(selected) ?: "",Modifier.weight(1f,fill=false),maxLines=1,overflow=androidx.compose.ui.text.style.TextOverflow.Ellipsis,color=LocalPalette.current.settingsSecondary)
            SharedIcon("chevron-down",null,Modifier.size(16.dp))
        }
        DropdownMenu(open && enabled,{open=false}) {
            choices.forEachIndexed {index,value -> DropdownMenuItem(text={Text(value)},enabled=available.getOrElse(index){true},onClick={open=false;onChange(index)},
                trailingIcon={Box(Modifier.size(20.dp)){if(index==selected)SharedIcon("check",null,Modifier.fillMaxSize())}})}
            extra?.invoke {open=false}
        }
    }
}
