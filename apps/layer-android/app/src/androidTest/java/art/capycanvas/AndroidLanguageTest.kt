package art.capycanvas

import android.view.InputDevice
import android.view.MotionEvent
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.json.JSONObject

class AndroidLanguageTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    @Test fun preferencesRelabelInPlaceAndPaintingHistorySurvivesInBothThemes() {
        launchCapy(compose = compose).use { scenario ->
            val activity = scenario.activity()
            val host = activity.host
            val surface = activity.window.decorView.descendant<CanvasSurfaceView>()!!
            val owner = runBlocking { host.withNative { it } }
            fun state() = host.snapshot!!.getJSONObject("state")
            fun file() = state().getJSONObject("document_file").toString()
            fun select(index: Int, tag: String, theme: String) {
                compose.onNodeWithTag("setting-choice-language").performScrollTo().performClick()
                val started = android.os.SystemClock.elapsedRealtimeNanos()
                compose.onNodeWithTag("setting-choice-option-language-$index").performClick()
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
                assertEquals(tag, futureTag)
                compose.waitForIdle()
                android.os.SystemClock.sleep(50)
                val capture = java.io.File(instrumentation.targetContext.getExternalFilesDir(null), "validation/language-$theme-$tag.png")
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
                    host.drain(obj("type" to "open_settings", "page" to "appearance"))
                    val unchanged = file()
                    val tags = host.bootstrap!!.array("shipped_tags").values().map { it as String }
                    for ((index, tag) in tags.mapIndexed { offset, tag -> offset + 1 to tag } + (0 to "en")) {
                        select(index, tag, theme)
                        assertEquals(unchanged, file())
                    }
                    compose.onNodeWithTag("settings-done").performClick()
                    host.awaitMain("preferences close", 10_000, { "" }, compose) { !state().getBoolean("settings_open") }
                    paint()
                }
            } finally {
                host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to 1)))
                host.awaitMain("restore English", 30_000, { "" }, compose) { host.languageTag == "en" }
            }
        }
    }

    @Test fun languagePublicationWaitsForCompositionAndKeepsNumericDraftSelection() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            val field = "setting-number-canvas-size-width"
            fun literal() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun selection() = findTag(field)!!.second.config.getOrNull(SemanticsProperties.TextSelectionRange)
            fun switch(index: Int, tag: String) {
                host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to index)))
                host.awaitMain("language $tag", 30_000, { "${host.bootstrap}" }, compose) { host.languageTag == tag }
            }
            try {
                switch(1, "en")
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                    compose.onNodeWithTag(field).performClick()
                    lateinit var connection: android.view.inputmethod.InputConnection
                    compose.runOnIdle {
                        connection = findTag(field)!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                        assertTrue(connection.setSelection(0, literal().length))
                        assertTrue(connection.setComposingText("２＋３日本", 1))
                        assertTrue(host.textComposition.active)
                    }
                    val nativeView = findTag(field)!!.first.view
                    host.drain(obj("type" to "preferences", "action" to obj("type" to "edit", "id" to "language", "value" to 2)))
                    compose.runOnIdle {
                        assertEquals("en", host.languageTag)
                        assertEquals("２＋３日本", literal())
                        assertTrue(connection.finishComposingText())
                        assertTrue(connection.setSelection(1, 3))
                    }
                    compose.waitForIdle()
                    val selected = selection()
                    host.awaitMain("Japanese after composition", 30_000, { "${host.bootstrap}" }, compose) { host.languageTag == "ja" }
                    compose.runOnIdle {
                        assertSame(nativeView, findTag(field)!!.first.view)
                        assertEquals("２＋３日本", literal())
                        assertEquals(selected, selection())
                    }
                    compose.onNodeWithTag("canvas-size-apply").performClick()
                    compose.runOnIdle { assertEquals("２＋３日本", literal()) }
                    compose.onNodeWithTag("canvas-size-cancel").performClick()
                    switch(1, "en")
                }
            } finally { switch(1, "en") }
        }
    }
}
