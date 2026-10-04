package art.capycanvas

import android.app.Application
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.Process
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

internal data class CameraReadout(val zoomPercent: Int, val rotationDegrees: Int)

internal fun obj(vararg pairs: Pair<String, Any?>) = JSONObject().apply {
    pairs.forEach { (key, value) -> put(key, value ?: JSONObject.NULL) }
}
internal fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
internal fun JSONArray.values(): List<Any> = (0 until length()).map { get(it) }
internal fun JSONObject.array(key: String) = optJSONArray(key) ?: JSONArray()
internal fun JSONObject.objectOrNull(key: String) = optJSONObject(key)
internal fun JSONObject.number(key: String, default: Double = 0.0) = optDouble(key, default).toFloat()
internal fun JSONObject.copy() = JSONObject().also { copy -> keys().forEach { copy.put(it, opt(it)) } }

/** Platform ownership and transport, not application policy. The UI never waits
 * for a GPU submission. One dedicated Looper owns both Rust and the swapchain. */
class CanvasHost(application: Application) : AndroidViewModel(application) {
    internal val layerSwipe=LayerSwipe()
    internal val palettes=PaletteController(this)
    internal val proof=ProofController(this)
    internal val hdr=HdrController(this)
    companion object {
        /** Instrumentation can hold device creation while checking the real UI. */
        @Volatile internal var beforeGpuAttachForTest: (() -> Unit)? = null
        @Volatile internal var workspaceDirectoryForTest: String? = null
        internal val preferencesName get() = workspaceDirectoryForTest?.let { "capy-test-${it.hashCode()}" } ?: "capy-canvas"
    }
    internal val strokeRecording = StrokeRecording(this)
    internal var bootstrap by mutableStateOf<JSONObject?>(null)
        private set
    private var bootstrapForOwner: JSONObject? = null
    internal val languageTag get() = bootstrap?.getString("active_tag").orEmpty()
    private val languageWorker = java.util.concurrent.Executors.newSingleThreadExecutor()
    private var preferredLocales = emptyArray<String>()
    private fun refreshLanguage() {
        Native.languageRequest(handle, preferredLocales)?.let { serialized ->
            val request = JSONObject(serialized)
            languageWorker.execute {
                val started = System.nanoTime()
                val prepared = runCatching { Native.prepareLanguage(request.getString("language")) }
                if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) Log.i("CapyLanguage",
                    "language=${request.getString("language")} prepare_ms=${(System.nanoTime() - started) / 1e6}")
                if (!worker.post {
                    prepared.onSuccess { context ->
                        if (disposed || handle == 0L) Native.freeLanguage(context)
                        else {
                            Native.publishLanguage(handle, request.getLong("generation"), context, textComposition.active)
                            publish(true)
                        }
                    }.onFailure { Log.e("CapyCanvas", "Could not prepare UI language", it) }
                }) prepared.getOrNull()?.let { Native.freeLanguage(it) }
            }
        }
        Native.publishLanguage(handle, 0L, 0L, textComposition.active)
    }
    internal fun systemLocalesChanged(locales: android.os.LocaleList) {
        val tags = Array(locales.size()) { locales[it].toLanguageTag() }
        post { preferredLocales = tags; refreshLanguage(); publish(true) }
    }
    var snapshot by mutableStateOf<JSONObject?>(null)
        private set
    internal var scopes by mutableStateOf<Map<String, ScopePlot>>(emptyMap())
        private set
    private var scopePublication: Map<String, ScopePlot> = emptyMap()
    internal var workspaceManager by mutableStateOf<JSONObject?>(null)
        private set
    private var workspaceManagerKey: String? = null
    private val workspaceTick = object : Runnable {
        override fun run() {
            if (disposed || handle == 0L) return
            attempt(canvas = false) { refreshLanguage(); updateWorkspaceManager(obj("type" to "tick")); publish(false) }
            worker.postDelayed(this, 100)
        }
    }
    private fun updateWorkspaceManager(request: JSONObject) {
        val result = JSONObject(Native.workspace(handle, request.toString()).takeUnless { it == "null" } ?: "{}")
        if (result.optBoolean("refresh")) refreshChrome()
        if (result.optBoolean("wake")) wake()
        val text = result.objectOrNull("view")?.toString() ?: return
        if (text != workspaceManagerKey && text != "null") {
            workspaceManagerKey = text
            val view = JSONObject(text)
            main.post { workspaceManager = view }
        }
    }
    internal fun workspaceInput(request: JSONObject) = post {
        updateWorkspaceManager(request); refreshChrome(); publish(true); wake()
    }
    private var closingWorkspaceWindow = false
    // Main-thread window attachment survives the gap during configuration
    // recreation without retaining or finishing a retired Activity.
    private var attachedWindow = java.lang.ref.WeakReference<MainActivity>(null)
    private var finishWindowPending = false
    internal fun attachWindow(activity: MainActivity) {
        attachedWindow = java.lang.ref.WeakReference(activity)
        finishAttachedWindow()
    }
    internal fun detachWindow(activity: MainActivity) {
        if(attachedWindow.get() === activity) attachedWindow.clear()
    }
    private fun finishAttachedWindow() {
        val activity = attachedWindow.get() ?: return
        if(finishWindowPending && !activity.isChangingConfigurations && !activity.isDestroyed) {
            finishWindowPending = false
            activity.finish()
        }
    }
    internal fun closeWorkspaceWindow() = post {
        if (closingWorkspaceWindow) return@post
        closingWorkspaceWindow = true
        updateWorkspaceManager(obj("type" to "suspend"))
        val check = object : Runnable {
            override fun run() {
                if (disposed || handle == 0L) return
                attempt(canvas = false) {
                    updateWorkspaceManager(obj("type" to "tick"))
                    val view = workspaceManagerKey?.let(::JSONObject)
                    if (view?.optBoolean("busy") == true) { worker.postDelayed(this, 20); return@attempt }
                    closingWorkspaceWindow = false
                    if (view == null || (view.isNull("error") && !view.optBoolean("dirty"))) main.post {
                        finishWindowPending = true
                        finishAttachedWindow()
                    }
                }
            }
        }
        worker.post(check)
    }
    internal var colorPreview by mutableStateOf<JSONObject?>(null)
        private set
    internal var keymapFile by mutableStateOf<JSONObject?>(null)
    private var modelSnapshot: JSONObject? = null // Native owner only.
    var surfaceReady by mutableStateOf(false)
        private set
    internal var cameraReadout by mutableStateOf(CameraReadout(100, 0))
        private set
    /** Exact camera zoom for the readout's field; only an open zoom menu reads it. */
    internal var cameraZoom by mutableFloatStateOf(1f)
        private set
    internal var cameraRotation by mutableFloatStateOf(0f)
        private set
    internal var cameraLocks by mutableStateOf(false to false)
        private set
    internal val canvasBar: JSONObject? get() = snapshot?.objectOrNull("state")?.objectOrNull("canvas_bar")
    internal var canvasBarVisible by mutableStateOf(true)
        private set
    private var canvasBarHold = 0 // Native owner only.
    private val canvasBarReturn = Runnable { canvasBarVisible = true }
    internal fun holdCanvasBar(hold: Int) {
        main.removeCallbacks(canvasBarReturn)
        canvasBarVisible = false
        if (hold % 2 == 0) main.postDelayed(canvasBarReturn, catalog.optLong("canvas_bar_reappear_ms"))
    }
    internal suspend fun glassPresented() = kotlinx.coroutines.suspendCancellableCoroutine<Unit> { continuation ->
        main.post { worker.post { worker.post { main.post { if (continuation.isActive) continuation.resumeWith(Result.success(Unit)) } } } }
    }
    private fun syncCanvasBar() {
        val hold = Native.canvasBarHold(handle)
        if (hold == canvasBarHold) return
        canvasBarHold = hold
        main.post { holdCanvasBar(hold) }
    }
    internal var surfaceOrigin = androidx.compose.ui.geometry.Offset.Zero
    private val overviewSlots = linkedMapOf<Any, JSONObject>() // UI thread; native owner receives immutable JSON.
    internal fun navigatorPlacement(key: Any, placement: JSONObject?) {
        if (overviewSlots[key]?.toString() == placement?.toString()) return
        if (placement == null) overviewSlots.remove(key) else overviewSlots[key] = placement
        val payload = JSONArray(overviewSlots.values.toList()).toString()
        post { Native.navigatorPlacements(handle, payload); wake() }
    }
    private val glassBoxes = linkedMapOf<Any, FloatArray>()
    internal val glassBoxesForTest get() = glassBoxes.values.toList()
    private val glassConnections = linkedMapOf<Any, JSONObject>()
    private var glassQueued = false
    internal fun glassBox(key: Any, box: FloatArray?) {
        if (glassBoxes[key]?.contentEquals(box) == true || box == null && key !in glassBoxes) return
        if (box == null) glassBoxes.remove(key) else glassBoxes[key] = box
        queueGlass()
    }
    internal fun glassConnection(key: Any, connection: JSONObject?, scale: Float) {
        val entry = connection?.let { obj("connection" to it, "scale" to scale) }
        if (glassConnections[key]?.toString() == entry?.toString()) return
        if (entry == null) glassConnections.remove(key) else glassConnections[key] = entry
        queueGlass()
    }
    private fun queueGlass() {
        if (glassQueued) return
        glassQueued = true
        main.post {
            glassQueued = false
            val payload = obj("boxes" to JSONArray(glassBoxes.values.map { JSONArray(it.toList()) }),
                "connections" to JSONArray(glassConnections.values.toList())).toString()
            post { Native.glassRegions(handle, payload); wake() }
        }
    }
    var catalog by mutableStateOf(JSONObject())
        private set
    internal var commandSearch by mutableStateOf<JSONObject?>(null)
        private set
    var failure by mutableStateOf<String?>(null)
        private set
    /** Host file and platform errors, shown in a dialog until acknowledged. */
    internal var dialogError by mutableStateOf<String?>(null)
        private set
    /** The core's file and renderer error, shown once for each value it publishes. */
    internal var hostError by mutableStateOf<String?>(null)
        private set
    private var publishedHostError: String? = null
    internal var notice by mutableStateOf<CanvasNotice?>(null)
        private set
    private var publishedNotice: Long? = null
    private val hideNotice = Runnable { notice = null }
    /** The failure the user is shown: a dialog, or a refused command's notice. */
    val actionError: String? get() = dialogError ?: notice?.takeIf { it.id == null }?.text
    // Native focus, not application state; prevents typing from invoking tools.
    var editingText = false
    internal val textComposition = TextComposition()
    internal var toolbarEditorBounds: androidx.compose.ui.geometry.Rect? = null
    internal var headerKeyHandler: ((android.view.KeyEvent) -> Boolean)? = null
    private var platformPredictionAvailable: Boolean? = null
    internal val nativePredictionEnabled: Boolean
        get() = platformPredictionAvailable == true && (snapshot?.objectOrNull("state")?.objectOrNull("settings")?.let {
            it.optBoolean("feedback", true) && it.optBoolean("platform_prediction", true)
        } ?: true)
    internal fun updatePredictionAvailability(available: Boolean) {
        if (platformPredictionAvailable == available) return
        platformPredictionAvailable = available
        post { Native.predictionAvailability(handle, available); publish(true) }
    }
    private val main = Handler(Looper.getMainLooper())
    private val thread = HandlerThread("capy-canvas", Process.THREAD_PRIORITY_DISPLAY).apply { start() }
    private val worker = Handler(thread.looper)
    @Volatile internal var documentInputBlocked = false
    internal fun documentCanvasFailure(message: String?) { failure=message }
    internal val drawingTabs = DrawingTabsController(this)
    internal val documents = DocumentController(this, application)
    internal val recovery = RecoveryController(this, application)
    private val saved = application.getSharedPreferences(preferencesName, 0)
    private var handle = 0L
    internal val filterPreviewCache = FilterPreviewCache(this)
    internal val brushPreviews = object : android.util.LruCache<String, androidx.compose.ui.graphics.ImageBitmap>(4 * 1024 * 1024) {
        override fun sizeOf(key: String, value: androidx.compose.ui.graphics.ImageBitmap) = value.width * value.height * 4
    }
    private var choreographer: Choreographer? = null
    private var attached = false
    private var currentSurface: Surface? = null
    private var awaitingSurfaceFrame = false
    private var surfaceGeneration = 0
    @Volatile private var firstUiDraw = 0L
    @Volatile private var firstSurfaceReady = 0L
    private var disposed = false
    private var frameInterval = 8_333_333L
    private var snapshotAt = 0L
    private var lastCanvasReady = false
    private var startupCacheFinished = false
    private var lastStartupStage = -1
    private val startupTimes = LongArray(4)
    private var documentEpoch = 0L
    private val measuredFrames = if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) LongArray(8192 * 18) else null
    private val measuredInputs = if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) LongArray(8192 * 7) else null
    private val frameCosts = if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) LongArray(11) else null
    private var frameCount = 0
    private var inputCount = 0
    private val measuredPublications = if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) LongArray(8192 * 4) else null
    private var publicationCount = 0
    private var panelContentChanges = 0L
    private var snapshotAttempts = 0L
    private var snapshotsPublished = 0L
    private var workspaceUpdatesPublished = 0L
    internal var workspaceGeometry by mutableStateOf<WorkspaceGeometry?>(null)
        private set
    internal var lastWorkspaceGroup: Pair<Int, androidx.compose.ui.geometry.Rect>? = null
        private set
    /** Focused native color buttons own Space/Enter instead of canvas shortcuts. */
    internal var colorControlFocus: Any? = null
    /** Focused interval handles own their native adjustment keys. */
    internal var rangeControlFocus: Any? = null
    internal var curveControlFocus: Any? = null
    internal var pickerPopupOpen = false
    internal var restartingWindow = false
    private var workspaceContentRevision = -1L
    private var workspaceModelRevision = -1L // Main thread: model required by the geometry.
    private var lastWorkspaceUpdate: WorkspaceGeometry? = null // Native owner only.
    internal fun beginWorkspaceGesture() { lastWorkspaceGroup = null }
    private var cameraUpdatesPublished = 0L
    // Buffers have one owner: input callback -> render task -> this bounded pool.
    // A backlog may allocate extra buffers, but no input is dropped or overwritten.
    private val pointerBuffers = ArrayBlockingQueue<DoubleArray>(8)
    @Volatile private var pointerAllocations = 0L
    private val suppressedContacts = mutableSetOf<Long>()

    init {
        worker.post {
            attempt {
                val savedSettings = runCatching { saved.getString("settings", null).orEmpty() }.getOrElse {
                    Log.e("CapyCanvas", "Could not read saved settings", it); ""
                }
                val locales = application.resources.configuration.locales.let { languages ->
                    Array(languages.size()) { languages[it].toLanguageTag() }
                }
                preferredLocales = locales
                val launch = JSONObject(Native.bootstrap(savedSettings, locales))
                bootstrapForOwner = launch
                main.post { bootstrap = launch }
                handle = Native.create(savedSettings, locales, BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK)
                choreographer = Choreographer.getInstance()
                attempt(canvas = false) {
                    val directory = workspaceDirectoryForTest ?: java.io.File(application.filesDir, "workspaces").absolutePath
                    updateWorkspaceManager(obj("type" to "start", "directory" to directory))
                    worker.post(workspaceTick)
                }
                val value = JSONObject(Native.query(handle, obj("type" to "catalog").toString()))
                main.post { catalog = value }
                publish(true)
            }
        }
    }
    private fun attempt(canvas: Boolean = true, block: () -> Unit) {
        try { block() } catch (e: Exception) {
            Log.e("CapyCanvas", "Native canvas operation failed", e)
            main.post {
                if (canvas) failure = bootstrapForOwner?.getString("canvas_init_failed")
                else notice = CanvasNotice(null, bootstrapForOwner?.getString("action_failed").orEmpty(), null)
            }
        }
    }
    private fun post(canvas: Boolean = false, block: () -> Unit) {
        worker.post { if (!disposed && handle != 0L) attempt(canvas, block) }
    }
    // Coalesce window UI traffic without serializing another input/model batch.
    private val shaderInputPending = java.util.concurrent.atomic.AtomicBoolean()
    internal fun shaderInput() {
        if (shaderInputPending.compareAndSet(false, true)) worker.post {
            shaderInputPending.set(false)
            if (!disposed && handle != 0L) Native.shaderInput(handle)
        }
    }
    internal suspend fun <T> withNative(block: (Long) -> T): T = kotlin.coroutines.suspendCoroutine { continuation ->
        if (!worker.post {
            val result = runCatching { check(!disposed && handle != 0L) { bootstrapForOwner?.getString("editor_closed").orEmpty() }; block(handle) }
            main.post { continuation.resumeWith(result) }
        }) continuation.resumeWith(Result.failure(IllegalStateException(bootstrapForOwner?.getString("editor_closed"))))
    }
    internal fun documentChanged(complete: () -> Unit = {}) = post {
        refreshChrome(); publish(true); wake(); main.post { drawingTabs.refresh(); complete() }
    }
    internal fun reportActionError(message: String) { dialogError = message }
    fun clearActionError() { dialogError = null }
    internal fun dismissHostError() { hostError = null }
    /** Accept runs the core's action; declining, or the timeout, dismisses it. */
    internal fun answerNotice(shown: CanvasNotice, accept: Boolean) {
        if (notice !== shown) return
        notice = null
        val id = shown.id ?: return
        val answer = obj("type" to "notice", "id" to id, "accept" to accept)
        if (accept) dispatch(answer)
        else post { runCatching { Native.dispatch(handle, answer.toString()) }; publish(true) }
    }
    private fun publishNotice(published: JSONObject?) {
        val id = published?.getLong("id")
        if (id == publishedNotice) return
        publishedNotice = id
        notice = published?.let { CanvasNotice(id, it.getString("text"), it.optJSONObject("action")?.getString("label")) }
            ?: notice?.takeIf { it.id == null }
    }
    private fun publishHostError(error: String?) {
        if (error == publishedHostError) return
        publishedHostError = error
        hostError = error
    }
    internal fun commandFocus() = if (editingText) "text" else if (palettes.focus != null) "palette" else "canvas"
    fun dispatch(action: JSONObject) {
        // Capture the editor owner before a menu action opens a native dialog.
        val focus = if (action.optString("type") == "invoke" && action.optString("command") == "search_commands") commandFocus() else null
        post {
            val type=action.optString("type")
            if(documentInputBlocked && !type.startsWith("measure_") && type !in listOf("complete_request", "system_theme_changed", "window_fullscreen")) return@post
            val tracing = android.os.Trace.isEnabled()
            if (tracing) android.os.Trace.beginSection("capy.action." + type + "." + action.optString("command"))
            try {
                if (focus != null) Native.dispatch(handle, obj("type" to "command_search", "action" to obj("type" to "focus", "focus" to focus)).toString())
                Native.dispatch(handle, action.toString())
            }
            finally { if (tracing) android.os.Trace.endSection() }
            refreshLanguage()
            refreshChrome()
            publish(true)
            wake()
        }
    }
    private data class WorkspacePresentation(val preview: JSONObject?, val reply: (Any?) -> Unit)
    private var workspacePresentation: WorkspacePresentation? = null // Native owner only.
    private val workspaceFrame = Choreographer.FrameCallback {
        val presentation = workspacePresentation
        workspacePresentation = null
        if (presentation != null && !disposed) attempt(canvas = false) { presentWorkspace(presentation) }
    }
    private fun presentWorkspace(presentation: WorkspacePresentation) {
        refreshChrome()
        publish(true)
        val value = lastWorkspaceUpdate?.hint
        if (presentation.preview != null) main.post { presentation.reply(value) }
        wake()
    }
    internal fun workspaceGesture(actions: List<JSONObject>, preview: JSONObject? = null, moving: Boolean = false, reply: (Any?) -> Unit = {}) = post {
        // Preserve every movement for tear-off/cancellation/history. Present the
        // latest result once per display frame; incoming moves do not postpone it.
        actions.forEach { Native.dispatch(handle, it.toString()) }
        val presentation = WorkspacePresentation(preview, reply)
        if (moving) {
            if (workspacePresentation == null) choreographer?.postFrameCallback(workspaceFrame)
            workspacePresentation = presentation
        } else {
            choreographer?.removeFrameCallback(workspaceFrame)
            workspacePresentation = null
            presentWorkspace(presentation)
        }
    }
    fun invoke(command: String) = dispatch(obj("type" to "invoke", "command" to command))
    fun customize(action: JSONObject) = dispatch(obj("type" to "customize", "action" to action))
    fun preference(action: JSONObject) = dispatch(obj("type" to "preferences", "action" to action))
    fun query(query: JSONObject, reply: (Any?) -> Unit) = post {
        val tracing = android.os.Trace.isEnabled()
        if (tracing) android.os.Trace.beginSection("capy.query." + query.optString("type"))
        try {
            val serialized = Native.query(handle, query.toString())
            if (tracing) android.os.Trace.beginSection("capy.query.parse")
            val value = try { org.json.JSONTokener(serialized).nextValue() }
                finally { if (tracing) android.os.Trace.endSection() }
            main.post { reply(if (value == JSONObject.NULL) null else value) }
        } finally { if (tracing) android.os.Trace.endSection() }
    }
    internal fun revealPanel(panel: String) = post {
        Native.query(handle, obj("type" to "reveal_panel", "panel" to panel).toString())
        refreshChrome(); publish(true); wake()
    }
    internal fun paletteAction(action: JSONObject, dryRun: Boolean = false, reply: (String?) -> Unit = {}) = post {
        val result = JSONObject(Native.query(handle, obj("type" to "palette_action", "action" to action, "dry_run" to dryRun).toString()))
        if (!dryRun) { refreshChrome(); publish(true); wake() }
        main.post { reply(result.optString("error").takeUnless { result.isNull("error") }) }
    }
    /** Validate and apply on the same native owner turn. A delayed main-thread
     * reply must never apply an old drop after Done, cancellation or a switch. */
    internal fun headerAction(request: JSONObject) = post {
        val value = org.json.JSONTokener(Native.query(handle, obj("type" to "header", "request" to request).toString())).nextValue()
        if (value is JSONObject) {
            Native.dispatch(handle, value.toString())
            refreshChrome(); publish(true); wake()
        }
    }
    internal fun filterPreviews(query: JSONObject, reply: (FilterPreviewReply?) -> Unit) = post {
        try {
            val status = JSONObject(Native.query(handle, query.toString()))
            val atlas = Native.takeFilterPreviews(handle)
            val response = FilterPreviewReply(status, atlas?.let { JSONArray(it[0] as String) }, atlas?.get(1) as? ByteArray)
            main.post { reply(response) }
        } catch (e: Exception) {
            Log.w("CapyCanvas", "Filter previews unavailable", e)
            main.post { reply(null) }
        }
    }
    fun input(input: JSONObject, reply: ((JSONObject) -> Unit)? = null) = post {
        if(documentInputBlocked && input.optString("type") in listOf("key_down", "scroll")) return@post
        val value = JSONObject(Native.input(handle, input.toString()))
        if (reply != null) main.post { reply(value) }
        // Discrete key actions must publish immediately even when the previous
        // focus event was inside the camera/pointer publication interval.
        publish(input.optString("type") in listOf("key", "pen_button") && (value.optJSONObject("change")?.optInt("regions", 0) ?: 0) != 0)
        wake()
    }
    // These are presentation facts, supplied by native widgets. Rust owns Zen
    // visibility and the rules for dismissing/pinning expanded panels.
    private var chromeFacts = obj("held" to false, "dragging" to false, "popup_open" to false)
    private var logicalWidth = 1f
    private var logicalHeight = 1f
    private var surfaceDensity = 1f
    fun chrome(event: JSONObject, facts: JSONObject? = null, reply: ((JSONObject) -> Unit)? = null) = post {
        if (facts != null) chromeFacts = facts
        val result = JSONObject(Native.input(handle, chromeInput(event).toString()))
        if (reply != null) main.post { reply(result) }
        publish(true)
    }
    private fun chromeInput(event: JSONObject) = obj("type" to "chrome", "event" to event,
        "facts" to chromeFacts, "viewport" to JSONArray(listOf(logicalWidth, logicalHeight)))
    private fun refreshChrome() { Native.input(handle, chromeInput(obj("kind" to "refresh")).toString()) }

    internal fun displayInfo(available:Boolean) = post {
        Native.displayInfo(handle,available);publish(true);wake()
    }
    internal fun screenInfo(name:String,wide:Boolean,panelWide:Boolean,hdr:Boolean,peak:Float) = post {
        Native.screenInfo(handle,name,wide,panelWide,hdr,peak);publish(true);wake()
    }

    fun attach(surface: Surface, width: Int, height: Int, density: Float, refreshRate: Float) {
        currentSurface=surface
        if(documentInputBlocked) {
            main.postDelayed({if(currentSurface===surface&&surface.isValid)attach(surface,width,height,density,refreshRate)},16)
            return
        }
        proof.resume()
        hdr.resume()
        filterPreviewCache.resume()
        currentSurface = surface
        surfaceReady = false
        val generation = ++surfaceGeneration
        post(canvas = true) {
            activeSurfaceGeneration = generation
            logicalWidth = width / density; logicalHeight = height / density; surfaceDensity = density
            frameInterval = (1_000_000_000.0 / refreshRate.coerceAtLeast(30f)).toLong()
            Native.resize(handle, width, height, density)
            touchPolicy()
            // Sizing computes the toolbars/panels without a GPU. Publish their layout
            // before device creation or even the first compositing shader can block.
            publish(true)
            if (BuildConfig.DEBUG) beforeGpuAttachForTest?.invoke()
            Log.i("CapyStartup", "gpu_attach boot_ns=${SystemClock.elapsedRealtimeNanos()}")
            Native.attach(handle, surface, java.io.File(getApplication<Application>().cacheDir, "shader-pipelines").absolutePath)
            attached = true
            awaitingSurfaceFrame = true
            main.post { failure = null }
            publish(true)
            wake()
        }
    }
    fun restartCanvas() {
        val surface = currentSurface ?: return
        surfaceReady = false
        post(canvas = true) {
            Native.resetGpu(handle)
            check(surface.isValid) { "The canvas surface is unavailable" }
            Native.attach(handle, surface, java.io.File(getApplication<Application>().cacheDir, "shader-pipelines").absolutePath)
            attached = true; awaitingSurfaceFrame = true; startupCacheFinished = false
            main.post { failure = null }
            publish(true); wake()
        }
    }
    private var activeSurfaceGeneration = 0 // Render Looper only.
    private fun touchPolicy() {
        val configuration = android.view.ViewConfiguration.get(getApplication<Application>())
        Native.touchPolicy(handle, android.view.ViewConfiguration.getLongPressTimeout(), configuration.scaledTouchSlop.toFloat())
    }
    fun resize(width: Int, height: Int, density: Float) = post {
        logicalWidth = width / density; logicalHeight = height / density; surfaceDensity = density
        Native.resize(handle, width, height, density)
        touchPolicy()
        publish(true)
        wake()
    }
    /** SurfaceHolder requires rendering to have stopped before this callback
     * returns. This wait is only at surface teardown, never in an input/frame. */
    fun detach() {
        proof.pause()
        hdr.pause()
        filterPreviewCache.pause()
        currentSurface = null
        surfaceReady = false
        ++surfaceGeneration
        val stopped = CountDownLatch(1)
        if (!worker.post {
            try {
                worker.removeCallbacks(renderFrame)
                renderQueued = false
                renderDelayed = false
                if (handle != 0L) { attached = false; Native.detach(handle) }
            }
            finally { stopped.countDown() }
        }) return // The owning thread has already destroyed the native session.
        check(stopped.await(10, TimeUnit.SECONDS)) { "Canvas surface did not detach" }
    }
    fun pointerBuffer(size: Int): DoubleArray {
        val buffer = pointerBuffers.poll()
        if (buffer != null && buffer.size >= size) return buffer
        if (BuildConfig.DEBUG) pointerAllocations++
        return DoubleArray(maxOf(16, Integer.highestOneBit(size - 1) shl 1))
    }
    fun pointer(id: Long, tool: Int, button: Int, samples: DoubleArray, count: Int, predicted: Boolean = false, barrelTwist: Boolean = false) {
        val arrival = System.nanoTime()
        val accepted = worker.post {
            try {
                if (disposed || handle == 0L) return@post
                attempt {
                    val started = System.nanoTime()
                    android.os.Trace.setCounter("Capy input queue ns", started - arrival)
                    android.os.Trace.setCounter("Capy input age ns", started - samples[count - 2].toLong())
                    android.os.Trace.setCounter("Capy input samples", (count / 9).toLong())
                    val phase = samples[count - 1].toInt()
                    val refining = phase == 1 && measuredInputs != null &&
                        JSONObject(Native.displayStatus(handle)).optBoolean("pending_composition")
                    if (phase == 1 && !predicted) main.post(hideNotice)
                    if (phase == 1 && !predicted && documentInputBlocked) suppressedContacts.add(id)
                    if (phase == 1 && !predicted && !documentInputBlocked) {
                        val event = obj("kind" to "contact", "canvas" to true,
                            "position" to JSONArray(listOf(samples[0] / surfaceDensity, samples[1] / surfaceDensity)))
                        val reply = JSONObject(Native.input(handle, chromeInput(event).toString()))
                        if (reply.optBoolean("handled")) suppressedContacts.add(id)
                    }
                    if (id !in suppressedContacts) Native.pointer(handle, id, tool, button, samples, count, predicted, barrelTwist)
                    if (!predicted) syncCanvasBar()
                    if (phase == 3 || phase == 4) suppressedContacts.remove(id)
                    if (!predicted && measuredInputs != null && inputCount < 8192) {
                        val offset = inputCount++ * 7
                        measuredInputs[offset] = samples[count - 2].toLong()
                        measuredInputs[offset + 1] = arrival
                        measuredInputs[offset + 2] = started
                        measuredInputs[offset + 3] = System.nanoTime() - started
                        measuredInputs[offset + 4] = (count / 9).toLong()
                        measuredInputs[offset + 5] = phase.toLong()
                        measuredInputs[offset + 6] = if (refining) 1 else 0
                    }
                    wake()
                }
            } finally { pointerBuffers.offer(samples) }
        }
        if (!accepted) pointerBuffers.offer(samples)
    }
    fun scroll(x: Float, y: Float, dx: Float, dy: Float, zoom: Boolean, horizontal: Boolean) = post {
        Native.scroll(handle, x, y, dx * 40, dy * 40, zoom, horizontal)
        wake()
    }
    private var renderQueued = false
    private var renderDelayed = false
    private var renderGeneration = 0
    private val renderFrame = Runnable {
        renderQueued = false
        renderDelayed = false
        if (attached && !disposed && renderGeneration == activeSurfaceGeneration) {
            val now = System.nanoTime()
            draw(now, now + frameInterval)
        }
    }
    private fun wake(retryDelay: Long = 0L) {
        if (!attached) return
        if (renderQueued) {
            // New input needn't wait for a previously scheduled GPU/startup retry.
            if (retryDelay == 0L && renderDelayed) {
                worker.removeCallbacks(renderFrame)
                renderDelayed = false
                worker.post(renderFrame)
            }
            return
        }
        renderQueued = true
        renderDelayed = retryDelay > 0L
        renderGeneration = activeSurfaceGeneration
        if (renderDelayed) worker.postDelayed(renderFrame, retryDelay) else worker.post(renderFrame)
    }
    private fun draw(frameTime: Long, expectedPresentation: Long) {
        if (!attached || disposed) return
        attempt {
            android.os.Trace.beginSection("capy.callback")
            try {
                val start = System.nanoTime()
                val threadStart = if (measuredFrames != null) android.os.Debug.threadCpuTimeNanos() else 0L
                val again = Native.frame(handle, start, expectedPresentation.coerceAtLeast(start))
                if (awaitingSurfaceFrame && Native.surfaceReady(handle)) {
                    awaitingSurfaceFrame = false
                    val generation = activeSurfaceGeneration
                    main.post {
                        if (generation == surfaceGeneration) {
                            surfaceReady = true
                            if (firstSurfaceReady == 0L) {
                                firstSurfaceReady = SystemClock.elapsedRealtimeNanos()
                                Log.i("CapyStartup", "surface_ready boot_ns=$firstSurfaceReady")
                            }
                        }
                    }
                }
                val elapsed = System.nanoTime() - start
                if (frameCosts != null) Native.frameCost(handle, frameCosts)
                val publicationStart = if (measuredFrames != null) System.nanoTime() else 0L
                // Shader warmup needs polling, not hundreds of empty presents/s.
                if (again || awaitingSurfaceFrame) wake(if (lastStartupStage < 2) (frameInterval / 1_000_000).coerceAtLeast(1L) else 1L)
                publish(!again)
                if (!startupCacheFinished && lastCanvasReady) {
                    Native.finishStartupCache(handle)
                    startupCacheFinished = true
                    wake()
                }
                if (measuredFrames != null && frameCount < 8192) {
                    val end = System.nanoTime()
                    val offset = frameCount++ * 18
                    measuredFrames[offset] = frameTime
                    measuredFrames[offset + 1] = start
                    measuredFrames[offset + 2] = elapsed
                    measuredFrames[offset + 3] = expectedPresentation
                    frameCosts!!.copyInto(measuredFrames, offset + 4, 0, 5)
                    frameCosts.copyInto(measuredFrames, offset + 11, 5, 11)
                    measuredFrames[offset + 9] = end - publicationStart
                    measuredFrames[offset + 10] = end - start
                    measuredFrames[offset + 17] = android.os.Debug.threadCpuTimeNanos() - threadStart
                }
            } finally { android.os.Trace.endSection() }
        }
    }
    /** Debug-build measurement only. CPU submission is deliberately not labelled
     * GPU completion or on-screen presentation; collect compositor data separately. */
    fun measurements(reset: Boolean = false, reply: (JSONObject) -> Unit) = post {
        fun rows(data: LongArray?, count: Int, width: Int) = JSONArray().apply {
            if (data != null) repeat(count) { row -> put(JSONArray().apply { repeat(width) { col -> put(data[row * width + col]) } }) }
        }
        val report = obj("startup_boot_ns" to JSONArray(startupTimes.toList()),
            "ui_first_draw_boot_ns" to firstUiDraw, "surface_ready_boot_ns" to firstSurfaceReady,
            "frames" to rows(measuredFrames, frameCount, 18),
            "inputs" to rows(measuredInputs, inputCount, 7),
            "snapshot_attempts" to snapshotAttempts, "snapshots_published" to snapshotsPublished,
            "camera_updates_published" to cameraUpdatesPublished,
            "workspace_updates_published" to workspaceUpdatesPublished,
            "publications" to rows(measuredPublications, publicationCount, 4),
            "publication_fields" to JSONArray(listOf("native_ns", "parse_ns", "prepare_ns", "utf16_units")),
            "panel_content_changes" to panelContentChanges,
            "pointer_allocations" to pointerAllocations,
            "frame_fields" to JSONArray(listOf("vsync_ns", "start_ns", "cpu_render_present_ns", "expected_presentation_ns", "paint_ns", "acquire_ns", "viewport_ns", "queue_present_ns", "poll_ns", "publish_schedule_ns", "cpu_callback_ns", "prepare_ns", "committed_paint_ns", "capture_ns", "prediction_ns", "composition_ns", "submission_ns", "owner_thread_cpu_ns")),
            "input_fields" to JSONArray(listOf("event_ns", "arrival_ns", "worker_start_ns", "cpu_input_ns", "sample_count", "phase", "pending_composition")))
        if (reset) { publicationCount = 0; panelContentChanges = 0; frameCount = 0; inputCount = 0; snapshotAttempts = 0; snapshotsPublished = 0; cameraUpdatesPublished = 0; workspaceUpdatesPublished = 0 }
        main.post { reply(report) }
    }
    internal fun recordUiDraw() {
        if (firstUiDraw == 0L) {
            firstUiDraw = SystemClock.elapsedRealtimeNanos()
            Log.i("CapyStartup", "ui_first_draw boot_ns=$firstUiDraw")
        }
    }
    private fun publish(force: Boolean) {
        syncCanvasBar()
        val now = SystemClock.uptimeMillis()
        if (!force && now - snapshotAt < 33) return
        snapshotAt = now
        if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) snapshotAttempts++
        val publicationStart = if (measuredPublications != null) System.nanoTime() else 0L
        val tracing = android.os.Trace.isEnabled()
        if (tracing) android.os.Trace.beginSection("capy.publish.native")
        val serialized = try { Native.modelUpdate(handle) } finally { if (tracing) android.os.Trace.endSection() } ?: return
        val nativeEnd = if (measuredPublications != null) System.nanoTime() else 0L
        val previousModel = modelSnapshot
        if (tracing) android.os.Trace.beginSection("capy.publish.parse")
        val next = try {
            val packet = JSONObject(serialized)
            if (!packet.has("model_update")) {
                if (packet.has("state") && previousModel != null) shareModel(previousModel, packet) as JSONObject else packet
            } else applyModelUpdate(checkNotNull(previousModel), packet)
        } finally { if (tracing) android.os.Trace.endSection() }
        if (next.has("state")) modelSnapshot = next
        val parsedEnd = if (measuredPublications != null) System.nanoTime() else 0L
        fun recordPublication() {
            if (measuredPublications != null && publicationCount < 8192) {
                val offset = publicationCount++ * 4
                measuredPublications[offset] = nativeEnd - publicationStart
                measuredPublications[offset + 1] = parsedEnd - nativeEnd
                measuredPublications[offset + 2] = System.nanoTime() - parsedEnd
                measuredPublications[offset + 3] = serialized.length.toLong()
            }
        }
        if (!next.has("state") && next.has("command_search")) {
            recordPublication()
            main.post {
                commandSearch = next.objectOrNull("command_search")
                snapshot?.getJSONObject("state")?.apply {
                    put("command_search", next.get("command_search")); put("revision", next.getLong("revision"))
                }
            }
            return
        }
        val geometry = next.objectOrNull("workspace_update")?.let(WorkspaceGeometry::read)
        if (geometry != null) lastWorkspaceUpdate = geometry
        if (!next.has("state") && next.has("layout") && geometry != null) {
            workspaceUpdatesPublished++
            val contentRevision = next.getJSONObject("workspace_update").getLong("content_revision")
            recordPublication()
            main.post {
                val model = snapshot ?: return@post
                if (contentRevision != workspaceContentRevision || geometry.revision < (workspaceGeometry?.revision ?: -1L)) return@post
                model.put("layout", next.getJSONObject("layout"))
                model.put("panel_measurements", next.getJSONArray("panel_measurements"))
                model.put("workspace_update", next.getJSONObject("workspace_update"))
                val camera = next.getJSONObject("camera")
                model.getJSONObject("state").apply {
                    put("camera", camera); put("revision", geometry.revision)
                    val workspace = getJSONObject("workspace").copy()
                    val layout = workspace.getJSONObject("layout").copy()
                    val patch = next.getJSONObject("workspace_layout")
                    patch.keys().forEach { layout.put(it, patch.get(it)) }
                    put("workspace", workspace.put("layout", layout))
                }
                workspaceModelRevision = geometry.modelRevision
                applyWorkspaceGeometry(geometry)
                updateCameraReadout(camera)
            }
            return
        }
        if (!next.has("state") && geometry != null) {
            workspaceUpdatesPublished++
            recordPublication()
            main.post {
                next.objectOrNull("color_preview")?.let { colorPreview = it }
                applyWorkspaceGeometry(geometry)
                next.objectOrNull("camera")?.let { camera ->
                    snapshot?.getJSONObject("state")?.put("camera", camera)
                    updateCameraReadout(camera)
                }
            }
            return
        }
        next.objectOrNull("camera")?.let { camera ->
            if (BuildConfig.DEBUG) cameraUpdatesPublished++
            recordPublication()
            main.post {
                snapshot?.getJSONObject("state")?.apply {
                    put("camera", camera)
                    put("revision", next.getLong("revision"))
                }
                updateCameraReadout(camera)
            }
            return
        }
        if (BuildConfig.DEBUG || BuildConfig.WORKSPACE_BENCHMARK) snapshotsPublished++
        lastCanvasReady = next.optBoolean("canvas_ready")
        val stage = when { next.optBoolean("shaders_ready") -> 3; next.optBoolean("brush_ready") -> 2; lastCanvasReady -> 1; next.optBoolean("gpu_ready") -> 0; else -> -1 }
        if (stage > lastStartupStage) {
            val now = SystemClock.elapsedRealtimeNanos()
            for (index in (lastStartupStage + 1)..stage) startupTimes[index] = now
            lastStartupStage = stage
            Log.i("CapyStartup", "stage=$stage boot_ns=$now")
        }
        val localization = next.objectOrNull("bootstrap")
        if (localization != null) bootstrapForOwner = localization
        val state = next.getJSONObject("state")
        val plots = Native.takeScopes(handle)?.let { scopePlots(it, scopePublication).also { scopePublication = it } }
        val epoch = state.getJSONObject("document_file").optLong("epoch")
        if (epoch != documentEpoch) {
            documentEpoch = epoch
            main.post { filterPreviewCache.reset() }
        }
        // Legacy preferences remain a migration backup. Named workspaces are
        // saved asynchronously by Rust's shared SQLite worker.
        state.array("requests").objects().forEach { request ->
            val kind = request.getJSONObject("kind")
            when (kind.getString("type")) {
                "save_settings" -> {
                    saved.edit().putString("settings", kind.getJSONObject("settings").toString()).apply()
                    Native.dispatch(handle, obj("type" to "complete_request", "id" to request.getLong("id"), "error" to null).toString())
                }
                "export_keymap", "import_keymap" -> {
                    main.post { keymapFile = kind }
                    Native.dispatch(handle, obj("type" to "complete_request", "id" to request.getLong("id"), "error" to null).toString())
                }
            }
        }
        if (measuredPublications != null && contentChanged(previousModel, next)) panelContentChanges++
        recordPublication()
        main.post { androidx.compose.runtime.snapshots.Snapshot.withMutableSnapshot {
            plots?.let { scopes = it }
            localization?.let { bootstrap = it }
            next.objectOrNull("catalog")?.let { catalog = it }
            colorPreview = next.objectOrNull("color_preview")
            commandSearch = state.objectOrNull("command_search")
            (snapshot as? ObservedModel ?: ObservedModel(listOf("state", "state.document_file", "state.layer_tools", "state.brush",
                "state.tool_set", "state.tool_panels.brush_sets", "state.tool_panels.sculpt_sets", "state.tool_panels.tools")).also { snapshot = it }).assign(next)
            publishNotice(state.optJSONObject("notice"))
            publishHostError(state.optString("host_error").takeUnless { state.isNull("host_error") })
            drawingTabs.refresh()
            workspaceContentRevision = next.objectOrNull("workspace_update")?.optLong("content_revision", -1L) ?: -1L
            workspaceModelRevision = geometry?.modelRevision ?: -1L
            if (geometry != null) applyWorkspaceGeometry(geometry) else workspaceGeometry = null
            updateCameraReadout(state.getJSONObject("camera"))
        } }
    }
    private fun applyWorkspaceGeometry(next: WorkspaceGeometry) {
        if (next.modelRevision != workspaceModelRevision || next.revision < (workspaceGeometry?.revision ?: -1L)) return
        workspaceGeometry = next
        if (next.group != null && next.bounds != null) lastWorkspaceGroup = next.group to next.bounds
    }
    private fun contentChanged(previous: JSONObject?, next: JSONObject): Boolean {
        val before = previous?.optJSONObject("state") ?: return true
        val after = next.getJSONObject("state")
        return listOf("panels", "color_panel", "palette_panel").any { previous.opt(it) !== next.opt(it) } ||
            (before.keys().asSequence() + after.keys().asSequence()).distinct()
                .filter { it !in listOf("workspace", "revision", "settings_open", "preferences", "command_search", "canvas_bar") }
                .any { before.opt(it) !== after.opt(it) }
    }
    private fun applyModelUpdate(previous: JSONObject, packet: JSONObject): JSONObject {
        val result = previous.copy()
        val copied = java.util.Collections.newSetFromMap(java.util.IdentityHashMap<Any, Boolean>()).apply { add(result) }
        fun get(container: Any, segment: String): Any? =
            if (container is JSONArray) container.opt(segment.toInt()) else (container as JSONObject).opt(segment)
        fun set(container: Any, segment: String, value: Any?) {
            if (container is JSONArray) container.put(segment.toInt(), value) else (container as JSONObject).put(segment, value)
        }
        fun parent(path: JSONArray): Any {
            var target: Any = result
            for (i in 0 until path.length() - 1) {
                val segment = path.getString(i)
                val child = checkNotNull(get(target, segment))
                target = if (child in copied) child else when (child) {
                    is JSONArray -> JSONArray().also { copy -> for (index in 0 until child.length()) copy.put(child.opt(index)) }
                    else -> (child as JSONObject).copy()
                }.also { copy -> copied.add(copy); set(target, segment, copy) }
            }
            return target
        }
        val changes = packet.getJSONArray("model_update")
        for (i in 0 until changes.length()) {
            val change = changes.getJSONArray(i); val path = change.getJSONArray(0)
            val target = parent(path)
            val name = path.getString(path.length() - 1)
            set(target, name, shareModel(get(target, name), change.get(1)))
        }
        return result
    }
    private fun updateCameraReadout(camera: JSONObject) {
        cameraZoom = camera.number("zoom", 1.0)
        cameraRotation = camera.number("rotation")
        cameraLocks = camera.optBoolean("zoom_locked") to camera.optBoolean("rotation_locked")
        cameraReadout = CameraReadout((cameraZoom * 100).roundToInt(),
            (camera.number("rotation") * 180 / Math.PI).roundToInt())
    }
    override fun onCleared() {
        proof.pause()
        hdr.pause()
        documents.images.cancel()
        recovery.close()
        worker.post {
            disposed = true
            languageWorker.shutdown()
            worker.removeCallbacks(workspaceTick)
            attached = false
            if (handle != 0L) attempt(canvas = false) { updateWorkspaceManager(obj("type" to "close")) }
            // Continue polling storage replies on their exclusive native owner
            // after Activity teardown. Never block the UI or discard an accepted
            // close just because its SQLite reply has not arrived yet.
            val drain = object : Runnable {
                override fun run() {
                    var busy = false
                    if (handle != 0L) runCatching {
                        val result = JSONObject(Native.workspace(handle, obj("type" to "tick").toString()))
                        busy = result.objectOrNull("view")?.optBoolean("busy") == true
                    }
                    if (busy) { worker.postDelayed(this, 20); return }
                    disposed = true
                    if (handle != 0L) { Native.destroy(handle); handle = 0 }
                    thread.quitSafely()
                }
            }
            worker.post(drain)
        }
    }
}
