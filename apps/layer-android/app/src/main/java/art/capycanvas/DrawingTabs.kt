package art.capycanvas

import android.view.KeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

private class DrawingDrag(val vertical:Boolean) {
    val rows=mutableMapOf<Long,Rect>();val handles=mutableMapOf<Long,Rect>();val closes=mutableMapOf<Long,Rect>()
    var area=Rect.Zero;var order=emptyList<Long>();var enabled=false;var generation=0
    var active by mutableStateOf<Long?>(null);var menu by mutableStateOf<Long?>(null)
    var before by mutableStateOf<Long?>(null);var valid by mutableStateOf(false)
    var point=Offset.Zero
    fun cancel(){generation++;active=null;menu=null;valid=false}
    fun hitMap()=JSONArray(order.mapNotNull {id->rows[id]?.intersect(area)?.takeIf{it.width>0&&it.height>0}?.let {
        obj("id" to id,"bounds" to obj("x" to it.left,"y" to it.top,"width" to it.width,"height" to it.height))
    }})
    fun drop()=obj("op" to "drop","hits" to hitMap(),"point" to JSONArray(listOf(point.x,point.y)),"vertical" to vertical)
}
private fun Modifier.drawingBounds(map:MutableMap<Long,Rect>,id:Long)=onGloballyPositioned{map[id]=Rect(it.positionInRoot(),it.size.toSize())}
private fun Modifier.drawingInput(drag:DrawingDrag,focused:Boolean,preview:()->Unit,finish:(Long,JSONObject)->Unit,menu:(Long)->Unit)=pointerInput(drag,focused) {
    if(!focused)return@pointerInput
    awaitEachGesture {
        val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial)
        val start=down.position+drag.area.topLeft
        val id=drag.order.firstOrNull{drag.rows[it]?.contains(start)==true}
        if(!drag.enabled||id==null||drag.closes[id]?.contains(start)==true)return@awaitEachGesture
        val direct=!drag.vertical||down.type==PointerType.Mouse||drag.handles[id]?.contains(start)==true
        val secondary=currentEvent.buttons.isSecondaryPressed
        val generation=drag.generation;var held=false;var retired=false;var released=false
        var remaining=viewConfiguration.longPressTimeoutMillis;var eventTime=down.uptimeMillis
        if(secondary){down.consume();menu(id)}
        try {
            while(true) {
                val event=if(!held&&!retired&&drag.active==null&&!secondary)withTimeoutOrNull(remaining){awaitPointerEvent(PointerEventPass.Initial)}else awaitPointerEvent(PointerEventPass.Initial)
                if(event==null){held=true;if(down.type!=PointerType.Mouse&&drag.enabled&&generation==drag.generation){drag.menu=id};continue}
                val change=event.changes.find{it.id==down.id}?:break
                remaining=(remaining-(change.uptimeMillis-eventTime)).coerceAtLeast(1);eventTime=change.uptimeMillis
                if(!change.pressed&&change.isConsumed)break
                if(!drag.enabled||id !in drag.order||generation!=drag.generation)retired=true
                val moved=(change.position-down.position).getDistance()>viewConfiguration.touchSlop
                if(!direct&&!held&&moved)retired=true
                if(!retired&&!secondary&&moved&&change.pressed&&(direct||held)){drag.active=id;drag.menu=null}
                if(drag.active==id&&!retired){drag.point=change.position+drag.area.topLeft;preview();change.consume()}
                if((held&&down.type!=PointerType.Mouse)||secondary||(retired&&!change.pressed))change.consume()
                if(!change.pressed){released=true;if(!retired&&drag.active==id)finish(id,drag.drop());break}
            }
        }finally{if(!released)drag.menu=null;drag.active=null;drag.valid=false}
    }
}
private fun Modifier.drawingKeys(controller:DrawingTabsController,id:Long,menu:()->Unit)=onPreviewKeyEvent {
    val event=it.nativeKeyEvent
    if(event.action!=KeyEvent.ACTION_DOWN)return@onPreviewKeyEvent false
    val forward=event.keyCode in listOf(KeyEvent.KEYCODE_DPAD_RIGHT,KeyEvent.KEYCODE_DPAD_DOWN)
    when {
        event.keyCode==KeyEvent.KEYCODE_FORWARD_DEL->{controller.select(id,true);true}
        event.isCtrlPressed&&event.keyCode==KeyEvent.KEYCODE_Z->{controller.order(obj("op" to "history","redo" to event.isShiftPressed));true}
        event.isShiftPressed&&event.keyCode==KeyEvent.KEYCODE_F10->{menu();true}
        event.keyCode in listOf(KeyEvent.KEYCODE_DPAD_LEFT,KeyEvent.KEYCODE_DPAD_RIGHT,KeyEvent.KEYCODE_DPAD_UP,KeyEvent.KEYCODE_DPAD_DOWN)->{
            if(event.isCtrlPressed&&event.isShiftPressed)controller.order(obj("op" to "step","id" to id,"forward" to forward))
            else{val order=controller.rows.map{row->row.getLong("id")};order.getOrNull(order.indexOf(id)+if(forward)1 else -1)?.let{next->controller.select(next)}};true
        }
        event.keyCode in listOf(KeyEvent.KEYCODE_MOVE_HOME,KeyEvent.KEYCODE_MOVE_END)->{val rows=controller.rows;if(rows.isNotEmpty())controller.select((if(event.keyCode==KeyEvent.KEYCODE_MOVE_HOME)rows.first()else rows.last()).getLong("id"));true}
        else->false
    }
}
@Composable internal fun DrawingHeader(host:CanvasHost,title:String,editing:Boolean) {
    val controller=host.drawingTabs
    if(editing){Text(title,Modifier.padding(horizontal=6.dp).testTag("document-title"),maxLines=1,overflow=TextOverflow.Ellipsis);return}
    BoxWithConstraints(Modifier.fillMaxSize()) {
        var compact by remember{mutableStateOf(true)}
        LaunchedEffect(maxWidth,controller.rows.size){compact=JSONObject(controller.query(obj("op" to "view","width" to maxWidth.value))).getBoolean("compact")}
        if(controller.rows.size<=1||compact) {
            val row=controller.rows.firstOrNull{it.getLong("id")==controller.selected}
            TextButton({controller.selector=true},Modifier.fillMaxSize().testTag("drawing-selector-button")) {
                Text(row?.let{it.getString("title")+(if(it.getBoolean("modified"))" •" else "")+(if(controller.rows.size>1)" ▾"else title.substringAfter(" · ", "").let { dimensions -> if(dimensions.isEmpty())"" else " · $dimensions" })}?:title,Modifier.testTag("document-title"),maxLines=1,overflow=TextOverflow.Ellipsis)
            }
        }else DrawingRows(host,false,Modifier.fillMaxSize())
    }
}
@Composable internal fun DrawingSelector(host:CanvasHost) {
    val controller=host.drawingTabs
    if(!controller.selector)return
    AlertDialog(onDismissRequest={controller.selector=false},title={Text("Drawings")},
        text={Column(Modifier.fillMaxWidth().testTag("drawing-selector")) {
            DrawingRows(host,true,Modifier.fillMaxWidth().heightIn(max=420.dp))
            controller.view.optString("storage_error").takeUnless{it.isEmpty()||it=="null"}?.let{Text(it)}
            Row {
                TextButton({controller.order(obj("op" to "history","redo" to false))},enabled=controller.view.optBoolean("can_undo"),modifier=Modifier.testTag("drawing-order-undo")){Text("Undo reorder")}
                TextButton({controller.order(obj("op" to "history","redo" to true))},enabled=controller.view.optBoolean("can_redo"),modifier=Modifier.testTag("drawing-order-redo")){Text("Redo")}
            }
        }},confirmButton={TextButton({controller.selector=false}){Text("Done")}})
}
@Composable private fun DrawingRows(host:CanvasHost,vertical:Boolean,modifier:Modifier) {
    val controller=host.drawingTabs;val rows=controller.rows;val colors=LocalPalette.current
    val drag=remember(vertical){DrawingDrag(vertical)};val scope=rememberCoroutineScope();val scroll=rememberScrollState()
    val focused=LocalWindowInfo.current.isWindowFocused;val density=LocalDensity.current.density
    val order=rows.map{it.getLong("id")}
    SideEffect{drag.order=order;drag.enabled=!controller.blocked}
    LaunchedEffect(order,focused,controller.switching){drag.cancel()}
    DisposableEffect(drag){onDispose{drag.cancel()}}
    fun preview(){val generation=drag.generation;val point=drag.point;scope.launch {
        val result=controller.query(drag.drop());if(generation==drag.generation&&point==drag.point&&drag.active!=null){drag.valid=result!="null";drag.before=if(drag.valid)JSONObject(result).let{if(it.isNull("before"))null else it.getLong("before")}else null}
    }}
    LaunchedEffect(drag.active){while(drag.active!=null){withFrameNanos{};if(vertical&&drag.area.contains(drag.point)){val dy=when{drag.point.y<drag.area.top+28*density->-8*density;drag.point.y>drag.area.bottom-28*density->8*density;else->0f};if(dy!=0f){scroll.scrollBy(dy);preview()}}}}
    val input=modifier.onGloballyPositioned{val area=Rect(it.positionInRoot(),it.size.toSize());if(drag.area!=area)drag.cancel();drag.area=area}
        .onPreviewKeyEvent{if(it.nativeKeyEvent.keyCode==KeyEvent.KEYCODE_ESCAPE&&(drag.active!=null||drag.menu!=null)){drag.cancel();true}else false}
        .drawingInput(drag,focused,::preview,{id,query->scope.launch{val target=controller.query(query);if(target!="null")controller.order(obj("op" to "reorder","id" to id,"before" to JSONObject(target).opt("before"))) }},{id->if(vertical)drag.menu=id else controller.selector=true})
    Box(input) {
        @Composable fun row(item:JSONObject,entry:Modifier) {
            val id=item.getLong("id");val selected=controller.selected==id
            Row(entry.drawingBounds(drag.rows,id).testTag("drawing-tab-$id")
                .clip(RoundedCornerShape(8.dp))
                .background(if(selected)colors.accent.copy(alpha=.16f)else Color.Transparent)
                .then(if(drag.active==id)Modifier.border(2.dp,colors.accent,RoundedCornerShape(8.dp))else Modifier)
                .drawingKeys(controller,id){if(vertical)drag.menu=id else controller.selector=true}
                .selectable(selected,enabled=!controller.blocked,role=Role.Tab){controller.select(id)}
                .semantics{contentDescription="${item.getString("title")}${if(item.getBoolean("modified"))", modified"else ""}, ${item.getString("location")}"},verticalAlignment=Alignment.CenterVertically) {
                if(vertical)Box(Modifier.width(32.dp).fillMaxHeight().drawingBounds(drag.handles,id).testTag("drawing-handle-$id"),contentAlignment=Alignment.Center){PanelGrip("Move drawing")}
                else Spacer(Modifier.width(36.dp))
                Column(Modifier.weight(1f).padding(horizontal=6.dp),horizontalAlignment=if(vertical)Alignment.Start else Alignment.CenterHorizontally) {
                    Text(item.getString("title")+if(item.getBoolean("modified"))" •"else "",maxLines=1,overflow=TextOverflow.Ellipsis)
                    if(vertical)Text(item.getString("location"),style=MaterialTheme.typography.bodySmall,maxLines=2,overflow=TextOverflow.Ellipsis)
                }
                IconButton({controller.select(id,true)},Modifier.size(if(vertical)48.dp else 36.dp).drawingBounds(drag.closes,id).testTag("drawing-close-$id"),enabled=!controller.blocked){SharedIcon("close","Close ${item.getString("title")}",Modifier.size(16.dp))}
            }
        }
        if(vertical)Column(Modifier.fillMaxWidth().verticalScroll(scroll)){rows.forEach{item->key(item.getLong("id")){row(item,Modifier.fillMaxWidth().height(64.dp))}}}
        else Row(Modifier.fillMaxSize()){rows.forEach{item->key(item.getLong("id")){row(item,Modifier.weight(1f).fillMaxHeight())}}}
        if(drag.active!=null&&drag.valid) {
            val target=drag.before?.let{drag.rows[it]}?:order.lastOrNull()?.let{drag.rows[it]}
            if(target!=null){val at=if(vertical){(if(drag.before==null)target.bottom else target.top)-drag.area.top}else{(if(drag.before==null)target.right else target.left)-drag.area.left}
                Box(Modifier.offset{if(vertical)IntOffset(0,at.roundToInt())else IntOffset(at.roundToInt(),0)}.then(if(vertical)Modifier.fillMaxWidth().height(2.dp)else Modifier.width(2.dp).fillMaxHeight()).background(colors.accent).testTag("drawing-drop-indicator"))}
        }
        drag.menu?.let{id->Surface(Modifier.align(Alignment.BottomCenter).testTag("drawing-row-menu"),shadowElevation=6.dp){Row {
            TextButton({drag.menu=null;controller.select(id)}){Text("Select")}
            TextButton({drag.menu=null;controller.order(obj("op" to "step","id" to id,"forward" to false))}){Text("Move earlier")}
            TextButton({drag.menu=null;controller.order(obj("op" to "step","id" to id,"forward" to true))}){Text("Move later")}
            TextButton({drag.menu=null;controller.select(id,true)}){Text("Close")}
        }}}
    }
}
