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
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
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

/** The core's notice, or a refused command or action, which has no id. */
internal class CanvasNotice(val id: Long?, val text: String, val action: String?)

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
        Row(Modifier.heightIn(min = 40.dp).padding(start = 14.dp, end = if (notice.action == null) 14.dp else 4.dp, top = 4.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(notice.text, Modifier.weight(1f, fill = false).testTag("canvas-notice-text"))
            notice.action?.let { label ->
                Box(Modifier.heightIn(min = 32.dp).clip(ControlShape).focusProperties { canFocus = false }.testTag("canvas-notice-action")
                    .clickable(role = Role.Button) { host.answerNotice(notice, accept = true) }.padding(horizontal = 10.dp),
                    contentAlignment = Alignment.Center) {
                    Text(label, color = colors.accent, fontWeight = FontWeight.Bold, maxLines = 1)
                }
            }
        }
    }
}
