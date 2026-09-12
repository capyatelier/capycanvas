package art.capycanvas

import android.view.KeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.toSize
import androidx.compose.ui.window.PopupProperties
import org.json.JSONObject
import kotlin.math.roundToInt

/** Native contact/scroll geometry only. Rust validates and persists every edit. */
internal class WorkspaceRowInteraction {
    val rows = linkedMapOf<String, Rect>()
    val grips = mutableMapOf<String, Rect>()
    val options = mutableMapOf<String, Rect>()
    var area = Rect.Zero
    var order = emptyList<String>()
    var enabled = false
    var contact by mutableStateOf(false)
    var menu by mutableStateOf<String?>(null)
    var active by mutableStateOf<String?>(null)
    var hint by mutableStateOf<Float?>(null)
    var point = Offset.Zero
    var before: String? = null
    var valid = false
    var generation = 0
    fun cancel() { generation++; menu = null; active = null; hint = null; valid = false }
    fun target() {
        valid = area.contains(point)
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
        if (!drag.enabled || id == null || drag.options[id]?.contains(start) == true) return@awaitEachGesture
        val direct = down.type == PointerType.Mouse || drag.grips[id]?.contains(start) == true
        val secondary = currentEvent.buttons.isSecondaryPressed
        val generation = drag.generation
        var held = false
        var retired = false
        var released = false
        var remaining = viewConfiguration.longPressTimeoutMillis
        var eventTime = down.uptimeMillis
        drag.contact = true; drag.menu = if (secondary) id else null
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
                if (!direct && !held && moved) retired = true // Leave scrolling with the list.
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

@Composable internal fun WorkspaceRows(host: CanvasHost, view: JSONObject, drag: WorkspaceRowInteraction, modifier: Modifier) {
    val colors = LocalPalette.current
    val rows = view.array("rows").objects()
    val workspaces = view.optString("page") == "workspaces"
    val enabled = !view.optBoolean("busy") && !view.optBoolean("switcher_busy") && view.isNull("form")
    val pinned = view.array("switcher").objects().map { it.getString("id") }
    val order = view.array("order").values().map { it.toString() }
    val scroll = rememberScrollState()
    val focused = LocalWindowInfo.current.isWindowFocused
    val density = LocalDensity.current.density
    fun edit(value: JSONObject) { host.workspaceInput(obj("type" to "edit_switcher", "edit" to value)) }
    SideEffect { drag.order = rows.map { it.getString("id") }; drag.enabled = enabled && workspaces }
    LaunchedEffect(view.optString("page"), view.optString("form"), rows.map { it.getString("id") }) {
        if (!workspaces || !view.isNull("form") || (drag.active != null && drag.active !in drag.order)) drag.cancel()
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
    Box(modifier.clip(RoundedCornerShape(10.dp)).background(colors.settingsCard)
        .testTag("workspace-rows").onGloballyPositioned { drag.area = Rect(it.positionInRoot(), it.size.toSize()) }
        .workspaceRowInput(drag, focused) { id, before -> edit(obj("type" to "move", "id" to id, "before" to before)) }) {
        Column(Modifier.fillMaxSize().verticalScroll(scroll)) {
            rows.forEachIndexed { index, row -> key(row.getString("id")) {
                val id = row.getString("id")
                val selected = view.optString("selected") == id
                DisposableEffect(id) { onDispose { drag.rows.remove(id); drag.grips.remove(id); drag.options.remove(id) } }
                if (index > 0) HorizontalDivider(color = colors.divider)
                Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).rowBounds(drag.rows, id)
                    .alpha(if (drag.active == id) .45f else 1f).background(if (selected) colors.active else colors.settingsCard),
                    verticalAlignment = Alignment.CenterVertically) {
                    if (workspaces) Box(Modifier.width(24.dp).height(56.dp).rowBounds(drag.grips, id)
                        .testTag("workspace-grip-$id"), contentAlignment = Alignment.Center) {
                        SharedIcon("grip", "Drag to reorder", Modifier.size(12.dp).alpha(.45f))
                    }
                    Column(Modifier.weight(1f).onPreviewKeyEvent { event ->
                        val key = event.nativeKeyEvent
                        if (workspaces && enabled && key.action == KeyEvent.ACTION_DOWN &&
                            (key.keyCode == KeyEvent.KEYCODE_MENU || (key.keyCode == KeyEvent.KEYCODE_F10 && key.isShiftPressed))) {
                            drag.menu = id; true
                        } else false
                    }.selectable(selected, enabled = enabled, role = Role.RadioButton) {
                        host.workspaceInput(obj("type" to "select", "id" to id))
                    }.padding(vertical = 12.dp, horizontal = if (workspaces) 4.dp else 12.dp).testTag("workspace-row-$id"),
                        verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        Text(row.getString("title"))
                        row.getString("subtitle").takeIf { it.isNotEmpty() }?.let {
                            Text(it, color = colors.settingsSecondary, fontSize = LocalTextStyle.current.fontSize * .88f)
                        }
                    }
                    if (workspaces) {
                        if (id in pinned) SharedIcon("pin", "Shown in top bar", Modifier.size(16.dp).testTag("workspace-pin-$id"))
                        if (view.optString("id") == id) SharedIcon("check", "Current workspace", Modifier.padding(start = 6.dp).size(16.dp))
                        Box(Modifier.rowBounds(drag.options, id)) {
                            IconButton({ drag.menu = id }, enabled = enabled,
                                modifier = Modifier.size(36.dp).testTag("workspace-options-$id").semantics { contentDescription = "Options for ${row.getString("title")}" }) { Text("⋮", fontSize = 22.sp) }
                            DropdownMenu(drag.menu == id, { drag.menu = null }, properties = PopupProperties(focusable = !drag.contact),
                                modifier = Modifier.testTag("workspace-row-menu")) {
                                fun closeEdit(value: JSONObject) { drag.menu = null; edit(value) }
                                DropdownMenuItem(text = { Text("Show in top bar") }, trailingIcon = { if (id in pinned) SharedIcon("check", null) },
                                    enabled = enabled, modifier = Modifier.testTag("workspace-pin").semantics {
                                        toggleableState = if (id in pinned) ToggleableState.On else ToggleableState.Off
                                    },
                                    onClick = { closeEdit(obj("type" to "show", "id" to id, "visible" to (id !in pinned))) })
                                val position = order.indexOf(id)
                                DropdownMenuItem(text = { Text("Move Up") }, enabled = enabled && position > 0, modifier = Modifier.testTag("workspace-up"),
                                    onClick = { closeEdit(obj("type" to "move", "id" to id, "before" to order.getOrNull(position - 1))) })
                                DropdownMenuItem(text = { Text("Move Down") }, enabled = enabled && position >= 0 && position < order.lastIndex, modifier = Modifier.testTag("workspace-down"),
                                    onClick = { closeEdit(obj("type" to "move", "id" to id, "before" to order.getOrNull(position + 2))) })
                                HorizontalDivider()
                                for ((kind, label) in listOf("rename" to "Rename…", "delete" to "Delete…")) {
                                    if (kind != "delete" || row.optBoolean("delete")) DropdownMenuItem(text = { Text(label) },
                                        enabled = enabled && row.optBoolean("options"), modifier = Modifier.testTag("workspace-$kind"), onClick = {
                                            drag.menu = null; host.workspaceInput(obj("type" to "form", "kind" to kind, "id" to id))
                                        })
                                }
                            }
                        }
                    }
                }
            } }
        }
        drag.hint?.let { y -> Box(Modifier.offset { IntOffset(0, (y - density).roundToInt()) }
            .fillMaxWidth().height(2.dp).background(colors.accent).testTag("workspace-row-drop-hint")) }
    }
}
