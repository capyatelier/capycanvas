package art.capycanvas

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
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
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
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

internal fun CanvasHost.layer(action: JSONObject) = dispatch(obj("type" to "layer", "action" to action))
private data class LayerThumbnail(val hit: Rect, val anchor: Rect)
private data class LayerDrag(val id: Long, val pointer: Offset, val target: Long? = null, val fraction: Float = 0f, val surface: String = "row", val hintTarget: Long? = null, val position: String? = null, val effectOwner: Long? = null)
private fun iconName(name: String) = name.removePrefix("layer-").removeSuffix("-symbolic")
internal val LayerEdge = 6.dp

/** Transient native gesture state, shared by retained layer panels in this window. */
internal class LayerSwipe {
    var owner by mutableStateOf<Any?>(null)
    var offset by mutableFloatStateOf(0f)
    var tracking by mutableStateOf(false)
    var bounds = Rect.Zero
    fun close() { owner=null; offset=0f; tracking=false }
}

/** The native view translates the shared layer model; no layer policy lives here. */
@Composable internal fun LayerPanel(host: CanvasHost, state: JSONObject, modifier: Modifier = Modifier, listState: LazyListState = rememberLazyListState(), full: Boolean = true, onContent: (PanelContentSize) -> Unit = {}) {
    val colors = LocalPalette.current
    val density = LocalDensity.current
    val bubbles = LocalDock.current?.bubbles
    val view = state.getJSONObject("layer_tools")
    val active = view.objectOrNull("editing_layer")
    val controls = view.getJSONObject("controls")
    val layers = state.array("layers").objects()
    val addPaper = view.objectOrNull("add_paper")
    var headerHeight by remember { mutableFloatStateOf(0f) }
    var footerHeight by remember { mutableFloatStateOf(0f) }
    var footerWidth by remember { mutableFloatStateOf(Float.MAX_VALUE) }
    val footer = view.array("footer").objects()
    val overflow = footer.filter { footerWidth + .5f < it.getDouble("min_width") }
    fun shown(button: String) = overflow.none { it.getString("button") == button }
    var rowHeight by remember { mutableFloatStateOf(40f) }
    val fixedHeight = if (full) headerHeight + footerHeight else 0f
    val measured = if (!full || headerHeight > 0f && footerHeight > 0f)
        PanelContentSize(fixedHeight + (layers.size + if (addPaper != null) 1 else 0) * rowHeight, fixedHeight, rowHeight) else null
    SideEffect { measured?.let(onContent) }
    val currentLayers by rememberUpdatedState(layers)
    val list = listState
    val epoch = state.getJSONObject("document_file").optLong("epoch")
    LaunchedEffect(epoch, active?.optLong("id")) {
        val index = layers.indexOfFirst { it.optLong("id") == active?.optLong("id") }
        val visible = list.layoutInfo.visibleItemsInfo.find { it.index == index }
        if (index >= 0 && (visible == null || visible.offset < list.layoutInfo.viewportStartOffset
            || visible.offset + visible.size > list.layoutInfo.viewportEndOffset)) list.scrollToItem(index)
    }
    val images = rememberLayerThumbnails(host, epoch) {
        val visible = list.layoutInfo.visibleItemsInfo.map { it.key }.toSet()
        currentLayers.filter { it.getLong("id") in visible }.flatMap { layer ->
            listOf(false, true).mapNotNull { mask ->
                if (if (mask) !full || !layer.getBoolean("has_mask") else !layer.getBoolean("has_thumbnail")) null
                else LayerThumbnailTarget("${layer.getLong("id")}:$mask", layer.getLong(if (mask) "mask_id" else "id"), layer.getLong(if (mask) "mask_revision" else "paint_revision"))
            }
        }
    }
    val bounds = remember { mutableMapOf<Long, Rect>() }
    val touchSurface = remember { layerTouchSurfaces.incrementAndGet() }
    var listOrigin by remember { mutableStateOf(Offset.Zero) }
    val thumbnails = remember { mutableStateMapOf<Long, LayerThumbnail>() }
    var dragGeneration by remember { mutableIntStateOf(0) }
    var filterPoint by remember { mutableStateOf(Offset.Zero) }
    var panelOrigin by remember { mutableStateOf(Offset.Zero) }
    var drag by remember { mutableStateOf<LayerDrag?>(null) }
    var menu by remember { mutableStateOf<JSONObject?>(null) }
    var menuRequest by remember { mutableStateOf<JSONObject?>(null) }
    var menuGeneration by remember { mutableIntStateOf(0) }
    LaunchedEffect(epoch) { host.layerSwipe.close(); dragGeneration++; drag = null; menuGeneration++; menu = null; menuRequest = null }
    var contactHeld by remember { mutableStateOf(false) }
    var menuPoint by remember { mutableStateOf(Offset.Zero) }
    fun contextMenu(layer: JSONObject, mask: Boolean, point: Offset, extra: List<JSONObject> = emptyList()) {
        if (drag != null) return
        val request = ++menuGeneration
        val id = layer.getLong("id")
        host.layer(obj("op" to "context", "id" to id, "mask" to mask))
        val query = obj("type" to "layer_menu", "id" to id, "mask" to mask)
        host.query(query) {
            if (request == menuGeneration && host.menuEpoch() == epoch && drag == null) {
                val shown = it as? JSONObject
                if (shown != null && extra.isNotEmpty()) {
                    val sections = JSONArray().put(JSONArray(extra.map { slot -> slot.getJSONObject("item") }))
                    val old = shown.array("sections")
                    for (i in 0 until old.length()) sections.put(old.get(i))
                    shown.put("sections", sections)
                }
                menuRequest = query; menu = shown; menuPoint = point - panelOrigin
            }
        }
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
    Box(modifier.fillMaxSize().onGloballyPositioned { panelOrigin = it.boundsInRoot().topLeft }) {
        Column(Modifier.fillMaxSize()) {
            if (full) Column(Modifier.wrapContentHeight(unbounded = true).onSizeChanged { headerHeight = it.height / density.density }.padding(horizontal = LayerEdge, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(panelMetrics().spacing)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(panelMetrics().spacing)) {
                    var blendMenu by remember { mutableStateOf<JSONObject?>(null) }
                    var blendRequest by remember { mutableStateOf<JSONObject?>(null) }
                    var blendGeneration by remember { mutableIntStateOf(0) }
                    LaunchedEffect(epoch) { blendGeneration++; blendMenu = null; blendRequest = null }
                    Box(Modifier.weight(1f)) {
                        Row(Modifier.fillMaxWidth().height(panelMetrics().height).background(colors.input,ControlShape).testTag("layer-blend")
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
                Row(horizontalArrangement=Arrangement.spacedBy(panelMetrics().compound)) {
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
                LazyColumn(Modifier.fillMaxSize().mouseScrollbar(list).testTag("layer-rows")
                    .onGloballyPositioned { listOrigin = it.boundsInRoot().topLeft }
                    .pointerInput(bubbles, list) {
                        awaitEachGesture {
                            val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                            if (!down.isConsumed && list.layoutInfo.visibleItemsInfo.none { down.position.y >= it.offset && down.position.y < it.offset + it.size }) bubbles?.cardBackground(down)
                        }
                    }
                    .layerTouchInput(host,touchSurface,epoch,{listOrigin},bounds) {
                        host.layerSwipe.close();dragGeneration++;drag=null;menuGeneration++;menu=null;menuRequest=null;contactHeld=false
                    },state=list) {
                    for (layer in layers) {
                    item(key=layer.getLong("id")) {
                        val id=layer.getLong("id")
                        val highlight=drag?.takeIf { it.hintTarget==id }?.position
                        LayerRow(host,layer,view.optLong("rename_layer",-1),images,Modifier.imageDropTarget(host,id).onSizeChanged { rowHeight = it.height / density.density }.onGloballyPositioned { bounds[id]=it.boundsInRoot() },highlight,full=full,attachment=drag?.effectOwner==id,
                            context={mask,point -> contextMenu(layer,mask,point)},
                            contentBounds={rect,shift -> if(rect==null) { thumbnails.remove(id);bounds.remove(id) } else thumbnails[id]=LayerThumbnail(rect,rect.translate(Offset(shift.roundToInt().toFloat(),0f)))},
                            held={contactHeld=it},cancelContext={menuGeneration++; menu=null},
                            drag={point,finished,cancelled -> moveLayer(id,point,finished,cancelled)})
                    }
                    }
                    if (addPaper != null) item(key="add-paper") {
                        Row(Modifier.fillMaxWidth().heightIn(min=40.dp).clipToBounds().testTag("layer-add-paper")
                            .onSizeChanged { rowHeight = it.height / density.density }
                            .clickable(role=Role.Button) { host.dispatch(obj("type" to "effect", "action" to addPaper.getJSONObject("action"))) }
                            .padding(horizontal=6.dp,vertical=2.dp), verticalAlignment=Alignment.CenterVertically,
                            horizontalArrangement=Arrangement.spacedBy(panelMetrics().group,Alignment.CenterHorizontally)) {
                            SharedIcon(iconName(addPaper.getString("icon")),null,Modifier.size(16.dp))
                            Text(addPaper.getString("label"),Modifier.weight(1f,fill=false),maxLines=1,overflow=TextOverflow.Ellipsis)
                        }
                    }
                }
                LayerConnections(layers,view.array("connections").objects(),thumbnails,Modifier.matchParentSize())
            }
            if (full) Row(Modifier.fillMaxWidth().wrapContentHeight(unbounded = true).onSizeChanged { footerHeight = it.height / density.density; footerWidth = it.width / density.density }.padding(horizontal=LayerEdge,vertical=4.dp),horizontalArrangement=Arrangement.spacedBy(panelMetrics().compound)) {
                val copy = host.catalog.getJSONObject("native_copy").getJSONObject("layers")
                if (shown("new_layer")) LayerButton(host,"add-layer",copy.getString("new_layer"),action=obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
                if (shown("new_vector_layer")) LayerButton(host,"add-vector-layer",copy.getString("new_vector_layer"),action=obj("type" to "invoke","command" to "new_vector_layer"))
                if (shown("new_group")) LayerButton(host,"folder",copy.getString("new_group"),action=obj("type" to "layer","action" to obj("op" to "new","group" to true,"clipped" to false)))
                if (shown("new_selection_layer")) LayerButton(host,"selection-brush",copy.getString("new_selection_layer"),action=obj("type" to "invoke","command" to "new_selection_layer"))
                if (shown("add_mask")) LayerButton(host,"mask",copy.getString("add_mask"),enabled=controls.getBoolean("mask"),action=active?.let { obj("type" to "layer","action" to obj("op" to "add_mask","id" to it.getLong("id"),"replace" to false)) })
                if (shown("add_filter")) LayerButton(host,"add-filter",copy.getString("add_filter"),Modifier.testTag("layer-add-filter").onGloballyPositioned { filterPoint = it.boundsInRoot().topLeft - panelOrigin },enabled=view.optJSONObject("add_filter")!=null) {
                    menuGeneration++; menuRequest = null; menu = view.optJSONObject("add_filter"); menuPoint = filterPoint
                }
                if (shown("import_image")) LayerButton(host,"image",copy.getString("import_image"), action=obj("type" to "invoke", "command" to "import_image"))
                if (shown("delete")) LayerButton(host,"delete",copy.getString("delete_selected"),enabled=view.getBoolean("can_delete"),action=obj("type" to "layer","action" to obj("op" to "delete_selected")))
                Spacer(Modifier.weight(1f))
                LayerButton(host,"more-small",copy.getString("actions"),enabled=active!=null||overflow.isNotEmpty()) {
                    val point = panelOrigin+Offset(0f,40f)
                    if (active != null) contextMenu(active,active.getBoolean("mask_selected"),point,overflow)
                    else { menuGeneration++; menuRequest = null; menu = obj("title" to copy.getString("actions"), "sections" to JSONArray().put(JSONArray(overflow.map { it.getJSONObject("item") }))); menuPoint = point - panelOrigin }
                }
            }
        }
        if (!view.isNull("opacity_popup") && view.optLong("opacity_surface") == touchSurface && active != null) {
            val target = view.getLong("opacity_popup")
            Popup(alignment=Alignment.Center,properties=PopupProperties(focusable=true),
                onDismissRequest={host.layer(obj("op" to "close_opacity"))}) {
                Surface(color=colors.panel,shadowElevation=8.dp,shape=androidx.compose.foundation.shape.RoundedCornerShape(8.dp)) {
                    NumericSetting(host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("opacity"),
                        active.number("opacity"),host.catalog.getJSONObject("layer_opacity"),
                        Modifier.width(240.dp).padding(12.dp).testTag("layer-gesture-opacity"),enabled=controls.getBoolean("opacity")) {
                        host.dispatch(obj("type" to "set_layer_opacity","id" to target,"opacity" to it))
                    }
                }
            }
        }
        drag?.let { d -> layers.find { it.getLong("id")==d.id }?.let { layer ->
            LayerRow(host,layer,-1,images,Modifier.testTag("layer-drag-preview").offset { IntOffset(0,(d.pointer.y-panelOrigin.y-20*density.density).roundToInt()) }
                .alpha(.7f).background(colors.panel),preview=true,full=full)
        } }
        if (menu!=null) Box(Modifier.offset { IntOffset(menuPoint.x.roundToInt(),menuPoint.y.roundToInt()) }.size(1.dp)) {
            WorkspaceMenu(host,menu!!,preserveContact=contactHeld, copy = { menuRequest?.let { host.menuCopy(it) } }) { menuGeneration++; menu=null; menuRequest=null }
        }
    }
}

@Composable private fun LayerButton(host:CanvasHost,icon:String,label:String,modifier:Modifier=Modifier,enabled:Boolean=true,selected:Boolean=false,subtle:Boolean=false,
    action:JSONObject?=null,size:Dp=panelMetrics().height,iconSize:Dp=16.dp,onClick:()->Unit={action?.let { host.dispatch(it) }}) {
    val colors=LocalPalette.current
    val content: @Composable () -> Unit = {
    Box(Modifier.size(size).clip(ControlShape).alpha(if(enabled)1f else .4f)
        .background(if(selected) { if(subtle) colors.text.copy(alpha=.12f) else colors.active } else Color.Transparent)
        .clickable(enabled=enabled,onClick=onClick),contentAlignment=Alignment.Center) { SharedIcon(icon,label,Modifier.size(iconSize)) }
    }
    if(action!=null) ActionTip(host,label,action,modifier,content) else HoverTip(label,modifier,content=content)
}

@Composable private fun LayerRow(host:CanvasHost,layer:JSONObject,rename:Long,images:Map<String,ImageBitmap>,modifier:Modifier=Modifier,highlight:String?=null,
    preview:Boolean=false,full:Boolean=true,attachment:Boolean=false,context:(Boolean,Offset)->Unit={_,_->},contentBounds:(Rect?,Float)->Unit={_,_->},held:(Boolean)->Unit={},cancelContext:()->Unit={},drag:(Offset,Boolean,Boolean)->Unit={_,_,_->}) {
    val colors=LocalPalette.current
    val id=layer.getLong("id")
    val label=layer.getString("label")
    val rowCaption=remember(label, host.languageTag) { JSONObject(Native.nativeCaption(obj("type" to "layer_row", "title" to label).toString(), host.languageTag)).getString("text") }
    val latest by rememberUpdatedState(layer)
    val currentContext by rememberUpdatedState(context)
    val currentDrag by rememberUpdatedState(drag)
    val currentHeld by rememberUpdatedState(held)
    val currentCancelContext by rememberUpdatedState(cancelContext)
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
    val swipeActions=layer.array("swipe_actions").objects()
    val canSwipe=swipeActions.any { it.optBoolean("enabled") }
    val swipeOwner=remember { Any() }
    var rowBounds by remember { mutableStateOf(Rect.Zero) }
    val shift by animateFloatAsState(if(swipe.owner===swipeOwner) swipe.offset else 0f,
        tween(if(swipe.owner===swipeOwner && swipe.tracking) 0 else 150),label="Layer swipe")
    val atRest=shift==0f && (swipe.owner!==swipeOwner || swipe.offset==0f)
    val resting by rememberUpdatedState(atRest)
    var gripBounds by remember { mutableStateOf(Rect.Zero) }
    DisposableEffect(swipeOwner) { onDispose { if(swipe.owner===swipeOwner)swipe.close() } }
    LaunchedEffect(focused,canSwipe) {
        if((!focused || !canSwipe) && swipe.owner===swipeOwner)swipe.close()
    }
    val density=LocalDensity.current.density
    var extendSelection by remember { mutableStateOf(false) }
    var toggleSelection by remember { mutableStateOf(false) }
    fun select(toggle:Boolean=toggleSelection) = host.layer(obj("op" to "select_row","id" to id,"extend" to extendSelection,"toggle" to toggle))
    fun openContext(mask:Boolean) {
        if (!focused || (contactActive && !contactMenus)) return
        if (!contactActive || (holdEligible && !longPressed)) { longPressed=true; currentContext(mask,origin+press) }
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
        }.then(if(preview || rename==id) Modifier else Modifier.edgeControl().pointerInput(id,focused,full) {
            if (!focused) return@pointerInput
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial); press=down.position
                if (down.isConsumed) return@awaitEachGesture
                extendSelection=currentEvent.keyboardModifiers.isShiftPressed
                toggleSelection=currentEvent.keyboardModifiers.isCtrlPressed || currentEvent.keyboardModifiers.isMetaPressed
                if (toggleSelection) return@awaitEachGesture
                longPressed=false
                contactActive=true
                holdEligible=true
                val row=latest
                val kind=pointerKind(down.type)
                val handle=full && resting && gripBounds.contains(origin+down.position)
                fun pickup(row:JSONObject)=if(handle)row.getBoolean("drag_from_handle") else kind in row.array("drag_from_body").values()
                var directDrag=resting && pickup(row)
                if(handle)down.consume()
                val secondary=currentEvent.buttons.isSecondaryPressed
                contactMenus=down.type!=PointerType.Mouse || secondary
                var dragging=false
                var swiping=false
                val swipeStart=if(swipe.owner===swipeOwner)swipe.offset else 0f
                var released=false
                var remaining=viewConfiguration.longPressTimeoutMillis
                var eventTime=down.uptimeMillis
                currentHeld(true)
                if (secondary) { openContext(full && row.getBoolean("has_mask") && maskBounds.contains(origin+press)); down.consume() }
                try { do {
                    val event=if (holdEligible && !longPressed && !dragging) {
                        withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                    } else awaitPointerEvent(PointerEventPass.Initial)
                    if (event==null) {
                        if (contactMenus) openContext(full && row.getBoolean("has_mask") && maskBounds.contains(origin+press))
                        else longPressed=true // Mouse holds suppress clicks without opening menus.
                        continue
                    }
                    val change=event.changes.find { it.id==down.id } ?: break
                    remaining=(remaining-(change.uptimeMillis-eventTime)).coerceAtLeast(1)
                    eventTime=change.uptimeMillis
                    if (change.isConsumed) break
                    val moved=(change.position-down.position).getDistance()>viewConfiguration.touchSlop
                    if (kind!="mouse" && !handle && !longPressed && moved && holdEligible) {
                        holdEligible=false
                        val delta=change.position-down.position
                        val allowed=if(delta.x<0)row.array("swipe_actions").objects().any { it.optBoolean("enabled") } else swipeStart>0 || !row.isNull("right_swipe")
                        if(allowed && kotlin.math.abs(delta.x)>kotlin.math.abs(delta.y)) {
                            swiping=true; swipe.owner=swipeOwner; swipe.bounds=rowBounds; swipe.tracking=true
                            currentCancelContext()
                        }
                    }
                    if(swiping) {
                        change.consume()
                        val minimum=if(swipeStart==0f && !row.isNull("right_swipe"))-72*density else 0f
                        val maximum=(row.array("swipe_actions").length()*72*density).coerceAtMost(size.width.toFloat())
                        swipe.offset=(swipeStart-(change.position.x-down.position.x)).coerceIn(minimum,maximum)
                        if(!change.pressed) { released=true; break }
                        continue
                    }
                    if (directDrag && !pickup(latest)) {
                        directDrag=false
                        if (dragging) { currentDrag(origin+down.position,true,true); dragging=false }
                    }
                    if (!secondary && directDrag && resting && !dragging && change.pressed &&
                        moved) { dragging=true; holdEligible=false }
                    if (dragging) { change.consume(); currentDrag(origin+change.position,!change.pressed,false); if(!change.pressed)dragging=false }
                    if (longPressed) change.consume()
                    if (!change.pressed) { released=true; break }
                } while(true) } finally {
                    if(swiping) {
                        val toggle=released && swipeStart==0f && swipe.offset<=-72*density*.4f
                        swipe.tracking=false
                        val threshold=if(swipeStart>0)swipeStart-72*density*.4f else 72*density*.4f
                        if(released && swipe.offset>=threshold)swipe.offset=(latest.array("swipe_actions").length()*72*density).coerceAtMost(size.width.toFloat()) else swipe.close()
                        if(toggle)latest.objectOrNull("right_swipe")?.let { host.layer(it) }
                    }
                    if(dragging)currentDrag(origin+down.position,true,true)
                    if(!released)currentCancelContext()
                    longPressed=false; holdEligible=false; contactActive=false; contactMenus=true; currentHeld(false)
                }
            }
        }.combinedClickable(onClick={select()},onLongClick={openContext(false)}))
        ) {
        if(shift>0f) Box(Modifier.matchParentSize(),contentAlignment=Alignment.CenterEnd) {
            Box(Modifier.width((shift/density).dp).fillMaxHeight().clipToBounds(),contentAlignment=Alignment.CenterEnd) {
                Row(Modifier.wrapContentWidth(Alignment.End,unbounded=true).width((swipeActions.size*72f).coerceAtMost(rowBounds.width/density).dp).fillMaxHeight()) {
                    swipeActions.forEach { model ->
                        val actionId=model.getString("id")
                        val fill=when(actionId) {"delete"->Color(0xffc62828);"duplicate"->colors.text.copy(alpha=.24f);else->colors.text.copy(alpha=.16f)}
                        Box(Modifier.weight(1f).fillMaxHeight().background(fill).padding(horizontal=4.dp)
                            .testTag("layer-$actionId-$id").clickable(enabled=model.getBoolean("enabled")) {
                                swipe.close(); host.layer(model.getJSONObject("action"))
                            },contentAlignment=Alignment.Center) { Text(model.getString("label"),color=if(actionId=="delete")Color.White else colors.text,maxLines=1,overflow=TextOverflow.Ellipsis) }
                    }
                }
            }
        }
        Row(Modifier.fillMaxWidth().heightIn(min=40.dp).offset { IntOffset(-shift.roundToInt(),0) }
            .background(if(layer.getBoolean("selected")) colors.active else Color.Transparent)
            .padding(horizontal=LayerEdge,vertical=2.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(2.dp)) {
        LayerButton(host,if(layer.getBoolean("visible") && !layer.getBoolean("visibility_blocked")) "eye" else "eye-hidden",if(layer.optBoolean("selection_layer"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(if(layer.getBoolean("visible")) "hide_selection" else "show_selection") else if(layer.getBoolean("visible"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("hide") else host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("show"),
            Modifier.testTag("layer-eye-$id").alpha(if(layer.getBoolean("visibility_blocked")) .35f else 1f),action=obj("type" to "set_layer_visibility","id" to id,"visible" to !layer.getBoolean("visible")))
        if(full) LayerButton(host,iconName(layer.getString("selection_icon")),host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("select_row_help"),
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
            ActionTip(host,label,obj("type" to "layer","action" to operation),Modifier.size(30.dp).then(if(preview) Modifier else if(mask) Modifier.testTag("layer-mask-$id") else Modifier.testTag("layer-content-$id").onGloballyPositioned { contentBounds(it.boundsInRoot(),shift) })) {
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
                ThumbnailEdge(selected,{ selection.value },!mask && (attachment || highlight=="attach"))
            }
            }
        }
        thumb(false)
        if(full && layer.optBoolean("selection_layer")) {
            val load = obj("type" to "selection", "action" to obj("op" to "load_layer", "id" to id, "mode" to "new", "inverted" to false))
            LayerButton(host,"selection-load",layer.getString("load_selection_tooltip"),Modifier.testTag("selection-load-$id"),action=load,size=30.dp)
        }
        if(full && layer.getBoolean("has_mask")) {
            LayerButton(host,if(layer.getBoolean("mask_linked"))"link" else "unlink",if(layer.getBoolean("mask_linked"))host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("unlink_mask") else host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("link_mask_to_layer"),
                Modifier.size(10.dp,panelMetrics().height),enabled=!layer.getBoolean("locked"),iconSize=10.dp,
                action=obj("type" to "layer","action" to obj("op" to "link_mask","id" to id,"value" to !layer.getBoolean("mask_linked"))))
            thumb(true)
        }
        layer.optJSONObject("paper")?.takeIf { full }?.let { paper ->
            val windowed=inSeparateWindow()
            val choose=host.catalog.getJSONObject("native_copy").getJSONObject("paper").getString("choose")
            HoverTip(choose,Modifier.size(30.dp)) {
                Box(Modifier.fillMaxSize()) {
                    Box(Modifier.fillMaxSize().clip(TileShape).combinedClickable(onClick={
                        host.layer(obj("op" to "select","id" to id,"mask" to false))
                        if(paper.optBoolean("editable")) host.textureDialog=TextureDialogRequest(id,windowed)
                    },onLongClick={openContext(false)}).semantics { contentDescription=choose; stateDescription=paper.getString("label") }
                        .then(if(preview) Modifier else Modifier.testTag("layer-texture-$id")),contentAlignment=Alignment.Center) {
                        PaperPreview(host,paper.paperId(),28.dp)
                    }
                    ThumbnailEdge(false,{ 0f },false)
                }
            }
        }
        Column(Modifier.weight(1f).padding(start=6.dp)) {
            if(rename==id && !preview) {
                var name by remember { mutableStateOf(TextFieldValue(layer.getString("label"),TextRange(0,layer.getString("label").length))) }
                val focus=remember { FocusRequester() }; var hadFocus by remember { mutableStateOf(false) }; var done by remember { mutableStateOf(false) }
                fun finish(cancel: Boolean = false) { if(!done) { done=true; host.layer(if(cancel || name.text.isBlank())obj("op" to "cancel_rename") else obj("op" to "rename","id" to id,"name" to name.text)) } }
                BasicTextField(name,{name=it;host.textComposition.update(focus,name,hadFocus)},Modifier.edgeControl().fillMaxWidth().textCursorRegion().testTag("layer-name-editor-$id").focusRequester(focus).onPreviewKeyEvent { event ->
                    val key = event.nativeKeyEvent
                    if (key.keyCode == android.view.KeyEvent.KEYCODE_ESCAPE) {
                        val owned=name.composition != null || host.textComposition.owns(key)
                        if (!owned && key.action == android.view.KeyEvent.ACTION_DOWN) finish(true)
                        true
                    } else false
                }.onFocusChanged { if(hadFocus && !it.isFocused)finish(); hadFocus=it.isFocused; host.editingText=it.isFocused;host.textComposition.update(focus,name,hadFocus) },
                    textStyle=LocalTextStyle.current.copy(color=colors.text),singleLine=true,keyboardOptions=KeyboardOptions(imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={if(name.composition==null)finish()}))
                LaunchedEffect(id) { focus.requestFocus() }
                DisposableEffect(id) { onDispose { host.editingText=false;host.textComposition.clear(focus) } }
            } else Text(layer.getString("label"),Modifier.combinedClickable(onClick={select()},onDoubleClick={if(layer.optBoolean("can_rename"))host.layer(obj("op" to "begin_rename","id" to id))},onLongClick={openContext(false)}),
                maxLines=1,overflow=TextOverflow.Ellipsis)
            val meta=layer.getString("description")
            if(full && meta.isNotEmpty())Text(meta,color=colors.secondary,fontSize=LocalTextStyle.current.fontSize*.83333f,lineHeight=LocalTextStyle.current.lineHeight*.83333f,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
        SharedIcon(if(layer.getBoolean("locked"))"lock" else "alpha-lock",null,Modifier.size(12.dp).alpha(if(layer.getBoolean("locked") || layer.getBoolean("alpha_locked"))1f else 0f))
        if(full && layer.getBoolean("can_drop_below")) Box(Modifier.size(16.dp,32.dp).testTag("layer-grip-$id")
            .graphicsLayer {
                val maximum=if(shift>0f) (swipeActions.size*72*density).coerceAtMost(rowBounds.width).coerceAtLeast(1f) else 72*density
                alpha=1f-(kotlin.math.abs(shift)/maximum).coerceIn(0f,1f)
            }
            .onGloballyPositioned { gripBounds=it.boundsInRoot() },contentAlignment=Alignment.Center) {
            SharedIcon("grip",host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString("move_layer"),Modifier.size(16.dp).alpha(.6f))
        }
        }
    }
}

@Composable private fun BoxScope.ThumbnailEdge(selected:Boolean,selection:()->Float,attached:Boolean) {
    val colors=LocalPalette.current
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
            val progress=selection()
            if(progress>0) {
                edge(outline,(-4+3*progress).dp.toPx(),(-4+6*progress).dp.toPx())
                drawPath(outline,colors.accent.copy(alpha=progress))
            }
            if(attached) drawRect(colors.accent,style=androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
        }
    })
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
