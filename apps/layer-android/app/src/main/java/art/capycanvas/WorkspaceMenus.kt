package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import android.view.KeyEvent
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.PopupProperties
import org.json.JSONArray
import org.json.JSONObject
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume

internal fun CanvasHost.menuEpoch(): Long? = snapshot?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch")

internal suspend fun CanvasHost.menuCopy(request: JSONObject): JSONObject? = suspendCancellableCoroutine { continuation ->
    val epoch = menuEpoch()
    query(request) { value ->
        if (continuation.isActive) continuation.resume(if (menuEpoch() != epoch) null else when (value) {
            is JSONObject -> value
            is JSONArray -> obj("sections" to value)
            else -> null
        })
    }
}

/** Header menus, context menus and configuration options render the same Rust
 * items. They never reconstruct eligibility, naming, defaults or commands. */
@Composable internal fun WorkspaceMenu(host: CanvasHost, menu: JSONObject, preserveContact: Boolean = false,
    focusable: Boolean = !preserveContact, command: ((JSONObject) -> Unit)? = null,
    copy: (suspend () -> JSONObject?)? = null, showTitle: Boolean = true, maxHeight: Dp = Dp.Unspecified,
    presentation: JSONObject? = null,anchor: Rect? = null,dismiss: () -> Unit) {
    var projected by remember(menu) { mutableStateOf(menu) }
    val currentCopy by rememberUpdatedState(copy)
    var refresh by remember { mutableIntStateOf(0) }
    LaunchedEffect(host, host.languageTag, menu, refresh) {
        val tag = host.languageTag
        val epoch = host.menuEpoch()
        currentCopy?.invoke()?.let { if (host.languageTag == tag && host.menuEpoch() == epoch) projected = it }
    }
    val current = if (copy == null) menu else projected
    var popup by remember { mutableStateOf(Offset.Zero) }
    val scale = androidx.compose.ui.platform.LocalDensity.current.density
    val shape: Shape = if(presentation?.optBoolean("has_arrow") == true && anchor != null) androidx.compose.foundation.shape.GenericShape { size,_ ->
        val arrow = 8f*scale
        val below = popup.y >= anchor.center.y
        val tip = (anchor.center.x-popup.x).coerceIn(16f*scale,(size.width-16f*scale).coerceAtLeast(16f*scale))
        addRoundRect(RoundRect(Rect(0f,if(below)arrow else 0f,size.width,if(below)size.height else size.height-arrow),CornerRadius(10f*scale)))
        if(below) { moveTo(tip-6f*scale,arrow);lineTo(tip,0f);lineTo(tip+6f*scale,arrow) }
        else { moveTo(tip-6f*scale,size.height-arrow);lineTo(tip,size.height);lineTo(tip+6f*scale,size.height-arrow) }
        close()
    } else RoundedCornerShape(10.dp)
    // A focusable Android popup cancels the contact in the activity that opened
    // it. Context menus must leave that contact with the original drag owner.
    BackHandler(!focusable, dismiss)
    DropdownMenu(true, dismiss, modifier = Modifier.widthIn(min = 240.dp, max = 380.dp).heightIn(max = maxHeight).testTag("workspace-menu")
        .onGloballyPositioned { popup = it.localToScreen(Offset.Zero) }.onPreviewKeyEvent { event ->
            val key = event.nativeKeyEvent
            if (key.keyCode == KeyEvent.KEYCODE_ESCAPE) { if (key.action == KeyEvent.ACTION_DOWN) dismiss(); true } else false
        },
        properties = if (focusable) PopupProperties(focusable = true) else WindowlessMenu,
        shape = shape, containerColor = LocalPalette.current.panel) {
        if (!focusable) {
            val view = LocalView.current
            val currentDismiss by rememberUpdatedState(dismiss)
            DisposableEffect(host, view) {
                val keys: (KeyEvent) -> Boolean = { key ->
                    if (key.keyCode == KeyEvent.KEYCODE_ESCAPE) {
                        if (key.action == KeyEvent.ACTION_DOWN) currentDismiss()
                        true
                    } else view.dispatchKeyEvent(key)
                }
                host.menuKeyHandler = keys
                onDispose { if (host.menuKeyHandler === keys) host.menuKeyHandler = null }
            }
        }
        WorkspaceMenuItems(host, current.array("sections"), dismiss, if (showTitle && current.has("title")) current.getString("title") else null, command, refresh = { refresh++ })
    }
}

internal val WindowlessMenu = PopupProperties(focusable = false)

@Composable internal fun WindowlessPopup(open: Boolean, dismiss: () -> Unit) {
    BackHandler(open, dismiss)
    PopupOwner(open)
}

@Composable internal fun PopupOwner(open: Boolean) {
    val dock = LocalDock.current
    DisposableEffect(dock, open) {
        if (open) dock?.popup(true)
        onDispose { if (open) dock?.popup(false) }
    }
}

internal class WindowlessMenuButton {
    var menu by mutableStateOf<JSONObject?>(null)
    var pressedAt = 0L
    var closedAt = 0L
    var copy: (suspend () -> JSONObject?)? = null
}

internal fun Modifier.opensWindowlessMenu(button: WindowlessMenuButton, label: String, load: ((JSONObject?) -> Unit) -> Unit) =
    pointerInput(button) {
        awaitEachGesture { button.pressedAt = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial).uptimeMillis }
    }.clickable(role = Role.Button, onClickLabel = label) {
        if (button.menu == null && button.closedAt < button.pressedAt) load { button.menu = it }
    }

@Composable internal fun WindowlessMenuHost(host: CanvasHost, button: WindowlessMenuButton, command: ((JSONObject) -> Unit)? = null,
    current: JSONObject? = null) {
    PopupOwner(button.menu != null)
    button.menu?.let { WorkspaceMenu(host, current ?: it, focusable = false, command = command, copy = if (current == null) button.copy else null) { button.menu = null; button.closedAt = android.os.SystemClock.uptimeMillis() } }
}

@Composable internal fun ToolGroupIndicator(modifier: Modifier = Modifier) = SharedIcon("tool-group", null, modifier)

@Composable internal fun WorkspaceMenuItems(host: CanvasHost, sections: JSONArray,
    dismiss: () -> Unit = {}, title: String? = null, command: ((JSONObject) -> Unit)? = null, refresh: () -> Unit = {}) {
    val colors = LocalPalette.current
    var pages by remember { mutableStateOf<List<Pair<Int, Int>>>(emptyList()) }
    var page: JSONObject? = null
    var currentSections = sections
    var resolved = 0
    for ((sectionIndex, itemIndex) in pages) {
        page = currentSections.optJSONArray(sectionIndex)?.optJSONObject(itemIndex)
            ?.takeIf { it.array("sections").length() > 0 } ?: break
        currentSections = page.array("sections")
        resolved++
    }
    SideEffect { if (resolved < pages.size) pages = pages.take(resolved) }
    val currentPage = page
    if (currentPage != null) {
        Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).testTag("workspace-menu-back").clickable { pages = pages.dropLast(1) }
            .padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            SharedIcon("down", host.bootstrap!!.getJSONObject("common").getString("back"), Modifier.rotate(90f))
            Text(currentPage.getString("label"), fontWeight = FontWeight.Bold)
        }
        HorizontalDivider(color = colors.menuSeparator)
    } else if (title != null) Text(title, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = colors.secondary)
    (currentPage?.array("sections") ?: sections).values().mapIndexed { index, value -> index to (value as JSONArray) }
        .filter { it.second.length() > 0 }.forEachIndexed { index, (sectionIndex, section) ->
        if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = colors.menuSeparator)
        section.objects().forEachIndexed { itemIndex, item ->
            MenuRow(item) {
                if (item.array("sections").length() > 0) pages = pages + (sectionIndex to itemIndex)
                else item.objectOrNull("command")?.takeIf { command != null }?.let { dismiss(); command!!(it) }
                    ?: item.objectOrNull("action")?.let { action ->
                        if (!item.optBoolean("keep_open")) dismiss()
                        host.dispatch(action)
                        if (item.optBoolean("keep_open")) refresh()
                    }
            }
        }
    }
}


@Composable internal fun MenuRow(item: JSONObject, modifier: Modifier = Modifier, checks: Boolean = false,
    submenu: Boolean = item.array("sections").length() > 0, detail: String = item.optString("hint").takeUnless { it == "null" }.orEmpty(),
    pressed: Boolean = false, maxLines: Int = Int.MAX_VALUE,
    overflow: androidx.compose.ui.text.style.TextOverflow = androidx.compose.ui.text.style.TextOverflow.Clip, onClick: () -> Unit) {
    val colors = LocalPalette.current
    val heading = item.optBoolean("is_heading")
    val enabled = item.optBoolean("enabled", true) && !heading
    val checkbox = item.objectOrNull("action")?.objectOrNull("command")?.optString("type") == "show_in_switcher"
    Row(modifier.fillMaxWidth().heightIn(min = 36.dp).padding(horizontal = 6.dp)
        .then(if (checkbox) Modifier.semantics { role = Role.Checkbox; toggleableState = ToggleableState(item.optBoolean("selected")) } else Modifier)
        .clip(RoundedCornerShape(6.dp)).alpha(if (enabled || heading) 1f else .4f)
        .then(if (checkbox || item.isNull("selected")) Modifier else Modifier.semantics { selected = item.getBoolean("selected") })
        .background(if (pressed) colors.text.copy(alpha = .10f) else androidx.compose.ui.graphics.Color.Transparent)
        .clickable(enabled = enabled, onClick = onClick).padding(horizontal = 10.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        if (checks) Box(Modifier.width(16.dp), contentAlignment = Alignment.Center) { if (item.optBoolean("selected")) SharedIcon("check", null, Modifier.size(16.dp)) }
        item.optString("icon").takeIf { it.isNotEmpty() && it != "null" }?.let { SharedIcon(it, null) }
        Text(item.getString("label"), Modifier.weight(1f), fontWeight = FontWeight.Bold,
            maxLines = maxLines, overflow = overflow)
        if (!checks && item.optBoolean("selected")) SharedIcon("check", null)
        if (detail.isNotEmpty()) Text(detail, color = colors.secondary)
        if (submenu) SharedIcon("down", null, Modifier.rotate(-90f))
    }
}

@Composable internal fun ToolbarManager(host: CanvasHost, view: JSONObject) {
    val colors = LocalPalette.current
    val close = { host.customize(obj("type" to "close_toolbar_manager")) }
    val phone=host.snapshot?.getJSONObject("layout")?.objectOrNull("phone")!=null
    val content=@Composable {
            Column((if(phone)Modifier.fillMaxSize().padding(16.dp) else Modifier.width(480.dp).heightIn(max=420.dp).padding(24.dp)), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                if(!phone)Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(view.getString("title"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                    IconButton(close, Modifier.size(36.dp).semantics { contentDescription = view.getString("close_label") }.testTag("close-toolbar-manager")) {
                        SharedIcon("close", null)
                    }
                }
                Text(view.getString("description"), color = colors.settingsSecondary)
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                    val toolbars = view.array("toolbars").objects()
                    if (toolbars.isEmpty()) Box(Modifier.fillMaxWidth().padding(vertical = 60.dp), contentAlignment = Alignment.Center) {
                        Text(view.getString("empty_label"), color = colors.settingsSecondary)
                    }
                    Column(Modifier.clip(RoundedCornerShape(12.dp)).background(colors.settingsCard)) {
                        toolbars.forEachIndexed { index, toolbar ->
                            if (index > 0) HorizontalDivider(color = colors.rowDivider)
                            val id = toolbar.getString("panel")
                            val selected = view.optString("selected") == id
                            Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).background(if (selected) colors.active else colors.settingsCard)
                                .testTag("managed-toolbar-$id").selectable(selected, role = Role.RadioButton) {
                                    host.customize(obj("type" to "select_managed_toolbar", "panel" to id))
                                }.padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                SharedIcon(toolbar.getString("icon"), null)
                                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                                    Text(toolbar.getString("title"))
                                    Text(toolbar.getString("subtitle"), color = colors.settingsSecondary, fontSize = LocalTextStyle.current.fontSize * .83333f)
                                }
                            }
                        }
                    }
                }
                val delete = view.objectOrNull("delete_action")
                Button({ delete?.let(host::customize) }, enabled = delete != null, modifier = Modifier.align(Alignment.End).testTag("delete-managed-toolbar"),
                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError)) { Text(view.getString("delete_label")) }
            }
    }
    if(phone)DialogShell(host,view.getJSONObject("dialog"),view.getString("title"),onCancel=close,tag="toolbar-manager",scrollBody=false,body={content()})
    else Dialog(onDismissRequest=close) {
        Surface(shape=RoundedCornerShape(16.dp),color=colors.settingsBackground,modifier=Modifier.testTag("toolbar-manager")){content()}
    }
}

@Composable internal fun ToolbarPrompt(host: CanvasHost, view: JSONObject) {
    fun send(type: String) = host.customize(obj("type" to type))
    val close={send("cancel_toolbar")}
    val confirm={send("confirm_toolbar")}
    DialogShell(host,view.getJSONObject("dialog"),view.getString("title"),onCancel=close,onPrimary=confirm,
        primaryLabel=view.getString("confirm_label"),primaryEnabled=view.getBoolean("can_confirm"),tag="toolbar-prompt",
        responses=listOf(DialogResponse(view.getString("cancel_label"),"toolbar-prompt-cancel",action=close),
            DialogResponse(view.getString("confirm_label"),"toolbar-prompt-confirm",accent=true,enabled=view.getBoolean("can_confirm"),destructive=view.getBoolean("destructive"),action=confirm)),
        body={padding ->
            Column(Modifier.padding(padding),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                view.getString("message").takeIf { it.isNotEmpty() }?.let { message ->
                    // Keep core copy (including the accessible arrow) intact,
                    // without relying on a fallback font's low arrow glyph.
                    Text(buildAnnotatedString {
                        message.forEach { if (it == '→') appendInlineContent("menu-arrow", "→") else append(it) }
                    }, modifier = Modifier.testTag("toolbar-prompt-message"), inlineContent = mapOf(
                        "menu-arrow" to InlineTextContent(Placeholder(1.em, 1.em, PlaceholderVerticalAlign.TextCenter)) {
                            SharedIcon("back", null, Modifier.fillMaxSize().rotate(180f), tint = LocalContentColor.current)
                        }))
                }
                if (!view.isNull("name")) CoreTextField(view.getString("name"), {
                    host.customize(obj("type" to "toolbar_name", "name" to it))
                }, Modifier.testTag("toolbar-name"), label = { Text(view.getString("name_label")) }, background = LocalPalette.current.dialogField)
                view.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}
