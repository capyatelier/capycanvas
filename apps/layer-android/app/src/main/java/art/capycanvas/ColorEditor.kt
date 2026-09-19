package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject

internal fun displayColor(preview: JSONObject): Color {
    val v = preview.getJSONArray("rgba")
    return Color(v.getDouble(0).toFloat(), v.getDouble(1).toFloat(), v.getDouble(2).toFloat(), v.getDouble(3).toFloat())
}
internal fun documentRgbSpace(host: CanvasHost): String =
    host.panelContent?.objectOrNull("color_panel")?.optString("rgb_space")?.takeIf { it.isNotBlank() } ?: "Srgb"
private fun colorEpoch(host: CanvasHost): Long =
    host.panelContent?.objectOrNull("state")?.objectOrNull("document_file")?.optLong("epoch") ?: 0L

@Composable internal fun ManagedColorButton(host: CanvasHost, label: String, value: JSONObject, enabled: Boolean, onChange: (JSONObject) -> Unit) {
    var editing by remember { mutableStateOf<JSONObject?>(null) }
    val preview = remember(value.toString()) {
        JSONArray(Native.colorUi(obj("type" to "preview", "colors" to JSONArray().put(value)).toString())).getJSONObject(0)
    }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Text(label, Modifier.weight(1f))
        Button(onClick = { editing = JSONObject(value.toString()) }, enabled = enabled,
            modifier = Modifier.size(56.dp, 32.dp).testTag("property-color-$label"), contentPadding = PaddingValues(0.dp),
            colors = ButtonDefaults.buttonColors(containerColor = displayColor(preview))) { Text("…") }
    }
    if (!preview.getBoolean("in_gamut")) Text("Outside sRGB preview gamut", style = MaterialTheme.typography.labelSmall)
    editing?.let { color -> ColorEditorDialog(host, color, { editing = null }) { selected -> editing = null; onChange(selected) } }
}

@Composable internal fun ColorEditorDialog(host: CanvasHost, initial: JSONObject, onDismiss: () -> Unit, initialIntensity:Float?=null, onIntensity:(Float?)->Unit={}, onUse: (JSONObject) -> Unit) {
    val epoch = remember { colorEpoch(host) }
    val hdrIntensity=initialIntensity ?: if(host.panelContent?.objectOrNull("color_panel")?.optBoolean("hdr")==true)0f else null
    var form by remember {
        mutableStateOf(JSONObject(Native.colorUi(obj("type" to "form", "request" to obj(
            "color" to initial, "document_space" to documentRgbSpace(host), "model" to (if(hdrIntensity!=null)"linear_rgb" else "document_rgb"), "intensity" to hdrIntensity, "rendition" to host.panelContent?.objectOrNull("color_panel")?.objectOrNull("rendition")
        )).toString())))
    }
    var intensityText by remember {mutableStateOf(if(form.getJSONObject("draft").isNull("intensity"))"" else form.getJSONObject("draft").getDouble("intensity").toString())}
    var transportError by remember { mutableStateOf<String?>(null) }
    fun change(draft: JSONObject) {
        try { form = JSONObject(Native.colorUi(obj("type" to "form", "request" to draft).toString())); transportError = null }
        catch (e: Exception) { transportError = e.message ?: "Could not read this color" }
    }
    val draft = form.getJSONObject("draft")
    val labels = form.getJSONArray("labels")
    var modelsOpen by remember { mutableStateOf(false) }
    AlertDialog(onDismissRequest = onDismiss, title = { Text("Edit Color") },
        confirmButton = {
            TextButton(enabled = !form.isNull("value") && transportError == null, onClick = {
                if (colorEpoch(host) == epoch) {onIntensity(if(draft.isNull("intensity"))null else draft.number("intensity"));onUse(form.getJSONObject("value"))} else onDismiss()
            }) { Text("Use Color") }
        }, dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        text = {
            Column(Modifier.fillMaxWidth().heightIn(max = 540.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(form.getString("description"))
                Box {
                    val models = form.getJSONArray("models")
                    val selected = (0 until models.length()).map { models.getJSONArray(it) }.first { it.getString(0) == draft.getString("model") }
                    TextButton(onClick = { modelsOpen = true }, modifier = Modifier.testTag("color-input-model")) { Text(selected.getString(1)) }
                    DropdownMenu(expanded = modelsOpen, onDismissRequest = { modelsOpen = false }) {
                        for (i in 0 until models.length()) {
                            val option = models.getJSONArray(i)
                            DropdownMenuItem(text = { Text(option.getString(1)) }, onClick = {
                                modelsOpen = false; change(JSONObject(draft.toString()).put("change_model", option.getString(0)))
                            })
                        }
                    }
                }
                if(!draft.isNull("intensity")) OutlinedTextField(value=intensityText,onValueChange={text->intensityText=text;val n=text.toFloatOrNull();if(n!=null)change(JSONObject(draft.toString()).put("change_intensity",n))else transportError="Enter an intensity in stops"},label={Text("Intensity (EV)")},singleLine=true,modifier=Modifier.testTag("color-intensity-value"))
                form.objectOrNull("preview")?.let { preview ->
                    Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)){
                        form.objectOrNull("base_preview")?.let{base->Column(Modifier.weight(1f)){Box(Modifier.fillMaxWidth().height(48.dp).background(displayColor(base)));Text("Base")}}
                        Column(Modifier.weight(1f)){Box(Modifier.fillMaxWidth().height(48.dp).background(displayColor(preview)));if(!form.isNull("base_preview"))Text("Adjusted")}
                    }
                    if (!preview.getBoolean("in_gamut")) Text("Outside the sRGB preview gamut. The stored color is preserved.")
                }
                for (i in 0..3) if (labels.getString(i).isNotBlank()) {
                    OutlinedTextField(value = draft.getJSONArray("fields").getString(i), onValueChange = { text ->
                        val next = JSONObject(draft.toString()); next.getJSONArray("fields").put(i, text); change(next)
                    }, label = { Text(labels.getString(i)) }, singleLine = true, modifier = Modifier.fillMaxWidth().testTag("color-input-$i"))
                }
                val error = transportError ?: form.optString("error").takeIf { it != "null" && it.isNotBlank() }
                if (error != null) Text(error, color = MaterialTheme.colorScheme.error)
            }
        })
}
