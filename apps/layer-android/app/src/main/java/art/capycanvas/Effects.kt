package art.capycanvas

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import org.json.JSONArray
import org.json.JSONObject

private fun CanvasHost.effect(action: JSONObject) = dispatch(obj("type" to "effect", "action" to action))

/** The catalog and all parameter semantics come from Rust. These views know
 * control kinds, never the individual filters. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun AdjustmentPanel(host: CanvasHost, state: JSONObject) {
    val colors = LocalPalette.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(2.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        state.array("adjustments").objects().forEach { choice ->
            val cells = choice.getJSONArray("tile_cells")
            Column(Modifier.size((cells.getInt(0)*36).dp, (cells.getInt(1)*36).dp)
                .testTag("adjustment-${choice.getString("id")}").clip(RoundedCornerShape(6.dp))
                .clickable { host.dispatch(choice.getJSONObject("action")) }.padding(4.dp),
                verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                SharedIcon(choice.getString("icon"), null, Modifier.size(24.dp))
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (choice.getBoolean("animated")) SharedIcon("animation", choice.getString("tooltip"), Modifier.size(12.dp).alpha(.55f))
                    Text(choice.getString("label"), maxLines = 2, overflow = TextOverflow.Ellipsis,
                        color = colors.text, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                }
            }
        }
    }
}

@Composable private fun PropertyChoice(label: String, options: List<String>, selected: Int, enabled: Boolean = true, select: (Int) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.fillMaxWidth().heightIn(min = 32.dp).clip(RoundedCornerShape(6.dp))
            .background(LocalPalette.current.input).clickable(enabled = enabled) { open = true }.padding(6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(options.getOrNull(selected) ?: label, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            SharedIcon("chevron-down", label)
        }
        DropdownMenu(open, { open = false }) {
            options.forEachIndexed { index, text -> DropdownMenuItem(text = { Text(text) }, onClick = { open = false; select(index) }) }
        }
    }
}

@Composable internal fun LayerPropertiesPanel(host: CanvasHost, state: JSONObject) {
    val view = state.getJSONObject("layer_properties")
    val controls = view.array("controls").objects()
    val layer = view.optLong("layer")
    val enabled = view.getBoolean("enabled")
    val curves = controls.filter { it.getJSONObject("kind").getString("kind") == "curve" }
    var selectedCurve by remember(layer) { mutableIntStateOf(0) }
    Column(Modifier.fillMaxWidth().testTag("layer-properties").alpha(if(enabled) 1f else .4f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(view.getString("title"), fontWeight = FontWeight.Bold)
        if(curves.isNotEmpty()) {
            PropertyChoice("Channel", curves.map { it.getString("label") }, selectedCurve, enabled) { selectedCurve = it }
            CurveControl(host, layer, curves[selectedCurve.coerceIn(curves.indices)], enabled)
        }
        controls.forEachIndexed { index, control ->
            val section = control.takeUnless { it.isNull("section") }?.getString("section")
            val previousSection = controls.getOrNull(index - 1)?.takeUnless { it.isNull("section") }?.getString("section")
            if(section != previousSection) {
                if(index > 0) HorizontalDivider(Modifier.padding(vertical = 3.dp), color = LocalPalette.current.text.copy(alpha = .15f))
                if(section != null) Text(section, Modifier.padding(start = 6.dp), fontWeight = FontWeight.Bold)
            }
            val key = control.getString("key")
            val label = control.getString("label")
            val kind = control.getJSONObject("kind")
            val value = control.getJSONObject("value").get("value")
            fun change(value: Any) = host.effect(obj("op" to "set", "layer" to layer, "key" to key,
                "value" to obj("kind" to kind.getString("kind"), "value" to value)))
            when(kind.getString("kind")) {
                "number" -> NumericSetting(label, (value as Number).toFloat(), kind.getJSONObject("numeric"), enabled = enabled) { change(it) }
                "toggle" -> Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(label, Modifier.weight(1f)); Switch(value as Boolean, { change(it) }, enabled = enabled)
                }
                "choice" -> { Text(label); PropertyChoice(label, kind.array("options").values().map { it.toString() }, (value as Number).toInt(), enabled) { change(it) } }
                "color" -> PropertyColor(host,label,value as JSONArray,enabled) { change(it) }
                "gradient" -> GradientControl(host,layer,control,enabled)
            }
        }
    }
}

/** Compact color swatch expands to the existing native numeric controls. */
@Composable private fun PropertyColor(host:CanvasHost,label:String,value:JSONArray,enabled:Boolean,onChange:(JSONArray)->Unit) {
    var expanded by remember(label) { mutableStateOf(false) }
    val rgba=(0..3).map { value.getDouble(it).toFloat() }
    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
        Text(label,Modifier.weight(1f))
        Box(Modifier.size(48.dp,28.dp).clip(RoundedCornerShape(6.dp)).background(Color(rgba[0],rgba[1],rgba[2],rgba[3]))
            .clickable(enabled=enabled) {expanded=!expanded})
    }
    if(expanded) listOf("Red","Green","Blue","Alpha").forEachIndexed { i,name ->
        NumericSetting(name,rgba[i],host.catalog.getJSONObject("opacity"),enabled=enabled) { next ->
            val color=rgba.toMutableList();color[i]=next;onChange(JSONArray(color))
        }
    }
}

@Composable private fun GradientControl(host:CanvasHost,layer:Long,control:JSONObject,enabled:Boolean) {
    val colors=LocalPalette.current
    val key=control.getString("key")
    val stops=control.getJSONObject("value").getJSONArray("value").objects()
    var selected by remember(layer,key) {mutableIntStateOf(0)}
    val index=selected.coerceIn(stops.indices)
    val current by rememberUpdatedState(stops)
    fun change(i:Int?,position:Float,color:JSONArray?=null,remove:Boolean=false) = host.effect(obj("op" to "gradient_stop","layer" to layer,"key" to key,"index" to i,"position" to position,"color" to color,"remove" to remove))
    Canvas(Modifier.fillMaxWidth().height(44.dp).testTag("effect-gradient").pointerInput(layer,key,enabled) {
        if(!enabled)return@pointerInput
        awaitEachGesture {
            val down=awaitFirstDown();down.consume();val p=((down.position.x-6.dp.toPx())/(size.width-12.dp.toPx())).coerceIn(0f,1f)
            val found=current.indexOfFirst { kotlin.math.abs(it.number("position")-p)*(size.width-12.dp.toPx())<12.dp.toPx() }
            if(found>=0)selected=found else {selected=current.count {it.number("position")<p};change(null,p)}
        }
    }) {
        val margin=6.dp.toPx();val width=size.width-2*margin
        val ramp=stops.map { s -> val c=s.getJSONArray("color");s.number("position") to Color(c.getDouble(0).toFloat(),c.getDouble(1).toFloat(),c.getDouble(2).toFloat(),c.getDouble(3).toFloat()) }.toTypedArray()
        drawRect(Brush.horizontalGradient(*ramp,startX=margin,endX=size.width-margin),Offset(margin,0f),androidx.compose.ui.geometry.Size(width,32.dp.toPx()))
        stops.forEachIndexed { i,s ->drawCircle(colors.text,(if(index==i)4f else 2.5f).dp.toPx(),Offset(margin+s.number("position")*width,39.dp.toPx())) }
    }
    NumericSetting("Position",stops[index].number("position"),host.catalog.getJSONObject("opacity"),enabled=enabled && index>0 && index<stops.lastIndex) {change(index,it)}
    PropertyColor(host,"Color",stops[index].getJSONArray("color"),enabled) {change(index,stops[index].number("position"),it)}
    Row(horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        TextButton(enabled=enabled && index>0 && index<stops.lastIndex,onClick={selected=(index-1).coerceAtLeast(0);change(index,0f,remove=true)}) {Text("Remove stop")}
        TextButton(enabled=enabled,onClick={host.effect(obj("op" to "reset","layer" to layer,"key" to key))}) {Text("Reset")}
    }
}

@Composable private fun CurveControl(host: CanvasHost, layer: Long, control: JSONObject, enabled: Boolean) {
    val colors = LocalPalette.current
    val current by rememberUpdatedState(control)
    val key = control.getString("key")
    var selected by remember(layer, key) { mutableStateOf<Int?>(null) }
    Canvas(Modifier.fillMaxWidth().aspectRatio(1f).testTag("effect-curve").clip(RoundedCornerShape(6.dp)).background(colors.input)
        .pointerInput(layer, control.getString("key"), enabled) {
            if(!enabled)return@pointerInput
            awaitEachGesture {
                val down = awaitFirstDown(); down.consume()
                fun point(p: Offset) = JSONArray(listOf(p.x / size.width, 1f - p.y / size.height))
                val points = current.getJSONObject("value").getJSONArray("value")
                val index = (0 until points.length()).firstOrNull { i ->
                    val p = points.getJSONArray(i)
                    (Offset(p.getDouble(0).toFloat()*size.width, (1-p.getDouble(1).toFloat())*size.height)-down.position).getDistance() < 16.dp.toPx()
                }
                fun update(p: Offset, index: Int?) = host.effect(obj("op" to "curve_point", "layer" to layer, "key" to current.getString("key"), "index" to index, "point" to point(p), "remove" to false))
                update(down.position, index)
                // A new point is inserted in sorted order by Rust. Its insertion
                // index follows that order; the renderer receives every move.
                val dragging = index ?: (0 until points.length()).count { points.getJSONArray(it).getDouble(0) < down.position.x / size.width }
                selected = dragging
                do {
                    val change = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if(!change.pressed) break
                    if(change.position != change.previousPosition) { change.consume(); update(change.position, dragging) }
                } while(true)
            }
        }) {
        for(i in 1..3) {
            drawLine(colors.text.copy(alpha=.2f),Offset(size.width*i/4,0f),Offset(size.width*i/4,size.height))
            drawLine(colors.text.copy(alpha=.2f),Offset(0f,size.height*i/4),Offset(size.width,size.height*i/4))
        }
        val path = Path()
        control.getJSONArray("plot").values().forEachIndexed { i, raw ->
            val p = raw as JSONArray; val x = p.getDouble(0).toFloat()*size.width; val y = (1-p.getDouble(1).toFloat())*size.height
            if(i==0)path.moveTo(x,y) else path.lineTo(x,y)
        }
        drawPath(path, colors.text, style=Stroke(1.5.dp.toPx()))
        control.getJSONObject("value").getJSONArray("value").values().forEachIndexed { i, raw ->
            val p=raw as JSONArray;drawCircle(colors.text,(if(selected==i)5f else 3.5f).dp.toPx(),Offset(p.getDouble(0).toFloat()*size.width,(1-p.getDouble(1).toFloat())*size.height))
        }
    }
    val count = control.getJSONObject("value").getJSONArray("value").length()
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        TextButton(enabled = enabled && selected != null && selected!! > 0 && selected!! < count - 1,
            onClick = {
                host.effect(obj("op" to "curve_point", "layer" to layer, "key" to key, "index" to selected,
                    "point" to JSONArray(listOf(0, 0)), "remove" to true))
                selected = null
            }) { Text("Remove point") }
        TextButton(enabled = enabled, onClick = {
            selected = null
            host.effect(obj("op" to "reset", "layer" to layer, "key" to key))
        }) { Text("Reset") }
    }
}

@Composable internal fun RendererStatsPanel(host: CanvasHost) {
    var stats by remember { mutableStateOf<JSONObject?>(null) }
    LaunchedEffect(host) { while(isActive) { host.query(obj("type" to "renderer_stats")) { stats = it as? JSONObject }; delay(200) } }
    val colors = LocalPalette.current
    Column(Modifier.fillMaxWidth().testTag("renderer-stats"), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        stats?.let { view ->
            view.array("rows").objects().forEach { row ->
                HoverTip(row.getString("description")) {
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(row.getString("label")); Text(row.getString("value"))
                    }
                }
            }
            Canvas(Modifier.fillMaxWidth().height(46.dp)) {
                val samples = view.array("samples").values().map { (it as Number).toFloat() }
                val budget = view.number("budget_ms"); val max = maxOf(budget,samples.maxOrNull() ?: 0f)*1.1f
                drawLine(colors.secondary,Offset(0f,size.height*(1-budget/max)),Offset(size.width,size.height*(1-budget/max)))
                val path=Path();samples.forEachIndexed { i,v -> val x=i*size.width/119;val y=size.height*(1-v/max);if(i==0)path.moveTo(x,y) else path.lineTo(x,y) }
                drawPath(path,colors.text,style=Stroke(1.dp.toPx()))
            }
        }
    }
}
