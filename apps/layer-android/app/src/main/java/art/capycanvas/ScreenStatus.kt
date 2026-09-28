package art.capycanvas

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.JSONObject

@Composable internal fun ScreenStatus(host: CanvasHost, screen: JSONObject?) {
    val chip = screen?.optJSONObject("chip") ?: return
    val details = screen.optJSONObject("details")
    val palette = LocalPalette.current
    val warning = if (palette.dark) Color(0xFFE5A50A) else Color(0xFF9C5700)
    val button = remember { WindowlessMenuButton() }
    val open = button.menu != null && details != null
    val dismiss = { button.menu = null; button.closedAt = android.os.SystemClock.uptimeMillis() }
    WindowlessPopup(open, dismiss)
    Box {
        Surface(Modifier.glass(TileShape), color = palette.headerSurface, shape = TileShape) {
            Row(Modifier.testTag("screen-status").opensWindowlessMenu(button, "Screen details") { load -> load(JSONObject()) }
                .padding(horizontal = 10.dp, vertical = 3.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                if (chip.optBoolean("warning")) SharedIcon("warning", null, tint = warning)
                Text(chip.getString("label"), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        if (open && details != null) DropdownMenu(true, dismiss, Modifier.width(320.dp).testTag("screen-details"),
            properties = WindowlessMenu, shape = RoundedCornerShape(10.dp), containerColor = palette.panel) {
            Column(Modifier.padding(horizontal = 16.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(details.getString("title"), color = palette.text.copy(alpha = .7f))
                Text(details.getString("headline"), fontWeight = FontWeight.SemiBold,
                    color = if (details.optBoolean("warning")) warning else palette.text)
                if (!details.isNull("body")) Text(details.getString("body"), color = palette.text)
                if (!details.isNull("show_clipped")) {
                    val checked = details.optBoolean("show_clipped")
                    val toggle = { visible: Boolean -> host.dispatch(obj("type" to "show_clipped_colors", "visible" to visible)) }
                    Row(Modifier.fillMaxWidth().testTag("screen-highlight").toggleable(checked, role = Role.Checkbox, onValueChange = toggle),
                        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        EditorCheck(checked, "Highlight these colors", Modifier.clearAndSetSemantics {}, true, toggle)
                        Text("Highlight these colors", color = palette.text)
                    }
                }
            }
        }
    }
}
