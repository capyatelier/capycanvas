package art.capycanvas

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

internal const val MODIFIER_SECTION = "Modifier keys"
private const val MODIFIER_CAPTURE = "modifier"
private val textEditing = setOf("a", "c", "v", "x", "backspace", "delete", "arrowleft", "arrowright", "home", "end")

/** A pressed chord for looking up shortcuts; plain typing stays in the field. */
internal fun searchChord(event: android.view.KeyEvent): JSONObject? {
    if (event.action != android.view.KeyEvent.ACTION_DOWN) return null
    val name = keyName(event) ?: return null
    if (name in listOf("shift", "alt", "control", "meta")) return null
    val chorded = event.isCtrlPressed || event.isAltPressed || event.isMetaPressed
    val named = Regex("f\\d+").matches(name) || deviceKey(event.keyCode) != null || name.startsWith("pad_button_")
    if (!chorded && !named) return null
    val onlyControl = event.isCtrlPressed && !event.isAltPressed && !event.isMetaPressed && !event.isShiftPressed
    if (onlyControl && name in textEditing) return null
    return obj("key" to name, "command" to (event.isCtrlPressed || event.isMetaPressed), "shift" to event.isShiftPressed, "alt" to event.isAltPressed)
}

private fun JSONObject.nullableString(key: String) = if (isNull(key)) null else optString(key)

private fun subtitle(row: JSONObject): String {
    val scope = when (val value = row.optString("scope")) {
        "" -> ""
        "Canvas" -> "On the canvas"
        else -> "With ${value.lowercase()} tools"
    }
    return listOf(row.optString("detail"), scope).filter { it.isNotEmpty() }.joinToString(" · ")
}

@Composable private fun GroupTitle(title: String, modifier: Modifier = Modifier) {
    Text(title, modifier.padding(horizontal = 4.dp), fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold)
}

@Composable private fun Description(text: String, modifier: Modifier = Modifier, align: TextAlign = TextAlign.Start) {
    Text(text, modifier.fillMaxWidth().padding(horizontal = 4.dp), color = LocalPalette.current.settingsSecondary,
        fontSize = 14.sp, lineHeight = 20.sp, textAlign = align)
}

@Composable private fun Card(modifier: Modifier = Modifier, rows: List<@Composable () -> Unit>) {
    val colors = LocalPalette.current
    Surface(modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), color = colors.settingsCard, shadowElevation = 1.dp) {
        Column {
            rows.forEachIndexed { index, row ->
                if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp), color = colors.divider)
                row()
            }
        }
    }
}

@Composable private fun SettingRow(title: String, tag: String, subtitle: String = "", onClick: (() -> Unit)? = null,
    titleColor: androidx.compose.ui.graphics.Color = LocalPalette.current.text, trailing: @Composable RowScope.() -> Unit = {}) {
    val colors = LocalPalette.current
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
        .testTag(tag).padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, color = titleColor)
            if (subtitle.isNotEmpty()) Text(subtitle, color = colors.settingsSecondary, fontSize = 13.sp, lineHeight = 18.sp)
        }
        trailing()
    }
}

@Composable private fun Value(text: String, tag: String? = null) {
    Text(text, Modifier.widthIn(max = 220.dp).then(if (tag != null) Modifier.testTag(tag) else Modifier),
        color = LocalPalette.current.settingsSecondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

@Composable private fun Chevron() {
    SharedIcon("chevron-down", null, Modifier.size(20.dp).rotate(-90f), tint = LocalPalette.current.settingsSecondary)
}

@Composable private fun ResetButton(tag: String, onClick: () -> Unit) {
    IconButton(onClick, Modifier.testTag(tag)) { SharedIcon("reset", "Reset to default", Modifier.size(20.dp)) }
}

@Composable private fun Dropdown(tag: String, label: String, options: List<String>, selected: Int, choose: (Int) -> Unit) {
    val colors = LocalPalette.current
    var open by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp)).background(colors.input)
            .clickable(role = Role.Button) { open = true }.testTag(tag).padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(label, maxLines = 1)
            SharedIcon("chevron-down", null, Modifier.size(16.dp), tint = colors.settingsSecondary)
        }
        DropdownMenu(open && LocalPreferencesOpen.current, { open = false }, containerColor = colors.settingsCard) {
            options.forEachIndexed { index, option ->
                DropdownMenuItem({ Text(option) }, { open = false; choose(index) }, Modifier.testTag("$tag-option-$index"),
                    trailingIcon = { Box(Modifier.size(20.dp)) { if (index == selected) SharedIcon("check", null, Modifier.fillMaxSize()) } })
            }
        }
    }
}

@Composable internal fun ShortcutsHome(host: CanvasHost, view: JSONObject) {
    val page = view.getJSONObject("shortcut_page")
    Keymap(host, view.getJSONObject("keymap"))
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        GroupTitle("Shortcuts")
        ShortcutFilters(host, view, page)
        if (!page.getBoolean("filtering")) Card(Modifier.testTag("shortcut-categories"), page.array("categories").objects().map { category ->
            @Composable {
                val id = category.getString("id")
                SettingRow(id, "shortcut-category-$id", onClick = { host.preference(obj("type" to "shortcut_category", "id" to id)) }) {
                    Value(category.getInt("count").toString())
                    Chevron()
                }
            }
        })
        page.objectOrNull("empty")?.let { EmptyStatus(it) }
    }
    if (page.getBoolean("filtering")) {
        val modifiers = page.array("modifiers").objects().filter { it.getBoolean("visible") }
        if (modifiers.isNotEmpty()) Column(Modifier.testTag("modifier-results"), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            GroupTitle(MODIFIER_SECTION)
            Card(rows = modifiers.map { @Composable { ModifierRow(host, it) } })
        }
        ShortcutResults(host, view, grouped = false)
    }
}

@Composable private fun ShortcutFilters(host: CanvasHost, view: JSONObject, page: JSONObject) {
    val key = page.nullableString("key")
    var keySearch by remember { mutableLongStateOf(0L) }
    LaunchedEffect(key) { if (key != null) keySearch++ }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        CoreTextField(view.optString("shortcut_query"), { host.preference(obj("type" to "search_shortcuts", "query" to it)) },
            modifier = Modifier.weight(1f).testTag("shortcuts-search").onPreviewKeyEvent { event ->
                val chord = searchChord(event.nativeKeyEvent) ?: return@onPreviewKeyEvent false
                host.preference(obj("type" to "search_shortcut_key", "chord" to chord))
                true
            }, height = 48.dp, focusRequest = keySearch,
            placeholder = { Text("Search or press a shortcut") }, leadingIcon = { SharedIcon("search", null, Modifier.size(20.dp)) })
        val contexts = page.array("contexts").objects()
        val context = page.nullableString("context")
        val selected = contexts.indexOfFirst { it.nullableString("category") == context }.coerceAtLeast(0)
        Dropdown("shortcut-context", contexts.getOrNull(selected)?.getString("label") ?: "", contexts.map { it.getString("label") }, selected) {
            host.preference(obj("type" to "shortcut_context", "category" to contexts[it].nullableString("category")))
        }
        val shows = page.array("shows").objects()
        val show = shows.indexOfFirst { it.getString("show") == page.getString("show") }.coerceAtLeast(0)
        Dropdown("shortcut-show", shows.getOrNull(show)?.getString("label") ?: "", shows.map { it.getString("label") }, show) {
            host.preference(obj("type" to "shortcut_show", "show" to shows[it].getString("show")))
        }
    }
}

@Composable private fun EmptyStatus(empty: JSONObject) {
    Column(Modifier.fillMaxWidth().padding(vertical = 32.dp).testTag("shortcut-empty"),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SharedIcon("search", null, Modifier.size(48.dp), tint = LocalPalette.current.settingsSecondary)
        Text(empty.getString("title"), fontSize = 18.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        Text(empty.getString("description"), color = LocalPalette.current.settingsSecondary, textAlign = TextAlign.Center)
    }
}

@Composable private fun ShortcutResults(host: CanvasHost, view: JSONObject, grouped: Boolean) {
    val groups = mutableListOf<Pair<String, MutableList<JSONObject>>>()
    for (row in view.array("shortcuts").objects().filter { it.getBoolean("visible") }) {
        val title = if (grouped) row.optString("subgroup") else row.getString("group")
        if (groups.lastOrNull()?.first == title) groups.last().second.add(row) else groups.add(title to mutableListOf(row))
    }
    for ((title, rows) in groups) Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (title.isNotEmpty()) GroupTitle(title)
        Card(rows = rows.map { @Composable { ShortcutRow(host, it) } })
    }
}

@Composable private fun ShortcutRow(host: CanvasHost, row: JSONObject) {
    val id = row.getString("id")
    key(id) {
        SettingRow(row.getString("label"), "shortcut-$id", subtitle(row), onClick = { host.preference(obj("type" to "edit_shortcut", "id" to id)) }) {
            Value(row.getString("shortcut"), "shortcut-binding-$id")
            if (row.getBoolean("modified")) ResetButton("shortcut-reset-$id") { host.preference(obj("type" to "reset_shortcut", "id" to id)) }
        }
    }
}

@Composable private fun ModifierRow(host: CanvasHost, modifier: JSONObject) {
    val label = modifier.getString("label")
    SettingRow(label, "modifier-$label", modifier.optString("detail"), onClick = {
        host.preference(obj("type" to "edit_modifier_key", "key" to modifier.getJSONObject("key")))
    }) {
        Value(modifier.getString("action"))
        Chevron()
    }
}

@Composable internal fun ShortcutCategory(host: CanvasHost, view: JSONObject, category: String) {
    val page = view.getJSONObject("shortcut_page")
    if (category == MODIFIER_SECTION) {
        Column(Modifier.testTag("modifier-keys"), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Description("Hold a key to use a tool or mode until you let go.")
            Card(rows = page.array("modifiers").objects().filter { it.getBoolean("visible") }.map { @Composable { ModifierRow(host, it) } } +
                listOf(@Composable {
                    SettingRow("Add Modifier Key", "add-modifier-key", onClick = { host.preference(obj("type" to "add_modifier_key")) }) {
                        SharedIcon("plus", null, Modifier.size(20.dp))
                    }
                }))
        }
    } else ShortcutResults(host, view, grouped = true)
    page.objectOrNull("empty")?.let { EmptyStatus(it) }
}

/** What a modifier key or pen button does, per kind of tool. */
@Composable internal fun PerToolPage(host: CanvasHost, prefix: String, summary: String, editor: JSONObject, reset: JSONObject,
    same: (Boolean) -> JSONObject, pick: (String?) -> JSONObject, remove: Pair<String, JSONObject>?) {
    Column(Modifier.testTag("$prefix-page"), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Description(summary, Modifier.weight(1f))
            if (editor.getBoolean("modified")) ResetButton("$prefix-reset") { host.preference(reset) }
        }
        val perTool = editor.getBoolean("per_tool")
        Card(rows = listOf<@Composable () -> Unit>({
            Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).toggleable(!perTool, role = Role.Switch) { host.preference(same(!it)) }
                .testTag("$prefix-same").padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Same for every tool", Modifier.weight(1f))
                Switch(!perTool, onCheckedChange = null)
            }
        }) + editor.array("actions").objects().map { action ->
            @Composable {
                val category = action.nullableString("category")
                SettingRow(action.getString("label"), "$prefix-action-${category ?: "all"}", onClick = { host.preference(pick(category)) }) {
                    Value(action.getString("action"))
                    Chevron()
                }
            }
        })
        if (remove != null) Card(rows = listOf {
            SettingRow(remove.first, "$prefix-remove", onClick = { host.preference(remove.second) }, titleColor = MaterialTheme.colorScheme.error) {
                SharedIcon("delete", null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.error)
            }
        })
    }
}

@Composable private fun RecordingRow(host: CanvasHost, capture: JSONObject) {
    val colors = LocalPalette.current
    val existing = capture.optBoolean("existing")
    val conflict = !capture.isNull("conflict")
    val notice = capture.optString("notice")
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        SharedIcon(if (existing || notice.isNotEmpty()) "info" else "keyboard", null, Modifier.size(20.dp),
            tint = if (!existing && notice.isNotEmpty()) MaterialTheme.colorScheme.error else colors.text)
        Column(Modifier.weight(1f)) {
            Text(capture.optString("shortcut"), Modifier.testTag("shortcut-recording"))
            if (notice.isNotEmpty()) Text(notice, color = colors.settingsSecondary, fontSize = 13.sp, lineHeight = 18.sp)
        }
        TextButton({ host.preference(obj("type" to "cancel_shortcut")) }, Modifier.testTag("cancel-shortcut")) { Text("Cancel") }
        Button({ host.preference(obj("type" to "confirm_shortcut", "replace" to conflict)) }, Modifier.testTag("confirm-shortcut"),
            enabled = capture.objectOrNull("chord") != null && capture.isNull("error")) {
            Text(when { existing -> "Open"; conflict -> "Reassign"; else -> "Add" })
        }
    }
}

/** One sheet: the action's keys, inline recording and reset. */
@Composable internal fun ShortcutEditor(host: CanvasHost, view: JSONObject, editor: JSONObject) {
    val id = editor.getString("id")
    val capture = view.objectOrNull("capture")?.takeIf { it.getString("id") == id }
    val focus = remember { FocusRequester() }
    val visible = LocalPreferencesOpen.current
    LaunchedEffect(visible, capture != null) { if (visible && capture != null) focus.requestFocus() }
    Column(Modifier.fillMaxWidth().onPreviewKeyEvent { event ->
        if (capture != null && event.key != Key.Back) { host.key(event.nativeKeyEvent); true } else false
    }.focusRequester(focus).focusable(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        editor.optString("description").takeIf { it.isNotEmpty() }?.let {
            Description(it, Modifier.testTag("shortcut-editor-description"), TextAlign.Center)
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                val defaults = editor.array("defaults").values().joinToString(" / ").ifEmpty { "none" }
                Column(Modifier.weight(1f)) {
                    Description("Default: $defaults", Modifier.testTag("shortcut-editor-default"))
                    editor.array("overlaps").values().forEach { Description(it.toString()) }
                }
                if (editor.getBoolean("modified")) ResetButton("shortcut-editor-reset") { host.preference(obj("type" to "reset_shortcut", "id" to id)) }
            }
            val rows = editor.array("bindings").values().mapIndexed { index, binding ->
                @Composable {
                    SettingRow(binding.toString(), "shortcut-binding-row-$index") {
                        IconButton({ host.preference(obj("type" to "remove_shortcut", "id" to id, "index" to index)) }, Modifier.testTag("remove-shortcut-$index")) {
                            SharedIcon("delete", "Remove shortcut", Modifier.size(20.dp))
                        }
                    }
                }
            }
            val last: List<@Composable () -> Unit> = when {
                capture != null -> listOf({ RecordingRow(host, capture) })
                editor.getBoolean("can_add") -> listOf({
                    SettingRow("Add Shortcut", "add-shortcut", onClick = { host.preference(obj("type" to "begin_shortcut", "id" to id)) }) {
                        SharedIcon("plus", null, Modifier.size(20.dp))
                    }
                })
                else -> emptyList()
            }
            Card(rows = rows + last)
        }
        val gestures = editor.array("gestures").values()
        if (gestures.isNotEmpty()) Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            GroupTitle("Pen and Touch")
            Description("Change these on the Pen & Input page.")
            Card(rows = gestures.map { gesture -> @Composable { SettingRow(gesture.toString(), "shortcut-gesture-$gesture") } })
        }
    }
}

@Composable internal fun TriggerGroups(host: CanvasHost, view: JSONObject) {
    val sections = view.getJSONObject("shortcut_page").array("triggers").objects().groupBy { it.getString("section") }
    for ((section, triggers) in sections) Column(Modifier.testTag("triggers-$section"), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        GroupTitle(section)
        Card(rows = triggers.map { trigger ->
            @Composable {
                val id = trigger.getString("id")
                SettingRow(trigger.getString("label"), "trigger-$id", trigger.optString("detail"), onClick = {
                    host.preference(if (id.startsWith("pen.")) obj("type" to "edit_pen_button", "trigger" to id)
                        else obj("type" to "open_action_picker", "trigger" to id))
                }) {
                    Value(trigger.getString("action"), "trigger-action-$id")
                    Chevron()
                }
            }
        })
    }
}

@Composable private fun SheetFrame(title: String, tag: String, close: () -> Unit, keys: (androidx.compose.ui.input.key.KeyEvent) -> Boolean,
    content: @Composable ColumnScope.() -> Unit) {
    Dialog(close) {
        val focus = remember { FocusRequester() }
        LaunchedEffect(Unit) { focus.requestFocus() }
        Surface(shape = RoundedCornerShape(16.dp), color = LocalPalette.current.settingsBackground) {
            Column(Modifier.widthIn(max = 520.dp).heightIn(max = 680.dp).testTag(tag).onPreviewKeyEvent(keys)
                .focusRequester(focus).focusable().padding(bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Box(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 8.dp)) {
                    Text(title, Modifier.align(Alignment.Center).padding(horizontal = 56.dp).testTag("$tag-title"),
                        fontSize = 18.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    IconButton(close, Modifier.align(Alignment.CenterEnd)) { SharedIcon("close", "Close", Modifier.size(20.dp)) }
                }
                content()
            }
        }
    }
}

@Composable internal fun ShortcutDialogs(host: CanvasHost, view: JSONObject) {
    val capture = view.objectOrNull("capture")
    if (capture?.getString("id") == MODIFIER_CAPTURE) {
        SheetFrame("New Modifier Key", "modifier-key", { host.preference(obj("type" to "cancel_shortcut")) }, { event ->
            if (event.key == Key.Back) false else { host.key(event.nativeKeyEvent); true }
        }) {
            Description("Press the key or button to hold.", Modifier.padding(horizontal = 16.dp), TextAlign.Center)
            Box(Modifier.padding(horizontal = 16.dp)) { Card(rows = listOf { RecordingRow(host, capture) }) }
        }
    }
    val picker = view.getJSONObject("shortcut_page").objectOrNull("picker") ?: return
    val close = { host.preference(obj("type" to "close_action_picker")) }
    SheetFrame(picker.getString("title"), "action-picker", close, { event ->
        if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) { close(); true } else false
    }) {
        Description(picker.getString("description"), Modifier.padding(horizontal = 16.dp).testTag("action-picker-description"), TextAlign.Center)
        Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            CoreTextField(picker.optString("query"), { host.preference(obj("type" to "search_action_picker", "query" to it)) },
                Modifier.weight(1f).testTag("action-picker-search"), height = 48.dp, placeholder = { Text("Search actions") },
                leadingIcon = { SharedIcon("search", null, Modifier.size(20.dp)) })
            if (picker.getBoolean("modified")) TextButton({ host.preference(obj("type" to "reset_trigger", "trigger" to picker.getString("trigger"))) },
                Modifier.testTag("action-picker-reset")) { Text("Reset") }
        }
        Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)) {
            fun choice(id: String, label: String, detail: String, selected: Boolean): @Composable () -> Unit = {
                SettingRow(label, "action-$id", detail, onClick = { host.preference(obj("type" to "choose_action", "id" to id)) }) {
                    Box(Modifier.size(20.dp)) { if (selected) SharedIcon("check", null, Modifier.fillMaxSize()) }
                }
            }
            val query = picker.optString("query").trim().lowercase()
            val sections = picker.array("sections").objects()
            if ("nothing".contains(query)) Card(rows = listOf(choice("", "Nothing", "", picker.getBoolean("nothing"))))
            for (section in sections) Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                GroupTitle(section.getString("title"))
                Card(rows = section.array("actions").objects().map {
                    choice(it.getString("id"), it.getString("label"), it.optString("detail"), it.getBoolean("selected"))
                })
            }
            if (sections.isEmpty() && !"nothing".contains(query)) EmptyStatus(obj("title" to "No Results Found", "description" to "Try a different search."))
        }
    }
}

@Composable internal fun Keymap(host: CanvasHost, keymap: JSONObject) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    var choosing by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    val exporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val text = host.keymapFile?.optString("text"); host.keymapFile = null
        if (uri != null && text != null) host.viewModelScope.launch {
            try { withContext(Dispatchers.IO) { context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(text.toByteArray()) } ?: error("Could not open the export destination") } }
            catch (e: Exception) { host.preference(obj("type" to "cancel_keymap_import")) }
        }
    }
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        host.keymapFile = null
        if (uri != null) host.viewModelScope.launch {
            val text = withContext(Dispatchers.IO) {
                context.contentResolver.openInputStream(uri)?.use { input ->
                    val buffer = ByteArray(1 shl 20); var length = 0
                    while (length < buffer.size) { val n = input.read(buffer, length, buffer.size - length); if (n < 0) break; length += n }
                    String(buffer, 0, length)
                }
            }
            if (text != null) host.preference(obj("type" to "import_keymap", "text" to text))
        }
    }
    LaunchedEffect(host.keymapFile) {
        val request = host.keymapFile ?: return@LaunchedEffect
        when (request.getString("type")) {
            "export_keymap" -> exporter.launch(request.getString("name"))
            "import_keymap" -> importer.launch(arrayOf("*/*"))
        }
    }
    val presets = keymap.array("presets").objects()
    val selected = presets.firstOrNull { it.getString("id") == keymap.getString("selected") }
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        GroupTitle("Keymap")
        Card(rows = listOf {
            Box {
                SettingRow("Preset", "keymap-preset", if (keymap.optBoolean("outdated")) "Updated since you chose it" else "", onClick = { choosing = true }) {
                    Value(selected?.getString("title") ?: "")
                    SharedIcon("chevron-down", null, Modifier.size(16.dp), tint = colors.settingsSecondary)
                    Box {
                        IconButton({ menu = true }, Modifier.testTag("keymap-menu")) { SharedIcon("more", "Keymap options", Modifier.size(20.dp)) }
                        DropdownMenu(menu && LocalPreferencesOpen.current, { menu = false }, containerColor = colors.settingsCard) {
                            for ((label, tag, action) in listOf(
                                Triple("Import…", "keymap-import", obj("type" to "choose_keymap_file")),
                                Triple("Export…", "keymap-export", obj("type" to "export_keymap")),
                                Triple("Differences…", "keymap-differences", obj("type" to "keymap_details", "open" to true)),
                                Triple("Reset All Shortcuts", "keymap-reset-all", obj("type" to "reset_all_shortcuts")))) {
                                DropdownMenuItem({ Text(label) }, { menu = false; host.preference(action) }, Modifier.testTag(tag))
                            }
                        }
                    }
                }
                DropdownMenu(choosing && LocalPreferencesOpen.current, { choosing = false }, containerColor = colors.settingsCard) {
                    presets.forEach { preset ->
                        DropdownMenuItem({ Text(preset.getString("title")) }, {
                            choosing = false
                            host.preference(obj("type" to "select_keymap", "id" to preset.getString("id")))
                        }, Modifier.testTag("keymap-choice-" + preset.getString("id")),
                            trailingIcon = { Box(Modifier.size(20.dp)) { if (preset == selected) SharedIcon("check", null, Modifier.fillMaxSize()) } })
                    }
                }
            }
        })
    }
    if (keymap.optBoolean("details") && LocalPreferencesOpen.current) {
        val close = { host.preference(obj("type" to "keymap_details", "open" to false)) }
        SheetFrame(keymap.getString("title"), "keymap-details", close, { false }) {
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Description(keymap.getString("source"))
                val differences = keymap.array("differences").objects()
                Card(rows = if (differences.isEmpty()) listOf { SettingRow("No differences", "keymap-no-differences", "This keymap uses the CapyCanvas defaults.") }
                    else differences.mapIndexed { index, item -> @Composable { SettingRow(item.getString("trigger"), "keymap-difference-$index", item.getString("note")) } })
            }
        }
    }
    keymap.objectOrNull("import")?.let { preview ->
        AlertDialog({ host.preference(obj("type" to "cancel_keymap_import")) },
            confirmButton = { TextButton({ host.preference(obj("type" to "confirm_keymap_import")) }, Modifier.testTag("keymap-confirm-import")) { Text("Import") } },
            dismissButton = { TextButton({ host.preference(obj("type" to "cancel_keymap_import")) }) { Text("Cancel") } },
            title = { Text("Import " + preview.getString("title") + "?") },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()).testTag("keymap-import-preview")) {
                    var any = false
                    for ((title, key) in listOf("Added" to "added", "Changed" to "changed", "Removed" to "removed", "Not available" to "unavailable")) {
                        val lines = preview.array(key).values()
                        if (lines.isEmpty()) continue
                        any = true
                        Text("$title (${lines.size})", fontWeight = FontWeight.SemiBold)
                        lines.forEach { Text(it.toString(), fontSize = 13.sp) }
                    }
                    if (!any) Text("No shortcuts change.")
                }
            })
    }
}
