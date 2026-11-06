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
    @Test fun menu() = visit(false, "menu")

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
                report.put("family",if (name == "hidden") "frame" else family).put("state", if (admissionOnly) "rest" else name).put("status", "failed").put("passed", false)
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

    private fun predictiveBack(activity: MainActivity, cancel: Boolean) {
        val dispatcher = activity.onBackPressedDispatcher
        instrumentation.runOnMainSync { dispatcher.dispatchOnBackStarted(androidx.activity.BackEventCompat(0f,100f,0f,androidx.activity.BackEventCompat.EDGE_LEFT)) }
        compose.waitForIdle()
        instrumentation.runOnMainSync { dispatcher.dispatchOnBackProgressed(androidx.activity.BackEventCompat(80f,100f,.6f,androidx.activity.BackEventCompat.EDGE_LEFT)) }
        compose.waitForIdle()
        instrumentation.runOnMainSync { if (cancel) dispatcher.dispatchOnBackCancelled() else dispatcher.onBackPressed() }
        compose.waitForIdle()
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
