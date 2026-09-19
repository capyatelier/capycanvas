package art.capycanvas

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

@Composable internal fun ColorChoice(label: String, choices: List<Pair<String, String>>, value: String, enabled: Boolean = true, onChange: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Column {
        Text(label, style = MaterialTheme.typography.labelMedium)
        Box {
            OutlinedButton({ open = true }, enabled = enabled, modifier = Modifier.fillMaxWidth().testTag("color-choice-$label")) {
                Text(choices.firstOrNull { it.first == value }?.second ?: value)
            }
            DropdownMenu(open && enabled, { open = false }) {
                for ((id, title) in choices) DropdownMenuItem(text = { Text(title) }, onClick = { open = false; onChange(id) })
            }
        }
    }
}
@Composable internal fun NewDrawingDialog(host: CanvasHost, spec: JSONObject, onDismiss: () -> Unit, onCreate: (JSONObject) -> Unit) {
    val model = remember { spec.getJSONObject("creation") }
    var options by remember { mutableStateOf(JSONObject(model.getJSONObject("options").toString())) }
    var width by remember { mutableStateOf(options.getJSONArray("extent").getInt(0).toString()) }
    var height by remember { mutableStateOf(options.getJSONArray("extent").getInt(1).toString()) }
    var preset by remember { mutableStateOf("custom") }
    var name by remember { mutableStateOf("") }
    var defaults by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val w = width.toIntOrNull(); val h = height.toIntOrNull()
    val valid = w != null && h != null && w in 1..spec.getInt("max_dimension") && h in 1..spec.getInt("max_dimension")
    fun color(field: String, value: String) {
        options = JSONObject(options.toString()).apply { getJSONObject("color").put(field, value) }
    }
    AlertDialog(onDismissRequest = { if (!saving) onDismiss() }, title = { Text(spec.getString("new_title")) },
        confirmButton = { TextButton(enabled = valid && !saving, modifier = Modifier.testTag("new-document-create"), onClick = {
            val selected = JSONObject(options.toString()).put("extent", JSONArray(listOf(w!!, h!!)))
            scope.launch {
                saving = true
                try {
                    if (name.isNotBlank() || defaults) {
                        host.withNative { Native.dispatch(it, obj("type" to "new_document_preferences", "action" to obj("type" to "remember", "options" to selected, "name" to name, "defaults" to defaults)).toString()) }
                        host.documentChanged()
                    }
                    onCreate(selected)
                } catch (e: Exception) { error = e.message ?: "Could not create the drawing" }
                finally { saving = false }
            }
        }) { Text("Create") } }, dismissButton = { TextButton(onDismiss, enabled = !saving) { Text("Cancel") } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 560.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                val presets = model.getJSONArray("presets")
                ColorChoice("Preset", listOf("custom" to "Custom") + presets.objects().mapIndexed { i, p -> i.toString() to p.getString("name") }, preset) { id ->
                    preset = id
                    id.toIntOrNull()?.let { index ->
                        options = JSONObject(presets.getJSONObject(index).getJSONObject("options").toString())
                        width = options.getJSONArray("extent").getInt(0).toString(); height = options.getJSONArray("extent").getInt(1).toString()
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(width, { width = it }, label = { Text(spec.getString("width_label")) }, modifier = Modifier.weight(1f).testTag("new-document-width"), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true)
                    OutlinedTextField(height, { height = it }, label = { Text(spec.getString("height_label")) }, modifier = Modifier.weight(1f).testTag("new-document-height"), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), singleLine = true)
                }
                val spaces = model.getJSONArray("spaces")
                ColorChoice("Color space", (0 until spaces.length()).map { spaces.getJSONArray(it).let { a -> a.getString(0) to a.getString(1) } }, options.getJSONObject("color").getString("space")) { color("space", it) }
                ColorChoice("Bit depth", listOf("U8" to "8-bit SDR", "U16" to "16-bit SDR", "F16" to "16-bit float HDR"), options.getJSONObject("color").getString("depth")) { color("depth", it) }
                ColorChoice("Background", listOf("White" to "White", "Transparent" to "Transparent"), options.getString("background")) { options = JSONObject(options.toString()).put("background", it) }
                OutlinedTextField(name, { name = it }, label = { Text("Save as preset (optional)") }, singleLine = true)
                Row { Checkbox(defaults, { defaults = it }); Text("Use as defaults", Modifier.padding(top = 12.dp)) }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
        })
}
