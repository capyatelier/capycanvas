package art.capycanvas

import android.view.InputDevice
import android.view.MotionEvent
import org.junit.Assert.*
import org.junit.Test
import kotlin.math.PI

class AndroidStylusTest {
    @Test fun deviceFrameUsesAndroidDisplayRotationWithoutChangingHandedness() {
        val expected = listOf(.3 to .7, .7 to -.3, -.3 to -.7, -.7 to .3)
        for (rotation in 0..3) {
            val axes = androidPenFrame(rotation)
            fun mapped(x: Double, y: Double): Pair<Double, Double> {
                val (horizontal, vertical) = if (axes.getBoolean("swap")) y to x else x to y
                return (if (axes.getBoolean("reverse_horizontal")) -horizontal else horizontal) to
                    (if (axes.getBoolean("reverse_vertical")) -vertical else vertical)
            }
            val actual = mapped(.3, .7)
            assertEquals(expected[rotation].first, actual.first, .000001)
            assertEquals(expected[rotation].second, actual.second, .000001)
            val horizontal = mapped(1.0, 0.0)
            val vertical = mapped(0.0, 1.0)
            assertEquals("A display rotation preserves handedness", 1.0,
                horizontal.first * vertical.second - vertical.first * horizontal.second, .000001)
        }
    }

    @Test fun indirectStylusSourcesPreservePenIdentityAndBarrelTwist() {
        for (tool in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_ERASER, MotionEvent.TOOL_TYPE_MOUSE, MotionEvent.TOOL_TYPE_FINGER, MotionEvent.TOOL_TYPE_UNKNOWN)) {
            for ((source, indirect) in listOf(
                InputDevice.SOURCE_STYLUS to false,
                InputDevice.SOURCE_BLUETOOTH_STYLUS to false,
                InputDevice.SOURCE_TOUCHSCREEN to false,
                (InputDevice.SOURCE_STYLUS or InputDevice.SOURCE_MOUSE) to true,
                (InputDevice.SOURCE_STYLUS or InputDevice.SOURCE_TOUCHPAD) to true,
            )) {
                val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 1; toolType = tool })
                val event = MotionEvent.obtain(100, 100, MotionEvent.ACTION_MOVE, 1, properties,
                    arrayOf(coordinates(.5f)), 0, 0, 1f, 1f, 0, 0, source, 0)
                try {
                    val expected = if (indirect && tool in listOf(MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_ERASER)) 0x40
                        else if (!indirect && tool == MotionEvent.TOOL_TYPE_UNKNOWN) 0x04 else 0
                    assertEquals(expected, pointerDeviceFlags(event, 0, false))
                    assertEquals(expected or 0x80, pointerDeviceFlags(event, 0, true))
                    assertEquals(tool, event.getToolType(0))
                } finally { event.recycle() }
            }
        }
    }

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
            packPointerSamples(event, 0, 2, event.historySize, false, false, samples)
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
            packPointerSamples(event, 0, 4, 0, false, false, samples)
            assertEquals(.8, samples[3], .000001)
            assertEquals(0.0, samples[4], .000001)
            assertEquals(4.0, samples[8], 0.0)
        } finally { event.recycle() }
    }

    @Test fun diagonalInclinationBecomesProjectedAnglesForEveryStylusSource() {
        for (source in listOf(InputDevice.SOURCE_STYLUS,
            InputDevice.SOURCE_TOUCHSCREEN or InputDevice.SOURCE_MOUSE or InputDevice.SOURCE_STYLUS)) {
            val properties = arrayOf(MotionEvent.PointerProperties().apply {
                id = 7; toolType = MotionEvent.TOOL_TYPE_STYLUS
            })
            fun pose(azimuth: Float) = coordinates(azimuth).apply {
                setAxisValue(MotionEvent.AXIS_TILT, (PI / 3).toFloat())
            }
            val event = MotionEvent.obtain(100, 100, MotionEvent.ACTION_MOVE, 1, properties,
                arrayOf(pose((-PI / 4).toFloat())), 0, 0, 1f, 1f, 0, 0, source, 0)
            try {
                event.addBatch(105, arrayOf(pose((-PI / 3).toFloat())), 0)
                val samples = DoubleArray(18)
                packPointerSamples(event, 0, 2, event.historySize, false, false, samples)
                val diagonal = kotlin.math.atan2(kotlin.math.sqrt(3.0 / 8.0), .5)
                assertEquals(diagonal, samples[3], .000001)
                assertEquals(diagonal, samples[4], .000001)
                assertEquals(kotlin.math.atan2(.75, .5), samples[12], .000001)
                assertEquals(kotlin.math.atan2(kotlin.math.sqrt(3.0) / 4, .5), samples[13], .000001)
            } finally { event.recycle() }
        }
    }
}
