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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
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
    Row(Modifier.testTag("system-status").padding(horizontal = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(time, Modifier.testTag("system-clock"), maxLines = 1, fontWeight = FontWeight.Medium)
        battery?.let { BatteryIndicator(it) }
    }
}

@Composable internal fun BatteryIndicator(battery: DeviceBattery) {
    val color = if (battery.low && !battery.charging) MaterialTheme.colorScheme.error else LocalPalette.current.text
    val percent = NumberFormat.getIntegerInstance().format(battery.percent)
    val description = "Battery $percent%${if (battery.charging) ", charging" else if (battery.low) ", low" else ""}"
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(2.dp),
        modifier = Modifier.testTag("system-battery").clearAndSetSemantics {
            contentDescription = description
            stateDescription = if (battery.low && !battery.charging) "low" else if (battery.charging) "charging" else "normal"
        }) {
        if (battery.charging) Text("ϟ", color = color, fontSize = 16.sp)
        Box(Modifier.size(42.dp, 22.dp), contentAlignment = Alignment.Center) {
            Canvas(Modifier.fillMaxSize()) {
                val border = 1.5.dp.toPx()
                val body = Size(size.width - 4.dp.toPx(), size.height - border)
                drawRoundRect(color.copy(alpha = .18f), Offset(border, border),
                    Size((body.width - border * 2) * battery.percent / 100, body.height - border * 2), CornerRadius(3.dp.toPx()))
                drawRoundRect(color, Offset(border / 2, border / 2), body,
                    CornerRadius(4.dp.toPx()), style = Stroke(border))
                drawRoundRect(color, Offset(size.width - 2.dp.toPx(), size.height * .3f),
                    Size(2.dp.toPx(), size.height * .4f), CornerRadius(1.dp.toPx()))
            }
            Text(percent, Modifier.padding(end = 4.dp), color = color, fontSize = 12.sp, fontWeight = FontWeight.Bold)
        }
    }
}
