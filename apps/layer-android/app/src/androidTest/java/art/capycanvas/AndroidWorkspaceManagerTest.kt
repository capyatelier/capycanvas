package art.capycanvas

import kotlinx.coroutines.launch
import android.graphics.Bitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Real Compose controls and MotionEvents, with a private test workspace store. */
class AndroidWorkspaceManagerTest {
    @get:Rule val compose = createEmptyComposeRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private lateinit var directory: File
    private lateinit var legacy: Map<String, *>
    private fun view() = host.workspaceManager!!
    private fun state() = host.snapshot!!.getJSONObject("state")
    @Before fun ready() {
        directory = File(instrumentation.targetContext.filesDir, "workspace-tests/${UUID.randomUUID()}")
        CanvasHost.workspaceDirectoryForTest = directory.absolutePath
        legacy = instrumentation.targetContext.getSharedPreferences("capy-canvas", 0).all
        launch()
    }
    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { host = it.host }
        compose.waitUntil(60000) { host.snapshot?.optBoolean("brush_ready") == true && host.workspaceManager?.optBoolean("ready") == true }
        idle()
        if (state().getJSONObject("workspace").optBoolean("zen_mode")) action(obj("type" to "invoke", "command" to "zen_mode"))
        shot("startup")
        File(instrumentation.targetContext.getExternalFilesDir(null), "validation/workspaces/startup.json").writeText(host.snapshot.toString())
        assertNull(host.failure)
    }
    private fun idle() {
        compose.waitUntil(15000) { host.workspaceManager?.let { !it.optBoolean("busy") && !it.optBoolean("switcher_busy") && !it.optBoolean("dirty") } == true }
        compose.waitForIdle()
        assertTrue(view().isNull("error"))
    }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        scenario.onActivity { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        Thread.sleep(350); idle()
    }
    private fun manager(value: JSONObject) {
        scenario.onActivity { host.workspaceInput(value) }
        Thread.sleep(300); compose.waitForIdle()
    }
    private fun tap(tag: String) {
        val node = compose.onNodeWithTag(tag, useUnmergedTree = true)
        if (tag.startsWith("workspace-switch-")) {
            idle(); node.performScrollTo(); compose.waitForIdle(); node.assertIsEnabled().assertIsDisplayed()
        }
        node.performTouchInput { click() }; Thread.sleep(250); compose.waitForIdle()
    }
    private fun menu(label: String) {
        tap("application-menu-window")
        compose.onNodeWithText("Workspaces", useUnmergedTree = true).performTouchInput { click() }
        compose.onNodeWithText(label, useUnmergedTree = true).performTouchInput { click() }
        Thread.sleep(300); idle()
    }
    private fun capture(): JSONObject {
        var result = ""
        val done = CountDownLatch(1)
        scenario.onActivity {
            kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Main).launch {
                result = host.withNative { Native.workspace(it, obj("type" to "capture").toString()) }; done.countDown()
            }
        }
        assertTrue(done.await(10, TimeUnit.SECONDS)); return JSONObject(result)
    }
    private fun normalized(value: JSONObject): String {
        val copy = JSONObject(value.toString())
        copy.getJSONObject("history").getJSONObject("revisions").apply { keys().forEach { getJSONObject(it).put("timestamp_ms", "date") } }
        return copy.toString()
    }
    private fun shot(name: String) {
        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/workspaces/$name.png")
        file.parentFile!!.mkdirs()
        instrumentation.uiAutomation.takeScreenshot()?.let { image -> file.outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }; image.recycle() }
    }
    @After fun cleanup() {
        if (::scenario.isInitialized) scenario.close()
        CanvasHost.workspaceDirectoryForTest = null
        assertEquals("Legacy preferences are preserved", legacy, instrumentation.targetContext.getSharedPreferences("capy-canvas", 0).all)
    }
    @Test fun workspacePreviewSwitchHistoryAndRestart() {
        val original = view().getString("id")
        val initial = capture()
        menu("Manage Workspaces…")
        compose.onNodeWithTag("workspace-confirm").assertIsNotEnabled()
        shot("workspaces")
        tap("new-workspace")
        compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("workspace-name")), useUnmergedTree = true).performTextReplacement("Tablet Painting")
        shot("new-workspace")
        tap("workspace-submit"); idle()
        val painting = view().getString("id")
        assertNotEquals(original, painting)
        assertEquals("Tablet Painting", view().getString("name"))
        action(obj("type" to "invoke", "command" to "eraser"))
        action(obj("type" to "move_panel", "panel" to "layers", "viewport" to org.json.JSONArray(listOf(1400, 900)), "target" to obj("kind" to "float", "position" to org.json.JSONArray(listOf(480, 220)))))
        val changed = capture()
        assertNotEquals(initial.getJSONObject("history").toString(), changed.getJSONObject("history").toString())
        menu("Manage Workspaces…")
        tap("workspace-row-$original")
        assertEquals(painting, view().getString("id"))
        assertEquals(changed.toString(), capture().toString())
        shot("preview-original")
        instrumentation.sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_BACK)
        Thread.sleep(300); idle(); assertTrue(view().isNull("page"))
        assertEquals(changed.toString(), capture().toString())
        menu("Manage Workspaces…"); tap("workspace-row-$original")
        tap("workspace-cancel"); idle()
        assertEquals(changed.toString(), capture().toString())
        menu("Manage Workspaces…"); tap("workspace-row-$original"); tap("workspace-confirm"); idle()
        assertEquals(original, view().getString("id"))
        assertEquals(initial.getJSONObject("working").toString(), capture().getJSONObject("working").toString())
        menu("Manage Workspaces…"); tap("workspace-row-$painting"); tap("workspace-confirm"); idle()
        assertEquals(changed.getJSONObject("working").toString(), capture().getJSONObject("working").toString())
        menu("Layout History…")
        compose.onNodeWithTag("workspace-confirm").assertIsNotEnabled()
        shot("history")
        tap("workspace-row-r0"); tap("workspace-confirm"); idle()
        val restored = capture()
        assertEquals(changed.getJSONObject("working").toString(), restored.getJSONObject("working").toString())
        action(obj("type" to "invoke", "command" to "undo_workspace"))
        val beforeRestart = capture()
        scenario.close(); Thread.sleep(500); launch()
        assertEquals(painting, view().getString("id"))
        assertEquals(normalized(beforeRestart), normalized(capture()))
        menu("Manage Workspaces…"); tap("workspace-options-$painting"); tap("workspace-rename")
        compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("workspace-name")), useUnmergedTree = true).performTextReplacement("Tablet Inking")
        tap("workspace-submit"); idle(); tap("workspace-cancel")
        assertEquals("Tablet Inking", view().getString("name"))
        for (row in view().array("defaults").objects()) {
            val id = row.getString("id")
            shot("before-header-switch")
            tap("workspace-switch-$id"); idle()
            if (id != view().getString("id")) {
                shot("header-switch-failure")
                File(instrumentation.targetContext.getExternalFilesDir(null), "validation/workspaces/header-switch-failure.json").writeText(view().toString())
            }
            assertEquals(id, view().getString("id"))
            compose.onNodeWithTag("workspace-switch-$id").assertIsSelected()
        }
        menu("Manage Workspaces…")
        for (row in view().array("rows").objects().filter { it.getString("id").startsWith("builtin:workspace:") }) {
            assertTrue(row.getBoolean("options")); assertFalse(row.getBoolean("delete"))
        }
        tap("workspace-row-$painting"); tap("workspace-confirm"); idle()
        assertEquals(normalized(beforeRestart), normalized(capture()))
        action(obj("type" to "invoke", "command" to "brush")); action(obj("type" to "set_brush_size", "value" to 73))
        action(obj("type" to "invoke", "command" to "eraser")); action(obj("type" to "set_brush_size", "value" to 91))
        val beforeReset = capture()
        menu("Reset All Brushes…")
        compose.onNodeWithText("Cancel", useUnmergedTree = true).performTouchInput { click() }; Thread.sleep(250); idle()
        assertEquals(normalized(beforeReset), normalized(capture()))
        menu("Reset All Brushes…"); shot("reset-brushes"); tap("workspace-submit"); idle()
        beforeReset.getJSONObject("working").getJSONObject("tools").put("overrides", JSONObject())
        assertEquals(normalized(beforeReset), normalized(capture()))
        menu("Restore Starting Layout…"); tap("workspace-submit"); idle()
        assertEquals(beforeReset.getJSONObject("working").toString(), capture().getJSONObject("working").toString())
        scenario.close(); Thread.sleep(500); launch()
        assertEquals(painting, view().getString("id"))
        assertEquals(beforeReset.getJSONObject("working").toString(), capture().getJSONObject("working").toString())
    }
}
