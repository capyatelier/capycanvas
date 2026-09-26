package art.capycanvas

import android.content.res.Configuration
import android.view.KeyEvent
import android.view.View
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.*
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import org.json.JSONObject

/** Native focus, IME and modal capture; Rust owns results, applicability and execution. */
@Composable internal fun CommandSearch(host: CanvasHost) {
    val view = host.commandSearch ?: return
    val colors = LocalPalette.current.onGlass
    val style = host.catalog?.objectOrNull("command_search_style")
    fun metric(name: String, default: Int) = (style?.optInt(name, default) ?: default).dp
    val inset = metric("inset", 12)
    val gap = metric("gap", 8)
    val rowHeight = metric("row_height", 44).coerceAtLeast(48.dp)
    val shape = SquircleShape(metric("radius", 12))
    val parameter = view.objectOrNull("parameter")
    val parameterId = parameter?.getString("id")
    val results = view.array("results").objects()
    val selected = view.optInt("selected")
    val configuration = LocalConfiguration.current
    val keyboard = LocalSoftwareKeyboardController.current
    val editor = LocalView.current
    val focus = remember { FocusRequester() }
    val list = rememberLazyListState()
    var text by remember { mutableStateOf(TextFieldValue(view.optString("query"))) }
    var visible by remember { mutableStateOf(false) }
    var entryFocused by remember { mutableStateOf(false) }
    val progress by animateFloatAsState(if (visible) 1f else 0f, tween(120), label = "command-search")
    fun send(action: JSONObject) = host.dispatch(obj("type" to "command_search", "action" to action))
    fun close() = send(obj("type" to "close"))
    fun back() = send(obj("type" to "back"))
    fun commit() = send(obj("type" to "commit", "text" to text.text))
    LaunchedEffect(parameterId) {
        val value = parameter?.getJSONObject("parameter")?.getString("text") ?: view.optString("query")
        text = TextFieldValue(value, if (parameter != null) TextRange(0, value.length) else TextRange(value.length))
    }
    LaunchedEffect(Unit) {
        visible = true
        focus.requestFocus()
        if (configuration.keyboard == Configuration.KEYBOARD_NOKEYS) keyboard?.show()
    }
    LaunchedEffect(selected, parameterId) { if (parameter == null && selected in results.indices) list.scrollToItem(selected) }
    Dialog(onDismissRequest = { back() }, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val root = LocalView.current
        val window = (root.parent as DialogWindowProvider).window
        DisposableEffect(window) {
            window.setDimAmount(0f)
            window.enterCanvasFullscreen()
            onDispose { }
        }
        // A stable native window avoids a WindowManager resize for every result
        // count. Compose sizes only the visible card inside the IME-safe area.
        BoxWithConstraints(Modifier.fillMaxSize().imePadding()) {
            Box(Modifier.matchParentSize().pointerInput(Unit) { detectTapGestures { close() } })
            val top = if (configuration.screenWidthDp < 600) 16.dp else (maxHeight / 5).coerceIn(metric("top_min", 48), metric("top_max", 192))
            val availableHeight = (maxHeight - top - 16.dp).coerceAtLeast(144.dp)
            Surface(color = colors.panelFill, shape = shape,
                modifier = Modifier.align(Alignment.TopCenter).padding(start = 16.dp, end = 16.dp, top = top).widthIn(max = metric("width", 560))
                    .fillMaxWidth().testTag("command-bar").graphicsLayer { alpha = progress; translationY = (1f - progress) * -4.dp.toPx() }
                    .panelShadow(12.dp, shape).glass(shape) { root.screenOffset(editor) }
                    .onPreviewKeyEvent { event ->
                        val native = event.nativeKeyEvent
                        if (native.action == KeyEvent.ACTION_UP) {
                            host.key(native)
                            // Dialog also treats Escape-up as dismissal. Consume
                            // our navigation release so Back happens exactly once.
                            native.keyCode == KeyEvent.KEYCODE_ESCAPE ||
                                (entryFocused && native.keyCode in listOf(KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER)) ||
                                (parameter == null && native.keyCode in listOf(KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN))
                        }
                        else if (native.keyCode == KeyEvent.KEYCODE_ESCAPE) { back(); true }
                        else if (text.composition != null) false
                        else when (native.keyCode) {
                            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> if (entryFocused) { commit(); true } else false
                            KeyEvent.KEYCODE_DPAD_UP, KeyEvent.KEYCODE_DPAD_DOWN -> if (parameter == null) {
                                send(obj("type" to "move", "delta" to if (native.keyCode == KeyEvent.KEYCODE_DPAD_DOWN) 1 else -1)); true
                            } else false
                            else -> false
                        }
                    }) {
                Column(Modifier.heightIn(max = availableHeight).padding(inset), verticalArrangement = Arrangement.spacedBy(gap)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(gap)) {
                        Row(Modifier.weight(1f).heightIn(min = rowHeight).background(colors.input, RoundedCornerShape(8.dp)).padding(horizontal = inset),
                            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(gap)) {
                            SharedIcon("search", null)
                            BasicTextField(text, onValueChange = {
                                text = it
                                if (parameter == null) send(obj("type" to "query", "text" to it.text))
                            }, singleLine = true, textStyle = LocalTextStyle.current.copy(color = colors.text), cursorBrush = SolidColor(colors.accent),
                                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search, autoCorrectEnabled = false), keyboardActions = KeyboardActions(onSearch = { commit() }),
                                modifier = Modifier.weight(1f).focusRequester(focus).onFocusChanged { entryFocused = it.isFocused }.testTag("command-search").semantics { contentDescription = parameter?.getString("label") ?: "Search commands" },
                                decorationBox = { inner -> Box { if (text.text.isEmpty()) Text(if (parameter == null) "Search commands" else "Enter a value", color = colors.secondary); inner() } })
                            parameter?.getJSONObject("parameter")?.getJSONObject("numeric")?.optString("unit")?.takeIf { it.isNotEmpty() }?.let { Text(it, color = colors.secondary) }
                        }
                        IconButton({ close() }, Modifier.size(rowHeight).testTag("command-close").semantics { contentDescription = "Close command search" }) { SharedIcon("close", null) }
                    }
                    if (parameter == null) {
                        if (results.isEmpty()) Text("No matching commands", Modifier.padding(inset), color = colors.secondary)
                        else LazyColumn(Modifier.weight(1f, fill = false).fillMaxWidth(), state = list) {
                            itemsIndexed(results, key = { _, command -> command.getString("id") }) { index, command ->
                                val enabled = command.optBoolean("enabled")
                                Row(Modifier.fillMaxWidth().heightIn(min = rowHeight).background(if (index == selected) colors.active else androidx.compose.ui.graphics.Color.Transparent, RoundedCornerShape(6.dp))
                                    .testTag("command-result-$index").semantics { this.selected = index == selected; if (!enabled) stateDescription = "Unavailable" }
                                    .clickable { send(obj("type" to "execute", "id" to command.getString("id"))) }.padding(horizontal = inset),
                                    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(gap)) {
                                    Text(command.getString("label"), Modifier.weight(1f), color = if (enabled) colors.text else colors.secondary, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    if (command.optBoolean("selected")) SharedIcon("check", "On")
                                    command.optString("shortcut").takeUnless { it.isEmpty() || it == "null" }?.let { Text(it, color = colors.secondary, maxLines = 1) }
                                }
                            }
                        }
                    }
                    Text(view.optString("detail"), Modifier.padding(horizontal = inset).heightIn(min = 20.dp).testTag("command-detail").semantics { liveRegion = LiveRegionMode.Polite },
                        color = colors.secondary, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

private fun View.screenOffset(from: View): Offset {
    val position = IntArray(2).also(::getLocationOnScreen)
    val origin = IntArray(2).also(from::getLocationOnScreen)
    return Offset((position[0] - origin[0]).toFloat(), (position[1] - origin[1]).toFloat())
}
