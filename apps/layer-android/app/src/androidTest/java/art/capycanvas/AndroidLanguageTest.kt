package art.capycanvas

import android.content.res.Configuration
import android.net.Uri
import android.os.LocaleList
import java.io.File
import android.view.KeyEvent
import android.view.InputDevice
import android.view.MotionEvent
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject

class AndroidLanguageTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    private fun tags(host: CanvasHost) = host.bootstrap!!.array("shipped_tags").values().map { it as String }
    private fun index(host: CanvasHost, tag: String) = tags(host).indexOf(tag).also { check(it >= 0) } + 1
    private fun switch(host: CanvasHost, tag: String) {
        host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, tag))))
        host.awaitMain("language $tag", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
    }

    private fun capture(surface: String, theme: String, tag: String) {
        compose.waitForIdle()
        val file = File(instrumentation.targetContext.getExternalFilesDir(null), "validation/resolution-$surface-$theme-$tag.png")
        file.parentFile!!.mkdirs()
        instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
            file.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
        }
    }

    @Test fun preferencesRelabelInPlaceAndPaintingHistorySurvivesInBothThemes() {
        launchCapy(compose = compose).use { scenario ->
            val layout = InstrumentationRegistry.getArguments().getString("languageLayout", "standard")!!
            if (layout == "narrow-large") {
                device.portrait(scenario)
                scenario.onActivity { activity ->
                    val configuration = Configuration(activity.resources.configuration).apply { fontScale = 1.3f }
                    @Suppress("DEPRECATION")
                    activity.resources.updateConfiguration(configuration, activity.resources.displayMetrics)
                    activity.window.decorView.dispatchConfigurationChanged(configuration)
                    activity.onConfigurationChanged(configuration)
                }
                compose.waitForIdle()
            }
            val activity = scenario.activity()
            val host = activity.host
            val surface = activity.window.decorView.descendant<CanvasSurfaceView>()!!
            val owner = runBlocking { host.withNative { it } }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun file() = state().getJSONObject("document_file").toString()
            fun select(index: Int, tag: String, theme: String) {
                if (layout == "narrow-large") {
                    compose.onNodeWithTag("settings-category-appearance").performScrollTo().performClick()
                    compose.waitUntil(10_000) { compose.onAllNodesWithTag("setting-choice-language").fetchSemanticsNodes().isNotEmpty() }
                }
                compose.onNodeWithTag("setting-choice-language").performScrollTo().assertIsDisplayed().performClick()
                val started = android.os.SystemClock.elapsedRealtimeNanos()
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("setting-choice-option-language-$index").fetchSemanticsNodes().isNotEmpty() }
                compose.onNodeWithTag("setting-choice-option-language-$index").performScrollTo().performClick()
                host.awaitMain("language $tag", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
                compose.waitForIdle()
                val visible = android.os.SystemClock.elapsedRealtimeNanos()
                compose.runOnIdle {
                    assertSame(activity, scenario.activity())
                    assertSame(host, activity.host)
                    assertSame(surface, activity.window.decorView.descendant<CanvasSurfaceView>())
                    assertTrue(state().getBoolean("settings_open"))
                }
                assertEquals(owner, runBlocking { host.withNative { it } })
                val futureTag = runBlocking { host.withNative {
                    val next = Native.create("{\"language\":{\"Explicit\":\"en\"}}", arrayOf("en-US"), false)
                    try { JSONObject(Native.modelUpdate(next)!!).getJSONObject("bootstrap").getString("active_tag") }
                    finally { Native.destroy(next) }
                } }
                assertEquals(if (index == 0) "en" else tag, futureTag)
                compose.onNodeWithTag("settings-done").assertTextContains(host.bootstrap!!.getJSONObject("common").getString("done"))
                compose.waitForIdle()
                android.os.SystemClock.sleep(200)
                val capture = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/resolution-language-$layout-$theme-$tag-$index.png")
                capture.parentFile!!.mkdirs()
                instrumentation.uiAutomation.takeScreenshot().let { bitmap ->
                    capture.outputStream().use { bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG, 100, it) }
                    bitmap.recycle()
                }
                android.util.Log.i("CapyLanguage", "theme=$theme language=$tag selection_to_composed_ms=${(visible - started) / 1_000_000} selection_to_capture_ms=${(android.os.SystemClock.elapsedRealtimeNanos() - started) / 1_000_000}")
            }
            fun paint() {
                val before = state().getJSONObject("document_file").getLong("revision")
                val down = android.os.SystemClock.uptimeMillis()
                for ((phase, dx) in listOf(MotionEvent.ACTION_DOWN to -40f, MotionEvent.ACTION_MOVE to 0f, MotionEvent.ACTION_UP to 40f)) {
                    compose.runOnIdle {
                        val properties = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_STYLUS })
                        val coordinates = arrayOf(MotionEvent.PointerCoords().apply { x = surface.width / 2f + dx; y = surface.height / 2f; pressure = .7f; size = .01f })
                        MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), phase, 1, properties, coordinates,
                            0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_STYLUS, 0).let { event -> surface.dispatchTouchEvent(event); event.recycle() }
                    }
                }
                host.awaitMain("painting after relabel", 30_000, { state().getJSONObject("document_file").toString() }, compose) {
                    state().getJSONObject("document_file").getLong("revision") > before
                }
                host.drain(obj("type" to "invoke", "command" to "undo"))
                host.drain(obj("type" to "invoke", "command" to "redo"))
            }
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    val systemTag = if ("fr" in tags(host)) "fr" else "ja"
                    compose.runOnIdle { host.systemLocalesChanged(LocaleList.forLanguageTags(if (systemTag == "fr") "fr-CA" else "ja-JP")) }
                    for ((choice, tag) in tags(host).mapIndexed { i, tag -> i + 1 to tag } + listOf(index(host, "en") to "en", 0 to systemTag)) {
                        host.drain(obj("type" to "invoke", "command" to "select_all"))
                        host.drain(obj("type" to "invoke", "command" to "move"))
                        assertTrue(state().getJSONObject("layer_tools").getBoolean("has_selection"))
                        val anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor").toString()
                        val unchanged = file()
                        host.drain(obj("type" to "open_settings", "page" to "appearance"))
                        select(choice, tag, theme)
                        assertEquals(unchanged, file())
                        assertTrue(state().getJSONObject("layer_tools").getBoolean("has_selection"))
                        assertEquals(anchor, state().getJSONObject("canvas_bar").getJSONArray("anchor").toString())
                        compose.onNodeWithTag("settings-done").performClick()
                        host.awaitMain("preferences close", 10_000, { "" }, compose) { !state().getBoolean("settings_open") }
                        host.drain(obj("type" to "invoke", "command" to "deselect"))
                        host.drain(obj("type" to "invoke", "command" to "brush"))
                        paint()
                    }
                }
            } finally {
                compose.runOnIdle { host.systemLocalesChanged(LocaleList.forLanguageTags("en-US")) }
                switch(host, "en")
            }
        }
    }

    @Test fun openLayerAndSelectionMenuPagesRelabelWithoutReplacingTheirOwner() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val activity = scenario.activity()
            val surface = activity.window.decorView.descendant<CanvasSurfaceView>()!!
            val owner = runBlocking { host.withNative { it } }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun children(menu: JSONObject) = menu.array("sections").values().flatMap { (it as org.json.JSONArray).objects() }
            fun descendants(menu: JSONObject): List<JSONObject> = children(menu).flatMap { listOf(it) + descendants(it) }
            fun coverage(item: JSONObject, mask: Boolean): Boolean {
                val action = item.objectOrNull("action") ?: return false
                val selection = action.objectOrNull("action") ?: return false
                return action.optString("type") == "selection" && selection.optString("op") == "load_coverage" && selection.optBoolean("mask") == mask
            }
            fun source(menu: JSONObject, mask: Boolean) = children(menu).first { descendants(it).any { item -> coverage(item, mask) } }
            fun coveragePage(menu: JSONObject, mask: Boolean) = descendants(menu).first { children(it).any { item -> coverage(item, mask) } }
            fun current(request: JSONObject) = runBlocking { host.awaitQuery(request) }!!
            fun text(label: String) = compose.onNode(hasText(label) and hasAnyAncestor(hasTestTag("workspace-menu")))
            fun menuShown() = compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isNotEmpty() }
            fun closeMenu() {
                pressKey(KeyEvent.KEYCODE_BACK)
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isEmpty() }
            }
            try {
                host.newDocument(320, 240)
                val layer = state().array("layers").objects().first { it.getBoolean("can_rename") }.getLong("id")
                val literal = "İı ไทย Tie\u0302\u0301ng Vie\u0323\u0302t { \$name } 🎨"
                host.drain(obj("type" to "layer", "action" to obj("op" to "rename", "id" to layer, "name" to literal)))
                host.drain(obj("type" to "layer", "action" to obj("op" to "add_mask", "id" to layer, "replace" to false)))
                host.drain(obj("type" to "layer", "action" to obj("op" to "select", "id" to layer, "mask" to false)))
                host.drain(obj("type" to "invoke", "command" to "select_all"))
                host.drain(obj("type" to "invoke", "command" to "move"))
                assertTrue(state().getJSONObject("layer_tools").getBoolean("has_selection"))
                var document = state().getJSONObject("document_file").toString()
                var anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor").toString()
                fun unchanged(popup: android.view.View) {
                    compose.runOnIdle {
                        assertSame(popup, findTag("workspace-menu")!!.first.view)
                        assertSame(surface, activity.window.decorView.descendant<CanvasSurfaceView>())
                        assertEquals(document, state().getJSONObject("document_file").toString())
                        assertEquals(anchor, state().getJSONObject("canvas_bar").getJSONArray("anchor").toString())
                        assertTrue(state().getJSONObject("layer_tools").getBoolean("has_selection"))
                        assertEquals(literal, state().array("layers").objects().first { it.getLong("id") == layer }.getString("label"))
                        assertFalse(host.textComposition.active)
                    }
                    assertEquals(owner, runBlocking { host.withNative { it } })
                }
                fun page(request: JSONObject, mask: Boolean, popup: android.view.View, theme: String, name: String) {
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val projected = coveragePage(current(request), mask)
                        val leaf = descendants(projected).first { coverage(it, mask) }
                        try { compose.waitUntil(10_000) { compose.onAllNodes(hasText(leaf.getString("label")) and hasAnyAncestor(hasTestTag("workspace-menu"))).fetchSemanticsNodes().isNotEmpty() } }
                        catch (failure: ComposeTimeoutException) {
                            println("Menu caption for $tag: expected ${leaf.getString("label")}; ${compose.onNodeWithTag("workspace-menu").printToString()}")
                            capture("$name-timeout", theme, tag)
                            throw failure
                        }
                        text(projected.getString("label")).assertIsDisplayed()
                        text(leaf.getString("label")).performScrollTo().assertIsDisplayed()
                        compose.onNodeWithContentDescription(host.bootstrap!!.getJSONObject("common").getString("back")).assertExists()
                        unchanged(popup)
                        capture(name, theme, tag)
                    }
                }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    switch(host, "en")
                    val layerRequest = obj("type" to "layer_menu", "id" to layer, "mask" to false)
                    compose.onNodeWithTag("layer-row-$layer").performTouchInput { longClick(androidx.compose.ui.geometry.Offset(width * .7f, height / 2f)) }
                    menuShown()
                    text(current(layerRequest).getString("title")).assertIsDisplayed()
                    text(source(current(layerRequest), true).getString("label")).performScrollTo().performClick()
                    text(coveragePage(current(layerRequest), true).getString("label")).performScrollTo().performClick()
                    compose.waitForIdle()
                    val layerPopup = findTag("workspace-menu")!!.first.view
                    document = state().getJSONObject("document_file").toString()
                    anchor = state().getJSONObject("canvas_bar").getJSONArray("anchor").toString()
                    page(layerRequest, true, layerPopup, theme, "postlayout-layer-mask-page")
                    closeMenu()
                    switch(host, "en")
                    val selectRequest = obj("type" to "application_menu", "menu" to "select")
                    compose.onNodeWithTag("application-menu-select").performClick()
                    menuShown()
                    val selectPopup = findTag("workspace-menu")!!.first.view
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val menu = current(selectRequest)
                        val opacity = source(menu, false).getString("label")
                        val mask = source(menu, true).getString("label")
                        text(opacity).performScrollTo().assertIsDisplayed()
                        text(mask).performScrollTo().assertIsDisplayed()
                        unchanged(selectPopup)
                        capture("postlayout-selection-source-root", theme, tag)
                    }
                    text(source(current(selectRequest), false).getString("label")).performScrollTo().performClick()
                    page(selectRequest, false, selectPopup, theme, "postlayout-selection-opacity-page")
                    compose.onNodeWithTag("workspace-menu-back").performClick()
                    text(source(current(selectRequest), true).getString("label")).performScrollTo().performClick()
                    page(selectRequest, true, selectPopup, theme, "postlayout-selection-mask-page")
                    closeMenu()
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun languagePublicationWaitsForCompositionAndKeepsNumericDraftSelection() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val field = "setting-number-canvas-size-width"
            fun literal() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun selection() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange)
            try {
                switch(host, "en")
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host).filter { it != "en" }) {
                        switch(host, "en")
                        val draft = when (tag) {
                            "th" -> "๒＋๓ผู้วาด"
                            "vi" -> "２＋３Tie\u0302\u0301ng Vie\u0323\u0302t"
                            else -> "２＋３日本"
                        }
                        host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                        compose.onNodeWithTag(field).performClick()
                        lateinit var connection: android.view.inputmethod.InputConnection
                        compose.runOnIdle {
                            connection = findTag(field)!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                            assertTrue(connection.setSelection(0, literal().length))
                            assertTrue(connection.setComposingText(draft, 1))
                            assertTrue(host.textComposition.active)
                        }
                        val nativeView = findTag(field)!!.first.view
                        host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, tag))))
                        compose.runOnIdle {
                            assertEquals("en", host.languageTag)
                            assertEquals(draft, literal())
                            assertTrue(connection.finishComposingText())
                            assertTrue(connection.setSelection(1, 3))
                        }
                        compose.waitForIdle()
                        val selected = selection()
                        host.awaitMain("$tag after InputConnection composition", 30_000, { "${host.bootstrap}" }, compose) { host.languageTag == tag }
                        compose.runOnIdle {
                            assertSame(nativeView, findTag(field)!!.first.view)
                            assertEquals(draft, literal())
                            assertEquals(selected, selection())
                        }
                        compose.onNodeWithTag("canvas-size-apply").performClick()
                        compose.runOnIdle { assertEquals(draft, literal()) }
                        compose.onNodeWithTag("canvas-size-cancel").performClick()
                        switch(host, "en")
                    }
                }
            } finally { switch(host, "en") }
        }
    }
    @Test fun languageCopyKeepsCommittedNumericRefusalWhileReplacementDraftIsUnfinished() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val field = "setting-number-canvas-size-width"
            fun state() = host.snapshot!!.getJSONObject("state")
            fun canvasSize() = state().getJSONObject("layer_tools").getJSONObject("canvas_size")
            fun literal() = findTag(field)!!.second.config[SemanticsProperties.EditableText].text
            fun selection() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange)
            try {
                for (theme in listOf("light", "dark")) {
                    switch(host, "en")
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                    host.awaitMain("numeric draft presented", 10_000, { canvasSize().toString() }, compose) { findTag(field) != null }
                    val request = obj("control" to canvasSize().getJSONArray("numeric").getJSONObject(0),
                        "value" to canvasSize().getJSONArray("values").getDouble(0),
                        "operation" to obj("type" to "expression", "text" to "12+"))
                    fun expected() = runCatching { Native.number(request.toString(), host.languageTag) }.exceptionOrNull()!!.message!!
                    compose.onNodeWithTag(field).performClick()
                    compose.onNodeWithTag(field).performTextReplacement("12+")
                    compose.onNodeWithTag(field).performImeAction()
                    host.awaitMain("committed numeric refusal", 10_000, { canvasSize().toString() }, compose) { findNode(hasLabel(expected())) != null }
                    compose.onNodeWithTag(field).performTextReplacement("48+48")
                    compose.runOnIdle {
                        assertTrue(findTag(field)!!.second.config[SemanticsActions.SetSelection].action!!.invoke(1, 3, false))
                        val view = findTag(field)!!.first.view
                        view.context.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(view.windowToken, 0)
                    }
                    compose.waitForIdle()
                    val view = findTag(field)!!.first.view
                    val selected = selection()
                    val values = canvasSize().getJSONArray("values").toString()
                    val document = state().getJSONObject("document_file").toString()
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val refusal = expected()
                        host.awaitMain("retained refusal over unfinished replacement $tag", 10_000, { canvasSize().toString() }, compose) { findNode(hasLabel(refusal)) != null }
                        compose.runOnIdle {
                            assertSame(view, findTag(field)!!.first.view)
                            assertEquals("48+48", literal())
                            assertEquals(selected, selection())
                            assertEquals(values, canvasSize().getJSONArray("values").toString())
                            assertEquals(document, state().getJSONObject("document_file").toString())
                        }
                        capture("numeric-unfinished-replacement", theme, tag)
                    }
                    compose.onNodeWithTag("canvas-size-cancel").performClick()
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun committedNumericRefusalsProjectCopyWithoutReparsingDirtyDrafts() { retainedNumericRefusals() }
    @Test fun russianNumericRefusalsRemainVisibleWithoutNativeSelectionHandles() { retainedNumericRefusals(listOf("ru"), true) }
    private fun retainedNumericRefusals(languages: List<String>? = null, closeSelection: Boolean = false) {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val field = "setting-number-canvas-size-width"
            val errorTag = "number-error-canvas-size-width"
            val owner = runBlocking { host.withNative { it } }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun canvasSize() = state().getJSONObject("layer_tools").getJSONObject("canvas_size")
            fun literal() = findTag(field)!!.second.config[SemanticsProperties.EditableText].text
            fun selection() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange)
            fun caption(reason: Any, language: String) = JSONObject(Native.nativeCaption(obj("type" to "numeric_error", "reason" to reason).toString(), language)).getString("text")
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for ((draft, expectedReason) in listOf("12+" to "invalid_expression", "1/0" to "finite_number")) {
                        switch(host, "en")
                        host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                        host.awaitMain("numeric field presented", 10_000, { canvasSize().toString() }, compose) { findTag(field) != null }
                        val control = canvasSize().getJSONArray("numeric").getJSONObject(0)
                        val value = canvasSize().getJSONArray("values").getDouble(0)
                        val request = obj("control" to control, "value" to value, "operation" to obj("type" to "expression", "text" to draft))
                        val failure = runCatching { Native.number(request.toString(), host.languageTag) }.exceptionOrNull()!!
                        assertEquals("JNI retains the actual known numeric reason", "NumericFailure", failure.javaClass.simpleName)
                        assertEquals(expectedReason, (failure as NumericFailure).reason.let { it as JSONObject }.getString("reason"))
                        val toolbarFailure = runCatching { Native.toolbarUi(obj("type" to "number", "request" to request, "compact" to true, "units" to false).toString(), host.languageTag) }.exceptionOrNull()!!
                        assertTrue(toolbarFailure is NumericFailure)
                        assertEquals(failure.reason.toString(), (toolbarFailure as NumericFailure).reason.toString())
                        val nativeView = findTag(field)!!.first.view
                        compose.onNodeWithTag(field).performClick()
                        compose.onNodeWithTag(field).performTextReplacement(draft)
                        compose.onNodeWithTag(field).performImeAction()
                        host.awaitMain("committed $expectedReason", 10_000, { canvasSize().toString() }, compose) { findTag(errorTag) != null }
                        compose.onNodeWithTag(errorTag).assertTextEquals(caption(failure.reason, "en"))
                        val before = canvasSize().getJSONArray("values").toString()
                        val file = state().getJSONObject("document_file").toString()
                        compose.runOnIdle {
                            assertTrue(findTag(field)!!.second.config[SemanticsActions.SetSelection].action!!.invoke(1, 2, false))
                        }
                        compose.waitForIdle()
                        compose.runOnIdle {
                            assertEquals(androidx.compose.ui.text.TextRange(1, 2), selection())
                            nativeView.context.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(nativeView.windowToken, 0)
                        }
                        compose.waitForIdle()
                        val selected = selection()
                        for (tag in languages ?: tags(host)) {
                            switch(host, tag)
                            compose.runOnIdle {
                                assertSame(nativeView, findTag(field)!!.first.view)
                                assertEquals(draft, literal())
                                assertEquals(selected, selection())
                                assertEquals(before, canvasSize().getJSONArray("values").toString())
                                assertEquals(file, state().getJSONObject("document_file").toString())
                                assertFalse(host.textComposition.active)
                            }
                            assertEquals(owner, runBlocking { host.withNative { it } })
                            val expected = caption(failure.reason, tag)
                            compose.onNodeWithTag(errorTag).assertTextEquals(expected)
                            assertEquals(expected, numericFailureCopy(failure, tag, "fallback"))
                            val lookalike = "{\"reason\":\"finite_number\"}"
                            assertEquals(lookalike, numericFailureCopy(IllegalStateException(lookalike), tag, "fallback"))
                            val range = obj("reason" to "range", "label" to "İı ไทย Tie\u0302\u0301ng Vie\u0323\u0302t", "min" to 1, "max" to 2)
                            assertTrue(caption(range, tag).contains("İı ไทย"))
                            capture("current-296-numeric-$expectedReason", theme, tag)
                        }
                        if (closeSelection) {
                            compose.runOnIdle {
                                assertTrue(findTag(field)!!.second.config[SemanticsActions.SetSelection].action!!.invoke(2, 2, false))
                            }
                            compose.waitForIdle()
                            compose.runOnIdle { assertEquals(androidx.compose.ui.text.TextRange(2, 2), selection()) }
                            compose.onNodeWithTag("setting-number-canvas-size-height").performClick()
                            compose.waitForIdle()
                            compose.runOnIdle {
                                assertSame(nativeView, findTag(field)!!.first.view)
                                assertEquals(draft, literal())
                                assertEquals(androidx.compose.ui.text.TextRange(2, 2), selection())
                                assertEquals(before, canvasSize().getJSONArray("values").toString())
                                assertEquals(file, state().getJSONObject("document_file").toString())
                                nativeView.context.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(nativeView.windowToken, 0)
                            }
                            compose.waitForIdle()
                            compose.onNodeWithTag(errorTag).assertTextEquals(caption(failure.reason, host.languageTag))
                            capture("current-296-numeric-closed-$expectedReason", theme, host.languageTag)
                        }
                        compose.onNodeWithTag(field).performTextReplacement(value.toString())
                        compose.onNodeWithTag(field).performImeAction()
                        host.awaitMain("numeric success clears retained refusal", 10_000, { canvasSize().toString() }, compose) { findTag(errorTag) == null }
                        compose.onNodeWithTag("canvas-size-cancel").performClick()
                    }
                }
            } finally { switch(host, "en") }
        }
    }
    @Test fun searchDraftsAndAliasesSurviveRapidRequestsAndActivityRecreation() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val owner = runBlocking { host.withNative { it } }
            fun search() = host.snapshot!!.getJSONObject("state").objectOrNull("command_search")
            fun query(value: String) {
                compose.onNodeWithTag("command-search").performTextReplacement(value)
                host.awaitMain("search query", 10_000, { search().toString() }, compose) { search()?.optString("query") == value }
            }
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, "en")
                        pressKey(KeyEvent.KEYCODE_K, KeyEvent.META_CTRL_ON)
                        host.awaitMain("search open", 10_000, { search().toString() }, compose) { findTag("command-search") != null }
                        val literal = "Capy 日本 ไทย Tie\u0302\u0301ng Vie\u0323\u0302t İı 😀"
                        query(literal)
                        val nativeView = findTag("command-search")!!.first.view
                        compose.runOnIdle {
                            assertTrue(findTag("command-search")!!.second.config[SemanticsActions.SetSelection].action!!.invoke(2, 8, false))
                        }
                        compose.waitForIdle()
                        val selection = findTag("command-search")!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange)
                        val document = host.snapshot!!.getJSONObject("state").getJSONObject("document_file").toString()
                        for (requested in tags(host).takeLast(3) + tag) host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, requested))))
                        host.awaitMain("latest language $tag", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
                        compose.runOnIdle {
                            assertSame(nativeView, findTag("command-search")!!.first.view)
                            assertEquals(literal, findTag("command-search")!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertEquals(selection, findTag("command-search")!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange))
                            assertEquals(document, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").toString())
                        }
                        val undo = host.snapshot!!.getJSONObject("state").array("commands").objects().first { it.getString("id") == "undo" }.getString("label")
                        for (value in listOf("UNDO", undo, java.text.Normalizer.normalize(undo, java.text.Normalizer.Form.NFD))) {
                            query(value)
                            host.awaitMain("localized and English search aliases $tag", 10_000, { search().toString() }, compose) {
                                search()!!.array("results").objects().any { it.getString("id") == "command.undo" }
                            }
                        }
                        if (tag == "de") {
                            for (value in listOf("Bildgröße", "BILDGRÖSSE", "BILDGRÖẞE", "IMAGE SIZE")) {
                                query(value)
                                host.awaitMain("German sharp-S and English image-size aliases", 10_000, { search().toString() }, compose) { search()!!.array("results").objects().any { it.getString("id") == "command.image_size" } }
                            }
                        }
                        if (tag == "tr") {
                            val localized = host.snapshot!!.getJSONObject("state").array("commands").objects().first { it.getString("id") == "liquify" }.getString("label").uppercase(java.util.Locale.forLanguageTag("tr"))
                            for (value in listOf("LIQUIFY", "LİQUİFY", localized)) {
                                query(value)
                                host.awaitMain("Turkish dotted/dotless I aliases", 10_000, { search().toString() }, compose) { search()!!.array("results").objects().any { it.getString("id") == "command.liquify" } }
                            }
                        }
                        compose.onNodeWithTag("command-close").performClick()
                        host.awaitMain("search close", 10_000, { search().toString() }, compose) { search() == null }
                        scenario.recreate()
                        val resumed = scenario.activity()
                        assertSame(host, resumed.host)
                        assertEquals(owner, runBlocking { host.withNative { it } })
                        host.awaitReady(60_000, compose)
                        assertEquals(tag, host.languageTag)
                        assertEquals(document, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").toString())
                    }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun unicodeNamesSavedDrawingsAndExportDraftsSurviveEveryLanguage() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun field(tag: String) = findTag(tag)!!.second.find { it.config.getOrNull(SemanticsProperties.EditableText) != null }!!
            fun input(tag: String) = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag(tag)), useUnmergedTree = true)
            fun tabs() = native { JSONObject(Native.documentTabs(it, obj("op" to "view").toString())) }
            fun idle(label: String) = host.awaitMain(label, 120_000, { "${host.documents.working} ${host.drawingTabs.view}" }, compose) {
                !host.documents.working && !host.drawingTabs.blocked
            }
            try {
                host.newDocument(320, 240)
                val layer = state().array("layers").objects().first { it.getBoolean("can_rename") }.getLong("id")
                val literal = "Capy 日本 ไทย Tie\u0302\u0301ng Vie\u0323\u0302t İı 😀"
                host.drain(obj("type" to "layer", "action" to obj("op" to "rename", "id" to layer, "name" to literal)))
                host.awaitMain("literal layer name", 10_000, { state().toString() }, compose) { state().array("layers").objects().any { it.getString("label") == literal } }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, tag)
                        idle("before saved fixture")
                        val file = File(device.root, "$theme-$tag-日本-ไทย-Tiếng Việt.capy")
                        host.writeDrawingCopy(file)
                        val count = tabs().array("tabs").length()
                        compose.runOnIdle { assertTrue(host.documents.openUris(listOf(Uri.fromFile(file)))) }
                        host.awaitMain("saved Unicode fixture reopened", 120_000, { tabs().toString() }, compose) { host.drawingTabs.rows.size == count + 1 && !host.documents.working && !host.drawingTabs.blocked }
                        assertTrue(state().array("layers").objects().any { it.getString("label") == literal })
                        val before = file.readBytes()
                        host.drain(obj("type" to "set_layer_opacity", "opacity" to .5 + index(host, tag) * .01 + if (theme == "dark") .2 else 0.0))
                        host.drain(obj("type" to "invoke", "command" to "save_document"))
                        host.awaitMain("localized fixture save", 120_000, { state().getJSONObject("document_file").toString() }, compose) { !state().getJSONObject("document_file").getBoolean("modified") && !host.documents.working }
                        assertFalse(before.contentEquals(file.readBytes()))
                        switch(host, "en")
                        host.drain(obj("type" to "invoke", "command" to "export_document"))
                        host.awaitMain("export dialog", 30_000, { "${host.documents.exportRequest}" }, compose) { findTag("export-preset-name") != null }
                        val exportCopy = host.catalog.getJSONObject("export_copy")
                        compose.onNodeWithTag("color-choice-${exportCopy.getString("format")}").performScrollTo().performClick()
                        compose.onNode(hasText(exportCopy.getString("format_jpeg")) and !hasTestTag("color-choice-${exportCopy.getString("format")}")).performClick()
                        host.awaitMain("JPEG quality control", 30_000, { "${host.documents.exportRequest}" }, compose) { findTag("export-quality") != null }
                        input("export-preset-name").performScrollTo().performTextReplacement(literal)
                        input("export-quality").performScrollTo().performTextReplacement("１＋２ไทย")
                        compose.onNodeWithTag("export-choose-file").performClick()
                        val originalError = runBlocking { colorFailureCopy(host, ColorFeatureFailure("ExportQuality")) }
                        host.awaitMain("typed export quality refusal", 10_000, { host.catalog.toString() }, compose) { findNode(hasLabel(originalError)) != null }
                        input("export-quality").performScrollTo().performClick()
                        compose.waitForIdle()
                        val qualityView = findTag("export-quality")!!.first.view
                        compose.runOnIdle {
                            qualityView.context.getSystemService(android.view.inputmethod.InputMethodManager::class.java).hideSoftInputFromWindow(qualityView.windowToken, 0)
                        }
                        host.awaitMain("keyboard hidden before synthetic export composition", 10_000, { qualityView.rootWindowInsets.toString() }, compose) {
                            qualityView.rootWindowInsets?.isVisible(android.view.WindowInsets.Type.ime()) != true
                        }
                        android.os.SystemClock.sleep(400)
                        compose.waitForIdle()
                        val nameView = findTag("export-preset-name")!!.first.view
                        val exportId = host.documents.exportRequest!!.getInt("id")
                        val drawing = state().getJSONObject("document_file").toString()
                        val draft = if (tag == "vi") "１＋２Tie\u0302\u0301ng Vie\u0323\u0302t" else "１＋２ผู้วาด"
                        lateinit var connection: android.view.inputmethod.InputConnection
                        compose.runOnIdle {
                            connection = findTag("export-quality")!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                            assertTrue(connection.setSelection(0, field("export-quality").config.getOrNull(SemanticsProperties.EditableText)!!.text.length))
                            assertTrue(connection.setComposingText(draft, 1))
                            assertTrue(host.textComposition.active)
                        }
                        host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, tag))))
                        compose.runOnIdle {
                            assertEquals("en", host.languageTag)
                            assertEquals(draft, field("export-quality").config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertTrue(connection.finishComposingText())
                            assertTrue(connection.setSelection(1, 3))
                        }
                        compose.waitForIdle()
                        val selection = field("export-quality").config.getOrNull(SemanticsProperties.TextSelectionRange)
                        host.awaitMain("export language $tag after InputConnection composition", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
                        compose.runOnIdle {
                            assertSame(nameView, findTag("export-preset-name")!!.first.view)
                            assertEquals(literal, field("export-preset-name").config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertEquals(draft, field("export-quality").config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertEquals(selection, field("export-quality").config.getOrNull(SemanticsProperties.TextSelectionRange))
                            assertEquals(exportId, host.documents.exportRequest!!.getInt("id"))
                            assertEquals(drawing, state().getJSONObject("document_file").toString())
                        }
                        val title = host.catalog.getJSONObject("export_copy").getString("title")
                        compose.onNodeWithText(title).assertIsDisplayed()
                        val localizedError = runBlocking { colorFailureCopy(host, ColorFeatureFailure("ExportQuality")) }
                        host.awaitMain("retained typed quality refusal relabel", 10_000, { host.catalog.toString() }, compose) { findNode(hasLabel(localizedError)) != null }
                        capture("export-draft", theme, tag)
                        input("export-quality").performTextReplacement("90")
                        val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
                        val savedRecipe = runBlocking { ColorPreferencesStore.presets(scenario.activity(), color, obj("type" to "get", "index" to 0)) }.getJSONObject("recipe")
                        val recipe = native { JSONObject(Native.query(it, obj("type" to "export_draft", "recipe" to savedRecipe, "action" to obj("type" to "format", "value" to "Jpeg")).toString())) }.getJSONObject("recipe").put("jpeg_quality", 90)
                        val output = File(device.root, "$theme-$tag-日本-ไทย-Tiếng Việt.jpg")
                        compose.runOnIdle {
                            host.documents.chooseExport(recipe, 0)
                            host.documents.picked(Uri.fromFile(output))
                        }
                        host.awaitMain("localized fixture export", 120_000, { "${host.documents.working} ${host.hostError} ${host.notice}" }, compose) { assertNull(host.hostError); !host.documents.working && host.documents.exportRequest == null && output.exists() && output.length() > 0 }
                        assertEquals(JSONObject(drawing).put("busy", false).toString(), state().getJSONObject("document_file").toString())
                        assertTrue(state().array("layers").objects().any { it.getString("label") == literal })
                    }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun retainedNamelessProfilesAndPreparedSourceRelabelWithoutStorageOrJobRestart() {
        launchCapy(compose = compose).use { scenario ->
            val activity = scenario.activity()
            val host = activity.host
            fun <T> native(block: (Long) -> T): T = runBlocking { host.withNative(block) }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun idle(label: String) = host.awaitMain(label, 120_000, { "${host.documents.working} ${host.hostError}" }, compose) { assertNull(host.hostError); !host.documents.working && !host.drawingTabs.blocked }
            fun expected(entry: JSONObject) = runBlocking { profileEntriesCopy(host, listOf(entry)) }.single()
            fun visible(text: String) = findNode(hasLabel(text)) != null
            val literalDiagnostic = IllegalStateException("{\"color_feature_error\":\"ProfileMissing\"}")
            var held: File? = null
            var stored: File? = null
            try {
                host.newDocument(160, 128)
                val color = native { JSONObject(Native.query(it, obj("type" to "document_color").toString())) }
                val recipe = runBlocking { ColorPreferencesStore.presets(activity, color, obj("type" to "get", "index" to 1)) }.getJSONObject("recipe").put("format", "Png")
                host.drain(obj("type" to "invoke", "command" to "export_document"))
                host.awaitMain("profile fixture export dialog", 30_000, { state().toString() }, compose) { host.documents.exportRequest != null }
                val png = File(device.root, "nameless-profile-source.png")
                compose.runOnIdle { host.documents.chooseExport(recipe, 1); host.documents.picked(Uri.fromFile(png)) }
                idle("wide-gamut profile fixture export")
                assertTrue(png.length() > 0)
                val output = java.io.ByteArrayOutputStream()
                val encoded = png.readBytes()
                output.write(encoded, 0, 8)
                var offset = 8
                lateinit var icc: ByteArray
                while (offset < encoded.size) {
                    val length = java.nio.ByteBuffer.wrap(encoded, offset, 4).int
                    val type = encoded.copyOfRange(offset + 4, offset + 8).decodeToString()
                    if (type == "iCCP") {
                        val data = encoded.copyOfRange(offset + 8, offset + 8 + length)
                        val compressed = data.indexOf(0) + 2
                        icc = java.util.zip.InflaterInputStream(data.copyOfRange(compressed, data.size).inputStream()).use { it.readBytes() }
                        val buffer = java.nio.ByteBuffer.wrap(icc)
                        val count = buffer.getInt(128)
                        val tags = (0 until count).map { icc.copyOfRange(132 + it * 12, 144 + it * 12) }.filter { it.copyOfRange(0, 4).decodeToString() != "desc" }
                        assertTrue("The ICC fixture originally has a description", tags.size < count)
                        buffer.putInt(128, tags.size)
                        tags.forEachIndexed { index, tag -> tag.copyInto(icc, 132 + index * 12) }
                        val deflated = java.io.ByteArrayOutputStream().also { stream -> java.util.zip.DeflaterOutputStream(stream).use { it.write(icc) } }.toByteArray()
                        val replacement = data.copyOfRange(0, compressed) + deflated
                        output.write(java.nio.ByteBuffer.allocate(4).putInt(replacement.size).array())
                        val chunk = type.toByteArray() + replacement
                        output.write(chunk)
                        output.write(java.nio.ByteBuffer.allocate(4).putInt(java.util.zip.CRC32().apply { update(chunk) }.value.toInt()).array())
                    } else output.write(encoded, offset, length + 12)
                    offset += length + 12
                }
                png.writeBytes(output.toByteArray())
                val entry = runBlocking { ProfileStore.import(activity, icc) }
                assertEquals("", entry.getString("name"))
                val refusal = runCatching { Native.profileLibrary(obj("type" to "get", "id" to entry.getString("id")).toString(), byteArrayOf(1, 2, 3)) }.exceptionOrNull()
                assertTrue(refusal is ColorFeatureFailure)
                assertEquals("ProfileChanged", (refusal as ColorFeatureFailure).reason)
                stored = File(AppStorage.of(activity).colorProfiles, "${entry.getString("id")}.icc")
                held = File(stored!!.parentFile, "${entry.getString("id")}.held")
                host.drain(obj("type" to "open_settings", "page" to "color"))
                compose.onNodeWithText(host.catalog.getJSONObject("profile_copy").getString("manage")).performScrollTo().performClick()
                val initial = expected(entry)
                host.awaitMain("nameless profile inventory loaded", 30_000, { state().toString() }, compose) { visible(initial.getString("details")) }
                assertTrue(stored!!.renameTo(held!!))
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val projected = expected(entry)
                        assertEquals(literalDiagnostic.message, runBlocking { colorFailureCopy(host, literalDiagnostic) })
                        host.awaitMain("retained profile metadata $tag", 30_000, { projected.toString() }, compose) { visible(projected.getString("name")) && visible(projected.getString("details")) }
                        assertFalse(stored!!.exists())
                        capture("retained-profile-library", theme, tag)
                    }
                }
                assertTrue(held!!.renameTo(stored!!))
                println("PROFILE_FIXTURE library retained all languages and both themes")
                compose.onNodeWithTag("profile-library-done").performClick()
                host.drain(obj("type" to "close_settings"))
                compose.runOnIdle { host.proof.action(obj("type" to "mode", "mode" to "print")); host.proof.action(obj("type" to "reveal")) }
                host.awaitMain("proof profile control", 30_000, { host.proof.form.toString() }, compose) { findTag("proof-profile") != null }
                compose.onNodeWithTag("proof-profile").performScrollTo().performClick()
                val selectedCaption = expected(entry).getString("name")
                host.awaitMain("proof saved profile loaded", 30_000, { host.proof.form.toString() }, compose) { visible(selectedCaption) }
                compose.onNodeWithText(selectedCaption).performClick()
                val profileBytes = entry.getJSONObject("profile").getJSONArray("Icc").toString()
                host.awaitMain("nameless ICC proof prepared", 120_000, { "${host.proof.busy} ${host.proof.status} ${host.proof.error}" }, compose) { assertNull(host.proof.error); !host.proof.busy && !host.proof.hasPending() && host.proof.form?.objectOrNull("document_profile")?.objectOrNull("profile")?.optJSONArray("Icc")?.toString() == profileBytes }
                compose.waitForIdle()
                android.os.SystemClock.sleep(200)
                val settings = host.proof.settings
                val profiles = host.proof.form!!.getJSONArray("profiles")
                val generation = native { JSONObject(Native.query(it, obj("type" to "proof_status").toString())).getLong("generation") }
                compose.onNodeWithTag("proof-profile").performClick()
                host.awaitMain("retained proof picker loaded", 30_000, { host.proof.form.toString() }, compose) { findTag("proof-profile-picker") != null && visible(selectedCaption) }
                assertTrue(stored!!.renameTo(held!!))
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val caption = expected(entry).getString("name")
                        host.awaitMain("retained proof picker $tag", 30_000, { host.proof.form.toString() }, compose) { visible(caption) && host.proof.form!!.getJSONObject("copy").getString("title") == host.catalog.getJSONObject("proof_copy").getString("title") }
                        compose.runOnIdle { assertSame(settings, host.proof.settings); assertSame(profiles, host.proof.form!!.getJSONArray("profiles")); assertFalse(host.proof.busy); assertFalse(host.proof.hasPending()) }
                        assertEquals(generation, native { JSONObject(Native.query(it, obj("type" to "proof_status").toString())).getLong("generation") })
                        assertFalse(stored!!.exists())
                        capture("retained-proof-picker", theme, tag)
                    }
                }
                assertTrue(held!!.renameTo(stored!!))
                println("PROFILE_FIXTURE proof retained all languages and both themes")
                compose.onNodeWithText(host.catalog.getJSONObject("proof_copy").getJSONObject("common").getString("done")).performClick()
                compose.runOnIdle { host.proof.action(obj("type" to "mode", "mode" to "off")) }
                host.awaitMain("proof disabled before opening source", 30_000, { host.proof.form.toString() }, compose) { host.proof.form?.optString("mode") == "off" }
                val tabCount = host.drawingTabs.rows.size
                val sourceEpoch = state().getJSONObject("document_file").getLong("epoch")
                compose.runOnIdle { assertTrue(host.documents.openUris(listOf(Uri.fromFile(png)))) }
                host.awaitMain("nameless ICC source adopted into new tab", 120_000, { "${host.documents.working} ${host.drawingTabs.view}" }, compose) { assertNull(host.hostError); host.drawingTabs.rows.size == tabCount + 1 && state().getJSONObject("document_file").getLong("epoch") != sourceEpoch && !host.documents.working && !host.drawingTabs.blocked }
                val sourceLayer = state().array("layers").objects().single { it.getString("label") == png.nameWithoutExtension }.getLong("id")
                host.drain(obj("type" to "layer", "action" to obj("op" to "select", "id" to sourceLayer, "mask" to false)))
                assertTrue(state().array("layers").objects().single { it.getLong("id") == sourceLayer }.getBoolean("editing"))
                assertEquals("null", native { Native.query(it, obj("type" to "command_reason", "command" to "rasterize_source").toString()) })
                println("PROFILE_FIXTURE nameless ICC source adopted and Rasterize Source enabled")
                host.drain(obj("type" to "invoke", "command" to "rasterize_source"))
                val copy = host.catalog.getJSONObject("document_color_copy")
                compose.onNodeWithText(copy.getString("preview")).performScrollTo().performClick()
                val sourceCaption = expected(entry).getString("name")
                host.awaitMain("nameless source prepared", 60_000, { state().toString() }, compose) { findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(copy.getString("prepared_composition")) == true }) != null && visible(sourceCaption) }
                val prepared = findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(copy.getString("prepared_composition")) == true })!!.first.view
                val drawing = JSONObject(state().getJSONObject("document_file").toString()).apply { remove("busy") }.toString()
                val request = state().array("requests").objects().first { it.getJSONObject("kind").getString("type") == "document" }.getInt("id")
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val localized = host.catalog.getJSONObject("document_color_copy")
                        val localizedSourceCaption = expected(entry).getString("name")
                        host.awaitMain("retained nameless source $tag", 30_000, { state().toString() }, compose) { visible(localizedSourceCaption) && findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(localized.getString("prepared_composition")) == true }) != null }
                        assertSame(prepared, findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(localized.getString("prepared_composition")) == true })!!.first.view)
                        assertEquals(drawing, JSONObject(state().getJSONObject("document_file").toString()).apply { remove("busy") }.toString())
                        assertEquals(request, state().array("requests").objects().first { it.getJSONObject("kind").getString("type") == "document" }.getInt("id"))
                        capture("retained-nameless-source", theme, tag)
                    }
                }
                compose.onNodeWithText(host.catalog.getJSONObject("document_color_copy").getJSONObject("common").getString("cancel")).performClick()
                idle("nameless source comparison cancelled")
            } finally {
                if(held?.exists() == true) assertTrue(held!!.renameTo(stored!!))
                switch(host, "en")
            }
        }
    }

    @Test fun preparedColorAndSourceDialogsKeepTheirCandidateAcrossRelabel() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            fun state() = host.snapshot!!.getJSONObject("state")
            fun file() = JSONObject(state().getJSONObject("document_file").toString()).apply { remove("busy") }.toString()
            fun request() = state().array("requests").objects().first { it.getJSONObject("kind").getString("type") == "document" }.getInt("id")
            try {
                host.newDocument(320, 240)
                host.importStripes(32, 32)
                host.drain(obj("type" to "invoke", "command" to "apply_transform"))
                host.awaitMain("source placed", 30_000, { state().toString() }, compose) { state().objectOrNull("canvas_bar")?.objectOrNull("context")?.optString("kind") != "placement" }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) for ((command, titleKey) in listOf("convert_color_space" to "convert_title", "rasterize_source" to "rasterize_title")) {
                        switch(host, "en")
                        val drawing = file()
                        host.drain(obj("type" to "invoke", "command" to command))
                        val copy = host.catalog.getJSONObject("document_color_copy")
                        host.awaitMain("color dialog", 30_000, { state().toString() }, compose) { findNode(hasLabel(copy.getString(titleKey))) != null }
                        if (command == "convert_color_space") {
                            compose.onNodeWithTag("color-choice-${copy.getString("result")}").performScrollTo().performClick()
                            compose.onNodeWithText(copy.getString("flattened_copy")).performClick()
                        }
                        compose.onNodeWithText(copy.getString("preview")).performScrollTo().performClick()
                        host.awaitMain("prepared comparison", 60_000, { state().toString() }, compose) {
                            findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(copy.getString("prepared_composition")) == true }) != null
                        }
                        val nativeView = findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(copy.getString("prepared_composition")) == true })!!.first.view
                        val id = request()
                        switch(host, tag)
                        val localized = host.catalog.getJSONObject("document_color_copy")
                        compose.onNodeWithText(localized.getString(titleKey)).assertIsDisplayed()
                        compose.runOnIdle {
                            assertSame(nativeView, findNode({ it.config.getOrNull(SemanticsProperties.ContentDescription)?.contains(localized.getString("prepared_composition")) == true })!!.first.view)
                            assertEquals(id, request())
                            assertEquals(drawing, file())
                        }
                        if (command == "convert_color_space") compose.onNodeWithTag("color-choice-${localized.getString("result")}").assertTextContains(localized.getString("flattened_copy"))
                        capture(command, theme, tag)
                        compose.onNodeWithText(localized.getJSONObject("common").getString("cancel")).performClick()
                        host.awaitMain("comparison cancelled", 30_000, { state().toString() }, compose) { !state().getJSONObject("document_file").getBoolean("busy") }
                        assertEquals(drawing, file())
                    }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun newDrawingDialogRelabelKeepsComposingNumericDraftAndSelection() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            fun input() = compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("new-document-width")), useUnmergedTree = true)
            fun field() = findTag("new-document-width")!!.second.find { it.config.getOrNull(SemanticsProperties.EditableText) != null }!!
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, "en")
                        host.drain(obj("type" to "invoke", "command" to "new_document"))
                        host.awaitMain("new drawing dialog", 30_000, { host.snapshot.toString() }, compose) { findTag("new-document-width") != null }
                        input().performClick()
                        val draft = if (tag == "vi") "๒＋๓Tie\u0302\u0301ng Vie\u0323\u0302t" else "๒＋๓ผู้วาด"
                        val nativeView = findTag("new-document-width")!!.first.view
                        lateinit var connection: android.view.inputmethod.InputConnection
                        compose.runOnIdle {
                            connection = nativeView.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                            assertTrue(connection.setSelection(0, field().config.getOrNull(SemanticsProperties.EditableText)!!.text.length))
                            assertTrue(connection.setComposingText(draft, 1))
                            assertTrue(host.textComposition.active)
                        }
                        host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, tag))))
                        compose.runOnIdle {
                            assertEquals("en", host.languageTag)
                            assertEquals(draft, field().config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertTrue(connection.finishComposingText())
                            assertTrue(connection.setSelection(1, 3))
                        }
                        compose.waitForIdle()
                        val selection = field().config.getOrNull(SemanticsProperties.TextSelectionRange)
                        host.awaitMain("new drawing $tag after InputConnection composition", 30_000, { host.bootstrap.toString() }, compose) { host.languageTag == tag }
                        compose.runOnIdle {
                            assertSame(nativeView, findTag("new-document-width")!!.first.view)
                            assertEquals(draft, field().config.getOrNull(SemanticsProperties.EditableText)!!.text)
                            assertEquals(selection, field().config.getOrNull(SemanticsProperties.TextSelectionRange))
                        }
                        val creation = host.snapshot!!.getJSONObject("document_options").getJSONObject("creation").getJSONObject("text")
                        capture("new-drawing-draft", theme, tag)
                        compose.onNodeWithTag("new-document-create").assertTextContains(creation.getString("create")).assertIsNotEnabled()
                        compose.onNodeWithTag("new-document-cancel").assertTextContains(creation.getString("cancel")).performClick()
                    }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun retainedColorFormProjectsCopyWithoutChangingDirtyInputsOrPreview() = retainedColorForm()

    @Test fun frenchIntensityRangeRetainsDraftAcrossPublicationInBothThemes() =
        retainedColorForm(listOf("fr", "en"), "18", true, "range", "french-range-")

    @Test fun russianFiniteIntensityRetainsDraftAcrossPublicationInBothThemes() =
        retainedColorForm(listOf("ru", "en"), "NaN", true, "finite_number", "russian-finite-")

    private fun retainedColorForm(languages: List<String>? = null, invalidText: String = "12+", hdrOnly: Boolean = false,
                                  expectedReason: String? = null, capturePrefix: String = "") {
        launchCapy(compose = compose).use { scenario ->
            device.portrait(scenario)
            val host = scenario.activity().host
            fun hideKeyboard(tag: String) {
                compose.runOnIdle {
                    val manager = scenario.activity().getSystemService(android.content.Context.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager
                    manager.hideSoftInputFromWindow(findTag(tag)!!.first.view.windowToken, 0)
                }
                compose.waitForIdle()
                android.os.SystemClock.sleep(250)
            }
            fun <T> native(block: (Long) -> T) = runBlocking { host.withNative(block) }
            for (depth in if (hdrOnly) listOf("F16") else listOf("U8", "F16")) {
            val hdr = depth == "F16"
            val task = runBlocking { host.withNative { handle ->
                val (id, file) = documentRequest(handle, "new_document")
                Native.projectTask(handle, id, "null", file.getLong("epoch"), file.getLong("revision"))
            } }
            try {
                Native.projectOptions(task, obj("extent" to org.json.JSONArray(listOf(320, 240)), "color" to obj("space" to "DisplayP3", "depth" to depth), "background" to "White").toString())
                Native.projectWork(task, -1, 320, 240)
                native { Native.projectAdopt(it, task, "null") }
            } finally { Native.projectFree(task) }
            host.documentChanged()
            host.awaitMain("$depth color document adopted", 60_000, { host.snapshot.toString() }, compose) { host.snapshot?.objectOrNull("color_panel")?.optBoolean("hdr") == hdr && host.snapshot?.optBoolean("brush_ready") == true }
            fun field(tag: String) = findTag(tag)!!.second
            fun text(tag: String) = field(tag).config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun selection(tag: String) = field(tag).config.getOrNull(SemanticsProperties.TextSelectionRange)
            fun select(tag: String, start: Int, end: Int) {
                compose.runOnIdle { assertTrue(field(tag).config[SemanticsActions.SetSelection].action!!.invoke(start, end, false)) }
            }
            fun preview(): androidx.compose.ui.graphics.Color? {
                if (findTag("color-form-preview") == null) return null
                val pixels = compose.onNodeWithTag("color-form-preview").captureToImage().toPixelMap()
                return pixels[pixels.width / 2, pixels.height / 2]
            }
            try {
                for (theme in listOf("light", "dark")) {
                    switch(host, "en")
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    val state = host.snapshot!!.getJSONObject("state")
                    val colors = state.displayColors()
                    val source = colors.toString()
                    val drawing = state.getJSONObject("document_file").toString()
                    val initial = JSONObject(Native.colorUi(obj("type" to "form", "request" to obj("color" to colors.getJSONObject(colors.getString("slot")), "document_depth" to colors.optString("hdr_depth"), "document_space" to "DisplayP3", "model" to (if (hdr) "linear_rgb" else "document_rgb"), "intensity" to (if (hdr) host.snapshot!!.getJSONObject("color_panel").number("intensity") else null), "rendition" to host.snapshot!!.getJSONObject("color_panel").objectOrNull("rendition"))).toString(), "en"))
                    compose.onNodeWithTag("color-edit-button").performClick()
                    compose.onNodeWithTag("color-input-0").performTextReplacement("0.2500")
                    hideKeyboard("color-input-0")
                    val validRequest = JSONObject(initial.getJSONObject("draft").toString())
                    validRequest.getJSONArray("fields").put(0, "0.2500")
                    val validCopy = JSONObject(Native.colorUi(obj("type" to "form", "request" to validRequest).toString(), "en")).getJSONObject("copy")
                    val validPreview = preview()
                    assertNotNull(validPreview)
                    for (tag in languages ?: tags(host)) {
                        switch(host, tag)
                        val captions = JSONObject(Native.colorUi(obj("type" to "form_copy", "copy" to validCopy).toString(), tag))
                        compose.onNodeWithTag("color-form-status").assertTextEquals(captions.getString("validation"))
                        compose.onNodeWithTag("color-form-use").assertIsEnabled()
                        assertEquals("0.2500", text("color-input-0"))
                        assertEquals(validPreview, preview())
                        assertEquals(source, host.snapshot!!.getJSONObject("state").displayColors().toString())
                        assertEquals(drawing, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").toString())
                        capture("${capturePrefix}retained-color-form-valid-$depth", theme, tag)
                    }
                    switch(host, "en")
                    val invalidTag = if (hdr) "color-intensity-value" else "color-input-2"
                    compose.onNodeWithTag(invalidTag).performTextReplacement(invalidText)
                    val focusedRange = androidx.compose.ui.text.TextRange(if (invalidText.length > 2) 1 else 0, invalidText.length)
                    select(invalidTag, focusedRange.start, focusedRange.end)
                    compose.waitForIdle()
                    assertEquals(focusedRange, selection(invalidTag))
                    compose.onNodeWithTag("color-input-1").performClick().performTextReplacement("2.0000")
                    hideKeyboard("color-input-1")
                    select("color-input-1", 1, 4)
                    compose.waitForIdle()
                    val intensitySelection = selection(invalidTag)
                    val numericSelection = selection("color-input-1")
                    assertEquals(androidx.compose.ui.text.TextRange(1, 4), numericSelection)
                    val nativeView = findTag(invalidTag)!!.first.view
                    val color = preview()
                    val request = initial.getJSONObject("draft")
                    request.getJSONArray("fields").put(0, "0.2500").put(1, "2.0000")
                    if (hdr) request.put("change_intensity_text", invalidText) else request.getJSONArray("fields").put(2, invalidText)
                    val expected = JSONObject(Native.colorUi(obj("type" to "form", "request" to request).toString(), "en"))
                    val copy = expected.getJSONObject("copy")
                    assertFalse(copy.isNull("error"))
                    if (expectedReason != null) {
                        assertEquals("intensity", copy.getJSONObject("error").getString("type"))
                        assertEquals(expectedReason, copy.getJSONObject("error").getJSONObject("detail").getString("reason"))
                    }
                    compose.onNodeWithTag("color-input-model").performClick()
                    for (tag in languages ?: tags(host)) {
                        switch(host, tag)
                        val captions = JSONObject(Native.colorUi(obj("type" to "form_copy", "copy" to copy).toString(), tag))
                        compose.onNodeWithTag("color-form-status").assertTextEquals(captions.getString("error"))
                        if (expectedReason == "range" && tag == "fr") assertTrue(captions.getString("error").startsWith("La valeur du champ "))
                        if (expectedReason == "finite_number" && tag == "ru") assertTrue(captions.getString("error").startsWith("Для поля «"))
                        compose.onNodeWithTag("color-form-use").assertIsNotEnabled()
                        val models = captions.getJSONArray("models")
                        for (i in 0 until models.length()) compose.onAllNodesWithText(models.getJSONArray(i).getString(1)).onLast().assertExists()
                        compose.runOnIdle {
                            assertEquals(invalidText, text(invalidTag))
                            assertEquals("0.2500", text("color-input-0"))
                            assertEquals("2.0000", text("color-input-1"))
                            assertEquals(intensitySelection, selection(invalidTag))
                            assertEquals(numericSelection, selection("color-input-1"))
                            assertSame(nativeView, findTag(invalidTag)!!.first.view)
                            assertEquals(source, host.snapshot!!.getJSONObject("state").displayColors().toString())
                            assertEquals(drawing, host.snapshot!!.getJSONObject("state").getJSONObject("document_file").toString())
                        }
                        assertEquals(color, preview())
                        capture("${capturePrefix}retained-color-form-$depth", theme, tag)
                        instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                        compose.onNodeWithTag("color-form-status").assertIsDisplayed()
                        compose.onNodeWithTag("color-form-use").assertIsDisplayed()
                        capture("${capturePrefix}retained-color-form-closed-$depth", theme, tag)
                        compose.onNodeWithTag("color-input-model").performClick()
                    }
                    instrumentation.sendKeyDownUpSync(KeyEvent.KEYCODE_BACK)
                    compose.onNodeWithText(host.bootstrap!!.getJSONObject("common").getString("cancel")).performClick()
                }
            } finally { switch(host, "en") }
            }
        }
    }

    @Test fun sizePanelsShowCompleteNumericValuesAtSystemTextSize() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            host.newDocument(320, 240)
            for (theme in listOf("light", "dark")) {
                host.drain(obj("type" to "set_theme", "theme" to theme))
                for (type in listOf("image_size", "canvas_size")) {
                    val tag = type.replace('_', '-')
                    host.drain(obj("type" to "invoke", "command" to type))
                    host.awaitMain("$tag visible", 10_000, { host.snapshot.toString() }, compose) { findTag("$tag-panel") != null }
                    for (axis in listOf("width", "height")) {
                        val node = compose.onNodeWithTag("setting-number-$tag-$axis")
                        val layouts = mutableListOf<TextLayoutResult>()
                        node.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
                        val layout = layouts.single()
                        android.util.Log.i("CapyLanguage", "$tag $axis text=${layout.layoutInput.text} bounds=${node.fetchSemanticsNode().boundsInRoot} size=${layout.size} line=${layout.getLineLeft(0)}..${layout.getLineRight(0)}")
                        val bounds = node.fetchSemanticsNode().boundsInRoot
                        assertTrue("$tag $axis numeric glyphs exceed its field: ${layout.getLineLeft(0)}..${layout.getLineRight(0)} in $bounds", layout.getLineLeft(0) >= -1f && layout.getLineRight(0) <= bounds.width + 1f)
                        assertTrue("$tag $axis must show all numeric glyphs", layout.getLineLeft(0) >= -1f && layout.getLineRight(0) <= layout.size.width + 1f)
                    }
                    capture("size-numeric-$tag", theme, host.languageTag)
                    compose.onNodeWithTag("$tag-cancel").performClick()
                }
            }
        }
    }

    @Test fun retainedWorkspaceSwitcherOptionsRelabelWithoutReplacingTheirOwner() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val activity = scenario.activity()
            val owner = runBlocking { host.withNative { it } }
            fun options() = runBlocking { host.withNative {
                JSONObject(Native.workspace(it, obj("type" to "tick").toString())).getJSONObject("view").getJSONObject("switcher_options")
            } }
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    switch(host, "en")
                    val saved = jsonValue(JSONObject(host.workspaceCapture()))
                    val current = host.workspaceManager!!.getString("id")
                    compose.onNodeWithTag("workspace-switcher-options").performClick()
                    compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isNotEmpty() }
                    val popup = findTag("workspace-menu")!!.first.view
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val menu = options()
                        val rows = menu.array("sections").values().flatMap { (it as org.json.JSONArray).objects() }
                        val labels = listOf(menu.getString("title")) + rows.map { it.getString("label") }
                        host.awaitMain("retained workspace options $tag", 10_000, { menu.toString() }, compose) {
                            labels.all { findTag("workspace-menu")?.second?.find(hasLabel(it)) != null }
                        }
                        labels.forEach { compose.onNode(hasText(it) and hasAnyAncestor(hasTestTag("workspace-menu"))).assertIsDisplayed() }
                        rows.filter { it.objectOrNull("action")?.objectOrNull("command")?.optString("type") == "show_in_switcher" }.forEach { row ->
                            val item = compose.onNode(hasText(row.getString("label")) and hasAnyAncestor(hasTestTag("workspace-menu"))).fetchSemanticsNode()
                            val checkbox = generateSequence(item) { it.parent }.first { it.config.getOrNull(SemanticsProperties.Role) == androidx.compose.ui.semantics.Role.Checkbox }
                            assertEquals(androidx.compose.ui.state.ToggleableState(row.optBoolean("selected")), checkbox.config.getOrNull(SemanticsProperties.ToggleableState))
                        }
                        compose.runOnIdle {
                            assertSame(popup, findTag("workspace-menu")!!.first.view)
                            assertSame(activity, scenario.activity())
                            assertEquals(current, host.workspaceManager!!.getString("id"))
                        }
                        assertEquals(owner, runBlocking { host.withNative { it } })
                        assertEquals(saved, jsonValue(JSONObject(host.workspaceCapture())))
                        capture("workspace-switcher-options", theme, tag)
                    }
                    pressKey(KeyEvent.KEYCODE_ESCAPE)
                    compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isEmpty() }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun primaryContextAndResampleMenusRelabelWithoutReplacingTheirOwner() {
        launchCapy(compose = compose).use { scenario ->
            val activity = scenario.activity()
            val host = activity.host
            val surface = activity.window.decorView.descendant<CanvasSurfaceView>()!!
            val owner = runBlocking { host.withNative { it } }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun headerItems() = host.snapshot!!.getJSONObject("header").getJSONObject("model").array("zones").values().flatMap { (it as org.json.JSONArray).objects() }
            fun children(menu: JSONObject) = menu.array("sections").values().flatMap { (it as org.json.JSONArray).objects() }
            fun descendants(menu: JSONObject): List<JSONObject> = children(menu).flatMap { listOf(it) + descendants(it) }
            fun current(request: JSONObject) = runBlocking { host.awaitQuery(request) }!!
            fun menuText(label: String) = compose.onNode(hasText(label) and hasAnyAncestor(hasTestTag("workspace-menu")))
            fun opened() = compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isNotEmpty() }
            fun closeMenu() {
                pressKey(KeyEvent.KEYCODE_BACK)
                compose.waitUntil(10_000) { compose.onAllNodesWithTag("workspace-menu").fetchSemanticsNodes().isEmpty() }
            }
            fun header(action: JSONObject) = host.drain(obj("type" to "customize", "action" to obj("type" to "header", "action" to action)))
            fun units(view: JSONObject, tag: String) {
                view.getJSONArray("units").objects().forEach { choice ->
                    val label = choice.getString("label")
                    val layouts = mutableListOf<TextLayoutResult>()
                    compose.onNodeWithTag("$tag-unit-${choice.getString("unit")}").assertTextEquals(label)
                        .performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layouts) }
                    assertTrue("$tag unit $label layout", layouts.isNotEmpty())
                    val layout = layouts.single()
                    assertEquals(label, layout.layoutInput.text.text)
                    val last = layout.lineCount - 1
                    assertEquals("$tag unit $label loses text in ${host.languageTag}", label.length, layout.getLineEnd(last, visibleEnd = true))
                    assertTrue("$tag unit $label clipped in ${host.languageTag}", (0..last).all {
                        !layout.isLineEllipsized(it) && layout.getLineLeft(it) >= -1f && layout.getLineRight(it) <= layout.size.width + 1f &&
                            layout.getLineTop(it) >= -1f && layout.getLineBottom(it) <= layout.size.height + 1f
                    })
                }
            }
            fun retain(name: String, theme: String, labels: () -> List<String>, submenu: Boolean = false, checkLayout: () -> Unit = {}, checkDraft: () -> Unit = {}) {
                val popup = findTag("workspace-menu")!!.first.view
                val drawing = state().getJSONObject("document_file").toString()
                for (tag in tags(host)) {
                    switch(host, tag)
                    val expected = labels()
                    host.awaitMain("retained $name $tag", 10_000, { expected.toString() }, compose) { expected.all { findNode(hasLabel(it)) != null } }
                    expected.forEach { menuText(it).performScrollTo().assertIsDisplayed() }
                    if (submenu) compose.onNodeWithContentDescription(host.bootstrap!!.getJSONObject("common").getString("back")).assertExists()
                    compose.runOnIdle {
                        assertSame(popup, findTag("workspace-menu")!!.first.view)
                        assertSame(activity, scenario.activity())
                        assertSame(surface, activity.window.decorView.descendant<CanvasSurfaceView>())
                        assertEquals(drawing, state().getJSONObject("document_file").toString())
                        assertFalse(host.textComposition.active)
                        checkDraft()
                    }
                    assertEquals(owner, runBlocking { host.withNative { it } })
                    checkLayout()
                    capture("adjacent-$name", theme, tag)
                }
                closeMenu()
            }
            try {
                switch(host, "en")
                host.newDocument(320, 240)
                header(obj("type" to "edit", "editing" to true))
                val menuLabels = headerItems().first { it.getJSONObject("item").getString("kind") == "menu_labels" }.getInt("id")
                header(obj("type" to "remove", "id" to menuLabels))
                header(obj("type" to "add", "zone" to "left", "before" to null, "item" to obj("kind" to "menu")))
                header(obj("type" to "edit", "editing" to false))
                val primaryId = headerItems().first { it.getJSONObject("item").getString("kind") == "menu" }.getInt("id")
                val settingsId = headerItems().first { it.getJSONObject("item").getString("kind") == "settings" }.getInt("id")
                fun filePage(root: JSONObject) = children(root).first { descendants(it).any { leaf -> leaf.objectOrNull("action")?.optString("command") == "new_document" } }
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    switch(host, "en")
                    val primary = obj("type" to "application_menu", "menu" to "primary")
                    compose.onNodeWithTag("header-control-$primaryId").performClick()
                    opened()
                    menuText(filePage(current(primary)).getString("label")).performScrollTo().performClick()
                    retain("primary-file-page", theme, {
                        val page = filePage(current(primary))
                        listOf(page.getString("label"), descendants(page).first { it.objectOrNull("action")?.optString("command") == "new_document" }.getString("label"))
                    }, true)
                    switch(host, "en")
                    val headerRequest = obj("type" to "context", "target" to obj("kind" to "header", "id" to settingsId))
                    compose.onNodeWithTag("header-control-$settingsId").performTouchInput { longClick() }
                    opened()
                    val headerAction = descendants(current(headerRequest)).first { it.objectOrNull("action") != null }.getJSONObject("action").toString()
                    retain("header-context", theme, {
                        listOf(descendants(current(headerRequest)).first { it.objectOrNull("action")?.toString() == headerAction }.getString("label"))
                    })
                    switch(host, "en")
                    val panelRequest = obj("type" to "context", "target" to obj("kind" to "panel", "panel" to "layers"))
                    compose.onNodeWithTag("tab-layers").performTouchInput { longClick() }
                    opened()
                    val panelAction = descendants(current(panelRequest)).first { it.objectOrNull("action") != null }.getJSONObject("action").toString()
                    retain("panel-context", theme, {
                        listOf(descendants(current(panelRequest)).first { it.objectOrNull("action")?.toString() == panelAction }.getString("label"))
                    })
                    switch(host, "en")
                    host.drain(obj("type" to "invoke", "command" to "image_size"))
                    host.awaitMain("resample control", 10_000, { state().toString() }, compose) { findTag("image-size-resample") != null }
                    compose.onNodeWithTag("image-size-resample").performClick()
                    opened()
                    fun imageSize() = state().getJSONObject("layer_tools").getJSONObject("image_size")
                    val values = imageSize().getJSONArray("values").toString()
                    val chosen = imageSize().getString("resample")
                    retain("resample", theme, {
                        imageSize().getJSONArray("resamples").objects().map { it.getString("label") }
                    }, checkLayout = { units(imageSize(), "image-size") }, checkDraft = {
                        assertEquals(values, imageSize().getJSONArray("values").toString())
                        assertEquals(chosen, imageSize().getString("resample"))
                    })
                    compose.onNodeWithTag("image-size-cancel").performClick()
                    host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                    host.awaitMain("canvas size units", 10_000, { state().toString() }, compose) { findTag("canvas-size-panel") != null }
                    fun canvasSize() = state().getJSONObject("layer_tools").getJSONObject("canvas_size")
                    val canvasValues = canvasSize().getJSONArray("values").toString()
                    val canvasUnit = canvasSize().getString("unit")
                    for (tag in tags(host)) {
                        switch(host, tag)
                        units(canvasSize(), "canvas-size")
                        assertEquals(canvasValues, canvasSize().getJSONArray("values").toString())
                        assertEquals(canvasUnit, canvasSize().getString("unit"))
                        assertEquals(owner, runBlocking { host.withNative { it } })
                        capture("adjacent-canvas-units", theme, tag)
                    }
                    compose.onNodeWithTag("canvas-size-cancel").performClick()
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun retainedProofRefusalProjectsKnownReasonWithoutRestartingPreparation() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            fun state() = host.snapshot!!.getJSONObject("state")
            fun generation() = runBlocking { host.withNative {
                JSONObject(Native.query(it, obj("type" to "proof_status").toString())).getLong("generation")
            } }
            try {
                switch(host, "en")
                compose.runOnIdle { host.proof.action(obj("type" to "reveal")) }
                host.awaitMain("retained proof panel", 30_000, { host.proof.form.toString() }, compose) { host.proof.form != null }
                compose.runOnIdle { host.proof.action(obj("type" to "number", "key" to "balance", "value" to 2.0)) }
                host.awaitMain("typed native proof refusal", 10_000, { "${host.proof.error}" }, compose) { host.proof.error != null }
                val error = host.proof.error!!
                assertTrue(error is ColorFeatureFailure)
                assertEquals("ProofInvalidValue", (error as ColorFeatureFailure).reason)
                val settings = host.proof.settings
                val profiles = host.proof.form!!.getJSONArray("profiles")
                val drawing = state().getJSONObject("document_file").toString()
                val preparedGeneration = generation()
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    for (tag in tags(host)) {
                        switch(host, tag)
                        val caption = runBlocking { colorFailureCopy(host, error, proof = true) }
                        host.awaitMain("retained proof refusal $tag", 10_000, { "${host.proof.form}" }, compose) { findNode(hasLabel(caption)) != null }
                        compose.onNodeWithText(caption).assertIsDisplayed()
                        compose.runOnIdle {
                            assertSame(error, host.proof.error)
                            assertSame(settings, host.proof.settings)
                            assertSame(profiles, host.proof.form!!.getJSONArray("profiles"))
                            assertFalse(host.proof.busy)
                            assertFalse(host.proof.hasPending())
                        }
                        assertEquals(preparedGeneration, generation())
                        assertEquals(drawing, state().getJSONObject("document_file").toString())
                        capture("retained-proof-refusal", theme, tag)
                    }
                }
            } finally { switch(host, "en") }
        }
    }

    @Test fun genuineKeyboardCompositionDefersLanguagePublication() {
        org.junit.Assume.assumeTrue(InstrumentationRegistry.getArguments().getString("genuineIme") == "true")
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val automation = instrumentation.uiAutomation
            val service = automation.serviceInfo
            val serviceFlags = service.flags
            service.flags = service.flags or android.accessibilityservice.AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS
            automation.serviceInfo = service
            fun literal() = findTag("export-preset-name")!!.second.find { it.config.getOrNull(SemanticsProperties.EditableText) != null }!!.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun key(value: String): android.view.accessibility.AccessibilityNodeInfo? {
                fun find(node: android.view.accessibility.AccessibilityNodeInfo): android.view.accessibility.AccessibilityNodeInfo? {
                    val description = node.contentDescription?.toString().orEmpty()
                    if (description.equals(value, true) || node.text?.toString()?.equals(value, true) == true) return node
                    return (0 until node.childCount).firstNotNullOfOrNull { node.getChild(it)?.let(::find) }
                }
                return instrumentation.uiAutomation.windows.filter { it.type == android.view.accessibility.AccessibilityWindowInfo.TYPE_INPUT_METHOD }.firstNotNullOfOrNull { it.root?.let(::find) }
            }
            fun tap(value: String) {
                val node = key(value) ?: throw AssertionError("Genuine keyboard key $value is unavailable")
                val bounds = android.graphics.Rect().also(node::getBoundsInScreen)
                val down = android.os.SystemClock.uptimeMillis()
                for (phase in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                    val event = MotionEvent.obtain(down, android.os.SystemClock.uptimeMillis(), phase, bounds.exactCenterX(), bounds.exactCenterY(), 0).apply { source = InputDevice.SOURCE_TOUCHSCREEN }
                    try { assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true)) } finally { event.recycle() }
                }
                android.os.SystemClock.sleep(80)
            }
            try {
                for (theme in listOf("light", "dark")) for (tag in tags(host)) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    switch(host, "en")
                    host.drain(obj("type" to "invoke", "command" to "export_document"))
                    host.awaitMain("genuine IME export field", 30_000, { "${host.documents.exportRequest}" }, compose) { findTag("export-preset-name") != null }
                    compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("export-preset-name")), useUnmergedTree = true).performScrollTo().performClick()
                    val until = android.os.SystemClock.uptimeMillis() + 10_000
                    while (key("p") == null && android.os.SystemClock.uptimeMillis() < until) android.os.SystemClock.sleep(100)
                    android.os.SystemClock.sleep(350)
                    val arguments = InstrumentationRegistry.getArguments()
                    val keys = arguments.getString("genuineImeKeys", "paint")!!
                    val preedit = arguments.getString("genuineImePreedit", "paint")!!
                    val candidate = arguments.getString("genuineImeCandidate")
                    for (letter in keys) tap(letter.toString())
                    host.awaitMain("genuine keyboard preedit $preedit", 10_000, { "${literal()} active=${host.textComposition.active}" }, compose) { host.textComposition.active && literal() == preedit }
                    val draft = literal()
                    val nativeView = findTag("export-preset-name")!!.first.view
                    capture("genuine-ime-preedit-$tag", theme, "en")
                    host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index(host, tag))))
                    compose.runOnIdle { assertEquals("en", host.languageTag); assertEquals(draft, literal()); assertTrue(host.textComposition.active) }
                    capture("genuine-ime-pending-$tag", theme, "en")
                    tap(candidate ?: "space")
                    host.awaitMain("language after genuine keyboard commit", 30_000, { "${literal()} active=${host.textComposition.active}" }, compose) { !host.textComposition.active && host.languageTag == tag }
                    compose.runOnIdle { assertSame(nativeView, findTag("export-preset-name")!!.first.view); assertEquals(candidate ?: "$draft ", literal()) }
                    capture("genuine-ime-export-preset", theme, tag)
                    compose.onNodeWithText(host.catalog.getJSONObject("export_copy").getJSONObject("common").getString("cancel")).performClick()
                }
            } finally {
                switch(host, "en")
                service.flags = serviceFlags
                automation.serviceInfo = service
            }
        }
    }

}
