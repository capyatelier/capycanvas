package art.capycanvas

import android.view.WindowManager
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
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

/** Compose owns focus/scroll/input; Rust owns records, selections and previews. */
@Composable internal fun WorkspaceManager(host: CanvasHost) {
    val view = host.workspaceManager ?: return
    val page = view.optString("page").takeUnless { it == "null" || it.isEmpty() }
    val form = view.objectOrNull("form")
    val error = view.optString("error").takeUnless { it == "null" || it.isEmpty() }
    val busy = view.optBoolean("busy")
    val focusWindow = view.optString("focus_window").takeUnless { it == "null" || it.isEmpty() }
    LaunchedEffect(focusWindow) { if (focusWindow != null && !MainActivity.focusWorkspace(focusWindow)) host.reportActionError("This workspace is open in another window. Switch to that window to continue.") }
    val colors = LocalPalette.current
    fun send(type: String) = host.workspaceInput(obj("type" to type))
    val cancel = { send("cancel") }
    if (page != null) Dialog(onDismissRequest = cancel, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        PreviewBackdrop()
        Surface(shape = RoundedCornerShape(16.dp), color = colors.settingsBackground,
            modifier = Modifier.widthIn(max = 480.dp).fillMaxWidth(.94f).testTag("workspace-manager")) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(view.getString("title"), Modifier.weight(1f), fontWeight = FontWeight.Bold)
                    if (page != "history") FilledTonalIconButton({ host.workspaceInput(obj("type" to "form", "kind" to "new")) },
                        enabled = !busy, modifier = Modifier.size(32.dp).testTag("new-workspace").semantics { contentDescription = "New Workspace" }, shape = RoundedCornerShape(6.dp)) { Text("+", fontSize = 22.sp) }
                    IconButton(cancel, Modifier.size(32.dp).semantics { contentDescription = "Close" }) { Text("×", fontSize = 22.sp) }
                }
                view.getString("intro").takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.settingsSecondary) }
                var query by remember(page) { mutableStateOf("") }
                CoreTextField(query, { query = it; host.workspaceInput(obj("type" to "filter", "query" to it)) },
                    Modifier.fillMaxWidth().testTag("workspace-search"), label = { Text("Search") })
                val rows = view.array("rows").objects()
                Column(Modifier.weight(1f, fill = false).height(315.dp).fillMaxWidth().clip(RoundedCornerShape(10.dp))
                    .background(colors.settingsCard).verticalScroll(rememberScrollState())) {
                    rows.forEachIndexed { index, row ->
                        val id = row.getString("id")
                        val selected = view.optString("selected") == id
                        if (index > 0) HorizontalDivider(color = colors.divider)
                        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).background(if (selected) colors.active else colors.settingsCard), verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f).selectable(selected, enabled = !busy, role = Role.RadioButton) {
                                host.workspaceInput(obj("type" to "select", "id" to id))
                            }.padding(12.dp).testTag("workspace-row-$id"), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                                Text(row.getString("title"))
                                row.getString("subtitle").takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.settingsSecondary, fontSize = LocalTextStyle.current.fontSize * .88f) }
                            }
                            if (row.optBoolean("options")) {
                                var options by remember(id) { mutableStateOf(false) }
                                Box {
                                    IconButton({ options = true }, enabled = !busy,
                                        modifier = Modifier.size(36.dp).testTag("workspace-options-$id").semantics { contentDescription = "Options for ${row.getString("title")}" }) { Text("⋮", fontSize = 22.sp) }
                                    DropdownMenu(options, { options = false }) {
                                        for ((kind, label) in (listOf("rename" to "Rename…") + if (row.optBoolean("delete")) listOf("delete" to "Delete…") else emptyList())) DropdownMenuItem(text = { Text(label) }, onClick = {
                                            options = false; host.workspaceInput(obj("type" to "form", "kind" to kind, "id" to id))
                                        }, modifier = Modifier.testTag("workspace-$kind"))
                                    }
                                }
                            }
                        }
                    }
                }
                if (error != null && form == null) Text(error, color = MaterialTheme.colorScheme.error)
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(cancel, Modifier.weight(1f).testTag("workspace-cancel")) { Text("Cancel") }
                    if (view.optBoolean("retry")) TextButton({ send("retry") }, enabled = !busy) { Text("Retry") }
                    Button({ send("confirm") }, Modifier.weight(1f).testTag("workspace-confirm"), enabled = view.optBoolean("enabled") && !busy) { Text(view.getString("primary")) }
                }
            }
        }
    }
    if (form != null) {
        val kind = form.getString("kind")
        var name by remember(kind, form.optString("id"), form.getString("name")) { mutableStateOf(form.getString("name")) }
        val title = form.getString("title")
        AlertDialog(onDismissRequest = cancel, modifier = Modifier.testTag("workspace-form"), title = { Text(title) }, text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                form.getString("message").takeIf { it.isNotEmpty() }?.let { Text(it) }
                if (kind !in listOf("delete", "reset", "reset_brushes")) CoreTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("workspace-name"), label = { Text("Name") })
                if (error != null) Text(error, color = MaterialTheme.colorScheme.error)
            }
        }, dismissButton = { TextButton(cancel) { Text("Cancel") } }, confirmButton = {
            TextButton({ host.workspaceInput(obj("type" to "submit", "name" to name, "source" to null)) }, enabled = !busy,
                modifier = Modifier.testTag("workspace-submit")) { Text(if (view.optBoolean("retry") && kind != "recover") "Retry" else form.getString("confirm"),
                color = if (kind == "delete") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary) }
        })
    }
    var dismissedError by remember { mutableStateOf<String?>(null) }
    if (error == null) dismissedError = null
    if (error != null && page == null && form == null && error != dismissedError) AlertDialog(
        onDismissRequest = { dismissedError = error; send("resume") }, title = { Text("Workspace could not be saved") }, text = { Text(error) },
        dismissButton = { TextButton({ dismissedError = error; send("resume") }) { Text("Keep Open") } }, confirmButton = {
            Column {
                TextButton({ send("retry") }) { Text("Retry") }
                TextButton({ host.workspaceInput(obj("type" to "form", "kind" to "recover")) }) { Text("Save as New Workspace…") }
            }
        })
}

@Composable internal fun WorkspaceSwitcher(host: CanvasHost) {
    val view = host.workspaceManager ?: return
    val colors = LocalPalette.current
    Row(Modifier.clip(RoundedCornerShape(9.dp)).background(colors.text.copy(alpha = .08f)).padding(3.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        view.array("defaults").objects().forEach { row ->
            val id = row.getString("id")
            val selected = view.optString("id") == id
            Box(Modifier.widthIn(max = 128.dp).heightIn(min = 28.dp).clip(RoundedCornerShape(6.dp))
                .background(if (selected) colors.active else androidx.compose.ui.graphics.Color.Transparent)
                .selectable(selected, enabled = view.optBoolean("ready") && !view.optBoolean("busy") && view.isNull("page") && view.isNull("form"), role = Role.RadioButton) {
                    host.workspaceInput(obj("type" to "switch", "id" to id))
                }.padding(horizontal = 10.dp, vertical = 4.dp).testTag("workspace-switch-$id"), contentAlignment = Alignment.Center) {
                Text(row.getString("title"), maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
            }
        }
    }
    if (!view.isNull("error")) TextButton({ host.workspaceInput(obj("type" to "retry")) }, enabled = !view.optBoolean("busy")) { Text("Retry workspace save") }
}

@Composable private fun PreviewBackdrop() {
    val view = LocalView.current
    SideEffect { (view.parent as? DialogWindowProvider)?.window?.apply {
        addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
        setDimAmount(.18f)
    } }
}
