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
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.util.Date

class AndroidSystemStatusTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private fun shell(command: String) {
        ParcelFileDescriptor.AutoCloseInputStream(InstrumentationRegistry.getInstrumentation().uiAutomation.executeShellCommand(command)).use { it.readBytes() }
    }
    @Test fun headerTracksAndroidClockPreferenceAndShowsLiveBatteryInOrder() {
        shell("input keyevent KEYCODE_WAKEUP"); shell("wm dismiss-keyguard")
        compose.waitUntil(30_000) { compose.onAllNodesWithTag("system-clock").fetchSemanticsNodes().isNotEmpty() }
        val resolver = compose.activity.contentResolver
        val original = Settings.System.getString(resolver, Settings.System.TIME_12_24)
        try {
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
            val title = compose.onNodeWithTag("document-title").fetchSemanticsNode().boundsInRoot
            val clock = compose.onNodeWithTag("system-clock").fetchSemanticsNode().boundsInRoot
            val icon = compose.onNodeWithTag("system-battery").fetchSemanticsNode().boundsInRoot
            val settings = compose.onNodeWithTag("header-settings").fetchSemanticsNode().boundsInRoot
            assertTrue(title.right <= clock.left && clock.right <= icon.left && icon.right <= settings.left)
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
