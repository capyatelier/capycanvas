package art.capycanvas

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import org.json.JSONObject

@Composable internal fun PreferencesScreen(host: CanvasHost, view: JSONObject) {
    val colors = LocalPalette.current
    val searching = view.optBoolean("searching")
    var showPage by remember { mutableStateOf(false) }
    BackHandler { host.dispatch(obj("type" to "cancel_settings")) }
    Surface(Modifier.fillMaxSize(), color = colors.panel) {
        BoxWithConstraints {
            val wide = maxWidth >= 700.dp
            Column {
                Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    TextButton({ if (!wide && showPage) showPage = false else host.dispatch(obj("type" to "cancel_settings")) }) { Text("Back") }
                    Text("Preferences", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                    TextButton({ host.dispatch(obj("type" to "apply_settings")) }) { Text("Save") }
                }
                HorizontalDivider()
                Row(Modifier.weight(1f)) {
                    if (wide || !showPage) Column(Modifier.then(if (wide) Modifier.width(248.dp) else Modifier.fillMaxWidth())
                        .fillMaxHeight().background(colors.tabs).padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        if (searching) CoreTextField(view.optString("query"), { host.preference(obj("type" to "search", "query" to it)) },
                            modifier = Modifier.fillMaxWidth(), placeholder = { Text("Search preferences") },
                            leadingIcon = { SharedIcon("search", null) }, trailingIcon = { TextButton({ host.preference(obj("type" to "toggle_search", "open" to false)) }) { Text("×") } })
                        else IconTile("search", "Search preferences") { host.preference(obj("type" to "toggle_search", "open" to true)) }
                        Column(Modifier.verticalScroll(rememberScrollState())) {
                            if (searching) view.array("search_results").objects().forEach { result ->
                                ListItem(headlineContent = { Text(result.getString("title")) }, supportingContent = { Text(result.getString("description")) },
                                    modifier = Modifier.clickable { host.preference(result.getJSONObject("action")); showPage = true }, colors = ListItemDefaults.colors(containerColor = colors.tabs))
                            } else view.array("pages").objects().forEach { page ->
                                val selected = view.getString("page") == page.getString("id")
                                Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).background(if (selected) colors.active else colors.tabs, RoundedCornerShape(8.dp))
                                    .clickable { host.preference(obj("type" to "page", "page" to page.getString("id"))); showPage = true }.padding(12.dp),
                                    horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                                    SharedIcon(page.getString("icon"), null)
                                    Text(page.getString("title"))
                                }
                            }
                        }
                    }
                    if (wide || showPage) Column(Modifier.weight(1f).fillMaxHeight().verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                        val page = view.array("pages").objects().find { it.getString("id") == view.getString("page") }
                        page?.let {
                            Text(it.getString("title"), style = MaterialTheme.typography.headlineSmall)
                            if (view.getString("page") == "shortcuts") Shortcuts(host, view)
                            else it.array("groups").objects().forEach { group ->
                                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Text(group.getString("title"), style = MaterialTheme.typography.titleMedium)
                                    Surface(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), color = colors.input) {
                                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                                            group.array("rows").objects().filter { row -> row.optBoolean("visible", true) }.forEachIndexed { index, row ->
                                                if (index > 0) HorizontalDivider(color = colors.tabs)
                                                PreferenceRow(host, row)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        view.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                    }
                }
            }
        }
    }
    view.objectOrNull("shortcut_editor")?.let { ShortcutEditor(host, view, it) }
}

@Composable private fun PreferenceRow(host: CanvasHost, row: JSONObject) {
    val kind = row.getJSONObject("kind")
    val context = LocalContext.current
    val enabled = row.optBoolean("enabled", true)
    fun edit(value: Any) = host.preference(obj("type" to "edit", "id" to row.getString("id"), "value" to value))
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        when (kind.getString("type")) {
            "number" -> NumericSetting(row.getString("title"), kind.number("value"), kind.getJSONObject("control")) { if (enabled) edit(it) }
            "switch" -> Row(verticalAlignment = Alignment.CenterVertically) {
                Text(row.getString("title"), Modifier.weight(1f))
                Switch(kind.getBoolean("active"), { edit(it) }, enabled = enabled)
            }
            "choice" -> {
                var expanded by remember { mutableStateOf(false) }
                Text(row.getString("title"))
                Box {
                    OutlinedButton({ expanded = true }, enabled = enabled) {
                        kind.array("icons").optString(kind.getInt("selected")).takeIf { it.isNotEmpty() }?.let { SharedIcon(it, null); Spacer(Modifier.width(8.dp)) }
                        Text(kind.array("options").getString(kind.getInt("selected")))
                    }
                    DropdownMenu(expanded, { expanded = false }) {
                        kind.array("options").values().forEachIndexed { index, label ->
                            DropdownMenuItem(text = { Text(label.toString()) }, leadingIcon = {
                                kind.array("icons").optString(index).takeIf { it.isNotEmpty() }?.let { SharedIcon(it, null) }
                            }, onClick = { expanded = false; edit(index) })
                        }
                    }
                }
            }
            "link" -> {
                Text(row.getString("title"))
                TextButton({ context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(kind.getString("url")))) }) { Text(kind.getString("label")) }
            }
            else -> {
                Text(row.getString("title"))
                Text(kind.optString("value"), color = LocalPalette.current.secondary)
            }
        }
        row.optString("description").takeIf { it.isNotEmpty() }?.let { Text(it, color = LocalPalette.current.secondary) }
    }
}
@Composable private fun Shortcuts(host: CanvasHost, view: JSONObject) {
    CoreTextField(view.optString("shortcut_query"), { host.preference(obj("type" to "search_shortcuts", "query" to it)) },
        modifier = Modifier.fillMaxWidth(), placeholder = { Text("Search keyboard shortcuts") }, leadingIcon = { SharedIcon("search", null) })
    view.array("shortcuts").objects().forEach { shortcut ->
        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).clickable { host.preference(obj("type" to "edit_shortcut", "id" to shortcut.getString("id"))) }.padding(8.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Text(shortcut.getString("label"), Modifier.weight(1f))
            Text(shortcut.getString("shortcut"), color = LocalPalette.current.secondary)
        }
    }
}
@Composable private fun ShortcutEditor(host: CanvasHost, view: JSONObject, editor: JSONObject) {
    val capture = view.objectOrNull("capture")
    AlertDialog(onDismissRequest = { host.preference(obj("type" to "close_shortcut_editor")) },
        title = { Text(editor.getString("label")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Default: " + editor.array("defaults").values().joinToString(" / "))
                editor.array("bindings").values().forEachIndexed { index, binding ->
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(binding.toString(), Modifier.weight(1f))
                        IconTile("minus", "Remove shortcut") { host.preference(obj("type" to "remove_shortcut", "id" to editor.getString("id"), "index" to index)) }
                    }
                }
                if (capture != null) {
                    Text("Press a key combination on your keyboard")
                    Text(capture.optString("shortcut"))
                    capture.optString("conflict").takeIf { it.isNotEmpty() && it != "null" }?.let { conflict ->
                        Text("Already assigned to $conflict")
                        TextButton({ host.preference(obj("type" to "confirm_shortcut", "replace" to true)) }) { Text("Replace assignment") }
                    }
                    TextButton({ host.preference(obj("type" to "confirm_shortcut", "replace" to false)) }) { Text("Use shortcut") }
                    TextButton({ host.preference(obj("type" to "cancel_shortcut")) }) { Text("Cancel recording") }
                } else TextButton({ host.preference(obj("type" to "begin_shortcut", "id" to editor.getString("id"))) }, enabled = editor.optBoolean("can_add")) { Text("Add shortcut") }
                TextButton({ host.preference(obj("type" to "reset_shortcut", "id" to editor.getString("id"))) }, enabled = editor.optBoolean("modified")) { Text("Restore default") }
                view.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        }, confirmButton = { TextButton({ host.preference(obj("type" to "close_shortcut_editor")) }) { Text("Done") } })
}

@Composable internal fun ToolPicker(host: CanvasHost, picker: JSONObject) {
    Dialog({ host.customize(obj("type" to "cancel_tools")) }) {
        Surface(shape = RoundedCornerShape(16.dp)) {
            Column(Modifier.widthIn(max = 560.dp).heightIn(max = 650.dp).padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(picker.getString("title"), style = MaterialTheme.typography.titleLarge)
                picker.optString("name").takeIf { !picker.isNull("name") }?.let { name ->
                    CoreTextField(name, { host.customize(obj("type" to "picker_name", "name" to it)) },
                        label = { Text(picker.getString("name_label")) })
                }
                CoreTextField(picker.optString("query"), { host.customize(obj("type" to "picker_search", "query" to it)) },
                    placeholder = { Text(picker.getString("search_hint")) })
                Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                    picker.array("choices").objects().forEach { tool ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(tool.optBoolean("selected"), { selected -> host.customize(obj("type" to "picker_select", "control" to tool.getJSONObject("control"), "selected" to selected)) })
                            Text(tool.getString("label"))
                        }
                    }
                }
                Row(Modifier.align(Alignment.End)) {
                    TextButton({ host.customize(obj("type" to "cancel_tools")) }) { Text("Cancel") }
                    Button({ host.customize(obj("type" to "confirm_tools")) }, enabled = picker.getBoolean("can_confirm")) { Text(picker.getString("confirm_label")) }
                }
            }
        }
    }
}
