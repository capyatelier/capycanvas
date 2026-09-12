package art.capycanvas

import android.annotation.SuppressLint
import android.content.res.Configuration
import android.app.ActivityManager
import androidx.activity.OnBackPressedCallback
import java.lang.ref.WeakReference
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.ReportDrawnWhen
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : ComponentActivity() {
    companion object {
        private val windows = mutableListOf<WeakReference<MainActivity>>()
        internal fun workspaceSwitcherChanged(source: CanvasHost) {
            windows.removeAll { it.get() == null }
            windows.mapNotNull { it.get()?.host }.distinct().filter { it !== source }.forEach {
                it.workspaceInput(obj("type" to "refresh_switcher"))
            }
        }
        internal fun focusWorkspace(id: String): Boolean {
            windows.removeAll { it.get() == null }
            val activity = windows.firstNotNullOfOrNull { it.get()?.takeIf { a -> a.host.workspaceManager?.optString("id") == id } } ?: return false
            activity.getSystemService(ActivityManager::class.java).appTasks.firstOrNull { it.taskInfo?.taskId == activity.taskId }?.moveToFront()
            activity.window.decorView.requestFocus()
            return true
        }
    }
    val host: CanvasHost by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        windows.add(WeakReference(this))
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() { host.closeWorkspaceWindow { finish() } }
        })
        enableEdgeToEdge()
        enterFullscreen()
        updateTheme(resources.configuration)
        setContent {
            ReportDrawnWhen { host.snapshot?.optBoolean("brush_ready") == true }
            CapyApp(host)
        }
    }
    private fun enterFullscreen() {
        WindowCompat.getInsetsController(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
    }
    private fun updateTheme(config: Configuration) {
        host.dispatch(obj("type" to "system_theme_changed", "theme" to
            if (config.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES) "dark" else "light"))
    }
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        updateTheme(newConfig)
    }
    override fun onStart() {
        super.onStart()
        host.workspaceInput(obj("type" to "resume"))
    }
    override fun onStop() {
        host.workspaceInput(obj("type" to "suspend"))
        super.onStop()
    }
    override fun onDestroy() {
        windows.removeAll { it.get() == null || it.get() === this }
        super.onDestroy()
    }
    // This is Activity's public Window.Callback override. AndroidX's internal
    // superclass carries a class-wide restriction that lint also inherits here.
    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        host.key(event)
        return super.dispatchKeyEvent(event)
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            enterFullscreen()
            host.workspaceInput(obj("type" to "refresh_switcher"))
        } else host.input(obj("type" to "blur"))
    }
}

/** Both the Activity and native dialog windows forward the same key schema. */
internal fun CanvasHost.key(event: KeyEvent) {
    val key = when (event.keyCode) {
            KeyEvent.KEYCODE_SPACE -> " "
            KeyEvent.KEYCODE_ESCAPE -> "escape"
            KeyEvent.KEYCODE_ENTER -> "enter"
            KeyEvent.KEYCODE_TAB -> "tab"
            KeyEvent.KEYCODE_DEL -> "backspace"
            KeyEvent.KEYCODE_FORWARD_DEL -> "delete"
            KeyEvent.KEYCODE_INSERT -> "insert"
            KeyEvent.KEYCODE_MOVE_HOME -> "home"
            KeyEvent.KEYCODE_MOVE_END -> "end"
            KeyEvent.KEYCODE_PAGE_UP -> "pageup"
            KeyEvent.KEYCODE_PAGE_DOWN -> "pagedown"
            in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> "f${event.keyCode - KeyEvent.KEYCODE_F1 + 1}"
            KeyEvent.KEYCODE_DPAD_LEFT -> "arrowleft"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "arrowright"
            KeyEvent.KEYCODE_DPAD_UP -> "arrowup"
            KeyEvent.KEYCODE_DPAD_DOWN -> "arrowdown"
            else -> event.getUnicodeChar(event.metaState and (KeyEvent.META_SHIFT_MASK or KeyEvent.META_CAPS_LOCK_ON))
                .takeIf { it > 0 && Character.isValidCodePoint(it) }?.let { String(Character.toChars(it)) }
        }
        if (key != null) input(obj("type" to "key", "key" to key, "pressed" to (event.action == KeyEvent.ACTION_DOWN),
            "repeat" to (event.repeatCount > 0), "editing" to editingText,
            "modifiers" to obj("command" to (event.isCtrlPressed || event.isMetaPressed), "shift" to event.isShiftPressed, "alt" to event.isAltPressed)))
}
