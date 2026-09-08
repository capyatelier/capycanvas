package art.capycanvas

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.*
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.tween
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.foundation.selection.selectable
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import org.json.JSONObject

/** Retain only the outgoing view during the exit animation; Rust owns the
 * settings session. The full-size input surface also blocks the exposed canvas
 * while the settings sheet is entering or leaving. */
@Composable internal fun PreferencesOverlay(host: CanvasHost, view: JSONObject?) {
    val visible = remember { MutableTransitionState(false) }
    visible.targetState = view != null
    var retained by remember { mutableStateOf<JSONObject?>(null) }
    if (view != null) SideEffect { retained = view }
    val model = view ?: retained
    if (visible.currentState || visible.targetState) {
        Box(Modifier.fillMaxSize().pointerInput(Unit) {
            awaitPointerEventScope { while (true) awaitPointerEvent().changes.forEach { it.consume() } }
        }) {
            AnimatedVisibility(visible,
                enter = slideInVertically(tween(240, easing = FastOutSlowInEasing)) { -it },
                exit = slideOutVertically(tween(200, easing = FastOutSlowInEasing)) { -it }) {
                model?.let { PreferencesScreen(host, it) }
            }
        }
    }
}

private fun JSONObject.settingsRoute(): String = objectOrNull("detail")?.let { "value:" + it.getString("id") }
    ?: objectOrNull("shortcut_editor")?.let { "shortcut:" + it.getString("id") } ?: "page:" + getString("page")

@Composable private fun PreferencesScreen(host: CanvasHost, view: JSONObject) {
    val colors = LocalPalette.current
    val focus = androidx.compose.ui.platform.LocalFocusManager.current
    var showPage by rememberSaveable { mutableStateOf(view.getString("page") != "appearance") }
    BoxWithConstraints(Modifier.fillMaxSize().background(colors.settingsBackground).imePadding().testTag("preferences-surface")) {
        val wide = maxWidth >= 840.dp
        val detail = view.objectOrNull("detail")
        val shortcut = view.objectOrNull("shortcut_editor")
        fun close() { focus.clearFocus(); host.dispatch(obj("type" to "close_settings")) }
        fun back() {
            focus.clearFocus()
            when {
                view.objectOrNull("capture") != null -> host.preference(obj("type" to "cancel_shortcut"))
                shortcut != null -> host.preference(obj("type" to "close_shortcut_editor"))
                detail != null -> host.preference(obj("type" to "close_preference"))
                !wide && showPage -> showPage = false
                else -> close()
            }
        }
        BackHandler(onBack = ::back)
        // Two full-height panes, not a global app bar stacked over two columns.
        Row(Modifier.fillMaxSize()) {
            if (wide || !showPage) PreferencesNavigation(host, view,
                Modifier.then(if (wide) Modifier.width(260.dp) else Modifier.fillMaxWidth()).fillMaxHeight(),
                showDone = !wide, close = ::close) { showPage = true }
            if (wide || showPage) {
                Column(Modifier.weight(1f).fillMaxHeight().testTag("settings-main-pane")) {
                    val page = view.array("pages").objects().find { it.getString("id") == view.getString("page") }
                    // Pane controls stay put while only its contents slide.
                    Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).heightIn(min = 48.dp)) {
                        Text(detail?.getString("title") ?: shortcut?.getString("label") ?: page?.getString("title") ?: "",
                            Modifier.align(Alignment.Center).fillMaxWidth().padding(horizontal = 88.dp).testTag("settings-page-title"),
                            fontSize = 18.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
                        if (detail != null || shortcut != null || !wide) {
                            IconButton(::back, Modifier.align(Alignment.CenterStart).size(48.dp)) {
                                SharedIcon("back", "Back", Modifier.size(20.dp))
                            }
                        }
                        SettingsDone(::close, Modifier.align(Alignment.CenterEnd))
                    }
                    AnimatedContent(view, Modifier.weight(1f).fillMaxHeight().clipToBounds(), contentKey = { it.settingsRoute() },
                        transitionSpec = {
                            if (initialState.settingsRoute().startsWith("page:") && targetState.settingsRoute().startsWith("page:")) {
                                fadeIn(tween(140)) togetherWith fadeOut(tween(100))
                            } else {
                                val direction = if (targetState.settingsRoute().startsWith("page:")) -1 else 1
                                (slideInHorizontally(tween(220)) { it * direction } + fadeIn(tween(160))) togetherWith
                                    (slideOutHorizontally(tween(220)) { -it * direction / 3 } + fadeOut(tween(120)))
                            }
                        }, label = "settings-detail") { model ->
                        val row = model.objectOrNull("detail")
                        val editor = model.objectOrNull("shortcut_editor")
                        val page = model.array("pages").objects().find { it.getString("id") == model.getString("page") }
                        Column(Modifier.fillMaxSize().testTag("settings-content-" + model.settingsRoute())
                            .verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 16.dp),
                            horizontalAlignment = Alignment.CenterHorizontally) {
                            Column(Modifier.widthIn(max = 680.dp).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                                when {
                                    row != null -> PreferenceDetail(host, row)
                                    editor != null -> ShortcutEditor(host, model, editor)
                                    else -> {
                                        if (model.getString("page") == "shortcuts") Shortcuts(host, model)
                                        else page?.array("groups")?.objects()?.forEach { group ->
                                            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                                                Text(group.getString("title"), Modifier.padding(horizontal = 4.dp), fontWeight = FontWeight.SemiBold)
                                                Surface(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
                                                    color = colors.settingsCard, shadowElevation = 1.dp) {
                                                    Column {
                                                        group.array("rows").objects().filter { it.optBoolean("visible", true) }.forEachIndexed { index, setting ->
                                                            if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp), color = colors.divider)
                                                            key(setting.getString("id")) { PreferenceRow(host, setting) }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                model.optString("error").takeIf { it.isNotEmpty() && it != "null" }?.let {
                                    Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("settings-error"))
                                }
                                host.actionError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable private fun SettingsDone(close: () -> Unit, modifier: Modifier = Modifier) {
    Button(close, modifier.widthIn(min = 80.dp).heightIn(min = 48.dp).testTag("settings-done"),
        shape = RoundedCornerShape(8.dp), contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp)) {
        Text("Done", fontWeight = FontWeight.SemiBold)
    }
}

@Composable private fun PreferencesNavigation(host: CanvasHost, view: JSONObject, modifier: Modifier,
    showDone: Boolean, close: () -> Unit, onPage: () -> Unit) {
    val colors = LocalPalette.current
    val focus = androidx.compose.ui.platform.LocalFocusManager.current
    val searching = view.optBoolean("searching")
    Column(modifier.background(if (colors.dark) colors.tabs else colors.panel).testTag("settings-sidebar").padding(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth().heightIn(min = 48.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton({
                focus.clearFocus(); host.preference(obj("type" to "toggle_search", "open" to !searching))
            }, Modifier.size(48.dp).background(if (searching) colors.active else Color.Transparent, RoundedCornerShape(8.dp))
                .testTag("settings-search-button")) {
                SharedIcon("search", "Search settings", Modifier.size(20.dp))
            }
            Text("Settings", Modifier.weight(1f).testTag("settings-sidebar-title"),
                fontSize = 18.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
            if (showDone) SettingsDone(close)
        }
        if (searching) CoreTextField(view.optString("query"), { host.preference(obj("type" to "search", "query" to it)) },
            Modifier.fillMaxWidth(), height = 48.dp, placeholder = { Text("Search settings") },
            trailingIcon = {
                IconButton({
                    focus.clearFocus(); host.preference(obj("type" to "toggle_search", "open" to false))
                }) { SharedIcon("plus", "Close search", Modifier.size(20.dp).rotate(45f)) }
            })
        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (searching) {
                view.array("search_results").objects().forEach { result ->
                    Column(Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(RoundedCornerShape(8.dp)).clickable {
                        focus.clearFocus(); host.preference(result.getJSONObject("action")); onPage()
                    }.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(result.getString("title"), fontWeight = FontWeight.Medium)
                        Text(result.getString("description"), color = colors.settingsSecondary)
                    }
                }
                if (view.optBoolean("empty")) Text("No matching settings", Modifier.padding(12.dp), color = colors.settingsSecondary)
            } else view.array("pages").objects().forEach { page ->
                val selected = view.getString("page") == page.getString("id")
                Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(RoundedCornerShape(8.dp))
                    .background(if (selected) colors.active else Color.Transparent)
                    .selectable(selected, role = Role.Tab) {
                        focus.clearFocus(); host.preference(obj("type" to "page", "page" to page.getString("id"))); onPage()
                    }.testTag("settings-category-" + page.getString("id"))
                    .padding(horizontal = 12.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                        SharedIcon(page.getString("icon"), null, Modifier.size(20.dp).testTag("settings-category-icon-" + page.getString("id")))
                    }
                    Text(page.getString("title"), Modifier.testTag("settings-category-label-" + page.getString("id")),
                        fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
                }
            }
        }
    }
}

private fun numberLabel(kind: JSONObject): String =
    "%.${kind.getJSONObject("control").optInt("digits", 0)}f".format(java.util.Locale.ROOT, kind.number("value"))

@Composable private fun PreferenceRow(host: CanvasHost, row: JSONObject) {
    val colors = LocalPalette.current
    val kind = row.getJSONObject("kind")
    val type = kind.getString("type")
    val context = LocalContext.current
    val enabled = row.optBoolean("enabled", true)
    val interactive = type in listOf("number", "choice", "link")
    val action = when {
        type == "switch" -> Modifier.toggleable(kind.getBoolean("active"), enabled = enabled, role = Role.Switch) {
            host.preference(obj("type" to "edit", "id" to row.getString("id"), "value" to it))
        }
        interactive -> Modifier.clickable(enabled = enabled) {
            if (type == "link") context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(kind.getString("url"))))
            else host.preference(obj("type" to "edit_preference", "id" to row.getString("id")))
        }
        else -> Modifier
    }
    Row(Modifier.fillMaxWidth().heightIn(min = 64.dp).then(action).padding(horizontal = 16.dp, vertical = 12.dp)
        .testTag("preference-" + row.getString("id")),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(row.getString("title"), fontWeight = FontWeight.Medium, color = if (enabled) colors.text else colors.settingsSecondary)
            val summary = when (type) {
                "number" -> numberLabel(kind)
                "choice" -> kind.array("options").optString(kind.optInt("selected"))
                "link" -> kind.getString("label")
                "info" -> kind.optString("value")
                else -> row.optString("description")
            }
            if (summary.isNotEmpty()) {
                if (type == "info") SelectionContainer { Text(summary, color = colors.settingsSecondary) }
                else Text(summary, color = if (type == "link") colors.accent else colors.settingsSecondary)
            }
        }
        when (type) {
            "switch" -> Switch(kind.getBoolean("active"), onCheckedChange = null, enabled = enabled)
            "number", "choice" -> SharedIcon("chevron-down", null, Modifier.size(20.dp).rotate(-90f), tint = colors.settingsSecondary)
        }
    }
}

@Composable private fun PreferenceDetail(host: CanvasHost, row: JSONObject) {
    val kind = row.getJSONObject("kind")
    val colors = LocalPalette.current
    val enabled = row.optBoolean("enabled", true)
    fun edit(value: Any) = host.preference(obj("type" to "edit", "id" to row.getString("id"), "value" to value))
    Text(row.optString("description"), color = colors.settingsSecondary)
    if (kind.getString("type") == "choice") {
        Surface(shape = RoundedCornerShape(12.dp), color = colors.settingsCard) {
            Column {
                kind.array("options").values().forEachIndexed { index, name ->
                    if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 16.dp), color = colors.divider)
                    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp)
                        .selectable(index == kind.getInt("selected"), enabled = enabled, role = Role.RadioButton) { edit(index) }
                        .padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        kind.array("icons").optString(index).takeIf { it.isNotEmpty() }?.let { SharedIcon(it, null, Modifier.size(20.dp)) }
                        Text(name.toString(), Modifier.weight(1f))
                        RadioButton(index == kind.getInt("selected"), onClick = null, enabled = enabled)
                    }
                }
            }
        }
    } else {
        val focus = androidx.compose.ui.platform.LocalFocusManager.current
        val control = kind.getJSONObject("control")
        val value = kind.number("value")
        var text by rememberSaveable(row.getString("id")) { mutableStateOf(numberLabel(kind)) }
        var focused by remember { mutableStateOf(false) }
        var slider by remember(value) { mutableFloatStateOf(value) }
        LaunchedEffect(value) { if (!focused) text = numberLabel(kind) }
        DisposableEffect(Unit) { onDispose { if (focused) host.editingText = false } }
        OutlinedTextField(text, { text = it }, Modifier.fillMaxWidth().onFocusChanged {
            if (focused && !it.isFocused) edit(text)
            focused = it.isFocused; host.editingText = focused
        }.testTag("setting-number"),
            enabled = enabled, singleLine = true, label = { Text(row.getString("title")) },
            keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Decimal,
                imeAction = androidx.compose.ui.text.input.ImeAction.Done),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { focus.clearFocus() }),
            shape = RoundedCornerShape(12.dp))
        EditorSlider(slider, { slider = it }, enabled = enabled, range = control.number("min")..control.number("max"),
            height = 48.dp, label = row.getString("title"), inactiveTrackColor = colors.divider,
            onValueChangeFinished = {
                focus.clearFocus()
                val scale = Math.pow(10.0, control.optInt("digits", 0).toDouble()).toFloat()
                edit(kotlin.math.round(slider * scale) / scale)
            }, modifier = Modifier.fillMaxWidth().testTag("setting-slider"))
    }
}

@Composable private fun Shortcuts(host: CanvasHost, view: JSONObject) {
    CoreTextField(view.optString("shortcut_query"), { host.preference(obj("type" to "search_shortcuts", "query" to it)) },
        modifier = Modifier.fillMaxWidth(), height = 48.dp,
        placeholder = { Text("Search keyboard shortcuts") }, leadingIcon = { SharedIcon("search", null, Modifier.size(20.dp)) })
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
                    SharedIcon("chevron-down", null, Modifier.size(20.dp).rotate(-90f), tint = colors.settingsSecondary)
                }
            }
        }
    }
}

/** Shortcut information, recording and conflict resolution are all inline. */
@Composable private fun ShortcutEditor(host: CanvasHost, view: JSONObject, editor: JSONObject) {
    val capture = view.objectOrNull("capture")
    val focus = remember { FocusRequester() }
    LaunchedEffect(capture != null) { if (capture != null) focus.requestFocus() }
    Column(Modifier.fillMaxWidth().onPreviewKeyEvent { event ->
        if (capture != null) { host.key(event.nativeKeyEvent); true } else false
    }.focusRequester(focus).focusable(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text("Default: " + editor.array("defaults").values().joinToString(" / "), color = LocalPalette.current.settingsSecondary)
        editor.array("bindings").values().forEachIndexed { index, binding ->
            Row(Modifier.fillMaxWidth().background(LocalPalette.current.settingsCard, RoundedCornerShape(12.dp))
                .padding(start = 16.dp, end = 8.dp).heightIn(min = 56.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(binding.toString(), Modifier.weight(1f))
                IconButton({ host.preference(obj("type" to "remove_shortcut", "id" to editor.getString("id"), "index" to index)) }) {
                    SharedIcon("minus", "Remove shortcut", Modifier.size(20.dp))
                }
            }
        }
        if (capture != null) {
            Surface(shape = RoundedCornerShape(12.dp), color = LocalPalette.current.settingsCard) {
                Column(Modifier.fillMaxWidth().padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Press a key combination on your keyboard")
                    Text(capture.optString("shortcut"), fontSize = 20.sp, fontWeight = FontWeight.Medium)
                    capture.optString("conflict").takeIf { it.isNotEmpty() && it != "null" }?.let { conflict ->
                        Text("Already assigned to $conflict")
                        TextButton({ host.preference(obj("type" to "confirm_shortcut", "replace" to true)) }) { Text("Replace assignment") }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TextButton({ host.preference(obj("type" to "cancel_shortcut")) }) { Text("Cancel recording") }
                        TextButton({ host.preference(obj("type" to "confirm_shortcut", "replace" to false)) },
                            enabled = capture.objectOrNull("chord") != null && capture.isNull("conflict")) { Text("Use shortcut") }
                    }
                }
            }
        } else TextButton({ host.preference(obj("type" to "begin_shortcut", "id" to editor.getString("id"))) },
            enabled = editor.optBoolean("can_add")) { Text("Add shortcut") }
        TextButton({ host.preference(obj("type" to "reset_shortcut", "id" to editor.getString("id"))) },
            enabled = editor.optBoolean("modified")) { Text("Restore default") }
    }
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
