package art.capycanvas

import android.view.KeyEvent as AndroidKeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

@Composable internal fun PickerDialog(host: CanvasHost, windowed: Boolean, tag: String, keys: (AndroidKeyEvent) -> Boolean, dismiss: () -> Unit,
    page: @Composable BoxScope.() -> Unit, sheet: @Composable BoxScope.() -> Unit = {}, footer: @Composable RowScope.() -> Unit) {
    val colors = LocalPalette.current
    val currentKeys by rememberUpdatedState(keys)
    val keyHandler = remember(host) { { event: AndroidKeyEvent -> currentKeys(event) } }
    val focus = remember { FocusRequester() }
    DisposableEffect(host, keyHandler) {
        host.dialogKeyHandler = keyHandler
        onDispose { if (host.dialogKeyHandler === keyHandler) host.dialogKeyHandler = null }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    BackHandler(!windowed, dismiss)
    val card = @Composable {
        Surface(Modifier.padding(16.dp).widthIn(max = 720.dp).fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp - 32).dp)
            .focusRequester(focus).onPreviewKeyEvent { keyHandler(it.nativeKeyEvent) }.focusable()
            .testTag(tag), shape = SquircleShape(20.dp), color = colors.settingsBackground) {
            Box {
                Column {
                    Box(Modifier.weight(1f, fill = false).clipToBounds(), content = page)
                    PickerFooter(tag, footer)
                }
                sheet()
            }
        }
    }
    if (windowed) Dialog(dismiss, DialogProperties(usePlatformDefaultWidth = false)) { Box(contentAlignment = Alignment.Center) { card() } }
    else Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Box(Modifier.matchParentSize().background(Color.Black.copy(alpha = .32f)).pointerInput(Unit) {
            awaitEachGesture { awaitFirstDown(requireUnconsumed = false).consume(); do { val event = awaitPointerEvent(); event.changes.forEach { it.consume() } } while (event.changes.any { it.pressed }) }
        }.testTag("$tag-scrim"))
        card()
    }
}

@Composable internal fun PickerFooter(tag: String, content: @Composable RowScope.() -> Unit) {
    HorizontalDivider(Modifier.testTag("$tag-divider"), color = LocalPalette.current.dialogSeparator)
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 20.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp), content = content)
}

@Composable internal fun DialogTitle(text: String, tag: String) =
    Text(text, Modifier.fillMaxWidth().testTag(tag), fontWeight = FontWeight.Bold, fontSize = 16.sp, textAlign = TextAlign.Center)

@Composable internal fun DialogButton(label: String, tag: String, accent: Boolean = false, enabled: Boolean = true, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Button(onClick, Modifier.height(34.dp).testTag(tag), enabled = enabled, shape = ControlShape, contentPadding = PaddingValues(horizontal = 16.dp),
        colors = if (accent) ButtonDefaults.buttonColors() else ButtonDefaults.buttonColors(containerColor = colors.button, contentColor = colors.text)) {
        Text(label, fontWeight = FontWeight.Medium)
    }
}
