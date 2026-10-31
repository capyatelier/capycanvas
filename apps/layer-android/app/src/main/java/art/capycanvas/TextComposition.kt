package art.capycanvas

import android.view.KeyEvent
import android.view.Window
import androidx.compose.ui.text.input.TextFieldValue

internal class TextComposition {
    @Volatile private var owner: Any? = null
    private val candidateKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    fun update(owner: Any, value: TextFieldValue, focused: Boolean) {
        if (focused && value.composition != null) this.owner = owner else clear(owner)
    }
    fun clear(owner: Any) { if (this.owner === owner) this.owner = null }
    fun window(window: Window): () -> Unit {
        val previous = window.callback
        val input = (previous as? Input)?.takeIf { it.composition === this }
            ?: Input(requireNotNull(previous), this).also { window.callback = it }
        input.fields++
        return { if (--input.fields == 0 && window.callback === input) window.callback = input.previous }
    }
    private class Input(val previous: Window.Callback, val composition: TextComposition) : Window.Callback by previous {
        var fields = 0
        var ordinaryKey = false
        override fun dispatchKeyEvent(event: KeyEvent): Boolean {
            if (fields > 0 && composition.active && composition.consumes(event)) return true
            val previousKey = ordinaryKey
            ordinaryKey = !composition.active
            return try { previous.dispatchKeyEvent(event) } finally { ordinaryKey = previousKey }
        }
    }
    fun canAct(window: Window): Boolean = !active || (window.callback as? Input)?.let { it.composition === this && it.ordinaryKey } == true
    private fun consumes(event: KeyEvent): Boolean = when (event.keyCode) {
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER, KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_BACK,
        KeyEvent.KEYCODE_TAB, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN,
        KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT -> owns(event)
        else -> false
    }
    val active: Boolean get() = owner != null || candidateKeys.isNotEmpty()
    fun owns(event: KeyEvent): Boolean {
        val owned = active || event.flags and KeyEvent.FLAG_SOFT_KEYBOARD != 0
        if (event.action == KeyEvent.ACTION_UP) return candidateKeys.remove(event.keyCode) || owned
        if (owned && event.action == KeyEvent.ACTION_DOWN) candidateKeys.add(event.keyCode)
        return owned
    }
}
