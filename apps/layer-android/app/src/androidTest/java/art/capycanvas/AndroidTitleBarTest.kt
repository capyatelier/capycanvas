package art.capycanvas

import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.asAndroidPath
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.TextLayoutResult
import androidx.test.core.app.ActivityScenario
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import kotlin.math.roundToInt

/** Real Compose/native-view contacts on isolated workspaces and preferences. */
class AndroidTitleBarTest {
    @get:Rule val device = CapyDeviceRule()
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var host: CanvasHost
    private var pressed: ViewRootForTest? = null
    private var point = Offset.Zero
    private var downAt = 0L
    private var tool = MotionEvent.TOOL_TYPE_FINGER
    private var button = MotionEvent.BUTTON_PRIMARY
    private var density = 1f
    private fun view() = host.workspaceManager!!
    private fun layout() = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").toString()
    private fun node(tag: String) = findTag(tag)?.takeIf { it.second.layoutInfo.isPlaced }
    private fun bounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync { result = node(tag)?.second?.boundsInRoot }
        return checkNotNull(result) { "Missing $tag" }
    }
    private fun assertHiddenChrome(tag:String) {
        instrumentation.runOnMainSync {
            findTag(tag)?.let { (root,_) ->
                assertNull("Hidden $tag is absent from accessibility",root.semanticsOwner.rootSemanticsNode.find(hasTag(tag)))
            }
        }
    }
    private fun screenBounds(tag: String): Rect {
        var result: Rect? = null
        instrumentation.runOnMainSync {
            val (root, node) = checkNotNull(node(tag))
            val screen = IntArray(2); root.view.getLocationOnScreen(screen)
            result = node.boundsInRoot.translate(Offset(screen[0].toFloat(), screen[1].toFloat()))
        }
        return checkNotNull(result)
    }
    private fun waitFor(label: String, timeout: Long = 15000, condition: () -> Boolean) =
        host.awaitMain(label, timeout, {
            shot("failure-${label.replace(Regex("[^A-Za-z0-9-]"), "-")}")
            "${view()}; customization=${state().getJSONObject("customization")}" }) {
            host.snapshot?.objectOrNull("state")?.let { assertTrue(it.optString("host_error"), it.isNull("host_error")) }
            condition()
        }
    private fun idle() {
        // A settled workspace-manager view alone does not mean that a queued
        // header edit and its snapshot have reached the native owner and UI.
        host.drain()
        SystemClock.sleep(220)
        waitFor("workspace idle") { !view().optBoolean("busy") && !view().optBoolean("switcher_busy") && !view().optBoolean("dirty") }
        assertTrue(view().toString(), view().isNull("error")); assertTrue(view().toString(), view().isNull("switcher_error"))
    }
    private fun send(value: JSONObject) { instrumentation.runOnMainSync { host.workspaceInput(value) }; idle() }
    private fun capture() = host.workspaceCapture()
    private fun event(action: Int, next: Offset = point, meta: Int = 0) {
        point = next
        if (action == MotionEvent.ACTION_DOWN) downAt = SystemClock.uptimeMillis()
        val event = motion(tool, action, next, downAt, button, meta)
        try { instrumentation.runOnMainSync { checkNotNull(pressed).view.dispatchTouchEvent(event) } }
        finally { event.recycle() }
        if (action in listOf(MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL)) pressed = null
        SystemClock.sleep(40)
    }
    private fun down(tag: String) {
        var start = Offset.Zero
        instrumentation.runOnMainSync { checkNotNull(node(tag)) { "Missing $tag; ${view()}" }.let { pressed = it.first; start = it.second.boundsInRoot.center } }
        event(MotionEvent.ACTION_DOWN, start)
    }
    private fun tap(tag: String) {
        android.util.Log.i("TitleBarAcceptance", "Tap $tag")
        down(tag); event(MotionEvent.ACTION_UP); idle()
    }
    private fun key(code: Int, meta: Int = 0) { pressKey(code, meta); SystemClock.sleep(220) }

    private lateinit var fixture: JSONObject
    private lateinit var startingWorkspace: JSONObject
    private fun snapshot() = host.snapshot!!
    private fun state() = snapshot().getJSONObject("state")
    private fun model() = snapshot().getJSONObject("header").getJSONObject("model")
    private fun entries() = model().array("zones").values().flatMap { (it as JSONArray).objects() }
    private fun editing() = snapshot().getJSONObject("header").optBoolean("editing")
    private fun action(value: JSONObject) { host.drain(value); SystemClock.sleep(250) }
    private fun edit(value: JSONObject) = action(obj("type" to "customize", "action" to obj("type" to "header", "action" to value)))
    private fun restore(size: String = "small") {
        val value = JSONObject(fixture.toString())
        value.getJSONObject("layout").getJSONObject("header").put("size", size)
        action(obj("type" to "restore_workspace", "workspace" to value))
        waitFor("bar settled") { node("zen-button") != null && !editing() }
        idle()
    }
    private fun resetPreset(id: String) {
        send(obj("type" to "switch", "id" to id))
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to id))) }
        host.drain()
        waitFor("starting group layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        waitFor("starting group layout confirmation closes") { node("workspace-submit") == null }
        idle()
    }
    private fun launch() {
        scenario = launchCapy(90_000)
        scenario.onActivity { host = it.host; density = it.resources.displayMetrics.density }
        idle()
    }
    @Before fun ready() {
        launch()
        startingWorkspace = JSONObject(state().getJSONObject("workspace").toString())
        fixture = JSONObject(startingWorkspace.toString())
        fixture.put("zen_mode", false)
        fixture.getJSONObject("layout").apply {
            for (name in listOf("bands", "floating", "collapsed", "column_stacks", "column_scroll", "fit_tab_groups", "fit_height_groups")) put(name, JSONArray())
            val left = JSONArray(listOf(
                obj("id" to 1, "item" to obj("kind" to "capy")),
                obj("id" to 2, "item" to obj("kind" to "menu_labels"))))
            val right = JSONArray(listOf(obj("id" to 3, "item" to obj("kind" to "settings"))))
            put("header", obj("size" to "small", "next_id" to 10,
                "zones" to JSONArray(listOf(left, JSONArray(), right))))
            put("canvas_info", obj("visible" to true))
        }
        restore()
    }
    @After fun cleanup() {
        if (pressed != null) event(MotionEvent.ACTION_CANCEL)
        if (::scenario.isInitialized) scenario.close()
    }
    private fun shot(name: String) = screenshot("validation/title-bar/$name.png")
    private fun tilePixel(tag: String): Int {
        var position = Offset.Zero
        instrumentation.runOnMainSync {
            val (root, node) = checkNotNull(node(tag))
            val screen = IntArray(2); root.view.getLocationOnScreen(screen)
            position = Offset(screen[0] + node.boundsInRoot.left + 8 * density, screen[1] + node.boundsInRoot.center.y)
        }
        val bitmap = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
        return try { bitmap.getPixel(position.x.toInt(), position.y.toInt()) } finally { bitmap.recycle() }
    }
    private fun center() = bounds("title-bar").let { Offset(it.center.x, it.center.y) }
    private fun outside() = bounds("workspace").let { Offset(it.center.x, it.bottom - 80 * density) }
    private fun drag(tag: String, destination: Offset, cancel: Boolean = false, inspect: (() -> Unit)? = null) {
        down(tag)
        event(MotionEvent.ACTION_MOVE, destination)
        inspect?.invoke()
        event(if (cancel) MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP)
        idle()
    }
    private fun tapMenuRow(label: String) {
        var menuRoot: ViewRootForTest? = null
        var target: SemanticsNode? = null
        waitFor("$label row") {
            fun text(node: SemanticsNode): Boolean = node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == label } == true ||
                node.children.filter { it.config.getOrNull(SemanticsActions.OnClick) == null }.any(::text)
            fun row(node: SemanticsNode): SemanticsNode? = if (node.config.getOrNull(SemanticsActions.OnClick) != null && text(node)) node
                else node.children.firstNotNullOfOrNull(::row)
            semanticsRoots().any { root -> root.find(hasTag("workspace-menu"))?.let(::row)?.let { menuRoot = root; target = it; true } == true }
        }
        pressed = menuRoot
        event(MotionEvent.ACTION_DOWN, target!!.boundsInRoot.center); event(MotionEvent.ACTION_UP)
    }
    private fun variantMenu(anchor: JSONObject): JSONObject {
        val done = java.util.concurrent.CountDownLatch(1)
        var result: JSONObject? = null
        instrumentation.runOnMainSync {
            host.query(obj("type" to "context", "target" to obj("kind" to "tool_variants", "anchor" to anchor))) {
                result = it as? JSONObject; done.countDown()
            }
        }
        assertTrue(done.await(10, java.util.concurrent.TimeUnit.SECONDS))
        return checkNotNull(result)
    }
    private fun variantRows(anchor: JSONObject) = variantMenu(anchor).array("sections").values().flatMap { (it as JSONArray).objects() }
    private fun openToolContext(tag: String) {
        button = if (tool == MotionEvent.TOOL_TYPE_MOUSE) MotionEvent.BUTTON_SECONDARY else MotionEvent.BUTTON_PRIMARY
        down(tag)
        if (tool != MotionEvent.TOOL_TYPE_MOUSE) SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        event(MotionEvent.ACTION_UP)
        button = MotionEvent.BUTTON_PRIMARY
        waitFor("tool context opens") { node("workspace-menu") != null }
    }

    private fun startEditor() {
        action(obj("type" to "invoke", "command" to "customize_workspace_ui"))
        waitFor("inline editor") { editing() && node("header-editor") != null }
        SystemClock.sleep(200)
    }

    @Test fun zenCapyKeepsNativeButtonAndGlyphBoundsAcrossWorkspaces() {
        fun rectEquals(label: String, expected: Rect, actual: Rect) {
            for ((name, before, after) in listOf(
                Triple("left", expected.left, actual.left), Triple("top", expected.top, actual.top),
                Triple("width", expected.width, actual.width), Triple("height", expected.height, actual.height)))
                assertEquals("$label $name", before, after, .6f)
        }
        fun glyph(tag: String): Rect {
            var result: Rect? = null
            instrumentation.runOnMainSync {
                val owner = checkNotNull(node(tag)).second
                result = owner.find { it.config.getOrNull(SemanticsProperties.Role) == androidx.compose.ui.semantics.Role.Image }?.boundsInRoot
            }
            return checkNotNull(result) { "Missing Capy image in $tag" }
        }
        for ((workspace, name) in listOf("illustrator" to "paint", "painter" to "sketch", "photographer" to "photo")) {
            send(obj("type" to "switch", "id" to "builtin:workspace:$workspace"))
            waitFor("native $name workspace") { view().optString("id") == "builtin:workspace:$workspace" && !view().optBoolean("busy") }
            idle()
            if (workspace == "illustrator") action(obj("type" to "restore_workspace", "workspace" to startingWorkspace))
            val original = state().getJSONObject("workspace").toString()
            for (theme in listOf("dark", "light")) for (size in listOf("small", "medium", "large"))
                for (placement in listOf("default", "center", "right", "absent", "overflow")) {
                action(obj("type" to "set_theme", "theme" to theme))
                val workspaceState = JSONObject(original)
                val header = workspaceState.getJSONObject("layout").getJSONObject("header")
                header.put("size", size)
                val zones = header.getJSONArray("zones")
                val capy = zones.values().flatMap { (it as JSONArray).objects() }
                    .first { it.getJSONObject("item").getString("kind") == "capy" }
                val capyId = capy.getInt("id")
                if (placement != "default") {
                    for (zone in 0..2) zones.put(zone, JSONArray(zones.getJSONArray(zone).objects().filter { it.getInt("id") != capyId }))
                    when (placement) {
                        "center" -> zones.getJSONArray(1).put(capy)
                        "right" -> zones.getJSONArray(2).put(capy)
                        "overflow" -> {
                            val spaces = JSONArray(List(80) { obj("id" to 1000 + it, "item" to obj("kind" to "space")) })
                            spaces.put(capy); zones.put(1, spaces); header.put("next_id", 2000)
                        }
                    }
                }
                action(obj("type" to "restore_workspace", "workspace" to workspaceState))
                idle()
                val label = "$name/$theme/$size/$placement"
                val tag = "zen-button"
                val fallback = placement in listOf("absent", "overflow")
                if (!fallback) waitFor("native Capy $label") { node(tag) != null }
                if (fallback) assertNull("$placement has no visible header Capy", node(tag))
                val tile = mapOf("small" to 36f, "medium" to 48f, "large" to 60f).getValue(size) * density
                val gap = (if (size == "small") 6f else if (size == "medium") 8f else 10f) * density
                val glyphSize = mapOf("small" to 31f, "medium" to 41f, "large" to 52f).getValue(size) * density
                val root = bounds("workspace")
                val normal = if (fallback) Rect(root.left + gap.roundToInt(), root.top + gap.roundToInt(),
                    root.left + gap.roundToInt() + tile.roundToInt(), root.top + gap.roundToInt() + tile.roundToInt()) else bounds(tag)
                val inset = ((tile.roundToInt() - glyphSize.roundToInt()) / 2f).roundToInt()
                val normalGlyph = if (fallback) Rect(normal.left + inset, normal.top + inset,
                    normal.left + inset + glyphSize.roundToInt(), normal.top + inset + glyphSize.roundToInt()) else glyph(tag)
                val baseline = layout()
                val camera = state().getJSONObject("camera").toString()
                val headerArea = bounds("title-bar").translate(-host.surfaceOrigin)
                action(obj("type" to "invoke", "command" to "zen_mode"))
                waitFor("Zen $label") { snapshot().optBoolean("chrome_hidden") && node("zen-button") != null }
                rectEquals("$label Capy", normal, bounds("zen-button"))
                rectEquals("$label glyph", normalGlyph, glyph("zen-button"))
                assertHiddenChrome("title-bar")
                val zenGlass = bounds("zen-button").translate(-host.surfaceOrigin)
                instrumentation.runOnMainSync {
                    val boxes = host.glassBoxesForTest.map { Rect(it[0], it[1], it[0] + it[2], it[1] + it[3]) }.filter { it.overlaps(headerArea) }
                    assertEquals("$label retains only Capy header glass", 1, boxes.size)
                    rectEquals("$label Capy glass", zenGlass, boxes.single())
                }
                shot("zen-bounds-$name-$theme-$size-$placement")
                tap("zen-button")
                waitFor("Zen bounds restored") { !snapshot().optBoolean("chrome_hidden") }
                if (fallback) assertNull("$placement stays absent after Zen", node(tag)) else {
                    rectEquals("$label restored Capy", normal, bounds(tag))
                    rectEquals("$label restored glyph", normalGlyph, glyph(tag))
                }
                assertEquals(baseline, layout())
                assertEquals(camera, state().getJSONObject("camera").toString())
            }
        }
        val restored = JSONObject(fixture.toString()).apply { put("zen_mode", true) }
        restored.getJSONObject("layout").getJSONObject("header").put("size", "small")
        val restoredRoot = bounds("workspace")
        action(obj("type" to "restore_workspace", "workspace" to restored))
        waitFor("restored Zen measures its new header size") {
            val capy = node("zen-button")?.second
            val image = capy?.find { it.config.getOrNull(SemanticsProperties.Role) == androidx.compose.ui.semantics.Role.Image }
            capy?.boundsInRoot?.let { it.width == (36 * density).roundToInt().toFloat() &&
                kotlin.math.abs(it.left - restoredRoot.left - (6 * density).roundToInt()) < .6f &&
                kotlin.math.abs(it.top - restoredRoot.top - (6 * density).roundToInt()) < .6f } == true &&
                image?.boundsInRoot?.width == (31 * density).roundToInt().toFloat()
        }
        val zen = bounds("zen-button")
        val zenGlyph = glyph("zen-button")
        assertEquals((31 * density).roundToInt().toFloat(), zenGlyph.height, .6f)
        assertHiddenChrome("title-bar")
        tap("zen-button")
        waitFor("restored Zen exits") { !snapshot().optBoolean("chrome_hidden") && node("zen-button") != null }
        rectEquals("restored Zen Capy", zen, bounds("zen-button"))
        rectEquals("restored Zen glyph", zenGlyph, glyph("zen-button"))
    }

    @Test fun zenHeaderRetiresChromeAndLetsCanvasReceiveOldHeaderContacts() {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            action(obj("type" to "invoke", "command" to "hand"))
            val oldHeader = bounds("header-control-3").center
            val camera = state().getJSONObject("camera").toString()
            action(obj("type" to "invoke", "command" to "zen_mode"))
            waitFor("Zen retires header controls") { snapshot().optBoolean("chrome_hidden") && node("title-bar")?.first?.semanticsOwner?.rootSemanticsNode?.find(hasTag("title-bar"))==null }
            instrumentation.runOnMainSync { pressed = checkNotNull(node("workspace")).first }
            event(MotionEvent.ACTION_DOWN, oldHeader)
            event(MotionEvent.ACTION_MOVE, oldHeader + Offset(-60 * density, 30 * density))
            event(MotionEvent.ACTION_UP)
            host.drain()
            waitFor("old header contact reaches the native Hand tool") { state().getJSONObject("camera").toString() != camera }
            assertTrue(snapshot().optBoolean("chrome_hidden"))
            tap("zen-button")
        }
    }

    @Test fun zenCapyPreferenceSwitchInputAndRestart() {
        fun hidden() = snapshot().optBoolean("chrome_hidden")
        action(obj("type" to "restore_settings", "settings" to JSONObject()))
        assertTrue(state().getJSONObject("settings").getBoolean("zen_show_capy"))
        for (theme in listOf("dark", "light")) {
            action(obj("type" to "set_theme", "theme" to theme))
            restore()
            for (show in listOf(true, false)) {
                action(obj("type" to "open_settings", "page" to "appearance"))
                val id = "zen_show_capy"
                action(obj("type" to "preferences", "action" to obj("type" to "reveal", "id" to id)))
                waitFor("preference row") { node("preference-$id") != null }
                if (state().getJSONObject("settings").getBoolean(id) != show) {
                    tool = MotionEvent.TOOL_TYPE_FINGER
                    tap("preference-$id")
                    waitFor("Capy switch") { state().getJSONObject("settings").getBoolean(id) == show }
                }
                shot("zen-settings-$theme-$show")
                action(obj("type" to "close_settings"))
                val devices = if (show) listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS) else listOf(MotionEvent.TOOL_TYPE_FINGER)
                for (device in devices) {
                    tool = device
                    action(obj("type" to "invoke", "command" to "zen_mode"))
                    waitFor("hidden chrome $theme/$show/$device") { hidden() && (node("zen-button") != null) == show }
                    shot("zen-$theme-$show-$device")
                    if (show) tap("zen-button") else key(KeyEvent.KEYCODE_TAB)
                    waitFor("Zen exit") { !state().getJSONObject("workspace").getBoolean("zen_mode") && !hidden() }
                }
            }
            scenario.close(); launch()
            assertFalse(state().getJSONObject("settings").getBoolean("zen_show_capy"))
            assertEquals(theme, state().getString("theme"))
        }
    }

    @Test fun bankBodiesGripsCancellationAndHistoryEveryDeviceAndSize() {
        val theme = "light"
        for (size in listOf("small", "medium", "large"))
            for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = device
                android.util.Log.i("TitleBarAcceptance", "Case $theme $size $device")
                action(obj("type" to "set_theme", "theme" to theme)); restore(size)
                val baseline = model().toString()
                val durable = capture()
                startEditor()
                val opened = model().toString()
                tap("header-component-space"); assertEquals("Bank taps are inert", opened, model().toString())
                tap("header-component-tools"); assertNull(snapshot().objectOrNull("picker"))
                drag("header-component-space", outside()); assertEquals("Outside bank drop cancels", opened, model().toString())
                drag("header-component-space", center(), inspect = {
                    waitFor("immediate bank preview") { node("header-drag-ghost") != null }
                    assertEquals("Motion does not publish layout", opened, model().toString())
                })
                val added = entries().first { it.getJSONObject("item").getString("kind") == "space" }.getInt("id")
                assertEquals(added, model().array("zones").getJSONArray(1).getJSONObject(0).getInt("id"))
                val preview = model().toString()
                drag("header-component-tools", center())
                waitFor("drop opens existing tool picker") { snapshot().objectOrNull("picker") != null && node("tool-picker-cancel") != null }
                tap("tool-picker-cancel")
                waitFor("picker Cancel keeps editor") { editing() && snapshot().objectOrNull("picker") == null }
                assertEquals(preview, model().toString())
                drag("header-item-1", outside(), cancel = true)
                assertEquals("ACTION_CANCEL preserves layout", preview, model().toString())
                down("header-grip-1")
                val grab = point - bounds("header-item-1").topLeft
                event(MotionEvent.ACTION_MOVE, outside())
                waitFor("detached original grab") { node("header-item-1")!!.second.boundsInRoot.top > 200 * density }
                assertEquals(grab.x, point.x - bounds("header-item-1").left, 2f)
                event(MotionEvent.ACTION_MOVE, center())
                waitFor("reentry") { node("header-item-1")!!.second.boundsInRoot.top < node("title-bar")!!.second.boundsInRoot.bottom }
                event(MotionEvent.ACTION_UP); idle()
                assertTrue(model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == 1 })
                drag("header-item-1", outside())
                assertFalse(entries().any { it.getInt("id") == 1 })
                waitFor("singleton returns to bank") { node("header-component-capy") != null }
                assertEquals("Preview never enters durable capture", durable, capture())
                shot("$theme-$size-$device")
                tap("header-edit-done"); waitFor("Done") { !editing() }
                val committed = model().toString()
                action(obj("type" to "invoke", "command" to "undo_workspace")); assertEquals(baseline, model().toString())
                action(obj("type" to "invoke", "command" to "redo_workspace")); assertEquals(committed, model().toString())
                startEditor(); tap("header-size-large"); tap("header-show-footer"); tap("header-edit-cancel")
                waitFor("Cancel leaves editor") { !editing() }
                assertEquals(committed, model().toString())
            }
    }

    @Test fun compactMenuKeepsItsGripAndNeighborsAndOverflowRemainsMovable() {
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device; restore(); startEditor()
            var stableId = 0
            instrumentation.runOnMainSync { stableId = node("header-item-2")!!.second.id }
            var added = 0
            while (true) {
                var compact = false
                instrumentation.runOnMainSync { compact = node("header-menu-labels-compact") != null }
                if (compact) break
                assertTrue("Menu compacts before capacity", added++ < 30)
                val menu = bounds("header-item-2")
                drag("header-component-space", Offset(menu.right - 2 * density, menu.center.y))
            }
            assertEquals(20f, bounds("header-grip-2").width / density, .5f)
            instrumentation.runOnMainSync { assertEquals("Same item survives compaction", stableId, node("header-item-2")!!.second.id) }
            val neighbor = model().array("zones").getJSONArray(0).objects().last().getInt("id")
            assertNotEquals(2, neighbor)
            assertTrue(bounds("header-item-$neighbor").width > 0)
            shot("compact-$device")
            drag("header-item-$neighbor", center())
            drag("header-item-2", center())
            assertTrue("Menu body moves with Capy retained", model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == 2 })
            assertTrue(entries().any { it.getInt("id") == 1 })
            tap("header-edit-cancel")
            restore("large"); startEditor()
            repeat(22) { edit(obj("type" to "add", "zone" to "left", "before" to null, "item" to obj("kind" to "space"))) }
            waitFor("real whole-item overflow") { node("header-overflow-0") != null }
            tap("header-overflow-0")
            waitFor("overflow chooser") { node("header-overflow-list") != null }
            val hidden = entries().map { it.getInt("id") }.first { id ->
                var exists = false; instrumentation.runOnMainSync { exists = node("header-overflow-item-$id") != null }; exists
            }
            drag("header-overflow-item-$hidden", center())
            assertTrue(model().array("zones").getJSONArray(1).objects().any { it.getInt("id") == hidden })
            tap("header-edit-done")
        }
    }

    @Test fun applicationMenusKeepNativeKeyboardNavigation() {
        fun focusedLabel(): String? {
            val focused = node("workspace-menu")?.second?.find {
                it.config.getOrNull(SemanticsProperties.Focused) == true && it.config.contains(SemanticsActions.OnClick)
            } ?: return null
            fun texts(value: SemanticsNode): List<String> = value.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } + value.children.flatMap(::texts)
            return texts(focused).firstOrNull()
        }
        for (theme in listOf("light", "dark")) for (navigation in listOf(KeyEvent.KEYCODE_DPAD_DOWN, KeyEvent.KEYCODE_TAB)) {
            action(obj("type" to "set_theme", "theme" to theme))
            restore()
            tool = MotionEvent.TOOL_TYPE_MOUSE
            tap("application-menu-file")
            waitFor("File menu opens for keyboard") { node("workspace-menu") != null }
            key(navigation)
            waitFor("Keyboard focuses a File command with key $navigation") {
                focusedLabel() != null
            }
            val first = snapshot().array("application_menus").objects().first { it.getString("id") == "file" }
                .getJSONObject("model").array("sections").getJSONArray(0).getJSONObject(0).getString("label")
            repeat(3) {
                var current: String? = null
                instrumentation.runOnMainSync { current = focusedLabel() }
                if (current != first) key(if (navigation == KeyEvent.KEYCODE_TAB) KeyEvent.KEYCODE_TAB else KeyEvent.KEYCODE_DPAD_UP,
                    if (navigation == KeyEvent.KEYCODE_TAB) KeyEvent.META_SHIFT_ON else 0)
            }
            waitFor("Keyboard reaches New command") { focusedLabel() == first }
            key(KeyEvent.KEYCODE_ENTER)
            waitFor("Keyboard invokes New Drawing") { node("workspace-menu") == null && node("new-drawing-create") != null }
            tap("new-drawing-cancel")
            waitFor("New Drawing closes") { node("new-drawing-create") == null }
        }
    }

    @Test fun openApplicationMenusFollowMouseAndPenHover() {
        fun hover(id: String, action: Int = MotionEvent.ACTION_HOVER_MOVE) {
            val event = motion(tool, action, screenBounds("application-menu-$id").center,
                SystemClock.uptimeMillis(), 0)
            try { assertTrue("OS accepts hover tool=$tool", instrumentation.uiAutomation.injectInputEvent(event, true)) }
            finally { event.recycle() }
            SystemClock.sleep(220)
        }
        fun contents(id: String): Boolean {
            val menu = snapshot().array("application_menus").objects().first { it.getString("id") == id }.getJSONObject("model")
            val popup = node("workspace-menu")?.second ?: return false
            fun texts(node: SemanticsNode): List<String> =
                node.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text } + node.children.flatMap(::texts)
            val labels = texts(popup)
            return (!menu.has("title") || menu.getString("title") in labels) &&
                menu.array("sections").values().flatMap { (it as JSONArray).objects() }.first().getString("label") in labels
        }
        for (theme in listOf("light", "dark")) for (inputTool in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme", "theme" to theme))
            restore()
            tool = inputTool
            hover("file", MotionEvent.ACTION_HOVER_ENTER)
            instrumentation.runOnMainSync { assertNull("Closed menus stay closed on hover", node("workspace-menu")) }
            tap("application-menu-file")
            waitFor("File menu contents") { contents("file") }
            for (id in listOf("edit", "view")) {
                hover(id)
                waitFor("Hover switches to $id") { contents(id) }
                val anchor = screenBounds("application-menu-$id")
                assertEquals("Hovered menu anchors to $id", anchor.left, screenBounds("workspace-menu").left, 2 * density)
                hover(id)
                waitFor("Repeated hover retains $id") { contents(id) }
                shot("hover-$theme-$inputTool-$id")
            }
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Escape closes hovered menu") { node("workspace-menu") == null }
            hover("file")
            instrumentation.runOnMainSync { assertNull("Dismissed menus stay closed on another label", node("workspace-menu")) }
            hover("file", MotionEvent.ACTION_HOVER_EXIT)
            tap("application-menu-file")
            waitFor("File menu reopens") { contents("file") }
            val outside = screenBounds("title-bar").center
            val downAt = SystemClock.uptimeMillis()
            for (phase in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val contact = motion(tool, phase, outside, downAt)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(contact, true)) }
                finally { contact.recycle() }
            }
            waitFor("Outside contact closes application menu") { node("workspace-menu") == null }
        }
    }

    @Test fun fullLabelsFitAndMenusAnchorToEachLabelAndEditorActionsAlignRight() {
        tool = MotionEvent.TOOL_TYPE_FINGER
        for (theme in listOf("dark", "light")) for ((size, gap, height) in listOf(Triple("small", 6f, 48f), Triple("medium", 8f, 64f), Triple("large", 10f, 80f))) {
            action(obj("type" to "set_theme", "theme" to theme))
            restore(size)
            val bar = bounds("title-bar")
            val capy = bounds("zen-button")
            val menus = bounds("header-item-2")
            val settings = bounds("header-item-3")
            assertEquals("Titlebar includes scaled padding", height * density, bar.height, density)
            assertEquals("Capy top clearance", gap * density, capy.top - bar.top, density)
            assertEquals("Capy left clearance", gap * density, capy.left - bar.left, density)
            assertEquals("Separate items use scaled spacing", gap * density, menus.left - capy.right, density)
            assertEquals("Settings right clearance", gap * density, bar.right - settings.right, density)
            shot("spacing-$theme-$size")
            for (menu in snapshot().array("application_menus").objects()) {
                val tag = "application-menu-${menu.getString("id")}"
                instrumentation.runOnMainSync {
                    fun textNode(node: SemanticsNode): SemanticsNode? =
                        if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == menu.getString("label") } == true) node
                        else node.children.firstNotNullOfOrNull(::textNode)
                    val label = checkNotNull(textNode(checkNotNull(node(tag)).second))
                    val layouts = mutableListOf<TextLayoutResult>()
                    assertTrue(label.config[SemanticsActions.GetTextLayoutResult].action!!.invoke(layouts))
                    val text = layouts.single()
                    val lastGlyph = text.getBoundingBox(text.layoutInput.text.lastIndex)
                    assertTrue("Final letter of ${menu.getString("label")} fits at $size: glyph=$lastGlyph, size=${text.size}", lastGlyph.right <= text.size.width + .5f)
                    assertEquals("Text is not clipped by its enclosing item", layouts.single().size.width.toFloat(), label.boundsInRoot.width, 1f)
                }
                if (size == "small") {
                    val anchor = screenBounds(tag)
                    tap(tag)
                    waitFor("${menu.getString("id")} menu retains header focus") { node("workspace-menu") != null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
                    val popup = screenBounds("workspace-menu")
                    assertEquals("Menu starts under its own label", anchor.left, popup.left, 2 * density)
                    val frame = android.graphics.Rect()
                    instrumentation.runOnMainSync { checkNotNull(node("title-bar")).first.view.getWindowVisibleDisplayFrame(frame) }
                    assertTrue("${menu.getString("id")} is below its label: $popup / $anchor", popup.top >= anchor.bottom - density && popup.top <= anchor.bottom + 12 * density)
                    assertTrue("Menus fit below the header: $popup / $frame", popup.bottom <= frame.bottom + density)
                    if (menu.getString("id") == "select") instrumentation.runOnMainSync {
                        fun row(label: String): SemanticsNode? {
                            fun find(n: SemanticsNode): SemanticsNode? =
                                if (n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == label } == true) n
                                else n.children.firstNotNullOfOrNull(::find)
                            return find(checkNotNull(node("workspace-menu")).first.semanticsOwner.rootSemanticsNode)
                        }
                        for (label in listOf("Load Selection", "Replace Selection Layer from Current Selection"))
                            assertTrue("$label is unavailable without saved layers", checkNotNull(row(label)).config.contains(SemanticsProperties.Disabled))
                        for (label in listOf("Grow Selection…", "Shrink Selection…", "Feather Selection…", "Border Selection…", "Smooth Selection…", "Transform Selection Outline")) assertNotNull(label, row(label))
                        assertNull(row("Modify"))
                    }
                    shot("anchored-$theme-${menu.getString("id")}")
                    key(KeyEvent.KEYCODE_BACK)
                    waitFor("menu dismissed") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
                    idle()
                }
            }
            startEditor()
            val actions = bounds("header-editor-actions")
            assertEquals("Editor actions end at the right padding", bounds("title-bar").right - 12 * density, actions.right, density)
            assertEquals("Done is the trailing control", actions.right, bounds("header-edit-done").right, density)
            for (tag in listOf("header-size-small", "header-size-medium", "header-size-large", "header-show-footer", "header-edit-cancel", "header-edit-done")) {
                assertEquals("Controls share a row", bounds("header-edit-done").center.y, bounds(tag).center.y, density)
            }
            shot("aligned-editor-$theme-$size")
            tap("header-edit-cancel"); waitFor("Cancel") { !editing() }
        }
    }

    @Test fun compactWorkspaceChoicesAndOverflowIconsFollowTheTitleBar() {
        val initial = view().array("switcher_display").objects().map { it.getString("id") }
        send(obj("type" to "edit_switcher", "edit" to obj("type" to "move", "id" to initial[2], "before" to initial[0])))
        var next = 1
        fun entry(kind: String) = obj("id" to next++, "item" to obj("kind" to kind))
        val left = JSONArray(listOf(entry("capy"), entry("menu")) + List(40) { entry("space") })
        val workspace = entry("workspaces")
        val id = workspace.getInt("id")
        val center = JSONArray(listOf(workspace) + List(30) { entry("space") })
        fixture.getJSONObject("layout").put("header", obj("size" to "small", "next_id" to next,
            "zones" to JSONArray(listOf(left, center, JSONArray()))))
        fun texts(node: SemanticsNode): List<String> =
            (node.config.getOrNull(SemanticsProperties.Text)?.map { it.text } ?: emptyList()) + node.children.flatMap(::texts)
        fun label(node: SemanticsNode, title: String): SemanticsNode? =
            if (node.config.getOrNull(SemanticsProperties.Text)?.any { it.text == title } == true) node
            else node.children.firstNotNullOfOrNull { label(it, title) }
        fun choose() {
            val choices = view().array("switcher_display").objects()
            val target = choices.first { it.getString("id") != view().getString("id") }
            waitFor("workspace choices") { node("workspace-menu") != null }
            instrumentation.runOnMainSync {
                val (root, popup) = checkNotNull(node("workspace-menu"))
                assertEquals("Only the pill's choices, in configured order", listOf(view().getJSONObject("switcher_menu").getString("title")) + view().getJSONObject("switcher_menu").array("sections").values().flatMap { (it as JSONArray).objects().map { row -> row.getString("label") } }, texts(popup))
                pressed = root
                point = checkNotNull(label(popup, target.getString("title"))).boundsInRoot.center
            }
            event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP)
            idle()
            assertEquals(target.getString("id"), view().getString("id"))
            instrumentation.runOnMainSync { assertNull(node("workspace-menu")) }
        }
        for (theme in listOf("dark", "light")) for ((index, size) in listOf("small", "medium", "large").withIndex()) {
            tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index]
            restore(size)
            action(obj("type" to "set_theme", "theme" to theme))
            waitFor("compact workspace selector") { node("header-control-$id") != null && node("workspace-switcher") == null }
            instrumentation.runOnMainSync {
                val overflow = checkNotNull(node("header-overflow-0")).second
                fun image(node: SemanticsNode): SemanticsNode? =
                    if (node.config.getOrNull(SemanticsProperties.ContentDescription)?.contains("More title bar items") == true) node
                    else node.children.firstNotNullOfOrNull(::image)
                val icon = checkNotNull(image(overflow)).boundsInRoot
                val expected = listOf(20f, 28f, 36f)[index]
                assertEquals("Hamburger width at $size", expected, icon.width / density, .5f)
                assertEquals("Hamburger height at $size", expected, icon.height / density, .5f)
                assertEquals(overflow.boundsInRoot.center.x, icon.center.x, 1f)
                assertEquals(overflow.boundsInRoot.center.y, icon.center.y, 1f)
            }
            tap("header-control-$id")
            shot("workspace-choices-$theme-$size")
            choose()
        }
        // Moving the selector into a crowded region must retain its menu action.
        restore("large")
        var before = 0
        instrumentation.runOnMainSync {
            before = left.objects().first { node("header-item-${it.getInt("id")}") == null }.getInt("id")
        }
        edit(obj("type" to "move", "id" to id, "zone" to "left", "before" to before))
        waitFor("hidden workspace selector") { node("header-item-$id") == null }
        tap("header-overflow-0")
        tap("header-overflow-item-$id")
        choose()
    }

    @Test fun nativeMenusToolPickerDrawersFooterZenAndRestart() {
        tool = MotionEvent.TOOL_TYPE_FINGER
        restore()
        tap("application-menu-window")
        tapMenuRow("Customize Title Bar…")
        waitFor("menu enters inline editor") { editing() && node("title-bar")?.first?.view?.hasWindowFocus() == true }
        SystemClock.sleep(250)
        drag("header-component-tools", center())
        waitFor("picker") { node("tool-picker-search") != null }
        action(obj("type" to "customize", "action" to obj("type" to "picker_search", "query" to "Brush color")))
        waitFor("Color choice") { node("tool-picker-choice-Brush color") != null }
        tap("tool-picker-choice-Brush color"); tap("tool-picker-confirm")
        val color = entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("kind") == "color" }.getInt("id")
        drag("header-component-workspaces", Offset(bounds("title-bar").right - 150 * density, center().y))
        tap("header-size-large")
        assertEquals("Pill remains compact", 36f, bounds("workspace-switcher").height / density, .5f)
        assertEquals("Pill remains centered", bounds("title-bar").center.y, bounds("workspace-switcher").center.y, density)
        tap("header-show-footer"); tap("header-edit-done")
        var readout = true; instrumentation.runOnMainSync { readout = node("camera-readout") != null }; assertFalse(readout)
        tap("header-control-$color")
        waitFor("shared header Color drawer") { node("tool-drawer") != null }
        tap("zen-button")
        waitFor("full Zen") { snapshot().optBoolean("chrome_hidden") }
        action(obj("type" to "invoke", "command" to "zen_mode")); waitFor("leave Zen") { !snapshot().optBoolean("chrome_hidden") }
        val committed = model().toString()
        val persisted = capture()
        startEditor(); tap("header-size-small"); tap("header-show-footer")
        assertEquals(persisted, capture())
        scenario.close(); launch()
        assertFalse(editing()); assertEquals("Restart uses committed header", committed, model().toString())
        shot("restart")
    }
    @Test fun photoDefaultColumnsAndPaintRestorationSurviveRestart() {
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        assertEquals("builtin:workspace:photographer", view().getString("id"))
        val before = capture()
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("latest default preview") { node("panel-body-color") != null && node("panel-body-layers") != null }
        assertEquals("Preview is not saved", before, capture())
        send(obj("type" to "cancel"))
        assertEquals("Cancel retains the current layout", before, capture())
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        fun checkColumns() {
            waitFor("primary Photo panels") { node("panel-body-color") != null && node("panel-body-properties") != null && node("panel-body-layers") != null }
            val color = bounds("group-14")
            val properties = bounds("group-15")
            val layers = bounds("group-16")
            val strip = bounds("collapsed-column-4")
            assertEquals(color.left, properties.left, 1f)
            assertEquals(properties.left, layers.left, 1f)
            assertTrue(color.bottom < properties.top && properties.bottom < layers.top)
            assertEquals("Secondary strip is immediately inward from the outer column", color.left, strip.right + 6 * density, density)
            val icons = listOf("brushes", "tool_settings", "sizes", "navigator").map { bounds("column-icon-$it") }
            assertTrue(icons.zipWithNext().all { (a, b) -> a.bottom < b.top })
            instrumentation.runOnMainSync { assertNull("Secondary column starts closed", node("group-6")) }
        }
        checkColumns()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            tap("tab-palettes"); waitFor("Palettes tab") { node("panel-body-palettes") != null }
            tap("tab-color")
            tap("tab-adjustments"); waitFor("Filters tab") { node("panel-body-adjustments") != null }
            tap("tab-properties")
            checkColumns(); shot("photo-default-$theme")
        }
        for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            for (panel in listOf("brushes", "tool_settings", "sizes", "navigator")) {
                tap("column-icon-$panel")
                waitFor("secondary $panel drawer opens") {
                    node("column-drawer-4") != null && state().getJSONObject("customization").array("column_drawers").objects()
                        .any { it.getJSONObject("anchor").optString("origin") == panel }
                }
                assertTrue(bounds("column-drawer-4").right < bounds("collapsed-column-4").left)
                assertNotNull(bounds("panel-body-color"))
                tap("column-icon-$panel")
                waitFor("secondary $panel drawer closes") { node("column-drawer-4") == null }
                checkColumns()
            }
        }
        val committed = layout()
        scenario.close(); launch()
        checkColumns()
        assertEquals("Default arrangement survives restart", committed, layout())
        shot("photo-default-restart")
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        tap("workspace-submit")
        waitFor("original Paint panels") { node("panel-body-brushes") != null && node("panel-body-color") != null && node("panel-body-navigator") != null }
        assertEquals("Paint restores its left panel column", bounds("group-6").left, bounds("group-10").left, 1f)
        assertTrue(bounds("group-10").right < bounds("group-14").left)
        assertTrue(bounds("group-14").right < bounds("collapsed-column-12").left)
        shot("paint-original-restored")
    }

    @Test fun paintColorAndNavigatorFitTheirContent() {
        waitFor("filter library", 60_000) { !state().getJSONObject("filter_load").optBoolean("pending") }
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        waitFor("restored fitted groups") { state().getJSONObject("workspace").getJSONObject("layout").getJSONArray("fit_height_groups").toString() == "[10,14]" }
        fun measured(panel: String) = snapshot().array("panel_measurements").objects().first { it.getString("panel") == panel }.number("content_height")
        fun reserved(group: String) = snapshot().getJSONObject("layout").array("groups").objects().first { it.getInt("id") == group.removePrefix("group-").toInt() }
            .array("panels").values().mapNotNull { panel -> snapshot().array("panel_measurements").objects().firstOrNull { it.getString("panel") == panel } }
            .maxOfOrNull { m -> m.optJSONObject("scroll")?.let { s -> minOf(s.number("fixed_height") + 4 * s.number("unit_height").takeIf { it > 0f }.let { it ?: 36f }, m.number("content_height")) } ?: m.number("content_height") }
        fun height(tag: String) = node(tag)!!.second.boundsInRoot.height / density
        fun fitted(label: String): Float {
            var settled = 0L
            var previous = ""
            waitFor("fitted Paint columns $label", 30_000) {
                if (listOf("group-6", "group-7", "group-10", "group-14", "navigator-overview").any { node(it) == null }) return@waitFor false
                val colors = reserved("group-10") ?: return@waitFor false
                val navigation = reserved("group-14") ?: return@waitFor false
                val tab = state().getJSONArray("tabs").getJSONObject(0)
                val aspect = (tab.getInt("height").toFloat() / tab.getInt("width")).coerceIn(.25f, 1f)
                val overview = node("navigator-overview")!!.second.boundsInRoot
                val navigationMeasurement = snapshot().array("panel_measurements").objects().first { it.getString("panel") == "navigator" }
                val scroll = navigationMeasurement.objectOrNull("scroll")
                val fixed = height("panel-body-navigator") - overview.height / density
                val continuous = scroll != null && scroll.number("unit_height") == 0f && kotlin.math.abs(scroll.number("fixed_height") - fixed) <= 1f
                val expectedOverview = minOf(overview.width * aspect, maxOf(0f, navigation - (scroll?.number("fixed_height") ?: 0f)) * density)
                val sample = listOf(height("group-6"), height("group-7"), height("group-10"), height("group-14"), overview.height / density, colors, navigation).joinToString()
                val fits = continuous && kotlin.math.abs(height("group-6") - height("group-7")) <= 1f &&
                    kotlin.math.abs(height("group-10") - colors - 36f) <= 1f &&
                    kotlin.math.abs(height("group-14") - navigation - 36f) <= 1f &&
                    kotlin.math.abs(overview.height - expectedOverview) <= 2f * density
                if (!fits || sample != previous) { previous = sample; settled = SystemClock.uptimeMillis() }
                fits && SystemClock.uptimeMillis() - settled > 1000
            }
            shot("paint-fitted-$label")
            return measured("color")
        }
        val sdr = fitted("sdr")
        action(obj("type" to "invoke", "command" to "new_document"))
        waitFor("new drawing dialog") { node("new-drawing-create") != null }
        for (edit in listOf(obj("op" to "unit", "unit" to "px"), obj("op" to "width", "value" to 900), obj("op" to "height", "value" to 1200), obj("op" to "depth", "depth" to "F16")))
            action(obj("type" to "new_drawing", "action" to edit))
        waitFor("HDR portrait choice") { state().objectOrNull("new_drawing")?.getJSONObject("sheet")?.getJSONArray("extent")?.let { it.getInt(0) == 900 && it.getInt(1) == 1200 } == true }
        tap("new-drawing-create")
        waitFor("HDR portrait document", 60_000) {
            val tab = state().getJSONArray("tabs").getJSONObject(0)
            tab.getInt("width") == 900 && tab.getInt("height") == 1200 && !state().getJSONObject("document_file").optBoolean("busy")
        }
        assertTrue("HDR Color panel is taller", fitted("hdr") > sdr + 10f)
        action(obj("type" to "invoke", "command" to "close_document"))
    }

    @Test fun colorExpansionClosesPromptlyWithoutRemeasuringTheWheel() {
        waitFor("filter library", 60_000) { !state().getJSONObject("filter_load").optBoolean("pending") }
        send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
        send(obj("type" to "form", "action" to obj("type" to "reset", "value" to view().getString("id"))))
        waitFor("starting layout confirmation") { node("workspace-submit") != null }
        tap("workspace-submit")
        waitFor("docked Color panel") { node("panel-body-color") != null && node("color-wheel") != null }
        SystemClock.sleep(1000)
        fun measured() = snapshot().array("panel_measurements").objects().first { it.getString("panel") == "color" }.number("content_height")
        val docked = bounds("group-10")
        val natural = measured()
        repeat(2) { round ->
            instrumentation.runOnMainSync { host.customize(obj("type" to "show_all_controls", "panel" to "color")) }
            waitFor("expanded Color $round") {
                state().getJSONObject("customization").optString("expanded") == "color" && (node("group-10")?.second?.boundsInRoot?.width ?: 0f) > docked.width + 100f
            }
            SystemClock.sleep(400)
            val start = SystemClock.uptimeMillis()
            instrumentation.runOnMainSync { host.customize(obj("type" to "close_expanded")) }
            waitFor("collapsed Color $round", 3_000) {
                node("group-10")?.second?.boundsInRoot?.let { r ->
                    kotlin.math.abs(r.top - docked.top) < 1f && kotlin.math.abs(r.width - docked.width) < 1f && kotlin.math.abs(r.height - docked.height) < 1f
                } == true
            }
            val elapsed = SystemClock.uptimeMillis() - start
            assertTrue("Collapse follows its own motion, not a remeasurement loop: $elapsed ms", elapsed < 800)
            SystemClock.sleep(500)
            assertEquals("Fitting the wheel in motion keeps its reported natural height", natural, measured(), 0f)
            assertEquals(docked, bounds("group-10"))
        }
    }

    @Test fun sketchDefaultsDrawersFeedbackStatusAndWorkspaceSwitch() {
        tool = MotionEvent.TOOL_TYPE_MOUSE
        resetPreset("builtin:workspace:painter")
        waitFor("Sketch") { view().optString("id") == "builtin:workspace:painter" && node("workspace-switcher") != null }
        val workspace = state().getJSONObject("workspace")
        val bands = workspace.getJSONObject("layout").array("bands").objects()
        assertEquals("Sketch docks only its compact brush toolbar", listOf(listOf("left", "center", 1)),
            bands.map { listOf(it.getString("edge"), it.optString("alignment"), it.getJSONObject("root").array("panels").length()) })
        val sliders = workspace.getJSONObject("layout").array("panels").objects()
            .first { it.getString("id") == bands[0].getJSONObject("root").array("panels").getString(0) }
            .getJSONObject("content").array("tiles").objects().map { it.getJSONObject("control").getString("kind") }
        assertTrue("Compact brush sliders: $sliders", "brush_size_slider" in sliders && "brush_strength_slider" in sliders)
        assertFalse(workspace.getJSONObject("layout").getJSONObject("canvas_info").getBoolean("visible"))
        val tools = entries().filter { it.getJSONObject("item").getString("kind") == "tool" }
        assertEquals(8, tools.size)
        val gap = snapshot().getJSONObject("header").array("sizes").objects().first { it.getString("id") == model().getString("size") }.number("gap")
        val (first, second) = model().array("zones").values().flatMap { (it as JSONArray).objects().zipWithNext() }
            .first { pair -> pair.toList().all { it.getJSONObject("item").getString("kind") == "tool" } }.toList()
            .map { bounds("header-control-${it.getInt("id")}") }
        assertEquals("Joined tiles use the toolbar tile gap", gap, (second.left - first.right) / density, .5f)
        // Transform is an action; the other seven default tools have drawers.
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for (entry in tools.filter { it.getJSONObject("item").getJSONObject("control").optString("command") != "scale_rotate" }) {
                val id = entry.getInt("id")
                tap("header-control-$id")
                // Selecting an inactive tool takes one click; its next click opens
                // the settings drawer, matching shared toolbar activation.
                if (state().getJSONObject("customization").objectOrNull("drawer") == null) tap("header-control-$id")
                waitFor("Sketch drawer $id") { node("tool-drawer") != null && state().getJSONObject("customization").objectOrNull("drawer")
                    ?.getJSONObject("anchor")?.optInt("id") == id }
                shot("sketch-drawer-$id-$theme")
                // Unused bar space dismisses the drawer without activating a tool.
                // Center is the switcher, so use the free gap beside the first region.
                val explicit = state().getJSONObject("customization").getJSONObject("drawer").getString("dismissal") == "explicit"
                instrumentation.runOnMainSync { pressed = node("title-bar")!!.first }
                val last = model().array("zones").getJSONArray(0).objects().last().getInt("id")
                val gap = Offset(bounds("header-item-$last").right + 12 * density, center().y)
                event(MotionEvent.ACTION_DOWN, gap); event(MotionEvent.ACTION_UP)
                if (explicit) {
                    idle(); assertNotNull("Explicit drawer $id ignores the bar gap", node("tool-drawer"))
                    tap("header-control-$id")
                }
                waitFor("bar gap dismisses drawer") { node("tool-drawer") == null }
            }
        }
        val brush = tools.first { it.getJSONObject("item").getJSONObject("control").optString("command") == "drawing_brush" }.getInt("id")
        val filters = tools.first { it.getJSONObject("item").getJSONObject("control").optString("panel") == "adjustments" }.getInt("id")
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            tap("header-control-$brush")
            val before = tilePixel("header-control-$brush")
            assertTrue("Selected tool is blue", android.graphics.Color.blue(before) > android.graphics.Color.red(before) + 10)
            down("header-control-$brush")
            assertEquals("Press retains selected blue", before, tilePixel("header-control-$brush"))
            event(MotionEvent.ACTION_UP); idle()
            down("header-control-$filters")
            val action = tilePixel("header-control-$filters")
            assertTrue("Action press stays neutral", kotlin.math.abs(android.graphics.Color.blue(action) - android.graphics.Color.red(action)) < 15)
            shot("$theme-action-feedback")
            event(MotionEvent.ACTION_UP); idle()
        }
        val committed = model().toString()
        val durable = capture()
        startEditor()
        drag("header-component-clock", Offset(bounds("header-item-5").right + 12 * density, center().y))
        drag("header-component-battery", Offset(bounds("title-bar").right - 12 * density, center().y))
        waitFor("native tablet status") { node("system-clock") != null && node("system-battery") != null }
        assertEquals(durable, capture())
        shot("sketch-status-preview")
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        assertFalse(editing()); assertEquals("Switch discards the temporary header", committed, model().toString())
        shot("sketch-default")
    }
    @Test fun selectionDrawerToolsModesAndRememberedIcons() {
        resetPreset("builtin:workspace:painter")
        fun header(command: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("command") == command
        }.getInt("id")
        fun command(id: String) = state().array("commands").objects().first { it.getString("id") == id }
        fun invoke(id: String) = action(obj("type" to "invoke", "command" to id))
        val select = header("select")
        tap(select)
        if (state().getJSONObject("customization").isNull("drawer")) tap(select)
        waitFor("Selection drawer opens") { !state().getJSONObject("customization").isNull("drawer") }
        assertEquals("[[\"tools\"],[\"tool_settings\"]]", state().getJSONObject("customization").getJSONObject("drawer").getJSONArray("columns").toString())
        val choices = state().getJSONObject("tool_set").array("subtools").objects()
        assertEquals(8, choices.size)
        val modes = listOf("selection_new", "selection_add", "selection_subtract", "selection_intersect")
        for ((i, choice) in choices.withIndex()) {
            tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[i % 3]
            val tag = "subtool-${choice.getString("label")}"
            assertTrue("Touch-friendly tool row", bounds(tag).height / density >= 48f)
            tap(tag)
            assertEquals(choice.getString("icon"), command("select").getString("icon"))
            assertFalse(state().getJSONObject("customization").isNull("drawer"))
            val brush = choice.getString("icon") == "selection-brush"
            assertNotNull(node("tool-setting-" + if(brush)"selection_brush_size" else "selection_feather"))
            if(!brush && choice.getString("icon")!="tonal-select") assertNotNull(node("tool-action-selection_antialias"))
            val availableModes = if(brush) listOf("selection_add", "selection_subtract") else modes
            val row = bounds("selection-mode-row")
            for (id in availableModes) {
                val button = bounds("tool-action-$id")
                assertEquals(row.top, button.top, 1f)
                assertTrue(button.right <= row.right + 1f)
                tap("tool-action-$id")
                assertTrue(command(id).getBoolean("selected"))
                assertEquals(1, availableModes.count { command(it).getBoolean("selected") })
                assertNull("Modes have no caption", node("tool-action-$id")!!.second.config.getOrNull(SemanticsProperties.Text))
            }
        }
        invoke("rectangle_select"); tap("tool-action-selection_new"); tap("tool-action-selection_fixed_size")
        assertNotNull(node("tool-setting-selection_width")); assertNotNull(node("tool-setting-selection_height"))
        tap("tool-action-selection_fixed_size")
        invoke("color_select")
        for (id in listOf("tolerance", "expansion", "smoothing")) assertNotNull(node("tool-setting-$id"))
        assertNotNull(node("tool-action-group-selection-source"))
        for (id in listOf("selection_visible", "selection_editing", "selection_reference")) {
            tap("tool-action-group-selection-source")
            waitFor("Selection source option $id") { node("tool-action-$id") != null }
            if (command(id).getBoolean("enabled")) {
                tap("tool-action-$id")
                assertTrue(command(id).getBoolean("selected"))
            } else key(KeyEvent.KEYCODE_ESCAPE)
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme)); shot("selection-drawer-$theme")
        }
        tap(header("drawing_brush"))
        assertEquals("color-select", command("select").getString("icon"))
        tap(select); assertTrue(command("color_select").getBoolean("selected"))
        tap(select)
        resetPreset("builtin:workspace:photographer")
        for (slot in listOf("marquee", "lasso", "automatic_selection"))
            assertTrue("Photo toolbar $slot", layout().contains("\"slot\":\"$slot\""))
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        idle(); tap(select)
        assertTrue(command("color_select").getBoolean("selected"))
    }

    @Test fun commandGroupsProjectTheirOwnChoicesIconsAndSelectionScope() {
        device.landscape(scenario); idle()
        fun tiles() = snapshot().array("panels").objects().first { it.getString("id") == "toolbar" }.array("tiles").objects()
        fun command(id: String) = state().array("commands").objects().first { it.getString("id") == id }
        fun header(id: Int) = snapshot().getJSONObject("header").array("items").objects().first { it.getInt("id") == id }
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun rowCommand(row: JSONObject) = row.getJSONObject("action").let {
            if (it.optString("type") == "choose_tool_variant") it.getJSONObject("variant").optString("command") else it.optString("command")
        }
        fun choose(anchor: JSONObject, tag: String, row: JSONObject, capture: String? = null) {
            assertTrue(row.optBoolean("enabled", true))
            openToolContext(tag)
            capture?.let { idle(); shot(it) }
            tapMenuRow(row.getString("label"))
            waitFor("group menu closes") { node("workspace-menu") == null }
            idle()
            assertEquals(row.getString("label"), variantRows(anchor).single { it.optBoolean("selected") }.getString("label"))
        }
        fun brushGroup(row: JSONObject) = row.getJSONObject("action").getJSONObject("variant").optString("group")
        val mediaIcons = mapOf("pen" to "pen", "pencil" to "pencil", "pastel" to "pastel", "paint" to "paint",
            "watercolor" to "watercolor", "oil" to "oil-paint", "airbrush" to "airbrush")
        val media = mapOf("pen" to listOf("Marker" to "marker"), "pencil" to listOf("Pastel" to "pastel"),
            "brush" to listOf("Watercolor" to "watercolor", "Oil paint" to "oil-paint"), "airbrush" to listOf("Spray" to "spray"))
        val mediaGroups = mapOf("pencil" to setOf("pencil", "pastel"), "brush" to setOf("paint", "watercolor", "oil"))
        val categories = host.catalog.array("brush_categories").objects()
        val presets = categories.flatMap { it.array("brushes").objects() }
        assertEquals(13, categories.size)
        assertEquals(65, presets.size)
        assertEquals(65, presets.map { it.getInt("id") }.toSet().size)
        resetPreset("builtin:workspace:illustrator")
        waitFor("Paint command groups") { snapshot().array("panels").objects().firstOrNull { it.getString("id") == "toolbar" }
            ?.array("tiles")?.objects()?.any { it.getJSONObject("control").optString("command") == "pen" } == true }
        val paintCommands = tiles().map { it.getJSONObject("control").optString("command") }
            .filter { it in listOf("pen", "pencil", "brush", "airbrush", "eraser", "blend", "liquify", "clone", "heal", "spot_heal") }
        assertTrue(paintCommands.containsAll(listOf("pen", "pencil", "brush", "airbrush", "eraser")))
        edit(obj("type" to "edit", "editing" to true))
        for (entry in entries().filter { it.getJSONObject("item").getString("kind") !in listOf("capy", "settings") })
            edit(obj("type" to "remove", "id" to entry.getInt("id")))
        val headers = paintCommands.associateWith { id ->
            val headerId = model().getInt("next_id")
            edit(obj("type" to "add", "zone" to "left", "before" to null,
                "item" to obj("kind" to "tool", "control" to obj("kind" to "command", "command" to id))))
            headerId
        }
        val leafId = model().getInt("next_id")
        edit(obj("type" to "add", "zone" to "left", "before" to null,
            "item" to obj("kind" to "tool", "control" to obj("kind" to "brush", "id" to state().getJSONObject("brush").getInt("preset")))))
        edit(obj("type" to "set_size", "size" to "medium")); edit(obj("type" to "edit", "editing" to false)); idle()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            assertFalse(header(leafId).getBoolean("has_variants"))
            assertNull(node("header-variants-$leafId"))
            for ((index, id) in paintCommands.withIndex()) {
                tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index % 3]
                val tileId = tiles().first { it.getJSONObject("control").optString("command") == id }.getInt("id")
                fun tile() = tiles().first { it.getInt("id") == tileId }
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                val headerAnchor = obj("kind" to "header", "id" to headers.getValue(id))
                action(obj("type" to "invoke", "command" to "hand"))
                assertFalse(tile().getBoolean("selected"))
                if (id == "eraser") {
                    assertFalse(tile().getBoolean("has_variants")); assertFalse(header(headers.getValue(id)).getBoolean("has_variants"))
                    assertNull(node("tile-variants-toolbar-$tileId")); assertNull(node("header-variants-${headers.getValue(id)}"))
                    for ((opener, expectedAnchor) in listOf("tile-toolbar-$tileId" to anchor, "header-control-${headers.getValue(id)}" to headerAnchor)) {
                        action(obj("type" to "invoke", "command" to "hand")); tap(opener)
                        assertTrue(command(id).getBoolean("selected")); assertNull(drawer())
                        tap(opener)
                        waitFor("Eraser settings drawer") { drawer()?.getJSONObject("anchor")?.toString() == expectedAnchor.toString() }
                        assertEquals("[[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
                        assertEquals(0, state().getJSONObject("tool_set").array("groups").length())
                        assertEquals(0, state().getJSONObject("tool_set").array("subtools").length())
                        tap(opener); waitFor("Eraser settings close") { drawer() == null }
                    }
                    continue
                }
                assertTrue(tile().getBoolean("has_variants")); assertTrue(header(headers.getValue(id)).getBoolean("has_variants"))
                assertTrue(bounds("tile-variants-toolbar-$tileId").width >= 14 * density)
                val rows = variantRows(anchor)
                assertTrue(rows.isNotEmpty())
                mediaGroups[id]?.let { assertEquals(it, rows.map(::brushGroup).toSet()) }
                    ?: assertTrue(rows.all { it.getJSONObject("action").getJSONObject("variant").getString("type") == "brush_preset" })
                assertEquals(rows.map { it.getString("label") }, variantRows(headerAnchor).map { it.getString("label") })
                if (id == "pen" || id == "airbrush") assertEquals(if (id == "pen") 10 else 2, rows.size)
                val choices = media[id]?.map { (label, icon) -> rows.firstOrNull { it.getString("label") == label && it.getString("icon") == icon }
                    ?: error("$id lacks $label with $icon: $rows") }
                    ?: listOf(rows.first { it.optBoolean("enabled", true) })
                for (choice in choices) {
                    val variant = choice.getJSONObject("action").getJSONObject("variant")
                    if (variant.getString("type") == "brush_preset") {
                        val presetIcon = presets.single { it.getInt("id") == variant.getInt("id") }.getString("icon")
                        assertEquals(presetIcon, choice.getString("icon"))
                    }
                    choose(anchor, "tile-variants-toolbar-$tileId", choice, "paint-menu-$id-${choice.getString("icon")}-$theme")
                    waitFor("$id chosen medium published") { command(id).getBoolean("selected") && tile().getString("icon") == choice.getString("icon") &&
                        header(headers.getValue(id)).getString("icon") == choice.getString("icon") }
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    assertEquals(tile().getString("icon"), header(headers.getValue(id)).getString("icon"))
                    assertEquals(tile().getString("tooltip"), header(headers.getValue(id)).getString("label"))
                    val preset = state().getJSONObject("brush").getInt("preset")
                    action(obj("type" to "invoke", "command" to "hand"))
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    assertEquals(choice.getString("icon"), header(headers.getValue(id)).getString("icon"))
                    choose(headerAnchor, "header-variants-${headers.getValue(id)}", choice)
                    assertEquals(preset, state().getJSONObject("brush").getInt("preset"))
                    action(obj("type" to "invoke", "command" to "hand")); tap("tile-toolbar-$tileId")
                    waitFor("$id body activates remembered medium") { command(id).getBoolean("selected") && tile().getBoolean("selected") && header(headers.getValue(id)).getBoolean("selected") }
                    assertEquals(preset, state().getJSONObject("brush").getInt("preset"))
                    assertEquals(choice.getString("icon"), tile().getString("icon"))
                    shot("paint-$id-${choice.getString("icon")}-$theme")
                }
            }
            for ((slot, expected) in listOf("manual_selection" to setOf("lasso", "rectangle_select", "ellipse_select", "polygon_select", "selection_brush"),
                "automatic_selection" to setOf("auto_select", "color_select", "tonal_select"))) {
                val tileId = tiles().first { it.getJSONObject("control").optString("slot") == slot }.getInt("id")
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                action(obj("type" to "invoke", "command" to "hand"))
                val rows = variantRows(anchor)
                assertEquals(expected, rows.map(::rowCommand).toSet())
                choose(anchor, "tile-variants-toolbar-$tileId", rows.first(), "paint-menu-$slot-$theme")
                waitFor("$slot choice published") { command(rowCommand(rows.first())).getBoolean("selected") &&
                    state().getJSONObject("tool_set").array("subtools").objects().map(::rowCommand).toSet() == expected }
                assertEquals(expected, state().getJSONObject("tool_set").array("subtools").objects().map(::rowCommand).toSet())
                tap("tile-toolbar-$tileId")
                waitFor("$slot drawer") { drawer()?.objectOrNull("tool_set") != null && node("tool-drawer") != null }
                assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                assertEquals(expected, drawer()!!.getJSONObject("tool_set").array("groups").objects().map(::rowCommand).toSet())
                val sibling = drawer()!!.getJSONObject("tool_set").array("groups").objects().first { !it.optBoolean("selected") }
                waitFor("selection sibling laid out") { (node("tool-drawer")?.second?.find(hasTag("tool-group-${sibling.getString("label")}"))?.boundsInRoot?.height ?: 0f) >= 42 * density }
                instrumentation.runOnMainSync {
                    val surface = node("tool-drawer")!!
                    val row = surface.second.find(hasTag("tool-group-${sibling.getString("label")}"))!!.find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                    pressed = surface.first; point = row.boundsInRoot.center
                }
                event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP); idle()
                waitFor("$slot sibling published") { command(rowCommand(sibling)).getBoolean("selected") && drawer()?.getJSONObject("anchor")?.toString() == anchor.toString() }
                assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                shot("paint-$slot-$theme"); tap("tile-toolbar-$tileId")
                waitFor("selection drawer closes") { drawer() == null }
            }
        }
        for ((workspace, slots) in listOf("illustrator" to listOf("fill", "blend", "retouch"), "photographer" to listOf("drawing", "healing", "photo_fill"))) {
            resetPreset("builtin:workspace:$workspace")
            for (theme in listOf("light", "dark")) for (slot in slots) {
                action(obj("type" to "set_theme", "theme" to theme))
                val tileId = tiles().first { it.getJSONObject("control").optString("slot") == slot }.getInt("id")
                val anchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
                val rows = variantRows(anchor)
                choose(anchor, "tile-variants-toolbar-$tileId", rows.first())
                val flat = slot in listOf("retouch", "healing")
                val section = if (flat) "subtools" else "groups"
                val prefix = if (flat) "subtool" else "tool-group"
                if (flat) assertEquals(0, state().getJSONObject("tool_set").array("groups").length())
                if (slot == "retouch") assertEquals(3, rows.size)
                val docked = state().getJSONObject("tool_set").array(section).objects()
                assertEquals(rows.map { it.getString("label") }, docked.map { it.getString("label") })
                assertEquals(rows.map { it.optBoolean("enabled", true) }, docked.map { it.optBoolean("enabled", true) })
                val sibling = rows[1].getString("label")
                if (workspace == "photographer" && node("$prefix-$sibling") == null) tap("column-icon-brushes")
                waitFor("$slot panel sibling laid out") { (node("$prefix-$sibling")?.second?.boundsInRoot?.height ?: 0f) >= 42 * density }
                tap("$prefix-$sibling")
                waitFor("$slot panel sibling published") { state().getJSONObject("tool_set").array(section).objects().any { it.getString("label") == sibling && it.optBoolean("selected") } }
                assertEquals(rows.map { it.getString("label") }, state().getJSONObject("tool_set").array(section).objects().map { it.getString("label") })
                assertEquals(sibling, variantRows(anchor).single { it.optBoolean("selected") }.getString("label"))
                shot("$workspace-$slot-panel-$theme")
            }
        }
        resetPreset("builtin:workspace:painter")
        waitFor("Sketch groups") { entries().firstOrNull { it.getJSONObject("item").objectOrNull("control")?.optString("command") == "drawing_brush" }
            ?.let { node("header-variants-${it.getInt("id")}") != null } == true }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((index, id) in listOf("drawing_brush", "sculpt", "select").withIndex()) {
                tool = listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)[index]
                val headerId = entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("command") == id }.getInt("id")
                val anchor = obj("kind" to "header", "id" to headerId)
                action(obj("type" to "invoke", "command" to "hand"))
                assertTrue(header(headerId).getBoolean("has_variants")); assertFalse(header(headerId).getBoolean("selected"))
                val rows = variantRows(anchor)
                if (id == "drawing_brush") assertEquals(mediaIcons.keys, rows.map(::brushGroup).toSet())
                if (id == "sculpt") assertEquals(setOf("blend", "wet_paint", "effects", "liquify", "retouch"), rows.map(::brushGroup).toSet())
                val choices = if (id == "drawing_brush") mediaIcons.keys.map { group -> rows.firstOrNull { brushGroup(it) == group } ?: error("Sketch lacks $group: $rows") }
                    else listOf(rows.first { it.optBoolean("enabled", true) })
                if (id == "select") assertEquals(setOf("rectangle_select", "ellipse_select", "lasso", "polygon_select", "auto_select", "color_select", "selection_brush", "tonal_select"), rows.map(::rowCommand).toSet())
                for (choice in choices) {
                    mediaIcons[brushGroup(choice)]?.let { assertEquals(it, choice.getString("icon")) }
                    choose(anchor, "header-variants-$headerId", choice, "sketch-menu-$id-${choice.getString("icon")}-$theme")
                    val selectedIcon = if (choice.getJSONObject("action").getJSONObject("variant").getString("type") == "brush_group")
                        presets.single { it.getInt("id") == state().getJSONObject("brush").getInt("preset") }.getString("icon") else choice.getString("icon")
                    waitFor("Sketch $id chosen group published") { command(id).getBoolean("selected") && header(headerId).getString("icon") == selectedIcon }
                    assertEquals(selectedIcon, header(headerId).getString("icon"))
                    tap("header-control-$headerId")
                    waitFor("Sketch $id grouped drawer") { drawer() != null && node("tool-drawer") != null }
                    assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                    if (id != "select") assertEquals("[[\"${if (id == "drawing_brush") "brush_sets" else "sculpt_sets"}\"],[\"tools\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
                    shot("sketch-$id-${choice.getString("icon")}-$theme")
                    tap("header-control-$headerId"); waitFor("Sketch group drawer closes") { drawer() == null }
                }
            }
        }
    }

    @Test fun toolVariantCornersAndContextMenusShareRememberedChoices() {
        device.landscape(scenario); idle()
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        waitFor("Photo grouped tools published") {
            view().optString("id") == "builtin:workspace:photographer" && snapshot().array("panels").objects()
                .firstOrNull { it.getString("id") == "toolbar" }?.array("tiles")?.objects()
                ?.any { it.getJSONObject("control").optString("slot") == "lasso" } == true
        }
        edit(obj("type" to "edit", "editing" to true))
        for (entry in entries().filter { it.getJSONObject("item").getString("kind") !in listOf("capy", "settings") }) {
            edit(obj("type" to "remove", "id" to entry.getInt("id")))
        }
        val headerId = model().getInt("next_id")
        edit(obj("type" to "add", "zone" to "left", "before" to null,
            "item" to obj("kind" to "tool", "control" to obj("kind" to "tool_slot", "slot" to "lasso"))))
        edit(obj("type" to "set_size", "size" to "medium"))
        edit(obj("type" to "edit", "editing" to false))
        idle()
        fun tile() = snapshot().array("panels").objects().first { it.getString("id") == "toolbar" }
            .array("tiles").objects().first { it.getJSONObject("control").optString("slot") == "lasso" }
        fun header() = snapshot().getJSONObject("header").array("items").objects().first { it.getInt("id") == headerId }
        val tileId = tile().getInt("id")
        val ribbonAnchor = obj("kind" to "tile", "panel" to "toolbar", "tile" to tileId)
        val headerAnchor = obj("kind" to "header", "id" to headerId)
        val stable = tile().getJSONObject("control").toString()
        fun rows(anchor: JSONObject) = variantRows(anchor)
        fun sameChoice() {
            assertEquals(stable, tile().getJSONObject("control").toString())
            assertEquals(tile().getString("tooltip"), header().getString("label"))
            instrumentation.runOnMainSync {
                for (tag in listOf("header-variants-$headerId", "tile-variants-toolbar-$tileId")) {
                    val marker = node(tag)!!.second.config
                    assertNull(marker.getOrNull(SemanticsActions.OnClick))
                    assertNull(marker.getOrNull(SemanticsActions.OnLongClick))
                    assertNull(marker.getOrNull(SemanticsProperties.ContentDescription))
                }
            }
            assertEquals(tile().getString("icon"), header().getString("icon"))
            assertEquals(tile().getJSONObject("resolved_control").toString(), header().getJSONObject("resolved_control").toString())
            assertEquals(1, rows(ribbonAnchor).count { it.optBoolean("selected") })
            assertEquals(1, rows(headerAnchor).count { it.optBoolean("selected") })
        }
        val raster = android.graphics.Bitmap.createBitmap(256, 256, android.graphics.Bitmap.Config.ARGB_8888)
        val pixels = try {
            val source = instrumentation.targetContext.assets.open("layer-tool-group-symbolic.svg").bufferedReader().use { it.readText() }
            val picture = com.caverock.androidsvg.SVG.getFromString(source.replace("currentColor", "#000000")).renderToPicture()
            android.graphics.Canvas(raster).drawPicture(picture, android.graphics.RectF(0f, 0f, 256f, 256f))
            (0 until raster.height).flatMap { y -> (0 until raster.width).filter { x -> android.graphics.Color.alpha(raster.getPixel(x, y)) > 0 }
                .map { x -> Offset((x + .5f) / raster.width, (y + .5f) / raster.height) } }.also { painted ->
                assertTrue("Group marker paints pixels", painted.isNotEmpty())
                assertTrue("Painted right inset is at least six SVG pixels", (1f - painted.maxOf { it.x } - .5f / raster.width) * 16 >= 6)
                assertTrue("Painted bottom inset is at least six SVG pixels", (1f - painted.maxOf { it.y } - .5f / raster.height) * 16 >= 6)
            }
        } finally { raster.recycle() }
        for (theme in listOf("light", "dark")) for (size in listOf("small", "medium", "large")) {
            action(obj("type" to "set_theme", "theme" to theme))
            edit(obj("type" to "set_size", "size" to size)); idle()
            waitFor("$size marker header published") { model().getString("size") == size && node("header-variants-$headerId") != null }
            val target = bounds("header-variants-$headerId")
            val body = bounds("header-control-$headerId")
            val outline = TileShape.createOutline(androidx.compose.ui.geometry.Size(body.width, body.height),
                androidx.compose.ui.unit.LayoutDirection.Ltr, androidx.compose.ui.unit.Density(density)) as androidx.compose.ui.graphics.Outline.Generic
            val region = android.graphics.Region().apply {
                setPath(outline.path.asAndroidPath(), android.graphics.Region(0, 0, kotlin.math.ceil(body.width).toInt(), kotlin.math.ceil(body.height).toInt()))
            }
            val painted = pixels.map { target.topLeft + Offset(it.x * target.width, it.y * target.height) }
            assertTrue("$size marker stays within its click target", painted.all(target::contains))
            assertTrue("$size marker stays within the header squircle", painted.all {
                body.contains(it) && region.contains((it.x - body.left).toInt(), (it.y - body.top).toInt()) })
            tool = MotionEvent.TOOL_TYPE_FINGER
            fun tapPaintedMarker() {
                instrumentation.runOnMainSync { pressed = node("header-variants-$headerId")!!.first }
                event(MotionEvent.ACTION_DOWN, Offset(painted.map { it.x }.average().toFloat(), painted.map { it.y }.average().toFloat()))
                event(MotionEvent.ACTION_UP); idle()
                assertNull("Marker tap has no context menu", node("workspace-menu"))
            }
            action(obj("type" to "invoke", "command" to "eraser"))
            tapPaintedMarker()
            waitFor("$size marker selects its owner") { header().getBoolean("selected") && state().getJSONObject("customization").isNull("drawer") }
            shot("tool-marker-$size-$theme")
            tapPaintedMarker()
            waitFor("$size marker opens its owner drawer") { node("tool-drawer") != null &&
                state().getJSONObject("customization").objectOrNull("drawer")?.getJSONObject("anchor")?.toString() == headerAnchor.toString() }
            tapPaintedMarker()
            waitFor("$size marker closes its owner drawer") { node("tool-drawer") == null }
        }
        edit(obj("type" to "set_size", "size" to "medium")); idle()
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for ((index, nativeTool) in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS).withIndex()) {
                tool = nativeTool
                for ((anchor, tag) in listOf(ribbonAnchor to "tile-variants-toolbar-$tileId", headerAnchor to "header-variants-$headerId")) {
                    assertTrue("Marker remains inside the owner", bounds(tag).width >= 14 * density && bounds(tag).height >= 14 * density)
                    val choices = rows(anchor)
                    assertTrue(choices.size > 1)
                    val alternatives = choices.filter { it.optBoolean("enabled", true) && !it.optBoolean("selected") }
                    val choice = alternatives[index % alternatives.size]
                    openToolContext(tag)
                    tapMenuRow(choice.getString("label"))
                    waitFor("variant popup closes") { node("workspace-menu") == null }
                    idle(); sameChoice()
                    assertEquals(choice.getString("label"), rows(anchor).first { it.optBoolean("selected") }.getString("label"))
                    action(obj("type" to "invoke", "command" to "eraser"))
                    tap(tag)
                    assertNull("Corner activates the owner without a menu", node("workspace-menu"))
                    host.awaitMain("remembered body choice activates", 15_000, {
                        shot("failure-remembered-body-choice")
                        "tile=${tile()}; header=${header()}; brush=${state().getJSONObject("brush")}; layer_tools=${state().objectOrNull("layer_tools")}"
                    }) { tile().getBoolean("selected") && header().getBoolean("selected") }
                    sameChoice()
                    tap(tag)
                    assertNull("Corner reclick opens the drawer without a menu", node("workspace-menu"))
                    fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
                    waitFor("full grouped drawer") { node("tool-drawer") != null && drawer()?.objectOrNull("tool_set") != null }
                    assertEquals("[[\"brushes\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
                    assertEquals(anchor.toString(), drawer()!!.getJSONObject("anchor").toString())
                    val sibling = drawer()!!.getJSONObject("tool_set").array("groups").objects().first { it.optBoolean("enabled", true) && !it.optBoolean("selected") }
                    val siblingTag = "tool-group-${sibling.getString("label")}"
                    waitFor("drawer sibling laid out") {
                        val row = node("tool-drawer")?.second?.find(hasTag(siblingTag))
                        (row?.boundsInRoot?.height ?: 0f) >= 42 * density
                    }
                    instrumentation.runOnMainSync {
                        val drawerNode = checkNotNull(node("tool-drawer"))
                        val row = checkNotNull(drawerNode.second.find(hasTag(siblingTag)))
                            .find { it.config.getOrNull(SemanticsActions.OnClick) != null }!!
                        pressed = drawerNode.first; point = row.boundsInRoot.center
                    }
                    event(MotionEvent.ACTION_DOWN); event(MotionEvent.ACTION_UP)
                    waitFor("drawer sibling keeps its opener") {
                        drawer()?.getJSONObject("anchor")?.toString() == anchor.toString() &&
                            drawer()?.getJSONObject("tool_set")?.array("groups")?.objects()?.any { it.optBoolean("selected") && it.getString("label") == sibling.getString("label") } == true
                    }
                    idle(); sameChoice()
                    tap(tag)
                    waitFor("grouped drawer closes") { node("tool-drawer") == null }
                }
                openToolContext("header-control-$headerId")
                val selected = rows(headerAnchor).first { it.optBoolean("selected") }
                tapMenuRow(selected.getString("label"))
                waitFor("full context closes") { node("workspace-menu") == null }
                button = MotionEvent.BUTTON_PRIMARY
                idle(); sameChoice()
            }
            shot("tool-variants-$theme")
        }
        val crowded = mutableListOf<Int>()
        while (node("header-item-$headerId") != null) {
            edit(obj("type" to "edit", "editing" to true))
            repeat(4) {
                assertTrue("Grouped item can overflow", crowded.size < 30)
                crowded.add(model().getInt("next_id"))
                edit(obj("type" to "add", "zone" to "left", "before" to headerId,
                    "item" to obj("kind" to "tool", "control" to obj("kind" to "command", "command" to "pen"))))
            }
            edit(obj("type" to "edit", "editing" to false)); idle()
        }
        waitFor("runtime grouped item overflow") { node("header-overflow-0") != null && node("header-item-$headerId") == null }
        for ((theme, nativeTool) in listOf("light" to MotionEvent.TOOL_TYPE_MOUSE, "dark" to MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme", "theme" to theme)); action(obj("type" to "invoke", "command" to "eraser"))
            tool = nativeTool
            tap("header-overflow-0")
            waitFor("overflow group marker") { node("header-overflow-variants-$headerId") != null }
            tap("header-overflow-item-$headerId")
            waitFor("overflow body activates its row") { header().getBoolean("selected") && node("header-overflow-list") == null }
            action(obj("type" to "invoke", "command" to "eraser"))
            tap("header-overflow-0")
            waitFor("overflow group marker returns") { node("header-overflow-variants-$headerId") != null }
            val marker = node("header-overflow-variants-$headerId")!!.second.config
            assertNull(marker.getOrNull(SemanticsActions.OnClick)); assertNull(marker.getOrNull(SemanticsActions.OnLongClick))
            tap("header-overflow-variants-$headerId")
            waitFor("overflow corner activates its row") { header().getBoolean("selected") && node("header-overflow-list") == null }
            assertNull("Overflow corner has no context menu", node("workspace-menu"))
            shot("tool-marker-overflow-$theme")
        }
        edit(obj("type" to "edit", "editing" to true))
        crowded.forEach { edit(obj("type" to "remove", "id" to it)) }
        edit(obj("type" to "edit", "editing" to false)); idle()
        val remembered = tile().getString("label")
        send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
        send(obj("type" to "switch", "id" to "builtin:workspace:photographer"))
        waitFor("remembered Photo tools published") {
            view().optString("id") == "builtin:workspace:photographer" && snapshot().array("panels").objects()
                .firstOrNull { it.getString("id") == "toolbar" }?.array("tiles")?.objects()
                ?.any { it.getJSONObject("control").optString("slot") == "lasso" && it.getString("label") == remembered } == true
        }
        assertEquals(remembered, tile().getString("label")); sameChoice()
    }

    @Test fun brushAndSculptDrawersKeepIndependentSelections() {
        resetPreset("builtin:workspace:painter")
        waitFor("Sketch drawer header ready") {
            view().optString("id") == "builtin:workspace:painter" && entries().firstOrNull {
                it.getJSONObject("item").objectOrNull("control")?.optString("command") == "drawing_brush"
            }?.let { node("header-control-${it.getInt("id")}") != null } == true
        }
        fun header(command: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("command") == command
        }.getInt("id")
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun checkColumns(first: String) {
            assertEquals("[[\"$first\"],[\"tools\"],[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
            val set = state().getJSONObject("tool_panels").getJSONObject(first).array("groups").objects().first().getString("label")
            val choice = state().getJSONObject("tool_set").array("subtools").objects().first().getString("label")
            waitFor("$first drawer content laid out") {
                (node("$first-$set")?.second?.boundsInRoot?.height ?: 0f) >= 48 * density &&
                    (node("subtool-$choice")?.second?.boundsInRoot?.width ?: 0f) > 0f
            }
            val a = bounds("$first-$set"); val b = bounds("subtool-$choice")
            assertTrue("Sets $a should be narrower than tools $b", a.width < b.width)
            assertTrue(state().array("tool_settings").length() > 0)
            assertNull("Tools has no category headers", node("tool-group-" + if(first == "sculpt_sets") "Blend" else "Paint"))
        }
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            val brush = header("drawing_brush")
            val sculpt = header("sculpt")
            tap(brush)
            if (drawer() == null) tap(brush)
            checkColumns("brush_sets")
            assertEquals(listOf("Pen", "Pencil", "Pastel", "Paint", "Watercolor", "Oil paint", "Airbrush"), state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").objects().map { it.getString("label") })
            assertFalse(state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").objects().any { it.getString("label") in listOf("Eraser", "Blend", "Liquify") })
            for ((device, label) in listOf(MotionEvent.TOOL_TYPE_MOUSE to "Pencil", MotionEvent.TOOL_TYPE_FINGER to "Pastel", MotionEvent.TOOL_TYPE_STYLUS to "Paint")) {
                tool = device
                assertTrue(bounds("brush_sets-$label").height / density >= 48f)
                tap("brush_sets-$label")
                checkColumns("brush_sets")
                val choice = state().getJSONObject("tool_set").array("subtools").objects().first()
                tap("subtool-${choice.getString("label")}")
                assertEquals(choice.getInt("preview"), state().getJSONObject("brush").getInt("preset"))
            }
            action(obj("type" to "set_brush_size", "value" to 37))
            val drawing = state().getJSONObject("brush").getInt("preset")
            shot("brush-drawer-$theme")
            tap(sculpt)
            checkColumns("sculpt_sets")
            assertEquals(listOf("Blend", "Paint mixing", "Effects", "Liquify", "Retouch"), state().getJSONObject("tool_panels").getJSONObject("sculpt_sets").array("groups").objects().map { it.getString("label") })
            for (device in listOf(MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
                tool = device
                for (label in listOf("Liquify", "Blend")) {
                    tap("sculpt_sets-$label"); checkColumns("sculpt_sets")
                    assertEquals(label.lowercase(), state().getJSONObject("brush").getString("tool"))
                }
            }
            tap("sculpt_sets-Liquify")
            action(obj("type" to "set_brush_size", "value" to 79))
            val sculpting = state().getJSONObject("brush").getInt("preset")
            shot("sculpt-drawer-$theme")
            tap(brush)
            assertEquals(drawing, state().getJSONObject("brush").getInt("preset"))
            assertEquals(37.0, state().getJSONObject("brush").getDouble("diameter"), .01)
            tap(header("eraser"))
            assertEquals("[[\"tool_settings\"]]", drawer()!!.getJSONArray("columns").toString())
            assertEquals(0, state().getJSONObject("tool_set").array("groups").length())
            assertEquals(0, state().getJSONObject("tool_set").array("subtools").length())
            assertNull("Eraser has no category header", node("tool-group-Eraser"))
            shot("eraser-drawer")
            assertFalse(state().array("commands").objects().filter { it.getString("id") in listOf("drawing_brush", "sculpt") }.any { it.getBoolean("selected") })
            tap(sculpt)
            assertEquals(sculpting, state().getJSONObject("brush").getInt("preset"))
            assertEquals(79.0, state().getJSONObject("brush").getDouble("diameter"), .01)
            tap(sculpt); assertNull(drawer())
            send(obj("type" to "switch", "id" to "builtin:workspace:illustrator"))
            send(obj("type" to "switch", "id" to "builtin:workspace:painter"))
            // The workspace is adopted after send's publication fence. Wait for its
            // native header measurements before delivering the next contact.
            idle()
            tap(brush)
            assertEquals(drawing, state().getJSONObject("brush").getInt("preset"))
            assertNull("First contact selects Brush", drawer())
            tap(brush); checkColumns("brush_sets")
        }
    }

    private fun showSwipeLayers() {
        resetPreset("builtin:workspace:illustrator")
    }

    @Test fun layerInputOwnership() {
        host.newDocument(2048,1536)
        repeat(2) { action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false))) }
        showSwipeLayers()
        fun order()=state().array("layers").objects().map { it.getLong("id") }
        val id=order().first()
        for(theme in listOf("light","dark")) for(pointer in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS,MotionEvent.TOOL_TYPE_MOUSE)) {
            action(obj("type" to "set_theme","theme" to theme));tool=pointer
            val before=order()
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE) {
                down("layer-row-$id")
                SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong()+150)
                waitFor("layer body hold menu") { node("workspace-menu")!=null }
                event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density))
                assertNull("Held body cannot pick up",node("layer-drag-preview"))
                event(MotionEvent.ACTION_CANCEL)
                waitFor("held menu cancelled") { node("workspace-menu")==null }
                assertEquals(before,order())
            }
            down(if(pointer==MotionEvent.TOOL_TYPE_MOUSE) "layer-row-$id" else "layer-grip-$id")
            event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density))
            waitFor("explicit layer pickup") { node("layer-drag-preview")!=null }
            shot("layer-input-$theme-$pointer")
            event(MotionEvent.ACTION_CANCEL);idle();assertEquals(before,order())
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE) {
                val actionWidth=bounds("layer-row-$id").width.coerceAtMost(216*density)/3
                down("layer-row-$id");val start=point
                event(MotionEvent.ACTION_MOVE,start+Offset(-36*density,0f))
                waitFor("Partial swipe buttons are laid out") { node("layer-delete-$id")!=null }
                instrumentation.runOnMainSync {
                    for(action in listOf("lock","duplicate","delete")) {
                        val button=checkNotNull(node("layer-$action-$id")) { "Missing partial-swipe $action button" }.second
                        assertEquals("Partial swipe preserves $action content width",actionWidth-8*density,button.size.width.toFloat(),1f)
                        if(action!="delete")assertEquals("Partial swipe keeps $action covered",0f,button.boundsInRoot.width,1f)
                    }
                    val delete=checkNotNull(node("layer-delete-$id")).second
                    assertEquals("Partial swipe reveals the trailing button",32*density,delete.boundsInRoot.width,1f)
                }
                shot("layer-partial-swipe-$theme-$pointer")
                for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(-80*density*i/4,0f))
                event(MotionEvent.ACTION_UP);idle()
                assertNotNull("Revealed row retains its grip for fading",node("layer-grip-$id"))
                assertNotNull(node("layer-delete-$id"))
                tool=MotionEvent.TOOL_TYPE_MOUSE
                down("layer-row-$id");event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density))
                assertNull("Swiped row cannot pick up",node("layer-drag-preview"))
                event(MotionEvent.ACTION_CANCEL);idle()
                tool=pointer
                down("layer-row-$id");val reverse=point
                for(i in 1..4)event(MotionEvent.ACTION_MOVE,reverse+Offset(90*density*i/4,0f))
                event(MotionEvent.ACTION_UP);idle()
                waitFor("returned layer grip") { node("layer-grip-$id")!=null }
                assertEquals(before,order())
            }
        }
    }

    @Test fun layerScrollbarsFollowNativePointer() {
        host.newDocument(2048,1536)
        repeat(18) { action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false))) }
        showSwipeLayers()
        val id=state().array("layers").objects().first().getLong("id")
        fun strip():List<Int> {
            val area=screenBounds("layer-rows")
            val image=instrumentation.uiAutomation.takeScreenshot()
            return try { (area.top.toInt()+2 until area.bottom.toInt()-2).map { image.getPixel((area.right-4*density).toInt(),it) } }
                finally { image.recycle() }
        }
        fun contact(pointer:Int) {
            tool=pointer;down("layer-row-$id");event(MotionEvent.ACTION_CANCEL);idle()
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            contact(MotionEvent.TOOL_TYPE_FINGER);val touch=strip();shot("layer-scrollbar-$theme-touch")
            contact(MotionEvent.TOOL_TYPE_MOUSE);val mouse=strip();shot("layer-scrollbar-$theme-mouse")
            assertTrue("Mouse shows native scrollbar",touch.zip(mouse).count { it.first!=it.second }>=20*density)
            contact(MotionEvent.TOOL_TYPE_STYLUS);val pen=strip();shot("layer-scrollbar-$theme-pen")
            assertEquals("Pen hides native scrollbar",touch,pen)
        }
    }

    @Test fun layerSwipeFrameTiming() {
        val args=androidx.test.platform.app.InstrumentationRegistry.getArguments()
        val reorder=args.getString("layerReorderBenchmark")=="true"
        val selection=args.getString("layerSelectionBenchmark")=="true"
        val properties=args.getString("layerPropertiesBenchmark")=="true"
        val discrete=selection || properties
        val swipeDirection=if(args.getString("layerSwipeDirection")=="left") -1 else 1
        org.junit.Assume.assumeTrue(discrete || reorder || args.getString("layerSwipeBenchmark")=="true")
        val width=args.getString("width")?.toInt() ?: 6000
        val height=args.getString("height")?.toInt() ?: 4000
        val label=if(properties) "layer-properties" else if(selection) "layer-selection" else if(reorder) "layer-reorder" else "layer-swipe"
        val selectionInterval=args.getString("layerSelectionIntervalMs")?.toLong() ?: 150L
        require(selectionInterval>0)
        val theme=args.getString("layerBenchmarkTheme") ?: "dark"
        host.openDocument(java.io.File(checkNotNull(args.getString("photo"))))
        action(obj("type" to "set_theme","theme" to theme))
        assertTrue(state().array("tabs").objects().any { it.optBoolean("active") && it.optInt("width")==width && it.optInt("height")==height })
        action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
        if(args.getString("layerRelationshipBenchmark")=="true") {
            val owner=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
            action(obj("type" to "layer","action" to obj("op" to "clip","id" to owner,"value" to true)))
            for(kind in listOf("gaussian_blur","curves")) {
                action(obj("type" to "effect","action" to obj("op" to "insert","effect" to kind)))
                val effect=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
                action(obj("type" to "layer","action" to obj("op" to "attach_effect","id" to effect,"owner" to owner)))
            }
            if(!reorder)action(obj("type" to "layer","action" to obj("op" to "select","id" to owner,"mask" to false)))
        }
        if(properties) {
            resetPreset("builtin:workspace:painter")
            tap("header-control-"+entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("panel")=="layers" }.getInt("id"))
            waitFor("benchmark layer inspector") { node("layer-inspector")!=null }
        } else showSwipeLayers()
        action(obj("type" to "invoke","command" to "fit_canvas"))
        val id=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        if(selection)action(obj("type" to "layer","action" to obj("op" to "add_mask","id" to id,"replace" to false)))
        var propertyContact:Pair<android.view.View,Offset>?=null
        if(properties)instrumentation.runOnMainSync {
            val target=checkNotNull(node("layer-properties-disclosure"))
            propertyContact=target.first.view to target.second.boundsInRoot.center
        }
        val frames=java.util.Collections.synchronizedList(mutableListOf<LongArray>())
        val thread=android.os.HandlerThread("$label-frames").apply { start() }
        lateinit var window: android.view.Window
        scenario.onActivity { window=it.window; window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
        val measureGeometry=properties || (!discrete && !reorder)
        val geometryFrames=mutableListOf<LongArray>()
        val arrangedFrames=mutableListOf<LongArray>()
        var collectGeometry=false
        var geometryContent:SemanticsNode?=null
        var geometryProperties:SemanticsNode?=null
        var geometryInspector:SemanticsNode?=null
        fun geometry():LongArray {
            val position=checkNotNull(geometryContent).positionInRoot
            return longArrayOf(System.nanoTime(),(position.x*1000).toLong(),(position.y*1000).toLong(),
                ((geometryProperties?.positionInRoot?.y ?: 0f)*1000).toLong(),
                ((geometryInspector?.boundsInRoot?.height ?: 0f)*1000).toLong(),SystemClock.elapsedRealtimeNanos())
        }
        instrumentation.runOnMainSync {
            if(measureGeometry) {
                geometryContent=checkNotNull(node(if(properties)"layer-rows" else "layer-content-$id")).second
                if(properties) {
                    geometryProperties=checkNotNull(node("layer-properties")).second
                    geometryInspector=checkNotNull(node("layer-inspector")).second
                }
            }
        }
        val listener=android.view.Window.OnFrameMetricsAvailableListener { _,metrics,dropped ->
            frames.add(longArrayOf(metrics.getMetric(android.view.FrameMetrics.VSYNC_TIMESTAMP),
                metrics.getMetric(android.view.FrameMetrics.TOTAL_DURATION),dropped.toLong(),
                metrics.getMetric(android.view.FrameMetrics.INTENDED_VSYNC_TIMESTAMP),
                metrics.getMetric(android.view.FrameMetrics.INPUT_HANDLING_DURATION),
                metrics.getMetric(android.view.FrameMetrics.ANIMATION_DURATION),
                metrics.getMetric(android.view.FrameMetrics.LAYOUT_MEASURE_DURATION),
                metrics.getMetric(android.view.FrameMetrics.DRAW_DURATION),
                metrics.getMetric(android.view.FrameMetrics.SYNC_DURATION),
                metrics.getMetric(android.view.FrameMetrics.COMMAND_ISSUE_DURATION),
                metrics.getMetric(android.view.FrameMetrics.SWAP_BUFFERS_DURATION)))
        }
        instrumentation.runOnMainSync { window.addOnFrameMetricsAvailableListener(listener,android.os.Handler(thread.looper)) }
        tool=if(reorder) MotionEvent.TOOL_TYPE_MOUSE else MotionEvent.TOOL_TYPE_FINGER
        try {
            awaitBenchmarkRefreshPolicy(args, checkNotNull(instrumentation.targetContext.getExternalFilesDir(null)), instrumentation, host, window.decorView)
            for(run in 0..3) {
                if(!discrete)down("layer-row-$id")
                val start=point
                if(!discrete)event(MotionEvent.ACTION_MOVE,start+Offset((if(reorder) -24 else 24*swipeDirection)*density,0f))
                if(reorder)waitFor("moving layer preview") { node("layer-drag-preview")!=null }
                val done=java.util.concurrent.CountDownLatch(1)
                val duration=args.getString("durationMs","6000")!!.toLong().also { require(it in 5000L..10000L) }
                val begin=SystemClock.uptimeMillis()
                val thermal=scenario.activity().getSystemService(android.os.PowerManager::class.java)
                val thermalBefore=thermal.currentThermalStatus
                val beginNs=System.nanoTime()
                val beginBoot=SystemClock.elapsedRealtimeNanos()
                val beginMonoAfterBoot=System.nanoTime()
                frames.clear()
                lateinit var callback: WindowFrames
                var selectedInterval=-1L
                fun selectedState()=if(properties)state().getJSONObject("customization").getJSONObject("drawer").getBoolean("layer_properties")
                    else state().array("layers").objects().first { it.getLong("id")==id }.getBoolean("mask_selected")
                var observedMask=selectedState()
                val transitions=mutableListOf<Long>()
                val transitionObservations=mutableListOf<Long>()
                instrumentation.runOnMainSync {
                    geometryFrames.clear()
                    arrangedFrames.clear()
                    if(measureGeometry) { geometryFrames.add(geometry()); collectGeometry=true }
                    callback=WindowFrames(onDraw = {
                        if(measureGeometry && collectGeometry)geometryFrames.add(geometry())
                    }) { _ ->
                        if(measureGeometry)arrangedFrames.add(geometry())
                        val elapsed=SystemClock.uptimeMillis()-begin
                        if(elapsed>=duration) { collectGeometry=false; done.countDown(); false }
                        else {
                            if(discrete) {
                                val mask=selectedState()
                                if(mask!=observedMask) { observedMask=mask;transitions.add(System.nanoTime());transitionObservations.add(SystemClock.elapsedRealtimeNanos()) }
                                val interval=elapsed/selectionInterval
                                if(interval!=selectedInterval) {
                                    selectedInterval=interval
                                    if(properties) {
                                        val target=checkNotNull(propertyContact)
                                        val center=target.second
                                        val time=SystemClock.uptimeMillis()
                                        for(kind in listOf(MotionEvent.ACTION_DOWN,MotionEvent.ACTION_UP)) {
                                            val contact=motion(MotionEvent.TOOL_TYPE_FINGER,kind,center,time,button)
                                            try { target.first.dispatchTouchEvent(contact) } finally { contact.recycle() }
                                        }
                                    } else host.dispatch(obj("type" to "layer","action" to obj("op" to "select","id" to id,"mask" to (interval%2==0L))))
                                }
                            } else {
                                val cycle=(elapsed%1000)/500f
                                val fraction=if(cycle<=1)cycle else 2-cycle
                                val delta=if(reorder) Offset(-24*density,40*density*fraction) else Offset((24+40*fraction)*density*swipeDirection,0f)
                                val move=motion(tool,MotionEvent.ACTION_MOVE,start+delta,downAt,button)
                                try { checkNotNull(pressed).view.dispatchTouchEvent(move) } finally { move.recycle() }
                            }
                            true
                        }
                    }
                    callback.watch(window.decorView)
                    callback.start()
                }
                try { assertTrue(done.await(15,java.util.concurrent.TimeUnit.SECONDS)) }
                finally { instrumentation.runOnMainSync { callback.close() } }
                val endNs=System.nanoTime()
                val endBoot=SystemClock.elapsedRealtimeNanos()
                val endMonoAfterBoot=System.nanoTime()
                SystemClock.sleep(200)
                val received=synchronized(frames) { frames.toList() }
                var observedGeometry=listOf<LongArray>()
                instrumentation.runOnMainSync { observedGeometry=geometryFrames.toList() }
                val samples=observedGeometry.sortedBy { it[0] }
                var arranged=listOf<LongArray>()
                instrumentation.runOnMainSync { arranged=arrangedFrames.toList() }
                val activeWindows=JSONArray()
                if(properties)for((index,transition) in transitions.withIndex()) {
                    val limit=transitions.getOrNull(index+1) ?: endNs
                    val changes=arranged.indices.filter { current -> current>0 && arranged[current][0]>=transition && arranged[current][0]<limit &&
                        (1..4).any { arranged[current][it]!=arranged[current-1][it] } }
                    if(changes.isNotEmpty()) {
                        val first=arranged[changes.first()-1]
                        val last=arranged[minOf(changes.last()+1,arranged.lastIndex)]
                        activeWindows.put(obj("begin_ns" to first[0],"end_ns" to last[0],
                            "begin_boot_ns" to first[5],"end_boot_ns" to last[5],
                            "kind" to "arranged_drawer_transition"))
                    }
                }
                var previous:LongArray?=null
                val moving=mutableSetOf<Long>()
                for(sample in samples) {
                    previous?.let { before -> if((1..4).any { sample[it]!=before[it] })moving.add(sample[0]) }
                    previous=sample
                }
                val windowFrames=received.filter { it[0] in beginNs..endNs }.associateBy { it[0] }.values.sortedBy { it[0] }
                val rows=windowFrames.filter { frame -> !discrete || if(properties)
                    (0 until activeWindows.length()).any { activeWindows.getJSONObject(it).let { window -> frame[0] in window.getLong("begin_ns")..window.getLong("end_ns") } }
                    else transitions.any { start -> frame[0] in start..(start+200_000_000L) } }
                if(!discrete)event(MotionEvent.ACTION_CANCEL)
                idle()
                if(discrete)shot("$label-$run")
                if(run>0) {
                    val result=obj("run" to run,"duration_ms" to duration,"begin_ns" to beginNs,"end_ns" to endNs,
                        "begin_boot_ns" to beginBoot,"end_boot_ns" to endBoot,
                        "begin_mono_after_boot_ns" to beginMonoAfterBoot,"end_mono_after_boot_ns" to endMonoAfterBoot,
                        "thermal_before" to thermalBefore,"thermal_after" to thermal.currentThermalStatus,
                        "width" to width,"height" to height,"photo" to args.getString("photo"),"motion" to label,"theme" to theme,
                        "transparency" to state().getJSONObject("settings").getString("transparency"),
                        "swipe_direction" to (if(swipeDirection<0) "left" else "right"),
                        "transitions_ns" to JSONArray(transitions),"selection_interval_ms" to (if(discrete)selectionInterval else JSONObject.NULL),
                        "transition_observations_boot_ns" to JSONArray(transitionObservations),
                        "animation_window_ms" to (if(selection)200 else JSONObject.NULL),
                        "frame_columns" to JSONArray(listOf("vsync_ns","total_ns","dropped_reports","intended_vsync_ns",
                            "input_ns","animation_ns","layout_ns","draw_ns","sync_ns","command_ns","swap_ns")),
                        "dropped_reports" to received.sumOf { it[2] },
                        "received_frames" to JSONArray(received.map { JSONArray(it.toList()) }),
                        "draw_geometry_columns" to JSONArray(listOf("draw_mono_ns","content_x_millipx","content_y_millipx","properties_y_millipx","inspector_height_millipx","draw_boot_ns")),
                        "draw_geometry" to JSONArray(observedGeometry.map { JSONArray(it.toList()) }),
                        "moving_draw_mono_ns" to JSONArray(moving.sorted()),
                        "camera" to state().getJSONObject("camera"),
                        "layers" to state().array("layers"),"frames" to JSONArray(rows.map { JSONArray(it.toList()) }))
                    if(properties)result.put("active_windows",activeWindows)
                    java.io.File(instrumentation.targetContext.getExternalFilesDir(null),"$label-$run.json").writeText(result.toString())
                    assertTrue("Moving frames were measured",rows.size>1)
                    if(properties)assertTrue("Native footer contacts changed pages throughout the gesture",transitions.size>=duration/selectionInterval-2)
                    if(measureGeometry)assertTrue("Drawn geometry changes during native contact",moving.size>1)
                    if(properties)assertTrue("Native footer transitions move arranged pages",activeWindows.length()>=transitions.size*.95)
                    android.util.Log.i("LayerSwipePerf","$label run $run: ${rows.size} moving frames in $duration ms")
                }
            }
        } finally {
            instrumentation.runOnMainSync {
                window.removeOnFrameMetricsAvailableListener(listener)
            }
            thread.quitSafely()
        }
    }

    @Test fun layerRelationships() {
        host.newDocument(2048,1536)
        val source=java.io.File(device.root,"relationship-source.png")
        android.graphics.Bitmap.createBitmap(2048,1536,android.graphics.Bitmap.Config.ARGB_8888).let { bitmap ->
            android.graphics.Canvas(bitmap).drawOval(350f,230f,1700f,1300f,android.graphics.Paint().apply { color=android.graphics.Color.rgb(75,174,158) })
            source.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it) };bitmap.recycle()
        }
        host.importImage(source)
        waitFor("imported relationship source",60_000) { state().array("layers").length()==3 }
        action(obj("type" to "invoke","command" to "apply_transform"))
        fun layer(value:JSONObject)=action(obj("type" to "layer","action" to value))
        fun current()=state().getJSONObject("layer_tools").getJSONObject("editing_layer").getLong("id")
        fun row(id:Long)=state().array("layers").objects().first { it.getLong("id")==id }
        fun order()=state().array("layers").objects().map { it.getLong("id") }
        fun rename(id:Long,name:String)=layer(obj("op" to "rename","id" to id,"name" to name))
        fun select(id:Long)=layer(obj("op" to "select","id" to id,"mask" to false))
        fun undo()=action(obj("type" to "invoke","command" to "undo"))
        val base=current();rename(base,"Base colors")
        layer(obj("op" to "new","group" to false,"clipped" to true))
        val owner=current();rename(owner,"Shadows")
        fun effect(name:String,kind:String):Long {
            select(owner)
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to kind)))
            return current().also { rename(it,name);layer(obj("op" to "attach_effect","id" to it,"owner" to owner)) }
        }
        val blur=effect("Blur","gaussian_blur");val curves=effect("Curves","curves")
        select(owner);layer(obj("op" to "new","group" to false,"clipped" to true))
        val highlights=current();rename(highlights,"Highlights")
        layer(obj("op" to "new","group" to true,"clipped" to false))
        val isolated=current();rename(isolated,"Isolated group");layer(obj("op" to "blend","id" to isolated,"value" to 0))
        select(highlights);layer(obj("op" to "new","group" to true,"clipped" to false))
        val through=current();rename(through,"Pass Through group")
        if(!row(through).getBoolean("pass_through"))layer(obj("op" to "toggle_pass_through","id" to through))
        action(obj("type" to "invoke","command" to "select_all"))
        action(obj("type" to "invoke","command" to "save_selection_layer"))
        waitFor("selection becomes a layer") { state().array("layers").objects().any { it.getBoolean("selection_layer") } }
        val saved=state().array("layers").objects().first { it.getBoolean("selection_layer") }.getLong("id")
        rename(saved,"Saved selection")
        layer(obj("op" to "drop","id" to saved,"target" to owner,"fraction" to 0,"surface" to "row"))
        val savedAt=order().indexOf(saved)
        assertEquals(listOf(saved,curves,blur,owner),order().subList(savedAt,savedAt+4))
        val relationshipWorkspace=JSONObject(fixture.toString())
        relationshipWorkspace.getJSONObject("layout").apply {
            put("bands",JSONArray(listOf(obj("id" to 40,"edge" to "right","extent" to 226,"root" to tabs(41,"layers")))))
            put("next_id",maxOf(42,getInt("next_id")))
        }
        action(obj("type" to "restore_workspace","workspace" to relationshipWorkspace));select(curves)
        waitFor("all relationship rows") { listOf(base,owner,blur,curves,isolated,through,saved).all { node("layer-row-$it")!=null } }
        waitFor("relationship controls") { node("layer-attachment")!=null && node("layer-content-$curves")!=null }
        assertFalse(row(isolated).getBoolean("pass_through"))
        assertNotNull(node("layer-group-pass-through-$through"));assertNull(node("layer-group-pass-through-$isolated"))
        tap("layer-attachment");assertNull(row(curves).objectOrNull("relationship"))
        tap("layer-attachment");assertEquals(highlights,row(curves).getJSONObject("relationship").getLong("target"))
        undo();undo();assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
        action(obj("type" to "set_layer_opacity","id" to owner,"opacity" to 0.5))
        for(width in listOf(226,300)) for(theme in listOf("light","dark")) {
            val workspace=JSONObject(state().getJSONObject("workspace").toString())
            workspace.getJSONObject("layout").array("bands").objects().first { it.getJSONObject("root").toString().contains("layers") }.put("extent",width)
            action(obj("type" to "restore_workspace","workspace" to workspace))
            action(obj("type" to "set_theme","theme" to theme))
            shot("layer-relationships-$theme-$width")
            val ordinary=bounds("layer-row-$base")
            for(id in listOf(owner,blur,curves,isolated,through,saved)) {
                assertEquals(ordinary.width,bounds("layer-row-$id").width,1f)
                assertEquals("Uniform row height: ${row(id).getString("label")}, $width/$theme",ordinary.height,bounds("layer-row-$id").height,1f)
                assertEquals(30*density,bounds("layer-content-$id").width,1f)
            }
            assertEquals(30*density,bounds("selection-load-$saved").width,1f)
        }
        val first=order().first()
        val firstThumb=bounds("layer-content-$first")
        val rail=screenBounds("layer-content-$owner").let { Offset(it.left-3.5f*density,it.center.y) }
        fun railPixel():Int {
            val bitmap=checkNotNull(instrumentation.uiAutomation.takeScreenshot())
            return try { bitmap.getPixel(rail.x.toInt(),rail.y.toInt()) } finally { bitmap.recycle() }
        }
        val railColor=railPixel()
        assertTrue("Clipping rail pixel",android.graphics.Color.blue(railColor)>android.graphics.Color.red(railColor)+20)
        down("layer-row-$first");val swipeStart=point
        for(i in 1..4)event(MotionEvent.ACTION_MOVE,swipeStart+Offset(-36*density*i/4,0f))
        assertTrue("Row face moves",bounds("layer-content-$first").left<firstThumb.left-20*density)
        assertEquals("Connector column stays fixed during first-row swipe",railColor,railPixel())
        shot("layer-relationships-swiping")
        event(MotionEvent.ACTION_CANCEL);idle()
        tap("layer-eye-$owner")
        for(id in listOf(blur,curves)) {
            assertTrue(row(id).getBoolean("visible"));assertTrue(row(id).getBoolean("visibility_blocked"))
            assertEquals(owner,row(id).getJSONObject("relationship").getLong("target"))
        }
        shot("layer-relationships-hidden-owner");undo()
        fun swipe(id:Long,dx:Float,cancel:Boolean=false) {
            down("layer-row-$id");val start=point
            for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/4,0f))
            event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP);idle()
        }
        layer(obj("op" to "blend","id" to isolated,"value" to 1))
        for((theme,pointer) in listOf("light" to MotionEvent.TOOL_TYPE_FINGER,"dark" to MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme","theme" to theme));tool=pointer
            swipe(isolated,12f);assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,60f,true);assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,-60f);assertNotNull(node("layer-delete-$isolated"));assertNotNull("Swiped row retains its grip for fading",node("layer-grip-$isolated"))
            val swipedOrder=order()
            tool=MotionEvent.TOOL_TYPE_MOUSE
            down("layer-row-$isolated");event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density))
            assertNull("Swiped mouse body cannot pick up",node("layer-drag-preview"))
            event(MotionEvent.ACTION_CANCEL);idle();assertEquals(swipedOrder,order())
            tool=pointer
            swipe(isolated,90f);assertNull(node("layer-delete-$isolated"));assertFalse(row(isolated).getBoolean("pass_through"))
            swipe(isolated,60f);assertTrue(row(isolated).getBoolean("pass_through"));assertFalse(row(isolated).getBoolean("alpha_locked"))
            swipe(isolated,60f);assertFalse(row(isolated).getBoolean("pass_through"));assertEquals("Normal",row(isolated).getString("blend_label"))
            undo();waitFor("Undo restores Pass Through") { row(isolated).getBoolean("pass_through") }
            undo();waitFor("Undo restores Normal blending") { !row(isolated).getBoolean("pass_through") }
            assertEquals("toggle_alpha_lock", row(owner).getJSONObject("right_swipe").getString("op"))
            swipe(owner,60f);waitFor("Swiping locks the paint layer alpha") { row(owner).getBoolean("alpha_locked") }
            undo();waitFor("Undo restores unlocked alpha") { !row(owner).getBoolean("alpha_locked") }
        }
        for((pointer,thumbnail) in listOf(MotionEvent.TOOL_TYPE_MOUSE to true,MotionEvent.TOOL_TYPE_FINGER to false,MotionEvent.TOOL_TYPE_STYLUS to true)) {
            action(obj("type" to "set_theme","theme" to if(pointer==MotionEvent.TOOL_TYPE_MOUSE) "light" else "dark"))
            tool=pointer;select(curves);val before=order()
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE) {
                down("layer-row-$curves");event(MotionEvent.ACTION_MOVE,point+Offset(0f,24*density));event(MotionEvent.ACTION_UP);idle();assertEquals(before,order())
            }
            down(if(pointer==MotionEvent.TOOL_TYPE_MOUSE) "layer-row-$curves" else "layer-grip-$curves")
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("layer pickup") { node("layer-drag-preview")!=null }
            val target=if(thumbnail)bounds("layer-content-$isolated").center else bounds("layer-row-$through").let { Offset(it.center.x,it.bottom-2*density) }
            event(MotionEvent.ACTION_MOVE,target)
            event(MotionEvent.ACTION_UP);idle()
            if(thumbnail)assertEquals(isolated,row(curves).getJSONObject("relationship").getLong("target")) else assertNotEquals(before,order())
            undo();assertEquals(before,order());assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
        }
        for(theme in listOf("light","dark")) for(pointer in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)) {
            action(obj("type" to "set_theme","theme" to theme));tool=pointer;select(curves)
            val before=order()
            if(pointer!=MotionEvent.TOOL_TYPE_MOUSE) {
                down("layer-row-$curves")
                SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong()+150)
                waitFor("layer body hold menu") { node("workspace-menu")!=null }
                assertNull("Body hold never picks up a layer",node("layer-drag-preview"))
                event(MotionEvent.ACTION_CANCEL)
                waitFor("cancelled layer body menu") { node("workspace-menu")==null }
                assertEquals(before,order())
            }
            down(if(pointer==MotionEvent.TOOL_TYPE_MOUSE) "layer-row-$curves" else "layer-grip-$curves")
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("filter gap pickup") { node("layer-drag-preview")!=null }
            val target=bounds("layer-row-$base").let { Offset(it.right-64*density,it.top+2*density) }
            event(MotionEvent.ACTION_MOVE,target)
            shot("layer-filter-gap-$theme-$pointer")
            event(MotionEvent.ACTION_UP);idle()
            assertEquals(base,row(curves).getJSONObject("relationship").getLong("target"))
            assertEquals(base,row(owner).getJSONObject("relationship").getLong("target"))
            undo();assertEquals(before,order());assertEquals(owner,row(curves).getJSONObject("relationship").getLong("target"))
            action(obj("type" to "invoke","command" to "redo"));assertEquals(base,row(curves).getJSONObject("relationship").getLong("target"));undo()
        }
        for(theme in listOf("light","dark")) {
            host.newDocument(2048,1536)
            repeat(3) { layer(obj("op" to "new","group" to false,"clipped" to false)) }
            action(obj("type" to "restore_workspace","workspace" to relationshipWorkspace))
            action(obj("type" to "set_theme","theme" to theme))
            val before=order()
            val checked=before.take(3)
            fun selection()=state().array("layers").objects().filter { it.getBoolean("selected") }.map { it.getLong("id") }
            fun clickRow(id:Long,meta:Int=0) {
                val b=bounds("layer-row-$id")
                instrumentation.runOnMainSync { pressed=checkNotNull(node("layer-row-$id")).first }
                val p=Offset(b.right-64*density,b.center.y)
                event(MotionEvent.ACTION_DOWN,p,meta);event(MotionEvent.ACTION_UP,p,meta);idle()
            }
            tool=MotionEvent.TOOL_TYPE_MOUSE
            layer(obj("op" to "add_mask","id" to checked[0],"replace" to false))
            fun clickLink() {
                val b=bounds("layer-content-${checked[0]}")
                instrumentation.runOnMainSync { pressed=checkNotNull(node("layer-row-${checked[0]}")).first }
                val p=Offset(b.right+8*density,b.center.y)
                event(MotionEvent.ACTION_DOWN,p);event(MotionEvent.ACTION_UP,p);idle()
            }
            assertTrue(row(checked[0]).getBoolean("mask_linked"))
            shot("layer-mask-linked-$theme")
            clickLink();assertFalse(row(checked[0]).getBoolean("mask_linked"))
            shot("layer-mask-unlinked-$theme")
            layer(obj("op" to "lock","id" to checked[0],"value" to true))
            clickLink();assertFalse("Locked layer disables mask linking",row(checked[0]).getBoolean("mask_linked"))
            layer(obj("op" to "lock","id" to checked[0],"value" to false))
            clickLink();assertTrue(row(checked[0]).getBoolean("mask_linked"))
            layer(obj("op" to "select","id" to checked[0],"mask" to true))
            waitFor("mask preview") { node("layer-thumbnail-${checked[0]}-true") != null }
            fun thumbnailPixels(mask:Boolean=true): List<Int> {
                val exit=motion(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.ACTION_HOVER_EXIT,point,SystemClock.uptimeMillis(),0)
                try { instrumentation.runOnMainSync { checkNotNull(node("layer-row-${checked[0]}")).first.view.dispatchGenericMotionEvent(exit) } }
                finally { exit.recycle() }
                SystemClock.sleep(400)
                val b=screenBounds("layer-thumbnail-${checked[0]}-$mask")
                val image=checkNotNull(instrumentation.uiAutomation.takeScreenshot())
                try {
                    fun pixel(x:Float,y:Float)=image.getPixel((b.left+x*density).toInt(),(b.top+y*density).toInt())
                    return listOf(pixel(14f,-1f),pixel(0f,0f),pixel(-5f,0f),pixel(3f,3f),
                        pixel(14f,2f),pixel(2f,14f),pixel(25f,14f),pixel(14f,25f))
                } finally { image.recycle() }
            }
            val activeMask=thumbnailPixels()
            val idleContent=thumbnailPixels(false)
            shot("layer-thumbnail-squircles-$theme")
            assertEquals("$theme editing border uses the accent outside the preview",android.graphics.Color.parseColor(state().getJSONObject("palette").getString("accent")),activeMask[0])
            layer(obj("op" to "select","id" to checked[0],"mask" to false))
            val idleMask=thumbnailPixels()
            assertEquals("$theme full squircle clears the preview corner",idleContent[2],idleContent[1])
            assertNotEquals("$theme full squircle retains more than a circular thumbnail",idleContent[2],idleContent[3])
            assertEquals("$theme preview pixels inside the editing border stay unchanged",activeMask.drop(4),idleMask.drop(4))
            assertNotEquals("$theme outer editing edge changes with the target",activeMask[0],idleMask[0])
            layer(obj("op" to "select","id" to checked[0],"mask" to true))
            clickRow(checked[0])
            assertTrue("Active row retains its mask target",row(checked[0]).getBoolean("mask_selected"))
            assertFalse(row(checked[0]).getBoolean("content_selected"))
            clickRow(checked[2],KeyEvent.META_SHIFT_ON)
            assertEquals("Shift extends the visible checked range",checked,selection())
            assertTrue("Range selection preserves the editing target",row(checked[0]).getBoolean("mask_selected"))
            shot("layer-range-mask-$theme")
            clickRow(checked[1])
            assertEquals("A checked row retains the other checks",checked,selection())
            assertTrue(row(checked[1]).getBoolean("content_selected"))
            assertFalse(row(checked[0]).getBoolean("mask_selected"))
            val target=bounds("layer-row-${before[3]}")
            down("layer-row-${checked[1]}")
            event(MotionEvent.ACTION_MOVE,point+Offset(-30*density,0f))
            waitFor("checked block pickup") { node("layer-drag-preview")!=null }
            event(MotionEvent.ACTION_MOVE,Offset(target.right-64*density,target.bottom-2*density))
            event(MotionEvent.ACTION_UP);idle()
            assertEquals("Dragging a checked row moves the ordered block",listOf(before[3])+checked+before.drop(4),order())
            undo();assertEquals(before,order());assertEquals(checked,selection())
            layer(obj("op" to "new","group" to true,"clipped" to false))
            val folder=current()
            assertTrue(row(folder).getBoolean("group"))
            for(id in checked)assertEquals("New group contains every checked layer",1,row(id).getInt("depth"))
            shot("layer-checked-group-$theme")
            layer(obj("op" to "select_all_layers","selected" to false))
            layer(obj("op" to "select_row","id" to folder,"extend" to false,"toggle" to false))
            layer(obj("op" to "delete_selected"))
            assertFalse(order().contains(folder))
            for(id in checked)assertEquals("Deleting an expanded group promotes unchecked children",0,row(id).getInt("depth"))
            undo();assertTrue(order().contains(folder))
            for(id in checked)assertEquals(1,row(id).getInt("depth"))
            undo();assertEquals(before,order());assertEquals(checked,selection())
        }
        assertNull(host.actionError)
    }

    @Test fun layerPropertiesDrawerUsesNativeSizingAndActions() {
        resetPreset("builtin:workspace:painter")
        val opener="header-control-"+entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("panel")=="layers"
        }.getInt("id")
        val layoutMetrics=JSONArray()
        fun observe(value:JSONObject) {
            layoutMetrics.put(value)
            java.io.File(instrumentation.targetContext.getExternalFilesDir(null),"layer-properties-layout.json").writeText(layoutMetrics.toString())
        }
        fun actionCenter(key:String):Offset {
            var result:Offset?=null
            instrumentation.runOnMainSync {
                val label=host.catalog.getJSONObject("native_copy").getJSONObject("layers").getString(key)
                result=checkNotNull(findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(label)==true })).second.boundsInRoot.center
            }
            return checkNotNull(result)
        }
        fun rows()=state().array("layers").objects()
        fun selected()=state().getJSONObject("layer_properties").getLong("layer")
        fun add()=action(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
        fun height()=bounds("layer-inspector").height/density
        var layerHomeY=0f
        fun page(target:Float,open:Boolean,label:String) {
            if(open)instrumentation.runOnMainSync { layerHomeY=checkNotNull(node("layer-rows")).second.positionInRoot.y }
            val begin=SystemClock.uptimeMillis()
            tap("layer-properties-disclosure")
            waitFor(label) {
                val inspector=checkNotNull(node("layer-inspector")).second
                val content=checkNotNull(node(if(open)"layer-properties" else "layer-rows")).second
                val expectedY=if(open)inspector.positionInRoot.y+8*density else layerHomeY
                kotlin.math.abs(inspector.boundsInRoot.height/density-target)<=2f && kotlin.math.abs(content.positionInRoot.y-expectedY)<=1f
            }
            observe(obj("transition" to label,"tap_to_settled_ms" to (SystemClock.uptimeMillis()-begin),"height_dp" to height()))
        }
        fun swipe(id:Long,dx:Float,cancel:Boolean=false,reverse:Boolean=false) {
            down("layer-row-$id");val start=point
            for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/4,0f))
            if(reverse)event(MotionEvent.ACTION_MOVE,start)
            event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP);idle()
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            tap(opener)
            waitFor("layer inspector six-row geometry") { (node("layer-inspector")?.second?.boundsInRoot?.height ?: 0f)>=240*density }
            idle()
            val floor=height()
            val before=rows().size
            while(rows().size<7)add()
            assertTrue("Seventh row grows drawer",height()>floor+20)
            shot("layer-drawer-$theme")
            val grown=height()
            val disclosure=bounds("layer-properties-disclosure")
            val first=actionCenter("new_layer");val last=actionCenter("actions")
            val firstSlot=bounds("layer-properties-icon-slot").center.x;val lastSlot=bounds("layer-properties-chevron-slot").center.x
            val labelLeft=bounds("layer-properties-disclosure-label").left
            observe(obj("theme" to theme,"floor_dp" to floor,"grown_dp" to grown,"density" to density,
                "first_action_center_x" to first.x,"last_action_center_x" to last.x,"first_disclosure_slot_center_x" to firstSlot,"last_disclosure_slot_center_x" to lastSlot,
                "label_left" to labelLeft,"expected_label_left" to (disclosure.left+38*density)))
            assertEquals("First footer slot aligns with New Layer",first.x,firstSlot,1f)
            assertEquals("Last footer slot aligns with Actions",last.x,lastSlot,1f)
            assertEquals("Disclosure label follows the 6 dp edge, the 26 dp slot and a 6 dp gap",disclosure.left+38*density,labelLeft,1f)
            fun inDrawer(tag:String):Rect { var r:Rect?=null; instrumentation.runOnMainSync { r=node("layer-inspector")?.second?.find(hasTag(tag))?.boundsInRoot }; return checkNotNull(r) { "Missing $tag in the drawer" } }
            val inspector=bounds("layer-inspector");val row=inDrawer("layer-row-${rows().first().getLong("id")}")
            val control=host.catalog.getJSONObject("panel_metrics").getDouble("control_height").toFloat()*density
            assertEquals("Rows run edge to edge in the drawer",inspector.width,row.width,1f)
            assertEquals("Rows start at the drawer edge",inspector.left,row.left,1f)
            for((name,x) in listOf("eye" to inDrawer("layer-eye-${rows().first().getLong("id")}").left,"New Layer" to first.x-control/2,"properties slot" to firstSlot-control/2))
                assertEquals("$name sits 6 dp in from the drawer edge",inspector.left+6*density,x,1f)
            for((name,x) in listOf("Actions" to last.x+control/2,"chevron slot" to lastSlot+control/2))
                assertEquals("$name sits 6 dp in from the right edge",inspector.right-6*density,x,1f)
            page(floor,true,"Raster Properties reaches floor")
            waitFor("native properties page") { state().getJSONObject("customization").getJSONObject("drawer").getBoolean("layer_properties") }
            assertEquals("Raster properties use six-row floor",floor,height(),2f)
            assertNotNull(node("layer-properties-disclosure-label"))
            assertNotNull("Raster Properties offers Add Filter",node("properties-add-filter"))
            shot("layer-properties-floor-$theme")
            val id=selected()
            page(grown,false,"All layers restores grown geometry")
            assertEquals("All layers restores grown height",grown,height(),2f)
            down("layer-properties-disclosure");event(MotionEvent.ACTION_UP)
            SystemClock.sleep(70)
            down("layer-properties-disclosure");event(MotionEvent.ACTION_UP)
            idle()
            assertEquals("Reversing transition restores native height",grown,height(),2f)
            assertFalse(state().getJSONObject("customization").getJSONObject("drawer").getBoolean("layer_properties"))
            assertEquals(id,selected())
            val footer=bounds("layer-properties-disclosure")
            assertEquals("Disclosure spans drawer width",bounds("layer-inspector").width,footer.width,1f)
            assertEquals("Disclosure sits at drawer bottom",bounds("layer-inspector").bottom,footer.bottom,1f)
            tool=MotionEvent.TOOL_TYPE_FINGER
            for(mode in 0..2) {
                swipe(id,if(mode==0)-18f else -80f,cancel=mode==1,reverse=mode==2)
                assertNull("Short, cancelled and reversed contacts close actions",node("layer-delete-$id"))
            }
            swipe(id,-80f)
            for(action in listOf("lock","duplicate","delete"))assertNotNull("Revealed $action",node("layer-$action-$id"))
            tap("layer-lock-$id")
            assertTrue(rows().first { it.getLong("id")==id }.getBoolean("locked"))
            swipe(id,-80f);tap("layer-lock-$id")
            assertFalse(rows().first { it.getLong("id")==id }.getBoolean("locked"))
            swipe(id,-80f);tap("layer-duplicate-$id")
            assertEquals(8,rows().size)
            action(obj("type" to "invoke","command" to "undo"));assertEquals(7,rows().size)
            swipe(id,-80f);tap("layer-delete-$id")
            assertEquals(6,rows().size);assertEquals(floor,height(),2f)
            action(obj("type" to "invoke","command" to "undo"));assertEquals(7,rows().size)
            assertEquals(grown,height(),2f)
            while(rows().size<32)add()
            val capped=height()
            assertTrue("Layer drawer obeys viewport cap",capped<=bounds("workspace").height/density*.85f+2)
            add();assertEquals("Rows beyond cap retain drawer height",capped,height(),2f)
            val retained=selected()
            page(floor,true,"Capped list Properties reaches floor")
            assertEquals("Large layer list shrinks to Properties floor",floor,height(),2f)
            page(capped,false,"Capped list returns to Layers")
            assertEquals(retained,selected());assertEquals(capped,height(),2f)
            while(rows().size>before)action(obj("type" to "layer","action" to obj("op" to "delete","id" to selected())))
            shot("layer-properties-actions-$theme")
            action(obj("type" to "layer","action" to obj("op" to "new","group" to true,"clipped" to false)))
            val parent=selected()
            page(floor,true,"Group Properties reaches floor")
            assertNull("Group Properties omits Add Filter",node("properties-add-filter"))
            page(floor,false,"Group Properties returns to Layers")
            add();val child=selected()
            swipe(child,-80f)
            action(obj("type" to "layer","action" to obj("op" to "lock","id" to parent,"value" to true)))
            assertTrue("Locked parent disables every child action",rows().first { it.getLong("id")==child }.array("swipe_actions").objects().none { it.getBoolean("enabled") })
            val revision=state().getJSONObject("document_file").getLong("revision")
            val count=rows().size
            val inheritedLock=rows().first { it.getLong("id")==child }.getBoolean("locked")
            swipe(child,-80f)
            for(command in listOf("lock","duplicate","delete"))assertNull("Disabled parent prevents native $command reveal",node("layer-$command-$child"))
            assertEquals("Disabled native swipe changes no artwork",revision,state().getJSONObject("document_file").getLong("revision"))
            assertEquals(count,rows().size);assertEquals(inheritedLock,rows().first { it.getLong("id")==child }.getBoolean("locked"))
            action(obj("type" to "layer","action" to obj("op" to "lock","id" to parent,"value" to false)))
            action(obj("type" to "layer","action" to obj("op" to "delete","id" to parent)))
            action(obj("type" to "invoke","command" to "quick_mask"))
            waitFor("Quick Mask native row") { node("layer-row-0")!=null }
            swipe(0,-80f)
            assertNotNull(node("layer-delete-0"));assertNull(node("layer-lock-0"));assertNull(node("layer-duplicate-0"))
            tap("layer-delete-0")
            assertFalse("Native Delete removes Quick Mask",rows().any { it.getBoolean("quick_mask") })
            action(obj("type" to "effect","action" to obj("op" to "insert","effect" to "curves")))
            assertNull("Filter Properties omits Add Filter",node("properties-add-filter"))
            assertEquals("Filter insertion opens Properties",true,state().getJSONObject("customization").getJSONObject("drawer").getBoolean("layer_properties"))
            shot("layer-properties-filter-$theme")
            page(floor,false,"Filter Properties returns to Layers")
            action(obj("type" to "layer","action" to obj("op" to "delete","id" to selected())))
            tap(opener)
        }
        fun customize(value:JSONObject)=action(obj("type" to "customize","action" to value))
        customize(obj("type" to "new_toolbar","group" to JSONObject.NULL))
        customize(obj("type" to "picker_name","name" to "Layers verification"))
        customize(obj("type" to "picker_select","control" to obj("kind" to "panel","panel" to "layers"),"selected" to true))
        customize(obj("type" to "confirm_tools"))
        val toolbar=snapshot().array("panels").objects().first { it.getString("title")=="Layers verification" }
        val tile="tile-${toolbar.getString("id")}-${toolbar.array("tiles").getJSONObject(0).getInt("id")}"
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            tap(tile)
            waitFor("custom toolbar Layers combo") { (node("layer-inspector")?.second?.boundsInRoot?.height ?: 0f)>=240*density }
            idle()
            val floor=height()
            page(floor,true,"Custom toolbar Properties reaches floor")
            assertTrue(state().getJSONObject("customization").getJSONObject("drawer").getBoolean("layer_properties"))
            assertEquals("Custom toolbar shares floor sizing",floor,height(),2f)
            shot("layer-properties-custom-toolbar-$theme")
            page(floor,false,"Custom toolbar Properties returns to Layers")
            tap(tile)
        }
        assertNull(host.actionError)
    }

    @Test fun zenBackdropStrengthFadesAndRetiresWithNativeChrome() {
        resetPreset("builtin:workspace:photographer")
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            tap("column-icon-brushes")
            waitFor("Expanded collapsed column registers its native connector") { host.glassConnectionsForTest.isNotEmpty() }
            val menuTag=if(node("application-menu-view")!=null) "application-menu-view" else "header-menu-labels-compact"
            val target=checkNotNull(node(menuTag));val menuPoint=target.second.boundsInRoot.center
            val surface=scenario.activity().window.decorView.descendant<CanvasSurfaceView>()!!
            val area=bounds("title-bar").translate(-host.surfaceOrigin)
            val capy=node("zen-button")?.second?.boundsInRoot?.translate(-host.surfaceOrigin) ?: Rect.Zero
            fun targets()=host.glassBoxesForTest.filter { box ->
                val rect=Rect(box[0],box[1],box[0]+box[2],box[1]+box[3])
                rect.overlaps(area) && !rect.overlaps(capy)
            }
            waitFor("Native header glass starts fully shown") { targets().isNotEmpty() && targets().all { it[8]==1f } }
            val strengths=java.util.Collections.synchronizedList(mutableListOf<Float>())
            val connectors=java.util.Collections.synchronizedList(mutableListOf<Double>())
            lateinit var callback:android.view.Choreographer.FrameCallback
            instrumentation.runOnMainSync {
                val clock=android.view.Choreographer.getInstance()
                callback=android.view.Choreographer.FrameCallback { strengths.addAll(targets().map { it[8] });connectors.addAll(host.glassConnectionsForTest.map { it.getDouble("strength") });clock.postFrameCallback(callback) }
                clock.postFrameCallback(callback)
                host.invoke("zen_mode")
            }
            try {
                waitFor("Native hidden chrome retires its backdrop") { snapshot().optBoolean("chrome_hidden") && targets().isEmpty() && host.glassConnectionsForTest.isEmpty() }
            } finally { instrumentation.runOnMainSync { android.view.Choreographer.getInstance().removeFrameCallback(callback) } }
            assertTrue("GPU backdrop uses intermediate native fade strength: $strengths",strengths.any { it>0f && it<1f })
            assertTrue("Native connector strengths stay valid until policy retirement: $connectors",connectors.all { it in 0.0..1.0 })
            assertHiddenChrome("title-bar")
            val contacts=mutableListOf<Int>()
            instrumentation.runOnMainSync { surface.setOnTouchListener { _,event -> contacts.add(event.actionMasked);false } }
            try {
                tool=MotionEvent.TOOL_TYPE_FINGER;pressed=target.first
                event(MotionEvent.ACTION_DOWN,menuPoint);event(MotionEvent.ACTION_UP);idle()
            } finally { instrumentation.runOnMainSync { surface.setOnTouchListener(null) } }
            assertTrue("Hidden header press reaches native canvas: $contacts",contacts.contains(MotionEvent.ACTION_DOWN) && contacts.contains(MotionEvent.ACTION_UP))
            assertNull("A retained hidden menu cannot activate",node("workspace-menu"))
            shot("zen-native-glass-$theme")
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Native header backdrop returns") { !snapshot().optBoolean("chrome_hidden") && targets().isNotEmpty() && targets().all { it[8]==1f } && host.glassConnectionsForTest.isEmpty() }
            tap("column-icon-brushes")
            waitFor("Collapsed column can reopen after Zen") { host.glassConnectionsForTest.isNotEmpty() && host.glassConnectionsForTest.all { it.getDouble("strength")==1.0 } }
            tap("column-icon-brushes")
            waitFor("Expanded collapsed column closes") { host.glassConnectionsForTest.isEmpty() }
            assertNull(host.actionError);assertNull(host.failure)
        }
    }

    @Test fun localizedNoticeRefreshKeepsItsOriginalDeadline() {
        val tags=host.bootstrap!!.array("shipped_tags").values().map { it.toString() }
        fun language(tag:String)=obj("type" to "preferences","action" to obj("type" to "edit","id" to "language","value" to tags.indexOf(tag)+1))
        require("de" in tags && "en" in tags)
        val pencil=host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }.first { it.getString("label")=="Pencil" }.getLong("id")
        val surface=scenario.activity().window.decorView.descendant<CanvasSurfaceView>()!!
        for(theme in listOf("light","dark")) {
            action(language("en"));waitFor("English notice fixture") { host.languageTag=="en" }
            action(obj("type" to "set_theme","theme" to theme))
            action(obj("type" to "layer","action" to obj("op" to "new_vector")))
            action(obj("type" to "select_brush","id" to pencil))
            waitFor("Notice brush ready") { snapshot().optBoolean("brush_ready") }
            val start=SystemClock.uptimeMillis()
            for((phase,x) in listOf(MotionEvent.ACTION_DOWN to .4f,MotionEvent.ACTION_MOVE to .6f,MotionEvent.ACTION_UP to .6f)) {
                val event=motion(MotionEvent.TOOL_TYPE_MOUSE,phase,Offset(surface.width*x,surface.height*.5f),start,MotionEvent.BUTTON_PRIMARY)
                try { instrumentation.runOnMainSync { surface.dispatchTouchEvent(event) } } finally { event.recycle() }
            }
            waitFor("Vector paint notice") { host.notice?.id!=null }
            val shown=checkNotNull(host.notice);val shownAt=SystemClock.uptimeMillis()
            val handler=android.os.Handler(android.os.Looper.getMainLooper())
            val localize=Runnable { host.dispatch(language("de")) }
            handler.postDelayed(localize,1700)
            try {
                waitFor("Same notice publishes translated copy",6000) { host.notice?.let { it.id==shown.id && it.text!=shown.text }==true }
                val shared=state().getJSONObject("notice")
                assertEquals(shared.getString("name"),host.notice!!.name)
                assertEquals(shared.getString("text"),host.notice!!.text)
                assertEquals(shared.array("actions").objects().map { it.getString("label") },host.notice!!.actions.map { it.label })
                waitFor("Notice expires on original deadline",6000) { host.notice==null }
                assertTrue("Localization must not restart the four-second timeout",SystemClock.uptimeMillis()-shownAt<4800)
            } finally { handler.removeCallbacks(localize) }
        }
    }

    @Test fun pickerDoubleActivationRequiresPointerPressesFromOneSource() {
        resetPreset("builtin:workspace:painter")
        val panel=state().getJSONObject("workspace").getJSONObject("layout").array("panels").objects().first { p ->
            p.getJSONObject("content").optJSONArray("tiles")?.objects()?.any { it.getJSONObject("control").optString("kind")=="color_picker" }==true
        }
        val tile=panel.getJSONObject("content").array("tiles").objects().first { it.getJSONObject("control").optString("kind")=="color_picker" }
        val tag="tile-${panel.getString("id")}-${tile.getInt("id")}"
        fun drawer()=state().getJSONObject("customization").objectOrNull("drawer")
        fun activate()=instrumentation.runOnMainSync {
            fun click(node:SemanticsNode):(() -> Boolean)?=node.config.getOrNull(SemanticsActions.OnClick)?.action ?: node.children.firstNotNullOfOrNull(::click)
            assertTrue(checkNotNull(click(checkNotNull(node(tag)).second)).invoke())
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            activate();activate();idle()
            assertNull("Keyboard/accessibility activation cannot pair",drawer())
            tool=MotionEvent.TOOL_TYPE_MOUSE;down(tag);event(MotionEvent.ACTION_UP)
            activate();idle()
            assertNull("Pointer and keyboard activation cannot pair",drawer())
            tool=MotionEvent.TOOL_TYPE_FINGER;down(tag);event(MotionEvent.ACTION_UP)
            tool=MotionEvent.TOOL_TYPE_STYLUS;down(tag);event(MotionEvent.ACTION_UP);idle()
            assertNull("Different native pointer sources cannot pair",drawer())
            tool=MotionEvent.TOOL_TYPE_FINGER;down(tag);event(MotionEvent.ACTION_UP)
            down(tag);event(MotionEvent.ACTION_UP);idle()
            waitFor("Pointer pair opens anchored picker settings") { drawer()!=null && node("picker-setting-source")!=null }
            waitFor("Picker settings rows are fully visible") {
                val drawerNode=node("tool-drawer")?.second
                val drawerBounds=drawerNode?.boundsInRoot
                listOf("picker-setting-source","picker-setting-size").all { tag ->
                    drawerNode?.find(hasTag(tag))?.let {
                        val row=it.boundsInRoot
                        drawerBounds!=null && it.size.height>0 && row.height>=it.size.height-1 &&
                            row.left>=drawerBounds.left-1 && row.top>=drawerBounds.top-1 &&
                            row.right<=drawerBounds.right+1 && row.bottom<=drawerBounds.bottom+1
                    }==true
                }
            }
            instrumentation.waitForIdleSync()
            instrumentation.runOnMainSync {
                val (root,drawerNode)=checkNotNull(node("tool-drawer"))
                val choice=checkNotNull(drawerNode.find(hasTag("picker-setting-size"))?.find { it.config.getOrNull(SemanticsActions.OnClick)?.action!=null })
                pressed=root;point=choice.boundsInRoot.center
            }
            event(MotionEvent.ACTION_DOWN);event(MotionEvent.ACTION_UP)
            waitFor("Picker sample-size menu accepts native contact") { host.pickerPopupOpen }
            key(KeyEvent.KEYCODE_BACK)
            waitFor("Picker sample-size menu closes") { !host.pickerPopupOpen }
            instrumentation.waitForIdleSync()
            shot("picker-pointer-double-$theme")
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Picker settings closes") { drawer()==null }
        }
    }

    @Test fun layerRenameEscapePreservesImeOwnership() {
        showSwipeLayers()
        val id=state().array("layers").objects().first { it.getBoolean("can_rename") }.getLong("id")
        fun row()=state().array("layers").objects().first { it.getLong("id")==id }
        val original=row().getString("label")
        val tag="layer-name-editor-$id"
        fun begin(composing:Boolean):android.view.inputmethod.InputConnection {
            action(obj("type" to "layer","action" to obj("op" to "begin_rename","id" to id)))
            waitFor("Layer name owns native focus") { node(tag)!=null && host.editingText }
            var connection:android.view.inputmethod.InputConnection?=null
            instrumentation.runOnMainSync {
                connection=checkNotNull(node(tag)).first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())
                val input=checkNotNull(connection)
                assertTrue(input.setSelection(0,original.length))
                assertTrue(if(composing)input.setComposingText("Candidate name",1) else input.commitText("Draft name",1))
            }
            return checkNotNull(connection)
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            begin(false)
            key(KeyEvent.KEYCODE_ESCAPE)
            waitFor("Escape cancels layer name") { node(tag)==null }
            assertEquals(original,row().getString("label"))
            val connection=begin(true)
            assertTrue(host.textComposition.active)
            val candidateDown=SystemClock.uptimeMillis()
            instrumentation.runOnMainSync {
                for(phase in listOf(KeyEvent.ACTION_DOWN,KeyEvent.ACTION_UP))
                    assertTrue(connection.sendKeyEvent(KeyEvent(candidateDown,candidateDown,phase,KeyEvent.KEYCODE_ESCAPE,0,0,-1,0,KeyEvent.FLAG_SOFT_KEYBOARD,android.view.InputDevice.SOURCE_KEYBOARD)))
            }
            idle()
            assertNotNull("IME Escape retains the layer editor: rename=${state().getJSONObject("layer_tools").opt("rename_layer")} layer=${node("layer-row-$id")!=null} activity=${scenario.state} composition=${host.textComposition.active} editing=${host.editingText}",node(tag))
            assertEquals(original,row().getString("label"))
            instrumentation.runOnMainSync {
                val editor=checkNotNull(node(tag)).first.view
                assertTrue(checkNotNull(editor.onCreateInputConnection(android.view.inputmethod.EditorInfo())).finishComposingText())
            }
            waitFor("Layer composition finishes") { !host.textComposition.active }
            val activity=scenario.activity()
            val down=SystemClock.uptimeMillis()
            instrumentation.runOnMainSync {
                for(phase in listOf(KeyEvent.ACTION_DOWN,KeyEvent.ACTION_UP))
                    assertTrue("Released app Escape is handled",activity.dispatchKeyEvent(KeyEvent(down,down,phase,KeyEvent.KEYCODE_ESCAPE,0,0,-1,0,0,android.view.InputDevice.SOURCE_KEYBOARD)))
            }
            waitFor("Escape cancels after IME releases") { node(tag)==null }
            assertEquals(original,row().getString("label"))
        }
    }

    @Test fun workspaceDividerKeyboardAndPointerDoubleTapIdentity() {
        for(theme in listOf("light","dark")) {
            resetPreset("builtin:workspace:illustrator")
            action(obj("type" to "set_theme","theme" to theme))
            fun dividers()=snapshot().getJSONObject("layout").array("dividers").objects()
            val divider=dividers().first { !it.optBoolean("fixed") && it.optBoolean("band") && it.getString("axis")=="horizontal" }
            val id=divider.getInt("id")
            fun position()=dividers().first { it.getInt("id")==id }.getJSONObject("bounds").number("x")
            val start=position()
            val revision=state().getJSONObject("document_file").getLong("revision")
            instrumentation.runOnMainSync { assertTrue(checkNotNull(node("divider-$id")).second.config[SemanticsActions.RequestFocus].action!!.invoke()) }
            waitFor("Native divider focus") { host.dividerFocus==id }
            key(KeyEvent.KEYCODE_DPAD_RIGHT)
            waitFor("Shared divider key advances") { kotlin.math.abs(position()-start-12f)<1f }
            key(KeyEvent.KEYCODE_DPAD_LEFT)
            waitFor("Shared divider key returns") { kotlin.math.abs(position()-start)<1f }
            assertEquals("Workspace resize keeps artwork revision",revision,state().getJSONObject("document_file").getLong("revision"))
            val group=snapshot().getJSONObject("layout").array("groups").objects().first { it.getBoolean("tabs_visible") && node("group-grip-${it.getInt("id")}")!=null }
            val grip="group-grip-${group.getInt("id")}"
            val before=layout()
            tool=MotionEvent.TOOL_TYPE_FINGER;down(grip);event(MotionEvent.ACTION_UP)
            tool=MotionEvent.TOOL_TYPE_STYLUS;down(grip);event(MotionEvent.ACTION_UP);idle()
            assertEquals("Different pointer sources cannot form a double tap",before,layout())
            tool=MotionEvent.TOOL_TYPE_FINGER;down(grip);event(MotionEvent.ACTION_UP)
            down(grip);event(MotionEvent.ACTION_UP);idle()
            waitFor("Same pointer source doubles panel handle") { layout()!=before }
            shot("workspace-divider-double-tap-$theme")
        }
    }

    @Test fun layerSwipeAlphaLock() {
        showSwipeLayers()
        fun row()=state().array("layers").objects().first { it.getLong("id")==1L }
        fun swipe(dx: Float, cancel: Boolean=false, reverse: Boolean=false) {
            down("layer-row-1"); val start=point
            for(i in 1..4)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/4,0f))
            if(reverse)event(MotionEvent.ACTION_MOVE,start)
            event(if(cancel)MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP);idle()
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            for(device in listOf(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS,MotionEvent.TOOL_TYPE_MOUSE)) {
                tool=device
                swipe(18f);assertFalse("Short swipe",row().getBoolean("alpha_locked"))
                swipe(60f,cancel=true);assertFalse("Cancelled swipe",row().getBoolean("alpha_locked"))
                swipe(60f,reverse=true);assertFalse("Returned to start",row().getBoolean("alpha_locked"))
                swipe(60f)
                assertEquals("Swipe right $theme/$device",device!=MotionEvent.TOOL_TYPE_MOUSE,row().getBoolean("alpha_locked"))
                if(device==MotionEvent.TOOL_TYPE_MOUSE)continue
                shot("layer-alpha-lock-$theme-$device")
                action(obj("type" to "invoke","command" to "undo"))
                waitFor("Undo restores alpha lock $theme/$device") { !row().getBoolean("alpha_locked") }
                action(obj("type" to "invoke","command" to "redo"))
                waitFor("Redo restores alpha lock $theme/$device") { row().getBoolean("alpha_locked") }
                swipe(60f);assertFalse("Second swipe unlocks",row().getBoolean("alpha_locked"))
                swipe(-60f);assertNotNull("Left reveals Delete",node("layer-delete-1"))
                swipe(90f);assertNull("Reverse closes Delete",node("layer-delete-1"))
                assertFalse("Closing Delete does not toggle alpha lock",row().getBoolean("alpha_locked"))
                action(obj("type" to "layer","action" to obj("op" to "lock","id" to 1,"value" to true)))
                swipe(60f);assertFalse("Locked layer",row().getBoolean("alpha_locked"))
                action(obj("type" to "layer","action" to obj("op" to "lock","id" to 1,"value" to false)))
            }
        }
    }

    @Test fun filterDrawerLayersAndPenScrolling() {
        resetPreset("builtin:workspace:painter")
        fun header(panel: String) = "header-control-" + entries().first {
            it.getJSONObject("item").objectOrNull("control")?.optString("panel") == panel
        }.getInt("id")
        fun drawer() = state().getJSONObject("customization").objectOrNull("drawer")
        fun openLayerRows() {
            val opener=header("layers")
            val headerId=opener.removePrefix("header-control-").toInt()
            fun currentLayers()=drawer()?.objectOrNull("anchor")?.let { it.optString("kind")=="header" && it.optInt("id")==headerId }==true
            if(currentLayers()) {
                if(drawer()!!.getBoolean("layer_properties")) {
                    tap("layer-properties-disclosure")
                    waitFor("Native Layers properties close") { currentLayers() && !drawer()!!.getBoolean("layer_properties") }
                }
                tap(opener)
                waitFor("Native Layers drawer closes") { drawer()==null }
            }
            tap(opener)
            waitFor("Native Layers drawer published") {
                drawer()?.let { current ->
                    val anchor=current.getJSONObject("anchor")
                    anchor.optString("kind")=="header" && anchor.optInt("id")==headerId && current.has("layer_properties") &&
                        current.getJSONArray("columns").values().any { (it as JSONArray).values().contains("layers") }
                }==true
            }
            if(drawer()!!.getBoolean("layer_properties"))tap("layer-properties-disclosure")
            waitFor("Native layer rows page is arranged") {
                if(drawer()?.optBoolean("layer_properties")!=false)return@waitFor false
                val inspector=node("layer-inspector")?.second?.boundsInRoot ?: return@waitFor false
                val target=node("layer-row-2") ?: return@waitFor false
                val row=target.second.boundsInRoot
                val visible=row.intersect(inspector).intersect(androidx.compose.ui.geometry.Rect(0f,0f,target.first.view.width.toFloat(),target.first.view.height.toFloat()))
                row.width>0f && row.height>0f && visible.width>=row.width*.9f && visible.height>=target.second.size.height*.9f
            }
        }
        fun layer(op: String, id: Long) = action(obj("type" to "layer", "action" to obj("op" to op, "id" to id, "mask" to false)))
        val filters=header("adjustments")
        tap(filters)
        waitFor("filter drawer") { drawer() != null }
        assertEquals("[[\"filter_types\"],[\"adjustments\"],[\"properties\"]]",drawer()!!.getJSONArray("columns").toString())
        val choices=state().array("adjustments").objects().take(2)
        val count=state().array("layers").length()
        var selected=0L
        for((i,device) in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS).withIndex()) {
            tool=device
            val id=choices[i%2].getString("id")
            waitFor("Visible filter $id") { (node("adjustment-$id")?.second?.boundsInRoot?.height ?: 0f)>0 }
            tap("adjustment-$id")
            waitFor("Selected filter $id") { state().getJSONObject("filter_picker").optString("selected")==id }
            val next=state().getJSONObject("layer_properties").getLong("layer")
            if(selected!=0L)assertEquals(selected,next) else selected=next
            assertEquals(count+1,state().array("layers").length())
            assertEquals(id,state().getJSONObject("filter_picker").getString("selected"))
            assertTrue(state().array("layers").objects().first { it.getLong("id")==1L }.getBoolean("drawing"))
        }
        tap(filters);assertNull(drawer());tap(filters)
        assertEquals(selected,state().getJSONObject("layer_properties").getLong("layer"))
        for(theme in listOf("light","dark")) { action(obj("type" to "set_theme","theme" to theme));shot("filter-drawer-$theme") }
        tap("cancel-filter");assertNull(drawer());assertEquals(count,state().array("layers").length())
        tap(filters)
        layer("select",2)
        action(obj("type" to "set_color","rgba" to JSONArray(listOf(1.0,0.0,0.0,1.0))))
        tap("color-bucket")
        val paper=state().array("layers").objects().first { it.getLong("id")==2L }
        assertTrue(paper.getBoolean("has_thumbnail"))
        assertEquals("layer-paper-symbolic",paper.getString("content_icon"))
        assertFalse(paper.getBoolean("has_mask"))
        assertFalse(paper.getString("description").contains("Protected"))
        assertTrue(state().getJSONObject("layer_tools").getJSONObject("controls").getBoolean("opacity"))
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            for(name in listOf("Paper", "Solid Color", "Paper")) {
                action(obj("type" to "layer", "action" to obj("op" to "rename", "id" to 2, "name" to name)))
                fun hasTitle(n: SemanticsNode): Boolean =
                    n.config.getOrNull(SemanticsProperties.Text)?.any { it.text == name } == true || n.children.any(::hasTitle)
                waitFor("Properties heading $name") { node("layer-properties")?.second?.let(::hasTitle) == true }
            }
            shot("paper-properties-$theme")
        }
        fun waitThumbnail(label: String, matches: (List<Int>) -> Boolean):List<Int> {
            waitFor(label) { node("layer-thumbnail-2-false") != null }
            val deadline=SystemClock.uptimeMillis()+15000
            while(true) {
                val bounds=screenBounds("layer-thumbnail-2-false")
                val image=instrumentation.uiAutomation.takeScreenshot()
                val last=bounds.width.toInt()-1
                val edge=(last*.2f).roundToInt()
                fun pixel(x:Int)=image.getPixel(bounds.left.toInt()+x,(bounds.top+bounds.height*.35f).toInt())
                val middleLeft=pixel(last/2);val middleRight=pixel((last+1)/2)
                val middle=android.graphics.Color.rgb((android.graphics.Color.red(middleLeft)+android.graphics.Color.red(middleRight))/2,
                    (android.graphics.Color.green(middleLeft)+android.graphics.Color.green(middleRight))/2,
                    (android.graphics.Color.blue(middleLeft)+android.graphics.Color.blue(middleRight))/2)
                val colors=listOf(pixel(edge),middle,pixel(last-edge))
                image.recycle()
                if(matches(colors)) return colors
                assertTrue("$label: $colors", SystemClock.uptimeMillis()<deadline)
                SystemClock.sleep(100)
            }
        }
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme","theme" to theme))
            openLayerRows()
            waitThumbnail("$theme red fill thumbnail") { colors ->
                val color=colors[1]
                android.graphics.Color.red(color)>240 && android.graphics.Color.green(color)<10 && android.graphics.Color.blue(color)<10
            }
            waitFor("Fill type symbol") { node("layer-type-symbol-2") != null }
            assertNull(node("layer-type-symbol-1"))
            shot("fill-thumbnail-$theme")
        }
        tap(filters)
        tap("filter-type-fill")
        tap("adjustment-gradient_fill")
        action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to 2, "key" to "angle", "value" to obj("kind" to "number", "value" to 0))))
        assertEquals("layer-gradient-symbolic",state().array("layers").objects().first { it.getLong("id")==2L }.getString("content_icon"))
        for(theme in listOf("light","dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            openLayerRows()
            waitFor("Gradient type symbol") { node("layer-type-symbol-2") != null }
            var forward=emptyList<Int>()
            for((step, reverse) in listOf(false,true,false).withIndex()) {
                if(reverse) {
                    val destination=state().getJSONObject("layer_properties").array("controls").objects().first { it.getString("key")=="gradient" }
                        .getJSONObject("gradient").getJSONObject("destination")
                    action(obj("type" to "effect","action" to obj("op" to "gradient","target" to destination,"edit" to obj("kind" to "reverse"))))
                } else if(step==2) action(obj("type" to "invoke", "command" to "undo"))
                val colors=waitThumbnail("$theme gradient thumbnail reverse=$reverse") { pixels ->
                    if(step==0) {
                        val gray=pixels.map { android.graphics.Color.red(it) }
                        pixels.all { kotlin.math.abs(android.graphics.Color.red(it)-android.graphics.Color.green(it))<=3 &&
                            kotlin.math.abs(android.graphics.Color.red(it)-android.graphics.Color.blue(it))<=3 } &&
                            gray[0]<gray[1] && gray[1]<gray[2] && gray[2]-gray[0]>100
                    } else pixels.zip(if(reverse)forward.reversed() else forward).all { (actual,expected) ->
                        listOf(16,8,0).all { shift -> kotlin.math.abs(((actual shr shift) and 255)-((expected shr shift) and 255))<=3 }
                    }
                }
                if(step==0)forward=colors
                shot("gradient-thumbnail-$theme-$reverse")
            }
        }
        fun swipe(id: Long, dx: Float) {
            down("layer-row-$id");val start=point
            for(i in 1..5)event(MotionEvent.ACTION_MOVE,start+Offset(dx*density*i/5,0f))
            event(MotionEvent.ACTION_UP);idle()
        }
        tool=MotionEvent.TOOL_TYPE_MOUSE
        swipe(1,-90f)
        assertNull("Mouse row drag does not reveal Delete",node("layer-delete-1"))
        for((id,device) in listOf(1L to MotionEvent.TOOL_TYPE_STYLUS,2L to MotionEvent.TOOL_TYPE_FINGER)) {
            tool=device
            swipe(id,-90f)
            waitFor("Delete is fully revealed") { node("layer-delete-$id")?.second?.let {
                it.size.width>0 && it.size.height>0 && kotlin.math.abs(it.boundsInRoot.width-it.size.width)<=1f &&
                    kotlin.math.abs(it.boundsInRoot.height-it.size.height)<=1f
            }==true }
            swipe(id,90f)
            waitFor("Reverse closes Delete") { node("layer-delete-$id")==null }
            swipe(id,-90f)
            shot("layer-delete-$id")
            tap("layer-delete-$id")
            assertFalse(state().array("layers").objects().any { it.getLong("id")==id })
        }
        assertEquals(0,state().array("layers").length())
        action(obj("type" to "invoke","command" to "undo"))
        action(obj("type" to "invoke","command" to "undo"))
        assertEquals(2,state().array("layers").length())
        // Constrain the native viewport so this small catalog actually overflows.
        instrumentation.runOnMainSync { host.resize((1200*density).toInt(),(450*density).toInt(),density) }
        idle()
        val brush="header-control-"+entries().first { it.getJSONObject("item").objectOrNull("control")?.optString("command")=="drawing_brush" }.getInt("id")
        tap(brush)
        if(drawer()==null)tap(brush)
        val sets=state().getJSONObject("tool_panels").getJSONObject("brush_sets").array("groups").objects()
        var longest=sets.first();var size=0
        for(set in sets) {
            action(set.getJSONObject("action"))
            val count=state().getJSONObject("tool_set").array("subtools").length()
            if(count>size){size=count;longest=set}
        }
        action(longest.getJSONObject("action"))
        val first="subtool-"+state().getJSONObject("tool_set").array("subtools").getJSONObject(0).getString("label")
        for(device in listOf(MotionEvent.TOOL_TYPE_MOUSE,MotionEvent.TOOL_TYPE_FINGER,MotionEvent.TOOL_TYPE_STYLUS)) {
            tool=device
            // Reopen to reset the native scroll position before each contact.
            tap(brush);tap(brush)
            waitFor("Tools shown") { (node(first)?.second?.boundsInRoot?.height ?: 0f)>0 }
            val before=bounds(first).top
            down(first);val start=point
            for(i in 1..6)event(MotionEvent.ACTION_MOVE,start+Offset(0f,-35*density*i))
            event(MotionEvent.ACTION_UP);idle()
            val after=node(first)?.second?.boundsInRoot
            if(device==MotionEvent.TOOL_TYPE_MOUSE)assertEquals(before,after!!.top,1f)
            else assertTrue("Touch and pen scroll tool choices ($device): $before -> $after",after==null || after.height==0f || after.top<before-10*density)
        }
        shot("pen-tools-scroll")
        android.util.Log.i("FilterAcceptance","PASS: three panels, replacement/reopen/cancel, paper color, mouse/touch/pen, reversible swipe, final layer deletion/undo, tool scrolling")
    }

    @Test fun touchAndPenHoldsOpenItemMenusOutsideEditing() {
        restore()
        fun preferencesOpen() = snapshot().objectOrNull("preferences") != null
        fun hold(device: Int) {
            tool = device
            down("header-item-3")
            SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        }
        hold(MotionEvent.TOOL_TYPE_MOUSE)
        instrumentation.runOnMainSync { assertNull("Mouse hold never opens a context menu", node("workspace-menu")) }
        event(MotionEvent.ACTION_UP)
        waitFor("mouse release clicks Settings") { preferencesOpen() }
        action(obj("type" to "close_settings"))
        for (device in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            hold(device)
            waitFor("held item menu") { node("workspace-menu") != null }
            event(MotionEvent.ACTION_CANCEL)
            waitFor("cancellation closes the held menu") { node("workspace-menu") == null }
            hold(device)
            event(MotionEvent.ACTION_UP)
            waitFor("release keeps the held menu") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
            idle()
            assertFalse("A hold suppresses the click", preferencesOpen())
            tapMenuRow("Customize Title Bar…")
            waitFor("held menu enters inline editor") { editing() && node("title-bar")?.first?.view?.hasWindowFocus() == true }
            tap("header-edit-cancel")
            waitFor("editor closed") { !editing() }
        }
    }

    @Test fun keyboardContextHoldAndFocusLossKeepTheirOwnership() {
        restore(); startEditor(); tool = MotionEvent.TOOL_TYPE_MOUSE
        tap("header-item-1")
        instrumentation.runOnMainSync { assertTrue("Pointer selects Capy", node("header-item-1")!!.second.config.getOrNull(SemanticsProperties.Selected) == true) }
        key(KeyEvent.KEYCODE_DPAD_RIGHT)
        assertEquals(1, model().array("zones").getJSONArray(0).getJSONObject(1).getInt("id"))
        key(KeyEvent.KEYCODE_DPAD_RIGHT)
        assertEquals(1, model().array("zones").getJSONArray(1).getJSONObject(0).getInt("id"))
        key(KeyEvent.KEYCODE_FORWARD_DEL)
        assertFalse(entries().any { it.getInt("id") == 1 })
        key(KeyEvent.KEYCODE_ESCAPE)
        assertFalse(editing()); assertTrue(entries().any { it.getInt("id") == 1 })
        startEditor()
        val baseline = model().toString()
        down("header-item-1")
        SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
        instrumentation.runOnMainSync { assertNull("Mouse hold never opens a context menu", node("workspace-menu")) }
        event(MotionEvent.ACTION_UP)
        assertEquals(baseline, model().toString())
        button = MotionEvent.BUTTON_SECONDARY
        down("header-item-1"); event(MotionEvent.ACTION_UP)
        waitFor("secondary context menu focus") { node("workspace-menu")?.first?.view?.hasWindowFocus() == true }
        shot("secondary-context")
        key(KeyEvent.KEYCODE_BACK)
        waitFor("context closed") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
        idle()
        button = MotionEvent.BUTTON_PRIMARY
        for (device in listOf(MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_STYLUS)) {
            tool = device
            android.util.Log.i("TitleBarAcceptance", "Hold context $device")
            down("header-item-1")
            SystemClock.sleep(android.view.ViewConfiguration.getLongPressTimeout().toLong() + 150)
            waitFor("touch or pen hold context") { node("workspace-menu") != null }
            event(MotionEvent.ACTION_MOVE, center())
            waitFor("drag dismisses held context") { node("workspace-menu") == null }
            event(MotionEvent.ACTION_CANCEL)
            waitFor("held contact releases native focus") { node("workspace-menu") == null && node("title-bar")?.first?.view?.hasWindowFocus() == true }
            idle()
            assertEquals(baseline, model().toString())
        }
        tool = MotionEvent.TOOL_TYPE_MOUSE
        down("header-item-1"); event(MotionEvent.ACTION_MOVE, outside())
        var dialog: android.app.Dialog? = null
        scenario.onActivity { activity ->
            dialog = android.app.Dialog(activity).apply {
                setContentView(android.widget.TextView(activity).apply { text = "Capture-loss test" })
                show()
            }
        }
        waitFor("native dialog takes focus") { dialog?.window?.decorView?.hasWindowFocus() == true }
        event(MotionEvent.ACTION_CANCEL)
        instrumentation.runOnMainSync { dialog!!.dismiss() }
        waitFor("activity focus restored") { node("title-bar")?.first?.view?.hasWindowFocus() == true }
        assertEquals("Focus loss cancels the drag", baseline, model().toString())
        tap("header-edit-cancel")
    }

}
