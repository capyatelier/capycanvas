package art.capycanvas

import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.ParcelFileDescriptor
import android.provider.Settings
import android.text.format.DateFormat
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.ExternalResource
import org.junit.rules.RuleChain
import java.io.File
import java.util.Date
import java.util.UUID

class AndroidSystemStatusTest {
    private val compose = createAndroidComposeRule<MainActivity>()
    @get:Rule val isolation: RuleChain = RuleChain.outerRule(object : ExternalResource() {
        override fun before() {
            val root = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir, "system-status-tests/${UUID.randomUUID()}")
            CanvasHost.workspaceDirectoryForTest = File(root, "workspace").absolutePath
            RecoveryController.directoryForTest = File(root, "recovery")
        }
        override fun after() {
            CanvasHost.workspaceDirectoryForTest = null
            RecoveryController.directoryForTest = null
        }
    }).around(compose)
    private fun shell(command: String) {
        ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)).use { it.readBytes() }
    }
    @Test fun headerTracksAndroidClockPreferenceAndShowsLiveBatteryInOrder() {
        shell("input keyevent KEYCODE_WAKEUP"); shell("wm dismiss-keyguard")
        compose.waitUntil(30_000) { compose.activity.host.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
        val host = compose.activity.host
        fun bar(vararg kinds: String) {
            val workspace = JSONObject(host.snapshot!!.getJSONObject("state").getJSONObject("workspace").toString())
            workspace.getJSONObject("layout").put("header", obj("size" to "small", "next_id" to kinds.size + 1, "zones" to JSONArray(listOf(JSONArray(), JSONArray(),
                JSONArray(kinds.mapIndexed { index, kind -> obj("id" to index + 1, "item" to obj("kind" to kind)) })))))
            compose.runOnIdle { host.dispatch(obj("type" to "restore_workspace", "workspace" to workspace)) }
            fun shown(id: Int) = compose.onAllNodesWithTag("header-item-$id").fetchSemanticsNodes().isNotEmpty()
            compose.waitUntil(5_000) { shown(kinds.size) && !shown(kinds.size + 1) }
        }
        bar("document_title", "clock", "battery", "settings")
        val resolver = compose.activity.contentResolver
        val original = Settings.System.getString(resolver, Settings.System.TIME_12_24)
        try {
            compose.onNodeWithTag("system-clock").assertIsDisplayed()
            for (format in listOf("12", "24")) {
                shell("settings put system time_12_24 $format")
                compose.waitUntil(5_000) {
                    compose.onAllNodesWithTag("system-clock").fetchSemanticsNodes().singleOrNull()?.config
                        ?.get(androidx.compose.ui.semantics.SemanticsProperties.Text)?.singleOrNull()?.text == DateFormat.getTimeFormat(compose.activity).format(Date())
                }
                compose.onNodeWithTag("system-clock").assertTextEquals(DateFormat.getTimeFormat(compose.activity).format(Date()))
            }
            val battery = DeviceBattery.from(compose.activity.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED)))!!
            compose.onNodeWithTag("system-battery").assertIsDisplayed().assertContentDescriptionContains("Battery ${battery.percent}%", substring = true)
            fun bounds(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
            val (title, clock, status, settings) = (1..4).map { bounds("header-item-$it") }
            val time = bounds("system-clock")
            val icon = bounds("system-battery")
            val tile = bounds("system-battery-tile")
            assertTrue(title.right <= clock.left && clock.right <= status.left && status.right <= settings.left)
            val density = compose.activity.resources.displayMetrics.density
            assertEquals(settings.width, tile.width, 1f)
            assertEquals(settings.height, tile.height, 1f)
            for ((before, after) in listOf(title to clock, clock to status, status to settings)) assertEquals(6f * density, after.left - before.right, 1f)
            assertEquals(clock.center.x, time.center.x, 1f)
            assertEquals(status.center.x, tile.center.x, 1f)
            assertEquals(tile.center.x, icon.center.x, 1f)
            assertEquals(tile.center.y, icon.center.y, 1f)
            bar("document_title", "settings")
            compose.onNodeWithTag("system-clock").assertDoesNotExist()
            compose.onNodeWithTag("system-battery").assertDoesNotExist()
            compose.onNodeWithTag("system-battery-tile").assertDoesNotExist()
        } finally {
            shell(if (original == null) "settings delete system time_12_24" else "settings put system time_12_24 $original")
        }
    }
    @Test fun batteryUsesSystemLowWarningAndScaledPercentage() {
        val intent = Intent(Intent.ACTION_BATTERY_CHANGED)
            .putExtra(BatteryManager.EXTRA_PRESENT, true).putExtra(BatteryManager.EXTRA_LEVEL, 7)
            .putExtra(BatteryManager.EXTRA_SCALE, 50).putExtra(BatteryManager.EXTRA_BATTERY_LOW, true)
            .putExtra(BatteryManager.EXTRA_STATUS, BatteryManager.BATTERY_STATUS_DISCHARGING)
        assertEquals(DeviceBattery(14, false, true), DeviceBattery.from(intent))
        intent.putExtra(BatteryManager.EXTRA_STATUS, BatteryManager.BATTERY_STATUS_CHARGING)
        assertTrue(DeviceBattery.from(intent)!!.charging)
        intent.putExtra(BatteryManager.EXTRA_PRESENT, false)
        assertNull(DeviceBattery.from(intent))
    }
}
