package art.capycanvas

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Rect
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.ColorDrawable
import android.util.TypedValue
import android.os.Build
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.view.ViewTreeObserver
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.TextView
import android.window.SplashScreenView
import androidx.activity.compose.LocalActivity
import androidx.lifecycle.viewModelScope
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.PointerInputModifierNode
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.ceil

internal class LaunchReveal {
    var covered by mutableStateOf(false)
        private set
    var started by mutableStateOf(false)
        private set
    var visible = true
        private set
    var finished = false
        private set
    var closed = false
        private set
    private var paint by mutableStateOf<Any?>(null)
    private var submitted: Any? = null

    fun holding(visible: Boolean, finished: Boolean) = !closed && (visible || finished && covered && !started)

    fun update(visible: Boolean, finished: Boolean) {
        if (closed) return
        this.visible = visible
        this.finished = finished
        if (visible || !finished) {
            covered = visible
            started = false
            paint = null
            submitted = null
        } else if (covered && !started && paint == null) paint = Any()
    }
    fun native() {
        if (closed || !visible && !finished) return
        covered = true
        started = false
        paint = if (finished) Any() else null
        submitted = null
    }
    fun drawn(): (() -> Unit)? {
        val current = paint ?: return null
        if (closed || submitted === current) return null
        submitted = current
        return {
            if (!closed && paint === current && submitted === current) {
                paint = null
                submitted = null
                started = true
            }
        }
    }
    fun close() {
        closed = true
        covered = false
        started = false
        paint = null
        submitted = null
    }
}

internal class LaunchCaptionTextView(context: android.content.Context) : TextView(context) {
    private val ink = Rect()
    init { includeFontPadding = false; gravity = Gravity.CENTER_HORIZONTAL }
    override fun onDraw(canvas: Canvas) {
        val end = layout?.getLineEnd(0) ?: text.length
        paint.getTextBounds(text.toString(), 0, end, ink)
        val saved = canvas.save()
        canvas.translate(0f, -baseline.toFloat() - ink.top)
        super.onDraw(canvas)
        canvas.restoreToCount(saved)
    }
}

internal class LaunchSplashWindow(private val activity: MainActivity) {
    val shownAt = SystemClock.uptimeMillis()
    val reveal = LaunchReveal()
    var nativeActive by mutableStateOf(false)
        private set
    var nativeCaptured by mutableStateOf(false)
        private set
    var hadNativeVisual by mutableStateOf(false)
        private set
    var background by mutableStateOf(activity.getColor(R.color.canvas_launch_background))
        private set
    val foreground = activity.getColor(R.color.capy_splash_foreground)
    private val marks = activity.resources.getStringArray(R.array.capy_launch_icons).let { icons ->
        fun resources(array: Int) = activity.resources.obtainTypedArray(array).let { values ->
            IntArray(icons.size) { values.getResourceId(it, 0) }.also { values.recycle() }
        }
        val drawables = resources(R.array.capy_launch_marks)
        val themes = resources(R.array.capy_launch_splash_themes)
        icons.indices.associate { icons[it] to LaunchMarkResources(drawables[it], themes[it]) }
    }
    private var persisted: String? = null
    private var mark: String? = null
    private var view: View? = null
    private var caption: TextView? = null
    private var frameCommit: Pair<ViewTreeObserver, Runnable>? = null
    private var fading = false
    private var duration: Long? = null
    private var gap = 0f
    private var text: String? = null
    private var captionSize = 0f
    private var captionColor = foreground
    private var nativeInkBottom: Float? = null
    private var iconBoundsJob: Job? = null

    fun install() {
        if (Build.VERSION.SDK_INT >= 31) activity.splashScreen.setOnExitAnimationListener { splash ->
            if (reveal.closed) { splash.remove(); return@setOnExitAnimationListener }
            background = (splash.background as? ColorDrawable)?.color ?: background
            view = splash
            splash.post {
                if (view !== splash) return@post
                val icon = splash.iconView
                nativeActive = icon != null && icon.width > 0 && icon.height > 0
                nativeCaptured = true
                hadNativeVisual = nativeActive
                if (nativeActive) {
                    cancelFrame()
                    reveal.native()
                    measureIcon(splash)
                    caption = LaunchCaptionTextView(activity).apply {
                        alpha = 0f
                        splash.addView(this, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT,
                            FrameLayout.LayoutParams.WRAP_CONTENT))
                    }
                }
                refresh()
            }
        }
    }
    @androidx.annotation.RequiresApi(31)
    private fun measureIcon(splash: SplashScreenView) {
        val icon = splash.iconView as? ImageView ?: return
        val drawable = icon.drawable ?: return
        val bounds = Rect(drawable.bounds)
        val matrix = Matrix(icon.imageMatrix)
        val height = icon.height
        val padding = icon.paddingTop
        val source = (drawable as? BitmapDrawable)?.bitmap
        val state = if (source == null) drawable.constantState else null
        iconBoundsJob?.cancel()
        nativeInkBottom = null
        iconBoundsJob = activity.host.viewModelScope.launch {
            val bottom = withContext(Dispatchers.Default) {
                val bitmap = if (source != null) {
                    if (source.config == Bitmap.Config.HARDWARE) source.copy(Bitmap.Config.ARGB_8888, false) else source
                } else state?.newDrawable(activity.resources)?.mutate()?.let { copy ->
                    Bitmap.createBitmap(bounds.width(), bounds.height(), Bitmap.Config.ARGB_8888).also {
                        copy.setBounds(0, 0, it.width, it.height)
                        copy.draw(Canvas(it))
                    }
                }
                if (bitmap == null) return@withContext null
                try {
                    val row = IntArray(bitmap.width)
                    var y = bitmap.height - 1
                    while (y >= 0) {
                        bitmap.getPixels(row, 0, row.size, 0, y, row.size, 1)
                        if (row.any { it ushr 24 != 0 }) break
                        y--
                    }
                    val point = floatArrayOf(bounds.exactCenterX(), bounds.top + bounds.height() * (y + 1f) / bitmap.height)
                    matrix.mapPoints(point)
                    (padding + point[1]) / height
                } finally { if (bitmap !== source) bitmap.recycle() }
            }
            if (view === splash && !reveal.closed) { nativeInkBottom = bottom; refresh() }
        }
    }
    fun markDrawable(icon: String?) = marks[icon]?.drawable

    fun choose(icon: String) {
        if (Build.VERSION.SDK_INT < 31) return
        val theme = marks[icon]?.theme?.takeIf { icon != persisted } ?: return
        persisted = icon
        activity.splashScreen.setSplashScreenTheme(theme)
        activity.host.recordSystemSplash(icon)
    }

    fun update(visible: Boolean, finished: Boolean, mark: String?, text: String?, reveal: Long?, gap: Float, size: Float, color: Int) {
        if (this.reveal.closed) return
        this.reveal.update(visible, finished)
        if (visible || !finished) cancelFrame()
        this.mark = mark
        this.text = text
        this.duration = reveal
        this.gap = gap
        this.captionSize = size
        this.captionColor = color
        refresh()
    }
    fun readyDrawn(content: View) {
        val observer = content.viewTreeObserver
        if (!observer.isAlive) return
        val complete = reveal.drawn() ?: return
        lateinit var callback: Runnable
        callback = Runnable {
            content.post {
                if (frameCommit?.second === callback) {
                    frameCommit = null
                    complete()
                    refresh()
                }
            }
        }
        frameCommit = observer to callback
        observer.registerFrameCommitCallback(callback)
        content.postInvalidateOnAnimation()
    }
    private fun cancelFrame() {
        frameCommit?.let { (observer, callback) ->
            if (observer.isAlive) observer.unregisterFrameCommitCallback(callback)
        }
        frameCommit = null
    }
    private fun refresh() {
        if (Build.VERSION.SDK_INT < 31) return
        val splash = view as? SplashScreenView ?: return
        if (!nativeCaptured) return
        if (!nativeActive || mark != null && mark != activity.host.systemSplash) {
            nativeActive = false
            hadNativeVisual = false
            caption = null
            view = null
            val decor = activity.window.decorView
            decor.postOnAnimation {
                decor.viewTreeObserver.registerFrameCommitCallback { decor.post { remove(splash) } }
                decor.invalidate()
            }
            return
        }
        caption?.apply {
            val showing = this.text.isNotEmpty()
            val inkBottom = nativeInkBottom
            this.text = if (inkBottom == null) "" else this@LaunchSplashWindow.text.orEmpty()
            setTextColor(captionColor)
            setTextSize(TypedValue.COMPLEX_UNIT_PX, captionSize)
            val icon = splash.iconView
            if (icon != null && inkBottom != null) {
                val bounds = Rect(0, 0, icon.width, icon.height)
                splash.offsetDescendantRectToMyCoords(icon, bounds)
                measure(View.MeasureSpec.makeMeasureSpec(splash.width, View.MeasureSpec.AT_MOST),
                    View.MeasureSpec.makeMeasureSpec(splash.height, View.MeasureSpec.AT_MOST))
                x = bounds.exactCenterX() - measuredWidth / 2f
                y = ceil(bounds.top + bounds.height() * inkBottom) + ceil(gap)
            }
            if (this.text.isNotEmpty() && !showing) animate().alpha(1f).setDuration(180).start()
            else if (this.text.isEmpty()) { animate().cancel(); alpha = 0f }
        }
        if (!reveal.visible) {
            if (!reveal.finished) {
                splash.animate().cancel()
                view = null
                caption = null
                nativeActive = false
                hadNativeVisual = false
                fading = false
                remove(splash)
            } else if (reveal.started && !fading && duration != null) {
                fading = true
                splash.animate().alpha(0f).setDuration(duration!!).withEndAction {
                    if (view === splash) { view = null; nativeActive = false }
                    fading = false
                    remove(splash)
                }.start()
            }
        }
    }
    @androidx.annotation.RequiresApi(31)
    private fun remove(splash: SplashScreenView) {
        val measuring = iconBoundsJob?.takeIf { !it.isCompleted } ?: return splash.remove()
        measuring.invokeOnCompletion { splash.post { splash.remove() } }
    }
    fun close() {
        reveal.close()
        iconBoundsJob?.cancel()
        cancelFrame()
        if (Build.VERSION.SDK_INT >= 31) (view as? SplashScreenView)?.let { it.animate().cancel(); remove(it) }
        view = null
        caption = null
        nativeActive = false
    }
}

internal class LaunchMarkResources(val drawable: Int, val theme: Int)

internal fun CanvasHost.launchCovered(): Boolean = failure == null && !dialogOpen &&
    !(snapshot?.objectOrNull("state") == null && notice?.id == null && notice != null) && snapshot?.objectOrNull("launch")?.optBoolean("visible", true) != false

internal fun CanvasHost.launchFinished(): Boolean = snapshot?.objectOrNull("launch")?.optBoolean("finished") == true && failure == null

internal fun CanvasHost.launchHolding(reveal: LaunchReveal?): Boolean {
    val visible = launchCovered()
    return reveal?.holding(visible, launchFinished()) ?: visible
}

private data class LaunchCoverInput(val covered: Boolean) : ModifierNodeElement<LaunchCoverInputNode>() {
    override fun create() = LaunchCoverInputNode(covered)
    override fun update(node: LaunchCoverInputNode) { node.covered = covered }
}

private class LaunchCoverInputNode(var covered: Boolean) : Modifier.Node(), PointerInputModifierNode {
    override fun onPointerEvent(pointerEvent: PointerEvent, pass: PointerEventPass, bounds: IntSize) {
        if (covered) pointerEvent.changes.forEach { it.consume() }
    }
    override fun onCancelPointerInput() {}
    override fun sharePointerInputWithSiblings() = !covered
}

@Composable internal fun LaunchSplash(host: CanvasHost) {
    val activity = LocalActivity.current as? MainActivity ?: return
    val window = activity.launchSplash
    val visible = host.launchCovered()
    val finished = host.launchFinished()
    val bootstrap = host.bootstrap
    val mark = bootstrap?.objectOrNull("launch_mark")
    val markIcon = mark?.getString("icon")
    LaunchedEffect(window, markIcon) { markIcon?.let { host.homeScreenIcon.show(it); window.choose(it) } }
    val density = LocalDensity.current
    val reveal = bootstrap?.getLong("launch_reveal_ms")
    val gap = bootstrap?.getDouble("launch_caption_gap")?.toFloat() ?: 0f
    val captionDelay = bootstrap?.getLong("launch_caption_delay_ms")
    var captionVisible by remember { mutableStateOf(false) }
    LaunchedEffect(window.shownAt, captionDelay) {
        captionVisible = false
        if (captionDelay != null) {
            delay((window.shownAt + captionDelay - SystemClock.uptimeMillis()).coerceAtLeast(0))
            captionVisible = true
        }
    }
    val captionAlpha by animateFloatAsState(if (captionVisible) 1f else 0f,
        tween(if (Build.VERSION.SDK_INT >= 31) 180 else 0), label = "launch-caption")
    val caption = if (captionVisible) bootstrap?.optString("launch_caption") else null
    val size = bootstrap?.optDouble("launch_caption_size")?.toFloat() ?: 0f
    val role = bootstrap?.optString("launch_caption_color_role").orEmpty()
    val color = host.snapshot?.objectOrNull("state")?.objectOrNull("palette")?.optString(role)
        ?.takeIf { it.isNotEmpty() }?.let { android.graphics.Color.parseColor(it) } ?: window.foreground
    SideEffect { window.update(visible, finished, markIcon, caption, reveal, gap * density.density, size * density.density, color) }
    val holding = host.launchHolding(window.reveal)
    val alpha by animateFloatAsState(if (holding) 1f else 0f,
        tween(if (finished) reveal?.toInt() ?: 0 else 0), label = "launch-reveal")
    val native = window.nativeActive || (window.hadNativeVisual && finished)
    if (alpha > 0f || window.nativeActive) BoxWithConstraints(Modifier.fillMaxSize() then LaunchCoverInput(visible)) {
        if (!native && alpha > 0f) {
            val drawable = window.markDrawable(markIcon)
            val inkBottom = mark?.getDouble("ink_bottom")?.toFloat() ?: 0f
            Layout(modifier = Modifier.fillMaxSize().graphicsLayer { this.alpha = alpha }.background(Color(window.background))
                .testTag("launch-splash"), content = {
                if (drawable == null) Spacer(Modifier)
                else Image(painterResource(drawable), null, Modifier.size(Native.launchMarkSize(maxWidth.value, maxHeight.value).dp)
                    .testTag("launch-splash-mark"), colorFilter = ColorFilter.tint(Color(window.foreground)))
                if (!caption.isNullOrEmpty()) AndroidView(factory = { LaunchCaptionTextView(it) }, update = {
                    it.text = caption
                    it.setTextSize(TypedValue.COMPLEX_UNIT_PX, size * density.density)
                    it.setTextColor(color)
                    it.alpha = captionAlpha
                }, modifier = Modifier.testTag("launch-splash-caption"))
            }) { children, constraints ->
                val loose = constraints.copy(minWidth = 0, minHeight = 0)
                val image = children[0].measure(loose)
                val text = children.getOrNull(1)?.measure(loose)
                val viewport = IntSize(constraints.maxWidth, constraints.maxHeight)
                val at = Alignment.Center.align(IntSize(image.width, image.height), viewport, layoutDirection)
                val textTop = ceil(at.y + image.height * inkBottom).toInt() + ceil(gap * density.density).toInt()
                layout(viewport.width, viewport.height) {
                    image.place(at.x, at.y)
                    text?.place((viewport.width - text.width) / 2, textTop)
                }
            }
        }
    }
}
