package art.capycanvas

import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONObject

/** Docked, floating and drawer tabs share geometry, colors and focus feedback. */
@Composable internal fun WorkspaceTab(host: CanvasHost, dock: DockInteraction, panel: JSONObject,
    group: Int, index: Int, selected: Boolean, modifier: Modifier, enabled: Boolean = true) {
    val colors = LocalPalette.current
    val id = panel.getString("id")
    val presentation = panel.getJSONObject("tab")
    val showIcon = presentation.getBoolean("show_icon")
    val showName = presentation.getBoolean("show_name")
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
                val r = 6.dp.toPx(); val w = size.width; val h = size.height
                val path = Path().apply {
                    moveTo(r, 0f); lineTo(w-r, 0f); quadraticTo(w, 0f, w, r)
                    lineTo(w, h-r); quadraticTo(w, h, w+r, h)
                    lineTo(-r, h); quadraticTo(0f, h, 0f, h-r)
                    lineTo(0f, r); quadraticTo(0f, 0f, r, 0f); close()
                }
                drawPath(path, colors.panel)
            }
        }
        .combinedClickable(enabled = enabled,
            onClick = { host.dispatch(obj("type" to "select_panel_tab", "group" to group, "panel" to id)) },
            onLongClick = { dock.context(obj("kind" to "panel", "panel" to id)) })
        .padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally)) {
        if (showIcon) SharedIcon(panel.getString("icon"), if (showName) null else panel.getString("title"),
            Modifier.testTag("tab-icon-$id"), tint = colors.text)
        if (showName) Text(panel.getString("title"), Modifier.testTag("tab-name-$id"), color = colors.text, fontWeight = FontWeight.Bold)
    }
}
