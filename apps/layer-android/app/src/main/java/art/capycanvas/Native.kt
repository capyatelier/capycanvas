package art.capycanvas

import android.view.Surface

/** Only CanvasHost's render Looper can access a native session handle. */
internal object Native {
    init { System.loadLibrary("layer_android") }
    @JvmStatic external fun create(profiling: Boolean): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun attach(handle: Long, surface: Surface, cacheDirectory: String)
    @JvmStatic external fun displayStatus(handle: Long): String
    @JvmStatic external fun displayInfo(handle: Long, available: Boolean)
    @JvmStatic external fun finishStartupCache(handle: Long)
    @JvmStatic external fun resetGpu(handle: Long)
    external fun surfacePixelsForTest(handle: Long): ByteArray
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
    @JvmStatic external fun modelUpdate(handle: Long): String?
    @JvmStatic external fun strokeRecording(handle: Long, action: Int): String
    @JvmStatic external fun strokeRecordingData(handle: Long): ByteArray
    @JvmStatic external fun query(handle: Long, query: String): String
    /** Stateless shared color forms/previews; safe without a session handle. */
    @JvmStatic external fun colorUi(request: String): String
    @JvmStatic external fun workspace(handle: Long, request: String): String
    @JvmStatic external fun navigatorPlacements(handle: Long, placements: String)
    @JvmStatic external fun takeFilterPreviews(handle: Long): Array<Any>?
    @JvmStatic external fun importLayer(handle: Long, name: String, width: Int, height: Int, rgba: ByteArray)
    @JvmStatic external fun documentTabs(handle: Long, request: String): String
    @JvmStatic external fun documentSwitch(handle: Long, id: Long, close: Boolean): Long
    @JvmStatic external fun documentResumeWork(task: Long)
    @JvmStatic external fun documentResume(handle: Long, task: Long)
    @JvmStatic external fun documentResumeFree(task: Long)
    @JvmStatic external fun documentSpillTask(handle: Long): Long
    @JvmStatic external fun documentSpillWork(task: Long)
    @JvmStatic external fun projectParkReady(handle: Long, task: Long): Boolean
    @JvmStatic external fun projectRecoveryFor(handle: Long, id: Long): Long
    @JvmStatic external fun projectRecoveryTask(handle: Long, opening: Boolean): Long
    /** File worker only: atomic publication of a captured recovery snapshot. */
    @JvmStatic external fun projectPublish(task: Long, path: String)
    @JvmStatic external fun projectTask(handle: Long, request: Int, location: String, epoch: Long, revision: Long): Long
    @JvmStatic external fun importSource(prefix: ByteArray): String
    @JvmStatic external fun photoFormats(): String
    @JvmStatic external fun imageImportContext(handle: Long, screen: String, destination: String): String
    @JvmStatic external fun imageImportTask(handle: Long, request: Int, context: String, cancel: Long): Long
    @JvmStatic external fun imageImportRead(task: Long, fd: Int, name: String)
    @JvmStatic external fun imageImportProfilePrompt(task: Long): String
    @JvmStatic external fun imageImportAssumeProfile(task: Long, profile: String)
    @JvmStatic external fun imageImportAdopt(handle: Long, task: Long)
    @JvmStatic external fun imageImportFree(task: Long)
    /** File worker only; consumes the detached descriptor, retains the task. */
    @JvmStatic external fun exportPresets(bytes: ByteArray, request: String, color: String): Array<Any?>
    @JvmStatic external fun recoveryUpdate(state: String, event: String): String
    @JvmStatic external fun profileLibrary(request: String, bytes: ByteArray): String
    @JvmStatic external fun paletteFile(request: String, bytes: ByteArray): Array<Any>
    @JvmStatic external fun inspectProfileSummary(bytes: ByteArray): String
    @JvmStatic external fun inspectProfile(bytes: ByteArray): String
    @JvmStatic external fun projectProfilePrompt(task: Long): String
    @JvmStatic external fun projectAssumeProfile(task: Long, profile: String)
    @JvmStatic external fun projectOptions(task: Long, options: String)
    @JvmStatic external fun projectOpenControl(task: Long, control: Long)
    @JvmStatic external fun projectWork(task: Long, fd: Int, width: Int, height: Int)
    @JvmStatic external fun projectAdopt(handle: Long, task: Long, location: String)
    @JvmStatic external fun projectFree(task: Long)
    @JvmStatic external fun documentComplete(handle: Long, request: Int, success: Boolean, error: String)
    @JvmStatic external fun documentClose(handle: Long, request: Int, decision: String)
    @JvmStatic external fun documentInfoTask(handle: Long): Long
    @JvmStatic external fun documentInfo(task: Long): String
    @JvmStatic external fun sourceTask(handle: Long, id: Int, cancel: Long): Long
    @JvmStatic external fun sourceWork(task: Long, profile: String)
    @JvmStatic external fun sourcePrepareComparison(handle: Long, task: Long)
    @JvmStatic external fun sourceCompare(task: Long): String
    @JvmStatic external fun sourcePreview(task: Long, after: Boolean): ByteArray
    @JvmStatic external fun sourceAdopt(handle: Long, task: Long)
    @JvmStatic external fun sourceFree(task: Long)
    @JvmStatic external fun colorTask(handle: Long, id: Int, cancel: Long): Long
    @JvmStatic external fun colorWork(task: Long, choice: String, copy: Boolean = false): String
    @JvmStatic external fun colorPreview(task: Long, after: Boolean): ByteArray
    @JvmStatic external fun colorAdopt(handle: Long, task: Long)
    @JvmStatic external fun colorWriteCopy(task: Long, fd: Int)
    @JvmStatic external fun colorFree(task: Long)
    @JvmStatic external fun captureControl(): Long
    @JvmStatic external fun proofTexture(edge: Int): IntArray
    @JvmStatic external fun proofControl(handle: Long, action: String)
    @JvmStatic external fun toneStatus(handle: Long): String
    @JvmStatic external fun toneTask(handle: Long, control: Long): Long
    @JvmStatic external fun toneReferenceDifference(task: Long): String
    @JvmStatic external fun toneWork(task: Long)
    @JvmStatic external fun toneApply(handle: Long, task: Long): Boolean
    @JvmStatic external fun toneFailed(handle: Long, generation: Int, error: String)
    @JvmStatic external fun toneRelease(task: Long)
    @JvmStatic external fun proofStatus(handle: Long): String
    @JvmStatic external fun proofForm(handle: Long): String
    @JvmStatic external fun presentationTimings(handle: Long, enabled: Boolean): String
    @JvmStatic external fun completionTimings(handle: Long, enabled: Boolean): String
    @JvmStatic external fun proofTask(handle: Long, id: Int, recipe: String, control: Long): Long
    @JvmStatic external fun proofWork(task: Long)
    @JvmStatic external fun proofCheck(handle: Long, task: Long)
    @JvmStatic external fun proofPreservation(task: Long): ByteArray?
    @JvmStatic external fun proofApply(handle: Long, task: Long, preserved: Boolean)
    @JvmStatic external fun proofFailed(handle: Long, task: Long, error: String)
    @JvmStatic external fun proofRelease(task: Long)
    @JvmStatic external fun captureCancelled(control: Long): Boolean
    @JvmStatic external fun captureCancel(control: Long)
    @JvmStatic external fun captureFree(control: Long)
    @JvmStatic external fun inspectionTask(handle: Long, control: Long): Long
    @JvmStatic external fun inspectionOutput(task: Long, recipe: String): Array<Any>
    @JvmStatic external fun inspectionHistogram(task: Long): String
    @JvmStatic external fun projectExportOptions(task: Long, recipe: String)
    @JvmStatic external fun projectExportTask(handle: Long, request: Int, now: Long, cancel: Long = 0): Long
    /** Pure shared number-field math; no native session handle or GPU work. */
    @JvmStatic external fun number(request: String): String
    @JvmStatic external fun toolbarUi(request: String): String
    @JvmStatic external fun automaticTabNames(request: String): String
    /** Pure shared color-wheel hit geometry, independent of the render thread. */
    @JvmStatic external fun colorWheelHit(request: String): String
    @JvmStatic external fun colorPanelLayout(size: Float): String
    @JvmStatic external fun colorHueStops(shape: String, space: String): String
    /** Shared sRGB field raster as Android ARGB pixels; no session access. */
    @JvmStatic external fun colorFieldMapped(size: Int, state: String, rendition: String): IntArray
    @JvmStatic external fun colorFieldPixels(size: Int, hue: Float, shape: String, space: String): IntArray
}
