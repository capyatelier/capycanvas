package art.capycanvas

import android.view.MotionEvent
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import kotlinx.coroutines.launch

internal object PointerScrollbars {
    var mouse by mutableStateOf(false)
    fun input(event: MotionEvent) {
        if(event.pointerCount>0) mouse=event.getToolType(0)==MotionEvent.TOOL_TYPE_MOUSE
    }
}

@Composable internal fun Modifier.panelScroll(scroll: ScrollState) = mouseScrollbar(scroll).verticalScroll(scroll)
@Composable internal fun Modifier.mouseScrollbar(scroll: ScrollState) = mouseScrollbar({ scroll.value.toFloat() to scroll.maxValue.toFloat() }) { scroll.scrollBy(it) }
@Composable internal fun Modifier.mouseScrollbar(scroll: LazyListState) = mouseScrollbar({
    val info=scroll.layoutInfo
    val unit=if(info.visibleItemsInfo.isEmpty())0f else info.visibleItemsInfo.sumOf { it.size }.toFloat()/info.visibleItemsInfo.size
    (scroll.firstVisibleItemIndex*unit+scroll.firstVisibleItemScrollOffset) to
        (info.totalItemsCount*unit-(info.viewportEndOffset-info.viewportStartOffset)).coerceAtLeast(0f)
}) { scroll.scrollBy(it) }

@Composable private fun Modifier.mouseScrollbar(metrics: () -> Pair<Float,Float>, move: suspend (Float) -> Unit): Modifier {
    val density=LocalDensity.current.density
    val color=LocalPalette.current.text.copy(alpha=.35f)
    val scope=rememberCoroutineScope()
    var viewport by remember { mutableStateOf(Size.Zero) }
    val read by rememberUpdatedState(metrics)
    val scroll by rememberUpdatedState(move)
    fun thumb(): Rect {
        val (top,maximum)=read()
        if(!PointerScrollbars.mouse || maximum<=0 || viewport.height<=0) return Rect.Zero
        val height=(viewport.height*viewport.height/(viewport.height+maximum)).coerceIn((28*density).coerceAtMost(viewport.height),viewport.height)
        val y=top.coerceIn(0f,maximum)/maximum*(viewport.height-height)
        return Rect(viewport.width-8*density,y,viewport.width,y+height)
    }
    return onSizeChanged { viewport=Size(it.width.toFloat(),it.height.toFloat()) }
        .pointerInput(Unit) {
            awaitPointerEventScope { while(true) {
                awaitPointerEvent(PointerEventPass.Initial).changes.firstOrNull()?.let { PointerScrollbars.mouse=it.type==PointerType.Mouse }
            } }
        }
        .pointerInput(Unit) {
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false,pass=PointerEventPass.Initial)
                if(down.type!=PointerType.Mouse || !thumb().contains(down.position)) return@awaitEachGesture
                down.consume()
                var last=down.position.y
                do {
                    val change=awaitPointerEvent(PointerEventPass.Initial).changes.find { it.id==down.id } ?: break
                    change.consume()
                    val maximum=read().second
                    val travel=viewport.height-thumb().height
                    if(travel>0) { val delta=(change.position.y-last)*maximum/travel; scope.launch { scroll(delta) } }
                    last=change.position.y
                    if(!change.pressed)break
                } while(true)
            }
        }.drawWithContent {
            drawContent()
            val rect=thumb()
            if(rect!=Rect.Zero)drawRoundRect(color,Offset(rect.left+2*density,rect.top),Size(4*density,rect.height),CornerRadius(2*density))
        }
}
