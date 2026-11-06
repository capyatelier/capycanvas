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

class AndroidPhoneAuditTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    private fun CanvasHost.awaitFrame(label: String, timeout: Long = 10_000, diagnostics: () -> String = { "" }, condition: () -> Boolean) =
        awaitMain(label, timeout, diagnostics, compose, condition)

    private fun shell(command: String): String = ParcelFileDescriptor.AutoCloseInputStream(
        instrumentation.uiAutomation.executeShellCommand(command)).bufferedReader().use { it.readText().trim() }

    private fun collect(node: SemanticsNode, allocations: JSONObject) {
        node.config.getOrNull(SemanticsProperties.TestTag)?.let { tag ->
            val bounds = node.boundsInRoot
            allocations.put(tag, JSONArray(listOf(bounds.left, bounds.top, bounds.width, bounds.height)))
        }
        node.children.forEach { collect(it, allocations) }
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

    private fun visit(admissionOnly: Boolean) {
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
        val report = obj("schema_version" to 1, "host" to "android", "family" to "frame", "state" to "rest", "case" to case,
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
                report.put("state", if (admissionOnly) "rest" else name).put("status", "failed").put("passed", false)
                host.drain()
                compose.waitForIdle()
                if (!admissionOnly) {
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
                    report.put("crop", obj("x" to crop[0], "y" to crop[1], "width" to crop[2], "height" to crop[3]))
                    val surface = checkNotNull(decor.descendant<CanvasSurfaceView>())
                    report.put("canvas_viewport", JSONArray(listOf(surface.width / density, surface.height / density)))
                    val canvasLocation = IntArray(2)
                    surface.getLocationOnScreen(canvasLocation)
                    report.put("canvas_origin_pixels", JSONArray(canvasLocation.toList()))
                    val root = checkNotNull(findTag("workspace"))
                    collect(root.second, allocations)
                    report.put("phone_active", host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") != null)
                    report.put("revision", host.snapshot?.optLong("revision"))
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
            else visitFrame(host, args.getString("states"), ::capture) { name, error ->
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
    private fun visitFrame(host: CanvasHost, states: String?, capture: (String) -> Unit, failed: (String, Throwable) -> Unit) {
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
        val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
        host.drain(obj("type" to "invoke", "command" to "new_document"))
        host.awaitFrame("New Drawing controller is open") { host.snapshot!!.getJSONObject("state").objectOrNull("new_drawing") != null }
        compose.waitForIdle()
        runBlocking { host.withNative { orderDrawing(it, 1200, 1600, create = false) } }
        instrumentation.runOnMainSync { host.documents.createNewDrawing() }
        host.awaitFrame("New Drawing controller adopts the fixture", 60_000) {
            val state = host.snapshot!!.getJSONObject("state")
            state.getJSONObject("document_file").getLong("epoch") != epoch &&
                state.array("tabs").objects().any { it.getBoolean("active") && it.getInt("width") == 1200 && it.getInt("height") == 1600 } &&
                !host.documents.working && !host.drawingTabs.switching && state.objectOrNull("new_drawing") == null
        }
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
            pressKey(KeyEvent.KEYCODE_TAB)
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
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to "builtin:workspace:illustrator")) }
            host.awaitFrame("Paint workspace") { host.workspaceManager?.optString("id") == "builtin:workspace:illustrator" && host.workspaceManager?.optBoolean("busy") == false }
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form", "action" to obj("type" to "reset", "value" to "builtin:workspace:illustrator"))) }
            host.awaitFrame("Paint starting layout confirmation") { host.workspaceManager?.objectOrNull("prompt") != null }
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "submit", "name" to "")) }
            host.awaitFrame("Paint starting layout restored") { host.workspaceManager?.objectOrNull("prompt") == null && host.workspaceManager?.optBoolean("busy") == false }
            compose.waitForIdle()
            assertNull("Starting layout confirmation is no longer presented", findTag("workspace-form"))
            host.drain(obj("type" to "invoke", "command" to "actual_pixels"))
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())

    }

}
