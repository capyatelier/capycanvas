package art.capycanvas

import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import androidx.compose.ui.platform.LocalDensity
import org.json.JSONArray
import org.json.JSONObject

private fun JSONObject.tabStyle(group: Int): String? {
    if (optString("kind") == "tabs" && optInt("id", -1) == group) return optString("tab_style", "automatic")
    for (key in keys()) when (val value = opt(key)) {
        is JSONObject -> value.tabStyle(group)?.let { return it }
        is JSONArray -> for (i in 0 until value.length()) (value.opt(i) as? JSONObject)?.tabStyle(group)?.let { return it }
    }
    return null
}

@Composable internal fun automaticTabNames(host: CanvasHost, group: Int, panels: List<JSONObject?>, available: Float): List<Boolean>? {
    val layout = host.snapshot?.optJSONObject("state")?.optJSONObject("workspace")?.optJSONObject("layout")
    val automatic = remember(layout, group) { layout?.tabStyle(group) == "automatic" }
    val measurer = rememberTextMeasurer()
    val style = LocalTextStyle.current.copy(fontWeight = FontWeight.Bold)
    val density = LocalDensity.current.density
    if (!automatic || available <= 0f) return null
    val widths = panels.map { panel ->
        panel?.let { listOf(16f + 22f + measurer.measure(AnnotatedString(it.getString("title")), style).size.width / density, 36f) } ?: listOf(0f, 0f)
    }
    return remember(widths, available) {
        JSONArray(Native.automaticTabNames(obj("available" to available, "widths" to JSONArray(widths.map { JSONArray(it) })).toString()))
            .let { names -> List(names.length()) { names.getBoolean(it) } }
    }
}

/** Docked, floating and drawer tabs share geometry, colors and focus feedback. */
@Composable internal fun WorkspaceTab(host: CanvasHost, dock: DockInteraction, panel: JSONObject,
    group: Int, index: Int, selected: Boolean, modifier: Modifier, enabled: Boolean = true, fittedName: Boolean? = null) {
    val colors = LocalPalette.current
    val id = panel.getString("id")
    val presentation = panel.getJSONObject("tab")
    val showIcon = fittedName != null || presentation.getBoolean("show_icon")
    val showName = fittedName ?: presentation.getBoolean("show_name")
    val measurer = rememberTextMeasurer()
    val style = LocalTextStyle.current.copy(fontWeight = FontWeight.Bold)
    val width = if (!showName) 36f else 16f + (if (showIcon) 22f else 0f) +
        measurer.measure(AnnotatedString(panel.getString("title")), style).size.width / dock.density
    SideEffect { if (enabled) dock.measure(id, tabWidth = width) }
    val dragged = dock.isDraggedTab(id)
    Row(modifier.height(36.dp).then(if (!showName) Modifier.width(36.dp) else Modifier)
        .zIndex(if (dragged) 2f else if (selected) 1f else 0f)
        .workspaceTabMotion(host, group, id, index, dock.density)
        .drawBehind {
            if (dragged && !selected) drawRect(colors.tabs)
            if (selected) {
                val r = SurfaceRadius.toPx(); val f = 6.dp.toPx(); val w = size.width; val h = size.height
                val path = Path().apply {
                    moveTo(r, 0f); lineTo(w-r, 0f); squircleTo(Offset(w-r, r), Offset(0f, -r), Offset(r, 0f))
                    lineTo(w, h-f); squircleTo(Offset(w+f, h-f), Offset(-f, 0f), Offset(0f, f))
                    lineTo(-f, h); squircleTo(Offset(-f, h-f), Offset(0f, f), Offset(f, 0f))
                    lineTo(0f, r); squircleTo(Offset(r, r), Offset(-r, 0f), Offset(0f, -r)); close()
                }
                drawPath(path, colors.panel)
            }
        }
        .combinedClickable(enabled = enabled,
            onClick = { host.dispatch(obj("type" to "select_panel_tab", "group" to group, "panel" to id)) },
            onLongClick = { dock.holdContext(obj("kind" to "panel", "panel" to id)) })
        .padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally)) {
        if (showIcon) SharedIcon(panel.getString("icon"), if (showName) null else panel.getString("title"),
            Modifier.testTag("tab-icon-$id"), tint = colors.text)
        if (showName) Text(panel.getString("title"), Modifier.testTag("tab-name-$id"), color = colors.text, fontWeight = FontWeight.Bold)
    }
}
