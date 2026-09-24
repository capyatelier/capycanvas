package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Color
import android.hardware.input.InputManager
import android.os.Build
import android.os.SystemClock
import android.util.Log
import android.view.InputDevice
import android.view.MotionPredictor
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.Assert.*
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Exercise native capability, disabled Compose controls and persisted selection.
 * Use a private workspace store and restore the user's exact settings afterward. */
class AndroidPredictionTest {
    @get:Rule val compose = createEmptyComposeRule()
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val preferences get() = context.getSharedPreferences("capy-canvas", 0)
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var original: JSONObject
    private var savedSettings: String? = null
    private var actualSupport = false
    private val tag = "preference-platform_prediction"
    private fun settings() = host.snapshot!!.getJSONObject("state").getJSONObject("settings")
    private fun rows() = host.snapshot!!.getJSONObject("preferences").array("pages").objects()
        .flatMap { it.array("groups").objects() }.flatMap { it.array("rows").objects() }
    private fun row(id: String = "platform_prediction") = rows().first { it.getString("id") == id }
    private fun manualControls(enabled: Boolean) {
        val id = "prediction_horizon"
        assertFalse(rows().any { it.getString("id") == "tip_lock" })
        compose.onNodeWithTag("preference-tip_lock").assertDoesNotExist()
        assertEquals(enabled, row(id).getBoolean("enabled"))
        assertEquals("slider", row(id).getJSONObject("kind").getJSONObject("control").getString("kind"))
        val control = compose.onNodeWithTag("setting-slider-$id").performScrollTo()
        if (enabled) control.assertIsEnabled() else control.assertIsNotEnabled()
        if (!enabled) assertFalse(row(id).getJSONObject("reset").getBoolean("enabled"))
        assertEquals(enabled, row("prediction_algorithm").getBoolean("enabled"))
        val choice = compose.onNodeWithTag("setting-choice-prediction_algorithm").performScrollTo()
        if (enabled) choice.assertIsEnabled() else choice.assertIsNotEnabled()
        compose.onNodeWithTag(tag).performScrollTo()
    }
    private fun waitFor(condition: () -> Boolean) {
        compose.waitUntil(20_000) { host.failure != null || host.actionError != null || condition() }
        assertNull(host.failure); assertNull(host.actionError)
        compose.waitForIdle()
    }
    private fun edit(id: String, value: Boolean) {
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to id, "value" to value)) }
        waitFor { settings().getBoolean(if (id == "platform_prediction") id else "feedback") == value }
    }
    private fun capability(available: Boolean) {
        compose.runOnIdle { host.updatePredictionAvailability(available) }
        waitFor { row().getBoolean("enabled") == (available && settings().getBoolean("feedback")) }
    }
    private fun shot(name: String) {
        val file = File(context.getExternalFilesDir(null), "validation/prediction-$name.png")
        file.parentFile!!.mkdirs()
        val bitmap = compose.onNodeWithTag("preferences-surface").captureToImage().asAndroidBitmap()
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
    @Before fun ready() {
        savedSettings = preferences.getString("settings", null)
        CanvasHost.workspaceDirectoryForTest = File(context.filesDir, "prediction-tests/${UUID.randomUUID()}").absolutePath
        RecoveryController.directoryForTest = File(CanvasHost.workspaceDirectoryForTest!!, "recovery")
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { host = it.host }
        compose.waitUntil(60_000) {
            host.failure != null || (host.snapshot?.optBoolean("brush_ready") == true &&
                host.workspaceManager?.optBoolean("ready") == true &&
                host.workspaceManager?.optBoolean("busy") == false)
        }
        assertNull(host.failure)
        original = JSONObject(settings().toString())
        actualSupport = if (Build.VERSION.SDK_INT >= 34) {
            val predictor = MotionPredictor(context)
            val manager = context.getSystemService(InputManager::class.java)
            manager.inputDeviceIds.any { id -> manager.getInputDevice(id)?.supportsSource(InputDevice.SOURCE_STYLUS) == true &&
                predictor.isPredictionAvailable(id, InputDevice.SOURCE_STYLUS) }
        } else false
        compose.runOnIdle { host.dispatch(obj("type" to "open_settings", "page" to "input")) }
        waitFor { host.snapshot?.objectOrNull("preferences") != null }
        edit("feedback", true)
        waitFor { row().getBoolean("enabled") == actualSupport }
        Log.i("CapyPredictionTest", "Connected stylus native prediction available: $actualSupport")
    }
    @After fun cleanup() {
        try {
            if (::original.isInitialized) {
                compose.runOnIdle {
                    host.updatePredictionAvailability(actualSupport)
                    host.dispatch(obj("type" to "restore_settings", "settings" to original))
                    host.dispatch(obj("type" to "close_settings"))
                }
                waitFor { settings().toString() == original.toString() }
            }
        } finally {
            try { if (::scenario.isInitialized) scenario.close() }
            finally {
                CanvasHost.workspaceDirectoryForTest = null
                RecoveryController.directoryForTest = null
                preferences.edit().apply {
                    if (savedSettings == null) remove("settings") else putString("settings", savedSettings)
                }.commit()
            }
        }
    }
    @Test fun nativePredictionCanBeComparedAndUnavailableControlIsDisabled() {
        val ids = rows().map { it.getString("id") }
        assertTrue(ids.contains("prediction_algorithm"))
        assertEquals("platform_prediction", ids[ids.indexOf("feedback") + 1])
        assertEquals("Use Android stroke prediction", row().getString("title"))
        manualControls(!actualSupport || !settings().getBoolean("platform_prediction"))
        compose.onNodeWithTag(tag).performScrollTo().assertIsDisplayed()
        shot("device-support")
        // A capability transition models connecting/disconnecting a supported pen,
        // and also covers systems without the API on this same physical device.
        capability(true)
        edit("platform_prediction", true)
        manualControls(false)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOn().performClick()
        waitFor { !settings().getBoolean("platform_prediction") && !host.nativePredictionEnabled }
        compose.onNodeWithTag(tag).assertIsOff()
        manualControls(true)
        compose.onNodeWithTag("setting-slider-prediction_horizon").performScrollTo()
            .performSemanticsAction(SemanticsActions.SetProgress) { it(.5f) }
        waitFor { settings().getDouble("prediction_ms") == 32.0 }
        compose.onNodeWithTag("number-value-prediction_horizon").assertTextEquals("32 ms")
        compose.onNodeWithTag(tag).performScrollTo()
        waitFor { preferences.getString("settings", null)?.let { !JSONObject(it).getBoolean("platform_prediction") } == true }
        shot("off")

        capability(false)
        manualControls(true)
        compose.onNodeWithTag(tag).assertIsNotEnabled().assertIsOff().performClick()
        assertFalse(settings().getBoolean("platform_prediction"))
        assertFalse(row().getJSONObject("reset").getBoolean("enabled"))
        shot("unavailable")

        capability(true)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOff().performClick()
        waitFor { settings().getBoolean("platform_prediction") && host.nativePredictionEnabled }
        compose.onNodeWithTag(tag).assertIsOn()
        manualControls(false)
        waitFor { preferences.getString("settings", null)?.let { JSONObject(it).getBoolean("platform_prediction") } == true }
        shot("on")
        edit("feedback", false)
        manualControls(false)
        compose.onNodeWithTag(tag).assertIsNotEnabled()
        assertFalse(host.nativePredictionEnabled)
        edit("feedback", true)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOn()
        capability(false)
        manualControls(true)
        compose.onNodeWithTag(tag).assertIsNotEnabled().assertIsOff()
        assertTrue("Losing support preserves the user's choice", settings().getBoolean("platform_prediction"))
        assertEquals("Manual prediction time survives native mode", 32.0, settings().getDouble("prediction_ms"), 0.0)
        compose.onNodeWithTag("number-value-prediction_horizon").performScrollTo().performClick()
        compose.onNodeWithTag("setting-number-prediction_horizon").performTextReplacement("")
        compose.onNodeWithTag("setting-number-prediction_horizon").performImeAction()
        waitFor { settings().getDouble("prediction_ms") == 16.0 }
        compose.onNodeWithTag("number-value-prediction_horizon").assertTextEquals("16 ms")
    }

    @Test fun optimizedPredictionIsTheOnlyChoiceAndPersists() {
        capability(false)
        val options = row("prediction_algorithm").getJSONObject("kind").getJSONArray("options")
        assertEquals(1, options.length())
        assertEquals("Smooth Motion (Optimized)", options.getString(0))
        compose.onNodeWithTag("setting-choice-prediction_algorithm").performScrollTo().performClick()
        compose.onNodeWithTag("setting-choice-option-prediction_algorithm-1").assertDoesNotExist()
        compose.onNodeWithTag("setting-choice-option-prediction_algorithm-0").performClick()
        // Selecting the current choice is a no-op. Persist a real edit so this
        // also exercises older settings files that have no algorithm field.
        val horizon = if (settings().getDouble("prediction_ms") == 24.0) 16.0 else 24.0
        compose.runOnIdle { host.preference(obj("type" to "edit", "id" to "prediction_horizon", "value" to horizon)) }
        waitFor { settings().getString("prediction_algorithm") == "optimized" &&
            preferences.getString("settings", null)?.let { JSONObject(it).optString("prediction_algorithm") } == "optimized" }
        shot("algorithm-optimized")
        scenario.recreate()
        scenario.onActivity { host = it.host }
        waitFor { host.snapshot?.objectOrNull("state")?.objectOrNull("settings")?.optString("prediction_algorithm") == "optimized" }
        compose.runOnIdle { host.dispatch(obj("type" to "open_settings", "page" to "input")) }
        waitFor { host.snapshot?.objectOrNull("preferences") != null }
        capability(false)
        assertEquals(0, row("prediction_algorithm").getJSONObject("kind").getInt("selected"))
        assertEquals(horizon, settings().getDouble("prediction_ms"), 0.0)
    }

    @Test fun fallingPressureStrokeRendersAndSurvivesUndoRedo() {
        compose.runOnIdle {
            host.dispatch(obj("type" to "restore_settings", "settings" to JSONObject(settings().toString())
                .put("feedback", true).put("platform_prediction", false).put("prediction_ms", 16)))
            host.dispatch(obj("type" to "close_settings"))
        }
        waitFor { host.snapshot?.objectOrNull("preferences") == null && !host.nativePredictionEnabled }
        fun find(view: View): CanvasSurfaceView? = when (view) {
            is CanvasSurfaceView -> view
            is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { find(view.getChildAt(it)) }
            else -> null
        }
        lateinit var canvas: CanvasSurfaceView
        val location = IntArray(2)
        scenario.onActivity { canvas = find(it.window.decorView)!!; canvas.getLocationOnScreen(location) }
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        fun pixels(name: String): Int {
            val presented = CountDownLatch(1)
            compose.runOnIdle { canvas.postOnAnimation { canvas.postOnAnimation { presented.countDown() } } }
            assertTrue(presented.await(5, TimeUnit.SECONDS))
            val bitmap = instrumentation.uiAutomation.takeScreenshot()
            val directory = File(context.getExternalFilesDir(null), "validation").apply { mkdirs() }
            File(directory, "prediction-$name.png").outputStream().use {
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
            }
            var dark = 0
            for (y in location[1] + canvas.height * 3 / 10 until location[1] + canvas.height * 7 / 10 step 2)
                for (x in location[0] + canvas.width * 3 / 10 until location[0] + canvas.width * 7 / 10 step 2) {
                    val c = bitmap.getPixel(x, y)
                    if (Color.red(c) < 100 && Color.green(c) < 100 && Color.blue(c) < 100) dark++
                }
            bitmap.recycle()
            return dark
        }
        val before = pixels("stroke-before")
        val down = SystemClock.uptimeMillis()
        for (i in 0..24) {
            val coords = MotionEvent.PointerCoords().apply {
                x = location[0] + canvas.width * (.35f + i / 24f * .3f)
                y = location[1] + canvas.height * (.5f + kotlin.math.sin(i / 6f) * .05f)
                pressure = if (i < 20) .65f else (24 - i) * .13f
            }
            val props = MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS }
            val phase = when (i) { 0 -> MotionEvent.ACTION_DOWN; 24 -> MotionEvent.ACTION_UP; else -> MotionEvent.ACTION_MOVE }
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), phase, 1, arrayOf(props), arrayOf(coords),
                0, 0, 1f, 1f, 1, 0, InputDevice.SOURCE_STYLUS, 0)
            assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
            event.recycle()
            if (i < 24) SystemClock.sleep(8)
        }
        fun commandEnabled(id: String) = host.snapshot!!.getJSONObject("state").array("commands").objects()
            .any { it.getString("id") == id && it.getBoolean("enabled") }
        waitFor { commandEnabled("undo") }
        val painted = pixels("stroke-painted")
        assertTrue("Pen input deposits visible pixels", painted > before + 100)
        compose.runOnIdle { host.dispatch(obj("type" to "invoke", "command" to "undo")) }
        waitFor { commandEnabled("redo") }
        assertTrue("Undo removes committed ink and leaves no predicted tail", pixels("stroke-undo") < before + (painted - before) / 10)
        compose.runOnIdle { host.dispatch(obj("type" to "invoke", "command" to "redo")) }
        waitFor { commandEnabled("undo") }
        assertTrue("Replay restores the real stroke", pixels("stroke-redo") >= before + (painted - before) * 9 / 10)
    }
}
