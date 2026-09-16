package art.capycanvas

import android.view.Surface

/** Only CanvasHost's render Looper can access a native session handle. */
internal object Native {
    init { System.loadLibrary("layer_android") }
    @JvmStatic external fun create(profiling: Boolean): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun attach(handle: Long, surface: Surface, cacheDirectory: String)
    @JvmStatic external fun finishStartupCache(handle: Long)
    @JvmStatic external fun resetGpu(handle: Long)
    external fun destroyGpuForTest(handle: Long)
    @JvmStatic external fun detach(handle: Long)
    @JvmStatic external fun resize(handle: Long, width: Int, height: Int, density: Float)
    @JvmStatic external fun scroll(handle: Long, x: Float, y: Float, dx: Float, dy: Float, zoom: Boolean, horizontal: Boolean)
    @JvmStatic external fun dispatch(handle: Long, action: String)
    @JvmStatic external fun input(handle: Long, input: String): String
    @JvmStatic external fun predictionAvailability(handle: Long, available: Boolean)
    @JvmStatic external fun pointer(handle: Long, id: Long, tool: Int, button: Int, records: DoubleArray, count: Int, predicted: Boolean)
    @JvmStatic external fun frame(handle: Long, now: Long, presentation: Long): Boolean
    /** First buffer on the current surface has completed GPU work. */
    @JvmStatic external fun surfaceReady(handle: Long): Boolean
    @JvmStatic external fun frameCost(handle: Long, output: LongArray)
    @JvmStatic external fun snapshot(handle: Long): String?
    @JvmStatic external fun query(handle: Long, query: String): String
    /** Stateless shared color forms/previews; safe without a session handle. */
    @JvmStatic external fun colorUi(request: String): String
    @JvmStatic external fun workspace(handle: Long, request: String): String
    @JvmStatic external fun navigatorPlacements(handle: Long, placements: String)
    @JvmStatic external fun takeFilterPreviews(handle: Long): Array<Any>?
    @JvmStatic external fun importLayer(handle: Long, name: String, width: Int, height: Int, rgba: ByteArray)
    @JvmStatic external fun projectRecoveryTask(handle: Long, opening: Boolean): Long
    /** File worker only: atomic publication of a captured recovery snapshot. */
    @JvmStatic external fun projectPublish(task: Long, path: String)
    @JvmStatic external fun projectTask(handle: Long, request: Int, location: String, epoch: Long, revision: Long): Long
    /** File worker only; consumes the detached descriptor, retains the task. */
    @JvmStatic external fun inspectProfile(bytes: ByteArray): String
    @JvmStatic external fun projectProfilePrompt(task: Long): String
    @JvmStatic external fun projectAssumeProfile(task: Long, profile: String)
    @JvmStatic external fun projectOptions(task: Long, options: String)
    @JvmStatic external fun projectWork(task: Long, fd: Int, width: Int, height: Int)
    @JvmStatic external fun projectAdopt(handle: Long, task: Long, location: String)
    @JvmStatic external fun projectFree(task: Long)
    @JvmStatic external fun documentComplete(handle: Long, request: Int, success: Boolean, error: String)
    @JvmStatic external fun documentClose(handle: Long, request: Int, decision: String)
    @JvmStatic external fun colorTask(handle: Long, id: Int, cancel: Long): Long
    @JvmStatic external fun colorWork(task: Long, choice: String): String
    @JvmStatic external fun colorPreview(task: Long, after: Boolean): ByteArray
    @JvmStatic external fun colorAdopt(handle: Long, task: Long)
    @JvmStatic external fun colorFree(task: Long)
    @JvmStatic external fun captureControl(): Long
    @JvmStatic external fun captureCancel(control: Long)
    @JvmStatic external fun captureFree(control: Long)
    @JvmStatic external fun inspectionTask(handle: Long, control: Long): Long
    @JvmStatic external fun inspectionHistogram(task: Long): String
    @JvmStatic external fun projectExportOptions(task: Long, recipe: String)
    @JvmStatic external fun projectExportTask(handle: Long, request: Int, now: Long, cancel: Long = 0): Long
    /** Pure shared number-field math; no native session handle or GPU work. */
    @JvmStatic external fun number(request: String): String
    /** Pure shared color-wheel hit geometry, independent of the render thread. */
    @JvmStatic external fun colorWheelHit(request: String): String
    @JvmStatic external fun colorPanelLayout(size: Float): String
    @JvmStatic external fun colorHueStops(shape: String, space: String): String
    /** Shared sRGB field raster as Android ARGB pixels; no session access. */
    @JvmStatic external fun colorFieldPixels(size: Int, hue: Float, shape: String, space: String): IntArray
}
