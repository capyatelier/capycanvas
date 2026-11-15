package art.capycanvas

import android.graphics.Bitmap
import android.net.Uri
import android.view.WindowManager
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.Assume.assumeTrue
import java.io.File

class AndroidStoreShowcaseTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()
    private lateinit var host: CanvasHost
    private val exactCaptions by lazy { JSONObject(instrumentation.context.assets.open("expected-scope-exact.json").bufferedReader().use { it.readText() }) }
    private val executableHash by lazy {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        File(instrumentation.targetContext.applicationInfo.sourceDir).inputStream().use { stream ->
            val buffer = ByteArray(65536)
            while (true) { val count = stream.read(buffer); if (count < 0) break; digest.update(buffer, 0, count) }
        }
        digest.digest().joinToString("") { "%02x".format(it) }
    }
    private fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
    private fun state(): JSONObject {
        var value = JSONObject()
        instrumentation.runOnMainSync { value = JSONObject(host.snapshot!!.getJSONObject("state").toString()) }
        return value
    }
    private fun action(value: JSONObject) { host.drain(value); compose.waitForIdle() }
    private fun invoke(command: String) = action(obj("type" to "invoke", "command" to command))
    private fun ready() {
        compose.waitUntil(120_000) {
            native { val now = System.nanoTime(); Native.frame(it, now, now + 16_666_667) }
            assertNull(host.failure); assertNull(host.actionError)
            host.snapshot?.optBoolean("canvas_ready") == true && host.snapshot?.optBoolean("brush_ready") == true &&
                host.workspaceManager?.optBoolean("busy") == false && !native { Native.renderingPending(it) }
        }
        instrumentation.runOnMainSync { host.documentChanged() }
        compose.waitForIdle()
    }
    private fun presentationReady(theme: String) {
        host.awaitMain("visible native previews and exact scopes", 120_000, { host.snapshot.toString() }, compose) {
            fun visible(tag: String) = findTag(tag)?.second?.boundsInRoot?.let { it.width > 0 && it.height > 0 } == true
            val state = host.snapshot!!.getJSONObject("state")
            val layers = state.array("layers").objects().all { layer ->
                val id = layer.getLong("id")
                !visible("layer-row-$id") || (!layer.optBoolean("has_thumbnail") || visible("layer-thumbnail-$id-false")) &&
                    (!layer.optBoolean("has_mask") || visible("layer-thumbnail-$id-true"))
            }
            val panels = state.getJSONObject("tool_panels")
            val previews = (listOf(state.getJSONObject("tool_set")) + panels.keys().asSequence().mapNotNull { panels.optJSONObject(it) }.toList())
                .flatMap { it.array("subtools").objects() }.all { item ->
                    val id = item.optInt("preview", -1)
                    id < 0 || !visible("subtool-${item.getString("label")}") ||
                        host.bundledPreviews.get("$id-$theme.webp") != null && visible("brush-preview-$id")
                }
            val scopes = listOf("histogram", "waveform", "tonal_histogram").all { kind ->
                !visible("scope-$kind-chart") || state.optJSONObject(kind)?.let { it.optString("status") == exactCaptions.getString(host.languageTag) && !it.isNull("data") } == true && kind in host.scopes
            }
            layers && previews && scopes
        }
        compose.waitForIdle()
    }
    private fun visibleTexts(): List<String> {
        val text = mutableListOf<String>()
        fun walk(node: androidx.compose.ui.semantics.SemanticsNode) {
            if (node.boundsInRoot.width > 0 && node.boundsInRoot.height > 0) {
                node.config.getOrNull(SemanticsProperties.Text)?.forEach { text += it.text }
                node.config.getOrNull(SemanticsProperties.ContentDescription)?.let { text.addAll(it) }
            }
            node.children.forEach(::walk)
        }
        instrumentation.runOnMainSync { semanticsRoots().forEach { walk(it.semanticsOwner.unmergedRootSemanticsNode) } }
        return text.distinct()
    }
    private fun openPhoneDrawer(scene: String) {
        if (state().getJSONObject("customization").objectOrNull("drawer") != null) {
            action(obj("type" to "customize", "action" to obj("type" to "close_expanded")))
        }
        val tiles = state().getJSONObject("workspace").getJSONObject("layout").array("phone").objects()
        val layers = tiles.first { it.getJSONObject("control").optString("kind") == "panel" && it.getJSONObject("control").optString("panel") == "layers" }
        compose.onNodeWithTag("phone-tool-${layers.getInt("id")}", useUnmergedTree = true).performClick()
        if (scene == "photo") {
            host.awaitMain("native Layers disclosure", 30_000, { host.snapshot.toString() }, compose) { findTag("layer-properties-disclosure") != null }
            compose.onNodeWithTag("layer-properties-disclosure", useUnmergedTree = true).performClick()
            host.awaitMain("native Properties page is presented", 30_000, { host.snapshot.toString() }, compose) {
                val page = findTag("layer-properties-pages")?.second?.boundsInRoot
                val properties = findTag("layer-properties")?.second?.boundsInRoot
                page != null && properties != null && properties.height > 0 &&
                    kotlin.math.abs(properties.top - page.top - 8 * instrumentation.targetContext.resources.displayMetrics.density) <= 1f
            }
        }
        host.awaitMain("localized native phone drawer", 30_000, { host.snapshot.toString() }, compose) {
            host.snapshot!!.getJSONObject("state").getJSONObject("customization").objectOrNull("drawer") != null &&
                findTag("tool-drawer")?.second?.boundsInRoot?.height?.let { it > 0 } == true
        }
        compose.waitForIdle()
    }
    @Test fun captureLocalizedShowcase() {
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Pass -e storeRecipe store-recipe.json with staged showcase test assets", arguments.containsKey("storeRecipe"))
        val recipeBytes = instrumentation.context.assets.open(arguments.getString("storeRecipe")!!).use { it.readBytes() }
        val recipe = JSONObject(recipeBytes.toString(Charsets.UTF_8))
        val recipeHash = java.security.MessageDigest.getInstance("SHA-256").digest(recipeBytes).joinToString("") { "%02x".format(it) }
        val scene = arguments.getString("scene", "paint")!!
        val run = requireNotNull(arguments.getString("storeRun")) { "Pass a fresh storeRun UUID" }
        require(run.matches(Regex("[a-f0-9]{32}")))
        val selected = recipe.array("scenes").objects().single { it.getString("id") == scene }
        val tags = arguments.getString("tags", "en")!!.split(',')
        val themes = arguments.getString("themes", "light,dark")!!.split(',')
        val kind = arguments.getString("kind", "tablet")!!
        require(kind in listOf("tablet", "phone"))
        wakeDevice()
        startupTheme()
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            if (kind == "tablet") device.landscape(scenario) else device.portrait(scenario)
            val activity = scenario.activity()
            host = activity.host
            scenario.onActivity { it.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) }
            host.awaitReady(120_000, compose)
            val workspace = "builtin:workspace:" + selected.getString("workspace")
            instrumentation.runOnMainSync { host.workspaceInput(obj("type" to "switch", "id" to workspace)) }
            host.awaitMain("showcase workspace", 120_000, { host.workspaceManager.toString() }, compose) {
                host.workspaceManager?.optString("id") == workspace && host.workspaceManager?.optBoolean("busy") == false
            }
            val source = selected.getString("source")
            val input = File(device.root, source)
            instrumentation.context.assets.open(source).use { stream -> input.outputStream().use { stream.copyTo(it) } }
            val inputHash = java.security.MessageDigest.getInstance("SHA-256").digest(input.readBytes()).joinToString("") { "%02x".format(it) }
            assertEquals("Staged source artwork matches its recipe", selected.getString("sourceSha256"), inputHash)
            instrumentation.runOnMainSync { assertTrue(host.documents.openUris(listOf(Uri.fromFile(input)))) }
            host.awaitMain("showcase source import", 120_000, { host.snapshot.toString() }, compose) {
                host.snapshot?.optJSONObject("state")?.array("tabs")?.objects()?.any {
                    it.optBoolean("active") && it.optInt("width") > 0 && it.optString("title").startsWith(input.nameWithoutExtension)
                } == true && !host.documents.working
            }
            ready()
            invoke("fit_canvas")
            val authoredFilters = linkedMapOf<String, Long>()
            fun layer(name: String): Long = state().array("layers").objects().single { it.getString("label") == name }.getLong("id")
            for (step in selected.array("steps").objects()) {
                when (step.getString("type")) {
                    "action" -> action(step.getJSONObject("action"))
                    "show_panel" -> instrumentation.runOnMainSync { host.customize(obj("type" to "set_panel_visible", "panel" to step.getString("panel"), "visible" to true)) }
                    "select_layer" -> action(obj("type" to "layer", "action" to obj("op" to "select", "id" to layer(step.getString("name")), "mask" to false)))
                    "layer_visibility" -> action(obj("type" to "layer", "action" to obj("op" to "visibility", "id" to layer(step.getString("name")), "value" to step.getBoolean("visible"))))
                    "effect" -> {
                        val effect = step.getString("effect")
                        action(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to effect)))
                        ready()
                        val id = state().getJSONObject("layer_properties").getLong("layer")
                        authoredFilters[effect] = id
                        val values = step.getJSONObject("parameters")
                        for (key in values.keys()) action(obj("type" to "effect", "action" to obj("op" to "set", "layer" to id, "key" to key, "value" to values.get(key))))
                    }
                    "frame" -> {
                        if (kind == "tablet") action(obj("type" to "set_zoom", "zoom" to step.getDouble("zoom")))
                        val camera = state().getJSONObject("camera")
                        val area = camera.getJSONArray("work_area")
                        val x = area.getDouble(0) + area.getDouble(2) / 2
                        val y = area.getDouble(1) + area.getDouble(3) / 2
                        val tab = state().array("tabs").objects().single { it.optBoolean("active") }
                        val focus = step.getJSONArray("focus")
                        val dx = (tab.getDouble("width") / 2 - focus.getDouble(0)) * camera.getDouble("zoom")
                        val dy = (tab.getDouble("height") / 2 - focus.getDouble(1)) * camera.getDouble("zoom")
                        for ((phase, offset) in listOf("down" to 0, "move" to 1, "up" to 1)) {
                            native { Native.input(it, obj("type" to "pointer", "kind" to "mouse", "button" to "pan", "id" to 17,
                                "phase" to phase, "position" to JSONArray(listOf(x + offset * dx, y + offset * dy)), "time_ns" to System.nanoTime()).toString()) }
                        }
                        instrumentation.runOnMainSync { host.documentChanged() }
                    }
                    "wait_histogram" -> ready()
                    else -> error("Unsupported store recipe step: $step")
                }
            }
            ready()
            val shipped = host.bootstrap!!.array("shipped_tags").values().map { it as String }
            for (tag in tags) {
                val index = shipped.indexOf(tag)
                assertTrue("Shipped $tag", index >= 0)
                action(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index + 1)))
                host.awaitMain("localized $tag", 60_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
                for ((effect, id) in authoredFilters) {
                    val label = state().array("adjustments").objects().first { it.getString("id") == effect }.getString("label")
                    action(obj("type" to "layer", "action" to obj("op" to "rename", "id" to id, "name" to label)))
                    assertEquals(label, state().array("layers").objects().first { it.getLong("id") == id }.getString("label"))
                }
                for (theme in themes) {
                    action(obj("type" to "set_theme", "theme" to theme))
                    ready()
                    if (kind == "phone") openPhoneDrawer(scene)
                    presentationReady(theme)
                    assertEquals(tag, host.languageTag)
                    assertEquals(theme, state().getString("theme"))
                    val texts = visibleTexts()
                    assertTrue("Native UI text present", texts.isNotEmpty())
                    assertFalse("No replacement characters", texts.any { '\uFFFD' in it || '\u25A1' in it })
                    val paint = android.graphics.Paint()
                    val glyphs = texts.flatMap { it.codePoints().toArray().toList() }.distinct().filter { Character.isLetter(it) }
                    val missing = glyphs.filter { !paint.hasGlyph(String(Character.toChars(it))) }
                    assertEquals("Native fonts cover visible text", emptyList<Int>(), missing)
                    val output = File(activity.getExternalFilesDir(null), "validation/store/$run/$kind/$tag").apply { mkdirs() }
                    require(!File(output, "$scene-$theme.png").exists()) { "Use a fresh capture run" }
                    var model = JSONObject()
                    instrumentation.runOnMainSync { model = JSONObject(host.snapshot!!.toString()) }
                    val report = obj("device_kind" to kind, "scene" to scene, "theme" to theme, "language" to tag, "source" to source,
                        "workspace" to workspace, "recipe_sha256" to recipeHash, "executable_sha256" to executableHash, "canvas_ready" to model.getBoolean("canvas_ready"),
                        "brush_ready" to model.getBoolean("brush_ready"), "rendering_pending" to native { Native.renderingPending(it) },
                        "ui_text" to JSONArray(texts), "glyphs_checked" to glyphs.size,
                        "missing_glyphs" to JSONArray(missing), "display" to JSONObject(native { Native.displayStatus(it) }), "snapshot" to model)
                    val screenshot = checkNotNull(instrumentation.uiAutomation.takeScreenshot())
                    try {
                        File(output, "$scene-$theme.png").outputStream().use { assertTrue(screenshot.compress(Bitmap.CompressFormat.PNG, 100, it)) }
                        report.put("width", screenshot.width); report.put("height", screenshot.height)
                    } finally { screenshot.recycle() }
                    File(output, "$scene-$theme.json").writeText(report.toString(2))
                    println("CAPY_STORE_CAPTURE $kind $tag $scene $theme")
                }
            }
        }
    }
}
