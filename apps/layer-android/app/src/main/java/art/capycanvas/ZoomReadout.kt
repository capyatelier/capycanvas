package art.capycanvas

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupProperties
import org.json.JSONObject

@Composable internal fun ZoomReadout(host: CanvasHost) {
    val camera = host.cameraReadout
    val button = remember { WindowlessMenuButton() }
    Box {
        Text("${camera.zoomPercent}% · ${camera.rotationDegrees}°",
            Modifier.testTag("camera-readout").focusProperties { canFocus = false }
                .opensWindowlessMenu(button, host.catalog.getJSONObject("native_copy").getJSONObject("header").getString("zoom")) { load -> host.query(obj("type" to "zoom_menu")) { load(it as? JSONObject) } }
                .padding(horizontal = 10.dp, vertical = 3.dp))
        ZoomMenu(host, button)
    }
}

@Composable private fun ZoomMenu(host: CanvasHost, button: WindowlessMenuButton) {
    val menu = button.menu
    val close = { button.menu = null; button.closedAt = android.os.SystemClock.uptimeMillis() }
    PopupOwner(menu != null)
    if (menu == null) return
    var typing by remember { mutableStateOf(false) }
    val zoom = host.cameraZoom
    val rotation = host.cameraRotation
    val locks = host.cameraLocks
    val commands = host.snapshot?.getJSONObject("state")?.array("commands")
    val copy = host.catalog.getJSONObject("native_copy").getJSONObject("header")
    LaunchedEffect(zoom, rotation, locks, commands) {
        host.query(obj("type" to "zoom_menu")) { next -> if (button.menu != null) (next as? JSONObject)?.let { button.menu = it } }
    }
    BackHandler(!typing, close)
    DropdownMenu(true, close, Modifier.widthIn(min = 240.dp, max = 380.dp).testTag("zoom-menu"),
        properties = if (typing) PopupProperties(focusable = true) else WindowlessMenu,
        shape = RoundedCornerShape(10.dp), containerColor = LocalPalette.current.panel) {
        Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp).testTag("zoom-field")) {
            NumericSetting(copy.getString("zoom"), zoom, host.catalog.getJSONObject("zoom"), Modifier.fillMaxWidth(), inline = true,
                onTyping = { if (it) typing = true }) { host.dispatch(obj("type" to "set_zoom", "zoom" to it)) }
        }
        Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp).testTag("rotation-field")) {
            NumericSetting(copy.getString("rotation"), rotation, host.catalog.getJSONObject("rotation"), Modifier.fillMaxWidth(),
                onTyping = { if (it) typing = true }) { host.dispatch(obj("type" to "set_rotation", "rotation" to it)) }
        }
        HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = LocalPalette.current.divider)
        WorkspaceMenuItems(host, menu.array("sections"), close)
        HorizontalDivider(Modifier.padding(horizontal = 6.dp, vertical = 6.dp), color = LocalPalette.current.divider)
        NavigationButtons(host, menu.array("buttons").objects(), "zoom")
    }
}
