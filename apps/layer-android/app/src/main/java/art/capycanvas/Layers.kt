package art.capycanvas

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.math.roundToInt

private fun CanvasHost.layer(action: JSONObject) = dispatch(obj("type" to "layer", "action" to action))
private suspend fun CanvasHost.layerQuery(value: JSONObject): JSONObject? = suspendCancellableCoroutine { continuation ->
    query(value) { if (continuation.isActive) continuation.resume(it as? JSONObject) }
}
private data class PreviewRequest(val id: Long, val key: String, val target: Long, val revision: Long)
private data class LayerDrag(val id: Long, val top: Float, val pointer: Offset, val target: Long? = null, val fraction: Float = 0f)
private fun iconName(name: String) = name.removePrefix("layer-").removeSuffix("-symbolic")

/** The native view translates the shared layer model; no layer policy lives here. */
@Composable internal fun LayerPanel(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier) {
    val colors = LocalPalette.current
    val density = LocalDensity.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val view = state.getJSONObject("layer_tools")
    val active = view.objectOrNull("editing_layer")
    val controls = view.getJSONObject("controls")
    val layers = state.array("layers").objects()
    val currentLayers by rememberUpdatedState(layers)
    val list = rememberLazyListState()
    val images = remember { mutableStateMapOf<String, ImageBitmap>() }
    val bounds = remember { mutableMapOf<Long, Rect>() }
    var panelOrigin by remember { mutableStateOf(Offset.Zero) }
    var drag by remember { mutableStateOf<LayerDrag?>(null) }
    var menu by remember { mutableStateOf<JSONObject?>(null) }
    var menuPoint by remember { mutableStateOf(Offset.Zero) }
    fun contextMenu(layer: JSONObject, mask: Boolean, point: Offset) {
        val id = layer.getLong("id")
        host.layer(obj("op" to "context", "id" to id, "mask" to mask))
        host.query(obj("type" to "layer_menu", "id" to id, "mask" to mask)) { menu = it as? JSONObject; menuPoint = point - panelOrigin }
    }
    LaunchedEffect(host) {
        val revisions = mutableMapOf<String, Long>()
        val pending = mutableMapOf<Long, PreviewRequest>()
        var next = 0L
        while (isActive) {
            delay(120)
            val visible = list.layoutInfo.visibleItemsInfo.map { it.key }.toSet()
            val requests = currentLayers.filter { it.getLong("id") in visible }.flatMap { layer ->
                listOf(false, true).mapNotNull { mask ->
                    if (if (mask) !layer.getBoolean("has_mask") else layer.getBoolean("group") || !layer.isNull("content_icon")) return@mapNotNull null
                    val key = "${layer.getLong("id")}:$mask"
                    val revision = layer.getLong(if (mask) "mask_revision" else "paint_revision")
                    if (revisions[key] == revision || pending.values.any { it.key == key }) null
                    else PreviewRequest(++next, key, layer.getLong(if (mask) "mask_id" else "id"), revision)
                }
            }.take((8-pending.size).coerceAtLeast(0))
            val response = host.layerQuery(obj("type" to "layer_thumbnails", "requests" to JSONArray(requests.map { JSONArray(listOf(it.id,it.target)) }))) ?: continue
            val accepted = response.array("accepted").values().map { (it as Number).toLong() }.toSet()
            requests.filter { it.id in accepted }.forEach { pending[it.id] = it }
            response.array("images").values().forEach { raw ->
                val image = raw as JSONArray
                val request = pending.remove(image.getLong(0)) ?: return@forEach
                val width = image.getInt(1); val height = image.getInt(2); val bytes = image.getJSONArray(3)
                val bitmap = withContext(Dispatchers.Default) {
                    val pixels = IntArray(width * height) { p ->
                        (bytes.getInt(p*4+3) shl 24) or (bytes.getInt(p*4) shl 16) or (bytes.getInt(p*4+1) shl 8) or bytes.getInt(p*4+2)
                    }
                    Bitmap.createBitmap(pixels,width,height,Bitmap.Config.ARGB_8888).asImageBitmap()
                }
                images[request.key] = bitmap; revisions[request.key] = request.revision
            }
            val ids = currentLayers.map { it.getLong("id").toString() }.toSet()
            images.keys.filter { it.substringBefore(':') !in ids }.forEach { images.remove(it); revisions.remove(it) }
        }
    }
    val import = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) scope.launch {
            try {
                val decoded = withContext(Dispatchers.IO) { context.contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) } }
                if (decoded != null) {
                    val rgba = withContext(Dispatchers.Default) {
                        val pixels = IntArray(decoded.width*decoded.height); decoded.getPixels(pixels,0,decoded.width,0,0,decoded.width,decoded.height)
                        ByteArray(pixels.size*4).also { bytes -> pixels.forEachIndexed { i,p ->
                            bytes[i*4] = (p shr 16).toByte(); bytes[i*4+1] = (p shr 8).toByte(); bytes[i*4+2] = p.toByte(); bytes[i*4+3] = (p ushr 24).toByte()
                        } }
                    }
                    host.importLayer("Imported image",decoded.width,decoded.height,rgba); decoded.recycle()
                }
            } catch (error: Exception) { android.util.Log.e("CapyCanvas","Image import failed",error) }
        }
    }
    Box(modifier.fillMaxSize().onGloballyPositioned { panelOrigin = it.boundsInRoot().topLeft }) {
        Column(Modifier.fillMaxSize()) {
            Column(Modifier.padding(horizontal = 6.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    var blendOpen by remember { mutableStateOf(false) }
                    Box(Modifier.weight(1f)) {
                        Row(Modifier.fillMaxWidth().height(26.dp).background(colors.input,RoundedCornerShape(6.dp))
                            .clickable(enabled = controls.getBoolean("blend")) { blendOpen = true }.padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(active?.getString("blend_label") ?: "Normal",Modifier.weight(1f),maxLines=1,overflow=TextOverflow.Ellipsis)
                            SharedIcon("chevron-down", "Layer blend mode",Modifier.size(12.dp))
                        }
                        DropdownMenu(blendOpen,{blendOpen=false}) {
                            host.catalog.array("layer_blends").values().forEachIndexed { i,label -> DropdownMenuItem(text={Text(label.toString())},onClick={
                                blendOpen=false; host.layer(obj("op" to "blend","id" to active!!.getLong("id"),"value" to i))
                            }) }
                        }
                    }
                    NumericSetting("Layer opacity",active?.number("opacity") ?: 1f,host.catalog.getJSONObject("layer_opacity"),Modifier.weight(1f),
                        enabled=controls.getBoolean("opacity"),inline=true) { host.dispatch(obj("type" to "set_layer_opacity","opacity" to it)) }
                }
                Row(horizontalArrangement=Arrangement.spacedBy(2.dp)) {
                    for ((icon, label, property, op, capability) in listOf(
                        listOf("alpha-lock","Alpha lock","alpha_locked","alpha_lock","alpha_lock"),
                        listOf("lock","Lock editing","locked","lock","edit_lock"),
                        listOf("clip","Clip to layer below","clipped","clip","clip"))) {
                        LayerButton(host,icon,label,enabled=controls.getBoolean(capability),selected=active?.optBoolean(property)==true,
                            action=active?.let { obj("type" to "layer","action" to obj("op" to op,"id" to it.getLong("id"),"value" to !it.getBoolean(property))) })
                    }
                    LayerButton(host,"reference",view.getString("reference_action_label"),enabled=view.getBoolean("can_reference"),
                        selected=view.getBoolean("references_selected"),subtle=true,action=obj("type" to "layer","action" to obj("op" to "reference_selection")))
                }
            }
            LazyColumn(Modifier.weight(1f).fillMaxWidth().testTag("layer-rows"),state=list) {
                items(layers,key={it.getLong("id")}) { layer ->
                    val id=layer.getLong("id")
                    val target=drag?.takeIf { it.target==id }
                    val highlight=when { target==null -> 0; layer.getBoolean("group") && target.fraction>.25f && target.fraction<.75f -> 3; target.fraction<.5f -> 1; else -> 2 }
                    LayerRow(host,layer,view.optLong("rename_layer"),images,Modifier.onGloballyPositioned { bounds[id]=it.boundsInRoot() },highlight,
                        context={mask,point -> contextMenu(layer,mask,point)},
                        drag={point,finished,cancelled ->
                            val origin=bounds[id] ?: return@LayerRow
                            if (finished) {
                                val end=drag; drag=null
                                if (!cancelled && end?.target!=null) host.layer(obj("op" to "drop","id" to id,"target" to end.target,"fraction" to end.fraction))
                            } else {
                                val to=currentLayers.find { it.getLong("id")!=id && bounds[it.getLong("id")]?.contains(point)==true }
                                val fraction=to?.let { if (!it.getBoolean("can_drop_below")) 0f else bounds[it.getLong("id")]!!.let { r -> (point.y-r.top)/r.height } } ?: 0f
                                drag=LayerDrag(id,origin.top,point,to?.getLong("id"),fraction)
                            }
                        })
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal=6.dp,vertical=4.dp),horizontalArrangement=Arrangement.spacedBy(2.dp)) {
                LayerButton(host,"plus","New layer",action=obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
                LayerButton(host,"folder","New group",action=obj("type" to "layer","action" to obj("op" to "new","group" to true,"clipped" to false)))
                LayerButton(host,"mask","Add layer mask",enabled=controls.getBoolean("mask"),action=active?.let { obj("type" to "layer","action" to obj("op" to "add_mask","id" to it.getLong("id"),"replace" to false)) })
                LayerButton(host,"image","Import image as layer") { import.launch("image/*") }
                LayerButton(host,"delete","Delete selected layers",enabled=view.getBoolean("can_delete"),action=obj("type" to "layer","action" to obj("op" to "delete_selected")))
                Spacer(Modifier.weight(1f))
                LayerButton(host,"more","Layer actions") { active?.let { contextMenu(it,it.getBoolean("mask_selected"),panelOrigin+Offset(0f,40f)) } }
            }
        }
        drag?.let { d -> layers.find { it.getLong("id")==d.id }?.let { layer ->
            LayerRow(host,layer,-1,images,Modifier.offset { IntOffset(0,(d.pointer.y-panelOrigin.y-20*density.density).roundToInt()) }
                .alpha(.7f).background(colors.panel),preview=true)
        } }
        if (menu!=null) Box(Modifier.offset { IntOffset(menuPoint.x.roundToInt(),menuPoint.y.roundToInt()) }.size(1.dp)) {
            WorkspaceMenu(host,menu!!) { menu=null }
        }
    }
}

@Composable private fun LayerButton(host:CanvasHost,icon:String,label:String,modifier:Modifier=Modifier,enabled:Boolean=true,selected:Boolean=false,subtle:Boolean=false,
    action:JSONObject?=null,onClick:()->Unit={action?.let { host.dispatch(it) }}) {
    val colors=LocalPalette.current
    val content: @Composable () -> Unit = {
    Box(Modifier.size(24.dp).clip(RoundedCornerShape(4.dp)).alpha(if(enabled)1f else .4f)
        .background(if(selected) { if(subtle) colors.text.copy(alpha=.12f) else colors.active } else Color.Transparent)
        .clickable(enabled=enabled,onClick=onClick),contentAlignment=Alignment.Center) { SharedIcon(icon,label,Modifier.size(16.dp)) }
    }
    if(action!=null) ActionTip(host,label,action,modifier,content) else HoverTip(label,modifier,content=content)
}

@Composable private fun LayerRow(host:CanvasHost,layer:JSONObject,rename:Long,images:Map<String,ImageBitmap>,modifier:Modifier=Modifier,highlight:Int=0,
    preview:Boolean=false,context:(Boolean,Offset)->Unit={_,_->},drag:(Offset,Boolean,Boolean)->Unit={_,_,_->}) {
    val colors=LocalPalette.current
    val id=layer.getLong("id")
    val latest by rememberUpdatedState(layer)
    var origin by remember { mutableStateOf(Offset.Zero) }
    var press by remember { mutableStateOf(Offset.Zero) }
    val density=LocalDensity.current.density
    fun select(mask:Boolean=false) = host.layer(obj("op" to "select","id" to id,"mask" to mask))
    Row(modifier.fillMaxWidth().heightIn(min=40.dp).onGloballyPositioned { origin=it.boundsInRoot().topLeft }
        .background(if(layer.getBoolean("selected")) colors.active else Color.Transparent)
        .drawWithContent {
            drawContent()
            when(highlight) { 1 -> drawLine(colors.accent,Offset.Zero,Offset(size.width,0f),2*density)
                2 -> drawLine(colors.accent,Offset(0f,size.height),Offset(size.width,size.height),2*density)
                3 -> drawRect(colors.accent,style=androidx.compose.ui.graphics.drawscope.Stroke(2*density)) }
        }.then(if(preview) Modifier else Modifier.pointerInput(id) {
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false); press=down.position
                val row=latest
                val canDrag=row.getBoolean("can_drop_below") &&
                    (down.type!=PointerType.Touch || down.position.x>=size.width-20*density)
                var dragging=false
                try { do {
                    val event=awaitPointerEvent(); val change=event.changes.find { it.id==down.id } ?: break
                    if (canDrag && !dragging && (change.position-down.position).getDistance()>6*density) dragging=true
                    if (dragging) { change.consume(); drag(origin+change.position,!change.pressed,false); if(!change.pressed)dragging=false }
                    if (!change.pressed) break
                } while(true) } finally { if(dragging)drag(origin+down.position,true,true) }
            }
        }.combinedClickable(onClick={select()},onLongClick={context(false,origin+press)}))
        .padding(horizontal=6.dp,vertical=2.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(2.dp)) {
        LayerButton(host,if(layer.getBoolean("visible")) "eye" else "eye-hidden",if(layer.getBoolean("visible"))"Hide layer" else "Show layer",
            action=obj("type" to "set_layer_visibility","id" to id,"visible" to !layer.getBoolean("visible")))
        LayerButton(host,iconName(layer.getString("selection_icon")),"Select layer without changing drawing target",
            action=obj("type" to "layer","action" to obj("op" to "toggle_selection","id" to id)))
        Spacer(Modifier.width((layer.getInt("depth")*8).coerceAtMost(24).dp))
        Box(Modifier.width(3.dp).height(28.dp).alpha(if(layer.getBoolean("clipped"))1f else 0f).background(Color(0xffe999a5),RoundedCornerShape(1.dp)))
        @Composable fun thumb(mask:Boolean) {
            val group=!mask && layer.getBoolean("group")
            val selected=if(mask)layer.getBoolean("mask_selected") else layer.getBoolean("editing") && !layer.getBoolean("mask_selected")
            val operation=if(group)obj("op" to "collapse","id" to id) else obj("op" to "select","id" to id,"mask" to mask)
            val label=if(group) "Expand or collapse group" else if(mask) "Edit layer mask" else "Edit layer content"
            ActionTip(host,label,obj("type" to "layer","action" to operation),Modifier.size(30.dp)) {
            Box(Modifier.fillMaxSize().then(if(group)Modifier else Modifier.background(colors.input,RoundedCornerShape(3.dp)))
                .combinedClickable(onClick={host.layer(operation)},onLongClick={context(mask,origin+press)})
                .drawWithContent {
                    drawContent()
                    if(selected) for((x,y,dx,dy) in listOf(listOf(1f,1f,1f,1f),listOf(size.width-1,1f,-1f,1f),listOf(1f,size.height-1,1f,-1f),listOf(size.width-1,size.height-1,-1f,-1f))) {
                        for((color,width) in listOf(Color.Black to 3f,Color.White to 1f)) {
                            drawLine(color,Offset(x,y+dy*6*density),Offset(x,y),width*density); drawLine(color,Offset(x,y),Offset(x+dx*6*density,y),width*density)
                        }
                    }
                },contentAlignment=Alignment.Center) {
                if(group) SharedIcon(if(layer.getBoolean("collapsed"))"folder" else "folder-open","Expand or collapse group",Modifier.size(28.dp))
                else if(!mask && !layer.isNull("content_icon")) SharedIcon(layer.getString("content_icon"),null,Modifier.size(24.dp))
                else images["$id:$mask"]?.let { Image(it,null,Modifier.size(28.dp).alpha(if(mask && !layer.getBoolean("mask_enabled")) .4f else 1f)) }
            }
            }
        }
        thumb(false)
        if(layer.getBoolean("has_mask")) {
            LayerButton(host,"link",if(layer.getBoolean("mask_linked"))"Unlink mask from layer" else "Link mask to layer",
                Modifier.size(12.dp,24.dp).alpha(if(layer.getBoolean("mask_linked"))1f else .35f),
                action=obj("type" to "layer","action" to obj("op" to "link_mask","id" to id,"value" to !layer.getBoolean("mask_linked"))))
            thumb(true)
        }
        Column(Modifier.weight(1f).padding(start=6.dp)) {
            if(rename==id && !preview) {
                var name by remember { mutableStateOf(TextFieldValue(layer.getString("label"),TextRange(0,layer.getString("label").length))) }
                val focus=remember { FocusRequester() }; var hadFocus by remember { mutableStateOf(false) }; var done by remember { mutableStateOf(false) }
                fun finish() { if(!done) { done=true; host.layer(if(name.text.isBlank())obj("op" to "cancel_rename") else obj("op" to "rename","id" to id,"name" to name.text)) } }
                BasicTextField(name,{name=it},Modifier.fillMaxWidth().focusRequester(focus).onFocusChanged { if(hadFocus && !it.isFocused)finish(); hadFocus=it.isFocused; host.editingText=it.isFocused },
                    textStyle=LocalTextStyle.current.copy(color=colors.text),singleLine=true,keyboardOptions=KeyboardOptions(imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finish()}))
                LaunchedEffect(id) { focus.requestFocus() }
                DisposableEffect(id) { onDispose { host.editingText=false } }
            } else Text(layer.getString("label"),Modifier.combinedClickable(onClick={select()},onDoubleClick={host.layer(obj("op" to "begin_rename","id" to id))},onLongClick={context(false,origin+press)}),
                maxLines=1,overflow=TextOverflow.Ellipsis)
            val meta=listOf(if(layer.getInt("blend")!=0)layer.getString("blend_label") else "",if(layer.number("opacity")<1f)"${(layer.number("opacity")*100).roundToInt()}%" else "").filter { it.isNotEmpty() }.joinToString(" · ")
            if(meta.isNotEmpty())Text(meta,color=colors.secondary,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
        SharedIcon(if(layer.getBoolean("locked"))"lock" else "alpha-lock",null,Modifier.size(12.dp).alpha(if(layer.getBoolean("locked") || layer.getBoolean("alpha_locked"))1f else 0f))
        SharedIcon("grip","Drag layer",Modifier.size(12.dp).alpha(if(layer.getBoolean("can_drop_below")) .6f else 0f))
    }
}
