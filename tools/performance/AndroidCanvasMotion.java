import android.os.SystemClock;
import android.view.InputDevice;
import android.view.MotionEvent;
import java.lang.reflect.Method;

/** Shell-only, OS-injected two-finger navigation on the already-open canvas.
 * Not packaged with the app. Coordinates are physical screen pixels. Run only
 * over an unobstructed canvas; this deliberately changes its camera.
 */
public final class AndroidCanvasMotion {
    public static void main(String[] args) throws Exception {
        boolean limits = args.length == 4 && args[3].equals("limits");
        if (args.length != 3 && args.length != 6 && !limits)
            throw new IllegalArgumentException("seconds centerX centerY [limits | periodMs minRadius maxRadius]");
        int seconds = Integer.parseInt(args[0]);
        float cx = Float.parseFloat(args[1]), cy = Float.parseFloat(args[2]);
        double periodMs = args.length == 6 ? Double.parseDouble(args[3]) : 5000;
        double minRadius = args.length == 6 ? Double.parseDouble(args[4]) : 60;
        double maxRadius = args.length == 6 ? Double.parseDouble(args[5]) : 480;
        if (seconds < 1 || seconds > 300) throw new IllegalArgumentException("seconds must be 1..300");
        if (!(periodMs >= 250 && minRadius > 0 && maxRadius >= minRadius && maxRadius <= 1000))
            throw new IllegalArgumentException("Invalid gesture range");
        Class<?> manager = Class.forName("android.hardware.input.InputManagerGlobal");
        Object input = manager.getMethod("getInstance").invoke(null);
        Method inject = manager.getMethod("injectInputEvent", android.view.InputEvent.class, int.class);
        MotionEvent.PointerProperties[] properties = new MotionEvent.PointerProperties[2];
        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[2];
        for (int i = 0; i < 2; ++i) {
            properties[i] = new MotionEvent.PointerProperties();
            properties[i].id = i;
            properties[i].toolType = MotionEvent.TOOL_TYPE_FINGER;
            coords[i] = new MotionEvent.PointerCoords();
            coords[i].pressure = 1;
            coords[i].size = .1f;
        }
        long began = SystemClock.uptimeMillis(), down = began;
        boolean touching = false;
        int ticks = seconds * 120;
        try {
            for (int tick = 0; tick <= ticks; ++tick) {
                long due = began + tick * 1000L / 120;
                long wait = due - SystemClock.uptimeMillis();
                if (wait > 0) SystemClock.sleep(wait);
                double phase = tick / 120.0 * 2 * Math.PI / (periodMs / 1000);
                double radius = minRadius * Math.pow(maxRadius / minRadius, (1 - Math.cos(phase)) / 2);
                double angle = .85 * Math.sin(phase);
                if (limits) {
                    // Two 100x zoom-out pinches, then two 100x zoom-in pinches.
                    // Recontacting between pinches reaches both camera clamps;
                    // a single oscillation otherwise only revisits its start zoom.
                    double t = (tick % 16) / 15.0;
                    boolean out = (tick / 16) % 4 < 2;
                    radius = out ? 600 * Math.pow(.01, t) : 6 * Math.pow(100, t);
                    angle = (out ? -1 : 1) * 1.3 * t;
                }
                for (int i = 0; i < 2; ++i) {
                    double sign = i == 0 ? -1 : 1;
                    coords[i].x = cx + (float)(sign * radius * Math.cos(angle));
                    coords[i].y = cy + (float)(sign * radius * Math.sin(angle));
                }
                if (!touching) {
                    down = SystemClock.uptimeMillis();
                    send(inject, input, down, MotionEvent.ACTION_DOWN, 1, properties, coords);
                    send(inject, input, down, MotionEvent.ACTION_POINTER_DOWN | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, properties, coords);
                    touching = true;
                } else {
                    send(inject, input, down, MotionEvent.ACTION_MOVE, 2, properties, coords);
                }
                if (limits && tick % 16 == 15) {
                    send(inject, input, down, MotionEvent.ACTION_POINTER_UP | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, properties, coords);
                    send(inject, input, down, MotionEvent.ACTION_UP, 1, properties, coords);
                    touching = false;
                }
            }
        } finally {
            if (touching) {
                send(inject, input, down, MotionEvent.ACTION_POINTER_UP | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, properties, coords);
                send(inject, input, down, MotionEvent.ACTION_UP, 1, properties, coords);
            }
        }
        System.out.println("Navigation complete: limits=" + limits + " ticks=" + ticks + " elapsed_ms=" + (SystemClock.uptimeMillis() - began));
    }

    private static void send(Method inject, Object input, long down, int action, int count,
            MotionEvent.PointerProperties[] properties, MotionEvent.PointerCoords[] coords) throws Exception {
        MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, count,
            properties, coords, 0, 0, 1, 1, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0);
        try {
            if (!((Boolean)inject.invoke(input, event, 2))) throw new IllegalStateException("Input injection failed");
        } finally {
            event.recycle();
        }
    }
}
