import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.KeyEvent;
import android.view.MotionEvent;
import java.lang.reflect.Method;

/** Shell-only stylus replay in the primary app. Screen coordinates, pressure 1,
 * 200 Hz samples. Output markers use CLOCK_BOOTTIME, as does Perfetto. This
 * exercises OS input delivery, not a physical pen or hardware prediction.
 */
public final class AndroidPenMotion {
    private final Object input;
    private final Method inject;
    private final float cx, cy, rx, ry;
    private final MotionEvent.PointerProperties[] properties = {new MotionEvent.PointerProperties()};
    private final MotionEvent.PointerCoords[] coords = {new MotionEvent.PointerCoords()};

    private AndroidPenMotion(float cx, float cy, float rx, float ry) throws Exception {
        this.cx = cx; this.cy = cy; this.rx = rx; this.ry = ry;
        Class<?> manager = Class.forName("android.hardware.input.InputManagerGlobal");
        input = manager.getMethod("getInstance").invoke(null);
        inject = manager.getMethod("injectInputEvent", InputEvent.class, int.class);
        properties[0].id = 7;
        properties[0].toolType = MotionEvent.TOOL_TYPE_STYLUS;
    }
    private void send(InputEvent event) throws Exception {
        if (!((Boolean)inject.invoke(input, event, 0))) throw new IllegalStateException("Injection failed");
    }
    private static void mark(String name) {
        System.out.println(name + " " + SystemClock.elapsedRealtimeNanos());
    }
    private void pen(long down, int action, double angle, boolean hover) throws Exception {
        coords[0].x = cx + rx * (float)Math.cos(angle);
        coords[0].y = cy + ry * (float)Math.sin(angle);
        coords[0].pressure = hover || action == MotionEvent.ACTION_UP ? 0 : 1;
        MotionEvent event = MotionEvent.obtain(down, SystemClock.uptimeMillis(), action, 1,
            properties, coords, 0, 0, 1, 1, 0, 0, InputDevice.SOURCE_STYLUS, 0);
        try { send(event); } finally { event.recycle(); }
    }
    private void motion(String name, boolean hover, int seconds, double hz) throws Exception {
        mark(name + "_start");
        long start = SystemClock.uptimeMillis();
        pen(start, hover ? MotionEvent.ACTION_HOVER_ENTER : MotionEvent.ACTION_DOWN, 0, hover);
        try {
            for (int tick = 1; tick <= seconds * 200; tick++) {
                long delay = start + tick * 5L - SystemClock.uptimeMillis();
                if (delay > 0) SystemClock.sleep(delay);
                pen(start, hover ? MotionEvent.ACTION_HOVER_MOVE : MotionEvent.ACTION_MOVE,
                    (SystemClock.uptimeMillis() - start) / 1000.0 * hz * 2 * Math.PI, hover);
            }
        } finally {
            pen(start, hover ? MotionEvent.ACTION_HOVER_EXIT : MotionEvent.ACTION_UP, 0, hover);
        }
        mark(name + "_end");
    }
    private void history(String name, boolean redo) throws Exception {
        mark(name + "_start");
        long now = SystemClock.uptimeMillis();
        int meta = KeyEvent.META_CTRL_ON | KeyEvent.META_CTRL_LEFT_ON;
        if (redo) meta |= KeyEvent.META_SHIFT_ON | KeyEvent.META_SHIFT_LEFT_ON;
        for (int action : new int[]{KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP})
            send(new KeyEvent(now, SystemClock.uptimeMillis(), action, KeyEvent.KEYCODE_Z, 0,
                meta, -1, 0, 0, InputDevice.SOURCE_KEYBOARD));
        mark(name + "_end");
    }
    public static void main(String[] args) throws Exception {
        if (args.length < 5 || args.length > 7) throw new IllegalArgumentException("centerX centerY radiusX radiusY [workflow|hover|stroke|undo|redo] [turnsPerSecond] [strokeSeconds]");
        double hz = args.length > 5 ? Double.parseDouble(args[5]) : 1;
        int seconds = args.length > 6 ? Integer.parseInt(args[6]) : 10;
        if (!(hz > 0 && hz <= 2 && seconds >= 1 && seconds <= 30)) throw new IllegalArgumentException("Invalid rate/duration");
        AndroidPenMotion p = new AndroidPenMotion(Float.parseFloat(args[0]), Float.parseFloat(args[1]),
            Float.parseFloat(args[2]), Float.parseFloat(args[3]));
        switch (args[4]) {
            case "hover": p.motion("hover", true, 10, 1); break;
            case "stroke": p.motion("stroke", false, seconds, hz); break;
            case "undo": p.history("undo", false); break;
            case "redo": p.history("redo", true); break;
            case "workflow":
                p.motion("hover_before", true, 5, 1); SystemClock.sleep(2000);
                p.motion("stroke", false, seconds, hz); SystemClock.sleep(3000);
                p.history("undo", false); SystemClock.sleep(3000);
                p.history("redo", true); SystemClock.sleep(3000);
                p.motion("hover_after", true, 5, 1); SystemClock.sleep(1000);
                break;
            default: throw new IllegalArgumentException("Unknown action");
        }
        mark("complete");
    }
}
