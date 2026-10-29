package art.capycanvas

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

@OptIn(androidx.compose.runtime.InternalComposeTracingApi::class)
internal class CompositionCounts : AutoCloseable {
    private val counts = mutableMapOf<String, Int>()
    init {
        androidx.compose.runtime.Composer.setTracer(object : androidx.compose.runtime.CompositionTracer {
            override fun isTraceInProgress() = true
            override fun traceEventStart(key: Int, dirty1: Int, dirty2: Int, info: String) {
                val name = info.substringBefore(" (").substringAfterLast('.')
                counts[name] = (counts[name] ?: 0) + 1
            }
            override fun traceEventEnd() {}
        })
    }
    operator fun get(name: String) = counts[name] ?: 0
    fun clear() = counts.clear()
    override fun close() = instrumentation.runOnMainSync { androidx.compose.runtime.Composer.setTracer(null) }
}

class AndroidPanelCompositionTest {
    @get:Rule(order=0) val device = CapyDeviceRule()
    @get:Rule(order=1) val compose = createAndroidComposeRule<MainActivity>()

    private fun node(tag:String)=compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("composition-panel")))

    private fun withPanel(test: (CompositionCounts, List<JSONObject>, (Int) -> Unit, () -> Unit) -> Unit) {
        val host=compose.activity.host
        host.awaitReady(compose=compose)
        repeat(2) { host.drain(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false))) }
        host.awaitReady(compose=compose)
        val state=host.snapshot!!.getJSONObject("state")
        val rows=state.array("layers").objects()
        assertTrue(rows.size>=3)
        val authored=rows.map { it.toString() }
        val palette=Palette(state.getString("theme")=="dark",state.getJSONObject("palette"))
        val panels=mapOf("layers" to obj("id" to "layers"))
        val group=PanelContent(1,"layers",org.json.JSONArray(listOf("layers")),false,0f,null,false,null)
        val dock=compose.runOnIdle { DockInteraction(host) }
        val epoch=state.getJSONObject("document_file").getLong("epoch")
        val thumbnails=rows.flatMap { row -> listOf(false,true).mapNotNull { mask ->
            if(row.getBoolean(if(mask)"has_mask" else "has_thumbnail")) "$epoch:${row.getLong("id")}:$mask" else null
        } }
        var extent by mutableStateOf(320)
        compose.runOnIdle { CompositionCounts() }.use { counts ->
            lateinit var view:androidx.compose.ui.platform.ComposeView
            compose.runOnIdle {
                view=androidx.compose.ui.platform.ComposeView(compose.activity)
                compose.activity.addContentView(view,android.widget.FrameLayout.LayoutParams(-1,-1))
                view.setContent {
                    val density=androidx.compose.ui.platform.LocalDensity.current
                    dock.density=density.density
                    dock.panelMetrics(density.density,density.fontScale,state.getString("theme"))
                    CompositionLocalProvider(LocalCanvasHost provides host,LocalPalette provides palette,LocalDock provides dock) {
                        MaterialTheme {
                            Box(Modifier.fillMaxSize()) {
                                PanelGroup(host,state,group,panels,dock,Modifier.size(320.dp,extent.dp).testTag("composition-panel"))
                            }
                        }
                    }
                }
            }
            try {
                compose.waitUntil(30_000) { host.layerThumbnails.images.keys.containsAll(thumbnails) }
                compose.waitUntil(30_000) { kotlinx.coroutines.runBlocking { host.withNative { Native.surfaceReady(it) && !Native.renderingPending(it) } } }
                compose.waitForIdle()
                compose.runOnIdle { assertTrue(counts["PanelControls"]>0);assertTrue(counts["LayerRow"]>=rows.size);counts.clear() }
                test(counts,rows,{ size -> compose.runOnIdle { extent=size };compose.waitForIdle() }) {
                    compose.runOnIdle {
                        val current=host.snapshot!!.getJSONObject("state").array("layers").objects()
                        assertEquals(authored,current.map { it.toString() })
                        assertTrue(rows.zip(current).all { (before,after) -> before===after })
                    }
                }
            } finally { compose.runOnIdle { view.disposeComposition();(view.parent as android.view.ViewGroup).removeView(view) } }
        }
    }

    @Test fun panelControlsSkipCompositionWhenOnlyParentSizeChanges() = withPanel { counts, _, resize, _ ->
        val before=node("panel-body-layers").fetchSemanticsNode().size
        resize(360)
        assertNotEquals(before,node("panel-body-layers").fetchSemanticsNode().size)
        resize(340);resize(320)
        compose.runOnIdle { assertEquals(0,counts["PanelControls"]) }
    }

    @Test fun layerReorderingRetainsUnchangedRowCompositions() = withPanel { counts, rows, _, verifyRows ->
        val row=node("layer-row-${rows.first().getLong("id")}")
        val travel=with(compose.density) { 24.dp.toPx() }
        val identities=rows.associate { it.getLong("id") to node("layer-row-${it.getLong("id")}").fetchSemanticsNode().id }
        row.performMouseInput { moveTo(center);press();moveTo(center+Offset(-travel,0f)) }
        node("layer-drag-preview").assertExists()
        compose.runOnIdle { counts.clear() }
        try {
            repeat(8) { index -> row.performMouseInput { moveTo(center+Offset(-travel,(index%2)*travel*.4f)) } }
            compose.runOnIdle {
                assertTrue(counts["LayerPanel"]>0)
                assertTrue("Only the drag preview row recomposes",counts["LayerRow"]<=counts["LayerPanel"])
            }
            verifyRows()
            for((id,identity) in identities)assertEquals(identity,node("layer-row-$id").fetchSemanticsNode().id)
        } finally { row.performMouseInput { release() } }
    }
}
