package art.capycanvas

import android.view.KeyEvent
import android.view.Window
import androidx.compose.ui.text.input.TextFieldValue

internal class TextComposition {
    @Volatile private var owner: Any? = null
    private val candidateKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    private val passedKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    private var commit: Pair<Any, () -> Unit>? = null
    fun update(owner: Any, value: TextFieldValue, focused: Boolean, commit: (() -> Unit)? = null) {
        if (focused && value.composition != null) this.owner = owner else if (this.owner === owner) this.owner = null
        if (focused && commit != null) this.commit = owner to commit
        if (!focused) clear(owner)
    }
    fun clear(owner: Any) {
        if (this.owner === owner) this.owner = null
        if (commit?.first === owner) commit = null
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
            if (fields > 0) {
                if (composition.candidateKeys.contains(event.keyCode) || event.keyCode == KeyEvent.KEYCODE_UNKNOWN) {
                    if (composition.owns(event)) return true
                } else if (event.action == KeyEvent.ACTION_DOWN) {
                    composition.passedKeys.add(event.keyCode)
                    if (!KeyEvent.isModifierKey(event.keyCode)) composition.commit?.second?.invoke()
                }
            }
            return try { previous.dispatchKeyEvent(event) } finally {
                if (event.action == KeyEvent.ACTION_UP) composition.passedKeys.remove(event.keyCode)
            }
        }
    }
    val active: Boolean get() = owner != null || candidateKeys.isNotEmpty() || passedKeys.isNotEmpty()
    fun owns(event: KeyEvent): Boolean {
        if (passedKeys.contains(event.keyCode)) {
            if (event.action == KeyEvent.ACTION_UP) passedKeys.remove(event.keyCode)
            return false
        }
        val owned = owner != null || candidateKeys.isNotEmpty() || event.flags and KeyEvent.FLAG_SOFT_KEYBOARD != 0
        if (event.action == KeyEvent.ACTION_UP) return candidateKeys.remove(event.keyCode) || owned
        if (owned && event.action == KeyEvent.ACTION_DOWN) candidateKeys.add(event.keyCode)
        return owned
    }
}
