package art.capycanvas

import android.os.Bundle
import android.os.SystemClock
import java.io.File

fun awaitBenchmarkRefreshPolicy(args: Bundle, output: File, instrumentation: android.app.Instrumentation, host: CanvasHost, view: android.view.View) {
    if (args.getString("waitForGo") != "true") return
    output.resolve("go").delete()
    val token = checkNotNull(args.getString("benchmarkReadyToken"))
    @android.annotation.SuppressLint("BlockedPrivateApi")
    val destroy = android.app.UiAutomation::class.java.getDeclaredMethod("destroy")
    destroy.invoke(instrumentation.uiAutomation)
    output.resolve("ready").writeText(org.json.JSONObject().put("token", token).put("package", instrumentation.targetContext.packageName).put("pid", android.os.Process.myPid()).toString())
    val deadline = SystemClock.uptimeMillis() + 180_000
    while (!output.resolve("go").exists()) {
        check(SystemClock.uptimeMillis() < deadline) { "Timed out waiting for benchmark refresh-policy setup" }
        SystemClock.sleep(50)
    }
    checkNotNull(instrumentation.getUiAutomation(0))
    var refresh = 0f
    var ready = false
    while (!ready) {
        instrumentation.runOnMainSync {
            val hz = view.display?.refreshRate ?: 0f
            refresh = hz
            ready = host.snapshot?.let { it.optBoolean("canvas_ready") && it.optBoolean("shaders_ready") } == true &&
                (args.getString("expectedRefreshHz")?.toFloat()?.let { kotlin.math.abs(hz - it) < 1f } ?: true)
        }
        check(SystemClock.uptimeMillis() < deadline) { "Canvas or expected refresh rate unavailable after policy setup" }
        if (!ready) SystemClock.sleep(50)
    }
    output.resolve("refresh-before-primer.json").writeText(org.json.JSONObject().put("display_hz", refresh).put("token", token).toString())
}
