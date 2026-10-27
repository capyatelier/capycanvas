package art.capycanvas

import android.graphics.Bitmap
import android.os.SystemClock
import android.view.MotionEvent
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.platform.ViewRootForTest
import org.junit.Assert.assertTrue
import kotlin.math.roundToInt

internal fun assertButtonContactFeedback(owner: ViewRootForTest, bounds: Rect, label: String,
    minimumChangedFraction: Float = .02f, send: (Int) -> Unit) {
    val origin = IntArray(2)
    instrumentation.runOnMainSync { owner.view.getLocationOnScreen(origin) }
    fun pixels(settle: Long = 200): IntArray {
        SystemClock.sleep(settle)
        val shot = instrumentation.uiAutomation.takeScreenshot()
        val crop = Bitmap.createBitmap(shot, origin[0] + bounds.left.roundToInt(), origin[1] + bounds.top.roundToInt(),
            bounds.width.roundToInt(), bounds.height.roundToInt())
        val values = IntArray(crop.width * crop.height)
        crop.getPixels(values, 0, crop.width, 0, 0, crop.width, crop.height)
        crop.recycle(); shot.recycle()
        return values
    }
    fun difference(a: IntArray, b: IntArray) = a.indices.count { i ->
        listOf(0, 8, 16).any { shift -> kotlin.math.abs((a[i] shr shift and 255) - (b[i] shr shift and 255)) > 8 }
    }
    val idle = pixels()
    val pressed = try { send(MotionEvent.ACTION_DOWN); pixels() } finally { send(MotionEvent.ACTION_CANCEL) }
    val cancelled = pixels(600)
    val heldChanges = difference(idle, pressed)
    val cancelledChanges = difference(idle, cancelled)
    val minimumHeldChanges = (idle.size * minimumChangedFraction).toInt()
    val largestChange = idle.indices.maxOf { i -> listOf(0, 8, 16).maxOf { shift ->
        kotlin.math.abs((idle[i] shr shift and 255) - (pressed[i] shr shift and 255))
    } }
    if (heldChanges <= minimumHeldChanges || cancelledChanges >= idle.size / 50) {
        listOf(idle, pressed, cancelled).forEachIndexed { index, values ->
            val file = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/button-feedback/${label.replace(Regex("[^A-Za-z0-9=]"), "_")}-$index.png")
            file.parentFile!!.mkdirs()
            val bitmap = Bitmap.createBitmap(values, bounds.width.roundToInt(), bounds.height.roundToInt(), Bitmap.Config.ARGB_8888)
            file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }
    assertTrue("$label shows held contact feedback: $heldChanges/${idle.size} pixels, largest channel change $largestChange", heldChanges > minimumHeldChanges)
    assertTrue("$label cancellation restores its idle appearance: $cancelledChanges/${idle.size} pixels", cancelledChanges < idle.size / 50)
}
