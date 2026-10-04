package art.capycanvas

import android.view.Surface

/** Only CanvasHost's render Looper can access a native session handle. */
internal object Native {
    @JvmStatic external fun nativeCaption(request: String, language: String = ""): String
    @JvmStatic external fun documentAppearance(options: String, language: String = ""): String
    @JvmStatic external fun shaderInput(handle: Long)
    init { System.loadLibrary("layer_android") }
    @JvmStatic external fun bootstrap(saved: String, locales: Array<String>): String
    @JvmStatic external fun storage(config: String, data: String, state: String, cache: String, temp: String): String
    @JvmStatic external fun languageRequest(handle: Long, locales: Array<String>): String?
    @JvmStatic external fun prepareLanguage(language: String): Long
    @JvmStatic external fun publishLanguage(handle: Long, generation: Long, context: Long, busy: Boolean)
    @JvmStatic external fun freeLanguage(context: Long)
    @JvmStatic external fun create(saved: String, locales: Array<String>, profiling: Boolean): Long
    @JvmStatic external fun destroy(handle: Long)
    @JvmStatic external fun attach(handle: Long, surface: Surface, cacheDirectory: String)
    @JvmStatic external fun displayStatus(handle: Long): String
    @JvmStatic external fun renderingPending(handle: Long): Boolean
    @JvmStatic external fun rendererMemory(handle: Long): String
    @JvmStatic external fun displayInfo(handle: Long, available: Boolean)
    @JvmStatic external fun screenInfo(handle: Long, name: String, wide: Boolean, panelWide: Boolean, hdr: Boolean, peak: Float)
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
    @JvmStatic external fun touchPolicy(handle: Long, tapMs: Int, slop: Float)
    @JvmStatic external fun pointer(handle: Long, id: Long, tool: Int, button: Int, records: DoubleArray, count: Int, predicted: Boolean, barrelTwist: Boolean)
    @JvmStatic external fun canvasBarHold(handle: Long): Int
    @JvmStatic external fun frame(handle: Long, now: Long, presentation: Long): Boolean
    /** First buffer on the current surface has completed GPU work. */
    @JvmStatic external fun surfaceReady(handle: Long): Boolean
    @JvmStatic external fun frameCost(handle: Long, output: LongArray)
    @JvmStatic external fun snapshot(handle: Long): String?
    @JvmStatic external fun modelUpdate(handle: Long): String?
    @JvmStatic external fun strokeRecordingData(handle: Long): ByteArray
    @JvmStatic external fun query(handle: Long, query: String): String
    /** Stateless shared color forms/previews; safe without a session handle. */
    @JvmStatic external fun colorUi(request: String, language: String = ""): String
    @JvmStatic external fun workspace(handle: Long, request: String): String
    @JvmStatic external fun navigatorPlacements(handle: Long, placements: String)
    @JvmStatic external fun glassRegions(handle: Long, glass: String)
    @JvmStatic external fun takeFilterPreviews(handle: Long): Array<Any>?
    @JvmStatic external fun takeScopes(handle: Long): Array<Any>?
    @JvmStatic external fun documentTabs(handle: Long, request: String): String
    @JvmStatic external fun documentSwitch(handle: Long, id: Long, close: Boolean): Long
    @JvmStatic external fun documentResumeWork(task: Long)
    @JvmStatic external fun documentResume(handle: Long, task: Long)
    @JvmStatic external fun documentResumeFree(task: Long)
    @JvmStatic external fun documentSpillTask(handle: Long): Long
    @JvmStatic external fun documentSpillWork(task: Long)
    @JvmStatic external fun projectParkReady(handle: Long, task: Long): Boolean
    /** File worker only: atomic publication of a captured recovery snapshot. */
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
    @JvmStatic external fun clipTask(handle: Long, request: Int): Long
    @JvmStatic external fun clipTaskLarge(task: Long): Boolean
    @JvmStatic external fun documentRequestTitle(handle: Long, id: Int): String?
    @JvmStatic external fun clipRun(task: Long, control: Long, nonce: String): Long
    @JvmStatic external fun clipTaskFree(task: Long)
    @JvmStatic external fun clipWritePng(clip: Long, path: String)
    @JvmStatic external fun clipAdopt(handle: Long, request: Int, clip: Long)
    @JvmStatic external fun clipFree(clip: Long)
    @JvmStatic external fun clipNonce(handle: Long): String?
    @JvmStatic external fun pasteClip(handle: Long, request: Int)
    /** File worker only; consumes the detached descriptor, retains the task. */
    @JvmStatic external fun exportPresets(bytes: ByteArray, request: String, color: String): Array<Any?>
    @JvmStatic external fun sessionStamp(handle: Long, id: Long): String
    @JvmStatic external fun sessionCapture(handle: Long, id: Long): Long
    @JvmStatic external fun sessionSettle(handle: Long, now: Long)
    @JvmStatic external fun sessionStoreOpen(directory: String): Long
    @JvmStatic external fun sessionStoreFree(store: Long)
    @JvmStatic external fun sessionStoreRetire(store: Long)
    @JvmStatic external fun sessionStorePrepareRetirement(store: Long)
    @JvmStatic external fun sessionPrepareRetirement(directory: String)
    @JvmStatic external fun sessionCommit(task: Long, store: Long)
    @JvmStatic external fun sessionRestoreTask(handle: Long, stamp: String = "null"): Long
    @JvmStatic external fun sessionReserveIdentities(handle: Long, identities: String)
    @JvmStatic external fun sessionRequireEmptyRetry(handle: Long)
    @JvmStatic external fun sessionFailure(handle: Long, detail: String, recovery: Boolean): String
    @JvmStatic external fun sessionRead(task: Long, store: Long, recovered: Boolean): String
    @JvmStatic external fun sessionObserve(task: Long, fd: Int): String
    @JvmStatic external fun sessionPrepare(task: Long, id: Long, observed: String)
    @JvmStatic external fun sessionAdopt(handle: Long, task: Long, active: Long, reserved: String): String
    @JvmStatic external fun sessionHydrate(handle: Long, task: Long, id: Long): Long
    @JvmStatic external fun documentPrepareClose(handle: Long): Long
    @JvmStatic external fun documentCommitClose(handle: Long, job: Long): Long
    @JvmStatic external fun documentCancelPreparedClose(handle: Long, job: Long)
    @JvmStatic external fun sessionFree(task: Long)
    @JvmStatic external fun sessionManifestRead(path: String): String
    @JvmStatic external fun sessionManifestWrite(path: String, value: String): String
    @JvmStatic external fun sessionManifestUpdate(state: String, event: String): String
    @JvmStatic external fun sessionCollect(directory: String, keys: String)
    @JvmStatic external fun sessionClose(handle: Long)
    @JvmStatic external fun sessionFingerprint(fd: Int): String
    @JvmStatic external fun sessionDestination(handle: Long): String
    @JvmStatic external fun sessionDestinationMatches(expectation: String, observed: String): Boolean
    @JvmStatic external fun sessionRecordDestination(handle: Long, location: String, fingerprint: String)
    @JvmStatic external fun sessionCompleteSave(handle: Long, request: Int, location: String, fingerprint: String)
    @JvmStatic external fun profileLibrary(request: String, bytes: ByteArray): String
    @JvmStatic external fun paletteFile(request: String, bytes: ByteArray): Array<Any>
    @JvmStatic external fun projectPackagePrompt(task: Long): String
    @JvmStatic external fun projectPackagePreview(task: Long): ByteArray
    @JvmStatic external fun projectPackageWrite(task: Long, fd: Int, preview: Boolean)
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
    @JvmStatic external fun presentationTimings(handle: Long, enabled: Boolean): String
    @JvmStatic external fun completionTimings(handle: Long, enabled: Boolean): String
    @JvmStatic external fun proofTask(handle: Long, id: Int, recipe: String, control: Long): Long
    @JvmStatic external fun proofWork(task: Long)
    @JvmStatic external fun proofCheck(handle: Long, task: Long)
    @JvmStatic external fun proofPreservation(task: Long): ByteArray?
    @JvmStatic external fun proofApply(handle: Long, task: Long, preserved: Boolean)
    @JvmStatic external fun proofFailed(handle: Long, task: Long, error: String)
    @JvmStatic external fun proofFailedReason(handle: Long, task: Long, reason: String)
    @JvmStatic external fun proofRelease(task: Long)
    @JvmStatic external fun captureCancel(control: Long)
    @JvmStatic external fun captureFree(control: Long)
    @JvmStatic external fun inspectionTask(handle: Long, control: Long): Long
    @JvmStatic external fun inspectionSample(task: Long, source: String, x: Float, y: Float, width: Int): String
    @JvmStatic external fun inspectionStatistics(task: Long, source: String, preview: Boolean, selection: Boolean, waveform: Boolean = false): String
    @JvmStatic external fun inspectionLevelsStatistics(task: Long, source: String): String
    @JvmStatic external fun inspectionOutput(task: Long, recipe: String): Array<Any>
    @JvmStatic external fun projectExportOptions(task: Long, recipe: String)
    @JvmStatic external fun projectExportTask(handle: Long, request: Int, now: Long, cancel: Long = 0): Long
    /** Pure shared number-field math; no native session handle or GPU work. */
    @JvmStatic external fun number(request: String, language: String = ""): String
    @JvmStatic external fun numericLabels(label: String, language: String = ""): String
    @JvmStatic external fun toolbarUi(request: String, language: String = ""): String
    @JvmStatic external fun automaticTabNames(request: String): String
    /** Pure shared color-wheel hit geometry, independent of the render thread. */
    @JvmStatic external fun colorWheelHit(request: String): String
    @JvmStatic external fun colorHueStops(shape: String, space: String): String
    /** Shared sRGB field raster as Android ARGB pixels; no session access. */
    @JvmStatic external fun colorFieldMapped(size: Int, state: String, rendition: String): IntArray
    @JvmStatic external fun colorFieldPixels(size: Int, hue: Float, shape: String, space: String): IntArray
}
