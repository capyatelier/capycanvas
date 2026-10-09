package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Hold the real renderer before device creation, with the real Compose UI running. */
class AndroidFirstUiTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    @Test fun grayCanvasAndControlsDrawWithoutAnyCanvasShaders() {
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        CanvasHost.beforeGpuAttachForTest = {
            entered.countDown()
            check(resume.await(30, TimeUnit.SECONDS)) { "Test did not release GPU initialization" }
        }
        var scenario: ActivityScenario<MainActivity>? = null
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            lateinit var host: CanvasHost
            scenario.onActivity { host = it.host }
            // Advance Compose's test clock while the first surface is mounted.
            compose.waitUntil(20_000) { entered.count == 0L }
            compose.waitUntil(20_000) { host.snapshot?.getJSONObject("layout")?.array("groups")?.length()?.let { it > 0 } == true }
            compose.onNodeWithTag("canvas-placeholder").assertIsDisplayed()
            compose.runOnIdle {
                assertFalse(host.snapshot!!.getBoolean("gpu_ready"))
                assertFalse(host.surfaceReady)
                assertNull(host.failure)
            }
            // A saved floating panel can cover the screen center. Choose an
            // uncovered point in the actual native placeholder, before opening a menu.
            val placeholder = compose.onNodeWithTag("canvas-placeholder").fetchSemanticsNode().boundsInRoot
            val panels = host.snapshot!!.getJSONObject("layout").array("groups").objects().flatMap { group ->
                compose.onAllNodesWithTag("group-${group.getInt("id")}").fetchSemanticsNodes().map { it.boundsInRoot.inflate(24f) }
            }
            val fractions = listOf(.5f, .25f, .75f, .125f, .875f)
            val point = fractions.flatMap { y -> fractions.map { x ->
                Offset(placeholder.left + placeholder.width*x, placeholder.top + placeholder.height*y)
            } }.first { candidate -> panels.none { it.contains(candidate) } }
            val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
            try {
                val state = host.snapshot!!.getJSONObject("state")
                val expected = Palette(state.getString("theme") != "light", state.getJSONObject("palette")).surround.toArgb()
                // Read the compositor's screenshot, including SurfaceView; a
                // Compose-only screenshot would miss the original black hole.
                assertEquals("Canvas shows the UI gray before Vulkan exists", expected, bitmap.getPixel(point.x.toInt(), point.y.toInt()))
                instrumentation.targetContext.getExternalFilesDir(null)!!.resolve("startup-without-gpu.png").outputStream().use {
                    bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
                }
            } finally { bitmap.recycle() }
            compose.onNodeWithText("View").assertIsDisplayed().performClick()
            // Menu expansion is native UI and must respond even while its canvas
            // worker is held. Native commands remain queued in their normal order.
            compose.onNodeWithText("Fit canvas").assertIsDisplayed()
            android.util.Log.i("CapyStartupTest", "gray_ui_without_gpu boot_ns=${SystemClock.elapsedRealtimeNanos()}")
            resume.countDown()
            compose.waitUntil(60_000) { host.surfaceReady || host.failure != null }
            compose.runOnIdle { assertNull(host.failure) }
            compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            // The retained GPU must not make a replacement SurfaceView reveal
            // an empty buffer on Activity recreation.
            scenario.recreate()
            compose.waitUntil(30_000) { host.surfaceReady || host.failure != null }
            compose.runOnIdle { assertNull(host.failure) }
            compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
        } finally {
            resume.countDown()
            CanvasHost.beforeGpuAttachForTest = null
            scenario?.close()
        }
    }

    @Test fun startupFailureRetainsLongCauseAndRestartStaysReachable() {
        for (theme in listOf("light", "dark")) {
            val cause = IllegalStateException("Vulkan attach diagnostic\n" + (1..60).joinToString("\n") {
                "Driver detail $it: preserved native cause for renderer initialization"
            })
            val diagnostic = cause.toString()
            CanvasHost.beforeGpuAttachForTest = { throw cause }
            try {
                ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                    val activity = scenario.activity()
                    val host = activity.host
                    host.awaitMain("original startup failure", 20_000, { "failure=${host.failure}" }, compose) {
                        host.failure != null
                    }
                    compose.runOnIdle {
                        assertEquals(diagnostic, host.failure)
                        host.dispatch(obj("type" to "set_theme", "theme" to theme))
                    }
                    host.awaitMain("diagnostic theme", 10_000, { "state=${host.snapshot?.optJSONObject("state")}" }, compose) {
                        host.snapshot?.optJSONObject("state")?.optString("theme") == theme
                    }
                    compose.onNodeWithText(host.bootstrap!!.getString("canvas_init_failed")).assertIsDisplayed()
                    val detail = compose.onNodeWithText(diagnostic, useUnmergedTree = true)
                    detail.assertIsDisplayed().assertTextEquals(diagnostic)
                    val viewport = detail.fetchSemanticsNode().boundsInRoot
                    assertTrue("Long diagnostic keeps a bounded viewport", viewport.height <= 180f * activity.resources.displayMetrics.density + 1f)
                    val scroller = compose.onNode(hasScrollAction() and (hasText(diagnostic) or hasAnyDescendant(hasText(diagnostic))), useUnmergedTree = true)
                    assertTrue("The entire cause remains available by scrolling",
                        scroller.fetchSemanticsNode().config[SemanticsProperties.VerticalScrollAxisRange].maxValue() > 0f)
                    scroller.performTouchInput { swipeUp() }
                    val restart = compose.onNodeWithText(host.bootstrap!!.getString("restart_canvas"))
                    restart.assertIsDisplayed().assertHasClickAction()
                    val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
                    val button = restart.fetchSemanticsNode().boundsInRoot
                    assertTrue("Restart stays within the editor", root.contains(button.topLeft) && root.contains(button.bottomRight))
                    val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
                    CanvasHost.beforeGpuAttachForTest = null
                    restart.performClick()
                    host.awaitMain("canvas restart", 60_000, { "failure=${host.failure}; ready=${host.surfaceReady}" }, compose) {
                        host.failure == null && host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true
                    }
                    compose.runOnIdle {
                        assertEquals(epoch, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch"))
                    }
                    detail.assertDoesNotExist()
                    compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
                }
            } finally { CanvasHost.beforeGpuAttachForTest = null }
        }
    }
}
