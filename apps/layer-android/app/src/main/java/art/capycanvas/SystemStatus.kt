package art.capycanvas

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.ContentObserver
import android.os.BatteryManager
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.format.DateFormat
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.PlatformTextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.NumberFormat
import java.util.Date
import kotlin.math.roundToInt

internal data class DeviceBattery(val percent: Int, val charging: Boolean, val low: Boolean) {
    companion object {
        fun from(intent: Intent?): DeviceBattery? {
            if (intent == null || !intent.getBooleanExtra(BatteryManager.EXTRA_PRESENT, false)) return null
            val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
            val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
            if (level < 0 || scale <= 0) return null
            val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
            return DeviceBattery((level.toDouble() * 100 / scale).roundToInt().coerceIn(0, 100),
                status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL,
                intent.getBooleanExtra(BatteryManager.EXTRA_BATTERY_LOW, false))
        }
    }
}

/** Native broadcasts/settings drive this small UI island, independently of the render owner. */
@Composable internal fun SystemStatus() {
    val context = LocalContext.current
    var time by remember(context) { mutableStateOf(DateFormat.getTimeFormat(context).format(Date())) }
    var battery by remember(context) { mutableStateOf<DeviceBattery?>(null) }
    DisposableEffect(context) {
        fun updateTime() { time = DateFormat.getTimeFormat(context).format(Date()) }
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context?, intent: Intent?) {
                if (intent?.action == Intent.ACTION_BATTERY_CHANGED) battery = DeviceBattery.from(intent)
                else updateTime()
            }
        }
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_BATTERY_CHANGED)
            addAction(Intent.ACTION_TIME_TICK)
            addAction(Intent.ACTION_TIME_CHANGED)
            addAction(Intent.ACTION_TIMEZONE_CHANGED)
            addAction(Intent.ACTION_LOCALE_CHANGED)
        }
        // These are exclusively protected system broadcasts (no app permissions).
        battery = DeviceBattery.from(context.registerReceiver(receiver, filter))
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) = updateTime()
        }
        context.contentResolver.registerContentObserver(Settings.System.getUriFor(Settings.System.TIME_12_24), false, observer)
        updateTime()
        onDispose {
            context.unregisterReceiver(receiver)
            context.contentResolver.unregisterContentObserver(observer)
        }
    }
    Row(Modifier.testTag("system-status"),
        horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.height(36.dp).testTag("system-clock").semantics(mergeDescendants = true) {}.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
            Text(time, maxLines = 1, fontWeight = FontWeight.Medium)
        }
        battery?.let {
            Box(Modifier.size(36.dp).testTag("system-battery-tile"), contentAlignment = Alignment.Center) {
                BatteryIndicator(it)
            }
        }
    }
}

@Composable internal fun BatteryIndicator(battery: DeviceBattery) {
    val dark = LocalPalette.current.dark
    // Both halves must contrast with the inset number, even when nearly empty.
    val (fill, track, ink) = when {
        battery.charging -> Triple(Color(0xFF91B89D), Color(0xFFC4C9CF), Color(0xFF13251A))
        battery.low && dark -> Triple(Color(0xFFBC9996), Color(0xFFA3A8B0), Color(0xFF202226))
        battery.low -> Triple(Color(0xFFA15D59), Color(0xFF707479), Color.White)
        dark -> Triple(Color(0xFFE5E7EB), Color(0xFFA3A8B0), Color(0xFF202226))
        else -> Triple(Color(0xFF3F4246), Color(0xFF707479), Color.White)
    }
    val percent = NumberFormat.getIntegerInstance().format(battery.percent)
    val description = "Battery $percent%${if (battery.charging) ", charging" else if (battery.low) ", low" else ""}"
    val height = with(LocalDensity.current) { 14.sp.toDp() }
    Box(Modifier.size(height * (26f / 14), height).testTag("system-battery").clearAndSetSemantics {
        contentDescription = description
        stateDescription = if (battery.low && !battery.charging) "low" else if (battery.charging) "charging" else "normal"
    }) {
        Canvas(Modifier.fillMaxSize()) {
            val u = size.height / 14
            val body = Size(22 * u, size.height)
            drawRoundRect(track, size = body, cornerRadius = CornerRadius(3 * u))
            clipRect(right = body.width * battery.percent / 100) {
                drawRoundRect(fill, size = body, cornerRadius = CornerRadius(3 * u))
            }
            if (battery.charging) {
                val bolt = Path().apply {
                    moveTo(23f * u, 2f * u); lineTo(18.5f * u, 8f * u)
                    lineTo(21.2f * u, 8f * u); lineTo(20f * u, 12f * u)
                    lineTo(25f * u, 5.9f * u); lineTo(22.7f * u, 5.9f * u)
                    lineTo(24.1f * u, 2f * u); close()
                }
                // A fine pale edge keeps the dark terminal mark legible on dark chrome.
                drawPath(bolt, track, style = Stroke(1.75f * u))
                drawPath(bolt, ink)
            } else {
                drawRoundRect(track, Offset(23 * u, 4 * u), Size(2 * u, 6 * u), CornerRadius(u))
            }
        }
        Box(Modifier.width(height * ((if (battery.charging) 21f else 22f) / 14)).fillMaxHeight(), contentAlignment = Alignment.Center) {
            Text(percent, color = ink, fontSize = if (battery.percent == 100) 10.sp else 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Bold,
                style = androidx.compose.ui.text.TextStyle(platformStyle = PlatformTextStyle(includeFontPadding = false)))
        }
    }
}
