package art.capycanvas

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlinx.coroutines.delay
import org.json.JSONObject
import kotlin.math.roundToInt

private const val NoticeTimeoutMs = 4_000L
private val NoticeMargin = 12.dp
private val RowTextPadding = 6.dp
private val StackedTextTop = 10.dp
private val StackedTextBottom = 2.dp

/** One of the core's ordered notice choices, identified by its stable token. */
internal data class CanvasNoticeAction(val id: String, val label: String, val enabled: Boolean, val accent: Boolean, val reason: String?)

/** The core's notice, or a refused command or action, which has no id. */
internal data class CanvasNotice(val id: Long?, val name: String, val text: String, val actions: List<CanvasNoticeAction>)

/** Projects the shared canvas notice at its shared placement: centred on the
 * placement's anchor, no wider than its maximum width, and above a canvas action
 * bar along the bottom edge. It is drawn as a canvas action bar strip whose
 * actions are the bar's worded controls, beside one line of text or right-aligned
 * under longer text. It is not a popup, so it never takes window focus. It hides
 * after a timeout, answering the core, and the host hides it at the next canvas
 * contact. */
@OptIn(ExperimentalLayoutApi::class)
@Composable internal fun NoticeBubble(host: CanvasHost, dock: DockInteraction, placement: JSONObject) {
    val notice = host.notice ?: return
    val current by rememberUpdatedState(notice)
    LaunchedEffect(notice.id ?: notice) {
        delay(NoticeTimeoutMs)
        host.answerNotice(current, accept = false)
    }
    val colors = LocalPalette.current
    val tiles = remember(host.languageTag) { toolbarUi(host.languageTag, obj("type" to "style", "style" to "small")) }
    val tile = tiles.array("size").getDouble(1).toFloat().dp
    val gap = tiles.number("gap").dp
    val control = panelMetrics().height
    Box(Modifier.zIndex(250f).layout { measurable, constraints ->
        val x = placement.number("x") * density
        val anchor = placement.number("y") * density
        val bar = dock.canvasBarSlot?.takeIf { it.bottom > placement.number("bar_floor") * density }
        val bottom = minOf(anchor, bar?.let { it.top - NoticeMargin.toPx() } ?: anchor)
        val bubble = measurable.measure(Constraints(maxWidth = (placement.number("max_width") * density).roundToInt().coerceAtLeast(0)))
        layout(constraints.maxWidth, constraints.maxHeight) {
            bubble.place((x - bubble.width / 2f).roundToInt(), (bottom - bubble.height).roundToInt())
        }
    }.chromeRegion(dock).testTag("canvas-notice").semantics { liveRegion = LiveRegionMode.Polite; paneTitle = notice.name }
        .panelSurface(BarElevation, SurfaceShape).glass(SurfaceShape)) {
        CompositionLocalProvider(LocalPalette provides colors.onGlass) {
            Surface(color = colors.onGlass.panelFill, contentColor = colors.text) {
                Layout({
                    Text(notice.text, Modifier.testTag("canvas-notice-text"))
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(gap, Alignment.End)) {
                        notice.actions.forEach { action ->
                            Box(Modifier.width(IntrinsicSize.Max).height(tile).padding(horizontal = FieldClear.dp), contentAlignment = Alignment.Center) {
                                Box(Modifier.height(control)) {
                                    ToolOptionAction(obj("id" to action.id, "label" to action.label, "enabled" to action.enabled, "tooltip" to action.label,
                                        "disabled_reason" to (action.reason ?: JSONObject.NULL), "icon" to JSONObject.NULL), false, 16, action.label, action.accent,
                                        "canvas-notice", focusable = true) { host.answerNotice(notice, accept = true, action = it) }
                                }
                            }
                        }
                    }
                }) { (text, actions), constraints ->
                    val start = LabelStart.dp.roundToPx()
                    val end = (if (notice.actions.isEmpty()) LabelStart else LabelEnd).dp.roundToPx()
                    val spacing = gap.roundToPx()
                    val offers = if (notice.actions.isEmpty()) 0 else actions.maxIntrinsicWidth(Constraints.Infinity)
                    val words = start + text.maxIntrinsicWidth(Constraints.Infinity) + end
                    val inline = toolbarUi(host.languageTag, obj("type" to "notice_layout", "text" to words, "content" to constraints.maxWidth,
                        "actions" to offers, "gap" to spacing, "min_text_width" to placement.number("min_text_width") * density)).getBoolean("beside")
                    if (inline) {
                        val row = actions.measure(Constraints(maxWidth = offers))
                        val beside = if (offers == 0) 0 else spacing + row.width
                        val label = text.measure(Constraints(maxWidth = (constraints.maxWidth - start - end - beside).coerceAtLeast(0)))
                        val width = start + label.width + end + beside
                        val height = maxOf(tile.roundToPx(), label.height + 2 * RowTextPadding.roundToPx(), row.height)
                        layout(width, height) {
                            label.place(start, (height - label.height) / 2)
                            row.place(width - row.width, (height - row.height) / 2)
                        }
                    } else {
                        val label = text.measure(Constraints(maxWidth = (constraints.maxWidth - 2 * start).coerceAtLeast(0)))
                        val row = actions.measure(Constraints(maxWidth = constraints.maxWidth))
                        val width = maxOf(start + label.width + start, row.width)
                        val top = StackedTextTop.roundToPx()
                        val below = top + label.height + StackedTextBottom.roundToPx() + spacing
                        layout(width, below + row.height) {
                            label.place(start, top)
                            row.place(width - row.width, below)
                        }
                    }
                }
            }
        }
    }
}
