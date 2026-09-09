package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas as ComposeCanvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.caverock.androidsvg.SVG

internal class Palette(val dark: Boolean, private val source: org.json.JSONObject) {
    private fun role(name: String) = Color(android.graphics.Color.parseColor(source.getString(name)))
    val surround = role("bg")
    val panel = role("panel")
    val tabs = role("tabbar")
    val sidebar = role("sidebar")
    val input = role("input")
    val text = role("text")
    val secondary = text.copy(alpha = .55f)
    val accent = Color(0xff3584e4)
    val sliderFill = lerp(panel, text, .5f)
    val active = accent.copy(alpha = .22f)
    val button = role("button").copy(alpha = 13 / 255f)
    val thumb = role("thumb")
    val divider = text.copy(alpha = .12f)
    val settingsBackground = role("settings")
    val settingsCard = role("card")
    val settingsSecondary = role("settings_secondary")
}
internal val LocalPalette = staticCompositionLocalOf<Palette> { error("Missing core palette") }
internal val LocalCanvasHost = staticCompositionLocalOf<CanvasHost> { error("Missing native host") }

/** Editing/composition is native widget state. Rust remains authoritative for
 * accepted values, but an asynchronous acknowledgement must not reset an IME's
 * current edit before the next key arrives. */
@Composable internal fun CoreTextField(value: String, onChange: (String) -> Unit,
    modifier: Modifier = Modifier, label: (@Composable () -> Unit)? = null,
    placeholder: (@Composable () -> Unit)? = null, leadingIcon: (@Composable () -> Unit)? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    height: Dp = 36.dp, enabled: Boolean = true, focusRequest: Long = 0,
    textStyle: TextStyle = LocalTextStyle.current,
    keyboardOptions: androidx.compose.foundation.text.KeyboardOptions = androidx.compose.foundation.text.KeyboardOptions.Default,
    maxLength: Int = Int.MAX_VALUE, onCommit: ((String) -> Unit)? = null) {
    var text by remember { mutableStateOf(TextFieldValue(value, TextRange(value.length))) }
    var focused by remember { mutableStateOf(false) }
    val requester = remember { FocusRequester() }
    val host = LocalCanvasHost.current
    val focusManager = LocalFocusManager.current
    LaunchedEffect(value, focused) { if (!focused) text = TextFieldValue(value, TextRange(value.length)) }
    // A core type-to-search request carries text as well as focus. Apply it
    // even if another key arrived before the native field gained focus.
    LaunchedEffect(focusRequest) {
        if (focusRequest != 0L && value.isNotEmpty()) {
            text = TextFieldValue(value, TextRange(value.length))
            requester.requestFocus()
        }
    }
    DisposableEffect(Unit) { onDispose { if (focused) host.editingText = false } }
    val colors = LocalPalette.current
    Column(modifier.background(colors.input, RoundedCornerShape(6.dp))) {
        label?.let { Box(Modifier.padding(start = 10.dp, top = 6.dp)) { it() } }
        BasicTextField(text, { next ->
            val changed = next.text != text.text
            text = next
            if (next.text.length > maxLength) text = TextFieldValue(next.text.take(maxLength))
            if (changed && onCommit == null) onChange(text.text)
        },
            Modifier.fillMaxWidth().height(height).focusRequester(requester).onFocusChanged {
                if (focused && !it.isFocused) onCommit?.invoke(text.text)
                focused = it.isFocused; host.editingText = focused
            },
            enabled = enabled, singleLine = true, textStyle = textStyle.copy(color = colors.text),
            cursorBrush = SolidColor(colors.accent), keyboardOptions = keyboardOptions.copy(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { focusManager.clearFocus() }),
            decorationBox = { field ->
                Row(Modifier.fillMaxSize().padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    leadingIcon?.invoke()
                    Box(Modifier.weight(1f)) {
                        if (text.text.isEmpty()) ProvideTextStyle(textStyle.copy(color = colors.secondary)) { placeholder?.invoke() }
                        field()
                    }
                    trailingIcon?.invoke()
                }
            })
    }
}

/** The same bank GTK and web ship; no duplicated/redrawn icon definitions. */
@Composable internal fun SharedIcon(name: String, description: String?, modifier: Modifier = Modifier,
    tint: Color = LocalPalette.current.text, fill: Color? = null) {
    val context = LocalContext.current
    val bitmap = remember(name, fill, if (fill != null) tint else null) {
        context.assets.open("layer-$name-symbolic.svg").bufferedReader().use { source ->
            fun hex(color: Color) = "#%06x".format(color.toArgb() and 0xffffff)
            val svg = SVG.getFromString(source.readText().replace("currentColor", if (fill == null) "#ffffff" else hex(tint))
                .replace("#33d17a", fill?.let(::hex) ?: "none"))
            Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888).also { image ->
                svg.documentWidth = 96f; svg.documentHeight = 96f
                svg.renderToCanvas(Canvas(image))
            }.asImageBitmap()
        }
    }
    Image(bitmap, description, modifier.size(16.dp), colorFilter = if (fill == null) ColorFilter.tint(tint) else null)
}

@Composable internal fun PanelGrip(description: String, vertical: Boolean = false) {
    SharedIcon("grip", description, Modifier.alpha(.65f)
        .offset(x = if (vertical) 0.dp else (-1.6).dp, y = if (vertical) (-1.6).dp else 0.dp)
        .rotate(if (vertical) 90f else 0f))
}

/** Native slider gestures/semantics with a 4dp track and an optional 16dp thumb. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun EditorSlider(value: Float, onChange: (Float) -> Unit,
    modifier: Modifier = Modifier, range: ClosedFloatingPointRange<Float> = 0f..1f,
    enabled: Boolean = true, label: String = "", height: Dp = 28.dp,
    inactiveTrackColor: Color = LocalPalette.current.input, showThumb: Boolean = true,
    activeTrackColor: Color = LocalPalette.current.accent,
    onValueChangeFinished: (() -> Unit)? = null) {
    val colors = LocalPalette.current
    val focus = LocalFocusManager.current
    CompositionLocalProvider(LocalMinimumInteractiveComponentSize provides 0.dp) {
        Slider(value.coerceIn(range), { focus.clearFocus(); onChange(it) }, modifier.height(height).semantics { contentDescription = label },
            enabled = enabled, valueRange = range, onValueChangeFinished = onValueChangeFinished,
            thumb = {
                // Material measures the slider from its thumb/track, so reserve
                // the hit height here without enlarging the visible knob.
                Box(Modifier.width(if (showThumb) 16.dp else 0.dp).height(height), contentAlignment = Alignment.Center) {
                    if (showThumb) Box(Modifier.size(16.dp).shadow(2.dp, CircleShape).background(colors.thumb, CircleShape))
                }
            },
            track = { state ->
                ComposeCanvas(Modifier.fillMaxWidth().height(4.dp)) {
                    val radius = CornerRadius(size.height / 2f)
                    drawRoundRect(inactiveTrackColor, cornerRadius = radius)
                    val fraction = (state.value - range.start) / (range.endInclusive - range.start)
                    if (fraction > 0f) drawRoundRect(activeTrackColor.copy(alpha = if (enabled) 1f else .4f),
                        size = Size(size.width * fraction, size.height), cornerRadius = radius)
                }
            })
    }
}

@Composable internal fun EditorCheck(checked: Boolean, label: String, modifier: Modifier = Modifier,
    onChange: (Boolean) -> Unit) {
    val colors = LocalPalette.current
    Box(modifier.size(22.dp, 28.dp).toggleable(checked, role = Role.Checkbox, onValueChange = onChange)
        .semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Box(Modifier.size(16.dp).clip(RoundedCornerShape(4.dp))
            .then(if (checked) Modifier.background(colors.accent) else Modifier.border(2.dp, colors.text.copy(alpha = .35f), RoundedCornerShape(4.dp)))) {
            if (checked) SharedIcon("check", null, tint = Color.White)
        }
    }
}
@Composable internal fun IconTile(name: String, label: String, selected: Boolean = false,
    enabled: Boolean = true, modifier: Modifier = Modifier, onLongClick: (() -> Unit)? = null, fill: Color? = null, iconSize: Dp = 16.dp, selectedColor: Color? = null, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Box(modifier.size(36.dp).alpha(if (enabled) 1f else 0.4f).background(if (selected) selectedColor ?: colors.active else Color.Transparent, RoundedCornerShape(6.dp))
        .combinedClickable(enabled = enabled, role = Role.Button, onClickLabel = label, onLongClick = onLongClick, onClick = onClick), contentAlignment = Alignment.Center) {
        SharedIcon(name, label, modifier = Modifier.size(iconSize), fill = fill)
    }
}

/** A centered, reusable choice grid. Content and selection come from the core. */
@Composable internal fun ImageSelector(options: List<String>, icons: List<String>, columns: Int,
    selected: Int, enabled: Boolean, modifier: Modifier = Modifier, onSelect: (Int) -> Unit) {
    val colors = LocalPalette.current
    Column(modifier.fillMaxWidth().selectableGroup().padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        options.indices.toList().chunked(columns).forEach { indices ->
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                indices.forEach { index ->
                    val active = index == selected
                    val shape = RoundedCornerShape(6.dp)
                    Box(Modifier.size(64.dp).testTag("image-choice-$index").clip(shape)
                        .background(if (active) colors.accent.copy(alpha = .18f) else colors.text.copy(alpha = .05f))
                        .then(if (active) Modifier.border(2.dp, colors.accent, shape) else Modifier)
                        .alpha(if (enabled) 1f else .4f)
                        .selectable(active, enabled = enabled, role = Role.RadioButton) { onSelect(index) },
                        contentAlignment = Alignment.Center) {
                        SharedIcon(icons[index], options[index], Modifier.size(48.dp))
                    }
                }
            }
        }
    }
}
