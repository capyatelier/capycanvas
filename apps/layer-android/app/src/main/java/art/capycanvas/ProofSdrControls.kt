package art.capycanvas

import android.graphics.Paint
import androidx.compose.foundation.clickable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
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
@Composable internal fun ProofSdrControls(form:JSONObject,texture:ImageBitmap?,send:(JSONObject)->Unit) {
    val action by rememberUpdatedState(send)
    val ink=LocalPalette.current.text
    var selectedPart by remember {mutableIntStateOf(0)}
    var gestureActive by remember {mutableStateOf(false)}
    var gestureCancelled by remember {mutableStateOf(false)}
    var keyActive by remember {mutableStateOf(false)}
    var live by remember {mutableStateOf(JSONObject(form.getJSONObject("rendition").toString()))}
    var before by remember {mutableStateOf(live)}
    val focus=remember{List(3){FocusRequester()}}
    var focusedPart by remember{mutableIntStateOf(-1)}
    // A fast commit/undo can return to the same recipe before the host observes
    // the intermediate value. Reconcile each publication, including that case.
    LaunchedEffect(form,gestureActive,keyActive){if(!gestureActive&&!keyActive)live=JSONObject(form.getJSONObject("rendition").toString())}
    fun sendRecipe(phase:String,recipe:JSONObject=live){live=recipe;action(obj("type" to "rendition","phase" to phase,"recipe" to recipe))}
    fun begin(){before=JSONObject(live.toString());sendRecipe("down")}
    fun cancel(){if(gestureActive||keyActive){gestureCancelled=true;gestureActive=false;keyActive=false;sendRecipe("cancel",before)}}
    fun control(part:Int,edit:JSONObject)=JSONObject(Native.colorUi(obj("type" to "proof_control","recipe" to live,"part" to part,"edit" to edit).toString()))
    fun atomic(recipe:JSONObject){cancel();begin();sendRecipe("up",recipe)}
    fun reset(part:Int)=atomic(control(part,obj("type" to "reset")))
    fun nudge(part:Int,axis:Int,steps:Double)=atomic(control(part,obj("type" to "step","axis" to axis,"steps" to steps)))
    fun key(e:KeyEvent):Boolean {
        if(e.key==Key.Escape&&e.type==KeyEventType.KeyDown){cancel();return true}
        if(e.key !in listOf(Key.DirectionLeft,Key.DirectionRight,Key.DirectionUp,Key.DirectionDown))return false
        if(e.type==KeyEventType.KeyUp){if(keyActive){keyActive=false;sendRecipe("up")};return true}
        if(e.type!=KeyEventType.KeyDown||gestureActive)return false
        if(!keyActive){begin();keyActive=true}
        val sign=if(e.key==Key.DirectionLeft||e.key==Key.DirectionDown)-1 else 1
        val axis=if(e.key==Key.DirectionLeft||e.key==Key.DirectionRight)0 else 1
        sendRecipe("move",control(selectedPart,obj("type" to "step","axis" to axis,"steps" to sign*(if(e.isShiftPressed)10 else 1))))
        return true
    }
    DisposableEffect(Unit){onDispose{cancel()}}
    BoxWithConstraints(Modifier.fillMaxWidth().aspectRatio(1f)) {
        // Retained drawers can pass a zero-width constraint while unmapping.
        val side=maxWidth.value.coerceIn(128f,2048f)
        val density=LocalDensity.current.density
        fun query(point:Offset?=null,part:Int?=null)=JSONObject(Native.colorUi(obj("type" to "proof_dial","size" to side,"recipe" to live,"point" to point?.let{JSONArray(listOf(it.x/density,it.y/density))},"part" to part).toString()))
        val dial=remember(live.toString(),side){query()}
        fun JSONArray.point()=Offset(getDouble(0).toFloat()*density,getDouble(1).toFloat()*density)
        Canvas(Modifier.fillMaxSize().testTag("sdr-tone-pad").semantics {
            contentDescription="SDR appearance"
            stateDescription=listOf("Contrast","Balance","Brightness","Color intensity").mapIndexed { index,label -> "$label ${round(dial.getJSONArray("percentages").getDouble(index)).toInt()}%" }.joinToString(", ")
            customActions=listOf(CustomAccessibilityAction("Reset SDR appearance"){reset(3);true},
                CustomAccessibilityAction("Increase contrast"){nudge(0,1,1.0);true},CustomAccessibilityAction("Decrease contrast"){nudge(0,1,-1.0);true},
                CustomAccessibilityAction("Favor fine texture"){nudge(0,0,1.0);true},CustomAccessibilityAction("Favor broad structure"){nudge(0,0,-1.0);true},
                CustomAccessibilityAction("Increase brightness"){nudge(1,0,1.0);true},CustomAccessibilityAction("Decrease brightness"){nudge(1,0,-1.0);true},
                CustomAccessibilityAction("Increase color intensity"){nudge(2,0,1.0);true},CustomAccessibilityAction("Decrease color intensity"){nudge(2,0,-1.0);true})
        }.onKeyEvent(::key).onFocusChanged{if(it.isFocused){focusedPart=0;selectedPart=0}else{if(focusedPart==0)focusedPart=-1;cancel()}}.focusRequester(focus[0]).focusable().pointerInput(side) {
            var lastTap=0L;var lastPart=-1;var lastPosition=Offset.Zero
            awaitEachGesture {
                val down=awaitFirstDown(requireUnconsumed=false);val initial=query(down.position)
                if(initial.isNull("hit")||initial.getInt("hit")==3)return@awaitEachGesture
                val part=initial.getInt("hit");focus[part].requestFocus();selectedPart=part;down.consume()
                val double=lastPart==part&&down.uptimeMillis-lastTap in viewConfiguration.doubleTapMinTimeMillis..viewConfiguration.doubleTapTimeoutMillis&&(down.position-lastPosition).getDistance()<=viewConfiguration.touchSlop*2
                if(double){reset(part);lastPart=-1;return@awaitEachGesture}
                begin();gestureActive=true;gestureCancelled=false;var completed=false;var moved=false
                fun update(position:Offset,phase:String){sendRecipe(phase,query(position,part).getJSONObject("recipe"))}
                try {
                    update(down.position,"move")
                    while(true){
                        val c=awaitPointerEvent().changes.firstOrNull{it.id==down.id}?:break
                        if(gestureCancelled){completed=true;break};if(c.isConsumed)break
                        c.consume();moved=moved||(c.position-down.position).getDistance()>viewConfiguration.touchSlop
                        if(!c.pressed){update(c.position,"up");completed=true;if(!moved){lastTap=c.uptimeMillis;lastPart=part;lastPosition=c.position}else lastPart=-1;break}
                        update(c.position,"move")
                    }
                } finally {if(!completed&&!gestureCancelled)sendRecipe("cancel",before);gestureActive=false}
            }
        }) {
            val center=dial.getJSONArray("center").point();val radius=dial.number("radius")*density
            val path=Path().apply{addOval(androidx.compose.ui.geometry.Rect(center-Offset(radius,radius),center+Offset(radius,radius)))}
            clipPath(path){if(texture==null)drawCircle(Color.Gray,radius,center)else drawImage(texture,dstOffset=IntOffset((center.x-radius).toInt(),(center.y-radius).toInt()),dstSize=IntSize((radius*2).toInt(),(radius*2).toInt()))}
            fun marker(point:Offset,r:Float,focused:Boolean){drawCircle(Color.Black.copy(alpha=.65f),r,point,style=Stroke(4*density));drawCircle(Color.White,r,point,style=Stroke(2*density));if(focused)drawCircle(Color.White.copy(alpha=.65f),r+3*density,point,style=Stroke(density))}
            dial.getJSONArray("arcs").objects().forEachIndexed{i,a->val g=a.getJSONObject("geometry");val points=a.getJSONArray("path");val arc=Path().apply{for(j in 0 until points.length()){val p=points.getJSONArray(j).point();if(j==0)moveTo(p.x,p.y)else lineTo(p.x,p.y)}}
                val colors=if(i==0)listOf(Color(.04f,.04f,.04f),Color(.55f,.55f,.55f),Color.White)else listOf(Color(.95f,.95f,.95f),Color(.15f,.55f,.85f))
                drawPath(arc,Brush.linearGradient(colors,points.getJSONArray(0).point(),points.getJSONArray(points.length()-1).point()),style=Stroke(g.number("width")*density,cap=StrokeCap.Round));marker(a.getJSONArray("point").point(),g.number("marker_radius")*density,focusedPart==i+1)
            }
            marker(dial.getJSONArray("marker").point(),dial.number("marker_radius")*density,focusedPart==0)
            val paint=Paint(Paint.ANTI_ALIAS_FLAG).apply{color=ink.toArgb();textSize=dial.number("text_size")*density;textAlign=Paint.Align.CENTER}
            val native=drawContext.canvas.nativeCanvas
            dial.getJSONArray("readouts").objects().forEachIndexed{i,r->val value=round(dial.getJSONArray("percentages").getDouble(i)).toInt();val text=(if(i in 1..2&&value>=0)"+" else "")+value+"%";val curve=r.optJSONArray("curve")
                if(curve==null){val p=r.getJSONArray("text").point();native.drawText(text,p.x,p.y,paint)}else{val radius=curve.getDouble(0).toFloat()*density;val angle=curve.getDouble(1);val reverse=curve.getBoolean(2);var advance=-paint.measureText(text)/2;for(ch in text){val label=ch.toString();val w=paint.measureText(label);val a=angle*PI/180+(if(reverse)-1 else 1)*(advance+w/2)/radius;native.save();native.translate(center.x+radius*cos(a).toFloat(),center.y+radius*sin(a).toFloat());native.rotate((a*180/PI+(if(reverse)-90 else 90)).toFloat());native.drawText(label,0f,0f,paint);native.restore();advance+=w}}
            }
        }
        form.getJSONArray("numbers").objects().forEachIndexed { i,spec ->
            val numeric=spec.getJSONObject("numeric");val part=i+1
            // Native keyboard/AT focus targets for both arcs; pointer geometry
            // continues through the shared circular hit test on the Canvas.
            Box(Modifier.offset((side*.17f).dp,(if(i==0)side*.06f else side*.74f).dp).size((side*.66f).dp,(side*.2f).dp)
                .testTag("sdr-arc-$part").semantics {
                    contentDescription=spec.getString("label")
                    val value=live.number(spec.getString("key"))
                    progressBarRangeInfo=ProgressBarRangeInfo(value,numeric.number("min")..numeric.number("max"))
                    setProgress { next -> atomic(JSONObject(live.toString()).put(spec.getString("key"),next.coerceIn(numeric.number("min"),numeric.number("max"))));true }
                }.onKeyEvent(::key).onFocusChanged { if(it.isFocused){focusedPart=part;selectedPart=part}else{if(focusedPart==part)focusedPart=-1;cancel()} }
                .focusRequester(focus[part]).focusable())
        }
        dial.getJSONArray("readouts").objects().forEachIndexed{i,r->val box=r.getJSONArray("icon");SharedIcon(dial.getJSONArray("icons").getString(i).removePrefix("layer-").removeSuffix("-symbolic"),null,Modifier.offset(box.getDouble(0).dp,box.getDouble(1).dp).size(box.getDouble(2).dp,box.getDouble(3).dp).then(Modifier.graphicsLayer{alpha=.62f}))}
        val reset=dial.getJSONArray("reset")
        SharedIcon("reset","Reset SDR appearance",Modifier.offset(reset.getDouble(0).dp,reset.getDouble(1).dp).size(reset.getDouble(2).dp,reset.getDouble(3).dp).testTag("sdr-appearance-reset").clickable(role=Role.Button){reset(3)})
    }
}
