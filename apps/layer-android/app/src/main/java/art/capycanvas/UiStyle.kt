package art.capycanvas

import androidx.compose.foundation.Canvas as ComposeCanvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
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
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupPositionProvider

internal class Palette(val dark: Boolean, private val source: org.json.JSONObject) {
    private fun role(name: String) = Color(android.graphics.Color.parseColor(source.getString(name)))
    val surround = role("bg")
    val headerSurface = surround.copy(alpha = .75f)
    val panel = role("panel")
    val tabs = role("tabbar")
    val sidebar = role("sidebar")
    val input = role("input")
    val text = role("text")
    val secondary = text.copy(alpha = .55f)
    val accent = role("accent")
    val accentForeground = role("accent_foreground")
    val sliderFill = lerp(panel, text, .5f)
    val active = role("selection")
    val headerActive = role("header_selection")
    val button = role("button").copy(alpha = 13 / 255f)
    val thumb = role("thumb")
    val checkerLight = role("checker_light")
    val checkerDark = role("checker_dark")
    val divider = text.copy(alpha = .12f)
    val settingsBackground = role("settings")
    val settingsCard = role("card")
    val settingsSecondary = role("settings_secondary")
}
internal val LocalPalette = staticCompositionLocalOf<Palette> { error("Missing core palette") }
internal val LocalCanvasHost = staticCompositionLocalOf<CanvasHost> { error("Missing native host") }
internal val HeaderTextPadding = 6.dp

/** Tab colors never depend on hover/press. A focus-only indication also avoids
 * Android's native ripple layer changing the rasterization of tab joins. */
@Composable internal fun PanelHeaderFeedback(content: @Composable () -> Unit) {
    CompositionLocalProvider(LocalRippleConfiguration provides null,
        LocalIndication provides rememberChromeFocusIndication(), content = content)
}

@Composable internal fun rememberChromeFocusIndication(): IndicationNodeFactory {
    val accent = LocalPalette.current.accent
    return remember(accent) { ChromeFocusIndication(accent) }
}

private data class ChromeFocusIndication(val color: Color) : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): Modifier.Node = object : Modifier.Node(), DrawModifierNode {
        var focused by mutableStateOf(false)
        override fun onAttach() {
            coroutineScope.launch {
                interactionSource.interactions.collect { interaction ->
                    when (interaction) {
                        is FocusInteraction.Focus -> focused = true
                        is FocusInteraction.Unfocus -> focused = false
                    }
                }
            }
        }
        override fun ContentDrawScope.draw() {
            drawContent()
            if (focused) drawOutline(ControlShape.createOutline(size, layoutDirection, this), color, style = Stroke(2.dp.toPx()))
        }
    }
}

/** Editing/composition is native widget state. Rust remains authoritative for
 * accepted values, but an asynchronous acknowledgement must not reset an IME's
 * current edit before the next key arrives. */
@Composable internal fun CoreTextField(value: String, onChange: (String) -> Unit,
    modifier: Modifier = Modifier, label: (@Composable () -> Unit)? = null,
    placeholder: (@Composable () -> Unit)? = null, leadingIcon: (@Composable () -> Unit)? = null,
    trailingIcon: (@Composable () -> Unit)? = null,
    height: Dp = 36.dp, enabled: Boolean = true, focusRequest: Long = 0,
    textStyle: TextStyle = LocalTextStyle.current, shape: Shape = RoundedCornerShape(6.dp),
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
    Column(modifier.background(colors.input, shape)) {
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
    enabled: Boolean = true, onChange: (Boolean) -> Unit) {
    val colors = LocalPalette.current
    Box(modifier.size(22.dp, 28.dp).toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = onChange)
        .semantics { contentDescription = label }, contentAlignment = Alignment.Center) {
        Box(Modifier.size(16.dp).clip(SquircleShape(4.dp))
            .then(if (checked) Modifier.background(colors.accent) else Modifier.border(2.dp, colors.text.copy(alpha = .35f), SquircleShape(4.dp)))) {
            if (checked) SharedIcon("check", null, tint = colors.accentForeground)
        }
    }
}
/** GTK's south/north anchor: center below the tile, flip up and slide at edges. */
private class TileTooltipPositionProvider(private val gap: Int) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize,
        layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val x = (anchorBounds.left + (anchorBounds.width - popupContentSize.width) / 2)
            .coerceIn(0, (windowSize.width - popupContentSize.width).coerceAtLeast(0))
        val below = anchorBounds.bottom + gap
        val y = if (below + popupContentSize.height <= windowSize.height) below
            else anchorBounds.top - gap - popupContentSize.height
        return IntOffset(x, y.coerceIn(0, (windowSize.height - popupContentSize.height).coerceAtLeast(0)))
    }
}

/** Hover only: touch holds remain available for editing and context menus. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun HoverTip(label: String, modifier: Modifier = Modifier,
    resolve: (((String) -> Unit) -> Unit)? = null, enabled: Boolean = true, content: @Composable () -> Unit) {
    val interaction = remember { MutableInteractionSource() }
    val hovered by interaction.collectIsHoveredAsState()
    val state = rememberTooltipState()
    val gap = with(LocalDensity.current) { 4.dp.roundToPx() }
    val position = remember(gap) { TileTooltipPositionProvider(gap) }
    var text by remember(label) { mutableStateOf(label) }
    val currentResolve by rememberUpdatedState(resolve)
    LaunchedEffect(hovered && enabled, label) {
        if (hovered && enabled) {
            currentResolve?.invoke { text = it }
            delay(500)
            state.show()
        } else state.dismiss()
    }
    Box(modifier, propagateMinConstraints = true) {
        Box(Modifier.hoverable(interaction)) {
            // Keep interactive content and pointer capture stable. Only the
            // hovered item needs popup infrastructure. Its outer Box owns the
            // match-parent constraint; TooltipBox applies modifiers internally.
            content()
            if ((hovered && enabled) || state.isVisible) Box(Modifier.matchParentSize()) {
                TooltipBox(modifier = Modifier.fillMaxSize(),
                    positionProvider = position,
                    tooltip = {
                        val shape = RoundedCornerShape(9.dp)
                        Box(Modifier.testTag("hover-tooltip").widthIn(max = 400.dp).background(Color(0xcc000006), shape)
                            .border(1.dp, Color.White.copy(alpha = .1f), shape).padding(horizontal = 11.dp, vertical = 7.dp)) {
                            Text(text, color = Color.White, style = MaterialTheme.typography.bodyMedium)
                        }
                    }, state = state,
                    focusable = false, enableUserInput = false,
                    content = { Box(Modifier.fillMaxSize()) })
            }
        }
    }
}

@Composable internal fun ActionTip(host: CanvasHost, label: String, action: JSONObject,
    modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    HoverTip(label, modifier, resolve = { reply ->
        host.query(obj("type" to "action_tooltip", "label" to label, "action" to action)) { reply(it as? String ?: label) }
    }, content = content)
}

@Composable internal fun IconTile(name: String, label: String, selected: Boolean = false,
    enabled: Boolean = true, modifier: Modifier = Modifier, onLongClick: (() -> Unit)? = null, fill: Color? = null, iconSize: Dp = 16.dp, selectedColor: Color? = null, onClick: () -> Unit) {
    val colors = LocalPalette.current
    HoverTip(label, modifier) {
    Box(Modifier.size(36.dp).alpha(if (enabled) 1f else 0.4f).background(if (selected) selectedColor ?: colors.active else Color.Transparent, TileShape)
        .combinedClickable(enabled = enabled, role = Role.Button, onClickLabel = label, onLongClick = onLongClick, onClick = onClick), contentAlignment = Alignment.Center) {
        SharedIcon(name, label, modifier = Modifier.size(iconSize), fill = fill)
    }
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
                        .background(if (active) colors.active else colors.text.copy(alpha = .05f))
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

/** Compact GTK panel choice: input surface, ellipsized value and trailing arrow. */
@Composable internal fun PanelChoiceButton(label:String,modifier:Modifier=Modifier,enabled:Boolean=true,onClick:()->Unit) {
    val colors=LocalPalette.current
    Row(modifier.fillMaxWidth().heightIn(min=24.dp).clip(ControlShape).background(colors.input)
        .clickable(enabled=enabled,role=Role.Button,onClick=onClick).padding(horizontal=6.dp),
        verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
        Text(label,Modifier.weight(1f),color=colors.text.copy(alpha=if(enabled)1f else .5f),maxLines=1,overflow=androidx.compose.ui.text.style.TextOverflow.Ellipsis)
        SharedIcon("chevron-down",null,Modifier.size(12.dp),tint=colors.text.copy(alpha=if(enabled)1f else .5f))
    }
}
