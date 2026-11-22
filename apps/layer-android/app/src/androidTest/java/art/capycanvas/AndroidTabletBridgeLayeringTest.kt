package art.capycanvas

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File

class AndroidTabletBridgeLayeringTest {
    @get:Rule(order=0) val device=CapyDeviceRule()
    @get:Rule(order=1) val compose=createAndroidComposeRule<MainActivity>()

    @Test fun tabletHeaderDrawerGlassMatchesOverPaperAtLowAndHighTransparency() {
        val host = compose.activity.host
        host.awaitReady(compose = compose)
        host.awaitMain("launch revealed", 30_000, { "" }, compose) { findTag("launch-splash") == null }
        assumeTrue("Requires the tablet workspace", host.snapshot!!.getJSONObject("layout").objectOrNull("phone") == null)
        host.awaitMain("Drawing accepts input", 30_000, { "" }, compose) { !host.documentInputBlocked }
        host.useFlatPaper(compose)
        fun action(value: JSONObject) { host.drain(value); compose.waitForIdle() }
        fun header(value: JSONObject) = action(obj("type" to "customize", "action" to obj("type" to "header", "action" to value)))
        header(obj("type" to "edit", "editing" to true))
        val headerId = host.snapshot!!.getJSONObject("header").getJSONObject("model").getInt("next_id")
        header(obj("type" to "add", "zone" to "left", "before" to null,
            "item" to obj("kind" to "tool", "control" to obj("kind" to "color"))))
        header(obj("type" to "edit", "editing" to false))
        val dock = checkNotNull(host.workspaceInteraction)
        val anchor = obj("kind" to "header", "id" to headerId)
        val failures = mutableListOf<String>()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((level, value) in listOf("low" to 1, "high" to 3)) {
                action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "transparency", "value" to value)))
                assertEquals(level, host.snapshot!!.getJSONObject("state").getJSONObject("settings").getString("transparency"))
                host.coverChromeWithPaper(compose)
                val snapshot = host.snapshot!!
                val presentation = snapshot.getJSONObject("header_presentation")
                val placements = presentation.array("items").objects()
                assertTrue("Header tool has a published placement", placements.any { it.getInt("id") == headerId })
                val geometry = checkNotNull(kotlinx.coroutines.runBlocking { host.awaitQuery(obj("type" to "header", "request" to obj(
                    "op" to "geometry", "model" to snapshot.getJSONObject("header").getJSONObject("model"), "editing" to false,
                    "documents" to host.drawingTabs.rows.size, "width" to snapshot.getJSONObject("layout").array("viewport").getDouble(0),
                    "insets" to org.json.JSONArray(listOf(0, 0)), "metrics" to org.json.JSONArray(placements.map {
                        obj("id" to it.getInt("id"), "width" to it.getJSONObject("bounds").number("width"))
                    })))) })
                assertTrue("Header tool joins a glass bar", geometry.array("bars").objects().any { bar ->
                    bar.array("items").values().any { (it as Number).toInt() == headerId }
                })
                val at = compose.runOnIdle { checkNotNull(findTag("header-control-$headerId")).second.boundsInRoot.center }
                for (event in listOf(android.view.MotionEvent.ACTION_DOWN, android.view.MotionEvent.ACTION_UP))
                    instrumentation.uiAutomation.injectInputEvent(motion(android.view.MotionEvent.TOOL_TYPE_FINGER, event, at, android.os.SystemClock.uptimeMillis()), true)
                host.drain()
                compose.mainClock.advanceTimeBy(400)
                host.awaitMain("Header drawer glass presented", 30_000, { "${dock.drawer}" }, compose) {
                    dock.drawer?.objectOrNull("connection") != null && dock.anchorSource(anchor)?.presented == true
                }
                compose.waitForIdle()
                host.captureDrawerGlass("tablet-header-$theme-$level", compose.runOnIdle { checkNotNull(dock.drawer).copy() }, failures)
                action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
                compose.mainClock.advanceTimeBy(400)
                host.awaitMain("Header drawer closed", 30_000, { "" }, compose) { dock.drawer == null }
            }
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test fun tabletDrawerBridgesAndBodiesStayAbovePanelWindows() {
        val host=compose.activity.host
        host.awaitReady(compose=compose)
        host.awaitMain("launch revealed",30_000,{ "" },compose) { findTag("launch-splash")==null }
        assumeTrue("Requires the tablet workspace",host.snapshot!!.getJSONObject("layout").objectOrNull("phone")==null)
        compose.runOnUiThread { host.workspaceInput(obj("type" to "switch","id" to "builtin:workspace:photographer")) }
        host.awaitMain("Photo workspace",30_000,{ "${host.workspaceManager}" },compose) {
            host.workspaceManager?.optString("id")=="builtin:workspace:photographer" && host.workspaceManager?.optBoolean("busy")==false
        }
        fun action(value:JSONObject) { host.drain(value);compose.waitForIdle() }
        fun header(value:JSONObject)=action(obj("type" to "customize","action" to obj("type" to "header","action" to value)))
        header(obj("type" to "edit","editing" to true))
        val headerId=host.snapshot!!.getJSONObject("header").getJSONObject("model").getInt("next_id")
        header(obj("type" to "add","zone" to "left","before" to null,"item" to obj("kind" to "tool","control" to obj("kind" to "color"))))
        header(obj("type" to "edit","editing" to false))
        val dock=checkNotNull(host.workspaceInteraction)
        val output=File(instrumentation.targetContext.getExternalFilesDir(null),"validation/tablet-bridges").apply { mkdirs() }
        fun bounds(tag:String):Rect=compose.runOnIdle { checkNotNull(findTag(tag)) { "Missing $tag" }.second.boundsInRoot }
        fun tap(tag:String) {
            val at=bounds(tag).center
            instrumentation.uiAutomation.injectInputEvent(motion(android.view.MotionEvent.TOOL_TYPE_FINGER,android.view.MotionEvent.ACTION_DOWN,at,android.os.SystemClock.uptimeMillis()),true)
            instrumentation.uiAutomation.injectInputEvent(motion(android.view.MotionEvent.TOOL_TYPE_FINGER,android.view.MotionEvent.ACTION_UP,at,android.os.SystemClock.uptimeMillis()),true)
            host.drain()
        }
        val failures=mutableListOf<String>()
        fun shot(name:String,tag:String) {
            lateinit var state:JSONObject
            val face=bounds(tag)
            compose.runOnIdle {
                state=obj("frame" to checkNotNull(dock.drawer).copy(),"origin" to org.json.JSONArray(listOf(dock.origin.x,dock.origin.y)),
                    "face" to face.let { obj("x" to it.left,"y" to it.top,"width" to it.width,"height" to it.height) },
                    "snapshot" to host.snapshot!!.copy())
            }
            File(output,"$name.json").writeText(state.toString(2))
            var inspected=false
            screenshot("validation/tablet-bridges/$name.png") { image ->
                inspected=true
                val scale=dock.density
                val origin=dock.origin
                val placement=state.getJSONObject("frame").getJSONObject("placement")
                fun screen(value:JSONObject):Rect=value.rect().let { Rect(it.topLeft*scale+origin,it.bottomRight*scale+origin) }
                val body=screen(placement.getJSONObject("bounds"))
                val anchor=screen(placement.getJSONObject("anchor"))
                val bridge=screen(state.getJSONObject("frame").getJSONObject("connection").getJSONObject("bounds"))
                val inset=4f*scale
                val expected=android.graphics.Color.parseColor(state.getJSONObject("snapshot").getJSONObject("state").getJSONObject("palette").getString("panel"))
                fun pixel(at:androidx.compose.ui.geometry.Offset)=image.getPixel(kotlin.math.round(at.x).toInt(),kotlin.math.round(at.y).toInt())
                val samples=org.json.JSONArray()
                fun equal(label:String,a:androidx.compose.ui.geometry.Offset,b:androidx.compose.ui.geometry.Offset?=null,tolerance:Int=3) {
                    val first=pixel(a);val second=b?.let(::pixel) ?: expected
                    val delta=listOf(16,8,0).maxOf { shift -> kotlin.math.abs(((first shr shift) and 255)-((second shr shift) and 255)) }
                    samples.put(obj("label" to label,"first" to org.json.JSONArray(listOf(a.x,a.y)),"second" to b?.let { org.json.JSONArray(listOf(it.x,it.y)) },"first_argb" to first,"second_argb" to second,"delta" to delta))
                    if(delta>tolerance)failures+="$name $label $a=${Integer.toHexString(first)} $b=${Integer.toHexString(second)} delta=$delta"
                }
                if(name.endsWith("rest") || name.contains("header") || name.contains("more")) {
                    val tangent=anchor.left+anchor.width*.25f
                    val facePoint=androidx.compose.ui.geometry.Offset(tangent,anchor.bottom-2f*inset)
                    val bodyPoint=androidx.compose.ui.geometry.Offset(body.left+inset,body.center.y)
                    if(name.endsWith("rest")) {
                        val connection=state.getJSONObject("frame").getJSONObject("connection")
                        val t=connection.array("transform")
                        val length=connection.number("length")*scale
                        val depth=connection.number("depth")*scale
                        val chrome=state.getJSONObject("snapshot").getJSONObject("layout").array("groups").objects().map { screen(it.getJSONObject("bounds")) }
                        fun point(u:Float,v:Float)=bridge.topLeft+androidx.compose.ui.geometry.Offset(
                            t.getDouble(0).toFloat()*u+t.getDouble(2).toFloat()*v+t.getDouble(4).toFloat()*scale,
                            t.getDouble(1).toFloat()*u+t.getDouble(3).toFloat()*v+t.getDouble(5).toFloat()*scale)
                        for(fraction in listOf(.25f,.5f,.75f)) {
                            equal("face/bridge joint $fraction",point(length*fraction,-2f),point(length*fraction,2f),2)
                            val before=point(length*fraction,depth-2f)
                            val after=point(length*fraction,depth+2f)
                            if(chrome.all { it.contains(before)==it.contains(after) })equal("bridge/body joint $fraction",before,after,2)
                        }
                    } else {
                        equal("face/panel",facePoint)
                        equal("body/panel",bodyPoint)
                        for(y in kotlin.math.ceil(bridge.top+1f).toInt() until kotlin.math.floor(bridge.bottom-1f).toInt())
                            equal("bridge/panel",androidx.compose.ui.geometry.Offset(tangent,y.toFloat()))
                    }
                    if(name.contains("more") && !name.endsWith("rest")) {
                        val panel=state.getJSONObject("snapshot").getJSONObject("layout").array("groups").objects().first { it.getString("active")=="histogram" }
                        val overlap=body.intersect(screen(panel.getJSONObject("bounds")))
                        assertTrue("$name overlaps histogram",overlap.width>2f*inset && overlap.height>anchor.height)
                        val y=overlap.top+anchor.height*2f/3f
                        equal("body over histogram",androidx.compose.ui.geometry.Offset(body.right-inset,y),androidx.compose.ui.geometry.Offset(body.left+inset,y))
                    }
                }
                state.put("samples",samples)
                File(output,"$name.json").writeText(state.toString(2))
            }
            assertTrue("$name screenshot captured",inspected)
        }
        fun journey(name:String,tag:String) {
            tap(tag)
            compose.mainClock.advanceTimeBy(400)
            host.awaitMain("$name presented",30_000,{ "${dock.drawer}" },compose) { dock.drawer?.objectOrNull("connection")!=null && host.glassConnectionsForTest.isNotEmpty() }
            compose.waitForIdle()
            val rest=compose.runOnIdle { checkNotNull(dock.drawer).getJSONObject("placement").getJSONObject("bounds").toString() }
            shot("$name-rest",tag)
            compose.mainClock.autoAdvance=false
            host.drain(obj("type" to "customize","action" to obj("type" to "close_expanded")))
            host.awaitMain("$name shared close published",10_000,{ "" }) { host.snapshot!!.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer")==null }
            repeat(3) { compose.mainClock.advanceTimeByFrame();instrumentation.waitForIdleSync() }
            host.awaitMain("$name glass retired",10_000,{ "" }) { host.glassConnectionsForTest.isEmpty() }
            val fullHeight=JSONObject(rest).number("height")
            val fullWidth=JSONObject(rest).number("width")
            for(step in 1..12) {
                compose.mainClock.advanceTimeByFrame()
                instrumentation.waitForIdleSync()
                val moving=compose.runOnIdle { dock.drawer?.getJSONObject("placement")?.getJSONObject("bounds")?.copy() }
                if(moving!=null && (moving.number("height")/fullHeight in .25f.. .8f || moving.number("width")/fullWidth in .25f.. .8f))break
            }
            assertNotNull("$name moving connection",dock.drawer?.objectOrNull("connection"))
            assertNotEquals("$name mid-slide bounds",rest,dock.drawer!!.getJSONObject("placement").getJSONObject("bounds").toString())
            shot("$name-mid",tag)
            compose.mainClock.autoAdvance=true
            compose.mainClock.advanceTimeBy(400)
            host.awaitMain("$name closed",30_000,{ "" },compose) { dock.drawer==null }
        }
        try {
            for(theme in listOf("light","dark")) {
                action(obj("type" to "set_theme","theme" to theme))
                journey("$theme-header","header-control-$headerId")
                val panel=host.snapshot!!.array("panels").objects().first { it.getString("id")=="toolbar" }
                val tile=panel.array("tiles").objects().first { it.getJSONObject("control").getString("kind")=="color" }
                journey("$theme-panel","tile-toolbar-${tile.getInt("id")}")
                action(obj("type" to "invoke","command" to "brush"))
                val options=host.snapshot!!.array("panels").objects().flatMap { p -> p.array("tiles").objects().map { p.getString("id") to it } }
                    .first { it.second.getJSONObject("control").getString("kind")=="tool_options" }
                journey("$theme-more","toolbar-more-${options.second.getInt("id")}")
            }
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
        } finally { compose.mainClock.autoAdvance=true }
    }
}
