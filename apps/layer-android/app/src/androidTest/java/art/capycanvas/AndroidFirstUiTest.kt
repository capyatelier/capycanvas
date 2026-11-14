package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.provider.Settings
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.platform.WindowRecomposerFactory
import androidx.compose.ui.platform.WindowRecomposerPolicy
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

/** Hold the real renderer before device creation, with the real Compose UI running. */
class AndroidFirstUiTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    private fun awaitLaunchReveal(activity: MainActivity) {
        compose.waitUntil(5_000) {
            !activity.launchSplash.nativeActive && compose.onAllNodesWithTag("launch-splash").fetchSemanticsNodes().isEmpty()
        }
    }

    @Test fun launchRevealWaitsForReadyDrawAndCommit() {
        val reveal = LaunchReveal()
        reveal.update(true, false)
        assertNull(reveal.drawn())
        reveal.update(false, true)
        assertTrue(reveal.covered)
        assertTrue(reveal.holding(false, true))
        assertFalse(reveal.started)
        val commit = checkNotNull(reveal.drawn())
        assertTrue(reveal.holding(false, true))
        assertNull(reveal.drawn())
        reveal.update(false, true)
        assertFalse(reveal.started)
        commit()
        assertTrue(reveal.started)
        assertFalse(reveal.holding(false, true))
        reveal.update(false, true)
        commit()
        assertTrue(reveal.started)
        assertNull(reveal.drawn())
    }

    @Test fun launchRevealCancelsBypassAndClosedReceipts() {
        val reveal = LaunchReveal()
        reveal.update(true, false)
        reveal.update(false, true)
        val obsolete = checkNotNull(reveal.drawn())
        assertFalse(reveal.holding(false, false))
        reveal.update(false, false)
        obsolete()
        assertFalse(reveal.covered)
        assertFalse(reveal.started)
        reveal.native()
        assertFalse(reveal.covered)
        reveal.update(true, false)
        reveal.update(false, true)
        val current = checkNotNull(reveal.drawn())
        obsolete()
        assertFalse(reveal.started)
        reveal.close()
        current()
        reveal.update(true, false)
        reveal.native()
        assertFalse(reveal.covered)
        assertFalse(reveal.started)
        assertNull(reveal.drawn())
        assertFalse(reveal.holding(true, false))
    }

    @Test fun launchRevealPreservesInheritanceAndRearmsNativeOwnership() {
        val reveal = LaunchReveal()
        reveal.update(false, true)
        assertFalse(reveal.covered)
        assertFalse(reveal.holding(false, true))
        assertNull(reveal.drawn())
        reveal.native()
        assertTrue(reveal.covered)
        assertTrue(reveal.holding(false, true))
        val obsolete = checkNotNull(reveal.drawn())
        reveal.native()
        obsolete()
        assertFalse(reveal.started)
        val current = checkNotNull(reveal.drawn())
        current()
        assertTrue(reveal.started)
        reveal.native()
        assertFalse(reveal.started)
        current()
        assertFalse(reveal.started)
        checkNotNull(reveal.drawn())()
        assertTrue(reveal.started)
    }

    private fun launchMarks(): List<Pair<String, Int>> {
        val resources = instrumentation.targetContext.resources
        val icons = resources.getStringArray(R.array.capy_launch_icons)
        val marks = resources.obtainTypedArray(R.array.capy_launch_marks)
        return try { icons.mapIndexed { index, icon -> icon to marks.getResourceId(index, 0) } } finally { marks.recycle() }
    }

    private fun ink(size: Int, saved: File? = null, alpha: Int = 127, draw: (android.graphics.Canvas) -> Unit): BooleanArray {
        val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        return try {
            draw(android.graphics.Canvas(bitmap))
            saved?.outputStream()?.use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
            val pixels = IntArray(size * size)
            bitmap.getPixels(pixels, 0, size, 0, 0, size, size)
            BooleanArray(pixels.size) { android.graphics.Color.alpha(pixels[it]) > alpha }
        } finally { bitmap.recycle() }
    }

    private fun ink(drawable: android.graphics.drawable.Drawable, size: Int, saved: File? = null, alpha: Int = 127) = ink(size, saved, alpha) {
        drawable.setBounds(0, 0, size, size)
        drawable.draw(it)
    }

    @Test fun launchMarksDrawTheSharedPoses() {
        val context = instrumentation.targetContext
        val marks = launchMarks()
        assertEquals(listOf("zen-looking-up", "zen-facing-forward", "zen-bathing", "zen-sleeping"), marks.map { it.first })
        val size = 256
        for ((icon, mark) in marks) {
            val source = context.assets.open("layer-$icon-symbolic.svg").bufferedReader().use { it.readText() }
            val picture = com.caverock.androidsvg.SVG.getFromString(source.replace("currentColor", "#000000")).renderToPicture()
            val expected = ink(size) { it.drawPicture(picture, android.graphics.RectF(0f, 0f, size.toFloat(), size.toFloat())) }
            val actual = ink(context.getDrawable(mark)!!, size)
            val union = expected.indices.count { expected[it] || actual[it] }
            val differing = expected.indices.count { expected[it] != actual[it] }
            assertTrue("$icon launch mark draws the shared artwork: $differing of $union", differing <= union / 50)
        }
    }

    @Test fun homeScreenShowsTheSavedCapy() {
        val context = instrumentation.targetContext
        val packages = context.packageManager
        val icons = launchMarks().map { it.first }
        for (icon in icons) {
            val info = packages.getActivityInfo(HomeScreenIcon.launcher(context, icon), android.content.pm.PackageManager.MATCH_DISABLED_COMPONENTS)
            assertEquals(LauncherActivity::class.java.name, info.targetActivity)
            assertEquals(context.resources.getIdentifier("capy_launcher_${icon.replace('-', '_')}", "mipmap", context.packageName), info.icon)
            assertEquals("Only the default capy starts on the home screen", icon == "zen-looking-up", info.enabled)
        }
        startupTheme()
        val saved = context.getSharedPreferences(CanvasHost.preferencesName, 0)
        assertTrue(saved.edit().putString("settings", JSONObject(saved.getString("settings", null)!!).put("zen_icon", "bathing").toString()).commit())
        try {
            launchCapy(compose = compose).use { compose.waitUntil(10_000) { homeScreenShows("zen-bathing") } }
        } finally {
            for (icon in icons) packages.setComponentEnabledSetting(HomeScreenIcon.launcher(context, icon),
                android.content.pm.PackageManager.COMPONENT_ENABLED_STATE_DEFAULT, android.content.pm.PackageManager.DONT_KILL_APP)
        }
        assertTrue(homeScreenShows("zen-looking-up"))
    }

    @Test fun launcherOpensTheWindowInItsOwnTask() {
        val context = instrumentation.targetContext
        startupTheme()
        val monitor = instrumentation.addMonitor(MainActivity::class.java.name, null, false)
        context.startActivity(checkNotNull(context.packageManager.getLaunchIntentForPackage(context.packageName))
            .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
        val activity = instrumentation.waitForMonitorWithTimeout(monitor, 20_000) as MainActivity
        instrumentation.removeMonitor(monitor)
        try {
            val task = context.getSystemService(android.app.ActivityManager::class.java).appTasks.mapNotNull { it.taskInfo }.single { it.taskId == activity.taskId }
            assertEquals("Switching home screen icons never removes the window's task", MainActivity::class.java.name, task.baseIntent.component?.className)
        } finally {
            instrumentation.runOnMainSync { activity.finish() }
            compose.waitUntil(10_000) { activity.isDestroyed }
        }
    }

    @Test fun splashMarkFitsSystemSafeCircle() {
        val context = instrumentation.targetContext
        val size = 288
        val themes = context.resources.obtainTypedArray(R.array.capy_launch_splash_themes)
        try {
            if (android.os.Build.VERSION.SDK_INT >= 31) {
                val empty = context.obtainStyledAttributes(R.style.Theme_CapyCanvas, intArrayOf(android.R.attr.windowSplashScreenAnimatedIcon))
                try { assertTrue("The default system splash shows no capy", ink(empty.getDrawable(0)!!, size).none { it }) } finally { empty.recycle() }
            }
            for ((index, pose) in launchMarks().withIndex()) {
                val (icon, mark) = pose
                val splash = context.resources.getIdentifier("capy_splash_mark_${icon.replace('-', '_')}", "drawable", context.packageName)
                if (android.os.Build.VERSION.SDK_INT >= 31) {
                    val theme = context.obtainStyledAttributes(themes.getResourceId(index, 0), intArrayOf(android.R.attr.windowSplashScreenAnimatedIcon))
                    try { assertEquals("$icon theme uses its splash mark", splash, theme.getResourceId(0, 0)) } finally { theme.recycle() }
                }
                val ink = ink(context.getDrawable(splash)!!, size, context.getExternalFilesDir(null)!!.resolve("splash-safe-circle-$icon.png"))
                val radius = ink.indices.filter { ink[it] }.maxOf {
                    kotlin.math.hypot(it % size + .5 - size / 2, it / size + .5 - size / 2)
                }
                assertTrue("The whole $icon mark fits the 192px safe circle: radius=$radius", radius <= 96)
                assertTrue("The $icon mark uses the safe circle: radius=$radius", radius >= 94)
                fun extent(ink: BooleanArray, width: Int): android.graphics.Rect {
                    val inked = ink.indices.filter { ink[it] }
                    return android.graphics.Rect(inked.minOf { it % width }, inked.minOf { it / width }, inked.maxOf { it % width } + 1, inked.maxOf { it / width } + 1)
                }
                val full = context.getDrawable(mark)!!
                val shown = extent(ink, size)
                val whole = extent(ink(full, size), size)
                val scale = shown.width().toDouble() / whole.width()
                assertEquals("The $icon splash mark keeps the capy's proportions", shown.height().toDouble(), whole.height() * scale, 2.0)
                assertEquals("The $icon splash mark keeps the whole capy", 1.0,
                    ink.count { it }.toDouble() / ink(full, (size * scale).roundToInt()).count { it }, .03)
            }
        } finally { themes.recycle() }
    }

    @OptIn(androidx.compose.ui.InternalComposeUiApi::class)
    @Test fun removedSystemAnimationsStillReachTheFirstCanvas() {
        val theme = startupTheme()
        val key = Settings.Global.ANIMATOR_DURATION_SCALE
        val previous = Settings.Global.getString(instrumentation.targetContext.contentResolver, key)
        check(previous == null || previous.toFloatOrNull() != null)
        try {
            shell("settings put global $key 0")
            WindowRecomposerPolicy.withFactory(WindowRecomposerFactory.LifecycleAware) {
                ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                    val host = scenario.activity().host
                    host.awaitMain("first canvas with system animations removed", 60_000, { host.snapshot.toString() }, compose) {
                        host.surfaceReady && host.snapshot?.objectOrNull("launch")?.optBoolean("finished") == true
                    }
                    instrumentation.runOnMainSync {
                        assertNull(host.failure)
                        assertTrue(host.surfaceReady)
                        assertEquals(theme, host.snapshot!!.getJSONObject("state").getString("theme"))
                    }
                }
            }
        } finally {
            shell(if (previous == null) "settings delete global $key" else "settings put global $key $previous")
        }
    }

    @Test fun workspaceFailureBeforeFirstCanvasAllowsRetry() = workspaceFailureBeforeFirstCanvas(false)

    @Test fun workspaceFailureBeforeFirstCanvasAllowsSaveAsNew() = workspaceFailureBeforeFirstCanvas(true)

    private fun workspaceFailureBeforeFirstCanvas(saveAsNew: Boolean) {
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        lateinit var host: CanvasHost
        CanvasHost.beforeGpuAttachForTest = {
            entered.countDown()
            check(resume.await(20, TimeUnit.SECONDS))
            val handle = CanvasHost::class.java.getDeclaredField("handle").apply { isAccessible = true }.getLong(host)
            val deadline = SystemClock.uptimeMillis() + 20_000
            do {
                val view = JSONObject(Native.workspace(handle, obj("type" to "tick").toString())).getJSONObject("view")
                if (view.optBoolean("ready") && !view.optBoolean("busy")) break
                check(SystemClock.uptimeMillis() < deadline)
                SystemClock.sleep(10)
            } while (true)
            val failed = JSONObject(Native.workspace(handle, obj("type" to "submit", "name" to "Unopened").toString())).getJSONObject("view")
            check(!failed.isNull("error"))
        }
        var scenario: ActivityScenario<MainActivity>? = null
        try {
            val theme = startupTheme()
            scenario = ActivityScenario.launch(MainActivity::class.java)
            scenario.onActivity { host = it.host }
            compose.waitUntil(20_000) { entered.count == 0L }
            resume.countDown()
            compose.waitUntil(30_000) { host.workspaceManager?.isNull("error") == false && host.snapshot?.objectOrNull("launch")?.optBoolean("visible") == false }
            compose.runOnIdle {
                assertFalse(host.snapshot!!.getJSONObject("launch").getBoolean("finished"))
                assertNull(host.failure)
            }
            compose.onNodeWithTag("launch-splash").assertDoesNotExist()
            val phone = host.snapshot!!.getJSONObject("layout").objectOrNull("phone") != null
            val action = if (saveAsNew) {
                if (phone) "workspace-status-action" else "workspace-error-action"
            } else if (phone) "workspace-status-retry" else "workspace-error-retry"
            compose.onNodeWithTag(action, useUnmergedTree = true).assertIsDisplayed().assertIsEnabled()
            screenshot("startup-workspace-error-$theme-${if(saveAsNew) "save" else "retry"}.png")
            compose.onNodeWithTag(action, useUnmergedTree = true).performTouchInput { click() }
            if (saveAsNew) {
                compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("workspace-name")), useUnmergedTree = true)
                    .assertIsDisplayed().performTextReplacement("Launch recovery")
                compose.runOnIdle {
                    assertFalse(host.snapshot!!.getJSONObject("launch").getBoolean("visible"))
                    assertFalse(host.snapshot!!.getJSONObject("launch").getBoolean("finished"))
                }
                screenshot("startup-workspace-form-$theme.png")
                compose.onNodeWithTag("workspace-submit", useUnmergedTree = true).assertIsEnabled().performTouchInput { click() }
            }
            compose.waitUntil(60_000) { host.workspaceManager?.isNull("error") == true && host.snapshot?.objectOrNull("launch")?.optBoolean("finished") == true }
            awaitLaunchReveal(scenario.activity())
            compose.onNodeWithTag("launch-splash").assertDoesNotExist()
            screenshot("startup-workspace-recovered-$theme-${if(saveAsNew) "save" else "retry"}.png")
        } finally {
            resume.countDown()
            CanvasHost.beforeGpuAttachForTest = null
            scenario?.close()
        }
    }

    @Test fun splashShowsOnlyTheSavedCapyAndYieldsToHostDialogs() {
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        CanvasHost.beforeGpuAttachForTest = {
            entered.countDown()
            check(resume.await(30, TimeUnit.SECONDS)) { "Test did not release GPU initialization" }
        }
        val context = instrumentation.targetContext
        var scenario: ActivityScenario<MainActivity>? = null
        try {
            val theme = startupTheme()
            val saved = context.getSharedPreferences(CanvasHost.preferencesName, 0)
            assertTrue(saved.edit().putString("settings", JSONObject(saved.getString("settings", null)!!).put("zen_icon", "sleeping").toString()).commit())
            scenario = ActivityScenario.launch(MainActivity::class.java)
            val activity = scenario.activity()
            val host = activity.host
            compose.waitUntil(20_000) { entered.count == 0L }
            compose.waitUntil(5_000) {
                host.bootstrap != null && (activity.launchSplash.nativeActive ||
                    compose.onAllNodesWithTag("launch-splash-mark").fetchSemanticsNodes().isNotEmpty())
            }
            compose.runOnIdle { assertEquals("zen-sleeping", host.bootstrap!!.getJSONObject("launch_mark").getString("icon")) }
            if (activity.launchSplash.nativeActive) compose.runOnIdle {
                assertEquals("The platform splash shows the saved capy", "zen-sleeping", host.systemSplash)
            } else {
                compose.waitForIdle()
                SystemClock.sleep(250)
                val bounds = compose.onNodeWithTag("launch-splash-mark").fetchSemanticsNode().boundsInWindow
                val origin = IntArray(2)
                scenario.onActivity { it.window.decorView.getLocationOnScreen(origin) }
                val size = bounds.width.roundToInt()
                val left = origin[0] + bounds.left.roundToInt()
                val top = origin[1] + bounds.top.roundToInt()
                var shown = BooleanArray(0)
                screenshot("validation/launch/saved-capy-$theme.png") { screen ->
                    shown = BooleanArray(size * size) { index ->
                        val pixel = screen.getPixel(left + index % size, top + index / size)
                        fun distance(color: Int) = listOf(16, 8, 0).sumOf { kotlin.math.abs((pixel shr it and 255) - (color shr it and 255)) }
                        distance(activity.launchSplash.foreground) < distance(activity.launchSplash.background)
                    }
                }
                assertEquals("The splash was captured", size * size, shown.size)
                val mark = launchMarks().single { it.first == "zen-sleeping" }.second
                val expected = ink(context.getDrawable(mark)!!, size, alpha = 127)
                val union = shown.indices.count { shown[it] || expected[it] }
                val matched = shown.indices.count { shown[it] && expected[it] }
                assertTrue("The splash draws the saved sleeping capy: $matched of $union", matched >= union * 9 / 10)
            }
            if (android.os.Build.VERSION.SDK_INT >= 31) compose.waitUntil(5_000) {
                context.getSharedPreferences("capy-launch-splash", 0).getString("icon", null) == "zen-sleeping"
            }
            pressKey(android.view.KeyEvent.KEYCODE_K, android.view.KeyEvent.META_CTRL_ON or android.view.KeyEvent.META_CTRL_LEFT_ON)
            compose.runOnIdle {
                assertNull("Shortcuts wait for the reveal", host.commandSearch)
                assertTrue(host.launchCovered())
            }
            compose.runOnIdle { host.reportActionError("Recovery needs attention") }
            compose.runOnIdle {
                assertTrue("An error without a workspace to show it keeps the splash", host.launchCovered())
            }
            assertTrue(compose.onAllNodesWithTag("action-error", useUnmergedTree = true).fetchSemanticsNodes().isEmpty())
            resume.countDown()
            compose.waitUntil(30_000) {
                host.snapshot?.getJSONObject("launch")?.let { !it.optBoolean("visible") && !it.optBoolean("finished") } == true &&
                    !activity.launchSplash.nativeActive && compose.onAllNodesWithTag("launch-splash").fetchSemanticsNodes().isEmpty() &&
                    compose.onAllNodesWithTag("action-error", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
            }
            screenshot("validation/launch/host-dialog-$theme.png")
            compose.runOnIdle { host.clearActionError() }
            compose.waitUntil(60_000) { host.snapshot?.objectOrNull("launch")?.optBoolean("finished") == true }
            awaitLaunchReveal(activity)
        } finally {
            resume.countDown()
            CanvasHost.beforeGpuAttachForTest = null
            scenario?.close()
        }
    }

    @Test fun splashCoversTheWorkspaceWithoutBlockingCanvasAttachment() {
        val entered = CountDownLatch(1)
        val resume = CountDownLatch(1)
        CanvasHost.beforeGpuAttachForTest = {
            entered.countDown()
            check(resume.await(30, TimeUnit.SECONDS)) { "Test did not release GPU initialization" }
        }
        val night = if (android.os.Build.VERSION.SDK_INT >= 31)
            instrumentation.targetContext.getSystemService(android.app.UiModeManager::class.java) else null
        val previousNight = if (android.os.Build.VERSION.SDK_INT >= 31) night?.nightMode else null
        var scenario: ActivityScenario<MainActivity>? = null
        try {
            val theme = startupTheme()
            if (android.os.Build.VERSION.SDK_INT >= 31) night?.setApplicationNightMode(
                if (theme == "dark") android.app.UiModeManager.MODE_NIGHT_YES else android.app.UiModeManager.MODE_NIGHT_NO)
            scenario = ActivityScenario.launch(MainActivity::class.java)
            InstrumentationRegistry.getArguments().getString("size")?.split("x")?.map(String::toInt)?.let { size ->
                if (size[0] < size[1]) device.portrait(scenario) else device.landscape(scenario)
            }
            lateinit var host: CanvasHost
            lateinit var activity: MainActivity
            scenario.onActivity { activity = it; host = it.host }
            // Advance Compose's test clock while the first surface is mounted.
            compose.waitUntil(20_000) { entered.count == 0L }
            scenario.onActivity {
                val canvas = it.window.decorView.descendant<CanvasSurfaceView>()!!
                assertTrue(canvas.isAttachedToWindow)
                assertTrue(canvas.width > 0 && canvas.height > 0)
            }
            fun nativeCover(): android.window.SplashScreenView? = if (android.os.Build.VERSION.SDK_INT >= 31)
                LaunchSplashWindow::class.java.getDeclaredField("view").apply { isAccessible = true }
                    .get(activity.launchSplash) as? android.window.SplashScreenView else null
            compose.waitUntil(5_000) {
                host.bootstrap != null && (activity.launchSplash.nativeActive ||
                    compose.onAllNodesWithTag("launch-splash-mark").fetchSemanticsNodes().isNotEmpty())
            }
            compose.runOnIdle {
                assertFalse(host.snapshot!!.getBoolean("gpu_ready"))
                assertFalse(host.surfaceReady)
                assertNull(host.failure)
            }
            val cover = android.graphics.Rect()
            val mark = android.graphics.Rect()
            var expected = activity.launchSplash.background
            scenario.onActivity {
                val native = nativeCover()
                if (it.launchSplash.nativeActive && native != null) {
                    native.getGlobalVisibleRect(cover)
                    assertTrue(native.iconView!!.getGlobalVisibleRect(mark))
                    expected = (native.background as android.graphics.drawable.ColorDrawable).color
                } else {
                    it.window.decorView.getGlobalVisibleRect(cover)
                    val density = it.resources.displayMetrics.density
                    val size = Native.launchMarkSize(cover.width() / density, cover.height() / density) * density
                    mark.set((cover.exactCenterX() - size / 2).toInt(), (cover.exactCenterY() - size / 2).toInt(),
                        (cover.exactCenterX() + size / 2).toInt(), (cover.exactCenterY() + size / 2).toInt())
                }
            }
            val point = Offset(cover.left + cover.width() * .15f, cover.top + cover.height() * .15f)
            compose.onNodeWithContentDescription("Main Menu").assertDoesNotExist()
            val captionDelay = host.bootstrap!!.getLong("launch_caption_delay_ms")
            val captionAt = activity.launchSplash.shownAt + captionDelay
            fun beforeCaption(): Boolean {
                val captions = compose.onAllNodesWithTag("launch-splash-caption").fetchSemanticsNodes()
                val before = SystemClock.uptimeMillis() < captionAt
                if (before) {
                    assertTrue("The shared delay has not elapsed", captions.isEmpty())
                    scenario.onActivity {
                        val caption = LaunchSplashWindow::class.java.getDeclaredField("caption").apply { isAccessible = true }
                            .get(it.launchSplash) as? android.widget.TextView
                        assertTrue(caption == null || caption.text.isEmpty())
                    }
                }
                return before
            }
            fun awaitSplashFrame() {
                val committed = CountDownLatch(1)
                scenario.onActivity {
                    val owner = nativeCover() ?: it.window.decorView
                    owner.viewTreeObserver.registerFrameCommitCallback { committed.countDown() }
                    owner.invalidate()
                }
                assertTrue("The splash capture follows its rendered frame", committed.await(5, TimeUnit.SECONDS))
            }
            var markReference: IntArray? = null
            var markBottom = 0
            fun splashScreenshot(name: String, captionShown: Boolean) {
                awaitSplashFrame()
                scenario.onActivity {
                    if (it.launchSplash.nativeActive) {
                        val current = android.graphics.Rect()
                        assertTrue(nativeCover()!!.iconView!!.getGlobalVisibleRect(current))
                        assertEquals("The retained platform icon keeps its original bounds", mark, current)
                    }
                }
                val bitmap = instrumentation.uiAutomation.takeScreenshot()!!
                try {
                    instrumentation.targetContext.getExternalFilesDir(null)!!.resolve(name).outputStream().use {
                        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
                    }
                    assertEquals("Splash covers the native canvas before Vulkan exists", expected, bitmap.getPixel(point.x.toInt(), point.y.toInt()))
                    val markPixels = (mark.top until mark.bottom).sumOf { y ->
                        (mark.left until mark.right).count { x -> bitmap.getPixel(x, y) != expected }
                    }
                    assertTrue("The capy mark is visible at its launch bounds", markPixels > 100)
                    val density = instrumentation.targetContext.resources.displayMetrics.density
                    if (!captionShown) {
                        val pixels = IntArray(mark.width() * mark.height())
                        bitmap.getPixels(pixels, 0, mark.width(), mark.left, mark.top, mark.width(), mark.height())
                        markReference = pixels
                        markBottom = mark.top + (mark.height() - 1 downTo 0).first { y ->
                            (0 until mark.width()).any { x -> pixels[y * mark.width() + x] != expected }
                        } + 1
                    } else {
                        val captionBounds = android.graphics.Rect()
                        scenario.onActivity {
                            val label = LaunchSplashWindow::class.java.getDeclaredField("caption").apply { isAccessible = true }
                                .get(it.launchSplash) as? android.widget.TextView
                                ?: it.window.decorView.descendant<LaunchCaptionTextView>()!!
                            assertTrue(label.getGlobalVisibleRect(captionBounds))
                            val padding = kotlin.math.ceil(label.textSize).toInt()
                            captionBounds.inset(-padding, -padding)
                            assertTrue(captionBounds.intersect(cover))
                        }
                        fun before(x: Int, y: Int) = if (mark.contains(x, y))
                            markReference!![(y - mark.top) * mark.width() + x - mark.left] else expected
                        fun differs(a: Int, b: Int) = listOf(0, 8, 16).any { shift ->
                            kotlin.math.abs((a shr shift and 255) - (b shr shift and 255)) >= 4
                        }
                        val inkTop = (captionBounds.top until captionBounds.bottom).firstOrNull { y ->
                            (captionBounds.left until captionBounds.right).any { x -> differs(bitmap.getPixel(x, y), before(x, y)) }
                        }
                        assertNotNull("The delayed caption is visible", inkTop)
                        val expectedTop = markBottom + host.bootstrap!!.getDouble("launch_caption_gap") * density
                        assertTrue("Caption ink uses the shared gap", kotlin.math.abs(inkTop!! - expectedTop) <= density)
                    }
                } finally { bitmap.recycle() }
            }
            assertTrue(beforeCaption())
            compose.waitUntil(captionDelay) {
                beforeCaption()
                SystemClock.uptimeMillis() - activity.launchSplash.shownAt >= captionDelay * 3 / 4
            }
            splashScreenshot("startup-splash-mark-only-$theme.png", false)
            assertTrue(beforeCaption())
            compose.waitUntil(captionDelay) { !beforeCaption() }
            compose.mainClock.advanceTimeBy(captionDelay)
            compose.waitUntil(captionDelay) {
                if (activity.launchSplash.nativeActive) {
                    var showing = false
                    scenario.onActivity {
                        val caption = LaunchSplashWindow::class.java.getDeclaredField("caption").apply { isAccessible = true }
                            .get(it.launchSplash) as? android.widget.TextView
                        showing = caption != null && caption.text.isNotEmpty() && caption.alpha == 1f
                    }
                    showing
                } else compose.onAllNodesWithTag("launch-splash-caption").fetchSemanticsNodes().isNotEmpty()
            }
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
            splashScreenshot("startup-splash-without-gpu-$theme.png", true)
            resume.countDown()
            compose.waitUntil(60_000) { host.snapshot?.objectOrNull("launch")?.optBoolean("finished") == true }
            awaitLaunchReveal(activity)
            compose.runOnIdle {
                assertTrue(host.snapshot!!.getJSONObject("state").getBoolean("workspace_ready"))
                val layout = host.snapshot!!.getJSONObject("layout")
                val phone = layout.objectOrNull("phone")
                if (phone == null) assertTrue(layout.array("groups").length() > 0)
                else {
                    assertTrue(phone.getJSONObject("capy").getDouble("width") > 0)
                    assertTrue(phone.getJSONObject("capy").getDouble("height") > 0)
                }
                assertEquals("builtin:workspace:painter", host.workspaceManager!!.getString("id"))
                assertEquals("Sketch", host.workspaceManager!!.getString("name"))
                InstrumentationRegistry.getArguments().getString("size")?.let { size ->
                    val pixels = size.split("x").map(String::toDouble)
                    val density = activity.resources.displayMetrics.density
                    for (axis in 0..1) assertEquals("Phone viewport axis $axis", pixels[axis] / density,
                        layout.getJSONArray("viewport").getDouble(axis), .01)
                }
            }
            awaitSplashFrame()
            val phone = host.snapshot!!.getJSONObject("layout").objectOrNull("phone") != null
            val menu = if (phone) compose.onNodeWithTag("phone-capy", useUnmergedTree = true)
                else compose.onNodeWithContentDescription("Main Menu")
            menu.assertIsDisplayed().performClick()
            host.drain();compose.waitForIdle()
            val menuItems = hasAnyAncestor(hasTestTag(if (phone) "phone-menu" else "workspace-menu"))
            val viewItem = hasText("View") and menuItems
            compose.waitUntil(5_000) { compose.onAllNodes(viewItem, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
            val view = compose.onNode(viewItem, useUnmergedTree = true).performScrollTo()
            compose.waitUntil(5_000) { view.isDisplayed() }
            view.assertIsDisplayed()
            view.performClick()
            host.drain();compose.waitForIdle()
            val fitItem = hasText("Fit canvas") and menuItems
            compose.waitUntil(5_000) { compose.onAllNodes(fitItem, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() }
            val fit = compose.onNode(fitItem, useUnmergedTree = true).performScrollTo()
            compose.waitUntil(5_000) { fit.isDisplayed() }
            fit.assertIsDisplayed()
            android.util.Log.i("CapyStartupTest", "splash_ui_without_gpu boot_ns=${SystemClock.elapsedRealtimeNanos()}")
            compose.waitUntil(60_000) { host.surfaceReady || host.failure != null }
            compose.runOnIdle { assertNull(host.failure) }
            compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
            compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
            // The retained GPU must not make a replacement SurfaceView reveal
            // an empty buffer on Activity recreation.
            scenario.recreate()
            compose.waitUntil(30_000) { host.surfaceReady || host.failure != null }
            compose.runOnIdle { assertNull(host.failure) }
            compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
        } finally {
            resume.countDown()
            CanvasHost.beforeGpuAttachForTest = null
            scenario?.close()
            if (android.os.Build.VERSION.SDK_INT >= 31 && previousNight != null) night?.setApplicationNightMode(previousNight)
        }
    }

    @Test fun startupFailureRetainsLongCauseAndRestartStaysReachable() {
        val directory = AppStorage.directoryForTest
        try {
            for (theme in listOf("light", "dark")) {
                AppStorage.directoryForTest = File(device.root, "startup-failure-$theme").apply { mkdirs() }
                val cause = IllegalStateException("Vulkan attach diagnostic\n" + (1..60).joinToString("\n") {
                    "Driver detail $it: preserved native cause for renderer initialization"
                })
                val diagnostic = cause.message!!
                CanvasHost.beforeGpuAttachForTest = { throw cause }
                try {
                    ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                        val activity = scenario.activity()
                        val host = activity.host
                        compose.waitUntil(20_000) { host.failure != null && host.workspaceManager?.optBoolean("ready") == true }
                        compose.runOnIdle {
                            assertEquals(diagnostic, host.failure)
                            host.dispatch(obj("type" to "set_theme", "theme" to theme))
                        }
                        compose.waitUntil(10_000) { host.snapshot?.optJSONObject("state")?.optString("theme") == theme }
                        assertNull(host.actionError)
                        compose.onNodeWithTag("launch-splash").assertDoesNotExist()
                        assertFalse(host.snapshot!!.getJSONObject("launch").getBoolean("finished"))
                        compose.onNodeWithText(host.bootstrap!!.getString("canvas_init_failed")).assertIsDisplayed()
                        val detail = compose.onNodeWithText(diagnostic, useUnmergedTree = true)
                        detail.assertIsDisplayed().assertTextEquals(diagnostic)
                        val viewport = detail.fetchSemanticsNode().boundsInRoot
                        assertTrue("Long diagnostic keeps a bounded viewport", viewport.height <= 180f * activity.resources.displayMetrics.density + 1f)
                        val scroller = compose.onNode(hasScrollAction() and (hasText(diagnostic) or hasAnyDescendant(hasText(diagnostic))), useUnmergedTree = true)
                        assertTrue("The entire cause remains available by scrolling",
                            scroller.fetchSemanticsNode().config[SemanticsProperties.VerticalScrollAxisRange].maxValue() > 0f)
                        scroller.performTouchInput { swipeUp() }
                        val restart = compose.onNodeWithText(host.bootstrap!!.getString("restart_canvas"))
                        restart.assertIsDisplayed().assertHasClickAction()
                        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
                        val button = restart.fetchSemanticsNode().boundsInRoot
                        assertTrue("Restart stays within the editor", root.contains(button.topLeft) && root.contains(button.bottomRight))
                        screenshot("startup-failure-$theme.png")
                        val epoch = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch")
                        CanvasHost.beforeGpuAttachForTest = null
                        restart.performClick()
                        compose.waitUntil(60_000) {
                            host.failure == null && host.surfaceReady && host.snapshot?.optBoolean("brush_ready") == true &&
                                host.snapshot?.objectOrNull("launch")?.optBoolean("finished") == true
                        }
                        compose.runOnIdle {
                            assertEquals(epoch, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").getLong("epoch"))
                        }
                        detail.assertDoesNotExist()
                        compose.onNodeWithTag("canvas-placeholder").assertDoesNotExist()
                    }
                } finally { CanvasHost.beforeGpuAttachForTest = null }
            }
        } finally { AppStorage.directoryForTest = directory }
    }

    @Test fun unsupportedGpuExplainsTheDeviceInTheCanvasArea() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("unsupportedGpu") == "true")
        for (theme in listOf("light", "dark")) {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                val activity = scenario.activity()
                val host = activity.host
                compose.waitUntil(60_000) { host.failure != null && host.workspaceManager?.optBoolean("ready") == true }
                compose.runOnIdle { host.dispatch(obj("type" to "set_theme", "theme" to theme)) }
                compose.waitUntil(10_000) { host.snapshot?.optJSONObject("state")?.optString("theme") == theme }
                assertNull(host.actionError)
                val failure = host.failure!!
                assertTrue(failure, failure.startsWith("This device’s graphics processor (GPU) can’t run Capy Canvas. Updating the system or graphics driver may help.\n\n"))
                assertFalse(failure, failure.contains("java.lang"))
                compose.onNodeWithText(host.bootstrap!!.getString("canvas_init_failed")).assertIsDisplayed()
                compose.onNodeWithText(failure, useUnmergedTree = true).assertIsDisplayed()
                compose.waitForIdle()
                val screen = InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
                File(activity.getExternalFilesDir(null), "unsupported-gpu-$theme.png").outputStream().use {
                    screen.compress(Bitmap.CompressFormat.PNG, 100, it)
                }
                screen.recycle()
            }
        }
    }
}
