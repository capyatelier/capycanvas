package art.capycanvas

import android.view.KeyEvent
import android.view.Window
import androidx.compose.ui.text.input.TextFieldValue

internal class TextComposition {
    @Volatile private var owner: Any? = null
    private val candidateKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    private val passedKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    private var enter: Pair<Any, () -> Unit>? = null
    fun update(owner: Any, value: TextFieldValue, focused: Boolean, commit: (() -> Unit)? = null) {
        if (focused && value.composition != null) this.owner = owner else if (this.owner === owner) this.owner = null
        if (focused && commit != null) enter = owner to commit
        if (!focused) clear(owner)
    }
    fun clear(owner: Any) {
        if (this.owner === owner) this.owner = null
        if (enter?.first === owner) enter = null
    }
    fun window(window: Window): () -> Unit {
        val previous = window.callback
        val input = (previous as? Input)?.takeIf { it.composition === this }
            ?: Input(requireNotNull(previous), this).also { window.callback = it }
        input.fields++
        return { if (--input.fields == 0 && window.callback === input) window.callback = input.previous }
    }
    private class Input(val previous: Window.Callback, val composition: TextComposition) : Window.Callback by previous {
        var fields = 0
        override fun dispatchKeyEvent(event: KeyEvent): Boolean {
            if (fields > 0 && composition.active && composition.consumes(event)) return true
            if (fields > 0 && event.keyCode in listOf(KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER)) {
                composition.owns(event)
                if (event.action == KeyEvent.ACTION_DOWN) composition.enter?.second?.invoke()
            }
            return previous.dispatchKeyEvent(event)
        }
    }
    private fun consumes(event: KeyEvent): Boolean = when (event.keyCode) {
        KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_BACK,
        KeyEvent.KEYCODE_TAB, KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN,
        KeyEvent.KEYCODE_DPAD_LEFT, KeyEvent.KEYCODE_DPAD_RIGHT -> owns(event)
        else -> false
    }
    val active: Boolean get() = owner != null || candidateKeys.isNotEmpty() || passedKeys.isNotEmpty()
    fun owns(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_ENTER || event.keyCode == KeyEvent.KEYCODE_NUMPAD_ENTER) {
            if (event.action == KeyEvent.ACTION_DOWN) passedKeys.add(event.keyCode)
            if (event.action == KeyEvent.ACTION_UP) passedKeys.remove(event.keyCode)
            return false
        }
        val owned = owner != null || candidateKeys.isNotEmpty() || event.flags and KeyEvent.FLAG_SOFT_KEYBOARD != 0
        if (event.action == KeyEvent.ACTION_UP) return candidateKeys.remove(event.keyCode) || owned
        if (owned && event.action == KeyEvent.ACTION_DOWN) candidateKeys.add(event.keyCode)
        return owned
    }
}
