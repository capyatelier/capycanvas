package art.capycanvas

import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import org.json.JSONArray
import org.json.JSONObject

/** Partial Zen uses the core's edge clusters, preserving saved dock topology. */
@Composable internal fun ZenToolbars(host: CanvasHost, snapshot: JSONObject, panels: Map<String, JSONObject>, dock: DockInteraction) {
    snapshot.objectOrNull("zen_toolbars")?.array("sections")?.objects()?.forEachIndexed { index, section ->
        val id = section.getString("panel")
        val panel = panels[id] ?: return@forEachIndexed
        val tiles = panel.array("tiles").objects().associateBy { it.getInt("id") }
        val projectedTiles = JSONArray()
        val bounds = JSONArray()
        section.array("tiles").values().forEach { pair ->
            pair as JSONArray
            tiles[pair.getInt(0)]?.let { projectedTiles.put(it); bounds.put(pair.getJSONObject(1)) }
        }
        val projected = JSONObject(panel.toString()).put("tiles", projectedTiles).put("tile_style", section.getString("style"))
        key(id, index) {
            ToolRibbon(host, projected, obj("tiles" to bounds), dock,
                Modifier.placed(section.getJSONObject("bounds"), dock.density).zIndex(150f)
                    .testTag("zen-section-$index").shadow(6.dp, RoundedCornerShape(6.dp))
                    .clip(RoundedCornerShape(6.dp)).background(LocalPalette.current.panel),
                section.getString("edge") in listOf("left", "right"))
        }
    }
}
