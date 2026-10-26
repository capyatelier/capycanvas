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
import androidx.compose.ui.Modifier
import androidx.compose.ui.MotionDurationScale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.unit.dp
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class AndroidWorkspacePresentationTest {
    private val motion=object:MotionDurationScale { override var scaleFactor=1f }
    @get:Rule val compose=createComposeRule(effectContext=motion)

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
