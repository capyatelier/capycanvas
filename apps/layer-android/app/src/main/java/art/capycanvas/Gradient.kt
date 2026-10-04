package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Surface
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.roundToInt

@Composable private fun GradientPreview(host: CanvasHost, gradient: JSONObject, modifier: Modifier) {
    val palette=LocalPalette.current
    var size by remember { mutableStateOf(IntSize.Zero) }
    val panel=host.snapshot?.objectOrNull("color_panel")
    val request=if(size.width>0 && size.height>0 && panel!=null) obj("type" to "gradient", "gradient" to gradient,
        "document_space" to panel.getString("rgb_space"), "rendition" to panel.objectOrNull("rendition"),
        "image" to obj("size" to JSONArray(listOf(size.width.coerceAtMost(2048),size.height.coerceAtMost(64))), "depth" to panel.getString("document_depth"))).toString() else null
    val bitmap by produceState<ImageBitmap?>(null,request) {
        if(request==null) {value=null;return@produceState}
        value=withContext(Dispatchers.Default) {
            val image=JSONObject(Native.colorUi(request,host.languageTag));val extent=image.getJSONArray("size");val pixels=image.getJSONArray("argb")
            Bitmap.createBitmap(IntArray(pixels.length()){pixels.getLong(it).toInt()},extent.getInt(0),extent.getInt(1),Bitmap.Config.ARGB_8888).asImageBitmap()
        }
    }
    Canvas(modifier.onSizeChanged {size=it}.testTag("gradient-preview")) {
        val cell=5.dp.toPx()
        for(y in 0..(this.size.height/cell).toInt()) for(x in 0..(this.size.width/cell).toInt())
            drawRect(if((x+y)%2==0)palette.checkerLight else palette.checkerDark,Offset(x*cell,y*cell),Size(cell,cell))
        bitmap?.let {drawImage(it,dstSize=IntSize(this.size.width.roundToInt(),this.size.height.roundToInt()),filterQuality=FilterQuality.None)}
    }
}

@Composable internal fun GradientControl(host: CanvasHost, control: JSONObject, enabled: Boolean=true, dispatch: (JSONObject)->Unit=host::dispatch) {
    val metadata=control.getJSONObject("gradient")
    val destination=metadata.getJSONObject("destination")
    key(host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optString("epoch"),destination.getString("kind"),destination.optString("layer"),destination.optString("key")) {
        val gradient=control.getJSONObject("value").getJSONObject("value")
        val stops=gradient.getJSONArray("stops").objects()
        val copy=host.catalog.getJSONObject("native_copy").getJSONObject("color")
        val palette=LocalPalette.current
        var selected by remember {mutableIntStateOf(0)}
        val index=selected.coerceIn(stops.indices)
        val current by rememberUpdatedState(stops)
        val currentIndex by rememberUpdatedState(index)
        val sender by rememberUpdatedState(dispatch)
        val currentDestination by rememberUpdatedState(destination)
        var gestureDestination by remember {mutableStateOf<JSONObject?>(null)}
        var held by remember {mutableStateOf<Key?>(null)}
        var dragging by remember {mutableStateOf(false)}
        var numericOwner by remember {mutableStateOf<Int?>(null)}
        val focus=remember {FocusRequester()}
        fun send(edit:JSONObject,phase:String?=null) {
            if(phase=="down")gestureDestination=currentDestination
            val owner=if(phase==null)currentDestination else gestureDestination?:return
            val action=obj("op" to "gradient","target" to owner,"edit" to edit)
            if(phase=="up"||phase=="cancel")gestureDestination=null
            sender(obj("type" to "effect","action" to if(phase==null)action else obj("op" to "gesture","phase" to phase,"action" to action)))
        }
        fun stop(position:Float,i:Int?=currentIndex)=obj("kind" to "stop","index" to i,"position" to position,"color" to null,"remove" to false)
        fun cancel() {if(dragging||held!=null){dragging=false;held=null;send(obj("kind" to "reset"),"cancel")}}
        DisposableEffect(Unit) {onDispose {cancel();if(host.pointControlFocus===focus)host.pointControlFocus=null}}
        @Composable fun icon(name:String,label:String,id:String,active:Boolean=enabled,click:()->Unit) {
            HoverTip(label) {Box(Modifier.size(32.dp).clip(ControlShape).testTag("gradient-$id").clickable(enabled=active,onClick=click),contentAlignment=Alignment.Center) {SharedIcon(name,label,Modifier.size(20.dp))}}
        }
        Column(Modifier.fillMaxWidth().testTag("gradient-editor"),verticalArrangement=Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(6.dp),verticalAlignment=Alignment.CenterVertically) {
                val modes=metadata.getJSONArray("interpolations").values().map {it as JSONArray}
                Box(Modifier.weight(1f).testTag("gradient-interpolation")) {PropertyChoice(metadata.getString("interpolation_label"),modes.map {it.getString(1)},modes.indexOfFirst {it.getString(0)==gradient.getString("interpolation")},enabled) {send(obj("kind" to "interpolation","value" to modes[it].getString(0)))}}
                icon("flip-horizontal",metadata.getString("reverse_label"),"reverse") {selected=stops.lastIndex-index;send(obj("kind" to "reverse"))}
                icon("reset",copy.getString("reset_gradient"),"reset") {send(obj("kind" to "reset"))}
            }
            Box(Modifier.fillMaxWidth().height(44.dp).testTag("effect-gradient").focusRequester(focus)
                .onFocusChanged {
                    if(it.isFocused)host.pointControlFocus=focus
                    else if(host.pointControlFocus===focus){host.pointControlFocus=null;cancel()}
                }.onKeyEvent {event ->
                    if(!enabled||host.textComposition.owns(event.nativeKeyEvent)||(event.type==KeyEventType.KeyDown&&(event.isCtrlPressed||event.isMetaPressed||event.isAltPressed)))return@onKeyEvent false
                    when(event.key) {
                        Key.Escape -> if(dragging||held!=null){cancel();true}else false
                        Key.Delete,Key.Backspace -> {if(event.type==KeyEventType.KeyDown){cancel();send(stop(0f).put("remove",true))};true}
                        Key.DirectionLeft,Key.DirectionRight -> {
                            if(event.type==KeyEventType.KeyDown){val phase=if(held==null)"down" else "move";held=event.key;send(obj("kind" to "position","index" to currentIndex,"operation" to obj("type" to "step","steps" to (if(event.key==Key.DirectionLeft)-1 else 1)*(if(event.isShiftPressed)10 else 1))),phase)}
                            else if(held==event.key){held=null;send(obj("kind" to "position","index" to currentIndex,"operation" to obj("type" to "step","steps" to 0)),"up")};true
                        }
                        else -> false
                    }
                }.focusable().pointerInput(enabled) {
                    if(!enabled)return@pointerInput
                    awaitEachGesture {
                        val down=awaitFirstDown();down.consume();focus.requestFocus()
                        val width=(size.width-12.dp.toPx()).coerceAtLeast(1f);val p=((down.position.x-6.dp.toPx())/width).coerceIn(0f,1f)
                        val found=current.indexOfFirst {abs(it.number("position")-p)*width<8.dp.toPx()}
                        if(found<0&&current.size>=32)return@awaitEachGesture
                        val chosen=if(found<0)current.count {it.number("position")<p} else found
                        val initial=if(found<0)p else current[found].number("position")
                        selected=chosen;dragging=true;send(stop(initial,found.takeIf {it>=0}),"down")
                        try {
                            while(dragging) {
                                val change=awaitPointerEvent().changes.firstOrNull {it.id==down.id}?:break
                                if(change.isConsumed)break
                                val position=(initial+(change.position.x-down.position.x)/width).coerceIn(0f,1f)
                                change.consume()
                                if(!change.pressed){dragging=false;send(stop(position,chosen),"up");break}
                                send(stop(position,chosen),"move")
                            }
                        } finally {cancel()}
                    }
                }.semantics {contentDescription=copy.getString("add_stop")}) {
                GradientPreview(host,gradient,Modifier.fillMaxWidth().padding(horizontal=6.dp).height(32.dp))
                Canvas(Modifier.fillMaxSize()) {val margin=6.dp.toPx();stops.forEachIndexed {i,stop->drawCircle(palette.text,(if(index==i)4f else 2.5f).dp.toPx(),Offset(margin+stop.number("position")*(size.width-2*margin),39.dp.toPx()))}}
            }
            Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(6.dp),verticalAlignment=Alignment.CenterVertically) {
                val interior=index>0&&index<stops.lastIndex
                key(index) {NumericSetting(copy.getString("position"),stops[index].getDouble("position"),host.catalog.getJSONObject("opacity"),Modifier.weight(1f),enabled=enabled&&interior,id="gradient-position",inline=true,valueOnly=true,showSlider=false,
                    onEditPhase={phase->if(phase=="down")numericOwner=index;numericOwner?.let {owner->send(obj("kind" to "position","index" to owner,"operation" to obj("type" to "step","steps" to 0)),phase)};if(phase=="up"||phase=="cancel")numericOwner=null}) {
                        send(obj("kind" to "position","index" to (numericOwner?:index),"operation" to obj("type" to "value","value" to it)),if(numericOwner!=null)"move" else null)
                    }}
                icon("minus",copy.getString("remove_stop"),"remove",enabled&&interior) {selected=(index-1).coerceAtLeast(0);send(stop(0f,index).put("remove",true))}
                key(index,gradient.toString(),destination.toString()) {ManagedColorButton(host,copy.getString("color"),stops[index].getJSONObject("color"),enabled,compact=true) {send(stop(current[index].number("position"),index).put("color",it))}}
                icon("fill",copy.getString("use_selected"),"use-color") {send(obj("kind" to "use_current_color","index" to index))}
            }
        }
    }
}

@Composable internal fun GradientButton(host: CanvasHost, control: JSONObject, dispatch: (JSONObject)->Unit) {
    var open by remember(control.getJSONObject("gradient").getJSONObject("destination").toString()) {mutableStateOf(false)}
    Box {
        GradientPreview(host,control.getJSONObject("value").getJSONObject("value"),Modifier.fillMaxSize().clip(ControlShape).testTag("toolbar-gradient")
            .semantics {contentDescription=control.getString("label")}.clickable {open=true})
        if(open) Popup(alignment=Alignment.BottomStart,onDismissRequest={open=false},properties=PopupProperties(focusable=true)) {
            Surface(color=LocalPalette.current.panel,shape=ControlShape,shadowElevation=6.dp) {Box(Modifier.width(260.dp).padding(8.dp)) {GradientControl(host,control,dispatch=dispatch)}}
        }
    }
}
