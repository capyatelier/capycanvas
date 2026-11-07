package art.capycanvas

import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.animation.core.updateTransition
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.tween
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.roundToInt

private val LocalDialogFocus=compositionLocalOf<FocusRequester?> {null}

@Composable internal fun <T> rememberDialogContent(current:T?):T? {
    var retained by remember {mutableStateOf<T?>(null)}
    if(current != null)SideEffect {retained=current}
    return current ?: retained
}

internal data class SearchState(val focused: Boolean, val text: String, val clear: () -> Unit, val focus: FocusRequester)
internal data class DialogResponse(val label: String, val tag: String, val accent: Boolean = false,
    val enabled: Boolean = true, val action: () -> Unit)

@Composable internal fun DialogShell(host: CanvasHost, intent: JSONObject, title: String, subpage: Boolean = false,
    search: SearchState? = null, onCancel: () -> Unit, onBack: () -> Unit = onCancel, onPrimary: () -> Unit = {},
    primaryEnabled: Boolean = true, primaryLabel: String = "", tag: String = "dialog-shell", naturalWidth: Dp = 720.dp,
    responses: List<DialogResponse> = emptyList(), shown: Boolean = true, scrollBody: Boolean = true, header: Boolean = true, windowed: Boolean = true, primaryTag: String? = null, cancelTag: String? = null,
    secondary: (@Composable () -> Unit)? = null, navigation: (@Composable () -> Unit)? = null, keys: ((KeyEvent) -> Boolean)? = null, overlay: (@Composable BoxScope.() -> Unit)? = null, body: @Composable (PaddingValues) -> Unit, actions: (@Composable RowScope.() -> Unit)? = null) {
    val colors = LocalPalette.current
    val density = LocalDensity.current
    val phone = host.snapshot?.getJSONObject("layout")?.objectOrNull("phone")
    val visibility = remember { MutableTransitionState(false) }
    visibility.targetState = shown
    val transition = updateTransition(visibility,label = "dialog-page")
    val slide by transition.animateFloat(transitionSpec = { tween(if(targetState)240 else 200,easing = FastOutSlowInEasing) },label = "dialog-slide") { if(it)0f else -1f }
    val currentCancel by rememberUpdatedState(onCancel)
    val currentBack by rememberUpdatedState(onBack)
    val currentSearch by rememberUpdatedState(search)
    val currentSubpage by rememberUpdatedState(subpage)
    val focus = remember { FocusRequester() }
    val active=visibility.currentState || visibility.targetState
    val ancestor=LocalDialogFocus.current
    val parentFocus=remember(active) {ancestor ?: host.dialogFocus.lastOrNull {it !== focus} ?: host.workspaceFocus}
    val opener=remember(active) {host.workspaceOpener}
    val focusManager = LocalFocusManager.current
    val scroll = rememberScrollState()
    LaunchedEffect(shown) { if(shown)scroll.scrollTo(0) }
    val currentBody by rememberUpdatedState(body)
    val content = remember { movableContentOf<PaddingValues> { currentBody(it) } }
    val headerView = remember(intent.toString(),subpage) { JSONObject(Native.dialogUi(obj("kind" to "header","intent" to intent,"subpage" to subpage).toString())) }
    fun escape() {
        val search = currentSearch
        when (org.json.JSONTokener(Native.dialogUi(obj("kind" to "escape","subpage" to currentSubpage,"search_focused" to (search?.focused == true),
            "search_has_text" to !search?.text.isNullOrEmpty()).toString())).nextValue()) {
            "clear_search" -> search?.clear?.invoke()
            "back" -> currentBack()
            else -> currentCancel()
        }
    }
    val keyHandler: (KeyEvent) -> Boolean = { event ->
        keys?.invoke(event) ?: if (event.keyCode == KeyEvent.KEYCODE_ESCAPE) { if (event.action == KeyEvent.ACTION_DOWN) escape();true } else false
    }
    val currentKeys by rememberUpdatedState(keyHandler)
    DisposableEffect(host,phone != null,shown,intent.getBoolean("modal")) {
        if (phone != null && shown)host.dialogFocus.add(focus)
        val dock = host.workspaceInteraction?.takeIf { phone != null && shown && intent.getBoolean("modal") }
        dock?.popup(true)
        if(dock != null)host.retirePhoneContacts()
        onDispose {
            dock?.popup(false)
            host.dialogFocus.remove(focus)
        }
    }
    DisposableEffect(host,phone != null,active) {
        if(phone != null && active)parentFocus.saveFocusedChild()
        onDispose {
            if(phone != null && active && host.snapshot?.getJSONObject("layout")?.objectOrNull("phone") != null) {
                if(host.dialogFocus.isEmpty()) {
                    if(opener?.requestFocus() != true)host.workspaceFocus.restoreFocusedChild()
                } else if(host.dialogFocus.lastOrNull() === parentFocus)parentFocus.restoreFocusedChild()
            }
        }
    }
    DisposableEffect(host,shown) {
        val keys: (KeyEvent) -> Boolean = { event -> currentKeys(event) }
        if(shown)host.dialogKeys.add(keys)
        onDispose { host.dialogKeys.remove(keys) }
    }
    BackHandler(phone == null && shown) { escape() }
    if (!visibility.currentState && !visibility.targetState) return
    BoxWithConstraints(Modifier.fillMaxSize().zIndex(500f).semantics { paneTitle = title },contentAlignment = AbsoluteAlignment.TopLeft) {
        val visible = obj("x" to 0,"y" to 0,"width" to maxWidth.value,
            "height" to (maxHeight.value-WindowInsets.ime.getBottom(density)/density.density).coerceAtLeast(0f))
        var presentation by remember { mutableStateOf<JSONObject?>(null) }
        LaunchedEffect(intent.getString("role"),visible.toString(),phone?.toString()) {
            if(shown)presentation = host.awaitQuery(obj("type" to "dialog_presentation","role" to intent.getString("role"),"visible" to visible))
        }
        val mode = presentation?.getString("kind") ?: return@BoxWithConstraints
        if (!shown && mode != "page") return@BoxWithConstraints
        val hosting = mode to (phone != null)
        val restoreSearch = remember(hosting) { search?.focused == true }
        LaunchedEffect(hosting,shown) { if(shown) { if (restoreSearch) currentSearch?.focus?.requestFocus() else focus.requestFocus() } }
        val frame = intent.getJSONObject("frame")
        val alert = frame.getString("kind") == "alert"
        val canvas = mode == "canvas_panel"
        val floatingArea=if(phone==null && !alert)obj("x" to 0,"y" to 0,"width" to maxWidth.value,"height" to maxHeight.value) else visible
        val floatingWidth = (org.json.JSONTokener(Native.dialogUi(obj("kind" to "floating_width","window" to maxWidth.value,"natural" to naturalWidth.value).toString())).nextValue() as Number).toFloat()
        val card = @Composable {
            if (canvas) Surface(Modifier.width(IntrinsicSize.Min).widthIn(max = naturalWidth).testTag(tag)
                .focusRequester(focus).onPreviewKeyEvent { currentKeys(it.nativeKeyEvent) }.focusable(),shape = SurfaceShape,color = colors.panel) {
                Column {
                    DialogHeader(host,headerView,title,tag,onCancel,onBack,onPrimary,primaryEnabled,primaryLabel,primaryTag,cancelTag)
                    HorizontalDivider(color = colors.dialogSeparator)
                    content(PaddingValues(16.dp))
                }
            }
            else if (alert) AlertShell(host,title,tag,frame.optBoolean("prefer_wide"),visible,responses,intent.getString("role")=="alert",
                Modifier.focusRequester(focus).onPreviewKeyEvent { currentKeys(it.nativeKeyEvent) }.focusable(),content)
            else Surface(Modifier.padding(16.dp).widthIn(max = floatingWidth.dp).fillMaxWidth().heightIn(max = (floatingArea.number("height")-32f).coerceAtLeast(0f).dp)
                .testTag(tag).focusRequester(focus).onPreviewKeyEvent { currentKeys(it.nativeKeyEvent) }.focusable(),shape = SurfaceShape,color = colors.settingsBackground) {
                Box {
                    Column {
                        navigation?.invoke()
                        if(header)DialogTitle(title,"$tag-title")
                        Box(Modifier.weight(1f,fill = false).then(if(scrollBody)Modifier.verticalScroll(scroll) else Modifier)) { content(PaddingValues(16.dp)) }
                        if(actions != null || secondary != null)PickerFooter(tag) { secondary?.invoke();actions?.invoke(this) }
                    }
                    overlay?.invoke(this)
                }
            }
        }
        CompositionLocalProvider(LocalDialogFocus provides focus) {
        if (mode == "page") {
            if (intent.getBoolean("modal")) DialogCapture(Color.Transparent)
            Surface(Modifier.placedAbsolute(presentation!!.getJSONObject("bounds"),density.density).testTag(tag)
                .graphicsLayer { translationY = slide*size.height }
                .focusRequester(focus).focusProperties { onExit = { if(shown && intent.getBoolean("modal"))cancelFocusChange() } }.focusGroup()
                .onPreviewKeyEvent { event ->
                    val key = event.nativeKeyEvent
                    if(intent.getBoolean("modal") && key.keyCode == KeyEvent.KEYCODE_TAB) {
                        if(key.action == KeyEvent.ACTION_DOWN)focusManager.moveFocus(if(key.isShiftPressed)FocusDirection.Previous else FocusDirection.Next)
                        true
                    } else currentKeys(key)
                }.focusable(),color = colors.settingsBackground) {
                Column {
                    navigation?.invoke()
                    if(header) {
                        DialogHeader(host,headerView,title,tag,onCancel,onBack,onPrimary,primaryEnabled,primaryLabel,primaryTag,cancelTag)
                        HorizontalDivider(color = colors.dialogSeparator)
                    }
                    Column(Modifier.weight(1f).fillMaxWidth().then(if(scrollBody)Modifier.verticalScroll(scroll) else Modifier)) {
                        Box(if(scrollBody)Modifier else Modifier.weight(1f)) { content(PaddingValues(16.dp)) }
                        secondary?.invoke()
                    }
                }
            }
            if(!shown)DialogCapture(Color.Transparent)
        } else if (canvas) {
            var bounds by remember { mutableStateOf<JSONObject?>(null) }
            var measured by remember { mutableStateOf<JSONArray?>(null) }
            LaunchedEffect(visible.toString(),measured?.toString()) {
                measured?.let { bounds = host.awaitQuery(obj("type" to "dialog_bounds","role" to intent.getString("role"),"visible" to visible,"measured" to it)) }
            }
            if (intent.getBoolean("modal")) DialogCapture(Color.Transparent)
            SubcomposeLayout(Modifier.fillMaxSize()) { constraints ->
                val area = presentation!!.getJSONObject("bounds")
                val width = bounds?.number("width")?.let { (it*density.density).roundToInt() }
                val child = subcompose("panel",card).single().measure(Constraints(minWidth = width ?: 0,
                    maxWidth = width ?: (area.number("width")*density.density).roundToInt(),maxHeight = (area.number("height")*density.density).roundToInt()))
                val next = JSONArray(listOf(child.width/density.density,child.height/density.density))
                if (measured?.toString() != next.toString()) measured = next
                layout(constraints.maxWidth,constraints.maxHeight) { bounds?.let { child.place((it.number("x")*density.density).roundToInt(),(it.number("y")*density.density).roundToInt()) } }
            }
        } else if (phone != null || !windowed) {
            if (intent.getBoolean("modal")) DialogCapture(Color.Black.copy(alpha = .32f))
            Box(Modifier.placedAbsolute(floatingArea,density.density),contentAlignment = Alignment.Center) { card() }
        } else Dialog({ onCancel() },DialogProperties(usePlatformDefaultWidth = false)) {
            Box(contentAlignment = Alignment.Center) { card() }
        }
        }
    }
}

@Composable private fun DialogCapture(color: Color) {
    Box(Modifier.fillMaxSize().background(color).pointerInput(Unit) {
        awaitEachGesture { awaitFirstDown(requireUnconsumed = false).consume();do { val event = awaitPointerEvent();event.changes.forEach { it.consume() } } while(event.changes.any { it.pressed }) }
    })
}

@Composable private fun AlertShell(host: CanvasHost,title: String,tag: String,preferWide: Boolean,visible: JSONObject,
    responses: List<DialogResponse>,centre:Boolean,modifier: Modifier,body: @Composable (PaddingValues) -> Unit) {
    val density = LocalDensity.current
    val font = with(density) { LocalTextStyle.current.fontSize.toPx()/density.density }
    val colors = LocalPalette.current
    val scroll = rememberScrollState()
    Surface(modifier.testTag(tag),shape = SurfaceShape,color = colors.settingsBackground) {
    SubcomposeLayout { constraints ->
        val heading = subcompose("heading") { DialogTitle(title,"$tag-title") }.single()
        val content = subcompose("body") { ProvideTextStyle(LocalTextStyle.current.copy(textAlign = if(centre)TextAlign.Center else TextAlign.Start)) { Box(Modifier.verticalScroll(scroll),contentAlignment = Alignment.TopCenter) { body(PaddingValues()) } } }.single()
        val buttons = responses.mapIndexed { index,response -> subcompose("response-$index") {
            DialogButton(response.label,response.tag,response.accent,response.enabled,modifier = Modifier.fillMaxWidth(),multiline = true,onClick = response.action)
        }.single() }
        val widths = buttons.map { it.maxIntrinsicWidth(Int.MAX_VALUE)/density.density }
        val minimum = maxOf(heading.minIntrinsicWidth(Int.MAX_VALUE),content.minIntrinsicWidth(Int.MAX_VALUE))/density.density+48f
        val natural = maxOf(heading.maxIntrinsicWidth(Int.MAX_VALUE),content.maxIntrinsicWidth(Int.MAX_VALUE))/density.density+48f
        val geometry = JSONObject(Native.dialogUi(obj("kind" to "alert_layout","viewport" to JSONArray(listOf(visible.number("width"),visible.number("height"))),
            "metrics" to obj("minimum" to minimum,"natural" to natural,"heading" to heading.maxIntrinsicWidth(Int.MAX_VALUE)/density.density,"responses" to JSONArray(widths)),
            "font_pixels" to font,"prefer_wide" to preferWide).toString()))
        val width = (geometry.number("width")*density.density).roundToInt()
        val inset = ((if (geometry.getBoolean("short")) 18 else 24)*density.density).roundToInt()
        val inner = (width-2*inset).coerceAtLeast(0)
        val gap = (12*density.density).roundToInt()
        val stacked = geometry.getBoolean("stacked")
        val buttonWidth = if (stacked) inner else ((inner-gap*(buttons.size-1))/buttons.size.coerceAtLeast(1)).coerceAtLeast(0)
        val placedButtons = buttons.map { it.measure(Constraints.fixedWidth(buttonWidth)) }
        val responseHeight = if (stacked) placedButtons.sumOf { it.height }+gap*(buttons.size-1).coerceAtLeast(0) else placedButtons.maxOfOrNull { it.height } ?: 0
        val title = heading.measure(Constraints.fixedWidth(inner))
        val text = content.measure(Constraints(maxWidth = inner,maxHeight = (visible.number("height")*density.density-2*inset-title.height-responseHeight-2*gap).roundToInt().coerceAtLeast(0)))
        val height = title.height+text.height+responseHeight+2*inset+2*gap
        layout(width,height) {
            title.place(inset,inset);text.place(inset,inset+title.height+gap)
            var x = inset;var y = inset+title.height+gap+text.height+gap
            placedButtons.forEach { button -> button.placeRelative(x,y);if(stacked)y+=button.height+gap else x+=button.width+gap }
        }
    }
    }
}

@Composable internal fun DialogHeader(host: CanvasHost,header: JSONObject,title: String,tag: String,onCancel: () -> Unit,
    onBack: () -> Unit,onPrimary: () -> Unit,primaryEnabled: Boolean,primaryLabel: String,primaryTag: String? = null,cancelTag: String? = null) {
    Layout(modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(horizontal = 8.dp),content = {
        Box(Modifier.width(56.dp),contentAlignment = Alignment.CenterStart) {
            if (!header.isNull("start")) IconButton({ if (header.getString("start") == "back") onBack() else onCancel() },Modifier.testTag(cancelTag ?: "$tag-cancel")) {
                SharedIcon(if (header.getString("start") == "back") "back" else "close",host.bootstrap!!.getJSONObject("common").getString(if (header.getString("start") == "back") "back" else "cancel"))
            }
        }
        Text(title,Modifier.testTag("$tag-title").semantics {heading()},fontWeight=FontWeight.Bold,textAlign = TextAlign.Center,maxLines = 1,overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
        Box(Modifier.widthIn(min = 56.dp),contentAlignment = Alignment.CenterEnd) {
            when (header.optString("end")) {
                "primary" -> DialogButton(primaryLabel,primaryTag ?: "$tag-confirm",accent = true,enabled = primaryEnabled,onClick = onPrimary)
                "close" -> DialogButton(host.bootstrap!!.getJSONObject("common").getString("done"),"$tag-done",accent=true,enabled=primaryEnabled,onClick=onCancel)
            }
        }
    }) { (start,title,end),constraints ->
        val width = if(constraints.hasBoundedWidth)constraints.maxWidth else
            maxOf(start.maxIntrinsicWidth(constraints.maxHeight),end.maxIntrinsicWidth(constraints.maxHeight))*2+title.maxIntrinsicWidth(constraints.maxHeight)
        val startPlace = start.measure(Constraints(maxWidth=56.dp.roundToPx(),maxHeight=constraints.maxHeight))
        val endPlace = end.measure(Constraints(maxWidth=(width-startPlace.width-48.dp.roundToPx()).coerceAtLeast(0),maxHeight=constraints.maxHeight))
        val remaining=width-startPlace.width-endPlace.width
        val natural=title.maxIntrinsicWidth(constraints.maxHeight)
        val titleWidth=minOf(natural,remaining).coerceAtLeast(0)
        val titlePlace = title.measure(Constraints.fixedWidth(titleWidth))
        val titleX=((width-titleWidth)/2).coerceIn(startPlace.width,(width-endPlace.width-titleWidth).coerceAtLeast(startPlace.width))
        val height = maxOf(constraints.minHeight,startPlace.height,titlePlace.height,endPlace.height)
        layout(width,height) {
            startPlace.placeRelative(0,(height-startPlace.height)/2)
            titlePlace.placeRelative(titleX,(height-titlePlace.height)/2)
            endPlace.placeRelative(width-endPlace.width,(height-endPlace.height)/2)
        }
    }

}

@Composable internal fun PickerFooter(tag: String, content: @Composable RowScope.() -> Unit) {
    HorizontalDivider(Modifier.testTag("$tag-divider"), color = LocalPalette.current.dialogSeparator)
    Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 20.dp), verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp), content = content)
}

@Composable internal fun DialogTitle(text: String, tag: String) =
    Text(text, Modifier.fillMaxWidth().testTag(tag).semantics {heading()}, fontWeight = FontWeight.Bold, fontSize = 16.sp, textAlign = TextAlign.Center)

@Composable internal fun DialogButton(label: String, tag: String, accent: Boolean = false, enabled: Boolean = true, modifier: Modifier = Modifier, multiline: Boolean = false, onClick: () -> Unit) {
    val colors = LocalPalette.current
    Button(onClick, modifier.then(if(multiline)Modifier.heightIn(min = 34.dp) else Modifier.height(34.dp)).testTag(tag), enabled = enabled, shape = ControlShape, contentPadding = PaddingValues(horizontal = 16.dp),
        colors = if (accent) ButtonDefaults.buttonColors() else ButtonDefaults.buttonColors(containerColor = colors.button, contentColor = colors.text)) {
        Text(label, fontWeight = FontWeight.Medium,maxLines=if(multiline)Int.MAX_VALUE else 1,overflow=androidx.compose.ui.text.style.TextOverflow.Ellipsis)
    }
}
