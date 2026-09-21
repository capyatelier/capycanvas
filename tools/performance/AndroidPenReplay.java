import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.MotionEvent;
import java.io.BufferedReader;
import java.io.FileReader;
import java.lang.reflect.Method;

/** Replay captured app coordinates/pressure, including stale pressure on up.
 * CSV columns: phase (1=down,2=move,3=up), elapsedMicros, x, y, pressure.
 * Millisecond MotionEvent pacing approximates the original nanosecond capture.
 */
public final class AndroidPenReplay {
    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("capture.csv");
        Class<?> manager = Class.forName("android.hardware.input.InputManagerGlobal");
        Object input = manager.getMethod("getInstance").invoke(null);
        Method inject = manager.getMethod("injectInputEvent", InputEvent.class, int.class);
        MotionEvent.PointerProperties prop = new MotionEvent.PointerProperties();
        prop.id = 7;
        prop.toolType = MotionEvent.TOOL_TYPE_STYLUS;
        long start = SystemClock.uptimeMillis();
        try (BufferedReader rows = new BufferedReader(new FileReader(args[0]))) {
            rows.readLine();
            String row;
            while ((row = rows.readLine()) != null) {
                String[] values = row.split(",");
                int phase = Integer.parseInt(values[0]);
                long time = start + Math.round(Double.parseDouble(values[1]) / 1000.);
                long wait = time - SystemClock.uptimeMillis();
                if (wait > 0) SystemClock.sleep(wait);
                MotionEvent.PointerCoords coords = new MotionEvent.PointerCoords();
                coords.x = Float.parseFloat(values[2]);
                coords.y = Float.parseFloat(values[3]);
                coords.pressure = Float.parseFloat(values[4]);
                int action = phase == 1 ? MotionEvent.ACTION_DOWN : phase == 3 ? MotionEvent.ACTION_UP : MotionEvent.ACTION_MOVE;
                MotionEvent event = MotionEvent.obtain(start, time, action, 1,
                    new MotionEvent.PointerProperties[]{prop}, new MotionEvent.PointerCoords[]{coords},
                    0, 0, 1, 1, 0, 0, InputDevice.SOURCE_STYLUS, 0);
                try {
                    if (!((Boolean)inject.invoke(input, event, 0))) throw new IllegalStateException("Injection failed");
                } finally { event.recycle(); }
                System.out.println(row);
            }
        }
    }
}
