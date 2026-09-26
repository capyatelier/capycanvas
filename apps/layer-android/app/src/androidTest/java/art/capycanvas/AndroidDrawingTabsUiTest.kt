package art.capycanvas

import android.net.Uri
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowManager
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.lifecycle.viewModelScope
import androidx.test.core.app.ActivityScenario
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.io.File

/** Production Choreographer/Compose timing, real InputDispatcher taps, real
 * document controllers and Vulkan. Deliberately no Compose test-clock rule:
 * advancing a virtual clock only between native calls masked close cancellation.
 */
class AndroidDrawingTabsUiTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var activity: MainActivity
    private lateinit var host: CanvasHost
    private fun <T> ui(block: () -> T): T {
        var result: Result<T>? = null
        instrumentation.runOnMainSync { result = runCatching(block) }
        return result!!.getOrThrow()
    }
    private fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    private fun tabs() = native { JSONObject(Native.documentTabs(it, obj("op" to "view").toString())) }
    private fun ids() = tabs().array("tabs").objects().map { it.getLong("id") }
    private fun healthy() = ui {
        assertNull("Canvas failure", host.failure)
        assertNull("Action error", host.actionError)
        val error = host.snapshot?.objectOrNull("state")?.optString("host_error")
        assertTrue("Shared error: $error", error.isNullOrEmpty() || error == "null")
    }
    private fun waitFor(label: String, timeout: Long = 60_000, checkErrors: Boolean = true, condition: () -> Boolean) {
        val end = SystemClock.uptimeMillis() + timeout
        while (true) {
            if (checkErrors) healthy()
            if (condition()) return
            if (SystemClock.uptimeMillis() > end) fail("Timed out: $label\n" + ui {
                "tabs=${host.drawingTabs.view}; switching=${host.drawingTabs.switching}; " +
                    "workspace=${host.workspaceManager}; file=${host.snapshot?.objectOrNull("state")?.objectOrNull("document_file")}; " +
                    "requests=${host.snapshot?.objectOrNull("state")?.array("requests")}"
            })
            SystemClock.sleep(20)
        }
    }
    private fun ready() {
        waitFor("drawing idle") { ui { !host.drawingTabs.blocked } && native { JSONObject(Native.documentTabs(it, obj("op" to "ready").toString())).getBoolean("park") } }
        healthy()
    }
    private fun point(match: (SemanticsNode) -> Boolean): Offset? = ui {
        semanticsRoots().asReversed().firstNotNullOfOrNull { root ->
            root.find(match)?.let { node ->
                val screen = IntArray(2); root.view.getLocationOnScreen(screen)
                node.boundsInRoot.center + Offset(screen[0].toFloat(), screen[1].toFloat())
            }
        }
    }
    private fun tag(value: String) = hasTag(value)
    private fun text(value: String) = hasLabel(value)
    private fun description(value: String): (SemanticsNode) -> Boolean = { it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(value) == true }
    /** The narrow title bar folds File into its primary menu; exercise the
     * same route a tablet user sees instead of assuming desktop menu labels. */
    private fun openFileMenu() {
        if (point(tag("application-menu-file")) != null) tap(tag("application-menu-file"))
        else { tap(tag("header-menu-labels-compact")); tap(text("File")) }
    }
    private fun tap(match: (SemanticsNode) -> Boolean, checkErrors: Boolean = true) {
        var target: Offset? = null
        var stableSince = SystemClock.uptimeMillis()
        waitFor("visible, settled tap target", checkErrors = checkErrors) {
            val next = point(match)
            if (next == null || target == null || (next - target!!).getDistance() > 1f) stableSince = SystemClock.uptimeMillis()
            target = next
            // Native popup windows animate independently of their Compose
            // semantics. Wait through their entry animation before tapping.
            next != null && SystemClock.uptimeMillis() - stableSince >= 250
        }
        val p = target!!; val down = SystemClock.uptimeMillis()
        for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, p.x, p.y, 0)
            event.source = InputDevice.SOURCE_TOUCHSCREEN
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
            SystemClock.sleep(40)
        }
    }
    private fun create(): Long {
        ready(); val count = ids().size
        tap(description("New…")); tap(text("Create"))
        waitFor("new drawing appended") { ids().size == count + 1 }; ready()
        return tabs().getLong("selected")
    }
    private fun dirty() { tap(description("New layer")); waitFor("modified drawing") { tabs().array("tabs").objects().first { it.getLong("id") == tabs().getLong("selected") }.getBoolean("modified") }; ready() }
    private fun closed(id: Long, count: Int) {
        waitFor("approved drawing removed") { id !in ids() && ids().size == count && ui { !host.drawingTabs.switching } }
        // Include later composition/publication frames, where the previous
        // implementation reported cancellation after already removing the tab.
        repeat(15) { SystemClock.sleep(20); healthy() }
        ready()
    }
    private fun finished() {
        waitFor("Activity destroyed after workspace flush") { ui { activity.isFinishing && activity.isDestroyed } }
        repeat(15) { SystemClock.sleep(20); healthy() }
        assertEquals(0L, ui { host.drawingTabs.selected })
        assertTrue(ui { host.drawingTabs.rows.isEmpty() })
        assertFalse(ui { host.workspaceManager?.optBoolean("dirty") == true })
        // onStop/onCleared release the workspace lease on the native owner.
        // Its final drain does not publish to the destroyed Compose view, whose
        // cached busy bit is therefore not a shutdown-completion signal.
    }
    private fun dismissDrawersWithBack() {
        fun count() = ui {
            val snapshot = host.snapshot!!
            val customization = snapshot.getJSONObject("state").getJSONObject("customization")
            customization.array("column_drawers").length() +
                (if (customization.objectOrNull("drawer") == null) 0 else 1) +
                snapshot.getJSONObject("layout").array("collapsed").objects().count { it.objectOrNull("open") != null }
        }
        val drawings = ids()
        while (count() > 0) {
            val before = count()
            instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
            waitFor("Back dismisses drawer before closing drawing") { count() < before }
            SystemClock.sleep(300)
            assertEquals(drawings, ids())
        }
    }
    private fun openFixture(name: String): Pair<Long, File> {
        ready()
        val file = File(device.root, name)
        val selected = tabs().getLong("selected")
        val capture = native { Native.projectRecoveryFor(it, selected) }
        try { Native.projectPublish(capture, file.absolutePath) } finally { Native.projectFree(capture) }
        val count = ids().size
        ui { assertTrue(host.documents.openUris(listOf(Uri.fromFile(file)))) }
        waitFor("fixture opened") { ids().size == count + 1 }; ready()
        return tabs().getLong("selected") to file
    }
    @Before fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { activity = it; host = it.host; it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        waitFor("production startup", 120_000) { ui { host.snapshot?.optBoolean("brush_ready") == true && host.workspaceManager?.optBoolean("ready") == true && host.snapshot?.getJSONObject("state")?.array("commands")?.objects()?.any { c -> c.optString("id") == "new_document" && c.optBoolean("enabled") } == true } }
        ready()
    }
    @After fun cleanup() {
        if (::scenario.isInitialized) scenario.close()
    }
    @Test fun closeButtonsAndFileMenuUseRealFrameTiming() {
        val first = tabs().getLong("selected"); val second = create()
        tap(tag("drawing-close-$second")); closed(second, 1)
        val third = create()
        tap(tag("drawing-close-$first")); closed(first, 1)
        assertEquals(third, tabs().getLong("selected"))
        openFileMenu(); tap(text("Close")); finished()
    }
    @Test fun dirtyCloseCancelAndActualSavePickerCancelPreserveDrawing() {
        val first = tabs().getLong("selected"); dirty(); val second = create()
        tap(tag("drawing-close-$first")); tap(tag("document-close-cancel")); ready()
        assertEquals(listOf(first, second), ids()); assertEquals(first, tabs().getLong("selected"))
        tap(tag("drawing-close-$first")); tap(tag("document-close-save"))
        waitFor("SAF save picker") { instrumentation.uiAutomation.rootInActiveWindow?.packageName?.toString()?.contains("documentsui") == true }
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        waitFor("save picker cancelled") { ui { host.documents.picker == null && !host.documents.working && activity.hasWindowFocus() } }; ready()
        assertEquals(listOf(first, second), ids()); assertTrue(tabs().array("tabs").getJSONObject(0).getBoolean("modified"))
        tap(tag("drawing-close-$first")); tap(tag("document-close-discard")); closed(first, 1)
        dismissDrawersWithBack()
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); finished()
    }
    @Test fun saveAndFailedSaveCloseOnlyTheirApprovedOwner() {
        val (saved, file) = openFixture("saved.capy"); val before = file.readBytes(); dirty()
        tap(tag("drawing-close-$saved")); tap(tag("document-close-save")); closed(saved, 1)
        assertFalse("Save wrote the changed drawing", before.contentEquals(file.readBytes()))
        val (failed, unavailable) = openFixture("unavailable.capy"); dirty()
        assertTrue(unavailable.delete()); assertTrue(unavailable.mkdir())
        tap(tag("drawing-close-$failed")); tap(tag("document-close-save"))
        waitFor("provider write failure", checkErrors = false) { ui { !host.documents.working && (host.actionError != null || host.snapshot?.getJSONObject("state")?.optString("host_error").let { !it.isNullOrEmpty() && it != "null" }) } }
        assertTrue(failed in ids()); assertEquals(failed, tabs().getLong("selected")); assertTrue(tabs().array("tabs").getJSONObject(1).getBoolean("modified"))
        tap(text("OK"), checkErrors = false)
        tap(tag("drawing-close-$failed"), checkErrors = false); tap(tag("document-close-cancel"), checkErrors = false)
        assertEquals(2, ids().size)
    }
    @Test fun approvedCloseSurvivesActivityRecreationDuringDrain() {
        val first = tabs().getLong("selected"); create()
        for (final in listOf(false, true)) {
            dirty(); val before = ids()
            if (final) { openFileMenu(); tap(text("Close")) }
            else tap(tag("drawing-close-${tabs().getLong("selected")}"))
            waitFor("close prompt") { point(tag("document-close-discard")) != null }
            val gate = CompletableDeferred<Unit>(); val control = Native.captureControl()
            ui { host.drawingTabs.registerInspection(control, host.viewModelScope.launch { gate.await() }) }
            try {
                tap(tag("document-close-discard"))
                waitFor("approved close draining") { ui { host.drawingTabs.switching } }
                if (!final) { tap(tag("drawing-close-$first")); assertEquals("A second close cannot steal the pending owner", before, ids()) }
                val owner = host
                scenario.recreate(); scenario.onActivity { activity = it; host = it.host }
                assertSame(owner, host)
                gate.complete(Unit)
                if (final) finished() else {
                    waitFor("close completed after recreation") { ids() == listOf(first) && ui { !host.drawingTabs.switching } }; ready()
                }
            } finally { gate.complete(Unit); ui { host.drawingTabs.releaseInspection(control) }; Native.captureFree(control) }
        }
    }

    @Test fun windowCloseStopsAtCancelAndContinuesOnlyApprovedDrawings() {
        val first = tabs().getLong("selected"); dirty(); val second = create(); dirty(); val third = create()
        ui { host.drawingTabs.closeWindow() }
        waitFor("window close reaches dirty neighbour") { third !in ids() && point(tag("document-close-cancel")) != null }
        tap(tag("document-close-cancel")); ready()
        assertEquals(listOf(first, second), ids()); assertFalse(ui { host.drawingTabs.closingWindow })
        ui { host.drawingTabs.closeWindow() }; tap(tag("document-close-discard"))
        waitFor("next dirty drawing gets its own decision") { ids() == listOf(first) && point(tag("document-close-cancel")) != null }
        tap(tag("document-close-cancel")); ready()
        assertTrue(tabs().array("tabs").getJSONObject(0).getBoolean("modified"))
        ui { host.drawingTabs.closeWindow() }; tap(tag("document-close-discard")); finished()
    }
}
