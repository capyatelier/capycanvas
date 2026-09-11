package art.capycanvas

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.JSONObject

/** The selected tool determines groups, subtools and settings in Rust. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun ToolSetControls(host: CanvasHost, state: JSONObject) {
    val view = state.getJSONObject("tool_set")
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("groups").objects().forEach { item ->
                ToolChoice(host, item, Modifier.width(108.dp).testTag("tool-group-${item.getString("label")}"))
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            view.array("subtools").objects().forEach { item ->
                ToolChoice(host, item, Modifier.fillMaxWidth().testTag("subtool-${item.getString("label")}"))
            }
        }
    }
}

@Composable private fun ToolChoice(host: CanvasHost, item: JSONObject, modifier: Modifier) {
    val colors = LocalPalette.current
    val context = LocalContext.current
    val label = item.getString("label")
    val action = item.getJSONObject("action")
    val preview = item.opt("preview").takeIf { it is Number } as? Number
    ActionTip(host, label, action, modifier) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp))
            .background(if (item.optBoolean("selected")) colors.active else colors.panel)
            .clickable { host.dispatch(action) }.padding(horizontal = 6.dp, vertical = 3.dp)) {
            if (preview != null) {
                val id = preview.toInt()
                val swatch = remember(id, colors.dark) {
                    context.assets.open("$id-${if (colors.dark) "dark" else "light"}.png").use { BitmapFactory.decodeStream(it).asImageBitmap() }
                }
                Image(swatch, null, Modifier.fillMaxWidth().height(40.dp).testTag("brush-preview-$id"), contentScale = ContentScale.FillBounds)
                Text(label, Modifier.fillMaxWidth(), textAlign = TextAlign.End, fontWeight = FontWeight.Bold)
            } else Row(Modifier.heightIn(min = 30.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                SharedIcon(item.getString("icon"), null)
                Text(label, fontWeight = FontWeight.Bold, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable internal fun ToolSettingsControls(host: CanvasHost, state: JSONObject) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        var group = ""
        state.array("tool_settings").objects().forEach { field ->
            val next = field.getString("group")
            if (next != group && next.isNotEmpty()) Text(next, fontWeight = FontWeight.Bold, color = LocalPalette.current.secondary)
            group = next
            val id = field.getString("id")
            Box(Modifier.testTag("tool-setting-$id")) {
                NumericSetting(field.getString("label"), field.number("value"), field.getJSONObject("numeric")) {
                    host.dispatch(obj("type" to "set_tool_setting", "id" to id, "value" to it))
                }
            }
        }
        state.array("tool_actions").objects().forEach { action ->
            val id = action.getString("command")
            state.array("commands").objects().find { it.getString("id") == id }?.let { command ->
                if (action.optBoolean("checkable")) Row(Modifier.testTag("tool-action-$id"), verticalAlignment = Alignment.CenterVertically) {
                    EditorCheck(command.optBoolean("selected"), command.getString("label"), enabled = command.getBoolean("enabled")) { host.invoke(id) }
                    Text(command.getString("label"))
                } else TextButton({ host.invoke(id) }, Modifier.testTag("tool-action-$id"), enabled = command.getBoolean("enabled")) {
                    Text(command.getString("label"))
                }
            }
        }
    }
}
