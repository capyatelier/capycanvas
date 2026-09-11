package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Hold the real renderer before device creation, with the real Compose UI running. */
class AndroidFirstUiTest {
    @get:Rule val compose = createEmptyComposeRule()

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
            val instrumentation = InstrumentationRegistry.getInstrumentation()
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
}
