package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.ViewGroup
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.abs

class AndroidPaletteTest {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val arguments get() = InstrumentationRegistry.getArguments()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private lateinit var owner: ViewRootForTest
    private var density = 1f
    private var downAt = 0L
    private var contact = false
    private var point = Offset.Zero
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var mouseButton = MotionEvent.BUTTON_PRIMARY
    private lateinit var root: File
    private val tools = listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_MOUSE)
    private val output get() = File(scenarioActivity().getExternalFilesDir(null), "validation/palettes").apply { mkdirs() }

    private fun scenarioActivity(): MainActivity { var result: MainActivity? = null; scenario.onActivity { result = it }; return result!! }
    private inline fun <reified T> findView(view: View): T? {
        val pending = ArrayDeque<View>(listOf(view))
        while (pending.isNotEmpty()) {
            val next = pending.removeFirst()
            if (next is T) return next
            if (next is ViewGroup) for (i in 0 until next.childCount) pending.add(next.getChildAt(i))
        }
        return null
    }
    private fun find(node: SemanticsNode, test: (SemanticsNode) -> Boolean): SemanticsNode? =
        if (test(node)) node else node.children.firstNotNullOfOrNull { find(it, test) }
    private fun tagged(test: (SemanticsNode) -> Boolean): Pair<ViewRootForTest, SemanticsNode>? {
        find(owner.semanticsOwner.unmergedRootSemanticsNode, test)?.let { return owner to it }
        return android.view.inspector.WindowInspector.getGlobalWindowViews().firstNotNullOfOrNull { view ->
            findView<ViewRootForTest>(view)?.takeIf { it !== owner }?.let { r -> find(r.semanticsOwner.unmergedRootSemanticsNode, test)?.let { r to it } }
        }
    }
    private fun tag(tag: String): (SemanticsNode) -> Boolean = { it.config.getOrNull(SemanticsProperties.TestTag) == tag }
    private fun text(text: String): (SemanticsNode) -> Boolean = { n -> n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == text } == true }
    private fun main(block: () -> Unit) = if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) block() else instrumentation.runOnMainSync(block)
    private fun exists(test: (SemanticsNode) -> Boolean): Boolean { var found = false; main { found = tagged(test) != null }; return found }
    private fun exists(tag: String) = exists(tag(tag))
    private fun bounds(test: (SemanticsNode) -> Boolean, label: String): Rect {
        var result: Rect? = null
        main {
            tagged(test)?.let { (r, node) ->
                val origin = IntArray(2); val base = IntArray(2)
                r.view.getLocationOnScreen(origin); owner.view.getLocationOnScreen(base)
                result = node.boundsInRoot.translate(Offset((origin[0] - base[0]).toFloat(), (origin[1] - base[1]).toFloat()))
            }
        }
        return checkNotNull(result) { "Missing $label" }
    }
    private fun bounds(tag: String) = bounds(tag(tag), tag)
    private fun node(tag: String): SemanticsNode { var result: SemanticsNode? = null; main { result = tagged(tag(tag))?.second }; return checkNotNull(result) { "Missing $tag" } }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun library() = state().getJSONObject("colors").getJSONObject("library")
    private fun view() = host.panelContent!!.getJSONObject("palette_panel")
    private fun order() = view().array("swatches").objects().map { it.getLong("id") }
    private fun popupCount(): Int {
        var result = 0
        main {
            result = android.view.inspector.WindowInspector.getGlobalWindowViews().count { v ->
                findView<ViewRootForTest>(v)?.let { find(it.semanticsOwner.unmergedRootSemanticsNode, tag("workspace-menu")) != null } == true
            }
        }
        return result
    }
    private fun waitFor(label: String, timeout: Long = 10_000, condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + timeout
        do {
            var ready = false
            instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError); ready = condition() }
            if (ready) return
            SystemClock.sleep(16)
        } while (SystemClock.uptimeMillis() < deadline)
        runCatching { capture("timeout-${label.replace(Regex("[^A-Za-z0-9]+"), "-")}") }
        fail("Timed out: $label")
    }
    private fun settle() { SystemClock.sleep(180); instrumentation.runOnMainSync { assertNull(host.failure); assertNull(host.actionError) } }
    private fun action(value: JSONObject) {
        val done = CountDownLatch(1)
        instrumentation.runOnMainSync { host.dispatch(value); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS)); settle()
    }
    private fun library(action: JSONObject): String? {
        val done = CountDownLatch(1); var error: String? = null
        instrumentation.runOnMainSync { host.paletteAction(action) { error = it; done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS)); settle()
        return error
    }
    private fun event(action: Int, next: Offset = point, metaState: Int = 0) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); contact = true }
        val origin = IntArray(2)
        instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool })
        val coords = arrayOf(MotionEvent.PointerCoords().apply { x = next.x + origin[0]; y = next.y + origin[1]; pressure = if (action == MotionEvent.ACTION_UP) 0f else .7f })
        val source = when (tool) { MotionEvent.TOOL_TYPE_STYLUS -> InputDevice.SOURCE_STYLUS; MotionEvent.TOOL_TYPE_MOUSE -> InputDevice.SOURCE_MOUSE; else -> InputDevice.SOURCE_TOUCHSCREEN }
        val buttons = if (tool == MotionEvent.TOOL_TYPE_MOUSE && action !in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) mouseButton else 0
        val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), action, 1, properties, coords, metaState, buttons, 1f, 1f, 0, 0, source, 0)
        try { assertTrue("System accepts ${MotionEvent.actionToString(action)}", instrumentation.uiAutomation.injectInputEvent(motion, true)) }
        finally { motion.recycle() }
        if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) contact = false
    }
    private fun glide(from: Offset, to: Offset, steps: Int = 12) {
        for (i in 1..steps) { event(MotionEvent.ACTION_MOVE, from + (to - from) * (i / steps.toFloat())); SystemClock.sleep(12) }
    }
    private fun tap(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(40); event(MotionEvent.ACTION_UP); settle() }
    private fun secondary(at: Offset) {
        tool = MotionEvent.TOOL_TYPE_MOUSE; mouseButton = MotionEvent.BUTTON_SECONDARY
        try { tap(at) } finally { mouseButton = MotionEvent.BUTTON_PRIMARY }
    }
    private fun hold(at: Offset) { event(MotionEvent.ACTION_DOWN, at); SystemClock.sleep(ViewConfiguration.getLongPressTimeout() + 250L) }
    private fun capture(name: String) {
        settle()
        val shot = instrumentation.uiAutomation.takeScreenshot()
        File(output, "$name.png").outputStream().use { shot.compress(Bitmap.CompressFormat.PNG, 100, it) }
        shot.recycle()
    }
    private fun key(code: Int, meta: Int = 0) {
        val now = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
            instrumentation.sendKeySync(KeyEvent(now, now, action, code, 0, meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD))
        settle()
    }
    private fun setText(tag: String, value: String) {
        val target = node(tag)
        instrumentation.runOnMainSync {
            fun editable(n: SemanticsNode): SemanticsNode? = if (n.config.getOrNull(SemanticsActions.SetText) != null) n else n.children.firstNotNullOfOrNull(::editable)
            assertTrue(checkNotNull(editable(target)).config[SemanticsActions.SetText].action!!.invoke(AnnotatedString(value)))
        }
        instrumentation.runOnMainSync {
            owner.view.context.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(owner.view.windowToken, 0)
        }
        waitFor("keyboard hidden") { owner.view.rootWindowInsets?.isVisible(android.view.WindowInsets.Type.ime()) != true }
        SystemClock.sleep(400)
    }
    private fun switchWorkspace(id: String) {
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to id)) }
        waitFor("workspace $id", 30_000) { host.workspaceManager?.optString("id") == id && host.workspaceManager?.optBoolean("busy") == false }
        settle()
    }
    private fun group(panel: String) = host.snapshot!!.getJSONObject("layout").array("groups").objects().first { panel in it.array("panels").values() }
    private fun showPalettes() {
        action(obj("type" to "select_panel_tab", "group" to group("palettes").getInt("id"), "panel" to "palettes"))
        waitFor("palette panel") { exists("palette-panel") }
    }
    private fun center(tag: String) = bounds(tag).center
    private fun closeMenu() {
        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
        waitFor("menu closes and focus returns") { popupCount() == 0 && owner.view.hasWindowFocus() }
        settle()
    }
    private fun swatch(index: Int) = "palette-swatch-${order()[index]}"

    @Before fun ready() {
        root = File(instrumentation.targetContext.cacheDir, "palette-tests/${java.util.UUID.randomUUID()}")
        CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
        RecoveryController.directoryForTest = File(root, "recovery")
        ColorPreferencesStore.directoryForTest = File(root, "color-preferences")
        launch()
        switchWorkspace("builtin:workspace:illustrator")
    }
    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { host = it.host; owner = findView<ViewRootForTest>(it.window.decorView)!!; density = it.resources.displayMetrics.density }
        waitFor("brush ready", 60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        waitFor("workspace ready", 60_000) { host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
        action(obj("type" to "close_settings"))
        if (state().getJSONObject("workspace").optBoolean("zen_mode")) action(obj("type" to "invoke", "command" to "zen_mode"))
    }
    @After fun cleanup() {
        try { if (contact) event(MotionEvent.ACTION_CANCEL) }
        finally {
            if (::scenario.isInitialized) scenario.close()
            CanvasHost.workspaceDirectoryForTest = null
            RecoveryController.directoryForTest = null
            ColorPreferencesStore.directoryForTest = null
            root.deleteRecursively()
        }
    }

    @Test fun defaultsPlacePalettesAfterColorAndInSketchDrawer() {
        for (id in listOf("builtin:workspace:illustrator", "builtin:workspace:photographer")) {
            switchWorkspace(id)
            fun fitted() = group("color").getJSONObject("bounds").let { b -> listOf("x", "y", "width", "height").map { b.getDouble(it) } }
            fun assertFitted(label: String, expected: List<Double>) = expected.zip(fitted()).forEach { (e, a) -> assertEquals(label, e, a, .5) }
            val fitted = fitted()
            showPalettes()
            assertFitted("Choosing Palettes keeps the fitted group", fitted)
            assertTrue(exists("palette-history") && exists("palette-add-color") && exists("palette-chooser"))
            assertTrue("minimum six columns", bounds("palette-swatches").width >= (6 * 44 - 4) * density - 1)
            capture("${id.substringAfterLast(':')}-palettes")
            action(obj("type" to "select_panel_tab", "group" to group("color").getInt("id"), "panel" to "color"))
            assertFitted("Color keeps the fitted group", fitted)
        }
        switchWorkspace("builtin:workspace:painter")
        val color = host.snapshot!!.getJSONObject("header").getJSONObject("model").array("zones").values().flatMap { (it as JSONArray).objects() }
            .first { it.getJSONObject("item").objectOrNull("control")?.optString("kind") == "color" }.getInt("id")
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap(center("header-control-$color"))
        waitFor("Sketch color drawer") { state().getJSONObject("customization").objectOrNull("drawer") != null }
        assertEquals("[[\"color\",\"palettes\"]]", state().getJSONObject("customization").getJSONObject("drawer").array("columns").toString())
        waitFor("drawer palettes") { exists("palette-panel") }
        capture("sketch-drawer")
    }

    @Test fun historySavingAndExpansionFollowArtworkUse() {
        showPalettes()
        assertEquals(0, view().array("history").length())
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap(center(swatch(0)))
        waitFor("swatch selects color") { view().array("swatches").objects()[0].getBoolean("current") }
        assertEquals("choosing a swatch is not usage", 0, view().array("history").length())
        val area = state().getJSONObject("camera").array("work_area")
        val canvas = Offset((area.getDouble(0) + area.getDouble(2) * .4).toFloat(), (area.getDouble(1) + area.getDouble(3) * .5).toFloat())
        tool = MotionEvent.TOOL_TYPE_STYLUS
        event(MotionEvent.ACTION_DOWN, canvas); glide(canvas, canvas + Offset(160f, 40f)); event(MotionEvent.ACTION_UP)
        waitFor("stroke records history") { view().array("history").length() == 1 }
        assertEquals("history keeps the used definition", view().array("swatches").objects()[0].getJSONObject("color").toString(),
            view().array("history").objects()[0].getJSONObject("color").toString())
        tool = MotionEvent.TOOL_TYPE_FINGER
        val footer = bounds("palette-footer")
        tap(center("palette-history-expand"))
        waitFor("history expands") { exists("palette-history-expanded") }
        assertEquals("footer is stable", footer, bounds("palette-footer"))
        capture("history-expanded")
        tap(center("palette-history-collapse"))
        waitFor("history collapses") { !exists("palette-history-expanded") }
        assertEquals(footer, bounds("palette-footer"))
        val count = order().size
        tap(center("palette-add-color"))
        waitFor("add stores current color") { order().size == count + 1 }
        waitFor("new swatch selected") { node("palette-swatch-${order().last()}").config.getOrNull(SemanticsProperties.Selected) == true }
        assertEquals("saving is not usage", 1, view().array("history").length())
    }

    @Test fun inlineNamesValidateDuplicates() {
        showPalettes()
        tool = MotionEvent.TOOL_TYPE_FINGER
        val names = view().array("swatches").objects().map { it.getString("name") }
        tap(center(swatch(0)))
        tap(center("palette-color-name"))
        waitFor("inline editor") { exists("palette-name-editor") }
        setText("palette-name-editor", names[1].uppercase())
        instrumentation.runOnMainSync { node("palette-name-editor").config[SemanticsActions.OnImeAction].action!!.invoke() }
        waitFor("duplicate rejected") { exists("palette-message") && exists("palette-name-editor") }
        assertEquals(names[0], view().array("swatches").objects()[0].getString("name"))
        setText("palette-name-editor", "  Unique   swatch ")
        instrumentation.runOnMainSync { node("palette-name-editor").config[SemanticsActions.OnImeAction].action!!.invoke() }
        waitFor("rename commits") { view().array("swatches").objects()[0].getString("name") == "Unique swatch" }
        assertFalse(exists("palette-name-editor"))
    }

    @Test fun chooserSearchesSelectsAndUsesSharedMenus() {
        showPalettes()
        tool = MotionEvent.TOOL_TYPE_FINGER
        val footer = bounds("palette-footer")
        tap(center("palette-chooser"))
        waitFor("chooser") { exists("palette-browser") }
        assertEquals(footer, bounds("palette-footer"))
        setText("palette-search", "ink")
        val ink = library().array("palettes").objects().first { it.getString("name") == "Ink" }.getLong("id")
        waitFor("search filters") { exists("palette-choice-$ink") && library().array("palettes").objects().none { it.getLong("id") != ink && exists("palette-choice-${it.getLong("id")}") } }
        setText("palette-search", "zzzz")
        waitFor("empty search") { exists("palette-empty-search") }
        setText("palette-search", "ink")
        capture("chooser")
        secondary(center("palette-choice-$ink"))
        waitFor("mouse secondary menu") { popupCount() == 1 && exists(text("Rename Palette…")) }
        closeMenu()
        tool = MotionEvent.TOOL_TYPE_MOUSE
        hold(center("palette-choice-$ink")); event(MotionEvent.ACTION_UP); settle()
        assertEquals("mouse holds never open menus", 0, popupCount())
        waitFor("mouse hold release still chooses") { view().getLong("palette") == ink && !exists("palette-browser") }
        tap(center("palette-chooser")); waitFor("chooser") { exists("palette-browser") }
        setText("palette-search", "ink"); waitFor("ink row") { exists("palette-choice-$ink") }
        for (kind in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)) {
            tool = kind
            val row = center("palette-choice-$ink")
            hold(row)
            waitFor("held menu") { popupCount() == 1 }
            event(MotionEvent.ACTION_UP); settle()
            assertEquals("release keeps the held menu", 1, popupCount())
            closeMenu()
            hold(row); waitFor("held menu") { popupCount() == 1 }
            glide(row, row + Offset(0f, 60f * density), 6); event(MotionEvent.ACTION_UP); settle()
            assertEquals("dragging the held contact closes its menu", 0, popupCount())
        }
        tool = MotionEvent.TOOL_TYPE_FINGER
        val count = library().array("palettes").length()
        tap(center("palette-library-add"))
        waitFor("library menu") { exists(text("New Palette…")) }
        tap(bounds(text("New Palette…"), "New Palette…").center)
        waitFor("name dialog") { exists("palette-library-name") }
        setText("palette-library-name", "INK")
        waitFor("duplicate palette name is rejected") { node("palette-name-save").config.getOrNull(SemanticsProperties.Disabled) != null }
        setText("palette-library-name", "Test palette")
        waitFor("valid name") { node("palette-name-save").config.getOrNull(SemanticsProperties.Disabled) == null }
        tap(center("palette-name-save"))
        waitFor("palette created") { library().array("palettes").length() == count + 1 && view().getString("name") == "Test palette" }
    }

    @Test fun savedColorsReorderImmediatelyWithEveryDeviceAndUndoOnce() {
        showPalettes()
        for (kind in tools) {
            tool = kind
            val before = order()
            val from = center(swatch(0)); val to = center(swatch(2))
            event(MotionEvent.ACTION_DOWN, from)
            glide(from, to)
            waitFor("${kind} lifted swatch") { exists("palette-drag-preview") }
            SystemClock.sleep(200)
            assertEquals("hovering does not mutate the library", before, order())
            val lifted = bounds("palette-drag-preview")
            assertTrue("lifted swatch follows the contact", lifted.contains(to))
            if (kind == MotionEvent.TOOL_TYPE_STYLUS) capture("reorder-preview")
            event(MotionEvent.ACTION_UP)
            waitFor("${kind} drop commits once") { order() == listOf(before[1], before[2], before[0]) + before.drop(3) }
            assertFalse(exists("palette-drag-preview"))
            assertTrue(view().getBoolean("can_undo"))
            tool = MotionEvent.TOOL_TYPE_MOUSE
            tap(center("palette-swatch-${before[0]}"))
            key(KeyEvent.KEYCODE_Z, KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON)
            waitFor("undo restores one step") { order() == before }
            key(KeyEvent.KEYCODE_Z, KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON or KeyEvent.META_SHIFT_ON or KeyEvent.META_SHIFT_LEFT_ON)
            waitFor("redo") { order() != before }
            library(obj("op" to "undo_reorder", "palette" to view().getLong("palette")))
            assertEquals(before, order())
        }
        tool = MotionEvent.TOOL_TYPE_FINGER
        val before = order()
        val from = center(swatch(0))
        event(MotionEvent.ACTION_DOWN, from); glide(from, from + Offset(0f, 900f)); event(MotionEvent.ACTION_UP); settle()
        assertEquals("outside release cancels", before, order())
        event(MotionEvent.ACTION_DOWN, from); glide(from, center(swatch(3))); event(MotionEvent.ACTION_CANCEL); settle()
        assertEquals("cancel leaves the palette", before, order())
        assertFalse("no ghost after cancel", exists("palette-drag-preview"))
        tool = MotionEvent.TOOL_TYPE_MOUSE
        event(MotionEvent.ACTION_DOWN, from); glide(from, center(swatch(3)))
        key(KeyEvent.KEYCODE_ESCAPE)
        assertFalse("Escape retires the drag", exists("palette-drag-preview"))
        event(MotionEvent.ACTION_UP); settle()
        assertEquals(before, order())
        tool = MotionEvent.TOOL_TYPE_FINGER
        tap(center(swatch(1)))
        waitFor("tap selects") { view().array("swatches").objects()[1].getBoolean("current") }
        assertEquals(before, order())
    }

    @Test fun heldSwatchMenusDragAndMouseHoldsStayQuiet() {
        showPalettes()
        secondary(center(swatch(0)))
        waitFor("secondary click menu") { popupCount() == 1 && exists(text("Rename Color…")) }
        closeMenu()
        tool = MotionEvent.TOOL_TYPE_MOUSE
        val current = view().array("swatches").objects().map { it.getBoolean("current") }
        hold(center(swatch(4))); event(MotionEvent.ACTION_UP); settle()
        assertEquals("mouse holds never open menus", 0, popupCount())
        assertEquals("held release does not activate", current, view().array("swatches").objects().map { it.getBoolean("current") })
        for (kind in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_FINGER)) {
            tool = kind
            val before = order()
            val from = center(swatch(0))
            hold(from)
            waitFor("held menu") { popupCount() == 1 }
            glide(from, center(swatch(2)))
            waitFor("drag closes the held menu") { popupCount() == 0 && exists("palette-drag-preview") }
            event(MotionEvent.ACTION_UP)
            waitFor("held drag reorders") { order() != before }
            library(obj("op" to "undo_reorder", "palette" to view().getLong("palette")))
            hold(center(swatch(0))); waitFor("held menu") { popupCount() == 1 }
            event(MotionEvent.ACTION_UP); settle()
            assertEquals("release keeps held menu", 1, popupCount())
            tap(bounds(text("Remove Color"), "Remove Color").center)
            waitFor("menu removes color") { order().size == before.size - 1 }
            assertFalse("adding/removing clears reorder history", view().getBoolean("can_undo"))
        }
    }

    @Test fun importsAndExportsPaletteFiles() {
        showPalettes()
        val palette = library().array("palettes").objects().first { it.getLong("id") == view().getLong("palette") }
        val names = palette.array("swatches").objects().map { it.getString("name") }
        val result = Native.paletteFile(obj("type" to "export", "palette" to palette, "format" to "capycolor").toString(), byteArrayOf())
        val metadata = JSONObject(result[0] as String)
        assertTrue(metadata.getString("file_name").endsWith(".capycolor"))
        val bytes = result[1] as ByteArray
        File(output, metadata.getString("file_name")).writeBytes(bytes)
        val count = library().array("palettes").length()
        val action = JSONObject(Native.paletteFile(obj("type" to "import", "file_name" to metadata.getString("file_name")).toString(), bytes)[0] as String)
        assertNull(library(action.getJSONObject("action")))
        assertEquals(count + 1, library().array("palettes").length())
        assertEquals(names, view().array("swatches").objects().map { it.getString("name") })
        library(obj("op" to "remove_palette", "id" to view().getLong("palette")))
        try { Native.paletteFile(obj("type" to "import", "file_name" to "bad.aco").toString(), byteArrayOf(0, 1, 0, 9)); fail("damaged file") }
        catch (e: IllegalStateException) { assertTrue(e.message!!.isNotEmpty()) }
        assertEquals("failed imports are atomic", count, library().array("palettes").length())
        arguments.getString("paletteDirectory")?.let { directory ->
            val files = File(directory).walkTopDown().filter { it.isFile }.sortedBy { it.path }.toList()
            assertTrue("real sample files", files.isNotEmpty())
            val results = JSONArray()
            for (file in files) {
                val entry = obj("file" to file.relativeTo(File(directory)).path)
                try {
                    val action = JSONObject(Native.paletteFile(obj("type" to "import", "file_name" to file.name).toString(), file.readBytes())[0] as String)
                    library(action.getJSONObject("action"))?.let { error(it) }
                    val imported = view()
                    entry.put("name", imported.getString("name"))
                    entry.put("swatches", JSONArray(imported.array("swatches").objects().map { obj("name" to it.getString("name"), "color" to it.getJSONObject("color"), "hex" to it.getString("detail").split(" · ").let { d -> d[d.size - 2] }) }))
                    assertNull(library(obj("op" to "remove_palette", "id" to imported.getLong("palette"))))
                } catch (e: Throwable) { entry.put("error", e.message ?: e.toString()) }
                results.put(entry)
            }
            File(output, "sample-imports.json").writeText(results.toString(1))
        }
    }

    @Test fun palettesPersistAcrossRelaunch() {
        action(obj("type" to "select_panel_tab", "group" to group("color").getInt("id"), "panel" to "color"))
        val fitted = group("color").getJSONObject("bounds").getDouble("height")
        showPalettes()
        assertNull(library(obj("op" to "create_palette", "name" to "Persisted")))
        val id = view().getLong("palette")
        val color = obj("space" to "DisplayP3", "rgba" to JSONArray(listOf(1, .25, .1, .5)))
        assertNull(library(obj("op" to "store", "palette" to id, "name" to "Wide", "color" to color)))
        val saved = library().toString()
        scenario.close()
        launch()
        switchWorkspace("builtin:workspace:illustrator")
        waitFor("restored palettes") { library().array("palettes").objects().any { it.getString("name") == "Persisted" } }
        waitFor("the hidden Color page still fits the group") {
            group("color").getString("active") == "palettes" && abs(group("color").getJSONObject("bounds").getDouble("height") - fitted) < .5
        }
        val restored = library().array("palettes").objects().first { it.getString("name") == "Persisted" }
        assertEquals(JSONObject(saved).array("palettes").objects().first { it.getLong("id") == id }.array("swatches").toString(), restored.array("swatches").toString())
    }

    @Test fun paletteDragFramesAndHoldLatency() {
        org.junit.Assume.assumeTrue(arguments.getString("paletteBenchmark") == "true")
        showPalettes()
        tool = MotionEvent.TOOL_TYPE_STYLUS
        val holds = (0 until 5).map {
            val at = center(swatch(1))
            event(MotionEvent.ACTION_DOWN, at)
            val start = SystemClock.uptimeMillis()
            waitFor("held menu") { popupCount() == 1 }
            val latency = SystemClock.uptimeMillis() - start
            event(MotionEvent.ACTION_UP); settle(); closeMenu()
            latency
        }
        val before = order()
        val from = center(swatch(0)); val cells = (0 until minOf(12, order().size)).map { center(swatch(it)) }
        event(MotionEvent.ACTION_DOWN, from); glide(from, cells[1], 4)
        waitFor("lifted") { exists("palette-drag-preview") }
        fun report(reset: Boolean): JSONObject {
            val done = CountDownLatch(1); var result = JSONObject()
            instrumentation.runOnMainSync { host.measurements(reset) { result = it; done.countDown() } }
            assertTrue(done.await(10, TimeUnit.SECONDS)); return result
        }
        report(true)
        val frames = java.util.Collections.synchronizedList(mutableListOf<LongArray>())
        val thread = android.os.HandlerThread("palette-frames").apply { start() }
        val listener = android.view.Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
            val ui = listOf(android.view.FrameMetrics.INPUT_HANDLING_DURATION, android.view.FrameMetrics.ANIMATION_DURATION,
                android.view.FrameMetrics.LAYOUT_MEASURE_DURATION, android.view.FrameMetrics.DRAW_DURATION).sumOf { metrics.getMetric(it) }
            frames.add(longArrayOf(metrics.getMetric(android.view.FrameMetrics.TOTAL_DURATION), metrics.getMetric(android.view.FrameMetrics.DEADLINE),
                ui, metrics.getMetric(android.view.FrameMetrics.GPU_DURATION), metrics.getMetric(android.view.FrameMetrics.INTENDED_VSYNC_TIMESTAMP)))
        }
        val window = scenarioActivity().window
        instrumentation.runOnMainSync { window.addOnFrameMetricsAvailableListener(listener, android.os.Handler(thread.looper)) }
        val origin = IntArray(2); instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
        val start = SystemClock.uptimeMillis(); var samples = 0
        while (SystemClock.uptimeMillis() - start < 2000) {
            val t = (SystemClock.uptimeMillis() - start) / 2000f * (cells.size - 1)
            val a = cells[t.toInt().coerceAtMost(cells.size - 2)]; val b = cells[t.toInt().coerceAtMost(cells.size - 2) + 1]
            point = a + (b - a) * (t - t.toInt())
            val motion = MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), MotionEvent.ACTION_MOVE, 1,
                arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = tool }),
                arrayOf(MotionEvent.PointerCoords().apply { x = point.x + origin[0]; y = point.y + origin[1]; pressure = .7f }),
                0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
            instrumentation.uiAutomation.injectInputEvent(motion, false); motion.recycle()
            samples++
            SystemClock.sleep(4)
        }
        instrumentation.runOnMainSync { window.removeOnFrameMetricsAvailableListener(listener) }
        val during = report(false)
        event(MotionEvent.ACTION_CANCEL); settle(); thread.quitSafely()
        assertEquals("cancelled drag leaves the palette", before, order())
        val rows = frames.toList().sortedBy { it[4] }
        fun stats(column: Int) = rows.map { it[column] / 1e6 }.sorted().let { v ->
            obj("p50" to v[(v.size - 1) / 2], "p90" to v[((v.size - 1) * .9).toInt()], "p99" to v[((v.size - 1) * .99).toInt()], "max" to v.last())
        }
        val intervals = rows.zipWithNext { a, b -> (b[4] - a[4]) / 1e6 }
        val result = obj("samples" to samples, "frames" to rows.size, "missed_deadline" to rows.count { it[0] > it[1] },
            "total_ms" to stats(0), "ui_thread_ms" to stats(2), "gpu_ms" to stats(3),
            "vsync_interval_ms" to intervals.sorted().let { obj("p50" to it[(it.size - 1) / 2], "max" to it.last(), "over_9ms" to it.count { v -> v > 9.0 }) },
            "snapshots_published" to during.getLong("snapshots_published"), "panel_content_changes" to during.getLong("panel_content_changes"),
            "hold_menu_latency_ms" to JSONArray(holds))
        File(output, "drag-frames.json").writeText(result.toString(1))
        assertEquals("a steady drag publishes no model", 0L, during.getLong("snapshots_published"))
    }

    @Test fun colorSwatchMenuRevealsPalettes() {
        switchWorkspace("builtin:workspace:photographer")
        for ((kind, open) in listOf<Pair<Int, (Offset) -> Unit>>(MotionEvent.TOOL_TYPE_MOUSE to { at -> secondary(at) },
            MotionEvent.TOOL_TYPE_STYLUS to { at -> tool = MotionEvent.TOOL_TYPE_STYLUS; hold(at); event(MotionEvent.ACTION_UP); settle() })) {
            action(obj("type" to "select_panel_tab", "group" to group("color").getInt("id"), "panel" to "color"))
            waitFor("Color page") { exists("color-swatch-background") }
            open(center("color-swatch-background"))
            waitFor("$kind swatch menu") { exists("color-library-menu") }
            tool = kind; mouseButton = MotionEvent.BUTTON_PRIMARY
            tap(center("color-library-menu"))
            waitFor("$kind reveals Palettes") { group("color").getString("active") == "palettes" && exists("palette-panel") }
            assertEquals("the chosen swatch becomes the paint slot", "background", state().getJSONObject("colors").getString("slot"))
        }
    }

    @Test fun automaticTabNamesFitTheMeasuredStrip() {
        showPalettes()
        val header = bounds("group-header-${group("color").getInt("id")}")
        val tabs = group("color").array("panels").values().map { it.toString() }
        var width = 0f
        for (panel in tabs) {
            assertTrue("every icon is reserved", exists("tab-icon-$panel"))
            width += bounds("tab-$panel").width
        }
        assertTrue("tabs fit their measured strip: $width <= ${header.width}", width <= header.width - 20 * density + 1)
        capture("tabs")
        assertEquals(tabs.size, tabs.count { exists("tab-icon-$it") })
        assertTrue((0 until tabs.size).all { exists("tab-${tabs[it]}") })
        assertTrue(tabs.map { exists("tab-name-$it") }.toString(), tabs.any { exists("tab-name-$it") })
    }
}
