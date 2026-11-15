package art.capycanvas

import android.graphics.Bitmap
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.view.WindowInsets
import android.view.RoundedCorner
import android.view.MotionEvent
import android.view.KeyEvent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.ViewRootForTest
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasSetTextAction
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
    private var tolerateActionError = false
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    private fun CanvasHost.awaitFrame(label: String, timeout: Long = 10_000, diagnostics: () -> String = { "" }, condition: () -> Boolean) =
        awaitMain(label, timeout, diagnostics, compose, condition)

    private fun shell(command: String): String = ParcelFileDescriptor.AutoCloseInputStream(
        instrumentation.uiAutomation.executeShellCommand(command)).bufferedReader().use { it.readText().trim() }

    private fun textLayouts(tag: String): List<androidx.compose.ui.text.TextLayoutResult> {
        val layouts = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        instrumentation.runOnMainSync {
            fun collect(node: SemanticsNode) {
                node.config.getOrNull(SemanticsActions.GetTextLayoutResult)?.action?.invoke(layouts)
                node.children.forEach(::collect)
            }
            collect(checkNotNull(findTag(tag)).second)
        }
        assertTrue("$tag has measured text", layouts.isNotEmpty())
        return layouts
    }

    private fun assertHeaderSpacing(tag: String, cancel: String, primary: String) {
        textLayouts("$tag-title").forEach { assertEquals("The page title stays on one line", 1, it.lineCount) }
        instrumentation.runOnMainSync {
            val title = checkNotNull(findTag("$tag-title")).second.boundsInRoot
            val start = checkNotNull(findTag(cancel)).second.boundsInRoot
            val end = checkNotNull(findTag(primary)).second.boundsInRoot
            val gap = 6 * instrumentation.targetContext.resources.displayMetrics.density - 1
            assertTrue("The title keeps a gap after its start control", title.left >= start.right + gap)
            assertTrue("The title keeps a gap before its end control", title.right <= end.left - gap)
        }
    }

    private fun assertSettingRowInset(tag: String) {
        instrumentation.runOnMainSync {
            val row = checkNotNull(findTag(tag)).second
            val label = checkNotNull(findTag("$tag-label")).second
            val density = instrumentation.targetContext.resources.displayMetrics.density
            assertTrue("The row leaves space for its label", label.size.width > 0)
            assertEquals("The label uses the native row inset", row.positionInRoot.x + 14 * density, label.positionInRoot.x, 1f)
        }
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
        var expected = Rect.Zero
        var visible = JSONObject()
        host.awaitFrame("$tag uses its shared visible bounds",diagnostics = { "expected=$expected; actual=${findTag(tag)?.second?.boundsInRoot}; visible=$visible" }) {
            val blocked = (host.windowInsets ?: decor.rootWindowInsets).getInsets(WindowInsets.Type.ime() or WindowInsets.Type.navigationBars())
            visible = obj("x" to blocked.left/density,"y" to blocked.top/density,"width" to (decor.width-blocked.left-blocked.right).coerceAtLeast(0)/density,
                "height" to (decor.height-blocked.top-blocked.bottom).coerceAtLeast(0)/density)
            val layout = host.snapshot!!.getJSONObject("layout")
            val presentation = JSONObject(Native.dialogUi(obj("kind" to "presentation","role" to "editor","viewport" to layout.getJSONArray("viewport"),
                "insets" to host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets"),
                "work_area" to layout.getJSONObject("phone").getJSONObject("work_area"),"visible" to visible).toString()))
            assertEquals("IME preserves the shared page presentation","page",presentation.getString("kind"))
            expected = presentation.getJSONObject("bounds").rect()
            findTag(tag)?.second?.boundsInRoot?.let { actual ->
                kotlin.math.abs(actual.left-expected.left*density)<=1f && kotlin.math.abs(actual.top-expected.top*density)<=1f &&
                    kotlin.math.abs(actual.width-expected.width*density)<=1f && kotlin.math.abs(actual.height-expected.height*density)<=1f
            } == true
        }
    }

    @Test fun phoneDrawerFieldsAndPaletteResultsStayAboveTheKeyboard() {
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val original = activity.requestedOrientation
        val rotationFilter = InstrumentationRegistry.getArguments().getString("phoneRotation")
        require(rotationFilter == null || rotationFilter in listOf("portrait","landscape","reverse-landscape"))
        val animations = mutableSetOf<android.view.WindowInsetsAnimation>()
        fun guard() {
            instrumentation.runOnMainSync {
                assertTrue("Only the private focused app receives input",decor.isAttachedToWindow && decor.hasWindowFocus())
                assertEquals(instrumentation.targetContext.packageName,decor.context.packageName)
                assertFalse(instrumentation.targetContext.packageName in listOf("art.capycanvas","art.capycanvas.editor","art.capycanvas.capycanvas9"))
            }
        }
        fun bars() = jsonValue(host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONArray("bars"))
        fun tap(tag: String) {
            compose.waitForIdle()
            val (root,node) = checkNotNull(findTag(tag))
            assertSame("The drawer field belongs to the Activity window",decor,root.view.rootView)
            val origin = IntArray(2)
            instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val point = node.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
            assertTrue("The contact stays inside the private window",node.boundsInRoot.width>0 && node.boundsInRoot.height>0 && point.x>=0 && point.y>=0 && point.x<decor.width && point.y<decor.height)
            val down = SystemClock.uptimeMillis()
            for (phase in listOf(MotionEvent.ACTION_DOWN,MotionEvent.ACTION_UP)) {
                guard()
                val event = motion(MotionEvent.TOOL_TYPE_FINGER,phase,point,down)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event,true)) } finally { event.recycle() }
            }
            compose.waitForIdle()
        }
        fun settled(visible: Boolean) {
            host.awaitFrame("The native IME animation completes") { animations.isEmpty() && decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) == visible }
            compose.waitForIdle()
            val committed = java.util.concurrent.CountDownLatch(1)
            instrumentation.runOnMainSync { decor.viewTreeObserver.registerFrameCommitCallback { committed.countDown() };decor.invalidate() }
            assertTrue("The native keyboard frame commits",committed.await(5,java.util.concurrent.TimeUnit.SECONDS))
        }
        fun fullyVisible(tag: String) {
            var geometry = ""
            host.awaitFrame("$tag has its complete allocation above the keyboard",diagnostics={geometry}) {
                val node = findTag(tag)?.second ?: return@awaitFrame false
                val origin = IntArray(2)
                findTag(tag)!!.first.view.getLocationOnScreen(origin)
                val bounds = node.boundsInRoot.translate(Offset(origin[0].toFloat(),origin[1].toFloat()))
                val blocked = decor.rootWindowInsets.getInsets(WindowInsets.Type.ime() or WindowInsets.Type.navigationBars())
                val visible = Rect(blocked.left.toFloat(),blocked.top.toFloat(),(decor.width-blocked.right).toFloat(),(decor.height-blocked.bottom).toFloat())
                geometry = "$tag $bounds natural=${node.size} visible=$visible"
                node.size.width>0 && node.size.height>0 && kotlin.math.abs(bounds.width-node.size.width)<=1f && kotlin.math.abs(bounds.height-node.size.height)<=1f &&
                    bounds.left>=visible.left-1 && bounds.top>=visible.top-1 && bounds.right<=visible.right+1 && bounds.bottom<=visible.bottom+1
            }
        }
        fun finish(tag: String) {
            val view = checkNotNull(findTag(tag)).first.view
            guard()
            instrumentation.runOnMainSync { assertTrue(checkNotNull(view.onCreateInputConnection(android.view.inputmethod.EditorInfo())).performEditorAction(android.view.inputmethod.EditorInfo.IME_ACTION_DONE)) }
            settled(false)
            if(tag == "palette-search") {
                compose.onNodeWithTag("palette-chooser",useUnmergedTree=true).performScrollTo()
                tap("palette-chooser")
                host.awaitFrame("The palette chooser closes before the next case") { findTag("palette-search") == null }
            }
            host.drain(obj("type" to "customize","action" to obj("type" to "close_expanded")))
            host.awaitFrame("The drawer closes after Done") { findTag("tool-drawer") == null }
        }
        instrumentation.runOnMainSync {
            decor.setWindowInsetsAnimationCallback(object: android.view.WindowInsetsAnimation.Callback(android.view.WindowInsetsAnimation.Callback.DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
                override fun onPrepare(animation: android.view.WindowInsetsAnimation) { if(animation.typeMask and WindowInsets.Type.ime()!=0)animations.add(animation) }
                override fun onProgress(insets: WindowInsets,running: MutableList<android.view.WindowInsetsAnimation>): WindowInsets {
                    animations.addAll(running.filter { it.typeMask and WindowInsets.Type.ime()!=0 });return insets
                }
                override fun onEnd(animation: android.view.WindowInsetsAnimation) { animations.remove(animation) }
            })
        }
        try {
            host.awaitFrame("The launch cover releases the private canvas") { findTag("launch-splash") == null }
            for((name,orientation,rotation) in listOf(Triple("portrait",android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,0),
                Triple("landscape",android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,1),
                Triple("reverse-landscape",android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE,3))) {
                if(rotationFilter != null && rotationFilter != name)continue
                instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
                host.awaitFrame("$name uses its native viewport and sensor insets") {
                    val cutout = decor.rootWindowInsets.displayCutout
                    val sideReady = cutout == null || when(rotation) { 1 -> cutout.safeInsetLeft>0;3 -> cutout.safeInsetRight>0;else -> cutout.safeInsetTop>0 }
                    val native = decor.rootWindowInsets.getInsets(WindowInsets.Type.displayCutout())
                    val waterfall = cutout?.waterfallInsets ?: android.graphics.Insets.NONE
                    val expected = listOf(maxOf(native.left,waterfall.left),maxOf(native.top,waterfall.top),maxOf(native.right,waterfall.right),maxOf(native.bottom,waterfall.bottom))
                    val reported = host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets")
                    val viewport=host.snapshot!!.getJSONObject("layout").getJSONArray("viewport")
                    val density=activity.resources.displayMetrics.density
                    activity.display!!.rotation == rotation && (decor.width>decor.height) == (rotation!=0) && sideReady &&
                        kotlin.math.abs(viewport.getDouble(0)-decor.width/density)<.01 && kotlin.math.abs(viewport.getDouble(1)-decor.height/density)<.01 &&
                        expected.indices.all { kotlin.math.abs(reported.getDouble(it)-expected[it]/activity.resources.displayMetrics.density)<.001 }
                }
                android.app.UiAutomation::class.java.getMethod("syncInputTransactions").invoke(instrumentation.uiAutomation)
                for(theme in listOf("light","dark")) {
                    host.drain(obj("type" to "set_theme","theme" to theme))
                    val color = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").array("phone").objects()
                        .single { it.getJSONObject("control").getString("kind") == "color" }.getInt("id")
                    tap("phone-tool-$color")
                    host.awaitFrame("Color and Palettes are presented",diagnostics={
                        val dock=host.workspaceInteraction
                        "drawer=${host.snapshot?.getJSONObject("state")?.getJSONObject("customization")?.opt("drawer")}; enabled=${dock?.enabled}; focused=${dock?.focused}; popup=${dock?.popupOpen}; phone=${dock?.phone?.preview}; windowFocus=${decor.hasWindowFocus()}"
                    }) { findTag("palette-chooser") != null }
                    compose.onNodeWithTag("palette-chooser",useUnmergedTree=true).performScrollTo()
                    tap("palette-chooser")
                    host.awaitFrame("Palette search is presented") { findTag("palette-search") != null }
                    val palette = host.palettes.view()!!.array("palettes").objects().first { it.getString("name").isNotEmpty() }
                    val before = bars()
                    compose.onNodeWithTag("palette-search",useUnmergedTree=true).performScrollTo()
                    tap("palette-search")
                    settled(true)
                    fullyVisible("palette-search")
                    fullyVisible("palette-choice-${palette.getLong("id")}")
                    assertEquals("Keyboard placement keeps both bars fixed",before,bars())
                    assertNotNull(findTag("tool-drawer"))
                    guard();screenshot("fix4d-$name-$theme-palette.png")
                    finish("palette-search")
                    host.drain(obj("type" to "invoke","command" to "drawing_brush"))
                    tap("phone-tool-settings")
                    host.awaitFrame("Brush settings are presented") { findTag("number-value-size") != null }
                    val field = host.snapshot!!.getJSONObject("state").array("tool_settings").objects().single { it.getString("id") == "size" }
                    val value = field.getDouble("value")
                    val sizeBars = bars()
                    compose.onNodeWithTag("number-value-size",useUnmergedTree=true).performScrollTo()
                    tap("number-value-size")
                    val tag = "number-${field.getString("label")}"
                    host.awaitFrame("Brush Size is editable") { findTag(tag) != null }
                    settled(true)
                    fullyVisible(tag)
                    assertEquals("Keyboard placement keeps both bars fixed",sizeBars,bars())
                    guard();screenshot("fix4d-$name-$theme-size.png")
                    finish(tag)
                    assertEquals(value,host.snapshot!!.getJSONObject("state").array("tool_settings").objects().single { it.getString("id")=="size" }.getDouble("value"),.0001)
                }
            }
        } finally {
            instrumentation.runOnMainSync { decor.setWindowInsetsAnimationCallback(null);activity.requestedOrientation=original }
            scenario.close()
        }
    }

    @Test fun phoneCapyHoldKeepsItsDropAcrossKeyboardMetrics() {
        val scenario=launchCapy(180_000,compose)
        val activity=scenario.activity()
        val host=activity.host
        val decor=activity.window.decorView
        fun phone()=host.snapshot!!.getJSONObject("state").getJSONObject("phone")
        fun event(phase: Int,point: Offset,down: Long) {
            instrumentation.runOnMainSync {
                assertTrue("Only the private focused window receives the Capy contact",decor.isAttachedToWindow && decor.hasWindowFocus())
                assertEquals(instrumentation.targetContext.packageName,decor.context.packageName)
                assertFalse(instrumentation.targetContext.packageName in listOf("art.capycanvas","art.capycanvas.editor","art.capycanvas.capycanvas9"))
            }
            val input=motion(MotionEvent.TOOL_TYPE_FINGER,phase,point,down)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(input,true)) } finally { input.recycle() }
        }
        try {
            host.awaitFrame("The private canvas releases launch input") { findTag("launch-splash")==null }
            host.awaitFrame("The phone viewport and hidden keyboard match the native window",diagnostics={"layout=${host.snapshot?.opt("layout")}; keyboard=${phone().opt("keyboard")}"}) {
                val viewport=host.snapshot!!.getJSONObject("layout").getJSONArray("viewport")
                val density=activity.resources.displayMetrics.density
                !decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) && phone().getDouble("keyboard")==0.0 &&
                    kotlin.math.abs(viewport.getDouble(0)-decor.width/density)<.01 && kotlin.math.abs(viewport.getDouble(1)-decor.height/density)<.01
            }
            compose.waitForIdle()
            val (root,node)=checkNotNull(findTag("phone-capy"))
            val origin=IntArray(2)
            instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val start=node.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
            assertTrue("The Capy contact is inside the private window",start.x>=0 && start.y>=0 && start.x<decor.width && start.y<decor.height)
            assertTrue("The contact targets Capy's shared hit bounds",host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONObject("capy_target").rect().contains(start/activity.resources.displayMetrics.density))
            val down=SystemClock.uptimeMillis()
            event(MotionEvent.ACTION_DOWN,start,down)
            host.awaitFrame("Capy is held before keyboard metrics change",diagnostics={
                val input=host.workspaceInteraction?.phone
                "capy=${input?.capy}; held=${input?.held}; down=${input?.pointerDown}; enabled=${host.workspaceInteraction?.enabled}; focus=${decor.hasWindowFocus()}; keyboard=${phone().opt("keyboard")}; target=${input?.layout?.opt("capy_target")}; point=$start"
            }) { host.workspaceInteraction?.phone?.let { it.capy && it.held } == true }
            val before=JSONObject(phone().toString())
            val target=host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONObject("capy_target").toString()
            instrumentation.runOnMainSync { host.reportPhoneMetrics(before.getJSONArray("insets"),before.getJSONArray("corners"),120f) }
            host.awaitFrame("The keyboard metric publishes during the Capy hold") { phone().getDouble("keyboard")==120.0 }
            compose.waitForIdle()
            host.drain()
            assertTrue("Keyboard metrics keep the original Capy hold",host.workspaceInteraction?.phone?.let { it.capy && it.held } == true)
            assertEquals(target,host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONObject("capy_target").toString())
            val bar=host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONArray("bars").getJSONObject(0).rect()
            val point=bar.center*activity.resources.displayMetrics.density
            event(MotionEvent.ACTION_MOVE,point,down)
            event(MotionEvent.ACTION_UP,point,down)
            host.awaitFrame("The original held Capy drop swaps sides") { !phone().getBoolean("capy_drag") && phone().getBoolean("swapped") != before.getBoolean("swapped") }
        } finally {
            scenario.close()
        }
    }

    @Test fun phoneHeldMenusPreserveTouchPenAndMouseContacts() {
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val original = activity.requestedOrientation
        fun tiles() = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").array("phone").objects()
        fun point(tile: Int): Offset {
            compose.waitForIdle()
            val (root,node) = checkNotNull(findTag("phone-tool-$tile"))
            val origin = IntArray(2)
            instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            return node.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
        }
        fun event(tool: Int,phase: Int,at: Offset,down: Long) {
            val input = motion(tool,phase,at,down)
            try { assertTrue(instrumentation.uiAutomation.injectInputEvent(input,true)) } finally { input.recycle() }
        }
        fun hold(tool: Int,at: Offset): Long {
            val down = SystemClock.uptimeMillis()
            event(tool,MotionEvent.ACTION_DOWN,at,down)
            host.awaitFrame("The original contact opens its held menu") {
                host.workspaceInteraction?.phone?.held == true && host.workspaceInteraction?.contactHeld == true &&
                    (tool == MotionEvent.TOOL_TYPE_MOUSE || findTag("workspace-menu")?.second?.let { it.layoutInfo.isPlaced && it.size.width>0 && it.size.height>0 } == true)
            }
            compose.waitForIdle()
            if(tool != MotionEvent.TOOL_TYPE_MOUSE) {
                val committed = java.util.concurrent.CountDownLatch(1)
                instrumentation.runOnMainSync {
                    val view = checkNotNull(findTag("workspace-menu")).first.view
                    view.viewTreeObserver.registerFrameCommitCallback { committed.countDown() }
                    view.invalidate()
                }
                assertTrue("The held menu commits its own window frame before Up",committed.await(5,java.util.concurrent.TimeUnit.SECONDS))
            }
            return down
        }
        fun closeMenu() {
            instrumentation.runOnMainSync { host.workspaceInteraction!!.closeContext() }
            host.awaitFrame("The editing menu closes") { findTag("workspace-menu") == null && activity.hasWindowFocus() }
        }
        try {
            host.awaitFrame("The launch cover releases input") { findTag("launch-splash") == null }
            for((name,orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
                val rotation = when(orientation) { 0 -> 1;8 -> 3;else -> 0 }
                host.awaitFrame("$name held-menu viewport") { activity.display!!.rotation == rotation && (decor.width>decor.height) == (rotation != 0) }
                for(theme in listOf("light","dark")) {
                    host.drain(obj("type" to "set_theme","theme" to theme))
                    val ids = tiles().map { it.getInt("id") }
                    val at = point(ids[0])
                    for((device,tool) in listOf("touch" to MotionEvent.TOOL_TYPE_FINGER,"pen" to MotionEvent.TOOL_TYPE_STYLUS,"mouse" to MotionEvent.TOOL_TYPE_MOUSE)) {
                        val down = hold(tool,at)
                        if(tool == MotionEvent.TOOL_TYPE_MOUSE)assertNull("A mouse hold only arms pickup",findTag("workspace-menu"))
                        screenshot("fix2-$name-$theme-$device-held.png")
                        event(tool,MotionEvent.ACTION_UP,at,down)
                        host.awaitFrame("Up retires only contact ownership") { host.workspaceInteraction?.contactHeld == false }
                        if(tool != MotionEvent.TOOL_TYPE_MOUSE) {
                            assertNotNull("Up leaves the held menu open",findTag("workspace-menu"))
                            closeMenu()
                        } else assertNull(findTag("workspace-menu"))
                        assertEquals("Hold and release cannot activate a row",ids,tiles().map { it.getInt("id") })
                    }
                    val target = point(ids[2])+if(decor.width>decor.height)Offset(0f,1f) else Offset(1f,0f)
                    val down = hold(MotionEvent.TOOL_TYPE_FINGER,at)
                    event(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.ACTION_MOVE,target,down)
                    host.awaitFrame("Dragging closes the menu and keeps the same contact") {
                        findTag("workspace-menu") == null && host.workspaceInteraction?.phone?.preview?.objectOrNull("outcome")?.optString("kind") == "dragging"
                    }
                    screenshot("fix2-$name-$theme-dragging.png")
                    event(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.ACTION_UP,target,down)
                    host.awaitFrame("The same contact reorders once") { tiles()[2].getInt("id") == ids[0] }
                    host.drain(obj("type" to "invoke","command" to "undo_workspace"))
                    host.awaitFrame("Undo restores the toolbar") { tiles().map { it.getInt("id") } == ids }
                    val cancelled = hold(MotionEvent.TOOL_TYPE_FINGER,point(ids[0]))
                    pressKey(KeyEvent.KEYCODE_ESCAPE)
                    host.awaitFrame("Escape closes the menu and cancels the contact") { findTag("workspace-menu") == null && host.workspaceInteraction?.phone?.contact == null }
                    event(MotionEvent.TOOL_TYPE_FINGER,MotionEvent.ACTION_UP,at,cancelled)
                    assertEquals("Cancelled release changes nothing",ids,tiles().map { it.getInt("id") })
                }
            }
        } finally {
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun pickerSearchFollowsNativeInputMode() {
        val scenario = launchCapy(180_000, compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val original = activity.requestedOrientation
        fun touchWorkspace() {
            compose.waitForIdle()
            val (root, node) = checkNotNull(findTag("workspace"))
            val origin = IntArray(2)
            instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val point = node.boundsInRoot.center+Offset(origin[0].toFloat(), origin[1].toFloat())
            val down = SystemClock.uptimeMillis()
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val event = motion(MotionEvent.TOOL_TYPE_FINGER, action, point, down)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
            }
            host.drain()
            assertTrue("A real touch selects native touch mode", root.view.isInTouchMode)
        }
        fun openPicker() {
            host.drain(obj("type" to "phone", "action" to obj("kind" to "insert_tools", "before" to null)))
            host.awaitFrame("Add Tools opens") { findTag("tool-picker-search") != null }
            compose.waitForIdle()
        }
        fun focused() = findTag("tool-picker-search")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null
        fun closePicker() {
            host.drain(obj("type" to "customize", "action" to obj("type" to "cancel_tools")))
            host.awaitFrame("The picker and keyboard close") { findTag("tool-picker") == null && !decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
        }
        try {
            host.awaitFrame("The launch cover no longer owns input") { findTag("launch-splash") == null }
            for ((name, orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                requestPhoneOrientation(activity,host,orientation,"$name input viewport")
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    touchWorkspace()
                    openPicker()
                    awaitEditorPage(activity, host, "tool-picker")
                    assertFalse("Touch opening does not focus search", focused())
                    assertFalse("Touch opening does not raise the keyboard", decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()))
                    capturePhonePage("tool-picker","fix4b-$name-$theme-touch.png")
                    closePicker()
                    pressKey(KeyEvent.KEYCODE_DPAD_RIGHT)
                    compose.waitForIdle()
                    assertFalse("A native key selects keyboard mode", checkNotNull(findTag("workspace")).first.view.isInTouchMode)
                    openPicker()
                    host.awaitFrame("Keyboard opening focuses search") { focused() }
                    awaitEditorPage(activity, host, "tool-picker")
                    capturePhonePage("tool-picker","fix4b-$name-$theme-keyboard.png")
                    closePicker()
                }
            }
        } finally {
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun explicitlyOpenedCommandSearchFocusesAfterTouch() {
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        fun touch(tag: String) {
            compose.waitForIdle()
            val (root,node) = checkNotNull(findTag(tag))
            val origin = IntArray(2)
            instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val point = node.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
            val down = SystemClock.uptimeMillis()
            for(action in listOf(MotionEvent.ACTION_DOWN,MotionEvent.ACTION_UP)) {
                val event = motion(MotionEvent.TOOL_TYPE_FINGER,action,point,down)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event,true)) } finally { event.recycle() }
            }
            compose.waitForIdle()
            assertTrue("The search is requested by touch",root.view.isInTouchMode)
        }
        fun focusedWithKeyboard(tag: String) = findTag(tag)?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null &&
            decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        try {
            host.awaitFrame("The launch cover releases input") { findTag("launch-splash") == null }
            host.drain(obj("type" to "set_theme","theme" to "light"))
            touch("phone-capy")
            host.awaitFrame("Search Commands is available") { findTag("phone-search") != null }
            touch("phone-search")
            host.awaitFrame("Requested Command Search focuses with IME") { focusedWithKeyboard("command-search") }
            screenshot("explicit-command-search.png")
            pressKey(KeyEvent.KEYCODE_BACK)
            host.awaitFrame("Back dismisses the Command Search keyboard") { !decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
            compose.onNodeWithTag("command-close",useUnmergedTree = true).performClick()
            host.awaitFrame("Command Search closes") { findTag("command-search") == null }
        } finally { scenario.close() }
    }

    @Test fun pickerKeepsOneCompleteResultAboveTheKeyboard() {
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val originalOrientation = activity.requestedOrientation
        val density = activity.resources.displayMetrics.density
        try {
            for ((name,orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
                val rotation = when(orientation) { 0 -> 1;8 -> 3;else -> 0 }
                host.awaitFrame("$name picker viewport") { activity.display!!.rotation == rotation && (decor.width>decor.height) == (rotation != 0) }
                for(theme in listOf("light","dark")) {
                    host.drain(obj("type" to "set_theme","theme" to theme))
                    host.drain(obj("type" to "phone","action" to obj("kind" to "insert_tools","before" to null)))
                    host.awaitFrame("The picker search opens") { findTag("tool-picker-search") != null }
                    compose.onNodeWithTag("tool-picker-search",useUnmergedTree = true).performClick()
                    host.awaitFrame("The real keyboard is visible") { decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
                    fun checkResults(mode: String) {
                        awaitEditorPage(activity,host,"tool-picker")
                        val insets = (host.windowInsets ?: decor.rootWindowInsets).getInsets(WindowInsets.Type.navigationBars() or WindowInsets.Type.ime())
                        val list = checkNotNull(findTag("tool-picker-list")).second.boundsInRoot
                        val label = host.snapshot!!.getJSONObject("picker").array("choices").objects().first().getString("label")
                        val first = checkNotNull(findTag("tool-picker-choice-$label")).second.boundsInRoot
                        assertTrue("$mode list has a complete row: $list / $first",first.height>=58*density-1 && first.top>=list.top-1 && first.bottom<=list.bottom+1)
                        for(tag in listOf("tool-picker-search","tool-picker-cancel","tool-picker-confirm")) {
                            compose.onNodeWithTag(tag,useUnmergedTree = true).assertIsDisplayed()
                            val bounds = checkNotNull(findTag(tag)).second.boundsInRoot
                            assertTrue("$mode $tag clears visible system UI: $bounds vs $insets",bounds.left>=insets.left-1 && bounds.right<=decor.width-insets.right+1 && bounds.bottom<=decor.height-insets.bottom+1)
                        }
                        if(rotation != 0)compose.onNodeWithTag("tool-picker-count",useUnmergedTree = true).assertIsNotDisplayed()
                        screenshot("fix4c-$name-$theme-$mode.png")
                    }
                    checkResults("native")
                    if(rotation != 0) {
                        val view = checkNotNull(decor.descendant<CanvasContent>())
                        val original = decor.rootWindowInsets
                        try {
                            instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(original)
                                .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.of(0,0,0,579)).setVisible(WindowInsets.Type.ime(),true).build()) }
                            host.awaitFrame("The measured Note10 inset is applied") { host.windowInsets?.getInsets(WindowInsets.Type.ime())?.bottom == 579 }
                            checkResults("injected")
                        } finally { instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(original) } }
                    }
                    host.drain(obj("type" to "customize","action" to obj("type" to "cancel_tools")))
                    host.awaitFrame("The picker closes") { findTag("tool-picker") == null }
                }
            }
        } finally {
            instrumentation.runOnMainSync { activity.requestedOrientation = originalOrientation }
            scenario.close()
        }
    }

    @Test fun doneInPhonePagesPreservesDraftsAndValidation() {
        val scenario = launchCapy(180_000, compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val original = activity.requestedOrientation
        fun entry(tag: String) = compose.onNode(hasSetTextAction() and (hasTestTag(tag) or hasAnyAncestor(hasTestTag(tag))), useUnmergedTree = true)
        fun keyboard() = decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        fun done(tag: String, value: String, page: String = "tool-picker") {
            entry(tag).performClick().performTextReplacement(value)
            host.awaitFrame("$tag has its native keyboard") { keyboard() }
            entry(tag).performImeAction()
            host.awaitFrame("$tag Done ends text entry") { !keyboard() && !host.editingText }
            awaitEditorPage(activity, host, page)
        }
        fun cancelPicker() {
            host.drain(obj("type" to "customize", "action" to obj("type" to "cancel_tools")))
            host.awaitFrame("The picker closes") { findTag("tool-picker") == null }
        }
        try {
            for ((name, orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                requestPhoneOrientation(activity,host,orientation,"$name phone page viewport")
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    host.drain(obj("type" to "phone", "action" to obj("kind" to "insert_tools", "before" to null)))
                    host.awaitFrame("Add Tools search opens") { findTag("tool-picker-search") != null }
                    done("tool-picker-search", "pen")
                    host.awaitFrame("Done preserves the search and page") { host.snapshot?.objectOrNull("picker")?.optString("query") == "pen" && findTag("tool-picker") != null }
                    capturePhonePage("tool-picker","fix4a-$name-$theme-search-done.png")
                    cancelPicker()
                    host.drain(obj("type" to "customize", "action" to obj("type" to "new_toolbar", "group" to JSONObject.NULL)))
                    host.awaitFrame("The toolbar name opens") { findTag("toolbar-name") != null }
                    done("toolbar-name", "Keyboard draft")
                    host.awaitFrame("Done preserves the unconfirmed name and page") { host.snapshot?.objectOrNull("picker")?.optString("name") == "Keyboard draft" && findTag("tool-picker") != null }
                    capturePhonePage("tool-picker","fix4a-$name-$theme-name-done.png")
                    cancelPicker()
                    host.drain(obj("type" to "invoke", "command" to "new_document"))
                    host.awaitFrame("New Drawing opens") { findTag("new-drawing-width") != null }
                    host.drain(obj("type" to "new_drawing", "action" to obj("op" to "unit", "unit" to "px")))
                    entry("new-drawing-width").performClick().performTextReplacement("bad")
                    host.awaitFrame("The numeric keyboard opens") { keyboard() }
                    entry("new-drawing-width").performImeAction()
                    assertTrue("Invalid numeric Done retains text entry", keyboard() && host.editingText)
                    assertNotNull("Invalid Done preserves New Drawing", findTag("new-drawing-width"))
                    done("new-drawing-width", "64", "new-drawing-dialog")
                    assertNotNull("Valid Done keeps New Drawing open", findTag("new-drawing-width"))
                    host.awaitFrame("Done commits the numeric value") { host.snapshot!!.getJSONObject("state").getJSONObject("new_drawing").getJSONArray("values").getDouble(0) == 64.0 }
                    capturePhonePage("new-drawing-dialog","fix4a-$name-$theme-number-done.png")
                    compose.onNodeWithTag("new-drawing-cancel", useUnmergedTree = true).performClick()
                    host.awaitFrame("New Drawing cancels") { findTag("new-drawing-width") == null }
                }
            }
        } finally {
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun phoneSafeInsetsIgnoreHiddenGestureStrips() {
        val scenario = launchCapy(180_000, compose)
        var content: CanvasContent? = null
        var original: WindowInsets? = null
        try {
            val host = scenario.activity().host
            instrumentation.runOnMainSync {
                content = checkNotNull(semanticsRoots().first().view.rootView.descendant<CanvasContent>())
                original = content!!.rootWindowInsets
            }
            val insets = checkNotNull(original)
            val cutout = insets.getInsets(WindowInsets.Type.displayCutout())
            val waterfall = insets.displayCutout?.waterfallInsets ?: android.graphics.Insets.NONE
            val density = instrumentation.targetContext.resources.displayMetrics.density
            val expected = listOf(maxOf(cutout.left, waterfall.left), maxOf(cutout.top, waterfall.top),
                maxOf(cutout.right, waterfall.right), maxOf(cutout.bottom, waterfall.bottom)).map { it/density }
            val before = jsonValue(host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONArray("bars"))
            val injected = WindowInsets.Builder(insets)
                .setInsets(WindowInsets.Type.mandatorySystemGestures(), android.graphics.Insets.of(80, 120, 160, 200))
                .setInsets(WindowInsets.Type.navigationBars() or WindowInsets.Type.statusBars(), android.graphics.Insets.NONE)
                .setVisible(WindowInsets.Type.navigationBars() or WindowInsets.Type.statusBars(), false).build()
            instrumentation.runOnMainSync { content!!.dispatchApplyWindowInsets(injected) }
            compose.waitForIdle()
            host.drain()
            val reported = host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets")
            expected.forEachIndexed { i, value -> assertEquals("Only cutout and waterfall belong to side $i", value.toDouble(), reported.getDouble(i), .01) }
            assertEquals("Hidden gesture strips cannot move either phone bar", before,
                jsonValue(host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONArray("bars")))
        } finally {
            instrumentation.runOnMainSync { original?.let { content?.dispatchApplyWindowInsets(it) } }
            scenario.close()
        }
    }

    @Test fun immersivePhoneWindowsKeepBarsAtSensorEdges() {
        val scenario = launchCapy(180_000, compose)
        val activity = scenario.activity()
        val host = activity.host
        val decor = activity.window.decorView
        val original = activity.requestedOrientation
        fun bars() = jsonValue(host.snapshot!!.getJSONObject("layout").getJSONObject("phone").getJSONArray("bars"))
        fun hidden(before: Any?, name: String) {
            compose.waitForIdle()
            assertFalse("$name keeps navigation hidden", decor.rootWindowInsets.isVisible(WindowInsets.Type.navigationBars()))
            assertEquals("$name cannot move the phone bars", before, bars())
        }
        try {
            for ((name, orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
                val rotation = when (orientation) { 0 -> 1; 8 -> 3; else -> 0 }
                host.awaitFrame("$name phone viewport") { activity.display!!.rotation == rotation &&
                    (decor.width > decor.height) == (rotation != 0) &&
                    host.snapshot!!.getJSONObject("layout").getJSONArray("viewport").let {
                        kotlin.math.abs(it.getDouble(0)*activity.resources.displayMetrics.density-decor.width)<1 &&
                            kotlin.math.abs(it.getDouble(1)*activity.resources.displayMetrics.density-decor.height)<1 } }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    val before = bars()
                    val tile = host.snapshot!!.getJSONObject("state").getJSONObject("workspace").getJSONObject("layout").getJSONArray("phone").getJSONObject(0).getInt("id")
                    host.awaitFrame("Phone tile anchor") { findTag("phone-tool-$tile") != null }
                    instrumentation.runOnMainSync { host.workspaceInteraction!!.context(obj("kind" to "phone_tile", "tile" to tile)) }
                    host.awaitFrame("Native tile menu opens") { findTag("workspace-menu") != null }
                    hidden(before, "$name tile menu")
                    screenshot("fix1-$name-$theme-tile-menu.png")
                    instrumentation.runOnMainSync { host.workspaceInteraction!!.closeContext() }
                    host.awaitFrame("Tile menu closes") { findTag("workspace-menu") == null }
                    host.drain(obj("type" to "open_settings", "page" to "appearance"))
                    host.awaitFrame("Preferences page opens") { findTag("setting-choice-theme") != null }
                    hidden(before, "$name page")
                    screenshot("fix1-$name-$theme-page.png")
                    compose.onNodeWithTag("setting-choice-theme", useUnmergedTree = true).performScrollTo().performClick()
                    host.awaitFrame("Native theme popup opens") { findTag("setting-choice-menu-theme") != null }
                    hidden(before, "$name theme popup")
                    screenshot("fix1-$name-$theme-theme-popup.png")
                    pressKey(KeyEvent.KEYCODE_ESCAPE)
                    host.awaitFrame("Theme popup closes") { findTag("setting-choice-menu-theme") == null }
                    host.drain(obj("type" to "close_settings"))
                    host.awaitFrame("Preferences page closes") { findTag("preferences-dialog") == null }
                    host.drain(obj("type" to "phone", "action" to obj("kind" to "insert_tools", "before" to null)))
                    host.awaitFrame("Keyboard page opens") { findTag("tool-picker-search") != null }
                    compose.onNodeWithTag("tool-picker-search", useUnmergedTree = true).performClick()
                    host.awaitFrame("Native keyboard is visible") { decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
                    awaitEditorPage(activity, host, "tool-picker")
                    compose.onNodeWithTag("tool-picker-confirm", useUnmergedTree = true).assertIsDisplayed()
                    val primary = checkNotNull(findTag("tool-picker-confirm")).second.boundsInRoot
                    val blocked = decor.rootWindowInsets.getInsets(WindowInsets.Type.navigationBars() or WindowInsets.Type.ime())
                    assertTrue("The primary action clears visible navigation and IME: $primary vs $blocked",
                        primary.left>=blocked.left-1 && primary.top>=blocked.top-1 &&
                            primary.right<=decor.width-blocked.right+1 && primary.bottom<=decor.height-blocked.bottom+1)
                    assertEquals("IME cannot move phone bars", before, bars())
                    screenshot("fix1-$name-$theme-keyboard.png")
                    instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
                    host.awaitFrame("The keyboard closes") { !decor.rootWindowInsets.isVisible(WindowInsets.Type.ime()) }
                    instrumentation.runOnMainSync { host.customize(obj("type" to "cancel_tools")) }
                    host.awaitFrame("Keyboard page closes") { findTag("tool-picker") == null }
                }
            }
        } finally {
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun calloutRowsClearEitherArrow() {
        val scenario = launchCapy(180_000, compose)
        val activity = scenario.activity()
        val host = activity.host
        val original = activity.requestedOrientation
        val anchor = mutableStateOf<Rect?>(null)
        var view: ComposeView? = null
        try {
            compose.runOnIdle {
                view = ComposeView(activity).also { mounted ->
                    activity.addContentView(mounted, android.widget.FrameLayout.LayoutParams(-1, -1))
                    mounted.setContent {
                        val state = host.snapshot!!.getJSONObject("state")
                        val palette = Palette(state.getString("theme") == "dark", state.getJSONObject("palette"))
                        CompositionLocalProvider(LocalCanvasHost provides host, LocalPalette provides palette,
                            androidx.compose.material3.LocalContentColor provides palette.text) {
                            MaterialTheme(colorScheme = if (state.getString("theme") == "dark") androidx.compose.material3.darkColorScheme() else androidx.compose.material3.lightColorScheme()) {
                                Box(Modifier.fillMaxSize()) {
                                    anchor.value?.let { target ->
                                        WorkspaceMenu(host, obj("sections" to JSONArray().put(JSONArray().put(obj("label" to "First")).put(obj("label" to "Last")))),
                                            showTitle = false, presentation = obj("has_arrow" to true), anchor = target, dismiss = { anchor.value = null })
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for ((name, orientation) in listOf("portrait" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT,
                "landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,
                "reverse-landscape" to android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE)) {
                instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
                val rotation = when (orientation) { 0 -> 1; 8 -> 3; else -> 0 }
                host.awaitFrame("$name rotation and layout") { activity.display!!.rotation == rotation &&
                    view!!.width == activity.window.decorView.width && view!!.height == activity.window.decorView.height &&
                    (view!!.width > view!!.height) == (rotation != 0) }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (below in listOf(true, false)) {
                        compose.runOnIdle {
                            val location = IntArray(2)
                            view!!.getLocationOnScreen(location)
                            val y = view!!.height * if (below) .25f else .75f
                            anchor.value = Rect(location[0]+view!!.width*.5f, location[1]+y, location[0]+view!!.width*.5f+1, location[1]+y+1)
                        }
                        compose.waitForIdle()
                        val menu = compose.onNodeWithTag("workspace-menu").assertIsDisplayed().fetchSemanticsNode().boundsInRoot
                        val first = compose.onNodeWithText("First").fetchSemanticsNode().boundsInRoot
                        val last = compose.onNodeWithText("Last").fetchSemanticsNode().boundsInRoot
                        val density = activity.resources.displayMetrics.density
                        val above = first.top-menu.top-if (below) 8*density else 0f
                        val under = menu.bottom-last.bottom-if (below) 0f else 8*density
                        screenshot("fix3-$name-$theme-${if (below) "below" else "above"}.png")
                        assertEquals("$name $theme below=$below: rounded body gaps; menu=$menu first=$first last=$last", above, under, density)
                        assertEquals("The rounded body retains 8dp padding", 8*density, above, density)
                        compose.runOnIdle { anchor.value = null }
                        compose.waitForIdle()
                    }
                }
            }
        } finally {
            instrumentation.runOnMainSync {
                view?.let { it.disposeComposition(); (it.parent as android.view.ViewGroup).removeView(it) }
                activity.requestedOrientation = original
            }
            scenario.close()
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

    @Test fun alertBodiesCanSubcomposeWithoutIntrinsicMeasurements() {
        val scenario = launchCapy(180_000, compose)
        var view: ComposeView? = null
        try {
            val activity = scenario.activity()
            val host = activity.host
            val state = host.snapshot!!.getJSONObject("state")
            val palette = Palette(state.getString("theme") == "dark", state.getJSONObject("palette"))
            val dialogs = mutableStateListOf<@Composable () -> Unit>()
            val copy = host.catalog.getJSONObject("document_color_copy")
            val cancel = host.bootstrap!!.getJSONObject("common").getString("cancel")
            compose.runOnIdle {
                view = ComposeView(activity).also { mounted ->
                    activity.addContentView(mounted, android.widget.FrameLayout.LayoutParams(-1, -1))
                    mounted.setContent {
                        CompositionLocalProvider(LocalCanvasHost provides host, LocalPalette provides palette, LocalDialogLayer provides dialogs) {
                            MaterialTheme {
                                Box(Modifier.fillMaxSize()) {
                                    DialogShell(host, host.bootstrap!!.getJSONObject("progress_dialog"), copy.getString("applying"),
                                        tag = "subcompose-alert", onCancel = {},
                                        responses = listOf(DialogResponse(cancel, "subcompose-alert-cancel", action = {})),
                                        body = { BoxWithConstraints(Modifier.testTag("subcompose-alert-body")) { Text(copy.getString("compare_before_apply"), Modifier.widthIn(max = maxWidth)) } })
                                    WorkspaceDialogs()
                                }
                            }
                        }
                    }
                }
            }
            host.awaitMain("The subcomposing alert body is composed", 30_000, diagnostics = { "" }, compose = compose) { findTag("subcompose-alert-body", view!!.descendant<ViewRootForTest>()) != null }
            val body = compose.onNodeWithTag("subcompose-alert-body").assertIsDisplayed().fetchSemanticsNode().boundsInRoot
            val response = compose.onNodeWithTag("subcompose-alert-cancel").assertIsDisplayed().fetchSemanticsNode().boundsInRoot
            val card = compose.onNodeWithTag("subcompose-alert").assertIsDisplayed().fetchSemanticsNode().boundsInRoot
            assertTrue("The alert contains its body", card.left <= body.left && card.top <= body.top && card.right >= body.right && card.bottom >= body.bottom)
            assertTrue("The alert contains its response", card.left <= response.left && card.top <= response.top && card.right >= response.right && card.bottom >= response.bottom)
            assertTrue("The response follows the body", response.top >= body.bottom)
            assertNull(host.failure)
        } catch (error: Throwable) {
            runCatching { screenshot("b6-subcompose-alert-failure.png") }
            throw error
        } finally {
            instrumentation.runOnMainSync { view?.let { it.disposeComposition(); (it.parent as android.view.ViewGroup).removeView(it) } }
            scenario.close()
        }
    }

    @Test fun bitDepthSuggestsAChangeAndKeepsBandingVisible() {
        val scenario = launchCapy(180_000, compose)
        try {
            val host = scenario.activity().host
            assertNotNull(host.snapshot!!.getJSONObject("layout").objectOrNull("phone"))
            val copy = host.catalog.getJSONObject("document_color_copy")
            val depthLabel = host.catalog.getJSONObject("export_copy").getString("depth")
            for (theme in listOf("light", "dark")) {
                host.drain(obj("type" to "set_theme", "theme" to theme))
                for ((current, suggested) in listOf("U16" to "U8", "U8" to "U16")) {
                    val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
                    host.drain(obj("type" to "invoke", "command" to "new_document"))
                    host.awaitMain("New Drawing opens", 30_000, diagnostics = { "" }, compose = compose) { findTag("new-drawing-create") != null }
                    for (action in listOf(obj("op" to "unit", "unit" to "px"), obj("op" to "width", "value" to 64),
                        obj("op" to "height", "value" to 64), obj("op" to "depth", "depth" to current))) {
                        host.drain(obj("type" to "new_drawing", "action" to action))
                    }
                    compose.onNodeWithTag("new-drawing-create").performClick()
                    host.awaitMain("The new drawing is adopted", 180_000, diagnostics = { "" }, compose = compose) {
                        host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch") != epoch && !host.documents.working && !host.drawingTabs.switching
                    }
                    host.awaitReady(compose = compose)
                    host.drain(obj("type" to "invoke", "command" to "change_bit_depth"))
                    host.awaitMain("Bit depth controls are composed", 30_000, diagnostics = { "" }, compose = compose) { findTag("document-color-dither") != null }
                    compose.onNodeWithTag("color-choice-$depthLabel").assertTextContains(copy.getString(if (suggested == "U8") "depth_8" else "depth_16"))
                    val dither = compose.onNodeWithTag("document-color-dither").performScrollTo().assertIsDisplayed().assertIsOff()
                    if (current == "U16") dither.assertIsEnabled() else dither.assertIsNotEnabled()
                    screenshot("f3-depth-$current-$theme.png")
                    if (current == "U16") {
                        dither.performClick().assertIsOn()
                        compose.onNodeWithTag("color-choice-$depthLabel").performScrollTo().performClick()
                        compose.onNodeWithText(copy.getString("depth_16"), useUnmergedTree = true).performClick()
                        dither.performScrollTo().assertIsNotEnabled().assertIsOn()
                    }
                    compose.onNodeWithTag("document-color-cancel").performClick()
                    host.awaitMain("Bit depth closes", 30_000, diagnostics = { "" }, compose = compose) { findTag("document-color") == null }
                    assertNull(host.failure)
                }
            }
        } finally { scenario.close() }
    }

    private fun requestPhoneOrientation(activity: MainActivity, host: CanvasHost, orientation: Int, label: String) {
        val landscape = orientation != android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        val previous = activity.display!!.rotation
        val turnsOver = landscape && activity.requestedOrientation in listOf(android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,android.content.pm.ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE) && activity.requestedOrientation != orientation
        instrumentation.runOnMainSync { activity.requestedOrientation = orientation }
        host.awaitFrame(label,diagnostics = { "rotation=${activity.display!!.rotation}, extent=${activity.window.decorView.width}x${activity.window.decorView.height}, configuration=${activity.resources.configuration.orientation}, focus=${activity.window.decorView.hasWindowFocus()}" }) {
            (activity.resources.configuration.orientation == android.content.res.Configuration.ORIENTATION_LANDSCAPE) == landscape &&
                (activity.window.decorView.width > activity.window.decorView.height) == landscape &&
                (!turnsOver || activity.display!!.rotation != previous) && activity.window.decorView.hasWindowFocus()
        }
    }

    private fun capturePhonePage(tag: String, path: String) {
        compose.waitForIdle()
        val owner = checkNotNull(findTag(tag)).first.view.rootView
        awaitNativeCaptureFrame(owner)
        screenshot(path)
    }

    @Test fun actionPickerAcceptsNativeKeyboardEntry() {
        startupTheme()
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val original = activity.requestedOrientation
        fun searchFocused() = findTag("action-picker-search")?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null
        fun keyboard() = findTag("action-picker-search")?.first?.view?.rootView?.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
        fun focusNodes(): List<SemanticsNode> {
            val result = mutableListOf<SemanticsNode>()
            fun visit(node: SemanticsNode) { if(node.config.getOrNull(SemanticsProperties.Focused) == true)result.add(node);node.children.forEach(::visit) }
            semanticsRoots().forEach { visit(it.semanticsOwner.unmergedRootSemanticsNode) }
            return result
        }
        try {
            requestPhoneOrientation(activity,host,android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,"The private landscape window owns keyboard input")
            host.drain(obj("type" to "open_settings","page" to "touch"))
            host.awaitFrame("The two-finger action row is composed") { findTag("trigger-touch.tap.2") != null }
            compose.onNodeWithTag("trigger-touch.tap.2",useUnmergedTree=true).performScrollTo()
            awaitEditorPage(activity,host,"preferences-dialog")
            val (root,row) = checkNotNull(findTag("trigger-touch.tap.2"))
            val origin = IntArray(2);instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val point = row.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat())
            val down = SystemClock.uptimeMillis()
            for(phase in listOf(MotionEvent.ACTION_DOWN,MotionEvent.ACTION_UP)) {
                assertTrue("The private window owns touch input",activity.window.decorView.hasWindowFocus())
                val input = motion(MotionEvent.TOOL_TYPE_FINGER,phase,point,down)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(input,true)) } finally { input.recycle() }
            }
            host.awaitFrame("Touch activation opens Choose Action") { findTag("action-picker-search") != null }
            awaitEditorPage(activity,host,"action-picker")
            assertNull("Touch opening leaves search unfocused",findTag("action-picker-search")!!.second.find { it.config.getOrNull(SemanticsProperties.Focused) == true })
            assertFalse("Touch opening leaves the keyboard hidden",findTag("action-picker-search")!!.first.view.rootView.rootWindowInsets.isVisible(WindowInsets.Type.ime()))
            capturePhonePage("action-picker","action-picker-touch.png")
            host.drain(obj("type" to "preferences","action" to obj("type" to "close_action_picker")))
            host.awaitFrame("The touch-opened picker dismisses") { findTag("action-picker-search") == null }
            assertTrue(activity.window.decorView.hasWindowFocus())
            pressKey(KeyEvent.KEYCODE_DPAD_RIGHT)
            compose.onNodeWithTag("trigger-touch.tap.2",useUnmergedTree=true).performSemanticsAction(SemanticsActions.RequestFocus) { assertTrue(it()) }
            assertTrue(activity.window.decorView.hasWindowFocus())
            pressKey(KeyEvent.KEYCODE_DPAD_CENTER)
            host.awaitFrame("Keyboard activation opens Choose Action") { findTag("action-picker-search") != null }
            awaitEditorPage(activity,host,"action-picker")
            val visited = mutableSetOf<List<Int>>()
            var traversals = 0
            while(!searchFocused()) {
                val before = focusNodes().map { it.id }
                assertTrue("Native traversal must not cycle before reaching search",visited.add(before))
                assertTrue("Search follows its page header in the native focus order",traversals++ < 4)
                assertTrue(activity.window.decorView.hasWindowFocus())
                pressKey(KeyEvent.KEYCODE_TAB)
                host.awaitFrame("Tab advances the native focused node",diagnostics = { "focused=${focusNodes().map { it.config.getOrNull(SemanticsProperties.TestTag) }}; search=${searchFocused()}; ime=${keyboard()}" }) { focusNodes().map { it.id } != before }
            }
            assertTrue("Native traversal focuses the search field",searchFocused())
            assertTrue(activity.window.decorView.hasWindowFocus())
            pressKey(KeyEvent.KEYCODE_DPAD_CENTER)
            assertTrue(activity.window.decorView.hasWindowFocus())
            instrumentation.sendStringSync("Undo")
            host.awaitFrame("Native keyboard text filters actions") { host.snapshot?.objectOrNull("preferences")?.getJSONObject("shortcut_page")?.objectOrNull("picker")?.optString("query") == "Undo" }
            host.awaitFrame("Native field activation opens the real IME") { keyboard() }
            awaitEditorPage(activity,host,"action-picker")
        } finally {
            if(findTag("action-picker") != null) {
                capturePhonePage("action-picker","action-picker-keyboard.png")
            }
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun paperRenameRequestsFocusOnOpen() = renameRequestsFocusOnOpen("paper")
    @Test fun workspaceRenameRequestsFocusOnOpen() = renameRequestsFocusOnOpen("workspace")

    @Test fun toolbarPromptRequestsNameFocusOnOpen() = renameRequestsFocusOnOpen("toolbar")
    @Test fun toolbarPickerRequestsNameFocusOnTouchOpen() = renameRequestsFocusOnOpen("picker")
    @Test fun toolbarPickerRequestsNameFocusOnKeyboardOpen() = renameRequestsFocusOnOpen("picker",true)
    @Test fun drawingPresetRequestsNameFocusOnOpen() = renameRequestsFocusOnOpen("preset")

    private fun renameRequestsFocusOnOpen(kind: String, keyboardOpening: Boolean = false) {
        startupTheme()
        val scenario = launchCapy(180_000,compose)
        val activity = scenario.activity()
        val host = activity.host
        val original = activity.requestedOrientation
        val tag = when(kind) { "paper" -> "paper-rename-name";"workspace" -> "workspace-name";"preset" -> "new-drawing-save-name";else -> "toolbar-name" }
        val page = when(kind) { "paper" -> "paper-rename-dialog";"workspace" -> "workspace-form";"picker" -> "tool-picker";"preset" -> "new-drawing-dialog";else -> "toolbar-prompt" }
        val case = InstrumentationRegistry.getArguments().getString("case","$kind-name-${if(keyboardOpening)"keyboard" else "touch"}")
        fun focused() = findTag(tag)?.second?.find { it.config.getOrNull(SemanticsProperties.Focused) == true } != null
        fun ime() = findTag(tag)?.first?.view?.rootView?.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
        try {
            requestPhoneOrientation(activity,host,android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE,"The private landscape canvas owns input")
            host.awaitFrame("The launch cover releases input") { findTag("launch-splash") == null }
            createDrawing(host,800,600,"Rename focus")
            val (root,canvas) = checkNotNull(findTag("workspace"))
            val origin = IntArray(2);instrumentation.runOnMainSync { root.view.getLocationOnScreen(origin) }
            val down = SystemClock.uptimeMillis()
            for(phase in listOf(MotionEvent.ACTION_DOWN,MotionEvent.ACTION_UP)) {
                assertTrue("The private rename fixture owns touch input",activity.window.decorView.hasWindowFocus())
                val input = motion(MotionEvent.TOOL_TYPE_FINGER,phase,canvas.boundsInRoot.center+Offset(origin[0].toFloat(),origin[1].toFloat()),down)
                try { assertTrue(instrumentation.uiAutomation.injectInputEvent(input,true)) } finally { input.recycle() }
            }
            assertTrue("The rename page opens in native touch mode",root.view.isInTouchMode)
            if(keyboardOpening) { assertTrue(activity.window.decorView.hasWindowFocus());pressKey(KeyEvent.KEYCODE_DPAD_RIGHT) }
            if(kind == "paper") {
                val fixture = File(device.root,"Paper.webp")
                instrumentation.targetContext.assets.open("10-light.webp").use { input -> fixture.outputStream().use { input.copyTo(it) } }
                runBlocking { importLibraryPaper(host,android.net.Uri.fromFile(fixture)) }
                host.drain()
                val layer = host.snapshot!!.getJSONObject("state").array("layers").objects().first { it.optJSONObject("paper")?.optBoolean("editable") == true }.getLong("id")
                host.drain(obj("type" to "layer","action" to obj("op" to "select","id" to layer,"mask" to false)))
                instrumentation.runOnMainSync { host.textureDialog = TextureDialogRequest(layer,false) }
                host.awaitFrame("The imported paper tile is composed") { findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("texture-tile-") == true && it.config.contains(SemanticsActions.OnLongClick) }) != null }
                val tile = checkNotNull(findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("texture-tile-") == true && it.config.contains(SemanticsActions.OnLongClick) })).second.config[SemanticsProperties.TestTag]
                compose.onNodeWithTag(tile,useUnmergedTree=true).performScrollTo().performSemanticsAction(SemanticsActions.OnLongClick)
                host.awaitFrame("The paper rename action is composed") { findTag("texture-action-rename_paper") != null }
                compose.onNodeWithTag("texture-action-rename_paper",useUnmergedTree=true).performClick()
            } else if(kind == "picker") {
                host.drain(obj("type" to "customize","action" to obj("type" to "new_toolbar","group" to JSONObject.NULL)))
            } else if(kind == "preset") {
                host.drain(obj("type" to "invoke","command" to "new_document"))
                host.awaitFrame("New Drawing opens") { findTag("new-drawing-create") != null }
                host.drain(obj("type" to "new_drawing","action" to obj("op" to "presets")))
                host.awaitFrame("Preset browser opens") { findTag("new-drawing-save-as") != null }
                host.drain(obj("type" to "new_drawing","action" to obj("op" to "save_as")))
            } else if(kind == "toolbar") {
                host.drain(obj("type" to "customize","action" to obj("type" to "manage_toolbars")))
                host.awaitFrame("Toolbar manager opens") { host.snapshot?.objectOrNull("toolbar_manager") != null }
                val panel = host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").objects().first().get("panel")
                host.drain(obj("type" to "customize","action" to obj("type" to "close_toolbar_manager")))
                host.awaitFrame("Toolbar manager closes") { findTag("toolbar-manager") == null }
                host.drain(obj("type" to "customize","action" to obj("type" to "rename_toolbar","panel" to panel)))
            } else {
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "open","page" to "workspaces")) }
                host.awaitFrame("Workspace manager opens",diagnostics = { "manager=${host.workspaceManager}; blocked=${host.documentInputBlocked}; nameCommit=${host.workspaceNameCommit != null}; privateFocus=${activity.window.decorView.hasWindowFocus()}" }) { findTag("workspace-manager") != null }
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form","action" to obj("type" to "new"))) }
                host.awaitFrame("New workspace form opens") { findTag("workspace-name") != null }
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "submit","name" to "Rename focus workspace")) }
                host.awaitFrame("The fixture workspace is stored") { host.workspaceManager?.objectOrNull("prompt_action") == null && host.workspaceManager?.optBoolean("busy") == false }
                host.awaitFrame("The new workspace form dismisses before Rename",diagnostics = { "form=${findTag("workspace-form") != null}; manager=${findTag("workspace-manager") != null}; blocked=${host.documentInputBlocked}; privateFocus=${activity.window.decorView.hasWindowFocus()}; view=${host.workspaceManager}" }) {
                    findTag("workspace-form") == null && !host.documentInputBlocked &&
                        findTag("workspace-manager")?.first?.view?.rootView?.hasWindowFocus() == true
                }
                val id = host.workspaceManager!!.array("rows").objects().first { it.optString("title") == "Rename focus workspace" }.getString("id")
                instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "form","action" to obj("type" to "rename","value" to id))) }
            }
            host.awaitFrame("The rename field is placed without tapping it") { findTag(tag)?.second?.layoutInfo?.isPlaced == true }
            compose.waitForIdle()
            host.awaitFrame("The actual rename page is fully placed inside its own window") {
                val (root,node) = checkNotNull(findTag(page))
                val bounds = node.boundsInRoot
                node.layoutInfo.isPlaced && bounds.width >= node.size.width-1 && bounds.height >= node.size.height-1 &&
                    bounds.left >= -1 && bounds.top >= -1 && bounds.right <= root.view.width+1 && bounds.bottom <= root.view.height+1
            }
            host.awaitFrame("Requested rename focus opens the real IME without a tap",diagnostics = { "focused=${focused()}, ownIme=${ime()}" }) { focused() && ime() }
            fun entry() = checkNotNull(findTag(tag)!!.second.find { it.config.getOrNull(SemanticsProperties.EditableText) != null })
            val originalName = entry().config[SemanticsProperties.EditableText].text
            assertEquals("Rename opens with its caret at the end",androidx.compose.ui.text.TextRange(originalName.length),entry().config[SemanticsProperties.TextSelectionRange])
            val start = minOf(1,originalName.length);val end = minOf(3,originalName.length)
            instrumentation.runOnMainSync {
                val input = checkNotNull(findTag(tag)!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo()))
                assertTrue(input.setSelection(start,end));assertTrue(input.commitText("X",1))
            }
            host.awaitFrame("Name entry preserves its native edit and caret") {
                entry().config[SemanticsProperties.EditableText].text == originalName.take(start)+"X"+originalName.drop(end) &&
                    entry().config[SemanticsProperties.TextSelectionRange] == androidx.compose.ui.text.TextRange(start+1)
            }
            compose.waitForIdle()
        } finally {
            if(findTag(tag) != null)capturePhonePage(page,"rename-focus/$case/open.png")
            instrumentation.runOnMainSync { activity.requestedOrientation = original }
            scenario.close()
        }
    }

    @Test fun admission() = visit(true)
    @Test fun frame() = visit(false)
    @Test fun proofProfileDialogUsesWindowBoundsOutsideItsPanel() = visit(false, "proofProfile")
    @Test fun menu() = visit(false, "menu")
    @Test fun drawers() = visit(false, "drawers")
    @Test fun customize() = visit(false, "customize")
    @Test fun dialogsA2() = visit(false, "dialogsA2")
    @Test fun dialogsD1() = visit(false, "dialogsD1")
    @Test fun pagesD1() = visit(false, "pagesD1")
    @Test fun dialogsD2() = visit(false, "dialogsD2")
    @Test fun pagesD2() = visit(false, "pagesD2")
    @Test fun pagesA4() = visit(false, "pagesA4")
    @Test fun sourcePages() = visit(false, "sourcePages")
    @Test fun fileProgress() = visit(false, "fileProgress")
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
        var originalOrientation: Int? = null
        try {
            wakeDevice()
            if (admissionOnly) {
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
            }
            if (!admissionOnly) startupTheme()
            scenario = androidx.test.core.app.ActivityScenario.launch(MainActivity::class.java)
            val activity = scenario.activity()
            val host = activity.host
            originalOrientation = activity.requestedOrientation
            instrumentation.runOnMainSync { activity.requestedOrientation = if (size[0] > size[1])
                android.content.pm.ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE else android.content.pm.ActivityInfo.SCREEN_ORIENTATION_PORTRAIT }
            host.awaitFrame("The fixture Activity uses its requested viewport") { activity.window.decorView.width == size[0] && activity.window.decorView.height == size[1] }
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
                report.put("family",if (name == "hidden") "frame" else if (name.startsWith("inventory-")) "inventory" else if(family in listOf("dialogsA2","pagesD1","pagesD2","pagesA4","sourcePages","fileProgress")) "pages" else family)
                    .put("fixture",host.workspaceManager?.optString("id") ?: "builtin:workspace:painter").put("state", if (admissionOnly) "rest" else name).put("status", "failed").put("passed", false)
                host.drain()
                compose.waitForIdle()
                if (!admissionOnly) awaitCanvasPresentation(host)
                if(tolerateActionError)compose.waitUntil(10_000) { host.snapshot?.optBoolean("brush_ready") == true && host.snapshot?.isNull("canvas_notice") == true }
                else host.awaitFrame("Brush ready and Compose idle") { host.snapshot?.optBoolean("brush_ready") == true &&
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
                    report.put("mandatory_system_gesture_insets",JSONArray(listOf(mandatory.left,mandatory.top,mandatory.right,mandatory.bottom)))
                    report.put("cutout_waterfall_insets",JSONArray(cutoutInsets))
                    report.put("reported_insets",host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets"))
                    val phone = host.snapshot!!.getJSONObject("layout").getJSONObject("phone")
                    val clear = androidx.compose.ui.geometry.Rect(cutoutInsets[0]/density,cutoutInsets[1]/density,
                        (decor.width-cutoutInsets[2])/density,(decor.height-cutoutInsets[3])/density)
                    val controls = phone.array("bars").objects()+phone.array("tools").objects()+listOf("size","strength","settings","capy").map { phone.getJSONObject(it) }
                    controls.filter { it.number("width")>0 && it.number("height")>0 }.forEach { control ->
                        val rect = control.rect()
                        assertTrue("Visible phone control $rect avoids the camera cutout $clear",rect.left>=clear.left-.05f && rect.top>=clear.top-.05f && rect.right<=clear.right+.05f && rect.bottom<=clear.bottom+.05f)
                    }
                    cutoutInsets.forEachIndexed { side,value ->
                        assertEquals("Reported phone inset $side is the native safe maximum",value/density,
                            host.snapshot!!.getJSONObject("state").getJSONObject("phone").getJSONArray("insets").getDouble(side).toFloat(),.05f)
                    }
                    report.put("insets_pixels", JSONArray(cutoutInsets))
                    report.put("insets", JSONArray(cutoutInsets.map { it / density }))
                        report.put("ime_bottom_pixels", insets.getInsets(WindowInsets.Type.ime()).bottom)
                    report.put("crop", obj("x" to crop[0], "y" to crop[1], "width" to crop[2], "height" to crop[3]))
                    val surface = checkNotNull(decor.descendant<CanvasSurfaceView>())
                    report.put("canvas_viewport", JSONArray(listOf(surface.width / density, surface.height / density)))
                    val canvasLocation = IntArray(2)
                    surface.getLocationOnScreen(canvasLocation)
                    report.put("canvas_origin_pixels", JSONArray(canvasLocation.toList()))
                    val root = checkNotNull(findTag("workspace"))
                    collect(if(family in listOf("pagesD1","pagesD2","pagesA4","sourcePages","fileProgress"))root.first.semanticsOwner.unmergedRootSemanticsNode else root.second, allocations)
                    report.put("phone_active", host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") != null)
                    report.put("revision", host.snapshot?.getJSONObject("state")?.getLong("revision"))
                    assertEquals("Fixture theme", expectedTheme, host.snapshot?.getJSONObject("state")?.getString("theme"))
                    assertEquals("Configured density", expectedDensity, metrics.densityDpi)
                    assertEquals("Configured font scale", expectedFont, activity.resources.configuration.fontScale, .001f)
                    assertEquals("App viewport width in pixels", size[0], decor.width)
                    assertEquals("App viewport height in pixels", size[1], decor.height)
                    assertEquals("App viewport width in dp", size[0] * 160f / expectedDensity, decor.width / density, .01f)
                    assertEquals("App viewport height in dp", size[1] * 160f / expectedDensity, decor.height / density, .01f)
                    if (cutout == "none") assertEquals("No cutout/waterfall insets",listOf(0,0,0,0),cutoutInsets)
                    else assertTrue("Corner cutout produces safe insets",cutoutInsets.any { it > 0 })
                    args.getString("insets")?.split(",")?.map(String::toInt)?.let { assertEquals("Configured cutout insets", it, cutoutInsets) }
                    val axis = display.getInt("surface_quarter_turns") % 2
                    assertEquals("Native surface width", surface.width, display.getJSONArray("extent").getInt(axis))
                    assertEquals("Native surface height", surface.height, display.getJSONArray("extent").getInt(1-axis))
                }
                report.put("allocations", allocations)
                awaitNativeCaptureFrame(activity, compose)
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
            originalOrientation?.let { orientation ->
                if (scenario?.state != androidx.lifecycle.Lifecycle.State.DESTROYED) scenario?.onActivity { it.requestedOrientation = orientation }
            }
            scenario?.close()
        }
    }
    private fun createDrawing(host: CanvasHost, width: Int, height: Int, name: String) {
        val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
        host.drain(obj("type" to "invoke", "command" to "new_document"))
        host.awaitFrame("New Drawing controller is open") { host.snapshot!!.getJSONObject("state").objectOrNull("new_drawing") != null }
        host.awaitFrame("New Drawing presenter is placed") { findTag("new-drawing-dialog")?.second?.layoutInfo?.isPlaced == true }
        val page = checkNotNull(findTag("new-drawing-dialog")).first.view.rootView
        val canvas = checkNotNull(findTag("workspace")).first.view.rootView
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
        host.awaitFrame("New Drawing dismisses before canvas input") {
            findTag("new-drawing-dialog") == null && (page === canvas || !page.isAttachedToWindow) &&
                canvas.isAttachedToWindow && canvas.hasWindowFocus() && !host.documentInputBlocked
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
        if (family == "proofProfile") {
            state("proof-profile-picker") {
                openProofProfile(host,::tap)
                awaitEditorPage(activity, host, "proof-profile-picker")
            }
            tap("proof-profile-picker-done")
            host.awaitFrame("Proof profile picker is removed") { findTag("proof-profile-picker") == null }
            assertTrue(failures.joinToString("\n"), failures.isEmpty())
            return
        }
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
        if(family=="pagesA4" || family=="sourcePages" || family=="fileProgress") {
            visitPagesA4(activity,host,::tap,::state,sourceOnly=family=="sourcePages",fileOnly=family=="fileProgress")
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
            instrumentation.runOnMainSync { host.reportPhoneMetrics(JSONArray(listOf(19,7,13,5)), originalCorners, 0f) }
            host.awaitFrame("Metrics arrive during a document interaction") { phone().getJSONArray("insets").getInt(0) == 19 }
        } finally {
            host.documentInputBlocked = blocked
            instrumentation.runOnMainSync { host.reportPhoneMetrics(originalInsets, originalCorners, 0f) }
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
            state("inventory-window-$panel") {
                if (panel == "sizes") {
                    assertBrushSizeLabelsAtFontScales(activity, host, compose)
                    compose.onNodeWithTag("size-preset-2000",useUnmergedTree = true).performScrollTo()
                }
            }
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

    private fun openProofProfile(host: CanvasHost,tap: (String) -> Unit) {
        host.drain(obj("type" to "phone", "action" to obj("kind" to "panel", "panel" to "proof")))
        host.awaitFrame("Proof print mode is ready",30_000) {
            findTag("proof-mode-print")?.second?.config?.let { it.getOrNull(SemanticsProperties.Disabled) == null } == true
        }
        compose.onNodeWithTag("proof-mode-print",useUnmergedTree = true).performScrollTo()
        tap("proof-mode-print")
        host.awaitFrame("Proof profile row is ready",30_000) { findTag("proof-profile") != null }
        compose.onNodeWithTag("proof-profile",useUnmergedTree = true).performScrollTo()
        tap("proof-profile")
        host.awaitFrame("Proof profile picker opens outside its scroller") { findTag("proof-profile-picker") != null }
        host.awaitFrame("Proof menu uses shared Standard section",30_000) { findTag("profile-menu-standard") != null }
    }

    private fun visitPagesA4(activity: MainActivity,host: CanvasHost,tap: (String) -> Unit,state: (String,() -> Unit) -> Unit,sourceOnly:Boolean=false,fileOnly:Boolean=false) {
        val decor = activity.window.decorView
        val fixtures = File(instrumentation.targetContext.getExternalFilesDir(null),"a4-fixtures")
        fun fixture(name: String) = File(fixtures,name).also { assertTrue("Fixture $name is pushed",it.isFile) }
        fun shared() = host.snapshot!!.getJSONObject("state")
        fun invoke(command: String) = host.drain(obj("type" to "invoke","command" to command))
        fun exists(tag: String) = findTag(tag) != null
        fun wait(tag: String,timeout: Long = 10_000) = host.awaitFrame("Pages A4 $tag is composed",timeout) { exists(tag) }
        fun gone(tag: String,timeout: Long = 10_000) = host.awaitFrame("Pages A4 $tag closes",timeout) { !exists(tag) }
        fun keyboard() = decor.rootWindowInsets.isVisible(WindowInsets.Type.ime())
        fun hideIme() {
            instrumentation.runOnMainSync { decor.windowInsetsController?.hide(WindowInsets.Type.ime()) }
            host.awaitFrame("Keyboard-free page reference") { !keyboard() }
            compose.waitForIdle()
        }
        fun settle() { compose.waitForIdle();if(!sourceOnly&&!fileOnly)SystemClock.sleep(400);if(keyboard())hideIme() }
        fun close(tag: String) { if(exists(tag)) { try { tap(tag) } catch(error: Throwable) { predictiveBack(activity,cancel = false) };compose.waitForIdle() } }
        fun reveal(tag: String) { compose.onNodeWithTag(tag,useUnmergedTree = true).performScrollTo();compose.waitForIdle() }
        fun <T> picking(action: String,uri: android.net.Uri,block: () -> T): T {
            val monitor = object : android.app.Instrumentation.ActivityMonitor() {
                override fun onStartActivity(intent: android.content.Intent): android.app.Instrumentation.ActivityResult? =
                    if(intent.action != action) null else android.app.Instrumentation.ActivityResult(android.app.Activity.RESULT_OK,android.content.Intent().setData(uri))
            }
            instrumentation.addMonitor(monitor)
            try { return block() } finally { instrumentation.removeMonitor(monitor) }
        }
        fun epoch() = shared().getJSONObject("document_file").getLong("epoch")
        fun openFile(name: String) = instrumentation.runOnMainSync { assertTrue(host.documents.openUris(listOf(android.net.Uri.fromFile(fixture(name))))) }
        fun opened(before: Long,timeout: Long = 120_000) = host.awaitFrame("Opened drawing is active",timeout) { epoch() != before && !host.documents.working && !host.drawingTabs.switching }
        fun failedOpen() {
            state("cannot-open-file") {
                val before=epoch();openFile("broken.png");wait("file-error-ok",60_000);settle()
                assertEquals("A failed open keeps the drawing",before,epoch())
                val message=checkNotNull(host.hostError)
                assertTrue("The failure names the selected file",message.contains("broken.png"))
                compose.onNodeWithText(message).assertIsDisplayed()
            }
            close("file-error-ok");gone("file-error")
        }
        val pick = android.content.Intent.ACTION_OPEN_DOCUMENT
        val create = android.content.Intent.ACTION_CREATE_DOCUMENT
        fun recordingSaveError() {
            state("recording-save-error") {
                tolerateActionError = true
                picking(create,android.net.Uri.fromFile(File("/proc/version"))) {
                    instrumentation.runOnMainSync { host.strokeRecording.click() }
                    compose.waitUntil(10_000) { host.strokeRecording.status?.optBoolean("recording") == true && !host.strokeRecording.busy }
                    instrumentation.runOnMainSync { host.strokeRecording.click() }
                    compose.waitUntil(20_000) { findTag("action-error-ok") != null }
                }
                val expected=host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("stroke_recording_save_failed")
                assertEquals(expected,host.dialogError)
                val title=host.catalog.getJSONObject("native_copy").getJSONObject("color").getString("stroke_recording")
                assertEquals(title,host.dialogErrorTitle)
                compose.onNodeWithTag("action-error-title").assertTextContains(title)
                compose.onNodeWithText(expected).assertIsDisplayed()
                runBlocking{host.strokeRecording.update()}
                assertTrue("Failed save keeps the recording",host.strokeRecording.status!!.getBoolean("ready"))
                assertFalse(host.strokeRecording.busy)
                settle()
            }
            if(findTag("action-error-ok") != null) { compose.onNodeWithTag("action-error-ok").performSemanticsAction(SemanticsActions.OnClick);compose.waitUntil(10_000) { findTag("action-error") == null } }
            assertNull(host.dialogErrorTitle)
            tolerateActionError = false
        }
        var keyboardHeight = 0
        fun withKeyboard(tag: String) {
            if(keyboardHeight == 0) {
                val palette = checkNotNull(host.palettes.view()?.array("palettes")?.objects()?.firstOrNull()?.getLong("id")) { "No palette" }
                instrumentation.runOnMainSync { host.palettes.command(obj("command" to "rename_palette","id" to palette)) }
                host.awaitFrame("Palette name raises the native keyboard") { keyboard() }
                keyboardHeight = decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom
                instrumentation.runOnMainSync { host.palettes.dialog = null };hideIme();gone("palette-name-dialog")
            }
            val view = checkNotNull(findTag("workspace")).first.view
            val original = decor.rootWindowInsets
            instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(original)
                .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.of(0,0,0,keyboardHeight)).setVisible(WindowInsets.Type.ime(),true).build()) }
            host.awaitFrame("$tag follows the reduced visible area",diagnostics = { "keyboard=$keyboardHeight bounds=${findTag(tag)?.second?.boundsInRoot}" }) {
                findTag(tag)?.second?.boundsInRoot?.let { it.bottom <= decor.height-keyboardHeight+1 } == true }
            compose.waitForIdle()
        }
        fun restoreKeyboard() {
            val view = findTag("workspace")?.first?.view ?: return
            instrumentation.runOnMainSync { view.dispatchApplyWindowInsets(WindowInsets.Builder(decor.rootWindowInsets)
                .setInsets(WindowInsets.Type.ime(),android.graphics.Insets.NONE).setVisible(WindowInsets.Type.ime(),false).build()) }
            compose.waitForIdle()
        }

        if(!sourceOnly&&!fileOnly) {
        invoke("select_all")
        for ((command,name) in listOf("feather_selection" to "selection-refine","frequency_separation" to "frequency-separation")) {
            state("$name-ime") { invoke(command);wait("$name-panel");withKeyboard("$name-panel") }
            restoreKeyboard();close("$name-panel-cancel");gone("$name-panel")
        }
        if(shared().array("commands").objects().any { it.optString("id") == "deselect" && it.optBoolean("enabled") })invoke("deselect")

        state("generic-error") {
            tolerateActionError = true
            instrumentation.runOnMainSync { host.reportActionError("Could not finish the action.") }
            compose.waitUntil(10_000) { findTag("action-error-ok") != null };settle()
        }
        if(findTag("action-error-ok") != null) { compose.onNodeWithTag("action-error-ok").performSemanticsAction(SemanticsActions.OnClick);compose.waitUntil(10_000) { findTag("action-error") == null } }
        tolerateActionError = false
        recordingSaveError()

        val paperLayer = shared().array("layers").objects().first { it.optJSONObject("paper")?.optBoolean("editable") == true }.getLong("id")
        state("paper-import-return") {
            host.drain(obj("type" to "layer","action" to obj("op" to "select","id" to paperLayer,"mask" to false)))
            instrumentation.runOnMainSync { host.textureDialog = TextureDialogRequest(paperLayer,false) }
            host.awaitFrame("Paper picker",20_000,{ "dialog=${host.textureDialog} prefs=${host.snapshot?.objectOrNull("preferences") != null} texture=${exists("texture-dialog")} use=${exists("texture-use")}" }) { exists("texture-import") }
            picking(pick,android.net.Uri.fromFile(fixture("prophoto16.png"))) { runCatching { reveal("texture-import") };tap("texture-import")
            host.awaitFrame("Imported paper is applied",60_000) { !exists("texture-dialog") && shared().array("layers").objects().first { it.getLong("id") == paperLayer }.optJSONObject("paper")?.optString("id")?.startsWith("builtin") == false } }
            settle()
        }
        fun renamePaper() {
            host.drain(obj("type" to "layer","action" to obj("op" to "select","id" to paperLayer,"mask" to false)))
            instrumentation.runOnMainSync { host.textureDialog = TextureDialogRequest(paperLayer,false) }
            wait("texture-use")
            val tile = checkNotNull(findNode({ it.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("texture-tile-") == true && it.config.getOrNull(SemanticsProperties.Selected) == true })) { "No selected paper tile" }
                .second.config[SemanticsProperties.TestTag]
            compose.onNodeWithTag(tile,useUnmergedTree = true).performScrollTo().performSemanticsAction(SemanticsActions.OnLongClick)
            wait("texture-action-rename_paper");tap("texture-action-rename_paper");wait("paper-rename-dialog")
        }
        state("paper-rename") { renamePaper();settle() }
        if(exists("paper-rename-dialog")) {
            close("paper-rename-dialog-cancel");gone("paper-rename-dialog")
            renamePaper()
            instrumentation.runOnMainSync { host.textureDialog = null }
            gone("paper-rename-dialog")
        }
        state("palette-import-return") {
            val count = host.palettes.view()?.array("palettes")?.length() ?: 0
            picking(pick,android.net.Uri.fromFile(fixture("a4-palette.gpl"))) {
                instrumentation.runOnMainSync { host.palettes.command(obj("command" to "import_palette")) }
                host.awaitFrame("Imported palette is listed",30_000,{ "message=${host.palettes.message} palettes=${host.palettes.view()?.array("palettes")?.length()}" }) { (host.palettes.view()?.array("palettes")?.length() ?: 0) > count }
            }
            settle()
        }
        state("keymap-import-chooser") {
            invoke("keyboard_shortcuts")
            host.awaitFrame("Shortcuts page") { host.snapshot?.objectOrNull("preferences")?.optString("page") == "shortcuts" && exists("keymap-menu") }
            reveal("keymap-menu");tap("keymap-menu");wait("keymap-import")
            picking(pick,android.net.Uri.fromFile(fixture("a4-keymap.json"))) { tap("keymap-import");wait("keymap-import-cancel",30_000) }
            settle()
        }
        close("keymap-import-cancel")
        if(host.snapshot?.objectOrNull("preferences") != null)host.drain(obj("type" to "close_settings"))

        val toolbar = run {
            host.drain(obj("type" to "customize","action" to obj("type" to "manage_toolbars")))
            host.awaitFrame("Toolbar manager view") { host.snapshot?.objectOrNull("toolbar_manager") != null }
            host.snapshot!!.getJSONObject("toolbar_manager").array("toolbars").objects().first().get("panel").also {
                host.drain(obj("type" to "customize","action" to obj("type" to "close_toolbar_manager")));gone("toolbar-manager") }
        }
        state("delete-toolbar") { host.drain(obj("type" to "customize","action" to obj("type" to "delete_toolbar","panel" to toolbar)));wait("toolbar-prompt");settle() }
        if(host.snapshot?.objectOrNull("toolbar_prompt") != null)host.drain(obj("type" to "customize","action" to obj("type" to "cancel_toolbar")))

        failedOpen()
        state("preserved-package-preview") { openFile("corrupt.capy");wait("preserved-package-preview",60_000);settle() }
        state("package-copy-return") {
            if(!exists("preserved-package-preview")) { openFile("corrupt.capy");wait("preserved-package-preview",60_000) }
            val copy = File(fixtures,"package-copy-${System.nanoTime()}.capy")
            picking(create,android.net.Uri.fromFile(copy)) { reveal("copy-original-package");tap("copy-original-package")
                host.awaitFrame("Package copy returns",60_000) { copy.isFile && copy.length() > 0 } }
            assertTrue("The original package was copied",copy.isFile && copy.length() > 0);copy.delete();settle()
        }
        close("package-close");gone("preserved-package-preview",30_000)

        }
        fun assertProgress(tag:String,title:String) {
            host.awaitFrame("$tag shows its operation title") {findTag("$tag-title")?.second?.config?.getOrNull(SemanticsProperties.Text)?.any{it.text==title}==true}
            compose.onNodeWithTag("$tag-status").assertTextContains(host.bootstrap!!.getString("preparing_document"))
        }
        if(fileOnly){recordingSaveError();failedOpen()}
        if(!sourceOnly) {
        state("document-opening-progress") {
            openFile("big.png");wait("document-progress",60_000)
            assertProgress("document-progress",host.bootstrap!!.getString("opening_files"));compose.waitForIdle()
        }
        state("document-opening-cancelled") {
            if(exists("document-progress-cancel"))tap("document-progress-cancel")
            host.awaitFrame("Open cancels",120_000) { !exists("document-progress") && !host.documents.working };settle()
        }
        }
        if(!fileOnly) {
        state("untagged-source-profile") {
            host.drain(obj("type" to "preferences","action" to obj("type" to "edit","id" to "missing_profile","value" to 1)))
            openFile("untagged.png");wait("source-profile",60_000);settle()
            if(sourceOnly)assertNull("No raw channel caption",findNode(hasLabel("Rgba")))
        }
        close("source-profile-cancel");host.awaitFrame("Interpretation prompt closes",60_000) { !exists("source-profile") && !host.documents.working }
        host.drain(obj("type" to "preferences","action" to obj("type" to "reset","id" to "missing_profile")))
        }
        if(!sourceOnly) {
        if(!fileOnly) {
        val bigEpoch = epoch()
        state("big-opened") { openFile("big.png");opened(bigEpoch,180_000);settle() }
        state("clipboard-progress") { invoke("copy_merged");wait("clipboard-progress",30_000);compose.waitForIdle() }
        close("clipboard-progress-cancel");host.awaitFrame("Clipboard progress closes",120_000) { !exists("clipboard-progress") }
        state("document-color-progress") {
            invoke("convert_color_space");wait("document-color-apply",30_000)
            host.awaitFrame("Conversion preview is ready",120_000) { findTag("document-color-apply")?.second?.config?.getOrNull(SemanticsProperties.Disabled) == null }
            tap("document-color-apply")
            host.awaitFrame("Conversion progress",30_000) { exists("document-color") && !exists("document-color-apply") };compose.waitForIdle()
        }
        close("document-color-cancel");host.awaitFrame("Colour change ends",180_000) { !exists("document-color") }
        }
        state("image-import-progress") {
            picking(pick,android.net.Uri.fromFile(fixture("big.png"))) { invoke("import_image");wait("image-import-progress",60_000) }
            val id=checkNotNull(host.documents.images.requestId)
            val title=runBlocking{host.withNative{Native.documentRequestTitle(it,id)}}!!
            assertProgress("image-import-progress",title);compose.waitForIdle()
        }
        close("image-import-progress-cancel");host.awaitFrame("Image import ends",180_000) { !exists("image-import-progress") }
        }
        if(fileOnly)return
        val photoEpoch = epoch()
        state("photo-opened") { openFile("prophoto16.png");opened(photoEpoch);settle() }
        for ((command,name,tag) in listOf(Triple("assign_profile","assign-profile","document-color"),Triple("convert_color_space","convert-color-space","document-color"),
            Triple("change_bit_depth","change-bit-depth","document-color"),Triple("repair_source_profile","repair-source-profile","source-edit"),Triple("rasterize_source","rasterize-source","source-edit")).filter { !sourceOnly || it.third=="source-edit" }) {
            state(name) {
                invoke(command);wait(tag,30_000)
                wait("color-comparison-after",120_000);settle()
                if(sourceOnly) {
                    val density=activity.resources.displayMetrics.density
                    val frame=findTag("color-comparison")!!.second.boundsInRoot
                    val split=Native.dialogUi(obj("kind" to "panes_fit","width" to frame.width/density,"minimum" to JSONArray(listOf(160,160)),"gap" to 12).toString()).toBoolean()
                    val before=findTag("color-comparison-before")!!.second.boundsInRoot
                    val after=findTag("color-comparison-after")!!.second.boundsInRoot
                    if(split) { assertEquals(before.top,after.top,1f);assertTrue(after.left>=before.right) }
                    else assertTrue(after.top>=before.bottom)
                    if(command=="repair_source_profile") {
                        val current=findTag("source-current-description")!!.second
                        val corrected=findTag("color-choice-"+host.catalog.getJSONObject("document_color_copy").getString("correct_profile"))!!.second
                        assertTrue("Current source precedes the chooser",current.positionInRoot.y+current.size.height<=corrected.positionInRoot.y)
                    }
                }
            }
            if(sourceOnly && command=="repair_source_profile") {
                val profileCopy=host.catalog.getJSONObject("profile_copy")
                state("repair-profile-menu") {
                    val chooser="color-choice-"+host.catalog.getJSONObject("document_color_copy").getString("correct_profile")
                    reveal(chooser);tap(chooser)
                    wait("profile-menu-current",30_000);wait("profile-menu-standard",30_000);settle()
                    val current=findTag("profile-menu-current")!!.second
                    val standard=findTag("profile-menu-standard")!!.second
                    val original=findTag("profile-menu-current-item")!!.second
                    assertTrue("Current precedes Standard",current.positionInRoot.y+current.size.height<=standard.positionInRoot.y)
                    assertTrue("Original ICC follows Current",current.positionInRoot.y+current.size.height<=original.positionInRoot.y && original.positionInRoot.y+original.size.height<=standard.positionInRoot.y)
                }
                state("repair-profile-library") {
                    compose.onNodeWithText(profileCopy.getString("saved_dialog")).performScrollTo().performClick();wait("profile-library",30_000);settle()
                }
                state("repair-profile-import-return") {
                    compose.waitUntil(30_000) { compose.onNodeWithText(profileCopy.getString("import")).fetchSemanticsNode().config.getOrNull(SemanticsProperties.Disabled)==null }
                    picking(pick,android.net.Uri.fromFile(fixture("proof-p3.icc"))) {
                        compose.onNodeWithText(profileCopy.getString("import")).performScrollTo().performClick()
                        host.awaitFrame("Imported source profile returns to its comparison",120_000) { !exists("profile-library") && exists("source-edit") && exists("color-comparison-after") }
                    }
                    settle()
                }
            }
            close("$tag-cancel");gone(tag,30_000)
        }
        if(sourceOnly)return
        state("proof-profile-picker") {
            openProofProfile(host,tap)
            compose.waitForIdle();settle()
        }
        val profileCopy=host.catalog.getJSONObject("profile_copy")
        state("proof-profile-library") {
            compose.onNodeWithText(profileCopy.getString("saved_dialog")).performScrollTo().performClick();wait("profile-library",30_000);settle()
        }
        state("proof-library-return") {
            close("profile-library-done");gone("profile-library");wait("proof-profile");settle()
        }
        state("profile-library-import") {
            if(!exists("profile-library")) {
                if(!exists("proof-profile-picker"))openProofProfile(host,tap)
                compose.onNodeWithText(profileCopy.getString("saved_dialog")).performScrollTo().performClick();wait("profile-library")
            }
            val before = findTag("profile-library")!!.second.let { node -> var count = 0;fun walk(n: SemanticsNode) { count++;n.children.forEach(::walk) };walk(node);count }
            picking(pick,android.net.Uri.fromFile(fixture("proof-p3.icc"))) { compose.onNodeWithText(profileCopy.getString("import")).performScrollTo().performClick()
            host.awaitFrame("Imported ICC profile appears",30_000) { findTag("profile-library")?.second?.let { node -> var count = 0;fun walk(n: SemanticsNode) { count++;n.children.forEach(::walk) };walk(node);count > before } == true } }
            settle()
        }
        close("profile-library-done");close("proof-profile-picker-done")
        instrumentation.runOnMainSync { host.proof.action(obj("type" to "mode","mode" to "off")) }
        if(exists("proof-mode-print"))predictiveBack(activity,cancel = false)

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
        fun withKeyboard(tag: String) {
            tap("number-value-"+tag.removeSuffix("-panel"))
            host.awaitFrame("$tag focuses its numeric field above the keyboard", diagnostics = {
                "field=${findTag(tag)?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null }?.boundsInRoot}; ime=${decor.rootWindowInsets.getInsets(WindowInsets.Type.ime()).bottom}"
            }) {
                val field = findTag(tag)?.second?.find { it.config.getOrNull(SemanticsActions.SetText) != null } ?: return@awaitFrame false
                val blocked = decor.rootWindowInsets.getInsets(WindowInsets.Type.ime() or WindowInsets.Type.navigationBars())
                keyboard() && field.config.getOrNull(SemanticsProperties.Focused) == true && field.size.width > 0 && field.size.height > 0 &&
                    field.boundsInRoot.height >= field.size.height-1 && field.boundsInRoot.bottom <= decor.height-blocked.bottom+1
            }
            compose.waitForIdle()
        }
        fun restoreKeyboard() { if(keyboard())hideIme() }
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
        tap("settings-category-touch")
        host.awaitFrame("Preferences uses the shared page route") {host.snapshot?.objectOrNull("preferences")?.getString("page")=="touch" && findTag("settings-main-pane")!=null}
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
            runCatching { reveal(tag) };tap(tag)
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
            assertEquals(listOf("appearance","color","touch","input","shortcuts","about"), preferences()!!.array("pages").objects().map { it.getString("id") })
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
        for ((query,destination) in listOf("pinch" to "touch","two-finger tap" to "touch","pan speed" to "input","pass through" to "color")) {
            preferencesRoot()
            type("settings-search",query) { preferences()?.optString("query") == query && preferences()?.array("search_results")?.objects()?.any { it.getJSONObject("action").optString("page") == destination } == true }
            val result = preferences()!!.array("search_results").objects().first { it.getJSONObject("action").optString("page") == destination }
            compose.onNode(hasText(result.getString("title")) and hasClickAction() and hasAnyAncestor(hasTestTag("settings-sidebar"))).performScrollTo().performClick()
            host.awaitFrame("Settings search $query opens $destination") { preferences()?.optString("page") == destination && placed("settings-main-pane") }
        }
        for ((id,rows) in listOf("appearance" to listOf("language","theme"),"color" to listOf("missing_profile","photo_depth","pass_through_groups"),
            "touch" to listOf("finger_drawing"),"input" to listOf("wheel_behavior","eraser_tool"),"about" to emptyList())) {
            state("preferences-$id") {
                page(id)
                assertHeaderSpacing("settings-page", "settings-page-back", "settings-done")
                if (id == "touch") {
                    val groups = preferences()!!.array("pages").objects().first { it.getString("id") == "touch" }.array("groups").objects()
                    assertEquals(listOf("Touch gestures", "Navigation"), groups.map { it.getString("title") })
                    compose.onAllNodesWithText("Touch gestures").assertCountEquals(1)
                    compose.onAllNodesWithText("Navigation").assertCountEquals(1)
                    for (tag in listOf("preference-finger_drawing", "trigger-touch.tap.2", "trigger-touch.tap.3", "trigger-touch.tap.4")) {
                        reveal(tag); wait(tag)
                    }
                    reveal("preference-finger_drawing")
                    instrumentation.runOnMainSync {
                        val rows = listOf("preference-finger_drawing", "trigger-touch.tap.2", "trigger-touch.tap.3", "trigger-touch.tap.4", "settings-group-title-Navigation")
                            .map { checkNotNull(findTag(it)).second.boundsInRoot }
                        assertTrue("Phone gesture rows stay before Navigation", rows.zipWithNext().all { (before, after) -> before.bottom <= after.top })
                    }
                }
                if (id == "appearance") {
                    for (label in listOf("transparency", "dark_base", "light_base")) textLayouts("preference-label-$label").forEach { layout ->
                        assertTrue("$label keeps measured text", layout.size.width > 0)
                        assertFalse("$label keeps its full height", layout.didOverflowHeight)
                        var singleLetters = 0
                        for (line in 0 until layout.lineCount) {
                            assertFalse("$label wraps without ellipsis", layout.isLineEllipsized(line))
                            val fragment = layout.layoutInput.text.text.substring(layout.getLineStart(line), layout.getLineEnd(line, true)).trim()
                            if (fragment.length == 1 && fragment[0].isLetterOrDigit()) singleLetters++
                        }
                        assertTrue("$label avoids a character column: size=${layout.size}, lines=${layout.lineCount}", singleLetters <= layout.lineCount / 2)
                    }
                    for (tag in listOf("settings-page-title", "setting-choice-language"))
                        textLayouts(tag).forEach { assertEquals("$tag stays on one line", 1, it.lineCount) }
                    instrumentation.runOnMainSync {
                        val row = checkNotNull(findTag("preference-transparency")).second
                        val density = activity.resources.displayMetrics.density
                        val width = (org.json.JSONTokener(Native.dialogUi(obj("kind" to "preferences_content_width",
                            "width" to activity.window.decorView.width / density, "minimum" to 0).toString())).nextValue() as Number).toFloat()
                        assertEquals("Preferences uses the shared content width", width, row.size.width / density, 1f / density)
                        val right = row.positionInRoot.x + row.size.width - 14 * activity.resources.displayMetrics.density
                        fun checkCircles(node: SemanticsNode) {
                            if (node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("setting-transparency-circle-") == true) {
                                assertEquals("Inline transparency choices match the native size", 28 * activity.resources.displayMetrics.density, node.size.width.toFloat(), 1f)
                                assertTrue("Transparency choices keep their full width inside the row", node.positionInRoot.x + node.size.width <= right + 1)
                            }
                            node.children.forEach(::checkCircles)
                        }
                        checkCircles(row)
                    }
                }
            }
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
        state("preferences-shortcuts") { page("shortcuts");assertSettingRowInset("keymap-preset") }
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
            if (host.snapshot!!.getJSONObject("layout").getJSONArray("viewport").getDouble(0) <= 320) {
                val divider = checkNotNull(findTag("shortcut-recording-divider")).second.boundsInRoot
                for (tag in listOf("cancel-shortcut", "confirm-shortcut")) {
                    val button = compose.onNodeWithTag(tag).assertIsDisplayed().fetchSemanticsNode()
                    assertTrue("Recording actions stay in the fixed footer", button.boundsInRoot.top >= divider.bottom)
                    assertFalse("Recording actions are outside the scrolling page", generateSequence(button.parent) { it.parent }.any { it.config.getOrNull(SemanticsActions.ScrollBy) != null })
                }
            }
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
            textLayouts("color-value-hex").forEach { layout ->
                assertTrue("The complete hex value fits: ${layout.layoutInput.text}", layout.multiParagraph.intrinsics.maxIntrinsicWidth <= layout.size.width)
                assertFalse("The hex value stays on one visible line", layout.didOverflowHeight)
            }
            assertHeaderSpacing("color-editor", "color-cancel", "color-use")
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
            host.awaitFrame("All Swatches list fills its allocation") { findTag("color-sheet-list")?.second?.let { it.layoutInfo.isPlaced && it.boundsInRoot.height > 0 } == true }
            val sheet = checkNotNull(findTag("color-sheet")).second.boundsInRoot
            val list = checkNotNull(findTag("color-sheet-list")).second.boundsInRoot
            assertEquals("All Swatches fills the available height", sheet.bottom, list.bottom, 1f)
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
            assertHeaderSpacing("texture-dialog", "texture-cancel", "texture-use")
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
        fun exportOffset() = checkNotNull(checkNotNull(findTag("export-dialog")).second.find {
            it.config.getOrNull(SemanticsProperties.VerticalScrollAxisRange) != null
        }).config[SemanticsProperties.VerticalScrollAxisRange].value()
        var exportRootOffset: Float? = null
        state("export-size") {
            exportRoot();reveal("export-size")
            exportRootOffset=exportOffset()
            if(decor.height<=320*activity.resources.displayMetrics.density)assertTrue("The narrow root is scrolled to Size",exportRootOffset!!>0)
            tap("export-size");wait("export-fit")
            val size=host.catalog.getJSONObject("export_copy").getString("size")
            val label=checkNotNull(checkNotNull(findTag("export-fit")).second.find {
                it.config.getOrNull(SemanticsProperties.Text)?.singleOrNull()?.text == size
            })
            val header=checkNotNull(findTag("export-back")).second.boundsInRoot
            assertTrue("Size starts below its header",label.boundsInRoot.top>=header.bottom)
            assertEquals("The whole Size label is visible",label.size.height.toFloat(),label.boundsInRoot.height,1f)
        }
        if(findTag("export-fit") != null) {
            tap("export-back");host.awaitFrame("Export Back returns to its root") { findTag("export-presets") != null }
            exportRootOffset?.let { assertEquals("Back preserves the root scroll offset",it,exportOffset(),1f) }
        }
        state("export-color") {
            exportRoot();reveal("export-color");tap("export-color")
            host.awaitFrame("Export colour page") { findTag("export-presets") == null && placed("export-back") }
            val background = "color-choice-" + host.catalog.getJSONObject("export_copy").getString("background")
            reveal(background);assertSettingRowInset(background)
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
            compose.waitForIdle()
            val buttons = listOf("document-close-save", "document-close-discard", "document-close-cancel").map { tag ->
                val node = compose.onNodeWithTag(tag).assertIsDisplayed().fetchSemanticsNode()
                textLayouts(tag).forEach { layout ->
                    assertFalse("Alert response $tag keeps its complete label height", layout.didOverflowHeight)
                    assertTrue("Alert response $tag shows every word", (0 until layout.lineCount).none { layout.isLineEllipsized(it) } &&
                        layout.getLineEnd(layout.lineCount-1,visibleEnd = true) == layout.layoutInput.text.length)
                }
                val bounds = node.boundsInRoot
                assertTrue("Alert responses fit the viewport", bounds.left >= 0 && bounds.top >= 0 && bounds.right <= activity.window.decorView.width && bounds.bottom <= activity.window.decorView.height)
                bounds
            }
            if (activity.window.decorView.width <= 320) {
                assertTrue("Short narrow alerts stack nonfitting responses", buttons.zipWithNext().all { (above, below) -> above.bottom <= below.top })
            }
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
                val graph = checkNotNull(findTag("effect-curve")).second
                val editor = shared().getJSONObject("${kind}_calibration").getJSONObject("editor")
                val inset = editor.getJSONObject("controls").number("inset") * density
                assertTrue("Calibration keeps a positive curve plot", graph.size.width > 2 * inset && graph.size.height > 2 * inset)
                assertEquals("Calibration keeps symmetric curve gutters", body.center.x, graph.boundsInRoot.center.x, 1f)
                if (activity.window.decorView.width > activity.window.decorView.height) {
                    assertTrue("Landscape calibration preserves the shared graph minimum", graph.size.width >= 200 * density - 1 && graph.size.height >= 200 * density - 1)
                }
                if (kind == "tilt") {
                    for (tag in listOf("pen-tilt-sensitivity-title", "pen-tilt-direction-title"))
                        textLayouts(tag).forEach { assertEquals("Tilt labels stay on one line", 1, it.lineCount) }
                }
            }
            if(shared().objectOrNull("${kind}_calibration") != null)state("$kind-calibration-controls") {
                for (tag in listOf("pen-$kind-firmer", "pen-$kind-lighter"))
                    compose.onNodeWithTag(tag).performScrollTo().assertIsDisplayed()
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
            host.awaitFrame("The editing menu opens before release") { findTag("workspace-menu") != null }
            event(MotionEvent.ACTION_UP,at)
            assertNotNull("Hold release keeps the editing menu open",findTag("workspace-menu"))
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
        event(MotionEvent.ACTION_MOVE,to)
        host.awaitFrame("Dragging closes its held menu") { findTag("workspace-menu") == null }
        state("customize-reorder-held") {}
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
        for (swapped in listOf(false, true)) {
            if (phone().getBoolean("swapped") != swapped) tap("phone-swap")
            val suffix = if (swapped) "-swapped" else ""
            tap("phone-finger")
            host.awaitFrame("Finger strip opens its page") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "one_finger_drag" }
            val finger = model().array("pages").objects().first { it.getJSONObject("id").objectOrNull("page")?.optString("kind") == "one_finger_drag" }
            assertEquals(listOf(4,1),finger.array("sections").values().map { (it as JSONArray).length() })
            assertEquals(listOf("Auto","Draw","Move canvas","Off","More Touch Settings…"),finger.array("sections").values().flatMap { (it as JSONArray).objects() }.map { it.getString("label") })
            state("menu-one-finger-drag$suffix", {})
            for ((index, mode) in listOf("auto", "on", "move_canvas", "off").withIndex()) {
                tap("phone-menu-row-0-$index")
                host.awaitFrame("Finger choice returns to the open main menu") { phone().getBoolean("menu") && phone().getJSONObject("menu_route").isNull("page") }
                assertEquals("layer-finger-draw-symbolic",model().array("strip").getJSONObject(2).getString("icon"))
                state("menu-finger-$mode$suffix", {})
                tap("phone-finger")
                host.awaitFrame("Finger strip reopens its page") { phone().getJSONObject("menu_route").objectOrNull("page")?.optString("kind") == "one_finger_drag" &&
                    findTag("phone-menu-row-0-$index") != null }
                assertEquals("Finger page marks its selected choice", true,
                    checkNotNull(findTag("phone-menu-row-0-$index")).second.config.getOrNull(SemanticsProperties.Selected))
            }
            tap("phone-menu-row-1-0")
            host.awaitFrame("Phone More opens Touch Settings") { host.snapshot?.objectOrNull("preferences")?.optString("page") == "touch" && findTag("setting-choice-finger_drawing") != null }
            tap("settings-done")
            host.awaitFrame("Phone More Settings closes") { host.snapshot?.objectOrNull("preferences") == null }
            open()
        }
        tap("phone-swap")
        fun swapChecked() = checkNotNull(findTag("phone-swap")).second.config.getOrNull(SemanticsProperties.Selected)
        val swapBefore = swapChecked()
        for (value in listOf(swapBefore != true,swapBefore == true)) {
            tap("phone-swap")
            host.awaitFrame("Swap Sides remains in the menu") { phone().getBoolean("menu") && swapChecked() == value }
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
