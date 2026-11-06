package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.provider.Settings
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.platform.WindowRecomposerFactory
import androidx.compose.ui.platform.WindowRecomposerPolicy
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Hold the real renderer before device creation, with the real Compose UI running. */
class AndroidFirstUiTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    @OptIn(androidx.compose.ui.InternalComposeUiApi::class)
    @Test fun removedSystemAnimationsStillReachTheFirstCanvas() {
        val key = Settings.Global.ANIMATOR_DURATION_SCALE
        val previous = Settings.Global.getString(instrumentation.targetContext.contentResolver, key)
        check(previous == null || previous.toFloatOrNull() != null)
        try {
            shell("settings put global $key 0")
            WindowRecomposerPolicy.withFactory(WindowRecomposerFactory.LifecycleAware) {
                ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                    val host = scenario.activity().host
                    host.awaitMain("first canvas with system animations removed", 60_000) { host.surfaceReady }
                    instrumentation.runOnMainSync {
                        assertNull(host.failure)
                        assertTrue(host.surfaceReady)
                    }
                }
            }
        } finally {
            shell(if (previous == null) "settings delete global $key" else "settings put global $key $previous")
        }
    }

    @Test fun grayCanvasAndControlsDrawWithoutAnyCanvasShaders() {
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        CanvasHost.beforeGpuAttachForTest = {
            entered.countDown()
            check(resume.await(30, TimeUnit.SECONDS)) { "Test did not release GPU initialization" }
        }
        var scenario: ActivityScenario<MainActivity>? = null
        try {
            val theme = InstrumentationRegistry.getArguments().getString("theme") ?: "dark"
            instrumentation.targetContext.getSharedPreferences(CanvasHost.preferencesName, 0).edit()
                .putString("settings", obj("version" to 1, "theme" to theme,"language" to "en").toString()).commit()
            scenario = ActivityScenario.launch(MainActivity::class.java)
            lateinit var host: CanvasHost
            scenario.onActivity { host = it.host }
            // Advance Compose's test clock while the first surface is mounted.
            compose.waitUntil(20_000) { entered.count == 0L }
            compose.waitUntil(20_000) { host.snapshot?.getJSONObject("layout")?.array("groups")?.length()?.let { it > 0 } == true }
            compose.onNodeWithTag("canvas-placeholder").assertIsDisplayed()
            compose.runOnIdle {
                assertFalse(host.snapshot!!.getBoolean("gpu_ready"))
                assertTrue(host.snapshot!!.getJSONObject("state").getBoolean("workspace_ready"))
                assertEquals("builtin:workspace:painter",host.workspaceManager!!.getString("id"))
                assertEquals("Sketch", host.workspaceManager!!.getString("name"))
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
            compose.onNodeWithContentDescription("Main Menu").assertIsDisplayed()
            resume.countDown()
            compose.onNodeWithContentDescription("Main Menu").performClick()
            host.drain();compose.waitForIdle()
            compose.onNodeWithText("View", useUnmergedTree = true).performScrollTo().assertIsDisplayed().performClick()
            host.drain();compose.waitForIdle()
            compose.onNodeWithText("Fit canvas").assertIsDisplayed()
            android.util.Log.i("CapyStartupTest", "gray_ui_without_gpu boot_ns=${SystemClock.elapsedRealtimeNanos()}")
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
            val diagnostic = cause.message!!
            CanvasHost.beforeGpuAttachForTest = { throw cause }
            try {
                ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                    val activity = scenario.activity()
                    val host = activity.host
                    compose.waitUntil(20_000) { host.failure != null && host.workspaceManager?.optBoolean("ready") == true }
                    compose.runOnIdle {
                        assertEquals(diagnostic, host.failure)
                        host.dispatch(obj("type" to "set_theme", "theme" to theme))
                    }
                    compose.waitUntil(10_000) { host.snapshot?.optJSONObject("state")?.optString("theme") == theme }
                    assertNull(host.actionError)
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
                    compose.waitUntil(60_000) {
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

    @Test fun unsupportedGpuExplainsTheDeviceInTheCanvasArea() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("unsupportedGpu") == "true")
        for (theme in listOf("light", "dark")) {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                val activity = scenario.activity()
                val host = activity.host
                compose.waitUntil(60_000) { host.failure != null && host.workspaceManager?.optBoolean("ready") == true }
                compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
                compose.waitUntil(10_000) { host.snapshot?.optJSONObject("state")?.optString("theme") == theme }
                assertNull(host.actionError)
                val failure = host.failure!!
                assertTrue(failure, failure.startsWith("This device’s graphics processor (GPU) can’t run Capy Canvas. Updating the system or graphics driver may help.\n\n"))
                assertFalse(failure, failure.contains("java.lang"))
                compose.onNodeWithText(host.bootstrap!!.getString("canvas_init_failed")).assertIsDisplayed()
                compose.onNodeWithText(failure, useUnmergedTree = true).assertIsDisplayed()
                compose.waitForIdle()
                val screen = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
                File(activity.getExternalFilesDir(null), "unsupported-gpu-$theme.png").outputStream().use {
                    screen.compress(Bitmap.CompressFormat.PNG, 100, it)
                }
                screen.recycle()
            }
        }
    }
}
