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

    @Test fun curveCoordinatesKeepNativeCompositionAndUnchangedPrecision() {
        launchCapy(compose = compose).use { scenario ->
            val arguments = androidx.test.platform.app.InstrumentationRegistry.getArguments()
            if (arguments.getString("presentationNarrow") == "true") {
                device.portrait(scenario)
                assertTrue(scenario.activity().resources.configuration.screenWidthDp <= 640)
                scenario.activity().host.narrowPhotoPanels(compose)
            } else if (arguments.getString("presentationWide") == "true") {
                device.landscape(scenario)
                assertTrue(scenario.activity().resources.configuration.screenWidthDp > 640)
            }
            val host = scenario.activity().host
            host.drain(obj("type" to "close_settings"))
            compose.waitUntil(60_000) { host.snapshot?.getJSONObject("state")?.getJSONObject("filter_load")?.optBoolean("pending") == false }
            host.drain(obj("type" to "effect", "action" to obj("op" to "insert", "effect" to "curves")))
            for (panel in listOf("navigator", "proof", "layers")) host.drain(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to false)))
            fun properties() = host.snapshot!!.getJSONObject("state").getJSONObject("layer_properties")
            fun control() = properties().array("controls").objects().first { !it.isNull("curve") }
            fun points() = control().getJSONObject("value").getJSONArray("value").toString()
            fun invoke(command: String) = host.drain(obj("type" to "invoke", "command" to command))
            fun field(axis: String) = "number-curve-${control().getString("key")}-$axis"
            fun literal(axis: String) = findTag(field(axis))!!.second.config.getOrNull(SemanticsProperties.EditableText)!!.text
            fun open(axis: String): android.view.inputmethod.InputConnection {
                if (findTag(field(axis)) == null)
                    compose.onNodeWithTag("number-value-curve-${control().getString("key")}-$axis").performScrollTo().performClick()
                compose.onNodeWithTag(field(axis)).performScrollTo().assertIsDisplayed()
                compose.onNodeWithTag(field(axis)).performTouchInput { click(center) }
                compose.onNodeWithTag(field(axis)).assertIsFocused()
                var connection: android.view.inputmethod.InputConnection? = null
                compose.waitUntil(10_000) {
                    compose.runOnUiThread { connection = findTag(field(axis))!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo()) }
                    connection != null
                }
                return connection!!
            }
            fun nativeEnter(connection: android.view.inputmethod.InputConnection) {
                for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) compose.runOnIdle {
                    val now = android.os.SystemClock.uptimeMillis()
                    connection.sendKeyEvent(KeyEvent(now, now, phase, KeyEvent.KEYCODE_ENTER, 0, 0, 0, 0, KeyEvent.FLAG_SOFT_KEYBOARD))
                }
                compose.waitForIdle()
            }
            for (theme in listOf("light", "dark")) {
                host.drain(obj("type" to "set_theme", "theme" to theme))
                val key = control().getString("key")
                host.drain(obj("type" to "effect", "action" to obj("op" to "set", "layer" to properties().getLong("layer"), "key" to key,
                    "value" to obj("kind" to "curve", "value" to org.json.JSONArray("[[0,0],[0.5,0.12345679],[1,1]]")))))
                compose.onNodeWithTag("effect-curve").performTouchInput { click(androidx.compose.ui.geometry.Offset(width * .5f, height * (1f - .12345679f))) }
                compose.waitUntil(10_000) { !control().getJSONObject("curve").isNull("selected") }
                val original = points()
                assertEquals("31.481", control().getJSONObject("curve").getJSONObject("output").getString("text"))
                var connection = open("output")
                assertEquals("31.481", literal("output"))
                nativeEnter(connection)
                open("input")
                assertEquals("Enter and focus loss do not quantize an unchanged presented knot", original, points())
                connection = open("output")
                compose.runOnIdle {
                    assertTrue(connection.setSelection(0, literal("output").length))
                    assertTrue(connection.setComposingText("12", 1))
                }
                compose.waitForIdle()
                assertTrue(host.textComposition.active)
                nativeEnter(connection)
                assertTrue("Candidate Enter belongs to the native composition", host.textComposition.active)
                assertEquals(original, points())
                compose.runOnIdle {
                    assertTrue(connection.commitText("127.5", 1))
                    assertTrue(connection.finishComposingText())
                }
                compose.waitUntil(10_000) { !host.textComposition.active }
                nativeEnter(connection)
                compose.waitUntil(10_000) { control().getJSONObject("curve").getJSONObject("output").getDouble("value") == .5 }
                invoke("undo"); assertEquals("Committed IME text has one undo", original, points())
                connection = open("output")
                compose.runOnIdle {
                    assertTrue(connection.setSelection(0, literal("output").length))
                    assertTrue(connection.commitText("123.4567890123", 1))
                    assertTrue(connection.setSelection(3, 8))
                }
                compose.waitForIdle()
                compose.onNodeWithTag(field("output")).assertIsFocused()
                assertEquals("A valid dirty field has not committed before Escape", original, points())
                assertEquals("123.4567890123", literal("output"))
                assertEquals(androidx.compose.ui.text.TextRange(3, 8), findTag(field("output"))!!.second.config[SemanticsProperties.TextSelectionRange])
                for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) compose.runOnIdle {
                    val time = android.os.SystemClock.uptimeMillis()
                    connection.sendKeyEvent(KeyEvent(time, time, phase, KeyEvent.KEYCODE_ESCAPE, 0))
                }
                compose.waitForIdle()
                assertEquals("Native Escape cancels valid dirty text; composition=${host.textComposition.active}", original, points())
                connection = open("output")
                compose.runOnIdle {
                    assertTrue(connection.setSelection(0, literal("output").length))
                    assertTrue(connection.commitText("123.4567890123", 1))
                    assertTrue(connection.setSelection(3, 8))
                }
                compose.waitForIdle()
                compose.onNodeWithTag(field("output")).assertIsFocused()
                assertEquals("Valid dirty text is uncommitted before a hardware key", original, points())
                for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) {
                    val time = android.os.SystemClock.uptimeMillis()
                    assertTrue(instrumentation.uiAutomation.injectInputEvent(KeyEvent(time, time, phase, KeyEvent.KEYCODE_ESCAPE, 0), true))
                    compose.waitForIdle()
                }
                assertEquals("Hardware Escape cancels valid dirty text; composition=${host.textComposition.active}", original, points())
                connection = open("output")
                compose.runOnIdle {
                    assertTrue(connection.setSelection(0, literal("output").length))
                    assertTrue(connection.commitText("1e-", 1))
                }
                nativeEnter(connection)
                assertEquals("A partial expression cannot publish an earlier draft", original, points())
                compose.onNodeWithTag("number-error-curve-$key-output").assertIsDisplayed()
                val retired = connection
                scenario.recreate()
                assertSame("Activity recreation retains the authoritative session", host, scenario.activity().host)
                compose.waitUntil(60_000) { findTag(field("output")) != null }
                compose.onNodeWithTag(field("output")).assertIsFocused()
                assertEquals("Native partial draft survives Activity recreation", "1e-", literal("output"))
                assertEquals("Recreation does not edit the document", original, points())
                compose.runOnIdle { retired.commitText("255", 1) }
                compose.waitForIdle()
                assertEquals("A retired InputConnection cannot edit the recreated controls", original, points())
                compose.runOnIdle { connection = findTag(field("output"))!!.first.view.onCreateInputConnection(android.view.inputmethod.EditorInfo())!! }
                val now = android.os.SystemClock.uptimeMillis()
                for (phase in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP)) compose.runOnIdle {
                    connection.sendKeyEvent(KeyEvent(now, now, phase, KeyEvent.KEYCODE_ESCAPE, 0))
                }
                compose.waitForIdle()
                assertEquals(original, points())
                compose.onNodeWithTag("number-error-curve-$key-output").assertDoesNotExist()
                assertNull(host.failure); assertNull(host.actionError)
            }
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
