package art.capycanvas

import androidx.compose.foundation.clickable
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
import androidx.compose.ui.window.Dialog
import org.json.JSONArray
import org.json.JSONObject

/** Header menus, context menus and configuration options render the same Rust
 * items. They never reconstruct eligibility, naming, defaults or commands. */
@Composable internal fun WorkspaceMenu(host: CanvasHost, menu: JSONObject, dismiss: () -> Unit) {
    DropdownMenu(true, dismiss, modifier = Modifier.widthIn(min = 240.dp, max = 380.dp).testTag("workspace-menu"),
        shape = RoundedCornerShape(10.dp), containerColor = LocalPalette.current.panel) {
        WorkspaceMenuItems(host, menu.array("sections"), dismiss, menu.getString("title"))
    }
}

@Composable internal fun WorkspaceMenuItems(host: CanvasHost, sections: JSONArray,
    dismiss: () -> Unit = {}, title: String? = null) {
    val colors = LocalPalette.current
    var pages by remember { mutableStateOf<List<JSONObject>>(emptyList()) }
    val page = pages.lastOrNull()
    if (page != null) {
        Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).clickable { pages = pages.dropLast(1) }
            .padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            SharedIcon("down", "Back", Modifier.rotate(90f))
            Text(page.getString("label"), fontWeight = FontWeight.Bold)
        }
        HorizontalDivider(color = colors.divider)
    } else if (title != null) Text(title, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), color = colors.secondary)
    (page?.array("sections") ?: sections).values().map { it as JSONArray }.filter { it.length() > 0 }.forEachIndexed { index, section ->
        if (index > 0) HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = colors.divider)
        section.objects().forEach { item ->
            val enabled = item.optBoolean("enabled", true)
            Row(Modifier.fillMaxWidth().heightIn(min = 36.dp).padding(horizontal = 6.dp)
                .clip(RoundedCornerShape(6.dp)).alpha(if (enabled) 1f else .4f)
                .clickable(enabled = enabled) {
                    if (item.array("sections").length() > 0) pages = pages + item
                    else item.objectOrNull("action")?.let { action -> dismiss(); host.dispatch(action) }
                }.padding(horizontal = 10.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp)) {
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
                        Text("×", fontSize = 24.sp)
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
