package art.capycanvas

import android.graphics.Bitmap
import android.view.KeyEvent as AndroidKeyEvent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.isSecondaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import kotlin.math.roundToInt
import kotlin.math.sqrt

internal class TextureDialogRequest(val layer: Long, val windowed: Boolean)

/** Gray paper previews, and the grain they lay over a sheet's color. */
internal class PaperPreviews(private val host: CanvasHost) {
    private val images = mutableStateMapOf<String, ImageBitmap>()
    private val grains = mutableStateMapOf<String, ImageBitmap>()
    private val loading = mutableSetOf<String>()
    operator fun get(id: String) = images[id]
    fun grain(id: String) = grains[id]
    suspend fun load(id: String) {
        if (id in images || !loading.add(id)) return
        try {
            val gray = runCatching { host.withNative { Native.paperPreview(it, id) } }.getOrNull() ?: return
            val (image, grain) = withContext(Dispatchers.Default) {
                val side = sqrt(gray.size.toDouble()).toInt()
                val bitmap = { pixel: (Int) -> Int -> Bitmap.createBitmap(IntArray(gray.size) { pixel(gray[it].toInt() and 255) }, side, side, Bitmap.Config.ARGB_8888).asImageBitmap() }
                bitmap { it * 0x010101 or (0xFF shl 24) } to bitmap {
                    val shade = it / 255f - .5f
                    val alpha = ((if (shade < 0f) -shade * .7f else shade * .25f) * 255).roundToInt()
                    (alpha shl 24) or (if (shade < 0f) 0 else 0xFFFFFF)
                }
            }
            images[id] = image
            grains[id] = grain
        } finally { loading.remove(id) }
    }
}

internal fun JSONObject.paperId(): String? = if (isNull("id")) null else getString("id")

@Composable internal fun PaperPreview(host: CanvasHost, id: String?, size: Dp, background: Color = Color.Transparent, empty: String = "paper") {
    if (id != null) LaunchedEffect(host, id) { host.paperPreviews.load(id) }
    val image = id?.let { host.paperPreviews[it] }
    Box(Modifier.size(size).clip(TileShape).background(background), contentAlignment = Alignment.Center) {
        if (image != null) Image(image, null, Modifier.fillMaxSize())
        else SharedIcon(empty, null, Modifier.size(size / 2))
    }
}

private class TexturePicker(val tiles: List<JSONObject>, val import: JSONObject?, val selected: Int, val key: String)
private data class Pick(val id: String?)

private fun texturePicker(state: JSONObject?, layer: Long): TexturePicker? {
    val properties = state?.objectOrNull("layer_properties")?.takeIf { !it.isNull("layer") && it.getLong("layer") == layer } ?: return null
    return properties.array("controls").objects().firstNotNullOfOrNull { control ->
        control.objectOrNull("paper")?.let { TexturePicker(it.array("tiles").objects(), it.objectOrNull("import"), control.getJSONObject("value").getInt("value"), control.getString("key")) }
    }
}

private fun documentEpoch(state: JSONObject?) = state?.objectOrNull("document_file")?.optLong("epoch")

@Composable internal fun TextureProperty(host: CanvasHost, layer: Long, label: String, labelWidth: Dp, control: JSONObject, enabled: Boolean) {
    val colors = LocalPalette.current
    val tile = control.getJSONObject("paper").array("tiles").objects().getOrNull(control.getJSONObject("value").getInt("value"))
    val name = tile?.getString("label").orEmpty()
    val windowed = inSeparateWindow()
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(label, Modifier.width(labelWidth), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Row(Modifier.weight(1f).clip(ControlShape).background(colors.input)
            .clickable(enabled, role = Role.Button) { host.textureDialog = TextureDialogRequest(layer, windowed) }
            .semantics { contentDescription = label; stateDescription = name }.testTag("property-texture")
            .padding(start = 2.dp, top = 2.dp, end = 6.dp, bottom = 2.dp),
            verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(panelMetrics().group)) {
            PaperPreview(host, tile?.paperId(), 28.dp)
            Text(name, Modifier.weight(1f), color = colors.text.copy(alpha = if (enabled) 1f else .5f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable internal fun TextureDialogLayer(host: CanvasHost) {
    val request = host.textureDialog ?: return
    key(request) { TextureDialog(host, request) }
}

@Composable private fun TextureDialog(host: CanvasHost, request: TextureDialogRequest) {
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("paper")
    val common = host.bootstrap!!.getJSONObject("common")
    val state = host.snapshot?.objectOrNull("state")
    val epoch = remember { documentEpoch(state) }
    val shown = remember { arrayOfNulls<TexturePicker>(1) }
    texturePicker(state, request.layer)?.takeIf { documentEpoch(state) == epoch }?.let { shown[0] = it }
    val picker = shown[0]
    if (picker == null) {
        LaunchedEffect(Unit) {
            runCatching { host.withNative {} }
            if (texturePicker(host.snapshot?.objectOrNull("state"), request.layer) == null) host.textureDialog = null
        }
        return
    }
    var pending by remember { mutableStateOf<Pick?>(null) }
    var renaming by remember { mutableStateOf<JSONObject?>(null) }
    val act = { action: JSONObject -> if (action.getString("op") == "rename_paper") renaming = action else host.dispatch(obj("type" to "effect", "action" to action)) }
    val choice = pending?.takeIf { pick -> picker.tiles.any { it.paperId() == pick.id } } ?: Pick(picker.tiles.getOrNull(picker.selected)?.paperId())
    SideEffect { if (pending != choice) pending = choice }
    fun use() {
        val now = host.snapshot?.objectOrNull("state")
        val target = texturePicker(now, request.layer)?.takeIf { documentEpoch(now) == epoch }
        host.textureDialog = null
        if (target == null) return
        val index = target.tiles.indexOfFirst { it.paperId() == choice.id }
        if (index >= 0 && index != target.selected) host.dispatch(obj("type" to "effect", "action" to obj("op" to "set", "layer" to request.layer,
            "key" to target.key, "value" to obj("kind" to "choice", "value" to index))))
    }
    val keys = { event: AndroidKeyEvent ->
        val pressed = event.action == AndroidKeyEvent.ACTION_DOWN && event.repeatCount == 0
        when (event.keyCode) {
            AndroidKeyEvent.KEYCODE_ESCAPE -> { if (pressed) host.textureDialog = null; true }
            AndroidKeyEvent.KEYCODE_ENTER, AndroidKeyEvent.KEYCODE_NUMPAD_ENTER -> { if (pressed) use(); true }
            else -> false
        }
    }
    val focus = remember { FocusRequester() }
    val scroll = rememberScrollState()
    val placed = remember { arrayOfNulls<LayoutCoordinates>(2) }
    val margin = with(LocalDensity.current) { TexturePageEnd.toPx() }
    PickerDialog(host, request.windowed, "texture-dialog", keys, { host.textureDialog = null }, page = {
        Column(Modifier.padding(start = 20.dp, top = 20.dp, end = 20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            DialogTitle(copy.getString("choose"), "texture-dialog-title")
            BoxWithConstraints(Modifier.weight(1f, fill = false).height(maxOf(colorEditorBodyHeight(), threeTextureRows()) + TexturePageEnd)) {
                val columns = ((maxWidth + TextureGap) / (TextureFrame + TextureGap)).toInt().coerceIn(1, 6)
                Column(Modifier.fillMaxSize().onPlaced { placed[0] = it }.panelScroll(scroll).padding(bottom = TexturePageEnd).testTag("texture-grid"), verticalArrangement = Arrangement.spacedBy(TextureRowGap)) {
                    picker.tiles.withIndex().chunked(columns).forEach { row ->
                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(TextureGap)) {
                            row.forEach { (index, tile) ->
                                val chosen = tile.paperId() == choice.id
                                TextureTile(host, tile, index, chosen, Modifier.weight(1f).then(if (chosen) Modifier.onPlaced { placed[1] = it }.focusRequester(focus) else Modifier), act) { pending = Pick(tile.paperId()) }
                            }
                            repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                        }
                    }
                }
            }
        }
        LaunchedEffect(Unit) {
            withFrameNanos {}
            val (area, tile) = placed
            if (area != null && tile != null && area.isAttached && tile.isAttached) {
                val bounds = area.localBoundingBoxOf(tile, clipBounds = false)
                scroll.scrollTo(scroll.value + (if (bounds.top < 0) bounds.top else (bounds.bottom + margin - area.size.height).coerceAtLeast(0f)).roundToInt())
            }
            runCatching { focus.requestFocus() }
        }
    }) {
        picker.import?.let { import ->
            DialogButton(import.getString("label"), "texture-import") {
                host.textureDialog = null
                host.dispatch(obj("type" to "effect", "action" to import.getJSONObject("action")))
            }
        }
        Spacer(Modifier.weight(1f))
        DialogButton(common.getString("cancel"), "texture-cancel") { host.textureDialog = null }
        DialogButton(copy.getString("use_texture"), "texture-use", accent = true) { use() }
    }
    renaming?.let { action -> PaperRenameDialog(host, action) { renaming = null } }
}

@Composable private fun PaperRenameDialog(host: CanvasHost, action: JSONObject, close: () -> Unit) {
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("paper")
    var name by remember { mutableStateOf(action.getString("name")) }
    val error = remember(name, host.languageTag) { JSONObject(Native.nativeCaption(obj("type" to "paper_name_error", "name" to name).toString(), host.languageTag)).getString("text") }
    AlertDialog(onDismissRequest = close, modifier = Modifier.testTag("paper-rename-dialog"),
        title = { Text(copy.getString("rename_title")) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            CoreTextField(name, { name = it }, Modifier.fillMaxWidth().testTag("paper-rename-name"), maxLength = 64, focusRequest = 1, background = LocalPalette.current.dialogField)
            if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error)
        } },
        dismissButton = { TextButton(close) { Text(host.bootstrap!!.getJSONObject("common").getString("cancel")) } },
        confirmButton = { TextButton({ close(); host.dispatch(obj("type" to "effect", "action" to JSONObject(action.toString()).put("name", name))) },
            enabled = error.isEmpty(), modifier = Modifier.testTag("paper-rename-save")) { Text(copy.getString("rename")) } })
}

internal val TextureFrame = 102.dp
private val TexturePageEnd = 20.dp
private val TextureRowGap = 14.dp
internal val TextureCaptionGap = 6.dp

@Composable private fun threeTextureRows(): Dp {
    val style = LocalTextStyle.current
    val caption = rememberTextMeasurer().measure("", style.copy(fontSize = style.fontSize * .83333f, lineHeight = style.lineHeight * .83333f)).size.height
    return (TextureFrame + TextureCaptionGap + with(LocalDensity.current) { caption.toDp() }) * 3 + TextureRowGap * 2
}
internal val TextureGap = 12.dp

/** A click chooses; a second click within the double-tap time uses the choice. */
@Composable internal fun rememberActivation(choose: () -> Unit, use: (() -> Unit)?): () -> Unit {
    val timeout = LocalViewConfiguration.current.doubleTapTimeoutMillis
    val last = remember { longArrayOf(0L) }
    val chosen by rememberUpdatedState(choose)
    val used by rememberUpdatedState(use)
    return remember(timeout) { {
        val now = android.os.SystemClock.uptimeMillis()
        val second = used?.takeIf { last[0] != 0L && now - last[0] <= timeout }
        last[0] = if (second == null) now else 0L
        second?.invoke() ?: chosen()
    } }
}

@Composable internal fun TextureFace(host: CanvasHost, id: String?, label: String, empty: String, hovered: Boolean, chosen: Boolean) {
    val colors = LocalPalette.current
    Box(Modifier.size(TextureFrame).clip(TileShape).background(if (hovered) colors.text.copy(alpha = .10f) else Color.Transparent)
        .then(if (chosen) Modifier.border(2.dp, colors.accent, TileShape) else Modifier).padding(3.dp)) {
        PaperPreview(host, id, 96.dp, colors.text.copy(alpha = .06f), empty)
    }
    Text(label, Modifier.fillMaxWidth(), fontSize = LocalTextStyle.current.fontSize * .83333f, lineHeight = LocalTextStyle.current.lineHeight * .83333f,
        textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
}

@Composable internal fun TextureTile(host: CanvasHost, tile: JSONObject, index: Int, chosen: Boolean, modifier: Modifier, act: (JSONObject) -> Unit,
    empty: String = "paper", tag: String = "texture-tile", use: (() -> Unit)? = null, choose: () -> Unit) {
    val label = tile.getString("label")
    val actions = tile.array("actions").objects()
    val interactions = remember { MutableInteractionSource() }
    val hovered by interactions.collectIsHoveredAsState()
    var menu by remember { mutableStateOf(false) }
    Box(modifier) {
        HoverTip(label, Modifier.fillMaxWidth()) {
            Column(Modifier.fillMaxWidth().semantics(mergeDescendants = true) { selected = chosen }.testTag("$tag-$index")
                .onFocusChanged { if (it.isFocused) choose() }
                .pointerInput(actions.isNotEmpty()) {
                    if (actions.isEmpty()) return@pointerInput
                    awaitEachGesture {
                        val down = awaitFirstDown(requireUnconsumed = false)
                        if (currentEvent.buttons.isSecondaryPressed) { down.consume(); menu = true }
                    }
                }
                .onKeyEvent { event -> if (actions.isNotEmpty() && event.type == KeyEventType.KeyDown && event.key == Key.Menu) { menu = true; true } else false }
                .hoverable(interactions)
                .combinedClickable(interactions, null, role = Role.RadioButton, onLongClick = if (actions.isEmpty()) null else ({ menu = true }), onClick = rememberActivation(choose, use)),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(TextureCaptionGap)) {
                TextureFace(host, tile.paperId(), label, empty, hovered, chosen)
            }
        }
        DropdownMenu(menu, { menu = false }) {
            actions.forEach { action ->
                DropdownMenuItem(text = { Text(action.getString("label")) }, modifier = Modifier.testTag("texture-action-${action.getJSONObject("action").getString("op")}"),
                    onClick = { menu = false; act(action.getJSONObject("action")) })
            }
        }
    }
}
