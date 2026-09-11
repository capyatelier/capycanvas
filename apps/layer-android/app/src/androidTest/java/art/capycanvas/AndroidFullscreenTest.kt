package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Rect
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.junit.Assert.*
import org.junit.Test

class AndroidFullscreenTest {
    private fun canvas(view: View): CanvasSurfaceView? {
        if (view is CanvasSurfaceView) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) canvas(view.getChildAt(i))?.let { return it }
        return null
    }

    @Test fun canvasSizeStaysStableFromFirstLayoutThroughSystemBarTransitions() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val samples = mutableListOf<String>()
        val violations = linkedSetOf<String>()
        var phase = "startup"
        // Register before launch: checking only the settled fullscreen state misses
        // both the first smaller surface and the resize during bar animations.
        val callback = ActivityLifecycleCallback { activity, stage ->
            if (activity is MainActivity && stage == Stage.CREATED) {
                val decor = activity.window.decorView
                decor.viewTreeObserver.addOnPreDrawListener {
                    val surface = canvas(decor)
                    val insets = ViewCompat.getRootWindowInsets(decor)
                    if (surface != null && surface.width > 0 && surface.height > 0 && insets != null) {
                        val obstruction = insets.getInsets(WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.captionBar())
                        val waterfall = insets.displayCutout?.waterfallInsets
                        val expected = Rect(maxOf(obstruction.left, waterfall?.left ?: 0),
                            maxOf(obstruction.top, waterfall?.top ?: 0),
                            decor.width - maxOf(obstruction.right, waterfall?.right ?: 0),
                            decor.height - maxOf(obstruction.bottom, waterfall?.bottom ?: 0))
                        val origin = IntArray(2).also { surface.getLocationInWindow(it) }
                        val actual = Rect(origin[0], origin[1], origin[0] + surface.width, origin[1] + surface.height)
                        val buffer = surface.holder.surfaceFrame
                        val sample = "$phase canvas=$actual buffer=${buffer.width()}x${buffer.height()} expected=$expected"
                        if (samples.lastOrNull() != sample) samples.add(sample)
                        // SurfaceView's first pre-draw can precede buffer creation.
                        val bufferMismatch = !buffer.isEmpty && (buffer.width() != surface.width || buffer.height() != surface.height)
                        if (actual != expected || bufferMismatch) {
                            violations.add(sample)
                        }
                    }
                    true
                }
            }
        }
        instrumentation.runOnMainSync {
            ActivityLifecycleMonitorRegistry.getInstance().addLifecycleCallback(callback)
        }
        fun screenshot(name: String) {
            instrumentation.uiAutomation.takeScreenshot()!!.let { bitmap ->
                try {
                    instrumentation.targetContext.getExternalFilesDir(null)!!.resolve("stable-immersive-$name.png").outputStream().use {
                        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
                    }
                } finally { bitmap.recycle() }
            }
        }
        try {
            ActivityScenario.launch(MainActivity::class.java).use { scenario ->
                fun waitFor(description: String, condition: (MainActivity) -> Boolean) {
                    val deadline = SystemClock.uptimeMillis() + 30_000
                    while (SystemClock.uptimeMillis() < deadline) {
                        var ready = false
                        scenario.onActivity {
                            assertNull(it.host.failure)
                            ready = condition(it)
                        }
                        if (ready) return
                        SystemClock.sleep(20)
                    }
                    fail("Timed out waiting for $description")
                }
                fun barsHidden(activity: MainActivity): Boolean {
                    val insets = ViewCompat.getRootWindowInsets(activity.window.decorView) ?: return false
                    return !insets.isVisible(WindowInsetsCompat.Type.statusBars()) &&
                        !insets.isVisible(WindowInsetsCompat.Type.navigationBars())
                }
                waitFor("first canvas and hidden bars") {
                    it.host.surfaceReady && it.host.snapshot?.optBoolean("brush_ready") == true && barsHidden(it)
                }
                screenshot("startup")

                // Exercise visible insets and their animations directly. A
                // transient edge reveal may not report visible insets to the app;
                // ordinary visible bars must not resize Vulkan either.
                scenario.onActivity {
                    phase = "bars-shown"
                    WindowCompat.getInsetsController(it.window, it.window.decorView).show(WindowInsetsCompat.Type.systemBars())
                }
                waitFor("visible system bars") {
                    val insets = ViewCompat.getRootWindowInsets(it.window.decorView)
                    insets != null && insets.isVisible(WindowInsetsCompat.Type.statusBars()) && insets.isVisible(WindowInsetsCompat.Type.navigationBars())
                }
                SystemClock.sleep(500)
                screenshot("bars-shown")
                scenario.onActivity {
                    phase = "bars-hiding"
                    WindowCompat.getInsetsController(it.window, it.window.decorView).hide(WindowInsetsCompat.Type.systemBars())
                }
                waitFor("hidden system bars", ::barsHidden)
                SystemClock.sleep(500)
                scenario.onActivity { phase = "resume" }
                scenario.moveToState(Lifecycle.State.CREATED)
                scenario.moveToState(Lifecycle.State.RESUMED)
                waitFor("restored surface") { it.host.surfaceReady && barsHidden(it) }
                SystemClock.sleep(500)
                scenario.onActivity {
                    assertTrue("Captured the very first canvas layout", samples.any { it.startsWith("startup ") })
                    assertTrue("Captured the canvas with bars visible", samples.any { it.startsWith("bars-shown ") })
                    assertTrue("System bars resized the canvas:\n${violations.joinToString("\n")}", violations.isEmpty())
                }
            }
        } finally {
            instrumentation.runOnMainSync {
                ActivityLifecycleMonitorRegistry.getInstance().removeLifecycleCallback(callback)
                val report = samples.joinToString("\n")
                instrumentation.targetContext.getExternalFilesDir(null)!!.resolve("stable-immersive-layouts.txt").writeText(report)
                android.util.Log.i("CapyFullscreenTest", report)
            }
        }
    }
}
