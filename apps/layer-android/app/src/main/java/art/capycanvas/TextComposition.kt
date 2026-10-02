package art.capycanvas

import android.view.KeyEvent
import androidx.compose.ui.text.input.TextFieldValue

internal class TextComposition {
    private var owner: Any? = null
    fun update(owner: Any, value: TextFieldValue, focused: Boolean) {
        if (focused && value.composition != null) this.owner = owner else clear(owner)
    }
    fun clear(owner: Any) { if (this.owner === owner) this.owner = null }
    val active: Boolean get() = owner != null
    fun owns(event: KeyEvent): Boolean = active || event.flags and KeyEvent.FLAG_SOFT_KEYBOARD != 0
}
