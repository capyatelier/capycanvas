package art.capycanvas

import android.annotation.SuppressLint
import android.content.res.Configuration
import androidx.activity.OnBackPressedCallback
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
    val host: CanvasHost by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        host.attachWindow(this)
        if(isFinishing)return
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() { host.drawingTabs.closeWindow() }
        })
        enableEdgeToEdge()
        window.enterCanvasFullscreen()
        updateTheme(resources.configuration)
        setContent {
            ReportDrawnWhen { host.snapshot?.optBoolean("brush_ready") == true }
            CapyApp(host)
        }
        if(savedInstanceState==null)openIntent(intent)
    }
    override fun onNewIntent(intent:android.content.Intent) {
        super.onNewIntent(intent);setIntent(intent);openIntent(intent)
    }
    private fun openIntent(intent:android.content.Intent) {
        val uris=when(intent.action) {
            android.content.Intent.ACTION_VIEW->listOfNotNull(intent.data)
            android.content.Intent.ACTION_SEND->listOfNotNull(androidx.core.content.IntentCompat.getParcelableExtra(intent,android.content.Intent.EXTRA_STREAM,android.net.Uri::class.java))
            android.content.Intent.ACTION_SEND_MULTIPLE->androidx.core.content.IntentCompat.getParcelableArrayListExtra(intent,android.content.Intent.EXTRA_STREAM,android.net.Uri::class.java).orEmpty()
            else->emptyList()
        }
        if(uris.isNotEmpty())host.documents.openUris(uris,intent.flags)
    }
    private fun updateTheme(config: Configuration) {
        host.dispatch(obj("type" to "system_theme_changed", "theme" to
            if (config.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES) "dark" else "light",
            "accent" to if (android.os.Build.VERSION.SDK_INT >= 31)
                "#%06x".format(getColor(android.R.color.system_accent1_500) and 0xffffff) else null))
    }
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        updateTheme(newConfig)
    }
    override fun onStart() {
        super.onStart()
        host.filterPreviewCache.resume()
        if (!host.restartingWindow) host.workspaceInput(obj("type" to "resume"))
        host.restartingWindow = false
    }
    override fun onPause() {
        host.input(obj("type" to "blur"))
        super.onPause()
    }
    override fun onStop() {
        host.filterPreviewCache.pause()
        host.recovery.capture()
        host.restartingWindow = isChangingConfigurations
        if (!host.restartingWindow) host.workspaceInput(obj("type" to "suspend"))
        super.onStop()
    }
    override fun onDestroy() {
        host.detachWindow(this)
        super.onDestroy()
    }
    // This is Activity's public Window.Callback override. AndroidX's internal
    // superclass carries a class-wide restriction that lint also inherits here.
    @SuppressLint("RestrictedApi")
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        host.shaderInput()
        if (host.headerKeyHandler?.invoke(event) == true) return true
        if (host.drawingTabs.key(event)) return true
        if (host.palettes.key(event)) return true
        if (event.action == KeyEvent.ACTION_DOWN && (event.isCtrlPressed || event.isMetaPressed))
            host.dispatch(obj("type" to "command_search", "action" to obj("type" to "focus",
                "focus" to host.commandFocus())))
        host.key(event)
        return super.dispatchKeyEvent(event)
    }
    override fun dispatchTouchEvent(event: android.view.MotionEvent): Boolean {
        host.shaderInput()
        return super.dispatchTouchEvent(event)
    }
    override fun dispatchGenericMotionEvent(event: android.view.MotionEvent): Boolean {
        host.shaderInput()
        return super.dispatchGenericMotionEvent(event)
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            window.enterCanvasFullscreen()
            host.workspaceInput(obj("type" to "refresh_switcher"))
        } else if (!host.pickerPopupOpen) host.input(obj("type" to "blur"))
    }
}

/** Apply the same immersive chrome policy to the editor and its native dialogs. */
internal fun android.view.Window.enterCanvasFullscreen() {
    WindowCompat.getInsetsController(this, decorView).apply {
        systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        hide(WindowInsetsCompat.Type.systemBars())
    }
}

/** Both the Activity and native dialog windows forward the same key schema. */
internal fun CanvasHost.key(event: KeyEvent) {
    if (rangeControlFocus != null && event.keyCode in listOf(KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT,
        KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN, KeyEvent.KEYCODE_PAGE_UP, KeyEvent.KEYCODE_PAGE_DOWN,
        KeyEvent.KEYCODE_MOVE_HOME, KeyEvent.KEYCODE_MOVE_END, KeyEvent.KEYCODE_ESCAPE)) return
    if (colorControlFocus != null && event.keyCode in listOf(KeyEvent.KEYCODE_SPACE, KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER)) return
    val key = when (event.keyCode) {
            KeyEvent.KEYCODE_SHIFT_LEFT, KeyEvent.KEYCODE_SHIFT_RIGHT -> "shift"
            KeyEvent.KEYCODE_ALT_LEFT, KeyEvent.KEYCODE_ALT_RIGHT -> "alt"
            KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_CTRL_RIGHT -> "control"
            KeyEvent.KEYCODE_META_LEFT, KeyEvent.KEYCODE_META_RIGHT -> "meta"
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
