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
    private fun closeTab(id:Long,checkErrors:Boolean=true) {
        if(point(tag("drawing-close-$id")) == null)tap(tag("drawing-selector-button"),checkErrors)
        tap(tag("drawing-close-$id"),checkErrors)
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
        host.writeDrawingCopy(file)
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
        androidx.test.platform.app.InstrumentationRegistry.getArguments().getString("theme")?.let {theme ->
            require(theme in listOf("light","dark"))
            ui {host.dispatch(obj("type" to "set_theme","theme" to theme))}
            waitFor("requested theme") {ui {host.snapshot?.objectOrNull("state")?.optString("theme") == theme}}
        }
    }
    @After fun cleanup() {
        if (::scenario.isInitialized) scenario.close()
    }
    @Test fun handToolPreservesDrawingCycleKeys() {
        val first = tabs().getLong("selected")
        val second = create()
        ui { host.invoke("hand"); activity.window.decorView.requestFocus() }
        waitFor("Hand selected") { ui { host.snapshot?.objectOrNull("state")?.objectOrNull("layer_tools")?.optString("tool") == "hand" } }
        fun cycle(code: Int, modifiers: Int, expected: Long) {
            val now = SystemClock.uptimeMillis()
            for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
                instrumentation.sendKeySync(KeyEvent(now, SystemClock.uptimeMillis(), action, code, 0,
                    modifiers, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
            }
            waitFor("Hand drawing cycle to $expected") { tabs().getLong("selected") == expected }
            ready()
        }
        val control = KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON
        val alt = KeyEvent.META_ALT_ON or KeyEvent.META_ALT_LEFT_ON
        cycle(KeyEvent.KEYCODE_PAGE_UP, control, first)
        cycle(KeyEvent.KEYCODE_PAGE_DOWN, control, second)
        cycle(KeyEvent.KEYCODE_PAGE_UP, alt, first)
        cycle(KeyEvent.KEYCODE_PAGE_DOWN, alt, second)
    }
    @Test fun closeButtonsAndFileMenuUseRealFrameTiming() {
        val first = tabs().getLong("selected"); val second = create()
        closeTab(second); closed(second, 1)
        val third = create()
        closeTab(first); closed(first, 1)
        assertEquals(third, tabs().getLong("selected"))
        openFileMenu(); tap(text("Close")); finished()
    }
    @Test fun dirtyCloseCancelAndActualSavePickerCancelPreserveDrawing() {
        val first = tabs().getLong("selected"); dirty(); val second = create()
        closeTab(first); tap(tag("document-close-cancel")); ready()
        assertEquals(listOf(first, second), ids()); assertEquals(first, tabs().getLong("selected"))
        closeTab(first); tap(tag("document-close-save"))
        waitFor("SAF save picker") { instrumentation.uiAutomation.rootInActiveWindow?.packageName?.toString()?.contains("documentsui") == true }
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        waitFor("save picker cancelled") { ui { host.documents.picker == null && !host.documents.working && activity.hasWindowFocus() } }; ready()
        assertEquals(listOf(first, second), ids()); assertTrue(tabs().array("tabs").getJSONObject(0).getBoolean("modified"))
        closeTab(first); tap(tag("document-close-discard")); closed(first, 1)
        dismissDrawersWithBack()
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK); finished()
    }
    @Test fun saveAndFailedSaveCloseOnlyTheirApprovedOwner() {
        val (saved, file) = openFixture("saved.capy"); val before = file.readBytes(); dirty()
        closeTab(saved); tap(tag("document-close-save")); closed(saved, 1)
        assertFalse("Save wrote the changed drawing", before.contentEquals(file.readBytes()))
        val (failed, unavailable) = openFixture("unavailable.capy"); dirty()
        assertTrue(unavailable.delete()); assertTrue(unavailable.mkdir())
        closeTab(failed); tap(tag("document-close-save"))
        waitFor("provider write failure", checkErrors = false) { ui { !host.documents.working && (host.actionError != null || host.snapshot?.getJSONObject("state")?.optString("host_error").let { !it.isNullOrEmpty() && it != "null" }) } }
        assertTrue(failed in ids()); assertEquals(failed, tabs().getLong("selected")); assertTrue(tabs().array("tabs").getJSONObject(1).getBoolean("modified"))
        tap(text("OK"), checkErrors = false)
        closeTab(failed,checkErrors=false); tap(tag("document-close-cancel"), checkErrors = false)
        assertEquals(2, ids().size)
    }
    @Test fun saveRefusesProviderReplacementWhenPrivateCheckpointFails() {
        waitFor("session ready",120_000) {ui {host.recovery.ready}}
        val (saved,file) = openFixture("checkpoint-barrier.capy")
        val before = file.readBytes()
        ui {host.recovery.background()}
        assertTrue(runBlocking {kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {host.recovery.flush()}})
        SystemClock.sleep(500)
        val storeDirectories = device.recovery.walkTopDown().filter {it.name == "session.json"}.flatMap {index ->
            JSONObject(Native.sessionManifestRead(index.absolutePath)).array("drawings").objects().map {File(index.parentFile,it.getString("key"))}.asSequence()
        }.toList()
        assertTrue(storeDirectories.isNotEmpty())
        try {
            for(directory in storeDirectories)assertTrue(directory.setWritable(false,false))
            ui {host.dispatch(obj("type" to "invoke","command" to "add_layer"))}
            waitFor("new unsaved layer") {tabs().array("tabs").objects().first {it.getLong("id") == saved}.getBoolean("modified")}
            ui {host.dispatch(obj("type" to "invoke","command" to "save_document"))}
            waitFor("private checkpoint blocks provider write",checkErrors=false) {ui {!host.documents.working&&host.actionError != null}}
            assertArrayEquals("Original remains intact when private durability fails",before,file.readBytes())
            assertTrue(saved in ids());assertEquals(saved,tabs().getLong("selected"))
            assertTrue(tabs().array("tabs").objects().first {it.getLong("id") == saved}.getBoolean("modified"))
            tap(text("OK"),checkErrors=false)
        } finally {
            for(directory in storeDirectories)assertTrue(directory.setWritable(true,true))
            ui {host.clearActionError();host.recovery.foreground()}
        }
    }

    @Test fun restoredSavedDrawingsRequireCloseDecisionWhenOriginalChangesOrDisappears() {
        waitFor("session ready",120_000) {ui {host.recovery.ready}}
        val blank = tabs().getLong("selected")
        val (intact, intactFile) = openFixture("restart-intact.capy")
        val (changed, changedFile) = openFixture("restart-changed.capy")
        val (missing, missingFile) = openFixture("restart-missing.capy")
        val intactBytes = intactFile.readBytes()
        val changedBytes = byteArrayOf(1,2,3)
        assertEquals(listOf(false,false,false,false),tabs().array("tabs").objects().map {it.getBoolean("modified")})
        ui {host.drawingTabs.closeWindow()};finished();scenario.close()
        changedFile.writeBytes(changedBytes);assertTrue(missingFile.delete())
        launch()
        waitFor("restored original checks",120_000) {ui {host.recovery.ready&&!host.recovery.working}}
        ready()
        assertEquals(listOf(blank,intact,changed,missing),ids())
        assertEquals(listOf(false,false,true,true),tabs().array("tabs").objects().map {it.getBoolean("modified")})
        assertNull(ui {host.recovery.candidate});assertNull(point(tag("recover-drawing")))
        closeTab(intact);closed(intact,3)
        assertNull(point(tag("document-close-discard")))
        assertTrue(intactBytes.contentEquals(intactFile.readBytes()))
        for(id in listOf(changed,missing)) {
            closeTab(id)
            waitFor("saved original needs explicit close decision") {point(tag("document-close-save")) != null&&point(tag("document-close-discard")) != null&&point(tag("document-close-cancel")) != null}
            tap(tag("document-close-cancel"));ready();assertTrue(id in ids())
            assertTrue(tabs().array("tabs").objects().first {it.getLong("id") == id}.getBoolean("modified"))
        }
        assertTrue(changedBytes.contentEquals(changedFile.readBytes()));assertFalse(missingFile.exists())
        closeTab(changed);tap(tag("document-close-discard"));closed(changed,2)
        closeTab(missing);tap(tag("document-close-discard"));closed(missing,1)
        assertEquals(listOf(blank),ids())
    }

    @Test fun approvedCloseSurvivesActivityRecreationDuringDrain() {
        val first = tabs().getLong("selected"); create()
        for (final in listOf(false, true)) {
            dirty(); val before = ids()
            if (final) { openFileMenu(); tap(text("Close")) }
            else closeTab(tabs().getLong("selected"))
            waitFor("close prompt") { point(tag("document-close-discard")) != null }
            val gate = CompletableDeferred<Unit>(); val control = Native.captureControl()
            ui { host.drawingTabs.registerInspection(control, host.viewModelScope.launch { gate.await() }) }
            try {
                tap(tag("document-close-discard"))
                waitFor("approved close draining") { ui { host.drawingTabs.switching } }
                if (!final) { ui {host.drawingTabs.select(first,true)}; assertEquals("A second close cannot steal the pending owner", before, ids()) }
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

    @Test fun windowQuitPreservesCleanAndModifiedDrawingsWithoutPrompt() {
        waitFor("session ready",120_000) {ui {host.recovery.ready}}
        val first = tabs().getLong("selected"); dirty(); val second = create(); dirty(); val third = create()
        val expected = ids()
        ui {host.drawingTabs.closeWindow()}
        waitFor("Activity destroyed after session and workspace flush") {ui {activity.isFinishing&&activity.isDestroyed}}
        scenario.close();scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity {activity=it;host=it.host}
        waitFor("automatic session restoration",120_000) {ui {host.recovery.ready&&!host.recovery.working}}
        ready()
        assertEquals(listOf(first,second,third),expected);assertEquals(expected,ids())
        assertEquals(third,tabs().getLong("selected"))
        assertEquals(listOf(true,true,false),tabs().array("tabs").objects().map {it.getBoolean("modified")})
        assertNull(ui {host.recovery.candidate})
        assertNull(point(tag("document-close-discard")));assertNull(point(tag("recover-drawing")))
    }
}
