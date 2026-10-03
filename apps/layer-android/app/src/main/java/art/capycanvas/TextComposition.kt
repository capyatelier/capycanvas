package art.capycanvas

import android.view.KeyEvent
import androidx.compose.ui.text.input.TextFieldValue

internal class TextComposition {
    @Volatile private var owner: Any? = null
    private val candidateKeys = java.util.concurrent.ConcurrentHashMap.newKeySet<Int>()
    fun update(owner: Any, value: TextFieldValue, focused: Boolean) {
        if (focused && value.composition != null) this.owner = owner else clear(owner)
    }
    fun clear(owner: Any) { if (this.owner === owner) this.owner = null }
    val active: Boolean get() = owner != null || candidateKeys.isNotEmpty()
    fun owns(event: KeyEvent): Boolean {
        val owned = active || event.flags and KeyEvent.FLAG_SOFT_KEYBOARD != 0
        if (event.action == KeyEvent.ACTION_UP) return candidateKeys.remove(event.keyCode) || owned
        if (owned && event.action == KeyEvent.ACTION_DOWN) candidateKeys.add(event.keyCode)
        return owned
    }
}
