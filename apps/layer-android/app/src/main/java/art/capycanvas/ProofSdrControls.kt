package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Paint
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.*
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.input.key.*
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.*

/** GTK geometry, hit testing and recipe mapping stay in shared Rust. */
@Composable internal fun ProofSdrControls(form:JSONObject,send:(JSONObject)->Unit) {
    val current by rememberUpdatedState(form)
    val action by rememberUpdatedState(send)
    val texture=remember { Bitmap.createBitmap(Native.proofTexture(256),256,256,Bitmap.Config.ARGB_8888).asImageBitmap() }
    val ink=LocalPalette.current.text
    var selectedPart by remember {mutableIntStateOf(0)}
    var gestureActive by remember {mutableStateOf(false)}
    var gestureCancelled by remember {mutableStateOf(false)}
    fun atomic(recipe:JSONObject){action(obj("type" to "rendition","phase" to "down","recipe" to current.getJSONObject("rendition")));action(obj("type" to "rendition","phase" to "up","recipe" to recipe))}
    fun reset(part:Int){val r=JSONObject(current.getJSONObject("rendition").toString());when(part){0->{r.put("balance",0);r.put("contrast",1)};1->r.put("exposure",0);2->r.put("highlight_color",.3);else->{r.put("balance",0);r.put("contrast",1);r.put("exposure",0);r.put("highlight_color",.3)}};atomic(r)}
    fun nudge(part:Int,delta:Double){val r=JSONObject(current.getJSONObject("rendition").toString());val key=if(part==1)"exposure" else "highlight_color";r.put(key,(r.getDouble(key)+delta).coerceIn(if(part==1)-2.0 else 0.0,if(part==1)2.0 else 1.0));atomic(r)}
    BoxWithConstraints(Modifier.fillMaxWidth().aspectRatio(1f)) {
        val side=maxWidth.value
        val density=LocalDensity.current.density
        fun query(point:Offset?=null,part:Int?=null)=JSONObject(Native.colorUi(obj("type" to "proof_dial","size" to side,"recipe" to current.getJSONObject("rendition"),"point" to point?.let{JSONArray(listOf(it.x/density,it.y/density))},"part" to part).toString()))
        val dial=remember(form.toString(),side){query()}
        fun JSONArray.point()=Offset(getDouble(0).toFloat()*density,getDouble(1).toFloat()*density)
        Canvas(Modifier.fillMaxSize().testTag("sdr-tone-pad").semantics {
            contentDescription="SDR appearance"
            stateDescription=listOf("Contrast","Balance","Brightness","Color intensity").mapIndexed { index,label -> "$label ${round(dial.getJSONArray("percentages").getDouble(index)).toInt()}%" }.joinToString(", ")
            customActions=listOf(CustomAccessibilityAction("Reset SDR appearance"){reset(3);true},CustomAccessibilityAction("Increase brightness"){nudge(1,.04);true},CustomAccessibilityAction("Decrease brightness"){nudge(1,-.04);true},CustomAccessibilityAction("Increase color intensity"){nudge(2,.01);true},CustomAccessibilityAction("Decrease color intensity"){nudge(2,-.01);true})
        }.onKeyEvent {e->
            if(e.type!=KeyEventType.KeyDown)false else when(e.key){
                Key.Escape->{if(gestureActive){gestureCancelled=true;action(obj("type" to "rendition","phase" to "cancel","recipe" to current.getJSONObject("rendition")))};true}
                Key.MoveHome->{reset(selectedPart);true}
                Key.DirectionLeft,Key.DirectionRight,Key.DirectionUp,Key.DirectionDown->{val sign=if(e.key==Key.DirectionLeft||e.key==Key.DirectionDown)-1 else 1;val step=sign*(if(e.isShiftPressed).1 else .02);if(selectedPart in 1..2)nudge(selectedPart,step*(if(selectedPart==1)2 else 1))else{val v=current.getJSONArray("pad_values");val i=if(e.key==Key.DirectionLeft||e.key==Key.DirectionRight)0 else 1;val next=JSONArray(v.toString()).put(i,(v.getDouble(i)+step).coerceIn(-1.0,1.0));action(obj("type" to "pad","phase" to "down","values" to v));action(obj("type" to "pad","phase" to "up","values" to next))};true}
                else->false
            }
        }.focusable().pointerInput(side) {
            var lastTap=0L;var lastPart=-1
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false);val initial=query(down.position)
                if(initial.isNull("hit"))return@awaitEachGesture
                val part=initial.getInt("hit");selectedPart=part;gestureActive=true;gestureCancelled=false;var completed=false;var moved=false
                fun update(position:Offset,phase:String){action(obj("type" to "rendition","phase" to phase,"recipe" to query(position,part).getJSONObject("recipe")))}
                try {
                    down.consume();action(obj("type" to "rendition","phase" to "down","recipe" to current.getJSONObject("rendition")));update(down.position,"move")
                    while(true){val c=awaitPointerEvent().changes.firstOrNull{it.id==down.id}?:break;if(gestureCancelled){completed=true;break};if(c.isConsumed)break;c.consume();moved=moved||(c.position-down.position).getDistance()>viewConfiguration.touchSlop
                        if(!c.pressed){update(c.position,"up");completed=true;if(!moved){if(lastPart==part&&c.uptimeMillis-lastTap<viewConfiguration.doubleTapTimeoutMillis){reset(part);lastPart=-1}else{lastTap=c.uptimeMillis;lastPart=part}};break};if(part!=3)update(c.position,"move")}
                } finally {gestureActive=false;if(!completed&&!gestureCancelled)action(obj("type" to "rendition","phase" to "cancel","recipe" to current.getJSONObject("rendition")))}
            }
        }) {
            val center=dial.getJSONArray("center").point();val radius=dial.number("radius")*density
            val path=Path().apply{addOval(androidx.compose.ui.geometry.Rect(center-Offset(radius,radius),center+Offset(radius,radius)))}
            clipPath(path){drawImage(texture,dstOffset=IntOffset((center.x-radius).toInt(),(center.y-radius).toInt()),dstSize=IntSize((radius*2).toInt(),(radius*2).toInt()))}
            fun marker(point:Offset,r:Float){drawCircle(Color.Black,r,point,style=Stroke(3*density));drawCircle(Color.White,r,point,style=Stroke(1.5f*density))}
            dial.getJSONArray("arcs").objects().forEachIndexed{i,a->val g=a.getJSONObject("geometry");val points=a.getJSONArray("path");val arc=Path().apply{for(j in 0 until points.length()){val p=points.getJSONArray(j).point();if(j==0)moveTo(p.x,p.y)else lineTo(p.x,p.y)}}
                val colors=if(i==0)listOf(Color(.04f,.04f,.04f),Color(.55f,.55f,.55f),Color.White)else listOf(Color(.95f,.95f,.95f),Color(.15f,.55f,.85f))
                drawPath(arc,Brush.linearGradient(colors,points.getJSONArray(0).point(),points.getJSONArray(points.length()-1).point()),style=Stroke(g.number("width")*density,cap=StrokeCap.Round));marker(a.getJSONArray("point").point(),g.number("marker_radius")*density)
            }
            marker(dial.getJSONArray("marker").point(),dial.number("marker_radius")*density)
            val paint=Paint(Paint.ANTI_ALIAS_FLAG).apply{color=ink.toArgb();textSize=dial.number("text_size")*density;textAlign=Paint.Align.CENTER}
            val native=drawContext.canvas.nativeCanvas
            dial.getJSONArray("readouts").objects().forEachIndexed{i,r->val value=round(dial.getJSONArray("percentages").getDouble(i)).toInt();val text=(if(i in 1..2&&value>=0)"+" else "")+value+"%";val curve=r.optJSONArray("curve")
                if(curve==null){val p=r.getJSONArray("text").point();native.drawText(text,p.x,p.y,paint)}else{val radius=curve.getDouble(0).toFloat()*density;val angle=curve.getDouble(1);val reverse=curve.getBoolean(2);var advance=-paint.measureText(text)/2;for(ch in text){val label=ch.toString();val w=paint.measureText(label);val a=angle*PI/180+(if(reverse)-1 else 1)*(advance+w/2)/radius;native.save();native.translate(center.x+radius*cos(a).toFloat(),center.y+radius*sin(a).toFloat());native.rotate((a*180/PI+(if(reverse)-90 else 90)).toFloat());native.drawText(label,0f,0f,paint);native.restore();advance+=w}}
            }
        }
        dial.getJSONArray("readouts").objects().forEachIndexed{i,r->val box=r.getJSONArray("icon");SharedIcon(dial.getJSONArray("icons").getString(i).removePrefix("layer-").removeSuffix("-symbolic"),null,Modifier.offset(box.getDouble(0).dp,box.getDouble(1).dp).size(box.getDouble(2).dp,box.getDouble(3).dp))}
        val reset=dial.getJSONArray("reset")
        SharedIcon("reset",null,Modifier.offset(reset.getDouble(0).dp,reset.getDouble(1).dp).size(reset.getDouble(2).dp,reset.getDouble(3).dp))
    }
}
