package art.capycanvas

import android.view.InputDevice
import android.view.MotionEvent
import org.junit.Assert.*
import org.junit.Test
import kotlin.math.PI

class AndroidStylusTest {
    private fun coordinates(azimuth: Float) = MotionEvent.PointerCoords().apply {
        x = 40f; y = 60f; pressure = .7f
        orientation = azimuth
        setAxisValue(MotionEvent.AXIS_TILT, .8f)
        setAxisValue(MotionEvent.AXIS_DISTANCE, .2f)
    }

    @Test fun tipAzimuthBecomesBarrelTiltForCurrentAndHistoricalSamples() {
        // MotionEvent's tip points right, up, down, left. The shared brush
        // model's tilt must point toward the barrel in the opposite direction.
        // Android's synthetic event constructor enables orientation only when
        // the initial value is nonzero, even if later history contains it.
        val orientations = floatArrayOf((PI / 2).toFloat(), 0f, PI.toFloat(), (-PI / 2).toFloat())
        val expected = arrayOf(doubleArrayOf(-.8, 0.0), doubleArrayOf(0.0, .8),
            doubleArrayOf(0.0, -.8), doubleArrayOf(.8, 0.0))
        val properties = arrayOf(MotionEvent.PointerProperties().apply {
            id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS
        })
        val event = MotionEvent.obtain(100, 100, MotionEvent.ACTION_MOVE, 1, properties,
            arrayOf(coordinates(orientations[0])), 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0)
        try {
            for (i in 1..3) event.addBatch(100L + i * 5, arrayOf(coordinates(orientations[i])), 0)
            assertEquals(3, event.historySize)
            val samples = DoubleArray(4 * 9)
            packPointerSamples(event, 0, 2, event.historySize, false, samples)
            for (i in expected.indices) {
                val offset = i * 9
                assertEquals("Barrel X for azimuth ${orientations[i]}", expected[i][0], samples[offset + 3], .000001)
                assertEquals("Barrel Y for azimuth ${orientations[i]}", expected[i][1], samples[offset + 4], .000001)
                assertEquals("Azimuth is not barrel twist", 0.0, samples[offset + 5], 0.0)
                assertEquals(.7, samples[offset + 2], .000001)
                assertEquals(.2, samples[offset + 6], .000001)
                assertEquals((100L + i * 5) * 1e6, samples[offset + 7], 0.0)
                assertEquals(2.0, samples[offset + 8], 0.0)
            }
            // Hover/cancellation can omit history; use the current pose only.
            packPointerSamples(event, 0, 4, 0, false, samples)
            assertEquals(.8, samples[3], .000001)
            assertEquals(0.0, samples[4], .000001)
            assertEquals(4.0, samples[8], 0.0)
        } finally { event.recycle() }
    }
}
