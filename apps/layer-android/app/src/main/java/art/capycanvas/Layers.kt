package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.isShiftPressed as keyShiftPressed
import androidx.compose.ui.input.key.isCtrlPressed as keyCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed as keyMetaPressed
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.isCtrlPressed
import androidx.compose.ui.input.pointer.isMetaPressed
import androidx.compose.ui.input.pointer.isShiftPressed
import androidx.compose.ui.input.pointer.isAltPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
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
private data class LayerThumbnail(val hit: Rect, val anchor: Rect)
private data class ObjectDrag(val id: Long, val pointer: Offset, val target: Long? = null, val below: Boolean = false)
private data class LayerDrag(val id: Long, val pointer: Offset, val target: Long? = null, val fraction: Float = 0f, val surface: String = "row", val hintTarget: Long? = null, val position: String? = null, val effectOwner: Long? = null)
private fun iconName(name: String) = name.removePrefix("layer-").removeSuffix("-symbolic")

/** Transient native gesture state, shared by retained layer panels in this window. */
internal class LayerSwipe {
    var owner by mutableStateOf<Any?>(null)
    var offset by mutableFloatStateOf(0f)
    var tracking by mutableStateOf(false)
    var bounds = Rect.Zero
    fun close() { owner=null; offset=0f; tracking=false }
}

/** The native view translates the shared layer model; no layer policy lives here. */
@Composable internal fun LayerPanel(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier, onContent: (PanelContentSize) -> Unit = {}) {
    val colors = LocalPalette.current
    val density = LocalDensity.current
    val view = state.getJSONObject("layer_tools")
    val active = view.objectOrNull("editing_layer")
    val controls = view.getJSONObject("controls")
    val layers = state.array("layers").objects()
    val objectRows = layers.associate { it.getLong("id") to it.array("objects").objects() }
    var headerHeight by remember { mutableFloatStateOf(0f) }
    var footerHeight by remember { mutableFloatStateOf(0f) }
    var rowHeight by remember { mutableFloatStateOf(40f) }
    val fixedHeight = headerHeight + footerHeight
    val measured = if (headerHeight > 0f && footerHeight > 0f)
        PanelContentSize(fixedHeight + (layers.size + objectRows.values.sumOf { it.size }) * rowHeight, fixedHeight, rowHeight) else null
    SideEffect { measured?.let(onContent) }
    val currentLayers by rememberUpdatedState(layers)
    val currentObjects by rememberUpdatedState(objectRows)
    val objectBounds = remember { mutableMapOf<Long, Rect>() }
    var objectDrag by remember { mutableStateOf<ObjectDrag?>(null) }
    val list = rememberLazyListState()
    val epoch = state.getJSONObject("document_file").optLong("epoch")
    val images = remember(epoch) { mutableStateMapOf<String, ImageBitmap>() }
    val bounds = remember { mutableMapOf<Long, Rect>() }
    val thumbnails = remember { mutableStateMapOf<Long, LayerThumbnail>() }
    var dragGeneration by remember { mutableIntStateOf(0) }
    var filterPoint by remember { mutableStateOf(Offset.Zero) }
    var panelOrigin by remember { mutableStateOf(Offset.Zero) }
    var drag by remember { mutableStateOf<LayerDrag?>(null) }
    var menu by remember { mutableStateOf<JSONObject?>(null) }
    var menuRequest by remember { mutableStateOf<JSONObject?>(null) }
    var menuGeneration by remember { mutableIntStateOf(0) }
    LaunchedEffect(epoch) { host.layerSwipe.close(); dragGeneration++; drag = null; objectDrag = null; menuGeneration++; menu = null; menuRequest = null }
    var contactHeld by remember { mutableStateOf(false) }
    var menuPoint by remember { mutableStateOf(Offset.Zero) }
    fun contextMenu(layer: JSONObject, mask: Boolean, point: Offset) {
        if (drag != null) return
        val request = ++menuGeneration
        val id = layer.getLong("id")
        host.layer(obj("op" to "context", "id" to id, "mask" to mask))
        val query = obj("type" to "layer_menu", "id" to id, "mask" to mask)
        host.query(query) {
            if (request == menuGeneration && host.menuEpoch() == epoch && drag == null) { menuRequest = query; menu = it as? JSONObject; menuPoint = point - panelOrigin }
        }
    }
    fun objectMenu(id: Long, point: Offset) {
        if (drag != null || objectDrag != null) return
        val request = ++menuGeneration
        val query = obj("type" to "object_menu", "id" to id)
        host.query(query) {
            if (request == menuGeneration && host.menuEpoch() == epoch && objectDrag == null) { menuRequest = query; menu = it as? JSONObject; menuPoint = point - panelOrigin }
        }
    }
    fun moveObject(row: JSONObject, point: Offset, finished: Boolean, cancelled: Boolean) {
        val id = row.getLong("id")
        val siblings = currentObjects[row.getLong("layer")].orEmpty()
        val to = siblings.find { it.getLong("id") != id && it.getBoolean("editable") && objectBounds[it.getLong("id")]?.contains(point) == true }
        val below = to?.let { point.y > objectBounds[it.getLong("id")]!!.center.y } ?: false
        if (finished) objectDrag = null else {
            if (objectDrag == null) { menuGeneration++; menu = null }
            objectDrag = ObjectDrag(id, point, to?.getLong("id"), below)
        }
        if (finished && !cancelled && to != null)
            host.dispatch(obj("type" to "object", "action" to obj("op" to "drop", "id" to id, "target" to to.getLong("id"), "below" to below)))
    }
    fun moveLayer(id: Long, point: Offset, finished: Boolean, cancelled: Boolean) {
        val ticket = ++dragGeneration
        val to = currentLayers.find { it.getLong("id") != id && bounds[it.getLong("id")]?.contains(point) == true }
        val target = to?.getLong("id")
        val fraction = target?.let { bounds[it]!!.let { rect -> (point.y - rect.top) / rect.height } } ?: 0f
        val surface = if (target != null && thumbnails[target]?.hit?.contains(point) == true) "thumbnail" else "row"
        val next = LayerDrag(id,point,target,fraction,surface)
        if (finished) drag = null else {
            if (drag == null) { menuGeneration++; menu = null }
            drag = next
        }
        if (cancelled || target == null) return
        host.query(obj("type" to "layer_drop","epoch" to epoch,"id" to id,"target" to target,"fraction" to fraction,"surface" to surface)) { raw ->
            val hint = raw as? JSONObject ?: return@query
            if (ticket != dragGeneration || hint.optLong("epoch") != epoch || host.menuEpoch() != epoch) return@query
            val position = hint.optString("position").takeUnless { hint.isNull("position") }
            val normalized = hint.optLong("target").takeUnless { hint.isNull("target") }
            if (finished) {
                if (normalized != null && position != null) host.layer(obj("op" to "drop","id" to id,"target" to target,"fraction" to fraction,"surface" to surface))
            } else drag = next.copy(hintTarget=normalized,position=position,effectOwner=hint.optLong("effect_owner").takeUnless { hint.isNull("effect_owner") })
        }
    }
    LaunchedEffect(host, epoch) {
        val revisions = mutableMapOf<String, Long>()
        val pending = mutableMapOf<Long, PreviewRequest>()
        var next = 0L
        while (isActive) {
            delay(120)
            val visible = list.layoutInfo.visibleItemsInfo.map { it.key }.toSet()
            fun request(key: String, target: Long, revision: Long) =
                if (revisions[key] == revision || pending.values.any { it.key == key }) null else PreviewRequest(++next, key, target, revision)
            val requests = (currentLayers.filter { it.getLong("id") in visible }.flatMap { layer ->
                listOf(false, true).mapNotNull { mask ->
                    if (if (mask) !layer.getBoolean("has_mask") else !layer.getBoolean("has_thumbnail")) return@mapNotNull null
                    request("${layer.getLong("id")}:$mask", layer.getLong(if (mask) "mask_id" else "id"), layer.getLong(if (mask) "mask_revision" else "paint_revision"))
                }
            } + currentObjects.values.flatten().filter { it.getLong("id") in visible }.mapNotNull { row ->
                request("${row.getLong("id")}:false", row.getLong("id"), row.getLong("thumbnail_revision"))
            }).take((8-pending.size).coerceAtLeast(0))
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
            val ids = (currentLayers.map { it.getLong("id") } + currentObjects.values.flatten().map { it.getLong("id") }).map { it.toString() }.toSet()
            images.keys.filter { it.substringBefore(':') !in ids }.forEach { images.remove(it); revisions.remove(it) }
        }
    }
    Box(modifier.fillMaxSize().onGloballyPositioned { panelOrigin = it.boundsInRoot().topLeft }) {
        Column(Modifier.fillMaxSize()) {
            Column(Modifier.wrapContentHeight(unbounded = true).onSizeChanged { headerHeight = it.height / density.density }.padding(horizontal = 6.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    var blendMenu by remember { mutableStateOf<JSONObject?>(null) }
                    var blendRequest by remember { mutableStateOf<JSONObject?>(null) }
                    var blendGeneration by remember { mutableIntStateOf(0) }
                    LaunchedEffect(epoch) { blendGeneration++; blendMenu = null; blendRequest = null }
                    Box(Modifier.weight(1f)) {
                        Row(Modifier.fillMaxWidth().height(26.dp).background(colors.input,ControlShape).testTag("layer-blend")
                            .clickable(enabled = controls.getBoolean("blend")) {
                                active?.let {
                                    val ticket = ++blendGeneration
                                    val request = obj("type" to "layer_blend_menu","id" to it.getLong("id"))
                                    host.query(request) { menu ->
                                        if (ticket == blendGeneration && host.menuEpoch() == epoch) { blendRequest = request; blendMenu = menu as? JSONObject }
                                    }
                                }
                            }.padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(active?.getString("blend_label").orEmpty(),Modifier.weight(1f),maxLines=1,overflow=TextOverflow.Ellipsis)
                            SharedIcon("chevron-down", host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("blend"),Modifier.size(12.dp))
                        }
                        blendMenu?.let { WorkspaceMenu(host,it, copy = { blendRequest?.let { host.menuCopy(it) } }) { blendGeneration++; blendMenu=null; blendRequest=null } }
                    }
                    NumericSetting(host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("opacity"),active?.number("opacity") ?: 1f,host.catalog.getJSONObject("layer_opacity"),Modifier.weight(1f).testTag("layer-opacity"),
                        enabled=controls.getBoolean("opacity"),inline=true) { host.dispatch(obj("type" to "set_layer_opacity","opacity" to it)) }
                }
                Row(horizontalArrangement=Arrangement.spacedBy(2.dp)) {
                    for ((icon, label, property, op, capability) in listOf(
                        listOf("alpha-lock",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("alpha_lock"),"alpha_locked","alpha_lock","alpha_lock"),
                        listOf("lock",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("lock_editing"),"locked","lock","edit_lock"))) {
                        LayerButton(host,icon,label,enabled=controls.getBoolean(capability),selected=active?.optBoolean(property)==true,
                            action=active?.let { obj("type" to "layer","action" to obj("op" to op,"id" to it.getLong("id"),"value" to !it.getBoolean(property))) })
                    }
                    val attachment = view.getJSONObject("attachment")
                    LayerButton(host,iconName(attachment.getString("icon")),attachment.getString("label"),Modifier.testTag("layer-attachment"),
                        enabled=!attachment.isNull("action"),selected=attachment.getBoolean("checked"),
                        action=attachment.objectOrNull("action")?.let { obj("type" to "layer","action" to it) })
                    LayerButton(host,"reference",view.getString("reference_action_label"),enabled=view.getBoolean("can_reference"),
                        selected=view.getBoolean("references_selected"),subtle=true,action=obj("type" to "layer","action" to obj("op" to "reference_selection")))
                }
            }
            Box(Modifier.weight(1f).fillMaxWidth()) {
                LazyColumn(Modifier.fillMaxSize().testTag("layer-rows"),state=list) {
                    for (layer in layers) {
                    item(key=layer.getLong("id")) {
                        val id=layer.getLong("id")
                        val highlight=drag?.takeIf { it.hintTarget==id }?.position
                        LayerRow(host,layer,view.optLong("rename_layer",-1),images,Modifier.imageDropTarget(host,id).onSizeChanged { rowHeight = it.height / density.density }.onGloballyPositioned { bounds[id]=it.boundsInRoot() },highlight,attachment=drag?.effectOwner==id,
                            context={mask,point -> contextMenu(layer,mask,point)},
                            contentBounds={rect,shift -> if(rect==null) { thumbnails.remove(id);bounds.remove(id) } else thumbnails[id]=LayerThumbnail(rect,rect.translate(Offset(shift.roundToInt().toFloat(),0f)))},
                            held={contactHeld=it},cancelContext={menuGeneration++; menu=null},
                            drag={point,finished,cancelled -> moveLayer(id,point,finished,cancelled)})
                    }
                    items(objectRows[layer.getLong("id")].orEmpty(),key={it.getLong("id")}) { row ->
                        val id=row.getLong("id")
                        ImageObjectRow(host,row,layer.getInt("depth")+1,images,Modifier.onSizeChanged { rowHeight = it.height / density.density }.onGloballyPositioned { objectBounds[id]=it.boundsInRoot() },
                            highlight=objectDrag?.takeIf { it.target==id }?.let { if(it.below) "below" else "above" },
                            context={point -> objectMenu(id,point)},held={contactHeld=it},cancelContext={menuGeneration++; menu=null},
                            drag={point,finished,cancelled -> moveObject(row,point,finished,cancelled)},forget={ objectBounds.remove(id) })
                    }
                    }
                }
                LayerConnections(layers,view.array("connections").objects(),thumbnails,Modifier.matchParentSize())
            }
            Row(Modifier.fillMaxWidth().wrapContentHeight(unbounded = true).onSizeChanged { footerHeight = it.height / density.density }.padding(horizontal=6.dp,vertical=4.dp),horizontalArrangement=Arrangement.spacedBy(2.dp)) {
                LayerButton(host,"add-layer",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("new_layer"),action=obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
                LayerButton(host,"folder",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("new_group"),action=obj("type" to "layer","action" to obj("op" to "new","group" to true,"clipped" to false)))
                LayerButton(host,"selection-brush",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("new_selection_layer"),action=obj("type" to "invoke","command" to "new_selection_layer"))
                LayerButton(host,"mask",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("add_mask"),enabled=controls.getBoolean("mask"),action=active?.let { obj("type" to "layer","action" to obj("op" to "add_mask","id" to it.getLong("id"),"replace" to false)) })
                LayerButton(host,"add-filter",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("add_filter"),Modifier.testTag("layer-add-filter").onGloballyPositioned { filterPoint = it.boundsInRoot().topLeft - panelOrigin },enabled=view.optJSONObject("add_filter")!=null) {
                    menuGeneration++; menuRequest = null; menu = view.optJSONObject("add_filter"); menuPoint = filterPoint
                }
                LayerButton(host,"image",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("import_image"), action=obj("type" to "invoke", "command" to "import_image"))
                LayerButton(host,"delete",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("delete_selected"),enabled=view.getBoolean("can_delete"),action=obj("type" to "layer","action" to obj("op" to "delete_selected")))
                Spacer(Modifier.weight(1f))
                LayerButton(host,"more-small",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("actions")) { active?.let { contextMenu(it,it.getBoolean("mask_selected"),panelOrigin+Offset(0f,40f)) } }
            }
        }
        objectDrag?.let { d -> objectRows.values.flatten().find { it.getLong("id")==d.id }?.let { row ->
            val depth=layers.find { it.getLong("id")==row.getLong("layer") }?.getInt("depth") ?: 0
            ImageObjectRow(host,row,depth+1,images,Modifier.testTag("image-object-drag-preview").offset { IntOffset(0,(d.pointer.y-panelOrigin.y-20*density.density).roundToInt()) }
                .alpha(.7f).background(colors.panel),preview=true)
        } }
        drag?.let { d -> layers.find { it.getLong("id")==d.id }?.let { layer ->
            LayerRow(host,layer,-1,images,Modifier.testTag("layer-drag-preview").offset { IntOffset(0,(d.pointer.y-panelOrigin.y-20*density.density).roundToInt()) }
                .alpha(.7f).background(colors.panel),preview=true)
        } }
        if (menu!=null) Box(Modifier.offset { IntOffset(menuPoint.x.roundToInt(),menuPoint.y.roundToInt()) }.size(1.dp)) {
            WorkspaceMenu(host,menu!!,preserveContact=contactHeld, copy = { menuRequest?.let { host.menuCopy(it) } }) { menuGeneration++; menu=null; menuRequest=null }
        }
    }
}

@Composable private fun LayerButton(host:CanvasHost,icon:String,label:String,modifier:Modifier=Modifier,enabled:Boolean=true,selected:Boolean=false,subtle:Boolean=false,
    action:JSONObject?=null,size:Dp=24.dp,iconSize:Dp=16.dp,onClick:()->Unit={action?.let { host.dispatch(it) }}) {
    val colors=LocalPalette.current
    val content: @Composable () -> Unit = {
    Box(Modifier.size(size).clip(ControlShape).alpha(if(enabled)1f else .4f)
        .background(if(selected) { if(subtle) colors.text.copy(alpha=.12f) else colors.active } else Color.Transparent)
        .clickable(enabled=enabled,onClick=onClick),contentAlignment=Alignment.Center) { SharedIcon(icon,label,Modifier.size(iconSize)) }
    }
    if(action!=null) ActionTip(host,label,action,modifier,content) else HoverTip(label,modifier,content=content)
}

@Composable private fun LayerRow(host:CanvasHost,layer:JSONObject,rename:Long,images:Map<String,ImageBitmap>,modifier:Modifier=Modifier,highlight:String?=null,
    preview:Boolean=false,attachment:Boolean=false,context:(Boolean,Offset)->Unit={_,_->},contentBounds:(Rect?,Float)->Unit={_,_->},held:(Boolean)->Unit={},cancelContext:()->Unit={},drag:(Offset,Boolean,Boolean)->Unit={_,_,_->}) {
    val colors=LocalPalette.current
    val id=layer.getLong("id")
    val label=layer.getString("label")
    val rowCaption=remember(label, host.languageTag) { JSONObject(Native.nativeCaption(obj("type" to "layer_row", "title" to label).toString(), host.languageTag)).getString("text") }
    val latest by rememberUpdatedState(layer)
    DisposableEffect(id) { onDispose { contentBounds(null,0f) } }
    var origin by remember { mutableStateOf(Offset.Zero) }
    var press by remember { mutableStateOf(Offset.Zero) }
    var longPressed by remember { mutableStateOf(false) }
    var contactActive by remember { mutableStateOf(false) }
    var contactMenus by remember { mutableStateOf(true) }
    var holdEligible by remember { mutableStateOf(false) }
    var maskBounds by remember { mutableStateOf(Rect.Zero) }
    val focused=LocalWindowInfo.current.isWindowFocused
    val swipe=host.layerSwipe
    val swipeOwner=remember { Any() }
    var rowBounds by remember { mutableStateOf(Rect.Zero) }
    val shift by animateFloatAsState(if(swipe.owner===swipeOwner) swipe.offset else 0f,
        tween(if(swipe.owner===swipeOwner && swipe.tracking) 0 else 150),label="Layer swipe")
    DisposableEffect(swipeOwner) { onDispose { if(swipe.owner===swipeOwner)swipe.close() } }
    LaunchedEffect(focused,layer.optBoolean("can_delete")) {
        if((!focused || !layer.optBoolean("can_delete")) && swipe.owner===swipeOwner)swipe.close()
    }
    val density=LocalDensity.current.density
    var extendSelection by remember { mutableStateOf(false) }
    var toggleSelection by remember { mutableStateOf(false) }
    fun select(toggle:Boolean=toggleSelection) = host.layer(obj("op" to "select_row","id" to id,"extend" to extendSelection,"toggle" to toggle))
    fun openContext(mask:Boolean) {
        if (!focused || (contactActive && !contactMenus)) return
        if (!contactActive || (holdEligible && !longPressed)) { longPressed=true; context(mask,origin+press) }
    }
    Box(modifier.onPreviewKeyEvent {
            extendSelection=it.keyShiftPressed; toggleSelection=it.keyCtrlPressed || it.keyMetaPressed; false
        }.semantics { contentDescription = rowCaption }.fillMaxWidth().heightIn(min=40.dp).clipToBounds().then(if(preview) Modifier else Modifier.testTag("layer-row-$id")).onGloballyPositioned {
            rowBounds=it.boundsInRoot(); origin=rowBounds.topLeft
            if(swipe.owner===swipeOwner)swipe.bounds=rowBounds
        }
        .drawWithContent {
            drawContent()
            when(highlight) { "above" -> drawLine(colors.accent,Offset.Zero,Offset(size.width,0f),2*density)
                "below" -> drawLine(colors.accent,Offset(0f,size.height),Offset(size.width,size.height),2*density)
                "into" -> drawRect(colors.accent,style=androidx.compose.ui.graphics.drawscope.Stroke(2*density)) }
        }.then(if(preview || rename==id) Modifier else Modifier.pointerInput(id,focused) {
            if (!focused) return@pointerInput
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial); press=down.position
                extendSelection=currentEvent.keyboardModifiers.isShiftPressed
                toggleSelection=currentEvent.keyboardModifiers.isCtrlPressed || currentEvent.keyboardModifiers.isMetaPressed
                if (toggleSelection) return@awaitEachGesture
                longPressed=false
                contactActive=true
                holdEligible=true
                val row=latest
                val directDrag=down.type==PointerType.Mouse || down.position.x>=size.width-20*density
                val secondary=currentEvent.buttons.isSecondaryPressed
                contactMenus=down.type!=PointerType.Mouse || secondary
                var dragging=false
                var swiping=false
                val swipeStart=if(swipe.owner===swipeOwner)swipe.offset else 0f
                var released=false
                var remaining=viewConfiguration.longPressTimeoutMillis
                var eventTime=down.uptimeMillis
                held(true)
                if (secondary) { openContext(row.getBoolean("has_mask") && maskBounds.contains(origin+press)); down.consume() }
                try { do {
                    val event=if (holdEligible && !longPressed && !dragging) {
                        withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                    } else awaitPointerEvent(PointerEventPass.Initial)
                    if (event==null) {
                        if (contactMenus) openContext(row.getBoolean("has_mask") && maskBounds.contains(origin+press))
                        else longPressed=true // Mouse holds suppress clicks without opening menus.
                        continue
                    }
                    val change=event.changes.find { it.id==down.id } ?: break
                    remaining=(remaining-(change.uptimeMillis-eventTime)).coerceAtLeast(1)
                    eventTime=change.uptimeMillis
                    if (!change.pressed && change.isConsumed) break
                    val moved=(change.position-down.position).getDistance()>viewConfiguration.touchSlop
                    // Touch and pen keep native scrolling until a stationary
                    // hold wins. Explicit grips and mouse bodies are immediate.
                    if (!directDrag && !longPressed && moved && holdEligible) {
                        holdEligible=false
                        val delta=change.position-down.position
                        val allowed=if(delta.x<0)row.getBoolean("can_delete") else swipeStart>0 || !row.isNull("right_swipe")
                        if(allowed && kotlin.math.abs(delta.x)>kotlin.math.abs(delta.y)) {
                            swiping=true; swipe.owner=swipeOwner; swipe.bounds=rowBounds; swipe.tracking=true
                            cancelContext()
                        }
                    }
                    if(swiping) {
                        change.consume()
                        val minimum=if(swipeStart==0f && !row.isNull("right_swipe"))-72*density else 0f
                        swipe.offset=(swipeStart-(change.position.x-down.position.x)).coerceIn(minimum,72*density)
                        if(!change.pressed) { released=true; break }
                        continue
                    }
                    if (!secondary && row.getBoolean("can_drop_below") && (directDrag || longPressed) && !dragging && change.pressed &&
                        moved) { dragging=true; holdEligible=false }
                    if (dragging) { change.consume(); drag(origin+change.position,!change.pressed,false); if(!change.pressed)dragging=false }
                    if (longPressed) change.consume()
                    if (!change.pressed) { released=true; break }
                } while(true) } finally {
                    if(swiping) {
                        val toggle=released && swipeStart==0f && swipe.offset<=-72*density*.4f
                        swipe.tracking=false
                        if(released && swipe.offset>=72*density*.4f)swipe.offset=72*density else swipe.close()
                        if(toggle)latest.objectOrNull("right_swipe")?.let { host.layer(it) }
                    }
                    if(dragging)drag(origin+down.position,true,true)
                    if(!released)cancelContext()
                    longPressed=false; holdEligible=false; contactActive=false; contactMenus=true; held(false)
                }
            }
        }.combinedClickable(onClick={select()},onLongClick={openContext(false)}))
        ) {
        if(shift>0f) Box(Modifier.matchParentSize(),contentAlignment=Alignment.CenterEnd) {
            Box(Modifier.width((shift/density).dp).fillMaxHeight().background(Color(0xffc62828))
                .testTag("layer-delete-$id").clickable(enabled=layer.getBoolean("can_delete")) {
                    swipe.close(); host.layer(obj("op" to "delete","id" to id))
                },contentAlignment=Alignment.Center) { Text(host.bootstrap!!.getJSONObject("common").getString("delete"),color=Color.White,maxLines=1) }
        }
        Row(Modifier.fillMaxWidth().heightIn(min=40.dp).offset { IntOffset(-shift.roundToInt(),0) }
            .background(if(layer.getBoolean("selected")) colors.active else Color.Transparent)
            .padding(horizontal=6.dp,vertical=2.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(2.dp)) {
        LayerButton(host,if(layer.getBoolean("visible") && !layer.getBoolean("visibility_blocked")) "eye" else "eye-hidden",if(layer.optBoolean("selection_layer"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(if(layer.getBoolean("visible")) "hide_selection" else "show_selection") else if(layer.getBoolean("visible"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("hide") else host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("show"),
            Modifier.testTag("layer-eye-$id").alpha(if(layer.getBoolean("visibility_blocked")) .35f else 1f),action=obj("type" to "set_layer_visibility","id" to id,"visible" to !layer.getBoolean("visible")))
        LayerButton(host,iconName(layer.getString("selection_icon")),host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("select_row_help"),
            Modifier.semantics { selected=layer.getBoolean("selected") },
            action=obj("type" to "layer","action" to obj("op" to "select_row","id" to id,"extend" to extendSelection,"toggle" to true)),onClick={select(true)})
        Spacer(Modifier.width((layer.getInt("depth")*8).coerceAtMost(24).dp))
        Spacer(Modifier.width(3.dp))
        @Composable fun thumb(mask:Boolean) {
            val group=!mask && layer.getBoolean("group")
            val selected=if(mask)layer.getBoolean("mask_selected") else layer.getBoolean("content_selected")
            val selection=animateFloatAsState(if(selected) 1f else 0f,
                tween(200,easing=CubicBezierEasing(.25f,.46f,.45f,.94f)),label="layer-thumbnail-selection")
            val operation=if(group)obj("op" to "collapse","id" to id) else obj("op" to "select","id" to id,"mask" to mask)
            val windowed=inSeparateWindow()
            val label=if(group) host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(if(layer.getBoolean("collapsed")) "expand" else "collapse") else if(mask) host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("edit_mask") else if(layer.optBoolean("selection_layer")) host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("edit_selection") else host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("edit_content")
            ActionTip(host,label,obj("type" to "layer","action" to operation),Modifier.size(30.dp).then(if(!mask && !preview) Modifier.testTag("layer-content-$id").onGloballyPositioned { contentBounds(it.boundsInRoot(),shift) } else Modifier)) {
            Box(Modifier.fillMaxSize()) {
            Box(Modifier.fillMaxSize().then(if(mask && !preview) Modifier.onGloballyPositioned { maskBounds=it.boundsInRoot() } else Modifier)
                .then(if(group || preview) Modifier else Modifier.pointerInput(id,mask) {
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed=false, pass=PointerEventPass.Initial)
                        val keys = currentEvent.keyboardModifiers
                        if (!keys.isCtrlPressed && !keys.isMetaPressed) return@awaitEachGesture
                        down.consume()
                        var click = true
                        do {
                            val event = awaitPointerEvent(PointerEventPass.Initial)
                            val change = event.changes.find { it.id == down.id } ?: break
                            if (change.isConsumed || (change.position-down.position).getDistance() > viewConfiguration.touchSlop) click = false
                            change.consume()
                            if (!change.pressed) {
                                if (click) host.dispatch(obj("type" to "selection", "action" to obj("op" to "load_thumbnail", "id" to id, "mask" to mask, "shift" to keys.isShiftPressed, "alt" to keys.isAltPressed)))
                                break
                            }
                        } while(true)
                    }
                })
                .clip(TileShape).combinedClickable(onClick={
                    host.layer(operation)
                    layer.optJSONObject("fill_color")?.takeIf { !mask }?.let { fill ->
                        host.colorEditor = ColorEditorRequest(null, fill.getJSONObject("color"), fill.getBoolean("opaque"), windowed) { color, _ ->
                            host.dispatch(obj("type" to "effect", "action" to obj("op" to "set", "layer" to id, "key" to fill.getString("key"), "value" to obj("kind" to "color", "value" to color))))
                        }
                    }
                },onLongClick={openContext(mask)}),contentAlignment=Alignment.Center) {
                if(group) {
                    SharedIcon(if(layer.getBoolean("collapsed"))"folder" else "folder-open",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(if(layer.getBoolean("collapsed")) "expand" else "collapse"),Modifier.size(28.dp))
                    if(layer.getBoolean("pass_through")) SharedIcon("group-pass-through",null,Modifier.align(Alignment.BottomEnd).padding(end=3.dp,bottom=3.dp).size(14.dp).background(colors.input,SquircleShape(2.dp)).padding(1.dp).testTag("layer-group-pass-through-$id"))
                }
                else {
                if(mask || layer.getBoolean("has_thumbnail")) images["$id:$mask"]?.let { Image(it,null,Modifier.size(28.dp).clip(TileShape).testTag("layer-thumbnail-$id-$mask").alpha(if(mask && !layer.getBoolean("mask_enabled")) .4f else 1f)) }
                if(!mask && !layer.optBoolean("selection_layer") && !layer.isNull("content_icon")) SharedIcon(iconName(layer.getString("content_icon")),null,
                    if(layer.getBoolean("has_thumbnail")) Modifier.align(Alignment.BottomEnd).padding(end=3.dp,bottom=3.dp).size(14.dp)
                        .background(colors.input,SquircleShape(2.dp)).padding(1.dp).testTag("layer-type-symbol-$id")
                    else Modifier.size(24.dp),tint=colors.text)
                }
            }
                Spacer(Modifier.matchParentSize().graphicsLayer().drawWithCache {
                    val radius=size.minDimension/2
                    val contour=Path().apply { addSquircle(Rect(Offset.Zero,size),radius,radius,radius,radius) }.asAndroidPath()
                    val transform=android.graphics.Matrix()
                    fun edge(path:Path,outside:Float,inside:Float) {
                        val target=path.asAndroidPath();target.rewind();target.fillType=android.graphics.Path.FillType.EVEN_ODD
                        fun add(inset:Float) {
                            transform.setScale((size.width-2*inset)/size.width,(size.height-2*inset)/size.height,size.width/2,size.height/2)
                            target.addPath(contour,transform)
                        }
                        add(outside);add(inside)
                    }
                    val base=Path().apply { edge(this,0f,1.dp.toPx()) }
                    val outline=Path()
                    onDrawBehind {
                        drawPath(base,if(selected) colors.accent else colors.text.copy(alpha=.1f))
                        val progress=selection.value
                        if(progress>0) {
                            edge(outline,(-4+3*progress).dp.toPx(),(-4+6*progress).dp.toPx())
                            drawPath(outline,colors.accent.copy(alpha=progress))
                        }
                        if(!mask && (attachment || highlight=="attach")) drawRect(colors.accent,style=androidx.compose.ui.graphics.drawscope.Stroke(2*density))
                    }
                })
            }
            }
        }
        thumb(false)
        if(layer.optBoolean("selection_layer")) {
            val load = obj("type" to "selection", "action" to obj("op" to "load_layer", "id" to id, "mode" to "new", "inverted" to false))
            LayerButton(host,"selection-load",layer.getString("load_selection_tooltip"),Modifier.testTag("selection-load-$id"),action=load,size=30.dp)
        }
        if(layer.getBoolean("has_mask")) {
            LayerButton(host,if(layer.getBoolean("mask_linked"))"link" else "unlink",if(layer.getBoolean("mask_linked"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("unlink_mask") else host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("link_mask_to_layer"),
                Modifier.size(10.dp,24.dp),enabled=!layer.getBoolean("locked"),iconSize=10.dp,
                action=obj("type" to "layer","action" to obj("op" to "link_mask","id" to id,"value" to !layer.getBoolean("mask_linked"))))
            thumb(true)
        }
        Column(Modifier.weight(1f).padding(start=6.dp)) {
            if(rename==id && !preview) {
                var name by remember { mutableStateOf(TextFieldValue(layer.getString("label"),TextRange(0,layer.getString("label").length))) }
                val focus=remember { FocusRequester() }; var hadFocus by remember { mutableStateOf(false) }; var done by remember { mutableStateOf(false) }
                fun finish() { if(!done) { done=true; host.layer(if(name.text.isBlank())obj("op" to "cancel_rename") else obj("op" to "rename","id" to id,"name" to name.text)) } }
                BasicTextField(name,{name=it;host.textComposition.update(focus,name,hadFocus)},Modifier.fillMaxWidth().focusRequester(focus).onFocusChanged { if(hadFocus && !it.isFocused)finish(); hadFocus=it.isFocused; host.editingText=it.isFocused;host.textComposition.update(focus,name,hadFocus) },
                    textStyle=LocalTextStyle.current.copy(color=colors.text),singleLine=true,keyboardOptions=KeyboardOptions(imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={if(name.composition==null)finish()}))
                LaunchedEffect(id) { focus.requestFocus() }
                DisposableEffect(id) { onDispose { host.editingText=false;host.textComposition.clear(focus) } }
            } else Text(layer.getString("label"),Modifier.combinedClickable(onClick={select()},onDoubleClick={if(layer.optBoolean("can_rename"))host.layer(obj("op" to "begin_rename","id" to id))},onLongClick={openContext(false)}),
                maxLines=1,overflow=TextOverflow.Ellipsis)
            val meta=layer.getString("description")
            if(meta.isNotEmpty())Text(meta,color=colors.secondary,fontSize=LocalTextStyle.current.fontSize*.83333f,lineHeight=LocalTextStyle.current.lineHeight*.83333f,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
        if(layer.optInt("object_count")>0) {
            val expanded=layer.getBoolean("expanded")
            val action=obj("type" to "object","action" to obj("op" to "expand","layer" to id,"expanded" to !expanded))
            LayerButton(host,"chevron-down",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(if(expanded) "collapse_images" else "expand_images"),
                Modifier.testTag("layer-expand-$id").rotate(if(expanded) 0f else -90f).semantics { if(expanded) collapse { host.dispatch(action); true } else expand { host.dispatch(action); true } },action=action)
        }
        SharedIcon(if(layer.getBoolean("locked"))"lock" else "alpha-lock",null,Modifier.size(12.dp).alpha(if(layer.getBoolean("locked") || layer.getBoolean("alpha_locked"))1f else 0f))
        SharedIcon("grip",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("move_layer"),Modifier.size(16.dp).alpha(if(layer.getBoolean("can_drop_below")) .6f else 0f))
        }
    }
}

@Composable private fun ImageObjectRow(host:CanvasHost,row:JSONObject,depth:Int,images:Map<String,ImageBitmap>,modifier:Modifier=Modifier,highlight:String?=null,
    preview:Boolean=false,context:(Offset)->Unit={},held:(Boolean)->Unit={},cancelContext:()->Unit={},drag:(Offset,Boolean,Boolean)->Unit={_,_,_->},forget:()->Unit={}) {
    val colors=LocalPalette.current
    val copy=host.catalog.getJSONObject("native_copy").getJSONObject("layers")
    val id=row.getLong("id")
    val label=row.getString("label")
    val latest by rememberUpdatedState(row)
    DisposableEffect(id) { onDispose { forget() } }
    var origin by remember { mutableStateOf(Offset.Zero) }
    var press by remember { mutableStateOf(Offset.Zero) }
    var extend by remember { mutableStateOf(false) }
    val focused=LocalWindowInfo.current.isWindowFocused
    val density=LocalDensity.current.density
    val movable=row.getBoolean("editable") && (row.getBoolean("can_raise") || row.getBoolean("can_lower"))
    fun select(add:Boolean=extend)=host.dispatch(obj("type" to "object","action" to obj("op" to "select","id" to id,"extend" to add)))
    Box(modifier.onPreviewKeyEvent { extend=it.keyShiftPressed || it.keyCtrlPressed || it.keyMetaPressed; false }
        .semantics { contentDescription=label; selected=row.getBoolean("selected") }
        .fillMaxWidth().heightIn(min=40.dp).clipToBounds().then(if(preview) Modifier else Modifier.testTag("image-object-row-$id"))
        .onGloballyPositioned { origin=it.boundsInRoot().topLeft }
        .drawWithContent {
            drawContent()
            when(highlight) { "above" -> drawLine(colors.accent,Offset.Zero,Offset(size.width,0f),2*density)
                "below" -> drawLine(colors.accent,Offset(0f,size.height),Offset(size.width,size.height),2*density) }
        }.then(if(preview) Modifier else Modifier.pointerInput(id,focused) {
            if (!focused) return@pointerInput
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial); press=down.position
                val keys=currentEvent.keyboardModifiers
                extend=keys.isShiftPressed || keys.isCtrlPressed || keys.isMetaPressed
                val secondary=currentEvent.buttons.isSecondaryPressed
                val mouse=down.type==PointerType.Mouse
                val directDrag=mouse || down.position.x>=size.width-20*density
                var longPressed=false
                var dragging=false
                var released=false
                var remaining=viewConfiguration.longPressTimeoutMillis
                var eventTime=down.uptimeMillis
                held(true)
                if (secondary) { context(origin+press); down.consume() }
                try { do {
                    val event=if (!longPressed && !dragging && !mouse) withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                        else awaitPointerEvent(PointerEventPass.Initial)
                    if (event==null) { longPressed=true; context(origin+press); continue }
                    val change=event.changes.find { it.id==down.id } ?: break
                    remaining=(remaining-(change.uptimeMillis-eventTime)).coerceAtLeast(1)
                    eventTime=change.uptimeMillis
                    if (!change.pressed && change.isConsumed) break
                    val moved=(change.position-down.position).getDistance()>viewConfiguration.touchSlop
                    if (!secondary && movable && (directDrag || longPressed) && !dragging && change.pressed && moved && latest.getBoolean("editable")) { dragging=true; cancelContext() }
                    if (dragging) { change.consume(); drag(origin+change.position,!change.pressed,false); if(!change.pressed)dragging=false }
                    if (longPressed) change.consume()
                    if (!change.pressed) { released=true; break }
                } while(true) } finally {
                    if(dragging)drag(origin+down.position,true,true)
                    if(!released)cancelContext()
                    held(false)
                }
            }
        }.combinedClickable(onClick={select()},onLongClick={context(origin+press)}))) {
        Row(Modifier.fillMaxWidth().heightIn(min=40.dp)
            .background(if(row.getBoolean("selected")) colors.active else Color.Transparent)
            .alpha(if(row.getBoolean("visible")) 1f else .6f)
            .padding(horizontal=6.dp,vertical=2.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(2.dp)) {
            LayerButton(host,if(row.getBoolean("visible")) "eye" else "eye-hidden",copy.getString(if(row.getBoolean("visible")) "hide_image" else "show_image"),
                Modifier.testTag("image-object-eye-$id"),enabled=row.getBoolean("editable"),
                action=obj("type" to "object","action" to obj("op" to "visibility","id" to id,"visible" to !row.getBoolean("visible"))))
            Spacer(Modifier.width(24.dp))
            Spacer(Modifier.width((depth*8).coerceAtMost(32).dp+3.dp))
            Box(Modifier.size(30.dp).drawWithCache {
                val edge=Path().apply {
                    fillType=PathFillType.EvenOdd
                    for(inset in listOf(0f,1.dp.toPx())) {
                        val radius=size.minDimension/2-inset
                        addSquircle(Rect(inset,inset,size.width-inset,size.height-inset),radius,radius,radius,radius)
                    }
                }
                onDrawWithContent { drawContent(); drawPath(edge,colors.text.copy(alpha=.1f)) }
            },contentAlignment=Alignment.Center) {
                images["$id:false"]?.let { Image(it,null,Modifier.size(28.dp).clip(TileShape).testTag("image-object-thumbnail-$id")) }
                    ?: SharedIcon("image",null,Modifier.size(20.dp).alpha(.5f),tint=colors.text)
            }
            Text(label,Modifier.weight(1f).padding(start=6.dp),maxLines=1,overflow=TextOverflow.Ellipsis)
            SharedIcon("grip",copy.getString("move_image"),Modifier.size(16.dp).alpha(if(movable) .6f else 0f))
        }
    }
}

@Composable private fun LayerConnections(layers:List<JSONObject>,connections:List<JSONObject>,thumbnails:Map<Long,LayerThumbnail>,modifier:Modifier) {
    val colors=LocalPalette.current
    val glyph=sharedIconPainter("effect-link",colors.text)
    var viewport by remember { mutableStateOf(Rect.Zero) }
    Canvas(modifier.clipToBounds().testTag("layer-connections").onGloballyPositioned { viewport=it.boundsInRoot() }) {
        val order=layers.mapIndexed { index,row -> row.getLong("id") to index }.toMap()
        val visible=layers.filter { thumbnails[it.getLong("id")]?.anchor?.overlaps(viewport)==true }
        val anchor=visible.firstOrNull() ?: return@Canvas
        val column=thumbnails[anchor.getLong("id")]!!.anchor.left-viewport.left-(anchor.getInt("depth")*8).coerceAtMost(24)*density
        val first=order[visible.first().getLong("id")]!!
        val last=order[visible.last().getLong("id")]!!
        fun endpoint(id:Long,bottom:Boolean):Float? {
            thumbnails[id]?.anchor?.let { return (if(bottom) it.bottom else it.top)-viewport.top }
            return order[id]?.let { if(it<first)0f else if(it>last)size.height else null }
        }
        for(connection in connections) {
            val effect=connection.getString("kind")=="effect"
            val from=connection.getLong("from");val to=connection.getLong("to")
            val top=endpoint(from,effect) ?: continue
            val bottom=endpoint(to,!effect) ?: continue
            if(bottom<=top || bottom<0 || top>size.height)continue
            val x=column+(connection.getInt("depth")*8).coerceAtMost(24)*density
            if(!effect) drawLine(colors.relationship,Offset(x-3.5f*density,top),Offset(x-3.5f*density,bottom),2*density)
            else if(order[to]==order[from]?.plus(1)) {
                val center=x+15*density;val y=(top+bottom)*.5f
                if(y-top>6*density)drawLine(colors.text,Offset(center,top),Offset(center,y-6*density),density)
                if(bottom-y>6*density)drawLine(colors.text,Offset(center,y+6*density),Offset(center,bottom),density)
                translate(center-6*density,y-6*density) { with(glyph) { draw(Size(12*density,12*density)) } }
            }
        }
    }
}
