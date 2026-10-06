package art.capycanvas

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import kotlinx.coroutines.delay
import org.json.JSONObject
import kotlin.math.roundToInt

private const val NoticeTimeoutMs = 4_000L
private val NoticeGap = 12.dp
private val NoticeMaxWidth = 560.dp

/** One of the core's ordered notice choices, identified by its stable token. */
internal class CanvasNoticeAction(val id: String, val label: String, val enabled: Boolean, val reason: String?)

/** The core's notice, or a refused command or action, which has no id. */
internal class CanvasNotice(val id: Long?, val text: String, val actions: List<CanvasNoticeAction>)

/** Projects the shared canvas notice over the canvas, centred above the status
 * strip, or above a canvas action bar along the bottom edge. It is not a popup,
 * so it never takes window focus. It hides after a timeout, answering the core,
 * and the host hides it at the next canvas contact. */
@Composable internal fun NoticeBubble(host: CanvasHost, dock: DockInteraction, status: JSONObject) {
    val notice = host.notice ?: return
    LaunchedEffect(notice) {
        delay(NoticeTimeoutMs)
        host.answerNotice(notice, accept = false)
    }
    val colors = LocalPalette.current
    Surface(Modifier.zIndex(250f).layout { measurable, constraints ->
        val gap = NoticeGap.toPx()
        val left = status.number("x") * density
        val width = status.number("width") * density
        val top = status.number("y") * density
        val bar = dock.canvasBarSlot?.takeIf { it.bottom > top - 2 * gap }
        val bottom = minOf(top, bar?.top ?: top)
        val bubble = measurable.measure(Constraints(maxWidth = minOf(NoticeMaxWidth.toPx(), width - 2 * gap).roundToInt().coerceAtLeast(0)))
        layout(constraints.maxWidth, constraints.maxHeight) {
            bubble.place((left + (width - bubble.width) / 2).roundToInt(), (bottom - gap - bubble.height).roundToInt())
        }
    }.chromeRegion(dock).testTag("canvas-notice").semantics { liveRegion = LiveRegionMode.Polite },
        shape = ControlShape, color = colors.panel, contentColor = colors.text, shadowElevation = 6.dp) {
        Layout({
            Text(notice.text, Modifier.testTag("canvas-notice-text"))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                notice.actions.forEach { action ->
                    Box(Modifier.heightIn(min = 32.dp).clip(ControlShape).focusProperties { canFocus = false }.testTag("canvas-notice-action-${action.id}")
                        .semantics { action.reason?.let { stateDescription = it } }
                        .clickable(enabled = action.enabled, role = Role.Button) { host.answerNotice(notice, accept = true, action = action.id) }.padding(horizontal = 10.dp),
                        contentAlignment = Alignment.Center) {
                        Text(action.label, color = if (action.enabled) colors.accent else colors.secondary, fontWeight = FontWeight.Bold, maxLines = 1)
                    }
                }
            }
        }, Modifier.heightIn(min = 40.dp).padding(start = 14.dp, end = if (notice.actions.isEmpty()) 14.dp else 4.dp, top = 4.dp, bottom = 4.dp)) { (text, actions), constraints ->
            val gap = 8.dp.roundToPx()
            val row = actions.measure(Constraints(maxWidth = constraints.maxWidth))
            val inline = notice.actions.isEmpty() || text.maxIntrinsicWidth(Constraints.Infinity) + gap + row.width <= constraints.maxWidth
            val label = text.measure(Constraints(maxWidth = if (inline) (constraints.maxWidth - row.width - if (notice.actions.isEmpty()) 0 else gap).coerceAtLeast(0) else constraints.maxWidth))
            val width = if (inline) label.width + row.width + if (notice.actions.isEmpty()) 0 else gap else maxOf(label.width, row.width)
            val height = maxOf(constraints.minHeight, if (inline) maxOf(label.height, row.height) else label.height + row.height)
            layout(width, height) {
                if (inline) {
                    label.place(0, (height - label.height) / 2)
                    row.place(label.width + gap, (height - row.height) / 2)
                } else {
                    label.place(0, 0)
                    row.place(width - row.width, label.height)
                }
            }
        }
    }
}
