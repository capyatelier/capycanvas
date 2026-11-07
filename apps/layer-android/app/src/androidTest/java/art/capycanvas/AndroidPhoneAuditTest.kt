package art.capycanvas

import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.WindowInsets
import android.view.RoundedCorner
import android.view.MotionEvent
import android.view.KeyEvent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.junit4.createEmptyComposeRule
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
import kotlin.math.roundToInt

class AndroidPhoneAuditTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    private fun CanvasHost.awaitFrame(label: String, timeout: Long = 10_000, diagnostics: () -> String = { "" }, condition: () -> Boolean) =
        awaitMain(label, timeout, diagnostics, compose, condition)

    private fun shell(command: String): String = ParcelFileDescriptor.AutoCloseInputStream(
        instrumentation.uiAutomation.executeShellCommand(command)).bufferedReader().use { it.readText().trim() }

    private fun awaitCanvasPresentation(host: CanvasHost) {
        val deadline = SystemClock.uptimeMillis() + 30_000
        var presented = false
        do {
            presented = runBlocking { host.withNative {
                val display = JSONObject(Native.displayStatus(it))
            !Native.renderingPending(it) && display.optLong("completed_frames") == display.optLong("submitted_frames") &&
                !display.isNull("presented_canvas_revision") &&
                    display.getLong("canvas_revision") == display.getLong("presented_canvas_revision")
            } }
            if (!presented) SystemClock.sleep(16)
        } while (!presented && SystemClock.uptimeMillis() < deadline)
        assertTrue("Current document composition is presented", presented)
    }

    private fun collect(node: SemanticsNode, allocations: JSONObject) {
        node.config.getOrNull(SemanticsProperties.TestTag)?.let { tag ->
            val bounds = node.boundsInRoot
            allocations.put(tag, JSONArray(listOf(bounds.left, bounds.top, bounds.width, bounds.height)))
        }
        node.children.forEach { collect(it, allocations) }
    }

    private fun awaitEditorPage(activity: MainActivity,host: CanvasHost,tag: String) {
        val decor = activity.window.decorView
        val density = activity.resources.displayMetrics.density
        val visible = obj("x" to 0,"y" to 0,"width" to decor.width/density,
            "height" to (decor.height-decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom).coerceAtLeast(0)/density)
        val presentation = runBlocking { host.awaitQuery(obj("type" to "dialog_presentation","role" to "editor","visible" to visible)) }!!
        assertEquals("IME preserves the shared page presentation","page",presentation.getString("kind"))
        val expected = presentation.getJSONObject("bounds").rect()
        host.awaitFrame("$tag uses its shared visible bounds",diagnostics = { "expected=$expected; actual=${findTag(tag)?.second?.boundsInRoot}; visible=$visible" }) {
            findTag(tag)?.second?.boundsInRoot?.let { actual ->
                kotlin.math.abs(actual.left-expected.left*density)<=1f && kotlin.math.abs(actual.top-expected.top*density)<=1f &&
                    kotlin.math.abs(actual.width-expected.width*density)<=1f && kotlin.math.abs(actual.height-expected.height*density)<=1f
            } == true
        }
    }

    @Test fun hardwareTab() {
        val scenario = launchCapy(180_000,compose)
        try {
            val host = scenario.activity().host
            fun hidden() = host.snapshot!!.getJSONObject("state").getJSONObject("phone").getBoolean("hidden") || host.snapshot!!.optBoolean("chrome_hidden")
            if(hidden())host.drain(obj("type" to "invoke","command" to "zen_mode"))
            compose.waitForIdle()
            assertFalse("The workspace owns the hardware key",host.editingText)
            pressKey(KeyEvent.KEYCODE_TAB)
            host.awaitFrame("Hardware Tab hides chrome",diagnostics = { "editing=${host.editingText}; dialog=${host.dialogKeyHandler != null}; header=${host.headerKeyHandler != null}; menu=${host.menuKeyHandler != null}; phone=${host.snapshot?.getJSONObject("state")?.opt("phone")}" }) { hidden() }
            pressKey(KeyEvent.KEYCODE_TAB)
            host.awaitFrame("Hardware Tab restores chrome") { !hidden() }
        } finally { scenario.close() }
    }

    @Test fun admission() = visit(true)
    @Test fun frame() = visit(false)
    @Test fun menu() = visit(false, "menu")
    @Test fun drawers() = visit(false, "drawers")
    @Test fun customize() = visit(false, "customize")
    @Test fun dialogsA2() = visit(false, "dialogsA2")
    @Test fun dialogsD1() = visit(false, "dialogsD1")
    @Test fun pagesD1() = visit(false, "pagesD1")
    @Test fun dialogsD2() = visit(false, "dialogsD2")
    @Test fun pagesD2() = visit(false, "pagesD2")
    @Test fun ime() = visit(false, "ime")
    @Test fun alertIme() = visit(false, "alertIme")
    @Test fun modalFocus() = visit(false, "modalFocus")

    private fun visit(admissionOnly: Boolean, family: String = "frame") {
        val args = InstrumentationRegistry.getArguments()
        val case = requireNotNull(args.getString("case"))
        require(case.matches(Regex("[a-zA-Z0-9_.-]+")))
        val size = requireNotNull(args.getString("size")).split("x").map(String::toInt)
        val expectedDensity = requireNotNull(args.getString("density")).toInt()
        val expectedTheme = requireNotNull(args.getString("theme"))
        val expectedFont = args.getString("fontScale", "1").toFloat()
        val cutout = args.getString("cutout", "none")
        val directory = File(instrumentation.targetContext.getExternalFilesDir(null), "phone-audit/$case").apply { mkdirs() }
        for (name in if (admissionOnly) listOf("admission") else listOf("rest", "swapped", "hidden", "slider-size", "slider-strength", "capy-hold", "paint-default")) {
            File(directory, "$name.json").delete()
            File(directory, "$name.png").delete()
        }
        val started = SystemClock.elapsedRealtime()
        val report = obj("schema_version" to 1, "host" to "android", "family" to family, "state" to "rest", "case" to case,
            "fixture" to "builtin:workspace:painter", "theme" to expectedTheme, "text_size" to args.getString("textSize", "11").toDouble(), "requested_pixels" to JSONArray(size),
            "requested_density" to expectedDensity, "requested_font_scale" to expectedFont, "status" to "failed", "passed" to false)
        var captureName = if (admissionOnly) "admission" else "startup"
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
            scenario = androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
            val activity = scenario.activity()
            val host = activity.host
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:painter")) }
            host.awaitFrame("Painter fixture", 30_000) { host.workspaceManager?.optString("id") == "builtin:workspace:painter" && host.workspaceManager?.optBoolean("busy") == false }
            host.awaitFrame("first canvas surface", 180_000, { "$report" }) { host.surfaceReady }
            val frameDeadline = SystemClock.elapsedRealtime() + 180_000
            var frameReady = false
            while (!frameReady && SystemClock.elapsedRealtime() < frameDeadline) {
                frameReady = runBlocking { host.withNative {
                    val status = JSONObject(Native.displayStatus(it))
                    status.optLong("completed_frames") > 0 && status.optLong("completed_frames") == status.optLong("submitted_frames") && !Native.renderingPending(it)
                } }
                if (!frameReady) SystemClock.sleep(16)
            }
            assertTrue("Completed first canvas frame", frameReady)
            fun capture(name: String) {
                captureName = name
                report.put("family",if (name == "hidden") "frame" else if (name.startsWith("inventory-")) "inventory" else if(family in listOf("dialogsA2","pagesD1","pagesD2")) "pages" else family)
                    .put("fixture",host.workspaceManager?.optString("id") ?: "builtin:workspace:painter").put("state", if (admissionOnly) "rest" else name).put("status", "failed").put("passed", false)
                host.drain()
                compose.waitForIdle()
                if (!admissionOnly) awaitCanvasPresentation(host)
                host.awaitFrame("Brush ready and Compose idle") { host.snapshot?.optBoolean("brush_ready") == true &&
                    host.snapshot?.isNull("canvas_notice") == true }
                compose.waitForIdle()
                instrumentation.waitForIdleSync()
                val display = runBlocking { host.withNative { JSONObject(Native.displayStatus(it)) } }
                report.put("display", display)
                report.put("navigation_mode",shell("settings get secure navigation_mode").toInt())
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
                    val safe = insets.getInsets(WindowInsets.Type.displayCutout())
                    val waterfall = insets.displayCutout?.waterfallInsets ?: android.graphics.Insets.NONE
                    val mandatory = insets.getInsets(WindowInsets.Type.mandatorySystemGestures())
                    val cutoutInsets = listOf(maxOf(safe.left,waterfall.left),maxOf(safe.top,waterfall.top),maxOf(safe.right,waterfall.right),maxOf(safe.bottom,waterfall.bottom))
                    val actualInsets = cutoutInsets.zip(listOf(mandatory.left,mandatory.top,mandatory.right,mandatory.bottom)).map { (a,b) -> maxOf(a,b) }
                    report.put("mandatory_system_gesture_insets",JSONArray(listOf(mandatory.left,mandatory.top,mandatory.right,mandatory.bottom)))
                    report.put("cutout_waterfall_insets",JSONArray(cutoutInsets))
                    report.put("reported_insets",host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets"))
                    val phone = host.snapshot!!.getJSONObject("layout").getJSONObject("phone")
                    val clear = androidx.compose.ui.geometry.Rect(mandatory.left.toFloat()/density,mandatory.top.toFloat()/density,
                        (decor.width-mandatory.right)/density,(decor.height-mandatory.bottom)/density)
                    val controls = phone.array("bars").objects()+phone.array("tools").objects()+listOf("size","strength","settings","capy").map { phone.getJSONObject(it) }
                    controls.filter { it.number("width")>0 && it.number("height")>0 }.forEach { control ->
                        val rect = control.rect()
                        assertTrue("Visible phone control $rect avoids mandatory gestures $clear",rect.left>=clear.left-.05f && rect.top>=clear.top-.05f && rect.right<=clear.right+.05f && rect.bottom<=clear.bottom+.05f)
                    }
                    actualInsets.forEachIndexed { side,value ->
                        assertEquals("Reported phone inset $side is the native safe maximum",value/density,
                            host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets").getDouble(side).toFloat(),.05f)
                    }
                    report.put("insets_pixels", JSONArray(actualInsets))
                    report.put("insets", JSONArray(actualInsets.map { it / density }))
                        report.put("ime_bottom_pixels", insets.getInsets(WindowInsets.Type.ime()).bottom)
                    report.put("crop", obj("x" to crop[0], "y" to crop[1], "width" to crop[2], "height" to crop[3]))
                    val surface = checkNotNull(decor.descendant<CanvasSurfaceView>())
                    report.put("canvas_viewport", JSONArray(listOf(surface.width / density, surface.height / density)))
                    val canvasLocation = IntArray(2)
                    surface.getLocationOnScreen(canvasLocation)
                    report.put("canvas_origin_pixels", JSONArray(canvasLocation.toList()))
                    val root = checkNotNull(findTag("workspace"))
                    collect(if(family == "pagesD1" || family == "pagesD2")root.first.semanticsOwner.unmergedRootSemanticsNode else root.second, allocations)
                    report.put("phone_active", host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") != null)
                    report.put("revision", host.snapshot?.getJSONObject("state")?.getLong("revision"))
                    assertEquals("System theme", expectedTheme, host.snapshot?.getJSONObject("state")?.getString("theme"))
                    assertEquals("Configured density", expectedDensity, metrics.densityDpi)
                    assertEquals("Configured font scale", expectedFont, activity.resources.configuration.fontScale, .001f)
                    assertEquals("App viewport width in pixels", size[0], decor.width)
                    assertEquals("App viewport height in pixels", size[1], decor.height)
                    assertEquals("App viewport width in dp", size[0] * 160f / expectedDensity, decor.width / density, .01f)
                    assertEquals("App viewport height in dp", size[1] * 160f / expectedDensity, decor.height / density, .01f)
                    if (cutout == "none") assertEquals("No cutout/waterfall insets",listOf(0,0,0,0),cutoutInsets)
                    else assertTrue("Corner cutout produces safe insets",cutoutInsets.any { it > 0 })
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
                    File(directory, "$captureName.png").outputStream().use { assertTrue(image.compress(Bitmap.CompressFormat.PNG, 100, it)) }
                    report.put("dimensions", JSONArray(listOf(image.width, image.height)))
                } finally {
                    if (image !== screenshot) image.recycle()
                    screenshot.recycle()
                }
                report.put("status", "captured").put("passed", true)
                report.put("elapsed_seconds", (SystemClock.elapsedRealtime() - started) / 1000.0)
                File(directory, "$captureName.json").writeText(report.toString(2) + "\n")
            }
            if (admissionOnly) capture("admission")
            else visitFrame(activity, host, family, args.getString("states"), ::capture) { name, error ->
                val failed = JSONObject(report.toString()).put("state", name).put("passed", false).put("status", "failed").put("error", error.toString())
                File(directory, "$name.png").delete()
                File(directory, "$name.json").writeText(failed.toString(2) + "\n")
            }

        } catch (error: Throwable) {
            report.put("error", error.toString())
            throw error
        } finally {
            report.put("elapsed_seconds", (SystemClock.elapsedRealtime() - started) / 1000.0)
            if (!File(directory, "$captureName.json").exists()) File(directory, "$captureName.json").writeText(report.toString(2) + "\n")
            File(directory, "run.json").writeText(obj("passed" to !report.has("error"), "error" to report.opt("error")).toString(2) + "\n")
            scenario?.close()
        }
    }
    private fun createDrawing(host: CanvasHost, width: Int, height: Int, name: String) {
        val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
        host.drain(obj("type" to "invoke", "command" to "new_document"))
        host.awaitFrame("New Drawing controller is open") { host.snapshot!!.getJSONObject("state").objectOrNull("new_drawing") != null }
        compose.waitForIdle()
        runBlocking { host.withNative {
            orderDrawing(it, width, height, create = false)
            Native.dispatch(it,obj("type" to "new_drawing", "action" to obj("op" to "name", "text" to name)).toString())
        } }
        instrumentation.runOnMainSync { host.documents.createNewDrawing() }
        host.awaitFrame("New Drawing controller adopts the fixture", 60_000) {
            val state = host.snapshot!!.getJSONObject("state")
            state.getJSONObject("document_file").getLong("epoch") != epoch &&
                state.array("tabs").objects().any { it.getBoolean("active") && it.getInt("width") == width && it.getInt("height") == height } &&
                !host.documents.working && !host.drawingTabs.switching && state.objectOrNull("new_drawing") == null
        }
    }

    private fun visitFrame(activity: MainActivity, host: CanvasHost, family: String, states: String?, capture: (String) -> Unit, failed: (String, Throwable) -> Unit) {
        host.awaitReady(180_000, compose)
        val requested = states?.split(",")?.toSet()
        val failures = mutableListOf<String>()
        fun state(name: String, body: () -> Unit = {}) {
            if (requested != null && name !in requested) return
            try { body(); capture(name) } catch (error: Throwable) {
                failures += "$name: $error"; failed(name, error)
            }
        }
        fun phone() = host.snapshot!!.getJSONObject("state").getJSONObject("phone")
        fun point(tag: String): Offset {
            compose.waitForIdle()
            host.awaitFrame("Placed $tag") { findTag(tag)?.second?.let { it.layoutInfo.isPlaced && it.boundsInRoot.width > 0 && it.boundsInRoot.height > 0 } == true }
            var result = Offset.Zero
            instrumentation.runOnMainSync {
                val (root, node) = checkNotNull(findTag(tag)) { "Missing $tag" }
                val screen = IntArray(2); root.view.getLocationOnScreen(screen)
                val insets = phone().getJSONArray("insets")
                val density = activity.resources.displayMetrics.density
                val clear = androidx.compose.ui.geometry.Rect(insets.getDouble(0).toFloat()*density,insets.getDouble(1).toFloat()*density,
                    activity.window.decorView.width-insets.getDouble(2).toFloat()*density,activity.window.decorView.height-insets.getDouble(3).toFloat()*density)
                val target = node.boundsInRoot.translate(Offset(screen[0].toFloat(),screen[1].toFloat())).intersect(clear)
                assertTrue("$tag has a native safe-area input point: $target",target.width>0 && target.height>0)
                result = target.center
            }
            return result
        }
        var downAt = 0L
        var pressedPoint: Offset? = null
        fun event(action: Int, p: Offset) {
            if (action == MotionEvent.ACTION_DOWN) { downAt = SystemClock.uptimeMillis(); pressedPoint = p }
            val event = motion(MotionEvent.TOOL_TYPE_FINGER, action, p, downAt)
            try { assertTrue("Inject $action at $p", instrumentation.uiAutomation.injectInputEvent(event, true)) }
            finally { event.recycle(); if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) pressedPoint = null }
        }
        fun tap(tag: String) { val p = point(tag); event(MotionEvent.ACTION_DOWN, p); event(MotionEvent.ACTION_UP, p); host.drain() }
        createDrawing(host,1200,1600,"Phone drawing")
        host.drain(obj("type" to "invoke", "command" to "sculpt"))
        host.drain(obj("type" to "select_tool_group", "group" to "blend"))
        host.drain(obj("type" to "select_brush", "id" to 2))
        host.drain(obj("type" to "invoke", "command" to "fit_canvas"))
        host.awaitFrame("Phone frame and sliders") {
            findTag("phone-capy") != null && findTag("component-slider-size") != null && findTag("component-slider-strength") != null
        }
        compose.waitForIdle()
        host.drain()
        compose.waitForIdle()
        if (family == "alertIme") {
            host.drain(obj("type" to "invoke","command" to "search_commands"))
            host.awaitFrame("Standalone alert fixture has a search field") { findTag("command-search") != null }
            val owner = checkNotNull(findTag("command-search")).first.view
            host.awaitFrame("Standalone alert fixture measures the native keyboard") { owner.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
            val keyboardHeight = owner.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom
            tap("command-close")
            host.awaitFrame("Search closes before the standalone alert") { host.commandSearch == null && findTag("command-bar") == null }
            alertImeAllocation(activity,host,keyboardHeight,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if (family == "ime" || family == "modalFocus") {
            visitIme(activity,host,::tap,::state,checkAlert = family == "ime")
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if(family=="pagesD2") {
            visitPagesD2(activity,host,::event,::tap,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if(family=="dialogsD2") {
            visitDialogsD2(activity,host,::tap)
            return
        }
        if(family=="dialogsD1") {
            visitDialogsD1(host,::tap)
            return
        }
        if(family=="pagesD1") {
            visitPagesD1(activity,host,::event,::tap,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if (family == "dialogsA2") {
            visitDialogsA2(activity,host,::event,::tap,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if (family == "customize") {
            visitCustomize(activity,host,::event,::tap,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if (family == "drawers") {
            visitDrawers(activity,host,::event,::tap,::state)
            assertTrue(failures.joinToString("\n"),failures.isEmpty())
            return
        }
        if (family == "menu") {
            visitMenu(activity,host, ::event, ::tap, ::state) { pressedPoint?.let { event(MotionEvent.ACTION_CANCEL,it) } }
            assertTrue(failures.joinToString("\n"), failures.isEmpty())
            return
        }
        if (android.os.Build.VERSION.SDK_INT >= 31) {
            var content: CanvasContent? = null
            var nativeInsets: WindowInsets? = null
            instrumentation.runOnMainSync {
                content = semanticsRoots().first().view.rootView.descendant<CanvasContent>()
                nativeInsets = checkNotNull(content).rootWindowInsets
            }
            val original = checkNotNull(nativeInsets)
            val originalCorners = jsonValue(phone().getJSONArray("corners"))
            val originalCorner = original.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)
            val radius = if (originalCorner?.radius == 40) 48 else 40
            val changed = WindowInsets.Builder(original).setRoundedCorner(RoundedCorner.POSITION_TOP_LEFT,
                RoundedCorner(RoundedCorner.POSITION_TOP_LEFT, radius, radius, radius)).build()
            try {
                instrumentation.runOnMainSync {
                    checkNotNull(content).dispatchApplyWindowInsets(changed)
                    assertEquals("Native inset callback observes corner-only changes", radius,
                        host.windowInsets?.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)?.radius)
                }
                host.awaitFrame("Corner-only native inset change is published", diagnostics = { "native=${host.windowInsets?.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)}, shared=${phone().getJSONArray("corners")}" }) {
                    kotlin.math.abs(phone().getJSONArray("corners").getDouble(0) - radius / checkNotNull(host.workspaceInteraction).density) < .001
                }
            } finally {
                val restored = WindowInsets.Builder(original).setRoundedCorner(RoundedCorner.POSITION_TOP_LEFT, originalCorner).build()
                instrumentation.runOnMainSync { checkNotNull(content).dispatchApplyWindowInsets(restored) }
            }
            host.awaitFrame("Native corner metrics restored", diagnostics = { "original=$originalCorners native=${original.getRoundedCorner(RoundedCorner.POSITION_TOP_LEFT)} current=${phone().getJSONArray("corners")}" }) {
                jsonValue(phone().getJSONArray("corners")) == originalCorners
            }
        }
        val originalInsets = JSONArray(phone().getJSONArray("insets").toString())
        val originalCorners = JSONArray(phone().getJSONArray("corners").toString())
        val blocked = host.documentInputBlocked
        try {
            runBlocking { host.withNative { host.documentInputBlocked = true } }
            instrumentation.runOnMainSync { host.reportPhoneMetrics(JSONArray(listOf(19,7,13,5)), originalCorners) }
            host.awaitFrame("Metrics arrive during a document interaction") { phone().getJSONArray("insets").getInt(0) == 19 }
        } finally {
            host.documentInputBlocked = blocked
            instrumentation.runOnMainSync { host.reportPhoneMetrics(originalInsets, originalCorners) }
            host.awaitFrame("Restore native inset metrics") { phone().getJSONArray("insets").toString() == originalInsets.toString() }
        }
        instrumentation.runOnMainSync {
            assertNull("Desktop header is absent", findTag("title-bar"))
            assertNotNull(host.snapshot!!.getJSONObject("layout").objectOrNull("phone"))
        }
        state("rest")
        host.drain(obj("type" to "invoke", "command" to "drawing_brush"))
        host.drain(obj("type" to "set_color", "rgba" to JSONArray(listOf(0, 0, 0, 1))))
        host.drain(obj("type" to "set_brush_size", "value" to 40))
        state("swapped") {
            host.drain(obj("type" to "phone", "action" to obj("kind" to "swap_sides")))
            host.awaitFrame("Swapped bars") { phone().getBoolean("swapped") }
        }
        if (phone().getBoolean("swapped")) host.drain(obj("type" to "phone", "action" to obj("kind" to "swap_sides")))
        state("hidden") {
            tap("phone-capy")
            host.awaitFrame("Capy menu") { findTag("phone-zen") != null }
            tap("phone-zen")
            host.awaitFrame("Hidden bars") { phone().getBoolean("hidden") }
        }
        if (phone().getBoolean("hidden")) {
            tap("phone-capy")
            host.awaitFrame("Capy restores bars") { !phone().getBoolean("hidden") }
        }
        for (name in listOf("size", "strength")) {
            val index = if (name == "size") 0 else 1
            fun field() = host.snapshot!!.array("phone_sliders").getJSONObject(index).getJSONObject("numeric")
            val original = field().number("value")
            fun fill(): Float = field().let { field -> JSONObject(Native.number(obj("control" to field.getJSONObject("numeric"),
                "value" to field.number("value"), "operation" to obj("type" to "format")).toString(), host.languageTag)).number("fill") }
            state("slider-$name") {
                val p = point("component-slider-$name")
                event(MotionEvent.ACTION_DOWN, p)
                host.awaitFrame("$name slider held") { !phone().isNull("slider") }
                val geometry = host.snapshot!!.getJSONObject("layout").getJSONObject("phone")
                val vertical = geometry.getString("edge") != "top"
                val viewport = host.snapshot!!.getJSONObject("layout").getJSONArray("viewport")
                val band = geometry.getJSONObject(name)
                val extent = viewport.getDouble(if (vertical) 1 else 0).toFloat()
                val across = viewport.getDouble(if (vertical) 0 else 1).toFloat()
                val start = band.number(if (vertical) "x" else "y")
                val end = start + band.number(if (vertical) "width" else "height")
                val distance = (phoneUi(obj("kind" to "slider_stretch_distance", "layout" to geometry)) as Number).toFloat() + 8
                val cross = if ((start+end)/2 < across/2) end+distance else start-distance
                val away = if (vertical) Offset(cross, extent*.25f) else Offset(extent*.75f, cross)
                event(MotionEvent.ACTION_MOVE, away * checkNotNull(host.workspaceInteraction).density)
                host.awaitFrame("$name whole-window mapping") { kotlin.math.abs(fill()-.75f) < .015f }
                event(MotionEvent.ACTION_MOVE, p)
                host.awaitFrame("$name returns to track mapping") { kotlin.math.abs(fill()-.5f) < .015f }
            }
            if (pressedPoint != null) {
                val wasBlocked = host.documentInputBlocked
                try {
                    runBlocking { host.withNative { host.documentInputBlocked = true } }
                    instrumentation.runOnMainSync {
                        val input = checkNotNull(host.workspaceInteraction?.phone)
                        assertTrue("Slider owns a cancellable contact", input.jobs.isNotEmpty())
                        host.retirePhoneContacts()
                        assertTrue("Retirement cancels contacts before returning", input.jobs.isEmpty())
                    }
                    pressedPoint?.let { event(MotionEvent.ACTION_CANCEL, it) }
                    host.awaitFrame("$name cancels while document input is blocked") { phone().isNull("slider") }
                    host.awaitFrame("$name cancellation restores its starting value") { kotlin.math.abs(field().number("value")-original) < .001f }
                } finally { runBlocking { host.withNative { host.documentInputBlocked = wasBlocked } } }
            }
        }
        state("capy-hold") {
            val p = point("phone-capy")
            event(MotionEvent.ACTION_DOWN, p)
            host.awaitFrame("Capy held") { host.workspaceInteraction?.phone?.let { it.capy && it.held } == true }
        }
        if (pressedPoint != null) {
            val wasBlocked = host.documentInputBlocked
            try {
                runBlocking { host.withNative { host.documentInputBlocked = true } }
                instrumentation.runOnMainSync {
                    val dock = checkNotNull(host.workspaceInteraction)
                    val input = checkNotNull(dock.phone)
                    assertTrue(input.held)
                    dock.popup(true)
                    assertFalse("Popup ownership retires Capy synchronously", input.active)
                    assertTrue(input.jobs.isEmpty())
                    dock.popup(false)
                }
                pressedPoint?.let { event(MotionEvent.ACTION_CANCEL, it) }
                host.awaitFrame("Capy rollback arrives while input is blocked") { !phone().getBoolean("capy_drag") }
            } finally { runBlocking { host.withNative { host.documentInputBlocked = wasBlocked } } }
            host.awaitFrame("Retired pointer click suppression ends with its frame") { host.workspaceInteraction?.phone?.suppressed == false }
            compose.waitForIdle()
            instrumentation.runOnMainSync {
                val node = checkNotNull(findTag("phone-tool-settings-hit")).second.find { it.config.getOrNull(SemanticsActions.OnClick) != null }
                assertTrue("Semantic activation after a hold", checkNotNull(node).config[SemanticsActions.OnClick].action!!.invoke())
            }
            host.awaitFrame("Semantic activation dispatches Tool settings") {
                host.snapshot!!.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer")
                    ?.getJSONObject("anchor")?.objectOrNull("anchor")?.optString("kind") == "tool_settings"
            }
            host.drain(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
            val p = point("phone-capy")
            event(MotionEvent.ACTION_DOWN, p)
            host.awaitFrame("Capy held before Escape") { host.workspaceInteraction?.phone?.held == true }
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            host.awaitFrame("Escape retires the held phone contact") { host.workspaceInteraction?.phone?.active == false && !phone().getBoolean("capy_drag") }
            instrumentation.runOnMainSync { activity.onWindowFocusChanged(false);activity.onWindowFocusChanged(true) }
            assertTrue("Focus retirement keeps the native pointer down",host.workspaceInteraction?.phone?.pointerDown == true)
            val retirementFrame = java.util.concurrent.CountDownLatch(1)
            instrumentation.runOnMainSync { activity.window.decorView.postOnAnimation { activity.window.decorView.post { retirementFrame.countDown() } } }
            assertTrue("The retired contact crosses a native frame",retirementFrame.await(5,java.util.concurrent.TimeUnit.SECONDS))
            assertTrue("The original pointer still owns click suppression",host.workspaceInteraction?.phone?.suppressed == true)
            event(MotionEvent.ACTION_UP, p)
            host.drain()
            assertFalse("Escaped pointer release cannot click Capy", phone().getBoolean("menu"))
            host.awaitFrame("Escaped gesture releases semantic activation") { host.workspaceInteraction?.phone?.suppressed == false }
            instrumentation.runOnMainSync {
                val node = checkNotNull(findTag("phone-tool-settings-hit")).second.find { it.config.getOrNull(SemanticsActions.OnClick) != null }
                assertTrue(checkNotNull(node).config[SemanticsActions.OnClick].action!!.invoke())
            }
            host.awaitFrame("Semantic activation after escaped Up") { host.snapshot!!.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer")?.getJSONObject("anchor")?.objectOrNull("anchor")?.optString("kind") == "tool_settings" }
            host.drain(obj("type" to "customize","action" to obj("type" to "close_expanded")))
        }
        state("inventory-retired-multipointer") {
            val p = point("phone-capy")
            val other = point("phone-tool-settings-hit")
            event(MotionEvent.ACTION_DOWN,p)
            host.awaitFrame("Capy contact begins before a second pointer") { host.workspaceInteraction?.phone?.capy == true }
            fun pointers(action: Int, ids: List<Int>) {
                val points = listOf(p,other)
                val input = MotionEvent.obtain(downAt,SystemClock.uptimeMillis(),action,ids.size,
                    ids.map { id -> MotionEvent.PointerProperties().apply { this.id = id;toolType = MotionEvent.TOOL_TYPE_FINGER } }.toTypedArray(),
                    ids.map { id -> MotionEvent.PointerCoords().apply { x=points[id].x;y=points[id].y;pressure=.7f } }.toTypedArray(),
                    0,0,1f,1f,0,0,android.view.InputDevice.SOURCE_TOUCHSCREEN,0)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(input,true)) } finally { input.recycle() }
            }
            var remaining = listOf(0,1)
            try {
                pointers(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),remaining)
                host.awaitFrame("Two pointers cancel the phone contact") { host.workspaceInteraction?.phone?.suppressed == true && host.workspaceInteraction?.phone?.capy == false }
                pointers(MotionEvent.ACTION_POINTER_UP,remaining);remaining = listOf(1)
                val frame = java.util.concurrent.CountDownLatch(1)
                instrumentation.runOnMainSync { activity.window.decorView.postOnAnimation { activity.window.decorView.post { frame.countDown() } } }
                assertTrue(frame.await(5,java.util.concurrent.TimeUnit.SECONDS))
                assertTrue("The remaining pointer keeps suppression",host.workspaceInteraction?.phone?.suppressed == true)
                pointers(MotionEvent.ACTION_UP,remaining);remaining = emptyList();pressedPoint = null
                host.awaitFrame("Final native Up releases suppression") { host.workspaceInteraction?.phone?.suppressed == false }
                host.drain()
                assertNull("The canceled second pointer cannot open Tool settings",host.snapshot!!.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer"))
                assertFalse(phone().getBoolean("menu"))
            } finally { if(remaining.isNotEmpty()) { pointers(MotionEvent.ACTION_CANCEL,remaining);pressedPoint = null } }
        }
        compose.waitForIdle()
        instrumentation.runOnMainSync {
            val dock = checkNotNull(host.workspaceInteraction)
            val tile = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").getJSONArray("phone").getJSONObject(0)
            dock.closeContext()
            dock.context(obj("kind" to "phone_tile", "tile" to tile.getInt("id")))
            host.retirePhoneContacts()
        }
        host.drain()
        instrumentation.runOnMainSync { assertNull("Retired context query cannot open a menu", host.workspaceInteraction?.contextMenu) }
        state("paint-default") {
            workspace(host,"illustrator")
            host.drain(obj("type" to "invoke", "command" to "actual_pixels"))
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())

    }

    private fun predictiveBack(activity: MainActivity, cancel: Boolean) {
        val dispatcher = activity.onBackPressedDispatcher
        instrumentation.runOnMainSync { dispatcher.dispatchOnBackStarted(androidx.activity.BackEventCompat(0f,100f,0f,androidx.activity.BackEventCompat.EDGE_LEFT)) }
        compose.waitForIdle()
        instrumentation.runOnMainSync { dispatcher.dispatchOnBackProgressed(androidx.activity.BackEventCompat(80f,100f,.6f,androidx.activity.BackEventCompat.EDGE_LEFT)) }
        compose.waitForIdle()
        instrumentation.runOnMainSync { if (cancel) dispatcher.dispatchOnBackCancelled() else dispatcher.onBackPressed() }
        compose.waitForIdle()
    }

    private fun workspace(host: CanvasHost, preset: String) {
        val id = "builtin:workspace:$preset"
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to id)) }
        host.awaitFrame("Workspace $id") { host.workspaceManager?.optString("id") == id && host.workspaceManager?.optBoolean("busy") == false }
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to id))) }
        host.awaitFrame("Starting layout confirmation") { host.workspaceManager?.objectOrNull("prompt") != null }
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "submit", "name" to "")) }
        host.awaitFrame("Starting layout restored") { host.workspaceManager?.objectOrNull("prompt") == null && host.workspaceManager?.optBoolean("busy") == false }
        compose.waitForIdle()
        assertNull("Starting layout confirmation is no longer presented",findTag("workspace-form"))
    }

    private fun visitDrawers(activity: MainActivity, host: CanvasHost, event: (Int,Offset) -> Unit, tap: (String) -> Unit, state: (String,() -> Unit) -> Unit) {
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun drawer() = shared().getJSONObject("customization").objectOrNull("drawer")
        fun close(nativeKey: Boolean = false) {
            if (nativeKey) { pressKey(KeyEvent.KEYCODE_ESCAPE); host.drain() }
            else host.drain(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
            host.awaitFrame("Drawer closed") { drawer() == null && findTag("tool-drawer") == null }
        }
        fun ready() {
            host.awaitFrame("Phone drawer allocated") { drawer() != null && findTag("tool-drawer")?.second?.boundsInRoot?.height?.let { it > 0f } == true &&
                host.workspaceInteraction?.drawer?.getJSONObject("placement")?.getJSONObject("bounds")?.number("height")?.let { it > 0f } == true }
            compose.waitForIdle()
            val root = checkNotNull(findTag("workspace")).second.boundsInRoot
            val body = checkNotNull(findTag("tool-drawer")).second.boundsInRoot
            assertTrue("Phone drawer remains inside the workspace: $body within $root",body.left >= root.left-1 && body.top >= root.top-1 && body.right <= root.right+1 && body.bottom <= root.bottom+1)
            assertNotNull("Phone drawer has a bridge",host.workspaceInteraction?.drawer?.objectOrNull("connection"))
        }
        fun tool(control: JSONObject): String {
            val tiles = shared().getJSONObject("workspace").getJSONObject("layout").array("phone").objects()
            return "phone-tool-${tiles.first { it.getJSONObject("control").toString() == control.toString() }.getInt("id")}"
        }
        fun open(control: JSONObject) {
            val tag = tool(control)
            tap(tag)
            if (drawer() == null) tap(tag)
            ready()
            val panels = drawer()!!.array("columns").values().flatMap { (it as JSONArray).values() }
            if (panels.size > 1) assertFalse("Mixed tool drawers omit their settings column",panels.contains("tool_settings"))
        }
        workspace(host,"painter")
        host.drain(obj("type" to "invoke","command" to "fit_canvas"))
        for (swapped in listOf(false,true)) {
            if (shared().getJSONObject("phone").getBoolean("swapped") != swapped) host.drain(obj("type" to "phone","action" to obj("kind" to "swap_sides")))
            for ((name,control) in listOf("brush" to obj("kind" to "command","command" to "drawing_brush"),"color" to obj("kind" to "color"),"layers" to obj("kind" to "panel","panel" to "layers"))) {
                open(control)
                state("$name-${if(swapped) "swapped" else "normal"}") {
                    if (name == "color") {
                        val wheel = checkNotNull(findTag("color-panel")).second.boundsInRoot
                        assertTrue("Shared wheel floor is retained",wheel.width >= 127f)
                        val root = checkNotNull(findTag("tool-drawer")).second
                        assertNotNull("Phone Color owns the shared scroller",root.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange))
                        assertTrue("Palette follows the wheel with a real allocation",checkNotNull(findTag("palette-swatches")).second.size.height > 1)
                        assertTrue("Palette footer has a natural allocation",checkNotNull(findTag("palette-footer")).second.size.height > 1)
                        compose.onNodeWithTag("palette-footer",useUnmergedTree=true).performScrollTo()
                        assertTrue("Color shares enough scroll extent to reach the palette footer",checkNotNull(findTag("palette-footer")).second.boundsInRoot.height > 1f)
                        compose.onNodeWithTag("color-panel",useUnmergedTree=true).performScrollTo()
                    }
                }
                close(nativeKey = true)
            }
        }
        if (shared().getJSONObject("phone").getBoolean("swapped")) host.drain(obj("type" to "phone","action" to obj("kind" to "swap_sides")))
        for ((preset,name,control) in listOf(
            Triple("painter","sketch-brush",obj("kind" to "command","command" to "drawing_brush")),
            Triple("painter","sketch-sculpt",obj("kind" to "command","command" to "sculpt")),
            Triple("illustrator","paint-brush",obj("kind" to "command","command" to "drawing_brush")),
            Triple("illustrator","paint-sculpt",obj("kind" to "command","command" to "sculpt")),
            Triple("illustrator","paint-fill",obj("kind" to "tool_slot","slot" to "fill")),
            Triple("illustrator","paint-gradient",obj("kind" to "tool_slot","slot" to "gradient")),
            Triple("photographer","photo-healing",obj("kind" to "tool_slot","slot" to "healing")),
            Triple("photographer","photo-eraser",obj("kind" to "command","command" to "eraser")))) {
            workspace(host,preset)
            open(control)
            state(name) {
                val view = drawer()!!.objectOrNull("tool_set")
                val preview = view?.array("subtools")?.objects()?.firstOrNull { !it.isNull("preview") }
                preview?.let {
                    val row = checkNotNull(findTag("subtool-${it.getString("label")}")).second
                    assertTrue("Preview rows honor the shared natural height",row.size.height >= 66*instrumentation.targetContext.resources.displayMetrics.density-1)
                }
                if (name.endsWith("brush") && host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getString("edge") == "top" &&
                    host.snapshot!!.getJSONObject("layout").getJSONArray("viewport").getDouble(0) <= 320) {
                    val pen = checkNotNull(findTag("tool-group-Pen")).second.boundsInRoot
                    val pencil = checkNotNull(findTag("tool-group-Pencil")).second.boundsInRoot
                    val pastel = checkNotNull(findTag("tool-group-Pastel")).second.boundsInRoot
                    assertEquals("First grid row shares one top",pen.top,pencil.top,1f)
                    assertTrue("Shared columns wrap Pastel under Pen",pastel.top > pen.top)
                    assertEquals("Wrapped chips retain their column edge",pen.left,pastel.left,1f)
                }
            }
            val view = drawer()?.objectOrNull("tool_set")
            if (name.endsWith("brush") || name.endsWith("sculpt")) {
                val alternative = view?.array("groups")?.objects()?.firstOrNull { !it.getBoolean("selected") && it.getBoolean("enabled") }
                state("$name-other-set") {
                    val row = checkNotNull(alternative)
                    val firstPanel = drawer()!!.array("columns").getJSONArray(0).getString(0)
                    val projection = runBlocking { host.awaitQuery(obj("type" to "tool_panel","panel" to firstPanel,"anchor" to drawer()!!.getJSONObject("anchor"))) }!!
                    val tag = if (projection.getBoolean("groups_are_rows")) "$firstPanel-${row.getString("label")}" else "tool-group-${row.getString("label")}"
                    compose.onNodeWithTag(tag,useUnmergedTree=true).performScrollTo();tap(tag)
                    host.awaitFrame("Choosing another set replaces the drawer list") { drawer()?.objectOrNull("tool_set")?.array("subtools")?.toString() != view.array("subtools").toString() }
                    ready()
                }
            }
            close()
        }
        workspace(host,"painter")
        open(obj("kind" to "panel","panel" to "layers"))
        val initialRows = shared().array("layers").length()
        val initialHeight = checkNotNull(findTag("layer-inspector")).second.size.height
        var previousHeight = initialHeight
        for (count in listOf(5,12,24)) {
            while (shared().array("layers").length() < count) host.drain(obj("type" to "layer","action" to obj("op" to "new","group" to false,"clipped" to false)))
            ready()
            val height = checkNotNull(findTag("layer-inspector")).second.size.height
            assertTrue("Open Layers follows its live row count",height >= previousHeight)
            previousHeight = height
        }
        val layer = shared().array("layers").objects().first { it.getBoolean("selected") }.getLong("id")
        val row = checkNotNull(findTag("layer-row-$layer")).second.boundsInRoot
        val from = row.center
        event(MotionEvent.ACTION_DOWN,from)
        event(MotionEvent.ACTION_MOVE,from+Offset(row.width*.45f,0f))
        event(MotionEvent.ACTION_CANCEL,from+Offset(row.width*.45f,0f))
        assertEquals("Cancelled layer swipe preserves its rows",24,shared().array("layers").length())
        tap("layer-properties-disclosure")
        host.awaitFrame("Layers properties page") { drawer()?.optBoolean("layer_properties") == true }
        ready()
        predictiveBack(activity,cancel = true)
        assertTrue("Cancelled predictive Back preserves Properties",drawer()?.optBoolean("layer_properties") == true)
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Predictive Back returns Layers to its list") { drawer()?.optBoolean("layer_properties") == false }
        repeat(24-initialRows) { host.drain(obj("type" to "invoke","command" to "undo")) }
        ready()
        host.awaitFrame("Removing layers restores the live drawer height") { kotlin.math.abs(checkNotNull(findTag("layer-inspector")).second.size.height-initialHeight) <= 1 }
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Predictive Back closes Layers after its list") { drawer() == null && findTag("tool-drawer") == null }
        for (command in listOf("drawing_brush","sculpt","fill","gradient","select","crop","eyedropper","eraser","hand")) {
            host.drain(obj("type" to "invoke","command" to command));tap("phone-tool-settings");ready()
            state("inventory-settings-$command") {
                assertEquals("Settings tile opens settings only",listOf("tool_settings"),drawer()!!.array("columns").values().flatMap { (it as JSONArray).values() })
            }
            close()
        }
        for (panel in listOf("navigator","histogram","waveform","stats","sizes","brushes","adjustments","filter_types","proof")) {
            host.drain(obj("type" to "phone","action" to obj("kind" to "panel","panel" to panel)));ready()
            state("inventory-window-$panel") {}
            close()
        }
        host.drain(obj("type" to "phone","action" to obj("kind" to "panel","panel" to "brushes")));ready()
        for (command in listOf("drawing_brush","sculpt")) {
            host.drain(obj("type" to "invoke","command" to command))
            val label = shared().getJSONObject("tool_set").array("subtools").getJSONObject(0).getString("label")
            host.awaitFrame("Window Brushes observes the shared fallback tool set") { findTag("subtool-$label") != null }
        }
        close()
        val nativeView = checkNotNull(findTag("workspace")).first.view
        val direction = nativeView.layoutDirection
        try {
            instrumentation.runOnMainSync { nativeView.layoutDirection = android.view.View.LAYOUT_DIRECTION_RTL }
            compose.waitForIdle()
            workspace(host,"painter")
            open(obj("kind" to "command","command" to "drawing_brush"))
            assertEquals(androidx.compose.ui.unit.LayoutDirection.Rtl,checkNotNull(findTag("tool-drawer")).second.layoutInfo.layoutDirection)
            val frame = host.workspaceInteraction!!.drawer!!.getJSONObject("placement")
            val density = activity.resources.displayMetrics.density
            val origin = checkNotNull(findTag("tool-drawer")).second.boundsInRoot.left
            drawer()!!.array("columns").values().forEachIndexed { index,column ->
                val x = origin+frame.array("columns").getJSONObject(index).number("x")*density
                (column as JSONArray).values().forEach { panel ->
                    assertEquals("RTL drawer columns keep physical x coordinates",x,checkNotNull(findTag("drawer-panel-$panel")).second.boundsInRoot.left,1f)
                }
            }
            state("inventory-rtl-columns") {}
            close()
        } finally {
            instrumentation.runOnMainSync { nativeView.layoutDirection = direction }
            compose.waitForIdle()
        }
    }

    private fun visitPagesD2(activity: MainActivity,host: CanvasHost,event: (Int,Offset) -> Unit,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit) {
        val decor = activity.window.decorView
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun invoke(command: String) = host.drain(obj("type" to "invoke","command" to command))
        fun exists(tag: String) = findTag(tag) != null
        fun wait(tag: String) = host.awaitFrame("Pages D2 $tag is composed") { exists(tag) }
        fun gone(tag: String) = host.awaitFrame("Pages D2 $tag closes") { !exists(tag) }
        fun keyboard() = decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        fun hideIme() {
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Keyboard-free page reference") { !keyboard() }
            compose.waitForIdle()
        }
        fun settle() { compose.waitForIdle();SystemClock.sleep(400);if(keyboard())hideIme() }
        fun customize(action: JSONObject) = host.drain(obj("type" to "customize","action" to action))
        fun manager(input: JSONObject) {
            instrumentation.runOnMainSync { host.workspaceInput(input) }
            host.awaitFrame("Workspace manager settles") { host.workspaceManager?.optBoolean("busy") == false && host.workspaceManager?.optBoolean("saving") != true }
        }
        fun close(tag: String) { if(exists(tag)) { try { tap(tag) } catch(error: Throwable) { predictiveBack(activity,cancel = false) };compose.waitForIdle() } }
        var keyboardHeight = 0
        fun measureKeyboard() {
            if(keyboardHeight > 0) return
            invoke("search_commands")
            host.awaitFrame("Command search measures the native keyboard") { findTag("command-search") != null && keyboard() }
            keyboardHeight = decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom
            if(keyboard())hideIme()
            close("command-close")
            host.awaitFrame("Search closes") { host.commandSearch == null && findTag("command-bar") == null }
        }
        fun withKeyboard(tag: String) {
            measureKeyboard()
            val view = checkNotNull(findTag("workspace")).first.view
            val original = decor.rootWindowInsets
            instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(original)
                .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.of(0,0,0,keyboardHeight)).setVisible(WindowInsets.Type.ime(),true).build()) }
            host.awaitFrame("$tag follows the reduced visible area") { findTag(tag)?.second?.boundsInRoot?.let { it.bottom <= decor.height-keyboardHeight+1 } == true }
            compose.waitForIdle()
        }
        fun restoreKeyboard() {
            val view = findTag("workspace")?.first?.view ?: return
            val original = decor.rootWindowInsets
            instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(original)
                .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.NONE).setVisible(WindowInsets.Type.ime(),false).build()) }
            compose.waitForIdle()
        }
        for ((command,tag) in listOf("canvas_size" to "canvas-size","image_size" to "image-size")) {
            state(tag) { invoke(command);wait("$tag-panel");settle() }
            close("$tag-cancel");gone("$tag-panel")
        }
        state("properties") { invoke("document_properties");wait("document-properties");settle() }
        close("document-properties-done");gone("document-properties")
        for ((command,name) in listOf("assign_profile" to "assign-profile","change_bit_depth" to "change-bit-depth","convert_color_space" to "convert-color-space")) {
            state(name) { invoke(command);wait("document-color");SystemClock.sleep(1500);settle() }
            close("document-color-cancel");gone("document-color")
        }
        state("brush-control") { customize(obj("type" to "open_control","control" to "brush_color"));wait("brush-control");settle() }
        close("brush-control-done");gone("brush-control")
        state("cannot-open-file") {
            val previous = DocumentController.nativeFileJobsForTest
            DocumentController.nativeFileJobsForTest = true
            try {
                invoke("open_document")
                host.awaitFrame("Open requests a document") { shared().array("requests").objects().any { it.getJSONObject("kind").optString("type") == "document" } }
                val id = shared().array("requests").objects().first { it.getJSONObject("kind").optString("type") == "document" }.getInt("id")
                runBlocking { host.withNative { Native.documentComplete(it,id,false,JSONObject.quote("Could not open “broken.png”.")) } }
                host.drain()
                wait("file-error-ok");settle()
                val alert=checkNotNull(findTag("file-error")).second.boundsInRoot
                val body=checkNotNull(findNode(hasLabel("Could not open “broken.png”."))).second.boundsInRoot
                assertEquals("The alert body is centred",alert.center.x,body.center.x,1f)
                assertNotNull(findNode(hasLabel(host.bootstrap!!.getString("cannot_open_file"))))
            } finally { DocumentController.nativeFileJobsForTest = previous }
        }
        close("file-error-ok");gone("file-error")
        state("profile-library") {
            invoke("settings");wait("settings-search")
            host.drain(obj("type" to "preferences","action" to obj("type" to "page","page" to "color")))
            compose.onNodeWithTag("settings-manage-profiles",useUnmergedTree = true).performScrollTo();tap("settings-manage-profiles")
            wait("profile-library");settle()
        }
        close("profile-library-done")
        if(host.snapshot?.objectOrNull("preferences") != null)host.drain(obj("type" to "close_settings"))
        host.awaitFrame("Preferences closes") { host.snapshot?.objectOrNull("preferences") == null && !exists("profile-library") }
        val palette = host.palettes.view()?.array("palettes")?.objects()?.firstOrNull()?.getLong("id")
        for ((command,name,tag) in listOf(Triple("rename_palette","palette-name","palette-name-dialog"),Triple("remove_palette","palette-delete","palette-remove-dialog"))) {
            state(name) {
                instrumentation.runOnMainSync { host.palettes.command(obj("command" to command,"id" to checkNotNull(palette) { "No palette" })) }
                wait(tag);settle()
            }
            instrumentation.runOnMainSync { host.palettes.dialog = null };gone(tag)
        }
        state("workspace-manager") { manager(obj("type" to "open","page" to "workspaces"));wait("workspace-manager");settle() }
        state("workspace-new") {
            if(!exists("workspace-manager")) { manager(obj("type" to "open","page" to "workspaces"));wait("workspace-manager") }
            manager(obj("type" to "form","action" to obj("type" to "new")));wait("workspace-form");settle()
        }
        if(exists("workspace-form")) {
            manager(obj("type" to "submit","name" to "Canvas study"))
            host.awaitFrame("New workspace is stored") { host.workspaceManager?.objectOrNull("prompt_action") == null }
        }
        state("workspace-delete") {
            if(!exists("workspace-manager")) { manager(obj("type" to "open","page" to "workspaces"));wait("workspace-manager") }
            val row = host.workspaceManager!!.array("rows").objects().first { it.optString("title") == "Canvas study" }.getString("id")
            manager(obj("type" to "select","id" to row))
            manager(obj("type" to "form","action" to obj("type" to "delete","value" to row)));wait("workspace-form");settle()
        }
        if(exists("workspace-form"))manager(obj("type" to "cancel"))
        manager(obj("type" to "cancel"));manager(obj("type" to "dismiss"))
        host.awaitFrame("Workspace manager closes") { !exists("workspace-manager") && !exists("workspace-form") }
        if(host.workspaceManager?.optString("id") != "builtin:workspace:painter")workspace(host,"painter")
        state("toolbar-manager") { customize(obj("type" to "manage_toolbars"));wait("toolbar-manager");settle() }
        if(host.snapshot?.objectOrNull("toolbar_manager") == null)customize(obj("type" to "manage_toolbars"))
        host.awaitFrame("Toolbar manager view") { host.snapshot?.objectOrNull("toolbar_manager") != null }
        val toolbar = host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").objects().firstOrNull()?.get("panel")
        customize(obj("type" to "close_toolbar_manager"));gone("toolbar-manager")
        for (kind in listOf("rename","duplicate","delete")) {
            state("$kind-toolbar") {
                customize(obj("type" to "${kind}_toolbar","panel" to checkNotNull(toolbar) { "No toolbar" }));wait("toolbar-prompt");settle()
                if(kind=="delete") {
                    val cancel=checkNotNull(findTag("toolbar-prompt-cancel")).second.boundsInRoot
                    val confirm=checkNotNull(findTag("toolbar-prompt-confirm")).second.boundsInRoot
                    if(kotlin.math.abs(cancel.top-confirm.top)>1f)assertTrue("Stacked alerts put the last response first",confirm.bottom<=cancel.top)
                }
            }
            if(host.snapshot?.objectOrNull("toolbar_prompt") != null)customize(obj("type" to "cancel_toolbar"))
            gone("toolbar-prompt")
        }
        invoke("select_all")
        state("selection-refine") { invoke("feather_selection");wait("selection-refine-panel");awaitCanvasPresentation(host);settle() }
        state("selection-refine-ime") {
            if(!exists("selection-refine-panel")) { invoke("feather_selection");wait("selection-refine-panel") }
            withKeyboard("selection-refine-panel")
        }
        restoreKeyboard()
        close("selection-refine-panel-cancel");gone("selection-refine-panel")
        state("frequency-separation") { invoke("frequency_separation");wait("frequency-separation-panel");awaitCanvasPresentation(host);settle() }
        state("frequency-separation-ime") {
            if(!exists("frequency-separation-panel")) { invoke("frequency_separation");wait("frequency-separation-panel") }
            withKeyboard("frequency-separation-panel")
        }
        restoreKeyboard()
        close("frequency-separation-panel-cancel");gone("frequency-separation-panel")
        if(shared().array("commands").objects().any { it.optString("id") == "deselect" && it.optBoolean("enabled") })invoke("deselect")
        var database: android.database.sqlite.SQLiteDatabase? = null
        try {
            state("workspace-save-status") {
                val path = AppStorage.of(instrumentation.targetContext).workspaces.resolve("workspaces.sqlite3")
                assertTrue("The injected failure belongs to this private test",path.canonicalPath.startsWith(device.root.canonicalPath+File.separator))
                database = android.database.sqlite.SQLiteDatabase.openDatabase(path.absolutePath,null,android.database.sqlite.SQLiteDatabase.OPEN_READWRITE or android.database.sqlite.SQLiteDatabase.NO_LOCALIZED_COLLATORS)
                database!!.execSQL("CREATE TRIGGER d2_fail_editing BEFORE INSERT ON editing WHEN NEW.id=1 BEGIN SELECT RAISE(ABORT,'owned workspace write failure'); END;")
                host.drain(obj("type" to "set_brush_size","value" to 83))
                host.awaitFrame("Owned workspace write failure is presented") { host.workspaceManager?.objectOrNull("status")?.optBoolean("error") == true && exists("workspace-status") }
                settle()
            }
        } finally {
            database?.let { try { it.execSQL("DROP TRIGGER IF EXISTS d2_fail_editing") } finally { it.close() } }
        }
        if(host.workspaceManager?.objectOrNull("status")?.optBoolean("error") == true) {
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "retry")) }
            host.awaitFrame("The owned failure recovers after Retry") { host.workspaceManager?.objectOrNull("status")?.optBoolean("error") != true }
        }
        state("proof-profile-picker") {
            host.drain(obj("type" to "phone","action" to obj("kind" to "panel","panel" to "proof")))
            instrumentation.runOnMainSync { host.proof.action(obj("type" to "mode","mode" to "print"));host.proof.action(obj("type" to "reveal")) }
            host.awaitFrame("Proof profile control",30_000) { exists("proof-profile") }
            compose.onNodeWithTag("proof-profile",useUnmergedTree = true).performScrollTo();tap("proof-profile")
            host.awaitFrame("Proof profile picker",30_000) { exists("proof-profile-picker") };settle()
        }
    }

    private fun visitDialogsD2(activity:MainActivity,host:CanvasHost,tap:(String)->Unit) {
        fun wait(tag:String)=host.awaitFrame("D2 $tag is composed"){findTag(tag)!=null}
        fun gone(tag:String)=host.awaitFrame("D2 $tag closes"){findTag(tag)==null}
        fun invoke(command:String)=host.drain(obj("type" to "invoke","command" to command))
        val file=host.snapshot!!.getJSONObject("state").getJSONObject("document_file")
        val epoch=file.getLong("epoch")
        val revision=file.getLong("revision")
        for((command,tag)in listOf("canvas_size" to "canvas-size-panel","image_size" to "image-size-panel")) {
            invoke(command);wait(tag);awaitEditorPage(activity,host,tag)
            tap(tag.removeSuffix("-panel")+"-cancel");gone(tag)
        }
        invoke("document_properties");wait("document-properties");awaitEditorPage(activity,host,"document-properties")
        tap("document-properties-done");gone("document-properties")
        invoke("assign_profile");wait("document-color");awaitEditorPage(activity,host,"document-color")
        tap("document-color-cancel");gone("document-color")
        host.drain(obj("type" to "customize","action" to obj("type" to "open_control","control" to "brush_color")))
        wait("brush-control");awaitEditorPage(activity,host,"brush-control")
        tap("brush-control-done");gone("brush-control")
        instrumentation.runOnMainSync {host.reportActionError("D2 owned error")}
        compose.waitUntil(10_000){findTag("action-error-ok")!=null}
        compose.onNodeWithTag("action-error-ok").performSemanticsAction(SemanticsActions.OnClick){assertTrue(it())}
        gone("action-error")
        val after=host.snapshot!!.getJSONObject("state").getJSONObject("document_file")
        assertEquals("Dialog cancellation keeps the drawing",epoch,after.getLong("epoch"))
        assertEquals("Dialog cancellation keeps authored history",revision,after.getLong("revision"))
    }

    private fun visitDialogsD1(host:CanvasHost,tap:(String)->Unit) {
        fun wait(tag:String)=host.awaitFrame("D1 $tag is composed"){findTag(tag)!=null}
        fun gone(tag:String)=host.awaitFrame("D1 $tag closes"){findTag(tag)==null}
        host.drain(obj("type" to "invoke","command" to "settings"))
        wait("settings-search")
        tap("settings-category-canvas")
        host.awaitFrame("Preferences uses the shared page route") {host.snapshot?.objectOrNull("preferences")?.getString("page")=="canvas" && findTag("settings-main-pane")!=null}
        tap("settings-done");gone("preferences-dialog")
        host.drain(obj("type" to "invoke","command" to "new_document"))
        wait("new-drawing-create")
        compose.onNodeWithTag("new-drawing-width").performScrollTo()
        val width=checkNotNull(findTag("new-drawing-width")).second.boundsInRoot
        val unit=checkNotNull(findTag("new-drawing-unit")).second.boundsInRoot
        val metrics=instrumentation.targetContext.resources.displayMetrics
        when((metrics.widthPixels/metrics.density).roundToInt()) {
            412->assertEquals("Dimensions stay on one row when their natural widths fit",width.center.y,unit.center.y,1f)
            280->assertTrue("Narrow dimensions wrap the unit below the values",unit.top>=width.bottom)
        }
        compose.onNodeWithTag("new-drawing-name").performScrollTo()
        instrumentation.runOnMainSync {
            val entry=checkNotNull(findTag("new-drawing-name")).second.find {it.config.getOrNull(SemanticsActions.SetText)!=null}
            assertTrue(checkNotNull(entry).config[SemanticsActions.SetText].action!!.invoke(androidx.compose.ui.text.AnnotatedString("D1 draft")))
        }
        host.awaitFrame("New drawing keeps its native draft") {host.snapshot?.getJSONObject("state")?.objectOrNull("new_drawing")?.getString("name")=="D1 draft"}
        compose.onNodeWithTag("new-drawing-preview").performScrollTo();tap("new-drawing-preview")
        wait("new-drawing-save-as")
        compose.onNodeWithTag("new-drawing-save-as").performScrollTo();tap("new-drawing-save-as")
        wait("new-drawing-save-name")
        tap("new-drawing-cancel")
        host.awaitFrame("Back keeps the preset browser") {host.snapshot?.getJSONObject("state")?.objectOrNull("new_drawing")?.objectOrNull("drawer")?.isNull("save")==true}
        tap("new-drawing-cancel")
        host.awaitFrame("Back restores the New drawing draft") {
            host.snapshot?.getJSONObject("state")?.objectOrNull("new_drawing")?.let {it.isNull("drawer") && it.getString("name")=="D1 draft"}==true
        }
        tap("new-drawing-cancel");gone("new-drawing-dialog")
        host.drain(obj("type" to "invoke","command" to "export_document"))
        wait("export-choose-file")
        tap("export-size");wait("export-fit");tap("export-back")
        wait("export-presets");tap("export-presets")
        wait("export-preset-save");tap("export-preset-save")
        wait("export-preset-name")
        tap("export-dialog-cancel")
        wait("export-preset-save");tap("export-back")
        wait("export-size");tap("export-back");gone("export-dialog")
        instrumentation.runOnMainSync {host.colorEditor=ColorEditorRequest("foreground",null,false,false){_,_->}}
        wait("color-use")
        val hex=checkNotNull(findTag("color-value-hex")).second.config[SemanticsProperties.ContentDescription]
        compose.onNodeWithTag("color-swatches").performScrollTo();tap("color-swatches")
        wait("color-sheet-search");tap("color-cancel")
        wait("color-value-hex")
        compose.onNodeWithTag("color-pick").performScrollTo();tap("color-pick")
        wait("color-strip")
        assertNull("Picking releases the page key owner",host.dialogKeyHandler)
        assertFalse("Picking releases page modality",host.workspaceInteraction!!.popupOpen)
        tap("color-strip");wait("color-value-hex")
        assertEquals("Picking keeps the editor draft",hex,checkNotNull(findTag("color-value-hex")).second.config[SemanticsProperties.ContentDescription])
        assertNotNull("Picking restores the page key owner",host.dialogKeyHandler)
        tap("color-cancel");gone("color-editor")
    }

    private fun visitPagesD1(activity: MainActivity,host: CanvasHost,event: (Int,Offset) -> Unit,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit) {
        val decor = activity.window.decorView
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun preferences() = host.snapshot?.objectOrNull("preferences")
        fun preference(action: JSONObject) = host.drain(obj("type" to "preferences","action" to action))
        fun invoke(command: String) = host.drain(obj("type" to "invoke","command" to command))
        fun placed(tag: String) = findTag(tag)?.second?.let { it.layoutInfo.isPlaced && it.boundsInRoot.width > 0f && it.boundsInRoot.height > 0f } == true
        fun wait(tag: String) = host.awaitFrame("Pages D1 $tag is placed") { placed(tag) }
        fun gone(tag: String) = host.awaitFrame("Pages D1 $tag closes") { findTag(tag) == null }
        fun keyboard() = decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        fun hideIme() {
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Keyboard-free page reference") { !keyboard() }
            compose.waitForIdle()
        }
        fun reveal(tag: String) { compose.onNodeWithTag(tag,useUnmergedTree = true).performScrollTo();compose.waitForIdle() }
        fun type(tag: String,text: String,done: () -> Boolean) {
            reveal(tag);tap(tag)
            host.awaitFrame("$tag owns native focus") { findTag(tag)?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null }
            pressKey(KeyEvent.KEYCODE_A,KeyEvent.META_CTRL_ON);compose.waitForIdle()
            instrumentation.sendStringSync(text)
            host.awaitFrame("$tag query reaches the session",diagnostics = { "${preferences()}" }) { done() }
            hideIme()
        }
        fun hold(tag: String) {
            reveal(tag)
            val center = checkNotNull(findTag(tag)).let { (root,node) ->
                val screen = IntArray(2);instrumentation.runOnMainSync { root.view.getLocationOnScreen(screen) }
                node.boundsInRoot.center + Offset(screen[0].toFloat(),screen[1].toFloat())
            }
            event(MotionEvent.ACTION_DOWN,center);SystemClock.sleep(900);event(MotionEvent.ACTION_UP,center);host.drain()
        }
        fun closeAll() {
            if(keyboard())hideIme()
            if(preferences() != null) {
                preferences()?.objectOrNull("capture")?.let { preference(obj("type" to "cancel_shortcut")) }
                host.drain(obj("type" to "close_settings"))
            }
            if(shared().objectOrNull("new_drawing") != null)host.drain(obj("type" to "new_drawing","action" to obj("op" to "cancel")))
            instrumentation.runOnMainSync { host.colorEditor = null;host.textureDialog = null }
            if(findTag("export-preset-cancel") != null) { tap("export-preset-cancel");compose.waitForIdle() }
            repeat(3) { if(findTag("export-back") != null) { tap("export-back");compose.waitForIdle() } }
            host.awaitFrame("Pages D1 surfaces close") { preferences() == null && shared().objectOrNull("new_drawing") == null &&
                findTag("color-editor") == null && findTag("texture-dialog") == null && findTag("export-dialog") == null }
            compose.waitForIdle()
        }
        fun preferencesRoot() {
            if(preferences() == null) { invoke("settings");wait("settings-search") }
            if(preferences()?.optBoolean("page_requested") == true)preference(obj("type" to "page_list"))
            host.awaitFrame("Preferences root list") { placed("settings-category-appearance") && findTag("settings-main-pane") == null }
        }
        fun page(id: String) {
            preferencesRoot()
            reveal("settings-category-$id");tap("settings-category-$id")
            host.awaitFrame("Preferences opens $id") { preferences()?.getString("page") == id && placed("settings-main-pane") }
        }

        state("preferences-root") {
            tap("phone-capy");wait("phone-preferences");tap("phone-preferences")
            wait("settings-search");preferencesRoot()
        }
        state("preferences-query") {
            preferencesRoot()
            type("settings-search","color") { preferences()?.optString("query") == "color" }
        }
        state("preferences-empty") {
            preference(obj("type" to "search","query" to ""))
            type("settings-search","zzznomatchzzz") { preferences()?.optString("query") == "zzznomatchzzz" }
        }
        if(preferences() != null) {
            preference(obj("type" to "search","query" to ""));preference(obj("type" to "toggle_search","open" to false))
            if(keyboard())hideIme()
        }
        for ((id,rows) in listOf("appearance" to listOf("language","theme"),"canvas" to listOf("wheel_behavior"),"color" to listOf("missing_profile","photo_depth"),
            "input" to listOf("eraser_tool","finger_drawing"),"about" to emptyList())) {
            state("preferences-$id") { page(id) }
            for (row in rows) state("preferences-$id-${row.replace('_','-')}") {
                if(preferences()?.getString("page") != id || findTag("settings-main-pane") == null)page(id)
                reveal("preference-$row")
            }
            if(id == "appearance")state("preferences-language-popup") {
                if(preferences()?.getString("page") != id || findTag("settings-main-pane") == null)page(id)
                reveal("setting-choice-language");tap("setting-choice-language");wait("setting-choice-menu-language")
            }
            if(findTag("setting-choice-menu-language") != null) { pressKey(KeyEvent.KEYCODE_ESCAPE);gone("setting-choice-menu-language") }
            if(id == "about") {
                state("preferences-about-website") {
                    if(preferences()?.getString("page") != id || findTag("settings-main-pane") == null)page(id)
                    reveal("preference-website")
                }
                state("preferences-about-values") {
                    if(preferences()?.getString("page") != id || findTag("settings-main-pane") == null)page(id)
                    val rows = preferences()!!.array("pages").objects().first { it.getString("id") == "about" }.array("groups").objects().flatMap { it.array("rows").objects() }
                    reveal("preference-"+rows.last().getString("id"))
                }
            }
        }
        state("preferences-shortcuts") { page("shortcuts") }
        state("shortcuts-query") {
            if(preferences()?.getString("page") != "shortcuts")page("shortcuts")
            type("shortcuts-search","Undo") { preferences()?.optString("shortcut_query") == "Undo" }
        }
        state("shortcuts-empty") {
            preference(obj("type" to "search_shortcuts","query" to ""))
            type("shortcuts-search","zzznomatchzzz") { preferences()?.optString("shortcut_query") == "zzznomatchzzz" }
        }
        if(preferences() != null) { preference(obj("type" to "search_shortcuts","query" to ""));if(keyboard())hideIme() }
        state("preferences-shortcuts-category") {
            if(preferences()?.getString("page") != "shortcuts")page("shortcuts")
            val first = preferences()!!.getJSONObject("shortcut_page").array("categories").objects().first().getString("id")
            reveal("shortcut-category-$first");tap("shortcut-category-$first")
            host.awaitFrame("The first Shortcuts category opens") { preferences()?.getJSONObject("shortcut_page")?.isNull("category") == false }
        }
        if(preferences()?.objectOrNull("shortcut_page")?.isNull("category") == false)preference(obj("type" to "shortcut_category","id" to JSONObject.NULL))
        state("preferences-shortcuts-bottom") {
            if(preferences()?.getString("page") != "shortcuts")page("shortcuts")
            host.awaitFrame("Shortcuts home") { findTag("shortcut-categories") != null }
            reveal("shortcut-category-"+preferences()!!.getJSONObject("shortcut_page").array("categories").objects().last().getString("id"))
        }
        closeAll()
        state("preferences-shortcuts-direct") {
            invoke("keyboard_shortcuts")
            host.awaitFrame("Direct Keyboard Shortcuts opens its page") { preferences()?.getString("page") == "shortcuts" && placed("settings-main-pane") }
        }
        if(preferences() == null)invoke("keyboard_shortcuts")
        state("choose-action") {
            preference(obj("type" to "open_action_picker","trigger" to "touch.tap.2"));wait("action-picker")
        }
        if(findTag("action-picker") != null) { preference(obj("type" to "close_action_picker"));gone("action-picker") }
        state("shortcut-editor") {
            preference(obj("type" to "edit_shortcut","id" to "command.Redo"));wait("settings-content-shortcut:command.Redo")
        }
        if(preferences()?.objectOrNull("shortcut_editor") != null) { preference(obj("type" to "close_shortcut_editor"));gone("settings-content-shortcut:command.Redo") }
        state("modifier-key") {
            preference(obj("type" to "add_modifier_key"));wait("modifier-key")
        }
        if(preferences()?.objectOrNull("capture") != null) { preference(obj("type" to "cancel_shortcut"));gone("modifier-key") }
        state("shortcut-recording") {
            preference(obj("type" to "edit_shortcut","id" to "command.Hand"));wait("settings-content-shortcut:command.Hand")
            reveal("add-shortcut");tap("add-shortcut")
            host.awaitFrame("Hand records a shortcut") { preferences()?.objectOrNull("capture") != null && placed("shortcut-recording") }
        }
        state("shortcut-conflict") {
            pressKey(KeyEvent.KEYCODE_E)
            host.awaitFrame("The Eraser chord conflicts with Hand",diagnostics = { "${preferences()?.opt("capture")}" }) { preferences()?.objectOrNull("capture")?.isNull("conflict") == false }
        }
        if(preferences()?.objectOrNull("capture") != null)preference(obj("type" to "cancel_shortcut"))
        if(preferences()?.objectOrNull("shortcut_editor") != null)preference(obj("type" to "close_shortcut_editor"))
        state("modifier-per-tool") {
            val key = preferences()!!.getJSONObject("shortcut_page").array("modifiers").objects().first().getJSONObject("key")
            preference(obj("type" to "edit_modifier_key","key" to key))
            preference(obj("type" to "modifier_key_per_tool","key" to key,"per_tool" to true))
            host.awaitFrame("Modifier per-tool page") { preferences()?.objectOrNull("modifier_editor")?.optBoolean("per_tool") == true && placed("modifier-page") }
        }
        state("pen-button-per-tool") {
            preference(obj("type" to "page","page" to "input"))
            preference(obj("type" to "edit_pen_button","trigger" to "pen.button.primary"))
            preference(obj("type" to "pen_button_per_tool","trigger" to "pen.button.primary","per_tool" to true))
            host.awaitFrame("Pen button per-tool page") { preferences()?.objectOrNull("pen_button_editor")?.optBoolean("per_tool") == true && placed("pen-button-page") }
        }
        if(preferences()?.getString("page") != "shortcuts")invoke("keyboard_shortcuts")
        state("keymap-details") {
            preference(obj("type" to "keymap_details","open" to true));wait("keymap-details")
        }
        if(findTag("keymap-details") != null) { preference(obj("type" to "keymap_details","open" to false));gone("keymap-details") }
        state("keymap-import") {
            preference(obj("type" to "import_keymap","text" to obj("format" to "capycanvas-keymap","version" to 1,"keymap" to obj("id" to "photoshop","revision" to 1)).toString()))
            wait("keymap-import")
        }
        if(findTag("keymap-import-cancel") != null) { tap("keymap-import-cancel");gone("keymap-import-cancel") }
        closeAll()

        state("new-drawing") { invoke("new_document");wait("new-drawing-create") }
        fun newDrawing() { if(shared().objectOrNull("new_drawing") == null) { invoke("new_document");wait("new-drawing-create") } }
        fun drawer() = shared().objectOrNull("new_drawing")?.objectOrNull("drawer")
        fun mainPage() { repeat(3) { if(drawer() != null) { tap("new-drawing-cancel");compose.waitForIdle() } };host.awaitFrame("New Drawing main page") { drawer() == null && placed("new-drawing-create") } }
        state("new-drawing-presets") {
            newDrawing();reveal("new-drawing-presets-button");tap("new-drawing-presets-button")
            host.awaitFrame("Preset browser subpage") { drawer()?.optString("kind") == "presets" && findTag("new-drawing-presets") != null }
        }
        state("new-drawing-save-preset") {
            reveal("new-drawing-save-as");tap("new-drawing-save-as");wait("new-drawing-save-name")
            if(keyboard())hideIme()
        }
        if(shared().objectOrNull("new_drawing") != null)mainPage()
        state("new-paper") {
            newDrawing();reveal("new-drawing-texture");tap("new-drawing-texture")
            host.awaitFrame("Paper subpage") { drawer()?.optString("kind") == "paper" && findTag("new-drawing-papers") != null }
        }
        if(shared().objectOrNull("new_drawing") != null)mainPage()
        state("new-drawing-paper-color") {
            newDrawing();reveal("new-drawing-paper-color");tap("new-drawing-paper-color")
            host.awaitFrame("Paper colour subpage") { drawer()?.optString("kind") == "paper_color" && findTag("new-drawing-paper-colors") != null }
        }
        state("new-drawing-paper-color-menu") {
            if(drawer()?.optString("kind") != "paper_color") { newDrawing();reveal("new-drawing-paper-color");tap("new-drawing-paper-color");host.awaitFrame("Paper colour subpage") { findTag("new-drawing-paper-colors") != null } }
            hold("new-drawing-paper-color-0")
            if(findTag("new-drawing-paper-color-delete") == null)compose.onNodeWithTag("new-drawing-paper-color-0",useUnmergedTree = true).performSemanticsAction(SemanticsActions.OnLongClick)
            wait("new-drawing-paper-color-delete")
        }
        if(findTag("new-drawing-paper-color-delete") != null) { pressKey(KeyEvent.KEYCODE_ESCAPE);gone("new-drawing-paper-color-delete") }
        closeAll()

        state("color") {
            instrumentation.runOnMainSync { host.colorEditor = ColorEditorRequest("foreground",null,false,false) { _,_ -> } }
            wait("color-use")
        }
        state("color-model-popover") {
            reveal("color-format-0");tap("color-format-0")
            host.awaitFrame("Colour format menu opens") { findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("color-form-") == true }) != null }
        }
        if(findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("color-form-") == true }) != null) {
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            host.awaitFrame("Colour format menu closes") { findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("color-form-") == true }) == null }
        }
        state("all-swatches") {
            if(findTag("color-use") == null) { instrumentation.runOnMainSync { host.colorEditor = ColorEditorRequest("foreground",null,false,false) { _,_ -> } };wait("color-use") }
            reveal("color-swatches");tap("color-swatches");wait("color-sheet-search")
            if(keyboard())hideIme()
        }
        if(findTag("color-sheet-search") != null) { tap("color-cancel");host.awaitFrame("All Swatches returns to Edit Color") { findTag("color-sheet-search") == null && findTag("color-value-hex") != null } }
        state("color-eyedropper") {
            if(findTag("color-use") == null) { instrumentation.runOnMainSync { host.colorEditor = ColorEditorRequest("foreground",null,false,false) { _,_ -> } };wait("color-use") }
            reveal("color-pick");tap("color-pick");wait("color-strip")
        }
        if(findTag("color-strip") != null) { tap("color-strip");host.awaitFrame("Picking returns to Edit Color") { findTag("color-strip") == null && findTag("color-value-hex") != null } }
        closeAll()

        state("paper") {
            val paper = shared().array("layers").objects().first { it.optJSONObject("paper")?.optBoolean("editable") == true }.getLong("id")
            host.drain(obj("type" to "layer","action" to obj("op" to "select","id" to paper,"mask" to false)))
            instrumentation.runOnMainSync { host.textureDialog = TextureDialogRequest(paper,false) }
            wait("texture-use")
        }
        closeAll()

        state("export") {
            invoke("export_document");wait("export-choose-file")
            val deadline = SystemClock.uptimeMillis()+20_000
            while(SystemClock.uptimeMillis() < deadline && findTag("export-dialog")?.second?.let { dialog ->
                    var images = 0
                    fun count(node: SemanticsNode) { if(node.config.getOrNull(SemanticsProperties.Role) == androidx.compose.ui.semantics.Role.Image)images++;node.children.forEach(::count) }
                    count(dialog);images < 2 } != false) SystemClock.sleep(100)
            compose.waitForIdle()
        }
        fun exportRoot() { if(findTag("export-dialog") == null) { invoke("export_document");wait("export-choose-file") };host.awaitFrame("Export root page") { findTag("export-presets") != null } }
        state("export-size") { exportRoot();reveal("export-size");tap("export-size");wait("export-fit") }
        if(findTag("export-fit") != null) { tap("export-back");host.awaitFrame("Export Back returns to its root") { findTag("export-presets") != null } }
        state("export-color") {
            exportRoot();reveal("export-color");tap("export-color")
            host.awaitFrame("Export colour page") { findTag("export-presets") == null && placed("export-back") }
        }
        if(findTag("export-presets") == null && findTag("export-dialog") != null) { tap("export-back");host.awaitFrame("Export Back returns to its root") { findTag("export-presets") != null } }
        state("export-presets") { exportRoot();reveal("export-presets");tap("export-presets");wait("export-preset-save") }
        state("export-preset-name") {
            if(findTag("export-preset-save") == null) { exportRoot();reveal("export-presets");tap("export-presets");wait("export-preset-save") }
            tap("export-preset-save");wait("export-preset-name")
            if(keyboard())hideIme()
        }
        closeAll()
    }

    private fun visitIme(activity: MainActivity,host: CanvasHost,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit,checkAlert: Boolean = true) {
        pressKey(KeyEvent.KEYCODE_DPAD_RIGHT);compose.waitForIdle()
        instrumentation.runOnMainSync {
            val opener = checkNotNull(findTag("phone-tool-settings-hit")).second.find { it.config.getOrNull(SemanticsActions.RequestFocus) != null }
            assertTrue(checkNotNull(opener).config[SemanticsActions.RequestFocus].action!!.invoke())
        }
        host.drain(obj("type" to "phone","action" to obj("kind" to "insert_tools","before" to null)))
        host.awaitFrame("IME journey opens the shared picker") { findTag("tool-picker-search") != null }
        compose.waitForIdle()
        val decor = activity.window.decorView
        fun keyboard() = decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        var keyboardHeight = 0
        state("inventory-picker-ime") {
            val viewport = jsonValue(host.snapshot!!.getJSONObject("layout").getJSONArray("viewport"))
            if(findTag("tool-picker-search")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } == null)tap("tool-picker-search")
            host.awaitFrame("Native keyboard appears for the picker search") { keyboard() }
            keyboardHeight = decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom
            awaitEditorPage(activity,host,"tool-picker")
            assertNotNull("IME does not change phone classification",host.snapshot!!.getJSONObject("layout").objectOrNull("phone"))
            assertEquals("IME does not shrink the logical viewport",viewport,jsonValue(host.snapshot!!.getJSONObject("layout").getJSONArray("viewport")))
            val search = checkNotNull(findTag("tool-picker-search")).second.boundsInRoot
            assertTrue("Focused search remains visible: $search; page=${findTag("tool-picker")?.second?.boundsInRoot}; window=${decor.width}x${decor.height}; ime=${decor.rootWindowInsets.getInsets(WindowInsets.Type.ime())}",search.width>0f && search.height>0f)
            assertTrue("Focused search $search stays above keyboard y=${decor.height-decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom}",search.bottom <= decor.height-decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom+1)
        }
        state("inventory-picker-modal-focus") {
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Focus traversal has no keyboard owner") { !keyboard() }
            val last = host.snapshot!!.getJSONObject("picker").array("choices").objects().last().getString("label")
            compose.onNodeWithTag("tool-picker-choice-$last",useUnmergedTree = true).performScrollTo().performSemanticsAction(SemanticsActions.RequestFocus) { assertTrue("Last picker choice accepts native keyboard focus",it()) }
            for (meta in listOf(0,KeyEvent.META_SHIFT_ON)) {
                pressKey(KeyEvent.KEYCODE_TAB,meta);compose.waitForIdle()
                assertNotNull("Modal picker confines keyboard traversal",findTag("tool-picker")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true })
            }
            instrumentation.runOnMainSync { host.workspaceFocus.requestFocus() }
            assertNotNull("Modal picker excludes workspace focus",findTag("tool-picker")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true })
        }
        state("inventory-picker-search-escape") {
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("IME releases the native search key route") { !keyboard() }
            tap("tool-picker-search")
            host.awaitFrame("Native search owns focus before typing") {
                findTag("tool-picker-search")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null
            }
            instrumentation.sendStringSync("pen")
            host.awaitFrame("Native search input reaches the shared picker") { host.snapshot?.objectOrNull("picker")?.optString("query") == "pen" }
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Search Escape is tested without IME ownership") { !keyboard() }
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            host.awaitFrame("Shared search Escape clears the focused native field") {
                host.snapshot?.objectOrNull("picker")?.optString("query") == "" &&
                    findTag("tool-picker-search")?.second?.find { it.config.getOrNull(SemanticsProperties.EditableText) != null }
                        ?.config?.getOrNull(SemanticsProperties.EditableText)?.text == ""
            }
        }
        instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
        host.awaitFrame("IME is hidden before page Back") { !keyboard() }
        predictiveBack(activity,cancel = true)
        assertNotNull("Cancelled predictive Back preserves the picker",host.snapshot?.objectOrNull("picker"))
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Completed predictive Back closes the picker") { host.snapshot?.objectOrNull("picker") == null && findTag("tool-picker") == null }
        host.awaitFrame("Closing the modal picker restores its opener") { findTag("phone-tool-settings-hit")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null }
        if(checkAlert)alertImeAllocation(activity,host,keyboardHeight,state)
    }

    private fun alertImeAllocation(activity: MainActivity,host: CanvasHost,keyboardHeight: Int,state: (String,() -> Unit) -> Unit) {
        val decor = activity.window.decorView
        val view = checkNotNull(findTag("workspace")).first.view
        val original = decor.rootWindowInsets
        val oldIme = original.getInsets(WindowInsets.Type.ime())
        val oldVisible = original.isVisible(WindowInsets.Type.ime())
        try {
            state("inventory-alert-ime") {
                assertTrue("The reduced area comes from the measured native keyboard",keyboardHeight>0)
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form","action" to obj("type" to "reset","value" to host.workspaceManager!!.getString("id")))) }
                host.awaitFrame("Restore alert is composed") { findTag("workspace-form") != null }
                instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(original)
                    .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.of(0,0,0,keyboardHeight)).setVisible(WindowInsets.Type.ime(),true).build()) }
                host.awaitFrame("Alert follows its reduced native visible area") {
                    findTag("workspace-form")?.second?.boundsInRoot?.center?.y?.let { kotlin.math.abs(it-(decor.height-keyboardHeight)/2f)<=1f } == true
                }
                val bounds = checkNotNull(findTag("workspace-form")).second.boundsInRoot
                val visible = androidx.compose.ui.geometry.Rect(0f,0f,decor.width.toFloat(),(decor.height-keyboardHeight).toFloat())
                assertTrue("Alert card $bounds lies inside native IME-visible $visible (keyboard=$keyboardHeight)",bounds.left>=visible.left && bounds.top>=visible.top && bounds.right<=visible.right && bounds.bottom<=visible.bottom)
            }
        } finally {
            instrumentation.runOnMainSync {
                view.dispatchApplyWindowInsets(WindowInsets.Builder(original).setInsets(WindowInsets.Type.ime(),oldIme).setVisible(WindowInsets.Type.ime(),oldVisible).build())
                host.workspaceInput(obj("type" to "cancel"))
            }
            host.awaitFrame("IME allocation alert cancels") { findTag("workspace-form") == null }
        }
    }

    private fun visitDialogsA2(activity: MainActivity,host: CanvasHost,event: (Int,Offset) -> Unit,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit) {
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun invoke(command: String) = host.drain(obj("type" to "invoke","command" to command))
        fun cancel(type: String,op: String) = host.drain(obj("type" to type,"action" to obj("op" to op)))
        fun panel(name: String) {
            host.awaitFrame("Canvas panel $name is placed") { findTag("$name-panel")?.second?.let { it.layoutInfo.isPlaced && it.boundsInRoot.width > 0f } == true }
            compose.waitForIdle()
            val native = checkNotNull(findTag("$name-panel")).second.boundsInRoot
            val density = activity.resources.displayMetrics.density
            val expected = runBlocking { host.awaitQuery(obj("type" to "dialog_bounds","role" to "canvas_panel",
                "visible" to obj("x" to 0,"y" to 0,"width" to activity.window.decorView.width/density,"height" to activity.window.decorView.height/density),
                "measured" to JSONArray(listOf(native.width/density,native.height/density)))) }!!
            assertEquals("Shared panel x",expected.number("x")*density,native.left,1f)
            assertEquals("Shared panel y",expected.number("y")*density,native.top,1f)
            awaitCanvasPresentation(host);host.drain()
            val revision = shared().getJSONObject("document_file").getLong("revision")
            val outside = checkNotNull(findTag("workspace")).second.boundsInRoot.center
            if(!native.contains(outside)) {
                event(MotionEvent.ACTION_DOWN,outside);event(MotionEvent.ACTION_UP,outside);host.drain()
                assertEquals("Modal preview captures canvas contacts",revision,shared().getJSONObject("document_file").getLong("revision"))
            }
            predictiveBack(activity,cancel = true)
            assertNotNull("Cancelled Back preserves the preview",findTag("$name-panel"))
        }
        workspace(host,"painter")
        invoke("select_all")
        state("selection-refine") {
            invoke("feather_selection");panel("selection-refine")
        }
        if(shared().getJSONObject("layer_tools").objectOrNull("selection_resize") != null) {
            predictiveBack(activity,cancel = false)
            host.awaitFrame("Predictive Back cancels Refine") { shared().getJSONObject("layer_tools").objectOrNull("selection_resize") == null }
        }
        state("frequency-separation") {
            invoke("frequency_separation");panel("frequency-separation")
        }
        if(shared().getJSONObject("layer_tools").objectOrNull("frequency_separation") != null) {
            tap("frequency-separation-panel-cancel")
            host.awaitFrame("Frequency Separation cancels") { shared().getJSONObject("layer_tools").objectOrNull("frequency_separation") == null }
        }
        invoke("add_layer")
        if(shared().array("commands").objects().any { it.optString("id") == "deselect" && it.optBoolean("enabled") })invoke("deselect")
        invoke("search_commands")
        host.awaitFrame("Command search opens") { findTag("command-bar") != null && findTag("command-search") != null }
        fun settleSearch() {
            compose.waitForIdle()
            val view = checkNotNull(findTag("command-search")).first.view
            instrumentation.runOnMainSync { view.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Keyboard-free search reference") { !view.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
            compose.waitForIdle()
        }
        host.awaitFrame("Placed command search receives native focus") { findTag("command-search")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null }
        settleSearch();state("command-search") {}
        instrumentation.sendStringSync("undo")
        host.awaitFrame("Native search query reaches the session") { host.commandSearch?.optString("query") == "undo" }
        settleSearch();state("command-search-query") {}
        compose.waitForIdle()
        pressKey(KeyEvent.KEYCODE_A,KeyEvent.META_CTRL_ON)
        host.awaitFrame("Native search selects the whole query") {
            findTag("command-search")?.second?.find { it.config.getOrNull(SemanticsProperties.TextSelectionRange) != null }
                ?.config?.getOrNull(SemanticsProperties.TextSelectionRange)?.let { it.min == 0 && it.max == 4 } == true
        }
        instrumentation.sendStringSync("zzznomatchzzz")
        host.awaitFrame("Native search has the exact empty-result query") {
            host.commandSearch?.optString("query") == "zzznomatchzzz" && host.commandSearch?.array("results")?.length() == 0
        }
        settleSearch();state("command-search-empty") {}
        tap("command-close")
        host.awaitFrame("Search closes") { host.commandSearch == null && findTag("command-bar") == null }
        state("unsaved-changes") {
            instrumentation.runOnMainSync { host.drawingTabs.closeSelected() }
            host.awaitFrame("Unsaved Changes opens") { findTag("document-close") != null }
        }
        if(findTag("document-close") != null) {
            tap("document-close-cancel")
            host.awaitFrame("Unsaved Changes cancels") { findTag("document-close") == null }
        }
        invoke("drawing_brush")
        val canvas = checkNotNull(findTag("workspace")).second.boundsInRoot.center
        val stylus = android.view.InputDevice.getDeviceIds().asSequence().mapNotNull(android.view.InputDevice::getDevice)
            .firstOrNull { it.sources and android.view.InputDevice.SOURCE_STYLUS == android.view.InputDevice.SOURCE_STYLUS }
        val started = SystemClock.uptimeMillis()
        for (phase in listOf(MotionEvent.ACTION_HOVER_ENTER,MotionEvent.ACTION_HOVER_MOVE,MotionEvent.ACTION_HOVER_EXIT)) {
            val hover = MotionEvent.obtain(started,SystemClock.uptimeMillis(),phase,1,
                arrayOf(MotionEvent.PointerProperties().apply { id=0;toolType=MotionEvent.TOOL_TYPE_STYLUS }),
                arrayOf(MotionEvent.PointerCoords().apply { x=canvas.x;y=canvas.y;pressure=0f;setAxisValue(MotionEvent.AXIS_TILT,Math.toRadians(30.0).toFloat()) }),
                0,0,1f,1f,stylus?.id ?: -1,0,android.view.InputDevice.SOURCE_STYLUS,0)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(hover,true)) } finally { hover.recycle() }
        }
        host.drain()
        for (kind in listOf("pressure","tilt")) {
            state("$kind-calibration") {
                host.drain(obj("type" to "open_settings","page" to "input"))
                host.awaitFrame("Input preferences open") { findTag("setting-action-pen_$kind") != null }
                val action = "setting-action-pen_$kind"
                compose.onNodeWithTag(action,useUnmergedTree=true).performScrollTo();tap(action)
                host.awaitFrame("$kind calibration is placed") { findTag("pen-$kind-dialog")?.second?.let { it.layoutInfo.isPlaced && it.boundsInRoot.width>0f } == true }
                host.awaitFrame("$kind calibration measurement is published",diagnostics = {
                    "shared=${shared().objectOrNull("${kind}_calibration")?.opt("bounds")}; native=${host.calibrationPenBounds}; area=${host.snapshot?.getJSONObject("layout")?.getJSONObject("phone")?.opt("work_area")}; blocked=${host.documentInputBlocked}"
                }) {
                    val bounds = shared().objectOrNull("${kind}_calibration")?.getJSONObject("bounds")
                    val native = host.calibrationPenBounds
                    val density = activity.resources.displayMetrics.density
                    bounds != null && native != null && kotlin.math.abs((bounds.number("width")*density).roundToInt()-native.width)<.5f &&
                        kotlin.math.abs((bounds.number("height")*density).roundToInt()-native.height)<.5f
                }
                host.drain();compose.waitForIdle()
                val body = checkNotNull(findTag("pen-$kind-dialog")).second.boundsInRoot
                val area = host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONObject("work_area").rect()
                val density = activity.resources.displayMetrics.density
                assertTrue("Calibration fits the shared work area: $body in $area",body.left>=area.left*density-1 && body.top>=area.top*density-1 && body.right<=area.right*density+1 && body.bottom<=area.bottom*density+1)
            }
            if(shared().objectOrNull("${kind}_calibration") != null) {
                tap("pen-$kind-close")
                host.awaitFrame("$kind calibration closes") { shared().objectOrNull("${kind}_calibration") == null }
            }
            if(host.snapshot?.objectOrNull("preferences") != null)host.drain(obj("type" to "close_settings"))
        }
        var database: android.database.sqlite.SQLiteDatabase? = null
        try {
            state("workspace-save-status") {
                assertNotEquals(android.os.Looper.getMainLooper().thread,Thread.currentThread())
                val path = AppStorage.of(instrumentation.targetContext).workspaces.resolve("workspaces.sqlite3")
                assertTrue("The injected failure belongs to this private test",path.canonicalPath.startsWith(device.root.canonicalPath+File.separator))
                assertTrue("The private workspace store exists",path.isFile)
                database = android.database.sqlite.SQLiteDatabase.openDatabase(path.absolutePath,null,android.database.sqlite.SQLiteDatabase.OPEN_READWRITE or android.database.sqlite.SQLiteDatabase.NO_LOCALIZED_COLLATORS)
                database!!.execSQL("CREATE TRIGGER a2_fail_editing BEFORE INSERT ON editing WHEN NEW.id=1 BEGIN SELECT RAISE(ABORT,'owned workspace write failure'); END;")
                host.drain(obj("type" to "set_brush_size","value" to 83))
                host.awaitFrame("Owned workspace write failure is presented") { host.workspaceManager?.objectOrNull("status")?.optBoolean("error") == true && findTag("workspace-status") != null }
                compose.waitForIdle()
                val body = checkNotNull(findTag("workspace-status")).second.boundsInRoot
                val notice = host.snapshot!!.getJSONObject("layout").getJSONObject("notice")
                val density = activity.resources.displayMetrics.density
                val placed = JSONObject(Native.dialogUi(obj("kind" to "notice_above","notice" to notice,"bar" to host.workspaceInteraction?.canvasBarSlot?.let { obj("x" to it.left/density,"y" to it.top/density,"width" to it.width/density,"height" to it.height/density) }).toString()))
                assertEquals("Status uses the shared bottom anchor",placed.number("y")*density,body.bottom,1f)
                assertEquals("Status uses the shared horizontal anchor",notice.number("x")*density,body.center.x,1f)
                assertTrue("Status respects the shared maximum width",body.width<=notice.number("max_width")*density+1)
                assertEquals("The phone failure has one persistent presenter",false,host.workspaceInteraction?.popupOpen)
                assertNotNull("Retry is available",findTag("workspace-status-retry"))
                assertNotNull("Save as New is available",findTag("workspace-status-action"))
                host.drain(obj("type" to "layer","action" to obj("op" to "new_vector")))
                val pencil = host.catalog.array("brush_categories").objects().flatMap { it.array("brushes").objects() }.first { it.getString("label") == "Pencil" }.getLong("id")
                host.drain(obj("type" to "select_brush","id" to pencil));host.awaitReady(180_000,compose)
                val surface = checkNotNull(activity.window.decorView.descendant<CanvasSurfaceView>())
                val start = SystemClock.uptimeMillis()
                for ((phase,x) in listOf(MotionEvent.ACTION_DOWN to .4f,MotionEvent.ACTION_MOVE to .6f,MotionEvent.ACTION_UP to .6f)) {
                    val input = motion(MotionEvent.TOOL_TYPE_MOUSE,phase,Offset(surface.width*x,surface.height*.5f),start)
                    try { instrumentation.runOnMainSync { surface.dispatchTouchEvent(input) } } finally { input.recycle() }
                }
                host.awaitFrame("Timed notice and persistent status coexist") { host.notice?.id != null && host.workspaceInteraction?.noticeBounds != null && findTag("workspace-status") != null }
                compose.waitForIdle()
                val bubble = checkNotNull(host.workspaceInteraction?.noticeBounds)
                val status = checkNotNull(findTag("workspace-status")).second.boundsInRoot
                assertTrue("Persistent status does not overlap its timed notice",status.bottom < bubble.top)
                val expected = JSONObject(Native.dialogUi(obj("kind" to "notice_above","notice" to notice,
                    "bar" to host.workspaceInteraction?.canvasBarSlot?.let { obj("x" to it.left/density,"y" to it.top/density,"width" to it.width/density,"height" to it.height/density) },
                    "bubble" to obj("x" to bubble.left/density,"y" to bubble.top/density,"width" to bubble.width/density,"height" to bubble.height/density)).toString()))
                assertEquals("Status clears both native surfaces through the shared placement",expected.number("y")*density,status.bottom,1f)
            }
        } finally {
            database?.let { try { it.execSQL("DROP TRIGGER IF EXISTS a2_fail_editing") } finally { it.close() } }
        }
        if(host.workspaceManager?.objectOrNull("status")?.optBoolean("error") == true) {
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "retry")) }
            host.awaitFrame("The owned failure recovers after Retry") { host.workspaceManager?.objectOrNull("status")?.optBoolean("error") != true }
        }
    }

    private fun visitCustomize(activity: MainActivity,host: CanvasHost,event: (Int,Offset) -> Unit,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit) {
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun layout() = shared().getJSONObject("workspace").getJSONObject("layout")
        fun tiles() = layout().array("phone").objects()
        fun history() = JSONObject(runBlocking { host.withNative { Native.workspace(it,obj("type" to "capture").toString()) } }).getJSONObject("history")
        fun invoke(command: String) = host.drain(obj("type" to "invoke","command" to command))
        fun allocatedTools() {
            compose.waitForIdle()
            host.awaitFrame("Phone contacts and allocations match the published toolbar") {
                val dock = host.workspaceInteraction ?: return@awaitFrame false
                val input = dock.phone ?: return@awaitFrame false
                val phone = host.snapshot!!.getJSONObject("layout").getJSONObject("phone")
                val density = activity.resources.displayMetrics.density
                dock.enabled && !dock.popupOpen && input.tiles.toString() == layout().array("phone").toString() && input.layout.toString() == phone.toString() &&
                    tiles().withIndex().all { (index,tile) ->
                        findTag("phone-tool-${tile.getInt("id")}")?.second?.let { node ->
                            val target = phone.array("tools").getJSONObject(index)
                            node.layoutInfo.isPlaced && kotlin.math.abs(node.boundsInRoot.left-dock.origin.x-target.number("x")*density)<=1f &&
                                kotlin.math.abs(node.boundsInRoot.top-dock.origin.y-target.number("y")*density)<=1f
                        } == true
                    }
            }
        }
        fun point(tag: String): Offset {
            compose.waitForIdle()
            val (view,node) = checkNotNull(findTag(tag)) { tag }
            val origin = IntArray(2);instrumentation.runOnMainSync { view.view.getLocationOnScreen(origin) }
            return node.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
        }
        fun hold(tile: Int?) {
            allocatedTools()
            val at = if(tile != null)point("phone-tool-$tile") else {
                val phone = host.snapshot!!.getJSONObject("layout").getJSONObject("phone")
                val excluded = phone.array("tool_hits").objects()+phone.array("controls").objects()+listOf("capy_hit","settings_hit","size","strength").map { phone.getJSONObject(it) }
                val empty = phone.array("bars").objects().asSequence().flatMap { bar ->
                    (bar.number("y").toInt() until (bar.number("y")+bar.number("height")).toInt()).asSequence().flatMap { y ->
                        (bar.number("x").toInt() until (bar.number("x")+bar.number("width")).toInt()).asSequence().map { x -> Offset(x+.5f,y+.5f) }
                    }
                }.first { p -> phone.array("bars").objects().any { it.rect().contains(p) } && excluded.none { it.rect().contains(p) } }
                val root = checkNotNull(findTag("workspace")).first.view
                val origin = IntArray(2);instrumentation.runOnMainSync { root.getLocationOnScreen(origin) }
                empty*instrumentation.targetContext.resources.displayMetrics.density+Offset(origin[0].toFloat(),origin[1].toFloat())
            }
            event(MotionEvent.ACTION_DOWN,at)
            host.awaitFrame("Hold arms native pickup without opening a menu",diagnostics = {
                val dock = host.workspaceInteraction
                "at=$at; enabled=${dock?.enabled}; popup=${dock?.popupOpen}; windowFocus=${activity.hasWindowFocus()}; contact=${dock?.phone?.contact != null}; outcome=${dock?.phone?.preview?.opt("outcome")}"
            }) { host.workspaceInteraction?.phone?.held == true }
            assertNull("The editing menu opens on release",findTag("workspace-menu"))
            event(MotionEvent.ACTION_UP,at)
            host.awaitFrame("Hold release opens the editing menu") { findTag("workspace-menu") != null }
            compose.waitForIdle()
            val presentation = checkNotNull(host.workspaceInteraction?.contextPresentation)
            assertEquals("contact",presentation.getString("anchor"))
            assertTrue(presentation.getBoolean("has_arrow"))
        }
        fun menuLabel(label: String) {
            val (root,menu) = checkNotNull(findTag("workspace-menu"))
            val text = checkNotNull(menu.find { it.config.getOrNull(SemanticsProperties.Text)?.any { value -> value.text == label } == true }) { label }
            val origin = IntArray(2);instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val point = text.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
            event(MotionEvent.ACTION_DOWN,point);event(MotionEvent.ACTION_UP,point);host.drain();compose.waitForIdle()
        }
        fun picker(): JSONObject {
            host.awaitFrame("ToolPicker is presented") { host.snapshot?.objectOrNull("picker") != null && findTag("tool-picker-search") != null }
            compose.waitForIdle()
            return host.snapshot!!.getJSONObject("picker")
        }
        fun settlePicker() {
            picker()
            val decor = activity.window.decorView
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Keyboard-free reference fixture hides IME") { !decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
            awaitEditorPage(activity,host,"tool-picker")
            compose.waitForIdle()
        }
        fun select(count: Int) {
            val choices = picker().array("choices").objects().filter { it.getJSONObject("control").getString("kind") != "divider" }.take(count)
            assertEquals(count,choices.size)
            choices.forEach { choice ->
                val tag = "tool-picker-choice-${choice.getString("label")}"
                compose.onNodeWithTag(tag,useUnmergedTree = true).performScrollTo();tap(tag)
            }
            assertEquals(count,picker().getInt("selected_count"))
        }
        fun confirm() {
            tap("tool-picker-confirm")
            host.awaitFrame("ToolPicker closes after confirmation") { host.snapshot?.objectOrNull("picker") == null && findTag("tool-picker") == null }
        }
        workspace(host,"painter")
        val before = jsonValue(layout())
        val removed = tiles().first().getInt("id")
        hold(removed);state("customize-tile-menu") {}
        val removeLabel = host.workspaceInteraction!!.contextMenu!!.array("sections").values().flatMap { (it as JSONArray).objects() }
            .first { it.objectOrNull("action")?.objectOrNull("action")?.optString("kind") == "remove_tool" }.getString("label")
        menuLabel(removeLabel)
        host.awaitFrame("Remove Tool changes the phone toolbar") { tiles().none { it.getInt("id") == removed } }
        val afterRemove = jsonValue(layout())
        invoke("undo_workspace");assertEquals(before,jsonValue(layout()))
        invoke("redo_workspace");assertEquals(afterRemove,jsonValue(layout()))
        val anchor = tiles().first().getInt("id")
        fun insert() {
            hold(anchor)
            val label = host.workspaceInteraction!!.contextMenu!!.array("sections").values().flatMap { (it as JSONArray).objects() }
                .first { it.objectOrNull("action")?.objectOrNull("action")?.optString("kind") == "insert_tools" }.getString("label")
            menuLabel(label);settlePicker()
        }
        insert();select(2);state("customize-insert-two") {};confirm()
        assertEquals(anchor,tiles()[2].getInt("id"))
        insert();select(1);confirm()
        assertEquals(10,tiles().count { it.getJSONObject("control").getString("kind") != "divider" })
        hold(null)
        menuLabel(host.workspaceInteraction!!.contextMenu!!.array("sections").getJSONArray(0).getJSONObject(0).getString("label"))
        settlePicker()
        val capacity = picker()
        assertFalse(capacity.getBoolean("can_confirm"))
        val disabled = capacity.array("disabled").values().map { it.toString() }.toSet()
        capacity.array("choices").objects().forEach { choice ->
            val row = checkNotNull(findTag("tool-picker-choice-${choice.getString("label")}")).second
            assertEquals("Native capacity row follows shared availability",choice.getJSONObject("control").toString() in disabled,
                row.config.getOrNull(SemanticsProperties.Disabled) != null)
        }
        state("customize-capacity") {}
        val divider = capacity.array("choices").objects().first { it.getJSONObject("control").getString("kind") == "divider" }
        val tag = "tool-picker-choice-${divider.getString("label")}"
        compose.onNodeWithTag(tag,useUnmergedTree = true).performScrollTo();tap(tag)
        assertTrue(picker().getBoolean("can_confirm"));state("customize-capacity-divider") {}
        pressKey(KeyEvent.KEYCODE_ESCAPE)
        host.awaitFrame("Capacity picker cancels") { host.snapshot?.objectOrNull("picker") == null && findTag("tool-picker") == null }
        state("frame-ten-tools") {}
        val beforeMove = jsonValue(layout())
        val first = tiles()[0].getInt("id")
        val from = point("phone-tool-$first")
        val to = point("phone-tool-${tiles()[2].getInt("id")}")+if(host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getString("edge") == "top")Offset(1f,0f) else Offset(0f,1f)
        event(MotionEvent.ACTION_DOWN,from)
        host.awaitFrame("Reorder pickup is armed") { host.workspaceInteraction?.phone?.held == true }
        event(MotionEvent.ACTION_MOVE,to);state("customize-reorder-held") {}
        event(MotionEvent.ACTION_UP,to);host.drain()
        host.awaitFrame("Reorder moves the first tile after two peers") { tiles()[2].getInt("id") == first }
        invoke("undo_workspace");assertEquals(beforeMove,jsonValue(layout()))
        var offbarDown = false
        state("customize-offbar-cancelled") {
            allocatedTools()
            event(MotionEvent.ACTION_DOWN,from);offbarDown = true
            host.awaitFrame("Off-bar pickup is armed",diagnostics = {
                val dock = host.workspaceInteraction
                "at=$from; enabled=${dock?.enabled}; popup=${dock?.popupOpen}; windowFocus=${activity.hasWindowFocus()}; contact=${dock?.phone?.contact != null}; outcome=${dock?.phone?.preview?.opt("outcome")}"
            }) { host.workspaceInteraction?.phone?.held == true }
            val outside = checkNotNull(findTag("workspace")).second.boundsInRoot.center
            event(MotionEvent.ACTION_MOVE,outside);event(MotionEvent.ACTION_UP,outside);offbarDown = false;host.drain()
            assertEquals("Off-bar release preserves the layout",beforeMove,jsonValue(layout()))
        }
        if(offbarDown)event(MotionEvent.ACTION_CANCEL,from)
        val workspace = JSONObject(shared().getJSONObject("workspace").toString())
        workspace.getJSONObject("layout").put("phone",JSONArray())
        host.drain(obj("type" to "restore_workspace","workspace" to workspace))
        state("frame-empty-tools") {}
        hold(null);state("customize-empty-toolbar") {}
        menuLabel(host.workspaceInteraction!!.contextMenu!!.array("sections").getJSONArray(0).getJSONObject(0).getString("label"));settlePicker()
        pressKey(KeyEvent.KEYCODE_ESCAPE)
        host.awaitFrame("Empty-toolbar picker cancels") { host.snapshot?.objectOrNull("picker") == null && findTag("tool-picker") == null }
        val empty = jsonValue(layout())
        val prior = history()
        instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form","action" to obj("type" to "reset","value" to host.workspaceManager!!.getString("id")))) }
        host.awaitFrame("Phone restore confirmation") { findTag("workspace-form") != null }
        state("customize-phone-restore-confirm") {}
        tap("workspace-submit")
        host.awaitFrame("Phone restore completes") { host.workspaceManager?.objectOrNull("prompt") == null && host.workspaceManager?.optBoolean("busy") == false && tiles().isNotEmpty() }
        val restored = jsonValue(layout())
        assertEquals("Restore appends one workspace edit",prior.array("undo").length()+1,history().array("undo").length())
        assertNotEquals(empty,restored)
        invoke("undo_workspace");assertEquals(empty,jsonValue(layout()))
        invoke("redo_workspace");assertEquals(restored,jsonValue(layout()))
        state("customize-phone-restored") {}
    }

    private fun visitMenu(activity: MainActivity, host: CanvasHost, event: (Int, Offset) -> Unit, tap: (String) -> Unit,
        state: (String, () -> Unit) -> Unit, cancel: () -> Unit) {
        fun phone() = host.snapshot!!.getJSONObject("state").getJSONObject("phone")
        fun route() = phone().getJSONObject("menu_route").toString()
        fun bounds() = checkNotNull(findTag("phone-menu")).second.boundsInRoot
        fun open() {
            if (!phone().getBoolean("menu")) tap("phone-capy")
            host.awaitFrame("Phone menu allocated") { phone().getBoolean("menu") && findTag("phone-menu-scroll")?.second?.boundsInRoot?.width?.let { it > 0 } == true }
            compose.waitForIdle()
        }
        fun name(route: JSONObject): String {
            val page = route.objectOrNull("page")
            val root = when (page?.optString("kind")) { null -> "main"; "application" -> "application-${page.getString("menu")}"; else -> page.getString("kind") }
            return root + route.array("path").values().flatMap { (it as JSONArray).values() }.joinToString("") { "-$it" }
        }
        fun model() = JSONObject(runBlocking { host.drawingTabs.query(obj("op" to "phone_menu")) })
        open(); tap("phone-drawing")
        host.awaitFrame("Drawings for native selection") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
        fun drawingRows() = model().array("pages").objects().first { it.getJSONObject("id").objectOrNull("page")?.optString("kind") == "drawings" }
            .array("sections").getJSONArray(0).objects()
        val original = host.drawingTabs.selected
        val selectedRows = drawingRows()
        val other = selectedRows.indexOfFirst { it.objectOrNull("close") != null && !it.optBoolean("selected") }
        assertTrue("Fixture has an inactive drawing",other >= 0)
        val selectedId = selectedRows[other].getJSONObject("action").getJSONObject("action").getLong("id")
        tap("phone-menu-row-0-$other")
        host.awaitFrame("select_drawing request selects the native tab",60_000) { host.drawingTabs.selected == selectedId && !host.drawingTabs.switching && !phone().getBoolean("menu") }
        open(); tap("phone-drawing")
        host.awaitFrame("Drawings for native close") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
        val closeRows = drawingRows()
        val closing = closeRows.indexOfFirst { it.objectOrNull("close") != null && it.optBoolean("selected") }
        val count = host.drawingTabs.rows.size
        tap("phone-menu-close-0-$closing")
        host.awaitFrame("select_drawing close resolves exactly once",60_000) { host.drawingTabs.rows.size == count-1 && host.drawingTabs.selected == original && !host.drawingTabs.switching }
        for (swapped in listOf(false, true)) {
            if (phone().getBoolean("swapped") != swapped) host.drain(obj("type" to "phone", "action" to obj("kind" to "swap_sides")))
            open()
            val initial = bounds()
            instrumentation.runOnMainSync {
                val b = checkNotNull(host.workspaceInteraction).facts().getJSONObject("content_drawer").rect()
                val density = checkNotNull(host.workspaceInteraction).density
                assertEquals("Menu contact bounds track the visible surface",initial.center.x/density,b.center.x,1f)
                assertEquals("Menu contact bounds track the visible surface",initial.center.y/density,b.center.y,1f)
            }
            state("menu-open-$swapped", {})
            val pages = model().array("pages").objects()
            fun visit(page: JSONObject) {
                val expected = page.getJSONObject("id").toString()
                host.awaitFrame("Typed menu route $expected") { route() == expected }
                compose.waitForIdle()
                assertEquals("Every page retains the root's outer bounds", initial, bounds())
                state("menu-${name(page.getJSONObject("id"))}${if (swapped) "-swapped" else ""}", {})
                page.array("sections").values().forEachIndexed { section, rows ->
                    (rows as JSONArray).objects().forEachIndexed { index, item ->
                        val target = item.objectOrNull("target") ?: return@forEachIndexed
                        val tag = if (target.objectOrNull("page")?.optString("kind") == "drawings") "phone-drawing" else "phone-menu-row-$section-$index"
                        compose.onNodeWithTag(tag, useUnmergedTree = true).performScrollTo()
                        tap(tag)
                        val next = pages.first { it.getJSONObject("id").toString() == target.toString() }
                        visit(next)
                        compose.onNodeWithTag("phone-menu-back", useUnmergedTree = true).performScrollTo()
                        tap("phone-menu-back")
                        host.awaitFrame("Back returns to $expected") { route() == expected }
                    }
                }
            }
            visit(pages.first { it.isNull("parent") })
            if (!swapped) {
                fun application(menu: String) {
                    val root = pages.first { it.isNull("parent") }
                    val position = root.array("sections").values().withIndex().firstNotNullOf { (section, rows) ->
                        (rows as JSONArray).objects().withIndex().firstOrNull { (_,item) -> item.objectOrNull("target")?.objectOrNull("page")?.optString("menu") == menu }?.let { section to it.index }
                    }
                    val tag = "phone-menu-row-${position.first}-${position.second}"
                    compose.onNodeWithTag(tag,useUnmergedTree=true).performScrollTo()
                    tap(tag)
                    host.awaitFrame("Application menu $menu") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("menu") == menu }
                    compose.waitForIdle()
                }
                application("view")
                fun scrollValue() = checkNotNull(findTag("phone-menu-scroll")).second.find { it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null }
                    ?.config?.getOrNull(SemanticsProperties.VerticalScrollAxisRange)?.value?.invoke() ?: error("Menu has no native scroller")
                val scrollBefore = scrollValue()
                val scrollBounds = checkNotNull(findTag("phone-menu-scroll")).second.boundsInRoot
                val scrollFrom = Offset(scrollBounds.center.x,scrollBounds.top+scrollBounds.height*.75f)
                val distance = (scrollBounds.height*.4f).coerceAtMost(100f)
                event(MotionEvent.ACTION_DOWN,scrollFrom)
                event(MotionEvent.ACTION_MOVE,scrollFrom-Offset(0f,distance*.4f))
                event(MotionEvent.ACTION_MOVE,scrollFrom-Offset(0f,distance))
                state("menu-touch-scrolling") {
                    host.awaitFrame("One finger scrolls the native menu") { scrollValue() > scrollBefore+1 }
                    assertNull("A scrolling contact releases the row highlight",host.workspaceInteraction?.phone?.menu?.pressed)
                }
                state("menu-touch-scrolled") {
                    event(MotionEvent.ACTION_UP,scrollFrom-Offset(0f,distance))
                    host.awaitFrame("The released menu keeps its scroll") { scrollValue() > scrollBefore+1 }
                    host.awaitFrame("Scrolling preserves the menu route") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("menu") == "view" }
                }
                cancel()
                compose.onNodeWithTag("phone-menu-back",useUnmergedTree=true).performScrollTo();tap("phone-menu-back")
                host.awaitFrame("Back returns from scrolling") { phone().getJSONObject("menu_route").isNull("page") }
                compose.waitForIdle()
                application("file")
                compose.waitForIdle()
                val childRoute = route()
                instrumentation.runOnMainSync {
                    val input = checkNotNull(host.workspaceInteraction?.phone?.menu)
                    assertEquals("The displayed menu tracker has applied the route",childRoute,input.route.toString())
                    assertTrue("File exposes its Back contact",input.canBack)
                }
                val scroll = checkNotNull(findTag("phone-menu-scroll")).second.boundsInRoot
                val from = Offset(scroll.left+scroll.width*.15f,scroll.top+scroll.height*.5f)
                state("menu-swipe-cancelled") {
                    event(MotionEvent.ACTION_DOWN,from)
                    event(MotionEvent.ACTION_MOVE,from+Offset(scroll.width*.1f,0f))
                    host.awaitFrame("Shared swipe previews Back") { host.workspaceInteraction?.phone?.menu?.swiping == true }
                    cancel()
                    host.awaitFrame("Cancelled swipe retires") { host.workspaceInteraction?.phone?.menu?.swiping == false }
                    assertEquals("Cancel does not navigate",childRoute,route())
                }
                cancel()
                state("menu-swipe-halfway") {
                    event(MotionEvent.ACTION_DOWN,from)
                    event(MotionEvent.ACTION_MOVE,from+Offset(scroll.width*.5f,0f))
                    host.awaitFrame("Shared halfway Back preview") { host.workspaceInteraction?.phone?.menu?.progress?.let { it >= .49f } == true }
                }
                cancel()
                state("menu-swipe-completed") {
                    event(MotionEvent.ACTION_DOWN,from)
                    event(MotionEvent.ACTION_MOVE,from+Offset(scroll.width*.5f,0f))
                    host.awaitFrame("Shared Back owns its completion") { host.workspaceInteraction?.phone?.menu?.swiping == true }
                    event(MotionEvent.ACTION_UP,from+Offset(scroll.width*.5f,0f))
                    host.awaitFrame("Shared completed swipe returns to root") { phone().getJSONObject("menu_route").isNull("page") }
                }
                cancel()
                if (!phone().getJSONObject("menu_route").isNull("page")) tap("phone-menu-back")
            }
            tap("phone-capy")
            host.awaitFrame("Menu closed") { !phone().getBoolean("menu") && findTag("phone-menu") == null }
        }
        if (phone().getBoolean("swapped")) host.drain(obj("type" to "phone", "action" to obj("kind" to "swap_sides")))
        host.drain(obj("type" to "invoke", "command" to "add_layer"))
        val first = host.drawingTabs.selected
        createDrawing(host,128,96,"Menu drawing")
        open()
        val feedbackBounds = bounds()
        state("feedback-root-two", {})
        for (tag in listOf("phone-finger","phone-swap")) {
            fun checked() = checkNotNull(findTag(tag)).second.config.getOrNull(SemanticsProperties.Selected)
            val before = checked()
            for (value in listOf(before != true,before == true)) {
                tap(tag)
                host.awaitFrame("Strip toggle $tag remains in the menu") { phone().getBoolean("menu") && checked() == value }
            }
        }
        tap("phone-drawing")
        host.awaitFrame("Two drawings page") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
        compose.waitForIdle()
        assertEquals("Drawings retains root bounds",feedbackBounds,bounds())
        state("feedback-drawings-two", {})
        val retiredBack = checkNotNull(findTag("phone-menu-back")).second
        val firstRow = drawingRows().indexOfFirst { it.getJSONObject("action").getJSONObject("action").getLong("id") == first }
        tap("phone-menu-row-0-$firstRow")
        host.awaitFrame("The modified drawing becomes current",60_000) { host.drawingTabs.selected == first && !host.drawingTabs.switching && !phone().getBoolean("menu") }
        host.drain(obj("type" to "invoke", "command" to "drawings"))
        host.awaitFrame("Drawings command opens the shared page") { phone().getBoolean("menu") && phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
        compose.waitForIdle()
        assertEquals("The external opener retains root bounds",feedbackBounds,bounds())
        assertFalse("The retired page detaches its semantics",retiredBack.layoutInfo.isAttached)
        assertNotEquals("A reopened page owns a new Back control",retiredBack.id,checkNotNull(findTag("phone-menu-back")).second.id)
        assertEquals("Reopened Drawings owns navigation","drawings",phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind"))
        state("feedback-drawings-current", {})
        open(); tap("phone-zen")
        host.awaitFrame("Zen hides the bars") { phone().getBoolean("hidden") && !phone().getBoolean("menu") }
        state("hidden", {})
        tap("phone-capy")
        host.awaitFrame("Capy restores hidden bars") { !phone().getBoolean("hidden") }
        open()
        fun focusedRows() = checkNotNull(host.workspaceInteraction?.phone?.menu).let { input -> input.rows.filter { it in input.hits } }
        fun focused() = host.workspaceInteraction?.phone?.menu?.focused
        pressKey(KeyEvent.KEYCODE_MOVE_END);compose.waitForIdle()
        assertEquals("End focuses the last page row",focusedRows().last(),focused())
        pressKey(KeyEvent.KEYCODE_MOVE_HOME);compose.waitForIdle()
        assertEquals("Home focuses the first page row",focusedRows().first(),focused())
        pressKey(KeyEvent.KEYCODE_DPAD_DOWN);compose.waitForIdle()
        assertEquals("Down stays inside page rows",focusedRows()[1],focused())
        pressKey(KeyEvent.KEYCODE_DPAD_UP);compose.waitForIdle()
        assertEquals("Up returns to the first page row",focusedRows().first(),focused())
        pressKey(KeyEvent.KEYCODE_DPAD_RIGHT)
        host.awaitFrame("Forward opens the focused submenu") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
        compose.waitForIdle()
        val child = route()
        predictiveBack(activity,cancel = true)
        assertEquals("Cancelled Back keeps the menu subpage",child,route())
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Predictive Back returns to the root") { phone().getJSONObject("menu_route").isNull("page") }
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Predictive Back closes the root menu") { !phone().getBoolean("menu") }
        open();tap("phone-zen")
        host.awaitFrame("Zen ready for predictive Back") { phone().getBoolean("hidden") }
        predictiveBack(activity,cancel = true)
        assertTrue("Cancelled Back keeps hidden bars",phone().getBoolean("hidden"))
        predictiveBack(activity,cancel = false)
        host.awaitFrame("Predictive Back restores the bars") { !phone().getBoolean("hidden") }
        open()
        val scroll = checkNotNull(findTag("phone-menu-scroll")).second.find { it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null }!!
        val axis = scroll.config[SemanticsProperties.VerticalScrollAxisRange]
        if (axis.maxValue() > 1f) {
            val root = model().array("pages").objects().first { it.isNull("parent") }
            val position = root.array("sections").values().withIndex().firstNotNullOf { (section,rows) ->
                (rows as JSONArray).objects().withIndex().firstOrNull { (_,item) -> item.objectOrNull("target")?.objectOrNull("page")?.optString("menu") == "file" }?.let { section to it.index }
            }
            val (view,row) = checkNotNull(findTag("phone-menu-row-${position.first}-${position.second}"))
            val screen = IntArray(2);instrumentation.runOnMainSync { view.view.getLocationOnScreen(screen) }
            val point = row.boundsInRoot.center+Offset(screen[0].toFloat(),screen[1].toFloat())
            val start = SystemClock.uptimeMillis()
            fun mouse(action: Int, at: Offset) {
                val event = motion(MotionEvent.TOOL_TYPE_MOUSE,action,at,start)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event,true)) } finally { event.recycle() }
            }
            mouse(MotionEvent.ACTION_DOWN,point)
            mouse(MotionEvent.ACTION_MOVE,point+Offset(1f,0f))
            fun canRelease(): Boolean {
                var accepted = false
                instrumentation.runOnMainSync {
                    val input = checkNotNull(host.workspaceInteraction?.phone?.menu)
                    val hit = obj("kind" to "row","position" to JSONArray(listOf(position.first,position.second)))
                    val reply = phoneUi(obj("kind" to "menu_input","contact" to input.contact,"route" to input.route,"can_back" to input.canBack,
                        "input" to obj("kind" to "release","hit" to hit,"inside" to true))) as JSONObject
                    accepted = reply.getJSONObject("reply").getBoolean("accepted")
                }
                return accepted
            }
            assertTrue("The below-slop root contact is still armed before scrolling",canRelease())
            val before = axis.value()
            val wheel = MotionEvent.obtain(start,SystemClock.uptimeMillis(),MotionEvent.ACTION_SCROLL,1,
                arrayOf(MotionEvent.PointerProperties().apply { id=0;toolType=MotionEvent.TOOL_TYPE_MOUSE }),
                arrayOf(MotionEvent.PointerCoords().apply { x=point.x+1f;y=point.y;setAxisValue(MotionEvent.AXIS_VSCROLL,-.05f) }),
                0,MotionEvent.BUTTON_PRIMARY,1f,1f,0,0,android.view.InputDevice.SOURCE_MOUSE,0)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(wheel,true)) } finally { wheel.recycle() }
            host.awaitFrame("A small wheel step moves the menu") { axis.value() > before }
            compose.waitForIdle()
            assertFalse("Every scroll-position change cancels the shared press",canRelease())
            mouse(MotionEvent.ACTION_UP,point+Offset(1f,0f))
            host.drain();compose.waitForIdle()
            assertTrue("Wheel scrolling cancels even a visually unpressed row",phone().getBoolean("menu") && phone().getJSONObject("menu_route").isNull("page"))
        }
        pressKey(KeyEvent.KEYCODE_ESCAPE)
        host.awaitFrame("Escape closes after keyboard and wheel checks") { !phone().getBoolean("menu") }
        val nativeView = checkNotNull(findTag("workspace")).first.view
        val direction = nativeView.layoutDirection
        try {
            instrumentation.runOnMainSync { nativeView.layoutDirection = android.view.View.LAYOUT_DIRECTION_RTL }
            compose.waitForIdle()
            open()
            pressKey(KeyEvent.KEYCODE_MOVE_HOME);compose.waitForIdle()
            pressKey(KeyEvent.KEYCODE_DPAD_LEFT)
            host.awaitFrame("RTL forward opens the focused submenu") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "drawings" }
            compose.waitForIdle()
            pressKey(KeyEvent.KEYCODE_DPAD_RIGHT)
            host.awaitFrame("RTL back returns to the root") { phone().getJSONObject("menu_route").isNull("page") }
            compose.waitForIdle()
            pressKey(KeyEvent.KEYCODE_ESCAPE)
            host.awaitFrame("RTL Escape closes the root menu") { !phone().getBoolean("menu") }
        } finally {
            instrumentation.runOnMainSync { nativeView.layoutDirection = direction }
            compose.waitForIdle()
        }
    }

}
