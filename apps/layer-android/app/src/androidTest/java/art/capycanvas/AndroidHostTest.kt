package art.capycanvas

import android.graphics.Bitmap
import android.content.ContentValues
import android.provider.MediaStore
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real native widgets, JNI and Vulkan in the tablet emulator. No fake renderer. */
class AndroidHostTest {
    companion object { private val runId = System.currentTimeMillis().toString() }
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    @Before fun ready() {
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("gpu_ready") == true || host.failure != null }
        assertNull("GPU initialization", host.failure)
        compose.runOnIdle {
            host.dispatch(obj("type" to "cancel_settings"))
            host.dispatch(obj("type" to "set_theme", "theme" to "light"))
            host.invoke("reset_layout")
        }
        compose.waitUntil(10_000) { host.snapshot?.getJSONObject("state")?.optString("theme") == "light" }
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun waitState(test: (JSONObject) -> Boolean) = compose.waitUntil(10_000) { test(state()) }
    private fun findCanvas(view: View): CanvasSurfaceView? = when (view) {
        is CanvasSurfaceView -> view
        is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { findCanvas(view.getChildAt(it)) }
        else -> null
    }
    private fun penStroke(steps: Int = 60, synchronous: Boolean = true) {
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        instrumentation.runOnMainSync {
            canvas = findCanvas(compose.activity.window.decorView)!!
            canvas.getLocationOnScreen(location)
        }
        val down = SystemClock.uptimeMillis()
        for (i in 0..steps) {
            val coords = MotionEvent.PointerCoords().apply {
                x = canvas.width * (0.35f + i.toFloat() / steps * 0.3f) + location[0]
                y = canvas.height * (0.45f + kotlin.math.sin(i / 12f) * 0.08f) + location[1]
                pressure = 0.3f + i.toFloat() / steps * 0.6f
                setAxisValue(MotionEvent.AXIS_TILT, 0.3f)
                setAxisValue(MotionEvent.AXIS_ORIENTATION, 0.2f)
            }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val action = when (i) { 0 -> MotionEvent.ACTION_DOWN; steps -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1, arrayOf(props), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
            assertTrue("Stylus event accepted", instrumentation.uiAutomation.injectInputEvent(event, synchronous))
            event.recycle()
            if (i != steps) SystemClock.sleep(8)
        }
    }
    private fun capture(name: String): Bitmap {
        compose.waitForIdle()
        val bitmap = instrumentation.uiAutomation.takeScreenshot()
        val directory = File(compose.activity.getExternalFilesDir(null), "validation").apply { mkdirs() }
        File(directory, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        // Gradle's connected-test runner uninstalls the app, removing its private
        // output directory. MediaStore test captures survive for visual review.
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, "$name.png")
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/CapyCanvasValidation/$runId")
            put(MediaStore.Images.Media.IS_PENDING, 1)
        })!!
        resolver.openOutputStream(uri)!!.use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
        return bitmap
    }
    @Test fun stylusDrawsAndUndoRedoChangePixels() {
        penStroke()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        assertNull(host.failure)
        val painted = capture("01-stylus-light")
        var dark = 0
        for (y in painted.height * 35 / 100 until painted.height * 65 / 100 step 2) {
            for (x in painted.width * 35 / 100 until painted.width * 65 / 100 step 2) {
                val c = painted.getPixel(x, y)
                if (android.graphics.Color.red(c) < 100 && android.graphics.Color.green(c) < 100) dark++
            }
        }
        assertTrue("Stroke deposits visible pixels in the canvas, not just cursor state ($dark)", dark > 100)
        compose.onNodeWithContentDescription("Undo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "redo" && c.getBoolean("enabled") } }
        capture("02-undo")
        compose.onNodeWithContentDescription("Redo").performClick()
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        capture("03-redo")
    }
    @Test fun measureHighRateStylusIngressAndRenderScheduling() {
        val cleared = CountDownLatch(1)
        host.measurements(true) { cleared.countDown() }
        assertTrue(cleared.await(10, TimeUnit.SECONDS))
        penStroke(600, synchronous = false)
        waitState { it.array("commands").objects().any { c -> c.getString("id") == "undo" && c.getBoolean("enabled") } }
        val collected = CountDownLatch(1)
        var report: JSONObject? = null
        host.measurements { report = it; collected.countDown() }
        assertTrue(collected.await(10, TimeUnit.SECONDS))
        val data = report!!
        assertTrue("Input stream reached the native host", data.array("inputs").length() > 100)
        assertTrue("Renderer produced continuous frames", data.array("frames").length() > 100)
        val rows = data.array("frames").values().map { it as org.json.JSONArray }
        val input = data.array("inputs").values().map { it as org.json.JSONArray }
        fun summary(values: List<Double>): JSONObject {
            val sorted = values.sorted()
            fun p(q: Double) = sorted[((sorted.size - 1) * q).toInt()]
            return obj("count" to sorted.size, "p50_ms" to p(0.5), "p95_ms" to p(0.95), "p99_ms" to p(0.99), "max_ms" to sorted.last())
        }
        val summary = obj("cpu_render_present" to summary(rows.map { it.getDouble(2) / 1e6 }),
            "cpu_paint" to summary(rows.map { it.getDouble(4) / 1e6 }),
            "surface_acquire" to summary(rows.map { it.getDouble(5) / 1e6 }),
            "cpu_present" to summary(rows.map { it.getDouble(6) / 1e6 }),
            "frame_interval" to summary(rows.zipWithNext { a, b -> (b.getDouble(0) - a.getDouble(0)) / 1e6 }),
            "input_delivery" to summary(input.map { (it.getDouble(1) - it.getDouble(0)) / 1e6 }),
            "input_queue" to summary(input.map { (it.getDouble(2) - it.getDouble(1)) / 1e6 }),
            "cpu_input" to summary(input.map { it.getDouble(3) / 1e6 }))
        android.util.Log.i("CapyBenchmark", summary.toString())
        val resolver = compose.activity.contentResolver
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, "latency.json")
            put(MediaStore.Downloads.MIME_TYPE, "application/json")
            put(MediaStore.Downloads.RELATIVE_PATH, "Download/CapyCanvasValidation/$runId")
            put(MediaStore.Downloads.IS_PENDING, 1)
        })!!
        data.put("summary", summary)
        resolver.openOutputStream(uri)!!.bufferedWriter().use { it.write(data.toString()) }
        resolver.update(uri, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
        capture("11-high-rate-stylus")
    }
    @Test fun preferencesSearchAndThemeAreCoreDriven() {
        compose.onNodeWithContentDescription("Preferences").performClick()
        compose.waitUntil(10_000) { host.snapshot?.objectOrNull("preferences") != null }
        capture("04-preferences-light")
        compose.onNodeWithContentDescription("Search preferences").performClick()
        compose.onNodeWithText("Search preferences").performTextInput("prediction")
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").array("search_results").length() > 0 }
        capture("05-settings-search")
        compose.onNodeWithText("×").performClick()
        compose.onNodeWithText("Pen & Input").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("preferences").getString("page") == "input" }
        capture("06-pen-input")
        compose.onNodeWithText("Back").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") == null }
        compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to "dark")) }
        waitState { it.getString("theme") == "dark" }
        capture("07-workspace-dark")
        compose.onNodeWithContentDescription("Preferences").performClick()
        compose.waitUntil(10_000) { host.snapshot!!.objectOrNull("preferences") != null }
        capture("08-preferences-dark")
    }
    @Test fun panelDrawerAndDividerUseSharedLayout() {
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").optString("expanded") == "brushes" }
        capture("09-brush-drawer")
        compose.onAllNodesWithText("Brushes", useUnmergedTree = true).onFirst().performClick()
        waitState { it.getJSONObject("customization").isNull("expanded") }
        val before = host.snapshot!!.getJSONObject("layout").toString()
        val divider = host.snapshot!!.getJSONObject("layout").array("dividers").objects().first { !it.getBoolean("band") }
        val density = compose.activity.resources.displayMetrics.density
        compose.onNodeWithTag("divider-${divider.getInt("id")}").performTouchInput {
            swipe(center, center - androidx.compose.ui.geometry.Offset(0f, 70 * density), 600)
        }
        compose.waitUntil(10_000) { host.snapshot!!.getJSONObject("layout").toString() != before }
        capture("10-divider-resize")
    }
}
