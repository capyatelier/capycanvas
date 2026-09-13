package art.capycanvas

import android.graphics.Bitmap
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.UUID

/** Actual toolbar controls around a new, untouched document and isolated workspace. */
class AndroidIconEditorTest {
    @get:Rule val compose = createEmptyComposeRule()

    @Test fun allToolCategoriesModesFiltersAndToolbarIconsRender() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val output = File(context.getExternalFilesDir(null), "validation/icon-editor").apply { mkdirs() }
        CanvasHost.workspaceDirectoryForTest = File(context.filesDir, "icon-editor-${UUID.randomUUID()}").absolutePath
        var scenario: ActivityScenario<MainActivity>? = null
        var host: CanvasHost? = null
        var settings: JSONObject? = null
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            scenario.onActivity { host = it.host }
            val app = host!!
            compose.waitUntil(60_000) { app.snapshot?.optBoolean("brush_ready") == true || app.failure != null }
            assertNull(app.failure)
            compose.waitUntil(60_000) { app.workspaceManager?.let { it.optBoolean("ready") && !it.optBoolean("busy") } == true }
            fun state() = app.snapshot!!.getJSONObject("state")
            settings = JSONObject(state().getJSONObject("settings").toString())
            val document = state().getJSONObject("document_file")
            assertTrue("Fixture never opens an existing document", document.isNull("location"))
            assertFalse("Fixture never paints", document.getBoolean("modified"))
            auditToolAndFilterControls(app, output)
            for (theme in listOf("light", "dark")) for ((style, size) in listOf("small" to 16, "medium" to 24, "large" to 32)) {
                compose.runOnIdle {
                    app.dispatch(obj("type" to "set_theme", "theme" to theme))
                    app.customize(obj("type" to "set_tile_style", "panel" to "toolbar", "style" to style))
                }
                compose.waitUntil(10_000) {
                    app.panelContent?.array("panels")?.objects()?.firstOrNull { it.getString("id") == "toolbar" }
                        ?.optInt("tile_icon_size") == size
                }
                compose.waitForIdle()
                val first = app.snapshot!!.array("panels").objects().first { it.getString("id") == "toolbar" }
                    .array("tiles").objects().first().getInt("id")
                val glyph = compose.onNodeWithTag("tile-icon-toolbar-$first", useUnmergedTree = true)
                glyph.assertWidthIsEqualTo(size.dp).assertHeightIsEqualTo(size.dp)
                val tile = compose.onNodeWithTag("tile-toolbar-$first")
                val a = glyph.fetchSemanticsNode().boundsInRoot
                val b = tile.fetchSemanticsNode().boundsInRoot
                assertEquals("Icon centered horizontally", b.center.x, a.center.x, 1f)
                assertEquals("Icon centered vertically", b.center.y, a.center.y, 1f)
                // Capture only the production toolbar group, excluding the canvas.
                val group = app.snapshot!!.getJSONObject("layout").array("groups").objects()
                    .first { "toolbar" in it.array("panels").values() }.getInt("id")
                val image = compose.onNodeWithTag("group-$group").captureToImage()
                File(output, "toolbar-$theme-$size.png").outputStream().use {
                    image.asAndroidBitmap().compress(Bitmap.CompressFormat.PNG, 100, it)
                }
                assertNull(app.actionError)
            }
        } finally {
            settings?.let { original -> compose.runOnIdle { host?.dispatch(obj("type" to "restore_settings", "settings" to original)) } }
            scenario?.close()
            CanvasHost.workspaceDirectoryForTest = null
        }
    }

    private fun auditToolAndFilterControls(app: CanvasHost, output: File) {
        fun state() = app.snapshot!!.getJSONObject("state")
        fun dispatch(action: JSONObject) {
            val done = java.util.concurrent.CountDownLatch(1)
            compose.runOnIdle {
                app.dispatch(action)
                app.query(obj("type" to "catalog")) { done.countDown() }
            }
            assertTrue("Native action queue drained", done.await(10, java.util.concurrent.TimeUnit.SECONDS))
            compose.waitUntil(30_000) { app.snapshot?.optBoolean("brush_ready") == true || app.actionError != null }
            // The render owner publishes retained panel content separately from
            // the action reply; wait for the same tool projection before walking it.
            compose.waitUntil(10_000) {
                app.panelContent?.getJSONObject("state")?.getJSONObject("tool_set")?.toString() == state().getJSONObject("tool_set").toString()
            }
            compose.waitForIdle()
            assertNull(app.actionError)
        }
        fun cancelTransform() {
            if (state().array("commands").objects().any { it.getString("id") == "cancel_transform" && it.getBoolean("enabled") }) dispatch(obj("type" to "invoke", "command" to "cancel_transform"))
        }
        fun show(panel: String): Int {
            app.snapshot!!.getJSONObject("layout").array("collapsed").objects().firstOrNull { column ->
                column.array("groups").objects().any { group -> group.array("icons").objects().any { it.getString("panel") == panel } }
            }?.let { column ->
                dispatch(obj("type" to "customize", "action" to obj("type" to "set_column_collapsed", "group" to column.getInt("id"), "collapsed" to false)))
            }
            if (app.snapshot!!.getJSONObject("layout").array("groups").objects().none { panel in it.array("panels").values() }) {
                dispatch(obj("type" to "customize", "action" to obj("type" to "set_panel_visible", "panel" to panel, "visible" to true)))
                compose.waitUntil(10_000) { app.snapshot!!.getJSONObject("layout").array("groups").objects().any { panel in it.array("panels").values() } }
            }
            val group = app.snapshot!!.getJSONObject("layout").array("groups").objects()
                .first { panel in it.array("panels").values() }
            if (group.getString("active") != panel) dispatch(obj("type" to "select_panel_tab", "group" to group.getInt("id"), "panel" to panel))
            return group.getInt("id")
        }
        fun capture(tag: String, name: String) {
            val bitmap = compose.onNodeWithTag(tag).captureToImage().asAndroidBitmap()
            File(output, "$name.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        }
        var toolsGroup = 0
        fun toolNode(tag: String, unmerged: Boolean = false) = compose.onNode(hasTestTag(tag) and hasAnyAncestor(hasTestTag("group-$toolsGroup")), useUnmergedTree = unmerged)
        fun checkChoices() {
            val view = state().getJSONObject("tool_set")
            for (kind in listOf("groups", "subtools")) {
                val items = view.array(kind).objects()
                val plain = items.filter { it.isNull("preview") }
                assertEquals("Modes need distinct icons", plain.size, plain.map { it.getString("icon") }.toSet().size)
                for (item in items) {
                    val label = item.getString("label")
                    val glyph = toolNode("tool-${if (kind == "groups") "group" else "subtool"}-icon-$label", unmerged = true)
                    glyph.performScrollTo().assertIsDisplayed().assertWidthIsEqualTo(16.dp).assertHeightIsEqualTo(16.dp)
                    val parent = toolNode("${if (kind == "groups") "tool-group" else "subtool"}-$label").fetchSemanticsNode().boundsInRoot
                    val bounds = glyph.fetchSemanticsNode().boundsInRoot
                    assertTrue("$label glyph is inside its control", bounds.left >= parent.left && bounds.right <= parent.right && bounds.top >= parent.top && bounds.bottom <= parent.bottom)
                }
            }
        }
        val categories = app.catalog.array("brush_categories").objects()
        assertEquals("Every painting medium has an icon", 13, categories.map { it.getString("icon") }.toSet().size)
        val records = org.json.JSONArray()
        for (theme in listOf("light", "dark")) {
            dispatch(obj("type" to "set_theme", "theme" to theme))
            val group = show("brushes")
            toolsGroup = group
            for (category in categories) {
                val label = category.getString("label")
                val brushes = category.array("brushes").objects()
                dispatch(obj("type" to "select_brush", "id" to brushes.first().getInt("id")))
                toolNode("tool-group-$label").performScrollTo().performClick()
                compose.waitForIdle()
                for (brush in brushes) {
                    toolNode("subtool-${brush.getString("label")}").performScrollTo().performClick()
                    try {
                        compose.waitUntil(10_000) { state().getJSONObject("brush").getInt("preset") == brush.getInt("id") && app.snapshot?.optBoolean("brush_ready") == true }
                    } catch (failure: Throwable) {
                        capture("group-$group", "failure-${category.getString("icon")}")
                        throw AssertionError("Selecting ${brush.getString("label")}: wanted ${brush.getInt("id")}, got ${state().getJSONObject("brush")}; ready=${app.snapshot?.optBoolean("brush_ready")}; error=${app.actionError}", failure)
                    }
                }
                checkChoices()
                val selected = state().getJSONObject("tool_set").array("groups").objects().first { it.getBoolean("selected") }
                assertEquals(category.getString("icon"), selected.getString("icon"))
                toolNode("tool-group-$label").performScrollTo()
                capture("group-$group", "medium-$theme-${category.getString("icon")}")
                records.put(obj("theme" to theme, "category" to label, "icon" to selected.getString("icon")))
            }
            val painting = setOf("pen", "pencil", "brush", "eraser", "airbrush", "decoration", "blend", "liquify")
            for (command in app.catalog.array("tool_commands").values().map { it.toString() }.filter { it !in painting }) {
                cancelTransform()
                val available = state().array("commands").objects().first { it.getString("id") == command }.getBoolean("enabled")
                if (!available) {
                    assertEquals("Only content transforms are unavailable on blank artwork", "scale_rotate", command)
                    continue // Its glyph is inspected in the Move/Scale group and command toolbar.
                }
                dispatch(obj("type" to "invoke", "command" to command))
                for (choice in state().getJSONObject("tool_set").array("groups").objects()) {
                    cancelTransform()
                    val action = choice.getJSONObject("action")
                    if (action.optString("type") == "invoke" && !state().array("commands").objects().first { it.getString("id") == action.getString("command") }.getBoolean("enabled")) {
                        assertEquals("scale_rotate", action.getString("command"))
                        checkChoices()
                        continue
                    }
                    dispatch(action)
                    checkChoices()
                    for (subtool in state().getJSONObject("tool_set").array("subtools").objects()) {
                        val label = subtool.getString("label")
                        toolNode("subtool-$label").performScrollTo().performClick()
                        compose.waitUntil(10_000) { state().getJSONObject("tool_set").array("subtools").objects().first { it.getString("label") == label }.getBoolean("selected") }
                        compose.waitForIdle()
                    }
                    capture("group-$group", "mode-$theme-$command-${choice.getString("icon")}")
                }
            }
            dispatch(obj("type" to "layer", "action" to obj("op" to "tool", "tool" to "lasso_fill")))
            checkChoices()
            capture("group-$group", "mode-$theme-lasso-fill")
            val filters = show("adjustments")
            for (category in state().array("filter_categories").objects().filter { !it.isNull("id") }) {
                dispatch(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to category.getString("id"))))
                for (choice in state().array("adjustments").objects()) {
                    val id = choice.getString("id")
                    assertNotEquals("Filter needs a specific glyph", "adjustments", choice.getString("icon"))
                    compose.onNodeWithTag("filter-list").performScrollToNode(hasTestTag("adjustment-$id"))
                    compose.onNodeWithTag("filter-icon-$id", useUnmergedTree = true).assertIsDisplayed()
                    records.put(obj("theme" to theme, "filter" to id, "icon" to choice.getString("icon")))
                }
                compose.onNodeWithTag("filter-list").performScrollToIndex(0)
                capture("group-$filters", "filters-$theme-${category.getString("id")}")
            }
        }
        File(output, "controls.json").writeText(records.toString(2))
        dispatch(obj("type" to "filter_picker", "action" to obj("op" to "category", "category" to JSONObject.NULL)))
        dispatch(obj("type" to "invoke", "command" to "brush"))
        show("brushes")
        assertFalse("Icon audit never paints", state().getJSONObject("document_file").getBoolean("modified"))
    }
}
