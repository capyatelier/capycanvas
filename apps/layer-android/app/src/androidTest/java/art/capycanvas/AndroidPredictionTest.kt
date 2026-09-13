package art.capycanvas

import android.graphics.Bitmap
import android.hardware.input.InputManager
import android.os.Build
import android.util.Log
import android.view.InputDevice
import android.view.MotionPredictor
import androidx.compose.ui.graphics.asAndroidBitmap
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
    private fun row() = host.snapshot!!.getJSONObject("preferences").array("pages").objects()
        .flatMap { it.array("groups").objects() }.flatMap { it.array("rows").objects() }
        .first { it.getString("id") == "platform_prediction" }
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
                preferences.edit().apply {
                    if (savedSettings == null) remove("settings") else putString("settings", savedSettings)
                }.commit()
            }
        }
    }
    @Test fun nativePredictionCanBeComparedAndUnavailableControlIsDisabled() {
        compose.onNodeWithTag(tag).performScrollTo().assertIsDisplayed()
        shot("device-support")
        // A capability transition models connecting/disconnecting a supported pen,
        // and also covers systems without the API on this same physical device.
        capability(true)
        edit("platform_prediction", true)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOn().performClick()
        waitFor { !settings().getBoolean("platform_prediction") && !host.nativePredictionEnabled }
        compose.onNodeWithTag(tag).assertIsOff()
        waitFor { preferences.getString("settings", null)?.let { !JSONObject(it).getBoolean("platform_prediction") } == true }
        shot("off")

        capability(false)
        compose.onNodeWithTag(tag).assertIsNotEnabled().assertIsOff().performClick()
        assertFalse(settings().getBoolean("platform_prediction"))
        assertFalse(row().getJSONObject("reset").getBoolean("enabled"))
        shot("unavailable")

        capability(true)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOff().performClick()
        waitFor { settings().getBoolean("platform_prediction") && host.nativePredictionEnabled }
        compose.onNodeWithTag(tag).assertIsOn()
        waitFor { preferences.getString("settings", null)?.let { JSONObject(it).getBoolean("platform_prediction") } == true }
        shot("on")
        edit("feedback", false)
        compose.onNodeWithTag(tag).assertIsNotEnabled()
        assertFalse(host.nativePredictionEnabled)
        edit("feedback", true)
        compose.onNodeWithTag(tag).assertIsEnabled().assertIsOn()
        capability(false)
        compose.onNodeWithTag(tag).assertIsNotEnabled().assertIsOn()
        assertTrue("Losing support preserves the user's choice", settings().getBoolean("platform_prediction"))
    }
}
