package art.capycanvas

import android.annotation.SuppressLint
import android.content.res.Configuration
import android.os.Bundle
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.ReportDrawnWhen
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels

class MainActivity : ComponentActivity() {
    val host: CanvasHost by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        updateTheme(resources.configuration)
        setContent {
            ReportDrawnWhen { host.snapshot?.optBoolean("brush_ready") == true }
            CapyApp(host)
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
    // This is Activity's public Window.Callback override. AndroidX's internal
    // superclass carries a class-wide restriction that lint also inherits here.
    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        host.key(event)
        return super.dispatchKeyEvent(event)
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) host.input(obj("type" to "blur"))
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
