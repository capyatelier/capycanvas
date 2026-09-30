package art.capycanvas

import android.content.ContentValues
import android.graphics.Bitmap
import android.os.SystemClock
import android.provider.MediaStore
import android.view.KeyEvent
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Keyboard shortcuts, modifier keys, pen buttons and taps on the tablet. */
class AndroidShortcutsTest {
    companion object { private val runId = System.currentTimeMillis().toString() }
    @get:Rule(order = 0) val device = CapyDeviceRule()
    @get:Rule(order = 1) val compose = createAndroidComposeRule<MainActivity>()
    private val host get() = compose.activity.host
    @Before fun ready() {
        compose.waitUntil(60_000) { host.snapshot?.optBoolean("brush_ready") == true }
        host.awaitReady()
        compose.runOnIdle {
            host.dispatch(obj("type" to "set_theme", "theme" to "light"))
            host.dispatch(obj("type" to "open_settings", "page" to "shortcuts"))
            host.preference(obj("type" to "reset_all_shortcuts"))
            host.dispatch(obj("type" to "close_settings"))
        }
        compose.waitUntil(10_000) { host.snapshot?.getJSONObject("state")?.optString("theme") == "light" }
        compose.waitForIdle()
    }
    private fun state() = host.snapshot!!.getJSONObject("state")
    private fun preferences() = host.snapshot!!.getJSONObject("preferences")
    private fun page() = preferences().getJSONObject("shortcut_page")
    private fun action(action: JSONObject) {
        val done = CountDownLatch(1)
        compose.runOnIdle { host.dispatch(action); host.query(obj("type" to "catalog")) { done.countDown() } }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        compose.waitForIdle()
    }
    private fun preference(action: JSONObject) = action(obj("type" to "preferences", "action" to action))
    private fun open(page: String) {
        action(obj("type" to "open_settings", "page" to page))
        compose.waitUntil(10_000) { host.snapshot?.objectOrNull("preferences")?.optString("page") == page }
        compose.waitForIdle()
    }
    private fun tap(tag: String) {
        compose.waitUntil(10_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
        val node = compose.onNodeWithTag(tag)
        if (compose.onAllNodes(hasTestTag(tag) and hasAnyAncestor(hasScrollAction())).fetchSemanticsNodes().isNotEmpty()) node.performScrollTo()
        node.performClick()
        compose.waitForIdle()
    }
    private fun press(code: Int, meta: Int = 0) {
        val now = SystemClock.uptimeMillis()
        for (action in listOf(KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP))
            assertTrue(instrumentation.uiAutomation.injectInputEvent(KeyEvent(now, SystemClock.uptimeMillis(), action, code, 0, meta), true))
        compose.waitForIdle()
    }
    private fun capture(name: String) {
        for (theme in listOf("light", "dark")) {
            action(obj("type" to "set_theme", "theme" to theme))
            compose.waitForIdle()
            val presented = CountDownLatch(1)
            compose.runOnIdle {
                val view = compose.activity.window.decorView
                view.postOnAnimation { view.postOnAnimation { presented.countDown() } }
            }
            assertTrue(presented.await(5, TimeUnit.SECONDS))
            SystemClock.sleep(150)
            val bitmap = instrumentation.uiAutomation.takeScreenshot()
            val resolver = compose.activity.contentResolver
            val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, ContentValues().apply {
                put(MediaStore.Images.Media.DISPLAY_NAME, "$name-$theme.png")
                put(MediaStore.Images.Media.MIME_TYPE, "image/png")
                put(MediaStore.Images.Media.RELATIVE_PATH, "Pictures/CapyCanvasShortcuts/$runId")
                put(MediaStore.Images.Media.IS_PENDING, 1)
            })!!
            resolver.openOutputStream(uri)!!.use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
        }
        action(obj("type" to "set_theme", "theme" to "light"))
    }

    @Test fun categoriesSlideAndFiltersShareOneLine() {
        open("shortcuts")
        assertEquals("Modifier keys", page().array("categories").objects().first().getString("id"))
        val search = compose.onNodeWithTag("shortcuts-search").fetchSemanticsNode().boundsInRoot
        for (tag in listOf("shortcut-context", "shortcut-show")) {
            val bounds = compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
            assertEquals("$tag is on the search line", search.center.y, bounds.center.y, 2f)
            assertTrue("$tag follows the search field", bounds.left > search.left)
        }
        capture("shortcuts-home")
        tap("shortcut-category-Tools")
        compose.waitUntil(10_000) { page().optString("category") == "Tools" }
        compose.onNodeWithTag("settings-page-title").assertTextEquals("Tools")
        compose.onNodeWithTag("shortcut-command.Pencil").assertExists()
        capture("shortcuts-category")
        compose.onNodeWithContentDescription("Back").performClick()
        compose.waitUntil(10_000) { page().isNull("category") }
        tap("shortcut-show")
        tap("shortcut-show-option-2")
        compose.waitUntil(10_000) { page().getString("show") == "customized" }
        compose.onNodeWithTag("shortcut-empty").assertExists()
        capture("shortcuts-empty")
        tap("shortcut-show")
        tap("shortcut-show-option-0")
        compose.onNodeWithTag("shortcuts-search").performClick()
        press(KeyEvent.KEYCODE_Z, KeyEvent.META_CTRL_ON or KeyEvent.META_CTRL_LEFT_ON)
        compose.waitUntil(10_000) { page().optString("key") == "Ctrl+Z" }
        compose.onNodeWithTag("shortcut-command.Undo").assertExists()
        compose.onNode(hasSetTextAction() and hasAnyAncestor(hasTestTag("shortcuts-search"))).assertTextEquals("Ctrl+Z")
        capture("shortcuts-key-search")
    }

    @Test fun editorRecordsInlineAndReassigns() {
        open("shortcuts")
        preference(obj("type" to "edit_shortcut", "id" to "command.ZenMode"))
        compose.onNodeWithTag("settings-page-title").assertTextEquals("Zen mode")
        tap("add-shortcut")
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        press(KeyEvent.KEYCODE_E)
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").optString("conflict") == "Eraser" }
        compose.onNodeWithTag("confirm-shortcut").assertTextEquals("Reassign")
        capture("shortcut-editor-conflict")
        press(KeyEvent.KEYCODE_ESCAPE)
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") == null }
        assertNotNull("Escape cancels recording, not the editor", preferences().objectOrNull("shortcut_editor"))
        tap("add-shortcut")
        press(KeyEvent.KEYCODE_E)
        compose.waitUntil(10_000) { preferences().objectOrNull("capture")?.optString("conflict") == "Eraser" }
        tap("confirm-shortcut")
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").values().contains("E") }
        capture("shortcut-editor")
        tap("shortcut-editor-reset")
        compose.waitUntil(10_000) { !preferences().getJSONObject("shortcut_editor").getBoolean("modified") }
        tap("add-shortcut")
        press(KeyEvent.KEYCODE_BUTTON_3)
        compose.waitUntil(10_000) { preferences().objectOrNull("capture")?.optString("shortcut") == "Pad button 3" }
        tap("confirm-shortcut")
        compose.waitUntil(10_000) { preferences().getJSONObject("shortcut_editor").array("bindings").values().contains("Pad button 3") }
        tap("shortcut-editor-reset")
    }

    @Test fun modifierKeysUseSlidePagesAndThePicker() {
        open("shortcuts")
        tap("shortcut-category-Modifier keys")
        compose.waitUntil(10_000) { page().optString("category") == "Modifier keys" }
        tap("modifier-Alt")
        compose.waitUntil(10_000) { preferences().objectOrNull("modifier_editor") != null }
        compose.onNodeWithTag("settings-page-title").assertTextEquals("Alt")
        compose.onAllNodes(isDialog()).assertCountEquals(0)
        fun editor() = preferences().getJSONObject("modifier_editor")
        val contexts = page().array("contexts").objects().filter { !it.isNull("category") }.map { it.getString("category") }
        fun categories() = editor().array("actions").objects().map { it.getString("category") }
        assertTrue(contexts.isNotEmpty())
        assertTrue("Alt depends on the tool", editor().getBoolean("per_tool") && categories() == contexts)
        capture("modifier-key")
        tap("modifier-same")
        compose.waitUntil(10_000) { !editor().getBoolean("per_tool") && editor().array("actions").length() == 1 }
        tap("modifier-same")
        compose.waitUntil(10_000) { editor().getBoolean("per_tool") && categories() == contexts }
        tap("modifier-action-selection")
        compose.waitUntil(10_000) { page().objectOrNull("picker") != null }
        compose.onNodeWithTag("action-picker-title").assertTextEquals("Alt · Selection tools")
        compose.onNodeWithTag("action-picker-description").assertTextEquals("Holding Alt uses this until you let go.")
        capture("modifier-picker")
        press(KeyEvent.KEYCODE_ESCAPE)
        compose.waitUntil(10_000) { page().objectOrNull("picker") == null }
        assertNotNull("Escape closes only the picker", preferences().objectOrNull("modifier_editor"))
        tap("modifier-action-selection")
        tap("action-command.Move")
        compose.waitUntil(10_000) {
            state().getJSONObject("settings").optJSONArray("hold_keys")?.objects()?.any {
                it.getJSONObject("key").getString("key") == "alt" && it.getJSONObject("actions").optString("selection") == "command.Move"
            } == true
        }
        capture("modifier-key-per-tool")
        compose.activity.runOnUiThread { compose.activity.onBackPressedDispatcher.onBackPressed() }
        compose.waitUntil(10_000) { preferences().objectOrNull("modifier_editor") == null }
        tap("add-modifier-key")
        compose.waitUntil(10_000) { preferences().objectOrNull("capture") != null }
        press(KeyEvent.KEYCODE_ALT_LEFT)
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").optBoolean("existing") }
        compose.onNodeWithTag("confirm-shortcut").assertTextEquals("Open")
        capture("modifier-existing")
        press(KeyEvent.KEYCODE_F5)
        compose.waitUntil(10_000) { preferences().getJSONObject("capture").optString("shortcut") == "F5" }
        tap("confirm-shortcut")
        compose.waitUntil(10_000) { preferences().objectOrNull("modifier_editor")?.getString("label") == "F5" }
        tap("modifier-action-all")
        tap("action-command.Pencil")
        compose.waitUntil(10_000) { preferences().getJSONObject("modifier_editor").array("actions").objects().single().getString("action") == "Pencil" }
        tap("modifier-remove")
        compose.waitUntil(10_000) { preferences().objectOrNull("modifier_editor") == null }
        assertFalse(page().array("modifiers").objects().any { it.getString("label") == "F5" })
    }

    @Test fun penButtonsChoosePerToolAndTapsUseThePicker() {
        open("input")
        tap("trigger-pen.button.primary")
        compose.waitUntil(10_000) { preferences().objectOrNull("pen_button_editor") != null }
        compose.onNodeWithTag("settings-page-title").assertTextEquals("Lower side button")
        tap("pen-button-same")
        tap("pen-button-action-drawing")
        tap("action-command.Pencil")
        tap("pen-button-action-selection")
        tap("action-command.Undo")
        compose.waitUntil(10_000) { state().getJSONObject("settings").optJSONObject("pen_buttons")?.optJSONObject("pen.button.primary")?.optString("selection") == "command.Undo" }
        capture("pen-button")
        compose.onNodeWithContentDescription("Back").performClick()
        compose.waitUntil(10_000) { preferences().objectOrNull("pen_button_editor") == null }
        compose.onNodeWithTag("trigger-action-pen.button.primary", useUnmergedTree = true).assertTextEquals("Depends on the tool")
        compose.onNodeWithTag("trigger-touch.tap.4").performScrollTo()
        capture("pen-and-input")
        tap("trigger-touch.tap.4")
        compose.waitUntil(10_000) { page().objectOrNull("picker") != null }
        compose.onNodeWithTag("action-picker-title").assertTextEquals("Four-finger tap")
        capture("tap-picker")
        tap("action-command.ZenMode")
        compose.waitUntil(10_000) { page().objectOrNull("picker") == null }
        compose.onNodeWithTag("trigger-action-touch.tap.4", useUnmergedTree = true).assertTextEquals("Zen mode")
        preference(obj("type" to "reset_trigger", "trigger" to "touch.tap.4"))
        preference(obj("type" to "reset_trigger", "trigger" to "pen.button.primary"))
    }

    @Test fun keymapMenuDifferencesAndImportPreview() {
        open("shortcuts")
        fun keymap() = preferences().getJSONObject("keymap")
        preference(obj("type" to "select_keymap", "id" to "krita"))
        tap("keymap-menu")
        compose.onNodeWithTag("keymap-differences").assertExists()
        capture("keymap-menu")
        tap("keymap-differences")
        compose.waitUntil(10_000) { keymap().optBoolean("details") }
        compose.onNodeWithTag("keymap-details").assertExists()
        capture("keymap-differences")
        compose.onNodeWithContentDescription("Close").performClick()
        compose.waitUntil(10_000) { !keymap().optBoolean("details") }
        val text = obj("format" to "capycanvas-keymap", "version" to 1, "keymap" to obj("id" to "photoshop", "revision" to 1)).toString()
        preference(obj("type" to "import_keymap", "text" to text))
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("keymap-import-preview").fetchSemanticsNodes().isNotEmpty() }
        capture("keymap-import")
        tap("keymap-confirm-import")
        compose.waitUntil(10_000) { state().getJSONObject("settings").optJSONObject("keymap")?.getString("id") == "photoshop" }
        preference(obj("type" to "select_keymap", "id" to "capy"))
    }
}
