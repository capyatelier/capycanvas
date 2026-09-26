package art.capycanvas

import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.inspector.WindowInspector
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.rules.ExternalResource
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

internal val instrumentation get() = InstrumentationRegistry.getInstrumentation()

class CapyDeviceRule(private val nativeFileJobs: Boolean = false) : ExternalResource() {
    lateinit var root: File
        private set
    val recovery get() = File(root, "recovery")
    private lateinit var userPreferences: Map<String, *>
    private var rotation: Pair<Int, Boolean>? = null
    fun landscape(scenario: ActivityScenario<MainActivity>) {
        var portrait = false
        scenario.onActivity {
            if (rotation == null) rotation = it.window.decorView.display.rotation to (android.provider.Settings.System.getInt(it.contentResolver, android.provider.Settings.System.ACCELEROMETER_ROTATION, 1) != 0)
            portrait = it.resources.configuration.orientation != android.content.res.Configuration.ORIENTATION_LANDSCAPE
        }
        if (!portrait) return
        assertTrue(instrumentation.uiAutomation.setRotation(if (rotation!!.first % 2 == 0) android.app.UiAutomation.ROTATION_FREEZE_90 else android.app.UiAutomation.ROTATION_FREEZE_0))
        val until = SystemClock.uptimeMillis() + 10_000
        while (portrait && SystemClock.uptimeMillis() < until) {
            SystemClock.sleep(50)
            scenario.onActivity { portrait = it.resources.configuration.orientation != android.content.res.Configuration.ORIENTATION_LANDSCAPE }
        }
        assertFalse("The fixtures use the landscape tablet layout", portrait)
    }
    override fun before() {
        val context = instrumentation.targetContext
        userPreferences = context.getSharedPreferences("capy-canvas", 0).all
        root = File(context.cacheDir, "capy-tests/${UUID.randomUUID()}").apply { mkdirs() }
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        RecoveryController.directoryForTest = recovery
        ColorPreferencesStore.directoryForTest = File(root, "colors")
        DocumentController.nativeFileJobsForTest = nativeFileJobs
    }
    override fun after() {
        val context = instrumentation.targetContext
        rotation?.let { (previous, auto) -> instrumentation.uiAutomation.setRotation(if (auto) android.app.UiAutomation.ROTATION_UNFREEZE else previous) }
        rotation = null
        context.deleteSharedPreferences(CanvasHost.preferencesName)
        CanvasHost.workspaceDirectoryForTest = null
        RecoveryController.directoryForTest = null
        ColorPreferencesStore.directoryForTest = null
        DocumentController.nativeFileJobsForTest = false
        root.deleteRecursively()
        assertEquals("User preferences are preserved", userPreferences, context.getSharedPreferences("capy-canvas", 0).all)
    }
}

fun ActivityScenario<MainActivity>.activity(): MainActivity {
    var result: MainActivity? = null
    onActivity { result = it }
    return result!!
}

fun launchCapy(timeout: Long = 60_000): ActivityScenario<MainActivity> =
    ActivityScenario.launch(MainActivity::class.java).also { it.activity().host.awaitReady(timeout) }

fun CanvasHost.awaitReady(timeout: Long = 60_000) = awaitMain("brush and workspace ready", timeout, { "$workspaceManager" }) {
    snapshot?.optBoolean("brush_ready") == true && workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true
}

fun CanvasHost.awaitMain(label: String, timeout: Long = 10_000, diagnostics: () -> String = { "" }, condition: () -> Boolean) {
    val until = SystemClock.uptimeMillis() + timeout
    do {
        var ready = false
        instrumentation.runOnMainSync { assertNull(failure); assertNull(actionError); ready = condition() }
        if (ready) return
        SystemClock.sleep(16)
    } while (SystemClock.uptimeMillis() < until)
    fail("Timed out: $label" + diagnostics().let { if (it.isEmpty()) "" else "; $it" })
}

fun CanvasHost.drain(action: JSONObject? = null, seconds: Long = 15) {
    val done = CountDownLatch(1)
    instrumentation.runOnMainSync { action?.let { dispatch(it) }; query(obj("type" to "catalog")) { done.countDown() } }
    assertTrue("Native UI publication", done.await(seconds, TimeUnit.SECONDS))
}

fun CanvasHost.workspaceCapture(): String {
    var result = ""
    val done = CountDownLatch(1)
    instrumentation.runOnMainSync {
        CoroutineScope(Dispatchers.Main).launch {
            result = withNative { Native.workspace(it, obj("type" to "capture").toString()) }; done.countDown()
        }
    }
    assertTrue(done.await(10, TimeUnit.SECONDS))
    return result
}

fun screenshot(path: String, inspect: ((Bitmap) -> Unit)? = null) {
    val file = File(instrumentation.targetContext.getExternalFilesDir(null), path).apply { parentFile!!.mkdirs() }
    val bitmap = instrumentation.uiAutomation.takeScreenshot() ?: return
    try {
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        inspect?.let { check -> bitmap.copy(Bitmap.Config.ARGB_8888, false).let { pixels -> try { check(pixels) } finally { pixels.recycle() } } }
    } finally { bitmap.recycle() }
}

fun shell(command: String) {
    ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }
}

fun wakeDevice() {
    shell("input keyevent KEYCODE_WAKEUP")
    shell("wm dismiss-keyguard")
}

fun pressKey(code: Int, meta: Int = 0) {
    val now = SystemClock.uptimeMillis()
    for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
        instrumentation.sendKeySync(KeyEvent(now, now, action, code, 0, meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
}

fun toolSource(tool: Int) = when (tool) {
    MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS
    MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE
    else -> InputDevice.SOURCE_TOUCHSCREEN
}

fun motion(tool: Int, action: Int, point: Offset, downAt: Long, button: Int = MotionEvent.BUTTON_PRIMARY, meta: Int = 0): MotionEvent {
    val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action != MotionEvent.ACTION_UP && action != MotionEvent.ACTION_CANCEL) button else 0
    return MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1,
        arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
        arrayOf(MotionEvent.PointerCoords().apply { x = point.x; y = point.y; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f }),
        meta, buttons, 1f, 1f, 0, 0, toolSource(tool), 0)
}

fun tabs(id: Int, vararg panels: String, style: String = "icon") = obj("kind" to "tabs", "id" to id,
    "panels" to JSONArray(panels.toList()), "active" to panels[0], "tab_style" to style)

inline fun <reified T> View.descendant(): T? {
    val pending = ArrayDeque(listOf(this))
    while (pending.isNotEmpty()) {
        val next = pending.removeFirst()
        if (next is T) return next
        if (next is ViewGroup) for (i in 0 until next.childCount) pending.add(next.getChildAt(i))
    }
    return null
}

fun semanticsRoots(): List<ViewRootForTest> = WindowInspector.getGlobalWindowViews().mapNotNull { it.descendant<ViewRootForTest>() }

fun SemanticsNode.find(match: (SemanticsNode) -> Boolean): SemanticsNode? =
    if (match(this)) this else children.firstNotNullOfOrNull { it.find(match) }

fun ViewRootForTest.find(match: (SemanticsNode) -> Boolean) = semanticsOwner.unmergedRootSemanticsNode.find(match)

fun hasTag(tag: String): (SemanticsNode) -> Boolean = { it.config.getOrNull(SemanticsProperties.TestTag) == tag }

fun hasLabel(text: String): (SemanticsNode) -> Boolean = { node -> node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == text } == true }

fun findNode(match: (SemanticsNode) -> Boolean, first: ViewRootForTest? = null): Pair<ViewRootForTest, SemanticsNode>? =
    (listOfNotNull(first) + semanticsRoots().filter { it !== first }).firstNotNullOfOrNull { root -> root.find(match)?.let { root to it } }

fun findTag(tag: String, first: ViewRootForTest? = null) = findNode(hasTag(tag), first)
