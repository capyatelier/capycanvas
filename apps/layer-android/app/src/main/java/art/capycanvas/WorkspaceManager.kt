package art.capycanvas

import android.view.WindowManager
import android.view.KeyEvent
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
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
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.zIndex
import kotlin.math.roundToInt
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import org.json.JSONObject
import org.json.JSONArray

/** Compose owns focus/scroll/input; Rust owns records, selections and previews. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun WorkspaceManager(host: CanvasHost) {
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
    val view = host.workspaceManager ?: return
    val page = view.optString("page").takeUnless { it == "null" || it.isEmpty() }
    val form = view.objectOrNull("prompt")
    val error = view.optString("error").takeUnless { it == "null" || it.isEmpty() }
    val busy = view.optBoolean("busy")
    val renaming=page=="workspaces"&&view.objectOrNull("prompt_action")?.optString("type")=="rename"
    val toolbarMode = page in listOf("this_workspace", "toolbar_library")
    val rowInteraction = remember { WorkspaceRowInteraction() }
    val focusWindow = view.optJSONObject("focus_window")?.optString("id")
    LaunchedEffect(focusWindow) { if (focusWindow != null) host.reportActionError(copy.getString("owned_elsewhere")) }
    val colors = LocalPalette.current
    fun send(type: String) = host.workspaceInput(obj("type" to type))
    val cancel = { send("cancel") }
    val dismiss = {
        if (rowInteraction.active != null || rowInteraction.menu != null) rowInteraction.cancel() else send("dismiss")
    }
    if (page != null) Dialog(onDismissRequest = dismiss, properties = DialogProperties(usePlatformDefaultWidth = false,decorFitsSystemWindows=false)) {
        PreviewBackdrop()
        Box(Modifier.fillMaxSize()) {
        Box(Modifier.matchParentSize().pointerInput(dismiss) {detectTapGestures {dismiss()}})
        Box(Modifier.fillMaxSize().imePadding().padding(vertical=16.dp),contentAlignment=Alignment.Center) {
        Surface(shape = SquircleShape(16.dp), color = colors.settingsBackground, contentColor=colors.text,
            modifier = Modifier.widthIn(max = 480.dp).fillMaxWidth(.94f).testTag("workspace-manager").onPreviewKeyEvent { event ->
                if (event.nativeKeyEvent.keyCode == KeyEvent.KEYCODE_ESCAPE && event.nativeKeyEvent.action == KeyEvent.ACTION_DOWN &&
                    !host.editingText) { if(rowInteraction.active != null || rowInteraction.menu != null || rowInteraction.contact)rowInteraction.cancel() else send("dismiss"); true } else false
            }) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(view.getString("title"), Modifier.weight(1f), fontWeight = FontWeight.Bold)
                    IconButton(cancel, Modifier.size(32.dp).semantics { contentDescription = host.bootstrap!!.getJSONObject("common").getString("close") }) { SharedIcon("close", null) }
                }
                view.getString("intro").takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.settingsSecondary) }
                if (toolbarMode) Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((target, label) in listOf("this_workspace" to "this_workspace", "toolbar_library" to "saved_toolbars")) TextButton({ host.workspaceInput(obj("type" to "open", "page" to target)) }, enabled = !busy) { Text(copy.getString(label)) }
                }
                if (toolbarMode) CoreTextField(view.optString("query"), { host.workspaceInput(obj("type" to "search", "query" to it)) }, Modifier.fillMaxWidth().testTag("workspace-search"), placeholder = { Text(copy.getString("search")) }, background = colors.dialogField)
                if (page == "this_workspace") TextButton({ host.workspaceInput(obj("type" to "form", "action" to obj("type" to "new_toolbar", "value" to null))) }, enabled = !busy, modifier = Modifier.testTag("new-toolbar")) { Text(copy.getString("new_toolbar_action")) }
                Column(Modifier.weight(1f,fill=false),verticalArrangement=Arrangement.spacedBy(if(page=="workspaces")6.dp else 4.dp)) {
                    WorkspaceRows(host, view, rowInteraction, Modifier.weight(1f, fill=false).height(if(page=="workspaces")workspaceRowHeight()*minOf(7,view.array("rows").length()) else 315.dp).fillMaxWidth())
                    if(page=="workspaces")Row(horizontalArrangement=Arrangement.spacedBy(4.dp)) {
                        IconButton({host.workspaceInput(obj("type" to "form","action" to obj("type" to "new")))},enabled=!busy||renaming,modifier=Modifier.size(34.dp).testTag("new-workspace").semantics { contentDescription=copy.getString("new_workspace") }) {SharedIcon("plus",null,Modifier.size(16.dp),tint=colors.text.copy(alpha=if(busy&&!renaming) .36f else 1f))}
                        val delete=view.array("rows").objects().find {it.getString("id")==view.optString("selected")}?.array("options")?.objects()?.find {it.getJSONObject("action").getString("type")=="delete"}
                        IconButton({delete?.let {host.workspaceInput(obj("type" to "action","action" to it.getJSONObject("action"))) }},enabled=(!busy||renaming)&&delete?.optBoolean("enabled")==true,modifier=Modifier.size(34.dp).testTag("workspace-remove").semantics {contentDescription=host.bootstrap!!.getJSONObject("common").getString("delete")}) {SharedIcon("delete",null,Modifier.size(16.dp),tint=colors.text.copy(alpha=if((!busy||renaming)&&delete?.optBoolean("enabled")==true) 1f else .36f))}
                    }
                }
                if (toolbarMode) view.objectOrNull("details")?.let { details ->
                    Text(details.getString("title"), fontWeight = FontWeight.Bold)
                    Text(details.getString("description"), color = colors.settingsSecondary)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (button in details.array("actions").objects()) TextButton({ host.workspaceInput(obj("type" to "action", "action" to button.getJSONObject("action"))) }, enabled = !busy && button.optBoolean("enabled"), modifier = Modifier.testTag("toolbar-action-${button.getJSONObject("action").getString("type")}")) { Text(button.getString("label")) }
                    }
                }
                view.optString("switcher_error").takeUnless { it == "null" || it.isEmpty() }?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
                if (error != null && form == null) Text(error, color = MaterialTheme.colorScheme.error)
                if (!toolbarMode) Row(Modifier.fillMaxWidth(),horizontalArrangement = Arrangement.spacedBy(8.dp,Alignment.End)) {
                    FilledTonalButton(cancel, Modifier.height(34.dp).testTag("workspace-cancel"),shape=ControlShape,colors=ButtonDefaults.filledTonalButtonColors(containerColor=colors.text.copy(alpha=.1f),contentColor=colors.text),contentPadding=PaddingValues(horizontal=16.dp,vertical=0.dp)) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) }
                    if (view.optBoolean("retry")) TextButton({ send("retry") }, enabled = !busy) { Text(copy.getString("retry")) }
                    Button({ send("confirm") }, Modifier.height(34.dp).testTag("workspace-confirm"), shape=ControlShape,contentPadding=PaddingValues(horizontal=16.dp,vertical=0.dp),enabled = view.optBoolean("enabled") && (!busy || renaming)) { Text(view.getString("primary")) }
                }
            }
        }
        }
        }
    }
    if (form != null && !(page=="workspaces"&&view.objectOrNull("prompt_action")?.optString("type")=="rename")) {
        val action = view.getJSONObject("prompt_action")
        val named = !form.isNull("name")
        var name by remember(action.toString(), form.optString("name")) { mutableStateOf(if (named) form.getString("name") else "") }
        var description by remember(action.toString()) { mutableStateOf(form.optString("description").takeUnless { form.isNull("description") }.orEmpty()) }
        val choices = form.array("choices").objects()
        var choice by remember(action.toString()) { mutableStateOf(form.optString("selected").takeUnless { form.isNull("selected") }) }
        val nameError = if (!named) null else remember(name, host.languageTag) { JSONObject(Native.workspaceNameError(name, host.languageTag)).let { it.optString("error").takeUnless { _ -> it.isNull("error") } } }
        val title = form.getString("title")
        val submit = { host.workspaceInput(obj("type" to "submit", "name" to name, "description" to description.takeUnless { form.isNull("description") }, "choice" to choice)) }
        if (action.getString("type") == "reset") DialogShell(host,form.getJSONObject("dialog"),title,onCancel = cancel,tag = "workspace-form",
            responses = listOf(
                DialogResponse(form.nullableString("cancel") ?: host.bootstrap!!.getJSONObject("common").getString("cancel"),"workspace-reset-cancel",action = cancel),
                DialogResponse(form.getString("confirm"),"workspace-submit",accent = true,enabled = !busy,action = submit)),
            body = { padding -> Column(Modifier.padding(padding),verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(form.getString("message"))
                error?.let { Text(it,color = MaterialTheme.colorScheme.error) }
            } })
        else AlertDialog(onDismissRequest = cancel, modifier = Modifier.testTag("workspace-form"), title = { Text(title) }, text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                form.getString("message").takeIf { it.isNotEmpty() }?.let { Text(it) }
                if (named) CoreTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("workspace-name"), label = { Text(host.bootstrap!!.getJSONObject("common").getString("name")) }, background = LocalPalette.current.dialogField)
                if (!form.isNull("description")) CoreTextField(description, { description = it }, Modifier.fillMaxWidth().testTag("workspace-description"), label = { Text(copy.getString("description")) }, background = LocalPalette.current.dialogField)
                if (choices.isNotEmpty()) PropertyChoice(form.getString("choice_label"), choices.map { it.getString("label") }, choices.indexOfFirst { it.getString("id") == choice }.coerceAtLeast(0), enabled = !busy, fill = colors.dialogField) { choice = choices[it].getString("id") }
                if (nameError != null || error != null) Text(nameError ?: error.orEmpty(), color = MaterialTheme.colorScheme.error)
            }
        }, dismissButton = { TextButton(cancel) { Text(form.optString("cancel").takeUnless {form.isNull("cancel")} ?: host.bootstrap!!.getJSONObject("common").getString("cancel")) } }, confirmButton = {
            TextButton(submit, enabled = !busy && nameError == null,
                modifier = Modifier.testTag("workspace-submit")) { Text(if (view.optBoolean("retry") && action.getString("type") != "save_as_new") copy.getString("retry") else form.getString("confirm"),
                color = if (form.optBoolean("destructive")) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary) }
        })
    }
    var dismissedError by remember { mutableStateOf<String?>(null) }
    if (error == null) dismissedError = null
    if (host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") == null && error != null && page == null && form == null && error != dismissedError) AlertDialog(
        onDismissRequest = { dismissedError = error; send("resume") }, title = { Text(copy.getString("workspaces")) }, text = { Text(error) },
        dismissButton = { TextButton({ dismissedError = error; send("resume") }) { Text(host.bootstrap!!.getJSONObject("common").getString("keep_open")) } }, confirmButton = {
            Column {
                TextButton({ send("retry") }) { Text(copy.getString("retry")) }
                TextButton({ host.workspaceInput(obj("type" to "form", "action" to obj("type" to "save_as_new"))) }) { Text(copy.getString("save_as_new_workspace")) }
            }
        })
}

internal fun workspaceSwitcherMenu(view: JSONObject?): JSONObject = view?.optJSONObject("switcher_menu") ?: obj("sections" to JSONArray())

@Composable internal fun WorkspaceStatus(host: CanvasHost,dock: DockInteraction,notice: JSONObject) {
    val status = host.workspaceManager?.objectOrNull("status") ?: return
    val colors = LocalPalette.current
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
    Surface(Modifier.zIndex(250f).layout { measurable,constraints ->
        fun bounds(rect: androidx.compose.ui.geometry.Rect?) = rect?.let { obj("x" to it.left/density,"y" to it.top/density,"width" to it.width/density,"height" to it.height/density) }
        val placed = JSONObject(Native.dialogUi(obj("kind" to "notice_above","notice" to notice,
            "bar" to bounds(dock.canvasBarSlot),"bubble" to bounds(dock.noticeBounds)).toString()))
        val child = measurable.measure(Constraints(maxWidth = (placed.number("max_width")*density).roundToInt().coerceAtLeast(0)))
        layout(constraints.maxWidth,constraints.maxHeight) {
            child.place((placed.number("x")*density-child.width/2f).roundToInt(),(placed.number("y")*density-child.height).roundToInt())
        }
    }.testTag("workspace-status").chromeRegion(dock).semantics { liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite },
        shape = SurfaceShape,color = colors.surround,contentColor = if(status.getBoolean("error"))colors.error else colors.text) {
        Layout(modifier = Modifier.padding(vertical = 6.dp),content = {
            Text(status.getString("text"),Modifier.testTag("workspace-status-text"))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp),modifier = Modifier.testTag("workspace-status-actions")) {
                if (status.getBoolean("retry")) DialogButton(copy.getString("retry"),"workspace-status-retry",multiline = true) { host.workspaceInput(obj("type" to "retry")) }
                status.objectOrNull("action")?.let { action -> DialogButton(action.getString("label"),"workspace-status-action",enabled = action.getBoolean("enabled"),multiline = true) {
                    host.workspaceInput(obj("type" to "form","action" to action.getJSONObject("action")))
                } }
            }
        }) { (text,actions),constraints ->
            val gap = 8.dp.roundToPx()
            val natural = text.maxIntrinsicWidth(Int.MAX_VALUE)
            val offers = actions.maxIntrinsicWidth(Int.MAX_VALUE)
            val beside = toolbarUi(host.languageTag,obj("type" to "notice_layout","text" to natural,"content" to constraints.maxWidth,
                "actions" to offers,"gap" to gap,"min_text_width" to notice.number("min_text_width")*density)).getBoolean("beside")
            val buttons = actions.measure(Constraints(maxWidth = minOf(offers,constraints.maxWidth)))
            val label = text.measure(Constraints(maxWidth = (constraints.maxWidth-if(beside && buttons.width>0)buttons.width+gap else 0).coerceAtLeast(0)))
            val width = if(beside)label.width+buttons.width+(if(buttons.width>0)gap else 0) else maxOf(label.width,buttons.width)
            val lines = if(buttons.height>0)6.dp.roundToPx() else 0
            val height = if(beside)maxOf(label.height,buttons.height) else label.height+lines+buttons.height
            layout(width,height) {
                label.placeRelative(0,if(beside)(height-label.height)/2 else 0)
                buttons.placeRelative(width-buttons.width,if(beside)(height-buttons.height)/2 else label.height+lines)
            }
        }
    }
}

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
    val window=(view.parent as? DialogWindowProvider)?.window
    DisposableEffect(window) {
        window?.apply {
            enterCanvasFullscreen()
            addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
            setDimAmount(.18f)
        }
        onDispose { }
    }
}
