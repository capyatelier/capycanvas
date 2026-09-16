package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

@Composable internal fun ColorLibraryDialog(host: CanvasHost, slot: String, onDismiss: () -> Unit) {
    var colors by remember { mutableStateOf(JSONObject(host.panelContent!!.getJSONObject("state").getJSONObject("colors").toString())) }
    var palette by remember { mutableStateOf(colors.getJSONObject("library").getJSONArray("palettes").getJSONObject(0).getLong("id")) }
    var name by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    fun apply(action: JSONObject) {
        scope.launch {
            busy = true
            try {
                val next = host.withNative {
                    Native.dispatch(it, obj("type" to "color", "action" to obj("op" to "library", "action" to action)).toString())
                    JSONObject(Native.snapshot(it)!!).getJSONObject("state").getJSONObject("colors")
                }
                colors = next
                val list = next.getJSONObject("library").getJSONArray("palettes").objects()
                if (action.getString("op") == "create_palette") palette = list.last().getLong("id")
                if (list.none { it.getLong("id") == palette }) palette = list.first().getLong("id")
                error = null; host.documentChanged()
            } catch (e: Exception) { error = e.message ?: "Could not change the palette" }
            finally { busy = false }
        }
    }
    if (removing) AlertDialog(onDismissRequest = { removing = false }, title = { Text("Remove palette?") },
        text = { Text("This removes the palette and its saved colors.") },
        confirmButton = { TextButton({ removing = false; apply(obj("op" to "remove_palette", "id" to palette)) }) { Text("Remove") } },
        dismissButton = { TextButton({ removing = false }) { Text("Cancel") } })
    AlertDialog(onDismissRequest = onDismiss, title = { Text("Palettes") }, confirmButton = { TextButton(onDismiss) { Text("Close") } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 560.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                val list = colors.getJSONObject("library").getJSONArray("palettes").objects().toList()
                ColorChoice("Palette", list.map { it.getLong("id").toString() to it.getString("name") }, palette.toString()) { palette = it.toLong() }
                OutlinedTextField(name, { name = it }, label = { Text("Palette or swatch name") }, singleLine = true)
                Row {
                    TextButton({ apply(obj("op" to "create_palette", "name" to name)) }, enabled = !busy) { Text("New") }
                    TextButton({ apply(obj("op" to "rename_palette", "id" to palette, "name" to name)) }, enabled = !busy) { Text("Rename") }
                    TextButton({ removing = true }, enabled = !busy && list.size > 1) { Text("Remove") }
                }
                OutlinedButton({ apply(obj("op" to "store", "palette" to palette, "name" to name, "color" to colors.getJSONObject(slot))) }, enabled = !busy) { Text("Save Current Color") }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                val swatches = list.first { it.getLong("id") == palette }.getJSONArray("swatches").objects().toList()
                if (swatches.isEmpty()) Text("No saved colors yet.")
                for (swatch in swatches) key(swatch.getLong("id")) {
                    var text by remember(swatch.getString("name")) { mutableStateOf(swatch.getString("name")) }
                    val preview = remember(swatch.getJSONObject("color").toString()) {
                        JSONArray(Native.colorUi(obj("type" to "preview", "colors" to JSONArray().put(swatch.getJSONObject("color"))).toString())).getJSONObject(0)
                    }
                    Column {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button({ host.dispatch(obj("type" to "color", "action" to obj("op" to "set_slot", "slot" to slot, "color" to swatch.getJSONObject("color")))); onDismiss() }, enabled = !busy,
                                colors = ButtonDefaults.buttonColors(containerColor = displayColor(preview)), modifier = Modifier.width(56.dp).height(48.dp), contentPadding = PaddingValues(0.dp)) { Text("Use") }
                            OutlinedTextField(text, { text = it }, label = { Text("Swatch name") }, singleLine = true, modifier = Modifier.weight(1f))
                        }
                        if (!preview.getBoolean("in_gamut")) Text("Outside sRGB preview gamut", style = MaterialTheme.typography.labelSmall)
                        Row {
                            TextButton({ apply(obj("op" to "rename", "id" to swatch.getLong("id"), "name" to text)) }, enabled = !busy) { Text("Rename") }
                            TextButton({ apply(obj("op" to "remove", "id" to swatch.getLong("id"))) }, enabled = !busy) { Text("Remove") }
                        }
                    }
                }
            }
        })
}
