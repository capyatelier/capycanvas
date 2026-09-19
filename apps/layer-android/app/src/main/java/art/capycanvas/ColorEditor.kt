package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
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
    val hdr=host.panelContent?.objectOrNull("color_panel")?.optBoolean("hdr")==true
    var form by remember {
        mutableStateOf(JSONObject(Native.colorUi(obj("type" to "form", "request" to obj(
            "color" to initial, "document_depth" to host.panelContent?.objectOrNull("state")?.objectOrNull("colors")?.optString("hdr_depth"), "document_space" to documentRgbSpace(host), "model" to (if(hdr)"linear_rgb" else "document_rgb"), "intensity" to initialIntensity, "rendition" to host.panelContent?.objectOrNull("color_panel")?.objectOrNull("rendition")
        )).toString())))
    }
    var intensityText by remember {mutableStateOf(if(form.getJSONObject("draft").isNull("intensity"))"" else form.getJSONObject("draft").getDouble("intensity").toString())}
    var transportError by remember { mutableStateOf<String?>(null) }
    fun change(draft: JSONObject) {
        try {
            if(hdr){val stops=intensityText.trim().toFloatOrNull();require(stops!=null&&stops.isFinite()){"Enter a finite EV value"};draft.put("change_intensity",stops)}
            form = JSONObject(Native.colorUi(obj("type" to "form", "request" to draft).toString())); transportError = null }
        catch (e: Exception) { transportError = e.message ?: "Could not read this color" }
    }
    val draft = form.getJSONObject("draft")
    val labels = form.getJSONArray("labels")
    var modelsOpen by remember { mutableStateOf(false) }
    val colors=LocalPalette.current
    @Composable fun comparison(){
        form.objectOrNull("preview")?.let { preview ->
            Row(Modifier.fillMaxWidth()){
                form.objectOrNull("base_preview")?.let{base->Column(Modifier.weight(1f),horizontalAlignment=Alignment.CenterHorizontally){Text("Base",color=colors.secondary,style=MaterialTheme.typography.labelSmall);Spacer(Modifier.height(4.dp));Box(Modifier.fillMaxWidth().height(48.dp).background(displayColor(base)))}}
                Column(Modifier.weight(1f),horizontalAlignment=Alignment.CenterHorizontally){if(!form.isNull("base_preview")){Text("Adjusted",color=colors.secondary,style=MaterialTheme.typography.labelSmall);Spacer(Modifier.height(4.dp))};Box(Modifier.fillMaxWidth().height(48.dp).background(displayColor(preview)))}
            }
        }
    }
    Dialog(onDismissRequest=onDismiss,properties=DialogProperties(usePlatformDefaultWidth=false)) {
        Surface(Modifier.padding(24.dp).widthIn(max=400.dp).fillMaxWidth().heightIn(max=(LocalConfiguration.current.screenHeightDp-48).dp),
            shape=RoundedCornerShape(20.dp),color=colors.settingsBackground) {
            Column(Modifier.padding(24.dp),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                Text("Edit Color",Modifier.fillMaxWidth(),textAlign=TextAlign.Center,fontSize=20.sp,fontWeight=androidx.compose.ui.text.font.FontWeight.Bold)
                Column(Modifier.weight(1f,fill=false).verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                    Text(form.getString("description"),color=colors.secondary)
                    if(hdr)comparison()
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(colors.button)) {
                        Box {
                            val models=form.getJSONArray("models")
                            val selected=(0 until models.length()).map{models.getJSONArray(it)}.first{it.getString(0)==draft.getString("model")}
                            Row(Modifier.fillMaxWidth().heightIn(min=54.dp).clickable{modelsOpen=true}.testTag("color-input-model").padding(horizontal=12.dp),
                                verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                                Text("Model",Modifier.weight(1f));Text(selected.getString(1));SharedIcon("chevron-down",null,Modifier.size(12.dp))
                            }
                            DropdownMenu(modelsOpen,{modelsOpen=false}) {
                                for(i in 0 until models.length()) {val option=models.getJSONArray(i)
                                    DropdownMenuItem(text={Text(option.getString(1))},onClick={modelsOpen=false;change(JSONObject(draft.toString()).put("change_model",option.getString(0)))})
                                }
                            }
                        }
                        if(!draft.isNull("intensity"))ColorEntry("Intensity (EV)",intensityText,"color-intensity-value") {text->
                            intensityText=text;val n=text.toFloatOrNull()
                            if(n!=null&&n.isFinite())change(JSONObject(draft.toString()).put("change_intensity",n))else transportError="Enter a finite EV value"
                        }
                        for(i in 0..3)if(labels.getString(i).isNotBlank())ColorEntry(labels.getString(i),draft.getJSONArray("fields").getString(i),"color-input-$i") {text->
                            val next=JSONObject(draft.toString());next.getJSONArray("fields").put(i,text);change(next)
                        }
                    }
                    if(!hdr)comparison()
                    val error=transportError?:form.optString("error").takeIf{it!="null"&&it.isNotBlank()}
                    Text(error?:form.optString("validation",""),color=if(error!=null)MaterialTheme.colorScheme.error else colors.text)
                }
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    Button(onDismiss,Modifier.weight(1f),shape=RoundedCornerShape(8.dp),colors=ButtonDefaults.buttonColors(containerColor=colors.button,contentColor=colors.text)){Text("Cancel")}
                    Button(enabled=!form.isNull("value")&&form.isNull("error")&&transportError==null,onClick={
                        if(colorEpoch(host)==epoch){onIntensity(if(draft.isNull("intensity"))null else draft.number("intensity"));onUse(form.getJSONObject("value"))}else onDismiss()
                    },modifier=Modifier.weight(1f),shape=RoundedCornerShape(8.dp)){Text("Use Color")}
                }
            }
        }
    }
}

/** GTK EntryRow geometry, with the host retaining partial text while editing. */
@Composable private fun ColorEntry(label:String,value:String,tag:String,onChange:(String)->Unit) {
    val colors=LocalPalette.current
    HorizontalDivider(color=colors.divider)
    BasicTextField(value,onChange,singleLine=true,textStyle=MaterialTheme.typography.bodyMedium.copy(color=colors.text),cursorBrush=SolidColor(colors.accent),
        modifier=Modifier.fillMaxWidth().heightIn(min=53.dp).testTag(tag).semantics{contentDescription=label},
        decorationBox={inner->Row(Modifier.fillMaxWidth().padding(horizontal=12.dp,vertical=6.dp),verticalAlignment=Alignment.CenterVertically) {
            Column(Modifier.weight(1f)){Text(label,color=colors.secondary,fontSize=12.sp);inner()}
            SharedIcon("pencil",null,Modifier.padding(start=8.dp).size(16.dp),tint=colors.secondary)
        }})
}
