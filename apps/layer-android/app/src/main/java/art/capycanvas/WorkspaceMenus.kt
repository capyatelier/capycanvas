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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
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
    copy: (suspend () -> JSONObject?)? = null, showTitle: Boolean = true, dismiss: () -> Unit) {
    var projected by remember(menu) { mutableStateOf(menu) }
    val currentCopy by rememberUpdatedState(copy)
    LaunchedEffect(host, host.languageTag, menu) {
        val tag = host.languageTag
        val epoch = host.menuEpoch()
        currentCopy?.invoke()?.let { if (host.languageTag == tag && host.menuEpoch() == epoch) projected = it }
    }
    val current = if (copy == null) menu else projected
    // A focusable Android popup cancels the contact in the activity that opened
    // it. Context menus must leave that contact with the original drag owner.
    BackHandler(!focusable, dismiss)
    DropdownMenu(true, dismiss, modifier = Modifier.widthIn(min = 240.dp, max = 380.dp).testTag("workspace-menu").onPreviewKeyEvent { event ->
            val key = event.nativeKeyEvent
            if (key.keyCode == KeyEvent.KEYCODE_ESCAPE) { if (key.action == KeyEvent.ACTION_DOWN) dismiss(); true } else false
        },
        properties = if (focusable) PopupProperties(focusable = true) else WindowlessMenu,
        shape = RoundedCornerShape(10.dp), containerColor = LocalPalette.current.panel) {
        WorkspaceMenuItems(host, current.array("sections"), dismiss, if (showTitle && current.has("title")) current.getString("title") else null, command)
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

@Composable internal fun ToolVariantsButton(host: CanvasHost, anchor: JSONObject, label: String, modifier: Modifier = Modifier) {
    val button = remember(anchor.toString()) { WindowlessMenuButton() }
    button.copy = { host.menuCopy(obj("type" to "context", "target" to obj("kind" to "tool_variants", "anchor" to anchor))) }
    Box(modifier.size(16.dp).semantics { contentDescription = label }
        .opensWindowlessMenu(button, label) { open ->
            host.query(obj("type" to "context", "target" to obj("kind" to "tool_variants", "anchor" to anchor))) { open(it as? JSONObject) }
        }) {
        SharedIcon("tool-group", null, Modifier.align(Alignment.BottomEnd))
        WindowlessMenuHost(host, button)
    }
}

@Composable internal fun WorkspaceMenuItems(host: CanvasHost, sections: JSONArray,
    dismiss: () -> Unit = {}, title: String? = null, command: ((JSONObject) -> Unit)? = null) {
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
        HorizontalDivider(color = colors.divider)
    } else if (title != null) Text(title, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = colors.secondary)
    (currentPage?.array("sections") ?: sections).values().mapIndexed { index, value -> index to (value as JSONArray) }
        .filter { it.second.length() > 0 }.forEachIndexed { index, (sectionIndex, section) ->
        if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = colors.divider)
        section.objects().forEachIndexed { itemIndex, item ->
            val enabled = item.optBoolean("enabled", true)
            val checkbox = item.objectOrNull("action")?.objectOrNull("command")?.optString("type") == "show_in_switcher"
            Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).padding(horizontal = 6.dp)
                .then(if (checkbox) Modifier.semantics {
                    role = Role.Checkbox
                    toggleableState = ToggleableState(item.optBoolean("selected"))
                } else Modifier)
                .clip(RoundedCornerShape(6.dp)).alpha(if (enabled) 1f else .4f)
                .then(if (checkbox || item.isNull("selected")) Modifier else Modifier.semantics { selected = item.getBoolean("selected") })
                .clickable(enabled = enabled) {
                    if (item.array("sections").length() > 0) pages = pages + (sectionIndex to itemIndex)
                    else item.objectOrNull("command")?.takeIf { command != null }?.let { dismiss(); command!!(it) }
                        ?: item.objectOrNull("action")?.let { action -> dismiss(); host.dispatch(action) }
                }.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                item.optString("icon").takeIf { it.isNotEmpty() && it != "null" }?.let { SharedIcon(it, null) }
                Text(item.getString("label"), Modifier.weight(1f), fontWeight = FontWeight.Bold)
                if (item.optBoolean("selected")) SharedIcon("check", null)
                item.optString("hint").takeIf { it.isNotEmpty() && it != "null" }?.let { Text(it, color = colors.secondary) }
                if (item.array("sections").length() > 0) SharedIcon("down", null, Modifier.rotate(-90f))
            }
        }
    }
}

@Composable internal fun ToolbarManager(host: CanvasHost, view: JSONObject) {
    val colors = LocalPalette.current
    val close = { host.customize(obj("type" to "close_toolbar_manager")) }
    Dialog(onDismissRequest = close) {
        Surface(shape = RoundedCornerShape(16.dp), color = colors.settingsBackground, modifier = Modifier.testTag("toolbar-manager")) {
            Column(Modifier.width(480.dp).heightIn(max = 420.dp).padding(24.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
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
                            if (index > 0) HorizontalDivider(color = colors.divider)
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
    }
}

@Composable internal fun ToolbarPrompt(host: CanvasHost, view: JSONObject) {
    fun send(type: String) = host.customize(obj("type" to type))
    AlertDialog(onDismissRequest = { send("cancel_toolbar") },
        modifier = Modifier.testTag("toolbar-prompt"),
        title = { Text(view.getString("title"), style = MaterialTheme.typography.titleLarge) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
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
                }, Modifier.testTag("toolbar-name"), label = { Text(view.getString("name_label")) })
                view.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        },
        dismissButton = { TextButton({ send("cancel_toolbar") }) { Text(view.getString("cancel_label")) } },
        confirmButton = {
            Button({ send("confirm_toolbar") }, enabled = view.getBoolean("can_confirm"),
                colors = if (view.getBoolean("destructive")) ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.error,
                    contentColor = MaterialTheme.colorScheme.onError)
                    else ButtonDefaults.buttonColors()) { Text(view.getString("confirm_label")) }
        })
}
