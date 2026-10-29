package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.runtime.snapshots.SnapshotStateObserver
import androidx.compose.ui.Modifier
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.json.JSONArray
import org.json.JSONObject

class AndroidWorkspacePresentationTest {
    private val motion=object:MotionDurationScale { override var scaleFactor=1f }
    @get:Rule(order=0) val device=CapyDeviceRule()
    @get:Rule(order=1) val compose=createComposeRule(effectContext=motion)

    @androidx.compose.runtime.Composable private fun MeasurementOriginProbe(dock:DockInteraction,panel:String,
        onOrigin:(PanelMeasurementOrigin?)->Unit) {
        val origin=dock.measurementOrigin(panel)
        androidx.compose.runtime.SideEffect { onOrigin(origin) }
    }

    private fun observedModelReads(test: (SnapshotStateObserver, MutableSet<String>, (String) -> Unit) -> Unit) {
        compose.setContent { }
        compose.runOnIdle {
            val invalidated=linkedSetOf<String>()
            val changed: (String) -> Unit={ invalidated.add(it) }
            val observer=SnapshotStateObserver { callback -> callback() }
            try {
                observer.start()
                test(observer,invalidated,changed)
            } finally { observer.stop();observer.clear() }
        }
    }

    private fun workspaceModel()=ObservedModel(listOf("layout","state","state.workspace","state.workspace.layout")).apply {
        assign(obj("layout" to obj("collapsed" to JSONArray().put(obj("id" to 4)),
            "groups" to JSONArray().put(obj("id" to 5,"height" to 200))),
            "state" to obj("workspace" to obj("layout" to obj("bands" to JSONArray().put(obj("id" to 4,"extent" to 240)),
                "floating" to JSONArray())))))
    }


    @Test fun panelMeasurementOriginsObserveOwnRequestsAndCurrentMetrics() {
        val store=androidx.lifecycle.ViewModelStore()
        lateinit var host:CanvasHost
        lateinit var dock:DockInteraction
        val counts=mutableMapOf<String,Int>()
        val origins=mutableMapOf<String,PanelMeasurementOrigin?>()
        fun request(panel:String, body:String="body", width:Int=240)=obj("panel" to panel,
            "tab" to "tab-$panel","body" to body,"width" to width)
        val model=ObservedModel(listOf("layout")).apply {
            assign(obj("layout" to obj("measurement_requests" to JSONArray(listOf(request("layers"),request("palettes"))))))
        }
        compose.runOnIdle {
            host=CanvasHost(androidx.test.core.app.ApplicationProvider.getApplicationContext())
            store.put("measurements",host)
            dock=DockInteraction(host)
            dock.panelMetrics(1f,1f,"light")
        }
        @Suppress("UNCHECKED_CAST")
        val snapshot=CanvasHost::class.java.getDeclaredField("snapshot\$delegate").let {
            it.isAccessible=true
            it.get(host) as androidx.compose.runtime.MutableState<JSONObject?>
        }
        try {
            compose.waitUntil(30_000) { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") }==true }
            compose.runOnIdle { snapshot.value=model }
            compose.setContent {
                for (panel in listOf("layers","palettes")) {
                    val observed=androidx.compose.runtime.remember(panel) { { origin:PanelMeasurementOrigin? ->
                        counts[panel]=(counts[panel] ?: 0)+1
                        origins[panel]=origin
                    } }
                    MeasurementOriginProbe(dock,panel,observed)
                }
            }
            compose.waitForIdle()
            fun requests(layers:JSONObject=request("layers"),palettes:JSONObject=request("palettes")) {
                compose.runOnIdle { model.getJSONObject("layout").put("measurement_requests",JSONArray(listOf(layers,palettes))) }
                compose.waitForIdle()
            }
            val original=compose.runOnIdle { counts.toMap() to origins.toMap() }
            requests(palettes=request("palettes","different"))
            compose.runOnIdle {
                assertEquals("Layer origin ${original.second["layers"]} -> ${origins["layers"]}",original.first["layers"],counts["layers"])
                assertEquals(original.second["layers"],origins["layers"])
                assertTrue(counts.getValue("palettes")>original.first.getValue("palettes"))
            }
            val beforeOwn=compose.runOnIdle { origins.getValue("layers")!! }
            requests(layers=request("layers","own",260),palettes=request("palettes","different"))
            compose.runOnIdle {
                val next=origins.getValue("layers")!!
                assertNotEquals(beforeOwn.key,next.key)
                assertEquals(260,JSONObject(next.request).getInt("width"))
                assertEquals("own",JSONObject(next.request).getString("body"))
            }
            val beforeMetrics=compose.runOnIdle { counts.toMap() to origins.toMap() }
            compose.runOnIdle { dock.panelMetrics(1f,1f,"light") }
            compose.waitForIdle()
            compose.runOnIdle { assertEquals(beforeMetrics.first,counts) }
            for ((scale,textScale,style) in listOf(Triple(1f,1.5f,"light"),Triple(1f,1.5f,"dark"),Triple(2f,1.5f,"dark"))) {
                val before=compose.runOnIdle { origins.toMap() }
                compose.runOnIdle { dock.panelMetrics(scale,textScale,style) }
                compose.waitForIdle()
                compose.runOnIdle {
                    for (panel in listOf("layers","palettes")) {
                        val next=origins.getValue(panel)!!
                        assertNotEquals(before.getValue(panel)!!.context,next.context)
                        assertEquals(scale,JSONObject(next.context).number("scale"),0f)
                        assertEquals(textScale,JSONObject(next.context).number("text_scale"),0f)
                        assertEquals(style,JSONObject(next.context).getString("style"))
                    }
                }
            }
            requests(palettes=request("palettes","different"))
            compose.runOnIdle { assertEquals("body",JSONObject(origins.getValue("layers")!!.request).getString("body")) }
            compose.runOnIdle {
                val batch=DockInteraction::class.java.getDeclaredMethod("measurementBatch").apply { isAccessible=true }
                fun body(panel:String)=(batch.invoke(dock) as JSONObject).array("reports").objects()
                    .find { it.getJSONObject("request").getString("panel")==panel }?.objectOrNull("body")
                for ((panel,widthFirst) in listOf("layers" to true,"palettes" to false)) {
                    val origin=origins.getValue(panel)!!
                    val content=PanelContentSize(180f,40f,28f)
                    if (widthFirst) dock.measure(panel,origin,actualWidth=240f) else dock.measure(panel,origin,content=content)
                    assertNull(body(panel))
                    if (widthFirst) dock.measure(panel,origin,content=content) else dock.measure(panel,origin,actualWidth=240f)
                    assertEquals(240f,body(panel)!!.number("actual_width"),0f)
                    assertEquals(180f,body(panel)!!.number("content_height"),0f)
                    val changed=origin.copy(key=origin.key+"changed",request=JSONObject(origin.request).put("body","changed").toString())
                    dock.measure(panel,changed,content=PanelContentSize(220f))
                    assertNull(body(panel))
                    dock.measure(panel,changed,actualWidth=260f)
                    assertEquals(260f,body(panel)!!.number("actual_width"),0f)
                    assertEquals(220f,body(panel)!!.number("content_height"),0f)
                }
            }
        } finally { compose.runOnIdle { store.clear() } }
    }

    @Test fun physicalPanelCornerRadiiRemainFixedInRtlWhileLegacyShapesMirror() {
        val physical=SquircleShape(0.dp,18.dp,6.dp,12.dp,physical=true)
        val legacy=SquircleShape(0.dp,18.dp,6.dp,12.dp)
        val mirrored=SquircleShape(18.dp,0.dp,12.dp,6.dp)
        for (scale in listOf(1f,2f)) {
            val size=Size(100f*scale,80f*scale)
            val density=Density(scale)
            fun points(shape:androidx.compose.ui.graphics.Shape,direction:LayoutDirection)=
                (shape.createOutline(size,direction,density) as Outline.Generic).path.asAndroidPath().approximate(0.01f)
            val original=points(legacy,LayoutDirection.Ltr)
            assertArrayEquals(original,points(physical,LayoutDirection.Ltr),0f)
            assertArrayEquals("Shared physical corners remain unchanged in RTL at density $scale",original,points(physical,LayoutDirection.Rtl),0f)
            val legacyRtl=points(legacy,LayoutDirection.Rtl)
            assertFalse("Legacy start/end corners must continue to mirror",original.contentEquals(legacyRtl))
            assertArrayEquals(points(mirrored,LayoutDirection.Ltr),legacyRtl,0f)
        }
    }

    @Test fun vectorCachesReusePreparedPathsAndColumnsWithoutPublishingRetiredGeometry() {
        fun bounds(x:Int,y:Int,width:Int,height:Int)=obj("x" to x,"y" to y,"width" to width,"height" to height)
        fun layout(height:Int=36,secondY:Int=42,open:Boolean=true)=obj(
            "groups" to JSONArray().put(obj("id" to 5,"bounds" to bounds(42,0,100,height)))
                .put(obj("id" to 6,"bounds" to bounds(42,secondY,100,36))),
            "collapsed" to JSONArray().put(obj("id" to 4,"bounds" to bounds(0,0,36,100),"corner_radii" to JSONArray(listOf(0,0,0,0)),
                "open" to if(open) obj("bounds" to bounds(42,0,100,100)) else JSONObject.NULL)),
            "vector_surfaces" to if(open) JSONArray().put(obj("group" to 5,"column" to 4)).put(obj("group" to 6,"column" to 4)) else JSONArray())
        fun payload(height:Int,generation:Long):IntArray {
            val words=mutableListOf<Int>()
            fun receiver(group:Int,version:Long,h:Int) {
                val commands=listOf(intArrayOf(0,-6*1024,0,0,0,0,0),intArrayOf(1,0,0,0,0,0,0),
                    intArrayOf(1,0,h*1024,0,0,0,0),intArrayOf(1,-6*1024,h*1024,0,0,0,0),intArrayOf(3,0,0,0,0,0,0),
                    intArrayOf(0,0,0,0,0,0,0),intArrayOf(1,100*1024,0,0,0,0,0),
                    intArrayOf(1,100*1024,h*1024,0,0,0,0),intArrayOf(1,0,h*1024,0,0,0,0),intArrayOf(3,0,0,0,0,0,0))
                words.addAll(listOf(group,version.toInt(),(version ushr 32).toInt(),commands.size,0))
                commands.forEach { words.addAll(it.asList()) }
            }
            receiver(5,generation,height);receiver(6,22L,36)
            return words.toIntArray()
        }
        fun pathBounds(shape:PanelVectorShape,scale:Float)=
            (shape.createOutline(Size.Zero,LayoutDirection.Ltr,Density(scale)) as Outline.Generic).path.getBounds()
        val cache=PanelVectorCache()
        val aLayout=layout();val aPayload=payload(36,0x18000000bL)
        val a=cache.prepare(aLayout,1f,aPayload,101L)
        assertEquals(setOf(5,6),a.surfaces.keys)
        val aColumn=a.columns.single()
        assertEquals(4,aColumn.id);assertEquals(setOf(5,6),aColumn.groups)
        assertEquals(36f,pathBounds(a.surfaces.getValue(5),1f).height,0f)
        val body=(a.surfaces.getValue(5).body.createOutline(Size.Zero,LayoutDirection.Rtl,Density(1f)) as Outline.Generic).path.getBounds()
        assertEquals(0f,body.left,0f);assertEquals(100f,body.right,0f);assertEquals(36f,body.height,0f)
        val b=cache.prepare(layout(48,54),1f,payload(48,0x28000000bL),101L)
        assertNotSame(a.surfaces.getValue(5),b.surfaces.getValue(5))
        assertSame("Unchanged member path stays prepared",a.surfaces.getValue(6),b.surfaces.getValue(6))
        assertEquals(48f,pathBounds(b.surfaces.getValue(5),1f).height,0f)
        assertNotSame(aColumn,b.columns.single());assertNotSame(aColumn.shape,b.columns.single().shape)
        fun assertRestored(current:PanelVectorGeometry) {
            assertEquals(setOf(5,6),current.surfaces.keys)
            for(id in listOf(5,6)) assertSame(a.surfaces.getValue(id),current.surfaces.getValue(id))
            assertSame(aColumn,current.columns.single());assertSame(aColumn.shape,current.columns.single().shape)
            assertEquals(setOf(5,6),current.columns.single().groups)
            assertEquals(36f,pathBounds(current.surfaces.getValue(5),1f).height,0f)
        }
        assertRestored(cache.prepare(layout(),1f,aPayload.copyOf(),101L))
        val closed=cache.prepare(layout(open=false),1f,intArrayOf(),101L)
        assertTrue(closed.surfaces.isEmpty());assertTrue(closed.columns.isEmpty())
        assertRestored(cache.prepare(layout(),1f,aPayload.copyOf(),101L))
        val moved=cache.prepare(layout(secondY=54),1f,aPayload,101L)
        for(id in listOf(5,6)) assertSame(a.surfaces.getValue(id),moved.surfaces.getValue(id))
        assertNotSame("Member placement changes the merged path despite unchanged path identities",aColumn,moved.columns.single())
        for(key in listOf("x","y","width","height")) assertEquals(aColumn.bounds.number(key),moved.columns.single().bounds.number(key),0f)
        assertRestored(cache.prepare(layout(),1f,aPayload.copyOf(),101L))
        val newOwner=cache.prepare(aLayout,1f,aPayload,202L)
        for(id in listOf(5,6)) assertNotSame(a.surfaces.getValue(id),newOwner.surfaces.getValue(id))
        assertNotSame(aColumn,newOwner.columns.single());assertNotSame(aColumn.shape,newOwner.columns.single().shape)
        val scaled=cache.prepare(aLayout,2f,aPayload,101L)
        for(id in listOf(5,6)) assertNotSame(a.surfaces.getValue(id),scaled.surfaces.getValue(id))
        assertNotSame(aColumn,scaled.columns.single());assertNotSame(aColumn.shape,scaled.columns.single().shape)
        assertEquals(72f,pathBounds(scaled.surfaces.getValue(5),2f).height,0f)
        assertEquals(200f,pathBounds(scaled.surfaces.getValue(5),2f).right,0f)
        assertEquals(-12f,pathBounds(scaled.surfaces.getValue(5),2f).left,0f)
        val scaledBody=(scaled.surfaces.getValue(5).body.createOutline(Size.Zero,LayoutDirection.Rtl,Density(2f)) as Outline.Generic).path.getBounds()
        assertEquals(0f,scaledBody.left,0f);assertEquals(200f,scaledBody.right,0f);assertEquals(72f,scaledBody.height,0f)
        assertRestored(cache.prepare(layout(),1f,aPayload.copyOf(),101L))
    }

    @Test fun layoutPublicationInvalidatesOnlyChangedLayoutReaders() = observedModelReads { observer,invalidated,changed ->
        val root=workspaceModel()
        val layout=root.getJSONObject("layout")
        val collapsed=layout.getJSONArray("collapsed")
        val groups=layout.getJSONArray("groups")
        Snapshot.sendApplyNotifications()
        observer.observeReads("outer",changed) { root.getJSONObject("layout") }
        observer.observeReads("collapsed",changed) { layout.getJSONArray("collapsed") }
        observer.observeReads("groups",changed) { layout.getJSONArray("groups") }
        val next=shareModel(layout,JSONObject(layout.toString()).put("groups",JSONArray().put(obj("id" to 5,"height" to 212)))) as JSONObject
        assertSame(collapsed,next.getJSONArray("collapsed"))
        Snapshot.withMutableSnapshot { root.put("layout",next) }
        Snapshot.sendApplyNotifications()
        assertSame(layout,root.getJSONObject("layout"))
        assertSame(collapsed,layout.getJSONArray("collapsed"))
        assertNotSame(groups,layout.getJSONArray("groups"))
        assertEquals(212,layout.getJSONArray("groups").getJSONObject(0).getInt("height"))
        assertEquals(setOf("groups"),invalidated)
        invalidated.clear()
        Snapshot.withMutableSnapshot { root.put("layout",next) }
        Snapshot.sendApplyNotifications()
        assertTrue("Repeating identical layout must not invalidate any scope",invalidated.isEmpty())
    }

    @Test fun nestedWorkspaceLayoutPublicationRetainsReaderScopes() = observedModelReads { observer,invalidated,changed ->
        val root=workspaceModel()
        val state=root.getJSONObject("state")
        val workspace=state.getJSONObject("workspace")
        val layout=workspace.getJSONObject("layout")
        val floating=layout.getJSONArray("floating")
        Snapshot.sendApplyNotifications()
        observer.observeReads("state",changed) { root.getJSONObject("state") }
        observer.observeReads("workspace",changed) { state.getJSONObject("workspace") }
        observer.observeReads("layout",changed) { workspace.getJSONObject("layout") }
        observer.observeReads("bands",changed) { layout.getJSONArray("bands") }
        observer.observeReads("floating",changed) { layout.getJSONArray("floating") }
        val next=JSONObject(workspace.toString()).apply {
            getJSONObject("layout").put("bands",JSONArray().put(obj("id" to 4,"extent" to 252)))
        }
        val retained=shareModel(workspace,next) as JSONObject
        Snapshot.withMutableSnapshot { state.put("workspace",retained) }
        Snapshot.sendApplyNotifications()
        assertSame(state,root.getJSONObject("state"))
        assertSame(workspace,state.getJSONObject("workspace"))
        assertSame(layout,workspace.getJSONObject("layout"))
        assertSame(floating,layout.getJSONArray("floating"))
        assertEquals(252,layout.getJSONArray("bands").getJSONObject(0).getInt("extent"))
        assertEquals(setOf("bands"),invalidated)
    }

    @Test fun removedNestedKeysInvalidateTheirReadersAndCanBeReadded() = observedModelReads { observer,invalidated,changed ->
        val root=workspaceModel()
        val layout=root.getJSONObject("layout")
        layout.put("notice",obj("id" to 7)).put("nullable",JSONObject.NULL)
        val groups=layout.getJSONArray("groups")
        Snapshot.sendApplyNotifications()
        observer.observeReads("outer",changed) { root.getJSONObject("layout") }
        observer.observeReads("groups",changed) { layout.getJSONArray("groups") }
        observer.observeReads("notice",changed) { layout.has("notice") }
        observer.observeReads("nullable",changed) { layout.has("nullable") }
        observer.observeReads("keys",changed) { layout.keys().asSequence().toList() }
        val next=JSONObject(layout.toString()).apply { remove("notice");remove("nullable") }
        Snapshot.withMutableSnapshot { root.put("layout",shareModel(layout,next)) }
        Snapshot.sendApplyNotifications()
        assertSame(layout,root.getJSONObject("layout"))
        assertSame(groups,layout.getJSONArray("groups"))
        assertFalse(layout.has("notice"))
        assertFalse("A present JSON-null key must be removed, too",layout.has("nullable"))
        assertEquals(setOf("notice","nullable","keys"),invalidated)
        invalidated.clear()
        observer.observeReads("notice",changed) { layout.has("notice") }
        observer.observeReads("nullable",changed) { layout.has("nullable") }
        observer.observeReads("keys",changed) { layout.keys().asSequence().toList() }
        val readded=JSONObject(layout.toString()).put("notice",obj("id" to 8)).put("nullable",JSONObject.NULL)
        Snapshot.withMutableSnapshot { root.put("layout",shareModel(layout,readded)) }
        Snapshot.sendApplyNotifications()
        assertSame(layout,root.getJSONObject("layout"))
        assertTrue(layout.has("notice"));assertTrue(layout.has("nullable"))
        assertEquals(8,layout.getJSONObject("notice").getInt("id"))
        assertEquals(setOf("notice","nullable","keys"),invalidated)
    }

    @Test fun zenFadeRetainsContentButImmediatelyReleasesHitTargets() {
        var visible by mutableStateOf(true)
        var hiddenClicks=0
        var canvasClicks=0
        var disposed=0
        lateinit var opacity:androidx.compose.runtime.State<Float>
        compose.mainClock.autoAdvance=false
        compose.setContent {
            Box(Modifier.size(120.dp).testTag("backdrop").background(Color.White).clickable { canvasClicks++ }) {
                opacity=rememberZenOpacity(visible)
                ZenChrome(visible,Modifier.fillMaxSize().testTag("retained-chrome"),opacity) {
                    DisposableEffect(Unit) { onDispose { disposed++ } }
                    Box(Modifier.fillMaxSize().background(Color.Red).clickable { hiddenClicks++ })
                }
            }
        }
        compose.mainClock.advanceTimeByFrame()
        assertEquals("Initial visible chrome must be drawn before hiding",Color.Red,compose.onNodeWithTag("backdrop").captureToImage().toPixelMap()[20,20])
        compose.runOnIdle { visible=false }
        compose.mainClock.advanceTimeByFrame()
        compose.onNodeWithTag("retained-chrome").assertDoesNotExist()
        compose.mainClock.advanceTimeBy(64)
        val during=compose.onNodeWithTag("backdrop").captureToImage().toPixelMap()[20,20]
        assertTrue("Outgoing retained layer remains visible during fade: pixel=$during opacity=${opacity.value} scale=${motion.scaleFactor}",during.green>0f && during.green<.95f)
        compose.onNodeWithTag("backdrop").performTouchInput { click() }
        compose.runOnIdle { assertEquals(0,hiddenClicks);assertEquals(1,canvasClicks);assertEquals(0,disposed) }
        compose.mainClock.advanceTimeBy(200)
        val after=compose.onNodeWithTag("backdrop").captureToImage().toPixelMap()[20,20]
        assertEquals(Color.White,after)
        compose.runOnIdle { visible=true }
        compose.mainClock.advanceTimeByFrame()
        compose.mainClock.advanceTimeBy(200)
        compose.onNodeWithTag("backdrop").performTouchInput { click() }
        compose.runOnIdle { assertEquals(1,hiddenClicks);assertEquals(0,disposed) }
    }

    @Test fun disabledManualPlacementDoesNotAwaitAnimationFrames() {
        val frames=object:androidx.compose.runtime.MonotonicFrameClock {
            override suspend fun <R> withFrameNanos(onFrame:(Long)->R):R = error("Disabled placement requested an animation frame")
        }
        motion.scaleFactor=0f
        val positions=mutableListOf<Float>()
        kotlinx.coroutines.runBlocking(motion+frames) { workspaceFrames(200) { positions.add(it) } }
        assertEquals(listOf(1f),positions)
    }

    @Test fun disabledMotionReachesTheFinalZenFrame() {
        motion.scaleFactor=0f
        var visible by mutableStateOf(true)
        compose.setContent {
            Box(Modifier.size(120.dp).testTag("backdrop").background(Color.White)) {
                ZenChrome(visible,Modifier.fillMaxSize()) { Box(Modifier.fillMaxSize().background(Color.Red)) }
            }
        }
        compose.runOnIdle { visible=false }
        compose.waitForIdle()
        assertEquals(Color.White,compose.onNodeWithTag("backdrop").captureToImage().toPixelMap()[20,20])
    }
}
