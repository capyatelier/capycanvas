package art.capycanvas

import android.view.WindowManager
import android.view.KeyEvent
import androidx.compose.foundation.horizontalScroll
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import org.json.JSONObject
import org.json.JSONArray

/** Compose owns focus/scroll/input; Rust owns records, selections and previews. */
@Composable internal fun WorkspaceManager(host: CanvasHost) {
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
    val view = host.workspaceManager ?: return
    val page = view.optString("page").takeUnless { it == "null" || it.isEmpty() }
    val form = view.objectOrNull("prompt")
    val error = view.optString("error").takeUnless { it == "null" || it.isEmpty() }
    val busy = view.optBoolean("busy")
    val rowInteraction = remember { WorkspaceRowInteraction() }
    val focusWindow = view.optJSONObject("focus_window")?.optString("id")
    LaunchedEffect(focusWindow) { if (focusWindow != null) host.reportActionError(copy.getString("owned_elsewhere")) }
    val colors = LocalPalette.current
    fun send(type: String) = host.workspaceInput(obj("type" to type))
    val cancel = { send("cancel") }
    if (page != null) Dialog(onDismissRequest = {
        if (rowInteraction.active != null || rowInteraction.menu != null) rowInteraction.cancel() else cancel()
    }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        PreviewBackdrop()
        Surface(shape = RoundedCornerShape(16.dp), color = colors.settingsBackground,
            modifier = Modifier.widthIn(max = 480.dp).fillMaxWidth(.94f).testTag("workspace-manager").onPreviewKeyEvent { event ->
                if (event.nativeKeyEvent.keyCode == KeyEvent.KEYCODE_ESCAPE && event.nativeKeyEvent.action == KeyEvent.ACTION_DOWN &&
                    (rowInteraction.active != null || rowInteraction.menu != null || rowInteraction.contact)) { rowInteraction.cancel(); true } else false
            }) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(view.getString("title"), Modifier.weight(1f), fontWeight = FontWeight.Bold)
                    if (page != "history") FilledTonalIconButton({ host.workspaceInput(obj("type" to "form", "action" to obj("type" to "new"))) },
                        enabled = !busy, modifier = Modifier.size(32.dp).testTag("new-workspace").semantics { contentDescription = copy.getString("new_workspace") }, shape = RoundedCornerShape(6.dp)) { SharedIcon("plus", null) }
                    IconButton(cancel, Modifier.size(32.dp).semantics { contentDescription = host.bootstrap!!.getJSONObject("common").getString("close") }) { SharedIcon("close", null) }
                }
                view.getString("intro").takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.settingsSecondary) }
                WorkspaceRows(host, view, rowInteraction, Modifier.weight(1f, fill = false).height(315.dp).fillMaxWidth())
                view.optString("switcher_error").takeUnless { it == "null" || it.isEmpty() }?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
                if (error != null && form == null) Text(error, color = MaterialTheme.colorScheme.error)
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(cancel, Modifier.weight(1f).testTag("workspace-cancel")) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) }
                    if (view.optBoolean("retry")) TextButton({ send("retry") }, enabled = !busy) { Text(copy.getString("retry")) }
                    Button({ send("confirm") }, Modifier.weight(1f).testTag("workspace-confirm"), enabled = view.optBoolean("enabled") && !busy) { Text(view.getString("primary")) }
                }
            }
        }
    }
    if (form != null) {
        val action = view.getJSONObject("prompt_action")
        val named = !form.isNull("name")
        var name by remember(action.toString(), form.optString("name")) { mutableStateOf(if (named) form.getString("name") else "") }
        val title = form.getString("title")
        AlertDialog(onDismissRequest = cancel, modifier = Modifier.testTag("workspace-form"), title = { Text(title) }, text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                form.getString("message").takeIf { it.isNotEmpty() }?.let { Text(it) }
                if (named) CoreTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("workspace-name"), label = { Text(host.bootstrap!!.getJSONObject("common").getString("name")) })
                if (error != null) Text(error, color = MaterialTheme.colorScheme.error)
            }
        }, dismissButton = { TextButton(cancel) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) } }, confirmButton = {
            TextButton({ host.workspaceInput(obj("type" to "submit", "name" to name)) }, enabled = !busy,
                modifier = Modifier.testTag("workspace-submit")) { Text(if (view.optBoolean("retry") && action.getString("type") != "save_as_new") copy.getString("retry") else form.getString("confirm"),
                color = if (form.optBoolean("destructive")) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary) }
        })
    }
    var dismissedError by remember { mutableStateOf<String?>(null) }
    if (error == null) dismissedError = null
    if (error != null && page == null && form == null && error != dismissedError) AlertDialog(
        onDismissRequest = { dismissedError = error; send("resume") }, title = { Text(copy.getString("workspaces")) }, text = { Text(error) },
        dismissButton = { TextButton({ dismissedError = error; send("resume") }) { Text(host.bootstrap!!.getJSONObject("common").getString("keep_open")) } }, confirmButton = {
            Column {
                TextButton({ send("retry") }) { Text(copy.getString("retry")) }
                TextButton({ host.workspaceInput(obj("type" to "form", "action" to obj("type" to "save_as_new"))) }) { Text(copy.getString("save_as_new_workspace")) }
            }
        })
}

internal fun workspaceSwitcherMenu(view: JSONObject?): JSONObject = view?.optJSONObject("switcher_menu") ?: obj("sections" to JSONArray())

@Composable internal fun WorkspaceSwitcher(host: CanvasHost, modifier: Modifier = Modifier, interactive: Boolean = true, options: () -> Unit) {
    val view = host.workspaceManager ?: return
    val colors = LocalPalette.current
    val choices = view.array("switcher_display").objects()
    val scroll = rememberScrollState()
    LaunchedEffect(choices.firstOrNull()?.optString("id"), view.optString("id")) {
        if (choices.firstOrNull()?.optString("id") == view.optString("id")) scroll.scrollTo(0)
    }
    Row(modifier.height(36.dp).clip(SquircleShape(50)).glass(SquircleShape(50), colors.switcher)
        .testTag("workspace-switcher").padding(end = 5.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        Row(Modifier.weight(1f).horizontalScroll(scroll).padding(start = 5.dp, top = 5.dp, bottom = 5.dp)
            .testTag("workspace-switcher-choices"), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            choices.forEach { row ->
                val id = row.getString("id")
                val selected = view.optString("id") == id
                Box(Modifier.widthIn(max = 128.dp).height(26.dp).clip(SquircleShape(50))
                    .background(if (selected) colors.switcherActive else Color.Transparent)
                    .selectable(selected, enabled = interactive && view.optBoolean("ready") && !view.optBoolean("busy") && view.isNull("page") && view.isNull("prompt"), role = Role.RadioButton) {
                        host.workspaceInput(obj("type" to "switch", "id" to id))
                    }.padding(horizontal = 8.dp).testTag("workspace-switch-$id"), contentAlignment = Alignment.Center) {
                    Text(row.getString("title"), maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
                }
            }
        }
        HeaderButton(view.getString("switcher_options_label"), false, true, false,
            Modifier.size(20.dp, 26.dp).testTag("workspace-switcher-options"), surface = false, shape = SquircleShape(50), onClick = options) {
            SharedIcon("more-small", view.getString("switcher_options_label"), Modifier.size(16.dp), tint = colors.secondary)
        }
    }
    if (!view.isNull("error")) TextButton({ host.workspaceInput(obj("type" to "retry")) }, enabled = !view.optBoolean("busy")) { Text(host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("retry")) }
}

@Composable private fun PreviewBackdrop() {
    val view = LocalView.current
    SideEffect { (view.parent as? DialogWindowProvider)?.window?.apply {
        addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
        setDimAmount(.18f)
    } }
}
