package art.capycanvas

import android.view.KeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import kotlin.math.abs
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import androidx.compose.ui.window.PopupProperties
import org.json.JSONObject
import kotlin.math.roundToInt

/** Native contact/scroll geometry only. Rust validates and persists every edit. */
internal class WorkspaceRowInteraction {
    val rows = linkedMapOf<String, Rect>()
    val grips = mutableMapOf<String, Rect>()
    val options = mutableMapOf<String, Rect>()
    val pins = mutableMapOf<String, Rect>()
    var swipeRow by mutableStateOf<String?>(null)
    var swipeOffset by mutableFloatStateOf(0f)
    var deletable = emptySet<String>()
    var area = Rect.Zero
    var order = emptyList<String>()
    var enabled = false
    var contact by mutableStateOf(false)
    var menu by mutableStateOf<String?>(null)
    var reorderMenu by mutableStateOf(true)
    var active by mutableStateOf<String?>(null)
    var hint by mutableStateOf<Float?>(null)
    var point = Offset.Zero
    var before: String? = null
    var valid = false
    var generation = 0
    fun cancel() { generation++; menu = null; active = null; hint = null; valid = false }
    fun target() {
        valid = active != null && area.contains(point)
        val visible = order.mapNotNull { id -> rows[id]?.let { id to it } }
        val row = visible.firstOrNull { it.second.bottom > point.y } ?: visible.lastOrNull()
        if (!valid || row == null) { valid = false; hint = null; return }
        val after = point.y > row.second.center.y
        before = if (after) order.getOrNull(order.indexOf(row.first) + 1) else row.first
        hint = ((if (after) row.second.bottom else row.second.top) - area.top).coerceIn(0f, area.height)
    }
}

private fun Modifier.rowBounds(map: MutableMap<String, Rect>, id: String) = onGloballyPositioned {
    map[id] = Rect(it.positionInRoot(), it.size.toSize())
}

private fun Modifier.workspaceRowInput(drag: WorkspaceRowInteraction, focused: Boolean, move: (String, String?) -> Unit) = pointerInput(drag, focused) {
    if (!focused) return@pointerInput
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        val start = down.position + drag.area.topLeft
        val id = drag.order.firstOrNull { drag.rows[it]?.contains(start) == true }
        if (!drag.enabled || id == null || drag.options[id]?.contains(start) == true || drag.pins[id]?.contains(start) == true) return@awaitEachGesture
        val direct = down.type == PointerType.Mouse || drag.grips[id]?.contains(start) == true
        val secondary = currentEvent.buttons.isSecondaryPressed
        val generation = drag.generation
        var held = false
        var swiping = false
        var retired = false
        var released = false
        var remaining = viewConfiguration.longPressTimeoutMillis
        var eventTime = down.uptimeMillis
        drag.contact = true; drag.reorderMenu = drag.grips[id]?.contains(start)==true; drag.menu = if (secondary) id else null
        if(drag.swipeRow!=id){drag.swipeRow=null;drag.swipeOffset=0f}
        if (secondary) down.consume()
        try {
            while (true) {
                val event = if (!held && !retired && drag.active == null && !secondary)
                    withTimeoutOrNull(remaining) { awaitPointerEvent(PointerEventPass.Initial) }
                else awaitPointerEvent(PointerEventPass.Initial)
                if (event == null) {
                    if (!drag.enabled || id !in drag.order || generation != drag.generation) { retired = true; continue }
                    held = true
                    if (down.type != PointerType.Mouse) drag.menu = id
                    continue
                }
                val change = event.changes.find { it.id == down.id } ?: break
                remaining = (remaining - (change.uptimeMillis - eventTime)).coerceAtLeast(1)
                eventTime = change.uptimeMillis
                if (!change.pressed && change.isConsumed) break // Native CANCEL.
                if (!drag.enabled || id !in drag.order || generation != drag.generation) retired = true
                val moved = (change.position - down.position).getDistance() > viewConfiguration.touchSlop
                val delta=change.position-down.position
                if(!direct&&!held&&!retired&&id in drag.deletable&&delta.x < -viewConfiguration.touchSlop&&abs(delta.x)>abs(delta.y)*1.5f)swiping=true
                if(swiping){
                    drag.swipeRow=id;drag.swipeOffset=(-delta.x).coerceIn(0f,72.dp.toPx());change.consume()
                    if(!change.pressed){released=true;if(drag.swipeOffset<36.dp.toPx()){drag.swipeRow=null;drag.swipeOffset=0f}else drag.swipeOffset=72.dp.toPx();break}
                    continue
                }
                if (!direct && !held && moved) retired = true
                if (!retired && !secondary && moved && change.pressed && (direct || held)) {
                    drag.active = id; drag.menu = null
                }
                if (drag.active == id && !retired) {
                    drag.point = change.position + drag.area.topLeft; drag.target(); change.consume()
                }
                val heldMenu = held && down.type != PointerType.Mouse
                if (heldMenu || secondary || retired) {
                    // Early scrolling stays unconsumed; suppress only its release/click.
                    if (heldMenu || secondary || !change.pressed) change.consume()
                }
                if (!change.pressed) {
                    released = true
                    if (!retired && drag.active == id && drag.valid) move(id, drag.before)
                    break
                }
            }
        } finally {
            if (!released) drag.menu = null
            drag.active = null; drag.hint = null; drag.valid = false; drag.contact = false
        }
    }
}

@Composable internal fun workspaceRowHeight(): androidx.compose.ui.unit.Dp {
    val style=LocalTextStyle.current
    val measurer=androidx.compose.ui.text.rememberTextMeasurer()
    val metrics=panelMetrics()
    return with(LocalDensity.current) {
        maxOf(56.dp.roundToPx(),maxOf(metrics.height.roundToPx(),measurer.measure(" ",style).size.height)+
            measurer.measure(" ",style.copy(fontSize=style.fontSize*.88f)).size.height+2.dp.roundToPx()+2*4.dp.roundToPx()).toDp()
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable internal fun WorkspaceRows(host: CanvasHost, view: JSONObject, drag: WorkspaceRowInteraction, modifier: Modifier) {
    val colors = LocalPalette.current
    val rows = view.array("rows").objects()
    val workspaces = view.optString("page") == "workspaces"
    val metrics=panelMetrics()
    val rowHeight=if(workspaces)workspaceRowHeight() else 56.dp
    val renaming=workspaces&&view.objectOrNull("prompt_action")?.optString("type")=="rename"
    val enabled = !view.optBoolean("busy") && !view.optBoolean("switcher_busy") && view.isNull("prompt")
    val order = view.array("order").values().map { it.toString() }
    val scroll = rememberScrollState()
    var viewportHeight by remember {mutableIntStateOf(0)}
    val focused = LocalWindowInfo.current.isWindowFocused
    val density = LocalDensity.current.density
    fun edit(value: JSONObject) { host.workspaceInput(obj("type" to "edit_switcher", "edit" to value)) }
    SideEffect { drag.order = rows.map { it.getString("id") }; drag.enabled = enabled && workspaces
        drag.deletable=rows.filter {row->row.array("options").objects().any {it.optBoolean("enabled")&&it.getJSONObject("action").getString("type")=="delete"}}.map {it.getString("id")}.toSet()
    }
    LaunchedEffect(view.optString("page"), view.optString("prompt"), rows.map { it.getString("id") }) {
        if (!workspaces || !view.isNull("prompt") || (drag.active != null && drag.active !in drag.order)) drag.cancel()
        if (drag.menu != null && drag.menu !in drag.order) drag.menu = null
    }
    DisposableEffect(drag) { onDispose { drag.cancel() } }
    LaunchedEffect(drag.active) {
        while (drag.active != null) {
            withFrameNanos { }
            if (drag.area.contains(drag.point)) {
                val speed = when {
                    drag.point.y < drag.area.top + 28 * density -> -8 * density
                    drag.point.y > drag.area.bottom - 28 * density -> 8 * density
                    else -> 0f
                }
                if (speed != 0f) scroll.scrollBy(speed)
            }
            drag.target()
        }
    }
    Box(modifier.clip(ControlShape).background(colors.settingsCard)
        .testTag("workspace-rows").onGloballyPositioned { drag.area = Rect(it.positionInRoot(), it.size.toSize());viewportHeight=it.size.height }
        .workspaceRowInput(drag, focused) { id, before -> edit(obj("type" to "move", "id" to id, "before" to before)) }) {
        Column(Modifier.fillMaxSize().panelScroll(scroll)) {
            rows.forEachIndexed { index, row -> key(row.getString("id")) {
                val id = row.getString("id")
                val editing=workspaces&&view.objectOrNull("prompt_action")?.let {it.optString("type")=="rename"&&it.optString("value")==id}==true
                val bringIntoView=remember {BringIntoViewRequester()}
                LaunchedEffect(editing,viewportHeight,rowHeight) {if(editing)bringIntoView.bringIntoView()}
                val title = row.getString("title")
                val switchCaption=remember(title, workspaces, host.languageTag) { if (workspaces) JSONObject(Native.nativeCaption(obj("type" to "switch_workspace", "title" to title).toString(), host.languageTag)).getString("text") else title }
                val optionsCaption=remember(title, host.languageTag) { JSONObject(Native.nativeCaption(obj("type" to "options_for", "title" to title).toString(), host.languageTag)).getString("text") }
                val reorderCaption=remember(title, host.languageTag) { JSONObject(Native.nativeCaption(obj("type" to "reorder", "title" to title).toString(), host.languageTag)).getString("text") }
                val selected = view.optString("selected") == id
                DisposableEffect(id) { onDispose { drag.rows.remove(id); drag.grips.remove(id); drag.options.remove(id); drag.pins.remove(id) } }
                Box(Modifier.fillMaxWidth()) {
                    val delete=row.array("options").objects().find {it.getJSONObject("action").getString("type")=="delete"}
                    if(drag.swipeRow==id&&delete!=null)TextButton({host.workspaceInput(obj("type" to "action","action" to delete.getJSONObject("action")));drag.swipeRow=null;drag.swipeOffset=0f},Modifier.align(Alignment.CenterEnd).width(72.dp).height(rowHeight).background(MaterialTheme.colorScheme.error).rowBounds(drag.options,id).testTag("workspace-swipe-delete-$id"),enabled=delete.optBoolean("enabled")) {Text(delete.getString("label"),color=MaterialTheme.colorScheme.onError)}
                    Row(Modifier.fillMaxWidth().heightIn(min = rowHeight).bringIntoViewRequester(bringIntoView).offset {IntOffset(if(drag.swipeRow==id)-drag.swipeOffset.roundToInt() else 0,0)}.rowBounds(drag.rows, id)
                    .alpha(if (drag.active == id) .45f else 1f).background(if (selected) colors.active else colors.settingsCard).padding(start=if(workspaces)12.dp-metrics.padding else 12.dp,end=12.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f).onPreviewKeyEvent { event ->
                        val key = event.nativeKeyEvent
                        if(workspaces&&enabled&&key.action==KeyEvent.ACTION_DOWN&&key.keyCode==KeyEvent.KEYCODE_F2){
                            row.array("options").objects().find {it.optBoolean("enabled")&&it.getJSONObject("action").getString("type")=="rename"}?.let {host.workspaceInput(obj("type" to "action","action" to it.getJSONObject("action")))};true
                        }else if (workspaces && enabled && key.action == KeyEvent.ACTION_DOWN &&
                            (key.keyCode == KeyEvent.KEYCODE_MENU || (key.keyCode == KeyEvent.KEYCODE_F10 && key.isShiftPressed))) {
                            drag.reorderMenu = false; drag.menu = id; true
                        } else false
                    }.selectable(selected, enabled = enabled || renaming, role = Role.RadioButton) {
                        host.workspaceInput(obj("type" to "select", "id" to id))
                    }.padding(vertical = if(workspaces)4.dp else 8.dp).testTag("workspace-row-$id").semantics { contentDescription = switchCaption },
                        verticalArrangement = Arrangement.spacedBy(if(workspaces)2.dp else 3.dp)) {
                        val renameAction=if(workspaces)row.array("options").objects().find {it.optBoolean("enabled")&&it.getJSONObject("action").getString("type")=="rename"} else null
                        val rename=view.objectOrNull("prompt_action")
                        Box(Modifier.fillMaxWidth().heightIn(min=if(workspaces)metrics.height else 0.dp).testTag("workspace-name-slot-$id"),contentAlignment=Alignment.CenterStart) {
                        if(workspaces&&rename?.optString("type")=="rename"&&rename.optString("value")==id)WorkspaceNameEditor(host,view,id)
                        else Text(row.getString("title"),Modifier.fillMaxWidth().padding(horizontal=if(workspaces)metrics.padding else 0.dp).testTag("workspace-name-$id").combinedClickable(enabled=enabled || renaming,
                            onClick={host.workspaceInput(obj("type" to "select","id" to id))},
                            onDoubleClick=renameAction?.let {action->{host.workspaceInput(obj("type" to "action","action" to action.getJSONObject("action")))}}),maxLines=1,overflow=TextOverflow.Ellipsis)
                        }
                        row.getString("subtitle").takeIf { it.isNotEmpty() }?.let {
                            Text(it, Modifier.padding(horizontal=if(workspaces)metrics.padding else 0.dp), color = colors.settingsSecondary, fontSize = LocalTextStyle.current.fontSize * .88f,maxLines=if(workspaces)1 else Int.MAX_VALUE,overflow=TextOverflow.Ellipsis)
                        }
                    }
                    if (workspaces) {
                        if (view.optString("id") == id) SharedIcon("check", host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("current_workspace"), Modifier.padding(start = 6.dp).size(16.dp))
                        val pin = row.getJSONObject("pin")
                        Box(Modifier.size(34.dp).alpha(if(pin.getBoolean("enabled"))1f else .5f).toggleable(value=pin.getBoolean("selected"),enabled=pin.getBoolean("enabled"),role=Role.Checkbox,
                            interactionSource=remember {MutableInteractionSource()},indication=null,onValueChange={edit(pin.getJSONObject("edit"))}).rowBounds(drag.pins,id)
                            .testTag("workspace-pin-$id").semantics {contentDescription=pin.getString("label");this.selected=pin.getBoolean("selected")},contentAlignment=Alignment.Center) {
                            SharedIcon(pin.getString("icon"), null, Modifier.size(16.dp),tint=if(pin.getBoolean("selected"))colors.accent else colors.text)
                        }
                        if (workspaces) Box(Modifier.width(16.dp).height(34.dp), contentAlignment = Alignment.Center) {
                        IconButton({ drag.reorderMenu = true; drag.menu = id }, enabled = enabled,
                            modifier = Modifier.fillMaxSize().rowBounds(drag.grips, id).testTag("workspace-grip-$id")
                                .semantics { contentDescription = reorderCaption }) {
                            SharedIcon("grip", null, Modifier.size(16.dp).alpha(.45f))
                        }
                        DropdownMenu(drag.menu == id && drag.reorderMenu, { drag.menu = null }, properties = PopupProperties(focusable = !drag.contact),
                            modifier = Modifier.testTag("workspace-row-menu")) {
                                fun closeEdit(value: JSONObject) { drag.menu = null; edit(value) }
                                val position = order.indexOf(id)
                                DropdownMenuItem(text = { Text(host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("move_up")) }, enabled = enabled && position > 0, modifier = Modifier.testTag("workspace-up"),
                                    onClick = { closeEdit(obj("type" to "move", "id" to id, "before" to order.getOrNull(position - 1))) })
                                DropdownMenuItem(text = { Text(host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("move_down")) }, enabled = enabled && position >= 0 && position < order.lastIndex, modifier = Modifier.testTag("workspace-down"),
                                    onClick = { closeEdit(obj("type" to "move", "id" to id, "before" to order.getOrNull(position + 2))) })
                        }
                    }
                        val offered = row.array("options").objects()
                        if (offered.isEmpty()) SideEffect { drag.options.remove(id) }
                        Box {
                            if (offered.isNotEmpty() || order.isNotEmpty()) {
                                DropdownMenu(drag.menu == id && !drag.reorderMenu, { drag.menu = null }, properties = PopupProperties(focusable = !drag.contact),
                                    modifier = Modifier.testTag("workspace-row-menu")) {
                                    for (button in offered) {
                                        val action = button.getJSONObject("action")
                                        val kind = action.getString("type")
                                        DropdownMenuItem(text = { Text(button.getString("label")) },
                                            enabled = enabled && button.optBoolean("enabled"), modifier = Modifier.testTag("workspace-$kind"), onClick = {
                                                drag.menu = null; host.workspaceInput(obj("type" to "action", "action" to action))
                                            })
                                    }
                                    if(offered.isNotEmpty())HorizontalDivider(color=colors.menuSeparator)
                                    val position=order.indexOf(id)
                                    DropdownMenuItem(text={Text(host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("move_up"))},enabled=enabled&&position>0,modifier=Modifier.testTag("workspace-up"),onClick={drag.menu=null;edit(obj("type" to "move","id" to id,"before" to order.getOrNull(position-1)))})
                                    DropdownMenuItem(text={Text(host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("move_down"))},enabled=enabled&&position>=0&&position<order.lastIndex,modifier=Modifier.testTag("workspace-down"),onClick={drag.menu=null;edit(obj("type" to "move","id" to id,"before" to order.getOrNull(position+2)))})
                                }
                            }
                        }
                    } else {
                        var menu by remember(id) { mutableStateOf(false) }
                        Box {
                            IconButton({ menu = true }, enabled = enabled, modifier = Modifier.testTag("toolbar-options-$id")) { SharedIcon("more", optionsCaption) }
                            DropdownMenu(menu, { menu = false }) {
                                for (button in row.array("actions").objects()) DropdownMenuItem(text = { Text(button.getString("label")) }, enabled = enabled && button.optBoolean("enabled"), onClick = { menu = false; host.workspaceInput(obj("type" to "action", "action" to button.getJSONObject("action"))) })
                            }
                        }
                    }
                }
                if(index>0)HorizontalDivider(Modifier.align(Alignment.TopCenter),color=colors.rowDivider)
                }
            } }
        }
        drag.hint?.let { y -> Box(Modifier.offset { IntOffset(0, (y - density).roundToInt()) }
            .fillMaxWidth().height(2.dp).background(colors.accent).testTag("workspace-row-drop-hint")) }
    }
}

private fun JSONObject.offers(type: String) =
    array("actions").objects().any { it.optBoolean("enabled") && it.getJSONObject("action").getString("type") == type }

@Composable private fun WorkspaceNameEditor(host: CanvasHost,view:JSONObject,id:String) {
    val metrics=panelMetrics();val colors=LocalPalette.current
    val initial=view.getJSONObject("prompt").getString("name")
    var name by remember(id) {mutableStateOf(TextFieldValue(initial,TextRange(0,initial.length)))}
    val focus=remember {FocusRequester()};var hadFocus by remember {mutableStateOf(false)};var submitted by remember {mutableStateOf(false)}
    fun finish(cancel:Boolean=false):Boolean {
        if(submitted)return true
        if(!cancel&&!JSONObject(Native.workspaceNameError(name.text,host.languageTag)).isNull("error"))return false
        submitted=true
        host.workspaceInput(if(cancel)obj("type" to "cancel") else obj("type" to "submit","name" to name.text,"description" to null,"choice" to null));return true
    }
    val commit=remember(id) {{finish()}}
    SideEffect {host.workspaceNameCommit=commit}
    LaunchedEffect(view.optBoolean("busy"),view.optString("error")) {if(!view.optBoolean("busy"))submitted=false}
    CoreTextField(name,{name=it;},Modifier.fillMaxWidth().textCursorRegion().testTag("workspace-name-$id").focusRequester(focus).onPreviewKeyEvent {event->
        val key=event.nativeKeyEvent
        if(key.keyCode==KeyEvent.KEYCODE_ESCAPE){if(name.composition==null&&!host.textComposition.owns(key)&&key.action==KeyEvent.ACTION_DOWN)finish(true);true}else false
    }.onFocusChanged {if(hadFocus&&!it.isFocused)finish();hadFocus=it.isFocused;},
        textStyle=LocalTextStyle.current.copy(color=colors.text),singleLine=true,cursorBrush=androidx.compose.ui.graphics.SolidColor(colors.accent),keyboardOptions=KeyboardOptions(imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={if(name.composition==null)finish()}),
        decorationBox={field->Box(Modifier.fillMaxWidth().heightIn(min=metrics.height).clip(ControlShape).background(colors.input).padding(horizontal=metrics.padding),contentAlignment=Alignment.CenterStart){field()}})
    LaunchedEffect(id){focus.requestFocus()}
    DisposableEffect(id){onDispose {if(host.workspaceNameCommit===commit)host.workspaceNameCommit=null;}}
}
