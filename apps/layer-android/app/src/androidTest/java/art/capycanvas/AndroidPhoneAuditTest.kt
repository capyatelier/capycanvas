package art.capycanvas

import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.WindowInsets
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.runtime.Recomposer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.sp
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File

class AndroidPhoneAuditTest {
    @get:Rule val device = CapyDeviceRule()

    private fun shell(command: String): String = ParcelFileDescriptor.AutoCloseInputStream(
        instrumentation.uiAutomation.executeShellCommand(command)).bufferedReader().use { it.readText().trim() }

    private fun collect(node: SemanticsNode, allocations: JSONObject) {
        node.config.getOrNull(SemanticsProperties.TestTag)?.let { tag ->
            val bounds = node.boundsInRoot
            allocations.put(tag, JSONArray(listOf(bounds.left, bounds.top, bounds.width, bounds.height)))
        }
        node.children.forEach { collect(it, allocations) }
    }

    @Test fun admission() {
        val args = InstrumentationRegistry.getArguments()
        val case = requireNotNull(args.getString("case"))
        require(case.matches(Regex("[a-zA-Z0-9_.-]+")))
        val size = requireNotNull(args.getString("size")).split("x").map(String::toInt)
        val expectedDensity = requireNotNull(args.getString("density")).toInt()
        val expectedTheme = requireNotNull(args.getString("theme"))
        val expectedFont = args.getString("fontScale", "1").toFloat()
        val cutout = args.getString("cutout", "none")
        val directory = File(instrumentation.targetContext.getExternalFilesDir(null), "phone-audit/$case").apply { mkdirs() }
        val started = SystemClock.elapsedRealtime()
        val report = obj("schema_version" to 1, "host" to "android", "family" to "frame", "state" to "rest", "case" to case,
            "fixture" to "builtin:workspace:painter", "theme" to expectedTheme, "text_size" to args.getString("textSize", "11").toDouble(), "requested_pixels" to JSONArray(size),
            "requested_density" to expectedDensity, "requested_font_scale" to expectedFont, "status" to "failed", "passed" to false)
        var scenario: androidx.test.core.app.ActivityScenario<MainActivity>? = null
        try {
            wakeDevice()
            val devices = JSONObject(shell("cmd gpu vkjson")).getJSONArray("devices")
            assertEquals("A single Vulkan adapter makes admission unambiguous", 1, devices.length())
            val gpu = devices.getJSONObject(0).getJSONObject("properties")
            val adapter = gpu.getString("deviceName")
            val hal = shell("getprop ro.hardware.vulkan")
            report.put("gpu", obj("name" to adapter, "type" to gpu.getInt("deviceType"), "api_version" to gpu.getLong("apiVersion"),
                "driver" to devices.getJSONObject(0).optJSONObject("core12")?.optJSONObject("properties")?.optString("driverName"),
                "vulkan_hal" to hal, "backend" to "Vulkan"))
            assertEquals("Expected the gfxstream (ranchu) Vulkan HAL", "ranchu", hal)
            assertNotEquals("CPU adapter is forbidden", 4, gpu.getInt("deviceType"))
            assertFalse("Software adapter is forbidden: $adapter", listOf("swiftshader", "llvmpipe", "lavapipe").any { adapter.contains(it, ignoreCase = true) })
            scenario = launchCapy(180_000)
            val activity = scenario.activity()
            val host = activity.host
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:painter")) }
            host.awaitMain("Painter fixture", 30_000) { host.workspaceManager?.optString("id") == "builtin:workspace:painter" && host.workspaceManager?.optBoolean("busy") == false }
            host.awaitMain("first canvas surface", 180_000, { "$report" }) { host.surfaceReady }
            val frameDeadline = SystemClock.elapsedRealtime() + 180_000
            var frameReady = false
            while (!frameReady && SystemClock.elapsedRealtime() < frameDeadline) {
                frameReady = runBlocking { host.withNative {
                    val status = JSONObject(Native.displayStatus(it))
                    status.optLong("completed_frames") > 0 && !Native.renderingPending(it)
                } }
                if (!frameReady) SystemClock.sleep(16)
            }
            assertTrue("Completed first canvas frame", frameReady)
            host.drain()
            host.awaitMain("Compose idle") { Recomposer.runningRecomposers.value.none { it.hasPendingWork } }
            instrumentation.waitForIdleSync()
            val display = runBlocking { host.withNative { JSONObject(Native.displayStatus(it)) } }
            report.put("display", display)
            val crop = IntArray(4)
            val allocations = JSONObject()
            instrumentation.runOnMainSync {
                val decor = activity.window.decorView
                val location = IntArray(2)
                decor.getLocationOnScreen(location)
                crop[0] = location[0]; crop[1] = location[1]; crop[2] = decor.width; crop[3] = decor.height
                val metrics = activity.resources.displayMetrics
                val density = metrics.density
                report.put("density", metrics.densityDpi)
                report.put("dpr", density)
                report.put("viewport", JSONArray(listOf(decor.width / density, decor.height / density)))
                report.put("font_scale", activity.resources.configuration.fontScale)
                val textSize = (host.catalog.optDouble("text_size_pt", 11.0) * 4 / 3).toFloat()
                report.put("workspace_text_sp", textSize)
                report.put("workspace_font_pixels", Density(activity).run { textSize.sp.toPx() })
                val insets = decor.rootWindowInsets
                val safe = insets.getInsets(WindowInsets.Type.displayCutout() or WindowInsets.Type.captionBar())
                val waterfall = insets.displayCutout?.waterfallInsets ?: android.graphics.Insets.NONE
                val actualInsets = listOf(maxOf(safe.left, waterfall.left), maxOf(safe.top, waterfall.top), maxOf(safe.right, waterfall.right), maxOf(safe.bottom, waterfall.bottom))
                report.put("insets_pixels", JSONArray(actualInsets))
                report.put("insets", JSONArray(actualInsets.map { it / density }))
                report.put("crop", obj("x" to crop[0], "y" to crop[1], "width" to crop[2], "height" to crop[3]))
                val surface = checkNotNull(decor.descendant<CanvasSurfaceView>())
                report.put("canvas_viewport", JSONArray(listOf(surface.width / density, surface.height / density)))
                val canvasLocation = IntArray(2)
                surface.getLocationOnScreen(canvasLocation)
                report.put("canvas_origin_pixels", JSONArray(canvasLocation.toList()))
                val root = checkNotNull(findTag("workspace"))
                collect(root.second, allocations)
                report.put("phone_active", host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") != null)
                report.put("revision", host.snapshot?.optLong("revision"))
                assertEquals("System theme", expectedTheme, host.snapshot?.getJSONObject("state")?.getString("theme"))
                assertEquals("Configured density", expectedDensity, metrics.densityDpi)
                assertEquals("Configured font scale", expectedFont, activity.resources.configuration.fontScale, .001f)
                assertEquals("App viewport width in pixels", size[0], decor.width)
                assertEquals("App viewport height in pixels", size[1], decor.height)
                assertEquals("App viewport width in dp", size[0] * 160f / expectedDensity, decor.width / density, .01f)
                assertEquals("App viewport height in dp", size[1] * 160f / expectedDensity, decor.height / density, .01f)
                if (cutout == "none") assertEquals("No safe-area insets", listOf(0, 0, 0, 0), actualInsets)
                else assertTrue("Corner cutout produces safe insets", actualInsets.any { it > 0 })
                args.getString("insets")?.split(",")?.map(String::toInt)?.let { assertEquals("Configured cutout insets", it, actualInsets) }
                assertEquals("Native surface width", surface.width, display.getJSONArray("extent").getInt(0))
                assertEquals("Native surface height", surface.height, display.getJSONArray("extent").getInt(1))
            }
            report.put("allocations", allocations)
            val screenshot = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
            report.put("raw_size", JSONArray(listOf(screenshot.width, screenshot.height)))
            assertTrue("App crop is inside display capture", crop[0] >= 0 && crop[1] >= 0 && crop[0] + crop[2] <= screenshot.width && crop[1] + crop[3] <= screenshot.height)
            val image = Bitmap.createBitmap(screenshot, crop[0], crop[1], crop[2], crop[3])
            try {
                assertEquals("Crop width equals app viewport", size[0], image.width)
                assertEquals("Crop height equals app viewport", size[1], image.height)
                File(directory, "admission.png").outputStream().use { assertTrue(image.compress(Bitmap.CompressFormat.PNG, 100, it)) }
                report.put("dimensions", JSONArray(listOf(image.width, image.height)))
            } finally {
                if (image !== screenshot) image.recycle()
                screenshot.recycle()
            }
            report.put("status", "captured").put("passed", true)
        } catch (error: Throwable) {
            report.put("error", error.toString())
            throw error
        } finally {
            report.put("elapsed_seconds", (SystemClock.elapsedRealtime() - started) / 1000.0)
            File(directory, "admission.json").writeText(report.toString(2) + "\n")
            scenario?.close()
        }
    }
}
