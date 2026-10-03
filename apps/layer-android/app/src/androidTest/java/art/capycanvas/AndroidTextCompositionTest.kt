package art.capycanvas

import android.view.KeyEvent
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.*
import org.junit.Test
import org.junit.Rule
import org.json.JSONObject

class AndroidTextCompositionTest {
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createEmptyComposeRule()

    @Test fun sizeChoicesRequireEveryRetainedDraftAndCancelDiscardsThem() {
        val sent = mutableListOf<String>()
        var focusCleared = 0
        val actions = SizePanelActions({ sent.add(it.getString("op")) }, { focusCleared++ })
        val first = Any(); val second = Any()
        var composing = true
        var accepted = false
        var cancelled = 0
        actions.register(first) { cancel -> if (cancel) { cancelled++; true } else !composing }
        actions.register(second) { cancel -> if (cancel) { cancelled++; true } else accepted }
        for (operation in listOf("apply", "unit", "resample")) actions.choose(obj("op" to operation))
        assertEquals(emptyList<String>(), sent)
        assertEquals(0, focusCleared)
        composing = false
        actions.choose(obj("op" to "apply"))
        assertEquals(emptyList<String>(), sent)
        actions.register(second, null)
        actions.choose(obj("op" to "resample"))
        assertEquals(listOf("resample"), sent)
        assertEquals(1, focusCleared)
        actions.register(second) { cancel -> if (cancel) { cancelled++; true } else accepted }
        actions.choose(obj("op" to "cancel"))
        assertEquals(listOf("resample", "cancel"), sent)
        assertEquals(2, focusCleared)
        assertEquals(2, cancelled)
        accepted = true
        actions.choose(obj("op" to "apply"))
        assertEquals(listOf("resample", "cancel", "apply"), sent)
    }

    @Test fun numericInputConnectionRefusalsAndCandidateEnterCannotApplyAnEarlierDraft() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            host.drain(obj("type" to "close_settings"))
            fun state() = host.snapshot!!.getJSONObject("state")
            fun panel() = state().getJSONObject("layer_tools").objectOrNull("canvas_size")
            fun width() = state().array("tabs").objects().first { it.getBoolean("active") }.getInt("width")
            val originalWidth = width()
            val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
            val field = "setting-number-canvas-size-width"
            fun literal(): String = findTag(field)!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun replace(text: String, composing: Boolean = false): android.view.inputmethod.InputConnection {
                compose.onNodeWithTag(field).performClick()
                lateinit var connection: android.view.inputmethod.InputConnection
                compose.runOnIdle {
                    connection = findTag(field)!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                    assertTrue(connection.setSelection(0, literal().length))
                    assertTrue(if (composing) connection.setComposingText(text, 1) else connection.commitText(text, 1))
                }
                compose.waitForIdle()
                return connection
            }
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                    compose.waitUntil(10_000) { panel() != null }
                    replace("12+3")
                    compose.waitUntil(10_000) { panel()!!.getJSONArray("values").getDouble(0) == 15.0 }
                    val label = panel()!!.getJSONArray("labels").getString(0)
                    val captions = JSONObject(Native.numericLabels(label))
                    for (refused in listOf("12+", "２＋３", "1/0", "{ $" + "label }日本")) {
                        replace(refused)
                        compose.onNodeWithContentDescription(captions.getString("increase")).performClick()
                        compose.onNodeWithTag("canvas-size-apply").performClick()
                        compose.runOnIdle {
                            assertNotNull(panel())
                            assertEquals(refused, literal())
                            assertEquals(15.0, panel()!!.getJSONArray("values").getDouble(0), 0.0)
                            assertEquals(originalWidth, width())
                        }
                    }
                    val connection = replace("12", composing = true)
                    compose.runOnIdle { assertTrue(host.textComposition.active) }
                    for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) compose.runOnIdle {
                        val now = android.os.SystemClock.uptimeMillis()
                        connection.sendKeyEvent(KeyEvent(now, now, phase, KeyEvent.KEYCODE_ENTER, 0, 0, 0, 0, KeyEvent.FLAG_SOFT_KEYBOARD))
                    }
                    compose.onNodeWithContentDescription(captions.getString("increase")).performClick()
                    compose.onNodeWithTag("canvas-size-apply").performClick()
                    compose.runOnIdle {
                        assertTrue(host.textComposition.active)
                        assertEquals("12", literal())
                        assertEquals(15.0, panel()!!.getJSONArray("values").getDouble(0), 0.0)
                        assertEquals(originalWidth, width())
                        assertTrue(connection.commitText("12+3", 1))
                        assertTrue(connection.finishComposingText())
                    }
                    compose.waitUntil(10_000) { !host.textComposition.active }
                    compose.onNodeWithTag("canvas-size-apply").performClick()
                    compose.waitUntil(10_000) { panel() == null && width() == 15 }
                    host.drain(obj("type" to "invoke", "command" to "undo"))
                    compose.waitUntil(10_000) { width() == originalWidth }
                    host.drain(obj("type" to "invoke", "command" to "canvas_size"))
                    compose.waitUntil(10_000) { panel() != null }
                    replace("２＋３")
                    compose.onNodeWithTag("canvas-size-cancel").performClick()
                    compose.waitUntil(10_000) { panel() == null }
                    assertEquals(originalWidth, width())
                }
            } finally { host.drain(obj("type" to "set_theme", "theme" to originalTheme)) }
        }
    }

    @Test fun numericBlurCommitsNativeTextAndPristineLinkedFieldsFollowTheDraft() {
        launchCapy(compose = compose).use { scenario ->
            val host = scenario.activity().host
            host.drain(obj("type" to "close_settings"))
            fun state() = host.snapshot!!.getJSONObject("state")
            fun panel() = state().getJSONObject("layer_tools").objectOrNull("image_size")
            fun drawing() = state().array("tabs").objects().first { it.getBoolean("active") }
            val originalWidth = drawing().getInt("width")
            val originalHeight = drawing().getInt("height")
            val originalTheme = state().getJSONObject("settings").opt("theme") ?: JSONObject.NULL
            val width = "setting-number-image-size-width"
            val height = "setting-number-image-size-height"
            fun literal(field: String) = findTag(field)!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun replace(field: String, text: String, composing: Boolean = false) {
                compose.onNodeWithTag(field).performClick()
                compose.runOnIdle {
                    val connection = findTag(field)!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!!
                    assertTrue(connection.setSelection(0, literal(field).length))
                    assertTrue(if (composing) connection.setComposingText(text, 1) else connection.commitText(text, 1))
                }
                compose.waitForIdle()
            }
            fun open() {
                host.drain(obj("type" to "invoke", "command" to "image_size"))
                compose.waitUntil(10_000) { panel() != null }
            }
            fun values() = panel()!!.getJSONArray("values")
            try {
                for (theme in listOf("light", "dark")) {
                    host.drain(obj("type" to "set_theme", "theme" to theme))
                    open()
                    replace(width, "12+3")
                    compose.waitUntil(10_000) { values().getDouble(0) == 15.0 && values().getDouble(1) == 11.0 }
                    replace(width, "12", composing = true)
                    compose.onNodeWithTag(height).performClick()
                    compose.waitUntil(10_000) { values().getDouble(0) == 12.0 && values().getDouble(1) == 9.0 }
                    compose.waitUntil(10_000) { literal(height) == "9 px" }
                    compose.onNodeWithTag("image-size-apply").performTouchInput { click() }
                    compose.waitUntil(10_000) { panel() == null }
                    assertEquals(12, drawing().getInt("width"))
                    assertEquals(9, drawing().getInt("height"))
                    host.drain(obj("type" to "invoke", "command" to "undo"))
                    compose.waitUntil(10_000) { drawing().getInt("width") == originalWidth }

                    open()
                    replace(width, "12+3")
                    compose.waitUntil(10_000) { values().getDouble(0) == 15.0 }
                    replace(height, "6")
                    compose.waitUntil(10_000) { values().getDouble(0) == 8.0 && values().getDouble(1) == 6.0 }
                    compose.onNodeWithTag("image-size-apply").performTouchInput { click() }
                    compose.waitUntil(10_000) { panel() == null }
                    assertEquals(8, drawing().getInt("width"))
                    assertEquals(6, drawing().getInt("height"))
                    host.drain(obj("type" to "invoke", "command" to "undo"))
                    compose.waitUntil(10_000) { drawing().getInt("width") == originalWidth }

                    for (refused in listOf("12+", "２＋３")) {
                        open()
                        replace(width, refused)
                        compose.onNodeWithTag(height).performClick()
                        compose.onNodeWithTag("image-size-apply").performTouchInput { click() }
                        compose.runOnIdle {
                            assertNotNull(panel())
                            assertEquals(refused, literal(width))
                            assertEquals(originalWidth, drawing().getInt("width"))
                            assertEquals(originalHeight, drawing().getInt("height"))
                        }
                        compose.onNodeWithTag("image-size-cancel").performClick()
                        compose.waitUntil(10_000) { panel() == null }
                    }
                }
            } finally { host.drain(obj("type" to "set_theme", "theme" to originalTheme)) }
        }
    }

    @Test fun candidateKeysBelongToFocusedCompositionUntilNativeCommit() {
        val state=TextComposition()
        val owner=Any()
        for(text in listOf("日本語", "한국어", "中文", "{ $"+"name }\u2068字\u2069")) {
            val value=TextFieldValue(text,TextRange(text.length),TextRange(0,text.length))
            state.update(owner,value,true)
            for(code in listOf(KeyEvent.KEYCODE_ESCAPE,KeyEvent.KEYCODE_ENTER,KeyEvent.KEYCODE_DPAD_UP,KeyEvent.KEYCODE_DPAD_DOWN)) {
                for(action in listOf(KeyEvent.ACTION_DOWN,KeyEvent.ACTION_UP)) assertTrue(state.owns(KeyEvent(action,code)))
            }
            assertEquals(TextRange(0,text.length),value.composition)
            state.update(owner,value.copy(composition=null),true)
            assertFalse(state.owns(KeyEvent(KeyEvent.ACTION_DOWN,KeyEvent.KEYCODE_ENTER)))
        }
    }
    @Test fun languageBoundaryWaitsUntilCandidateKeyRelease() {
        val state = TextComposition()
        val owner = Any()
        state.update(owner, TextFieldValue("日本", composition = TextRange(0, 2)), true)
        assertTrue(state.owns(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER)))
        state.update(owner, TextFieldValue("日本"), true)
        assertTrue(state.active)
        assertTrue(state.owns(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER)))
        assertFalse(state.active)
        assertFalse(state.owns(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER)))
    }
    @Test fun imeKeysAndRetiredFieldsCannotBeClaimedByEditorCaptures() {
        val state=TextComposition()
        val first=Any();val second=Any()
        val value=TextFieldValue("日本",composition=TextRange(0,2))
        state.update(first,value,true)
        state.update(second,value,true)
        state.clear(first)
        assertTrue(state.active)
        state.update(second,value,false)
        assertFalse(state.active)
        val ime=KeyEvent(1,1,KeyEvent.ACTION_UP,KeyEvent.KEYCODE_ESCAPE,0,0,0,0,KeyEvent.FLAG_SOFT_KEYBOARD)
        assertTrue(state.owns(ime))
        assertFalse(state.owns(KeyEvent(KeyEvent.ACTION_DOWN,KeyEvent.KEYCODE_ESCAPE)))
    }
}
