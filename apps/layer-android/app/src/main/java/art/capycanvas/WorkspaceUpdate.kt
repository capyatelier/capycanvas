package art.capycanvas

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Easing
import androidx.compose.animation.core.tween
import androidx.compose.runtime.*
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import org.json.JSONObject
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.collectLatest

internal val TabSlideEasing = Easing { t -> 1f - (1f - t) * (1f - t) * (1f - t) }

/** Absolute logical geometry for one shared Rust model revision. No widget models
 * or bitmap copies cross the bridge during ordinary movement. */
internal data class WorkspaceGeometry(val revision: Long, val modelRevision: Long,
    val group: Int?, val bounds: Rect?, val tab: Tab?, val hint: JSONObject?) {
    data class Tab(val group: Int, val panel: String, val sourceOffset: Float, val offsets: Map<Int, Float>)
    companion object {
        fun read(value: JSONObject): WorkspaceGeometry {
            val drag = value.objectOrNull("drag")
            val group = drag?.objectOrNull("group")
            val tab = drag?.objectOrNull("tab")?.let { tab ->
                val preview = tab.getJSONObject("preview")
                Tab(tab.getInt("group"), tab.getString("panel"),
                    preview.getJSONObject("bounds").number("x") - tab.getJSONObject("source").number("x"),
                    preview.array("offsets").objects().associate { it.getInt("index") to it.number("x") })
            }
            return WorkspaceGeometry(value.getLong("revision"), value.getLong("model_revision"),
                group?.getInt("id"), group?.getJSONObject("bounds")?.rect(), tab, drag?.objectOrNull("drop_hint"))
        }
    }
}

internal fun JSONObject.rect() = Rect(number("x"), number("y"), number("x") + number("width"), number("y") + number("height"))

/** Motion only invalidates placement. A late measurement can change the saved
 * layout, but the held panel and its handles retain their original allocation. */
@Composable internal fun Modifier.workspacePlaced(host: CanvasHost, group: Int, rect: JSONObject,
    base: JSONObject, density: Float, edge: String? = null): Modifier {
    val baseWidth = base.number("width"); val baseHeight = base.number("height")
    val size by remember(host, group, baseWidth, baseHeight) { derivedStateOf {
        host.workspaceGeometry?.takeIf { it.group == group }?.bounds?.size ?: Size(baseWidth, baseHeight)
    } }
    val dw = size.width - baseWidth; val dh = size.height - baseHeight
    return offset {
        val motion = host.workspaceGeometry?.takeIf { it.group == group }?.bounds
        val x = if (motion == null) rect.number("x") else motion.left + rect.number("x") - base.number("x") + if (edge?.contains("right") == true) dw else 0f
        val y = if (motion == null) rect.number("y") else motion.top + rect.number("y") - base.number("y") + if (edge?.contains("bottom") == true) dh else 0f
        IntOffset((x * density).roundToInt(), (y * density).roundToInt())
    }.size((rect.number("width") + if (edge == null || edge == "top" || edge == "bottom") dw else 0f).coerceAtLeast(0f).dp,
        (rect.number("height") + if (edge == null || edge == "left" || edge == "right") dh else 0f).coerceAtLeast(0f).dp)
}

/** Tab slots stay frozen for insertion decisions. Only retained native drawing
 * instructions translate inside the header clip; there is no bitmap rescaling. */
@Composable internal fun Modifier.workspaceTabMotion(host: CanvasHost, group: Int, panel: String, index: Int, density: Float): Modifier {
    val neighbor = remember(group, panel) { Animatable(0f) }
    LaunchedEffect(host, group, panel, index) {
        snapshotFlow {
            host.workspaceGeometry?.tab?.takeIf { it.group == group && it.panel != panel }
                ?.let { it.offsets[index] ?: 0f }
        }.collectLatest { offset ->
            if (offset == null) neighbor.snapTo(0f) else neighbor.animateTo(offset, tween(120, easing = TabSlideEasing))
        }
    }
    return graphicsLayer {
        val tab = host.workspaceGeometry?.tab?.takeIf { it.group == group }
        translationX = ((if (tab?.panel == panel) tab.sourceOffset else if (tab != null) neighbor.value else 0f) * density).roundToInt().toFloat()
    }
}
