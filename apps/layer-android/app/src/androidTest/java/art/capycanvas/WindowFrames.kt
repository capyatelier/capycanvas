package art.capycanvas

import android.os.SystemClock
import android.view.Choreographer
import android.view.View
import android.view.ViewTreeObserver

internal class WindowFrames(private val onDraw: () -> Unit = {}, private val onFrame: (Long) -> Boolean) : AutoCloseable {
    var drawNs = 0L
        private set
    var drawBootNs = 0L
        private set
    private val roots = linkedSetOf<View>()
    private val draw = ViewTreeObserver.OnDrawListener {
        drawNs = System.nanoTime()
        drawBootNs = SystemClock.elapsedRealtimeNanos()
        onDraw()
    }
    private val callback = object : Choreographer.FrameCallback {
        override fun doFrame(time: Long) {
            if (onFrame(time)) Choreographer.getInstance().postFrameCallback(this)
        }
    }
    fun watch(view: View) { if (roots.add(view)) view.viewTreeObserver.addOnDrawListener(draw) }
    fun start() = Choreographer.getInstance().postFrameCallback(callback)
    override fun close() {
        Choreographer.getInstance().removeFrameCallback(callback)
        roots.forEach { if (it.viewTreeObserver.isAlive) it.viewTreeObserver.removeOnDrawListener(draw) }
        roots.clear()
    }
}
