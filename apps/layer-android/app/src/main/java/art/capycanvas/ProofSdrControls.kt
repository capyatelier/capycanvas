package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

@Composable internal fun ProofSdrControls(form:JSONObject,send:(JSONObject)->Unit) {
    val current by rememberUpdatedState(form)
    val action by rememberUpdatedState(send)
    val texture=remember { Bitmap.createBitmap(Native.proofTexture(256),256,256,Bitmap.Config.ARGB_8888).asImageBitmap() }
    val values=form.getJSONArray("pad_values")
    Canvas(Modifier.fillMaxWidth().aspectRatio(1f).testTag("sdr-tone-pad").semantics {
        contentDescription="SDR balance and contrast"
        stateDescription="Balance ${round(values.getDouble(0)*100).toInt()}%, contrast ${round(values.getDouble(1)*100).toInt()}%"
    }.pointerInput(Unit) {
        awaitEachGesture {
            val down=awaitFirstDown(requireUnconsumed=false)
            val center=Offset(size.width/2f,size.height/2f)
            if((down.position-center).getDistance()>size.width/2f)return@awaitEachGesture
            var completed=false
            fun update(position:Offset,phase:String){var v=(position-center)/(size.width/2f);val length=v.getDistance();if(length>1f)v/=length;action(obj("type" to "pad","phase" to phase,"values" to JSONArray(listOf(v.x,-v.y))))}
            try {
                down.consume();update(down.position,"down");update(down.position,"move")
                while(true){val change=awaitPointerEvent().changes.firstOrNull{it.id==down.id}?:break
                    if(change.isConsumed)break
                    change.consume();if(!change.pressed){update(change.position,"up");completed=true;break};update(change.position,"move")}
            } finally {if(!completed)action(obj("type" to "pad","phase" to "cancel","values" to JSONArray(listOf(0,0))))}
        }
    }) {
        val path=Path().apply{addOval(androidx.compose.ui.geometry.Rect(Offset.Zero,size))}
        clipPath(path){drawImage(texture,dstSize=androidx.compose.ui.unit.IntSize(size.width.toInt(),size.height.toInt()))}
        val marker=Offset(size.width*(.5f+values.getDouble(0).toFloat()*.46f),size.height*(.5f-values.getDouble(1).toFloat()*.46f))
        drawCircle(Color.Black,8.dp.toPx(),marker,style=Stroke(3.dp.toPx()));drawCircle(Color.White,8.dp.toPx(),marker,style=Stroke(1.5.dp.toPx()))
    }
    for(spec in form.getJSONArray("numbers").objects())key(spec.getString("key")) {
        val name=spec.getString("key");val numeric=spec.getJSONObject("numeric")
        var active by remember {mutableStateOf(false)}
        var value by remember {mutableFloatStateOf(form.getJSONObject("rendition").number(name))}
        LaunchedEffect(form.toString(),active){if(!active)value=form.getJSONObject("rendition").number(name)}
        fun edit(phase:String){action(obj("type" to "rendition","phase" to phase,"recipe" to JSONObject(current.getJSONObject("rendition").toString()).put(name,value)))}
        val interactions=remember{MutableInteractionSource()}
        LaunchedEffect(interactions){interactions.interactions.collect{if(it is DragInteraction.Cancel&&active){active=false;edit("cancel")}}}
        DisposableEffect(Unit){onDispose{if(active)edit("cancel")}}
        Text("${spec.getString("label")} ${round(value*numeric.number("scale")).toInt()}${numeric.optString("unit")}")
        Slider(value,onValueChange={if(!active){active=true;edit("down")};value=it;edit("move")},onValueChangeFinished={if(active){active=false;edit("up")}},valueRange=numeric.number("min")..numeric.number("max"),interactionSource=interactions,modifier=Modifier.testTag("sdr-$name"))
    }
    TextButton({val recipe=JSONObject(current.getJSONObject("rendition").toString()).put("exposure",0).put("contrast",1).put("balance",0).put("highlight_color",.3);action(obj("type" to "rendition","phase" to "down","recipe" to recipe));action(obj("type" to "rendition","phase" to "up","recipe" to recipe))}){Text("Reset SDR appearance")}
    Text("Saved for SDR viewing, print simulation and SDR delivery. This display presents mapped SDR.")
}
