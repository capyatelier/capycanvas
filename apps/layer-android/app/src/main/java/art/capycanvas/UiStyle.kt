package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.LocalTextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.caverock.androidsvg.SVG

internal data class Palette(val dark: Boolean) {
    val surround = Color(if (dark) 0xff333333 else 0xffb8b8b8)
    val panel = Color(if (dark) 0xff414141 else 0xffededed)
    val tabs = Color(if (dark) 0xff2e2e2e else 0xffdedede)
    val input = Color(if (dark) 0xff333333 else 0xfffafafa)
    val text = Color(if (dark) 0xffeeeeee else 0xff242424)
    val secondary = Color(if (dark) 0xffbbbbbb else 0xff555555)
    val active = Color(if (dark) 0xff375a80 else 0xffc2d9f1)
    val accent = Color(if (dark) 0xff91b9f0 else 0xff2863a8)
}
internal val LocalPalette = staticCompositionLocalOf { Palette(true) }
internal val LocalCanvasHost = staticCompositionLocalOf<CanvasHost> { error("Missing native host") }

/** Editing/composition is native widget state. Rust remains authoritative for
 * accepted values, but an asynchronous acknowledgement must not reset an IME's
 * current edit before the next key arrives. */
@Composable internal fun CoreTextField(value: String, onChange: (String) -> Unit,
    modifier: Modifier = Modifier, label: (@Composable () -> Unit)? = null,
    placeholder: (@Composable () -> Unit)? = null, leadingIcon: (@Composable () -> Unit)? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    keyboardOptions: androidx.compose.foundation.text.KeyboardOptions = androidx.compose.foundation.text.KeyboardOptions.Default) {
    var text by remember { mutableStateOf(value) }
    var focused by remember { mutableStateOf(false) }
    val host = LocalCanvasHost.current
    LaunchedEffect(value, focused) { if (!focused) text = value }
    DisposableEffect(Unit) { onDispose { if (focused) host.editingText = false } }
    OutlinedTextField(text, { text = it; onChange(it) }, modifier.onFocusChanged { focused = it.isFocused; host.editingText = focused },
        singleLine = true, label = label, placeholder = placeholder, leadingIcon = leadingIcon, trailingIcon = trailingIcon,
        keyboardOptions = keyboardOptions, textStyle = LocalTextStyle.current,
        colors = OutlinedTextFieldDefaults.colors(unfocusedContainerColor = LocalPalette.current.input))
}

/** The same bank GTK and web ship; no duplicated/redrawn icon definitions. */
@Composable internal fun SharedIcon(name: String, description: String?, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val bitmap = remember(name) {
        context.assets.open("layer-$name-symbolic.svg").bufferedReader().use { source ->
            val svg = SVG.getFromString(source.readText().replace("currentColor", "#ffffff"))
            Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888).also { image ->
                svg.documentWidth = 96f; svg.documentHeight = 96f
                svg.renderToCanvas(Canvas(image))
            }.asImageBitmap()
        }
    }
    Image(bitmap, description, modifier.size(20.dp), colorFilter = ColorFilter.tint(LocalPalette.current.text))
}
@Composable internal fun IconTile(name: String, label: String, selected: Boolean = false,
    enabled: Boolean = true, modifier: Modifier = Modifier, onLongClick: (() -> Unit)? = null, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(modifier.size(36.dp).alpha(if (enabled) 1f else 0.4f).background(if (selected) colors.active else Color.Transparent, RoundedCornerShape(6.dp))
        .combinedClickable(enabled = enabled, role = Role.Button, onClickLabel = label, onLongClick = onLongClick, onClick = onClick), contentAlignment = Alignment.Center) {
        SharedIcon(name, label)
    }
}
