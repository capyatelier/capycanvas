package art.capycanvas

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import org.json.JSONObject

@Composable internal fun PreferencesScreen(host: CanvasHost, view: JSONObject) {
    val colors = LocalPalette.current
    var showPage by remember { mutableStateOf(view.getString("page") != "appearance") }
    val page = view.array("pages").objects().find { it.getString("id") == view.getString("page") }
    Dialog({ host.dispatch(obj("type" to "cancel_settings")) },
        properties = DialogProperties(usePlatformDefaultWidth = false)) {
        BoxWithConstraints(Modifier.fillMaxSize().imePadding(),
            contentAlignment = Alignment.Center) {
            val wide = maxWidth >= 840.dp
            val inset = if (wide) 24.dp else 0.dp
            fun back() { if (!wide && showPage) showPage = false else host.dispatch(obj("type" to "cancel_settings")) }
            BackHandler(onBack = ::back)
            Surface(Modifier.padding(inset).then(if (wide) Modifier.widthIn(max = 960.dp).heightIn(max = 720.dp) else Modifier)
                .fillMaxSize().testTag("preferences-surface"),
                color = colors.settingsBackground, shape = RoundedCornerShape(if (inset > 0.dp) 20.dp else 0.dp)) {
                Column {
                    Row(Modifier.fillMaxWidth().height(56.dp).padding(horizontal = 12.dp),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton(::back) { Text("Back") }
                        Text(if (!wide && showPage) page?.getString("title") ?: "Preferences" else "Preferences",
                            Modifier.weight(1f), fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                        Button({ host.dispatch(obj("type" to "apply_settings")) }, shape = RoundedCornerShape(8.dp),
                            contentPadding = PaddingValues(horizontal = 18.dp)) { Text("Save") }
                    }
                    HorizontalDivider(color = colors.divider)
                    Row(Modifier.weight(1f)) {
                        if (wide || !showPage) PreferencesNavigation(host, view,
                            Modifier.then(if (wide) Modifier.width(240.dp) else Modifier.fillMaxWidth()).fillMaxHeight()) { showPage = true }
                        if (wide || showPage) {
                            Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.TopCenter) {
                                key(view.getString("page")) {
                                    Column(Modifier.widthIn(max = 680.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp),
                                        verticalArrangement = Arrangement.spacedBy(24.dp)) {
                                        if (wide) Text(page?.getString("title") ?: "", fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
                                        if (view.getString("page") == "shortcuts") Shortcuts(host, view)
                                        else page?.array("groups")?.objects()?.forEach { group ->
                                            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                                                Text(group.getString("title"), Modifier.padding(horizontal = 4.dp), fontWeight = FontWeight.Bold)
                                                Surface(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
                                                    color = colors.settingsCard, shadowElevation = 1.dp) {
                                                    Column {
                                                        group.array("rows").objects().filter { it.optBoolean("visible", true) }.forEachIndexed { index, row ->
                                                            if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp), color = colors.divider)
                                                            key(row.getString("id")) { PreferenceRow(host, row) }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        view.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let {
                                            Text(it, color = MaterialTheme.colorScheme.error)
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        view.objectOrNull("shortcut_editor")?.let { ShortcutEditor(host, view, it) }
    }
}

@Composable private fun PreferencesNavigation(host: CanvasHost, view: JSONObject, modifier: Modifier, onPage: () -> Unit) {
    val colors = LocalPalette.current
    val focus = androidx.compose.ui.platform.LocalFocusManager.current
    val searching = view.optBoolean("searching")
    Column(modifier.background(colors.tabs).padding(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (searching) CoreTextField(view.optString("query"), { host.preference(obj("type" to "search", "query" to it)) },
            Modifier.fillMaxWidth(), placeholder = { Text("Search preferences") },
            leadingIcon = { SharedIcon("search", null) },
            trailingIcon = {
                Box(Modifier.size(24.dp).clip(CircleShape).clickable {
                    focus.clearFocus(); host.preference(obj("type" to "toggle_search", "open" to false))
                }, contentAlignment = Alignment.Center) { Text("×", fontSize = 20.sp) }
            })
        else IconTile("search", "Search preferences") { host.preference(obj("type" to "toggle_search", "open" to true)) }
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (searching) {
                view.array("search_results").objects().forEach { result ->
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable {
                        focus.clearFocus(); host.preference(result.getJSONObject("action")); onPage()
                    }.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(result.getString("title"), fontWeight = FontWeight.Medium)
                        Text(result.getString("description"), color = colors.settingsSecondary)
                    }
                }
            } else view.array("pages").objects().forEach { page ->
                val selected = view.getString("page") == page.getString("id")
                Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp))
                    .background(if (selected) colors.active else Color.Transparent)
                    .clickable { focus.clearFocus(); host.preference(obj("type" to "page", "page" to page.getString("id"))); onPage() }
                    .padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    SharedIcon(page.getString("icon"), null)
                    Text(page.getString("title"), fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
                }
            }
        }
    }
}

@Composable private fun PreferenceRow(host: CanvasHost, row: JSONObject) {
    val colors = LocalPalette.current
    val kind = row.getJSONObject("kind")
    val context = LocalContext.current
    val enabled = row.optBoolean("enabled", true)
    val title = row.getString("title")
    val description = row.optString("description")
    fun edit(value: Any) = host.preference(obj("type" to "edit", "id" to row.getString("id"), "value" to value))
    BoxWithConstraints(Modifier.fillMaxWidth().testTag("preference-" + row.getString("id")).padding(16.dp)) {
        val narrow = maxWidth < 400.dp
        @Composable fun caption(modifier: Modifier = Modifier) {
            Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, fontWeight = FontWeight.Medium)
                if (description.isNotEmpty()) Text(description, color = colors.settingsSecondary, lineHeight = 20.sp)
            }
        }
        @Composable fun control() {
            when (kind.getString("type")) {
                "number" -> NumberStepper(title, kind.number("value"), kind.getJSONObject("control"), enabled = enabled, onChange = ::edit)
                "switch" -> Switch(kind.getBoolean("active"), { edit(it) }, enabled = enabled)
                "choice" -> {
                    var expanded by remember { mutableStateOf(false) }
                    Box {
                        TextButton({ expanded = true }, enabled = enabled, shape = RoundedCornerShape(6.dp),
                            contentPadding = PaddingValues(horizontal = 8.dp)) {
                            kind.array("icons").optString(kind.getInt("selected")).takeIf { it.isNotEmpty() }?.let {
                                SharedIcon(it, null); Spacer(Modifier.width(8.dp))
                            }
                            Text(kind.array("options").getString(kind.getInt("selected")))
                            Spacer(Modifier.width(8.dp)); SharedIcon("chevron-down", null)
                        }
                        DropdownMenu(expanded, { expanded = false }, shape = RoundedCornerShape(12.dp), containerColor = colors.panel) {
                            kind.array("options").values().forEachIndexed { index, label ->
                                DropdownMenuItem(text = { Text(label.toString()) }, leadingIcon = {
                                    kind.array("icons").optString(index).takeIf { it.isNotEmpty() }?.let { SharedIcon(it, null) }
                                }, trailingIcon = { if (index == kind.getInt("selected")) SharedIcon("check", null) },
                                    onClick = { expanded = false; edit(index) })
                            }
                        }
                    }
                }
                else -> SelectionContainer { Text(kind.optString("value"), color = colors.settingsSecondary) }
            }
        }
        if (kind.getString("type") == "link") {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, fontWeight = FontWeight.Medium)
                TextButton({ context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(kind.getString("url")))) },
                    contentPadding = PaddingValues(0.dp)) { Text(kind.getString("label")) }
            }
        } else if (narrow && kind.getString("type") != "switch") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) { caption(); Box(Modifier.align(Alignment.End)) { control() } }
        } else Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            caption(Modifier.weight(1f)); control()
        }
    }
}
@Composable private fun Shortcuts(host: CanvasHost, view: JSONObject) {
    CoreTextField(view.optString("shortcut_query"), { host.preference(obj("type" to "search_shortcuts", "query" to it)) },
        modifier = Modifier.fillMaxWidth(), placeholder = { Text("Search keyboard shortcuts") }, leadingIcon = { SharedIcon("search", null) })
    val colors = LocalPalette.current
    Surface(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), color = colors.settingsCard, shadowElevation = 1.dp) {
        Column {
            view.array("shortcuts").objects().filter { it.getBoolean("visible") }.forEachIndexed { index, shortcut ->
                if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp), color = colors.divider)
                Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
                    .clickable { host.preference(obj("type" to "edit_shortcut", "id" to shortcut.getString("id"))) }.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Text(shortcut.getString("label"), Modifier.weight(1f))
                    Text(shortcut.getString("shortcut"), color = colors.settingsSecondary)
                }
            }
        }
    }
}
@Composable private fun ShortcutEditor(host: CanvasHost, view: JSONObject, editor: JSONObject) {
    val capture = view.objectOrNull("capture")
    val focus = remember { FocusRequester() }
    LaunchedEffect(capture != null) { if (capture != null) focus.requestFocus() }
    AlertDialog(onDismissRequest = { host.preference(obj("type" to "close_shortcut_editor")) },
        modifier = Modifier.onPreviewKeyEvent { event ->
            if (capture != null) { host.key(event.nativeKeyEvent); true } else false
        }.focusRequester(focus).focusable(),
        title = { Text(editor.getString("label"), fontSize = 20.sp, fontWeight = FontWeight.SemiBold) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Default: " + editor.array("defaults").values().joinToString(" / "))
                editor.array("bindings").values().forEachIndexed { index, binding ->
                    Row(Modifier.fillMaxWidth().background(LocalPalette.current.input, RoundedCornerShape(8.dp)).padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(binding.toString(), Modifier.weight(1f), color = LocalPalette.current.text, fontWeight = FontWeight.Medium)
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
                    TextButton({ host.preference(obj("type" to "confirm_shortcut", "replace" to false)) }, enabled = capture.objectOrNull("chord") != null) { Text("Use shortcut") }
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
                Text(picker.getString("title"), fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                picker.optString("name").takeIf { !picker.isNull("name") }?.let { name ->
                    CoreTextField(name, { host.customize(obj("type" to "picker_name", "name" to it)) }, Modifier.fillMaxWidth().testTag("toolbar-name"),
                        label = { Text(picker.getString("name_label")) })
                }
                CoreTextField(picker.optString("query"), { host.customize(obj("type" to "picker_search", "query" to it)) },
                    placeholder = { Text(picker.getString("search_hint")) })
                Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                    picker.array("choices").objects().forEachIndexed { index, tool ->
                        if (index > 0) HorizontalDivider(color = LocalPalette.current.divider)
                        Row(Modifier.fillMaxWidth().heightIn(min = 58.dp).toggleable(tool.optBoolean("selected"), role = Role.Checkbox) { selected ->
                            host.customize(obj("type" to "picker_select", "control" to tool.getJSONObject("control"), "selected" to selected))
                        }.padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                            tool.optString("icon").takeIf { it.isNotEmpty() && it != "null" }?.let { SharedIcon(it, null) }
                            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                Text(tool.getString("label"), fontWeight = FontWeight.Medium)
                                Text(tool.getString("description"), color = LocalPalette.current.settingsSecondary)
                            }
                            CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides 0.dp) {
                                Checkbox(tool.optBoolean("selected"), onCheckedChange = null, modifier = Modifier.size(20.dp))
                            }
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
