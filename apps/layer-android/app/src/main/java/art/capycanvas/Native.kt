package art.capycanvas

import android.view.Surface

/** Only CanvasHost's render Looper can access a native session handle. */
internal object Native {
    init { System.loadLibrary("layer_android") }
    @JvmStatic external fun create(profiling: Boolean): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun attach(handle: Long, surface: Surface)
    @JvmStatic external fun detach(handle: Long)
    @JvmStatic external fun resize(handle: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun scroll(handle: Long, x: Float, y: Float, dx: Float, dy: Float, zoom: Boolean, horizontal: Boolean)
    @JvmStatic external fun dispatch(handle: Long, action: String)
    @JvmStatic external fun input(handle: Long, input: String): String
    @JvmStatic external fun pointer(handle: Long, id: Long, tool: Int, button: Int, records: DoubleArray, count: Int, predicted: Boolean)
    @JvmStatic external fun frame(handle: Long, now: Long, presentation: Long): Boolean
    @JvmStatic external fun frameCost(handle: Long, output: LongArray)
    @JvmStatic external fun snapshot(handle: Long): String?
    @JvmStatic external fun query(handle: Long, query: String): String
    /** Pure shared number-field math; no native session handle or GPU work. */
    @JvmStatic external fun number(request: String): String
}
