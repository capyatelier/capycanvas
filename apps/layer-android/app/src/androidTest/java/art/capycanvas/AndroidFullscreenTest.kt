package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test

class AndroidFullscreenTest {
    private fun canvas(view: View): CanvasSurfaceView? {
        if (view is CanvasSurfaceView) return view
        if (view is ViewGroup) for (i in 0 until view.childCount) canvas(view.getChildAt(i))?.let { return it }
        return null
    }

    @Test fun canvasUsesSystemBarSpaceOnLaunchAndResume() {
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            fun waitFor(condition: (MainActivity) -> Boolean) {
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
                fail("Fullscreen canvas did not become ready")
            }
            fun fullscreen(activity: MainActivity): Boolean {
                val decor = activity.window.decorView
                val insets = ViewCompat.getRootWindowInsets(decor) ?: return false
                val surface = canvas(decor) ?: return false
                if (!activity.host.surfaceReady || activity.host.snapshot?.optBoolean("brush_ready") != true) return false
                if (insets.isVisible(WindowInsetsCompat.Type.statusBars()) || insets.isVisible(WindowInsetsCompat.Type.navigationBars())) return false
                // Display cutouts remain protected; hidden bars reserve no space.
                val cutout = insets.getInsets(WindowInsetsCompat.Type.displayCutout())
                val origin = IntArray(2).also { surface.getLocationInWindow(it) }
                return origin[0] == cutout.left && origin[1] == cutout.top &&
                    surface.width == decor.width - cutout.left - cutout.right &&
                    surface.height == decor.height - cutout.top - cutout.bottom
            }
            waitFor(::fullscreen)
            val instrumentation = InstrumentationRegistry.getInstrumentation()
            instrumentation.uiAutomation.takeScreenshot()!!.let { bitmap ->
                try {
                    instrumentation.targetContext.getExternalFilesDir(null)!!.resolve("immersive-fullscreen.png").outputStream().use {
                        bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)
                    }
                } finally { bitmap.recycle() }
            }
            // System UI can reveal the bars. A return to the app must restore the
            // fullscreen layout and its retained renderer's replacement surface.
            scenario.onActivity {
                WindowCompat.getInsetsController(it.window, it.window.decorView).show(WindowInsetsCompat.Type.systemBars())
            }
            waitFor {
                val insets = ViewCompat.getRootWindowInsets(it.window.decorView)
                insets != null && insets.isVisible(WindowInsetsCompat.Type.statusBars()) && insets.isVisible(WindowInsetsCompat.Type.navigationBars())
            }
            scenario.moveToState(Lifecycle.State.CREATED)
            scenario.moveToState(Lifecycle.State.RESUMED)
            waitFor(::fullscreen)
            scenario.onActivity {
                val surface = canvas(it.window.decorView)!!
                android.util.Log.i("CapyFullscreenTest", "canvas=${surface.width}x${surface.height} status_and_navigation_hidden=true")
            }
        }
    }
}
